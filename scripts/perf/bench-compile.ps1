<#
.SYNOPSIS
  Compile benchmarks: clean, no-op, one-line Rust edits, frontend edit, rebuild after cleanup.
.DESCRIPTION
  Everything builds into an isolated, marked target (.gg/perf-target/<Label>); the developer's
  src-tauri/target and dist are never touched and `cargo clean` is never run. Edit scenarios
  only touch files that are clean in git, restore them in `finally`, and fail closed if the
  restored hash differs. Local incremental builds are asserted (no RUSTC_WRAPPER, incremental on).
.EXAMPLE
  powershell -File scripts/perf/bench-compile.ps1 -Runs 5
  powershell -File scripts/perf/bench-compile.ps1 -Label lld -CargoConfig 'target.x86_64-pc-windows-msvc.linker="rust-lld"'
#>
[CmdletBinding()]
param(
  [int]$Runs = 5,
  [int]$CleanRuns = 0,
  [string]$Label = "baseline",
  [string[]]$CargoConfig = @(),
  # Comma-separated (works with -File): clean,noop,rust-leaf,rust-app,frontend,after-cleanup
  [string]$Scenarios = "clean,noop,rust-leaf,rust-app,frontend,after-cleanup",
  [string]$LeafFile = "src-tauri/crates/cleanup-core/src/lib.rs",
  [string]$AppFile = "src-tauri/src/lib.rs",
  [string]$FrontendFile = "src/main.tsx",
  [switch]$SkipWarmup
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")
$perfStartedUtc = [DateTime]::UtcNow

if ($Label -notmatch '^[a-z0-9][a-z0-9-]{0,39}$') { throw "Label must be lowercase letters, digits and dashes." }
if ($CleanRuns -le 0) { $CleanRuns = $Runs }
$knownScenarios = @("clean", "noop", "rust-leaf", "rust-app", "frontend", "after-cleanup")
$selectedScenarios = @($Scenarios -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
foreach ($scenario in $selectedScenarios) { if ($knownScenarios -notcontains $scenario) { throw "Unknown scenario '$scenario'." } }
if ($selectedScenarios.Count -eq 0) { throw "No scenarios selected." }

# Local development must keep Cargo incremental builds; sccache/wrappers are CI/clean-only.
if ($env:CARGO_INCREMENTAL -and $env:CARGO_INCREMENTAL -ne "1") { throw "CARGO_INCREMENTAL must be unset or 1 for local compile benchmarks." }
if ($env:RUSTC_WRAPPER -or $env:CARGO_BUILD_RUSTC_WRAPPER) { throw "RUSTC_WRAPPER must be unset for local compile benchmarks." }

Assert-PerfQuietToolchain
$repo = Get-PerfRepoRoot
$workspace = Join-Path $repo "src-tauri"
$perfTargets = New-PerfFixtureDirectory -Path (Join-Path $repo ".gg\perf-target")
$target = New-PerfFixtureDirectory -Path (Join-Path $perfTargets $Label)
$cargoTarget = Join-Path $target "cargo"
$distTarget = Join-Path $target "dist"
$runDir = New-PerfRunDirectory -Kind "compile-$Label"
$log = Join-Path $runDir "build.log"
$env:CARGO_TARGET_DIR = $cargoTarget

$cargoArgs = @("build", "--workspace", "--locked")
foreach ($config in $CargoConfig) { $cargoArgs += @("--config", $config) }

function Invoke-Cargo([switch]$Timings) {
  $arguments = $cargoArgs
  if ($Timings) { $arguments = $arguments + @("--timings") }
  Invoke-PerfNative -FilePath "cargo" -Arguments $arguments -LogPath $log -WorkingDirectory $workspace
}
function Invoke-Tsc { Invoke-PerfNative -FilePath "pnpm" -Arguments @("exec", "tsc", "--noEmit") -LogPath $log }
function Invoke-Vite { Invoke-PerfNative -FilePath "pnpm" -Arguments @("exec", "vite", "build", "--outDir", $distTarget, "--emptyOutDir", "--logLevel", "warn") -LogPath $log }

function Save-Timings([string]$Name) {
  $dir = Join-Path $cargoTarget "cargo-timings"
  if (-not (Test-Path $dir)) { return }
  $latest = Get-ChildItem $dir -Filter "cargo-timing-*.html" | Sort-Object LastWriteTimeUtc | Select-Object -Last 1
  if ($latest) { Copy-Item $latest.FullName (Join-Path $runDir "timings-$Name.html") -Force }
}

function Reset-CargoTarget {
  # Whole isolated target delete = a clean build. Never `cargo clean`; never the user's target.
  if (Test-Path $cargoTarget) {
    Assert-PerfFixture -Path $target
    Remove-Item -LiteralPath $cargoTarget -Recurse -Force
  }
}

function Remove-GenerationOutputs {
  # Keep deps/, incremental/, build/ and .fingerprint/ like artifact budgets do.
  $debug = Join-Path $cargoTarget "debug"
  $removed = 0
  Get-ChildItem -LiteralPath $debug -File | Where-Object { $_.Extension -in ".exe", ".pdb", ".dll", ".lib", ".rlib", ".d", ".exp" } | ForEach-Object {
    Remove-Item -LiteralPath $_.FullName -Force; $removed++
  }
  if ($removed -eq 0) { throw "No generation outputs found to remove in $debug." }
  return $removed
}

function Get-Sha256([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }

function Invoke-WithEdit([string]$RelativePath, [string]$Comment, [scriptblock]$Measure) {
  $path = Join-Path $repo $RelativePath
  $status = git -C $repo status --porcelain -- $RelativePath
  if ($status) { throw "Refusing to edit ${RelativePath}: it has uncommitted changes." }
  $original = [IO.File]::ReadAllBytes($path)
  $originalHash = Get-Sha256 $path
  try {
    $appended = [Text.Encoding]::UTF8.GetBytes("`n$Comment`n")
    [IO.File]::WriteAllBytes($path, [byte[]]($original + $appended))
    return (& $Measure)
  }
  finally {
    [IO.File]::WriteAllBytes($path, $original)
    if ((Get-Sha256 $path) -ne $originalHash -or (git -C $repo status --porcelain -- $RelativePath)) {
      throw "FAILED to restore $RelativePath exactly; stop and inspect the file."
    }
  }
}

$samples = [ordered]@{}
$contention = New-Object Collections.Generic.List[string]
function Test-Contention([string]$Where) {
  # Our cargo has exited between measurements; also counts only non-descendant builds.
  $count = Get-PerfForeignBuildCount -RootPid $PID
  if ($count -gt 0) { $contention.Add("$Where`: $count foreign cargo/rustc"); Write-Warning "Contention at $Where" }
}
$monitor = Start-PerfContentionMonitor
function Add-Sample([string]$Name, [double]$Ms) {
  if (-not $samples.Contains($Name)) { $samples[$Name] = New-Object Collections.Generic.List[double] }
  $samples[$Name].Add([Math]::Round($Ms, 1))
}

$environment = Get-PerfEnvironment -VolumePaths @($repo, $cargoTarget)
$started = [DateTime]::UtcNow
Write-Output "Compile benchmark '$Label' -> $runDir"

# Prime: ensures a built state exists for incremental scenarios.
if (-not (Test-Path (Join-Path $cargoTarget "debug"))) {
  Write-Output "Priming isolated target (untimed)..."
  Invoke-Cargo
}

if ($selectedScenarios -contains "clean") {
  $total = $CleanRuns + $(if ($SkipWarmup) { 0 } else { 1 })
  for ($i = 0; $i -lt $total; $i++) {
    $warm = (-not $SkipWarmup) -and $i -eq 0
    Test-Contention "clean[$i]"
    Reset-CargoTarget
    $rust = Invoke-PerfTimed { Invoke-Cargo -Timings }
    $tsc = Invoke-PerfTimed { Invoke-Tsc }
    $vite = Invoke-PerfTimed { Invoke-Vite }
    if (-not $warm) {
      Add-Sample "clean.rust" $rust; Add-Sample "clean.tsc" $tsc; Add-Sample "clean.vite" $vite
      Save-Timings "clean-$i"
    }
    Write-Output ("clean[{0}]{1}: rust {2:n0} ms, tsc {3:n0} ms, vite {4:n0} ms" -f $i, $(if ($warm) { " (warm-up)" } else { "" }), $rust, $tsc, $vite)
  }
}

$total = $Runs + $(if ($SkipWarmup) { 0 } else { 1 })
for ($i = 0; $i -lt $total; $i++) {
  $warm = (-not $SkipWarmup) -and $i -eq 0
  $tag = [Guid]::NewGuid().ToString("N")
  $row = [ordered]@{}
  Test-Contention "iteration[$i]"
  if ($selectedScenarios -contains "noop") {
    Invoke-Cargo
    $row["noop.rust"] = Invoke-PerfTimed { Invoke-Cargo }
  }
  if ($selectedScenarios -contains "rust-leaf") {
    $row["rust-leaf.rust"] = Invoke-WithEdit $LeafFile "// perf-bench edit $tag" { Invoke-PerfTimed { Invoke-Cargo -Timings } }
    Save-Timings "rust-leaf-$i"
    Invoke-Cargo  # settle after restore (untimed)
  }
  if ($selectedScenarios -contains "rust-app") {
    $row["rust-app.rust"] = Invoke-WithEdit $AppFile "// perf-bench edit $tag" { Invoke-PerfTimed { Invoke-Cargo -Timings } }
    Save-Timings "rust-app-$i"
    Invoke-Cargo
  }
  if ($selectedScenarios -contains "frontend") {
    $pair = Invoke-WithEdit $FrontendFile "// perf-bench edit $tag" {
      @((Invoke-PerfTimed { Invoke-Tsc }), (Invoke-PerfTimed { Invoke-Vite }))
    }
    $row["frontend.tsc"] = $pair[0]; $row["frontend.vite"] = $pair[1]
  }
  if ($selectedScenarios -contains "after-cleanup") {
    Invoke-Cargo
    $removed = Remove-GenerationOutputs
    $row["after-cleanup.rust"] = Invoke-PerfTimed { Invoke-Cargo }
    $row["after-cleanup.removedFiles"] = $removed
  }
  foreach ($key in $row.Keys) { if (-not $warm -and $key -notlike "*.removedFiles") { Add-Sample $key $row[$key] } }
  Write-Output ("iteration[{0}]{1}: {2}" -f $i, $(if ($warm) { " (warm-up)" } else { "" }), (($row.Keys | ForEach-Object { "$_=$([Math]::Round($row[$_]))" }) -join ", "))
}

$monitorSamples = Stop-PerfContentionMonitor -Job $monitor
foreach ($sample in $monitorSamples) { $contention.Add($sample) }
$results = [ordered]@{}
foreach ($key in $samples.Keys) {
  $results[$key] = [ordered]@{ unit = "ms"; samples = @($samples[$key]); stats = Get-PerfStats -Values $samples[$key].ToArray() }
}
Write-PerfJson -Path (Join-Path $runDir "results.json") -Value ([ordered]@{
  schemaVersion = 1
  sleepEvents = @(Get-PerfSleepEvents -SinceUtc $perfStartedUtc)
  kind = "compile"
  label = $Label
  runId = Split-Path $runDir -Leaf
  startedUtc = $started.ToString("o")
  finishedUtc = [DateTime]::UtcNow.ToString("o")
  runs = $Runs
  cleanRuns = $CleanRuns
  warmupDiscarded = (-not $SkipWarmup)
  cargoArgs = $cargoArgs
  isolatedTarget = $target
  environment = $environment
  results = $results
  contention = @($contention)
  notes = @(
    "after-cleanup removes only top-level debug generation outputs; the whole-target equivalent is the clean scenario."
    "frontend scenarios build into the isolated dist; noop for Vite is equivalent to frontend.vite because Vite has no build cache."
  )
})
Write-Output "Wrote $(Join-Path $runDir 'results.json')"
