param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
  [string]$Target,
  [string]$Directory,
  [string]$ArtifactDirectory,
  [string]$BuildRevision
)

$ErrorActionPreference = "Stop"
if (-not $ArtifactDirectory) {
  $ArtifactDirectory = Join-Path (Get-Location) "artifacts/native-smoke/$Target"
}
if (-not $BuildRevision) {
  $BuildRevision = if ($env:GITHUB_SHA) { $env:GITHUB_SHA } else { (& git rev-parse --verify HEAD 2>$null).Trim() }
}
if ($BuildRevision -notmatch "^[0-9a-f]{40}$") {
  throw "Native smoke requires the 40-character source revision used for this build."
}
$username = "SupaNativeSmoke"
$password = [REDACTED] + [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(16))
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
  $profilePath = Join-Path $env:SystemDrive "Users\$username"
  $script = Join-Path $PSScriptRoot "smoke-native.ps1"
  $commandPath = Join-Path $smokeTempPath "run-smoke.ps1"
  $innerArtifacts = Join-Path $smokeTempPath "artifacts"
  $directoryArgument = if ($Directory) { " -Directory '$($Directory.Replace("'", "''"))'" } else { "" }
  @"
`$env:USERPROFILE = '$profilePath'
`$env:HOME = `$env:USERPROFILE
`$env:HOMEDRIVE = '$env:SystemDrive'
`$env:HOMEPATH = '\Users\$username'
`$env:LOCALAPPDATA = '$profilePath\AppData\Local'
`$env:APPDATA = '$profilePath\AppData\Roaming'
`$env:TEMP = '$($smokeTempPath.Replace("'", "''"))'
`$env:TMP = `$env:TEMP
& '$($script.Replace("'", "''"))' -Target '$Target'$directoryArgument -ArtifactDirectory '$($innerArtifacts.Replace("'", "''"))' -BuildRevision '$BuildRevision'
"@ | Set-Content -Path $commandPath -Encoding UTF8

  $shell = (Get-Process -Id $PID).Path
  $process = Start-Process -FilePath $shell -ArgumentList "-NoProfile", "-File", $commandPath -Credential $credential -LoadUserProfile -WorkingDirectory (Get-Location).Path -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -Wait -PassThru

  [Console]::Out.Write((Get-Content -Path $stdoutPath -Raw))
  [Console]::Error.Write((Get-Content -Path $stderrPath -Raw))
  $evidence = @(Get-ChildItem -LiteralPath $innerArtifacts -File -ErrorAction SilentlyContinue)
  if ($evidence.Count -gt 0) {
    New-Item -ItemType Directory -Path $ArtifactDirectory -Force | Out-Null
    Copy-Item -LiteralPath $evidence.FullName -Destination $ArtifactDirectory -Force
  }
  if ($process.ExitCode -ne 0) {
    throw "The standard-user smoke process exited with code $($process.ExitCode)."
  }
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
