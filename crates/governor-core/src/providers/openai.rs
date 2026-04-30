//! OpenAI Chat Completions provider.
//!
//! Calls `POST {base_url}/v1/chat/completions` with `Authorization: Bearer …`.
//! Asks for `response_format = json_object` so we get back well-formed JSON.

use crate::error::{GovernorError, Result};
use crate::providers::Provider;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use tracing::{debug, instrument};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// OpenAI provider — talks to `/v1/chat/completions`.
#[derive(Debug)]
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    /// When `true`, this provider was constructed via `Custom` config and the
    /// `name()` we return changes to `"custom"` for clearer logs.
    is_custom: bool,
}

impl OpenAiProvider {
    /// Construct a new OpenAI-compatible provider.
    pub fn new(client: Client, api_key: String, base_url: Option<String>, model: String) -> Self {
        let base = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            client,
            api_key,
            base_url: base.trim_end_matches('/').to_string(),
            model,
            is_custom: false,
        }
    }

    /// Mark this instance as a "custom" OpenAI-compatible endpoint (changes
    /// the value returned by `name()`).
    pub fn into_custom(mut self) -> Self {
        self.is_custom = true;
        self
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    #[instrument(skip(self, system_prompt, user_payload), fields(provider = "openai", model = %self.model))]
    async fn classify_raw(&self, system_prompt: &str, user_payload: &str) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_payload},
            ],
            "response_format": {"type": "json_object"},
        });

        debug!(url = %url, "openai classifier call");

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| GovernorError::ProviderRequest(format!("openai: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| GovernorError::ProviderResponse(format!("openai body read: {e}")))?;

        if !status.is_success() {
            return Err(GovernorError::ProviderRequest(format!(
                "openai HTTP {status}: {}",
                truncate_for_log(&text, 256)
            )));
        }

        let value: Value = serde_json::from_str(&text).map_err(|e| {
            GovernorError::ProviderResponse(format!(
                "openai non-JSON ({e}): {}",
                truncate_for_log(&text, 256)
            ))
        })?;

        let content = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GovernorError::ProviderResponse(
                    "openai response missing choices[0].message.content".into(),
                )
            })?;

        Ok(content.to_string())
    }

    fn name(&self) -> &'static str {
        if self.is_custom { "custom" } else { "openai" }
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
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn happy_path_extracts_choices_message_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer secret-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {"role": "assistant", "content": "{\"tier\":\"hk\"}"}
                }]
            })))
            .mount(&server)
            .await;

        let p = OpenAiProvider::new(
            Client::new(),
            "secret-key".into(),
            Some(server.uri()),
            "o1".into(),
        );
        let raw = p
            .classify_raw("sys", "{\"task_id\":\"T-1\"}")
            .await
            .unwrap();
        assert_eq!(raw, "{\"tier\":\"hk\"}");
        assert_eq!(p.name(), "openai");
    }

    #[tokio::test]
    async fn into_custom_changes_name() {
        let p = OpenAiProvider::new(
            Client::new(),
            "k".into(),
            Some("http://example".into()),
            "x".into(),
        )
        .into_custom();
        assert_eq!(p.name(), "custom");
    }

    #[tokio::test]
    async fn missing_content_is_provider_response_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices": []})))
            .mount(&server)
            .await;
        let p = OpenAiProvider::new(Client::new(), "k".into(), Some(server.uri()), "o1".into());
        let err = p.classify_raw("sys", "{}").await.unwrap_err();
        assert!(
            matches!(err, GovernorError::ProviderResponse(_)),
            "got {err:?}"
        );
    }
}
