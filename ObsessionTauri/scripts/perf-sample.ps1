# Samples CPU% and memory of the running Obsession WebView2 tree.
# Usage: powershell -File perf-sample.ps1 [-Seconds 8] [-Label name]
param([int]$Seconds = 8, [string]$Label = "")

$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
$before = @{}
Get-Process msedgewebview2, obsession -EA 0 | ForEach-Object { $before[$_.Id] = $_.CPU }
Start-Sleep -Seconds $Seconds

$rows = Get-Process msedgewebview2, obsession -EA 0 | ForEach-Object {
  $prev = if ($before.ContainsKey($_.Id)) { $before[$_.Id] } else { $_.CPU }
  [pscustomobject]@{
    Id      = $_.Id
    Role    = if ($_.ProcessName -eq "obsession") { "host" }
              elseif ($_.PriorityClass -eq "AboveNormal") { "renderer" }
              else { $_.ProcessName }
    Cores   = [math]::Round(($_.CPU - $prev) / $Seconds, 3)
    WS_MB   = [math]::Round($_.WorkingSet64 / 1MB, 1)
    Priv_MB = [math]::Round($_.PrivateMemorySize64 / 1MB, 1)
  }
}

$busiest = $rows | Sort-Object Cores -Descending | Select-Object -First 2
$totalCores = ($rows | Measure-Object Cores -Sum).Sum

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
# живут здесь, а не в приватном рабочем наборе.
$vram = 0
try {
  $ids = (Get-Process msedgewebview2 -EA 0).Id
  $gpu = Get-Counter -Counter "\GPU Process Memory(*)\Local Usage" -EA Stop
  foreach ($s in $gpu.CounterSamples) {
    if ($s.CookedValue -le 0) { continue }
    if ($s.InstanceName -match "pid_(\d+)" -and $ids -contains [int]$Matches[1]) { $vram += $s.CookedValue }
  }
} catch {}

$out = [pscustomobject]@{
  label       = $Label
  cpu_pct     = [math]::Round($totalCores / $cores * 100, 2)
  cores       = [math]::Round($totalCores, 3)
  top1        = "$($busiest[0].Role)=$($busiest[0].Cores)"
  top2        = "$($busiest[1].Role)=$($busiest[1].Cores)"
  privWS_MB   = [math]::Round($pws / 1MB, 1)
  commit_MB   = [math]::Round($commit / 1MB, 1)
  vram_MB     = [math]::Round($vram / 1MB, 1)
}
$out | ConvertTo-Json -Compress
