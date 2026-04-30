//! End-to-end smoke tests for `governor-core` using the deterministic mock
//! provider. These tests do not touch the network or the user's home dir.

use governor_core::{Classifier, ClassifyRequest, Config, Tier};
use std::time::Duration;

/// Build a `Config::mock()` with a unique cache directory rooted in tempdir.
fn isolated_mock_config() -> (tempfile::TempDir, Config) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::mock();
    cfg.cache_dir = tmp.path().to_path_buf();
    cfg.cache_ttl_secs = 600; // 10 minutes is plenty for the test.
    (tmp, cfg)
}

#[tokio::test]
async fn end_to_end_classify_then_cache_hit() {
    let (_tmp, cfg) = isolated_mock_config();
    let cls = Classifier::new(cfg).await.unwrap();

    let req = ClassifyRequest {
        task_id: "T-INT-1".into(),
        scope_md: "Implement a list endpoint with pagination.".into(),
        ssot_refs: vec![],
        estimated_loc: Some(150),
        estimated_files: Some(2),
        no_cache: false,
    };

    // First call → live (mock), no cache.
    let first = cls.classify(req.clone()).await.expect("first classify");
    assert!(!first.from_cache, "first call should be live");
    assert_eq!(first.tier, Tier::So);
    assert_eq!(first.model_hint.as_deref(), Some("mock-sonnet"));
    assert!(first.estimated_cost_usd > 0.0);

    // Second identical call → cache hit.
    let second = cls.classify(req).await.expect("second classify");
    assert!(second.from_cache, "second call should hit cache");
    assert_eq!(second.tier, first.tier);
    assert_eq!(second.model_hint, first.model_hint);
}

#[tokio::test]
async fn end_to_end_hk_via_fast_path() {
    let (_tmp, cfg) = isolated_mock_config();
    let cls = Classifier::new(cfg).await.unwrap();

    let req = ClassifyRequest {
        task_id: "T-INT-HK".into(),
        scope_md: "Fix typos in README.md.".into(),
        ssot_refs: vec![],
        estimated_loc: Some(5),
        estimated_files: Some(1),
        no_cache: false,
    };
    let resp = cls.classify(req).await.unwrap();
    assert_eq!(resp.tier, Tier::Hk);
    assert_eq!(resp.model_hint.as_deref(), Some("mock-haiku"));
    // Fast-path confidence floor.
    assert!(resp.confidence >= 90);
}

#[tokio::test]
async fn end_to_end_op_via_fast_path() {
    let (_tmp, cfg) = isolated_mock_config();
    let cls = Classifier::new(cfg).await.unwrap();

    let req = ClassifyRequest {
        task_id: "T-INT-OP".into(),
        scope_md: "Cross-service migration with breaking auth changes.".into(),
        ssot_refs: vec!["ssot/contracts.md".into()],
        estimated_loc: Some(800),
        estimated_files: Some(20),
        no_cache: false,
    };
    let resp = cls.classify(req).await.unwrap();
    assert_eq!(resp.tier, Tier::Op);
    assert_eq!(resp.model_hint.as_deref(), Some("mock-opus"));
}

#[tokio::test]
async fn no_cache_flag_forces_fresh_call() {
    let (_tmp, cfg) = isolated_mock_config();
    let cls = Classifier::new(cfg).await.unwrap();

    let mut req = ClassifyRequest {
        task_id: "T-INT-NC".into(),
        scope_md: "Add new list endpoint.".into(),
        ssot_refs: vec![],
        estimated_loc: Some(150),
        estimated_files: Some(2),
        no_cache: false,
    };

    // Prime the cache.
    let _ = cls.classify(req.clone()).await.unwrap();

    // Re-run with no_cache=true → must NOT come from cache.
    req.no_cache = true;
    let fresh = cls.classify(req).await.unwrap();
    assert!(!fresh.from_cache, "no_cache=true must bypass cache");
}

#[tokio::test]
async fn classifier_construction_is_fast_and_idempotent() {
    let (_tmp, cfg) = isolated_mock_config();
    // Construction must be cheap — no real I/O for the mock provider.
    let start = std::time::Instant::now();
    let _cls = Classifier::new(cfg.clone()).await.unwrap();
    let _cls2 = Classifier::new(cfg).await.unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "construction took too long"
    );
}
