param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target
)

$ErrorActionPreference = "Stop"
$exe = Resolve-Path "src-tauri/target/$Target/debug/supa-diska-klinah.exe"
$process = Start-Process -FilePath $exe -PassThru
try {
  Start-Sleep -Seconds 8
  $process.Refresh()
  if ($process.HasExited) {
    throw "Native executable exited during the smoke window with code $($process.ExitCode)."
  }
  Write-Output "$Target native executable remained alive."
}
finally {
  $process.Refresh()
  if (-not $process.HasExited) {
    Stop-Process -Id $process.Id
  }
}
