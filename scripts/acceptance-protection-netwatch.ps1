# Samples TCP connections owned by the app process and its descendants (WebView2
# children included) and records every non-loopback remote endpoint.
# Stops when the stop file appears or after MaxSeconds.
param(
  [Parameter(Mandatory)] [int]$RootProcessId,
  [string]$OutFile = ".gg/smoke-artifacts/protection-acceptance/netwatch.jsonl",
  [string]$StopFile = ".gg/smoke-artifacts/protection-acceptance/netwatch.stop",
  [int]$IntervalMs = 250,
  [int]$MaxSeconds = 1800
)
function Get-Tree([int]$Root) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.HashSet[int]]::new()
  [void]$ids.Add($Root)
  do {
    $added = $false
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
    }
  } while ($added)
  return $ids
}
Remove-Item -LiteralPath $StopFile -ErrorAction SilentlyContinue
$deadline = (Get-Date).AddSeconds($MaxSeconds)
$seen = @{}
$treeAt = [datetime]::MinValue
while ((Get-Date) -lt $deadline -and -not (Test-Path -LiteralPath $StopFile)) {
  if (((Get-Date) - $treeAt).TotalSeconds -ge 5) { $tree = Get-Tree $RootProcessId; $treeAt = Get-Date }
  foreach ($c in Get-NetTCPConnection -ErrorAction SilentlyContinue) {
    if (-not $tree.Contains([int]$c.OwningProcess)) { continue }
    $remote = [string]$c.RemoteAddress
    if ($remote -in @('0.0.0.0', '::', '127.0.0.1', '::1') -or $remote.StartsWith('127.')) { continue }
    $key = "$($c.OwningProcess)|$remote|$($c.RemotePort)"
    if ($seen.ContainsKey($key)) { continue }
    $seen[$key] = $true
    $name = try { [System.Net.Dns]::GetHostEntry($remote).HostName } catch { $null }
    [ordered]@{ utc = (Get-Date).ToUniversalTime().ToString('o'); pid = $c.OwningProcess; remote = $remote; port = $c.RemotePort; state = [string]$c.State; reverseDns = $name } |
      ConvertTo-Json -Compress | Add-Content -LiteralPath $OutFile -Encoding UTF8
  }
  Start-Sleep -Milliseconds $IntervalMs
}
Write-Output "netwatch stopped; $($seen.Count) distinct non-loopback endpoints"
