# Release-candidate acceptance for Supa Diska Klinah, in the declared signing mode.
#
# Phases (run in this order; each appends a JSON line to <ArtifactDirectory>/results.jsonl):
#   Rehearsal  update tamper/downgrade/interrupt rehearsal against REAL Windows
#              signature checks (cargo test ... real_signature_rehearsal). No install.
#   Offline    launches the candidate app hidden with every opt-in off, tours every
#              route, and requires zero non-loopback TCP connections from the app's
#              process tree; also requires the main process to run at medium integrity.
#   Install    (elevated; a person answers UAC) silent per-machine install of the
#              candidate, then Offline against the INSTALLED app.
#   Uninstall  (elevated) registers a disposable task in \SupaDiskaKlinah, uninstalls,
#              and requires: no install directory, no HKLM uninstall key, no
#              \SupaDiskaKlinah task folder, no running helper, and user data kept
#              (a marker in %APPDATA%\com.supadiskaklinah.app survives the silent
#              uninstall, which leaves the delete-app-data checkbox unticked).
# Parity, project cleanup, retention, undo and per-operation UAC counts are covered by
# the existing built-app drivers (acceptance-storage/system/protection-builtapp.ps1)
# and the build-artifact drill in docs/release-checklist.md; they need a person.
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("Rehearsal", "Offline", "Install", "Uninstall")]
  [string]$Phase,
  [Parameter(Mandatory = $true)]
  [ValidateSet("unsigned", "authenticode")]
  [string]$SigningMode,
  [string]$InstallerPath,
  [string]$Executable = "src-tauri/target/x86_64-pc-windows-msvc/release/supa-diska-klinah.exe",
  [string]$ArtifactDirectory = ".gg/smoke-artifacts/release-acceptance",
  [int]$Port = 9340,
  [int]$DwellMs = 1500
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
$results = Join-Path $ArtifactDirectory "results.jsonl"
$executionId = [guid]::NewGuid().ToString()
$productDirectory = Join-Path ${env:ProgramFiles} "Supa Diska Klinah"
$uninstallKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Supa Diska Klinah"
# Every route in src/app/router.tsx and the feature route files.
$routes = @("/", "/drives", "/disk-analyzer", "/large-files", "/duplicates", "/empty-folders", "/cleaner", "/browser",
  "/uninstaller", "/cleanup", "/optimizer", "/startup", "/services", "/privacy", "/firewall", "/hosts", "/power",
  "/drivers", "/restore-points", "/windows-update", "/scheduled-scans", "/settings",
  "/protection", "/protection/scan", "/protection/processes", "/protection/quarantine", "/protection/rules", "/protection/breach")

function Write-Result([string]$Name, [bool]$Passed, $Detail) {
  $entry = [ordered]@{
    executionId = $executionId; phase = $Phase; check = $Name; passed = $Passed; signingMode = $SigningMode
    utc = (Get-Date).ToUniversalTime().ToString("o"); detail = $Detail
  }
  Add-Content -LiteralPath $results -Value ($entry | ConvertTo-Json -Depth 8 -Compress) -Encoding UTF8
  Write-Output ("[{0}] {1}: {2}" -f ($(if ($Passed) { "PASS" } else { "FAIL" }), $Name, ($Detail | ConvertTo-Json -Depth 6 -Compress)))
  if (-not $Passed) { $script:failed = $true }
}

function Test-Elevated {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  return [Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-ProcessTree([int]$Root) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.HashSet[int]]::new()
  [void]$ids.Add($Root)
  do {
    $added = $false
    foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true } }
  } while ($added)
  return $ids
}

function Get-IntegrityRid([int]$ProcessId) {
  # whoami-free: read the process token's mandatory label through PowerShell's .NET access.
  Add-Type -Namespace SdkAcceptance -Name Token -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
[DllImport("advapi32.dll", SetLastError = true)] public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
[DllImport("advapi32.dll", SetLastError = true)] public static extern bool GetTokenInformation(IntPtr token, int cls, IntPtr info, int length, out int returned);
[DllImport("advapi32.dll")] public static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
[DllImport("advapi32.dll")] public static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
"@ -ErrorAction SilentlyContinue
  $process = [SdkAcceptance.Token]::OpenProcess(0x1000, $false, $ProcessId)
  if ($process -eq [IntPtr]::Zero) { throw "Cannot open process $ProcessId" }
  try {
    $token = [IntPtr]::Zero
    if (-not [SdkAcceptance.Token]::OpenProcessToken($process, 0x0008, [ref]$token)) { throw "Cannot open token" }
    try {
      $size = 0
      [void][SdkAcceptance.Token]::GetTokenInformation($token, 25, [IntPtr]::Zero, 0, [ref]$size)
      $buffer = [Runtime.InteropServices.Marshal]::AllocHGlobal($size)
      try {
        if (-not [SdkAcceptance.Token]::GetTokenInformation($token, 25, $buffer, $size, [ref]$size)) { throw "No integrity label" }
        $sid = [Runtime.InteropServices.Marshal]::ReadIntPtr($buffer)
        $count = [Runtime.InteropServices.Marshal]::ReadByte([SdkAcceptance.Token]::GetSidSubAuthorityCount($sid))
        return [Runtime.InteropServices.Marshal]::ReadInt32([SdkAcceptance.Token]::GetSidSubAuthority($sid, [uint32]($count - 1)))
      }
      finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($buffer) }
    }
    finally { [void][SdkAcceptance.Token]::CloseHandle($token) }
  }
  finally { [void][SdkAcceptance.Token]::CloseHandle($process) }
}

function Invoke-OfflineTour([string]$AppPath) {
  . (Join-Path $PSScriptRoot "smoke-project-discovery.ps1")
  $exe = (Resolve-Path -LiteralPath $AppPath).Path
  # Fresh, isolated WebView profile and hidden window; the debug port is loopback-only.
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-address=127.0.0.1 --remote-debugging-port=$Port"
  $env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED = "1"
  $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Hidden -PassThru
  $seen = @{}
  try {
    $socket = Connect-WebViewDebugSocket -Port $Port
    Wait-WebViewExpression -Socket $socket -Expression "typeof window.__TAURI_INTERNALS__?.invoke === 'function'" -Failure "App did not mount"
    $settings = Invoke-WebViewExpression -Socket $socket -Expression "window.__TAURI_INTERNALS__.invoke('get_app_settings', {})"
    Write-Result "update check is off by default" (-not $settings.updateCheck) $settings
    $rid = Get-IntegrityRid $process.Id
    Write-Result "main process runs at medium integrity" ($rid -eq 0x2000) @{ integrityRid = ('0x{0:X}' -f $rid) }
    $visited = @()
    foreach ($route in $routes) {
      $null = Invoke-WebViewExpression -Socket $socket -Expression "(location.hash = '#$route', true)"
      $deadline = (Get-Date).AddMilliseconds($DwellMs)
      $tree = Get-ProcessTree $process.Id
      while ((Get-Date) -lt $deadline) {
        foreach ($c in Get-NetTCPConnection -ErrorAction SilentlyContinue) {
          if (-not $tree.Contains([int]$c.OwningProcess)) { continue }
          $remote = [string]$c.RemoteAddress
          if ($remote -in @("0.0.0.0", "::", "::1") -or $remote.StartsWith("127.")) { continue }
          $seen["$remote|$($c.RemotePort)"] = $route
        }
        Start-Sleep -Milliseconds 100
      }
      $heading = Invoke-WebViewExpression -Socket $socket -Expression "document.querySelector('main h1')?.textContent ?? null"
      $visited += [ordered]@{ route = $route; heading = $heading }
    }
    $unrendered = @($visited | Where-Object { -not $_.heading })
    Write-Result "every route renders a heading" ($unrendered.Count -eq 0) @{ routes = $routes.Count; missing = $unrendered }
    Write-Result "no outbound connections with every opt-in off" ($seen.Count -eq 0) @{ endpoints = $seen; processTree = @($tree).Count }
  }
  finally {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, Env:\SUPA_DISKA_KLINAH_SMOKE_MINIMIZED -ErrorAction SilentlyContinue
  }
}

$script:failed = $false
switch ($Phase) {
  "Rehearsal" {
    if (-not $InstallerPath) { throw "-InstallerPath (the unsigned candidate installer) is required." }
    $env:SDK_REHEARSAL_UNSIGNED = (Resolve-Path -LiteralPath $InstallerPath).Path
    $signature = Get-AuthenticodeSignature -LiteralPath $env:SDK_REHEARSAL_UNSIGNED
    if ($SigningMode -eq "unsigned" -and $signature.Status -ne "NotSigned") { throw "The candidate is not plainly unsigned ($($signature.Status))." }
    Push-Location src-tauri
    try {
      $output = & cargo test -p windows-platform --locked --lib real_signature_rehearsal -- --ignored --nocapture --test-threads 1 2>&1 | Out-String
      $passed = $LASTEXITCODE -eq 0 -and $output -match "test result: ok\. 1 passed"
    }
    finally { Pop-Location; Remove-Item Env:\SDK_REHEARSAL_UNSIGNED -ErrorAction SilentlyContinue }
    $lines = @(($output -split "`r?`n") | Where-Object { $_ -match "^(REHEARSAL RESULTS|[a-z].*: )" })
    Write-Result "update rehearsal (real WinVerifyTrust)" $passed @{ candidateSha256 = (Get-FileHash -LiteralPath $InstallerPath).Hash; results = $lines }
  }
  "Offline" {
    Invoke-OfflineTour $Executable
  }
  "Install" {
    if (-not (Test-Elevated)) { throw "Install needs an elevated session; a person must approve UAC." }
    if (-not $InstallerPath) { throw "-InstallerPath is required." }
    $install = Start-Process -FilePath $InstallerPath -ArgumentList "/S" -Wait -PassThru
    Write-Result "silent per-machine install" ($install.ExitCode -eq 0 -and (Test-Path -LiteralPath $productDirectory)) @{ exitCode = $install.ExitCode }
    Invoke-OfflineTour (Join-Path $productDirectory "supa-diska-klinah.exe")
  }
  "Uninstall" {
    if (-not (Test-Elevated)) { throw "Uninstall needs an elevated session; a person must approve UAC." }
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    try { $folder = $service.GetFolder("\SupaDiskaKlinah") } catch { $folder = $service.GetFolder("\").CreateFolder("\SupaDiskaKlinah") }
    $definition = $service.NewTask(0)
    $definition.Settings.Enabled = $false
    $action = $definition.Actions.Create(0)
    $action.Path = "C:\Windows\System32\cmd.exe"
    $action.Arguments = "/c exit"
    $null = $folder.RegisterTaskDefinition("scan-00000000-0000-4000-8000-00000000acce", $definition, 6, $null, $null, 3)
    # BUNDLEID is tauri.conf.json's identifier; NSIS deletes it only when the checkbox is ticked.
    $dataDirectory = Join-Path $env:APPDATA "com.supadiskaklinah.app"
    $dataDirectoryExisted = Test-Path -LiteralPath $dataDirectory
    New-Item -ItemType Directory -Path $dataDirectory -Force | Out-Null
    $marker = Join-Path $dataDirectory "sdk-acceptance-marker.txt"
    Set-Content -LiteralPath $marker -Value $executionId -Encoding UTF8
    $uninstaller = Join-Path $productDirectory "uninstall.exe"
    $run = Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait -PassThru
    Start-Sleep -Seconds 3
    $folderGone = $true
    try { $null = $service.GetFolder("\SupaDiskaKlinah"); $folderGone = $false } catch { }
    Write-Result "uninstaller exit code" ($run.ExitCode -eq 0) @{ exitCode = $run.ExitCode }
    Write-Result "install directory removed" (-not (Test-Path -LiteralPath (Join-Path $productDirectory "supa-diska-klinah.exe"))) @{ directory = $productDirectory }
    Write-Result "HKLM uninstall key removed" (-not (Test-Path -LiteralPath $uninstallKey)) @{ key = $uninstallKey }
    Write-Result "\SupaDiskaKlinah scheduled-task folder removed" $folderGone @{}
    $helpers = @(Get-Process -Name "supa-diska-klinah-privileged-helper" -ErrorAction SilentlyContinue)
    Write-Result "no helper process left" ($helpers.Count -eq 0) @{ running = $helpers.Count }
    Write-Result "user data kept on default uninstall" (Test-Path -LiteralPath $marker) @{ directory = $dataDirectory; existedBefore = $dataDirectoryExisted }
    Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
    if (-not $dataDirectoryExisted) { Remove-Item -LiteralPath $dataDirectory -Recurse -Force -ErrorAction SilentlyContinue }
  }
}
Write-Output "execution $executionId -> $results"
if ($script:failed) { exit 1 }
