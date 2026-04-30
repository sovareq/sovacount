//! Public data types shared across all `token-governor` frontends.
//!
//! These types are the wire-format and the in-process API surface. Once
//! published they must remain backwards-compatible across minor versions.

use serde::{Deserialize, Serialize};

/// LLM model-tier tag. Matches the Sovareq `@op`/`@so`/`@hk` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// `@op` — Opus-class reasoning. Architecture, multi-domain, fan-out, >300 LOC.
    #[serde(alias = "@op", alias = "opus", alias = "@opus")]
    Op,
    /// `@so` — Sonnet-class. Code-impl on a known pattern, <300 LOC, ≤3 files.
    #[serde(alias = "@so", alias = "sonnet", alias = "@sonnet")]
    So,
    /// `@hk` — Haiku-class. Trivial: docs, format, lint, comment-fix, <50 LOC.
    #[serde(alias = "@hk", alias = "haiku", alias = "@haiku")]
    Hk,
}

impl Tier {
    /// Canonical short tag (`@op`, `@so`, `@hk`).
    pub fn tag(&self) -> &'static str {
        match self {
            Tier::Op => "@op",
            Tier::So => "@so",
            Tier::Hk => "@hk",
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

/// Conceptual complexity bucket — orthogonal to Tier so a `@so` task can still
/// be flagged as `Complex` if the classifier sees signals beyond plain LOC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    /// Documentation, formatting, trivial fixes.
    Trivial,
    /// Standard implementation on a known pattern.
    Standard,
    /// Architecture, multi-domain, novel design.
    Complex,
}

/// One classification request — what the agent wants to know about a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyRequest {
    /// External tranche/task identifier (e.g. `T-G-1`, `TD-201-F`).
    pub task_id: String,

    /// Free-text scope description in markdown. The classifier reads this
    /// and uses it together with SSOT references and rough size estimates.
    pub scope_md: String,

    /// Optional list of SSOT files relevant to the task. Plain paths,
    /// resolved by the caller. The classifier may use these to inflate
    /// confidence about novelty / architectural impact.
    #[serde(default)]
    pub ssot_refs: Vec<String>,

    /// Caller's rough LOC estimate. `None` if unknown.
    #[serde(default)]
    pub estimated_loc: Option<u32>,

    /// Caller's rough file-count estimate. `None` if unknown.
    #[serde(default)]
    pub estimated_files: Option<u32>,

    /// If `true`, governor must skip cache lookup for this request.
    #[serde(default)]
    pub no_cache: bool,
}

/// One alternative tier the classifier considered but did not pick as primary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeTier {
    pub tier: Tier,
    pub rationale: String,
    /// Extra cost in USD vs the chosen primary. May be negative when this
    /// alternative would actually be cheaper but was rejected on quality grounds.
    pub extra_cost_usd: f64,
}

/// Classifier response — what the governor recommends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResponse {
    /// Primary recommended tier.
    pub tier: Tier,

    /// Concrete provider model id resolved from `tier` via the active mapping
    /// (e.g. `claude-sonnet-4-6`). May be `None` if the caller asked for
    /// tier-only output.
    pub model_hint: Option<String>,

    /// Conceptual complexity (orthogonal to tier).
    pub complexity: Complexity,

    /// One- or two-sentence justification, intended for log-readability.
    pub rationale: String,

    /// Classifier confidence in %, `0..=100`.
    pub confidence: u8,

    /// Estimated tokens the executing agent will consume on input
    /// (system+user prompt+context). Best-effort.
    pub estimated_input_tokens: u32,

    /// Estimated tokens the executing agent will emit on output. Best-effort.
    pub estimated_output_tokens: u32,

    /// Estimated USD cost for executing the task at the chosen tier.
    pub estimated_cost_usd: f64,

    /// Alternative tiers considered. Empty if the choice was unambiguous.
    #[serde(default)]
    pub alternative_tiers: Vec<AlternativeTier>,

    /// True if the answer came from cache rather than a live classifier call.
    #[serde(default)]
    pub from_cache: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_tag_roundtrip_json() {
        for (t, want) in [
            (Tier::Op, "\"op\""),
            (Tier::So, "\"so\""),
            (Tier::Hk, "\"hk\""),
        ] {
            let got = serde_json::to_string(&t).unwrap();
            assert_eq!(got, want);
            let back: Tier = serde_json::from_str(want).unwrap();
            assert_eq!(back, t);
        }
    }

    #[test]
    fn tier_accepts_aliases() {
        for s in ["\"@op\"", "\"opus\"", "\"@opus\""] {
            let t: Tier = serde_json::from_str(s).unwrap();
            assert_eq!(t, Tier::Op);
        }
    }

    #[test]
    fn tier_display_uses_short_tag() {
        assert_eq!(format!("{}", Tier::Op), "@op");
        assert_eq!(format!("{}", Tier::So), "@so");
        assert_eq!(format!("{}", Tier::Hk), "@hk");
    }

    #[test]
    fn classify_request_minimal_deserialises() {
        let json = r#"{"task_id":"X","scope_md":"do thing"}"#;
        let r: ClassifyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(r.task_id, "X");
        assert!(r.ssot_refs.is_empty());
        assert!(r.estimated_loc.is_none());
        assert!(!r.no_cache);
    }
}
