//! The cloud models thera recommends on OpenRouter, so the therapist picks a tier instead of a
//! model id. Every entry was measured with `cargo run -p eval` (suite `de-therapy-v1`,
//! 2026-10-28) and has a zero-retention endpoint; the app sends `zdr: true`, so a model without
//! one would fail anyway. Prices are OpenRouter list prices of that day; what was actually billed
//! comes from the audit log ([`crate::store::Store::llm_spend`]).
//!
//! Not listed on purpose: Mistral (user decision), MiniMax M3.1 Flash (invalid JSON, invented
//! content), GPT-6 Luna Pro (misses facts), Qwen3.8 27B (reasons so much it costs 4× Haiku),
//! Qwen3.8 Flash/Omni (no zero-retention endpoint). Any model id still works under "Erweitert".

use serde::Serialize;
use ts_rs::TS;

use crate::llm::Effort;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Tier {
    Guenstig,
    Ausgewogen,
    Beste,
}

/// The tier shown with the "Empfohlen" badge.
pub const RECOMMENDED: Tier = Tier::Ausgewogen;

/// One curated model. Costs are estimates in USD, computed from [`Profile`].
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CloudModel {
    pub id: String,
    pub label: String,
    pub vendor: String,
    /// Set for the one model that stands for a tier.
    pub tier: Option<Tier>,
    /// 1–5, from the eval: facts found and filed right, draft precision, nothing invented.
    pub quality: u8,
    /// USD per million tokens.
    pub price_in: f64,
    pub price_out: f64,
    pub cost_note: f64,
    pub cost_bericht: f64,
    pub cost_month: f64,
}

struct Entry {
    id: &'static str,
    label: &'static str,
    vendor: &'static str,
    tier: Option<Tier>,
    quality: u8,
    price_in: f64,
    price_out: f64,
    /// Reasoning levels the model accepts; a wanted level it lacks maps to the next lower one.
    efforts: &'static [Effort],
    /// Share of output tokens spent on reasoning, measured in the eval.
    reasoning_share: f64,
}

const ALL: &[Effort] = &[Effort::Low, Effort::Medium, Effort::High];

const ENTRIES: &[Entry] = &[
    Entry {
        id: "anthropic/claude-haiku-5.5",
        label: "Claude Haiku 5.5",
        vendor: "Anthropic",
        tier: Some(Tier::Guenstig),
        quality: 4,
        price_in: 0.10,
        price_out: 0.50,
        efforts: ALL,
        reasoning_share: 0.68,
    },
    Entry {
        id: "google/gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        vendor: "Google",
        tier: Some(Tier::Ausgewogen),
        quality: 5,
        price_in: 0.825,
        price_out: 4.125,
        efforts: ALL,
        reasoning_share: 0.67,
    },
    Entry {
        id: "openai/gpt-6.1-sol-pro",
        label: "GPT-6.1 Sol Pro",
        vendor: "OpenAI",
        tier: Some(Tier::Beste),
        quality: 5,
        price_in: 2.0,
        price_out: 10.0,
        efforts: ALL,
        reasoning_share: 0.29,
    },
    Entry {
        id: "deepseek/deepseek-v4.1-flash-1017",
        label: "DeepSeek V4.1 Flash",
        vendor: "DeepSeek",
        tier: None,
        quality: 3,
        price_in: 0.30,
        price_out: 1.20,
        // No "medium": OpenRouter rounds it up and a draft ran out of tokens thinking.
        efforts: &[Effort::Low, Effort::High],
        reasoning_share: 0.84,
    },
    Entry {
        id: "google/gemini-4-argon",
        label: "Gemini 4 Argon",
        vendor: "Google",
        tier: None,
        quality: 4,
        price_in: 2.0,
        price_out: 10.0,
        efforts: ALL,
        reasoning_share: 0.0,
    },
    Entry {
        id: "anthropic/claude-sonnet-5.5",
        label: "Claude Sonnet 5.5",
        vendor: "Anthropic",
        tier: None,
        quality: 3,
        price_in: 2.0,
        price_out: 10.0,
        efforts: ALL,
        reasoning_share: 0.0,
    },
];

/// Token use of one unit of work, without reasoning. A 50-minute session is ~15k tokens of
/// transcript; a Bericht drafts ~8 sections from the applied facts.
pub struct Profile {
    pub note_in: f64,
    pub note_out: f64,
    pub bericht_in: f64,
    pub bericht_out: f64,
    pub notes_per_month: f64,
    pub berichte_per_month: f64,
}

/// 20 sessions a week, a Bericht now and then.
pub const PROFILE: Profile = Profile {
    note_in: 15_000.0,
    note_out: 2_000.0,
    bericht_in: 25_000.0,
    bericht_out: 5_000.0,
    notes_per_month: 80.0,
    berichte_per_month: 2.0,
};

fn cost(e: &Entry, input: f64, visible_out: f64) -> f64 {
    let out = visible_out / (1.0 - e.reasoning_share.min(0.9));
    (input * e.price_in + out * e.price_out) / 1e6
}

fn view(e: &Entry) -> CloudModel {
    let p = &PROFILE;
    let note = cost(e, p.note_in, p.note_out);
    let bericht = cost(e, p.bericht_in, p.bericht_out);
    CloudModel {
        id: e.id.into(),
        label: e.label.into(),
        vendor: e.vendor.into(),
        tier: e.tier,
        quality: e.quality,
        price_in: e.price_in,
        price_out: e.price_out,
        cost_note: note,
        cost_bericht: bericht,
        cost_month: note * p.notes_per_month + bericht * p.berichte_per_month,
    }
}

/// Tier models first (Günstig, Ausgewogen, Beste), then the others by quality.
pub fn models() -> Vec<CloudModel> {
    let mut v: Vec<CloudModel> = ENTRIES.iter().map(view).collect();
    let rank = |m: &CloudModel| match m.tier {
        Some(Tier::Guenstig) => 0,
        Some(Tier::Ausgewogen) => 1,
        Some(Tier::Beste) => 2,
        None => 3,
    };
    v.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.quality.cmp(&a.quality)));
    v
}

/// The reasoning level actually sent: `wanted` if the model knows it, else the next lower level,
/// else its lowest. Models outside the catalog get `wanted` unchanged.
pub fn effort_for(model: &str, wanted: Effort) -> Effort {
    let Some(e) = ENTRIES.iter().find(|e| e.id == model) else { return wanted };
    let rank = |x: Effort| match x {
        Effort::Low => 0,
        Effort::Medium => 1,
        Effort::High => 2,
    };
    e.efforts
        .iter()
        .copied()
        .filter(|x| rank(*x) <= rank(wanted))
        .max_by_key(|x| rank(*x))
        .or_else(|| e.efforts.iter().copied().min_by_key(|x| rank(*x)))
        .unwrap_or(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_model_per_tier_and_no_mistral() {
        let m = models();
        for t in [Tier::Guenstig, Tier::Ausgewogen, Tier::Beste] {
            assert_eq!(m.iter().filter(|x| x.tier == Some(t)).count(), 1, "{t:?}");
        }
        assert_eq!(m[0].tier, Some(Tier::Guenstig));
        assert!(m.iter().all(|x| !x.id.starts_with("mistralai/")));
        assert!(m.iter().all(|x| (1..=5).contains(&x.quality)));
    }

    #[test]
    fn missing_effort_maps_down_not_up() {
        assert_eq!(effort_for("deepseek/deepseek-v4.1-flash-1017", Effort::Medium), Effort::Low);
        assert_eq!(effort_for("deepseek/deepseek-v4.1-flash-1017", Effort::High), Effort::High);
        assert_eq!(effort_for("anthropic/claude-haiku-5.5", Effort::Medium), Effort::Medium);
        assert_eq!(effort_for("someone/unknown", Effort::Medium), Effort::Medium);
    }

    #[test]
    fn costs_follow_prices_and_reasoning() {
        let m = models();
        let by = |id: &str| m.iter().find(|x| x.id == id).unwrap().clone();
        let haiku = by("anthropic/claude-haiku-5.5");
        // 15k in at 0.10 + 2k / (1 - 0.68) = 6250 out at 0.50.
        assert!((haiku.cost_note - (15_000.0 * 0.10 + 6_250.0 * 0.50) / 1e6).abs() < 1e-9);
        assert!(haiku.cost_month < by("google/gemini-3.8-flash").cost_month);
        assert!(by("google/gemini-3.8-flash").cost_month < by("openai/gpt-6.1-sol-pro").cost_month);
    }
}
