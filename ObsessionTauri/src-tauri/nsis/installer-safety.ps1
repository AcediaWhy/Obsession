$ErrorActionPreference = 'Stop'

$ownerId = 'com.vlarpsu.obsession'
$ownerMarker = '.obsession-install-owner'
$installDir = $env:OBSESSION_INSTALL_DIR
$action = $env:OBSESSION_SETUP_ACTION

function Get-NormalizedPath([string] $Path) {
    return [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar)
}

function Test-SameOrChild([string] $Candidate, [string] $Root) {
    if ($Candidate.Equals($Root, [StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    $prefix = $Root.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    return $Candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
}

function Assert-SafeInstallDirectory {
    if ([string]::IsNullOrWhiteSpace($installDir)) {
        throw 'Install directory is empty.'
    }

    $target = Get-NormalizedPath $installDir
    $root = [IO.Path]::GetPathRoot($target).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ($target.Equals($root, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Installing into a drive root is forbidden.'
    }

    foreach ($name in @('WINDIR', 'ProgramFiles', 'ProgramFiles(x86)', 'ProgramData', 'TEMP', 'TMP')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            $protected = Get-NormalizedPath $value
            if (Test-SameOrChild $target $protected) {
                throw "Installing into protected directory '$protected' is forbidden."
            }
        }
    }

    foreach ($name in @('USERPROFILE', 'APPDATA', 'LOCALAPPDATA')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if (-not [string]::IsNullOrWhiteSpace($value)) {
            $protected = Get-NormalizedPath $value
            if ($target.Equals($protected, [StringComparison]::OrdinalIgnoreCase) -or
                (Test-SameOrChild $protected $target)) {
                throw "Install directory '$target' is too broad."
            }
        }
    }

    $cursor = $target
    while (-not (Test-Path -LiteralPath $cursor)) {
        $parent = [IO.Directory]::GetParent($cursor)
        if ($null -eq $parent) {
            throw "No existing parent found for '$target'."
        }
        $cursor = $parent.FullName
    }

    while ($null -ne $cursor) {
        $item = Get-Item -Force -LiteralPath $cursor
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Install directory crosses a reparse point: '$cursor'."
        }
        $parent = [IO.Directory]::GetParent($cursor)
        $cursor = if ($null -eq $parent) { $null } else { $parent.FullName }
    }

    if (Test-Path -LiteralPath $target -PathType Leaf) {
        throw 'Install path points to a file.'
    }
    if (-not (Test-Path -LiteralPath $target -PathType Container)) {
        return
    }

    $hasEntries = $null -ne (Get-ChildItem -Force -LiteralPath $target | Select-Object -First 1)
    if (-not $hasEntries) {
        return
    }

    $marker = Join-Path $target $ownerMarker
    $owned = (Test-Path -LiteralPath $marker -PathType Leaf) -and
        ((Get-Content -Raw -LiteralPath $marker).Trim() -eq $ownerId)
    $legacy = (Test-Path -LiteralPath (Join-Path $target 'Obsession.exe') -PathType Leaf) -and
        (Test-Path -LiteralPath (Join-Path $target 'uninstall.exe') -PathType Leaf)
    if (-not $owned -and -not $legacy) {
        throw "Directory '$target' is not empty and is not owned by Obsession."
    }
}

function Assert-SafeApplicationStopDirectory {
    if ([string]::IsNullOrWhiteSpace($installDir)) {
        throw 'Install directory is empty.'
    }

    $programFiles = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
    if (-not [string]::IsNullOrWhiteSpace($programFiles)) {
        $machineRoot = Get-NormalizedPath (Join-Path $programFiles 'Obsession')
        $target = Get-NormalizedPath $installDir
        if ($target.Equals($machineRoot, [StringComparison]::OrdinalIgnoreCase)) {
            $cursor = $target
            while ($null -ne $cursor) {
                $item = Get-Item -Force -LiteralPath $cursor
                if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                    throw "Install directory crosses a reparse point: '$cursor'."
                }
                $parent = [IO.Directory]::GetParent($cursor)
                $cursor = if ($null -eq $parent) { $null } else { $parent.FullName }
            }
            return
        }
    }

    Assert-SafeInstallDirectory
}

function Stop-OwnedApplication {
    Assert-SafeApplicationStopDirectory
    $expected = Get-NormalizedPath (Join-Path $installDir 'Obsession.exe')
    $processes = @(Get-CimInstance Win32_Process -Filter "Name='Obsession.exe'")

    foreach ($process in $processes) {
        if ([string]::IsNullOrWhiteSpace($process.ExecutablePath)) {
            throw "Cannot verify ownership of running Obsession.exe process $($process.ProcessId)."
        }
        $actual = Get-NormalizedPath $process.ExecutablePath
        if (-not $actual.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
            continue
        }

        & "$env:SystemRoot\System32\taskkill.exe" /PID $process.ProcessId /T /F | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to stop owned Obsession.exe process $($process.ProcessId)."
        }
    }
}

# winws/winws2/Obsession Telegram Proxy are NOT launched from the install directory: the app
# unpacks them into %APPDATA%\Obsession\bin on first start (paths.rs -
# bin_dir/winws_path/winws2_path/tgproxy_path), and winws2 sits one level deeper
# in bin\zapret2. Ownership is therefore checked against that root, not
# $installDir.
#
# The exact-path check is mandatory: matching on the image name alone would kill
# a FOREIGN winws.exe the user may be running in parallel (stock Zapret,
# GoodbyeDPI). Avoiding exactly that is why hooks.nsh does !macroundef
# CheckIfAppIsRunning.
#
# Keep this file ASCII-only: Windows PowerShell 5.1 reads a .ps1 without a BOM
# using the system ANSI codepage, so non-ASCII literals here break the parser.
function Stop-OwnedRuntimeProcesses {
    $appData = $env:APPDATA
    if ([string]::IsNullOrWhiteSpace($appData)) {
        throw 'APPDATA is not set; cannot locate the Obsession runtime directory.'
    }
    $binRoot = Get-NormalizedPath (Join-Path (Join-Path $appData 'Obsession') 'bin')

    # Keep old proxy image names only for bounded upgrade cleanup below binRoot.
    $names = @('winws.exe', 'winws2.exe', 'obsession-tg-proxy.exe', 'tg_ws_proxy.exe', 'TgWsProxy.exe', 'tg-ws-proxy.exe')
    $failed = @()
    $unverified = @()

    foreach ($name in $names) {
        $processes = @(Get-CimInstance Win32_Process -Filter "Name='$name'" -ErrorAction SilentlyContinue)
        foreach ($process in $processes) {
            if ([string]::IsNullOrWhiteSpace($process.ExecutablePath)) {
                # Path unreadable - typically a higher-integrity process. Ownership
                # is unproven, so do NOT kill it; but do not stay silent either,
                # because this may well be our own orphaned winws.
                $unverified += "$name (PID $($process.ProcessId))"
                continue
            }
            $actual = Get-NormalizedPath $process.ExecutablePath
            if (-not (Test-SameOrChild $actual $binRoot)) {
                continue
            }

            & "$env:SystemRoot\System32\taskkill.exe" /PID $process.ProcessId /T /F | Out-Null
            if ($LASTEXITCODE -ne 0) {
                $failed += "$name (PID $($process.ProcessId))"
            }
        }
    }

    if ($failed.Count -gt 0 -or $unverified.Count -gt 0) {
        $parts = @()
        if ($failed.Count -gt 0) {
            $parts += "could not stop: $($failed -join ', ')"
        }
        if ($unverified.Count -gt 0) {
            $parts += "could not verify ownership (administrator rights required): $($unverified -join ', ')"
        }
        throw "Bypass processes are still running - $($parts -join '; '). The WinDivert driver will keep filtering traffic."
    }
}

function Remove-OwnedFirewallRules {
    $baseName = 'Obsession TgWsProxy'
    $rules = @(Get-NetFirewallRule -DisplayName "$baseName*" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.DisplayName -eq $baseName -or
            $_.DisplayName.StartsWith("$baseName ", [StringComparison]::Ordinal)
        })
    if ($rules.Count -eq 0) {
        return
    }

    try {
        $rules | Remove-NetFirewallRule -ErrorAction Stop
    }
    catch {
        # Never elevate a script extracted below $PLUGINSDIR/%TEMP%. A future
        # per-machine uninstaller performs this cleanup through its signed
        # native worker. Until then, fail closed and tell the caller why the
        # old rule could not be removed automatically.
        throw "Could not remove Obsession firewall rules without administrator rights. No temporary script was elevated: $($_.Exception.Message)"
    }
}

function Sync-OwnedShortcuts {
    Assert-SafeInstallDirectory
    $target = Get-NormalizedPath (Join-Path $installDir 'obsession.exe')
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
        throw "Shortcut target '$target' does not exist."
    }

    $shell = New-Object -ComObject WScript.Shell

    function Set-ShortcutState([string] $ShortcutPath, [bool] $Enabled) {
        if ($Enabled) {
            $parent = [IO.Path]::GetDirectoryName($ShortcutPath)
            if (-not [string]::IsNullOrWhiteSpace($parent)) {
                [IO.Directory]::CreateDirectory($parent) | Out-Null
            }
            $shortcut = $shell.CreateShortcut($ShortcutPath)
            $shortcut.TargetPath = $target
            $shortcut.WorkingDirectory = $installDir
            $shortcut.IconLocation = "$target,0"
            $shortcut.Save()
            return
        }

        if (-not (Test-Path -LiteralPath $ShortcutPath -PathType Leaf)) {
            return
        }
        $shortcut = $shell.CreateShortcut($ShortcutPath)
        if (-not [string]::IsNullOrWhiteSpace($shortcut.TargetPath)) {
            $actual = Get-NormalizedPath $shortcut.TargetPath
            if ($actual.Equals($target, [StringComparison]::OrdinalIgnoreCase)) {
                Remove-Item -LiteralPath $ShortcutPath -Force
            }
        }
    }

    $desktopPath = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Obsession.lnk'
    $startMenuPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Obsession.lnk'
    Set-ShortcutState $desktopPath ($env:OBSESSION_SHORTCUT_DESKTOP -eq '1')
    Set-ShortcutState $startMenuPath ($env:OBSESSION_SHORTCUT_START_MENU -eq '1')
}

function Cleanup-MachineUserShortcuts {
    if ([string]::IsNullOrWhiteSpace($installDir)) {
        throw 'Machine install directory is empty.'
    }
    $programFiles = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
    if ([string]::IsNullOrWhiteSpace($programFiles)) {
        throw 'Program Files could not be resolved.'
    }
    $expected = Get-NormalizedPath (Join-Path $programFiles 'Obsession')
    $actual = Get-NormalizedPath $installDir
    if (-not $actual.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Unexpected machine install directory '$actual'."
    }

    $target = Get-NormalizedPath (Join-Path $actual 'obsession.exe')
    $ownedTargets = @($target)
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        $ownedTargets += Get-NormalizedPath (Join-Path $env:LOCALAPPDATA 'Obsession\Obsession.exe')
    }

    $shell = New-Object -ComObject WScript.Shell
    function Remove-OwnedUserShortcut([string] $ShortcutPath) {
        if (-not (Test-Path -LiteralPath $ShortcutPath -PathType Leaf)) {
            return
        }

        $item = Get-Item -Force -LiteralPath $ShortcutPath
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            return
        }

        try {
            $shortcut = $shell.CreateShortcut($ShortcutPath)
            if (-not [string]::IsNullOrWhiteSpace($shortcut.TargetPath)) {
                $shortcutTarget = Get-NormalizedPath $shortcut.TargetPath
                foreach ($ownedTarget in $ownedTargets) {
                    if ($shortcutTarget.Equals($ownedTarget, [StringComparison]::OrdinalIgnoreCase)) {
                        Remove-Item -LiteralPath $ShortcutPath -Force
                        return
                    }
                }
            }
        }
        catch {
            # A malformed or unreadable shortcut cannot be proven to belong to
            # Obsession. Preserve it instead of deleting by name alone.
            return
        }
    }

    foreach ($folder in @(
        [Environment]::GetFolderPath('Desktop'),
        [Environment]::GetFolderPath('Programs')
    )) {
        if (-not [string]::IsNullOrWhiteSpace($folder)) {
            Remove-OwnedUserShortcut (Join-Path $folder 'Obsession.lnk')
        }
    }
}

try {
    switch ($action) {
        'ValidateInstallDir' { Assert-SafeInstallDirectory }
        'StopOwnedApplication' { Stop-OwnedApplication }
        'StopOwnedRuntime' { Stop-OwnedRuntimeProcesses }
        'CleanupFirewall' { Remove-OwnedFirewallRules }
        'SyncShortcuts' { Sync-OwnedShortcuts }
        'CleanupMachineUserShortcuts' { Cleanup-MachineUserShortcuts }
        default { throw "Unknown installer safety action '$action'." }
    }
    exit 0
}
catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
