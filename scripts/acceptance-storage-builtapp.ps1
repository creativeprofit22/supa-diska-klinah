param(
  [string]$Executable = "src-tauri/target/x86_64-pc-windows-msvc/debug/supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/storage-acceptance",
  [string]$SecondVolumeRoot = ""
)
$ErrorActionPreference = "Stop"

# Bounded built-app acceptance for the destructive storage chain. Unlike
# smoke-storage-root.ps1 this DOES open the native picker and the native
# permanent-deletion confirmation, so it requires a human at the machine.
# It never sends synthetic desktop input and never captures the screen.
# It mutates only disposable fixture roots it creates itself.
. (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run storage acceptance from a standard-user session."
}

function New-AcceptanceFixture {
  param([Parameter(Mandatory = $true)][string]$Parent)
  $root = Join-Path $Parent ("storage-acceptance-" + [Guid]::NewGuid().ToString("n"))
  New-Item -ItemType Directory -Path $root -Force | Out-Null
  # Ownership marker: cleanup refuses to remove any tree without it.
  Set-Content -LiteralPath (Join-Path $root ".acceptance-fixture") -Value "disposable" -Encoding ASCII

  $make = {
    param($path, $sizeBytes, $seed)
    $bytes = [byte[]]::new($sizeBytes)
    $rng = [Random]::new($seed)
    $rng.NextBytes($bytes)
    [IO.File]::WriteAllBytes($path, $bytes)
  }

  New-Item -ItemType Directory -Path (Join-Path $root "large") -Force | Out-Null
  & $make (Join-Path $root "large\alpha.bin") (3MB) 11
  & $make (Join-Path $root "large\beta.bin") (2MB) 22
  & $make (Join-Path $root "large\gamma.bin") (1MB) 33
  Set-Content -LiteralPath (Join-Path $root "large\tiny.txt") -Value "small" -Encoding ASCII

  # Duplicate group: three byte-identical copies in distinct directories.
  $dupPayload = [byte[]]::new(1MB)
  [Random]::new(77).NextBytes($dupPayload)
  foreach ($name in @("one", "two", "three")) {
    $dir = Join-Path $root "dupes\$name"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $dir "copy.bin"), $dupPayload)
  }

  New-Item -ItemType Directory -Path (Join-Path $root "empties\parent\childA") -Force | Out-Null
  New-Item -ItemType Directory -Path (Join-Path $root "empties\parent\childB") -Force | Out-Null
  New-Item -ItemType Directory -Path (Join-Path $root "empties\late") -Force | Out-Null
  New-Item -ItemType Directory -Path (Join-Path $root "deep\l1\l2\l3\l4") -Force | Out-Null
  & $make (Join-Path $root "deep\l1\l2\l3\l4\deep.bin") (256KB) 44
  return $root
}

function Get-AcceptanceInventory {
  param([Parameter(Mandatory = $true)][string]$Root)
  $items = [ordered]@{}
  foreach ($entry in Get-ChildItem -LiteralPath $Root -Recurse -Force) {
    $relative = $entry.FullName.Substring($Root.Length).TrimStart("\")
    if ($entry.PSIsContainer) {
      $items[$relative] = [ordered]@{ kind = "directory" }
    }
    else {
      $items[$relative] = [ordered]@{
        kind = "file"
        bytes = $entry.Length
        sha256 = (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash
      }
    }
  }
  return $items
}

function Remove-AcceptanceFixture {
  param([Parameter(Mandatory = $true)][string]$Root)
  if (-not (Test-Path -LiteralPath $Root)) { return "already-absent" }
  if (-not (Test-Path -LiteralPath (Join-Path $Root ".acceptance-fixture"))) {
    return "refused-missing-marker"
  }
  Remove-Item -LiteralPath $Root -Recurse -Force
  return "removed"
}

function Invoke-AcceptanceStep {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Body,
    [string]$Name = "unnamed"
  )
  $expression = @"
(async () => {
  const invoke = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body)));
  const plain = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const settle = async (module, snapshotId, budgetMs = 30000) => {
    const deadline = Date.now() + budgetMs;
    let polls = 0;
    for (;;) {
      const status = await invoke('storage_scan_status', { module, snapshotId });
      polls += 1;
      if (['complete','cancelled','failed'].includes(status.phase)) return { status, polls };
      if (Date.now() > deadline) throw Error('scan did not settle: ' + status.phase);
      await new Promise(r => setTimeout(r, 100));
    }
  };
  const memory = () => (performance.memory ? performance.memory.usedJSHeapSize : null);
  $Body
})().then(value => ({ ok: true, value }), error => ({ ok: false, code: error?.code ?? null, message: String(error?.message ?? error?.code ?? error) }))
"@
  $result = Invoke-WebViewExpression -Socket $Socket -Expression $expression
  if (-not $result.ok) {
    # A single refused step must not discard the whole session: every hands-on
    # dialog is a person's time. Record the failure and let the run continue so
    # one pass surfaces every defect instead of only the first.
    Write-Host "STEP FAILED [$Name]: $($result.message)" -ForegroundColor Yellow
    $script:StepFailures += [ordered]@{ step = $Name; code = $result.code; message = $result.message }
    return [ordered]@{ acceptanceStepFailed = $true; code = $result.code; message = $result.message }
  }
  return $result.value
}

function Wait-ForOperator {
  param([Parameter(Mandatory = $true)][string]$Instruction)
  # 'go' rather than a bare Enter: a stray Enter lands in the native dialog and
  # silently accepts the highlighted folder instead of signalling this console.
  Write-Host ""
  Write-Host "=== HANDS-ON STEP =============================================="
  Write-Host $Instruction
  Write-Host "Then type  go  here and press Enter."
  Write-Host "================================================================"
  for (;;) {
    $reply = (Read-Host).Trim().ToLowerInvariant()
    if ($reply -eq "go") { return }
    if ($reply -eq "abort") { throw "Operator aborted at a hands-on step." }
    Write-Host "Type 'go' to continue, or 'abort' to stop."
  }
}

$exe = (Resolve-Path -LiteralPath $Executable).Path
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
$artifacts = (Resolve-Path -LiteralPath $ArtifactDirectory).Path

$fixtureRoot = New-AcceptanceFixture -Parent ([IO.Path]::GetTempPath())
$fixtureBefore = Get-AcceptanceInventory -Root $fixtureRoot
$crossVolumeRoot = ""
if ($SecondVolumeRoot) {
  $crossVolumeRoot = New-AcceptanceFixture -Parent $SecondVolumeRoot
}
Write-Host "Fixture root: $fixtureRoot"
if ($crossVolumeRoot) { Write-Host "Cross-volume fixture root: $crossVolumeRoot" }

$script:StepFailures = @()
$script:RunFinalized = $false
function Complete-AcceptanceRun {
  param([string]$Outcome = "aborted", [string]$Reason = "")
  if ($script:RunFinalized) { return }
  $script:RunFinalized = $true
  $record.outcome = $Outcome
  if ($Reason) { $record.abortReason = $Reason }
  $record.stepFailures = $script:StepFailures
  if (Test-Path -LiteralPath $fixtureRoot) {
    $record.fixtureFinal = Get-AcceptanceInventory -Root $fixtureRoot
  }
  # Only ever removes trees carrying this run's own ownership marker.
  $record.cleanup = [ordered]@{ fixtureRoot = Remove-AcceptanceFixture -Root $fixtureRoot }
  if ($crossVolumeRoot) { $record.cleanup.crossVolumeRoot = Remove-AcceptanceFixture -Root $crossVolumeRoot }
  $record.finishedUtc = (Get-Date).ToUniversalTime().ToString("o")
  $reportPath = Join-Path $artifacts "acceptance.json"
  $record | ConvertTo-Json -Depth 24 | Set-Content -LiteralPath $reportPath -Encoding UTF8
  Write-Host "Acceptance record written to $reportPath (outcome: $Outcome)"
  if ($script:StepFailures.Count) {
    Write-Host "Steps that failed: $(($script:StepFailures | ForEach-Object { $_.step }) -join ', ')" -ForegroundColor Yellow
  }
}
trap {
  Complete-AcceptanceRun -Outcome "aborted" -Reason $_.Exception.Message
  break
}
$record = [ordered]@{
  startedUtc = (Get-Date).ToUniversalTime().ToString("o")
  executable = $exe
  executableSha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
  fixtureRoot = $fixtureRoot
  crossVolumeRoot = $crossVolumeRoot
  fixtureBefore = $fixtureBefore
  steps = [ordered]@{}
}

$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
$process = $null
$socket = $null

try {
  # Normal window: the operator must be able to reach the native dialogs.
  $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -PassThru
  $socket = Connect-WebViewDebugSocket -Port $port
  Wait-WebViewExpression -Socket $socket -Expression "!!document.querySelector('main h1') && typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "Native app did not finish mounting"
  $record.processId = $process.Id

  # --- Gate 1: native picker cancellation -------------------------------
  Wait-ForOperator -Instruction "A folder picker will open. Press Cancel (or Esc). Do not choose anything."
  # A mis-click here must not destroy the whole session: retry the gate.
  $cancelAttempts = @()
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    $observed = Invoke-AcceptanceStep -Socket $socket -Body @"
    const before = memory();
    const choice = await invoke('choose_storage_root', { module: 'largeFiles' });
    return { choice, cancelledToNull: choice === null, heapBefore: before, heapAfter: memory() };
"@
    $cancelAttempts += $observed
    if ($observed.cancelledToNull) { break }
    if ($attempt -eq 3) { throw "Picker cancellation never observed after 3 attempts." }
    Wait-ForOperator -Instruction "That dialog returned a CHOSEN folder, not a cancellation.`nThe picker opens once more: this time press Esc, or click Cancel with the mouse.`nDo not press Enter while the dialog has focus."
  }
  $record.steps.pickerCancel = [ordered]@{ attempts = $cancelAttempts; cancelledToNull = $true }

  # --- Gate 2: native picker selection ----------------------------------
  Wait-ForOperator -Instruction "The folder picker will open again. Choose EXACTLY this folder:`n  $fixtureRoot"
  $record.steps.pickerChoose = Invoke-AcceptanceStep -Socket $socket -Name "pickerChoose" -Body @"
    const choice = await invoke('choose_storage_root', { module: 'largeFiles' });
    if (!choice) throw Error('picker returned null on the selection gate');
    window.__acceptanceRoots = { largeFiles: choice.rootId };
    return choice;
"@
  $chosenPath = $record.steps.pickerChoose.displayPath
  if ($chosenPath -ne $fixtureRoot) {
    throw "Operator selected '$chosenPath' but the fixture root is '$fixtureRoot'."
  }

  # Authorizations are single-use AND only a couple may sit unused at once, so
  # each module is picked immediately before the scan that consumes it. That
  # mirrors the real UI (pick -> scan -> release) instead of hoarding roots.
  function Request-ModuleRoot {
    param([Parameter(Mandatory = $true)][string]$Module)
    Wait-ForOperator -Instruction "Picker opens for '$Module'. Choose the SAME folder again:`n  $fixtureRoot"
    $choice = Invoke-AcceptanceStep -Socket $socket -Name "root_$Module" -Body @"
      const choice = await invoke('choose_storage_root', { module: '$Module' });
      if (!choice) throw Error('picker returned null');
      window.__acceptanceRoots['$Module'] = choice.rootId;
      return choice;
"@
    if ($choice.acceptanceStepFailed) { return $choice }
    if ($choice.displayPath -ne $fixtureRoot) { throw "Wrong folder chosen for $Module." }
    $record.steps["root_$Module"] = $choice
    return $choice
  }

  # --- Analyzer navigation and totals -----------------------------------
  [void](Request-ModuleRoot -Module "diskAnalyzer")
  $record.steps.analyzer = Invoke-AcceptanceStep -Socket $socket -Name "analyzer" -Body @"
    const started = performance.now();
    const snapshotId = await invoke('start_disk_analyzer', { rootId: window.__acceptanceRoots.diskAnalyzer, displayedDepth: 2 });
    const settled = await settle('diskAnalyzer', snapshotId);
    const top = await invoke('storage_scan_page', { module:'diskAnalyzer', snapshotId, collection:'tree', pageSize: 50 });
    const children = [];
    for (const row of top.records.slice(0, 4)) {
      const id = row.record?.recordId;
      if (!id) continue;
      const page = await invoke('storage_scan_page', { module:'diskAnalyzer', snapshotId, collection:'tree', parentId: id, pageSize: 50 });
      children.push({ parent: row.record.displayPath, count: page.records.length, names: page.records.map(r => r.record.displayPath) });
    }
    return {
      snapshotId, phase: settled.status.phase, polls: settled.polls,
      elapsedMs: Math.round(performance.now() - started),
      visitedEntries: settled.status.visitedEntries,
      completeness: settled.status.completeness,
      top: top.records.map(r => ({ path: r.record.displayPath, logicalBytes: r.record.logicalBytes, allocatedBytes: r.record.allocatedBytes, depth: r.record.depth })),
      children
    };
"@

  # --- Large files: scan and cursor paging -------------------------------
  # One pick authorizes exactly one scan (the UI states this: "Folder
  # authorizations are single-use"), so the filter and cancellation variants
  # stay in the native IPC fixtures rather than costing extra native dialogs.
  $record.steps.largeFiles = Invoke-AcceptanceStep -Socket $socket -Name "largeFiles" -Body @"
    const rootId = window.__acceptanceRoots.largeFiles;
    const filter = { minimumBytes: 1048576, maximumBytes: null, extensions: [], category: 'any', sort: 'size', descending: true };
    const heapBefore = memory();
    const snapshotId = await invoke('start_large_files', { rootId, depth: 8, filter });
    const settled = await settle('largeFiles', snapshotId);
    const first = await invoke('storage_scan_page', { module:'largeFiles', snapshotId, collection:'files', pageSize: 2 });
    let second = null;
    if (first.nextCursor) second = await invoke('storage_scan_page', { module:'largeFiles', snapshotId, collection:'files', cursor: first.nextCursor, pageSize: 2 });
    const rows = [...first.records, ...(second ? second.records : [])].map(r => ({
      path: r.record.displayPath, logicalBytes: r.record.logicalBytes, eligibility: r.record.eligibility.kind,
      candidateId: r.record.eligibility.kind === 'eligible' ? r.record.eligibility.candidate_id : null
    }));
    window.__acceptanceLarge = { snapshotId, rows };
    // A consumed pick must not authorize a second scan.
    let reuseRefused = null;
    try { await invoke('start_large_files', { rootId, depth: 8, filter }); reuseRefused = 'accepted-second-scan'; }
    catch (e) { reuseRefused = e.code; }
    return {
      snapshotId, phase: settled.status.phase, polls: settled.polls, retainedTotal: first.retainedTotal,
      firstPage: first.records.length, pagedTotal: rows.length, hadCursor: !!first.nextCursor, rows,
      reuseRefused, heapBefore, heapAfter: memory()
    };
"@

  # --- Duplicates: keeper retention -------------------------------------
  [void](Request-ModuleRoot -Module "duplicates")
  $record.steps.duplicates = Invoke-AcceptanceStep -Socket $socket -Name "duplicates" -Body @"
    const snapshotId = await invoke('start_duplicates', { rootId: window.__acceptanceRoots.duplicates, depth: 8, minimumBytes: 1024 });
    const settled = await settle('duplicates', snapshotId, 60000);
    const groups = await invoke('storage_scan_page', { module:'duplicates', snapshotId, collection:'duplicateGroups', pageSize: 10 });
    const detail = [];
    for (const group of groups.records) {
      const groupId = group.record.groupId ?? group.record.recordId;
      const members = await invoke('storage_scan_page', { module:'duplicates', snapshotId, collection:'duplicateMembers', parentId: groupId, pageSize: 20 });
      detail.push({
        groupId,
        members: members.records.map(m => ({
          path: m.record.file.displayPath, bytes: m.record.file.logicalBytes,
          eligibility: m.record.file.eligibility.kind,
          candidateId: m.record.file.eligibility.kind === 'eligible' ? m.record.file.eligibility.candidate_id : null
        }))
      });
    }
    const first = detail[0];
    const keepers = first ? first.members.filter(m => m.eligibility !== 'eligible').length : 0;
    const eligible = first ? first.members.filter(m => m.eligibility === 'eligible').length : 0;
    // Every member is listed as eligible (nothing is auto-selected); the safety
    // property is that a plan wiping out an ENTIRE group must be refused, while
    // leaving one copy behind must be accepted.
    const ids = first ? first.members.filter(m => m.candidateId).map(m => m.candidateId) : [];
    let wholeGroupRefused = null, partialAccepted = null;
    if (ids.length > 1) {
      try {
        await invoke('create_storage_plan', { selection: { module:'duplicates', snapshotId, candidateIds: ids }, disposition: 'recycleBin' });
        wholeGroupRefused = 'accepted-whole-group';
      } catch (e) { wholeGroupRefused = e.code; }
      try {
        const plan = await invoke('create_storage_plan', { selection: { module:'duplicates', snapshotId, candidateIds: ids.slice(0, ids.length - 1) }, disposition: 'recycleBin' });
        partialAccepted = { planId: plan.planId, selectedCount: plan.selectedCount };
      } catch (e) { partialAccepted = { refused: e.code }; }
    }
    await invoke('release_storage_scan', { module:'duplicates', snapshotId });
    return { phase: settled.status.phase, hashedBytes: settled.status.hashedBytes, groupCount: groups.records.length, detail, keepersRetained: keepers, eligibleMembers: eligible, wholeGroupRefused, partialAccepted };
"@

  # --- Empty folders: root retention and late-child refusal -------------
  [void](Request-ModuleRoot -Module "emptyFolders")
  $record.steps.emptyFoldersScan = Invoke-AcceptanceStep -Socket $socket -Name "emptyFoldersScan" -Body @"
    const snapshotId = await invoke('start_empty_folders', { rootId: window.__acceptanceRoots.emptyFolders, depth: 8 });
    const settled = await settle('emptyFolders', snapshotId);
    const page = await invoke('storage_scan_page', { module:'emptyFolders', snapshotId, collection:'emptyFolders', pageSize: 50 });
    const rows = page.records.map(r => ({
      path: r.record.displayPath, depth: r.record.depth, descendantDirectories: r.record.descendantDirectories,
      eligibility: r.record.eligibility.kind,
      candidateId: r.record.eligibility.kind === 'eligible' ? r.record.eligibility.candidate_id : null
    }));
    window.__acceptanceEmpty = { snapshotId, rows };
    return { phase: settled.status.phase, rows, rootListed: rows.some(r => r.depth === 0) };
"@

  # Mutate a scanned empty folder behind the snapshot's back.
  $lateChild = Join-Path $fixtureRoot "empties\late\appeared.txt"
  Set-Content -LiteralPath $lateChild -Value "late" -Encoding ASCII
  $record.steps.lateChildRefusal = Invoke-AcceptanceStep -Socket $socket -Name "lateChildRefusal" -Body @"
    const target = window.__acceptanceEmpty.rows.find(r => r.candidateId && /late$/i.test(r.path));
    if (!target) return { skipped: 'late folder was not an eligible candidate', rows: window.__acceptanceEmpty.rows };
    try {
      const plan = await invoke('create_storage_plan', { selection: { module:'emptyFolders', snapshotId: window.__acceptanceEmpty.snapshotId, candidateIds: [target.candidateId] }, disposition: 'recycleBin' });
      return { refused: false, plan };
    } catch (error) {
      return { refused: true, code: error?.code ?? String(error), target: target.path };
    }
"@
  Remove-Item -LiteralPath $lateChild -Force

  # --- Opaque module scope availability and safety refusal ---------------
  $record.steps.opaqueScopes = Invoke-AcceptanceStep -Socket $socket -Name "opaqueScopes" -Body @"
    const out = {};
    for (const module of ['cleaner','browser']) {
      const scopes = await invoke('list_storage_scopes', { module });
      out[module] = { count: scopes.length, shapesValid: scopes.every(s => /^[a-f0-9]{32}$/.test(s.scopeId) && s.module === module) };
      try { await invoke('choose_storage_root', { module }); out[module].pickerRefused = false; }
      catch (error) { out[module].pickerRefused = true; out[module].pickerCode = error?.code ?? String(error); }
    }
    const catalog = await invoke('list_cleaner_catalog', {});
    const policy = await invoke('list_browser_policy', {});
    return { ...out, catalogEntries: Array.isArray(catalog) ? catalog.length : null, policyEntries: Array.isArray(policy) ? policy.length : null };
"@

  # --- Immutable plan review + same-volume app recovery + undo -----------
  $record.steps.recovery = Invoke-AcceptanceStep -Socket $socket -Name "recovery" -Body @"
    const pick = window.__acceptanceLarge.rows.filter(r => r.candidateId).slice(0, 2);
    if (pick.length < 2) throw Error('not enough eligible large-file candidates');
    const selection = { module:'largeFiles', snapshotId: window.__acceptanceLarge.snapshotId, candidateIds: pick.map(r => r.candidateId) };
    const plan = await invoke('create_storage_plan', { selection, disposition: 'quarantine' });

    // Immutability: the same candidate set must mint a distinct plan, and the
    // returned summary must not be mutable by the renderer.
    const second = await invoke('create_storage_plan', { selection, disposition: 'quarantine' });
    const distinctPlans = plan.planId !== second.planId;

    const execution = await plain('execute_cleanup_plan', { planId: plan.planId });
    window.__acceptanceExecution = execution.executionId;
    return {
      selected: pick.map(r => ({ path: r.path, bytes: r.logicalBytes })),
      plan, distinctPlans, unusedPlanId: second.planId,
      execution: {
        executionId: execution.executionId, disposition: execution.disposition, completed: execution.completed,
        purgeAfter: execution.purgeAfter ?? null,
        items: execution.items.map(i => ({ state: i.state, logicalBytes: i.logicalBytes, failure: i.failure ?? null, displayPath: i.displayPath ?? null })),
        accounting: execution.accounting
      }
    };
"@

  $record.steps.afterRecovery = Get-AcceptanceInventory -Root $fixtureRoot

  $record.steps.undo = Invoke-AcceptanceStep -Socket $socket -Name "undo" -Body @"
    const summary = await plain('undo_cleanup', { executionId: window.__acceptanceExecution });
    const history = await invoke('cleanup_history', { cursor: null, limit: 10 });
    return {
      summary: { executionId: summary.executionId, completed: summary.completed, items: summary.items.map(i => ({ state: i.state, logicalBytes: i.logicalBytes, failure: i.failure ?? null })), accounting: summary.accounting },
      historyCount: Array.isArray(history?.records) ? history.records.length : (Array.isArray(history) ? history.length : null)
    };
"@

  $record.steps.afterUndo = Get-AcceptanceInventory -Root $fixtureRoot

  $record.steps.undoReplayRefusal = Invoke-AcceptanceStep -Socket $socket -Name "undoReplayRefusal" -Body @"
    try { const again = await plain('undo_cleanup', { executionId: window.__acceptanceExecution }); return { refused: false, again }; }
    catch (error) { return { refused: true, code: error?.code ?? String(error) }; }
"@

  # --- Guarded refusal after fixture identity change ---------------------
  # Undo restores content but not the original native identity, so the older
  # snapshot's candidates must no longer satisfy the plan's evidence check.
  $record.steps.guardedRefusalSetup = "undo restored the items; identity changed relative to the original snapshot"
  $record.steps.staleSelection = Invoke-AcceptanceStep -Socket $socket -Name "staleSelection" -Body @"
    const pick = window.__acceptanceLarge.rows.filter(r => r.candidateId).slice(0, 1);
    try {
      const plan = await invoke('create_storage_plan', { selection: { module:'largeFiles', snapshotId: window.__acceptanceLarge.snapshotId, candidateIds: pick.map(r => r.candidateId) }, disposition: 'recycleBin' });
      return { refused: false, plan };
    } catch (error) { return { refused: true, code: error?.code ?? String(error) }; }
"@

  if ($crossVolumeRoot) {
    Wait-ForOperator -Instruction "Picker opens once more. Choose the CROSS-VOLUME folder:`n  $crossVolumeRoot"
    $record.steps.crossVolume = Invoke-AcceptanceStep -Socket $socket -Name "crossVolume" -Body @"
      const choice = await invoke('choose_storage_root', { module: 'largeFiles' });
      if (!choice) throw Error('picker returned null');
      const filter = { minimumBytes: 1048576, maximumBytes: null, extensions: [], category: 'any', sort: 'size', descending: true };
      const snapshotId = await invoke('start_large_files', { rootId: choice.rootId, depth: 8, filter });
      const settled = await settle('largeFiles', snapshotId);
      const page = await invoke('storage_scan_page', { module:'largeFiles', snapshotId, collection:'files', pageSize: 5 });
      const candidates = page.records.map(r => r.record.eligibility).filter(e => e.kind === 'eligible').map(e => e.candidate_id);
      let recovery;
      try {
        const plan = await invoke('create_storage_plan', { selection: { module:'largeFiles', snapshotId, candidateIds: candidates.slice(0,1) }, disposition: 'quarantine' });
        recovery = { accepted: true, planId: plan.planId };
      } catch (error) { recovery = { accepted: false, code: error?.code ?? String(error) }; }
      await invoke('release_storage_scan', { module:'largeFiles', snapshotId });
      return { displayPath: choice.displayPath, phase: settled.status.phase, candidateCount: candidates.length, recovery };
"@
  }

  # --- Native permanent confirmation: default No, cancel then confirm ----
  # Needs its own authorization: the earlier pick was consumed by its scan.
  Wait-ForOperator -Instruction "Picker opens for the permanent-deletion check. Choose the SAME folder again:`n  $fixtureRoot"
  $record.steps.permanentRoot = Invoke-AcceptanceStep -Socket $socket -Name "permanentRoot" -Body @"
    const choice = await invoke('choose_storage_root', { module: 'largeFiles' });
    if (!choice) throw Error('picker returned no folder');
    window.__acceptanceRoots.permanent = choice.rootId;
    return { displayPath: choice.displayPath };
"@
  $record.steps.permanentPlan = Invoke-AcceptanceStep -Socket $socket -Name "permanentPlan" -Body @"
    const rootId = window.__acceptanceRoots.permanent;
    const filter = { minimumBytes: 1048576, maximumBytes: null, extensions: [], category: 'any', sort: 'size', descending: true };
    const snapshotId = await invoke('start_large_files', { rootId, depth: 8, filter });
    await settle('largeFiles', snapshotId);
    const page = await invoke('storage_scan_page', { module:'largeFiles', snapshotId, collection:'files', pageSize: 10 });
    const rows = page.records.map(r => ({ path: r.record.displayPath, bytes: r.record.logicalBytes, eligibility: r.record.eligibility }));
    const target = rows.find(r => r.eligibility.kind === 'eligible' && /gamma\.bin$/i.test(r.path)) ?? rows.find(r => r.eligibility.kind === 'eligible');
    if (!target) throw Error('no eligible candidate for permanent deletion');
    const planCancel = await invoke('create_storage_plan', { selection: { module:'largeFiles', snapshotId, candidateIds: [target.eligibility.candidate_id] }, disposition: 'permanent' });
    const planConfirm = await invoke('create_storage_plan', { selection: { module:'largeFiles', snapshotId, candidateIds: [target.eligibility.candidate_id] }, disposition: 'permanent' });
    window.__acceptancePermanent = { snapshotId, target: target.path, planCancel: planCancel.planId, planConfirm: planConfirm.planId };
    return { target: target.path, bytes: target.bytes, planCancel, planConfirm };
"@

  Wait-ForOperator -Instruction "A Windows permanent-deletion confirmation will appear.`nCHECK that the default focused button is 'No', then choose NO / Cancel."
  $record.steps.permanentCancel = Invoke-AcceptanceStep -Socket $socket -Name "permanentCancel" -Body @"
    try {
      const summary = await plain('execute_permanent_cleanup_plan', { planId: window.__acceptancePermanent.planCancel });
      return { cancelledCleanly: false, summary: { completed: summary.completed, items: summary.items.map(i => i.state), accounting: summary.accounting } };
    } catch (error) { return { cancelledCleanly: true, code: error?.code ?? String(error) }; }
"@
  $record.steps.afterPermanentCancel = Get-AcceptanceInventory -Root $fixtureRoot

  Wait-ForOperator -Instruction "The confirmation appears again for the SAME file. This time choose YES / Delete.`n(The file is a disposable fixture: $($record.steps.permanentPlan.target))"
  $record.steps.permanentConfirm = Invoke-AcceptanceStep -Socket $socket -Name "permanentConfirm" -Body @"
    const summary = await plain('execute_permanent_cleanup_plan', { planId: window.__acceptancePermanent.planConfirm });
    let undoRefused = null;
    try { await plain('undo_cleanup', { executionId: summary.executionId }); undoRefused = false; }
    catch (error) { undoRefused = error?.code ?? String(error); }
    return {
      executionId: summary.executionId, disposition: summary.disposition, completed: summary.completed,
      items: summary.items.map(i => ({ state: i.state, logicalBytes: i.logicalBytes, failure: i.failure ?? null })),
      accounting: summary.accounting, undoAfterPermanent: undoRefused
    };
"@
  $record.steps.afterPermanentConfirm = Get-AcceptanceInventory -Root $fixtureRoot

  # Process-tree sample: the app plus every descendant (WebView2 browser, renderer,
  # GPU and utility processes). Bounded to descendants of the launched PID only.
  function Get-AppProcessTree {
    param([Parameter(Mandatory = $true)][int]$RootId)
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name)
    $ids = [Collections.Generic.HashSet[int]]::new()
    [void]$ids.Add($RootId)
    do {
      $added = 0
      foreach ($p in $all) {
        if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added++ }
      }
    } while ($added -gt 0)
    $rows = foreach ($id in $ids) {
      $p = Get-Process -Id $id -ErrorAction SilentlyContinue
      if ($p) {
        [ordered]@{ pid = $p.Id; name = $p.ProcessName; workingSetBytes = $p.WorkingSet64; privateBytes = $p.PrivateMemorySize64; cpuSeconds = [math]::Round($p.TotalProcessorTime.TotalSeconds, 3); handles = $p.HandleCount; threads = $p.Threads.Count }
      }
    }
    $rows = @($rows)
    $total = @{ workingSetBytes = [int64]0; privateBytes = [int64]0; cpuSeconds = 0.0; handles = 0; threads = 0 }
    foreach ($row in $rows) { foreach ($key in @($total.Keys)) { $total[$key] += $row[$key] } }
    [ordered]@{
      at = (Get-Date).ToUniversalTime().ToString("o")
      processCount = $rows.Count
      workingSetBytes = $total.workingSetBytes
      privateBytes = $total.privateBytes
      cpuSeconds = [math]::Round($total.cpuSeconds, 3)
      handles = $total.handles
      threads = $total.threads
      processes = $rows
    }
  }

  # Authorizations are single-use, so every cycle picks a fresh root. The earlier
  # harness reused the consumed analyzer root and failed with invalid_evidence.
  $resource = [ordered]@{ treeStart = Get-AppProcessTree -RootId $process.Id; cycles = @() }
  for ($cycle = 1; $cycle -le 3; $cycle++) {
    $root = Request-ModuleRoot -Module "diskAnalyzer"
    if ($root.acceptanceStepFailed) { $resource.cycles += [ordered]@{ cycle = $cycle; root = $root }; break }
    $step = Invoke-AcceptanceStep -Socket $socket -Name "resourceCycle$cycle" -Body @"
      const began = performance.now();
      const heapBefore = memory();
      const snapshotId = await invoke('start_disk_analyzer', { rootId: window.__acceptanceRoots.diskAnalyzer, displayedDepth: 2 });
      const settled = await settle('diskAnalyzer', snapshotId);
      await invoke('storage_scan_page', { module:'diskAnalyzer', snapshotId, collection:'tree', pageSize: 50 });
      await invoke('release_storage_scan', { module:'diskAnalyzer', snapshotId });
      let afterRelease;
      try { await invoke('storage_scan_status', { module:'diskAnalyzer', snapshotId }); afterRelease = 'still-present'; }
      catch (error) { afterRelease = error?.code ?? String(error); }
      return { elapsedMs: Math.round(performance.now() - began), polls: settled.polls, phase: settled.status.phase, afterRelease, heapBefore, heapAfter: memory() };
"@
    $resource.cycles += [ordered]@{ cycle = $cycle; scan = $step; tree = Get-AppProcessTree -RootId $process.Id }
  }
  Start-Sleep -Seconds 3
  $resource.treeSettled = Get-AppProcessTree -RootId $process.Id
  $record.steps.resourceObservation = $resource
}
finally {
  if ($socket) { $socket.Dispose() }
  if ($process -and -not $process.HasExited) {
    $process.CloseMainWindow() | Out-Null
    if (-not $process.WaitForExit(5000)) { $process.Kill() }
  }
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArguments
}

# --- Restart reconciliation: no automatic deletion on a fresh launch -----
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$restartPort = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$restartPort"
$restart = $null
$restartSocket = $null
try {
  $beforeRestart = Get-AcceptanceInventory -Root $fixtureRoot
  $restart = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Minimized -PassThru
  $restartSocket = Connect-WebViewDebugSocket -Port $restartPort
  Wait-WebViewExpression -Socket $restartSocket -Expression "!!document.querySelector('main h1') && typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "Restarted app did not finish mounting"
  Start-Sleep -Seconds 5
  $record.steps.restart = [ordered]@{
    beforeRestart = $beforeRestart
    afterRestart = Get-AcceptanceInventory -Root $fixtureRoot
    history = Invoke-AcceptanceStep -Socket $restartSocket -Name "restartHistory" -Body @"
      const history = await invoke('cleanup_history', { cursor: null, limit: 20 });
      const records = Array.isArray(history?.records) ? history.records : (Array.isArray(history) ? history : []);
      return { count: records.length, states: records.map(r => ({ disposition: r.disposition, completed: r.completed, items: r.items?.map(i => i.state) ?? null })) };
"@
  }
}
finally {
  if ($restartSocket) { $restartSocket.Dispose() }
  if ($restart -and -not $restart.HasExited) {
    $restart.CloseMainWindow() | Out-Null
    if (-not $restart.WaitForExit(5000)) { $restart.Kill() }
  }
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArguments
}

Complete-AcceptanceRun -Outcome "completed"