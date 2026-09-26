<#
.SYNOPSIS
  Built-app runtime benchmark: startup, idle resource use, route-visit growth, and (optionally)
  in-app scan responsiveness and cancellation.
.DESCRIPTION
  Launches the built executable with a loopback-only WebView2 debug port (the same pattern as
  the smoke/acceptance scripts) and measures the main process plus its WebView2 child tree.
  Startup is timed from Start-Process to the first page where document.readyState is 'complete'
  and the app shell (main h1) has rendered; that includes CDP polling granularity (~50 ms).
  -Interactive asks a person to choose the corpus folder in the native picker (no synthetic
  desktop input), then measures a large-files scan: time, requestAnimationFrame gap histogram
  during the scan, and time from cancel request to a cancelled status.
.EXAMPLE
  powershell -File scripts/perf/bench-app.ps1 -Runs 5
  powershell -File scripts/perf/bench-app.ps1 -Runs 1 -Interactive -CorpusRoot .gg\perf-corpus\medium
#>
[CmdletBinding()]
param(
  [string]$Executable = "src-tauri/target/x86_64-pc-windows-msvc/debug/supa-diska-klinah.exe",
  [int]$Runs = 5,
  [int]$IdleSeconds = 60,
  [int]$RouteCycles = 5,
  [switch]$SkipWarmup,
  [switch]$Interactive,
  [string]$CorpusRoot
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")
$perfStartedUtc = [DateTime]::UtcNow
. (Join-Path $PSScriptRoot "..\smoke-project-discovery.ps1")

$exe = (Resolve-Path -LiteralPath $Executable).Path
$routes = @("/", "/drives", "/disk-analyzer", "/large-files", "/duplicates", "/empty-folders", "/cleaner", "/browser", "/cleanup", "/settings")

if (Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($exe)) -ErrorAction SilentlyContinue) {
  throw "Close running app instances first; the benchmark needs an exclusive, cold process tree."
}
if ($Interactive) {
  if (-not $CorpusRoot) { throw "-Interactive needs -CorpusRoot (a generated perf corpus)." }
  $CorpusRoot = (Resolve-Path -LiteralPath $CorpusRoot).Path
  Assert-PerfFixture -Path $CorpusRoot
}

function Get-FreePort {
  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  $listener.Start(); $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port; $listener.Stop()
  return $port
}

function Get-ProcessTreeIds([int]$RootId) {
  $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId)
  $ids = New-Object 'Collections.Generic.HashSet[int]'
  [void]$ids.Add($RootId)
  do {
    $added = $false
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
    }
  } while ($added)
  return @($ids)
}

function Get-TreeSample([int]$RootId) {
  $sample = [ordered]@{ processes = 0; cpuSeconds = 0.0; workingSetBytes = 0; privateBytes = 0; handles = 0; threads = 0; mainWorkingSetBytes = 0; mainHandles = 0; mainThreads = 0 }
  foreach ($id in (Get-ProcessTreeIds $RootId)) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if (-not $p) { continue }
    $sample.processes++
    $sample.cpuSeconds += [double]$p.TotalProcessorTime.TotalSeconds
    $sample.workingSetBytes += [int64]$p.WorkingSet64
    $sample.privateBytes += [int64]$p.PrivateMemorySize64
    $sample.handles += [int]$p.HandleCount
    $sample.threads += [int]$p.Threads.Count
    if ($id -eq $RootId) {
      $sample.mainWorkingSetBytes = [int64]$p.WorkingSet64
      $sample.mainHandles = [int]$p.HandleCount
      $sample.mainThreads = [int]$p.Threads.Count
    }
  }
  return $sample
}

function Wait-AppReady([Net.WebSockets.ClientWebSocket]$Socket) {
  for ($i = 0; $i -lt 1200; $i++) {
    if (Invoke-WebViewExpression -Socket $Socket -Expression "document.readyState === 'complete' && !!document.querySelector('main h1') && typeof window.__TAURI_INTERNALS__?.invoke === 'function'") { return }
    Start-Sleep -Milliseconds 50
  }
  throw "App shell did not render within 60 s."
}

function Stop-AppTree([Diagnostics.Process]$Process) {
  if (-not $Process -or $Process.HasExited) { return }
  $taskkill = Start-Process -FilePath "$env:SystemRoot\System32\taskkill.exe" -ArgumentList "/PID", $Process.Id, "/T", "/F" -WindowStyle Hidden -Wait -PassThru
  if ($taskkill.ExitCode -ne 0 -and -not $Process.HasExited) { throw "Could not stop app process tree $($Process.Id)." }
}

function Start-App {
  $port = Get-FreePort
  $previous = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
  $process = $null
  try {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -PassThru
    $socket = Connect-WebViewDebugSocket -Port $port
    $connectedMs = $watch.Elapsed.TotalMilliseconds
    Wait-AppReady $socket
    $readyMs = $watch.Elapsed.TotalMilliseconds
  }
  catch {
    # Never leave an orphaned app tree behind when startup fails.
    Stop-AppTree $process
    throw
  }
  finally {
    if ($null -eq $previous) { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
    else { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previous }
  }
  return [pscustomobject]@{ Process = $process; Socket = $socket; ConnectedMs = $connectedMs; ReadyMs = $readyMs }
}

function Invoke-RouteCycle([Net.WebSockets.ClientWebSocket]$Socket) {
  foreach ($route in $routes) {
    $hash = ("#" + $route) | ConvertTo-Json -Compress
    [void](Invoke-WebViewExpression -Socket $Socket -Expression "(() => { window.location.hash = $hash; return true; })()")
    Wait-WebViewExpression -Socket $Socket -Expression "!!document.querySelector('main h1')" -Failure "Route $route did not render."
    Start-Sleep -Milliseconds 300
  }
}

$runDir = New-PerfRunDirectory -Kind "app"
$environment = Get-PerfEnvironment -VolumePaths @($exe)
$startupConnected = New-Object Collections.Generic.List[double]
$startupReady = New-Object Collections.Generic.List[double]
$startupRows = @()
Write-Output "App benchmark -> $runDir"

$total = $Runs + $(if ($SkipWarmup) { 0 } else { 1 })
for ($i = 0; $i -lt $total; $i++) {
  $app = Start-App
  try {
    $settled = Get-TreeSample $app.Process.Id
    $row = [ordered]@{ run = $i; warmup = ((-not $SkipWarmup) -and $i -eq 0); connectedMs = [Math]::Round($app.ConnectedMs, 1); readyMs = [Math]::Round($app.ReadyMs, 1); atReady = $settled }
    $startupRows += $row
    if (-not $row.warmup) { $startupConnected.Add($row.connectedMs); $startupReady.Add($row.readyMs) }
    Write-Output ("startup[{0}]{1}: ready {2:n0} ms" -f $i, $(if ($row.warmup) { " (warm-up)" } else { "" }), $row.readyMs)
  }
  finally { if ($app.Socket) { $app.Socket.Dispose() }; Stop-AppTree $app.Process }
  Start-Sleep -Seconds 2
}

# Idle + route growth + optional interactive scan share one long-lived instance.
$app = Start-App
$idle = @()
$routeSamples = @()
$interactiveResult = $null
try {
  Start-Sleep -Seconds 5  # let first-render work settle before idle sampling
  $idleStart = Get-TreeSample $app.Process.Id
  for ($s = 1; $s -le $IdleSeconds; $s++) {
    Start-Sleep -Seconds 1
    if ($s % 5 -eq 0 -or $s -eq $IdleSeconds) { $idle += (Get-TreeSample $app.Process.Id) }
  }
  $idleEnd = $idle[$idle.Count - 1]
  $idleCpuPercent = 100.0 * ($idleEnd.cpuSeconds - $idleStart.cpuSeconds) / ($IdleSeconds * [Environment]::ProcessorCount)

  $routeSamples += [ordered]@{ cycle = 0; sample = (Get-TreeSample $app.Process.Id) }
  for ($c = 1; $c -le $RouteCycles; $c++) {
    Invoke-RouteCycle $app.Socket
    Start-Sleep -Seconds 1
    $routeSamples += [ordered]@{ cycle = $c; sample = (Get-TreeSample $app.Process.Id) }
  }

  if ($Interactive) {
    Write-Host ""
    Write-Host "=== HANDS-ON STEP ==============================================" -ForegroundColor Cyan
    Write-Host "A folder picker will open in the app. Choose EXACTLY:`n  $CorpusRoot"
    Write-Host "Type 'go' then Enter here when ready."
    while ((Read-Host) -ne "go") { }
    $rootJson = $CorpusRoot | ConvertTo-Json -Compress
    $expression = @"
(async () => {
  const invoke = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body)));
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const choice = await invoke('choose_storage_root', { module: 'largeFiles' });
  if (!choice) return { ok: false, message: 'picker cancelled' };
  if (choice.displayPath.toLowerCase() !== $rootJson.toLowerCase()) return { ok: false, message: 'wrong folder: ' + choice.displayPath };
  const gaps = []; let last = performance.now(); let running = true;
  const frame = (t) => { gaps.push(t - last); last = t; if (running) requestAnimationFrame(frame); };
  requestAnimationFrame(frame);
  const filter = { minimumBytes: 1048576, maximumBytes: null, extensions: [], category: 'any', sort: 'size', descending: true };
  const started = performance.now();
  const snapshotId = await invoke('start_large_files', { rootId: choice.rootId, depth: 32, filter });
  let firstProgressMs = null, cancelRequestedMs = null, status;
  for (;;) {
    status = await invoke('storage_scan_status', { module: 'largeFiles', snapshotId });
    const now = performance.now() - started;
    if (firstProgressMs === null && status.visitedEntries > 0) firstProgressMs = now;
    if (['complete','cancelled','failed'].includes(status.phase)) break;
    // Cancel halfway through the manifest's entry count to measure UI-side cancellation.
    if (cancelRequestedMs === null && status.visitedEntries >= window.__perfCancelAt) {
      cancelRequestedMs = now; await invoke('cancel_storage_scan', { module: 'largeFiles', snapshotId });
    }
    await sleep(16);
  }
  const endedMs = performance.now() - started;
  running = false;
  await invoke('release_storage_scan', { module: 'largeFiles', snapshotId });
  const sorted = [...gaps].sort((a, b) => a - b);
  const pct = (p) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)] : null;
  return { ok: true, phase: status.phase, visitedEntries: status.visitedEntries, firstProgressMs, cancelRequestedMs,
    cancelLatencyMs: cancelRequestedMs === null ? null : endedMs - cancelRequestedMs, totalMs: endedMs,
    frames: sorted.length, frameGapMs: { p50: pct(0.5), p90: pct(0.9), p99: pct(0.99), max: sorted[sorted.length - 1] ?? null },
    longFrames: { over50: gaps.filter(g => g > 50).length, over100: gaps.filter(g => g > 100).length } };
})()
"@
    $manifest = Get-Content -LiteralPath (Join-Path $CorpusRoot "perf-manifest.json") -Raw | ConvertFrom-Json
    $cancelAt = [int64]([double]$manifest.counts.files * 0.5)
    [void](Invoke-WebViewExpression -Socket $app.Socket -Expression "(() => { window.location.hash = '#/large-files'; window.__perfCancelAt = $cancelAt; return true; })()")
    $interactiveResult = Invoke-WebViewExpression -Socket $app.Socket -Expression $expression
    $interactiveResult | Add-Member -NotePropertyName afterScan -NotePropertyValue (Get-TreeSample $app.Process.Id)
    if ($interactiveResult.ok -ne $true) {
      Write-Output "interactive scan: NOT MEASURED ($($interactiveResult.message))"
    }
    else {
      Write-Output ("interactive scan: phase {0}, cancel latency {1} ms" -f $interactiveResult.phase, $interactiveResult.cancelLatencyMs)
    }
  }
}
finally { if ($app.Socket) { $app.Socket.Dispose() }; Stop-AppTree $app.Process }

$first = $routeSamples[0].sample
$last = $routeSamples[$routeSamples.Count - 1].sample
Write-PerfJson -Path (Join-Path $runDir "results.json") -Value ([ordered]@{
  schemaVersion = 1
  sleepEvents = @(Get-PerfSleepEvents -SinceUtc $perfStartedUtc)
  kind = "app"
  runId = Split-Path $runDir -Leaf
  executable = $exe
  executableSha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
  environment = $environment
  startup = [ordered]@{
    runs = $startupRows
    connectedMs = Get-PerfStats -Values $startupConnected.ToArray()
    readyMs = Get-PerfStats -Values $startupReady.ToArray()
  }
  idle = [ordered]@{
    seconds = $IdleSeconds
    cpuPercentOfMachine = [Math]::Round($idleCpuPercent, 3)
    start = $idleStart
    samples = $idle
    workingSetGrowthBytes = $idleEnd.workingSetBytes - $idleStart.workingSetBytes
    handleGrowth = $idleEnd.handles - $idleStart.handles
    # The app's own process; WebView2 helper processes fluctuate by ~100 handles on their own.
    mainHandleGrowth = $idleEnd.mainHandles - $idleStart.mainHandles
  }
  routes = [ordered]@{
    visitedPerCycle = $routes
    cycles = $RouteCycles
    samples = $routeSamples
    workingSetGrowthBytes = $last.workingSetBytes - $first.workingSetBytes
    privateGrowthBytes = $last.privateBytes - $first.privateBytes
    mainHandleGrowth = $last.mainHandles - $first.mainHandles
    treeHandleGrowth = $last.handles - $first.handles
  }
  interactive = $interactiveResult
})
Write-Output "Wrote $(Join-Path $runDir 'results.json')"
