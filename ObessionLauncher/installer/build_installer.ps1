# Builds the Flutter Windows app and packages it into an Inno Setup installer.
# Requires: portable Inno Setup 7 in installer/tools/is7 (downloaded automatically if missing).

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $projectRoot "build\windows\x64\runner\Release"
$is7Dir = Join-Path $PSScriptRoot "tools\is7"
$is7Installer = Join-Path $PSScriptRoot "tools\innosetup-7.0.1-beta-x64.exe"
$iscc = Join-Path $is7Dir "ISCC.exe"
$iss = Join-Path $PSScriptRoot "obsession.iss"

# 1. Build Flutter Windows release
Write-Host "Building Flutter Windows release..." -ForegroundColor Cyan
Set-Location $projectRoot
& flutter build windows --release
if ($LASTEXITCODE -ne 0) { throw "Flutter build failed" }

# 2. Ensure Inno Setup compiler is available
if (-not (Test-Path $iscc)) {
    Write-Host "Inno Setup compiler not found. Downloading portable version..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Path (Split-Path $is7Installer -Parent) -Force | Out-Null
    $url = "https://github.com/jrsoftware/issrc/releases/download/is-7_0_1/innosetup-7.0.1-beta-x64.exe"
    Invoke-WebRequest -Uri $url -OutFile $is7Installer -UseBasicParsing
    Write-Host "Installing portable Inno Setup..." -ForegroundColor Yellow
    & $is7Installer /SILENT /PORTABLE=1 /DIR="$is7Dir"
}

# 3. Compile installer
Write-Host "Compiling installer..." -ForegroundColor Cyan
& $iscc $iss
if ($LASTEXITCODE -ne 0) { throw "Installer compilation failed" }

Write-Host "Done. Installer:" -ForegroundColor Green
Get-ChildItem (Join-Path $PSScriptRoot "Obsession-*.exe") | Select-Object FullName, Length
