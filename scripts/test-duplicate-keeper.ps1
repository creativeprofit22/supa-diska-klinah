$ErrorActionPreference = 'Stop'
# Compile before starting the bounded race handshake; cold linking is not a
# keeper-protection timeout. A failed build must never launch the fixture.
& cargo test --manifest-path src-tauri/Cargo.toml -p windows-platform --test duplicate_keeper_process --locked --no-run
if ($LASTEXITCODE -ne 0) { throw "Keeper fixture build failed: $LASTEXITCODE" }
$root = Join-Path ([IO.Path]::GetTempPath()) ('duplicate-keeper-fixture-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
[IO.File]::WriteAllText((Join-Path $root 'fixture-marker'), 'disposable-duplicate-test-v1')
$previous = $env:DUPLICATE_KEEPER_FIXTURE
$env:DUPLICATE_KEEPER_FIXTURE = $root
$process = $null
function Wait-Marker($name) {
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    while (-not (Test-Path (Join-Path $root $name))) {
        if ($process.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw "Fixture exited/timed out before $name" }
        Start-Sleep -Milliseconds 10
    }
}
function Assert-Denied($action, $label) {
    $denied = $false
    try { & $action } catch { $denied = $true }
    if (-not $denied) { throw "Keeper protection failed: $label" }
}
try {
    # Test-only external orchestration; no application/runtime process exception.
    $process = Start-Process cargo -ArgumentList @('test', '--manifest-path', 'src-tauri/Cargo.toml', '-p', 'windows-platform', '--test', 'duplicate_keeper_process', '--locked', '--', '--ignored', '--exact', 'keeper_is_pinned_through_entire_group', '--nocapture') -NoNewWindow -PassThru
    $null = $process.Handle # Retain native process handle for ExitCode in Windows PowerShell.
    $group = Join-Path $root 'group'
    $keeper = Join-Path $group 'keeper'
    foreach ($stage in @(@('ready','probe-one'), @('between','probe-two'), @('completed-held','probe-three'))) {
        Wait-Marker $stage[0]
        Assert-Denied { [IO.File]::Delete($keeper) } 'delete'
        Assert-Denied { [IO.File]::Move($keeper, (Join-Path $group 'replaced')) } 'replace'
        Assert-Denied { [IO.File]::WriteAllBytes($keeper, [byte[]](1,2,3)) } 'write'
        Assert-Denied { [IO.Directory]::Move($group, (Join-Path $root 'moved')) } 'ancestor replace'
        [IO.File]::WriteAllText((Join-Path $root $stage[1]), 'checked')
    }
    Wait-Marker 'released'
    [IO.File]::Delete($keeper)
    [IO.File]::WriteAllText((Join-Path $root 'probe-released'), 'checked')
    if (-not $process.WaitForExit(30000)) { throw 'Fixture did not finish' }
    if ($process.ExitCode -ne 0) { throw "Fixture failed: $($process.ExitCode)" }
    Write-Host 'PASS: another process cannot delete/write/replace keeper or ancestor before, between, or after member removals until guards release.'
} finally {
    if ($null -ne $process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $env:DUPLICATE_KEEPER_FIXTURE = $previous
    Remove-Item -LiteralPath $root -Recurse -Force
}
