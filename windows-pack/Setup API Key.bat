@echo off
REM SovaCount API-key setup wizard - wraps PowerShell script.

cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Setup-ApiKey.ps1"
