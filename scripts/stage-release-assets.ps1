# Stages verified release assets for attestation and publishing:
#   Supa-Diska-Klinah_<ver>_x64-setup.exe, update.json(.sig), SHA256SUMS,
#   dependency-inventory.json
# Runs after verify-windows-release.ps1 in the release build job.
param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("unsigned", "authenticode")]
  [string]$SigningMode,
  [string]$ExpectedThumbprint,
  [ValidateSet("x86_64-pc-windows-msvc")]
  [string]$Target = "x86_64-pc-windows-msvc",
  [Parameter(Mandatory = $true)]
  [string]$OutDir
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($env:UPDATE_SIGNING_KEY)) {
  throw "UPDATE_SIGNING_KEY is not available; refusing to stage a release without a signed update manifest."
}
if ($SigningMode -eq "authenticode" -and $ExpectedThumbprint -notmatch '^[0-9A-F]{40}$') {
  throw "Authenticode mode needs the expected product certificate thumbprint."
}

$version = (Get-Content -LiteralPath (Join-Path $PSScriptRoot "../package.json") -Raw | ConvertFrom-Json).version
$tag = $env:GITHUB_REF_NAME
if ($env:GITHUB_REF -like "refs/tags/*" -and $tag -cne "v$version") {
  throw "Tag $tag does not match package version $version."
}

$installers = @(Get-ChildItem "src-tauri/target/$Target/release/bundle/nsis/*.exe")
if ($installers.Count -ne 1) {
  throw "Expected exactly one NSIS installer for $Target, found $($installers.Count)."
}
if (Test-Path -LiteralPath $OutDir) {
  throw "$OutDir already exists; staging never overwrites assets."
}
New-Item -ItemType Directory -Path $OutDir | Out-Null
$assetName = "Supa-Diska-Klinah_${version}_x64-setup.exe"
$asset = Join-Path $OutDir $assetName
Copy-Item -LiteralPath $installers[0].FullName -Destination $asset

# Locked dependency inventory (Rust + production npm), sorted for reproducibility.
Push-Location src-tauri
try {
  $metadata = cargo metadata --format-version 1 --locked --filter-platform $Target | ConvertFrom-Json
  if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed." }
}
finally { Pop-Location }
$used = @{}
foreach ($node in $metadata.resolve.nodes) { $used[$node.id] = $true }
$rust = $metadata.packages |
  Where-Object { $used[$_.id] -and $_.source } |
  ForEach-Object { [ordered]@{ name = $_.name; version = $_.version; license = $_.license; source = $_.source } } |
  Sort-Object { $_.name }, { $_.version }
$npmJson = pnpm list --prod --depth Infinity --json
if ($LASTEXITCODE -ne 0) { throw "pnpm list failed." }
$inventory = [ordered]@{ version = $version; rust = @($rust); npm = ($npmJson | ConvertFrom-Json) }
$inventory | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath (Join-Path $OutDir "dependency-inventory.json") -Encoding UTF8

$signArgs = @(
  "scripts/sign-update-manifest.mjs",
  "--installer", $asset,
  "--version", $version,
  "--signing-mode", $SigningMode,
  "--out-dir", $OutDir
)
if ($SigningMode -eq "authenticode") { $signArgs += @("--thumbprint", $ExpectedThumbprint) }
& node @signArgs
if ($LASTEXITCODE -ne 0) { throw "Signing the update manifest failed." }

# SHA256SUMS over every other asset, sorted, "<hash>  <name>" (sha256sum format).
$lines = Get-ChildItem -LiteralPath $OutDir -File |
  Sort-Object Name -CaseSensitive |
  ForEach-Object { "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)" }
[IO.File]::WriteAllText((Join-Path (Resolve-Path $OutDir) "SHA256SUMS"), (($lines -join "`n") + "`n"))
Write-Output "Staged $SigningMode release assets for $version in $OutDir."
