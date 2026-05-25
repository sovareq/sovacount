# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-05-26

### Added
- **`crates/governor-launcher-gui`**: native wry+tao desktop launcher (macOS `.app`).
  Binary `sovacount-launcher`. UI met aan/uit-knop, dashboard-link, reset-cache (twee-klik bevestiging),
  live status-dot (groen=server up), provider-modus label (mock/anthropic).
  Leest `~/.config/sovacount/anthropic-key` (chmod 600) bij startup voor auto-anthropic-provider.
- **HTTP endpoint `POST /reset`**: verwijdert alle `.json` cache-bestanden, retourneert
  `{deleted_files, cache_dir}`. Dashboard counters resetten naar 0.
- **HTTP endpoint `GET /recent`**: laatste 20 ClassifyResponses gesorteerd op mtime DESC,
  voor dashboard live-feed.
- **Dashboard "Live feed" sectie**: laatste 20 beslissingen, auto-refresh elke 2s, tier-pill +
  model-hint + confidence + cost + baseline + bespaard + rationale per row.
- **Dashboard "Reset cache" knop**: in Cumulatief-sectie met confirm-dialog.
- README volledig herschreven na code↔doc cross-check (37 expliciete claims geverifieerd,
  alle PASS na fix; 15 ontbrekende-uit-README features toegevoegd).
- Volledige documentatie van CLI-flags `--shift`, `--provider`, `--no-cache`, `--task-id`.
- Volledige documentatie van endpoints `/health`, `/cost`, `/shift`, `/recent`, `/reset`.
- Gear-lever (`--shift` / `/shift`) sectie met `-2..=2` waarden + persistente opslag.
- Heuristic fast-path drempels gedocumenteerd (HK<50 LOC/1 file, OP>300 LOC/>5 files/2+ markers).
- ClassifyResponse-shape gedocumenteerd: `Complexity` enum, `alternative_tiers`, `from_cache`.
- Pricing-defaults tabel + `GOVERNOR_PRICING_FILE` override documentatie.
- Env-vars `CLASSIFY_QUEUE_DEPTH`, `GOVERNOR_HTTP_API_KEY` toegevoegd aan config-sectie.

### Changed
- **License**: workspace `Cargo.toml` `license = "MIT"` → `"UNLICENSED"` (proprietary, all rights reserved).
  Workspace `publish = false` toegevoegd; alle crates inherit via `publish.workspace = true`.
  `LICENSE`-bestand vervangen door proprietary tekst met NDA-pad voor klant-toegang.
- **Repository-velden** in workspace `Cargo.toml`: `repository`/`homepage` → `codeberg.org/sovareq_bv/sovacount`.
  Author: `Sovareq BV <bjorn@sovareq.com>` → `bjorn@sovareq.com`.
- **Dashboard alignment**: `th.num` + `td.num` beide right-aligned (per-dag tabel kolom-headers
  staan nu onder waarden ipv ernaast).
- **Dashboard footer**: "Built by Sovareq BV" → "Built by bjorn@sovareq.com".
- **cargo-deny config**: `private = { ignore = true }` voor proprietary workspace-crates.
  RUSTSEC ignore-list uitgebreid met gtk-rs/proc-macro-error/fxhash (compile-time deps via
  wry/tao op Linux-pad, geen runtime-impact op macOS WKWebView pad) met rationale.
- Clippy hygiene: `sort_by` → `sort_by_key(Reverse(...))` in `/recent` handler;
  doc-list indentation in classify-handler doc-comment.

### Fixed
- Server status-polling in launcher GUI: 600ms initial-delay zodat webview JS geladen is
  vóór eerste `StatusChanged` event.
- Reset-knop in launcher: twee-klik-bevestiging (wry blokkeert native `confirm()` op macOS).

### Removed
- Stub `parse_png_to_icon` in launcher GUI: icoon-handling volledig via macOS Info.plist
  + `icon.icns` (geen runtime PNG-fallback meer nodig).

### Internal
- `PLAN.md` (T-G-1 initiële tranche-planning, 30 april) verplaatst naar
  `docs/internal/PLAN-T-G-1-historical.md` met historisch-marker.

## [0.2.0] — 2026-05-05

### Added
- Per-provider pricing config: `PricingConfig` met `anthropic`, `openai`, `ollama`, `custom` rate-tabellen.
- `pricing.toml` aan workspace-root met geverifieerde May 2026 list-prices als ship-default.
- `GOVERNOR_PRICING_FILE` env-var voor pricing-override; fallback naar `~/.config/token-governor/pricing.toml`, dan built-in defaults.
- Savings-teller op `GET /cost`: `baseline_opus_usd` en `savings_usd` per tier, per dag en in totals — gebruikt actieve provider's Opus-class rate.
- `ProviderKind::pricing_provider()` mapper voor active-provider-aware kost-berekening.

### Changed
- Dashboard CSS aligned to YuniTrack/Sovaguard design-token system.
- Dashboard: classify-panel added (POST /classify inline in UI).
- Fixed: empty `scope_md` in classify-panel POST payload now correctly
  omitted so server returns HTTP 400 instead of silent bogus-200 pass-through.

## [0.1.0] — 2026-04-30

### Added
- Initial public release.
- `governor-core` library:
  - `Classifier` orchestrates cache lookup → heuristic fast-path → provider call.
  - `Provider` trait with implementations for Anthropic Messages, OpenAI Chat
    Completions, Ollama (`/api/chat`), a deterministic in-process Mock, and a
    Custom OpenAI-compatible endpoint.
  - File-based cache keyed by SHA-256 of canonical input JSON, atomic writes,
    mtime-based TTL (default 30 days).
  - Heuristic fast-path: HK for tiny+single-file+no-SSOT+no-architectural-markers,
    OP for >300 LOC or >5 files or 2+ markers.
  - Tier mapping per provider with optional `mapping.toml` override.
  - Compile-time-embedded classifier system prompt; runtime override supported.
  - `cost::aggregate` walks the on-disk cache and rolls up cumulative spend
    by tier and by UTC day; chrono-free (Howard Hinnant date-from-days).
- `tier-classify` CLI with `--task` / `--scope` / `--stdin` input modes and
  four output formats (`json` / `yaml` / `oneline` / `pretty`).
- `governor-http` axum server with `POST /classify`, `GET /cost`, `GET /health`.
  Optional Bearer-token auth. Loopback-only by default.
- `governor-mcp` stdio MCP server (rmcp 1.5) exposing one tool:
  `governor_classify`. 20 black-box integration tests speak real JSON-RPC
  over stdio against the spawned binary; 20 Python tests under
  `tools/mcp-blackbox/` provide a language-agnostic external sanity check.
- Three-mode router reference (`examples/router.py` + `examples/three-modes.md`)
  documenting `strict` / `light` / `auto` execution patterns.
- Working Claude Code PreToolUse hook (`examples/claude-code/`) with
  fail-open behavior on governor-http unavailability.
- Workspace-level supply-chain gate via `cargo-deny`.
- MIT licence.

### Known upstream issues
- rmcp 1.5 closes the stdio stream cleanly on parse error / UTF-8 BOM
  prefix instead of replying with `-32700 Parse Error`. Tracked at
  [modelcontextprotocol/rust-sdk#825](https://github.com/modelcontextprotocol/rust-sdk/issues/825).
  Pinned in two integration tests so we notice if upstream fixes it.

### Models recommended (defaults)
- Anthropic: `@hk` → `claude-haiku-4-5`, `@so` → `claude-sonnet-4-6`,
  `@op` → `claude-opus-4-7`. Classifier itself runs on `claude-opus-4-7`.
- OpenAI: `@hk` → `gpt-4o-mini`, `@so` → `gpt-4o`, `@op` → `o1`.
  Classifier itself: `o1`.
- Ollama: `@hk` → `llama3.2:3b`, `@so` → `llama3.3:70b`,
  `@op` → `deepseek-r1:70b`. Classifier itself: `deepseek-r1:70b`.

[Unreleased]: https://github.com/sovareq/token-governor/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/sovareq/token-governor/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sovareq/token-governor/releases/tag/v0.1.0
