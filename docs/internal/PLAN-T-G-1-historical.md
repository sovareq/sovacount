> **HISTORISCH DOCUMENT** — initiële implementation-plan van 30 april 2026.
> Bewaard voor archief; weerspiegelt NIET de huidige codebase-staat.
> Voor actuele documentatie: zie [`../../README.md`](../../README.md) en
> [`../../CHANGELOG.md`](../../CHANGELOG.md).

---

# T-G-1 — Token Governor — Implementation Plan

**Tranche-ID:** T-G-1
**Branch:** `wt/T-G-1-initial-impl`
**Eigenaar (orchestrator):** Claude (Opus 4.7), namens Bjorn Lambrechts
**Datum:** 30 april 2026
**Doel:** Eerste publieke open-source release van Sovareq BV — agent-agnostic
LLM-routing classifier (CLI + HTTP + MCP).

---

## 1. Probleemstelling

Bestaande tiered-routing-tools (TokenMix.ai, Morph Router, MintMCP, claude-router)
classificeren prompts zonder kennis van de **tranche** of het **SSOT**. Sovareq's
internal protocol gebruikt @op/@so/@hk-tags op tranche-niveau (zie
`~/Sovareq/_workspace/conventions/token-governor-tags.md`). Geen bestaande tool
combineert: tranche-aware + SSOT-readend + agent-agnostic + drie integratie-paden.

**Token Governor sluit dat gat** — en wordt als eerste publiek geopend.

## 2. Architectuur (high-level)

```
                 ┌──────────────────────────────────────────────┐
                 │         governor-core (library crate)        │
                 │                                              │
                 │   types ── classifier ── cache ── config     │
                 │     │          │           │                 │
                 │     │   ┌──────┴──────┐    │                 │
                 │     │   │  Provider   │    └─ ~/.cache/...   │
                 │     │   │   trait     │                      │
                 │     │   └──────┬──────┘                      │
                 │     │          │                             │
                 │     │   ┌──────┼─────┬─────┬───────┐         │
                 │     │   │      │     │     │       │         │
                 │     │  Anth   OAI  Ollama  Mock  Custom      │
                 │     │                                        │
                 │     └─ heuristic (LOC/files-based fallback)  │
                 │                                              │
                 └──────────────┬───────────────────────────────┘
                                │
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                 ▼
    ┌──────────────────┐ ┌─────────────┐ ┌─────────────────┐
    │  governor-cli    │ │ governor-   │ │  governor-mcp   │
    │  (binary)        │ │  http       │ │   (binary)      │
    │                  │ │  (binary)   │ │                 │
    │  tier-classify   │ │  axum       │ │  rmcp + stdio   │
    └──────────────────┘ └─────────────┘ └─────────────────┘
         │                    │                     │
         ▼                    ▼                     ▼
    bash, codex,         curl, claude-       Claude Code/Desktop,
    cursor, makefile     code-shim, ...      Codex with MCP
```

**Trait + impls in core, three thin binary frontends.** Adding a new integration
(e.g. gRPC) = nieuwe binary crate die `governor-core` consumeert. Geen
vendor-lock op één frontend.

## 3. Technische beslissingen (research-onderbouwd)

| Beslissing | Keuze | Reden |
|---|---|---|
| Taal | Rust 2024 edition, MSRV 1.94 | Single static binary, geen runtime-deps. Match CLAUDE.md §D. |
| MCP SDK | `rmcp` 0.x (officieel) | 4.7M downloads, official Anthropic-aligned, stdio-transport built-in. |
| HTTP server | `axum` + `tokio` | De-facto standaard, JSON-vriendelijk via serde. |
| HTTP client (provider calls) | `reqwest` (rustls) | Geen system-openssl-dep. |
| CLI parsing | `clap` v4 | Industry standard, derive-API. |
| Config-paths | `dirs` crate | XDG correct op Linux, `~/Library/Caches/` op macOS. |
| Hashing (cache) | `sha2` | RustCrypto, no-std mogelijk. |
| Errors | `thiserror` (lib) + `anyhow` (bins) | Standard split. |
| Logging | `tracing` + `tracing-subscriber` | Async-aware. |
| Provider-abstractie | **In-house dunne `Provider` trait** | Alternatief was `genai`/`llm-connector` crates — afgewezen wegens vendor-lock op een specifieke abstractie-set. Onze 3 providers passen elk in <150 LOC. |
| Release-pipeline | `cargo-dist` + GH Actions | 2026-standaard voor cross-platform binaries. |
| Models per tier (Anthropic) | `@hk`→`claude-haiku-4-5`, `@so`→`claude-sonnet-4-6`, `@op`→`claude-opus-4-7` | Per huidige model-IDs (april 2026). User kan overriden via `mapping.toml`. |
| Default classifier-model | `claude-opus-4-7` | Best classification quality; cost-recovery is enorm via correcte routing. Configurable. |

## 4. Workspace-layout

```
token-governor/
├── PLAN.md                              <- dit bestand
├── README.md                            <- value-prop + quickstart
├── CONTRIBUTING.md
├── CHANGELOG.md                         <- Keep-a-Changelog
├── LICENSE                              <- MIT
├── .env.example
├── .gitignore
├── Cargo.toml                           <- workspace root (resolver=2)
├── rust-toolchain.toml                  <- 1.94+ pin
├── rustfmt.toml
├── deny.toml                            <- cargo-deny allowlist
├── crates/
│   ├── governor-core/                   <- WORKER A
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs                   <- public API re-exports
│   │   │   ├── types.rs                 <- ClassifyRequest, ClassifyResponse, Tier
│   │   │   ├── error.rs                 <- GovernorError
│   │   │   ├── classifier.rs            <- core engine: cache → heuristic → LLM
│   │   │   ├── heuristic.rs             <- rule-based fallback
│   │   │   ├── cache.rs                 <- file-based, sha2, TTL
│   │   │   ├── config.rs                <- env-vars + mapping.toml
│   │   │   ├── prompt.rs                <- include_str! classifier-prompt.md
│   │   │   ├── prompts/
│   │   │   │   └── classifier.md        <- compile-time embedded
│   │   │   └── providers/
│   │   │       ├── mod.rs               <- Provider trait + factory
│   │   │       ├── anthropic.rs
│   │   │       ├── openai.rs
│   │   │       ├── ollama.rs
│   │   │       └── mock.rs              <- deterministic, for tests
│   │   └── tests/
│   │       └── integration.rs           <- mock-provider end-to-end
│   ├── governor-cli/                    <- WORKER B
│   │   ├── Cargo.toml
│   │   └── src/main.rs                  <- `tier-classify` binary
│   ├── governor-http/                   <- WORKER C
│   │   ├── Cargo.toml
│   │   └── src/main.rs                  <- `governor-http` binary on :8989
│   └── governor-mcp/                    <- WORKER D
│       ├── Cargo.toml
│       └── src/main.rs                  <- `governor-mcp` rmcp/stdio binary
├── examples/
│   ├── claude-code-integration.md
│   ├── codex-integration.md
│   ├── cursor-integration.md
│   └── bash-precommit-hook.sh
├── .github/
│   └── workflows/
│       ├── ci.yml                       <- build/test/clippy/fmt/deny
│       └── release.yml                  <- cargo-dist binaries to GH Releases
└── tests/                               <- workspace-level smoke tests
```

## 5. File-boundaries (no-overlap-bewijs voor fan-out)

**Per CLAUDE.md §B**: fan-out vereist non-overlapping file-boundaries.

| Worker | Crate | Mag schrijven | Mag NIET aanraken |
|---|---|---|---|
| A | `governor-core` | `crates/governor-core/**` | alle andere crates, root, `.github/`, `examples/` |
| B | `governor-cli` | `crates/governor-cli/**` | alle andere crates, root, `.github/`, `examples/` |
| C | `governor-http` | `crates/governor-http/**` | alle andere crates, root, `.github/`, `examples/` |
| D | `governor-mcp` | `crates/governor-mcp/**` | alle andere crates, root, `.github/`, `examples/` |
| Orchestrator (sequentieel) | — | `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`, `.gitignore`, `.env.example`, `LICENSE`, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `examples/**`, `.github/workflows/**`, `tests/**` | crates/** (workers exclusief) |

**Shared API contract**: vóór fan-out schrijft de orchestrator `governor-core/src/types.rs`
**+** een minimale `governor-core/src/lib.rs` die de publieke types re-exporteert.
Workers B/C/D kunnen dan tegen een stabiele API compileren terwijl A de body
implementeert.

**Conflict-strategie**: geen overlap → geen merge-conflicts verwacht. Bij
incidentele edge-case (bv. shared dep-version): orchestrator beslist via Cargo
workspace-deps in root `Cargo.toml`.

## 6. Tijdslijn

| Fase | Eigenaar | Schatting | Output |
|---|---|---|---|
| 0. Bootstrap (DONE) | orchestrator | 30 min | `PLAN.md`, repo skeleton, conventions, research |
| 1. Pre-fan-out: types.rs, workspace Cargo.toml, configs, LICENSE, .env.example | orchestrator | 30 min | shared-API contract gefixeerd |
| 2. **Fan-out parallel** (A+B+C+D) | 4× sub-Task | ~1.5 uur wallclock (parallel) | 4 crates compilable, unit-tests groen |
| 3. Integration: README, CI, examples, CHANGELOG | orchestrator | 45 min | volledige repo klaar |
| 4. VERIFY: 6 gates lokaal | orchestrator (`verify` agent) | 20 min | groen-rapport |
| 5. PAUSE: Bjorn-confirmatie publieke repo | — | — | go-ahead |
| 6. Public push + PR | orchestrator | 15 min | PR-link + eindrapport |

**Totaal**: ~4 uur (binnen budget van 4-6).

## 7. Gates (CLAUDE.md §A.4)

| Gate | Command | Owner | Pass-criterium |
|---|---|---|---|
| build | `cargo build --release --workspace` | verify | exit 0, geen warnings |
| test | `cargo test --workspace --no-fail-fast` | verify | alle tests groen incl. mock-provider end-to-end |
| lint | `cargo clippy --all-targets --all-features -- -D warnings` | verify | 0 warnings |
| format | `cargo fmt --check` | verify | 0 diff |
| dep-audit | `cargo deny check` | verify | geen unfree/insecure deps |
| SSOT-trace | manual: tag-conventie consistent met `token-governor-tags.md` | review | matched |

Bonus (optioneel pre-merge):
- Integration smoke: `cargo run -p governor-cli -- --task "test scope" --provider mock --format json`
- HTTP smoke: spin server, `curl localhost:8989/classify`
- MCP smoke: `governor-mcp` stdio-handshake test

## 8. Stop-condities (CLAUDE.md §F)

- `cargo build` faalt na 3 fix-pogingen → STOP, blocker-rapport in PR
- Port 8989 conflict → fallback naar willekeurige in 8980-8999, documenteren
- Provider-API onbereikbaar tijdens dev → mock-provider (default)
- `gh repo create --public` zou onomkeerbare publieke exposure veroorzaken
  → **expliciete go-ahead van Bjorn vereist** (verplicht pauze-moment)

## 9. Security & governance pre-checks

- `#![forbid(unsafe_code)]` in elke crate
- Geen hardcoded API-keys in code; `.env.example` met dummy waarden
- `cargo-deny` allowlist zal third-party crates expliciet whitelisten
- Provider-API-keys NOOIT loggen (filtered tracing-layer in `core/src/lib.rs`)
- Cache stores **input-hash + output**, geen plain-text API-keys
- HTTP-server bindt default op `127.0.0.1` (niet `0.0.0.0`)

## 10. Vendor-onafhankelijkheid (CLAUDE.md §A.7)

Het runtime-product is fully vendor-agnostic:
- Provider via env-var (`GOVERNOR_PROVIDER=anthropic|openai|ollama|custom`)
- `mapping.toml` user-overridable per tier per provider
- Classifier-prompt user-overridable
- MCP-adapter optioneel; CLI/HTTP zijn alternatief
- Geen Anthropic-SDK in productie — alleen `reqwest` + JSON

Build-time mag MCP-tooling van Anthropic gebruiken; productie blijft
agent-agnostic.

## 11. Volgende stap

Implement Phase 1 sequentially (types.rs, workspace Cargo.toml, configs, LICENSE), dan
Phase 2 fan-out via 4 parallel `Agent` calls.
