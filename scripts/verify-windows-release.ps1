param(
  [ValidateSet("x86_64-pc-windows-msvc")]
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$InstallerPath
)

$ErrorActionPreference = "Stop"
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

function Assert-ValidSignature {
  param([string]$Path)

  $signature = Get-AuthenticodeSignature -LiteralPath $Path
  if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid -or -not $signature.SignerCertificate) {
    throw "Authenticode validation failed for $Path`: $($signature.StatusMessage)"
  }
  return $signature.SignerCertificate.Thumbprint
}

$installerThumbprint = Assert-ValidSignature $InstallerPath
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

  $appThumbprint = Assert-ValidSignature $appPath
  $helperThumbprint = Assert-ValidSignature $helperPath
  if ($installerThumbprint -ne $appThumbprint -or $appThumbprint -ne $helperThumbprint) {
    throw "The installer, application, and privileged helper were not signed by one certificate."
  }

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
  Write-Output "Signed release signatures, adjacency, Program Files ACLs, and standard integrity verified."
}
finally {
  if ($installed) {
    $uninstaller = Join-Path $installDirectory "uninstall.exe"
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
      Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait | Out-Null
    }
  }
}
