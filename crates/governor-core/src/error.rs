//! Error type used across the `governor-core` library.
//!
//! All public APIs return [`Result<T>`], the alias for
//! `std::result::Result<T, GovernorError>`. The variants are intentionally
//! coarse-grained — they map to log lines and end-user CLI messages rather
//! than to internal control-flow.

use thiserror::Error;

/// Top-level error type for the classifier engine.
///
/// `From` impls auto-promote standard I/O, JSON, and HTTP failures so call
/// sites can use `?` without explicit conversion.
#[derive(Debug, Error)]
pub enum GovernorError {
    /// Configuration is structurally invalid (bad env-var, malformed
    /// `mapping.toml`, missing required key, …).
    #[error("invalid configuration: {0}")]
    Config(String),

    /// `GOVERNOR_PROVIDER` named a backend the engine doesn't recognise.
    #[error("provider `{0}` is not supported")]
    UnknownProvider(String),

    /// HTTP-level failure when talking to the upstream classifier model.
    #[error("provider request failed: {0}")]
    ProviderRequest(String),

    /// Provider returned non-2xx, non-JSON, or otherwise unexpected payload.
    #[error("provider response could not be parsed: {0}")]
    ProviderResponse(String),

    /// The LLM returned syntactically broken JSON or violated the agreed
    /// classifier output schema.
    #[error("classifier returned malformed JSON: {0}")]
    BadClassifierOutput(String),

    /// File-cache I/O could not complete (permission denied, disk full, …).
    #[error("cache I/O error: {0}")]
    Cache(String),

    /// Generic I/O error — auto-promoted from `std::io::Error`.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialisation/deserialisation error — auto-promoted from
    /// `serde_json::Error`.
    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),

    /// HTTP-client error — auto-promoted from `reqwest::Error`.
    #[error("HTTP-client error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, GovernorError>;
