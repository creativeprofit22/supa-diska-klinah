# Bounded built-app acceptance for local-first protection (ADR 0003).
#   -Launch            start the built app with a loopback-only WebView debug port
#   -StepFile <js>     run one async step body inside the app and append the result
# A person answers every native confirmation and folder picker; this script never
# sends synthetic desktop input and never captures the screen.
param(
  [string]$Executable = "src-tauri/target/x86_64-pc-windows-msvc/debug/supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/protection-acceptance",
  [int]$Port = 9348,
  [switch]$Launch,
  [string]$StepFile,
  [string]$StepName = "step"
)
. (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")

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
  const raw = (command, body) => window.__TAURI_INTERNALS__.invoke(command, new TextEncoder().encode(JSON.stringify(body ?? {})));
  const text = () => document.querySelector('main')?.innerText ?? document.body.innerText;
  // The app uses a hash router.
  const go = async (path) => {
    window.location.hash = '#' + path;
    await new Promise(r => setTimeout(r, 1500));
    return text();
  };
  $body
})().then(value => ({ ok: true, value }), error => ({ ok: false, code: error?.code ?? null, message: String(error?.message ?? error?.code ?? error) }))
"@
$socket = Connect-WebViewDebugSocket -Port $Port
$started = (Get-Date).ToUniversalTime().ToString("o")
$result = Invoke-WebViewExpression -Socket $socket -Expression $expression
$entry = [ordered]@{ step = $StepName; startedUtc = $started; finishedUtc = (Get-Date).ToUniversalTime().ToString("o"); result = $result }
Add-Content -LiteralPath (Join-Path $ArtifactDirectory "steps.jsonl") -Value ($entry | ConvertTo-Json -Depth 30 -Compress) -Encoding UTF8
Write-Output ($entry | ConvertTo-Json -Depth 30)
