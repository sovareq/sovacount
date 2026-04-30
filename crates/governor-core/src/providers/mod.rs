//! Provider abstraction.
//!
//! Each backend (Anthropic / OpenAI / Ollama / Mock / Custom) implements a
//! single tiny [`Provider`] trait method that turns a system+user pair into
//! the raw JSON string that the classifier model emitted. Higher-level
//! parsing lives in `crate::classifier`.

pub mod anthropic;
pub mod mock;
pub mod ollama;
pub mod openai;

use crate::config::{Config, ProviderKind};
use crate::error::{GovernorError, Result};

use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

/// One classifier-LLM call. Implementors transform the embedded
/// classifier prompt + the user request into a single chat completion.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Return raw JSON-string output produced by the classifier model.
    async fn classify_raw(&self, system_prompt: &str, user_payload: &str) -> Result<String>;

    /// A short, log-friendly identifier (e.g. `"anthropic"`).
    fn name(&self) -> &'static str;
}

/// Build the configured provider.
///
/// For `Custom`, this delegates to the OpenAI-compatible implementation but
/// reports `name() == "custom"`.
pub fn build(cfg: &Config) -> Result<Box<dyn Provider>> {
    match cfg.provider {
        ProviderKind::Mock => Ok(Box::new(mock::MockProvider::new())),
        ProviderKind::Anthropic => {
            let key = cfg.api_key.clone().ok_or_else(|| {
                GovernorError::Config("anthropic provider requires GOVERNOR_API_KEY".into())
            })?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                build_client()?,
                key,
                cfg.base_url.clone(),
                cfg.classifier_model.clone(),
            )))
        }
        ProviderKind::OpenAi => {
            let key = cfg.api_key.clone().ok_or_else(|| {
                GovernorError::Config("openai provider requires GOVERNOR_API_KEY".into())
            })?;
            Ok(Box::new(openai::OpenAiProvider::new(
                build_client()?,
                key,
                cfg.base_url.clone(),
                cfg.classifier_model.clone(),
            )))
        }
        ProviderKind::Ollama => Ok(Box::new(ollama::OllamaProvider::new(
            build_client()?,
            cfg.base_url.clone(),
            cfg.classifier_model.clone(),
        ))),
        ProviderKind::Custom => {
            // Custom is OpenAI-compatible at a user-supplied base URL.
            let key = cfg.api_key.clone().unwrap_or_default();
            let base = cfg.base_url.clone().ok_or_else(|| {
                GovernorError::Config("custom provider requires GOVERNOR_BASE_URL".into())
            })?;
            Ok(Box::new(
                openai::OpenAiProvider::new(
                    build_client()?,
                    key,
                    Some(base),
                    cfg.classifier_model.clone(),
                )
                .into_custom(),
            ))
        }
    }
}

/// Construct the shared HTTP client used by all real providers.
fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!("token-governor/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| GovernorError::Config(format!("reqwest client build failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn build_mock_works() {
        let cfg = Config::mock();
        let p = build(&cfg).unwrap();
        assert_eq!(p.name(), "mock");
    }

    #[test]
    fn build_anthropic_requires_key() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::Anthropic;
        cfg.api_key = None;
        match build(&cfg) {
            Err(GovernorError::Config(_)) => {}
            Err(other) => panic!("expected Config error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn build_anthropic_with_key() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::Anthropic;
        cfg.api_key = Some("k".into());
        let p = build(&cfg).unwrap();
        assert_eq!(p.name(), "anthropic");
    }

    #[test]
    fn build_openai_with_key() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::OpenAi;
        cfg.api_key = Some("k".into());
        let p = build(&cfg).unwrap();
        assert_eq!(p.name(), "openai");
    }

    #[test]
    fn build_ollama_no_key() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::Ollama;
        cfg.api_key = None;
        let p = build(&cfg).unwrap();
        assert_eq!(p.name(), "ollama");
    }

    #[test]
    fn build_custom_requires_base_url() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::Custom;
        cfg.base_url = None;
        match build(&cfg) {
            Err(GovernorError::Config(_)) => {}
            Err(other) => panic!("expected Config error, got {other:?}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }

    #[test]
    fn build_custom_ok() {
        let mut cfg = Config::mock();
        cfg.provider = ProviderKind::Custom;
        cfg.base_url = Some("https://example.com".into());
        cfg.api_key = Some("k".into());
        let p = build(&cfg).unwrap();
        assert_eq!(p.name(), "custom");
    }
}
