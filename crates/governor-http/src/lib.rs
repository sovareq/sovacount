//! Library half of `governor-http` — pure routing/handlers, no startup.
//!
//! The binary entry-point ([`main.rs`](../src/main.rs)) loads config, builds a
//! real [`governor_core::Classifier`], and hands it to [`router`]. Tests
//! substitute a fake implementation of [`ClassifierLike`] so they can exercise
//! every route without going through the real classifier (whose stubs panic
//! during the parallel fan-out phase).

#![forbid(unsafe_code)]

use std::path::PathBuf;
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
use tokio::sync::{mpsc, oneshot};
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

/// One enqueued classify-opdracht. De worker stuurt het resultaat terug via
/// `reply` zodra `Classifier::classify` klaar is. `pub` zodat `main.rs` (en
/// integration-tests) een `mpsc::channel::<ClassifyJob>(...)` kunnen openen.
pub struct ClassifyJob {
    /// Inkomende request van de HTTP-handler.
    pub req: ClassifyRequest,
    /// Eénmalig kanaal terug naar de handler met het resultaat. `oneshot`
    /// omdat elke job exact één antwoord krijgt.
    pub reply: oneshot::Sender<Result<ClassifyResponse, GovernorError>>,
}

/// Shared application state. Cheap to clone — wraps the classifier in an
/// `Arc` so handlers don't need to take ownership.
pub struct AppState<C: ClassifierLike> {
    classifier: Arc<C>,
    /// Optional Bearer-token API key. When `Some(...)` and non-empty,
    /// `/classify` requires `Authorization: Bearer <key>`.
    api_key: Option<Arc<String>>,
    /// Path to the persistent gear-lever (`/shift`) value-file. Default is
    /// `$HOME/.config/token-governor/shift`. Tests inject a tempdir.
    shift_path: Arc<PathBuf>,
    /// Zender naar de classify-worker. `None` betekent: directe path
    /// (gebruikt door tests en als fallback). `Some(tx)` betekent:
    /// non-blocking enqueue via een bounded mpsc-channel.
    classify_tx: Option<mpsc::Sender<ClassifyJob>>,
}

// Manual Clone — `#[derive(Clone)]` would synthesise `C: Clone`, but
// `Arc<C>` is `Clone` regardless of `C`, so we don't need the bound.
impl<C: ClassifierLike> Clone for AppState<C> {
    fn clone(&self) -> Self {
        Self {
            classifier: Arc::clone(&self.classifier),
            api_key: self.api_key.clone(),
            shift_path: Arc::clone(&self.shift_path),
            classify_tx: self.classify_tx.clone(),
        }
    }
}

impl<C: ClassifierLike> AppState<C> {
    /// Build a new state from a classifier and an optional API key.
    ///
    /// An empty-string `api_key` is normalised to `None` so the env-var "" case
    /// behaves the same as "unset". The shift-config path defaults to
    /// `$HOME/.config/token-governor/shift`.
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
            shift_path: Arc::new(default_shift_path()),
            classify_tx: None,
        }
    }

    /// Override the shift-config path (used by tests to isolate the
    /// filesystem state without touching the real user's `$HOME`).
    pub fn with_shift_path(mut self, path: PathBuf) -> Self {
        self.shift_path = Arc::new(path);
        self
    }

    /// Attach a classify-worker queue. When set, `/classify` enqueues each
    /// request via `try_send` (non-blocking, bounded backpressure) and waits
    /// for the worker's reply through a `oneshot`. When `None`, the handler
    /// falls back to calling the classifier directly (used by all 148
    /// existing tests).
    pub fn with_queue(mut self, tx: mpsc::Sender<ClassifyJob>) -> Self {
        self.classify_tx = Some(tx);
        self
    }

    /// Clone the inner classifier `Arc` so a worker task can share ownership
    /// without re-creating the classifier. Used by `main.rs` to hand the
    /// classifier to the spawned worker before passing the state to the
    /// router.
    pub fn classifier_arc(&self) -> Arc<C> {
        Arc::clone(&self.classifier)
    }

    /// `true` when Bearer-auth is required for `/classify`.
    pub fn auth_enabled(&self) -> bool {
        self.api_key.is_some()
    }
}

/// Default on-disk location for the persistent gear-lever value.
///
/// Cross-platform: `dirs::home_dir()` resolves `$HOME` on Unix and
/// `%USERPROFILE%` on Windows. Falls back to the current directory only
/// when neither is set (CI containers, sandboxed environments).
fn default_shift_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("token-governor").join("shift")
}

/// Build the axum [`Router`] with all routes wired to the given state.
///
/// Adds:
/// * [`TraceLayer`] for request logging (driven by `tracing-subscriber`),
/// * permissive CORS so browser-side agents work out of the box.
pub fn router<C: ClassifierLike>(state: AppState<C>) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/health", get(health))
        .route("/classify", post(classify::<C>))
        .route("/cost", get(cost::<C>))
        .route("/shift", get(shift_get::<C>).post(shift_post::<C>))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// `GET /` — embedded HTML dashboard. Pulls live data from `/cost`, `/shift`
/// and `/health` via fetch on the client side. Single-file, no external CDN
/// dependencies — works fully offline.
async fn dashboard() -> Response {
    let html = include_str!("dashboard.html");
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct ShiftBody {
    value: i32,
}

/// `GET /shift` — read the persistent gear-lever value. `0` if not set.
async fn shift_get<C: ClassifierLike>(State(state): State<AppState<C>>) -> Response {
    let value = std::fs::read_to_string(state.shift_path.as_ref())
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(0);
    let clamped = value.clamp(-2, 2);
    (StatusCode::OK, Json(ShiftBody { value: clamped })).into_response()
}

/// `POST /shift` — set the persistent gear-lever value. Body: `{"value": -1|0|1}`.
/// Out-of-range values clamp to `-2..=2` (one step beyond the tier extremes
/// so users can express "always-Hk" or "always-Op" intent without a
/// per-tier check).
async fn shift_post<C: ClassifierLike>(
    State(state): State<AppState<C>>,
    body: Json<ShiftBody>,
) -> Response {
    let value = body.value.clamp(-2, 2);
    let path = state.shift_path.as_ref();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("create shift-config dir: {e}"),
        );
    }
    if let Err(e) = std::fs::write(path, format!("{value}\n")) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("write shift-config: {e}"),
        );
    }
    (StatusCode::OK, Json(ShiftBody { value })).into_response()
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
/// 3a. If a `classify_tx` worker-channel is attached: enqueue via
///    [`mpsc::Sender::try_send`] and `await` the worker's reply through a
///    [`oneshot`] channel. Bounded backpressure: a full queue returns 503.
///    A closed channel (worker stopped) also returns 503.
/// 3b. Otherwise (tests): call [`ClassifierLike::classify`] directly.
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

    if let Some(tx) = &state.classify_tx {
        let (reply_tx, reply_rx) = oneshot::channel();
        let job = ClassifyJob {
            req,
            reply: reply_tx,
        };
        match tx.try_send(job) {
            Ok(()) => match reply_rx.await {
                Ok(Ok(resp)) => (StatusCode::OK, Json(resp)).into_response(),
                Ok(Err(e)) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
                Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "worker gone"),
            },
            Err(mpsc::error::TrySendError::Full(_)) => {
                error_response(StatusCode::SERVICE_UNAVAILABLE, "classify queue full")
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                error_response(StatusCode::SERVICE_UNAVAILABLE, "classify worker stopped")
            }
        }
    } else {
        match state.classifier.classify(req).await {
            Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        }
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

    /// Variant van `make_app` die een queue-enabled `AppState` opzet. De
    /// `FakeClassifier` wordt nog steeds in de state geplaatst voor non-
    /// `/classify`-routes (bv. `/cost`), maar `/classify` neemt het queue-
    /// pad zodra `tx` geleverd is.
    fn make_app_with_queue(
        result: Result<ClassifyResponse, GovernorError>,
        api_key: Option<String>,
        tx: mpsc::Sender<ClassifyJob>,
    ) -> Router {
        let fake = match result {
            Ok(r) => FakeClassifier::ok(r),
            Err(e) => FakeClassifier::err(e),
        };
        router(AppState::new(fake, api_key).with_queue(tx))
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
                baseline_opus_usd: 0.30,
                savings_usd: 0.24,
            },
        );
        let report = CostReport {
            by_tier,
            by_day: BTreeMap::new(),
            totals: DayTotals {
                count: 3,
                total_usd: 0.06,
                baseline_opus_usd: 0.30,
                savings_usd: 0.24,
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

    /// Build a router with an isolated shift-path under the supplied tempdir.
    fn make_app_with_shift_dir(tmp: &tempfile::TempDir) -> Router {
        let fake = FakeClassifier::ok(canned_response());
        let state = AppState::new(fake, None).with_shift_path(tmp.path().join("shift"));
        router(state)
    }

    #[tokio::test]
    async fn shift_get_returns_zero_when_file_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let app = make_app_with_shift_dir(&tmp);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/shift")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["value"], 0);
    }

    #[tokio::test]
    async fn shift_post_writes_and_get_reads_back() {
        let tmp = tempfile::tempdir().unwrap();
        let app = make_app_with_shift_dir(&tmp);
        let post = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/shift")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"value":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post.status(), StatusCode::OK);
        let bytes = read_body(post).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["value"], 1);

        let on_disk = std::fs::read_to_string(tmp.path().join("shift")).unwrap();
        assert_eq!(on_disk.trim(), "1");

        let get = app
            .oneshot(
                Request::builder()
                    .uri("/shift")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = read_body(get).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["value"], 1);
    }

    #[tokio::test]
    async fn shift_post_clamps_out_of_range() {
        let tmp = tempfile::tempdir().unwrap();
        let app = make_app_with_shift_dir(&tmp);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/shift")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"value":99}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["value"], 2);
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

    #[tokio::test]
    async fn classify_panel_returns_tier_and_model_hint() {
        // Arrange: mock classifier die altijd Tier::So teruggeeft
        let app = make_app(
            Ok(ClassifyResponse {
                tier: Tier::So,
                model_hint: Some("claude-sonnet-4-6".into()),
                complexity: Complexity::Standard,
                rationale: "testcase".into(),
                confidence: 80,
                estimated_input_tokens: 1000,
                estimated_output_tokens: 200,
                estimated_cost_usd: 0.003,
                alternative_tiers: vec![],
                from_cache: false,
            }),
            None,
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"task_id":"panel-test","scope_md":"Add endpoint","estimated_loc":120}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["tier"], "so");
        assert_eq!(v["model_hint"], "claude-sonnet-4-6");
        assert_eq!(v["from_cache"], false);
    }

    #[tokio::test]
    async fn classify_panel_missing_scope_md_returns_400() {
        let app = make_app(Ok(canned_response()), None);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/classify")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"task_id":"x"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn classify_queue_returns_result() {
        // Bouw een echte queue met diepte 4 + spawn een minimale worker die
        // elke job met een vaste ClassifyResponse beantwoordt.
        let (tx, mut rx) = mpsc::channel::<ClassifyJob>(4);

        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let _ = job.reply.send(Ok(ClassifyResponse {
                    tier: Tier::Hk,
                    model_hint: Some("queue-mock".into()),
                    complexity: Complexity::Trivial,
                    rationale: "from worker".into(),
                    confidence: 90,
                    estimated_input_tokens: 100,
                    estimated_output_tokens: 50,
                    estimated_cost_usd: 0.0001,
                    alternative_tiers: vec![],
                    from_cache: false,
                }));
            }
        });

        // FakeClassifier in de state wordt NIET aangeroepen — queue-pad
        // overschrijft het. We geven canned_response() mee als no-op.
        let app = make_app_with_queue(Ok(canned_response()), None, tx);

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
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // De worker-response moet doorkomen, niet de FakeClassifier-response.
        assert_eq!(v["model_hint"], "queue-mock");
        assert_eq!(v["tier"], "hk");
    }

    #[tokio::test]
    async fn classify_queue_full_returns_503() {
        // Channel met diepte 1 en GEEN worker — de buffer raakt vol bij de
        // eerste manuele send, dus de tweede send (vanuit de handler) faalt
        // met TrySendError::Full → 503.
        let (tx, _rx) = mpsc::channel::<ClassifyJob>(1);

        // Vul de queue manueel met één job. `_rx` wordt nooit gelezen, dus de
        // buffer blijft vol.
        let (dummy_reply, _dummy_rx) = oneshot::channel();
        let dummy_req: ClassifyRequest = serde_json::from_str(&valid_request_body()).unwrap();
        tx.try_send(ClassifyJob {
            req: dummy_req,
            reply: dummy_reply,
        })
        .expect("first send moet de buffer kunnen vullen");

        let app = make_app_with_queue(Ok(canned_response()), None, tx);

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

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = read_body(resp).await;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            v["error"].as_str().unwrap().contains("queue full"),
            "expected `queue full` in {v}",
        );
    }
}
