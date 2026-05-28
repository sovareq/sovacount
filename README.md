# SovaCount

> **Cost-optimizing classifier for AI-agent tasks. Tags work with `@op` / `@so` / `@hk` so cheaper models handle simpler work.**

> **Externe naam:** SovaCount. De interne module-naam is `token-governor` /
> `governor-*` en wordt zo behouden in alle Rust-crates en binary-namen.

SovaCount staat vóór een agentic-runtime (Claude Code, Codex, custom MCP-client, …) en beslist per taak welk model-tier nodig is: `@op` (Opus-class), `@so` (Sonnet-class) of `@hk` (Haiku-class). De agent draait vervolgens op het gekozen tier ipv altijd op het duurste model.

Drie integratie-paden, één engine:

| Pad | Wanneer gebruiken |
|---|---|
| **CLI** (`tier-classify`) | Pre-commit hooks, `Makefile`-targets, shell-scripts, CI-pipelines. |
| **HTTP API** (`governor-http`, axum :8989) | Web-IDE plugins, custom agents in elke taal, Claude Code via een wrapper-script. |
| **MCP server** (`governor-mcp`, stdio) | Claude Code / Claude Desktop / Cursor — alles dat het [Model Context Protocol](https://modelcontextprotocol.io/) spreekt. |

Daarnaast: **`governor-launcher-gui`** is een native wry+tao desktop-shell (macOS `.app`) die de HTTP-server start/stopt en het dashboard linkt — handig voor operators die de classifier visueel willen monitoren.

## Waarom dit bestaat

In april 2026 classifeerden de gangbare tiered-routers (TokenMix.ai, Morph Router, MintMCP, claude-router) op **prompt-niveau** — ze keken naar het user-bericht in isolatie. SovaCount classifieert op **tranche-niveau**: een eenheid werk beschreven in markdown, optioneel met SSOT-referenties. Dat extra signaal laat het tier-besluit nemen *vóór* de agent tokens uitgeeft aan verkenning.

## Architectuur

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
    │     │           │      ┌───────┬───┴───┬───────┐ │
    │     │           │      │       │       │       │ │
    │     │           │   Anthropic OpenAI Ollama  Mock │
    │     │           │   (& Custom OpenAI-compatible)│
    │     │           │                                │
    │     └───────────┴── persistent cache on disk     │
    │                                                  │
    └────┬─────────────┬─────────────┬──────────────┬──┘
         ▼             ▼             ▼              ▼
    governor-cli  governor-http  governor-mcp  governor-launcher-gui
    (tier-classify) (axum :8989)  (rmcp/stdio)  (desktop UI shim, macOS)
```

**Vijf workspace-crates** (zie [`Cargo.toml`](Cargo.toml) members):
- `crates/governor-core` — library: `Classifier`, `Config`, `Cache`, `Cost`, providers, heuristic
- `crates/governor-cli` — binary `tier-classify` (CLI)
- `crates/governor-http` — binary `governor-http` (HTTP-server + dashboard)
- `crates/governor-mcp` — binary `governor-mcp` (MCP-stdio-server, tool `governor_classify`)
- `crates/governor-launcher-gui` — binary `sovacount-launcher` (wry+tao desktop launcher)

## Install

### Vanuit source

```bash
git clone https://github.com/sovareq/sovacount.git
cd sovacount
cargo build --release --workspace

# Installeer alle binaries in PATH
mkdir -p ~/.local/bin
cp target/release/{tier-classify,governor-http,governor-mcp,sovacount-launcher} ~/.local/bin/
```

Cargo workspace is `edition = "2024"`, `rust-version = "1.94"` (zie [`Cargo.toml`](Cargo.toml) workspace.package). Toolchain-pin in [`rust-toolchain.toml`](rust-toolchain.toml).

Alle product-crates dragen `#![forbid(unsafe_code)]` — alleen `governor-launcher-gui` heeft één `unsafe` block voor de `libc::kill(pid, SIGTERM)` syscall (gedocumenteerd in [`main.rs`](crates/governor-launcher-gui/src/main.rs)).

### macOS — `.app`-launcher bundelen

De `LAUNCHER/SovaCount.app/Contents/MacOS/sovacount-launcher` in de repo is een
ontwikkel-stub. Voor een werkende `.app` gebruik je het package-script:

```bash
# Ad-hoc signing (eigen Mac, geen Apple Account nodig)
./scripts/package-macos.sh

# Verplaats naar Applications/
cp -R dist/SovaCount.app ~/Applications/

# Eerste run zonder quarantine-prompt (gebouwd op dezelfde Mac):
open ~/Applications/SovaCount.app
```

Het script bouwt `governor-http` + `sovacount-launcher`, kopieert ze naar
`dist/SovaCount.app/Contents/{MacOS,Resources}/`, en doet `codesign`. De
launcher vindt `governor-http` automatisch in `Contents/Resources/` —
geen extra `~/.local/bin/` install nodig voor de `.app`-flow.

Voor distributie naar andere Macs (Developer ID + notarization), zie
[`docs/distribution.md`](docs/distribution.md).

## Quickstart — CLI (`tier-classify`)

```bash
# Inline scope, geen LLM-call (heuristic fast-path beslist)
tier-classify --task "Fix typo in README"
# → {"tier":"hk","model_hint":"claude-haiku-4-5", …}

# Scope uit file + SSOT-referenties (geeft de classifier meer context)
tier-classify --scope ./tranches/T-MERGE-04.md --ssot ./ssot/contracts.md,./ssot/threat_model.md

# Stdin
echo "Refactor multi-tenant audit-chain" | tier-classify --stdin --loc-est 850 --files-est 12

# Output-formaat: json (default) / yaml / oneline / pretty
tier-classify --task "Fix typo" --format oneline
# → @hk
```

### CLI-flags (compleet)

| Flag | Default | Doel |
|---|---|---|
| `--task <TEXT>` | — | Inline scope-text. Mutually exclusive met `--scope` en `--stdin`. |
| `--scope <FILE>` | — | Lees scope uit markdown-file. |
| `--stdin` | — | Lees scope van stdin. |
| `--ssot <COMMA_LIST>` | `""` | Komma-separated SSOT-paden; whitespace rond komma's wordt getrimd. |
| `--task-id <STR>` | `cli-<unix-timestamp>` | Override taak-identifier (wordt in cache-key gebruikt). |
| `--loc-est <N>` | — | Geschatte LOC; feed voor heuristic. |
| `--files-est <N>` | — | Geschat aantal files; feed voor heuristic. |
| `--no-cache` | off | Skip cache-lookup, dwing verse classify-call. |
| `--provider <NAME>` | per env / default | Runtime override: `anthropic` \| `openai` \| `ollama` \| `mock` \| `custom`. |
| `--shift <N>` | (read from disk) | Eénmalige tier-shift voor deze call: `-2..=2` (`-1` = goedkoper, `+1` = duurder). Persistente waarde via `/shift` HTTP-endpoint. |
| `--format <FMT>` | `json` | Output-formaat: `json` \| `yaml` \| `oneline` \| `pretty`. |
| `-v` / `-vv` | warn | Verbosity. `-v` = info, `-vv` = debug. |

Volledig hulp-overzicht: `tier-classify --help`.

## Quickstart — HTTP (`governor-http`)

```bash
# Start (default = mock provider, geen API-key nodig)
governor-http
# → governor-http listening on http://127.0.0.1:8989 (auth=off, provider=mock)

# Met Anthropic provider
GOVERNOR_PROVIDER=anthropic GOVERNOR_API_KEY=sk-ant-... governor-http

# Classify-call
curl -X POST http://127.0.0.1:8989/classify \
  -H "Content-Type: application/json" \
  -d '{
    "task_id":"my-task-1",
    "scope_md":"Refactor audit-chain for multi-tenant isolation",
    "estimated_loc":850,
    "estimated_files":12,
    "ssot_refs":["ssot/threat_model.md"]
  }'
```

### HTTP-endpoints (compleet)

| Method | Path | Beschrijving |
|---|---|---|
| `GET` | `/` | HTML dashboard met live cost-tracking + classify-panel + live-feed |
| `GET` | `/health` | `{"status":"ok","version":"0.1.0"}` health-probe |
| `POST` | `/classify` | Classify-call met `ClassifyRequest`-body, returnt `ClassifyResponse` |
| `GET` | `/cost` | Aggregated `CostReport` (per_tier + per_day + totals + savings vs alles-Opus) |
| `GET` | `/shift` | Lees persistente gear-lever-waarde (`{"value": -2..=2}`) |
| `POST` | `/shift` | Zet persistente gear-lever (`{"value": -2..=2}`) |
| `GET` | `/recent` | Laatste 20 classify-responses, gesorteerd op mtime DESC (voor dashboard live-feed) |
| `POST` | `/reset` | Wis alle `.json`-bestanden uit de classifier-cache. Returnt `{"deleted_files": N, "cache_dir": "..."}` |
| `GET` | `/governor/state` | Lees routing-toggle (`{"enabled": bool}`). Absent state-file = `true` (fail-open). |
| `POST` | `/governor/state` | Zet routing-toggle (`{"enabled": bool}`). Bij `false` retourneert `/classify` 503 `"governor disabled"`. Gepersisteerd naar `~/.config/token-governor/enabled`. Auth-gated als API-key gezet. |
| `GET` | `/display/mode` | Lees dashboard display-mode preference (`{"mode": "usd"\|"tokens"\|"auto", "effective": "usd"\|"tokens", "oauth_detected": bool}`). `effective` resolveert `auto` op basis van OAuth-detectie in `~/.claude/projects/`. |
| `POST` | `/display/mode` | Zet display-mode (`{"mode": "usd"\|"tokens"\|"auto"}`). Onbekende mode → HTTP 400. Gepersisteerd naar `~/.config/token-governor/display_mode`. Auth-gated als API-key gezet. |

### Auth (optioneel)

Zet `GOVERNOR_HTTP_API_KEY` om Bearer-auth te activeren:

```bash
GOVERNOR_HTTP_API_KEY=secret-token-here governor-http
# Daarna in alle calls:
curl -H "Authorization: Bearer secret-token-here" http://127.0.0.1:8989/cost
```

Empty-string of unset = auth uit.

## Quickstart — MCP (Claude Code / Desktop / Cursor)

In Claude Code's MCP-config (`~/.claude/settings.json` of vergelijkbaar):

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

MCP-tool wordt geëxposeerd als `governor_classify`. Het accepteert dezelfde fields als de HTTP `/classify`-body en retourneert `ClassifyResponse`.

Voor automatische subagent-routing in Claude Code: combineer met een PreToolUse-hook die `Agent`/`Task` tool-input intercepteert, scope naar `/classify` stuurt, en het `model`-param overschrijft volgens de aanbevolen tier. Zie [`examples/`](examples/) voor wiring-voorbeelden.

## Native launcher GUI (`sovacount-launcher`)

`crates/governor-launcher-gui` is een wry + tao desktop-shell die:

- De HTTP-server als child-process start/stopt (Aanzetten / Uitzetten knop)
- Het dashboard-URL opent in de default browser
- De classify-cache wist (Reset cache, twee-klik bevestiging)
- Live status toont (groen = server up, rood = down)
- Provider-modus aangeeft in de subtitle (`mock provider` of `anthropic provider`)

Bouwt naar `target/release/sovacount-launcher`. Wordt op macOS verpakt als `LAUNCHER/SovaCount.app` (zie [`LAUNCHER/`](LAUNCHER/)).

Bij startup leest de launcher `~/.config/sovacount/anthropic-key` (chmod 600 file) — indien aanwezig: provider=anthropic + API-key uit file. Indien afwezig: provider=mock.

## Dashboard

`GET /` toont een single-file dashboard in Sovareq design-tokens (geen externe CDN, geen woff2-fetch). Secties:

- **Aan/uit-toggle** — pill naast health-badge in header; één klik schakelt routing aan/uit. Server-side state, vendor-agnostisch — elke client erft de toggle zonder per-client config.
- **Bespaard t.o.v. altijd-Opus** — savings-banner (procent + dollar)
- **Per tier** — HAIKU / SONNET / OPUS calls + cumulative spend
- **Tier-shift (gear-lever)** — drie-knops UI om globaal `-1` / `0` / `+1` shift te kiezen (verstuurt naar `/shift`)
- **Cumulatief (totaal)** — Calls / Werkelijk uitgegeven / Baseline (alles Opus) + Reset cache knop
- **Live feed — laatste 20 beslissingen** — auto-refresh elke 2s via `/recent`; tabel met Tijd · Tier-pill · Model-hint · Confidence · Cost · Baseline · Bespaard · Rationale
- **Per dag** — aggregate per UTC-datum (Calls / Kost / Bespaard)
- **Classify een taak** — direct LLM-classify-knop, voor handmatige experimenten

Polling-intervallen: `/cost`/`/shift`/`/health`/`/governor/state` elke 5s; `/recent` elke 2s.

## Configuratie

Alle config via environment variables (gelezen door [`crates/governor-core/src/config.rs`](crates/governor-core/src/config.rs) — `Config::from_env`):

| Env-var | Default | Doel |
|---|---|---|
| `GOVERNOR_PROVIDER` | `anthropic` indien `GOVERNOR_API_KEY` gezet, anders `mock` | Welke LLM-provider gebruikt wordt voor classify-calls die niet via heuristic fast-path beslist worden |
| `GOVERNOR_API_KEY` | (optioneel) | API-key voor de gekozen provider. Niet nodig voor `mock` of `ollama`. |
| `GOVERNOR_BASE_URL` | per-provider | Override base-URL (handig voor proxies of self-hosted endpoints) |
| `GOVERNOR_CLASSIFIER_MODEL` | `claude-opus-4-7` / `o1` / `deepseek-r1:70b` / `mock` | Welk model de classifier zelf gebruikt om tiers te kiezen (niet de te-classificeren werkmodellen) |
| `GOVERNOR_CLASSIFIER_PROMPT_FILE` | embedded default | Override classifier-system-prompt met een file |
| `GOVERNOR_MAPPING_FILE` | `~/.config/token-governor/mapping.toml` (indien bestaat) | TOML met custom tier→model-naam mapping per tier (`[mapping]` met `op`/`so`/`hk` keys) |
| `GOVERNOR_PRICING_FILE` | embedded defaults | TOML met custom per-tier pricing voor cost-report (per-1M-tokens) |
| `GOVERNOR_CACHE_DIR` | `dirs::cache_dir()/token-governor` (Linux: `$XDG_CACHE_HOME/token-governor`, macOS: `~/Library/Caches/token-governor`, Windows: `%LOCALAPPDATA%\token-governor`) | Classify-response cache directory |
| `GOVERNOR_CACHE_TTL_DAYS` | `30` | Hoe lang een gecachede response geldig blijft |
| `GOVERNOR_HTTP_BIND` | `127.0.0.1:8989` | Bind-address voor `governor-http`. Loopback default; flip naar `0.0.0.0:8989` enkel achter reverse-proxy + auth. |
| `GOVERNOR_HTTP_API_KEY` | (unset = auth off) | Bearer-token om HTTP-API achter auth te zetten |
| `CLASSIFY_QUEUE_DEPTH` | `512` | Backpressure-depth voor de HTTP classify-worker queue |
| `RUST_LOG` | `warn` | Standard `tracing-subscriber` filter |

### Tier-mapping (welk model voor welk tier)

Default model-aliassen per provider (zie [`config.rs:283-295`](crates/governor-core/src/config.rs)):

| Tier | Anthropic | OpenAI | Ollama |
|---|---|---|---|
| HK | `claude-haiku-4-5` | `gpt-4o-mini` | `llama3.2:3b` |
| SO | `claude-sonnet-4-6` | `gpt-4o` | `llama3.3:70b` |
| OP | `claude-opus-4-7` | `o1` | `deepseek-r1:70b` |

Custom-provider valt terug op Anthropic-equivalente prijzen voor cost-rekenwerk (zie [`pricing.rs:138`](crates/governor-core/src/pricing.rs)).

Override per project via `mapping.toml`:

```toml
[mapping]
op = "claude-opus-4-6"     # downgrade op naar 4.6 voor budget
so = "claude-sonnet-4-5"
hk = "claude-haiku-4-5"
```

## Heuristic fast-path

Voor scopes met duidelijke signalen beslist [`heuristic.rs`](crates/governor-core/src/heuristic.rs) zonder LLM-call. Drempels:

| Tier | Trigger |
|---|---|
| **HK** | `estimated_loc < 50` AND `estimated_files == 1` AND geen SSOT-refs AND geen architectural-markers in scope-text |
| **OP** | `estimated_loc > 300` OR `estimated_files > 5` OR ≥2 architectural-markers (zoals "refactor", "multi-tenant", "threat model", "SSOT", "audit-chain") |
| **SO** | Alles wat niet door HK of OP-fast-path geclaimd wordt → LLM-classify-call met confidence-score |

Confidence-score: heuristic-pad = 90-99 %, LLM-pad = 50-95 % afhankelijk van scope-eenduidigheid.

## Response-shape

`ClassifyResponse` (zie [`types.rs`](crates/governor-core/src/types.rs)):

```json
{
  "tier": "hk|so|op",
  "model_hint": "claude-haiku-4-5",
  "complexity": "trivial|standard|complex",
  "rationale": "Single typo correction; no architecture or multi-file impact.",
  "confidence": 98,
  "estimated_input_tokens": 800,
  "estimated_output_tokens": 200,
  "estimated_cost_usd": 0.0013,
  "alternative_tiers": [
    {"tier":"so","rationale":"...","extra_cost_usd":0.024}
  ],
  "from_cache": false
}
```

- `complexity`: drie-waardige enum (`trivial` ↔ HK, `standard` ↔ SO, `complex` ↔ OP)
- `alternative_tiers`: leeg voor HK/OP fast-path; bevat 1-2 alternatieven bij LLM-classified SO
- `from_cache`: `true` indien de response uit de cache kwam (niet uit een verse provider-call)

## Cost-tracking

`/cost` endpoint aggregeert alle `.json`-cachebestanden in `GOVERNOR_CACHE_DIR`. Voor elke gecachede response:
- Werkelijke kosten via tier-gerelateerde pricing
- Baseline-kost via altijd-Opus pricing
- Besparing = baseline − actual

Aggregation per tier en per UTC-dag (zie [`cost.rs`](crates/governor-core/src/cost.rs)).

Pricing-defaults (per 1M tokens, mei 2026, zie [`pricing.rs:106-146`](crates/governor-core/src/pricing.rs)):

| Provider | HK input/output | SO input/output | OP input/output |
|---|---|---|---|
| Anthropic | $1 / $5 | $3 / $15 | $5 / $25 |
| OpenAI | $0.15 / $0.60 | $2.50 / $10.00 | $15.00 / $60.00 |
| Ollama | $0 / $0 (lokaal) | $0 / $0 | $0 / $0 |
| Custom | (Anthropic-equivalent) | | |

Override met `GOVERNOR_PRICING_FILE` TOML.

## Gear-lever (`--shift` / `/shift`)

Persistente globale tier-shift, opgeslagen in `~/.config/token-governor/shift`:

| Value | Effect |
|---|---|
| `-2` | Altijd HK (force-Haiku) |
| `-1` | Eén tier omlaag waar mogelijk (SO→HK, OP→SO) |
| `0` | Geen shift (default) |
| `+1` | Eén tier omhoog (HK→SO, SO→OP) |
| `+2` | Altijd OP (force-Opus) |

Per call override via `tier-classify --shift N`. Per session override via `POST /shift {"value": N}` (gepersisteerd voor toekomstige process-starts).

## Tag-conventie

Output-tags die SovaCount produceert + bedoelde gebruik:

| Tag | Tier | Bedoeld voor |
|---|---|---|
| `@hk` | Haiku-class | Triviale taken: typo-fixes, mechanical execution, simpele greps, file-counts |
| `@so` | Sonnet-class | Standard implementation: nieuwe endpoints, feature-werk op bekende patronen, README-checks, code-reviews |
| `@op` | Opus-class | Architectuur: refactors >300 LOC, multi-tenant security, threat-modeling, cross-system migraties |

Een caller (wrapper-script, MCP-hook, ...) leest het tag uit het response-veld `tier` en stuurt het werk naar het overeenkomstige Anthropic/OpenAI/Ollama model.

## Roadmap

- [ ] v0.2.0 release met `cargo-dist` binaries
- [ ] Brew tap voor Mac-install zonder `cargo build`
- [ ] Streaming-classifier mode (long scopes door pipelinen)
- [x] Built-in cost-tracker dashboard `/cost` + `/` UI
- [x] Native desktop launcher (`governor-launcher-gui`, wry+tao)
- [x] Live-feed sectie in dashboard (auto-refresh, `/recent`)
- [x] Reset-cache UI + endpoint
- [ ] Native Python/TypeScript SDKs voor in-process gebruik
- [ ] Codex/Cursor/Continue.dev first-class plugins (verder dan de huidige doc-snippets in `examples/`)

## Contributing

Sovareq-intern. Workspace gates per [`CONTRIBUTING.md`](CONTRIBUTING.md):
- `cargo build --release --workspace`
- `cargo test --workspace --no-fail-fast`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo fmt --check`
- `cargo deny check` (zie [`deny.toml`](deny.toml))

Edition 2024, MSRV 1.94+ (pin in [`rust-toolchain.toml`](rust-toolchain.toml)).

## License

**UNLICENSED** (proprietary, all rights reserved) — `workspace.package.license = "UNLICENSED"` in [`Cargo.toml`](Cargo.toml).

SovaCount is **niet gelicenseerd voor externe redistributie**. Intern Sovareq-gebruik. Klant-toegang vereist een aparte NDA + pilot-overeenkomst; contact <bjorn@sovareq.com>.

---

Built by <bjorn@sovareq.com>.
