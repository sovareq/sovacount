# SovaCount Windows API-key setup — eenmalige wizard voor Anthropic API-key.
#
# Slaat key op in %APPDATA%\sovacount\anthropic-key met restricted ACL
# (alleen huidige user kan lezen).

$ErrorActionPreference = 'Stop'

Write-Host ''
Write-Host '================================================================' -ForegroundColor Cyan
Write-Host '  SovaCount — Anthropic API-key setup' -ForegroundColor Cyan
Write-Host '================================================================' -ForegroundColor Cyan
Write-Host ''
Write-Host 'Plak je Anthropic API-key (sk-ant-...) en druk Enter.'
Write-Host 'De key wordt opgeslagen in:'
Write-Host "  $env:APPDATA\sovacount\anthropic-key"
Write-Host ''
Write-Host 'Toegang: alleen huidige Windows-user (ACL-restricted).'
Write-Host 'Maak een nieuwe key op: https://console.anthropic.com/settings/keys'
Write-Host ''

$SecureKey = Read-Host 'API-key' -AsSecureString
$BSTR = [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($SecureKey)
$Key = [System.Runtime.InteropServices.Marshal]::PtrToStringAuto($BSTR)
[System.Runtime.InteropServices.Marshal]::ZeroFreeBSTR($BSTR)

if (-not $Key.StartsWith('sk-ant-')) {
    Write-Host '✗ Key begint niet met "sk-ant-" — afgebroken.' -ForegroundColor Red
    Read-Host 'Druk Enter om af te sluiten'
    exit 1
}

# Maak de directory aan
$KeyDir = Join-Path $env:APPDATA 'sovacount'
if (-not (Test-Path $KeyDir)) {
    New-Item -Path $KeyDir -ItemType Directory | Out-Null
}
$KeyPath = Join-Path $KeyDir 'anthropic-key'

# Schrijf key
[System.IO.File]::WriteAllText($KeyPath, $Key.Trim())

# Restricted ACL: alleen huidige user heeft Read+Write
$Acl = Get-Acl $KeyPath
$Acl.SetAccessRuleProtection($true, $false)  # Disable inheritance, remove inherited
$Acl.Access | ForEach-Object { $Acl.RemoveAccessRule($_) | Out-Null }
$UserSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$UserAccount = $UserSid.Translate([System.Security.Principal.NTAccount])
$Rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
    $UserAccount, 'Read,Write', 'Allow'
)
$Acl.AddAccessRule($Rule)
Set-Acl -Path $KeyPath -AclObject $Acl

Write-Host ''
Write-Host "✓ Key opgeslagen in: $KeyPath" -ForegroundColor Green
Write-Host '✓ ACL: alleen huidige user kan lezen' -ForegroundColor Green

# Test of de key werkt
Write-Host ''
Write-Host 'Test: 1 classify-call naar Anthropic Haiku om de key te valideren...' -ForegroundColor Cyan

$Body = @{
    model = 'claude-haiku-4-5'
    max_tokens = 10
    messages = @(
        @{
            role = 'user'
            content = 'Hi, just verifying my API key. Reply with one word: ok.'
        }
    )
} | ConvertTo-Json -Depth 4

$Headers = @{
    'x-api-key' = $Key
    'anthropic-version' = '2023-06-01'
    'content-type' = 'application/json'
}

try {
    $Response = Invoke-RestMethod -Uri 'https://api.anthropic.com/v1/messages' `
        -Method Post -Body $Body -Headers $Headers -TimeoutSec 10 -ErrorAction Stop
    Write-Host ''
    Write-Host '✓ API-key WERKT — Anthropic antwoordde.' -ForegroundColor Green
    Write-Host "  Response: $($Response.content[0].text.Trim())" -ForegroundColor DarkGray
} catch {
    Write-Host ''
    Write-Host '✗ API-key validatie GEFAALD:' -ForegroundColor Red
    Write-Host "  $_" -ForegroundColor Red
    Write-Host ''
    Write-Host 'Mogelijke oorzaken:'
    Write-Host '  - Key is ongeldig of verlopen'
    Write-Host '  - Key heeft geen Haiku-model toegang'
    Write-Host '  - Geen internet / firewall blokkeert api.anthropic.com'
    Write-Host ''
    $Remove = Read-Host 'Wil je de zojuist opgeslagen key verwijderen? (j/n)'
    if ($Remove -eq 'j' -or $Remove -eq 'J') {
        Remove-Item $KeyPath -Force
        Write-Host "✓ Key verwijderd uit $KeyPath" -ForegroundColor Green
    }
    Read-Host 'Druk Enter om af te sluiten'
    exit 1
}

Write-Host ''
Write-Host 'Klaar. Start SovaCount nu met "Start SovaCount.bat" — dashboard toont' -ForegroundColor Cyan
Write-Host '"anthropic provider" ipv "mock provider".' -ForegroundColor Cyan
Write-Host ''
Read-Host 'Druk Enter om af te sluiten'
