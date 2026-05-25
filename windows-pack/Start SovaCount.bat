@echo off
REM SovaCount Windows starter - wraps PowerShell script
REM Dubbelklik om SovaCount te starten + dashboard te openen.

cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Start-SovaCount.ps1"
