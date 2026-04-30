//! Public façade for the classifier engine.
//!
//! **Stub** — implementation is filled in by the core-worker. Public surface
//! is fixed so that `governor-cli`, `governor-http`, and `governor-mcp` can
//! compile against this contract in parallel.

use crate::config::Config;
use crate::error::Result;
use crate::types::{ClassifyRequest, ClassifyResponse};

/// The classifier orchestrates: cache lookup → heuristic fast-path → LLM call.
///
/// Cheap to clone (internally `Arc`-wrapped state).
pub struct Classifier {
    #[allow(dead_code)]
    pub(crate) config: Config,
    // Real state added by the core-worker (provider, cache, heuristic, etc.).
}

impl Classifier {
    /// Construct a classifier from a fully-resolved [`Config`].
    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self { config })
    }

    /// Classify a single task.
    ///
    /// Resolution order:
    /// 1. Cache (skipped if `req.no_cache`)
    /// 2. Heuristic fast-path (only if scope is unambiguous)
    /// 3. LLM provider call
    pub async fn classify(&self, _req: ClassifyRequest) -> Result<ClassifyResponse> {
        unimplemented!("filled in by Worker A (core)")
    }
}
