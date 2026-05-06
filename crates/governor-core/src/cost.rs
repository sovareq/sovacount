//! Cost-aggregation report built from cached classifier responses.
//!
//! Walks the on-disk cache directory, parses every `*.json` entry as a
//! [`ClassifyResponse`], and rolls them up by tier and by day. Used by
//! `governor-http`'s `GET /cost` endpoint to surface a one-glance view of
//! cumulative spend.
//!
//! ## Caveats
//!
//! - Aggregation reflects *cached* classifications only. If `no_cache=true`
//!   is set on every request, this report stays empty — fix by enabling
//!   caching for at least the requests you want to track.
//! - Day buckets use the cache file's modification-time, in **UTC**, not
//!   the time the classifier was originally invoked. Close enough for
//!   spend dashboards; not authoritative for billing.
//! - Cost values are the classifier's own `estimated_cost_usd` heuristic,
//!   not invoice-grade truth.
//!
//! ## Output shape
//!
//! ```json
//! {
//!   "by_tier": {
//!     "hk": {"count": 200, "total_usd": 0.10},
//!     "so": {"count":  50, "total_usd": 0.50},
//!     "op": {"count":   3, "total_usd": 1.20}
//!   },
//!   "by_day": {
//!     "2026-04-30": {"count": 25, "total_usd": 0.45}
//!   },
//!   "totals": {"count": 253, "total_usd": 1.80}
//! }
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::fs;
use tracing::warn;

use crate::error::{GovernorError, Result};
use crate::types::{ClassifyResponse, Tier};

/// Per-tier rollup: classification count + cumulative cost.
#[derive(Debug, Default, Clone, Serialize)]
pub struct TierTotals {
    /// Number of cached classifications for this tier.
    pub count: u64,
    /// Sum of `estimated_cost_usd` across those classifications.
    pub total_usd: f64,
    /// What it *would* have cost if every call had used the most-expensive
    /// tier (Opus-class), recomputed from each call's estimated input/output
    /// token counts using the same per-1M-token rate table that produced
    /// `total_usd`.
    pub baseline_opus_usd: f64,
    /// `baseline_opus_usd - total_usd`, never negative. The user's
    /// always-Opus-versus-routed savings, in USD.
    pub savings_usd: f64,
}

/// Per-day rollup: classification count + cumulative cost.
///
/// Day keys are `YYYY-MM-DD` strings in UTC, derived from each cache file's
/// modification time.
#[derive(Debug, Default, Clone, Serialize)]
pub struct DayTotals {
    /// Number of cached classifications on this day.
    pub count: u64,
    /// Sum of `estimated_cost_usd` across those classifications.
    pub total_usd: f64,
    /// What it *would* have cost if every call had used the most-expensive
    /// tier (Opus-class). Same recomputation as `TierTotals.baseline_opus_usd`.
    pub baseline_opus_usd: f64,
    /// `baseline_opus_usd - total_usd`, never negative.
    pub savings_usd: f64,
}

/// Aggregated cost report for a single cache directory.
#[derive(Debug, Default, Clone, Serialize)]
pub struct CostReport {
    /// Rollup keyed by tier (`hk` / `so` / `op`).
    pub by_tier: BTreeMap<Tier, TierTotals>,
    /// Rollup keyed by UTC date string (`YYYY-MM-DD`).
    pub by_day: BTreeMap<String, DayTotals>,
    /// Total across all entries.
    pub totals: DayTotals,
}

/// Walk `cache_dir` and aggregate every cached [`ClassifyResponse`].
///
/// Returns an empty report when the directory does not exist or contains
/// no usable entries — that is **not** an error, since a fresh install has
/// no cache yet. Returns `Err` only on hard I/O failures.
///
/// `provider` is used to compute the always-Opus baseline / savings using
/// the *active* provider's rate-card, so an Anthropic-routed deployment
/// reports Anthropic-Opus baselines and an OpenAI-routed one reports
/// o1-baselines. Per-call `total_usd` values are kept verbatim from the
/// cached `estimated_cost_usd`, which was already computed against the
/// active provider when the response was first produced.
pub async fn aggregate(
    cache_dir: &Path,
    provider: crate::pricing::PricingProvider,
) -> Result<CostReport> {
    let mut report = CostReport::default();

    let mut entries = match fs::read_dir(cache_dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(e) => {
            return Err(GovernorError::Cache(format!(
                "read_dir {}: {e}",
                cache_dir.display()
            )));
        }
    };

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| GovernorError::Cache(format!("next_entry: {e}")))?
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        // Skip half-written tmp files (".<key>.tmp" and atomic-write artifacts).
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        let bytes = match fs::read(&path).await {
            Ok(b) => b,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "cost: skip unreadable cache entry");
                continue;
            }
        };

        let resp: ClassifyResponse = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "cost: skip malformed cache entry");
                continue;
            }
        };

        let mtime = entry
            .metadata()
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::now());

        record(&mut report, &resp, mtime, provider);
    }

    Ok(report)
}

/// Add one [`ClassifyResponse`] into the running report.
fn record(
    report: &mut CostReport,
    resp: &ClassifyResponse,
    mtime: SystemTime,
    provider: crate::pricing::PricingProvider,
) {
    let cost = resp.estimated_cost_usd;

    // Recompute "what if every call had been Opus" from the call's own
    // input/output token estimates, using the same per-tier rate table.
    let baseline = crate::pricing::estimate_cost_usd(
        provider,
        Tier::Op,
        resp.estimated_input_tokens,
        resp.estimated_output_tokens,
    );
    let savings = (baseline - cost).max(0.0);

    let tier_totals = report.by_tier.entry(resp.tier).or_default();
    tier_totals.count += 1;
    tier_totals.total_usd += cost;
    tier_totals.baseline_opus_usd += baseline;
    tier_totals.savings_usd += savings;

    let day_key = day_key_utc(mtime);
    let day_totals = report.by_day.entry(day_key).or_default();
    day_totals.count += 1;
    day_totals.total_usd += cost;
    day_totals.baseline_opus_usd += baseline;
    day_totals.savings_usd += savings;

    report.totals.count += 1;
    report.totals.total_usd += cost;
    report.totals.baseline_opus_usd += baseline;
    report.totals.savings_usd += savings;
}

/// Format a `SystemTime` as `YYYY-MM-DD` in UTC.
///
/// Implementation: epoch-seconds → days-since-Unix-epoch →
/// civil-from-days via Howard Hinnant's algorithm (chrono-free).
/// See <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>.
fn day_key_utc(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400); // days since 1970-01-01
    let (y, m, d) = civil_from_days(z);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `(year, month, day)` from days-since-1970-01-01 (Howard Hinnant).
///
/// Valid for any signed 64-bit day count. Returns proleptic Gregorian
/// civil dates.
#[allow(clippy::manual_range_contains)]
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClassifyResponse, Complexity, Tier};
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_resp(tier: Tier, cost: f64) -> ClassifyResponse {
        ClassifyResponse {
            tier,
            model_hint: Some("model".into()),
            complexity: Complexity::Standard,
            rationale: "test".into(),
            confidence: 80,
            estimated_input_tokens: 1000,
            estimated_output_tokens: 200,
            estimated_cost_usd: cost,
            alternative_tiers: vec![],
            from_cache: false,
        }
    }

    async fn write_resp(dir: &Path, name: &str, resp: &ClassifyResponse) {
        let bytes = serde_json::to_vec(resp).unwrap();
        tokio::fs::write(dir.join(format!("{name}.json")), bytes)
            .await
            .unwrap();
    }

    #[test]
    fn civil_from_days_known_dates() {
        // Unix epoch
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // Y2K
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        // Pre-epoch
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // Far future
        assert_eq!(civil_from_days(36_525), (2070, 1, 1));
    }

    #[test]
    fn day_key_formats_iso_utc() {
        // 2026-01-15 00:00:00 UTC = 20468 days since epoch
        let t = UNIX_EPOCH + Duration::from_secs(20468 * 86_400);
        assert_eq!(day_key_utc(t), "2026-01-15");
    }

    #[tokio::test]
    async fn aggregate_empty_dir_returns_empty_report() {
        let tmp = TempDir::new().unwrap();
        let r = aggregate(tmp.path(), crate::pricing::PricingProvider::Anthropic)
            .await
            .unwrap();
        assert_eq!(r.totals.count, 0);
        assert!(r.by_tier.is_empty());
        assert!(r.by_day.is_empty());
    }

    #[tokio::test]
    async fn aggregate_missing_dir_returns_empty_report() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let r = aggregate(&missing, crate::pricing::PricingProvider::Anthropic)
            .await
            .unwrap();
        assert_eq!(r.totals.count, 0);
    }

    #[tokio::test]
    async fn aggregate_sums_per_tier_and_total() {
        let tmp = TempDir::new().unwrap();
        write_resp(tmp.path(), "a", &make_resp(Tier::Hk, 0.01)).await;
        write_resp(tmp.path(), "b", &make_resp(Tier::Hk, 0.02)).await;
        write_resp(tmp.path(), "c", &make_resp(Tier::So, 0.10)).await;
        write_resp(tmp.path(), "d", &make_resp(Tier::Op, 0.50)).await;

        let r = aggregate(tmp.path(), crate::pricing::PricingProvider::Anthropic)
            .await
            .unwrap();
        assert_eq!(r.totals.count, 4);
        assert!((r.totals.total_usd - 0.63).abs() < 1e-9);

        let hk = r.by_tier.get(&Tier::Hk).unwrap();
        assert_eq!(hk.count, 2);
        assert!((hk.total_usd - 0.03).abs() < 1e-9);

        let so = r.by_tier.get(&Tier::So).unwrap();
        assert_eq!(so.count, 1);
        assert!((so.total_usd - 0.10).abs() < 1e-9);

        let op = r.by_tier.get(&Tier::Op).unwrap();
        assert_eq!(op.count, 1);
        assert!((op.total_usd - 0.50).abs() < 1e-9);
    }

    #[tokio::test]
    async fn aggregate_skips_non_json_and_dotfiles() {
        let tmp = TempDir::new().unwrap();
        write_resp(tmp.path(), "real", &make_resp(Tier::Hk, 0.01)).await;
        // junk that should be skipped
        tokio::fs::write(tmp.path().join("README.md"), b"# notes")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join(".tmp.json"), b"{}")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("garbage.json"), b"{not json")
            .await
            .unwrap();

        let r = aggregate(tmp.path(), crate::pricing::PricingProvider::Anthropic)
            .await
            .unwrap();
        assert_eq!(r.totals.count, 1, "only the real entry should count");
    }

    #[tokio::test]
    async fn aggregate_serializes_with_lowercase_tier_keys() {
        let tmp = TempDir::new().unwrap();
        write_resp(tmp.path(), "a", &make_resp(Tier::Hk, 0.01)).await;
        let r = aggregate(tmp.path(), crate::pricing::PricingProvider::Anthropic)
            .await
            .unwrap();
        let json = serde_json::to_value(&r).unwrap();
        assert!(json["by_tier"]["hk"].is_object(), "got: {json}");
    }

    #[tokio::test]
    async fn aggregate_baseline_uses_passed_provider() {
        // Same calls aggregated under Ollama (zero-cost rate-card) must
        // yield zero baseline / zero savings, regardless of cached
        // `estimated_cost_usd` values that were originally computed against
        // some other rate-card.
        let tmp = TempDir::new().unwrap();
        write_resp(tmp.path(), "a", &make_resp(Tier::Hk, 0.01)).await;
        let r = aggregate(tmp.path(), crate::pricing::PricingProvider::Ollama)
            .await
            .unwrap();
        assert_eq!(r.totals.baseline_opus_usd, 0.0);
        assert_eq!(r.totals.savings_usd, 0.0);
    }
}
