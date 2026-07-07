<#
.SYNOPSIS
    Builds Obsession into a single self-extracting EXE.
.DESCRIPTION
    1. Runs flutter build windows.
    2. Packs build\windows\x64\runner\Release into obsession.zip.
    3. Compiles a C# launcher with the zip embedded as a resource.
    4. Outputs a single Obsession.exe that extracts and runs the app.
.PARAMETER OutputPath
    Output path for the final EXE. Default: build\windows\x64\runner\Obsession.exe
.PARAMETER SkipBuild
    Skip flutter build if already built.
#>
param(
    [string]$OutputPath = "build\windows\x64\runner\Obsession.exe",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseDir = Join-Path $repoRoot "build\windows\x64\runner\Release"
$stagingDir = Join-Path $repoRoot "build\windows\x64\runner\sfx_staging"
$zipPath = Join-Path $stagingDir "obsession.zip"
$launcherSource = Join-Path $repoRoot "scripts\SingleExeLauncher.cs"
$targetFullPath = Join-Path $repoRoot $OutputPath

function Assert-Command {
    param([string]$Command)
    if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) {
        throw "Command '$Command' not found. Make sure it is available in PATH."
    }
}

Assert-Command "flutter"

$cscPath = "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if (-not (Test-Path $cscPath)) {
    $cscPath = "C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe"
}
if (-not (Test-Path $cscPath)) {
    throw "csc.exe not found. .NET Framework 4.x is required."
}

if (-not $SkipBuild) {
    Write-Host "==> Building Flutter Windows release..." -ForegroundColor Cyan
    Set-Location $repoRoot
    flutter build windows
}

if (-not (Test-Path $releaseDir)) {
    throw "Release folder not found: $releaseDir"
}

Write-Host "==> Preparing staging..." -ForegroundColor Cyan
if (Test-Path $stagingDir) {
    Remove-Item -Recurse -Force $stagingDir
}
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null

Write-Host "==> Archiving Release folder..." -ForegroundColor Cyan
Compress-Archive -Path "$releaseDir\*" -DestinationPath $zipPath -Force

$zipSize = [math]::Round((Get-Item $zipPath).Length / 1MB, 2)
Write-Host "    Zip size: $zipSize MB" -ForegroundColor DarkGray

Write-Host "==> Compiling single EXE launcher..." -ForegroundColor Cyan
$outputDir = Split-Path -Parent $targetFullPath
if (-not (Test-Path $outputDir)) {
    New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
}
if (Test-Path $targetFullPath) {
    Remove-Item -Force $targetFullPath
}

& $cscPath `
    /nologo `
    /target:winexe `
    /out:"$targetFullPath" `
    /resource:"$zipPath",obsession.zip `
    /reference:"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\System.IO.Compression.dll" `
    /reference:"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\System.IO.Compression.FileSystem.dll" `
    /reference:"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\System.Windows.Forms.dll" `
    "$launcherSource"

if ($LASTEXITCODE -ne 0) {
    throw "csc.exe exited with code $LASTEXITCODE"
}

if (-not (Test-Path $targetFullPath)) {
    throw "csc.exe did not create EXE: $targetFullPath"
}

$size = [math]::Round((Get-Item $targetFullPath).Length / 1MB, 2)
Write-Host "==> Done: $targetFullPath" -ForegroundColor Green
Write-Host "    Size: $size MB" -ForegroundColor Green

Remove-Item -Recurse -Force $stagingDir
