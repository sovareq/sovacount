//! Anthropic Messages API provider.
//!
//! Calls `POST {base_url}/v1/messages` with the canonical `x-api-key` /
//! `anthropic-version` headers. Reads back the first text block from the
//! response. We do **not** depend on the `anthropic` SDK — only `reqwest` +
//! `serde_json`, to keep the runtime vendor-agnostic.

use crate::error::{GovernorError, Result};
use crate::providers::Provider;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use tracing::{debug, instrument};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

/// Anthropic provider — talks to `/v1/messages`.
#[derive(Debug)]
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl AnthropicProvider {
    /// Construct a new provider.
    ///
    /// `base_url` may be `None` to use the canonical `https://api.anthropic.com`.
    pub fn new(client: Client, api_key: String, base_url: Option<String>, model: String) -> Self {
        let base = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            client,
            api_key,
            base_url: base.trim_end_matches('/').to_string(),
            model,
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    #[instrument(skip(self, system_prompt, user_payload), fields(provider = "anthropic", model = %self.model))]
    async fn classify_raw(&self, system_prompt: &str, user_payload: &str) -> Result<String> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system_prompt,
            "messages": [{
                "role": "user",
                "content": user_payload,
            }],
        });

        debug!(url = %url, "anthropic classifier call");

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| GovernorError::ProviderRequest(format!("anthropic: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| GovernorError::ProviderResponse(format!("anthropic body read: {e}")))?;

        if !status.is_success() {
            return Err(GovernorError::ProviderRequest(format!(
                "anthropic HTTP {status}: {}",
                truncate_for_log(&text, 256)
            )));
        }

        let value: Value = serde_json::from_str(&text).map_err(|e| {
            GovernorError::ProviderResponse(format!(
                "anthropic non-JSON ({e}): {}",
                truncate_for_log(&text, 256)
            ))
        })?;

        // Extract content[0].text from {"content":[{"type":"text","text":"..."}]}
        let content = value
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                GovernorError::ProviderResponse("anthropic response missing `content` array".into())
            })?;

        let first = content.first().ok_or_else(|| {
            GovernorError::ProviderResponse("anthropic `content` array empty".into())
        })?;

        let out = first.get("text").and_then(Value::as_str).ok_or_else(|| {
            GovernorError::ProviderResponse(
                "anthropic content[0].text missing or not a string".into(),
            )
        })?;

        Ok(out.to_string())
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…(truncated)", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn happy_path_extracts_content_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "secret-key"))
            .and(header("anthropic-version", ANTHROPIC_VERSION))
            .and(header("content-type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_x",
                "content": [{"type": "text", "text": "{\"tier\":\"so\"}"}],
            })))
            .mount(&server)
            .await;

        let p = AnthropicProvider::new(
            Client::new(),
            "secret-key".into(),
            Some(server.uri()),
            "claude-opus-4-7".into(),
        );

        let raw = p
            .classify_raw("system", "{\"task_id\":\"T-1\"}")
            .await
            .unwrap();
        assert_eq!(raw, "{\"tier\":\"so\"}");
        assert_eq!(p.name(), "anthropic");
    }

    #[tokio::test]
    async fn http_error_surfaces_as_provider_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate-limited"))
            .mount(&server)
            .await;

        let p = AnthropicProvider::new(
            Client::new(),
            "k".into(),
            Some(server.uri()),
            "claude-opus-4-7".into(),
        );
        let err = p.classify_raw("sys", "{}").await.unwrap_err();
        assert!(
            matches!(err, GovernorError::ProviderRequest(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn missing_content_field_is_provider_response_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"id": "msg_x", "what": "?"})),
            )
            .mount(&server)
            .await;

        let p = AnthropicProvider::new(
            Client::new(),
            "k".into(),
            Some(server.uri()),
            "claude-opus-4-7".into(),
        );
        let err = p.classify_raw("sys", "{}").await.unwrap_err();
        assert!(
            matches!(err, GovernorError::ProviderResponse(_)),
            "got {err:?}"
        );
    }
}
