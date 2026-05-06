//! Public façade for the classifier engine.
//!
//! The [`Classifier`] orchestrates one classification request through three
//! layers (in order):
//!
//! 1. **Cache** — a keyed look-up against `<cache_dir>/<sha256>.json`. Skipped
//!    when the request sets `no_cache = true`.
//! 2. **Heuristic fast-path** — fires only on unambiguous extreme cases.
//! 3. **Provider** — a single chat-completion against the configured LLM,
//!    with the embedded system prompt + a JSON user payload.
//!
//! Cache writes are best-effort: if the disk is full or the directory is
//! unwritable, the request still returns the live answer; only a `warn!` is
//! logged.

use crate::cache::Cache;
use crate::config::Config;
use crate::error::{GovernorError, Result};
use crate::heuristic;
use crate::pricing::estimate_cost_usd;
use crate::prompt::DEFAULT_CLASSIFIER_PROMPT;
use crate::providers::{self, Provider};
use crate::types::{AlternativeTier, ClassifyRequest, ClassifyResponse, Complexity, Tier};

use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tracing::{Instrument, debug, info_span, warn};

/// The classifier orchestrates: cache lookup → heuristic fast-path → LLM call.
///
/// Cheap to clone (internally `Arc`-wrapped state).
#[derive(Clone)]
pub struct Classifier {
    pub(crate) config: Arc<Config>,
    pub(crate) provider: Arc<Box<dyn Provider>>,
    pub(crate) cache: Arc<Cache>,
    pub(crate) system_prompt: Arc<String>,
}

impl Classifier {
    /// Construct a classifier from a fully-resolved [`Config`].
    ///
    /// This:
    /// - Builds the configured provider (Anthropic/OpenAI/Ollama/Mock/Custom).
    /// - Opens the cache directory (creating it if missing).
    /// - Loads the optional `classifier_prompt_override` file, falling back to
    ///   the embedded [`DEFAULT_CLASSIFIER_PROMPT`].
    pub async fn new(config: Config) -> Result<Self> {
        let provider = providers::build(&config)?;
        let cache = Cache::new(
            config.cache_dir.clone(),
            Duration::from_secs(config.cache_ttl_secs),
        )?;

        let system_prompt = match &config.classifier_prompt_override {
            Some(path) => tokio::fs::read_to_string(path).await.map_err(|e| {
                GovernorError::Config(format!(
                    "read classifier-prompt-override {}: {e}",
                    path.display()
                ))
            })?,
            None => DEFAULT_CLASSIFIER_PROMPT.to_string(),
        };

        Ok(Self {
            config: Arc::new(config),
            provider: Arc::new(provider),
            cache: Arc::new(cache),
            system_prompt: Arc::new(system_prompt),
        })
    }

    /// Classify a single task.
    ///
    /// Resolution order:
    /// 1. Cache (skipped if `req.no_cache`)
    /// 2. Heuristic fast-path (only if scope is unambiguous)
    /// 3. LLM provider call
    pub async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse> {
        let span = info_span!("classify", task_id = %req.task_id);
        self.classify_inner(req).instrument(span).await
    }

    async fn classify_inner(&self, req: ClassifyRequest) -> Result<ClassifyResponse> {
        debug!(
            provider = self.provider.name(),
            classifier_model = %self.config.classifier_model,
            no_cache = req.no_cache,
            shift = req.shift,
            "classify start"
        );

        // 1. Cache lookup. Cache stays shift-agnostic so the same task is
        // only classified once regardless of how many shifts it goes through.
        if !req.no_cache
            && let Some(hit) = self.cache.get(&req).await?
        {
            debug!("cache hit");
            return Ok(self.apply_shift(hit, req.shift));
        }

        // 2. Heuristic fast-path. Cost-estimates use the *active* provider's
        //    rate-card, not a hardcoded one.
        let pricing = self.config.provider.pricing_provider();
        if let Some(mut resp) = heuristic::fast_path(&req, pricing) {
            resp.model_hint = self.config.tier_mapping.get(&resp.tier).cloned();
            self.cache_put_best_effort(&req, &resp).await;
            debug!(tier = %resp.tier, "fast-path hit");
            return Ok(self.apply_shift(resp, req.shift));
        }

        // 3. Live provider call.
        let user_payload = build_user_payload(&req);
        let raw = self
            .provider
            .classify_raw(&self.system_prompt, &user_payload)
            .await?;

        let mut resp = parse_classifier_output(&raw)?;
        resp.model_hint = self.config.tier_mapping.get(&resp.tier).cloned();
        resp.estimated_cost_usd = estimate_cost_usd(
            pricing,
            resp.tier,
            resp.estimated_input_tokens,
            resp.estimated_output_tokens,
        );
        resp.from_cache = false;

        self.cache_put_best_effort(&req, &resp).await;
        debug!(tier = %resp.tier, confidence = resp.confidence, "live classification");
        Ok(self.apply_shift(resp, req.shift))
    }

    /// Apply a user tier-shift on top of an already-classified response.
    /// Re-resolves `model_hint` and `estimated_cost_usd` against the new
    /// tier so the response stays internally consistent. `shift == 0` is a
    /// no-op fast-path.
    fn apply_shift(&self, mut resp: ClassifyResponse, shift: i32) -> ClassifyResponse {
        if shift == 0 {
            return resp;
        }
        let new_tier = resp.tier.shifted(shift);
        if new_tier == resp.tier {
            // Already at the clamp boundary; nothing to do.
            return resp;
        }
        resp.tier = new_tier;
        resp.model_hint = self.config.tier_mapping.get(&new_tier).cloned();
        resp.estimated_cost_usd = estimate_cost_usd(
            self.config.provider.pricing_provider(),
            new_tier,
            resp.estimated_input_tokens,
            resp.estimated_output_tokens,
        );
        resp
    }

    async fn cache_put_best_effort(&self, req: &ClassifyRequest, resp: &ClassifyResponse) {
        if let Err(e) = self.cache.put(req, resp).await {
            warn!(error = %e, "cache put failed; ignoring");
        }
    }

    /// Aggregate cached classifications into a [`CostReport`].
    ///
    /// Reads every entry under the configured cache directory and rolls up
    /// counts + cumulative `estimated_cost_usd` by tier and by UTC day. The
    /// always-Opus baseline / savings are computed against the *active*
    /// provider's rate-card. See [`crate::cost::aggregate`] for caveats
    /// (cached-only data, mtime-based day buckets).
    pub async fn cost_report(&self) -> Result<crate::cost::CostReport> {
        crate::cost::aggregate(&self.cache.dir, self.config.provider.pricing_provider()).await
    }
}

/// Construct the canonical user payload (JSON) we send to the provider.
fn build_user_payload(req: &ClassifyRequest) -> String {
    let v = json!({
        "task_id": req.task_id,
        "scope_md": req.scope_md,
        "ssot_refs": req.ssot_refs,
        "estimated_loc": req.estimated_loc,
        "estimated_files": req.estimated_files,
    });
    v.to_string()
}

/// Parse the inner classifier-output JSON.
///
/// Tolerates a leading/trailing markdown code-fence (some models still wrap
/// outputs even when asked not to).
fn parse_classifier_output(raw: &str) -> Result<ClassifyResponse> {
    let cleaned = strip_code_fence(raw.trim());

    #[derive(Deserialize)]
    struct ParsedAlt {
        tier: Tier,
        rationale: String,
        #[serde(default)]
        extra_cost_usd: f64,
    }

    #[derive(Deserialize)]
    struct Parsed {
        tier: Tier,
        complexity: Complexity,
        rationale: String,
        confidence: u8,
        estimated_input_tokens: u32,
        estimated_output_tokens: u32,
        #[serde(default)]
        alternative_tiers: Vec<ParsedAlt>,
    }

    // First parse to a generic Value so we can give a good error if it's not
    // even valid JSON.
    let _: Value = serde_json::from_str(cleaned)
        .map_err(|e| GovernorError::BadClassifierOutput(format!("not JSON: {e}; raw={cleaned}")))?;

    let parsed: Parsed = serde_json::from_str(cleaned).map_err(|e| {
        GovernorError::BadClassifierOutput(format!("schema mismatch: {e}; raw={cleaned}"))
    })?;

    let alternatives = parsed
        .alternative_tiers
        .into_iter()
        .map(|a| AlternativeTier {
            tier: a.tier,
            rationale: a.rationale,
            extra_cost_usd: a.extra_cost_usd,
        })
        .collect();

    Ok(ClassifyResponse {
        tier: parsed.tier,
        model_hint: None,
        complexity: parsed.complexity,
        rationale: parsed.rationale,
        confidence: parsed.confidence.min(100),
        estimated_input_tokens: parsed.estimated_input_tokens,
        estimated_output_tokens: parsed.estimated_output_tokens,
        // Filled in by caller.
        estimated_cost_usd: 0.0,
        alternative_tiers: alternatives,
        from_cache: false,
    })
}

/// Strip a single ```json …``` fence if present.
fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().strip_suffix("```").unwrap_or(rest).trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().strip_suffix("```").unwrap_or(rest).trim();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClassifyRequest, Tier};

    fn req(scope: &str, loc: Option<u32>, files: Option<u32>) -> ClassifyRequest {
        ClassifyRequest {
            task_id: "T".into(),
            scope_md: scope.into(),
            ssot_refs: vec![],
            estimated_loc: loc,
            estimated_files: files,
            no_cache: false,

            shift: 0,
        }
    }

    #[test]
    fn parse_well_formed() {
        let raw = r#"{
            "tier": "so",
            "complexity": "standard",
            "rationale": "ok",
            "confidence": 80,
            "estimated_input_tokens": 5000,
            "estimated_output_tokens": 1200,
            "alternative_tiers": []
        }"#;
        let r = parse_classifier_output(raw).unwrap();
        assert_eq!(r.tier, Tier::So);
        assert_eq!(r.confidence, 80);
    }

    #[test]
    fn parse_with_code_fence() {
        let raw = "```json\n{\"tier\":\"hk\",\"complexity\":\"trivial\",\"rationale\":\"r\",\"confidence\":99,\"estimated_input_tokens\":1500,\"estimated_output_tokens\":400}\n```";
        let r = parse_classifier_output(raw).unwrap();
        assert_eq!(r.tier, Tier::Hk);
    }

    #[test]
    fn parse_alternative_tiers() {
        let raw = r#"{
            "tier": "op",
            "complexity": "complex",
            "rationale": "r",
            "confidence": 90,
            "estimated_input_tokens": 12000,
            "estimated_output_tokens": 3500,
            "alternative_tiers": [
                {"tier": "so", "rationale": "if simpler", "extra_cost_usd": -0.3}
            ]
        }"#;
        let r = parse_classifier_output(raw).unwrap();
        assert_eq!(r.alternative_tiers.len(), 1);
        assert_eq!(r.alternative_tiers[0].tier, Tier::So);
    }

    #[test]
    fn parse_garbage_errors() {
        let err = parse_classifier_output("not json").unwrap_err();
        assert!(matches!(err, GovernorError::BadClassifierOutput(_)));
    }

    #[test]
    fn parse_schema_mismatch_errors() {
        let err = parse_classifier_output(r#"{"tier":"so"}"#).unwrap_err();
        assert!(matches!(err, GovernorError::BadClassifierOutput(_)));
    }

    #[test]
    fn parse_clamps_confidence_to_100() {
        let raw = r#"{
            "tier": "so",
            "complexity": "standard",
            "rationale": "r",
            "confidence": 200,
            "estimated_input_tokens": 1,
            "estimated_output_tokens": 1
        }"#;
        // serde_json refuses u8>255 but accepts <=255; we then clamp to 100.
        let r = parse_classifier_output(raw).unwrap();
        assert_eq!(r.confidence, 100);
    }

    #[tokio::test]
    async fn end_to_end_via_mock_provider() {
        let cfg = {
            let mut c = Config::mock();
            // Use a unique cache dir per test so we don't collide.
            c.cache_dir = tempfile::tempdir().unwrap().keep();
            c
        };
        let cls = Classifier::new(cfg).await.unwrap();

        // First: medium task → SO via mock fall-through.
        let r = req("Add a list endpoint with pagination.", Some(150), Some(2));
        let resp = cls.classify(r.clone()).await.unwrap();
        assert!(!resp.from_cache, "first call should not be cached");
        assert_eq!(resp.tier, Tier::So);
        assert!(resp.estimated_cost_usd > 0.0);
        assert_eq!(resp.model_hint.as_deref(), Some("mock-sonnet"));

        // Second identical call: cache hit.
        let resp2 = cls.classify(r).await.unwrap();
        assert!(resp2.from_cache);
        assert_eq!(resp2.tier, Tier::So);
    }

    #[tokio::test]
    async fn end_to_end_hk_via_fast_path() {
        let cfg = {
            let mut c = Config::mock();
            c.cache_dir = tempfile::tempdir().unwrap().keep();
            c
        };
        let cls = Classifier::new(cfg).await.unwrap();
        let r = req("Fix typo in README.", Some(5), Some(1));
        let resp = cls.classify(r).await.unwrap();
        assert_eq!(resp.tier, Tier::Hk);
        assert_eq!(resp.model_hint.as_deref(), Some("mock-haiku"));
        // Fast-path: confidence 92.
        assert!(resp.confidence >= 90);
    }

    #[tokio::test]
    async fn no_cache_flag_skips_cache() {
        let cfg = {
            let mut c = Config::mock();
            c.cache_dir = tempfile::tempdir().unwrap().keep();
            c
        };
        let cls = Classifier::new(cfg).await.unwrap();
        let mut r = req("Add a list endpoint.", Some(150), Some(2));
        let _ = cls.classify(r.clone()).await.unwrap();

        r.no_cache = true;
        let resp = cls.classify(r).await.unwrap();
        assert!(!resp.from_cache, "no_cache must bypass cache lookup");
    }
}
