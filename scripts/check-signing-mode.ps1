# Fails closed unless the release signing mode is declared exactly and matches
# the committed marker in docs/release.md ("Current signing mode: unsigned").
param(
  [string]$Mode
)

$ErrorActionPreference = "Stop"
if ($Mode -cne "unsigned" -and $Mode -cne "authenticode") {
  throw "Repository variable WINDOWS_SIGNING_MODE must be exactly 'unsigned' or 'authenticode' (got '$Mode'). Releases refuse to run without it."
}
$releaseDoc = Get-Content -LiteralPath (Join-Path $PSScriptRoot "../docs/release.md") -Raw
$match = [regex]::Match($releaseDoc, '(?m)^Current signing mode: `?(unsigned|authenticode)`?\s*$')
if (-not $match.Success) {
  throw "docs/release.md has no 'Current signing mode:' marker."
}
if ($match.Groups[1].Value -cne $Mode) {
  throw "WINDOWS_SIGNING_MODE is '$Mode' but docs/release.md declares '$($match.Groups[1].Value)'. Update one to match."
}
Write-Output "Release signing mode: $Mode (matches docs/release.md)."
