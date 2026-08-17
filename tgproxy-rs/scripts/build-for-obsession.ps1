[CmdletBinding()]
param(
    [switch]$SkipRuntimeServiceBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$proxyRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = Split-Path -Parent $proxyRoot
$obsessionRoot = Join-Path $repoRoot "ObsessionTauri"
$manifest = Join-Path $proxyRoot "Cargo.toml"
$sourceExe = Join-Path $proxyRoot "target\release\obsession-tg-proxy.exe"
$resourceExe = Join-Path $obsessionRoot "src-tauri\resources\bin\obsession-tg-proxy.exe"
$licenseRoot = Join-Path $obsessionRoot "src-tauri\resources\licenses"

& cargo build --manifest-path $manifest --locked --release
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
    throw "Release proxy was not produced: $sourceExe"
}

Copy-Item -LiteralPath $sourceExe -Destination $resourceExe -Force
New-Item -ItemType Directory -Path $licenseRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $proxyRoot "LICENSE") `
    -Destination (Join-Path $licenseRoot "tgproxy-rs-LICENSE.txt") -Force
Copy-Item -LiteralPath (Join-Path $proxyRoot "NOTICE") `
    -Destination (Join-Path $licenseRoot "tgproxy-rs-NOTICE.txt") -Force

$prepare = Join-Path $obsessionRoot "scripts\prepare-runtime.mjs"
$prepareArgs = @($prepare)
if ($SkipRuntimeServiceBuild) {
    $prepareArgs += "--skip-build"
}
& node @prepareArgs
if ($LASTEXITCODE -ne 0) {
    throw "prepare-runtime failed with exit code $LASTEXITCODE"
}

$artifact = Get-Item -LiteralPath $resourceExe
$sha256 = (Get-FileHash -LiteralPath $resourceExe -Algorithm SHA256).Hash
Write-Host "obsession-tg-proxy ready: $($artifact.FullName)"
Write-Host "size=$($artifact.Length) sha256=$sha256"
