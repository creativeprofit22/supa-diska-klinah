$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Net.WebSockets;
using System.Text;
using System.Threading;

public static class NativeSmokeWebSocket
{
    public static void Send(ClientWebSocket socket, string message)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(message);
        socket.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, CancellationToken.None)
            .GetAwaiter().GetResult();
    }

    public static string Receive(ClientWebSocket socket)
    {
        using (MemoryStream message = new MemoryStream())
        {
            byte[] buffer = new byte[16384];
            WebSocketReceiveResult result;
            do
            {
                result = socket.ReceiveAsync(new ArraySegment<byte>(buffer), CancellationToken.None)
                    .GetAwaiter().GetResult();
                if (result.MessageType == WebSocketMessageType.Close)
                    throw new IOException("WebView debug connection closed unexpectedly.");
                message.Write(buffer, 0, result.Count);
            }
            while (!result.EndOfMessage);
            return Encoding.UTF8.GetString(message.ToArray());
        }
    }
}
"@

function Get-FixtureSnapshot {
  param([Parameter(Mandatory = $true)][string]$Root)

  $entries = @(
    Get-ChildItem -LiteralPath $Root -Recurse -File |
      Sort-Object FullName |
      ForEach-Object {
        [ordered]@{
          path = $_.FullName.Substring($Root.Length).TrimStart([IO.Path]::DirectorySeparatorChar)
          bytes = $_.Length
          sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
      }
  )
  $lines = @($entries | ForEach-Object { "$($_.path)|$($_.bytes)|$($_.sha256)" })
  $bytes = [Text.Encoding]::UTF8.GetBytes(($lines -join "`n"))
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    $aggregate = -join ($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString("x2") })
  }
  finally {
    $sha.Dispose()
  }
  return [ordered]@{ sha256 = $aggregate; files = $entries }
}

function New-ProjectArtifactSmokeContext {
  param(
    [Parameter(Mandatory = $true)][string]$ArtifactDirectory,
    [Parameter(Mandatory = $true)][string]$BuildRevision,
    [Parameter(Mandatory = $true)][string]$Target
  )

  New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
  $repositoryRoot = Split-Path $PSScriptRoot -Parent
  $smokeRoot = Join-Path $repositoryRoot ".gg\smoke-temp"
  New-Item -ItemType Directory -Path $smokeRoot -Force | Out-Null
  $fixtureRoot = Join-Path $smokeRoot "project-discovery-$PID-$([Guid]::NewGuid().ToString('N'))"
  $project = Join-Path $fixtureRoot "projects"
  $empty = Join-Path $fixtureRoot "unmarked-sibling"
  $nested = Join-Path $project "nested-node"
  New-Item -ItemType Directory -Path (Join-Path $project "node_modules"), (Join-Path $nested "node_modules"), (Join-Path $project "rust\target"), (Join-Path $project "python\.venv"), (Join-Path $project "cmake\cmake-build-debug"), (Join-Path $project "unity\Assets"), (Join-Path $project "unity\ProjectSettings"), (Join-Path $project "unity\Library"), $empty | Out-Null
  foreach ($name in @("target", "build", "out", "Library", "outputs")) {
    New-Item -ItemType Directory -Path (Join-Path $empty $name) | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $empty "$name\ignored.bin"), [byte[]]::new(17))
  }
  [IO.File]::WriteAllText((Join-Path $project "package.json"), "{}", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $nested "package.json"), "{}", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $project "rust\Cargo.toml"), "[package]`nname='smoke'`nversion='0.1.0'", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $project "python\pyproject.toml"), "[project]`nname='smoke'", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $project "cmake\CMakeLists.txt"), "project(smoke)", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllBytes((Join-Path $project "node_modules\known-size.bin"), [byte[]]::new(4096))
  [IO.File]::WriteAllBytes((Join-Path $nested "node_modules\nested.bin"), [byte[]]::new(1024))

  $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
  $listener.Stop()

  $previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$port"

  return [pscustomobject]@{
    ArtifactDirectory = $ArtifactDirectory
    Before = Get-FixtureSnapshot -Root $fixtureRoot
    BuildRevision = $BuildRevision
    Empty = $empty
    FixtureRoot = $fixtureRoot
    Nested = $nested
    Port = $port
    PreviousBrowserArguments = $previousArguments
    Project = $project
    Target = $Target
  }
}

function Close-ProjectArtifactSmokeContext {
  param([Parameter(Mandatory = $true)]$Context)

  if ($null -eq $Context.PreviousBrowserArguments) {
    Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  }
  else {
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Context.PreviousBrowserArguments
  }
  Remove-Item -LiteralPath $Context.FixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

function Connect-WebViewDebugSocket {
  param([Parameter(Mandatory = $true)][int]$Port)

  $targets = $null
  for ($attempt = 0; $attempt -lt 100; $attempt++) {
    try {
      $targets = @(Invoke-RestMethod -Uri "http://127.0.0.1:$Port/json/list" -TimeoutSec 1)
      if ($targets.Count -gt 0) { break }
    }
    catch [Net.WebException] {}
    Start-Sleep -Milliseconds 100
  }
  $target = $targets | Where-Object { $_.type -eq "page" } | Select-Object -First 1
  if (-not $target.webSocketDebuggerUrl) {
    throw "Packaged WebView did not expose a page for native smoke automation."
  }
  $debugUri = [Uri]$target.webSocketDebuggerUrl
  if ($debugUri.Scheme -notin @("ws", "wss") -or $debugUri.Host -notin @("127.0.0.1", "localhost", "::1") -or $debugUri.Port -ne $Port) {
    throw "Packaged WebView exposed an unexpected debug endpoint."
  }

  $socket = [Net.WebSockets.ClientWebSocket]::new()
  [void]$socket.ConnectAsync($debugUri, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
  return $socket
}

$script:NativeSmokeMessageId = 0
function Invoke-WebViewProtocol {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Method,
    [hashtable]$Parameters = @{}
  )

  $id = ++$script:NativeSmokeMessageId
  $request = @{ id = $id; method = $Method; params = $Parameters } | ConvertTo-Json -Compress -Depth 20
  [NativeSmokeWebSocket]::Send($Socket, $request)
  do {
    $response = [NativeSmokeWebSocket]::Receive($Socket) | ConvertFrom-Json
  } while ($response.id -ne $id)
  if ($response.error) {
    throw "WebView automation command failed: $($response.error.message)"
  }
  return $response.result
}

function Invoke-WebViewExpression {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Expression
  )

  $result = Invoke-WebViewProtocol -Socket $Socket -Method "Runtime.evaluate" -Parameters @{
    expression = $Expression
    returnByValue = $true
    awaitPromise = $true
  }
  if ($result.exceptionDetails) {
    throw "Rendered UI automation expression failed."
  }
  return $result.result.value
}

function Set-WebViewViewport {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][int]$Width,
    [Parameter(Mandatory = $true)][int]$Height
  )

  Invoke-WebViewProtocol -Socket $Socket -Method "Emulation.setDeviceMetricsOverride" -Parameters @{
    width = $Width
    height = $Height
    deviceScaleFactor = 1
    mobile = $false
  } | Out-Null
}

function Submit-ProjectRoot {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Root
  )

  $rootJson = $Root | ConvertTo-Json -Compress
  $setValue = @"
(() => {
  const input = document.querySelector('#project-root');
  if (!input) return false;
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, $rootJson);
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return true;
})()
"@
  if (-not (Invoke-WebViewExpression -Socket $Socket -Expression $setValue)) {
    throw "Rendered project-root input was unavailable."
  }
  Start-Sleep -Milliseconds 100
  if (-not (Invoke-WebViewExpression -Socket $Socket -Expression "document.querySelector('.project-artifact-form')?.requestSubmit(); true")) {
    throw "Rendered project-root form was unavailable."
  }
}

function Invoke-WebViewButton {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Text
  )

  $textJson = $Text | ConvertTo-Json -Compress
  $clicked = Invoke-WebViewExpression -Socket $Socket -Expression "(() => { const button = [...document.querySelectorAll('button')].find(item => item.textContent.trim() === $textJson || item.getAttribute('aria-label')?.endsWith($textJson)); if (!button || button.disabled) return false; button.click(); return true; })()"
  if (-not $clicked) { throw "Rendered button was unavailable: $Text" }
}

function Invoke-SavedRootScan {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Root
  )

  $rootJson = $Root | ConvertTo-Json -Compress
  $clicked = Invoke-WebViewExpression -Socket $Socket -Expression "(() => { const wanted = $rootJson.toLocaleLowerCase(); const item = [...document.querySelectorAll('.project-root-list > li')].find(entry => entry.innerText.toLocaleLowerCase().includes(wanted)); const button = [...(item?.querySelectorAll('button') ?? [])].find(control => control.textContent.trim() === 'Scan'); if (!button || button.disabled) return false; button.click(); return true; })()"
  if (-not $clicked) { throw "Saved root scan button was unavailable." }
}

function Wait-WebViewExpression {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Expression,
    [Parameter(Mandatory = $true)][string]$Failure
  )

  for ($attempt = 0; $attempt -lt 150; $attempt++) {
    if (Invoke-WebViewExpression -Socket $Socket -Expression $Expression) { return }
    Start-Sleep -Milliseconds 100
  }
  throw $Failure
}

function Save-WebViewScreenshot {
  param(
    [Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket,
    [Parameter(Mandatory = $true)][string]$Path
  )

  $metrics = Invoke-WebViewProtocol -Socket $Socket -Method "Page.getLayoutMetrics"
  $size = $metrics.cssContentSize
  $capture = Invoke-WebViewProtocol -Socket $Socket -Method "Page.captureScreenshot" -Parameters @{
    format = "png"
    fromSurface = $true
    captureBeyondViewport = $true
    clip = @{ x = 0; y = 0; width = [double]$size.width; height = [double]$size.height; scale = 1 }
  }
  [IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String($capture.data))
}

function Get-ProjectDiscoveryView {
  param([Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket)

  $json = Invoke-WebViewExpression -Socket $Socket -Expression @"
JSON.stringify((() => {
  const section = document.querySelector('.project-artifacts');
  const records = section.querySelector('.project-artifact-groups');
  const controls = [...(records?.querySelectorAll('button, input, select') ?? [])];
  return {
    status: section.querySelector('.project-artifact-status')?.innerText ?? '',
    error: section.querySelector('.project-artifact-error')?.innerText ?? '',
    roots: section.querySelector('.project-root-manager')?.innerText ?? '',
    text: section.innerText,
    recordText: records?.innerText ?? '',
    records: section.querySelectorAll('.project-artifact-records > li').length,
    destructiveControls: controls.filter(control =>
      control.type === 'checkbox' || /select|delete|remove|clean/i.test(control.innerText || control.value || '')
    ).length
  };
})())
"@
  return $json | ConvertFrom-Json
}

function Install-BuildArtifactSmokeAdapter {
  param([Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket)

  $installed = Invoke-WebViewExpression -Socket $Socket -Expression @'
(() => {
  window.__supaSmokeRunState = null;
  const profile = {
    profileId: '11111111111111111111111111111111', rootId: '22222222222222222222222222222222',
    displayName: 'Smoke debug build', ecosystem: 'rust', executable: 'C:\\Tools\\cargo.exe',
    executableIdentity: { volume: 1, file: 2 }, argv: ['build'], workingDirectory: 'C:\\Smoke',
    profileLabel: 'debug', toolchainLabel: 'stable', targetLabel: 'x86_64-pc-windows-msvc',
    rebuildCost: 'low', artifactPaths: [{ relativePath: 'target/debug', role: 'generation' }]
  };
  const policy = {
    schemaVersion: 1, enabled: false, globalLimits: { maximumAllocatedBytes: 1, maximumAgeSeconds: null },
    scheduledAnalysisIntervalSeconds: 86400, staleChangeGraceSeconds: 86400,
    quarantineGraceSeconds: 604800, projectOverrides: []
  };
  const preview = {
    enabled: false,
    decision: {
      selectedGenerationIds: ['selected'],
      protected: [{ generationId: 'protected', rootId: profile.rootId, allocatedBytes: 8192, reasons: ['incremental'] }],
      currentAllocatedBytes: 12288, projectedAllocatedBytes: 8192, quarantineBytes: 4096,
      unsatisfiedProtectedByteFloor: 8192, projectCurrentBytes: {},
      projectProjectedBytes: {}, projectProtectedByteFloors: {}
    },
    generations: [
      { generationId: 'selected', normalizedPath: 'target/release', allocatedBytes: 4096 },
      { generationId: 'protected', normalizedPath: 'target/debug/incremental', allocatedBytes: 8192 }
    ]
  };
  window.__SUPA_ARTIFACT_SMOKE__ = {
    listBuildProfiles: () => [profile],
    getArtifactBudgetPolicy: () => policy,
    previewArtifactBudgets: () => preview,
    startBuildRun: () => {
      window.__supaSmokeRunState = 'running';
      return { runId: '33333333333333333333333333333333', profileId: profile.profileId, state: 'running' };
    },
    getActiveBuildRun: () => window.__supaSmokeRunState === 'running' ? ({
      runId: '33333333333333333333333333333333', profileId: profile.profileId, state: 'running'
    }) : null,
    getBuildRun: () => ({
      runId: '33333333333333333333333333333333', profileId: profile.profileId, state: window.__supaSmokeRunState
    }),
    cancelBuildRun: () => {
      window.__supaSmokeRunState = 'cancelled';
      return { runId: '33333333333333333333333333333333', profileId: profile.profileId, state: 'cancelled' };
    }
  };
  return true;
})()
'@
  if (-not $installed) { throw "Packaged WebView artifact smoke adapter could not be installed." }
}

function Restore-BuildArtifactSmokeAdapter {
  param([Parameter(Mandatory = $true)][Net.WebSockets.ClientWebSocket]$Socket)
  Invoke-WebViewExpression -Socket $Socket -Expression "(() => { delete window.__SUPA_ARTIFACT_SMOKE__; return true; })()" | Out-Null
}

function Invoke-ProjectArtifactDiscoverySmoke {
  param(
    [Parameter(Mandatory = $true)]$Context,
    [Parameter(Mandatory = $true)][Diagnostics.Process]$Process
  )

  $socket = Connect-WebViewDebugSocket -Port $Context.Port
  try {
    Invoke-WebViewProtocol -Socket $socket -Method "Page.enable" | Out-Null
    $openedCleanup = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find((item) => item.textContent.trim() === 'Cleanup'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedCleanup) { throw "Rendered Cleanup navigation was unavailable." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('#project-root') !== null" -Failure "Rendered project discovery form did not load."
    $hasNonSmokeRoot = Invoke-WebViewExpression -Socket $socket -Expression '(() => [...document.querySelectorAll(".project-root-list > li")].some(item => !item.innerText.includes(".gg\\smoke-temp\\project-discovery-")))()'
    if ($hasNonSmokeRoot) {
      throw "Native smoke refused to change saved project roots outside its fixture directory."
    }
    while ($true) {
      $savedCount = Invoke-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length"
      if ($savedCount -eq 0) { break }
      $removed = Invoke-WebViewExpression -Socket $socket -Expression '(() => { const button = [...document.querySelectorAll(".project-root-list button")].find(item => item.getAttribute("aria-label")?.startsWith("Remove ")); if (!button || button.disabled) return false; button.click(); return true; })()'
      if (-not $removed) { throw "Existing smoke project root could not be forgotten before smoke." }
      Wait-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length === $($savedCount - 1)" -Failure "Existing smoke project root was not forgotten before smoke."
    }

    Submit-ProjectRoot -Socket $socket -Root $Context.Project
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-root-manager')?.innerText.includes('$($Context.Project.Replace('\', '\\'))') === true" -Failure "Native project root was not saved."
    Submit-ProjectRoot -Socket $socket -Root $Context.Nested
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length === 2" -Failure "Nested project root was not saved."
    Invoke-WebViewButton -Socket $socket -Text "Scan active roots"
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('6 rebuildable artifacts found.') === true" -Failure "Native all-root discovery did not render its success state."
    $success = Get-ProjectDiscoveryView -Socket $socket
    $requiredSuccessText = @(
      (Split-Path $Context.Project -Leaf), $Context.Project, "Node.js", "Rust", "Python", "CMake", "Unity",
      "Installed dependencies", "Build output", "Virtual environment", "Imported asset cache", "Not selected"
    )
    foreach ($text in $requiredSuccessText) {
      if (-not $success.recordText.Contains($text)) {
        throw "Native success state omitted expected project intelligence: $text"
      }
    }
    if ($success.records -ne 6 -or $success.destructiveControls -ne 0) {
      throw "Native success state must show six read-only artifact records."
    }
    if (-not $success.roots.Contains("Last scan:") -or $success.roots -notmatch "Never") {
      throw "Collapsed roots did not preserve distinct scan metadata."
    }
    Set-WebViewViewport -Socket $socket -Width 1280 -Height 800
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "success-1280x800.png")
    Set-WebViewViewport -Socket $socket -Width 320 -Height 800
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "success-320x800.png")
    Invoke-WebViewProtocol -Socket $socket -Method "Emulation.clearDeviceMetricsOverride" | Out-Null

    Invoke-SavedRootScan -Socket $socket -Root $Context.Nested
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('1 rebuildable artifact found.') === true" -Failure "Exact nested-root rescan did not render one result."
    $nested = Get-ProjectDiscoveryView -Socket $socket
    if ($nested.records -ne 1 -or $nested.roots -match "Last scan:\s*Never") {
      throw "Exact nested-root rescan did not persist its scan timestamp."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "nested.png")

    $openedSettings = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Settings'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedSettings) { throw "Rendered Settings navigation was unavailable." }
    $openedCleanup = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Cleanup'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedCleanup) { throw "Rendered Cleanup navigation was unavailable after registry reload." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length === 2" -Failure "Saved roots did not persist after route reload."

    Submit-ProjectRoot -Socket $socket -Root $Context.Empty
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length === 3" -Failure "Collision fixture root was not saved."
    Invoke-SavedRootScan -Socket $socket -Root $Context.Empty
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('No marker-backed project artifacts were found.') === true" -Failure "Native marker-collision state did not render empty."
    $empty = Get-ProjectDiscoveryView -Socket $socket
    if ($empty.records -ne 0) {
      throw "Generic artifact names without project markers must not be discovered."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "empty.png")

    $missing = Join-Path $Context.FixtureRoot "missing-project"
    Submit-ProjectRoot -Socket $socket -Root $missing
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-error')?.innerText.includes('Enter an existing, unprotected absolute project path.') === true" -Failure "Native project discovery did not render its fixed error state."
    $errorView = Get-ProjectDiscoveryView -Socket $socket
    if ($errorView.error -match "(?i)access is denied|os error|the system cannot find|[a-z]:\\") {
      throw "Native error state exposed raw operating-system details."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "error.png")

    Install-BuildArtifactSmokeAdapter -Socket $socket
    $openedSettings = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Settings'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedSettings) { throw "Rendered Settings navigation was unavailable for artifact smoke." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('main h1')?.textContent.trim() === 'Settings'" -Failure "Settings did not open before artifact smoke remount."
    $openedCleanup = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Cleanup'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedCleanup) { throw "Rendered Cleanup navigation was unavailable for artifact smoke." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('main h1')?.textContent.trim() === 'Cleanup'" -Failure "Cleanup did not reopen for artifact smoke."
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.saved-build-profiles')?.innerText.includes('Smoke debug build') === true" -Failure "Synthetic artifact profile did not render."
    $artifactProbe = Invoke-WebViewExpression -Socket $socket -Expression "document.querySelector('.build-artifacts')?.innerText ?? document.querySelector('main')?.innerText ?? ''"
    if (-not $artifactProbe.Contains("Smoke debug build")) {
      throw "Synthetic artifact profile did not render. Visible artifact state: $artifactProbe"
    }
    $artifactView = Invoke-WebViewExpression -Socket $socket -Expression "document.querySelector('.build-artifacts')?.innerText ?? ''"
    foreach ($text in @("Automatic budgets disabled", "target/release", "target/debug/incremental", "limit remains unmet")) {
      if (-not $artifactView.Contains($text)) { throw "Artifact smoke omitted state: $text" }
    }
    Set-WebViewViewport -Socket $socket -Width 1280 -Height 800
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "artifact-disabled-preview.png")
    Set-WebViewViewport -Socket $socket -Width 320 -Height 800
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "artifact-disabled-preview-320.png")
    Set-WebViewViewport -Socket $socket -Width 1280 -Height 800
    $runClicked = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const button = document.querySelector('.saved-build-profiles button'); if (!button || button.disabled || button.textContent.trim() !== 'Run') return false; button.click(); return true; })()"
    if (-not $runClicked) { throw "Synthetic artifact run button was unavailable." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.build-run-status')?.innerText.includes('running') === true" -Failure "Synthetic running build state did not render."
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "artifact-running.png")
    $openedSettings = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Settings'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedSettings) { throw "Settings navigation was unavailable during the active build." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('main h1')?.textContent.trim() === 'Settings'" -Failure "Settings did not open during the active build."
    $openedCleanup = Invoke-WebViewExpression -Socket $socket -Expression "(() => { const link = [...document.querySelectorAll('a')].find(item => item.textContent.trim() === 'Cleanup'); if (!link) return false; link.click(); return true; })()"
    if (-not $openedCleanup) { throw "Cleanup navigation was unavailable during the active build." }
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.build-run-status')?.innerText.includes('running') === true && [...document.querySelectorAll('.saved-build-profiles button')].some(button => button.textContent.trim() === 'Cancel' && !button.disabled)" -Failure "Active build status and cancellation were not recovered after navigation."
    Wait-WebViewExpression -Socket $socket -Expression "[...document.querySelectorAll('.saved-build-profiles button')].filter(button => ['Run', 'Forget'].includes(button.textContent.trim())).every(button => button.disabled)" -Failure "Run and Forget remained enabled during the recovered active build."
    Invoke-WebViewButton -Socket $socket -Text "Cancel"
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.build-run-status')?.innerText.includes('cancelled') === true" -Failure "Synthetic cancelled build state did not render."
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "artifact-cancelled.png")
    Restore-BuildArtifactSmokeAdapter -Socket $socket

    $after = Get-FixtureSnapshot -Root $Context.FixtureRoot
    if ($Context.Before.sha256 -ne $after.sha256) {
      throw "Native project discovery changed fixture files."
    }
    while ($true) {
      $savedCount = Invoke-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length"
      if ($savedCount -eq 0) { break }
      $removed = Invoke-WebViewExpression -Socket $socket -Expression '(() => { const button = [...document.querySelectorAll(".project-root-list button")].find(item => item.getAttribute("aria-label")?.startsWith("Remove ")); if (!button || button.disabled) return false; button.click(); return true; })()'
      if (-not $removed) { throw "Smoke project root could not be forgotten during cleanup." }
      Wait-WebViewExpression -Socket $socket -Expression "document.querySelectorAll('.project-root-list > li').length === $($savedCount - 1)" -Failure "Smoke project root was not forgotten during cleanup."
    }
    $report = [ordered]@{
      buildRevision = $Context.BuildRevision
      target = $Context.Target
      processArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
      standardUser = -not [ProcessTokenProbe]::IsElevated($Process.Handle)
      user = [Environment]::UserName
      localApplicationData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
      fixtureRoot = $Context.FixtureRoot
      fixtureBefore = $Context.Before
      fixtureAfter = $after
      destructiveControls = $success.destructiveControls
      states = [ordered]@{ success = $success.status; nested = $nested.status; empty = $empty.status; error = $errorView.error; artifacts = $artifactView }
      screenshots = @(
        "success-1280x800.png", "success-320x800.png", "nested.png", "empty.png", "error.png",
        "artifact-disabled-preview.png", "artifact-disabled-preview-320.png",
        "artifact-running.png", "artifact-cancelled.png"
      )
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Context.ArtifactDirectory "report.json") -Encoding UTF8
  }
  finally {
    if ($socket) {
      $socket.Dispose()
    }
  }
}
