//! Runtime configuration — provider choice, model mapping, cache options.
//!
//! **Stub** — body filled in by the core-worker. Public surface fixed.

use crate::error::Result;
use crate::types::Tier;

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Which LLM provider runs the classifier itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Ollama,
    Mock,
    /// Custom OpenAI-compatible endpoint at a user-supplied base URL.
    Custom,
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

impl Config {
    /// Load configuration from environment variables and (optionally) a
    /// user `mapping.toml`.
    ///
    /// Recognised env-vars:
    /// * `GOVERNOR_PROVIDER` (default `mock` if no key present, else `anthropic`)
    /// * `GOVERNOR_API_KEY`
    /// * `GOVERNOR_BASE_URL`
    /// * `GOVERNOR_CLASSIFIER_MODEL`
    /// * `GOVERNOR_MAPPING_FILE`
    /// * `GOVERNOR_CACHE_DIR`
    /// * `GOVERNOR_CACHE_TTL_DAYS`
    pub fn from_env() -> Result<Self> {
        unimplemented!("filled in by Worker A (core)")
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
            cache_ttl_secs: 60 * 60 * 24 * 30,
            classifier_prompt_override: None,
        }
    }
}
