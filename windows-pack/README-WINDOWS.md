# SovaCount — Windows pack

Stand-alone Windows-distributie van SovaCount. **Geen Claude Code, geen Rust toolchain, geen build-stap nodig** — pre-built binaries zitten in deze map.

## Snel-start (USB-stick)

1. Plug de USB in een Windows-machine (10/11, 64-bit).
2. Open de USB-stick in Verkenner.
3. Dubbelklik **`Start SovaCount.bat`**.
4. Een PowerShell-venster opent (kort), de classify-server start in de achtergrond, en je default-browser opent automatisch op `http://127.0.0.1:8989/`.
5. Klaar — je ziet het SovaCount dashboard.

Stoppen: dubbelklik **`Stop SovaCount.bat`**.

## Anthropic API-key (optioneel — voor echte LLM-classificatie)

Zonder key draait SovaCount in **mock mode**: lokale heuristieken classifieren scopes zonder ooit een echte API te bellen. Voor 80% van duidelijke scopes is dit voldoende.

Voor **echte Anthropic Haiku-classify-calls op edge-cases**:

1. Maak een API-key op <https://console.anthropic.com/settings/keys> (scope: minstens `claude-haiku` toegang).
2. Dubbelklik **`Setup API Key.bat`**.
3. Plak je key in het venster, druk Enter. Key wordt opgeslagen in `%APPDATA%\sovacount\anthropic-key` met restricted ACL (alleen huidige Windows-user kan lezen).
4. Stop + restart SovaCount. Dashboard toont nu "anthropic provider" ipv "mock provider".

## Wat zit erin

```
sovacount-windows-pack/
├── bin/                          (pre-built Windows binaries, x86_64)
│   ├── governor-http.exe        ← HTTP-server + dashboard (port 8989)
│   ├── tier-classify.exe        ← tier-classify CLI
│   └── governor-mcp.exe         ← MCP-stdio server voor Claude Code / Cursor
├── Start SovaCount.bat           ← dubbelklik om te starten
├── Stop SovaCount.bat            ← dubbelklik om te stoppen
├── Setup API Key.bat             ← eenmalige API-key wizard
├── scripts/                      (PowerShell-implementatie van bovenstaande)
│   ├── Start-SovaCount.ps1
│   ├── Stop-SovaCount.ps1
│   └── Setup-ApiKey.ps1
├── icons/
│   └── sovacount.ico             ← voor desktop-snelkoppeling / taskbar pin
└── README-WINDOWS.md             ← dit bestand
```

## Pin to Taskbar (handig)

1. Klik rechts op `Start SovaCount.bat` → "Send to" → "Desktop (create shortcut)".
2. Op het bureaublad: rechts-klik de snelkoppeling → "Properties" → "Change icon" → blader naar `icons\sovacount.ico` op de USB → OK.
3. Sleep de snelkoppeling naar je taskbar.
4. Eén klik om SovaCount te starten.

## MCP-integratie (Claude Code op Windows)

Heb je Claude Code op je Windows-machine? Voeg dit toe aan `%USERPROFILE%\.claude\settings.json`:

```json
{
  "mcpServers": {
    "token-governor": {
      "command": "E:\\sovacount-windows-pack\\bin\\governor-mcp.exe",
      "env": {
        "GOVERNOR_PROVIDER": "anthropic"
      }
    }
  }
}
```

Pas het pad `E:\\sovacount-windows-pack\\` aan naar waar de USB gemount is (of waar je de map permanent gekopieerd hebt). Daarna kan Claude Code de `governor_classify` MCP-tool gebruiken.

## Troubleshooting

### "Windows beschermt deze computer" SmartScreen-waarschuwing

De binaries zijn **niet code-signed** (intern Sovareq-gebruik, geen publieke release). Klik **More info → Run anyway**.

### Port 8989 bezet

Iets anders draait op poort 8989. Stop dat eerst (Resource Monitor → Network → vind het proces), of pas `Start SovaCount.bat` aan om een andere poort te gebruiken (`set GOVERNOR_HTTP_BIND=127.0.0.1:9000`).

### Firewall-prompt

Eerste keer dat `governor-http.exe` start vraagt Windows of het binnen mag op het netwerk. Kies **Private network only** (je hebt geen externe toegang nodig — alles loopt op localhost).

### Antivirus blokkeert binary

Sommige antivirus markeert unsigned Rust-binaries als verdacht (heuristieke false-positive). Voeg de `bin/` map toe aan een exclusion-rule, of bouw zelf vanuit source (zie volgende sectie).

### Zelf bouwen vanuit source

Voor gebruikers met Rust 1.94+ toolchain:

```powershell
git clone https://github.com/brainzzlab-hub/token-governor.git
cd token-governor
cargo build --release --workspace
copy target\release\governor-http.exe windows-pack\bin\
copy target\release\tier-classify.exe windows-pack\bin\
copy target\release\governor-mcp.exe windows-pack\bin\
```

De `governor-launcher-gui` crate is **niet** in de Windows-pack opgenomen omdat de WKWebView-equivalent (WebView2) extra runtime-setup vereist. Het CLI/HTTP/MCP-stack werkt zonder GUI-launcher — je beheert SovaCount via de `.bat` bestanden + browser-dashboard.

## License

Proprietary — see `LICENSE` in de root van de SovaCount-repository. Internal Sovareq use only. Contact: <bjorn@sovareq.com>.
