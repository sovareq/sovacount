//! The rmcp 1.5 server: tool-router, request-mapping, and `ServerHandler`.
//!
//! The single tool `governor_classify` accepts a flat JSON object whose shape
//! matches [`ClassifyParams`] and returns the JSON form of
//! [`governor_core::ClassifyResponse`] as structured tool content. The text
//! content is auto-populated by [`rmcp::model::CallToolResult::structured`]
//! so older MCP clients that ignore `structuredContent` still see usable
//! output.

#![forbid(unsafe_code)]

use std::sync::Arc;

use governor_core::{ClassifyRequest, ClassifyResponse};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::state::ClassifierLike;

/// Public name advertised in `initialize.serverInfo.name`.
pub const SERVER_NAME: &str = "token-governor-mcp";

/// Public version advertised in `initialize.serverInfo.version`.
pub const SERVER_VERSION: &str = "0.1.0";

/// Human-readable hint shown to the agent host on `initialize`.
pub const SERVER_DESCRIPTION: &str = "Cost-optimizing classifier for AI-agent tasks. \
                                       Use governor_classify to get a recommended model tier.";

/// Flat tool-input shape — mirrors [`ClassifyRequest`] but lives here so its
/// JSON-schema can be derived via `schemars` without coupling that derive to
/// the core crate's wire types.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ClassifyParams {
    /// External tranche/task identifier (e.g. `T-G-1`, `TD-201-F`).
    pub task_id: String,

    /// Free-text scope description in markdown.
    pub scope_md: String,

    /// Optional list of SSOT files relevant to the task. Plain paths,
    /// resolved by the caller.
    #[serde(default)]
    pub ssot_refs: Vec<String>,

    /// Caller's rough LOC estimate. Omit if unknown.
    #[serde(default)]
    pub estimated_loc: Option<u32>,

    /// Caller's rough file-count estimate. Omit if unknown.
    #[serde(default)]
    pub estimated_files: Option<u32>,

    /// If `true`, governor must skip cache lookup for this request.
    #[serde(default)]
    pub no_cache: bool,

    /// Tier-shift override. `+1` = upshift (more capable), `-1` = downshift
    /// (cheaper), `0` = honour the classifier exactly. Out-of-range values
    /// clamp.
    #[serde(default)]
    pub shift: i32,
}

impl From<ClassifyParams> for ClassifyRequest {
    fn from(p: ClassifyParams) -> Self {
        Self {
            task_id: p.task_id,
            scope_md: p.scope_md,
            ssot_refs: p.ssot_refs,
            estimated_loc: p.estimated_loc,
            estimated_files: p.estimated_files,
            no_cache: p.no_cache,
            shift: p.shift,
        }
    }
}

/// The MCP server. Generic over the classifier so tests can inject a fake.
///
/// Cheap to clone — both fields are `Arc`-shared.
#[derive(Clone)]
pub struct GovernorServer<C: ClassifierLike> {
    classifier: Arc<C>,
    // Read by the `#[tool_handler]`-generated `ServerHandler::call_tool`
    // glue; the bare-field access is opaque to dead-code analysis.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl<C: ClassifierLike> GovernorServer<C> {
    /// Build a server around an existing classifier.
    pub fn new(classifier: Arc<C>) -> Self {
        Self {
            classifier,
            tool_router: Self::tool_router(),
        }
    }

    /// Run the classifier and convert the response into a structured
    /// [`CallToolResult`]. Used by both the rmcp tool-router entry point
    /// and the in-process unit tests.
    ///
    /// Errors from the classifier are surfaced as MCP `internal_error`
    /// responses so the agent host sees a structured failure rather than
    /// an aborted request.
    pub async fn handle_classify(
        &self,
        params: ClassifyParams,
    ) -> Result<CallToolResult, McpError> {
        let req: ClassifyRequest = params.into();
        let resp: ClassifyResponse = self
            .classifier
            .classify(req)
            .await
            .map_err(|e| McpError::internal_error(format!("classifier error: {e}"), None))?;
        let value = serde_json::to_value(&resp).map_err(|e| {
            McpError::internal_error(format!("response serialization failed: {e}"), None)
        })?;
        Ok(CallToolResult::structured(value))
    }
}

#[tool_router]
impl<C: ClassifierLike> GovernorServer<C> {
    /// Classify a coding task and recommend a model tier.
    #[tool(
        name = "governor_classify",
        description = "Classify a coding task and recommend a model tier (@op / @so / @hk). \
                       Returns the chosen tier, model hint, complexity, rationale, confidence, \
                       and rough cost estimate."
    )]
    pub async fn governor_classify(
        &self,
        Parameters(params): Parameters<ClassifyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.handle_classify(params).await
    }
}

#[tool_handler]
impl<C: ClassifierLike> ServerHandler for GovernorServer<C> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(SERVER_NAME, SERVER_VERSION)
                    .with_description(SERVER_DESCRIPTION),
            )
            .with_instructions(SERVER_DESCRIPTION)
    }
}
