# SovaCount Windows stopper — kill governor-http process.

$ErrorActionPreference = 'Continue'

$PidFile = Join-Path $env:TEMP 'sovacount.pid'
$Stopped = $false

if (Test-Path $PidFile) {
    $TargetPid = (Get-Content $PidFile -Raw).Trim()
    if ($TargetPid -match '^\d+$') {
        try {
            $proc = Get-Process -Id $TargetPid -ErrorAction Stop
            if ($proc.ProcessName -match 'governor-http') {
                Stop-Process -Id $TargetPid -Force -ErrorAction Stop
                Write-Host "✓ Gestopt: PID $TargetPid (via PID-file)" -ForegroundColor Green
                $Stopped = $true
            }
        } catch {
            Write-Host "PID-file wijst naar $TargetPid maar process bestaat niet meer" -ForegroundColor DarkGray
        }
    }
    Remove-Item $PidFile -ErrorAction SilentlyContinue
}

# Fallback: zoek alle governor-http processes
$Processes = Get-Process -Name 'governor-http' -ErrorAction SilentlyContinue
if ($Processes) {
    foreach ($p in $Processes) {
        try {
            Stop-Process -Id $p.Id -Force -ErrorAction Stop
            Write-Host "✓ Gestopt: PID $($p.Id) (via process-name)" -ForegroundColor Green
            $Stopped = $true
        } catch {
            Write-Host "✗ Kon PID $($p.Id) niet stoppen: $_" -ForegroundColor Red
        }
    }
}

if (-not $Stopped) {
    Write-Host "Geen draaiend governor-http process gevonden." -ForegroundColor DarkGray
}

Start-Sleep -Seconds 1
