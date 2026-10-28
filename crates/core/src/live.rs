//! Voice session prep over OpenAI GPT-Live (`gpt-live-1`, WebRTC). Rust creates the session
//! with the therapist's OpenAI key: the WebView sends its SDP offer, Rust posts it with the
//! session config to `POST /v1/live/sessions` and hands back the SDP answer. Key, instructions
//! and brief never reach JS; audio and the data channel then run between WebView and OpenAI.
//!
//! Afterwards the conversation (restored locally) is pseudonymized again and turned into a prep
//! sheet by the drafting model ([`prepare_sheet`]).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use crate::domain::{Patient, PrepSpeaker, PrepTurn};
use crate::draft::patient_as_pat;
use crate::extract::sha256;
use crate::llm::{api_error, net, ChatOptions, Effort, Endpoint, ProviderId};
use crate::pseudo::{Dictionary, Pseudonymized, Pseudonymizer, TokenMap};
use crate::{Error, Result};

/// Setting key holding the [`LiveConfig`] JSON.
pub const CONFIG_SETTING: &str = "live_config";
pub const MODEL: &str = "gpt-live-1";
/// OpenAI's price (2026-10-28), billed per second; the first 15 s are billed at setup.
pub const USD_PER_MINUTE: f64 = 0.05;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LiveConfig {
    /// Opt-in: voice and pseudonymized notes go to OpenAI.
    #[serde(default)]
    pub enabled: bool,
    /// `None` = [`DEFAULT_VOICE`].
    #[serde(default)]
    pub voice: Option<String>,
}

impl LiveConfig {
    pub fn voice(&self) -> &str {
        self.voice.as_deref().filter(|v| VOICES.iter().any(|x| x.id == *v)).unwrap_or(DEFAULT_VOICE)
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LiveVoice {
    pub id: &'static str,
    pub label: &'static str,
}

pub const DEFAULT_VOICE: &str = "nuri";

/// GPT-Live's German voices (the voice sets the language; there is no language parameter).
pub const VOICES: &[LiveVoice] = &[
    LiveVoice { id: "nuri", label: "Nuri (weiblich)" },
    LiveVoice { id: "juni", label: "Juni (weiblich)" },
    LiveVoice { id: "flitz", label: "Flitz (männlich)" },
];

const INSTRUCTIONS: &str = "\
Du bist eine ruhige, fachlich versierte Gesprächspartnerin für eine Psychotherapeutin. Sie bereitet \
sich auf die heutige Sitzung mit einer Patientin bzw. einem Patienten vor, im Text „Pat.“ genannt. \
Sprich Deutsch, knapp und natürlich, wie eine erfahrene Kollegin.

Unten steht, was aus früheren Sitzungen dokumentiert ist. Hilf ihr, sich zu erinnern und den Fokus \
für heute zu finden: fasse auf Wunsch zusammen, nenne offene Vereinbarungen und Hausaufgaben, schlage \
Fragen und Schwerpunkte für heute vor und denke mit, wenn sie laut überlegt.

Regeln:
- Erfinde nichts. Was nicht in der Dokumentation steht, weißt du nicht; sag das offen.
- Stelle keine Diagnosen, nenne keine Diagnoseschlüssel und empfiehl keine Medikamente.
- Du hast keine Werkzeuge und keinen Zugriff auf weitere Daten.
- Platzhalter wie [DATUM_1] liest du nicht vor; sag stattdessen „an dem notierten Datum“.
- Antworte in kurzen Absätzen; lass ihr Raum zu sprechen.

Beginne mit einer kurzen Begrüßung, sag in einem Satz, worauf die Dokumentation beruht, und frag, \
womit sie anfangen möchte.";

const NO_DOCS: &str = "Für diese Patientin bzw. diesen Patienten liegt noch keine unterschriebene Dokumentation vor. \
Hilf ihr trotzdem, sich zu sammeln und Schwerpunkte für die Sitzung zu überlegen.";

/// Sent as the first (developer) message so the assistant opens the conversation: with
/// instructions alone GPT-Live waits for the user to speak.
const OPENING: &str = "Begrüße die Therapeutin jetzt kurz auf Deutsch, sag in einem Satz, worauf die Dokumentation beruht, und frag, womit sie anfangen möchte.";

/// What the voice model says when it asks for backend work (client delegation).
pub const NO_BACKEND: &str = "Dazu liegen mir keine weiteren Informationen vor als die Dokumentation, die ich habe.";

/// The full instructions: fixed text plus the pseudonymized brief.
pub fn instructions(brief: &Pseudonymized<String>) -> String {
    let docs = if brief.get().trim().is_empty() { NO_DOCS } else { brief.get() };
    format!("{INSTRUCTIONS}\n\nDokumentation:\n{docs}")
}

/// `POST /live/sessions` body: client delegation (nothing runs on OpenAI's side but the voice
/// model), not stored.
pub fn session_body(voice: &str, brief: &Pseudonymized<String>, sdp: &str) -> Value {
    json!({
        "session": {
            "model": MODEL,
            "instructions": instructions(brief),
            "audio": { "output": { "voice": voice } },
            "input": [{ "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": OPENING }] }],
            "delegation": { "type": "client" },
            "store": false,
        },
        "transport": { "type": "webrtc", "sdp": sdp },
    })
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LiveSession {
    pub id: String,
    /// SDP answer for the WebView's `setRemoteDescription`.
    pub sdp: String,
}

/// Creates the session and exchanges the SDP offer for the answer.
pub fn create_session(ep: &Endpoint, voice: &str, brief: &Pseudonymized<String>, sdp_offer: &str) -> Result<LiveSession> {
    if ep.provider != ProviderId::Openai {
        return Err(Error::Invalid("Die Sprachvorbereitung läuft nur über OpenAI".into()));
    }
    if !sdp_offer.starts_with("v=0") {
        return Err(Error::Invalid("Ungültiges SDP-Angebot".into()));
    }
    let body = session_body(voice, brief, sdp_offer).to_string();
    let mut resp = ep
        .auth(ep.agent(Duration::from_secs(30)).post(&ep.url("/live/sessions")?))
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(net)?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(net)?;
    if !(200..300).contains(&status) {
        return Err(Error::Llm(api_error(status, &text)));
    }
    let v: Value = serde_json::from_str(&text)?;
    let id = v.pointer("/session/id").and_then(Value::as_str);
    let sdp = v.pointer("/transport/sdp").and_then(Value::as_str);
    match (id, sdp) {
        (Some(id), Some(sdp)) => Ok(LiveSession { id: id.into(), sdp: sdp.into() }),
        _ => Err(Error::Llm("Unerwartete Antwort von OpenAI (keine Sitzung)".into())),
    }
}

// ---------- prep sheet ----------

pub const SHEET_PROMPT_VERSION: &str = "prep-sheet-2026.1";

const SHEET_SYSTEM: &str = "\
Du erstellst aus einem Vorbereitungsgespräch einer Psychotherapeutin mit einer KI-Assistentin ein \
kurzes Vorbereitungsblatt für die heutige Sitzung. Die Person in Behandlung heißt durchgehend „Pat.“. \
Andere Personen nur mit ihrer Rolle nennen; Platzhalter in eckigen Klammern nicht übernehmen.

Gliederung, genau diese drei Überschriften, darunter Stichpunkte:
## Themen
## Offene Vereinbarungen
## Fragen und Schwerpunkte für heute

Nur Inhalte aus dem Gespräch, nichts erfinden. Die Überlegungen der Therapeutin haben Vorrang vor \
Vorschlägen der Assistentin. Keine Diagnosen, keine Medikamente. Ist ein Abschnitt leer, schreib \
„– nichts besprochen“. Keine Einleitung, kein Schluss.";

pub struct SheetPrepared {
    pub system: Pseudonymized<String>,
    pub user: Pseudonymized<String>,
    pub map: TokenMap,
    pub digest: String,
}

/// The conversation as one pseudonymized message (what the therapist said wasn't pseudonymized
/// on the way to OpenAI's voice model, but it is before going to the drafting model).
pub fn prepare_sheet(turns: &[PrepTurn], patient: &Patient, dict: Dictionary) -> Result<SheetPrepared> {
    if turns.iter().all(|t| t.text.trim().is_empty()) {
        return Err(Error::Invalid("Das Gespräch ist leer".into()));
    }
    let mut convo = String::from("Gespräch:\n");
    for t in turns {
        let who = match t.speaker {
            PrepSpeaker::Therapist => "Therapeutin",
            PrepSpeaker::Assistant => "Assistentin",
        };
        // A placeholder left unrestored in a caption would make the pseudonymizer refuse.
        let text = t.text.trim().replace('[', "(").replace(']', ")");
        convo.push_str(&format!("{who}: {}\n", patient_as_pat(&text, patient)));
    }
    let mut p = Pseudonymizer::new(dict);
    let system = p.pseudonymize(SHEET_SYSTEM)?;
    let user = p.pseudonymize(&convo)?;
    let digest = sha256(&format!("{SHEET_PROMPT_VERSION}\n{}\n{}", system.get(), user.get()));
    Ok(SheetPrepared { system, user, map: p.into_map(), digest })
}

pub fn sheet_options() -> ChatOptions {
    ChatOptions { temperature: 0.2, max_tokens: 4096, timeout: Duration::from_secs(180), reasoning: Some(Effort::Low) }
}

/// The model's answer (restored locally) as HTML: `## ` headings (bold), `- ` bullets, other lines as
/// paragraphs. Text is escaped.
pub fn sheet_html(answer: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut html = String::new();
    let mut in_list = false;
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let item = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).or_else(|| line.strip_prefix("• "));
        if item.is_none() && in_list {
            html.push_str("</ul>");
            in_list = false;
        }
        if let Some(h) = line.strip_prefix('#').map(|h| h.trim_start_matches('#').trim()) {
            // The note editor has no headings: bold paragraphs instead.
            html.push_str(&format!("<p><strong>{}</strong></p>", esc(h)));
        } else if let Some(item) = item {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>", esc(item.trim())));
        } else {
            html.push_str(&format!("<p>{}</p>", esc(line)));
        }
    }
    if in_list {
        html.push_str("</ul>");
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pseudo::EntityKind;

    fn brief(text: &str) -> Pseudonymized<String> {
        Pseudonymizer::new(Dictionary::new()).pseudonymize(text).unwrap()
    }

    #[test]
    fn body_has_client_delegation_voice_and_brief() {
        let b = session_body("juni", &brief("Letzte Sitzung:\nVerlauf: Pat. schläft besser."), "v=0\r\n...");
        assert_eq!(b["session"]["model"], MODEL);
        assert_eq!(b["session"]["audio"]["output"]["voice"], "juni");
        assert_eq!(b["session"]["delegation"]["type"], "client");
        assert_eq!(b["session"]["store"], false);
        assert_eq!(b["session"]["input"][0]["role"], "developer");
        assert!(b["session"]["input"][0]["content"][0]["text"].as_str().unwrap().starts_with("Begrüße"));
        assert_eq!(b["transport"]["type"], "webrtc");
        assert!(b["session"]["instructions"].as_str().unwrap().ends_with("Pat. schläft besser."));
        assert!(b["session"].get("audio").unwrap().get("format").is_none(), "WebRTC negotiates the format");
        let empty = session_body("nuri", &brief(""), "v=0");
        assert!(empty["session"]["instructions"].as_str().unwrap().contains("noch keine unterschriebene"));
    }

    #[test]
    fn voice_falls_back_to_a_german_one() {
        assert_eq!(LiveConfig::default().voice(), "nuri");
        assert_eq!(LiveConfig { voice: Some("flitz".into()), ..Default::default() }.voice(), "flitz");
        assert_eq!(LiveConfig { voice: Some("marin".into()), ..Default::default() }.voice(), "nuri");
    }

    #[test]
    fn sheet_input_is_pseudonymized_with_pat() {
        let patient: Patient = serde_json::from_value(serde_json::json!({
            "id": "p1", "first_name": "Anna", "last_name": "Beispiel", "consent_recording": false,
            "status": "active", "created_at": "x"
        }))
        .unwrap();
        let mut dict = Dictionary::from_patient(&patient);
        dict.add(EntityKind::Person, "Karl");
        let turns = vec![
            PrepTurn { speaker: PrepSpeaker::Therapist, text: "Wie ging es Frau Beispiel mit Karl?".into() },
            PrepTurn { speaker: PrepSpeaker::Assistant, text: "Pat. hatte Streit.".into() },
        ];
        let s = prepare_sheet(&turns, &patient, dict).unwrap();
        assert_eq!(s.user.get(), "Gespräch:\nTherapeutin: Wie ging es Pat. mit [PERSON_1]?\nAssistentin: Pat. hatte Streit.\n");
        assert_eq!(s.map.restore("[PERSON_1]").text, "Karl");
        assert!(prepare_sheet(&[], &patient, Dictionary::new()).is_err());
    }

    #[test]
    fn sheet_html_lists_and_escapes() {
        let h = sheet_html("## Themen\n- Schlaf <besser>\n- Arbeit\n## Fragen\nWie geht es?\n");
        assert_eq!(h, "<p><strong>Themen</strong></p><ul><li>Schlaf &lt;besser&gt;</li><li>Arbeit</li></ul><p><strong>Fragen</strong></p><p>Wie geht es?</p>");
    }
}
