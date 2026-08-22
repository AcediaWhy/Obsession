[CmdletBinding()]
param(
    [Parameter()]
    [string]$OutputDirectory,

    [Parameter()]
    [string]$WorkRoot,

    [Parameter()]
    [switch]$AllowArtifactHashChange
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-LastExitCode {
    param([Parameter(Mandatory)][string]$Operation)
    if ($LASTEXITCODE -ne 0) {
        throw "$Operation failed with exit code $LASTEXITCODE"
    }
}

function Get-Sha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Hash {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Expected
    )
    $actual = Get-Sha256 -Path $Path
    if ($actual -ne $Expected.ToLowerInvariant()) {
        throw "SHA-256 mismatch for '$Path': expected $Expected, got $actual"
    }
}

$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$patchRoot = Join-Path $projectRoot 'third-party\zapret2'
$lockPath = Join-Path $patchRoot 'upstream.lock.json'
$lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json

if (-not $WorkRoot) {
    $WorkRoot = Join-Path ([System.IO.Path]::GetTempPath()) 'obsession-zapret2-build'
}
$WorkRoot = [System.IO.Path]::GetFullPath($WorkRoot)
New-Item -ItemType Directory -Force -Path $WorkRoot | Out-Null
$runRoot = Join-Path $WorkRoot ("run-{0:yyyyMMdd-HHmmss}-{1}" -f (Get-Date), $PID)
New-Item -ItemType Directory -Path $runRoot | Out-Null

if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $runRoot 'output'
}
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$sourceRoot = Join-Path $runRoot 'source'
& git clone --quiet --no-checkout --filter=blob:none -- $lock.upstream.repository $sourceRoot
Assert-LastExitCode 'Cloning zapret2'
& git -C $sourceRoot checkout --quiet --detach $lock.upstream.commit
Assert-LastExitCode 'Checking out pinned zapret2 commit'
$actualCommit = (& git -C $sourceRoot rev-parse 'HEAD').Trim()
Assert-LastExitCode 'Reading zapret2 commit'
if ($actualCommit -ne $lock.upstream.commit) {
    throw "Unexpected zapret2 commit: expected $($lock.upstream.commit), got $actualCommit"
}

$patchPaths = @()
foreach ($patch in $lock.patchset.patches) {
    $patchPath = Join-Path $patchRoot (Join-Path 'patches' $patch.file)
    Assert-Hash -Path $patchPath -Expected $patch.sha256
    $patchPaths += $patchPath
}
& git -C $sourceRoot apply --check -- @patchPaths
Assert-LastExitCode 'Checking zapret2 patch applicability'
& git -c user.name='Obsession Patch Builder' -c user.email='patches@obsession.local' -C $sourceRoot am --quiet -- @patchPaths
Assert-LastExitCode 'Applying zapret2 patch series'

$windowsReleaseArchive = Join-Path $runRoot 'zapret2-windows-release.zip'
Invoke-WebRequest -UseBasicParsing -Uri $lock.windows_release.url -OutFile $windowsReleaseArchive
Assert-Hash -Path $windowsReleaseArchive -Expected $lock.windows_release.sha256
$windowsReleaseRoot = Join-Path $runRoot 'zapret2-windows-release'
Expand-Archive -LiteralPath $windowsReleaseArchive -DestinationPath $windowsReleaseRoot
$windivertSource = Join-Path $windowsReleaseRoot $lock.windows_release.windivert_dll.path
if (-not (Test-Path -LiteralPath $windivertSource -PathType Leaf)) {
    throw "Pinned WinDivert.dll is missing from the zapret2 Windows release: $windivertSource"
}
if ((Get-Item -LiteralPath $windivertSource).Length -ne $lock.windows_release.windivert_dll.size) {
    throw "Pinned WinDivert.dll size does not match upstream.lock.json"
}
Assert-Hash -Path $windivertSource -Expected $lock.windows_release.windivert_dll.sha256

$setupPath = Join-Path $runRoot 'setup-x86_64.exe'
Invoke-WebRequest -UseBasicParsing -Uri $lock.cygwin.setup_url -OutFile $setupPath
Assert-Hash -Path $setupPath -Expected $lock.cygwin.setup_sha256
$signature = Get-AuthenticodeSignature -LiteralPath $setupPath
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Cygwin|Jon Turney') {
    throw "Cygwin bootstrap signature is not trusted: $($signature.Status), $($signature.SignerCertificate.Subject)"
}

$cygwinRoot = Join-Path $runRoot 'cygwin'
$packageCache = Join-Path $runRoot 'cygwin-cache'
New-Item -ItemType Directory -Force -Path $packageCache | Out-Null
$packageNames = @($lock.cygwin.packages.PSObject.Properties.Name)
$quotedCygwinRoot = '"' + $cygwinRoot + '"'
$quotedPackageCache = '"' + $packageCache + '"'
$setupArgs = @(
    '--quiet-mode', '--no-admin', '--no-shortcuts', '--no-startmenu', '--no-desktop',
    '--root', $quotedCygwinRoot,
    '--local-package-dir', $quotedPackageCache,
    '--site', $lock.cygwin.mirror,
    '--packages', ($packageNames -join ',')
)
$setupProcess = Start-Process -FilePath $setupPath -ArgumentList $setupArgs -Wait -PassThru -WindowStyle Hidden
if ($setupProcess.ExitCode -ne 0) {
    throw "Cygwin bootstrap failed with exit code $($setupProcess.ExitCode)"
}

$installedDb = Join-Path $cygwinRoot 'etc\setup\installed.db'
$installed = @{}
foreach ($line in Get-Content -LiteralPath $installedDb) {
    if ($line -match '^(\S+)\s+\1-(.+)\.tar\.(?:bz2|xz|zst)\s+\d+$') {
        $installed[$Matches[1]] = $Matches[2]
    }
}
foreach ($package in $lock.cygwin.packages.PSObject.Properties) {
    if (-not $installed.ContainsKey($package.Name) -or $installed[$package.Name] -ne $package.Value) {
        $actual = if ($installed.ContainsKey($package.Name)) { $installed[$package.Name] } else { '<missing>' }
        throw "Cygwin package drift for $($package.Name): expected $($package.Value), got $actual"
    }
}

$luaArchive = Join-Path $runRoot ("luajit2-{0}.tar.gz" -f $lock.luajit.version)
Invoke-WebRequest -UseBasicParsing -Uri $lock.luajit.url -OutFile $luaArchive
Assert-Hash -Path $luaArchive -Expected $lock.luajit.sha256

$bashPath = Join-Path $cygwinRoot 'bin\bash.exe'
$env:OBSESSION_ZAPRET_SOURCE = $sourceRoot
$env:OBSESSION_LUA_ARCHIVE = $luaArchive
$env:OBSESSION_BUILD_ROOT = $runRoot
$env:OBSESSION_SOURCE_DATE_EPOCH = [string]$lock.upstream.source_date_epoch
$env:OBSESSION_PATCHSET_VERSION = [string]$lock.patchset.version
$env:OBSESSION_LUA_VERSION = [string]$lock.luajit.version
$env:OBSESSION_UPSTREAM_TAG = [string]$lock.upstream.tag
$env:OBSESSION_UPSTREAM_SHORT = ([string]$lock.upstream.commit).Substring(0, 7)
try {
    $buildScript = @'
set -euo pipefail
export PATH=/usr/local/bin:/usr/bin
source_root="$(cygpath -u "$OBSESSION_ZAPRET_SOURCE")"
lua_archive="$(cygpath -u "$OBSESSION_LUA_ARCHIVE")"
build_root="$(cygpath -u "$OBSESSION_BUILD_ROOT")"
lua_version="$OBSESSION_LUA_VERSION"
cd "$build_root"
tar -xzf "$lua_archive"
make -C "luajit2-$lua_version" \
  BUILDMODE=static \
  XCFLAGS="-DLUAJIT_USE_SYSMALLOC -DLUAJIT_DISABLE_FFI -ffat-lto-objects" \
  TARGET_CFLAGS="-Os -flto=auto -ffunction-sections -fdata-sections" \
  TARGET_LDFLAGS="-Wl,--gc-sections -flto=auto" \
  -j4
make -C "luajit2-$lua_version" install
export SOURCE_DATE_EPOCH="$OBSESSION_SOURCE_DATE_EPOCH"
export CFLAGS="-DZAPRET_GH_VER=obsession-$OBSESSION_UPSTREAM_TAG-$OBSESSION_PATCHSET_VERSION -DZAPRET_GH_HASH=$OBSESSION_UPSTREAM_SHORT-$OBSESSION_PATCHSET_VERSION"
make -C "$source_root/nfq2" clean
make -C "$source_root/nfq2" cygwin -j4
test -f "$source_root/nfq2/winws2.exe"
objdump -p "$source_root/nfq2/winws2.exe" > "$build_root/pe-headers.txt"
'@
    & $bashPath --noprofile --norc -lc $buildScript
    Assert-LastExitCode 'Building patched winws2'
}
finally {
    Remove-Item Env:OBSESSION_ZAPRET_SOURCE -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_LUA_ARCHIVE -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_BUILD_ROOT -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_SOURCE_DATE_EPOCH -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_PATCHSET_VERSION -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_LUA_VERSION -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_UPSTREAM_TAG -ErrorAction SilentlyContinue
    Remove-Item Env:OBSESSION_UPSTREAM_SHORT -ErrorAction SilentlyContinue
}

$peHeaders = Get-Content -LiteralPath (Join-Path $runRoot 'pe-headers.txt') -Raw
foreach ($requiredFlag in @('HIGH_ENTROPY_VA', 'DYNAMIC_BASE', 'NX_COMPAT', '__stack_chk_fail', '__stack_chk_guard')) {
    if ($peHeaders -notmatch [regex]::Escape($requiredFlag)) {
        throw "Built winws2.exe is missing required PE mitigation/import: $requiredFlag"
    }
}

$objdumpPath = Join-Path $cygwinRoot 'bin\objdump.exe'
$windivertPeHeaders = (& $objdumpPath -p $windivertSource | Out-String)
Assert-LastExitCode 'Inspecting pinned WinDivert.dll'
if ($windivertPeHeaders -notmatch 'DYNAMIC_BASE') {
    throw 'Pinned WinDivert.dll is missing DYNAMIC_BASE'
}
if ($windivertPeHeaders -match 'HIGH_ENTROPY_VA') {
    throw 'Pinned WinDivert.dll unexpectedly enables HIGH_ENTROPY_VA; zapret2 v1.0.4 intentionally removed it for Cygwin compatibility'
}

$winwsSource = Join-Path $sourceRoot 'nfq2\winws2.exe'
$cygwinSource = Join-Path $cygwinRoot 'bin\cygwin1.dll'
$winwsOutput = Join-Path $OutputDirectory 'winws2.exe'
$cygwinOutput = Join-Path $OutputDirectory 'cygwin1.dll'
$windivertOutput = Join-Path $OutputDirectory 'WinDivert.dll'
Copy-Item -LiteralPath $winwsSource -Destination $winwsOutput -Force
Copy-Item -LiteralPath $cygwinSource -Destination $cygwinOutput -Force
Copy-Item -LiteralPath $windivertSource -Destination $windivertOutput -Force

$provenance = [ordered]@{
    schema_version = 1
    upstream_repository = [string]$lock.upstream.repository
    upstream_tag = [string]$lock.upstream.tag
    upstream_commit = [string]$lock.upstream.commit
    patchset_version = [string]$lock.patchset.version
    patches = @($lock.patchset.patches)
    luajit = $lock.luajit
    windows_release = $lock.windows_release
    cygwin = [ordered]@{
        setup_sha256 = [string]$lock.cygwin.setup_sha256
        packages = $lock.cygwin.packages
    }
    outputs = [ordered]@{
        winws2 = [ordered]@{
            size = (Get-Item -LiteralPath $winwsOutput).Length
            sha256 = Get-Sha256 -Path $winwsOutput
        }
        cygwin1 = [ordered]@{
            size = (Get-Item -LiteralPath $cygwinOutput).Length
            sha256 = Get-Sha256 -Path $cygwinOutput
        }
        windivert = [ordered]@{
            size = (Get-Item -LiteralPath $windivertOutput).Length
            sha256 = Get-Sha256 -Path $windivertOutput
        }
    }
}
$provenance | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'build-provenance.json') -Encoding utf8NoBOM

$artifactLockPath = Join-Path $patchRoot 'artifact.lock.json'
if (-not $AllowArtifactHashChange -and (Test-Path -LiteralPath $artifactLockPath)) {
    $artifactLock = Get-Content -LiteralPath $artifactLockPath -Raw | ConvertFrom-Json
    foreach ($name in @('winws2', 'cygwin1', 'windivert')) {
        $expectedOutput = $artifactLock.outputs.$name
        $actualOutput = $provenance.outputs.$name
        if ($expectedOutput.size -ne $actualOutput.size -or $expectedOutput.sha256 -ne $actualOutput.sha256) {
            throw "Artifact drift for ${name}: expected $($expectedOutput.size)/$($expectedOutput.sha256), got $($actualOutput.size)/$($actualOutput.sha256). Review the build and update artifact.lock.json deliberately, or use -AllowArtifactHashChange for the review build."
        }
    }
}

Write-Host "Patched zapret2 runtime built at: $OutputDirectory"
Write-Host "winws2.exe SHA-256: $($provenance.outputs.winws2.sha256)"
Write-Host "cygwin1.dll SHA-256: $($provenance.outputs.cygwin1.sha256)"
Write-Host "WinDivert.dll SHA-256: $($provenance.outputs.windivert.sha256)"
