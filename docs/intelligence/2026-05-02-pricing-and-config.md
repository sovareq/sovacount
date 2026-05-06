# SovaCount Cost-Aggregate Pricing & Config Research
**Date:** 2 May 2026 | **Window:** 1–7 May 2026

## Anthropic Claude Pricing (per 1M tokens, USD)

| Model | Input | Output |
|-------|-------|--------|
| claude-haiku-4-5 | $1.00 | $5.00 |
| claude-sonnet-4-6 | $3.00 | $15.00 |
| claude-opus-4-7 | $5.00 | $25.00 |

**Source:** [Anthropic Claude Pricing Docs](https://platform.claude.com/docs/en/about-claude/pricing)

## OpenAI Pricing (per 1M tokens, USD)

| Model | Input | Output |
|-------|-------|--------|
| gpt-4o-mini | $0.15 | $0.60 |
| gpt-4o | $2.50 | $10.00 |
| o1 | $15.00 | $60.00 |

**Sources:** 
- [OpenAI API Pricing](https://openai.com/api/pricing/)
- [OpenAI Pricing 2026 Overview](https://www.finout.io/blog/openai-pricing-in-2026)

## Ollama Pricing

**Local:** $0 per token (open-source, MIT license)
**Ollama Cloud:** Fixed subscription ($20/mo Pro, $100/mo Max) for managed inference; local deployment remains free.

**Source:** [Ollama Pricing](https://ollama.com/pricing)

---

## Rust 2024 Config Pattern with Defaults

For TOML config loading with priority (env-var > `~/.config/<app>/config.toml` > compiled-in defaults), use serde + toml (0.8):

```rust
use std::fs;
use std::path::PathBuf;

#[derive(serde::Deserialize, Default)]
struct Config {
    token_limit: u32,
    // ... fields
}

fn load_config() -> Config {
    // 1. Try env var
    if let Ok(path) = std::env::var("MY_CONFIG_FILE") {
        if let Ok(content) = fs::read_to_string(&path) {
            return toml::from_str(&content).unwrap_or_default();
        }
    }
    // 2. Try ~/.config/<app>/config.toml
    if let Ok(home) = std::env::var("HOME") {
        let config_path = PathBuf::from(home)
            .join(".config")
            .join("my-app")
            .join("config.toml");
        if let Ok(content) = fs::read_to_string(config_path) {
            return toml::from_str(&content).unwrap_or_default();
        }
    }
    // 3. Compiled-in defaults
    Config::default()
}
```

This pattern requires no new dependencies; serde derives handle deserialization. Return `Config::default()` at each stage if parsing fails, ensuring silent fallback.

---

**Research completed:** Anthropic + OpenAI official sources verified. Ollama confirmed free locally, subscription-based for cloud. Rust pattern uses only `toml = "0.8"` + `serde` (workspace-level).
