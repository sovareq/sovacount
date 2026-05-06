//! Per-provider pricing model used to estimate `estimated_cost_usd` on
//! classifier responses.
//!
//! Pricing is loaded from a layered source (see [`PricingConfig::load`]):
//! 1. `GOVERNOR_PRICING_FILE` env-var (TOML file).
//! 2. `~/.config/token-governor/pricing.toml` (TOML file).
//! 3. Built-in defaults — [`PricingConfig::defaults`].
//!
//! Source of the per-1M-token rates: May 2026 list-prices for
//! Anthropic (claude-haiku-4-5 / claude-sonnet-4-6 / claude-opus-4-7),
//! OpenAI (gpt-4o-mini / gpt-4o / o1) and Ollama (free, local). The
//! `Custom` provider falls back to Anthropic-equivalent rates as a
//! best-effort default for unknown OpenAI-compatible endpoints.
//!
//! See `docs/intelligence/2026-05-02-pricing-and-config.md` for sources.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::types::Tier;

/// Identifies the LLM provider whose rates to apply.
///
/// Mirrors [`crate::config::ProviderKind`] but is decoupled so the pricing
/// layer can be used without pulling in the full provider runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PricingProvider {
    /// Anthropic Claude family (haiku-4-5 / sonnet-4-6 / opus-4-7).
    Anthropic,
    /// OpenAI family (gpt-4o-mini / gpt-4o / o1).
    OpenAI,
    /// Ollama — local inference, treated as zero-cost.
    Ollama,
    /// Custom OpenAI-compatible endpoint. Falls back to Anthropic-equivalent
    /// rates as a best-effort default.
    Custom,
}

/// Per-tier input/output rates expressed in USD per 1M tokens.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct TierRates {
    /// USD per 1M input tokens.
    pub input_per_mtok: f64,
    /// USD per 1M output tokens.
    pub output_per_mtok: f64,
}

/// Rates for one provider, broken down by tier.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderRates {
    /// Haiku-class tier rates.
    pub hk: TierRates,
    /// Sonnet-class tier rates.
    pub so: TierRates,
    /// Opus-class tier rates.
    pub op: TierRates,
}

/// Top-level pricing configuration covering every supported provider.
///
/// Field order matches the TOML schema written by `pricing.toml`:
/// `[anthropic.<tier>]`, `[openai.<tier>]`, `[ollama.<tier>]`, `[custom.<tier>]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PricingConfig {
    /// Anthropic-family rates.
    pub anthropic: ProviderRates,
    /// OpenAI-family rates.
    pub openai: ProviderRates,
    /// Ollama-family rates (zero-cost by default).
    pub ollama: ProviderRates,
    /// Custom-endpoint fallback rates.
    pub custom: ProviderRates,
}

impl PricingConfig {
    /// Load priority:
    /// 1. `GOVERNOR_PRICING_FILE` env-var → file path.
    /// 2. `~/.config/token-governor/pricing.toml`.
    /// 3. Built-in [`PricingConfig::defaults`].
    ///
    /// Any I/O or parse failure is logged via `tracing::warn!` and falls
    /// back to defaults — pricing must never block classification.
    pub fn load() -> Self {
        if let Some(path) = pricing_file_from_env() {
            return read_pricing_or_default(&path);
        }
        if let Some(path) = pricing_file_from_user_config()
            && path.exists()
        {
            return read_pricing_or_default(&path);
        }
        Self::defaults()
    }

    /// Built-in defaults — verified May 2026 list-prices.
    ///
    /// | Provider  | hk             | so              | op              |
    /// |-----------|----------------|-----------------|-----------------|
    /// | Anthropic | 1.00 / 5.00    | 3.00 / 15.00    | 5.00 / 25.00    |
    /// | OpenAI    | 0.15 / 0.60    | 2.50 / 10.00    | 15.00 / 60.00   |
    /// | Ollama    | 0 / 0          | 0 / 0           | 0 / 0           |
    /// | Custom    | 1.00 / 5.00    | 3.00 / 15.00    | 5.00 / 25.00    |
    pub fn defaults() -> Self {
        let anthropic = ProviderRates {
            hk: TierRates {
                input_per_mtok: 1.00,
                output_per_mtok: 5.00,
            },
            so: TierRates {
                input_per_mtok: 3.00,
                output_per_mtok: 15.00,
            },
            op: TierRates {
                input_per_mtok: 5.00,
                output_per_mtok: 25.00,
            },
        };
        let openai = ProviderRates {
            hk: TierRates {
                input_per_mtok: 0.15,
                output_per_mtok: 0.60,
            },
            so: TierRates {
                input_per_mtok: 2.50,
                output_per_mtok: 10.00,
            },
            op: TierRates {
                input_per_mtok: 15.00,
                output_per_mtok: 60.00,
            },
        };
        let ollama = ProviderRates::default();
        // Custom-fallback = Anthropic rates: best-effort for unknown
        // OpenAI-compatible endpoints. Override via pricing.toml.
        let custom = anthropic.clone();

        Self {
            anthropic,
            openai,
            ollama,
            custom,
        }
    }

    /// Lookup the rates for a `(provider, tier)` pair.
    pub fn rates(&self, provider: PricingProvider, tier: Tier) -> TierRates {
        let pr = match provider {
            PricingProvider::Anthropic => &self.anthropic,
            PricingProvider::OpenAI => &self.openai,
            PricingProvider::Ollama => &self.ollama,
            PricingProvider::Custom => &self.custom,
        };
        match tier {
            Tier::Hk => pr.hk,
            Tier::So => pr.so,
            Tier::Op => pr.op,
        }
    }
}

/// Process-wide cached pricing config, populated lazily on first use.
static PRICING: OnceLock<PricingConfig> = OnceLock::new();

/// Compute estimated USD cost for `(provider, tier, input_tokens, output_tokens)`.
///
/// Reads from `OnceLock<PricingConfig>` — initialised on first call via
/// [`PricingConfig::load`]. Subsequent calls are lock-free reads.
pub fn estimate_cost_usd(
    provider: PricingProvider,
    tier: Tier,
    input_tokens: u32,
    output_tokens: u32,
) -> f64 {
    let cfg = PRICING.get_or_init(PricingConfig::load);
    let rates = cfg.rates(provider, tier);
    let input = (input_tokens as f64) * rates.input_per_mtok / 1_000_000.0;
    let output = (output_tokens as f64) * rates.output_per_mtok / 1_000_000.0;
    input + output
}

fn pricing_file_from_env() -> Option<PathBuf> {
    std::env::var("GOVERNOR_PRICING_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn pricing_file_from_user_config() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("token-governor")
            .join("pricing.toml"),
    )
}

fn read_pricing_or_default(path: &std::path::Path) -> PricingConfig {
    match std::fs::read_to_string(path) {
        Ok(body) => match toml::from_str::<PricingConfig>(&body) {
            Ok(cfg) => cfg,
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "pricing: TOML parse failed; falling back to built-in defaults"
                );
                PricingConfig::defaults()
            }
        },
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "pricing: file read failed; falling back to built-in defaults"
            );
            PricingConfig::defaults()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost_with_defaults(provider: PricingProvider, tier: Tier, input: u32, output: u32) -> f64 {
        let cfg = PricingConfig::defaults();
        let rates = cfg.rates(provider, tier);
        (input as f64) * rates.input_per_mtok / 1_000_000.0
            + (output as f64) * rates.output_per_mtok / 1_000_000.0
    }

    #[test]
    fn cost_haiku_small() {
        // 1500 input + 400 output @ Anthropic HK rates ($1.00 / $5.00).
        // 1500 * 1.00 / 1e6 = 0.0015
        // 400  * 5.00 / 1e6 = 0.0020
        // total = 0.0035
        let c = cost_with_defaults(PricingProvider::Anthropic, Tier::Hk, 1500, 400);
        assert!((c - 0.0035).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn cost_opus_large() {
        // 12000 input + 3500 output @ Anthropic OP rates ($5.00 / $25.00).
        // 12000 * 5  / 1e6 = 0.06
        // 3500  * 25 / 1e6 = 0.0875
        // total = 0.1475
        let c = cost_with_defaults(PricingProvider::Anthropic, Tier::Op, 12_000, 3_500);
        assert!((c - 0.1475).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn cost_zero_tokens_is_zero() {
        for t in [Tier::Hk, Tier::So, Tier::Op] {
            assert_eq!(cost_with_defaults(PricingProvider::Anthropic, t, 0, 0), 0.0);
        }
    }

    #[test]
    fn ollama_is_free_at_every_tier() {
        for t in [Tier::Hk, Tier::So, Tier::Op] {
            let c = cost_with_defaults(PricingProvider::Ollama, t, 1_000_000, 1_000_000);
            assert_eq!(c, 0.0, "ollama tier {t:?} should be zero-cost");
        }
    }

    #[test]
    fn openai_hk_matches_listprice() {
        // 1M input + 1M output @ OpenAI HK ($0.15 / $0.60) = $0.75
        let c = cost_with_defaults(PricingProvider::OpenAI, Tier::Hk, 1_000_000, 1_000_000);
        assert!((c - 0.75).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn custom_falls_back_to_anthropic_equivalent() {
        let cfg = PricingConfig::defaults();
        let custom = cfg.rates(PricingProvider::Custom, Tier::Op);
        let anthro = cfg.rates(PricingProvider::Anthropic, Tier::Op);
        assert_eq!(custom.input_per_mtok, anthro.input_per_mtok);
        assert_eq!(custom.output_per_mtok, anthro.output_per_mtok);
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let cfg = PricingConfig::defaults();
        let serialised = toml::to_string(&cfg).expect("serialize defaults");
        let back: PricingConfig = toml::from_str(&serialised).expect("parse round-trip");
        assert!(
            (back.anthropic.so.input_per_mtok - 3.00).abs() < 1e-9,
            "anthropic so input rate survived round-trip"
        );
        assert!(
            (back.openai.op.output_per_mtok - 60.00).abs() < 1e-9,
            "openai op output rate survived round-trip"
        );
    }

    #[test]
    fn estimate_cost_usd_uses_oncelock() {
        // Smoke-test the public entrypoint. Defaults must be sane even
        // if no config file is present in the test environment.
        let c = estimate_cost_usd(PricingProvider::Anthropic, Tier::Hk, 1500, 400);
        // Either matches defaults (0.0035) or matches a user-supplied
        // pricing.toml on this machine — assert non-negative + finite.
        assert!(c.is_finite() && c >= 0.0, "got {c}");
    }
}
