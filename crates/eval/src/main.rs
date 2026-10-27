//! `cargo run -p eval -- --provider local --model qwen3-8b`
//!
//! Runs the golden set through the same prompts, parsers and request options as the app:
//! extraction per synthetic session, then drafting per Bericht section from the suite's expected
//! facts (so a drafting score doesn't depend on how the extraction went). Prints a summary and
//! writes every proposal with its score to `target/eval/<suite>/`. The suite is synthetic; never
//! put real patient text into it (real material goes to the gitignored `fixtures/`).

mod score;
mod suite;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thera_core::draft::{self, Frame, InputFact};
use thera_core::extract;
use thera_core::llm::{self, Endpoint, ProviderId, Role};
use thera_core::local::{self, LlamaServer};

use score::{EpisodeFact, ExtractScore, FilterScore, SectionScore};
use suite::{Res, Sources, Suite};

const USAGE: &str = "\
usage: cargo run -p eval -- [options]

  --suite <name|dir>      golden set (default de-therapy-v1, in crates/eval/suites)
  --check                 load and validate the suite, no model
  --provider <p>          local | openrouter | requesty | custom
  --model <id>            local: qwen3-8b | qwen3-14b | mistral-small-3.2-24b; else the provider's model id
  --base-url <url>        override the provider URL (custom: required)
  --gguf <path>           local: model file (default: target/eval-models, then the app's models folder)
  --server <path>         local: llama-server.exe (default: crates/app/llama)
  --only <stage>          extract | draft
  --case <id>             only this case / episode (repeatable)
  --sources <yaml>        draft with a per-section source mapping (Bericht section -> note sections)
  --runs <n>              repeat everything n times (default 1)

API key for remote providers: environment variable THERA_EVAL_API_KEY.";

#[derive(Default)]
struct Args {
    suite: String,
    check: bool,
    provider: Option<ProviderId>,
    model: Option<String>,
    base_url: Option<String>,
    gguf: Option<PathBuf>,
    server: Option<PathBuf>,
    only: Option<String>,
    cases: Vec<String>,
    sources: Option<PathBuf>,
    runs: usize,
}

fn parse_args() -> Res<Args> {
    let mut a = Args { suite: "de-therapy-v1".into(), runs: 1, ..Default::default() };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--suite" => a.suite = value()?,
            "--check" => a.check = true,
            "--provider" => {
                a.provider = Some(match value()?.as_str() {
                    "local" => ProviderId::Local,
                    "openrouter" => ProviderId::Openrouter,
                    "requesty" => ProviderId::Requesty,
                    "custom" => ProviderId::Custom,
                    other => return Err(format!("unknown provider {other}")),
                })
            }
            "--model" => a.model = Some(value()?),
            "--base-url" => a.base_url = Some(value()?),
            "--gguf" => a.gguf = Some(value()?.into()),
            "--server" => a.server = Some(value()?.into()),
            "--only" => {
                let v = value()?;
                if v != "extract" && v != "draft" {
                    return Err("--only extract|draft".into());
                }
                a.only = Some(v);
            }
            "--case" => a.cases.push(value()?),
            "--sources" => a.sources = Some(value()?.into()),
            "--runs" => a.runs = value()?.parse().map_err(|_| "--runs needs a number")?,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    if !a.check && (a.provider.is_none() || a.model.is_none()) {
        return Err(format!("--provider and --model are required (or --check)\n\n{USAGE}"));
    }
    Ok(a)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("crates/eval in a workspace").to_path_buf()
}

/// Keeps a local server alive for the whole run; dropping it kills the process.
struct Target {
    ep: Endpoint,
    model: String,
    _server: Option<LlamaServer>,
}

fn target(a: &Args) -> Res<Target> {
    let provider = a.provider.expect("checked");
    let model = a.model.clone().expect("checked");
    if provider == ProviderId::Local {
        let m = local::model(&model).map_err(|e| e.to_string())?;
        let gguf = match &a.gguf {
            Some(p) => p.clone(),
            None => {
                let app = std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("de.datorga.thera/models").join(m.file.file));
                let candidates = [Some(root().join("target/eval-models").join(m.file.file)), app];
                candidates.into_iter().flatten().find(|p| p.exists()).ok_or_else(|| format!("{} not found; pass --gguf", m.file.file))?
            }
        };
        let exe = a.server.clone().unwrap_or_else(|| root().join("crates/app/llama/llama-server.exe"));
        eprintln!("loading {} from {} …", m.label, gguf.display());
        let started = Instant::now();
        let mut server = LlamaServer::spawn(&exe, &gguf, m).map_err(|e| e.to_string())?;
        server.wait_ready(Duration::from_secs(600), &AtomicBool::new(false)).map_err(|e| e.to_string())?;
        eprintln!("loaded in {:.1} s", started.elapsed().as_secs_f64());
        return Ok(Target { ep: server.endpoint(), model, _server: Some(server) });
    }
    let base_url = match (&a.base_url, provider) {
        (Some(u), _) => u.clone(),
        (None, ProviderId::Custom) => return Err("--provider custom needs --base-url".into()),
        (None, p) => llm::provider(p).default_base_url,
    };
    let api_key = std::env::var("THERA_EVAL_API_KEY").ok().filter(|k| !k.is_empty());
    if api_key.is_none() && provider != ProviderId::Custom {
        return Err("set THERA_EVAL_API_KEY".into());
    }
    Ok(Target { ep: Endpoint { provider, base_url, api_key }, model, _server: None })
}

#[derive(Default, Serialize)]
struct Usage {
    requests: usize,
    failed: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    latency_ms: u64,
}

impl Usage {
    fn add(&mut self, c: &llm::Completion) {
        self.requests += 1;
        self.prompt_tokens += c.prompt_tokens.unwrap_or(0) as u64;
        self.completion_tokens += c.completion_tokens.unwrap_or(0) as u64;
        self.latency_ms += c.latency_ms;
    }
}

#[derive(Serialize)]
struct CaseReport {
    run: usize,
    case: String,
    chunks: usize,
    /// The answer had no JSON at all.
    error: Option<String>,
    score: ExtractScore,
    facts: Vec<(String, String, Vec<usize>)>,
}

#[derive(Serialize)]
struct EpisodeReport {
    run: usize,
    episode: String,
    errors: Vec<String>,
    sections: Vec<SectionScore>,
}

#[derive(Serialize)]
struct Report {
    suite: String,
    provider: Option<ProviderId>,
    model: Option<String>,
    answered_by: Option<String>,
    extract_prompt: &'static str,
    draft_prompt: &'static str,
    sources: Option<Sources>,
    filter: Vec<FilterScore>,
    extract: Vec<CaseReport>,
    draft: Vec<EpisodeReport>,
    usage: Usage,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn run() -> Res<()> {
    let a = parse_args()?;
    let dir = if Path::new(&a.suite).is_dir() { PathBuf::from(&a.suite) } else { Path::new(env!("CARGO_MANIFEST_DIR")).join("suites").join(&a.suite) };
    let suite = suite::load(&dir)?;
    let sources = a.sources.as_deref().map(|p| suite::load_sources(p, &suite)).transpose()?;
    let wanted = |id: &str| a.cases.is_empty() || a.cases.iter().any(|c| c == id);
    let facts: usize = suite.cases.iter().map(|c| c.facts.len()).sum();
    println!("suite {}: {} cases, {} expected facts, {} episodes", suite.name, suite.cases.len(), facts, suite.episodes.len());

    let filter = sources.as_ref().map(|s| filter_report(&suite, s)).unwrap_or_default();
    if a.check {
        for c in &suite.cases {
            println!("  {:<20} {:>2} segments {:>2} facts  {}", c.id, c.segments.len(), c.facts.len(), c.title);
        }
        return Ok(());
    }

    let t = target(&a)?;
    let mut report = Report {
        suite: suite.name.clone(),
        provider: a.provider,
        model: a.model.clone(),
        answered_by: None,
        extract_prompt: extract::PROMPT_VERSION,
        draft_prompt: draft::PROMPT_VERSION,
        sources: sources.clone(),
        filter,
        extract: Vec::new(),
        draft: Vec::new(),
        usage: Usage::default(),
    };
    for run in 1..=a.runs {
        if a.only.as_deref() != Some("draft") {
            for case in suite.cases.iter().filter(|c| wanted(&c.id)) {
                eprintln!("[{run}] extract {}", case.id);
                report.extract.push(extract_case(&t, case, run, &mut report.usage, &mut report.answered_by));
            }
        }
        if a.only.as_deref() != Some("extract") {
            for ep in suite.episodes.iter().filter(|e| wanted(&e.id)) {
                eprintln!("[{run}] draft {}", ep.id);
                report.draft.push(draft_episode(&t, &suite, ep, sources.as_ref(), run, &mut report.usage, &mut report.answered_by));
            }
        }
    }
    print_summary(&report);

    let out = root().join("target/eval").join(&suite.name);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let model = a.model.unwrap_or_default().replace(['/', ':', '\\'], "_");
    let path = out.join(format!("{stamp}-{model}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("\nreport: {}", path.display());
    Ok(())
}

fn chat(t: &Target, system: &thera_core::pseudo::Pseudonymized<String>, user: &thera_core::pseudo::Pseudonymized<String>, opts: &llm::ChatOptions, usage: &mut Usage, answered_by: &mut Option<String>) -> Res<String> {
    match llm::chat(&t.ep, &t.model, &[(Role::System, system), (Role::User, user)], opts) {
        Ok(c) => {
            usage.add(&c);
            *answered_by = Some(c.model.clone());
            Ok(c.text)
        }
        Err(e) => {
            usage.failed += 1;
            Err(e.to_string())
        }
    }
}

/// A parse error with the (pseudonymized, synthetic) answer, kept in the JSON report; the
/// summary prints only the first line.
fn with_answer(e: thera_core::Error, answer: &str) -> String {
    format!("{e}
{answer}")
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

fn extract_case(t: &Target, case: &suite::Case, run: usize, usage: &mut Usage, answered_by: &mut Option<String>) -> CaseReport {
    let mut r = CaseReport { run, case: case.id.clone(), chunks: 0, error: None, score: ExtractScore::default(), facts: Vec::new() };
    let result = (|| -> Res<(Vec<extract::DraftFact>, usize)> {
        let prepared = extract::prepare(&case.note, &case.segments, case.dictionary(), case.chunk_chars.unwrap_or(extract::CHUNK_CHARS)).map_err(|e| e.to_string())?;
        r.chunks = prepared.chunks.len();
        let keys: Vec<&str> = extract::target_sections(&case.note).iter().map(|s| s.key.as_str()).collect();
        let (mut facts, mut dropped) = (Vec::new(), 0);
        for chunk in &prepared.chunks {
            let answer = chat(t, &prepared.system, &chunk.user, &extract::chat_options(), usage, answered_by)?;
            let parsed = extract::parse(&answer, &chunk.range, &keys, &prepared.map).map_err(|e| with_answer(e, &answer))?;
            facts.extend(parsed.facts);
            dropped += parsed.dropped;
        }
        Ok((facts, dropped))
    })();
    match result {
        Ok((facts, dropped)) => {
            r.score = score::extraction(case, &facts, dropped);
            r.facts = facts.into_iter().map(|f| (f.section, f.text, f.segments.iter().map(|i| i + 1).collect())).collect();
        }
        Err(e) => {
            r.score = score::extraction(case, &[], 0);
            r.error = Some(e);
        }
    }
    r
}

fn input_facts(suite: &Suite, all: &[EpisodeFact], subset: &[usize]) -> Vec<InputFact> {
    subset
        .iter()
        .map(|&i| {
            let c = &suite.cases[all[i].case];
            let f = &c.facts[all[i].fact];
            let key = &f.sections[0];
            let label = c.note.sections.iter().find(|d| &d.key == key).map_or(key.clone(), |d| d.heading.clone());
            InputFact { session_no: all[i].session_no, label, text: f.def.text.clone() }
        })
        .collect()
}

/// Facts a section may draw on under `sources` (all when the section isn't listed).
fn kept(suite: &Suite, all: &[EpisodeFact], key: &str, sources: Option<&Sources>) -> Vec<usize> {
    let allowed = sources.and_then(|s| s.get(key));
    (0..all.len()).filter(|&i| allowed.is_none_or(|a| a.contains(&suite.cases[all[i].case].facts[all[i].fact].sections[0]))).collect()
}

fn draft_episode(t: &Target, suite: &Suite, ep: &suite::Episode, sources: Option<&Sources>, run: usize, usage: &mut Usage, answered_by: &mut Option<String>) -> EpisodeReport {
    let first = &suite.cases[ep.sessions[0]];
    let frame = Frame { method: first.method, age_group: first.age_group, kind: ep.kind };
    let mut dict = thera_core::pseudo::Dictionary::from_patient(&first.patient);
    for &c in &ep.sessions {
        for term in &suite.cases[c].terms {
            dict.add(term.kind, &term.text);
        }
    }
    let all = score::episode_facts(suite, ep);
    let keys: Vec<String> = draft::target_sections(&ep.bericht).iter().map(|s| s.key.clone()).collect();
    let mut r = EpisodeReport { run, episode: ep.id.clone(), errors: Vec::new(), sections: Vec::new() };
    for key in &keys {
        let subset = kept(suite, &all, key, sources);
        let result = (|| -> Res<SectionScore> {
            if subset.is_empty() {
                return Ok(score::section(suite, ep, key, &all, &subset, &[], 0));
            }
            let input = input_facts(suite, &all, &subset);
            let prepared = draft::prepare(&ep.bericht, &frame, std::slice::from_ref(key), &input, &first.patient, dict.clone(), None).map_err(|e| e.to_string())?;
            let answer = chat(t, &prepared.system, &prepared.sections[0].user, &draft::chat_options(), usage, answered_by)?;
            let parsed = draft::parse(&answer, input.len(), &prepared.map).map_err(|e| with_answer(e, &answer))?;
            let sentences: Vec<(String, Vec<usize>)> = parsed.sentences.into_iter().map(|s| (s.text, s.facts)).collect();
            Ok(score::section(suite, ep, key, &all, &subset, &sentences, parsed.dropped))
        })();
        match result {
            Ok(s) => r.sections.push(s),
            Err(e) => {
                r.errors.push(format!("{key}: {e}"));
                r.sections.push(score::section(suite, ep, key, &all, &subset, &[], 0));
            }
        }
    }
    r
}

fn filter_report(suite: &Suite, sources: &Sources) -> Vec<FilterScore> {
    let mut out: Vec<FilterScore> = Vec::new();
    for ep in &suite.episodes {
        let all = score::episode_facts(suite, ep);
        for d in draft::target_sections(&ep.bericht) {
            let s = score::filter(suite, &d.key, &all, &kept(suite, &all, &d.key, Some(sources)));
            match out.iter_mut().find(|o| o.section == s.section) {
                Some(o) => {
                    o.relevant += s.relevant;
                    o.relevant_kept += s.relevant_kept;
                    o.unrelated += s.unrelated;
                    o.unrelated_removed += s.unrelated_removed;
                }
                None => out.push(s),
            }
        }
    }
    println!("\nsource mapping (no model): relevant facts kept / unrelated facts removed");
    for s in &out {
        println!("  {:<26} {:>3}/{:<3} kept   {:>3}/{:<3} removed", s.section, s.relevant_kept, s.relevant, s.unrelated_removed, s.unrelated);
    }
    out
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "   –".into() } else { format!("{:>3.0}%", 100.0 * a as f64 / b as f64) }
}

fn print_summary(r: &Report) {
    if !r.extract.is_empty() {
        println!("\nextraction ({})", r.extract_prompt);
        println!("  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}", "case", "recall", "sect", "cite", "prec", "extra", "forbid", "drop");
        let mut total = ExtractScore::default();
        for c in &r.extract {
            let s = &c.score;
            total.add(s);
            let err = c.error.as_deref().map(|e| format!("  ERROR {}", first_line(e))).unwrap_or_default();
            println!(
                "  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}{err}",
                c.case,
                pct(s.found, s.expected),
                pct(s.section_ok, s.found),
                pct(s.citation_ok, s.found),
                pct(s.proposed - s.extra.len(), s.proposed),
                s.extra.len(),
                s.forbidden.len(),
                s.dropped
            );
            for m in &s.missed {
                println!("      missed   {m}");
            }
            for m in &s.misfiled {
                println!("      misfiled {m}");
            }
            for m in &s.forbidden {
                println!("      FORBID   {m}");
            }
        }
        println!(
            "  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5}",
            "TOTAL",
            pct(total.found, total.expected),
            pct(total.section_ok, total.found),
            pct(total.citation_ok, total.found),
            pct(total.proposed - total.extra.len(), total.proposed),
            total.extra.len(),
            total.forbidden.len(),
            total.dropped
        );
    }
    if !r.draft.is_empty() {
        println!("\ndrafting ({}{})", r.draft_prompt, if r.sources.is_some() { ", with source mapping" } else { "" });
        println!("  {:<26} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5}", "section", "prec", "recall", "sent", "pad", "forbid", "drop");
        let mut by_section: Vec<SectionScore> = Vec::new();
        for e in &r.draft {
            for err in &e.errors {
                println!("  {} ERROR {}", e.episode, first_line(err));
            }
            for s in &e.sections {
                match by_section.iter_mut().find(|o| o.section == s.section) {
                    Some(o) => o.add(s),
                    None => by_section.push(SectionScore { text: Vec::new(), ..s.clone() }),
                }
            }
        }
        let mut total = SectionScore::default();
        for s in &by_section {
            total.add(s);
            println!(
                "  {:<26} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5}",
                s.section,
                pct(s.correct, s.cited),
                pct(s.recalled, s.relevant),
                s.sentences,
                s.padding,
                s.forbidden.len(),
                s.dropped
            );
            for w in &s.wrong {
                println!("      wrong    {w}");
            }
            for m in &s.forbidden {
                println!("      FORBID   {m}");
            }
        }
        println!(
            "  {:<26} {:>6} {:>6} {:>6} {:>5} {:>6} {:>5}",
            "TOTAL",
            pct(total.correct, total.cited),
            pct(total.recalled, total.relevant),
            total.sentences,
            total.padding,
            total.forbidden.len(),
            total.dropped
        );
    }
    let u = &r.usage;
    println!(
        "\n{} requests ({} failed), {} prompt + {} completion tokens, {:.1} s model time{}",
        u.requests,
        u.failed,
        u.prompt_tokens,
        u.completion_tokens,
        u.latency_ms as f64 / 1000.0,
        r.answered_by.as_deref().map(|m| format!(", answered by {m}")).unwrap_or_default()
    );
}
