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
  $fixtureBase = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
  $fixtureRoot = Join-Path $fixtureBase "supa-project-discovery-$PID-$([Guid]::NewGuid().ToString('N'))"
  $project = Join-Path $fixtureRoot "native-smoke-project"
  $empty = Join-Path $fixtureRoot "unmarked-sibling"
  New-Item -ItemType Directory -Path (Join-Path $project "node_modules"), (Join-Path $empty "node_modules") | Out-Null
  [IO.File]::WriteAllText((Join-Path $project "package.json"), "{}", [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllBytes((Join-Path $project "node_modules\known-size.bin"), [byte[]]::new(4096))
  [IO.File]::WriteAllBytes((Join-Path $empty "node_modules\ignored.bin"), [byte[]]::new(17))

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
  const controls = [...section.querySelectorAll('button, input, select')];
  return {
    status: section.querySelector('.project-artifact-status')?.innerText ?? '',
    text: section.innerText,
    recordText: section.querySelector('.project-artifact-records')?.innerText ?? '',
    records: section.querySelectorAll('.project-artifact-records > li').length,
    destructiveControls: controls.filter(control =>
      control.type === 'checkbox' || /select|delete|remove|clean/i.test(control.innerText || control.value || '')
    ).length
  };
})())
"@
  return $json | ConvertFrom-Json
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

    Submit-ProjectRoot -Socket $socket -Root $Context.Project
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('1 rebuildable artifact found.') === true" -Failure "Native project discovery did not render its success state."
    $success = Get-ProjectDiscoveryView -Socket $socket
    $requiredSuccessText = @(
      (Split-Path $Context.Project -Leaf), $Context.Project, "Node.js", "Installed dependencies", "4 KB",
      "Recoverable", "Rebuildable", "Network download required"
    )
    foreach ($text in $requiredSuccessText) {
      if (-not $success.recordText.Contains($text)) {
        throw "Native success state omitted expected project intelligence: $text"
      }
    }
    if ($success.records -ne 1 -or $success.destructiveControls -ne 0) {
      throw "Native success state must show one read-only artifact record."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "success.png")

    Submit-ProjectRoot -Socket $socket -Root $Context.Empty
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('No marker-backed Node.js dependency folders were found.') === true" -Failure "Native project discovery did not render its empty state."
    $empty = Get-ProjectDiscoveryView -Socket $socket
    if ($empty.records -ne 0) {
      throw "A sibling node_modules folder without package.json must not be discovered."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "empty.png")

    $missing = Join-Path $Context.FixtureRoot "missing-project"
    Submit-ProjectRoot -Socket $socket -Root $missing
    Wait-WebViewExpression -Socket $socket -Expression "document.querySelector('.project-artifact-status')?.innerText.includes('Project artifacts could not be scanned. Check the root and try again.') === true" -Failure "Native project discovery did not render its fixed error state."
    $errorView = Get-ProjectDiscoveryView -Socket $socket
    foreach ($text in @("Project artifacts could not be scanned. Check the root and try again.", "Try project scan again")) {
      if (-not $errorView.status.Contains($text)) {
        throw "Native error state omitted fixed retry text."
      }
    }
    if ($errorView.text -match "(?i)access is denied|os error|the system cannot find|[a-z]:\\") {
      throw "Native error state exposed raw operating-system details."
    }
    Save-WebViewScreenshot -Socket $socket -Path (Join-Path $Context.ArtifactDirectory "error.png")

    $after = Get-FixtureSnapshot -Root $Context.FixtureRoot
    if ($Context.Before.sha256 -ne $after.sha256) {
      throw "Native project discovery changed fixture files."
    }
    $report = [ordered]@{
      buildRevision = $Context.BuildRevision
      target = $Context.Target
      processArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
      standardUser = -not [ProcessTokenProbe]::IsElevated($Process.Handle)
      user = [Environment]::UserName
      fixtureRoot = $Context.FixtureRoot
      fixtureBefore = $Context.Before
      fixtureAfter = $after
      states = [ordered]@{ success = $success.status; empty = $empty.status; error = $errorView.status }
      screenshots = @("success.png", "empty.png", "error.png")
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Context.ArtifactDirectory "report.json") -Encoding UTF8
  }
  finally {
    if ($socket) {
      $socket.Dispose()
    }
  }
}
