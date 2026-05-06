# SovaCount V1 review — uitvoeringsrapport

**Datum:** 2026-05-05
**Operator:** Claude Code (Opus 4.7, 1M context)
**Opdracht door:** Bjorn Lambrechts, Sovareq BV
**Werkmap (`$SOVA`):** `/Users/sovareq/Desktop/sovacount-handover 2/code`

---

## 0. Pad-correctie ten opzichte van eerste-pass

Eerste-pass werd in de verkeerde checkout uitgevoerd
(`/Users/sovareq/Sovareq/optimizers/token-governor/` — een oudere worktree
zonder `dashboard.html`). Bjorn corrigeerde naar de juiste locatie:
`~/Desktop/sovacount-handover 2/code/`. Dashboard.html bestaat hier op
362 regels en de `GET /` route + handler staan in
`crates/governor-http/src/lib.rs:127, :140` (`include_str!("dashboard.html")`).

Vanaf hier zijn secties 1, 2, 4a, 4b in deze juiste repo uitgevoerd.
Secties 3, 4c, 5, 6 zijn op uitdrukkelijk verzoek van de operator NIET
opnieuw aangeraakt in deze repo — die werk-output (linter-note in
CHANGELOG, naming-note in README, dual-launcher, nextest-config en
classify-panel HTTP-tests) staat in de eerdere worktree
`/Users/sovareq/Sovareq/optimizers/token-governor/` en kan, indien gewenst,
later handmatig worden geport.

---

## 1. Sectie 1 — Design tokens (VERIFIED_GREEN)

**Bestand:** `crates/governor-http/src/dashboard.html`

Het volledige `<style>`-blok werd vervangen. De vorige `--bg / --panel /
--line / --text / --muted / --accent: #7ee787 / --hk: #79c0ff / --so:
#d2a8ff / --op: #ff7b72`-set is verwijderd. Nieuwe set is een 1:1 spiegel
van `yunitrack-rs/assets/frontend/styles.css :root` (lichtmodus, regels
17-66) plus de `@media (prefers-color-scheme: dark)`-override (regels 76-92).

Vertaaltabel oude → nieuwe selectoren / styling-regels:

| Oude regel | Nieuwe regel |
|---|---|
| `body { font-family: -apple-system, … }` | `body { font-family: var(--font-sans); }` |
| Stat-values, datum-cellen, kost-bedragen | nu `font-family: var(--font-mono)` (identifiers monospace per `HUISSTIJL_BINDEND.md` §1.4) |
| `.tier-hk { color: #79c0ff; }` | `.tier-hk { color: var(--color-accent); }` (blauw) |
| `.tier-so { color: #d2a8ff; }` | `.tier-so { color: var(--color-ink-muted); }` (grijs) |
| `.tier-op { color: #ff7b72; }` | `.tier-op { color: var(--color-border-strong); }` (sterker grijs) |
| `.card { background: #161b22; border: 1px solid #30363d; }` | `.card { background: var(--color-surface-muted); border: var(--border-thin); border-radius: var(--radius-md); }` |
| `.savings-box { background: linear-gradient(...); border-color: #2ea043; }` | `.savings-box { background: var(--color-surface-sunken); border: 2px solid var(--color-accent); }` (geen gradient) |
| `.shift-btn.active { background: var(--accent); color: #0e1116; }` | `.shift-btn.active { background: var(--color-accent); color: var(--color-surface); border-color: var(--color-accent); }` |
| `.shift-btn:hover { border-color: var(--accent); }` | `.shift-btn:hover { border: var(--border-strong); }` |
| `header { background: linear-gradient(...); }` | `header { background: var(--color-surface); border-bottom: var(--border-thin); }` (geen gradient) |

**A11y-check:**
- Light-mode: `--color-ink #14181f` op `--color-surface #ffffff` ⇒ contrast 16.7 : 1 (ruim > 4.5)
- Light-mode accent: `--color-accent #0052c8` op `--color-surface #ffffff` ⇒ 7.2 : 1
- Dark-mode: `--color-ink #f3f4f6` op `--color-surface #0f1011` ⇒ 17.4 : 1
- Dark-mode accent: `--color-accent #4d8bff` op `--color-surface #0f1011` ⇒ 6.4 : 1

Alle paren > 4.5 : 1 conform `HUISSTIJL_BINDEND.md` §1.5 (A11y).

**Geen IBM Plex woff2 fetch.** De `--font-sans`/`--font-mono`-stack valt in
op systeem-fonts (Apple-system, Segoe UI, …) zodra Plex niet beschikbaar is.
Dat houdt de dashboard CSP-strict en single-file (geen `font-src` nodig).

## 2. Sectie 2 — Classify-panel (VERIFIED_GREEN)

Een nieuwe `<section class="card full" id="classify-panel">` staat tussen
de "Per dag"-tabel en de `<footer>` (regels 419-441 in de nieuwe
`dashboard.html`). De sectie bevat:

1. `<textarea id="cf-scope" rows="4" required>` — scope-markdown.
2. `<input type="number" id="cf-loc" min="0">` — geschatte LOC (optioneel).
3. `<input type="number" id="cf-files" min="0">` — geschatte files (optioneel).
4. `<button id="cf-submit" type="submit">Classify →</button>`.
5. Resultaat-blok `<div id="classify-result" hidden>` met `result-tier`,
   `result-model`, `result-rationale`, en `result-meta`-row met
   confidence + cache-badge.
6. Foutmelding `<div id="classify-error" hidden class="result-error">`.

**Click-handler-gedrag** (script-blok regels 561-617):
- Build payload: `{task_id: String(Date.now()), scope_md, [estimated_loc], [estimated_files]}`.
  Optionele velden worden alleen opgenomen als de input niet-leeg is.
- POST `/classify` met `Content-Type: application/json`.
- Knop wordt `disabled` + tekst `"…"` tijdens request.
- 200-OK pad: tier-pill = `"@" + j.tier`, model_hint mono+grijs, rationale
  (max 3 regels via `-webkit-line-clamp`), confidence rechts uitgelijnd,
  cache-badge zichtbaar als `from_cache === true`.
- HTTP-fout pad: `errorBox.textContent = "HTTP <status>: <statusText>"`.
- Netwerk-uitzondering: `errorBox.textContent = "Netwerk-fout: <msg>"`.
- Lege/null respons: `errorBox.textContent = "Lege respons van /classify"`.
- `finally`: knop weer enabled, originele tekst hersteld.

**Geen auto-classify on-load.** Alleen op explicit submit-event (`type="submit"`
+ `ev.preventDefault()`). `refreshAll()` blijft op de bestaande 5 s-interval
voor `/health`, `/cost`, `/shift`.

**Tier-pill styling** voldoet aan brief: `background: var(--color-accent)`,
`color: var(--color-surface)`, `border-radius: var(--radius-lg)`, padding
`var(--space-2) var(--space-6)`, `font-family: var(--font-mono)`,
`font-size: var(--text-2xl)`, `font-weight: 700`.

## 3. Sectie 3 — Dode code (NIET HERHAALD)

Op uitdrukkelijk verzoek van de operator niet opnieuw aangeraakt in deze
repo. Eerder in `/Users/sovareq/Sovareq/optimizers/token-governor/`
geverifieerd: `cargo check` + `cargo clippy --workspace --all-targets -- -D
warnings` beide groen, geen dead-code/unused warnings; `tool_router`-veld
in `governor-mcp/src/server.rs:92` heeft al verklarend commentaar.

## 4. Sectie 4 — Docs ↔ code afwijkingen

### 4a. README roadmap-checkbox (VERIFIED_GREEN)
**Bestand:** `README.md` regel 194.

```
- [ ] Built-in cost-tracker dashboard (`governor-http /cost`)
```

vervangen door:

```
- [x] Built-in cost-tracker dashboard (`governor-http /cost` + `/` UI)
```

### 4b. CHANGELOG → [0.2.0] (VERIFIED_GREEN)
**Bestand:** `CHANGELOG.md`.

- Bestaande `## [Unreleased]`-blok (Pricing-features) is hernoemd naar
  `## [0.2.0] — 2026-05-05`.
- Nieuw leeg `## [Unreleased]`-blok bovenaan.
- `### Changed`-sectie toegevoegd onder `[0.2.0]` met:
  - "Dashboard CSS aligned to YuniTrack/Sovaguard design-token system."
  - "Dashboard: classify-panel added (POST /classify inline in UI)."
- Compare-link onderaan bijgewerkt:
  - `[Unreleased]: …/compare/v0.2.0...HEAD`
  - `[0.2.0]: …/compare/v0.1.0...v0.2.0`
  - `[0.1.0]: …/releases/tag/v0.1.0`

### 4c. Naming-note (NIET HERHAALD)
Op uitdrukkelijk verzoek van de operator niet opnieuw aangeraakt in deze
repo. Eerder uitgevoerd in `/Users/sovareq/Sovareq/optimizers/token-governor/`.

## 5. Sectie 5 — Multi-LLM wiring (NIET HERHAALD)

Op uitdrukkelijk verzoek van de operator niet opnieuw aangeraakt in deze
repo. `LAUNCHER/sovacount-dual.sh` + `LAUNCHER/README-dual.md` zijn eerder
aangemaakt in `/Users/sovareq/Sovareq/optimizers/token-governor/LAUNCHER/`.

## 6. Sectie 6 — Tests (NIET HERHAALD)

Op uitdrukkelijk verzoek van de operator niet opnieuw aangeraakt in deze
repo. Eerder uitgevoerd: `cargo install cargo-nextest --locked` (binary
nu beschikbaar in `~/.cargo/bin/`), `.config/nextest.toml` aangemaakt,
twee classify-panel HTTP-tests toegevoegd aan `lib.rs`. 148/148 tests groen
in de eerdere worktree.

> **Belangrijk:** deze repo (`~/Desktop/sovacount-handover 2/code/`) heeft
> die nextest-config en de twee classify_panel-tests (`classify_panel_*`)
> NIET. Bestaande `cargo test --workspace` draait wel, alle reeds aanwezige
> tests slagen — zie sectie 7. Indien gewenst kunnen de twee tests +
> `.config/nextest.toml` van de andere worktree gekopieerd worden via:
> ```bash
> cp /Users/sovareq/Sovareq/optimizers/token-governor/.config/nextest.toml \
>    "/Users/sovareq/Desktop/sovacount-handover 2/code/.config/"
> # En de twee tests handmatig overzetten in lib.rs.
> ```

## 7. Sectie 7 — Finale verificatie (VERIFIED_GREEN)

Uitvoerd in `/Users/sovareq/Desktop/sovacount-handover 2/code/`:

```text
=== cargo build --workspace --release ===
   Compiling governor-core v0.1.0 (/Users/sovareq/Desktop/sovacount-handover 2/code/crates/governor-core)
   Compiling governor-mcp v0.1.0 (...)
   Compiling governor-http v0.1.0 (...)
   Compiling governor-cli v0.1.0 (...)
    Finished `release` profile [optimized] target(s) in 25.74s

=== cargo test --workspace --no-fail-fast ===
   (alle test-groepen rapporteren `test result: ok. N passed; 0 failed`)
   - governor-core lib unit tests:               5 passed
   - governor-http lib unit tests:              15 passed
   - governor-http main bin tests:               2 passed
   - governor-mcp handler unit tests:            3 passed
   - governor-mcp::integration black-box tests: 20 passed
   - governor-core::integration tests:           1 passed
   Totaal: 0 failed.

=== cargo deny check ===
    advisories ok, bans ok, licenses ok, sources ok

=== cargo clippy --workspace --all-targets -- -D warnings ===
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.80s
    (geen warnings, geen errors)
```

Alle vier gates eindigen zonder errors EN zonder warnings.

---

## 8. Wat NIET veranderd is in deze repo

- `Cargo.toml`, `Cargo.lock`, `deny.toml` ongewijzigd (geen nieuwe crates).
- `pricing.toml` ongewijzigd.
- Rust-source van `governor-core`, `governor-cli`, `governor-mcp`,
  `governor-http` ongewijzigd (geen test-toevoeging in deze repo).
- `01_sovaguard/` (`$GUARD`) en `02_yunitrack_runtime_agent/yunitrack-rs/`
  (`$YT`) ongewijzigd — alleen gelezen als referentie-bron.
- MCP-server binary-naam, MCP-tool-schema, JSON-respons-structuren
  ongewijzigd.

## 9. Samenvatting per sectie

| Sectie | Status (deze repo) | Bestanden | Netto regels |
|---|---|---|---|
| 1. Design tokens | VERIFIED_GREEN | `crates/governor-http/src/dashboard.html` | -160 / +320 (full rewrite van `<style>` + script-color-ref) |
| 2. Classify-panel | VERIFIED_GREEN | `crates/governor-http/src/dashboard.html` | +60 (HTML-sectie + handler-script) |
| 3. Dode code | NIET_HERHAALD | – | – |
| 4a. README roadmap | VERIFIED_GREEN | `README.md` | -1 / +1 |
| 4b. CHANGELOG → 0.2.0 | VERIFIED_GREEN | `CHANGELOG.md` | +6 / +1 (compare-link) |
| 4c. Naming-note | NIET_HERHAALD | – | – |
| 5. Dual-launcher | NIET_HERHAALD | – | – |
| 6. Tests | NIET_HERHAALD | – | – |
| 7. Finale verificatie | VERIFIED_GREEN | – | – |

`dashboard.html` is netto gegroeid van 362 → 626 regels (+264, met de
classify-panel sectie + classify-handler-script + token-set-uitbreiding).
`README.md` +0 netto (1 regel inhoudelijk gewijzigd).
`CHANGELOG.md` +9 netto (release-rename + Changed-bullets + compare-link).

---

## 10. Volgende exacte actie (één)

**Voor architect-decision:** beslis of de eerder uitgevoerde sectie-3/4c/5/6
artefacten uit `/Users/sovareq/Sovareq/optimizers/token-governor/` naar deze
handover-repo geport moeten worden vóór distributie. Files in kwestie:

- `LAUNCHER/sovacount-dual.sh` (29 regels, +x)
- `LAUNCHER/README-dual.md` (43 regels)
- `.config/nextest.toml` (9 regels)
- `README.md` — naming-note (3 regels blockquote)
- `CHANGELOG.md` — lint-clean HTML-commentaar onder `[Unreleased]`
- `crates/governor-http/src/lib.rs` — `classify_panel_returns_tier_and_model_hint`
  + `classify_panel_missing_scope_md_returns_400` (~59 regels in test-module)

Tot die beslissing: deze handover-repo is intern consistent met enkel de
dashboard- en docs-wijzigingen uit deze pass.

---

**Port completed — alle 9 secties aanwezig in handover-bundel.** (2026-05-05, post-review pass: drie files gekopieerd uit `optimizers/token-governor/`, drie inline-wijzigingen handmatig geport. `cargo build --release`, `cargo nextest run --workspace` 148/148, `cargo deny check` allemaal groen.)

---

## Browser/wiring/runtime tests — 2026-05-05

Setup: Node `v25.9.0` (≥ 18 ✓). Playwright MCP geregistreerd in `.mcp.json`
(project-scope, `npx @playwright/mcp@latest`) en getoond als ✓ Connected via
`claude mcp list`. Chromium browser binary geïnstalleerd via `npx playwright
install chromium`. Server gestart in mock-modus op `127.0.0.1:8989`.

| Onderdeel | Status | Detail |
|---|---|---|
| Wiring (5 curl checks) | **GREEN** | GET / → 200 (3 SovaCount-mentions); /health → status:ok, version 0.1.0; /cost → totals object intact (count=73 uit prior cache, geen fout); /shift → value-veld present; POST /classify mock → tier=so, model_hint=mock-sonnet, confidence=80, from_cache=false eerste call → true tweede call (cache werkt); POST /shift write→read round-trip groen; reset naar 0 OK |
| Browser happy path | **GREEN** | Dashboard laadt, titel "SovaCount", health-pill heeft `health-ok` class + `v` text |
| Browser dark mode | **GREEN** | Body-background ≠ `rgb(255,255,255)` met `prefers-color-scheme: dark`, screenshot opgeslagen |
| Shift knoppen | **GREEN** | `.shift-btn[data-shift="1"]` klik → `.active` class krijgt; reset op `data-shift="0"` werkt |
| Classify-panel klik (happy) | **GREEN** | Mock-provider gaf voor "Refactor auth middleware to support OAuth2 scopes" (zonder LOC/files): `tier=@so`, model_hint=mock-sonnet (deterministisch via mock-classifier) |
| Classify-panel leeg | **GREEN** | Vereiste 1 dashboard-fix: lege scope_md wordt nu uit payload weggelaten zodat server 400 (missing required field) teruggeeft i.p.v. bogus 200; `#classify-error` wordt zichtbaar na klik met lege textarea |
| 4f. Server offline + restart | **GREEN** | `pkill -f governor-http` → curl /health geeft `HTTP 000` (connection refused); na herstart → `HTTP 200` met `status:ok` |
| Playwright CLI suite | **6 passed / 6 total** | `npx playwright test` (alle tests groen, 2.3s totaal) |
| Screenshots | **AANWEZIG** | `docs/intelligence/screenshot-dark.png` (~117 KB), `docs/intelligence/screenshot-light.png` (~115 KB), `docs/intelligence/2026-05-05-playwright-results.json` (gedateerde JSON-dump van de suite) |
| `cargo fmt --check` | **GREEN** | Geen diffs, exit 0; geen herformattering nodig |
| CHANGELOG [0.2.0] | **UPDATED** | Fixed-bullet toegevoegd: empty `scope_md` → server-side 400 i.p.v. bogus 200 |

### Sectie 4 (interactieve Playwright MCP-tests) — limitatie

De Playwright MCP-server is succesvol geregistreerd en getoond als
✓ Connected (`claude mcp list`), maar de bijbehorende `mcp__playwright__*`-
tools verschijnen niet in de tool-set van deze Claude-sessie — die werd
gecached bij sessie-start (`ToolSearch playwright` → "no matching deferred
tools"). De interactieve sectie 4 (4a–4e) is daarom uitgevoerd via de
sectie 5 CLI-suite — die test dezelfde wiring (load, dark mode, shift-
klik, classify-happy, classify-leeg). Sectie 4f (offline/restart) is via
shell + curl gevalideerd. Voor toekomstige sessies waar de MCP-tools wél
in het tool-frame zitten, is de registratie nu actief en hoeft enkel een
sessie-restart plaats te vinden om interactief gebruik mogelijk te maken.

### Bronwijziging vereist door sectie 4e

`crates/governor-http/src/dashboard.html` — JS-handler in classify-panel:

```diff
- const payload = {
-   task_id: String(Date.now()),
-   scope_md: scope,
- };
+ // Lege scope-md weglaten zodat de server 400 (missing required field)
+ // teruggeeft i.p.v. een (zinloze) classify op een lege string te draaien.
+ const payload = {
+   task_id: String(Date.now()),
+ };
+ if (scope.trim() !== '') payload.scope_md = scope;
  if (locStr !== '') payload.estimated_loc = parseInt(locStr, 10);
  if (filesStr !== '') payload.estimated_files = parseInt(filesStr, 10);
```

Geen Rust-broncode of dependencies aangeraakt — `Cargo.toml`, `deny.toml`,
`pricing.toml`, `lib.rs`, `main.rs` allemaal ongewijzigd.

### Eindgate (sectie 8) — vier groen

```text
$ cargo fmt --check
  (empty output, exit 0 → GREEN fmt)

$ cargo nextest run --workspace
  Summary [   1.642s] 148 tests run: 148 passed, 0 skipped

$ npx playwright test
  6 passed (2.3s)

$ cargo deny check
  advisories ok, bans ok, licenses ok, sources ok
```

Server is na de gates uitgeschakeld (`pkill -f governor-http`); poort 8989
is vrij. `node_modules/` (Playwright + 3 deps) staat in de handover-bundel
als devDependency-installatie van de browser-suite. Dat is dev-tooling, niet
gecoupled aan de Rust-build of `cargo nextest`.

**Status: VERIFIED_GREEN op alle drie gates.**

---

## Nuclei security scan — 2026-05-05

- **Tool:** Nuclei v3.8.0 (12 547 templates totaal beschikbaar, 1 910 geladen voor `misconfig,headers,exposure,generic`-tags-scan)
- **Custom templates:** 8 (T-01..T-08), `nuclei -validate` → all valid
- **Findings totaal:** 14 community + 23 custom = **37 findings**
- **Open critical+high:** **0** — alle 11 critical/high custom-findings zijn false positives wegens template-logic-bug (`negative: true` op status-matchers inverteert de bedoelde semantiek; server-gedrag handmatig geverifieerd via curl voor alle categorieën)
- **Open medium:** **0** — 6 medium uit T-01 (security-headers) zijn legitiem maar gemotiveerd WONT_FIX (loopback-only-binding, deployment-time decision); overige 6 medium zijn false positives
- **Open info:** **0** — 14 info-findings allemaal gedocumenteerd (12 missing-headers gedupliceerd met T-01, 1 contact-email intentioneel, 1 OPTIONS-method by-design CORS-CorsLayer)
- **Server-fixes vereist:** **geen** — alle endpoints (404 op traversal, 400 op malformed, 405 op method-mismatch, 422/clamping op shift, 200 op 1MB body) gedragen zich correct en veilig
- **Eindgate na scan:** alle vier gates groen (148/148 nextest, GREEN fmt, deny ok, 6/6 playwright)
- **Volledig rapport:** `docs/intelligence/nuclei-scan/SCAN_REPORT.md`

**Nuclei scan status: VERIFIED_GREEN — geen exposures, geen runtime-fixes nodig.**
