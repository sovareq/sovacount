//! Library half of `governor-http` — pure routing/handlers, no startup.
//!
//! The binary entry-point ([`main.rs`](../src/main.rs)) loads config, builds a
//! real [`governor_core::Classifier`], and hands it to [`router`]. Tests
//! substitute a fake implementation of [`ClassifierLike`] so they can exercise
//! every route without going through the real classifier (whose stubs panic
//! during the parallel fan-out phase).

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use governor_core::{ClassifyRequest, ClassifyResponse, CostReport, GovernorError};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

/// Abstraction over the classifier — implemented by the real
/// [`governor_core::Classifier`] in production and by a stub in tests.
///
/// This trait lives in `governor-http` (not in `governor-core`) so the HTTP
/// crate can be tested without depending on the real classifier's stubs.
#[async_trait]
pub trait ClassifierLike: Send + Sync + 'static {
    /// Classify a single request. Mirrors [`governor_core::Classifier::classify`].
    async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse, GovernorError>;

    /// Aggregate cached classifications into a [`CostReport`]. Mirrors
    /// [`governor_core::Classifier::cost_report`].
    async fn cost_report(&self) -> Result<CostReport, GovernorError>;
}

#[async_trait]
impl ClassifierLike for governor_core::Classifier {
    async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse, GovernorError> {
        governor_core::Classifier::classify(self, req).await
    }

    async fn cost_report(&self) -> Result<CostReport, GovernorError> {
        governor_core::Classifier::cost_report(self).await
    }
}

/// Shared application state. Cheap to clone — wraps the classifier in an
/// `Arc` so handlers don't need to take ownership.
pub struct AppState<C: ClassifierLike> {
    classifier: Arc<C>,
    /// Optional Bearer-token API key. When `Some(...)` and non-empty,
    /// `/classify` requires `Authorization: Bearer <key>`.
    api_key: Option<Arc<String>>,
}

// Manual Clone — `#[derive(Clone)]` would synthesise `C: Clone`, but
// `Arc<C>` is `Clone` regardless of `C`, so we don't need the bound.
impl<C: ClassifierLike> Clone for AppState<C> {
    fn clone(&self) -> Self {
        Self {
            classifier: Arc::clone(&self.classifier),
            api_key: self.api_key.clone(),
        }
    }
}

impl<C: ClassifierLike> AppState<C> {
    /// Build a new state from a classifier and an optional API key.
    ///
    /// An empty-string `api_key` is normalised to `None` so the env-var "" case
    /// behaves the same as "unset".
    pub fn new(classifier: C, api_key: Option<String>) -> Self {
        let api_key = api_key.and_then(|k| {
            if k.is_empty() {
                None
            } else {
                Some(Arc::new(k))
            }
        });
        Self {
            classifier: Arc::new(classifier),
            api_key,
        }
    }

    /// `true` when Bearer-auth is required for `/classify`.
    pub fn auth_enabled(&self) -> bool {
        self.api_key.is_some()
    }
}

/// Build the axum [`Router`] with all routes wired to the given state.
///
/// Adds:
/// * [`TraceLayer`] for request logging (driven by `tracing-subscriber`),
/// * permissive CORS so browser-side agents work out of the box.
pub fn router<C: ClassifierLike>(state: AppState<C>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/classify", post(classify::<C>))
        .route("/cost", get(cost::<C>))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// `GET /health` — simple liveness probe. Always 200, never auth-gated.
async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// JSON error body returned by every error path.
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

/// `POST /classify` — main classification endpoint.
///
/// Order:
/// 1. Auth check (if `state.api_key` is set).
/// 2. JSON-body parse — bad bodies are rejected with 400 (axum's
///    [`JsonRejection`](axum::extract::rejection::JsonRejection) is mapped
///    here rather than letting axum return its default 422).
/// 3. Delegate to [`ClassifierLike::classify`]; map `Err` → 500.
async fn classify<C: ClassifierLike>(
    State(state): State<AppState<C>>,
    headers: HeaderMap,
    body: Result<Json<ClassifyRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    let Json(req) = match body {
        Ok(j) => j,
        Err(rej) => {
            return error_response(StatusCode::BAD_REQUEST, &rej.body_text());
        }
    };
    match state.classifier.classify(req).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Check the `Authorization: Bearer <key>` header against the configured key.
///
/// Returns `None` when auth is disabled or the header matches; otherwise
/// `Some(401-response)`. We use `Option` rather than `Result` because the
/// `Err` variant of an `axum::Response` triggers `clippy::result_large_err`.
fn check_auth<C: ClassifierLike>(state: &AppState<C>, headers: &HeaderMap) -> Option<Response> {
    let expected = state.api_key.as_deref()?;
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    match presented {
        Some(token) if token == expected.as_str() => None,
        _ => Some(error_response(StatusCode::UNAUTHORIZED, "unauthorized")),
    }
}

/// Build a JSON error response with the canonical `{"error": "..."}` shape.
fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            error: message.to_owned(),
        }),
    )
        .into_response()
}

/// `GET /cost` — aggregate cumulative spend from the on-disk cache.
///
/// Returns counts + summed `estimated_cost_usd` rolled up by tier and by
/// UTC day. Reflects only *cached* classifications — see
/// [`governor_core::cost`] for caveats. Auth-gated like `/classify` when
/// `state.api_key` is set.
async fn cost<C: ClassifierLike>(State(state): State<AppState<C>>, headers: HeaderMap) -> Response {
    if let Some(resp) = check_auth(&state, &headers) {
        return resp;
    }
    match state.classifier.cost_report().await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{self, Body};
    use axum::http::Request;
    use governor_core::{AlternativeTier, Complexity, Tier};
    use std::sync::Mutex;
    use tower::ServiceExt;

    /// Stub classifier — returns whatever `Result` was queued at construction.
    struct FakeClassifier {
        result: Mutex<Option<Result<ClassifyResponse, GovernorError>>>,
        cost: Mutex<Option<Result<CostReport, GovernorError>>>,
    }

    impl FakeClassifier {
        fn ok(resp: ClassifyResponse) -> Self {
            Self {
                result: Mutex::new(Some(Ok(resp))),
                cost: Mutex::new(None),
            }
        }
        fn err(e: GovernorError) -> Self {
            Self {
                result: Mutex::new(Some(Err(e))),
                cost: Mutex::new(None),
            }
        }
        fn with_cost(self, cost: Result<CostReport, GovernorError>) -> Self {
            *self.cost.lock().expect("poisoned") = Some(cost);
            self
        }
    }

    #[async_trait]
    impl ClassifierLike for FakeClassifier {
        async fn classify(&self, _req: ClassifyRequest) -> Result<ClassifyResponse, GovernorError> {
            self.result
                .lock()
                .expect("poisoned")
                .take()
                .expect("FakeClassifier::classify called more than once")
        }

        async fn cost_report(&self) -> Result<CostReport, GovernorError> {
            self.cost
                .lock()
                .expect("poisoned")
                .take()
                .unwrap_or_else(|| Ok(CostReport::default()))
        }
    }

    fn canned_response() -> ClassifyResponse {
        ClassifyResponse {
            tier: Tier::So,
            model_hint: Some("mock-sonnet".into()),
            complexity: Complexity::Standard,
            rationale: "standard impl".into(),
            confidence: 80,
            estimated_input_tokens: 1000,
            estimated_output_tokens: 500,
            estimated_cost_usd: 0.01,
            alternative_tiers: vec![AlternativeTier {
                tier: Tier::Hk,
                rationale: "too risky for haiku".into(),
                extra_cost_usd: -0.005,
            }],
            from_cache: false,
        }
    }

    fn valid_request_body() -> String {
        serde_json::to_string(&serde_json::json!({
            "task_id": "T-G-1",
            "scope_md": "Implement feature X",
        }))
        .unwrap()
    }

    fn make_app(
        result: Result<ClassifyResponse, GovernorError>,
        api_key: Option<String>,
    ) -> Router {
        let fake = match result {
            Ok(r) => FakeClassifier::ok(r),
            Err(e) => FakeClassifier::err(e),
        };
        router(AppState::new(fake, api_key))
    }

    async fn read_body(resp: Response) -> Vec<u8> {
        body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body")
            .to_vec()
    }

    #[tokio::test]
    async fn health_returns_200_with_status_ok() {
        let app = make_app(Ok(canned_response()), None);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn classify_valid_body_returns_200_and_parses_back() {
        let app = make_app(Ok(canned_response()), None);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = read_body(resp).await;
        let parsed: ClassifyResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.tier, Tier::So);
        assert_eq!(parsed.confidence, 80);
        assert_eq!(parsed.alternative_tiers.len(), 1);
    }

    #[tokio::test]
    async fn classify_malformed_json_returns_400() {
        let app = make_app(Ok(canned_response()), None);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.get("error").is_some(), "body had no `error` field: {v}");
    }

    #[tokio::test]
    async fn classify_classifier_error_returns_500_with_error_body() {
        let app = make_app(
            Err(GovernorError::ProviderRequest("upstream down".into())),
            None,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            v["error"].as_str().unwrap().contains("upstream down"),
            "expected `upstream down` in {v}",
        );
    }

    #[tokio::test]
    async fn auth_missing_header_returns_401() {
        let app = make_app(Ok(canned_response()), Some("secret-key".into()));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "unauthorized");
    }

    #[tokio::test]
    async fn auth_wrong_token_returns_401() {
        let app = make_app(Ok(canned_response()), Some("secret-key".into()));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer wrong")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_correct_token_returns_200() {
        let app = make_app(Ok(canned_response()), Some("secret-key".into()));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer secret-key")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_health_is_unauth_even_when_key_set() {
        let app = make_app(Ok(canned_response()), Some("secret-key".into()));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cost_returns_200_with_aggregation_shape() {
        use governor_core::{DayTotals, TierTotals};
        use std::collections::BTreeMap;

        let mut by_tier = BTreeMap::new();
        by_tier.insert(
            Tier::Hk,
            TierTotals {
                count: 3,
                total_usd: 0.06,
            },
        );
        let report = CostReport {
            by_tier,
            by_day: BTreeMap::new(),
            totals: DayTotals {
                count: 3,
                total_usd: 0.06,
            },
        };
        let fake = FakeClassifier::ok(canned_response()).with_cost(Ok(report));
        let app = router(AppState::new(fake, None));
        let resp = app
            .oneshot(Request::builder().uri("/cost").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["totals"]["count"], 3);
        assert_eq!(v["by_tier"]["hk"]["count"], 3);
    }

    #[tokio::test]
    async fn cost_propagates_classifier_error_as_500() {
        let fake = FakeClassifier::ok(canned_response())
            .with_cost(Err(GovernorError::Cache("boom".into())));
        let app = router(AppState::new(fake, None));
        let resp = app
            .oneshot(Request::builder().uri("/cost").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn cost_requires_auth_when_key_set() {
        let fake = FakeClassifier::ok(canned_response());
        let app = router(AppState::new(fake, Some("secret-key".into())));
        let resp = app
            .oneshot(Request::builder().uri("/cost").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_empty_env_string_disables_auth() {
        // An empty `GOVERNOR_HTTP_API_KEY` ("") must behave as unset.
        let app = make_app(Ok(canned_response()), Some(String::new()));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(valid_request_body()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
