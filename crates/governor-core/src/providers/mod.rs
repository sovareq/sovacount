//! Provider abstraction. **Stub** — body filled in by the core-worker.

#![allow(dead_code)]

pub mod anthropic;
pub mod mock;
pub mod ollama;
pub mod openai;

use crate::config::Config;
use crate::error::Result;

use async_trait::async_trait;

/// One classifier-LLM call. Implementors transform the embedded
/// classifier prompt + the user request into a single chat completion.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Return raw JSON-string output produced by the classifier model.
    async fn classify_raw(&self, system_prompt: &str, user_payload: &str) -> Result<String>;

    /// A short, log-friendly identifier (e.g. `"anthropic"`).
    fn name(&self) -> &'static str;
}

/// Build the configured provider. Implemented by the core-worker.
pub fn build(_cfg: &Config) -> Result<Box<dyn Provider>> {
    unimplemented!("filled in by Worker A (core)")
}
