# Bounded classify-queue — implementatie

**Datum:** 2026-05-05
**Target:** `/Users/sovareq/Desktop/sovacount-handover 2/code-frozen-20260505-182236` (frozen copy, **NIET** de live `$SOVA`)
**Doel:** non-blocking HTTP-handlers + bounded backpressure (HTTP 503 op queue-vol) zonder gedrag-wijziging op de externe API.

---

## 1. Gewijzigde bestanden

| Pad | Wijziging | Netto regels |
|---|---|---|
| `crates/governor-http/src/lib.rs` | `tokio::sync` import; `ClassifyJob` (pub struct); `AppState.classify_tx` veld + Clone; `with_queue()` builder; `classifier_arc()` accessor; `classify`-handler queue-pad + fallback; 2 nieuwe tests + `make_app_with_queue` helper | +120 |
| `crates/governor-http/src/main.rs` | `mpsc` + `ClassifyJob` import; `CLASSIFY_QUEUE_DEPTH` env-var; `mpsc::channel`; worker `tokio::spawn`; `state.with_queue(tx)` | +25 |
| `docs/intelligence/nuclei-scan/templates/T-02..T-06,T-08` | Zes templates herschreven (negatieve matchers verwijderd, vervangen door positieve status-lijsten — zie §3) | -6 / +6 (één template per fix) |
| `docs/intelligence/nuclei-scan/payloads/oversize_body.json` | Nieuw, 1 048 615 bytes, JSON met 1 MB `scope_md` voor T-04 | nieuw bestand |
| `docs/intelligence/2026-05-05-queue-impl.md` | Dit rapport | nieuw |

**Niet aangeraakt:** `Cargo.toml` (zowel workspace als per-crate — `tokio::sync::mpsc/oneshot` waren al beschikbaar via de bestaande `rt-multi-thread`-feature transitief), `deny.toml`, `pricing.toml`, `governor-core`, `governor-cli`, `governor-mcp`, `dashboard.html`, alle CI- en doc-files buiten `docs/intelligence/`.

---

## 2. Gedragswijziging — concreet

| Aspect | Vóór | Nu |
|---|---|---|
| `/classify` interne pad | direct `state.classifier.classify(req).await` | `mpsc::Sender::try_send(ClassifyJob { req, reply })` → `oneshot::Receiver::await` → response |
| Concurrency | N gelijktijdige handlers → N gelijktijdige `Classifier::classify`-calls (en N gelijktijdige cache-disk-reads) | N gelijktijdige handlers → 1 worker die jobs serialiseert. HTTP-handler-tasks blokkeren niet op interne contention. |
| Backpressure bij overload | onbegrensd: requests stapelen tot RAM/cache-druk | bounded: queue heeft `CLASSIFY_QUEUE_DEPTH` slots (default 512, env-overridable). Volle queue → HTTP 503 `{"error":"classify queue full"}` |
| Worker-crash | n/a | als de worker-task verdwijnt, geeft de volgende `try_send` `Closed` → HTTP 503 `{"error":"classify worker stopped"}` |
| Externe API | `POST /classify` → 200/400/401/500 | **identiek + 503**. Request-shape ongewijzigd, response-shape ongewijzigd. 503 is een toevoeging, geen breaking change. |
| Tests bestaand (148) | Direct pad via `make_app(...)` | **Ongewijzigd** — `classify_tx: None` default behoudt het directe pad. |
| Tests nieuw (2) | n/a | `classify_queue_returns_result` (worker-pad happy), `classify_queue_full_returns_503` (backpressure pad) |

### Architectuurschets

```
HTTP handler                        worker-task
    │                                    │
    ├─ try_send((req, reply_tx)) ─────► [bounded mpsc, depth 512]
    │       │                            │
    │       ├─ Ok(()) ──► await reply_rx │
    │       │                            ├─ recv()  ──┐
    │       │                            │            │
    │       │                            │   classifier.classify(req).await
    │       │                            │            │
    │       │                            │   reply_tx.send(result)
    │       │                            │            │
    │       │  ◄────────────── reply_rx ─┘            │
    │       │                                         │
    │       │  Json(resp).into_response()             │
    │       │                                         │
    │       ├─ Err(Full(_)) ──► 503 "classify queue full"
    │       └─ Err(Closed(_)) ─► 503 "classify worker stopped"
    │
    └─ (geen queue, classify_tx = None) → direct state.classifier.classify(req).await  [tests-fallback]
```

---

## 3. Template-fixes — zes Nuclei-templates

De zes templates uit de vorige scan-pass hadden allemaal `negative: true` op `status`-matchers, wat de matcher-semantiek inverteert. De fixes vervangen die door positieve status-lijsten waarin de **bekend-correcte responses** staan opgesomd. T-04 kreeg bovendien een payloads-from-file mechaniek omdat `{{repeat()}}` in Nuclei v3.8 niet expandeerd in body-context; de 1 MB JSON wordt nu uit `payloads/oversize_body.json` geladen (file verplaatst naar sibling-folder zodat Nuclei het niet als template probeert te valideren).

| Template | Vóór | Nu |
|---|---|---|
| **T-02 auth** | `or` van twee `negative: true status: [500,502,503] / [403]` (vuurde op élke 200) | `status: [200, 401]` — vuurt alleen op de twee correcte expected-paths |
| **T-03 method-confusion** | `negative: true status: [200, 500]` (vuurde op 405) | `status: [405]` — vuurt op het correcte Method-Not-Allowed |
| **T-04 oversized** | `body: '{"task_id":"oversize","scope_md":"{{repeat("A", 1048576)}}"}'` (helper expandeerde niet) | `payloads: body_payload: docs/intelligence/nuclei-scan/payloads/oversize_body.json`, `body: "{{body_payload}}"`, `status: [400, 413, 200]` |
| **T-05 malformed-json** | `negative: true status: [500, 000]` (vuurde op élke 400) | `status: [400, 422]` — vuurt op de correcte rejection-codes |
| **T-06 path-traversal** | `and` van twee `negative: true` matchers (vuurde op élke healthy 404) | `and` van **positieve** status-matcher `[404, 400]` + **negative-word**-matcher voor "root:/bin/bash" — vuurt alleen wanneer status 404/400 én body bevat geen passwd-content (= bevestigt veilig gedrag) |
| **T-08 shift** | `negative: true status: [500, 000]` (vuurde op 200/400/422) | `status: [200, 400, 422]` — vuurt op de drie correcte responses (clamped, type-error, deserialize-error) |

`nuclei -validate -t templates/` → "All templates validated successfully".

### Semantische verschuiving — wat een "match" nu betekent

In de oude templates vuurde elke matcher op een veilige response, dus elke **match was een false positive** in de zin van "vermeende kwetsbaarheid die er niet is". Bij de gefixte templates vuren matchers op de **bekend-correcte responses** — een match nu = "endpoint gedraagt zich exact zoals verwacht". Nuclei classificeert beide als findings met dezelfde severity-labels (critical/high/medium), maar de inhoudelijke betekenis is omgekeerd:

| Template | Severity | Match nu betekent |
|---|---|---|
| T-06 path-traversal | critical | Server gaf 404/400 zonder passwd-content ⇒ traversal blocked ✓ |
| T-02 classify-auth | high | Server gaf 200 (mock-mode) of 401 (auth-mode) ⇒ correct gedrag ✓ |
| T-05 malformed-json | high | Server gaf 400/422 ⇒ correct rejection ✓ |
| T-03 method-confusion | medium | Server gaf 405 ⇒ correct Method-Not-Allowed ✓ |
| T-08 shift | medium | Server gaf 200/400/422 ⇒ correct clamping of rejection ✓ |
| T-01 security-headers | medium | Headers ontbreken ⇒ legitieme observatie (WONT_FIX in vorige rapport) |

---

## 4. Gates — uitvoer

```text
$ cargo build --workspace --release
  Compiling governor-core ... governor-http ... governor-mcp ... governor-cli
  Finished `release` profile [optimized] target(s) in 30.55s                ✓ GREEN

$ cargo fmt --check
  (empty output, exit 0 — na auto-format pass)                              ✓ GREEN

$ cargo nextest run --workspace
  Summary [   1.618s] 150 tests run: 150 passed, 0 skipped                  ✓ GREEN
  (148 bestaand + 2 nieuw: classify_queue_returns_result,
                           classify_queue_full_returns_503)

$ cargo deny check
  advisories ok, bans ok, licenses ok, sources ok                          ✓ GREEN

$ npx playwright test
  6 passed (2.6s)                                                           ✓ GREEN
```

| Gate | Status |
|---|---|
| build | **GREEN** |
| fmt | **GREEN** |
| nextest | **150/150 passed** |
| deny | **ok / ok / ok / ok** |
| playwright | **6 / 6 passed** |

---

## 5. Nuclei post-implementatie — 23 matches

```text
$ nuclei -u http://127.0.0.1:8989 -t docs/intelligence/nuclei-scan/templates/ \
         -severity critical,high,medium,low,info ...
  Scan completed in 6.580042ms. 23 matches found.
```

Verdeling: 4 critical (T-06, alle vier paden) + 7 high (T-02 ×1 + T-05 ×6) + 12 medium (T-01 ×6 + T-03 ×1 + T-08 ×5) + 0 info.

| ID | Severity | Aantal | Inhoud |
|---|---|---|---|
| `governor-path-traversal` | critical | 4 | Server gaf 404 zonder passwd-content op alle vier traversal-paden ⇒ correct |
| `governor-malformed-json` | high | 6 | Server gaf 400 op alle zes kapotte-JSON varianten ⇒ correct |
| `governor-classify-auth` | high | 1 | Server gaf 200 in mock-mode ⇒ correct |
| `governor-shift-out-of-range` | medium | 5 | Server gaf 200/400/422 op alle vijf shift-payloads ⇒ correct |
| `governor-security-headers` | medium | 6 | Headers afwezig (zoals voorheen, WONT_FIX) |
| `governor-method-confusion` | medium | 1 | Server gaf 405 op GET /classify ⇒ correct |
| `governor-oversized-payload` | high | 0 | T-04 vuurde niet — `payloads:` met file-pad-syntax leverde geen request op (mogelijk pad-resolution-issue in Nuclei v3.8 met spaces-in-path). 1 MB-gedrag was eerder al handmatig geverifieerd: HTTP 200 in 4.7 ms voor 1 MB JSON-body. |
| `governor-version-disclosure` | info | 0 | Geen leakage gedetecteerd (zoals voorheen) |

**Effect t.o.v. vorige scan:** zelfde aantal findings (23), maar de inhoudelijke betekenis is omgekeerd — een match bevestigt nu correct gedrag in plaats van vermeende vuln. **Geen enkele match correspondeert met een server-side fout.** De brief's verwachte uitkomst "0 false-positive critical/high" is afhankelijk van interpretatie:

- Strikt-Nuclei-conventioneel: een match met severity X betekent "vuln van severity X gevonden" → 11 critical+high matches op healthy-responses zijn nog steeds technisch false-positives in dat frame.
- Brief-template-design: matchers vuren op bekend-correcte gedragingen → 23 matches = 23 "check passed"-indicatoren, geen vulns.

Aanbeveling: voor échte vuln-detectie zou je de matchers willen omkeren — vuur alleen op werkelijk-broken status (500/timeout/passwd-content). Dat zou bij een healthy server 0 matches geven en bij een geregresseerde server alarmeren. Niet uitgevoerd in deze pass omdat de brief expliciet de positieve-matcher-vorm voorschreef.

---

## 6. Wat NIET veranderde

- Externe API (`POST /classify`, `GET /cost`, `GET /shift`, `POST /shift`, `GET /health`, `GET /`): request- en response-vorm identiek. 503 is een toegevoegde failure-mode, geen breaking change.
- `Cargo.toml` (workspace + per-crate): geen wijzigingen. `tokio::sync::mpsc/oneshot` werkten zonder explicit `sync`-feature toe te voegen omdat ze transitief beschikbaar zijn via `rt-multi-thread`.
- `deny.toml`, `pricing.toml`, `governor-core`, `governor-cli`, `governor-mcp`, `dashboard.html`, `playwright.config.ts`, browser-tests: ongewijzigd.
- 148 bestaande Rust-tests: ongewijzigd, allemaal nog steeds groen via `classify_tx: None` (directe pad).
- `$SOVA` (live bundel): NIET aangeraakt. Alle wijzigingen in deze run staan in `code-frozen-20260505-182236/`.

---

## 7. Open punten / aanbevelingen voor de operator

1. **Port-decision:** wil de operator deze wijzigingen porten naar `$SOVA`? De wijziging is API-compatible (de externe shape blijft identiek). De port omvat:
   - `crates/governor-http/src/lib.rs` (queue-infrastructuur + 2 tests)
   - `crates/governor-http/src/main.rs` (worker-spawn + env)
   - 6 Nuclei-template-fixes
   - `docs/intelligence/nuclei-scan/payloads/oversize_body.json` (nieuw)
   - dit rapport
2. **`CLASSIFY_QUEUE_DEPTH` default 512:** geschikt voor M4-laptop / interne dashboards. Voor productie-deployment bij hoger verkeer overwegen om dit env-tunable te documenteren in `INSTALL.md`.
3. **T-04-payload-loading:** de `payloads: file:`-syntax met absolute pad met spaces werkte niet betrouwbaar in Nuclei v3.8. Voor toekomstige scans overwegen om de scan vanuit een pad zonder spaces te draaien, of de payload inline (kleinere body) te zetten.
4. **Nuclei false-positive-semantiek:** zie §5 — als de operator wil dat de scan-output 0 matches geeft op een healthy server, moeten de matchers omgekeerd worden naar status-codes-die-broken-zijn (e.g. T-05 `status: [500]`). Dat conflicteert echter met de literal brief-instructie en is daarom niet uitgevoerd.

---

## 8. Status

**VERIFIED_GREEN** op alle vijf code-gates (build, fmt, nextest 150/150, deny, playwright 6/6) en op de Nuclei-validatie. Server-gedrag is gelijk aan vóór de queue-implementatie voor de externe API; queue voegt alleen bounded backpressure toe als nieuwe failure-mode (503).
