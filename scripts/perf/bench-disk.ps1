<#
.SYNOPSIS
  Disk-reclamation benchmark through the real CleanupService (Quarantine, purge, Permanent and,
  with -AllowRecycleBin, Recycle Bin with self-undo).
.DESCRIPTION
  Wraps the ignored Rust test `perf_disk_reclamation`. Payloads are disposable folders under
  %TEMP%\supa-diska-perf-disk-* and are always removed. -AllowRecycleBin briefly places the
  benchmark's own payload in the current user's Recycle Bin and restores it with the app's undo.
.EXAMPLE
  powershell -File scripts/perf/bench-disk.ps1 -Runs 3 -PayloadMiB 256
  powershell -File scripts/perf/bench-disk.ps1 -Runs 3 -PayloadMiB 256 -AllowRecycleBin
#>
[CmdletBinding()]
param(
  [int]$Runs = 3,
  [ValidateRange(8, 8192)][int]$PayloadMiB = 256,
  [switch]$AllowRecycleBin
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")
$perfStartedUtc = [DateTime]::UtcNow
Assert-PerfQuietToolchain

$repo = Get-PerfRepoRoot
$runDir = New-PerfRunDirectory -Kind "disk"
$lines = Join-Path $runDir "records.jsonl"
$log = Join-Path $runDir "cargo.log"
$environment = Get-PerfEnvironment -VolumePaths @($env:TEMP)

$env:SUPA_PERF_DISK_MIB = [string]$PayloadMiB
$env:SUPA_PERF_OUT = $lines
if ($AllowRecycleBin) { $env:SUPA_PERF_ALLOW_RECYCLE = "1" } else { Remove-Item Env:SUPA_PERF_ALLOW_RECYCLE -ErrorAction SilentlyContinue }
$monitor = Start-PerfContentionMonitor
try {
  for ($i = 0; $i -lt $Runs; $i++) {
    Invoke-PerfNative -FilePath "cargo" -Arguments @("test", "-p", "windows-platform", "--lib", "--locked", "perf_disk_reclamation", "--", "--ignored", "--nocapture", "--test-threads=1") -LogPath $log -WorkingDirectory (Join-Path $repo "src-tauri")
    Write-Output "disk run $($i + 1)/$Runs done"
  }
}
finally {
  $contention = Stop-PerfContentionMonitor -Job $monitor
  Remove-Item Env:SUPA_PERF_DISK_MIB, Env:SUPA_PERF_OUT, Env:SUPA_PERF_ALLOW_RECYCLE -ErrorAction SilentlyContinue
}

$records = @(Get-Content -LiteralPath $lines | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
$scenarios = [ordered]@{}
foreach ($name in (@($records | ForEach-Object scenario) | Sort-Object -Unique)) {
  $rows = @($records | Where-Object scenario -eq $name)
  $summary = [ordered]@{ runs = $rows.Count; executeMs = Get-PerfStats -Values @($rows | ForEach-Object { [double]$_.executeMs }) }
  foreach ($field in "selectedBytes", "quarantinedBytes", "purgedBytes", "recycledBytes", "appReclaimedBytes", "externalReclaimedBytes", "recycleBinDeltaBytes", "freeSpaceNoiseBytes") {
    $values = @($rows | ForEach-Object { $_.$field } | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
    if ($values.Count -gt 0) { $summary[$field] = Get-PerfStats -Values $values }
  }
  $afterPurge = @($rows | Where-Object { $_.PSObject.Properties.Name -contains "afterPurge" })
  if ($afterPurge.Count -gt 0) {
    $summary["afterPurge.externalReclaimedBytes"] = Get-PerfStats -Values @($afterPurge | ForEach-Object { [double]$_.afterPurge.externalReclaimedBytes })
    $summary["afterPurge.appReclaimedBytes"] = Get-PerfStats -Values @($afterPurge | ForEach-Object { [double]$_.afterPurge.appReclaimedBytes })
  }
  $scenarios[$name] = $summary
}

Write-PerfJson -Path (Join-Path $runDir "results.json") -Value ([ordered]@{
  schemaVersion = 1
  contention = @($contention)
  sleepEvents = @(Get-PerfSleepEvents -SinceUtc $perfStartedUtc)
  kind = "disk"
  runId = Split-Path $runDir -Leaf
  payloadMiB = $PayloadMiB
  allowRecycleBin = [bool]$AllowRecycleBin
  environment = $environment
  scenarios = $scenarios
})
Write-Output "Wrote $(Join-Path $runDir 'results.json')"
