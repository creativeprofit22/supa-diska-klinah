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
$smokeTempPath = $null
$userCreated = $false

try {
  $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
  $user = New-LocalUser -Name $username -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -UserMayNotChangePassword
  $userCreated = $true

  $smokeTempPath = Join-Path $env:SystemRoot "Temp\supa-native-smoke-$PID"
  New-Item -ItemType Directory -Path $smokeTempPath | Out-Null
  & icacls.exe $smokeTempPath /grant "*$($user.SID.Value):(OI)(CI)M" /Q | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "Could not grant the standard-user smoke account access to its temporary directory."
  }

  $credential = [Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$username", $securePassword)
  $script = Join-Path $PSScriptRoot "smoke-native.ps1"
  $command = "`$env:TEMP = '$($smokeTempPath.Replace("'", "''"))'; `$env:TMP = `$env:TEMP; & '$($script.Replace("'", "''"))' -Target '$Target'"
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
  if ($smokeTempPath) {
    Remove-Item -Path $smokeTempPath -Recurse -Force
  }
  Remove-Item -Path $stdoutPath, $stderrPath -Force
}
