//! Handler-level tests for the `governor_classify` MCP tool.
//!
//! Strategy: build a [`GovernorServer`] over a [`MockClassifier`] that
//! returns canned [`ClassifyResponse`]s, drive the handler in-process, and
//! assert on its [`CallToolResult`]. We deliberately do **not** spawn the
//! binary or speak real MCP framing here — that would make the test
//! brittle against rmcp internals. The schema test inspects the tool
//! attribute the `#[tool]` macro emits at compile time.

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use governor_core::{ClassifyRequest, ClassifyResponse, Complexity, GovernorError, Result, Tier};
use governor_mcp::{ClassifierLike, ClassifyParams, GovernorServer};

/// Fake classifier returning a fixed response. Records the last request so
/// the test can assert that the handler forwarded params correctly.
#[derive(Default)]
struct MockClassifier {
    fixed: Option<ClassifyResponse>,
    fail_with: Option<&'static str>,
    last_request: tokio::sync::Mutex<Option<ClassifyRequest>>,
}

impl MockClassifier {
    fn with_response(resp: ClassifyResponse) -> Self {
        Self {
            fixed: Some(resp),
            fail_with: None,
            last_request: tokio::sync::Mutex::new(None),
        }
    }

    fn failing(message: &'static str) -> Self {
        Self {
            fixed: None,
            fail_with: Some(message),
            last_request: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl ClassifierLike for MockClassifier {
    async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse> {
        *self.last_request.lock().await = Some(req);
        if let Some(msg) = self.fail_with {
            return Err(GovernorError::Config(msg.into()));
        }
        Ok(self
            .fixed
            .clone()
            .expect("mock has neither response nor failure"))
    }
}

fn canned_response() -> ClassifyResponse {
    ClassifyResponse {
        tier: Tier::So,
        model_hint: Some("claude-sonnet-4-6".into()),
        complexity: Complexity::Standard,
        rationale: "Routine implementation, single file.".into(),
        confidence: 88,
        estimated_input_tokens: 1200,
        estimated_output_tokens: 400,
        estimated_cost_usd: 0.012,
        alternative_tiers: vec![],
        from_cache: false,
    }
}

fn sample_params() -> ClassifyParams {
    ClassifyParams {
        task_id: "T-G-1".into(),
        scope_md: "Implement governor-mcp stdio server.".into(),
        ssot_refs: vec!["docs/SSOT.md".into()],
        estimated_loc: Some(220),
        estimated_files: Some(3),
        no_cache: false,

        shift: 0,
    }
}

#[tokio::test]
async fn handler_returns_structured_classification() {
    let classifier = Arc::new(MockClassifier::with_response(canned_response()));
    let server = GovernorServer::new(classifier.clone());

    let result = server
        .handle_classify(sample_params())
        .await
        .expect("handler should succeed");

    // The handler must populate structured_content with the JSON form of
    // ClassifyResponse, and is_error must be false.
    assert_eq!(result.is_error, Some(false));
    let structured = result
        .structured_content
        .as_ref()
        .expect("structured_content must be present");
    assert_eq!(structured["tier"], "so");
    assert_eq!(structured["model_hint"], "claude-sonnet-4-6");
    assert_eq!(structured["complexity"], "standard");
    assert_eq!(structured["confidence"], 88);
    assert_eq!(structured["from_cache"], false);

    // The text content is auto-populated by CallToolResult::structured so
    // older clients still see something usable.
    assert_eq!(result.content.len(), 1);

    // And the request must have been forwarded with all fields intact.
    let last = classifier.last_request.lock().await;
    let req = last
        .as_ref()
        .expect("handler must call classifier exactly once");
    assert_eq!(req.task_id, "T-G-1");
    assert_eq!(req.scope_md, "Implement governor-mcp stdio server.");
    assert_eq!(req.ssot_refs, vec!["docs/SSOT.md".to_string()]);
    assert_eq!(req.estimated_loc, Some(220));
    assert_eq!(req.estimated_files, Some(3));
    assert!(!req.no_cache);
}

#[tokio::test]
async fn handler_propagates_classifier_errors_as_internal_error() {
    let classifier = Arc::new(MockClassifier::failing("boom"));
    let server = GovernorServer::new(classifier);

    let err = server
        .handle_classify(sample_params())
        .await
        .expect_err("handler must surface classifier failure");

    let msg = err.message.as_ref();
    assert!(
        msg.contains("classifier error"),
        "unexpected message: {msg}"
    );
    assert!(
        msg.contains("boom"),
        "expected underlying message in: {msg}"
    );
}

/// The `#[tool]` macro emits `<TypeName>::<method>_tool_attr()` returning a
/// `rmcp::model::Tool`. Inspecting its `input_schema` is the cleanest way to
/// assert the wire-shape advertised on `tools/list`.
#[test]
fn input_schema_has_required_and_optional_fields() {
    type Server = GovernorServer<MockClassifier>;

    let attr = Server::governor_classify_tool_attr();
    assert_eq!(attr.name, "governor_classify");
    assert!(
        attr.description
            .as_ref()
            .map(|d| d.contains("Classify"))
            .unwrap_or(false),
        "tool description should mention classification"
    );

    let schema = attr.input_schema.as_ref();
    assert_eq!(
        schema.get("type").and_then(|v| v.as_str()),
        Some("object"),
        "schema must be an object"
    );

    let required: Vec<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        required.contains(&"task_id".to_string()),
        "task_id must be required"
    );
    assert!(
        required.contains(&"scope_md".to_string()),
        "scope_md must be required"
    );
    assert!(
        !required.contains(&"ssot_refs".to_string()),
        "ssot_refs must be optional"
    );
    assert!(
        !required.contains(&"estimated_loc".to_string()),
        "estimated_loc must be optional"
    );
    assert!(
        !required.contains(&"estimated_files".to_string()),
        "estimated_files must be optional"
    );
    assert!(
        !required.contains(&"no_cache".to_string()),
        "no_cache must be optional"
    );

    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .expect("schema must have properties");
    for key in [
        "task_id",
        "scope_md",
        "ssot_refs",
        "estimated_loc",
        "estimated_files",
        "no_cache",
    ] {
        assert!(
            properties.contains_key(key),
            "schema is missing field {key}"
        );
    }
}
