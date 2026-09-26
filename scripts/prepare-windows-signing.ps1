# Imports the code-signing certificate for an Authenticode release. Runs only
# when the repository's signing mode is `authenticode`; unsigned releases use
# the plain tauri.conf.json and never touch certificate secrets.
param(
  [string]$ConfigPath,
  [Parameter(Mandatory = $true)]
  [ValidateSet("unsigned", "authenticode")]
  [string]$SigningMode,
  [string]$ExpectedThumbprint
)

$ErrorActionPreference = "Stop"
if ($SigningMode -ne "authenticode") {
  throw "Code signing is only prepared in authenticode mode (current mode: $SigningMode)."
}
if ($ExpectedThumbprint -notmatch '^[0-9A-F]{40}$') {
  throw "Authenticode mode needs the expected product certificate thumbprint (40 uppercase hex characters)."
}
$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
if (-not $ConfigPath) {
  $ConfigPath = Join-Path $tempRoot "tauri.release.conf.json"
}
$encodedCertificate = $env:WINDOWS_CODESIGN_PFX_BASE64
$certificatePassword = $env:WINDOWS_CODESIGN_PFX_PASSWORD
if ([string]::IsNullOrWhiteSpace($encodedCertificate) -or [string]::IsNullOrWhiteSpace($certificatePassword)) {
  throw "The protected Windows release environment must provide both code-signing secrets."
}

$pfxPath = Join-Path $tempRoot "windows-codesign-$PID.pfx"
try {
  [IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($encodedCertificate))
  $securePassword = ConvertTo-SecureString $certificatePassword -AsPlainText -Force
  $certificate = Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation Cert:\CurrentUser\My -Password $securePassword -Exportable:$false
  if (-not $certificate.HasPrivateKey) {
    throw "The imported Windows code-signing certificate has no private key."
  }
  if ($certificate.NotBefore -gt [DateTime]::Now -or $certificate.NotAfter -le [DateTime]::Now) {
    throw "The Windows code-signing certificate is not currently valid."
  }
  if (-not ($certificate.EnhancedKeyUsageList.ObjectId.Value -contains "1.3.6.1.5.5.7.3.3")) {
    throw "The imported certificate is not valid for code signing."
  }
  if ($certificate.Thumbprint -ne $ExpectedThumbprint) {
    throw "The provided certificate ($($certificate.Thumbprint)) is not the expected product certificate ($ExpectedThumbprint)."
  }

  @{ bundle = @{ windows = @{ certificateThumbprint = $certificate.Thumbprint } } } |
    ConvertTo-Json -Depth 4 |
    Set-Content -LiteralPath $ConfigPath -Encoding UTF8

  if ($env:GITHUB_ENV) {
    "TAURI_RELEASE_CONFIG=$ConfigPath" | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding UTF8
    "TAURI_WINDOWS_CERTIFICATE_THUMBPRINT=$($certificate.Thumbprint)" |
      Add-Content -LiteralPath $env:GITHUB_ENV -Encoding UTF8
  }
  Write-Output "Prepared the external Windows signing certificate and release config."
}
finally {
  Remove-Item -LiteralPath $pfxPath -Force -ErrorAction SilentlyContinue
}
