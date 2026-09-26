<#
.SYNOPSIS
  Native runtime benchmark over a generated corpus: throughput, first-progress latency,
  cancellation latency, working set, handles and threads for worker counts 1, 2 and 4.
.DESCRIPTION
  Wraps the ignored Rust test `perf_storage_suite` (windows-platform/tests/perf_storage.rs)
  and aggregates its JSON lines into results.json keyed "<scenario>.<mode>.w<workers>".
  Place the corpus on the volume you want to measure (-CorpusParent for new-corpus.ps1).
.EXAMPLE
  powershell -File scripts/perf/bench-runtime.ps1 -Corpus .gg\perf-corpus\medium -Runs 5 -Label ssd
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Corpus,
  [int]$Runs = 5,
  [string]$Workers = "1,2,4",
  [double]$CancelFraction = 0.25,
  [string]$Label = "baseline",
  [switch]$Release,
  # Trial-only Cargo config files (docs/performance.md); never committed as config.
  [string[]]$CargoConfig = @()
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")
$perfStartedUtc = [DateTime]::UtcNow
if ($Label -notmatch '^[a-z0-9][a-z0-9-]{0,39}$') { throw "Label must be lowercase letters, digits and dashes." }
$corpusRoot = (Resolve-Path -LiteralPath $Corpus).Path
Assert-PerfFixture -Path $corpusRoot
Assert-PerfQuietToolchain

$repo = Get-PerfRepoRoot
$runDir = New-PerfRunDirectory -Kind "runtime-$Label"
$lines = Join-Path $runDir "records.jsonl"
$log = Join-Path $runDir "cargo.log"
$environment = Get-PerfEnvironment -VolumePaths @($corpusRoot)
$manifest = Get-Content -LiteralPath (Join-Path $corpusRoot "perf-manifest.json") -Raw | ConvertFrom-Json

$arguments = @("test", "-p", "windows-platform", "--test", "perf_storage", "--locked")
if ($Release) { $arguments += "--release" }
foreach ($config in $CargoConfig) { $arguments += @("--config", $config) }
$arguments += @("--", "--ignored", "--nocapture", "--test-threads=1")
$env:SUPA_PERF_CORPUS = $corpusRoot
$env:SUPA_PERF_RUNS = [string]$Runs
$env:SUPA_PERF_WORKERS = $Workers
$env:SUPA_PERF_CANCEL_FRACTION = [string]$CancelFraction
$env:SUPA_PERF_OUT = $lines
$monitor = Start-PerfContentionMonitor
try {
  Invoke-PerfNative -FilePath "cargo" -Arguments $arguments -LogPath $log -WorkingDirectory (Join-Path $repo "src-tauri")
}
finally {
  $contention = Stop-PerfContentionMonitor -Job $monitor
  Remove-Item Env:SUPA_PERF_CORPUS, Env:SUPA_PERF_RUNS, Env:SUPA_PERF_WORKERS, Env:SUPA_PERF_CANCEL_FRACTION, Env:SUPA_PERF_OUT -ErrorAction SilentlyContinue
}

$records = @(Get-Content -LiteralPath $lines | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })
$measured = @($records | Where-Object { $_.kind -ne "process" -and $_.run -ge 1 })
$metrics = "elapsedMs", "entriesPerSec", "bytesPerSec", "firstProgressMs", "cancelLatencyMs", "workingSetPeak", "handlesPeak", "threadsPeak"
$results = [ordered]@{}
foreach ($group in ($measured | Group-Object { "{0}.{1}.w{2}" -f $_.scenario, $_.kind, $_.workers } | Sort-Object Name)) {
  $entry = [ordered]@{ runs = $group.Count }
  foreach ($metric in $metrics) {
    $values = @($group.Group | ForEach-Object { $_.$metric } | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
    if ($values.Count -gt 0) { $entry[$metric] = Get-PerfStats -Values $values }
  }
  $entry["handleLeak"] = [int](@($group.Group | ForEach-Object { [int]$_.handlesAfter - [int]$_.handlesBefore } | Measure-Object -Maximum).Maximum)
  $entry["threadLeak"] = [int](@($group.Group | ForEach-Object { [int]$_.threadsAfter - [int]$_.threadsBefore } | Measure-Object -Maximum).Maximum)
  $entry["completedBeforeCancel"] = @($group.Group | Where-Object { $_.completedBeforeCancel -eq $true }).Count
  $results[$group.Name] = $entry
}
$process = $records | Where-Object { $_.kind -eq "process" } | Select-Object -First 1

Write-PerfJson -Path (Join-Path $runDir "results.json") -Value ([ordered]@{
  schemaVersion = 1
  contention = @($contention)
  sleepEvents = @(Get-PerfSleepEvents -SinceUtc $perfStartedUtc)
  kind = "runtime"
  label = $Label
  runId = Split-Path $runDir -Leaf
  profile = $(if ($Release) { "release" } else { "dev" })
  cargoConfig = @($CargoConfig)
  runs = $Runs
  workers = $Workers
  cancelFraction = $CancelFraction
  corpus = [ordered]@{ path = $corpusRoot; size = $manifest.size; seed = $manifest.seed; counts = $manifest.counts; fingerprint = $manifest.fingerprint }
  environment = $environment
  results = $results
  process = $process
})
Write-Output "Wrote $(Join-Path $runDir 'results.json')"
