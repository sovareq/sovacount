# SovaCount Windows starter — start governor-http in background, open dashboard.
#
# Resolutie-volgorde voor binary:
#   1. ..\bin\governor-http.exe (USB-pack)
#   2. governor-http.exe in PATH
#   3. %USERPROFILE%\.local\bin\governor-http.exe
#
# Provider-keuze:
#   - %APPDATA%\sovacount\anthropic-key bestaat → provider=anthropic + key uit file
#   - Anders → provider=mock (lokale heuristieken, geen LLM-call)

$ErrorActionPreference = 'Stop'

# Pad-resolutie
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$PackDir   = Split-Path -Parent $ScriptDir
$BinPath   = Join-Path $PackDir 'bin\governor-http.exe'

if (-not (Test-Path $BinPath)) {
    $BinPath = (Get-Command governor-http.exe -ErrorAction SilentlyContinue).Source
}
if (-not $BinPath -or -not (Test-Path $BinPath)) {
    $BinPath = Join-Path $env:USERPROFILE '.local\bin\governor-http.exe'
}
if (-not (Test-Path $BinPath)) {
    Write-Host '✗ governor-http.exe niet gevonden.' -ForegroundColor Red
    Write-Host "Zocht in:"
    Write-Host "  - $PackDir\bin\"
    Write-Host "  - PATH"
    Write-Host "  - $env:USERPROFILE\.local\bin\"
    Write-Host ""
    Write-Host "Bouw zelf vanuit source met: cargo build --release -p governor-http"
    Read-Host "Druk Enter om af te sluiten"
    exit 1
}

# Check of server al draait
try {
    $health = Invoke-WebRequest -Uri 'http://127.0.0.1:8989/health' -TimeoutSec 1 -UseBasicParsing -ErrorAction Stop
    if ($health.StatusCode -eq 200) {
        Write-Host '✓ SovaCount draait al op 127.0.0.1:8989' -ForegroundColor Green
        Start-Process 'http://127.0.0.1:8989/'
        Start-Sleep -Seconds 1
        exit 0
    }
} catch {
    # Server is down — start hem
}

# Provider-keuze
$KeyPath = Join-Path $env:APPDATA 'sovacount\anthropic-key'
$Provider = 'mock'
$ApiKey = $null

if (Test-Path $KeyPath) {
    $ApiKey = (Get-Content $KeyPath -Raw).Trim()
    if ($ApiKey -and $ApiKey.StartsWith('sk-ant-')) {
        $Provider = 'anthropic'
        Write-Host "[sovacount] provider=anthropic (key uit $KeyPath)" -ForegroundColor Cyan
    } else {
        Write-Host "[sovacount] WAARSCHUWING: $KeyPath bestaat maar bevat geen geldige Anthropic-key, terugval naar mock" -ForegroundColor Yellow
        $ApiKey = $null
    }
} else {
    Write-Host "[sovacount] provider=mock (geen API-key gevonden, gebruik 'Setup API Key.bat' voor real Anthropic-classify)" -ForegroundColor DarkGray
}

# Start server in background
$EnvVars = @{
    GOVERNOR_PROVIDER = $Provider
    GOVERNOR_HTTP_BIND = '127.0.0.1:8989'
}
if ($ApiKey) {
    $EnvVars.GOVERNOR_API_KEY = $ApiKey
}

# Schrijf env-vars als omgevings-vars voor child process
foreach ($k in $EnvVars.Keys) {
    [Environment]::SetEnvironmentVariable($k, $EnvVars[$k], 'Process')
}

Write-Host "[sovacount] starten van $BinPath" -ForegroundColor Cyan
$Process = Start-Process -FilePath $BinPath -WindowStyle Hidden -PassThru
$PidFile = Join-Path $env:TEMP 'sovacount.pid'
$Process.Id | Out-File -FilePath $PidFile -Encoding ascii
Write-Host "[sovacount] gestart, PID=$($Process.Id), PID-file=$PidFile" -ForegroundColor Cyan

# Wacht op health
$timeout = 10
for ($i = 0; $i -lt $timeout; $i++) {
    Start-Sleep -Milliseconds 500
    try {
        $health = Invoke-WebRequest -Uri 'http://127.0.0.1:8989/health' -TimeoutSec 1 -UseBasicParsing -ErrorAction Stop
        if ($health.StatusCode -eq 200) {
            Write-Host "✓ Server is up — opening dashboard..." -ForegroundColor Green
            Start-Process 'http://127.0.0.1:8989/'
            Start-Sleep -Seconds 2
            exit 0
        }
    } catch {
        # nog niet ready
    }
}

Write-Host "✗ Server timeout — health endpoint reageert niet binnen 5s" -ForegroundColor Red
Write-Host "  Process PID = $($Process.Id) (mogelijk gefaald — check Task Manager)"
Read-Host "Druk Enter om af te sluiten"
exit 1
