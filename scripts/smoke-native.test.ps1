$ErrorActionPreference = "Stop"
$tempDirectory = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tempDirectory | Out-Null

try {
  $exe = Join-Path $tempDirectory "supa-diska-klinah.exe"
  Add-Type -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

public static class EarlyExitFixture
{
    public static int Main(string[] arguments)
    {
        if (arguments.Length == 1 && arguments[0] == "hold-handles")
        {
            Thread.Sleep(30000);
            return 0;
        }

        Process child = Process.Start(new ProcessStartInfo
        {
            FileName = Process.GetCurrentProcess().MainModule.FileName,
            Arguments = "hold-handles",
            CreateNoWindow = true,
            UseShellExecute = false
        });
        File.WriteAllText(Environment.GetEnvironmentVariable("SMOKE_DESCENDANT_PID_PATH"), child.Id.ToString());
        Console.Out.WriteLine("fixture stdout");
        Console.Error.WriteLine("fixture stderr");
        return 23;
    }
}
"@ -OutputAssembly $exe -OutputType ConsoleApplication
  New-Item -ItemType File -Path (Join-Path $tempDirectory "supa-diska-klinah-privileged-helper.exe") | Out-Null

  $descendantPidPath = Join-Path $tempDirectory "descendant.pid"
  $env:SMOKE_DESCENDANT_PID_PATH = $descendantPidPath
  $message = $null
  try {
    & "$PSScriptRoot/smoke-native.ps1" -Target x86_64-pc-windows-msvc -Directory $tempDirectory
  }
  catch {
    $message = $_.Exception.Message
  }
  finally {
    Remove-Item Env:SMOKE_DESCENDANT_PID_PATH
  }

  $fixtureChildId = [int](Get-Content -Path $descendantPidPath -Raw)
  if (Get-Process -Id $fixtureChildId -ErrorAction SilentlyContinue) {
    throw "Native smoke left descendant process $fixtureChildId holding redirected output handles."
  }

  foreach ($expected in @("code 23", "stdout:`nfixture stdout", "stderr:`nfixture stderr")) {
    if (-not $message.Contains($expected)) {
      throw "Expected native smoke failure output to contain '$expected', got: $message"
    }
  }

  $smokeSource = Get-Content -Path "$PSScriptRoot/smoke-native.ps1" -Raw
  if (-not $smokeSource.Contains("-WindowStyle Hidden")) {
    throw "Native smoke must prevent the CI desktop from showing its process window."
  }
  if ($smokeSource.IndexOf('$env:SUPA_DISKA_KLINAH_SMOKE_MINIMIZED') -gt $smokeSource.IndexOf('Start-Process')) {
    throw "Native smoke must configure hidden startup before launching the executable."
  }

  Write-Output "Native smoke diagnostics, hidden launch, and process-tree cleanup verified."
}
finally {
  if ($fixtureChildId) {
    Stop-Process -Id $fixtureChildId -Force -ErrorAction SilentlyContinue
    Wait-Process -Id $fixtureChildId -ErrorAction SilentlyContinue
  }
  Remove-Item Env:SMOKE_DESCENDANT_PID_PATH -ErrorAction SilentlyContinue
  Remove-Item -Path $tempDirectory -Recurse -Force
}
