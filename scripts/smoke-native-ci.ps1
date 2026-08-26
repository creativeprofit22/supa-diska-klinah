param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target
)

$ErrorActionPreference = "Stop"
$username = "SupaNativeSmoke"
$password = "Aa1!" + [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(16))
$stdoutPath = [IO.Path]::GetTempFileName()
$stderrPath = [IO.Path]::GetTempFileName()
$userCreated = $false

try {
  $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
  New-LocalUser -Name $username -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword | Out-Null
  $userCreated = $true

  $credential = [Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$username", $securePassword)
  $script = Join-Path $PSScriptRoot "smoke-native.ps1"
  $command = "& '$($script.Replace("'", "''"))' -Target '$Target'"
  $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
  $shell = (Get-Process -Id $PID).Path
  $process = Start-Process -FilePath $shell -ArgumentList "-NoProfile", "-EncodedCommand", $encodedCommand -Credential $credential -LoadUserProfile -WorkingDirectory (Get-Location).Path -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -Wait -PassThru

  [Console]::Out.Write((Get-Content -Path $stdoutPath -Raw))
  [Console]::Error.Write((Get-Content -Path $stderrPath -Raw))
  exit $process.ExitCode
}
finally {
  if ($userCreated) {
    Remove-LocalUser -Name $username
  }
  Remove-Item -Path $stdoutPath, $stderrPath -Force
}
