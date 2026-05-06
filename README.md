# Token Governor

> **Cost-optimizing classifier for AI-agent tasks. Tags work with `@op` / `@so` / `@hk` so cheaper models handle simpler work.**

> **Externe naam:** SovaCount. De interne module-naam is `token-governor` /
> `governor-*` en wordt zo behouden in alle Rust-crates en binary-namen.

Most agentic tools call the same expensive model for every task — even when the
work is a typo-fix that any cheap model could handle. Token Governor sits in
front of an agent and decides, *per task*, whether `@op` (Opus-class), `@so`
(Sonnet-class) or `@hk` (Haiku-class) is enough.

It is **agent-agnostic by design**. It does not assume Anthropic, Claude Code,
or any specific runtime. There are three integration paths — pick whichever
matches your stack:

| Path | When to use it |
|---|---|
| **CLI** (`tier-classify`) | Pre-commit hooks, `Makefile` targets, CI scripts, any shell pipeline. |
| **HTTP API** | Codex, web-IDE plugins, custom agents written in any language. |
| **MCP server** | Claude Code / Claude Desktop / Cursor — anything that speaks the [Model Context Protocol](https://modelcontextprotocol.io/). |

All three share the same classifier engine. Switching providers (Anthropic,
OpenAI, Ollama, custom OpenAI-compatible endpoints) is one environment
variable.

## Why this exists

In April 2026 the established tiered-routers (TokenMix.ai, Morph Router,
MintMCP, claude-router) all classify *prompts* — they look at the user message
in isolation. Token Governor classifies *tranches*: a unit of work described
in markdown, optionally with references to architecture documents (the
"SSOT"). That extra signal lets it route confidently *before* an agent burns
tokens exploring the task.

## Architecture

```
    ┌──────────────────────────────────────────────────┐
    │              governor-core (lib)                 │
    │                                                  │
    │  cache → heuristic fast-path → provider call     │
    │     │           │                  │             │
    │     │           │           ┌──────┴──────┐      │
    │     │           │           │  Provider   │      │
    │     │           │           │   trait     │      │
    │     │           │           └──────┬──────┘      │
    │     │           │                  │             │
    │     │           │           ┌──────┼──────┬─────┐│
    │     │           │           │      │      │     ││
    │     │           │       Anthropic OpenAI Ollama Mock
    │     │           │
    │     └───────────┴── XDG-cached on disk           │
    │                                                  │
    └────────────────────┬─────────────────────────────┘
                         │
       ┌─────────────────┼─────────────────┐
       ▼                 ▼                 ▼
  governor-cli      governor-http     governor-mcp
  (tier-classify)   (axum :8989)      (rmcp/stdio)
       │                 │                 │
       ▼                 ▼                 ▼
  bash, codex,      curl, codex       Claude Code / Desktop,
  cursor, makefile  custom agents     Cursor, Codex w/ MCP
```

## Install

### From source (today)

Requires Rust 1.94+ (`rustup install stable`).

```bash
git clone https://github.com/sovareq/token-governor.git
cd token-governor
cargo install --path crates/governor-cli      # tier-classify
cargo install --path crates/governor-http     # governor-http
cargo install --path crates/governor-mcp      # governor-mcp
```

Pre-built binaries for `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`
are attached to GitHub releases.

## Quickstart — CLI

Classify a task inline:

```bash
$ tier-classify --task "Fix typo in README.md" --loc-est 5 --files-est 1
{"tier":"hk","model_hint":"claude-haiku-4-5","complexity":"trivial",...}
```

Just the tag, for shell pipelines:

```bash
$ TIER=$(tier-classify --task "Add new audit endpoint" --loc-est 250 --files-est 3 --format oneline)
$ echo "Routing this work to: $TIER"
Routing this work to: @so
```

From a file or stdin:

```bash
$ tier-classify --scope task.md --ssot ssot/constitution.md,ssot/contracts.md
$ cat scope.md | tier-classify --stdin --format pretty
```

## Quickstart — HTTP

```bash
$ governor-http --bind 127.0.0.1:8989 &
governor-http listening on http://127.0.0.1:8989 (auth=off, provider=mock)

$ curl -s -X POST http://localhost:8989/classify \
    -H 'Content-Type: application/json' \
    -d '{"task_id":"T-G-2","scope_md":"Bootstrap auth layer","estimated_loc":600,"estimated_files":8}' | jq
```

Optional Bearer-token auth:

```bash
$ GOVERNOR_HTTP_API_KEY=secret-key governor-http &
$ curl -H "Authorization: Bearer secret-key" http://localhost:8989/classify ...
```

## Quickstart — MCP (Claude Code / Desktop / Cursor)

Add to your MCP host config (e.g. `~/.config/claude/mcp.json` or the equivalent
for your editor):

```json
{
  "mcpServers": {
    "token-governor": {
      "command": "governor-mcp",
      "env": {
        "GOVERNOR_PROVIDER": "anthropic",
        "GOVERNOR_API_KEY": "sk-ant-..."
      }
    }
  }
}
```

Restart your client. The `governor_classify` tool appears alongside the
built-ins. See [`examples/`](examples/) for one-page setup guides per host.

## Configuration

All configuration is via environment variables. `.env` files are loaded by
the binaries' shells, not by Token Governor itself.

| Env-var | Purpose | Default |
|---|---|---|
| `GOVERNOR_PROVIDER` | `anthropic` \| `openai` \| `ollama` \| `mock` \| `custom` | `anthropic` if `GOVERNOR_API_KEY` set, else `mock` |
| `GOVERNOR_API_KEY` | Bearer-token / x-api-key for the provider | none |
| `GOVERNOR_BASE_URL` | Override the provider endpoint (required for `custom`) | per-provider |
| `GOVERNOR_CLASSIFIER_MODEL` | Which model classifies (its own LLM call) | `claude-opus-4-7` / `o1` / `deepseek-r1:70b` |
| `GOVERNOR_MAPPING_FILE` | TOML overriding the per-tier model mapping | `~/.config/token-governor/mapping.toml` if present |
| `GOVERNOR_CACHE_DIR` | Where to cache responses | `$XDG_CACHE_HOME/token-governor` |
| `GOVERNOR_CACHE_TTL_DAYS` | Cache TTL | `30` |
| `GOVERNOR_CLASSIFIER_PROMPT_FILE` | Override the embedded system prompt | none (uses built-in) |
| `GOVERNOR_HTTP_BIND` | Bind address for `governor-http` | `127.0.0.1:8989` |
| `GOVERNOR_HTTP_API_KEY` | Optional Bearer-token requirement | unset (no auth) |

### Tier mapping

A `mapping.toml` under `~/.config/token-governor/` can override the model
that each tier resolves to:

```toml
[mapping]
op = "claude-opus-4-7"
so = "claude-sonnet-4-6"
hk = "claude-haiku-4-5"
```

Default mappings per provider:

| Provider | `@hk` | `@so` | `@op` |
|---|---|---|---|
| Anthropic | `claude-haiku-4-5` | `claude-sonnet-4-6` | `claude-opus-4-7` |
| OpenAI | `gpt-4o-mini` | `gpt-4o` | `o1` |
| Ollama | `llama3.2:3b` | `llama3.3:70b` | `deepseek-r1:70b` |

## Tag convention

The `@op` / `@so` / `@hk` (and `@auto`) tags are part of a wider
Sovareq tag-convention;
they may also be written long-form (`@opus` / `@sonnet` / `@haiku`).

## Roadmap

- [ ] V0.1.0 release with binaries via cargo-dist
- [ ] Brew tap for one-line install
- [ ] Streaming-classifier mode (large-batch tranche-files)
- [x] Built-in cost-tracker dashboard (`governor-http /cost` + `/` UI)
- [ ] Native Python and TypeScript SDKs that wrap the HTTP API
- [ ] First-class Codex / Cursor / Continue.dev plugins

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for dev setup, build/test commands and
the PR checklist. Forbid-unsafe-code, MIT-licensed, MSRV 1.94.

Built by [Sovareq BV](https://sovareq.example) · [@sovareq](https://github.com/sovareq).
