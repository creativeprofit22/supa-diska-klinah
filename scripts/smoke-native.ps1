param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target,
  [string]$Directory,
  [string]$ArtifactDirectory,
  [string]$BuildRevision,
  [switch]$LaunchOnly
)

$ErrorActionPreference = "Stop"
if (-not $Directory) {
  $Directory = "src-tauri/target/$Target/debug"
}
$exe = Resolve-Path "$Directory/supa-diska-klinah.exe"
$helper = "$Directory/supa-diska-klinah-privileged-helper.exe"
if (-not (Test-Path -Path $helper -PathType Leaf)) {
  throw "Privileged helper is missing beside the $Target application."
}

function Stop-ProcessTreeAndWait {
  param([Diagnostics.Process]$RootProcess)

  $treeIds = [Collections.Generic.HashSet[int]]::new()
  $treeIds.Add($RootProcess.Id) | Out-Null
  $snapshot = @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop)
  $changed = $true
  while ($changed) {
    $changed = $false
    foreach ($candidate in $snapshot) {
      if ($treeIds.Contains([int]$candidate.ParentProcessId) -and $treeIds.Add([int]$candidate.ProcessId)) {
        $changed = $true
      }
    }
  }

  $RootProcess.Refresh()
  if (-not $RootProcess.HasExited) {
    $taskkill = Start-Process -FilePath "$env:SystemRoot\System32\taskkill.exe" -ArgumentList "/PID", $RootProcess.Id, "/T", "/F" -WindowStyle Hidden -Wait -PassThru
    if ($taskkill.ExitCode -eq 0) {
      $taskkill.Dispose()
      $RootProcess.WaitForExit(5000) | Out-Null
      return
    }
    $taskkill.Dispose()
  }

  $descendants = @(
    $treeIds |
      Where-Object { $_ -ne $RootProcess.Id } |
      ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }
  )
  $RootProcess.Refresh()
  if (-not $RootProcess.HasExited) { $descendants += $RootProcess }
  if ($descendants.Count -eq 0) { return }
  Stop-Process -InputObject $descendants -Force -ErrorAction SilentlyContinue
  Wait-Process -InputObject $descendants -Timeout 2 -ErrorAction SilentlyContinue
  $remainingIds = @($descendants | Where-Object { -not $_.HasExited } | ForEach-Object Id)
  if ($remainingIds.Count -gt 0) {
    throw "Native smoke processes did not stop: $($remainingIds -join ', ')."
  }
}

function Remove-RedirectedOutputFiles {
  param([string[]]$Paths)

  for ($attempt = 0; $attempt -lt 50; $attempt++) {
    $remainingPaths = @($Paths | Where-Object { Test-Path -LiteralPath $_ })
    if ($remainingPaths.Count -eq 0) {
      return
    }

    try {
      Remove-Item -LiteralPath $remainingPaths -Force
      return
    }
    catch [IO.IOException], [UnauthorizedAccessException] {
      if ($attempt -eq 49) {
        throw
      }
      Start-Sleep -Milliseconds 100
    }
  }
}

Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public enum NativeWindowState
{
    None,
    Hidden,
    Visible
}

public static class ProcessTokenProbe
{
    private const int ExtendedWindowStyleIndex = -20;
    private const long ToolWindowStyle = 0x80L;
    private delegate bool EnumWindowsProcedure(IntPtr window, IntPtr parameter);

    [StructLayout(LayoutKind.Sequential)]
    private struct TokenElevation { public int TokenIsElevated; }

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern bool GetTokenInformation(
        IntPtr token, int informationClass, out TokenElevation information,
        int informationLength, out int returnedLength);

    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProcedure callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool IsWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr window, int index);

    public static bool IsElevated(IntPtr process)
    {
        IntPtr token;
        if (!OpenProcessToken(process, 0x0008, out token))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        try
        {
            TokenElevation elevation;
            int returned;
            int size = Marshal.SizeOf<TokenElevation>();
            if (!GetTokenInformation(token, 20, out elevation, size, out returned) || returned != size)
                throw new Win32Exception(Marshal.GetLastWin32Error());
            return elevation.TokenIsElevated != 0;
        }
        finally { CloseHandle(token); }
    }

    public static NativeWindowState WindowState(int processId)
    {
        NativeWindowState state = NativeWindowState.None;
        EnumWindows(delegate(IntPtr window, IntPtr parameter)
        {
            uint ownerProcessId;
            GetWindowThreadProcessId(window, out ownerProcessId);
            if (ownerProcessId != processId)
                return true;

            bool visible = IsWindowVisible(window);
            if (!IsWindow(window))
                return true;
            if ((GetWindowLongPtr(window, ExtendedWindowStyleIndex).ToInt64() & ToolWindowStyle) != 0)
                return true;

            if (visible)
                state = NativeWindowState.Visible;
            else if (state == NativeWindowState.None)
                state = NativeWindowState.Hidden;
            return true;
        }, IntPtr.Zero);
        return state;
    }

    public static uint ExitCode(IntPtr process)
    {
        uint exitCode;
        if (!GetExitCodeProcess(process, out exitCode))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return exitCode;
    }
}
"@

if (-not $LaunchOnly) {
  . "$PSScriptRoot/smoke-project-discovery.ps1"
  if (-not $ArtifactDirectory) {
    $ArtifactDirectory = Join-Path (Get-Location) "artifacts/native-smoke/$Target"
  }
  if (-not $BuildRevision) {
    $BuildRevision = (& git rev-parse --verify HEAD 2>$null).Trim()
  }
  if ($BuildRevision -notmatch "^[0-9a-f]{40}$") {
    throw "Native smoke requires the 40-character source revision used for this build."
  }
}

$env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED = "1"
$stdoutPath = [IO.Path]::GetTempFileName()
$stderrPath = [IO.Path]::GetTempFileName()
$process = $null
$projectSmoke = $null
try {
  if (-not $LaunchOnly) {
    $projectSmoke = New-ProjectArtifactSmokeContext -ArtifactDirectory $ArtifactDirectory -BuildRevision $BuildRevision -Target $Target
  }
  $process = Start-Process -FilePath $exe -WindowStyle Hidden -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
  $processHandle = $process.Handle
  $windowSeen = $false
  for ($attempt = 0; $attempt -lt 80; $attempt++) {
    $process.Refresh()
    if ($process.HasExited) {
      $exitCode = [ProcessTokenProbe]::ExitCode($processHandle)
      $stdout = Get-Content -Path $stdoutPath -Raw
      $stderr = Get-Content -Path $stderrPath -Raw
      throw "Native executable exited during the smoke window with code $exitCode.`nstdout:`n$stdout`nstderr:`n$stderr"
    }
    $windowState = [ProcessTokenProbe]::WindowState($process.Id)
    if ($windowState -eq [NativeWindowState]::Visible) {
      throw "Native executable became visible during the smoke window."
    }
    if ($windowState -eq [NativeWindowState]::Hidden) {
      $windowSeen = $true
    }
    Start-Sleep -Milliseconds 100
  }
  if (-not $windowSeen) {
    throw "Native executable did not create its main window."
  }
  if ([ProcessTokenProbe]::IsElevated($process.Handle)) {
    throw "Native executable unexpectedly runs with an elevated token."
  }
  if ($projectSmoke) {
    Invoke-ProjectArtifactDiscoverySmoke -Context $projectSmoke -Process $process
    Write-Output "$Target native project discovery passed through packaged WebView IPC; evidence: $ArtifactDirectory"
  }
  else {
    Write-Output "$Target native executable remained non-visible at standard integrity with its helper present."
  }
}
finally {
  if ($process) {
    Stop-ProcessTreeAndWait -RootProcess $process
    $process.Dispose()
  }
  if ($projectSmoke) {
    Close-ProjectArtifactSmokeContext -Context $projectSmoke
  }
  Remove-RedirectedOutputFiles -Paths $stdoutPath, $stderrPath
}
