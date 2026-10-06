[CmdletBinding()]
param(
    [ValidateRange(30, 180)][int]$CaptureSeconds = 120,
    [switch]$Preflight
)

# One local startup capture. No credentials, injected code, packet bodies,
# configuration changes, or automatic termination of Discord.
# Raw NetLog remains local and git-ignored; never print or upload its contents.
$ErrorActionPreference = 'Stop'
$discordTaskRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Discord'
$discordTaskApps = @(Get-ChildItem -LiteralPath $discordTaskRoot -Directory |
    Where-Object { $_.Name -match '^app-\d+\.\d+\.\d+$' } |
    Sort-Object { [version]$_.Name.Substring(4) } -Descending)
$discordTaskExe = $null
foreach ($discordTaskApp in $discordTaskApps) {
    $discordTaskCandidate = Join-Path $discordTaskApp.FullName 'Discord.exe'
    if (Test-Path -LiteralPath $discordTaskCandidate -PathType Leaf) {
        $discordTaskExe = $discordTaskCandidate
        break
    }
}
if ($null -eq $discordTaskExe) { throw 'Не найден установленный Discord Stable.' }

$discordTaskRunning = @(Get-Process -Name Discord -ErrorAction SilentlyContinue)
$discordTaskDpi = @(Get-Process -Name winws2 -ErrorAction SilentlyContinue)
$discordTaskVersion = (Get-Item -LiteralPath $discordTaskExe).VersionInfo.FileVersion
if ($Preflight) {
    [ordered]@{
        executable = $discordTaskExe
        version = $discordTaskVersion
        discordRunning = ($discordTaskRunning.Count -gt 0)
        zapret2Running = ($discordTaskDpi.Count -gt 0)
        captureSeconds = $CaptureSeconds
        launchesApplication = $false
    } | ConvertTo-Json
    exit 0
}
if ($discordTaskRunning.Count -gt 0) {
    Write-Host 'Discord уже открыт. Заверши его через значок в трее и запусти диагностику снова.'
    exit 2
}
if ($discordTaskDpi.Count -eq 0) {
    Write-Host 'Сначала включи DPI-обход Zapret2 в Obsession, затем запусти диагностику снова.'
    exit 3
}

$discordTaskSession = '{0}-{1}' -f ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')), ([guid]::NewGuid().ToString('N').Substring(0, 8))
$discordTaskOutput = Join-Path $PSScriptRoot ("..\.dev-logs\discord-startup\$discordTaskSession")
$discordTaskOutput = [IO.Path]::GetFullPath($discordTaskOutput)
New-Item -ItemType Directory -Path $discordTaskOutput -ErrorAction Stop | Out-Null
$discordTaskNetLog = Join-Path $discordTaskOutput 'netlog.json'
$discordTaskSummary = Join-Path $discordTaskOutput 'capture.json'
function Get-DiscordTaskContext {
    $discordContextPath = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'vlarpsu\Obsession\settings.json'
    try {
        $discordContextSettings = [IO.File]::ReadAllText($discordContextPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
        return [ordered]@{
            engine = $discordContextSettings.dpi_engine
            categories = $discordContextSettings.zapret2_selected_categories
            level = $discordContextSettings.zapret2_level
            profiles = $discordContextSettings.zapret2_profiles
            winws2Pids = @(Get-Process -Name winws2 -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
        }
    } catch { return $null }
}
$discordTaskRecord = [ordered]@{
    schema = 'discord_startup_capture_v1'
    startUtc = [DateTime]::UtcNow.ToString('o')
    discordVersion = $discordTaskVersion
    requestedSeconds = $CaptureSeconds
    captureModeRequested = 'default'
    stopMethod = 'manual_discord_exit'
    contextBefore = Get-DiscordTaskContext
    netLogFile = 'netlog.json'
    zapret2PidsAtStart = @($discordTaskDpi | ForEach-Object { $_.Id })
    status = 'recording'
    netLogBytes = 0
    eventCount = 0
}
$discordTaskRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $discordTaskSummary -Encoding UTF8

Write-Host 'Запускаю Discord и собираю сетевой журнал.'
Write-Host 'VPN должен быть выключен. Во время записи не запускай подбор в Obsession.'
Write-Host 'Когда Discord загрузится, полностью заверши его через значок в трее: так журнал допишется.'
Write-Host "Если за $CaptureSeconds секунд не загрузится, тоже заверши через трей. Окно диагностики оставь открытым до результата."
try {
    # Quoting is for Start-Process's Windows argument string. Neither path is
    # evaluated as shell code. Privacy mode stays at Chromium's default.
    # Do not rely on Chrome-only automatic export shutdown in Electron.
    # Keep the application's raw console output local as well.
    $discordTaskArguments = @("--log-net-log=`"$discordTaskNetLog`"")
    Start-Process -FilePath $discordTaskExe -ArgumentList $discordTaskArguments -WorkingDirectory (Split-Path -Parent $discordTaskExe) -RedirectStandardOutput (Join-Path $discordTaskOutput 'discord-stdout.log') -RedirectStandardError (Join-Path $discordTaskOutput 'discord-stderr.log') | Out-Null
} catch {
    $discordTaskRecord.status = 'launch_failed'
    $discordTaskRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $discordTaskSummary -Encoding UTF8
    Write-Host 'Не удалось запустить Discord. Сообщи это в чат.'
    exit 4
}

$discordTaskClock = [Diagnostics.Stopwatch]::StartNew()
$discordTaskComplete = $false
$discordTaskLimitBytes = 32 * 1024 * 1024
while ($discordTaskClock.Elapsed.TotalSeconds -lt ($CaptureSeconds + 20)) {
    Start-Sleep -Seconds 2
    if (-not (Test-Path -LiteralPath $discordTaskNetLog -PathType Leaf)) { continue }
    $discordTaskRecord.netLogBytes = (Get-Item -LiteralPath $discordTaskNetLog).Length
    $discordTaskRecord.lastPollUtc = [DateTime]::UtcNow.ToString('o')
    $discordTaskRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $discordTaskSummary -Encoding UTF8
    if ($discordTaskRecord.netLogBytes -gt $discordTaskLimitBytes) { break }
    # Read only a tail first: avoid deserializing a growing log every poll.
    $discordTaskTail = $null
    try {
        $discordTaskStream = [IO.File]::Open($discordTaskNetLog, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
        try {
            $discordTaskTailLength = [int][Math]::Min(96, $discordTaskStream.Length)
            [void]$discordTaskStream.Seek(-$discordTaskTailLength, [IO.SeekOrigin]::End)
            $discordTaskTailBytes = New-Object byte[] $discordTaskTailLength
            [void]$discordTaskStream.Read($discordTaskTailBytes, 0, $discordTaskTailLength)
            $discordTaskTail = [Text.Encoding]::UTF8.GetString($discordTaskTailBytes).TrimEnd()
        } finally { $discordTaskStream.Dispose() }
        if (-not $discordTaskTail.EndsWith('}')) { continue }
        # No raw JSON, parser errors, URLs, headers, or payloads reach stdout.
        $discordTaskParsed = [IO.File]::ReadAllText($discordTaskNetLog, [Text.Encoding]::UTF8) | ConvertFrom-Json -ErrorAction Stop
        if ($null -eq $discordTaskParsed.constants -or $null -eq $discordTaskParsed.events) { continue }
        $discordTaskRecord.eventCount = @($discordTaskParsed.events).Count
        $discordTaskComplete = $true
        $discordTaskParsed = $null
        break
    } catch {
        # An unfinished file is expected until the client exits and flushes.
        continue
    }
}
$discordTaskRecord.finishUtc = [DateTime]::UtcNow.ToString('o')
$discordTaskRecord.elapsedSeconds = [Math]::Round($discordTaskClock.Elapsed.TotalSeconds, 1)
$discordTaskRecord.status = if ($discordTaskComplete) { 'captured' } else { 'missing_or_incomplete' }
$discordTaskRecord.contextAfter = Get-DiscordTaskContext
$discordTaskRecord.analysisStatus = 'not_available'
if ((Test-Path -LiteralPath $discordTaskNetLog -PathType Leaf) -and (Get-Command node -ErrorAction SilentlyContinue)) {
    $discordTaskSafeSummary = Join-Path $discordTaskOutput 'safe-summary.json'
    & node (Join-Path $PSScriptRoot 'analyze-discord-startup.mjs') $discordTaskNetLog $discordTaskSafeSummary 1>$null 2>$null
    $discordTaskRecord.analysisStatus = if ($LASTEXITCODE -eq 0) { 'safe_summary_saved' } else { 'not_available' }
}
$discordTaskRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $discordTaskSummary -Encoding UTF8
if ($discordTaskComplete) {
    Write-Host 'Готово. Журнал сохранён локально. Сообщи в чат результат запуска и время ожидания.'
    Write-Host "Папка: $discordTaskOutput"
    exit 0
}
Write-Host 'Журнал ещё не завершён. Полностью заверши Discord через трей и напиши об этом в чат.'
Write-Host "Папка: $discordTaskOutput"
exit 5
