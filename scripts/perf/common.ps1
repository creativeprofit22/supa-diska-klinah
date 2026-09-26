# Shared helpers for the performance harness. Dot-source only; no side effects on load.
# Windows PowerShell 5.1 compatible (no ternary, no null-coalescing). No Set-StrictMode here:
# dot-sourcing would leak it into the shared smoke helpers, which probe optional properties.

$script:PerfMarkerName = ".perf-fixture"
$script:PerfMarkerContent = '{"kind":"supa-diska-klinah-perf-fixture","version":1}'

function Get-PerfRepoRoot {
  return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

function New-PerfFixtureDirectory {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (Test-Path -LiteralPath $Path) {
    # Reuse only a directory we own; never adopt an arbitrary existing folder.
    Assert-PerfFixture -Path $Path
    return (Resolve-Path -LiteralPath $Path).Path
  }
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
  $full = (Resolve-Path -LiteralPath $Path).Path
  [IO.File]::WriteAllText((Join-Path $full $script:PerfMarkerName), $script:PerfMarkerContent)
  return $full
}

function Test-PerfFixture {
  param([Parameter(Mandatory = $true)][string]$Path)
  $marker = Join-Path $Path $script:PerfMarkerName
  if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { return $false }
  $item = Get-Item -LiteralPath $Path -Force
  if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { return $false }
  return ([IO.File]::ReadAllText($marker).Trim() -eq $script:PerfMarkerContent)
}

function Assert-PerfFixture {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (-not (Test-PerfFixture -Path $Path)) {
    throw "Refusing to use '$Path': it is not a marked, non-reparse perf fixture."
  }
}

function Remove-PerfFixture {
  param([Parameter(Mandatory = $true)][string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return }
  Assert-PerfFixture -Path $Path
  $full = (Resolve-Path -LiteralPath $Path).Path
  $root = [IO.Path]::GetPathRoot($full)
  if ($full.TrimEnd('\') -eq $root.TrimEnd('\')) { throw "Refusing to remove a volume root." }
  Remove-Item -LiteralPath $full -Recurse -Force
}

function ConvertTo-PerfCanonical {
  param($Value)
  if ($null -eq $Value) { return $null }
  if ($Value -is [string] -or $Value -is [bool] -or $Value -is [ValueType]) { return $Value }
  if ($Value -is [Collections.IDictionary]) {
    $ordered = [ordered]@{}
    foreach ($key in (@($Value.Keys) | ForEach-Object { [string]$_ } | Sort-Object -CaseSensitive)) {
      $ordered[$key] = ConvertTo-PerfCanonical $Value[$key]
    }
    return $ordered
  }
  if ($Value -is [Collections.IEnumerable]) {
    return , @($Value | ForEach-Object { ConvertTo-PerfCanonical $_ })
  }
  $ordered = [ordered]@{}
  foreach ($name in ($Value.PSObject.Properties | ForEach-Object Name | Sort-Object -CaseSensitive)) {
    $ordered[$name] = ConvertTo-PerfCanonical $Value.$name
  }
  return $ordered
}

function Write-PerfJson {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)]$Value
  )
  $json = ConvertTo-Json -InputObject (ConvertTo-PerfCanonical $Value) -Depth 32
  [IO.File]::WriteAllText($Path, $json + "`n", (New-Object Text.UTF8Encoding($false)))
}

function Get-PerfStats {
  param([Parameter(Mandatory = $true)][double[]]$Values)
  $sorted = @($Values | Sort-Object)
  $count = $sorted.Count
  if ($count -eq 0) { throw "No samples." }
  $middle = [int][Math]::Floor($count / 2)
  if ($count % 2 -eq 1) { $median = $sorted[$middle] } else { $median = ($sorted[$middle - 1] + $sorted[$middle]) / 2 }
  # Nearest-rank percentile: deterministic and never interpolates past a real sample.
  $p90 = $sorted[[int][Math]::Ceiling(0.9 * $count) - 1]
  return [ordered]@{ count = $count; min = $sorted[0]; median = $median; p90 = $p90; max = $sorted[$count - 1] }
}

function New-PerfRunDirectory {
  param([Parameter(Mandatory = $true)][string]$Kind)
  $stamp = [DateTime]::UtcNow.ToString("yyyyMMdd'T'HHmmss'Z'")
  $dir = Join-Path (Get-PerfRepoRoot) ".gg\perf-artifacts\$stamp-$Kind"
  New-Item -ItemType Directory -Path $dir -Force | Out-Null
  return $dir
}

function Get-PerfCommandVersion {
  param([string]$Command, [string[]]$Arguments)
  try {
    $output = & $Command @Arguments 2>$null
    return (@($output) -join " ").Trim()
  }
  catch { return $null }
}

function Get-PerfCommandVersionIn {
  param([string]$Directory, [string]$Command, [string[]]$Arguments)
  Push-Location $Directory
  try { return Get-PerfCommandVersion $Command $Arguments }
  finally { Pop-Location }
}

function Get-PerfVolumeInfo {
  param([Parameter(Mandatory = $true)][string]$Path)
  $full = [IO.Path]::GetFullPath($Path)
  $letter = $full.Substring(0, 1)
  $info = [ordered]@{ driveLetter = $letter; mediaType = "Unknown"; busType = "Unknown"; model = $null; freeBytes = $null; sizeBytes = $null }
  try {
    $volume = Get-Volume -DriveLetter $letter
    $info.freeBytes = [int64]$volume.SizeRemaining
    $info.sizeBytes = [int64]$volume.Size
    $disk = Get-Partition -DriveLetter $letter | Get-Disk
    $physical = Get-PhysicalDisk | Where-Object { [string]$_.DeviceId -eq [string]$disk.Number } | Select-Object -First 1
    if ($physical) {
      $info.mediaType = [string]$physical.MediaType
      $info.busType = [string]$physical.BusType
      $info.model = [string]$physical.FriendlyName
    }
  }
  catch { $info.error = $_.Exception.Message }
  return $info
}

function Get-PerfEnvironment {
  param([string[]]$VolumePaths = @())
  $repo = Get-PerfRepoRoot
  $os = Get-CimInstance Win32_OperatingSystem
  $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
  $powerPlan = $null
  try { $powerPlan = ((powercfg.exe /getactivescheme) -join " ").Trim() } catch { }
  $defender = $null
  try { $defender = [bool](Get-MpComputerStatus).RealTimeProtectionEnabled } catch { }
  $node = Get-Command node -ErrorAction SilentlyContinue
  $volumes = @($VolumePaths | ForEach-Object { Get-PerfVolumeInfo -Path $_ })
  $dirty = (git -C $repo status --porcelain 2>$null)
  return [ordered]@{
    gitSha = (git -C $repo rev-parse HEAD 2>$null)
    gitDirty = [bool]$dirty
    os = [ordered]@{ caption = $os.Caption; version = $os.Version; build = $os.BuildNumber }
    cpu = [ordered]@{ model = $cpu.Name.Trim(); cores = [int]$cpu.NumberOfCores; logicalProcessors = [int]$cpu.NumberOfLogicalProcessors }
    ramBytes = [int64]$os.TotalVisibleMemorySize * 1024
    powerPlan = $powerPlan
    defenderRealTime = $defender
    toolchain = [ordered]@{
      nodePath = $(if ($node) { $node.Source } else { $null })
      node = Get-PerfCommandVersion node @("--version")
      pnpm = Get-PerfCommandVersion pnpm @("--version")
      # Run inside src-tauri so rust-toolchain.toml selects the toolchain the builds use.
      rustc = Get-PerfCommandVersionIn (Join-Path $repo "src-tauri") rustc @("--version")
      cargo = Get-PerfCommandVersionIn (Join-Path $repo "src-tauri") cargo @("--version")
    }
    cargoIncremental = $env:CARGO_INCREMENTAL
    rustcWrapper = $env:RUSTC_WRAPPER
    volumes = $volumes
  }
}

function Assert-PerfQuietToolchain {
  # Concurrent cargo/rustc share the package-cache lock and CPU; timings would be meaningless.
  param([int]$WaitSeconds = 900)
  $deadline = [DateTime]::UtcNow.AddSeconds($WaitSeconds)
  $busy = @(Get-Process -Name cargo, rustc -ErrorAction SilentlyContinue)
  while ($busy.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Seconds 10
    $busy = @(Get-Process -Name cargo, rustc -ErrorAction SilentlyContinue)
  }
  if ($busy.Count -gt 0) {
    throw "Another cargo/rustc process is running (PIDs $((@($busy | ForEach-Object Id)) -join ', ')); wait for it before benchmarking."
  }
}

function Get-PerfForeignBuildCount {
  # cargo/rustc processes that are NOT descendants of RootPid (another project's build).
  param([Parameter(Mandatory = $true)][int]$RootPid)
  $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name)
  $children = @{}
  foreach ($p in $all) {
    $parent = [int]$p.ParentProcessId
    if (-not $children.ContainsKey($parent)) { $children[$parent] = New-Object Collections.Generic.List[int] }
    $children[$parent].Add([int]$p.ProcessId)
  }
  $ours = New-Object 'Collections.Generic.HashSet[int]'
  $queue = New-Object Collections.Generic.Queue[int]
  $queue.Enqueue($RootPid)
  while ($queue.Count -gt 0) {
    $id = $queue.Dequeue()
    if (-not $ours.Add($id)) { continue }
    if ($children.ContainsKey($id)) { foreach ($child in $children[$id]) { $queue.Enqueue($child) } }
  }
  return @($all | Where-Object { $_.Name -in 'cargo.exe', 'rustc.exe' -and -not $ours.Contains([int]$_.ProcessId) }).Count
}

function Start-PerfContentionMonitor {
  # Samples foreign builds every 5 s in a background job for the duration of a run.
  $common = Join-Path $PSScriptRoot "common.ps1"
  return Start-Job -ArgumentList $PID, $common -ScriptBlock {
    param($RootPid, $Common)
    . $Common
    while ($true) {
      $count = Get-PerfForeignBuildCount -RootPid $RootPid
      if ($count -gt 0) { "{0}: {1} foreign cargo/rustc" -f [DateTime]::UtcNow.ToString('o'), $count }
      Start-Sleep -Seconds 5
    }
  }
}

function Stop-PerfContentionMonitor {
  param([Parameter(Mandatory = $true)]$Job)
  Stop-Job -Job $Job
  $samples = @(Receive-Job -Job $Job -ErrorAction SilentlyContinue | ForEach-Object { [string]$_ })
  Remove-Job -Job $Job -Force
  return , $samples
}

function Get-PerfSleepEvents {
  # Kernel-Power 42 (sleep) / 107 (resume): a sample spanning these is invalid, not slow.
  param([Parameter(Mandatory = $true)][DateTime]$SinceUtc)
  try {
    return @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Kernel-Power'; Id = 42, 107; StartTime = $SinceUtc.ToLocalTime() } -ErrorAction Stop |
      Sort-Object TimeCreated | ForEach-Object { [ordered]@{ id = $_.Id; utc = $_.TimeCreated.ToUniversalTime().ToString('o') } })
  }
  catch { return @() }
}

function Invoke-PerfTimed {
  # Runs a script block and returns elapsed milliseconds; throws if it throws.
  param([Parameter(Mandatory = $true)][scriptblock]$Action)
  $watch = [Diagnostics.Stopwatch]::StartNew()
  & $Action | Out-Null
  $watch.Stop()
  return [double]$watch.Elapsed.TotalMilliseconds
}

function Invoke-PerfNative {
  # Runs a native command, streaming output to a log, and fails closed on non-zero exit.
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [string[]]$Arguments = @(),
    [Parameter(Mandatory = $true)][string]$LogPath,
    [string]$WorkingDirectory = (Get-PerfRepoRoot)
  )
  Push-Location $WorkingDirectory
  try {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    # Stringify records so the log stays plain UTF-8 without PowerShell error wrapping.
    & $FilePath @Arguments 2>&1 | ForEach-Object { "$_" } | Out-File -LiteralPath $LogPath -Append -Encoding utf8
    $code = $LASTEXITCODE
    $ErrorActionPreference = $previous
    if ($code -ne 0) { throw "$FilePath $($Arguments -join ' ') exited $code (see $LogPath)" }
  }
  finally { Pop-Location }
}
