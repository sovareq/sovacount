# SovaCount — macOS distributie

Dit document beschrijft hoe je SovaCount bouwt, ondertekent en verspreidt voor
macOS. De pipeline is **vendor-onafhankelijk** (geen Tauri-bundler) en werkt
zowel lokaal (ad-hoc signing) als via GitHub Actions (Developer ID +
notarization).

## Drie distributie-niveaus

| Niveau | Signing | Audience | Friction op verse Mac |
|---|---|---|---|
| **ad-hoc** | `codesign --sign -` | Eigen machine, dev-iteraties | Hoog: `xattr -dr com.apple.quarantine` nodig |
| **Developer ID** | Apple-cert, geen notarize | Interne pilots (5-10 mensen, gedeelde Apple Account) | Middel: één "Open Anyway" via System Settings |
| **Notarized** | Developer ID + Apple notary + stapled | Publieke open-source release | Geen — dubbelklik werkt direct |

## Bouwen — één commando

```bash
# Default: ad-hoc (geen Apple-account nodig)
./scripts/package-macos.sh

# Met Developer ID certificaat (zonder notarize)
export SIGN_MODE=developer-id
export SIGNING_IDENTITY="Developer ID Application: Jouw Naam (TEAMID)"
./scripts/package-macos.sh

# Volledig notarized (vereist eerst `notarytool store-credentials`)
export SIGN_MODE=notarize
export SIGNING_IDENTITY="Developer ID Application: Jouw Naam (TEAMID)"
export NOTARY_PROFILE=sovacount-notary
./scripts/package-macos.sh
```

Output: `dist/SovaCount.app` en `dist/SovaCount.zip`.

## Apple Developer ID — enrollment

`USD 99/jaar` via [developer.apple.com/programs/enroll](https://developer.apple.com/programs/enroll/).
Een rechtspersoon is **niet** vereist — individuele inschrijving werkt met:

- Apple Account met 2FA
- Wettelijke persoonsnaam (verschijnt als "seller name")
- Telefoonnummer, geen P.O. box

**Belangrijk vóór enrollment:** je kiest tussen "Individual" of "Organization".
Bij Individual wordt je persoonlijke naam de seller-naam. Bij Organization is
een DUNS-nummer + rechtspersoon vereist. Apple ondersteunt een latere
organization-transfer als de rechtspersoon ontstaat.

Doorlooptijd: doorgaans 1-3 werkdagen na betaling.

## notarytool — eenmalige setup

Na enrollment:

1. Maak in [App Store Connect](https://appstoreconnect.apple.com/access/users)
   een API-key aan (Users and Access → Integrations → Team Keys).
   Bewaar de `.p8` file, key-ID (10 chars) en issuer-UUID.

2. Sla credentials op in de macOS keychain:

   ```bash
   xcrun notarytool store-credentials sovacount-notary \
     --key      ~/Downloads/AuthKey_XXXXXXXXXX.p8 \
     --key-id   XXXXXXXXXX \
     --issuer   12345678-1234-1234-1234-123456789012
   ```

3. Daarna werkt `SIGN_MODE=notarize ./scripts/package-macos.sh` zonder
   verdere interactie.

## GitHub Actions

De workflow `.github/workflows/macos-release.yml` automatiseert het hele
proces voor `git tag` pushes (`v*.*.*`). Required repository-secrets:

| Secret | Wat |
|---|---|
| `APPLE_DEVELOPER_ID_CERT_P12` | Base64-encoded `.p12` (cert + private key) |
| `APPLE_DEVELOPER_ID_CERT_PASSWORD` | Password van de `.p12` |
| `APPLE_NOTARY_API_KEY_P8` | Base64-encoded `.p8` API-key |
| `APPLE_NOTARY_API_KEY_ID` | 10-char key-ID |
| `APPLE_NOTARY_API_ISSUER_ID` | Issuer-UUID |
| `SOVACOUNT_SIGNING_IDENTITY` | Full identity-string |

**Base64-encode een file:**

```bash
base64 -i cert.p12 | pbcopy   # plak in GitHub Secret
```

Als de Apple-secrets nog niet gezet zijn, falt de workflow terug op ad-hoc
signing — handig voor eerste pilots vóór de Developer ID approval.

## Ad-hoc signing — wat het wel en niet doet

Op Sequoia 15.5+ doet macOS automatisch een ad-hoc sign op een ongetekende
binary bij eerste run, **mits er geen quarantine-xattr op staat**. Concreet:

- **Werkt zonder prompt:** `git clone` + `cargo build` + Finder-launch op
  dezelfde Mac waarop je gebouwd hebt.
- **Geeft Gatekeeper-prompt:** zip-download via browser (browser zet
  quarantine), naar verse Mac kopiëren, AirDrop.

Voor downloads: instructeer pilot-users:

```bash
xattr -dr com.apple.quarantine /Applications/SovaCount.app
```

## Hardened Runtime — entitlements

`crates/governor-launcher-gui/entitlements.plist` bevat:

- `com.apple.security.cs.allow-jit` — WKWebView/V8 JIT
- `com.apple.security.cs.allow-unsigned-executable-memory` — WebKit JIT pages
- `com.apple.security.network.client` — uitgaande HTTPS naar Anthropic/OpenAI/Ollama

**Geen sandbox** (`com.apple.security.app-sandbox` ontbreekt bewust) —
voor Developer ID buiten App Store is sandbox niet vereist, en zou
filesystem-toegang tot `~/.config/sovacount/` blokkeren.

## Process-lifecycle (T-G-launcher-prod)

De launcher heeft drie verdedigingslagen tegen orphan `governor-http` servers:

1. **`ChildGuard` Drop-impl** — `kill_tree::kill_tree(pid)` op de hele subtree
   bij elke exit-pad (normaal, panic, stack-unwind).
2. **`signal-hook` SIGTERM/SIGINT** — OS-kill zet shutdown-flag → tao-loop
   exit'et → `ChildGuard::drop` runt.
3. **`tao::Event::LoopDestroyed`** — vangt force-close-paden die
   `CloseRequested` niet triggeren (Cmd+Q via app-menu, OS-shutdown).

Plus:

- `single-instance` lock op `/tmp` voorkomt dat Finder-double-click een
  tweede launcher spawnt.
- `SOVACOUNT_LAUNCHER_GUARD=1` env-var voorkomt recursieve respawn.
- **Geen `pkill -f governor-http`** — dat killt processen van andere users
  op multi-user machines. PID-tracking only.

## Logging

De launcher schrijft naar `~/Library/Logs/SovaCount.log`. Niveau filterbaar
via env-var:

```bash
SOVACOUNT_LOG=debug open /Applications/SovaCount.app
```

Tail tijdens dev:

```bash
tail -f ~/Library/Logs/SovaCount.log
```

## Verificatie na bouw

```bash
codesign --verify --strict --verbose=4 dist/SovaCount.app
spctl --assess --type execute --verbose dist/SovaCount.app
```

Bij notarized builds zou `spctl` `accepted` moeten geven. Bij ad-hoc:
`rejected` is normaal — Gatekeeper accepteert ad-hoc niet, alleen de OS-runtime.
