# Contributing to Token Governor

Thanks for considering a contribution. This document describes the dev
workflow, the bar for code quality, and how to add a new provider.

## Dev setup

Token Governor is a Rust 2024 workspace, MSRV 1.94. The toolchain is pinned
in `rust-toolchain.toml`; rustup will pick it up automatically.

```bash
# Clone and build (debug)
git clone https://github.com/sovareq/token-governor.git
cd token-governor
cargo build --workspace

# Or: install the three binaries on your $PATH
cargo install --path crates/governor-cli      # tier-classify
cargo install --path crates/governor-http
cargo install --path crates/governor-mcp
```

You'll also want `cargo-deny` for the supply-chain gate:

```bash
cargo install cargo-deny --locked
```

## Workspace layout

```
crates/
├── governor-core/   # library — classifier engine, provider abstraction, cache
├── governor-cli/    # tier-classify binary (clap)
├── governor-http/   # governor-http binary (axum)
└── governor-mcp/    # governor-mcp binary (rmcp / stdio MCP)
```

`governor-core` is the only non-binary crate. Frontends are thin: they parse
input, hand a `ClassifyRequest` to the core `Classifier`, and format the
`ClassifyResponse`.

## Quality gates

All six must pass on every PR. CI (.github/workflows/ci.yml) enforces them.

```bash
cargo build --workspace --release
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo deny check
```

The sixth gate is manual: keep the `@op`/`@so`/`@hk` tag-convention in sync
between `crates/governor-core/src/prompts/classifier.md` and any
documentation that references it.

### Code style

- `#![forbid(unsafe_code)]` everywhere — non-negotiable.
- `#![warn(missing_docs)]` is on in `governor-core`. Public items need `///` docs.
- Errors: `thiserror` for libraries, `anyhow` for binaries.
- Tracing: `tracing::info_span!` per logical request. **Never** log API keys
  or full payloads that include keys. Use `#[instrument(skip(...))]` to be safe.
- One reqwest::Client per provider, reused across calls. Built with explicit
  timeouts (`60s` request, `10s` connect).

### Tests

- Inline `#[cfg(test)] mod tests` for unit tests close to the code.
- `tests/` directory for integration tests that exercise the full stack with
  the mock provider.
- HTTP-provider tests use [`wiremock`](https://crates.io/crates/wiremock).
- `cargo test --workspace` should be deterministic (no real network).

## Adding a new provider

The `Provider` trait is intentionally tiny:

```rust
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn classify_raw(
        &self,
        system_prompt: &str,
        user_payload: &str,
    ) -> Result<String>;

    fn name(&self) -> &'static str;
}
```

To add a backend:

1. Create `crates/governor-core/src/providers/<name>.rs`.
2. Implement `Provider` for a struct that wraps `reqwest::Client`, the
   API key, the base URL, and the model id.
3. Wire it up in `providers::build()` and add a `ProviderKind::<Name>` variant
   in `config::ProviderKind`.
4. Add default tier-mapping entries and a default classifier-model in
   `config::default_tier_mapping` / `default_classifier_model`.
5. Update README's configuration table and the `.env.example`.
6. Write a `wiremock`-driven test asserting the request shape and response parse.

## Pull-request checklist

- [ ] All six gates pass locally (`fmt / build / test / clippy / deny / SSOT`).
- [ ] New public items have `///` docs.
- [ ] No new third-party dependency without a one-line justification.
- [ ] No `unwrap()` / `expect()` on user-controlled paths.
- [ ] Updated `CHANGELOG.md` under `[Unreleased]` if user-visible.
- [ ] Updated relevant doc (README / examples / mapping defaults).

## Reporting issues

GitHub issues are the right place. For security-sensitive findings, please
follow [SECURITY.md](SECURITY.md) (TBD; for now email
`bjorn@sovareq.com`).
