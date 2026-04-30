//! File-based cache for classifier responses.
//!
//! Layout: `<cache_dir>/<sha256-hex>.json`. The key is the SHA-256 of a
//! canonical JSON form of the [`ClassifyRequest`] (the `no_cache` flag is
//! intentionally stripped from the canonical form so that toggling it does
//! not invalidate otherwise-equal entries).
//!
//! TTL is enforced via the file's mtime; `get()` treats anything older than
//! `ttl` as a miss without touching the file content.
//!
//! Writes are atomic: a temp file in the same directory is written first and
//! then renamed into place, so a concurrent reader never sees a partial JSON.
//!
//! All errors funnel through [`crate::error::GovernorError::Cache`].

use crate::error::{GovernorError, Result};
use crate::types::{ClassifyRequest, ClassifyResponse};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{trace, warn};

/// File-based response cache.
///
/// Cheap to construct — the directory is created lazily on first write so
/// `Cache::new` itself is non-failing as long as the parent path is sane.
pub struct Cache {
    pub(crate) dir: PathBuf,
    pub(crate) ttl: Duration,
}

impl Cache {
    /// Construct a cache rooted at `dir` with the given `ttl`.
    ///
    /// The directory is created eagerly (best-effort) so that subsequent
    /// `put()` calls don't have to race on `mkdir`.
    pub fn new(dir: PathBuf, ttl: Duration) -> Result<Self> {
        std::fs::create_dir_all(&dir).map_err(|e| {
            GovernorError::Cache(format!("create cache-dir {}: {e}", dir.display()))
        })?;
        Ok(Self { dir, ttl })
    }

    /// Look up a previously-cached response for `req`.
    ///
    /// Returns `Ok(None)` when:
    /// - the file does not exist,
    /// - the file is older than `ttl`,
    /// - the file body fails to deserialise (treated as a miss; warn-logged).
    ///
    /// Returns `Err` only on hard I/O failures (permission denied, etc.).
    pub async fn get(&self, req: &ClassifyRequest) -> Result<Option<ClassifyResponse>> {
        let path = self.path_for(req);

        let meta = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(GovernorError::Cache(format!(
                    "stat {}: {e}",
                    path.display()
                )));
            }
        };

        if let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > self.ttl
        {
            trace!(
                cache_path = %path.display(),
                age_secs = age.as_secs(),
                ttl_secs = self.ttl.as_secs(),
                "cache entry expired"
            );
            return Ok(None);
        }

        let bytes = fs::read(&path)
            .await
            .map_err(|e| GovernorError::Cache(format!("read {}: {e}", path.display())))?;

        match serde_json::from_slice::<ClassifyResponse>(&bytes) {
            Ok(mut resp) => {
                resp.from_cache = true;
                trace!(cache_path = %path.display(), "cache hit");
                Ok(Some(resp))
            }
            Err(e) => {
                warn!(
                    cache_path = %path.display(),
                    error = %e,
                    "cache entry malformed; treating as miss"
                );
                Ok(None)
            }
        }
    }

    /// Store `resp` under the canonical key for `req`.
    ///
    /// Writes are atomic via `rename(2)` from a temp file in the same
    /// directory.
    pub async fn put(&self, req: &ClassifyRequest, resp: &ClassifyResponse) -> Result<()> {
        let path = self.path_for(req);
        let parent = path.parent().unwrap_or(&self.dir);

        fs::create_dir_all(parent)
            .await
            .map_err(|e| GovernorError::Cache(format!("mkdir {}: {e}", parent.display())))?;

        // Temp file in the same dir → rename is atomic on the same FS.
        let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("entry");
        let tmp = parent.join(format!(".{key}.tmp"));

        let bytes = serde_json::to_vec(resp)?;
        let mut f = fs::File::create(&tmp)
            .await
            .map_err(|e| GovernorError::Cache(format!("create {}: {e}", tmp.display())))?;
        f.write_all(&bytes)
            .await
            .map_err(|e| GovernorError::Cache(format!("write {}: {e}", tmp.display())))?;
        f.flush()
            .await
            .map_err(|e| GovernorError::Cache(format!("flush {}: {e}", tmp.display())))?;
        // Drop the file handle before rename so Windows is happy
        // (no-op on Unix but cheap).
        drop(f);

        fs::rename(&tmp, &path).await.map_err(|e| {
            GovernorError::Cache(format!(
                "rename {} -> {}: {e}",
                tmp.display(),
                path.display()
            ))
        })?;

        trace!(cache_path = %path.display(), "cache put");
        Ok(())
    }

    /// Compute the on-disk path for `req`.
    pub(crate) fn path_for(&self, req: &ClassifyRequest) -> PathBuf {
        let key = canonical_key(req);
        self.dir.join(format!("{key}.json"))
    }
}

/// Canonical hex-encoded SHA-256 of the cache-key form of `req`.
pub(crate) fn canonical_key(req: &ClassifyRequest) -> String {
    let canonical = canonical_value(req);
    // serde_json's BTreeMap-ordering produces a stable ordering for the keys
    // we use, but we serialise to bytes via a deterministic recursive walk
    // for safety against future schema additions.
    let bytes = canonical_to_bytes(&canonical);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex::encode(hasher.finalize())
}

/// Strip non-key fields (`no_cache`) and produce a canonical `serde_json::Value`.
fn canonical_value(req: &ClassifyRequest) -> Value {
    json!({
        "task_id": req.task_id,
        "scope_md": req.scope_md,
        "ssot_refs": req.ssot_refs,
        "estimated_loc": req.estimated_loc,
        "estimated_files": req.estimated_files,
    })
}

/// Stable byte-level encoding of a `Value`: object keys sorted, no whitespace.
fn canonical_to_bytes(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            // serde_json::to_string handles JSON-string escaping for us.
            let escaped = serde_json::to_string(s).expect("string is always serialisable");
            out.extend_from_slice(escaped.as_bytes());
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            // Sort keys for determinism.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let key_escaped = serde_json::to_string(k).expect("string is always serialisable");
                out.extend_from_slice(key_escaped.as_bytes());
                out.push(b':');
                write_canonical(map.get(*k).expect("key from map"), out);
            }
            out.push(b'}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Complexity, Tier};
    use std::time::Duration;
    use tempfile::TempDir;

    fn sample_req(no_cache: bool) -> ClassifyRequest {
        ClassifyRequest {
            task_id: "T-1".into(),
            scope_md: "Add hello-world".into(),
            ssot_refs: vec!["ssot/contracts.md".into()],
            estimated_loc: Some(40),
            estimated_files: Some(1),
            no_cache,
        }
    }

    fn sample_resp() -> ClassifyResponse {
        ClassifyResponse {
            tier: Tier::Hk,
            model_hint: Some("claude-haiku-4-5".into()),
            complexity: Complexity::Trivial,
            rationale: "tiny scope".into(),
            confidence: 95,
            estimated_input_tokens: 1500,
            estimated_output_tokens: 400,
            estimated_cost_usd: 0.0028,
            alternative_tiers: vec![],
            from_cache: false,
        }
    }

    #[test]
    fn key_independent_of_no_cache_flag() {
        let a = canonical_key(&sample_req(false));
        let b = canonical_key(&sample_req(true));
        assert_eq!(a, b);
    }

    #[test]
    fn key_changes_with_scope() {
        let a = canonical_key(&sample_req(false));
        let mut r = sample_req(false);
        r.scope_md = "different".into();
        let b = canonical_key(&r);
        assert_ne!(a, b);
    }

    #[test]
    fn canonical_encoding_orders_keys() {
        // Hand-build two equal Values with different insertion order;
        // the canonical bytes must match.
        let a: Value = serde_json::from_str(r#"{"a":1,"b":2,"c":3}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"c":3,"a":1,"b":2}"#).unwrap();
        assert_eq!(canonical_to_bytes(&a), canonical_to_bytes(&b));
    }

    #[tokio::test]
    async fn put_then_get_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf(), Duration::from_secs(60)).unwrap();
        let req = sample_req(false);
        let resp = sample_resp();

        cache.put(&req, &resp).await.unwrap();

        let got = cache.get(&req).await.unwrap().expect("hit");
        assert_eq!(got.tier, resp.tier);
        assert!(got.from_cache, "from_cache must be set on hit");
    }

    #[tokio::test]
    async fn miss_when_absent() {
        let tmp = TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf(), Duration::from_secs(60)).unwrap();
        let req = sample_req(false);
        assert!(cache.get(&req).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn miss_when_expired() {
        let tmp = TempDir::new().unwrap();
        // ttl=0 → anything is expired.
        let cache = Cache::new(tmp.path().to_path_buf(), Duration::from_nanos(1)).unwrap();
        let req = sample_req(false);
        let resp = sample_resp();
        cache.put(&req, &resp).await.unwrap();
        // Sleep a tick to make sure mtime - now > ttl.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(cache.get(&req).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_entry_is_treated_as_miss() {
        let tmp = TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf(), Duration::from_secs(600)).unwrap();
        let req = sample_req(false);
        let path = cache.path_for(&req);
        fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        fs::write(&path, b"{not json").await.unwrap();
        assert!(cache.get(&req).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_is_atomic_no_partial_files() {
        let tmp = TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf(), Duration::from_secs(600)).unwrap();
        let req = sample_req(false);
        let resp = sample_resp();
        cache.put(&req, &resp).await.unwrap();
        // After put, only the final file should exist (no leftover .tmp).
        let mut entries = fs::read_dir(tmp.path()).await.unwrap();
        let mut names: Vec<String> = vec![];
        while let Some(e) = entries.next_entry().await.unwrap() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(names.len(), 1, "expected exactly one file, got {names:?}");
        assert!(names[0].ends_with(".json"));
    }
}
