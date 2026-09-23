# Bounded built-app acceptance for system-management changes.
#   -Launch            start the built app with a loopback-only WebView debug port
#   -StepFile <js>     run one async step body inside the app and append the result
# A person answers every native confirmation and UAC prompt; this script never
# sends synthetic desktop input and never captures the screen.
param(
  [string]$Executable = "src-tauri/target/x86_64-pc-windows-msvc/debug/supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/system-acceptance",
  [int]$Port = 9347,
  [switch]$Launch,
  [string]$StepFile,
  [string]$StepName = "step"
)
. (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if ([Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run system acceptance from a standard-user session so UAC is exercised."
}
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null

if ($Launch) {
  $exe = (Resolve-Path -LiteralPath $Executable).Path
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$Port"
  $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -PassThru
  $socket = Connect-WebViewDebugSocket -Port $Port
  Wait-WebViewExpression -Socket $socket -Expression "typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "App did not mount"
  [ordered]@{
    processId = $process.Id
    executable = $exe
    executableSha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    launchedUtc = (Get-Date).ToUniversalTime().ToString("o")
  } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $ArtifactDirectory "launch.json") -Encoding UTF8
  Write-Host "launched pid $($process.Id)"
  return
}

$body = Get-Content -LiteralPath $StepFile -Raw
$expression = @"
(async () => {
  const raw = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body)));
  const plain = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args ?? {});
  const sc = {
    preview: (change) => raw('preview_system_change', { change }),
    plan: (changes) => raw('create_system_change_plan', { changes }),
    confirm: (planId) => raw('confirm_system_change_plan', { planId }),
    execute: (planId) => raw('execute_system_change_plan', { planId }),
    journal: () => raw('system_change_journal', {}),
    rollbackPlan: (entryIds) => raw('create_system_rollback_plan', { entryIds }),
  };
  const apply = async (changes) => {
    const ticket = await sc.plan(changes);
    await sc.confirm(ticket.planId);
    const report = await sc.execute(ticket.planId);
    return { ticket, report };
  };
  const undo = async (report) => {
    const ids = report.results.map(r => r.journalEntryId).filter(Boolean);
    return apply_ticket(await sc.rollbackPlan(ids));
  };
  const apply_ticket = async (ticket) => {
    await sc.confirm(ticket.planId);
    return { ticket, report: await sc.execute(ticket.planId) };
  };
  $body
})().then(value => ({ ok: true, value }), error => ({ ok: false, code: error?.code ?? null, message: String(error?.message ?? error?.code ?? error) }))
"@
$socket = Connect-WebViewDebugSocket -Port $Port
$started = (Get-Date).ToUniversalTime().ToString("o")
$result = Invoke-WebViewExpression -Socket $socket -Expression $expression
$entry = [ordered]@{ step = $StepName; startedUtc = $started; finishedUtc = (Get-Date).ToUniversalTime().ToString("o"); result = $result }
$json = $entry | ConvertTo-Json -Depth 30
Add-Content -LiteralPath (Join-Path $ArtifactDirectory "steps.jsonl") -Value ($entry | ConvertTo-Json -Depth 30 -Compress) -Encoding UTF8
Write-Output $json
