# Installs, inspects, smoke-tests and uninstalls a release candidate.
#
# -SigningMode is mandatory and must match the repository's declared mode:
#   unsigned      the installer, app and helper must be plainly unsigned
#                 (NotSigned). A partial or corrupted signature fails.
#   authenticode  every file must be validly signed by the expected product
#                 certificate (thumbprint and subject) with a timestamp.
param(
  [ValidateSet("x86_64-pc-windows-msvc")]
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$InstallerPath,
  [Parameter(Mandatory = $true)]
  [ValidateSet("unsigned", "authenticode")]
  [string]$SigningMode,
  [string]$ExpectedThumbprint,
  [string]$ExpectedSubject
)

$ErrorActionPreference = "Stop"
if ($SigningMode -eq "authenticode") {
  if ($ExpectedThumbprint -notmatch '^[0-9A-F]{40}$') {
    throw "Authenticode mode needs -ExpectedThumbprint (40 uppercase hex characters)."
  }
  if ([string]::IsNullOrWhiteSpace($ExpectedSubject)) {
    throw "Authenticode mode needs -ExpectedSubject."
  }
}
elseif ($ExpectedThumbprint -or $ExpectedSubject) {
  throw "Unsigned mode must not be given an expected signer."
}
if (-not $InstallerPath) {
  $installer = Get-ChildItem "src-tauri/target/$Target/release/bundle/nsis/*.exe" |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
  if (-not $installer) {
    throw "No NSIS release installer was produced for $Target."
  }
  $InstallerPath = $installer.FullName
}
$InstallerPath = (Resolve-Path -LiteralPath $InstallerPath).Path

function Assert-ReleaseSignature {
  param([string]$Path)

  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  if ($SigningMode -eq "unsigned") {
    if ($signature.Status -ne [Management.Automation.SignatureStatus]::NotSigned -or $signature.SignerCertificate) {
      throw "Unsigned mode requires $Path to be plainly unsigned, but its signature status is $($signature.Status): $($signature.StatusMessage)"
    }
    return
  }
  if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid -or -not $signature.SignerCertificate) {
    throw "Authenticode validation failed for $Path`: $($signature.StatusMessage)"
  }
  if ($signature.SignerCertificate.Thumbprint -ne $ExpectedThumbprint) {
    throw "$Path is signed by $($signature.SignerCertificate.Thumbprint), not the product certificate $ExpectedThumbprint."
  }
  if ($signature.SignerCertificate.Subject -notlike "*CN=$ExpectedSubject*") {
    throw "$Path is signed by '$($signature.SignerCertificate.Subject)', not '$ExpectedSubject'."
  }
  if (-not $signature.TimeStamperCertificate) {
    throw "$Path has no timestamp countersignature."
  }
}

Assert-ReleaseSignature $InstallerPath
$programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
$installDirectory = Join-Path $programFiles "Supa Diska Klinah"
$appPath = Join-Path $installDirectory "supa-diska-klinah.exe"
$helperPath = Join-Path $installDirectory "supa-diska-klinah-privileged-helper.exe"
$installed = $false

try {
  $install = Start-Process -FilePath $InstallerPath -ArgumentList "/S" -Wait -PassThru
  if ($install.ExitCode -ne 0) {
    throw "The NSIS installer exited with code $($install.ExitCode)."
  }
  $installed = $true

  $resolvedInstallDirectory = (Resolve-Path -LiteralPath $installDirectory).Path
  $resolvedProgramFiles = (Resolve-Path -LiteralPath $programFiles).Path
  if (-not $resolvedInstallDirectory.StartsWith("$resolvedProgramFiles\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "The release was not installed under Program Files."
  }
  foreach ($path in $resolvedInstallDirectory, $appPath, $helperPath) {
    $item = Get-Item -LiteralPath $path
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw "The installed release contains a reparse-point path: $path"
    }
  }
  if ((Split-Path -Parent $appPath) -ne (Split-Path -Parent $helperPath)) {
    throw "The privileged helper is not adjacent to the application executable."
  }

  # Every file is checked against the declared mode (and, when signed, the
  # one expected product certificate), not just against each other.
  Assert-ReleaseSignature $appPath
  Assert-ReleaseSignature $helperPath

  $acl = Get-Acl -LiteralPath $resolvedInstallDirectory
  $broadPrincipals = @("S-1-1-0", "S-1-5-11", "S-1-5-32-545")
  $writeMask = [Security.AccessControl.FileSystemRights]::Write -bor
    [Security.AccessControl.FileSystemRights]::Modify -bor
    [Security.AccessControl.FileSystemRights]::FullControl -bor
    [Security.AccessControl.FileSystemRights]::Delete -bor
    [Security.AccessControl.FileSystemRights]::ChangePermissions -bor
    [Security.AccessControl.FileSystemRights]::TakeOwnership
  foreach ($rule in $acl.Access) {
    $sid = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
    if ($rule.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
        $sid -in $broadPrincipals -and
        ($rule.FileSystemRights -band $writeMask) -ne 0) {
      throw "A standard-user principal can modify the installed release: $sid"
    }
  }
  & icacls.exe $resolvedInstallDirectory
  if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect the installed-directory ACL."
  }

  & "$PSScriptRoot/smoke-native-ci.ps1" -Target $Target -Directory $resolvedInstallDirectory
  Write-Output "Release ($SigningMode) signatures, adjacency, Program Files ACLs, and standard integrity verified."
}
finally {
  if ($installed) {
    $uninstaller = Join-Path $installDirectory "uninstall.exe"
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
      Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait | Out-Null
    }
  }
}
