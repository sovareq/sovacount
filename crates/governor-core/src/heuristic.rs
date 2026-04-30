//! Rule-based fast-path. Used to short-circuit obvious cases without an LLM call.
//!
//! **Stub** — body filled in by the core-worker.
//!
//! Decision rules from `~/Sovareq/_workspace/conventions/token-governor-tags.md`:
//! - HAIKU: pure-text (docs/i18n/typo-fix), <50 LOC, 1 file
//! - SONNET: code-impl on known pattern, <300 LOC, ≤3 files
//! - OPUS: novel arch / multi-domain / fan-out / >300 LOC / SSOT-update

#![allow(dead_code)]

use crate::types::{ClassifyRequest, ClassifyResponse};

/// Returns `Some` only if the request is unambiguous on size signals.
/// Returns `None` if the LLM should make the call.
pub fn fast_path(_req: &ClassifyRequest) -> Option<ClassifyResponse> {
    None
}
