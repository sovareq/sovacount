//! Rule-based fast-path. Used to short-circuit obvious cases without an LLM call.
//!
//! Decision rules derived from the Sovareq tag-convention spec:
//! - **HK fast-path**: tiny single-file scope with no architectural language and
//!   no SSOT references.
//! - **OP fast-path**: clearly large or multi-domain scope (LOC / file count over
//!   the soft limits, or two-or-more architectural markers in the scope text).
//! - Everything in between is sent to the LLM — Sonnet is the most ambiguous
//!   tier and the cost of a wrong call there is asymmetric.
//!
//! These rules are intentionally narrow. The classifier prompt covers the full
//! decision-tree; the fast-path only fires when the answer is *obvious*.

use crate::pricing::{PricingProvider, estimate_cost_usd};
use crate::types::{ClassifyRequest, ClassifyResponse, Complexity, Tier};

/// Words in `scope_md` (case-insensitive) that signal architectural / multi-domain work.
const ARCHITECTURAL_MARKERS: &[&str] = &[
    "architecture",
    "design",
    "rewrite",
    "fan-out",
    "fanout",
    "ssot",
    "contract",
    "schema-change",
    "auth",
    "security",
    "migration",
    "breaking",
    "multi-module",
    "multi-domain",
];

/// HK fast-path: input ~1500, output ~400. Conservative.
const HK_INPUT_TOKENS: u32 = 1_500;
const HK_OUTPUT_TOKENS: u32 = 400;

/// OP fast-path: input ~12000, output ~3500. Conservative.
const OP_INPUT_TOKENS: u32 = 12_000;
const OP_OUTPUT_TOKENS: u32 = 3_500;

/// Confidence levels for the two fast-path branches.
const HK_CONFIDENCE: u8 = 92;
const OP_CONFIDENCE: u8 = 88;

/// Returns `Some(response)` only if the request is unambiguous on size or
/// architectural signals. Returns `None` if the LLM should make the call.
///
/// `provider` selects the rate-card used to fill `estimated_cost_usd` so the
/// fast-path's reported cost matches the active provider. The returned
/// response leaves `model_hint` empty — the classifier fills it in from
/// `Config::tier_mapping` after the fast-path returns.
pub fn fast_path(req: &ClassifyRequest, provider: PricingProvider) -> Option<ClassifyResponse> {
    let lower = req.scope_md.to_lowercase();
    let architectural_hits = count_architectural_markers(&lower);

    // OP fast-path: any one of the size/breadth signals is enough.
    let big_loc = req.estimated_loc.unwrap_or(0) > 300;
    let big_files = req.estimated_files.unwrap_or(0) > 5;
    let many_markers = architectural_hits >= 2;

    if big_loc || big_files || many_markers {
        return Some(ClassifyResponse {
            tier: Tier::Op,
            model_hint: None,
            complexity: Complexity::Complex,
            rationale: op_rationale(big_loc, big_files, many_markers),
            confidence: OP_CONFIDENCE,
            estimated_input_tokens: OP_INPUT_TOKENS,
            estimated_output_tokens: OP_OUTPUT_TOKENS,
            estimated_cost_usd: estimate_cost_usd(
                provider,
                Tier::Op,
                OP_INPUT_TOKENS,
                OP_OUTPUT_TOKENS,
            ),
            alternative_tiers: vec![],
            from_cache: false,
        });
    }

    // HK fast-path: tiny + single-file + no SSOT + no architectural language.
    let tiny_loc = req.estimated_loc.is_some_and(|n| n < 50);
    let single_file = req.estimated_files == Some(1);
    let no_ssot = req.ssot_refs.is_empty();
    let no_markers = architectural_hits == 0;

    if tiny_loc && single_file && no_ssot && no_markers {
        return Some(ClassifyResponse {
            tier: Tier::Hk,
            model_hint: None,
            complexity: Complexity::Trivial,
            rationale: "Tiny single-file scope with no architectural markers \
                        and no SSOT references — handled by the cheapest tier."
                .into(),
            confidence: HK_CONFIDENCE,
            estimated_input_tokens: HK_INPUT_TOKENS,
            estimated_output_tokens: HK_OUTPUT_TOKENS,
            estimated_cost_usd: estimate_cost_usd(
                provider,
                Tier::Hk,
                HK_INPUT_TOKENS,
                HK_OUTPUT_TOKENS,
            ),
            alternative_tiers: vec![],
            from_cache: false,
        });
    }

    None
}

fn count_architectural_markers(lower: &str) -> usize {
    ARCHITECTURAL_MARKERS
        .iter()
        .filter(|m| lower.contains(*m))
        .count()
}

fn op_rationale(big_loc: bool, big_files: bool, many_markers: bool) -> String {
    let mut reasons = Vec::with_capacity(3);
    if big_loc {
        reasons.push("scope exceeds 300 LOC");
    }
    if big_files {
        reasons.push("touches more than 5 files");
    }
    if many_markers {
        reasons.push("scope contains multiple architectural markers");
    }
    format!(
        "Routed to Opus tier because {} — fast-path heuristic.",
        reasons.join(" and "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(scope: &str, loc: Option<u32>, files: Option<u32>, ssot: Vec<&str>) -> ClassifyRequest {
        ClassifyRequest {
            task_id: "T-TEST".into(),
            scope_md: scope.into(),
            ssot_refs: ssot.into_iter().map(String::from).collect(),
            estimated_loc: loc,
            estimated_files: files,
            no_cache: false,

            shift: 0,
        }
    }

    #[test]
    fn hk_fast_path_typo_fix() {
        let r = req("Fix typos in README.md.", Some(5), Some(1), vec![]);
        let resp = fast_path(&r, PricingProvider::Anthropic).expect("hk fast-path should fire");
        assert_eq!(resp.tier, Tier::Hk);
        assert_eq!(resp.complexity, Complexity::Trivial);
        assert!(resp.confidence >= 90);
        assert!(resp.model_hint.is_none(), "classifier fills model_hint in");
    }

    #[test]
    fn hk_fast_path_blocked_by_architectural_word() {
        // Single file, tiny LOC — but mentions "auth", which is suspicious.
        let r = req("Tweak auth header handling.", Some(20), Some(1), vec![]);
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }

    #[test]
    fn hk_fast_path_blocked_by_ssot_ref() {
        let r = req("Fix typo.", Some(5), Some(1), vec!["ssot/contracts.md"]);
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }

    #[test]
    fn hk_fast_path_blocked_by_no_loc() {
        // estimated_loc unknown — we cannot decide HK without it.
        let r = req("Fix typo.", None, Some(1), vec![]);
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }

    #[test]
    fn op_fast_path_big_loc() {
        let r = req("Add a new module.", Some(500), Some(1), vec![]);
        let resp = fast_path(&r, PricingProvider::Anthropic).expect("op fast-path on LOC");
        assert_eq!(resp.tier, Tier::Op);
        assert_eq!(resp.complexity, Complexity::Complex);
    }

    #[test]
    fn op_fast_path_many_files() {
        let r = req("Refactor.", Some(100), Some(10), vec![]);
        let resp = fast_path(&r, PricingProvider::Anthropic).expect("op fast-path on files");
        assert_eq!(resp.tier, Tier::Op);
    }

    #[test]
    fn op_fast_path_two_markers() {
        let r = req(
            "Architecture migration with breaking auth changes.",
            Some(50),
            Some(2),
            vec![],
        );
        let resp = fast_path(&r, PricingProvider::Anthropic).expect("op fast-path on markers");
        assert_eq!(resp.tier, Tier::Op);
    }

    #[test]
    fn op_one_marker_alone_does_not_trigger() {
        // single marker is not enough for OP fast-path — defer to LLM.
        let r = req("Bug-fix to security module.", Some(50), Some(2), vec![]);
        // exactly one marker, mid-size: should fall through to LLM.
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }

    #[test]
    fn ambiguous_mid_size_returns_none() {
        let r = req("Add list-endpoint.", Some(150), Some(2), vec![]);
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }

    #[test]
    fn missing_estimates_with_clean_scope_returns_none() {
        let r = req("Some task.", None, None, vec![]);
        assert!(fast_path(&r, PricingProvider::Anthropic).is_none());
    }
}
