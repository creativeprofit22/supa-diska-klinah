param(
  [Parameter(Mandatory = $true)][ValidateSet("Day0", "Day1")][string]$Phase,
  [string]$Executable = "C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/purge-drill"
)
$ErrorActionPreference = "Stop"

# Automatic-cleanup grace/due-purge drill on the installed app, in two sessions.
# The app is launched hidden with TMP and TEMP pointed at a disposable marked
# folder, so its temp cleaner can only see that folder. Before auto-cleanup is
# switched on, the temp scan must find exactly the fixture's 'cache' folder and
# nothing else, or the run aborts. Auto-cleanup is always switched back to its
# original setting before the app closes. App state files are only read.
#   Day0: quarantine the fixture with a 1-day grace; nothing may be purged.
#   Day1 (after the deadline): switching auto-cleanup on purges it.
. (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run the purge drill from a standard-user session."
}
if (Get-Process -Name "supa-diska-klinah" -ErrorAction SilentlyContinue) {
  throw "Close Supa Diska Klinah first: a running copy would not see the drill's TEMP."
}

$exe = (Resolve-Path -LiteralPath $Executable).Path
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
$artifacts = (Resolve-Path -LiteralPath $ArtifactDirectory).Path
$handoffPath = Join-Path $artifacts "handoff.json"
$cleanupData = Join-Path $env:APPDATA "com.supadiskaklinah.app\cleanup"

# Read-only view of the app's quarantine journals.
function Get-QuarantineRecords {
  $records = [ordered]@{}
  $executions = Join-Path $cleanupData "executions"
  if (-not (Test-Path -LiteralPath $executions)) { return $records }
  foreach ($file in Get-ChildItem -LiteralPath $executions -Filter *.json | Sort-Object Name) {
    $journal = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    if ($journal.disposition -ne "quarantine") { continue }
    $held = @($journal.items | Where-Object { $_.state -eq "quarantined" }).Count
    $records[$journal.executionId] = [ordered]@{
      held = $held
      purgeAfter = $journal.purgeAfter
      startedAt = $journal.startedAt
      states = @($journal.items | ForEach-Object { $_.state })
    }
  }
  return $records
}

function Remove-DrillFixture {
  param([string]$Root)
  if (-not $Root -or -not (Test-Path -LiteralPath $Root)) { return "already-absent" }
  if (-not (Test-Path -LiteralPath (Join-Path $Root ".acceptance-fixture"))) { return "refused-missing-marker" }
  Remove-Item -LiteralPath $Root -Recurse -Force
  return "removed"
}

function Invoke-DrillStep {
  param([Net.WebSockets.ClientWebSocket]$Socket, [string]$Name, [string]$Body)
  $expression = @"
(async () => {
  const plain = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const invoke = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body)));
  const norm = (p) => String(p ?? '').replace(/^\\\\\?\\/, '').toLowerCase();
  $Body
})().then(value => ({ ok: true, value }), error => ({ ok: false, message: String(error?.message ?? error?.code ?? error) }))
"@
  $result = Invoke-WebViewExpression -Socket $Socket -Expression $expression
  if (-not $result.ok) { throw "Step '$Name' failed: $($result.message)" }
  return $result.value
}

$record = [ordered]@{
  phase = $Phase
  startedUtc = (Get-Date).ToUniversalTime().ToString("o")
  executable = $exe
  executableSha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
  recordsBefore = Get-QuarantineRecords
  steps = [ordered]@{}
}

if ($Phase -eq "Day0") {
  if (Test-Path -LiteralPath $handoffPath) { throw "A day-0 handoff already exists: $handoffPath. Run Day1 or remove it deliberately." }
  $fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("purge-drill-" + [Guid]::NewGuid().ToString("n"))
  New-Item -ItemType Directory -Path (Join-Path $fixtureRoot "cache") -Force | Out-Null
  Set-Content -LiteralPath (Join-Path $fixtureRoot ".acceptance-fixture") -Value "disposable" -Encoding ASCII
  $payload = [byte[]]::new(512KB)
  [Random]::new(91).NextBytes($payload)
  [IO.File]::WriteAllBytes((Join-Path $fixtureRoot "cache\payload.bin"), $payload)
  $record.payloadSha256 = (Get-FileHash -LiteralPath (Join-Path $fixtureRoot "cache\payload.bin") -Algorithm SHA256).Hash
}
else {
  if (-not (Test-Path -LiteralPath $handoffPath)) { throw "No day-0 handoff at $handoffPath." }
  $handoff = Get-Content -LiteralPath $handoffPath -Raw | ConvertFrom-Json
  $fixtureRoot = $handoff.fixtureRoot
  if (-not (Test-Path -LiteralPath (Join-Path $fixtureRoot ".acceptance-fixture"))) { throw "Day-0 fixture marker is missing: $fixtureRoot" }
  $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
  if ($now -le [int64]$handoff.purgeAfter) {
    throw "Not due yet: the deadline is $($handoff.purgeAfterUtc) and it is now $([DateTimeOffset]::FromUnixTimeSeconds($now).ToString('o'))."
  }
  $record.handoff = $handoff
}
$fixtureRoot = (Get-Item -LiteralPath $fixtureRoot).FullName
$record.fixtureRoot = $fixtureRoot
$fixtureJson = ConvertTo-Json $fixtureRoot
Write-Host "Fixture root: $fixtureRoot"

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$saved = @{ TMP = $env:TMP; TEMP = $env:TEMP; WV = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS }
$process = $null
$socket = $null
$originalPolicy = $null
$policyChanged = $false
$outcome = "aborted"

try {
  $env:TMP = $fixtureRoot
  $env:TEMP = $fixtureRoot
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
  # Hidden start puts the app in background mode: no window, no focus, no dialogs.
  $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Hidden -PassThru
  $env:TMP = $saved.TMP
  $env:TEMP = $saved.TEMP
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $saved.WV
  $record.processId = $process.Id
  $socket = Connect-WebViewDebugSocket -Port $port
  Wait-WebViewExpression -Socket $socket -Expression "!!document.querySelector('main h1') && typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "Native app did not finish mounting"

  $originalPolicy = Invoke-DrillStep -Socket $socket -Name "readPolicy" -Body "return await plain('get_auto_cleanup_policy', {});"
  $record.steps.originalPolicy = $originalPolicy
  if ($originalPolicy.enabled) { throw "Auto-cleanup is already on; the drill only runs from the default off state." }

  # Scope proof: the temp scan must see exactly the fixture's 'cache' folder.
  $scan = Invoke-DrillStep -Socket $socket -Name "scopeProof" -Body @"
    const root = norm($fixtureJson);
    const preview = await plain('preview_cleanup', {});
    const records = preview.records.map(r => ({ path: r.displayPath, bytes: r.bytes }));
    const outside = records.filter(r => !norm(r.path).startsWith(root + '\\'));
    const exact = records.filter(r => norm(r.path) === root + '\\cache');
    return { count: records.length, outsideCount: outside.length, outside, exactCount: exact.length, records, diagnostics: (preview.diagnostics || []).length };
"@
  $record.steps.scopeProof = $scan
  $expectedCount = 1
  if ($Phase -eq "Day1") { $expectedCount = 0 }
  if ([int]$scan.outsideCount -ne 0) { throw "ABORT: the temp scan found $($scan.outsideCount) candidate(s) outside the fixture." }
  if ([int]$scan.count -ne $expectedCount -or ([int]$scan.exactCount -ne $expectedCount)) {
    throw "ABORT: expected exactly $expectedCount fixture candidate(s), found $($scan.count) (exact matches $($scan.exactCount))."
  }

  $before = Invoke-DrillStep -Socket $socket -Name "historyBefore" -Body @"
    const page = await invoke('cleanup_history', { cursor: null, limit: 50 });
    window.__drillKnown = page.records.map(r => r.executionId);
    return { count: window.__drillKnown.length };
"@

  # Switching auto-cleanup on runs maintenance immediately: quarantine, then due purge.
  $policyChanged = $true
  $record.steps.enable = Invoke-DrillStep -Socket $socket -Name "enable" -Body "return await plain('set_auto_cleanup_policy', { enabled: true, graceDays: 1 });"
  $record.steps.enabledAtUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

  $record.steps.historyBefore = $before
  if ($Phase -eq "Day0") {
    $record.steps.afterEnable = Invoke-DrillStep -Socket $socket -Name "afterEnable" -Body @"
      const known = new Set(window.__drillKnown);
      const page = await invoke('cleanup_history', { cursor: null, limit: 50 });
      const fresh = page.records.filter(r => !known.has(r.executionId));
      return { fresh: fresh.map(r => ({ executionId: r.executionId, disposition: r.disposition, completed: r.completed, purgeAfter: r.purgeAfter ?? null,
        items: r.items.map(i => ({ state: i.state, displayPath: i.displayPath ?? null, logicalBytes: i.logicalBytes })), accounting: r.accounting })) };
"@
    $fresh = @($record.steps.afterEnable.fresh)
    if ($fresh.Count -ne 1) { throw "Expected exactly one new cleanup record, found $($fresh.Count)." }
    $held = $fresh[0]
    if ($held.disposition -ne "quarantine" -or @($held.items).Count -ne 1 -or $held.items[0].state -ne "quarantined") {
      throw "The new record is not a single quarantined item."
    }
    if (-not $held.purgeAfter) { throw "The quarantine has no purge deadline." }
    if ([int64]$held.accounting.purgedBytes -ne 0) { throw "Something was purged before the deadline." }
    if (Test-Path -LiteralPath (Join-Path $fixtureRoot "cache")) { throw "The fixture 'cache' folder is still on disk." }
    $record.executionId = $held.executionId
  }
  else {
    $heldJson = ConvertTo-Json ([string]$handoff.executionId)
    $record.steps.afterEnable = Invoke-DrillStep -Socket $socket -Name "afterEnable" -Body @"
      const known = new Set(window.__drillKnown);
      const page = await invoke('cleanup_history', { cursor: null, limit: 50 });
      const target = page.records.find(r => r.executionId === $heldJson);
      const fresh = page.records.filter(r => !known.has(r.executionId)).length;
      if (!target) throw Error('the day-0 quarantine is missing from history');
      return { fresh, target: { executionId: target.executionId, disposition: target.disposition, purgeAfter: target.purgeAfter ?? null,
        items: target.items.map(i => ({ state: i.state, displayPath: i.displayPath ?? null, logicalBytes: i.logicalBytes })), accounting: target.accounting } };
"@
    if ([int]$record.steps.afterEnable.fresh -ne 0) { throw "Switching auto-cleanup on created new cleanup records on day 1." }
    if ($record.steps.afterEnable.target.items[0].state -ne "purged") { throw "The due quarantine was not purged." }
  }

  $record.steps.restorePolicy = Invoke-DrillStep -Socket $socket -Name "restorePolicy" -Body @"
    return await plain('set_auto_cleanup_policy', { enabled: false, graceDays: $([int]$originalPolicy.graceDays) });
"@
  $policyChanged = $false
  $record.steps.policyAfter = Invoke-DrillStep -Socket $socket -Name "policyAfter" -Body "return await plain('get_auto_cleanup_policy', {});"
  if ($record.steps.policyAfter.enabled -or ([int]$record.steps.policyAfter.graceDays -ne [int]$originalPolicy.graceDays)) {
    throw "Auto-cleanup was not restored to its original setting."
  }
  $outcome = "completed"
}
catch {
  $record.abortReason = $_.Exception.Message
  Write-Host "ABORTED: $($_.Exception.Message)" -ForegroundColor Yellow
}
finally {
  $env:TMP = $saved.TMP
  $env:TEMP = $saved.TEMP
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $saved.WV
  if ($policyChanged -and $socket -and $originalPolicy) {
    try {
      $record.steps.emergencyRestorePolicy = Invoke-DrillStep -Socket $socket -Name "emergencyRestorePolicy" -Body @"
        return await plain('set_auto_cleanup_policy', { enabled: false, graceDays: $([int]$originalPolicy.graceDays) });
"@
      $policyChanged = $false
    }
    catch { $record.emergencyRestoreError = $_.Exception.Message }
  }
  if ($policyChanged) {
    $record.actionRequired = "Auto-cleanup may still be ON. Open Settings and switch automatic cleanup off before launching the app normally."
    Write-Host $record.actionRequired -ForegroundColor Red
  }
  if ($socket) { $socket.Dispose() }
  if ($process -and -not $process.HasExited) {
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit(15000) | Out-Null
  }
  $record.recordsAfter = Get-QuarantineRecords
  $record.fixtureAfter = @(Get-ChildItem -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($fixtureRoot.Length).TrimStart("\") })
  $held = $null
  if ($record.executionId) { $held = $record.recordsAfter[$record.executionId] }
  if ($Phase -eq "Day0" -and $outcome -eq "completed" -and $held) {
    $record.deadline = [ordered]@{
      startedAt = $held.startedAt
      purgeAfter = $held.purgeAfter
      graceSeconds = [int64]$held.purgeAfter - [int64]$held.startedAt
      purgeAfterUtc = [DateTimeOffset]::FromUnixTimeSeconds([int64]$held.purgeAfter).ToString("o")
    }
    [ordered]@{
      fixtureRoot = $fixtureRoot
      executionId = $record.executionId
      purgeAfter = $held.purgeAfter
      purgeAfterUtc = $record.deadline.purgeAfterUtc
      originalPolicy = $originalPolicy
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $handoffPath -Encoding UTF8
  }
  elseif ($Phase -eq "Day0" -and -not $record.executionId) {
    # Nothing was quarantined, so the fixture is not needed for day 1.
    $record.fixtureCleanup = Remove-DrillFixture -Root $fixtureRoot
  }
  elseif ($Phase -eq "Day1" -and $outcome -eq "completed") {
    $record.fixtureCleanup = Remove-DrillFixture -Root $fixtureRoot
    Move-Item -LiteralPath $handoffPath -Destination (Join-Path $artifacts "handoff.day1-consumed.json") -Force
  }
  $record.outcome = $outcome
  $record.finishedUtc = (Get-Date).ToUniversalTime().ToString("o")
  $reportPath = Join-Path $artifacts ("acceptance-" + $Phase.ToLowerInvariant() + ".json")
  $record | ConvertTo-Json -Depth 24 | Set-Content -LiteralPath $reportPath -Encoding UTF8
  Write-Host "Purge drill $Phase record written to $reportPath (outcome: $outcome)"
}
if ($outcome -ne "completed") { exit 1 }
