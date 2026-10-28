//! `cargo run -p eval -- --provider local --model qwen3-8b`
//!
//! Runs the golden set through the same prompts, parsers and request options as the app:
//! extraction per synthetic session, then drafting per Bericht section from the suite's expected
//! facts (so a drafting score doesn't depend on how the extraction went). Prints a summary and
//! writes every proposal with its score to `target/eval/<suite>/`. The suite is synthetic; never
//! put real patient text into it (real material goes to the gitignored `fixtures/`).

mod score;
mod suite;

use std::collections::HashMap;
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
  --model <id>            local: qwen3-8b | qwen3-14b; else the provider's model id
  --base-url <url>        override the provider URL (custom: required)
  --gguf <path>           local: model file (default: target/eval-models, then the app's models folder)
  --server <path>         local: llama-server.exe (default: crates/app/llama)
  --only <stage>          extract | draft
  --case <id>             only this case / episode (repeatable)
  --sources <yaml>        draft with a per-section source mapping (Bericht section -> note sections)
  --tags <mode>           Bericht tags the drafts filter by: extract (this run's extraction, default),
                          gold (the suite's expected sections) or none (every fact to every section;
                          default with --only draft)
  --runs <n>              repeat everything n times (default 1)
  --max-cost <usd>        stop before the next request once the billed cost (OpenRouter) reaches this

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
    tags: Option<String>,
    runs: usize,
    max_cost: Option<f64>,
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
            "--tags" => {
                let v = value()?;
                if !["extract", "gold", "none"].contains(&v.as_str()) {
                    return Err("--tags extract|gold|none".into());
                }
                a.tags = Some(v);
            }
            "--runs" => a.runs = value()?.parse().map_err(|_| "--runs needs a number")?,
            "--max-cost" => a.max_cost = Some(value()?.parse().map_err(|_| "--max-cost needs a number (USD)")?),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    if !a.check && (a.provider.is_none() || a.model.is_none()) {
        return Err(format!("--provider and --model are required (or --check)\n\n{USAGE}"));
    }
    if a.tags.as_deref() == Some("extract") && a.only.as_deref() == Some("draft") {
        return Err("--tags extract needs the extraction stage (drop --only draft)".into());
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
    /// Includes reasoning tokens.
    completion_tokens: u64,
    reasoning_tokens: u64,
    /// Billed by the router (OpenRouter); 0 for local and providers that don't report it.
    cost_usd: f64,
    latency_ms: u64,
    /// Requests skipped because `--max-cost` was reached.
    skipped: usize,
    #[serde(skip)]
    max_cost: Option<f64>,
}

impl Usage {
    fn add(&mut self, c: &llm::Completion) {
        self.requests += 1;
        self.prompt_tokens += c.prompt_tokens.unwrap_or(0) as u64;
        self.completion_tokens += c.completion_tokens.unwrap_or(0) as u64;
        self.reasoning_tokens += c.reasoning_tokens.unwrap_or(0) as u64;
        self.cost_usd += c.cost.unwrap_or(0.0);
        self.latency_ms += c.latency_ms;
    }

    fn over_budget(&self) -> bool {
        self.max_cost.is_some_and(|cap| self.cost_usd >= cap)
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
    /// (note section, text, cited segments 1-based, Bericht tags)
    facts: Vec<(String, String, Vec<usize>, Vec<String>)>,
    /// Per expected fact: the Bericht tags of its best match (`None`: not found).
    #[serde(skip)]
    gold_tags: Vec<Option<Vec<String>>>,
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
    /// Which Bericht tags the drafts filtered by: extract | gold | none.
    tags: String,
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
    let tags = a.tags.clone().unwrap_or_else(|| if a.only.as_deref() == Some("draft") { "none".into() } else { "extract".into() });
    let mut report = Report {
        suite: suite.name.clone(),
        provider: a.provider,
        model: a.model.clone(),
        answered_by: None,
        extract_prompt: extract::PROMPT_VERSION,
        draft_prompt: draft::PROMPT_VERSION,
        tags: tags.clone(),
        sources: sources.clone(),
        filter,
        extract: Vec::new(),
        draft: Vec::new(),
        usage: Usage { max_cost: a.max_cost, ..Default::default() },
    };
    for run in 1..=a.runs {
        // This run's tags per case and expected fact, for drafting with `--tags extract`.
        let mut extracted: HashMap<String, Vec<Option<Vec<String>>>> = HashMap::new();
        if a.only.as_deref() != Some("draft") {
            for case in suite.cases.iter().filter(|c| wanted(&c.id)) {
                eprintln!("[{run}] extract {}", case.id);
                let r = extract_case(&t, case, run, &mut report.usage, &mut report.answered_by);
                extracted.insert(case.id.clone(), r.gold_tags.clone());
                report.extract.push(r);
            }
        }
        if a.only.as_deref() != Some("extract") {
            let tag_of = |case: &suite::Case, fact: usize| -> Option<Vec<String>> {
                match tags.as_str() {
                    "gold" => Some(case.facts[fact].def.bericht.clone()),
                    "extract" => extracted.get(&case.id).and_then(|t| t[fact].clone()),
                    _ => None,
                }
            };
            for ep in suite.episodes.iter().filter(|e| wanted(&e.id)) {
                eprintln!("[{run}] draft {}", ep.id);
                report.draft.push(draft_episode(&t, &suite, ep, sources.as_ref(), &tag_of, run, &mut report.usage, &mut report.answered_by));
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
    if usage.over_budget() {
        usage.skipped += 1;
        return Err(format!("--max-cost {:.2} USD reached, request skipped", usage.max_cost.unwrap_or(0.0)));
    }
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
    let mut r = CaseReport { run, case: case.id.clone(), chunks: 0, error: None, score: ExtractScore::default(), facts: Vec::new(), gold_tags: Vec::new() };
    let result = (|| -> Res<(Vec<extract::DraftFact>, usize, usize)> {
        let tag_defs = extract::tag_sections(&case.tag_template);
        let budget = case.chunk_chars.unwrap_or(extract::CHUNK_CHARS);
        let prepared = extract::prepare(&case.note, &tag_defs, &case.segments, case.dictionary(), budget).map_err(|e| e.to_string())?;
        r.chunks = prepared.chunks.len();
        let keys: Vec<&str> = extract::target_sections(&case.note).iter().map(|s| s.key.as_str()).collect();
        let tags: Vec<&str> = tag_defs.iter().map(|s| s.key.as_str()).collect();
        let (mut facts, mut dropped) = (Vec::new(), 0);
        for chunk in &prepared.chunks {
            let answer = chat(t, &prepared.system, &chunk.user, &extract::chat_options(), usage, answered_by)?;
            let parsed = extract::parse(&answer, &chunk.range, &keys, &tags, &prepared.map).map_err(|e| with_answer(e, &answer))?;
            facts.extend(parsed.facts);
            dropped += parsed.dropped;
        }
        let (facts, merged) = extract::merge(facts);
        Ok((facts, dropped, merged))
    })();
    match result {
        Ok((facts, dropped, merged)) => {
            r.score = score::extraction(case, &facts, dropped, merged);
            r.gold_tags = r.score.matched.iter().map(|m| m.map(|i| facts[i].bericht.clone())).collect();
            r.facts = facts.into_iter().map(|f| (f.section, f.text, f.segments.iter().map(|i| i + 1).collect(), f.bericht)).collect();
        }
        Err(e) => {
            r.score = score::extraction(case, &[], 0, 0);
            r.gold_tags = vec![None; case.facts.len()];
            r.error = Some(e);
        }
    }
    r
}

/// The Bericht tags of a case's expected fact for drafting (`None`: offered to every section).
type TagOf<'a> = dyn Fn(&suite::Case, usize) -> Option<Vec<String>> + 'a;

fn input_facts(suite: &Suite, all: &[EpisodeFact], subset: &[usize], tag_of: &TagOf) -> Vec<InputFact> {
    subset
        .iter()
        .map(|&i| {
            let c = &suite.cases[all[i].case];
            let f = &c.facts[all[i].fact];
            let key = &f.sections[0];
            let label = c.note.sections.iter().find(|d| &d.key == key).map_or(key.clone(), |d| d.heading.clone());
            InputFact { session_no: all[i].session_no, label, text: f.def.text.clone(), bericht: tag_of(c, all[i].fact) }
        })
        .collect()
}

/// Facts a section may draw on under `sources` (all when the section isn't listed).
fn kept(suite: &Suite, all: &[EpisodeFact], key: &str, sources: Option<&Sources>) -> Vec<usize> {
    let allowed = sources.and_then(|s| s.get(key));
    (0..all.len()).filter(|&i| allowed.is_none_or(|a| a.contains(&suite.cases[all[i].case].facts[all[i].fact].sections[0]))).collect()
}

#[allow(clippy::too_many_arguments)]
fn draft_episode(
    t: &Target,
    suite: &Suite,
    ep: &suite::Episode,
    sources: Option<&Sources>,
    tag_of: &TagOf,
    run: usize,
    usage: &mut Usage,
    answered_by: &mut Option<String>,
) -> EpisodeReport {
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
            let input = input_facts(suite, &all, &subset, tag_of);
            let req = draft::Request { keys: std::slice::from_ref(key), facts: &input, tag_keys: &keys, existing: &[], instruction: None };
            let prepared = draft::prepare(&ep.bericht, &frame, &req, &first.patient, dict.clone()).map_err(|e| e.to_string())?;
            let Some(section) = prepared.sections.first() else {
                // No fact tagged for the section: nothing sent, an empty draft.
                return Ok(score::section(suite, ep, key, &all, &subset, &[], 0));
            };
            let answer = chat(t, &prepared.system, &section.user, &draft::chat_options(), usage, answered_by)?;
            let parsed = draft::parse(&answer, section, &input, &prepared.map).map_err(|e| with_answer(e, &answer))?;
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
        println!(
            "  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6} {:>4} {:>4}",
            "case", "recall", "sect", "cite", "prec", "extra", "forbid", "drop", "tagP", "tagR", "dup", "mrg"
        );
        let mut total = ExtractScore::default();
        for c in &r.extract {
            let s = &c.score;
            total.add(s);
            let err = c.error.as_deref().map(|e| format!("  ERROR {}", first_line(e))).unwrap_or_default();
            println!(
                "  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6} {:>4} {:>4}{err}",
                c.case,
                pct(s.found, s.expected),
                pct(s.section_ok, s.found),
                pct(s.citation_ok, s.found),
                pct(s.proposed - s.extra.len(), s.proposed),
                s.extra.len(),
                s.forbidden.len(),
                s.dropped,
                pct(s.tags_ok, s.tags),
                pct(s.tags_recalled, s.tags_expected),
                s.duplicates.len(),
                s.merged
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
            for m in &s.duplicates {
                println!("      twice    {m}");
            }
            for m in &s.wrong_tags {
                println!("      tag      {m}");
            }
        }
        println!(
            "  {:<20} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6} {:>4} {:>4}",
            "TOTAL",
            pct(total.found, total.expected),
            pct(total.section_ok, total.found),
            pct(total.citation_ok, total.found),
            pct(total.proposed - total.extra.len(), total.proposed),
            total.extra.len(),
            total.forbidden.len(),
            total.dropped,
            pct(total.tags_ok, total.tags),
            pct(total.tags_recalled, total.tags_expected),
            total.duplicates.len(),
            total.merged
        );
    }
    if !r.draft.is_empty() {
        println!("\ndrafting ({}, tags: {}{})", r.draft_prompt, r.tags, if r.sources.is_some() { ", with source mapping" } else { "" });
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
        "\n{} requests ({} failed, {} skipped by --max-cost), {} prompt + {} completion tokens ({} reasoning), {:.4} USD billed, {:.1} s model time{}",
        u.requests,
        u.failed,
        u.skipped,
        u.prompt_tokens,
        u.completion_tokens,
        u.reasoning_tokens,
        u.cost_usd,
        u.latency_ms as f64 / 1000.0,
        r.answered_by.as_deref().map(|m| format!(", answered by {m}")).unwrap_or_default()
    );
}
