# Private resident memory, identified by PID (not localized performance counters).
# Excludes unrelated WebViews and separately hosted runtime services.
param([string]$ProcessName = 'obsession', [int]$Samples = 1, [int]$IntervalSeconds = 3)
for ($sample = 0; $sample -lt $Samples; $sample++) {
  $roots = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
  if ($roots.Count -ne 1) { throw "Expected exactly one $ProcessName process, found $($roots.Count)" }
  $processes = @(Get-CimInstance Win32_Process)
  $ids = [System.Collections.Generic.HashSet[uint32]]::new()
  [void]$ids.Add([uint32]$roots[0].Id)
  do {
    $added = $false
    foreach ($process in $processes) {
      if ($ids.Contains([uint32]$process.ParentProcessId) -and $process.Name -eq 'msedgewebview2.exe') {
        if ($ids.Add([uint32]$process.ProcessId)) { $added = $true }
      }
    }
  } while ($added)
  $counters = @(Get-CimInstance Win32_PerfRawData_PerfProc_Process | Where-Object { $ids.Contains([uint32]$_.IDProcess) })
  [pscustomobject]@{
    at = [DateTime]::UtcNow.ToString('o')
    pid = $roots[0].Id
    count = $counters.Count
    privateResidentMiB = [math]::Round(($counters | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB, 2)
    committedMiB = [math]::Round(($counters | Measure-Object PrivateBytes -Sum).Sum / 1MB, 2)
  } | ConvertTo-Json -Compress
  if ($sample + 1 -lt $Samples) { Start-Sleep -Seconds $IntervalSeconds }
}
