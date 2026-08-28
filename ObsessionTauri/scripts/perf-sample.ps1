# Samples CPU% and memory of the running Obsession WebView2 tree.
# Usage: powershell -File perf-sample.ps1 [-Seconds 8] [-Label name] [-Detail]
# Switch не называть -Rows: имена переменных в PowerShell регистронезависимы, и
# параметр перетёр бы $rows с самими процессами.
param([int]$Seconds = 8, [string]$Label = "", [switch]$Detail)

$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors

# Роль процесса — из его командной строки, а не из PriorityClass. Прошлая версия
# угадывала по приоритету и сливала GPU-процесс с utility, а весь разбор нагрузки
# темы держится ровно на разделении «композитор (gpu) против рендерера».
$roleByPid = @{}
foreach ($p in Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -EA 0) {
  $line = [string]$p.CommandLine
  $type = if ($line -match '--type=([\w-]+)') { $Matches[1] } else { "browser" }
  $role = switch ($type) {
    "gpu-process"      { "gpu" }
    "renderer"         { "renderer" }
    "crashpad-handler" { "crashpad" }
    "utility" {
      # network.mojom.NetworkService → network; лишние сегменты не читаются в отчёте.
      if ($line -match '--utility-sub-type=([\w.-]+)') { "utility:" + ($Matches[1] -split '\.')[0] }
      else { "utility" }
    }
    default { $type }
  }
  $roleByPid[[int]$p.ProcessId] = $role
}

$before = @{}
Get-Process msedgewebview2, obsession -EA 0 | ForEach-Object { $before[$_.Id] = $_.CPU }
Start-Sleep -Seconds $Seconds

$rows = Get-Process msedgewebview2, obsession -EA 0 | ForEach-Object {
  $prev = if ($before.ContainsKey($_.Id)) { $before[$_.Id] } else { $_.CPU }
  [pscustomobject]@{
    Id      = $_.Id
    Role    = if ($_.ProcessName -eq "obsession") { "host" }
              elseif ($roleByPid.ContainsKey($_.Id)) { $roleByPid[$_.Id] }
              else { "webview2" }
    Cores   = [math]::Round(($_.CPU - $prev) / $Seconds, 3)
    WS_MB   = [math]::Round($_.WorkingSet64 / 1MB, 1)
    Priv_MB = [math]::Round($_.PrivateMemorySize64 / 1MB, 1)
  }
}

$totalCores = ($rows | Measure-Object Cores -Sum).Sum
# Роли повторяются (несколько utility), поэтому суммируем, а не переприсваиваем.
$byRole = [ordered]@{}
foreach ($r in $rows | Sort-Object Cores -Descending) {
  if ($byRole.Contains($r.Role)) { $byRole[$r.Role] = [math]::Round($byRole[$r.Role] + $r.Cores, 3) }
  else { $byRole[$r.Role] = $r.Cores }
}

# Память: "Working Set - Private" — то же, что колонка «Память» в диспетчере
# задач (резидентная приватная часть, без разделяемых страниц). Private Bytes —
# это commit charge: он включает и вытесненное, и закоммиченное, но не тронутое,
# поэтому у Chromium он в разы больше и на вопрос «сколько ест» не отвечает.
$pws = 0
$commit = 0
try {
  $counters = Get-Counter -Counter "\Process(msedgewebview2*)\Working Set - Private",
                                   "\Process(obsession)\Working Set - Private",
                                   "\Process(msedgewebview2*)\Private Bytes",
                                   "\Process(obsession)\Private Bytes" -EA Stop
  foreach ($s in $counters.CounterSamples) {
    if ($s.InstanceName -eq "_total") { continue }
    if ($s.Path -match "working set - private") { $pws += $s.CookedValue } else { $commit += $s.CookedValue }
  }
} catch {}

# Видеопамять процессов WebView2 — тот же счётчик, что в колонке «Память
# графического процессора» диспетчера задач. Слои композитора и буферы WebGL
# живут здесь, а не в приватном рабочем наборе. Раскладка по ролям показывает,
# что экономия текстур и MSAA-буфера действительно села в GPU-процесс.
$vram = 0
$vramByRole = [ordered]@{}
try {
  $ids = (Get-Process msedgewebview2 -EA 0).Id
  $gpu = Get-Counter -Counter "\GPU Process Memory(*)\Local Usage" -EA Stop
  foreach ($s in $gpu.CounterSamples) {
    if ($s.CookedValue -le 0) { continue }
    if ($s.InstanceName -notmatch "pid_(\d+)") { continue }
    $samplePid = [int]$Matches[1]
    if ($ids -notcontains $samplePid) { continue }
    $vram += $s.CookedValue
    $role = if ($roleByPid.ContainsKey($samplePid)) { $roleByPid[$samplePid] } else { "webview2" }
    $mb = [math]::Round($s.CookedValue / 1MB, 1)
    if ($vramByRole.Contains($role)) { $vramByRole[$role] = [math]::Round($vramByRole[$role] + $mb, 1) }
    else { $vramByRole[$role] = $mb }
  }
} catch {}

if ($Detail) { $rows | Sort-Object Cores -Descending | Format-Table -AutoSize | Out-String | Write-Host }

$out = [pscustomobject]@{
  label       = $Label
  cpu_pct     = [math]::Round($totalCores / $cores * 100, 2)
  cores       = [math]::Round($totalCores, 3)
  by_role     = $byRole
  privWS_MB   = [math]::Round($pws / 1MB, 1)
  commit_MB   = [math]::Round($commit / 1MB, 1)
  vram_MB     = [math]::Round($vram / 1MB, 1)
  vram_role   = $vramByRole
}
$out | ConvertTo-Json -Compress -Depth 4
