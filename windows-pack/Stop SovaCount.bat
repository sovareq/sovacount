@echo off
REM SovaCount Windows stopper - wraps PowerShell script.

cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Stop-SovaCount.ps1"
