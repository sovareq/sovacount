//! Ollama (local LLM server) provider.
//!
//! Calls `POST {base_url}/api/chat` with `stream:false` and `format:"json"` so
//! we get a single non-streamed JSON response back.

use crate::error::{GovernorError, Result};
use crate::providers::Provider;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use tracing::{debug, instrument};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Ollama provider — talks to `/api/chat`.
#[derive(Debug)]
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    /// Construct a new Ollama provider. `base_url` defaults to
    /// `http://localhost:11434`.
    pub fn new(client: Client, base_url: Option<String>, model: String) -> Self {
        let base = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            client,
            base_url: base.trim_end_matches('/').to_string(),
            model,
        }
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    #[instrument(skip(self, system_prompt, user_payload), fields(provider = "ollama", model = %self.model))]
    async fn classify_raw(&self, system_prompt: &str, user_payload: &str) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_payload},
            ],
            "stream": false,
            "format": "json",
        });

        debug!(url = %url, "ollama classifier call");

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| GovernorError::ProviderRequest(format!("ollama: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| GovernorError::ProviderResponse(format!("ollama body read: {e}")))?;

        if !status.is_success() {
            return Err(GovernorError::ProviderRequest(format!(
                "ollama HTTP {status}: {}",
                truncate_for_log(&text, 256)
            )));
        }

        let value: Value = serde_json::from_str(&text).map_err(|e| {
            GovernorError::ProviderResponse(format!(
                "ollama non-JSON ({e}): {}",
                truncate_for_log(&text, 256)
            ))
        })?;

        let content = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GovernorError::ProviderResponse("ollama response missing message.content".into())
            })?;

        Ok(content.to_string())
    }

    fn name(&self) -> &'static str {
        "ollama"
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn happy_path_extracts_message_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role": "assistant", "content": "{\"tier\":\"so\"}"},
                "done": true
            })))
            .mount(&server)
            .await;

        let p = OllamaProvider::new(Client::new(), Some(server.uri()), "deepseek-r1:70b".into());
        let raw = p.classify_raw("sys", "{\"task_id\":\"T\"}").await.unwrap();
        assert_eq!(raw, "{\"tier\":\"so\"}");
        assert_eq!(p.name(), "ollama");
    }

    #[tokio::test]
    async fn missing_message_content_is_provider_response_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"done": true})))
            .mount(&server)
            .await;
        let p = OllamaProvider::new(Client::new(), Some(server.uri()), "x".into());
        let err = p.classify_raw("sys", "{}").await.unwrap_err();
        assert!(
            matches!(err, GovernorError::ProviderResponse(_)),
            "got {err:?}"
        );
    }
}
