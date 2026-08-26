param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target,
  [string]$Directory
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

Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class ProcessTokenProbe
{
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
    public static extern bool ShowWindowAsync(IntPtr window, int command);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr window);

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

    public static uint ExitCode(IntPtr process)
    {
        uint exitCode;
        if (!GetExitCodeProcess(process, out exitCode))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return exitCode;
    }
}
"@

$env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED = "1"
$stdoutPath = [IO.Path]::GetTempFileName()
$stderrPath = [IO.Path]::GetTempFileName()
$process = $null
try {
  $process = Start-Process -FilePath $exe -WindowStyle Minimized -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
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
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
      $windowSeen = $true
      [ProcessTokenProbe]::ShowWindowAsync($process.MainWindowHandle, 0) | Out-Null
      Start-Sleep -Milliseconds 50
      $process.Refresh()
      $visible = [ProcessTokenProbe]::IsWindowVisible($process.MainWindowHandle)
      $minimized = [ProcessTokenProbe]::IsIconic($process.MainWindowHandle)
      if ($visible -and -not $minimized) {
        throw "Native executable became visible during the smoke window."
      }
    }
    Start-Sleep -Milliseconds 100
  }
  if (-not $windowSeen) {
    throw "Native executable did not create its main window."
  }
  if ([ProcessTokenProbe]::IsElevated($process.Handle)) {
    throw "Native executable unexpectedly runs with an elevated token."
  }
  Write-Output "$Target native executable remained non-visible at standard integrity with its helper present."
}
finally {
  if ($process) {
    $process.Refresh()
    if (-not $process.HasExited) {
      Stop-Process -Id $process.Id -PassThru | Wait-Process
    }
    $process.Dispose()
  }
  Remove-Item -Path $stdoutPath, $stderrPath -Force
}
