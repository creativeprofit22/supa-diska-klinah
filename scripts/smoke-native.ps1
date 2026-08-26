param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target
)

$ErrorActionPreference = "Stop"
$directory = "src-tauri/target/$Target/debug"
$exe = Resolve-Path "$directory/supa-diska-klinah.exe"
$helper = "$directory/supa-diska-klinah-privileged-helper.exe"
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
}
"@

$env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED = "1"
$process = Start-Process -FilePath $exe -WindowStyle Minimized -PassThru
try {
  $windowSeen = $false
  for ($attempt = 0; $attempt -lt 80; $attempt++) {
    $process.Refresh()
    if ($process.HasExited) {
      throw "Native executable exited during the smoke window with code $($process.ExitCode)."
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
  $process.Refresh()
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id
  }
}
