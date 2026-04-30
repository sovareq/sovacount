//! Tier-level pricing model used to estimate `estimated_cost_usd` on classifier
//! responses.
//!
//! Source of the per-1M-token rates: April 2026 list-prices for the canonical
//! Anthropic mapping (`claude-haiku-4-5` / `claude-sonnet-4-6` /
//! `claude-opus-4-7`). These are deliberately hardcoded for now — a future
//! tranche can wire them through `mapping.toml`.

use crate::types::Tier;

/// USD-per-1M-input-tokens for the chosen tier (April 2026 list-prices).
pub(crate) fn input_rate_per_mtok(tier: Tier) -> f64 {
    match tier {
        Tier::Hk => 0.80,
        Tier::So => 3.00,
        Tier::Op => 15.00,
    }
}

/// USD-per-1M-output-tokens for the chosen tier (April 2026 list-prices).
pub(crate) fn output_rate_per_mtok(tier: Tier) -> f64 {
    match tier {
        Tier::Hk => 4.00,
        Tier::So => 15.00,
        Tier::Op => 75.00,
    }
}

/// Compute estimated USD cost from token counts and tier.
pub(crate) fn estimate_cost_usd(tier: Tier, input_tokens: u32, output_tokens: u32) -> f64 {
    let input = (input_tokens as f64) * input_rate_per_mtok(tier) / 1_000_000.0;
    let output = (output_tokens as f64) * output_rate_per_mtok(tier) / 1_000_000.0;
    input + output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_haiku_small() {
        // 1500 input + 400 output @ HK rates
        let c = estimate_cost_usd(Tier::Hk, 1500, 400);
        // 1500 * 0.80 / 1e6 = 0.0012
        // 400  * 4.00 / 1e6 = 0.0016
        // total = 0.0028
        assert!((c - 0.0028).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn cost_opus_large() {
        // 12000 input + 3500 output @ OP rates
        let c = estimate_cost_usd(Tier::Op, 12_000, 3_500);
        // 12000 * 15  / 1e6 = 0.18
        // 3500  * 75  / 1e6 = 0.2625
        // total = 0.4425
        assert!((c - 0.4425).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn cost_zero_tokens_is_zero() {
        for t in [Tier::Hk, Tier::So, Tier::Op] {
            assert_eq!(estimate_cost_usd(t, 0, 0), 0.0);
        }
    }
}
