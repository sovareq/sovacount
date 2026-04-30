//! File-based cache for classifier responses.
//!
//! **Stub** — body filled in by the core-worker.
//!
//! Design: `~/.cache/token-governor/<sha256-of-canonical-input-json>.json`.
//! TTL via mtime check.

#![allow(dead_code)]

use crate::error::Result;
use crate::types::{ClassifyRequest, ClassifyResponse};

use std::path::PathBuf;
use std::time::Duration;

pub struct Cache {
    pub(crate) dir: PathBuf,
    pub(crate) ttl: Duration,
}

impl Cache {
    pub fn new(dir: PathBuf, ttl: Duration) -> Result<Self> {
        Ok(Self { dir, ttl })
    }

    pub async fn get(&self, _req: &ClassifyRequest) -> Result<Option<ClassifyResponse>> {
        unimplemented!("filled in by Worker A (core)")
    }

    pub async fn put(&self, _req: &ClassifyRequest, _resp: &ClassifyResponse) -> Result<()> {
        unimplemented!("filled in by Worker A (core)")
    }
}
