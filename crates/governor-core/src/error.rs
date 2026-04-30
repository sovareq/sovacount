//! Error type used across the `governor-core` library.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GovernorError {
    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("provider `{0}` is not supported")]
    UnknownProvider(String),

    #[error("provider request failed: {0}")]
    ProviderRequest(String),

    #[error("provider response could not be parsed: {0}")]
    ProviderResponse(String),

    #[error("classifier returned malformed JSON: {0}")]
    BadClassifierOutput(String),

    #[error("cache I/O error: {0}")]
    Cache(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("HTTP-client error: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, GovernorError>;
