//! Demo-Praxis (slice 34): fictional patients, sessions, transcripts, notes, summaries and a
//! Bericht draft, seeded through the normal store API into a database of its own. The app opens
//! it in a separate data directory, so it never mixes with real data.
//!
//! Everything is dated relative to `today`: sessions lie in the past weeks, one is today, a few
//! are planned. Timestamps the store sets to "now" are moved back to the session's time.

use std::path::Path;

use rusqlite::params;
use serde_json::json;
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};

use crate::diarize::{self, Diarization};
use crate::domain::*;
use crate::extract::DraftFact;
use crate::store::{Store, SummaryMeta};
use crate::summary::SummaryPoint;
use crate::{stt, Result};

/// Provider/model recorded on the seeded AI results ("Was wurde gesendet?" stays empty).
const PROVIDER: &str = "demo";
const MODEL: &str = "Beispieldaten";

/// Today's date in Germany.
pub fn today() -> Date {
    let utc = OffsetDateTime::now_utc();
    utc.to_offset(berlin_offset(utc.date())).date()
}

/// CET/CEST by date (the switch at 01:00 UTC on the last Sunday doesn't matter for sessions).
fn berlin_offset(day: Date) -> UtcOffset {
    let last_sunday = |m: Month| {
        let mut d = Date::from_calendar_date(day.year(), m, 31).expect("March and October have 31 days");
        while d.weekday() != Weekday::Sunday {
            d = d.previous_day().expect("not the first day of the calendar");
        }
        d
    };
    let summer = day >= last_sunday(Month::March) && day < last_sunday(Month::October);
    UtcOffset::from_hms(if summer { 2 } else { 1 }, 0, 0).expect("valid offset")
}

/// `day` at `hour:minute` German time, as the store's RFC 3339 UTC.
fn at(day: Date, hour: u8, minute: u8) -> String {
    let local = PrimitiveDateTime::new(day, Time::from_hms(hour, minute, 0).expect("valid time"));
    rfc3339(local.assume_offset(berlin_offset(day)))
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.to_offset(UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc3339)
        .expect("rfc3339 formatting cannot fail")
}

fn plus_minutes(ts: &str, minutes: i64) -> String {
    let t = OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).expect("seeded timestamps parse");
    rfc3339(t + Duration::minutes(minutes))
}

fn ymd(day: Date) -> String {
    format!("{:04}-{:02}-{:02}", day.year(), u8::from(day.month()), day.day())
}

// ---------- content ----------

/// T = therapist, P = patient.
type Line = (&'static str, &'static str);

/// Short signed notes for sessions without a recording: (Inhalt und Verlauf, Interventionen).
const ROUTINE_NOTES: &[(&str, &str)] = &[
    ("Pat. berichtet von einer ruhigeren Woche, Stimmung wechselhaft.", "Wochenrückblick, Aktivitätenprotokoll besprochen."),
    ("Pat. schildert Belastung durch Konflikte im Team.", "Situationsanalyse nach SORKC, Validierung."),
    ("Pat. konnte zwei geplante Aktivitäten umsetzen.", "Verstärkung, Planung der nächsten Woche."),
    ("Thema: Rückzug am Wochenende und Grübeln am Abend.", "Psychoedukation Grübeln, Grübelzeit eingeführt."),
    ("Pat. wirkt etwas zuversichtlicher, Schlaf weiter unruhig.", "Schlafhygiene besprochen, Schlafprotokoll vereinbart."),
    ("Pat. berichtet von einem Streit mit der Mutter.", "Kommunikationsübung, Rollenspiel."),
];

struct Recorded {
    lines: &'static [Line],
    /// Note facts as (note section, text, transcript lines, Bericht sections).
    facts: &'static [(&'static str, &'static str, &'static [usize], &'static [&'static str])],
    /// Applied and signed, or left as open proposals in an unsigned note.
    signed: bool,
    /// Key points with the transcript lines and note sections they rest on.
    summary: &'static [(&'static str, &'static [usize], &'static [&'static str])],
}

const LENA_SLEEP: Recorded = Recorded {
    lines: &[
        ("T", "Wie ist es Ihnen seit letzter Woche gegangen?"),
        ("P", "Gemischt. Ich bin fast jeden Morgen um vier aufgewacht und dann nicht mehr eingeschlafen."),
        ("P", "Tagsüber war ich so erschöpft, dass ich nach der Arbeit nur noch auf dem Sofa lag."),
        ("T", "Was ging Ihnen durch den Kopf, wenn Sie um vier wach lagen?"),
        ("P", "Dass ich alles nicht schaffe. Die Projekte im Büro, die Wohnung, einfach alles."),
        ("T", "Das klingt nach sehr viel Druck. Wie hat sich der Spaziergang angefühlt, den wir geplant hatten?"),
        ("P", "Den habe ich zweimal gemacht. Danach war es ein bisschen leichter, das hat mich überrascht."),
        ("T", "Das ist ein wichtiger Hinweis. Aktivität hebt die Stimmung, auch wenn der Antrieb fehlt."),
        ("T", "Lassen Sie uns für diese Woche drei feste Aktivitäten in den Plan eintragen."),
        ("P", "Okay. Spaziergang, einmal Schwimmen und am Sonntag meine Schwester anrufen."),
        ("T", "Und für die Nächte: Wenn Sie länger als zwanzig Minuten wach liegen, stehen Sie kurz auf."),
        ("P", "Das versuche ich. Gedanken, mir etwas anzutun, habe ich keine, falls Sie das fragen wollten."),
    ],
    facts: &[
        ("verlauf", "Pat. berichtet von frühmorgendlichem Erwachen gegen 4 Uhr und ausgeprägter Erschöpfung am Tag.", &[1, 2], &["symptomatik_befund"]),
        ("verlauf", "Grübeln über Überforderung im Beruf und Haushalt in den Wachphasen.", &[4], &["symptomatik_befund"]),
        ("verlauf", "Geplanter Spaziergang zweimal umgesetzt, danach subjektiv leichte Stimmungsbesserung.", &[6], &["behandlungsplan_prognose"]),
        ("interventionen", "Aktivitätenaufbau: drei feste Aktivitäten für die Woche geplant.", &[8, 9], &["behandlungsplan_prognose"]),
        ("interventionen", "Stimuluskontrolle bei nächtlichem Wachliegen eingeführt.", &[10], &[]),
        ("risiko", "Pat. verneint Suizidgedanken.", &[11], &[]),
        ("vereinbarungen", "Spaziergang, Schwimmen, Anruf bei der Schwester; Aufstehen nach 20 Minuten Wachliegen.", &[9, 10], &[]),
    ],
    signed: true,
    summary: &[
        ("Frühmorgendliches Erwachen gegen 4 Uhr, tagsüber erschöpft.", &[1, 2], &["verlauf"]),
        ("Zwei Spaziergänge umgesetzt, danach etwas leichtere Stimmung.", &[6], &[]),
        ("Drei feste Aktivitäten für die Woche vereinbart, Stimuluskontrolle für die Nacht.", &[8, 9, 10], &["vereinbarungen"]),
        ("Keine Suizidgedanken.", &[11], &["risiko"]),
    ],
};

const LENA_WORK: Recorded = Recorded {
    lines: &[
        ("T", "Sie hatten erwähnt, dass es diese Woche im Büro schwierig war."),
        ("P", "Ja. Meine Chefin hat in der Teamrunde gesagt, der Bericht müsse überarbeitet werden."),
        ("P", "Ich habe sofort gedacht: Ich bin unfähig, gleich merken alle, dass ich es nicht kann."),
        ("T", "Wie stark haben Sie diesen Gedanken in dem Moment geglaubt, von null bis hundert?"),
        ("P", "Neunzig. Ich bin danach auf die Toilette und habe geweint."),
        ("T", "Was spricht dafür, was dagegen, dass Sie unfähig sind?"),
        ("P", "Dagegen spricht, dass ich die letzten drei Berichte ohne Änderungen abgegeben habe."),
        ("P", "Und die Chefin hat bei anderen auch Änderungen verlangt."),
        ("T", "Wenn Sie das jetzt zusammennehmen, wie stark glauben Sie den Gedanken noch?"),
        ("P", "Vielleicht vierzig. Es fühlt sich immer noch schlecht an, aber weniger."),
        ("T", "Das Gedankenprotokoll können Sie diese Woche selbst ausprobieren, bei jeder starken Stimmungsschwankung."),
        ("P", "Die Aktivitäten habe ich übrigens alle drei gemacht. Das Schwimmen war richtig gut."),
    ],
    facts: &[
        ("verlauf", "Kritik der Vorgesetzten in der Teamrunde löste automatischen Gedanken „Ich bin unfähig“ aus (Überzeugung 90 %).", &[1, 2, 3, 4], &["symptomatik_befund", "lebensgeschichte_modell"]),
        ("verlauf", "Alle drei geplanten Aktivitäten umgesetzt, Schwimmen als positiv erlebt.", &[11], &["behandlungsplan_prognose"]),
        ("interventionen", "Kognitive Umstrukturierung mit Pro-Contra-Analyse; Überzeugung sank auf 40 %.", &[5, 6, 7, 8, 9], &["behandlungsplan_prognose"]),
        ("vereinbarungen", "Gedankenprotokoll bei starken Stimmungsschwankungen als Hausaufgabe.", &[10], &[]),
    ],
    signed: true,
    summary: &[
        ("Kritik im Team löste „Ich bin unfähig“ aus, anfangs zu 90 % geglaubt.", &[1, 2, 4], &["verlauf"]),
        ("Mit Pro-Contra-Analyse sank die Überzeugung auf 40 %.", &[6, 7, 9], &["interventionen"]),
        ("Alle drei Aktivitäten umgesetzt, Schwimmen tat gut.", &[11], &[]),
        ("Hausaufgabe: Gedankenprotokoll.", &[10], &["vereinbarungen"]),
    ],
};

const LENA_SISTER: Recorded = Recorded {
    lines: &[
        ("T", "Wie ist es mit dem Gedankenprotokoll gelaufen?"),
        ("P", "Ich habe es viermal ausgefüllt. Meistens ging es um die Arbeit, einmal um meine Schwester."),
        ("P", "Katrin hat am Wochenende abgesagt, und ich dachte sofort, sie hat keine Lust mehr auf mich."),
        ("T", "Was haben Sie im Protokoll als alternative Sicht gefunden?"),
        ("P", "Dass ihr Sohn krank war. Sie hat ja sogar angeboten, den Besuch nachzuholen."),
        ("T", "Wie war die Stimmung insgesamt in dieser Woche?"),
        ("P", "Besser. Ich schlafe jetzt meistens bis sechs, und ich war wieder beim Chor."),
        ("P", "Aber ich habe Angst, dass es wieder kippt, wenn im November der Jahresabschluss kommt."),
        ("T", "Diese Sorge nehmen wir ernst. Wir können einen Plan für belastende Wochen erarbeiten."),
        ("T", "Was wären frühe Warnzeichen, an denen Sie merken, dass es Ihnen schlechter geht?"),
        ("P", "Wenn ich wieder um vier aufwache und die Verabredungen absage."),
        ("T", "Gut. Das schreiben wir in den Plan, zusammen mit dem, was Ihnen bisher geholfen hat."),
    ],
    facts: &[
        ("verlauf", "Gedankenprotokoll viermal ausgefüllt, überwiegend zu Arbeitssituationen.", &[1], &["behandlungsplan_prognose"]),
        ("verlauf", "Absage der Schwester zunächst als Ablehnung gedeutet, im Protokoll alternative Erklärung gefunden.", &[2, 3, 4], &["lebensgeschichte_modell"]),
        ("befund", "Stimmung gebessert, Schlaf meist bis 6 Uhr, Wiederaufnahme der Chorproben.", &[6], &["symptomatik_befund", "umwandlung"]),
        ("verlauf", "Sorge vor Rückfall in der Belastungsphase zum Jahresabschluss im November.", &[7], &["umwandlung"]),
        ("interventionen", "Beginn eines Plans für belastende Wochen, frühe Warnzeichen gesammelt.", &[8, 9, 10, 11], &["behandlungsplan_prognose", "umwandlung"]),
    ],
    signed: false,
    summary: &[
        ("Gedankenprotokoll viermal geführt, auch zur Absage der Schwester.", &[1, 2, 4], &[]),
        ("Stimmung besser, Schlaf meist bis 6 Uhr, wieder im Chor.", &[6], &[]),
        ("Sorge vor Rückfall zum Jahresabschluss im November.", &[7], &[]),
        ("Warnzeichen gesammelt: frühes Erwachen, abgesagte Verabredungen.", &[9, 10], &[]),
    ],
};

/// Jonas's latest session: recorded, the transcript still waits for review.
const JONAS_SCHOOL: &[Line] = &[
    ("T", "Du hattest geschrieben, dass die Woche anstrengend war. Magst du erzählen?"),
    ("P", "Am Dienstag war die Mathearbeit. Ich hatte schon am Abend vorher Bauchschmerzen."),
    ("P", "Morgens wollte ich eigentlich nicht in die Schule, aber meine Mutter hat mich gefahren."),
    ("T", "Wie war es dann während der Arbeit?"),
    ("P", "Am Anfang war mein Kopf komplett leer. Nach zehn Minuten ging es dann irgendwie."),
    ("T", "Was hast du in den zehn Minuten gemacht?"),
    ("P", "Ich habe die Aufgabe übersprungen und mit der leichtesten angefangen."),
    ("T", "Das ist eine richtig gute Strategie. Die schreiben wir uns auf."),
    ("P", "Mein Vater meint, ich soll mich einfach nicht so anstellen."),
    ("T", "Wie geht es dir, wenn er das sagt?"),
    ("P", "Dann fühle ich mich noch schlechter, weil ich es ja nicht absichtlich mache."),
    ("T", "Vielleicht laden wir deine Eltern einmal zu einem gemeinsamen Termin ein. Was hältst du davon?"),
];

const JONAS_FIRST: (&str, &str) = (
    "Erstgespräch: Prüfungsängste seit dem Wechsel in die 10. Klasse, Bauchschmerzen vor Klassenarbeiten, zweimal Schule versäumt.",
    "Exploration, Aufklärung über Probatorik und Rahmenbedingungen, Einwilligungen besprochen.",
);

/// Lena's Umwandlungsbericht, partly written.
const LENA_REPORT: &[(&str, &str)] = &[
    (
        "soziodemographie",
        "Pat. ist 35 Jahre alt, ledig und lebt allein in einer Mietwohnung. Sie arbeitet in Vollzeit als Projektkoordinatorin in einem Ingenieurbüro. Eine ältere Schwester lebt in derselben Stadt, zu den Eltern besteht regelmäßiger Kontakt.",
    ),
    (
        "symptomatik_befund",
        "Zu Beginn der Kurzzeittherapie bestanden gedrückte Stimmung, Antriebsminderung, frühmorgendliches Erwachen gegen 4 Uhr und ausgeprägtes Grübeln über berufliche Überforderung. Im Verlauf hat sich die Stimmung gebessert; Pat. schläft meist bis 6 Uhr und hat soziale Aktivitäten wieder aufgenommen. Suizidgedanken werden durchgehend verneint.",
    ),
    (
        "lebensgeschichte_modell",
        "Pat. wuchs als jüngere von zwei Töchtern auf; Anerkennung erlebte sie vor allem über Leistung. Kritik aktiviert die Grundannahme, unfähig zu sein, und führt zu Rückzug und Grübeln, die die depressive Symptomatik aufrechterhalten.",
    ),
];

// ---------- seeding ----------

/// Seeds the Demo-Praxis into an empty store. `audio_root` is the demo data dir's audio folder.
pub fn seed(store: &Store, audio_root: &Path, today: Date) -> Result<()> {
    let day = |offset: i64| today + Duration::days(offset);
    store.set_setting("therapist_name", &json!("Dr. Paula Beispiel"))?;
    store.set_setting("practice_header", &json!("Praxis für Psychotherapie (Demo)\nBeispielstraße 1\n12345 Musterstadt"))?;
    // The demo has no backup folder; don't nag about it.
    store.set_setting("backup_reminder_days", &json!(0))?;

    // Lena: VT, KZT 1 nearly used up, Umwandlungsbericht in progress.
    let lena = store.create_patient(PatientInput {
        external_ref: Some("DEMO-1".into()),
        first_name: "Lena".into(),
        last_name: "Sommer".into(),
        birth_date: Some(format!("{}-03-14", today.year() - 35)),
        chiffre: Some(format!("S1403{:02}", (today.year() - 35) % 100)),
        pronouns: None,
        gender: Some(Gender::W),
    })?;
    let consent = store.grant_consent(&lena.id, ConsentKind::Recording, Some("Formular unterschrieben, in Akte"))?;
    backdate_consent(store, &consent.id, &at(day(-80), 9, 0))?;
    let lena_case = store.create_episode(EpisodeInput { patient_id: lena.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })?;
    backdate_episode(store, &lena_case.id, &at(day(-80), 9, 0))?;
    store.set_episode_diagnoses(&lena_case.id, vec![Diagnosis { code: "F32.1".into(), label: "Mittelgradige depressive Episode".into(), certainty: "G".into() }])?;
    store.set_episode_stage(&lena_case.id, Stage::Kzt1, Some(&ymd(day(-63))))?;
    for (i, d) in [-77, -70].into_iter().enumerate() {
        held_with_note(store, &lena_case, day(d), 10, 0, ROUTINE_NOTES[i])?;
    }
    for (i, d) in [-63, -56, -49, -42, -35, -28].into_iter().enumerate() {
        held_with_note(store, &lena_case, day(d), 10, 0, ROUTINE_NOTES[(i + 2) % ROUTINE_NOTES.len()])?;
    }
    for (d, rec) in [(-21, &LENA_SLEEP), (-14, &LENA_WORK), (-7, &LENA_SISTER)] {
        recorded(store, audio_root, &lena_case, day(d), 10, 0, rec)?;
    }
    planned(store, &lena_case, day(0), 10, 0)?;
    planned(store, &lena_case, day(7), 10, 0)?;
    let application = store.create_application(&lena_case.id, ApplicationKind::Umwandlung)?;
    let report_id = application.report_id.clone().expect("an application has its Bericht");
    for (key, body) in LENA_REPORT {
        store.save_section(&report_id, key, &format!("<p>{body}</p>"))?;
    }
    let started = at(day(-3), 18, 30);
    store.conn().execute("UPDATE application SET created_at = ?2 WHERE id = ?1", params![application.id, started])?;
    store.conn().execute("UPDATE report SET created_at = ?2 WHERE id = ?1", params![report_id, started])?;

    // Jonas: TP for adolescents in Probatorik; yesterday's recording waits for review.
    let jonas = store.create_patient(PatientInput {
        external_ref: Some("DEMO-2".into()),
        first_name: "Jonas".into(),
        last_name: "Albrecht".into(),
        birth_date: Some(format!("{}-06-02", today.year() - 16)),
        chiffre: Some(format!("A0206{:02}", (today.year() - 16) % 100)),
        pronouns: None,
        gender: Some(Gender::M),
    })?;
    let consent = store.grant_consent(&jonas.id, ConsentKind::Recording, Some("Pat. und Sorgeberechtigte haben unterschrieben"))?;
    backdate_consent(store, &consent.id, &at(day(-8), 16, 0))?;
    let jonas_case = store.create_episode(EpisodeInput { patient_id: jonas.id.clone(), method: Method::Tp, age_group: AgeGroup::Kjp, setting: Setting::Einzel })?;
    backdate_episode(store, &jonas_case.id, &at(day(-8), 16, 0))?;
    held_with_note(store, &jonas_case, day(-8), 16, 0, JONAS_FIRST)?;
    let s = store.create_session(session_input(&jonas_case, day(-1), 16, 0))?;
    let lines = to_segments(JONAS_SCHOOL);
    transcribe(store, audio_root, &s.id, SourceKind::SessionAudio, &lines, JONAS_SCHOOL)?;
    backdate_session(store, &s.id, &at(day(-1), 16, 0))?;
    planned(store, &jonas_case, day(6), 16, 0)?;
    store.create_task(TaskInput {
        date: ymd(today),
        time: None,
        title: "Konsiliarbericht für Jonas bei der Kinderärztin anfordern".into(),
        patient_id: Some(jonas.id.clone()),
        session_id: None,
    })?;

    // Miriam: VT, 10 of 12 KZT sessions held, no Bericht yet (the "bald fällig" banner); the
    // last session has no note.
    let miriam = store.create_patient(PatientInput {
        external_ref: Some("DEMO-3".into()),
        first_name: "Miriam".into(),
        last_name: "Keller".into(),
        birth_date: Some(format!("{}-11-23", today.year() - 48)),
        chiffre: Some(format!("K2311{:02}", (today.year() - 48) % 100)),
        pronouns: None,
        gender: Some(Gender::W),
    })?;
    let miriam_case = store.create_episode(EpisodeInput { patient_id: miriam.id.clone(), method: Method::Vt, age_group: AgeGroup::Adult, setting: Setting::Einzel })?;
    backdate_episode(store, &miriam_case.id, &at(day(-90), 14, 0))?;
    store.set_episode_diagnoses(&miriam_case.id, vec![Diagnosis { code: "F41.1".into(), label: "Generalisierte Angststörung".into(), certainty: "G".into() }])?;
    store.set_episode_stage(&miriam_case.id, Stage::Kzt1, Some(&ymd(day(-65))))?;
    for k in (1..10).rev() {
        held_with_note(store, &miriam_case, day(-2 - 7 * k), 14, 0, ROUTINE_NOTES[k as usize % ROUTINE_NOTES.len()])?;
    }
    store.create_session(session_input(&miriam_case, day(-2), 14, 0))?;
    planned(store, &miriam_case, day(5), 14, 0)?;
    store.create_task(TaskInput {
        date: ymd(today),
        time: Some("13:30".into()),
        title: "Rückruf Miriam wegen Terminverschiebung".into(),
        patient_id: Some(miriam.id.clone()),
        session_id: None,
    })?;
    Ok(())
}

fn session_input(case: &Episode, day: Date, hour: u8, minute: u8) -> SessionInput {
    SessionInput {
        patient_id: case.patient_id.clone(),
        episode_id: Some(case.id.clone()),
        scheduled_at: Some(at(day, hour, minute)),
        modality: Some("praesenz".into()),
        planned_minutes: None,
    }
}

fn planned(store: &Store, case: &Episode, day: Date, hour: u8, minute: u8) -> Result<Session> {
    store.create_session(session_input(case, day, hour, minute))
}

/// A held session with a short signed note, no recording.
fn held_with_note(store: &Store, case: &Episode, day: Date, hour: u8, minute: u8, (verlauf, interventionen): (&str, &str)) -> Result<()> {
    let s = store.create_session(session_input(case, day, hour, minute))?;
    let note = store.create_session_note(&s.id)?;
    store.save_section(&note.id, "verlauf", &format!("<p>{verlauf}</p>"))?;
    store.save_section(&note.id, "interventionen", &format!("<p>{interventionen}</p>"))?;
    store.sign_report(&note.id, "Dr. Paula Beispiel")?;
    backdate_session(store, &s.id, &at(day, hour, minute))
}

/// A recorded session: confirmed transcript with speakers, note with facts, key points.
fn recorded(store: &Store, audio_root: &Path, case: &Episode, day: Date, hour: u8, minute: u8, rec: &Recorded) -> Result<()> {
    let s = store.create_session(session_input(case, day, hour, minute))?;
    let segments = to_segments(rec.lines);
    let transcript = transcribe(store, audio_root, &s.id, SourceKind::SessionAudio, &segments, rec.lines)?;
    store.confirm_transcript(&transcript.id)?;
    let note = store.create_session_note(&s.id)?;
    let sources = store.source_segments(&s.id)?;
    let facts: Vec<DraftFact> = rec
        .facts
        .iter()
        .map(|(section, text, lines, bericht)| DraftFact {
            section: (*section).into(),
            text: (*text).into(),
            segments: lines.to_vec(),
            bericht: bericht.iter().map(|b| (*b).into()).collect(),
        })
        .collect();
    let meta = ExtractionMeta { provider: PROVIDER.into(), model: MODEL.into(), prompt_hash: String::new(), chunks: 1, dropped: 0, merged: 0, tagged: true };
    let saved = store.save_extraction(&note.id, &meta, &facts, &sources)?;
    if rec.signed {
        let edits: Vec<FactEdit> = saved.iter().map(|f| FactEdit { id: f.id.clone(), section_key: f.section_key.clone(), text: f.text.clone(), bericht: None }).collect();
        store.apply_facts(&note.id, &edits)?;
        store.sign_report(&note.id, "Dr. Paula Beispiel")?;
    }
    let points: Vec<SummaryPoint> = rec
        .summary
        .iter()
        .map(|(text, lines, note)| SummaryPoint {
            text: (*text).into(),
            segments: lines.iter().filter_map(|&i| sources.get(i).map(|s| s.segment_id.clone())).collect(),
            note: note.iter().map(|k| (*k).into()).collect(),
        })
        .collect();
    let note_chars = if rec.signed { 400 } else { 0 };
    store.save_summary(&s.id, &SummaryMeta { provider: PROVIDER.into(), model: MODEL.into(), prompt_hash: String::new(), chunks: 1, dropped: 0, note_chars }, &points)?;
    backdate_session(store, &s.id, &at(day, hour, minute))
}

/// Segments of about 12 s each, back to back.
fn to_segments(lines: &[Line]) -> Vec<stt::Segment> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (_, text))| stt::Segment { start_ms: i as i64 * 12_000, end_ms: (i as i64 + 1) * 12_000 - 500, text: (*text).into() })
        .collect()
}

/// Records silence as long as the segments and stores them as its (draft) transcript, with the
/// speakers already told apart.
fn transcribe(store: &Store, audio_root: &Path, session_id: &str, kind: SourceKind, segments: &[stt::Segment], lines: &[Line]) -> Result<Transcript> {
    let (source, mut writer) = store.start_source(session_id, kind, audio_root)?;
    let samples = segments.last().map_or(0, |s| s.end_ms as usize * crate::audio::SAMPLE_RATE as usize / 1000);
    let second = vec![0i16; crate::audio::SAMPLE_RATE as usize];
    let mut left = samples;
    while left > 0 {
        let n = left.min(second.len());
        writer.write(&second[..n])?;
        left -= n;
    }
    store.finish_source(&source.id, "stopped", &writer)?;
    let job = store.enqueue_transcription(&source.id, None)?;
    let claimed = store.claim_next_job()?.filter(|j| j.id == job.id).ok_or_else(|| crate::Error::Invalid("demo: job not claimed".into()))?;
    let speakers = Diarization {
        engine: MODEL.into(),
        labels: lines.iter().map(|(who, _)| Some(if *who == "T" { diarize::THERAPIST } else { diarize::PATIENT }.to_string())).collect(),
        speakers: vec![],
        matched: None,
    };
    store.finish_transcription_with(&claimed.id, stt::ENGINE, stt::DEFAULT_MODEL, segments, Some(&speakers))
}

/// Moves what the store stamped with "now" to the session's own time (50 minutes long).
fn backdate_session(store: &Store, session_id: &str, start: &str) -> Result<()> {
    let end = plus_minutes(start, 50);
    let later = plus_minutes(start, 120);
    let c = store.conn();
    c.execute(
        "UPDATE session SET started_at = CASE WHEN started_at IS NULL THEN NULL ELSE ?2 END,
                ended_at = CASE WHEN ended_at IS NULL THEN NULL ELSE ?3 END WHERE id = ?1",
        params![session_id, start, end],
    )?;
    c.execute("UPDATE session_source SET started_at = ?2, ended_at = ?3 WHERE session_id = ?1", params![session_id, start, end])?;
    c.execute(
        "UPDATE transcript SET created_at = ?2, confirmed_at = CASE WHEN confirmed_at IS NULL THEN NULL ELSE ?3 END
         WHERE source_id IN (SELECT id FROM session_source WHERE session_id = ?1)",
        params![session_id, end, later],
    )?;
    c.execute(
        "UPDATE report SET created_at = ?2, updated_at = ?3, signed_at = CASE WHEN signed_at IS NULL THEN NULL ELSE ?3 END
         WHERE session_id = ?1",
        params![session_id, end, later],
    )?;
    c.execute("UPDATE session_summary SET created_at = ?2, updated_at = ?2 WHERE session_id = ?1", params![session_id, later])?;
    Ok(())
}

fn backdate_consent(store: &Store, id: &str, at: &str) -> Result<()> {
    store.conn().execute("UPDATE consent SET granted_at = ?2 WHERE id = ?1", params![id, at])?;
    Ok(())
}

fn backdate_episode(store: &Store, id: &str, at: &str) -> Result<()> {
    store.conn().execute("UPDATE episode SET started_at = ?2 WHERE id = ?1", params![id, at])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (Store, tempfile::TempDir, Date) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let today = today();
        seed(&store, &dir.path().join("audio"), today).unwrap();
        (store, dir, today)
    }

    #[test]
    fn german_time_follows_daylight_saving() {
        let summer = Date::from_calendar_date(2026, Month::July, 1).unwrap();
        let winter = Date::from_calendar_date(2026, Month::November, 1).unwrap();
        assert_eq!(at(summer, 10, 0), "2026-07-01T08:00:00Z");
        assert_eq!(at(winter, 10, 0), "2026-11-01T09:00:00Z");
    }

    #[test]
    fn demo_practice_has_three_patients_with_work_to_show() {
        let (store, _dir, today) = seeded();
        let patients = store.list_patients(None).unwrap();
        assert_eq!(patients.len(), 3);

        // Lena: three recorded sessions, two signed with applied facts, one waiting.
        let lena = patients.iter().find(|p| p.first_name == "Lena").unwrap();
        let sessions = store.list_sessions(Some(&lena.id)).unwrap();
        assert_eq!(sessions.len(), 13);
        let with_summary = sessions.iter().filter(|s| store.session_summary(&s.id).unwrap().is_some()).count();
        assert_eq!(with_summary, 3);
        let case = &store.list_episodes(&lena.id).unwrap()[0];
        assert!(!store.episode_facts(&case.id).unwrap().is_empty());
        // The Bericht draft: three written sections plus the diagnosis taken over from the case.
        let reports = store.list_reports(None, &now()).unwrap();
        let bericht = reports.iter().find(|r| r.kind == Some(ApplicationKind::Umwandlung)).unwrap();
        assert_eq!(bericht.status, ReportStatus::Draft);
        assert_eq!(bericht.sections_filled, 4);

        // Past sessions keep their own dates, not the seeding time.
        let held = sessions.iter().find(|s| s.started_at.is_some()).unwrap();
        assert!(held.started_at.as_deref().unwrap() < at(today, 0, 0).as_str());
    }

    #[test]
    fn heute_shows_review_due_and_open_work() {
        let (store, _dir, today) = seeded();
        let from = at(today, 0, 0);
        let to = at(today + Duration::days(1), 0, 0);
        let due = store.due_items(&at(today - Duration::days(30), 0, 0), &from, &to).unwrap();
        let kinds: Vec<&str> = due.iter().map(|d| d.kind.as_str()).collect();
        assert!(kinds.contains(&"transcript_review"), "{kinds:?}");
        assert!(kinds.contains(&"application_due"), "{kinds:?}");
        assert_eq!(store.sessions_between(&from, &to).unwrap().len(), 1);
        // Jonas's recording keeps its audio until the transcript is confirmed.
        let jonas = store.list_patients(Some("Jonas")).unwrap().remove(0);
        let last = store.list_sessions(Some(&jonas.id)).unwrap().into_iter().find(|s| s.status == "recorded").unwrap();
        let source = &store.list_sources(&last.id).unwrap()[0];
        assert!(source.audio_deleted_at.is_none());
        assert_eq!(store.transcript_for_source(&source.id).unwrap().unwrap().status, TranscriptStatus::Draft);
    }
}
