//! Classifier abstraction so the MCP tool handler can be tested without
//! requiring a real LLM provider or filesystem cache.
//!
//! Production code uses [`RealClassifier`], which delegates to
//! [`governor_core::Classifier`]. Tests build their own fake type that
//! implements [`ClassifierLike`].

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use governor_core::{Classifier, ClassifyRequest, ClassifyResponse, Result};

/// Anything the MCP tool handler can call to classify a request.
///
/// Kept tiny (single method) so test fakes are cheap to write.
#[async_trait]
pub trait ClassifierLike: Send + Sync + 'static {
    /// Classify a single request.
    async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse>;
}

/// Production wrapper around [`governor_core::Classifier`].
///
/// The inner classifier is `Arc`-wrapped so the MCP server type can be cheap
/// to clone (rmcp clones the handler per inbound request).
#[derive(Clone)]
pub struct RealClassifier {
    inner: Arc<Classifier>,
}

impl RealClassifier {
    /// Wrap an already-constructed [`Classifier`].
    pub fn new(inner: Arc<Classifier>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ClassifierLike for RealClassifier {
    async fn classify(&self, req: ClassifyRequest) -> Result<ClassifyResponse> {
        self.inner.classify(req).await
    }
}
