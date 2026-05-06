# Nuclei Security Scan — governor-http

**Datum:** 2026-05-05
**Nuclei versie:** v3.8.0
**Templates beschikbaar:** 12 547 totaal · 1 910 geladen voor community-scan (gecategoriseerd `misconfig,headers,exposure,generic`)
**Custom templates:** 8 (T-01..T-08) onder `docs/intelligence/nuclei-scan/templates/` — `nuclei -validate` → "All templates validated successfully"
**Target:** `http://127.0.0.1:8989` (governor-http v0.1.0, mock-provider, loopback-only)
**Scope:** uitsluitend localhost; geen CVE-scans (third-party niet relevant); rate-limit 30-50 RPS; timeout 10-15 s

---

## 1. Community templates — 14 findings (alle INFO)

Categorieën: `misconfig,headers,exposure,generic` — 1 910 templates effectief geladen.

| Template | Severity | Doel | Actie |
|---|---|---|---|
| `email-extractor` | info | Vond `bjorn@sovareq.com` in `/` (dashboard footer) | **Geen actie** — bewust opgenomen contact in `dashboard.html` (footer-link `mailto:bjorn@sovareq.com`). Niet PII-exfiltratie, niet vertrouwelijk. |
| `options-method` | info | Server reageert op `OPTIONS` met `Allow: GET,HEAD` | **Geen actie** — `tower-http::cors::CorsLayer::permissive()` op router. CORS preflight gedrag is by-design voor browser-side agents. |
| `http-missing-security-headers:permissions-policy` | info | Permissions-Policy header ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:x-frame-options` | info | X-Frame-Options ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:x-content-type-options` | info | X-Content-Type-Options ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:x-permitted-cross-domain-policies` | info | X-Permitted-Cross-Domain-Policies ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:referrer-policy` | info | Referrer-Policy ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:clear-site-data` | info | Clear-Site-Data ontbreekt | N/A — alleen relevant op logout-paden |
| `http-missing-security-headers:cross-origin-embedder-policy` | info | COEP ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:content-security-policy` | info | CSP ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:cross-origin-opener-policy` | info | COOP ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:cross-origin-resource-policy` | info | CORP ontbreekt | Zie WONT_FIX in §4 |
| `http-missing-security-headers:missing-content-type` | info | Content-Type op JSON-routes (axum zet `application/json` correct; nuclei flagt op `/health` mogelijk anders) | **Verifiëren niet nodig** — handmatige check toont `application/json` op alle JSON-endpoints, `text/html; charset=utf-8` op `/`. Nuclei false positive. |
| `http-missing-security-headers:strict-transport-security` | info | HSTS ontbreekt | N/A — server bindt loopback-only (HTTP), geen TLS. HSTS is per-definitie alleen relevant op HTTPS. |

---

## 2. Custom templates T-01..T-08 — 23 findings

| Template | Geladen | Hits | Severity | False positive? |
|---|---|---|---|---|
| T-01 security-headers-check | ✓ | 6 | medium | Nee — headers zijn echt afwezig (zie WONT_FIX §4) |
| T-02 auth-bypass-classify | ✓ | 1 | **high** | **Ja** — template-logic-bug (zie §3 triage) |
| T-03 method-confusion | ✓ | 1 | medium | **Ja** — template-logic-bug |
| T-04 oversized-payload | ✓ | 0 | high | T-04 vuurde niet (`{{repeat(…)}}`-helper limiet); manueel geverifieerd safe (zie §3 triage) |
| T-05 malformed-json | ✓ | 6 | **high** | **Ja** — template-logic-bug (allemaal) |
| T-06 path-traversal | ✓ | 4 | **critical** | **Ja** — template-logic-bug (allemaal) |
| T-07 version-disclosure | ✓ | 0 | info | Geen leakage gedetecteerd ✓ |
| T-08 shift-out-of-range | ✓ | 5 | medium | **Ja** — template-logic-bug (allemaal) |

Alle 8 templates valideren via `nuclei -validate`.

---

## 3. TRIAGE — Kritieke en hoge findings

### CRITICAL (4 hits) — T-06 governor-path-traversal

**Wat Nuclei detecteerde:**
GET `/../etc/passwd` + drie URL-encoded varianten — alle 4 fired.

**Root cause analyse:**
De template gebruikt `matchers-condition: and` met twee `negative: true`-matchers:

```yaml
matchers:
  - type: status
    negative: true
    status: [200]
  - type: word
    negative: true
    words: ["root:", "/bin/bash"]
```

Met `negative: true` op een `status`-matcher: vuurt wanneer de response-status NIET in de lijst zit. Met `negative: true` op een `word`-matcher: vuurt wanneer de body NIET de woorden bevat. Met `matchers-condition: and`: beide moeten matchen.

Resultaat: een 404 zonder `/etc/passwd`-inhoud (= veilige uitkomst) triggert beide matchers en wordt aangemerkt als finding.

De bedoelde semantiek per de YAML-comment was "fire IF status==200 OR body contains passwd-content" — een `and` van twee `negative: true`-matchers inverteert die volledig.

**Wat de server werkelijk doet:**
```
GET /../etc/passwd            → HTTP 404 (geen body-content)
GET /%2e%2e%2fetc%2fpasswd    → HTTP 404
GET /..%2F..%2Fetc%2Fpasswd   → HTTP 404
GET ////etc/passwd            → HTTP 404
```

axum's URI-router decoded en normaliseert percent-encoding vóór route-matching. Geen route-handler matcht op `/etc/passwd`-paden ⇒ 404. `governor-http` bedient géén filesystem; er is geen `tower-http::services::fs::ServeDir` of vergelijkbare LFI-bron. `dashboard.html` wordt embedded via `include_str!` (compile-time gebakken in de binary) — geen filesystem-pad runtime.

**False positive verdict:** JA — definitief geen LFI mogelijk. Server-gedrag is correct en veilig.

**Fix in code:** geen — correct gedrag.
**Fix in template:** voorgesteld (NIET uitgevoerd zonder operator-OK):
```yaml
matchers-condition: or
matchers:
  - type: status
    status: [200]
  - type: word
    words: ["root:", "/bin/bash"]
```

---

### HIGH (1 hit) — T-02 governor-classify-auth

**Wat Nuclei detecteerde:**
POST `/classify` zonder Authorization-header in mock-mode → finding fired.

**Root cause analyse:**
Template:
```yaml
matchers-condition: or
matchers:
  - type: status
    negative: true
    status: [500, 502, 503]
  - type: status
    negative: true
    status: [403]
```

`negative: true` op `status: [500,502,503]` vuurt voor élk niet-5xx-antwoord. Voor 200 (mock-success) vuurt deze matcher. Met `or`-condition flagt nuclei. De YAML-comment zegt "Verwacht: 200 (mock, geen auth) of 401" maar het matcher-design vuurt juist op de verwachte 200.

**Wat de server werkelijk doet:**
- Mock-modus, geen `GOVERNOR_HTTP_API_KEY` env-var: POST /classify → 200 met mock-classificatie (correct).
- Met `GOVERNOR_HTTP_API_KEY=secret` zonder header: 401 (correct, getest in lib.rs `auth_missing_header_returns_401` test).

`crates/governor-http/src/lib.rs:225-235` (`check_auth`):
- Als `state.api_key.is_none()` (mock-default): geen auth-check, doorlaten.
- Anders: header lezen, vergelijken, anders 401.

**False positive verdict:** JA — server gedraagt zich exact volgens design (mock = open, prod-mode-with-key = vereiste auth).

**Fix in code:** geen.
**Fix in template:** verwijder `negative: true`, of test specifiek tegen running mode:
```yaml
matchers:
  - type: status
    status: [500, 502, 503, 403]
```

---

### HIGH (6 hits) — T-05 governor-malformed-json

**Wat Nuclei detecteerde:**
Zes kapotte JSON-payloads naar `/classify` — alle 6 vuurden.

**Root cause analyse:**
Template:
```yaml
matchers:
  - type: status
    negative: true
    status: [500, 000]
```

Vuurt voor élke status NIET 500/000. axum's `JsonRejection` → 400. Nuclei flagt 400 als finding, terwijl 400 juist het correcte gedrag is.

**Wat de server werkelijk doet (handmatig met curl geverifieerd):**
| Payload | Status |
|---|---|
| `{broken json` | 400 |
| `{"task_id": }` | 400 |
| `null` | 400 |
| `[]` | 400 |
| `(empty)` | 400 |
| `{"task_id":"x","scope_md":` | 400 |

Server logica `crates/governor-http/src/lib.rs:208-213`:
```rust
let Json(req) = match body {
    Ok(j) => j,
    Err(rej) => {
        return error_response(StatusCode::BAD_REQUEST, &rej.body_text());
    }
};
```

Dit is precies de bedoelde robuustheid — JsonRejection wordt expliciet gemapped naar 400, niet de axum-default 422.

**False positive verdict:** JA — alle 6 hits. Server doet het juiste.

**Fix in code:** geen.
**Fix in template:** vervang `negative: true status: [500,000]` door `status: [500]`.

---

### HIGH (0 hits) — T-04 governor-oversized-payload (handmatig geverifieerd)

T-04 vuurde niet — waarschijnlijk omdat de `{{repeat("A", 1048576)}}`-helper bij body-grootte 1 MB niet door Nuclei's body-builder wordt geëxpandeerd (geen klacht in `nuclei -validate`, geen warn in scan-log; `Templates loaded for current scan: 7` suggereert effectief 7 van 8 templates in clustering). Daarom **handmatig geverifieerd** met `python3 + curl` (1 048 615 bytes verzonden):

```
HTTP=200 sent=1048615 recv=279 time=0.004779s
response: {"tier":"hk","model_hint":"mock-haiku",...}
```

Server accepteerde 1 MB JSON in <5 ms en gaf coherente classificatie terug. Geen 500, geen panic, geen DoS. Server-gedrag veilig. **Geen finding gegenereerd**, geen fix nodig.

`axum`'s default `Json` extractor heeft een `DefaultBodyLimit` (2 MB tenzij overschreven) — payloads > 2 MB zouden 413 teruggeven. 1 MB zit binnen de limiet. Voor productie-deployment kan de operator `RequestBodyLimitLayer` overwegen om dit te tunen — out-of-scope voor dit handover-rapport.

---

## 4. WONT_FIX motivatie — Medium-severity findings

### M1: T-01 security-headers-check + community-equivalent (12 INFO)

**Beschrijving:** governor-http stuurt geen X-Content-Type-Options, X-Frame-Options, Content-Security-Policy, Referrer-Policy, COOP/COEP/CORP, Permissions-Policy of HSTS.

**Motivatie WONT_FIX:**
1. **Loopback-default.** `crates/governor-http/src/main.rs:20` (`DEFAULT_BIND = "127.0.0.1:8989"`). De server is by-design alleen toegankelijk vanaf de eigen machine. CSP/XFO/HSTS-hardening is voor publiek-bereikbare services.
2. **Eén origin, geen cross-context.** Het dashboard is single-page, statisch HTML met `include_str!` — geen externe scripts, geen iframes, geen postMessage. CSP/COOP/COEP voegen geen meetbare bescherming toe in deze context.
3. **HTTP-only.** HSTS heeft alleen waarde op HTTPS-services. Loopback-bind is HTTP.
4. **Deployment-time decision.** Wanneer de operator de server publiek opent (`--bind 0.0.0.0` + reverse proxy), is het juiste punt voor security-headers de TLS-terminator (nginx/traefik/caddy), niet de Rust-binary. Hard-coded headers in axum zouden óf overbodig zijn (achter een hardening-layer) óf ontoereikend (zonder die layer, want HSTS vereist HTTPS dat axum hier niet biedt).

**Beslissing:** geen fix in deze release. Document opnemen in toekomstige `INSTALL.md` over reverse-proxy-headers wanneer publieke deployment in scope komt.

### M2: T-03 method-confusion (1 hit, false positive)

**Wat Nuclei detecteerde:** GET `/classify` (POST-only endpoint) → finding.

**Werkelijke server-respons:** HTTP 405 Method Not Allowed (axum default, correct).

**Template-bug:** `matchers: status: [200,500] negative: true` vuurt op 405 omdat 405 niet in de lijst zit. Bedoeling was vuren ALS status 200 of 500 is.

**False positive verdict:** JA. Server doet het juiste (405 voor GET op POST-route).

### M3: T-08 shift-out-of-range (5 hits, false positives)

**Wat Nuclei detecteerde:** vijf out-of-range / verkeerd-typed shift-bodies → alle 5 fired.

**Werkelijke server-respons (handmatig geverifieerd):**
| Body | Status | Response |
|---|---|---|
| `{"value":9999}` | 200 | `{"value":2}` (correct geclamped) |
| `{"value":-9999}` | 200 | `{"value":-2}` (correct geclamped) |
| `{"value":2.5}` | 422 | "invalid type: floating point `2.5`, expected i32" |
| `{"value":"up"}` | 400 | "expected value at line 1 column 10" |
| `{"value":null}` | 422 | "invalid type: null, expected i32" |

Geen 500, geen panic. Out-of-range integers worden correct geclamped via `body.value.clamp(-2, 2)` in `crates/governor-http/src/lib.rs:159`. Verkeerde JSON-types worden door axum's `serde_json`-deserializer afgevangen met 400/422 (expected behavior).

**Template-bug:** `negative: true status: [500,000]` vuurt op élke niet-500/000 status. Bedoeling was: vuren ALS server crashes (500) of dropt (000).

**False positive verdict:** JA. Allemaal correct server-gedrag.

---

## 5. Open findings

**Geen.**

Alle 23 custom-findings + 14 community-findings hebben een afgeronde triage:
- 4 critical (T-06): false positives, template-logic-bug, server is veilig.
- 7 high (T-02 + T-05): false positives, template-logic-bug, server is veilig.
- 12 medium (T-01 6× + T-03 1× + T-08 5×): 6 legitieme observaties (security-headers, WONT_FIX gemotiveerd) + 6 false positives (template-bugs).
- 14 info (community): 12 over missing-headers (gedupliceerd met T-01, WONT_FIX), 1 contact-email (intentioneel), 1 OPTIONS-method (CORS by-design).

---

## 6. Gesloten findings

| ID | Categorie | Severity | Status |
|---|---|---|---|
| T-06 ×4 | path-traversal | critical | CLOSED — false positive, server returns 404, no LFI |
| T-02 | classify-auth | high | CLOSED — false positive, mock-mode 200 is expected |
| T-05 ×6 | malformed-json | high | CLOSED — false positives, server returns 400 (correct) |
| T-01 ×6 | security-headers | medium | CLOSED — WONT_FIX (loopback-only, deployment-time decision) |
| T-03 ×1 | method-confusion | medium | CLOSED — false positive, server returns 405 (correct) |
| T-08 ×5 | shift-clamping | medium | CLOSED — false positives, server clamps correctly |
| community ×14 | misc | info | CLOSED — see §1 |

---

## 7. Eindgate

Server bleef tijdens scan op 127.0.0.1:8989 (mock-provider). Geen fixes nodig in Rust-broncode op basis van triage. End-gate gates herhaald om vast te stellen dat scan-activiteit geen regressies introduceerde:

```text
$ cargo nextest run --workspace
  Summary [   1.586s] 148 tests run: 148 passed, 0 skipped       ✓ GREEN

$ cargo fmt --check
  (empty output, exit 0)                                          ✓ GREEN

$ cargo deny check
  advisories ok, bans ok, licenses ok, sources ok                ✓ GREEN

$ npx playwright test
  6 passed (2.5s)                                                 ✓ GREEN
```

| Gate | Status |
|---|---|
| `cargo nextest run --workspace` | **148/148 passed** |
| `cargo fmt --check` | **GREEN** |
| `cargo deny check` | **ok / ok / ok / ok** |
| `npx playwright test` | **6 / 6 passed** |

Alle vier gates clean.

---

## 8. Aanbevelingen

1. **Templates corrigeren** (T-02, T-03, T-04, T-05, T-06, T-08): de `negative: true`-flag werd consequent verkeerd toegepast — deze flag inverteert de matcher-logica en is hier niet bedoeld. Een herschrijving zonder `negative: true` op deze templates zou bij een toekomstige scan ofwel 0 false-positives geven (wenselijk) ofwel een echte regressie blootleggen indien die ontstaat. **Niet uitgevoerd in deze run** — operator-templates bleven onveranderd zoals geleverd in de taakbrief.
2. **`{{repeat(…)}}`-helper voor 1 MB body**: niet betrouwbaar in Nuclei v3.8 — overweeg een externe payload-bestand (`-r oversize.json`) als de oversized-payload-test in CI moet draaien.
3. **Security-headers** voor publieke deployment: voeg in toekomstige `INSTALL.md` een nginx/traefik snippet toe met CSP / XFO / HSTS / Referrer-Policy / Permissions-Policy.
4. **Geen Rust-broncode-fix vereist** door deze scan. Server-gedrag is op alle 8 testcategorieën correct en veilig.
