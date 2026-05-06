//! Core engine for [`token-governor`] — agent-agnostic LLM-tier classifier.
//!
//! # Quickstart
//!
//! ```no_run
//! use governor_core::{Classifier, ClassifyRequest, Config};
//!
//! # async fn ex() -> Result<(), governor_core::GovernorError> {
//! let cfg = Config::from_env()?;
//! let classifier = Classifier::new(cfg).await?;
//! let resp = classifier.classify(ClassifyRequest {
//!     task_id: "TD-201-F".into(),
//!     scope_md: "Fix path-bug in mcp-server dispatch.".into(),
//!     ssot_refs: vec![],
//!     estimated_loc: Some(50),
//!     estimated_files: Some(1),
//!     no_cache: false,
//!     shift: 0,
//! }).await?;
//! println!("{}", resp.tier);
//! # Ok(()) }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cache;
pub mod classifier;
pub mod config;
pub mod cost;
pub mod error;
pub mod heuristic;
pub mod pricing;
pub mod prompt;
pub mod providers;
pub mod types;

pub use classifier::Classifier;
pub use config::Config;
pub use cost::{CostReport, DayTotals, TierTotals};
pub use error::{GovernorError, Result};
pub use pricing::{PricingConfig, PricingProvider, ProviderRates, TierRates};
pub use types::{AlternativeTier, ClassifyRequest, ClassifyResponse, Complexity, Tier};
