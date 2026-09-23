param(
  [string]$Executable = "src-tauri/target/debug/supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/storage-root"
)
$ErrorActionPreference = "Stop"
# Non-interactive built-app check only. Never opens the picker, restores a window,
# sends desktop input or captures the screen. This does NOT verify picker UI.
. (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run storage boundary smoke from a standard-user session."
}
$exe = (Resolve-Path -LiteralPath $Executable).Path
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
$artifacts = (Resolve-Path -LiteralPath $ArtifactDirectory).Path
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"
$process = $null
$socket = $null
try {
  $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Minimized -PassThru
  $socket = Connect-WebViewDebugSocket -Port $port
  Wait-WebViewExpression -Socket $socket -Expression "!!document.querySelector('main h1') && typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "Native app did not finish mounting"
  $result = Invoke-WebViewExpression -Socket $socket -Expression @"
(async () => {
  const invoke = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body)));
  const scopes = await invoke('list_storage_scopes', {module:'cleaner'});
  if (!scopes.length || !scopes.every(s => /^[a-f0-9]{32}$/.test(s.scopeId) && s.module === 'cleaner')) throw Error('invalid native scopes');
  let rejected = false;
  try { await invoke('authorize_storage_scope', {module:'browser',scopeId:scopes[0].scopeId}); }
  catch (e) { rejected = e.code === 'snapshot_unavailable'; }
  if (!rejected) throw Error('cross-module scope accepted');
  const refreshed = await invoke('list_storage_scopes', {module:'cleaner'});
  try { await invoke('authorize_storage_scope', {module:'cleaner',scopeId:scopes[0].scopeId}); throw Error('stale scope accepted'); }
  catch (e) { if (e.code !== 'snapshot_unavailable') throw e; }
  const routes = [['drives','Fixed drives'],['disk-analyzer','Disk analyzer'],['large-files','Large files'],['cleaner','Rule cleaner'],['duplicates','Duplicate files'],['empty-folders','Empty folders'],['browser','Browser caches'],['uninstaller','Installed programs']];
  for (const [path,title] of routes) {
    location.hash='/'+path;
    const deadline=Date.now()+5000;
    while(document.querySelector('main h1')?.textContent!==title) {
      if(Date.now()>deadline)throw Error('Native route did not mount: '+path);
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    // Sort/opt-in controls are not file selections; candidate checkboxes live in result lists.
    if(document.querySelectorAll('main li input[type=checkbox]:checked').length)throw Error('Unexpected default file selection: '+path);
  }
  return {scopeCount:refreshed.length, crossModuleRejected:true, staleScopeRejected:true, mountedRoutes:routes.map(([path])=>path)};
})().catch(error => ({failure:String(error?.message || error?.code || error)}))
"@
  if ($result.failure) { throw "Native storage smoke failed: $($result.failure)" }
  if (-not $result.crossModuleRejected -or -not $result.staleScopeRejected) { throw "Native scope checks did not pass." }
  $keyboardRoutes = @()
  $reflowRoutes = @()
  foreach ($route in $result.mountedRoutes) {
    Invoke-WebViewExpression -Socket $socket -Expression "location.hash = '#/$route'; true" | Out-Null
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('nav a[aria-current=page]')?.getAttribute('href') === '#/$route'" -Failure "Native route navigation failed: $route"
    Invoke-WebViewExpression -Socket $socket -Expression "document.querySelector('.skip-link').focus(); true" | Out-Null
    foreach ($eventType in @('rawKeyDown', 'keyUp')) {
      Invoke-WebViewProtocol -Socket $socket -Method 'Input.dispatchKeyEvent' -Parameters @{
        type = $eventType; key = 'Enter'; code = 'Enter'; windowsVirtualKeyCode = 13
      } | Out-Null
    }
    Wait-WebViewExpression -Socket $socket -Expression "location.hash === '#/$route' && document.activeElement?.id === 'main-content'" -Failure "Native WebView skip-link keyboard flow failed: $route"
    $keyboardRoutes += $route
    foreach ($width in @(320, 640, 1280)) {
      Set-WebViewViewport -Socket $socket -Width $width -Height 900
      Wait-WebViewExpression -Socket $socket -Expression "document.documentElement.scrollWidth <= document.documentElement.clientWidth" -Failure "Native route overflow at ${width}px: $route"
    }
    Invoke-WebViewProtocol -Socket $socket -Method 'Emulation.setEmulatedMedia' -Parameters @{
      features = @(@{ name = 'forced-colors'; value = 'active' })
    } | Out-Null
    Wait-WebViewExpression -Socket $socket -Expression "matchMedia('(forced-colors: active)').matches && document.documentElement.scrollWidth <= document.documentElement.clientWidth" -Failure "Native forced-colors reflow failed: $route"
    Invoke-WebViewProtocol -Socket $socket -Method 'Emulation.setEmulatedMedia' -Parameters @{ features = @() } | Out-Null
    $reflowRoutes += $route
  }
  $lifecycleSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'fixtures/native-storage-lifecycle.js')
  $cancellationSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'fixtures/native-storage-cancellation.js')
  $lifecycleRuns = @()
  $resourceSamples = @()
  for ($round = 0; $round -le 2; $round++) {
    $process.Refresh()
    $resourceSamples += [ordered]@{
      completedRounds = $round
      privateBytes = $process.PrivateMemorySize64
      workingSetBytes = $process.WorkingSet64
      handles = $process.HandleCount
      cpuMilliseconds = $process.TotalProcessorTime.TotalMilliseconds
    }
    if ($round -eq 2) { break }
    $lifecycle = Invoke-WebViewExpression -Socket $socket -Expression "($lifecycleSource)($cancellationSource).catch(error => ({failure:String(error?.message || error?.code || error)}))"
    if ($lifecycle.failure) { throw "Native lifecycle smoke failed: $($lifecycle.failure)" }
    if ($lifecycle.cycles -ne 4 -or $lifecycle.uiCycles -ne 4 -or $lifecycle.releasedSnapshots -ne 5 -or $lifecycle.cancellationAttempts -ne 1) {
      throw 'Native read-only scan lifecycle evidence incomplete.'
    }
    if ($lifecycle.cancelOutcome -cnotin @('cancelled', 'complete') -or
        $lifecycle.cancellationAcknowledgements -notin @(0, 1) -or
        ($lifecycle.cancelOutcome -ceq 'cancelled' -and $lifecycle.cancellationAcknowledgements -ne 1)) {
      throw 'Native cancellation outcome or acknowledgement evidence invalid.'
    }
    $lifecycleRuns += $lifecycle
  }
  [ordered]@{
    checkedAt = [DateTime]::UtcNow.ToString("o")
    executableSha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    startup = "Minimized; no window restore, dialog, desktop input or screenshot"
    scopeCount = $result.scopeCount
    crossModuleRejected = $result.crossModuleRejected
    staleScopeRejected = $result.staleScopeRejected
    mountedRoutes = $result.mountedRoutes
    keyboardSkipLinkRoutes = $keyboardRoutes
    initialStateReflowRoutes = $reflowRoutes
    emulatedViewportWidths = @(320, 640, 1280)
    forcedColorsInitialReflow = $true
    readOnlyInventoryLifecycle = $lifecycleRuns
    nativeProcessResourceSamples = $resourceSamples
    resourceScope = 'Native process only; three observations, not a leak-free claim or WebView process-tree measurement'
    notExercised = @("native picker selection and cancellation", "full native keyboard flow or Narrator", "filesystem scans, selection and mutation from native routes", "cleanup execution or undo", "MSI packaging", "ARM64")
  } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $artifacts "result.json") -Encoding UTF8
  Write-Output "PASS: minimized built app mounts all eight routes, exercises keyboard skip links and native read-only inventory scan/page/release cycles with cancellation attempts, acknowledgements and validated terminal outcomes, and rejects cross-module/stale IDs. No cleanup or dialog was invoked."
} finally {
  if ($null -ne $socket) { $socket.Dispose() }
  if ($null -ne $process) {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id }
    if (-not $process.WaitForExit(5000)) { throw 'Smoke-owned native process did not exit.' }
    $process.Dispose()
  }
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArguments
}
