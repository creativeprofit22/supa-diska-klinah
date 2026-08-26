$ErrorActionPreference = "Stop"
$tempDirectory = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tempDirectory | Out-Null

try {
  $exe = Join-Path $tempDirectory "supa-diska-klinah.exe"
  Add-Type -TypeDefinition @"
using System;

public static class EarlyExitFixture
{
    public static int Main()
    {
        Console.Out.WriteLine("fixture stdout");
        Console.Error.WriteLine("fixture stderr");
        return 23;
    }
}
"@ -OutputAssembly $exe -OutputType ConsoleApplication
  New-Item -ItemType File -Path (Join-Path $tempDirectory "supa-diska-klinah-privileged-helper.exe") | Out-Null

  $message = $null
  try {
    & "$PSScriptRoot/smoke-native.ps1" -Target x86_64-pc-windows-msvc -Directory $tempDirectory
  }
  catch {
    $message = $_.Exception.Message
  }

  foreach ($expected in @("code 23", "stdout:`nfixture stdout", "stderr:`nfixture stderr")) {
    if (-not $message.Contains($expected)) {
      throw "Expected native smoke failure output to contain '$expected', got: $message"
    }
  }

  Write-Output "Native smoke early-exit diagnostics verified."
}
finally {
  Remove-Item -Path $tempDirectory -Recurse -Force
}
