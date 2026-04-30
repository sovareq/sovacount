# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/brainzzlab-hub/token-governor/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/brainzzlab-hub/token-governor/releases/tag/v0.1.0
