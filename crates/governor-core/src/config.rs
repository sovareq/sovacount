//! Runtime configuration — provider choice, model mapping, cache options.
//!
//! Configuration is layered:
//! 1. Environment variables (highest priority for the chosen provider).
//! 2. Optional user `mapping.toml` (overrides per-tier model ids only).
//! 3. Hard-coded per-provider defaults.
//!
//! See [`Config::from_env`] for the recognised env-vars.

use crate::error::{GovernorError, Result};
use crate::types::Tier;

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Which LLM provider runs the classifier itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    /// Anthropic Messages API (`/v1/messages`).
    Anthropic,
    /// OpenAI Chat Completions API (`/v1/chat/completions`).
    OpenAi,
    /// Local Ollama server (`/api/chat`).
    Ollama,
    /// Deterministic in-process mock — no I/O, used for tests/demos.
    Mock,
    /// Custom OpenAI-compatible endpoint at a user-supplied base URL.
    Custom,
}

impl ProviderKind {
    fn parse(raw: &str) -> Result<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "anthropic" => Ok(Self::Anthropic),
            "openai" | "open_ai" | "open-ai" => Ok(Self::OpenAi),
            "ollama" => Ok(Self::Ollama),
            "mock" => Ok(Self::Mock),
            "custom" => Ok(Self::Custom),
            other => Err(GovernorError::UnknownProvider(other.into())),
        }
    }

    /// Map this provider to its pricing-table key in
    /// [`crate::pricing::PricingProvider`].
    ///
    /// `Mock` and `Custom` both map to [`PricingProvider::Custom`] —
    /// the best-effort fallback rate-card. Anthropic / OpenAI / Ollama
    /// map one-to-one.
    pub fn pricing_provider(&self) -> crate::pricing::PricingProvider {
        match self {
            ProviderKind::Anthropic => crate::pricing::PricingProvider::Anthropic,
            ProviderKind::OpenAi => crate::pricing::PricingProvider::OpenAI,
            ProviderKind::Ollama => crate::pricing::PricingProvider::Ollama,
            // Mock has no real rate-card; route to Custom (best-effort,
            // user-overridable via pricing.toml).
            ProviderKind::Mock | ProviderKind::Custom => crate::pricing::PricingProvider::Custom,
        }
    }
}

/// Resolved configuration. Constructed via [`Config::from_env`] or builders.
#[derive(Debug, Clone)]
pub struct Config {
    /// Active classifier provider.
    pub provider: ProviderKind,

    /// API key (None for `Mock` and `Ollama`).
    pub api_key: Option<String>,

    /// Override base URL. None falls back to the provider's default.
    pub base_url: Option<String>,

    /// Model id used by the classifier itself (its own LLM call).
    pub classifier_model: String,

    /// Tier → concrete model id mapping for downstream agents.
    /// Keys: `Tier::Op`, `Tier::So`, `Tier::Hk`.
    pub tier_mapping: BTreeMap<Tier, String>,

    /// Cache directory. Default: `dirs::cache_dir().join("token-governor")`.
    pub cache_dir: PathBuf,

    /// Cache TTL in seconds. Default 30 days.
    pub cache_ttl_secs: u64,

    /// Path to user-overridable classifier prompt (if any).
    pub classifier_prompt_override: Option<PathBuf>,
}

/// TOML schema for `~/.config/token-governor/mapping.toml`.
#[derive(Debug, Default, Deserialize)]
struct MappingFile {
    #[serde(default)]
    mapping: MappingSection,
}

#[derive(Debug, Default, Deserialize)]
struct MappingSection {
    op: Option<String>,
    so: Option<String>,
    hk: Option<String>,
}

const DEFAULT_CACHE_TTL_SECS: u64 = 60 * 60 * 24 * 30; // 30 days

impl Config {
    /// Load configuration from environment variables and (optionally) a
    /// user `mapping.toml`.
    ///
    /// Recognised env-vars:
    /// * `GOVERNOR_PROVIDER` — one of `anthropic`, `openai`, `ollama`, `mock`,
    ///   `custom`. Defaults: `mock` if no `GOVERNOR_API_KEY`, else `anthropic`.
    /// * `GOVERNOR_API_KEY` — bearer-token / x-api-key for the chosen provider.
    /// * `GOVERNOR_BASE_URL` — override the provider's default endpoint.
    /// * `GOVERNOR_CLASSIFIER_MODEL` — override the model id used for the
    ///   classifier call itself. Required when `GOVERNOR_PROVIDER=custom`.
    /// * `GOVERNOR_MAPPING_FILE` — path to a TOML file overriding the default
    ///   tier-to-model mapping. Falls back to
    ///   `~/.config/token-governor/mapping.toml` if the file exists.
    /// * `GOVERNOR_CACHE_DIR` — override the response-cache directory.
    /// * `GOVERNOR_CACHE_TTL_DAYS` — override the cache TTL (in days).
    /// * `GOVERNOR_CLASSIFIER_PROMPT_FILE` — path to a custom system prompt.
    pub fn from_env() -> Result<Self> {
        // Snapshot the relevant env-vars into a HashMap and delegate to the
        // pure-fn variant. This keeps `from_env` thin and makes testing
        // possible without `unsafe { std::env::set_var(...) }`.
        let mut env: HashMap<&'static str, String> = HashMap::new();
        for k in [
            "GOVERNOR_PROVIDER",
            "GOVERNOR_API_KEY",
            "GOVERNOR_BASE_URL",
            "GOVERNOR_CLASSIFIER_MODEL",
            "GOVERNOR_MAPPING_FILE",
            "GOVERNOR_CACHE_DIR",
            "GOVERNOR_CACHE_TTL_DAYS",
            "GOVERNOR_CLASSIFIER_PROMPT_FILE",
        ] {
            if let Ok(v) = std::env::var(k)
                && !v.is_empty()
            {
                env.insert(k, v);
            }
        }
        Self::from_env_map(&env)
    }

    /// Pure-function variant of [`Config::from_env`] that reads directly from
    /// a string map. Used both by `from_env` and the test-suite (which must
    /// avoid `unsafe { std::env::set_var(...) }` under the crate-wide
    /// `#![forbid(unsafe_code)]` policy).
    pub(crate) fn from_env_map(env: &HashMap<&'static str, String>) -> Result<Self> {
        let api_key = env
            .get("GOVERNOR_API_KEY")
            .cloned()
            .filter(|s| !s.is_empty());

        let provider = match env.get("GOVERNOR_PROVIDER") {
            Some(s) if !s.is_empty() => ProviderKind::parse(s)?,
            _ => {
                if api_key.is_some() {
                    ProviderKind::Anthropic
                } else {
                    ProviderKind::Mock
                }
            }
        };

        let base_url = env
            .get("GOVERNOR_BASE_URL")
            .cloned()
            .filter(|s| !s.is_empty());

        let classifier_model = match env.get("GOVERNOR_CLASSIFIER_MODEL") {
            Some(s) if !s.is_empty() => s.clone(),
            _ => default_classifier_model(&provider).ok_or_else(|| {
                GovernorError::Config(
                    "GOVERNOR_CLASSIFIER_MODEL is required when \
                     GOVERNOR_PROVIDER=custom"
                        .into(),
                )
            })?,
        };

        let mut tier_mapping = default_tier_mapping(&provider);

        // Apply mapping.toml override (env-var path, then default user-config path).
        if let Some(path) = mapping_file_path_from(env) {
            apply_mapping_file(&path, &mut tier_mapping)?;
        }

        // For Custom provider with no env-mapping override, the default
        // tier_mapping is empty; fail fast if the user hasn't supplied one.
        if matches!(provider, ProviderKind::Custom)
            && (!tier_mapping.contains_key(&Tier::Op)
                || !tier_mapping.contains_key(&Tier::So)
                || !tier_mapping.contains_key(&Tier::Hk))
        {
            return Err(GovernorError::Config(
                "custom provider requires a mapping.toml with op/so/hk entries \
                 (or GOVERNOR_MAPPING_FILE pointing at one)"
                    .into(),
            ));
        }

        let cache_dir = match env.get("GOVERNOR_CACHE_DIR") {
            Some(s) if !s.is_empty() => PathBuf::from(s),
            _ => default_cache_dir(),
        };

        let cache_ttl_secs = match env.get("GOVERNOR_CACHE_TTL_DAYS") {
            Some(s) if !s.is_empty() => {
                let days: u64 = s.parse().map_err(|e| {
                    GovernorError::Config(format!("GOVERNOR_CACHE_TTL_DAYS not a number: {e}"))
                })?;
                days.saturating_mul(60 * 60 * 24)
            }
            _ => DEFAULT_CACHE_TTL_SECS,
        };

        let classifier_prompt_override = env
            .get("GOVERNOR_CLASSIFIER_PROMPT_FILE")
            .cloned()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        // Sanity: providers that need an API key must have one.
        if matches!(provider, ProviderKind::Anthropic | ProviderKind::OpenAi) && api_key.is_none() {
            return Err(GovernorError::Config(format!(
                "provider `{provider:?}` requires GOVERNOR_API_KEY"
            )));
        }

        Ok(Self {
            provider,
            api_key,
            base_url,
            classifier_model,
            tier_mapping,
            cache_dir,
            cache_ttl_secs,
            classifier_prompt_override,
        })
    }

    /// Quick factory for the deterministic mock provider — useful in tests
    /// and demos where no network access is desired.
    pub fn mock() -> Self {
        let mut tier_mapping = BTreeMap::new();
        tier_mapping.insert(Tier::Hk, "mock-haiku".into());
        tier_mapping.insert(Tier::So, "mock-sonnet".into());
        tier_mapping.insert(Tier::Op, "mock-opus".into());
        Self {
            provider: ProviderKind::Mock,
            api_key: None,
            base_url: None,
            classifier_model: "mock".into(),
            tier_mapping,
            cache_dir: std::env::temp_dir().join("token-governor-mock"),
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
            classifier_prompt_override: None,
        }
    }
}

fn default_classifier_model(p: &ProviderKind) -> Option<String> {
    Some(
        match p {
            ProviderKind::Anthropic => "claude-opus-4-7",
            ProviderKind::OpenAi => "o1",
            ProviderKind::Ollama => "deepseek-r1:70b",
            ProviderKind::Mock => "mock",
            ProviderKind::Custom => return None,
        }
        .into(),
    )
}

fn default_tier_mapping(p: &ProviderKind) -> BTreeMap<Tier, String> {
    let mut m = BTreeMap::new();
    match p {
        ProviderKind::Anthropic => {
            m.insert(Tier::Hk, "claude-haiku-4-5".into());
            m.insert(Tier::So, "claude-sonnet-4-6".into());
            m.insert(Tier::Op, "claude-opus-4-7".into());
        }
        ProviderKind::OpenAi => {
            m.insert(Tier::Hk, "gpt-4o-mini".into());
            m.insert(Tier::So, "gpt-4o".into());
            m.insert(Tier::Op, "o1".into());
        }
        ProviderKind::Ollama => {
            m.insert(Tier::Hk, "llama3.2:3b".into());
            m.insert(Tier::So, "llama3.3:70b".into());
            m.insert(Tier::Op, "deepseek-r1:70b".into());
        }
        ProviderKind::Mock => {
            m.insert(Tier::Hk, "mock-haiku".into());
            m.insert(Tier::So, "mock-sonnet".into());
            m.insert(Tier::Op, "mock-opus".into());
        }
        ProviderKind::Custom => {
            // Empty by design — Custom requires user-supplied mapping.toml.
        }
    }
    m
}

fn mapping_file_path_from(env: &HashMap<&'static str, String>) -> Option<PathBuf> {
    if let Some(p) = env.get("GOVERNOR_MAPPING_FILE")
        && !p.is_empty()
    {
        return Some(PathBuf::from(p));
    }
    let path = dirs::config_dir()?
        .join("token-governor")
        .join("mapping.toml");
    if path.exists() { Some(path) } else { None }
}

fn apply_mapping_file(path: &std::path::Path, mapping: &mut BTreeMap<Tier, String>) -> Result<()> {
    let body = std::fs::read_to_string(path)
        .map_err(|e| GovernorError::Config(format!("read mapping {}: {e}", path.display())))?;
    let parsed: MappingFile = toml::from_str(&body)
        .map_err(|e| GovernorError::Config(format!("parse mapping {}: {e}", path.display())))?;
    if let Some(s) = parsed.mapping.op {
        mapping.insert(Tier::Op, s);
    }
    if let Some(s) = parsed.mapping.so {
        mapping.insert(Tier::So, s);
    }
    if let Some(s) = parsed.mapping.hk {
        mapping.insert(Tier::Hk, s);
    }
    Ok(())
}

fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("token-governor")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn defaults_to_mock_when_no_key() {
        let cfg = Config::from_env_map(&env(&[])).unwrap();
        assert_eq!(cfg.provider, ProviderKind::Mock);
        assert_eq!(cfg.classifier_model, "mock");
        assert_eq!(cfg.tier_mapping[&Tier::Hk], "mock-haiku");
    }

    #[test]
    fn defaults_to_anthropic_when_key_present() {
        let cfg = Config::from_env_map(&env(&[("GOVERNOR_API_KEY", "sk-test")])).unwrap();
        assert_eq!(cfg.provider, ProviderKind::Anthropic);
        assert_eq!(cfg.classifier_model, "claude-opus-4-7");
        assert_eq!(cfg.tier_mapping[&Tier::Op], "claude-opus-4-7");
        assert_eq!(cfg.tier_mapping[&Tier::So], "claude-sonnet-4-6");
        assert_eq!(cfg.tier_mapping[&Tier::Hk], "claude-haiku-4-5");
    }

    #[test]
    fn explicit_openai_provider_uses_o1() {
        let cfg = Config::from_env_map(&env(&[
            ("GOVERNOR_PROVIDER", "openai"),
            ("GOVERNOR_API_KEY", "sk-openai"),
        ]))
        .unwrap();
        assert_eq!(cfg.provider, ProviderKind::OpenAi);
        assert_eq!(cfg.classifier_model, "o1");
        assert_eq!(cfg.tier_mapping[&Tier::Op], "o1");
        assert_eq!(cfg.tier_mapping[&Tier::Hk], "gpt-4o-mini");
    }

    #[test]
    fn ollama_provider_no_key_required() {
        let cfg = Config::from_env_map(&env(&[("GOVERNOR_PROVIDER", "ollama")])).unwrap();
        assert_eq!(cfg.provider, ProviderKind::Ollama);
        assert_eq!(cfg.classifier_model, "deepseek-r1:70b");
    }

    #[test]
    fn custom_provider_requires_classifier_model() {
        // No GOVERNOR_CLASSIFIER_MODEL → expect Config error.
        let result = Config::from_env_map(&env(&[("GOVERNOR_PROVIDER", "custom")]));
        match result {
            Err(GovernorError::Config(_)) => {}
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn anthropic_without_key_errors() {
        let result = Config::from_env_map(&env(&[("GOVERNOR_PROVIDER", "anthropic")]));
        match result {
            Err(GovernorError::Config(_)) => {}
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn cache_ttl_days_is_parsed() {
        let cfg = Config::from_env_map(&env(&[("GOVERNOR_CACHE_TTL_DAYS", "7")])).unwrap();
        assert_eq!(cfg.cache_ttl_secs, 7 * 24 * 60 * 60);
    }

    #[test]
    fn cache_ttl_days_garbage_errors() {
        let result = Config::from_env_map(&env(&[("GOVERNOR_CACHE_TTL_DAYS", "many")]));
        match result {
            Err(GovernorError::Config(_)) => {}
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_provider_errors() {
        let result = Config::from_env_map(&env(&[("GOVERNOR_PROVIDER", "bedrock")]));
        match result {
            Err(GovernorError::UnknownProvider(_)) => {}
            other => panic!("expected UnknownProvider, got {other:?}"),
        }
    }

    #[test]
    fn mapping_file_overrides_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let mapping = tmp.path().join("mapping.toml");
        std::fs::write(
            &mapping,
            r#"
[mapping]
op = "claude-opus-99"
so = "claude-sonnet-99"
hk = "claude-haiku-99"
"#,
        )
        .unwrap();
        let cfg = Config::from_env_map(&env(&[
            ("GOVERNOR_API_KEY", "sk"),
            ("GOVERNOR_MAPPING_FILE", mapping.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(cfg.tier_mapping[&Tier::Op], "claude-opus-99");
        assert_eq!(cfg.tier_mapping[&Tier::So], "claude-sonnet-99");
        assert_eq!(cfg.tier_mapping[&Tier::Hk], "claude-haiku-99");
    }

    #[test]
    fn mock_factory_is_self_consistent() {
        let cfg = Config::mock();
        assert_eq!(cfg.provider, ProviderKind::Mock);
        assert!(cfg.tier_mapping.contains_key(&Tier::Op));
        assert!(cfg.tier_mapping.contains_key(&Tier::So));
        assert!(cfg.tier_mapping.contains_key(&Tier::Hk));
    }

    #[test]
    fn custom_provider_with_classifier_and_mapping_works() {
        let tmp = tempfile::tempdir().unwrap();
        let mapping = tmp.path().join("mapping.toml");
        std::fs::write(
            &mapping,
            r#"
[mapping]
op = "x-op"
so = "x-so"
hk = "x-hk"
"#,
        )
        .unwrap();
        let cfg = Config::from_env_map(&env(&[
            ("GOVERNOR_PROVIDER", "custom"),
            ("GOVERNOR_BASE_URL", "https://example"),
            ("GOVERNOR_CLASSIFIER_MODEL", "x-classifier"),
            ("GOVERNOR_MAPPING_FILE", mapping.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(cfg.provider, ProviderKind::Custom);
        assert_eq!(cfg.tier_mapping[&Tier::Op], "x-op");
    }
}
