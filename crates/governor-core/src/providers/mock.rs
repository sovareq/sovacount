//! Deterministic, in-process mock provider.
//!
//! Used for tests and demos when no network access is desired. Mirrors the
//! prompt's decision-algorithm closely enough that downstream code paths
//! (cache, cost-estimation, response-parsing) can exercise realistic data.

use crate::error::Result;
use crate::providers::Provider;

use async_trait::async_trait;
use serde_json::{Value, json};

/// In-process classifier that picks a tier from light scope-keyword analysis.
///
/// Construction is a zero-allocation no-op. The provider does no I/O and is
/// safe to use in tests with `#[tokio::test]`.
#[derive(Debug, Default)]
pub struct MockProvider;

impl MockProvider {
    /// Construct a fresh mock provider.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn classify_raw(&self, _system_prompt: &str, user_payload: &str) -> Result<String> {
        // Parse the user payload (canonical JSON of ClassifyRequest fields)
        // and fish out the signals we care about. A real LLM would do
        // free-form reasoning here.
        let value: Value = serde_json::from_str(user_payload).unwrap_or(Value::Null);
        let scope = value
            .get("scope_md")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let loc = value
            .get("estimated_loc")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let files = value
            .get("estimated_files")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let ssot_refs = value
            .get("ssot_refs")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);

        let architectural_markers = [
            "architecture",
            "design",
            "rewrite",
            "fan-out",
            "ssot",
            "contract",
            "schema-change",
            "auth",
            "security",
            "migration",
            "breaking",
            "multi-module",
            "multi-domain",
        ]
        .iter()
        .filter(|m| scope.contains(*m))
        .count();

        // Tier decision (mirrors prompt logic in compressed form).
        let (tier, complexity, rationale, conf, in_tok, out_tok) =
            if loc > 300 || files > 5 || architectural_markers >= 2 || ssot_refs >= 2 {
                (
                    "op",
                    "complex",
                    "Mock: scope reads as multi-domain / large.",
                    90u32,
                    12000u32,
                    3500u32,
                )
            } else if loc < 50 && files <= 1 && architectural_markers == 0 && ssot_refs == 0 {
                (
                    "hk",
                    "trivial",
                    "Mock: tiny single-file scope, no architectural language.",
                    95,
                    1500,
                    400,
                )
            } else {
                (
                    "so",
                    "standard",
                    "Mock: medium implementation on a known pattern.",
                    80,
                    5000,
                    1200,
                )
            };

        let body = json!({
            "tier": tier,
            "complexity": complexity,
            "rationale": rationale,
            "confidence": conf,
            "estimated_input_tokens": in_tok,
            "estimated_output_tokens": out_tok,
            "alternative_tiers": [],
        });

        Ok(body.to_string())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_hk_for_tiny_scope() {
        let p = MockProvider::new();
        let user = json!({
            "task_id": "T-1",
            "scope_md": "Fix typo in README.",
            "ssot_refs": [],
            "estimated_loc": 5,
            "estimated_files": 1,
        })
        .to_string();
        let raw = p.classify_raw("sys", &user).await.unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["tier"], "hk");
    }

    #[tokio::test]
    async fn mock_returns_op_for_big_scope() {
        let p = MockProvider::new();
        let user = json!({
            "task_id": "T-2",
            "scope_md": "Bootstrap SSOT layer with fan-out across services.",
            "ssot_refs": ["ssot/contracts.md", "ssot/threat_model.md"],
            "estimated_loc": 800,
            "estimated_files": 12,
        })
        .to_string();
        let raw = p.classify_raw("sys", &user).await.unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["tier"], "op");
    }

    #[tokio::test]
    async fn mock_returns_so_for_mid_scope() {
        let p = MockProvider::new();
        let user = json!({
            "task_id": "T-3",
            "scope_md": "Add new list-endpoint.",
            "ssot_refs": [],
            "estimated_loc": 150,
            "estimated_files": 2,
        })
        .to_string();
        let raw = p.classify_raw("sys", &user).await.unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["tier"], "so");
    }

    #[tokio::test]
    async fn mock_is_deterministic() {
        let p = MockProvider::new();
        let user = json!({
            "task_id": "T-4",
            "scope_md": "Add new endpoint.",
            "ssot_refs": [],
            "estimated_loc": 150,
            "estimated_files": 2,
        })
        .to_string();
        let a = p.classify_raw("sys", &user).await.unwrap();
        let b = p.classify_raw("sys", &user).await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn mock_name() {
        assert_eq!(MockProvider::new().name(), "mock");
    }
}
