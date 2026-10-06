@echo off
setlocal
cd /d "%~dp0.."
set "obsessionCaptureMode="
if /i "%~1"=="--preflight" set "obsessionCaptureMode=-Preflight"
where pwsh.exe >nul 2>&1
if errorlevel 1 (
  powershell.exe -NoLogo -NoProfile -File "scripts\capture-discord-startup.ps1" -CaptureSeconds 60 %obsessionCaptureMode%
) else (
  pwsh.exe -NoLogo -NoProfile -File "scripts\capture-discord-startup.ps1" -CaptureSeconds 60 %obsessionCaptureMode%
)
if not defined obsessionCaptureMode pause
