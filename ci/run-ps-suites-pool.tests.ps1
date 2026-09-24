# #1053 item 8: the PowerShell suites run in a pool, and this file is what the pool had to earn.
#
# The stage was 658.2 s on the gate that first measured the rest of #1053 -- larger than `workspace
# tests` -- purely because `ci/run-ps-suites.ps1` walked a `foreach`. Running the same 48 children
# six at a time took it to ~365-490 s, and the remaining floor was one suite:
# `merge-proof.tests.ps1` (retired 2026-09-24) alone measured 487.7 s against a pool wall of
# 488.6 s. The pool was therefore already optimal; what was left was inside that suite, not in
# this scheduler.
#
# WHY THIS FILE EXISTS RATHER THAN A GREEN RUN. A pool converts a verdict into an aggregation, and
# every way an aggregation can lie is quiet. The first prototype of this change returned an EMPTY
# exit code for all 46 children and reported the run GREEN -- a total, flattering, silent failure.
# The cells below are the four ways that can happen, each with a stub that makes it happen.
#
# THE FIXTURE, in the shape `ci/gate-stage-reddens.tests.ps1` already uses: the runner is copied
# into a throwaway directory beside stub suites whose names are read out of the runner's own pinned
# inventory, because the inventory check runs before any child does and would otherwise refuse the
# stubs for being unpinned -- reddening this suite for the wrong reason.
#
# ONE ARM IS A SOURCE CLAIM AND SAYS SO. An exit code that reads back `$null` cannot be staged from
# outside the process (a stub can choose its code, not whether Windows reports it), so the cell for
# it reads the branch. The TIMEOUT cell exercises the same mechanism behaviourally -- both put a
# STRING into the ledger where an integer belongs, and both must land in HARNESS-BROKE -- so the
# string-typed path is proven by execution and only the choice of which string is proven by reading.
#
# Same homegrown PASS/FAIL harness as the sibling gate suites; this repository carries no Pester.
# Exit codes: 0 all passed, 1 an assertion failed, 2 the harness could not vouch for the run.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   1  ARRANGEMENT: the runner and its pinned inventory are readable, and the fixture drives the real file
#   1  CONTROL: every stub exiting 0 gives exit 0 -- without it, the reds below prove nothing
#   1  a stub exiting 1 gives exit 1        1  and names that suite
#   1  a stub exiting 2 gives exit 2        1  and the HARNESS-BROKE wording survives
#   1  2 BEATS 1 when both happen in one pool   1  and BOTH names appear
#   1  a stub that outlives the deadline is KILLED and gives exit 2, not 1 and not 0
#   1  and the refusal names the suite and the ceiling it was given
#   1  a timed descendant starts before the wrapper's deadline
#   1  the timed wrapper is actually refused by the pool
#   1  the timed descendant's own process exited before its release signal
#   1  the timeout also kills a descendant before it can outlive the suite wrapper
#   1  a wrapper that exits during taskkill's race window is already safely stopped
#   1  a failed tree snapshot stays a cleanup refusal even when the wrapper kill succeeds
#   1  a CIM PID whose creation identity changed is rejected before cleanup can kill it
#   1  SOURCE: an unreadable exit code is mapped to 'unreadable', not to 0
#   1  and 'unreadable' lands in HARNESS-BROKE rather than in the failed list
#   1  the ledger is set-compared against the discovered set in BOTH directions
#   1  stderr-only output survives into the replay
#   1  the replay carries a per-suite wall time
#   1  a briefly locked transcript is retried and read after its writer releases it
#   1  a transcript that stays locked fails closed instead of hanging or disappearing
#   1  an unparseable throttle runs SERIALLY, never wider
#   1  and says so
#   1  a missing fixture pid is a bounded harness result, not an exception
#   1  a child invocation reports the real missing-PID startup failure
#   1  that child invocation vouches wrapper and descendant cleanup
#   1  a Start-Process suite child resolves native Windows PowerShell utility commands
$ExpectedAssertionCount = 30

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0
$script:fixtureBroken = $false

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$runnerPath = Join-Path $PSScriptRoot 'run-ps-suites.ps1'
$runnerText = [System.IO.File]::ReadAllText($runnerPath)
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Wait-ForPidFile {
    param(
        [Parameter(Mandatory)] [string] $Path,
        # Loaded gate hosts have measured startup above ten seconds. This is still a bounded
        # admission window; the normal path returns as soon as a complete PID is readable. The
        # deadline bounds polling and retry admission; a synchronous ReadAllText call already in
        # progress is not interruptible by this script.
        [int] $TimeoutMilliseconds = 30000
    )

    $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max(0, $TimeoutMilliseconds))
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            if (Test-Path -LiteralPath $Path) {
                $raw = [System.IO.File]::ReadAllText($Path).Trim()
                $parsedPid = 0
                if ([int]::TryParse($raw, [ref] $parsedPid) -and $parsedPid -gt 0) {
                    return [pscustomobject]@{ Succeeded = $true; ProcessId = $parsedPid; Reason = $null }
                }
            }
        } catch [System.IO.IOException] {
            # A writer can publish the file while it is still being flushed. Keep polling until
            # the bounded admission window closes instead of turning that race into an exception.
        } catch {
            return [pscustomobject]@{ Succeeded = $false; ProcessId = $null; Reason = 'pid file could not be read' }
        }
        $remainingMilliseconds = [int][math]::Max(1, [math]::Ceiling(($deadline - [DateTime]::UtcNow).TotalMilliseconds))
        Start-Sleep -Milliseconds ([math]::Min(50, $remainingMilliseconds))
    }
    return [pscustomobject]@{
        Succeeded = $false
        ProcessId = $null
        Reason = "fixture did not publish a readable pid within $TimeoutMilliseconds ms"
    }
}

# The timeout branch first observes HasExited=false and only then launches taskkill. Reproduce the
# race between those two observations: the real wrapper exits while a controlled taskkill boundary
# reports PID-not-found (128), but its long-lived child remains. The root killer is controlled; the
# descendant kill is real, so success requires the production helper to remember and reap the tree.
$stopStart = $runnerText.IndexOf('function Get-SuiteProcessTreeSnapshot', [System.StringComparison]::Ordinal)
$stopEnd = $runnerText.IndexOf('$dispatchOrder', $stopStart, [System.StringComparison]::Ordinal)
if ($stopStart -ge 0 -and $stopEnd -gt $stopStart) {
    Invoke-Expression $runnerText.Substring($stopStart, $stopEnd - $stopStart)
}
$racePidFile = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-race-child-$([guid]::NewGuid().ToString('N')).pid"
$raceReleaseFile = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-race-release-$([guid]::NewGuid().ToString('N')).signal"
$raceStartedFile = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-race-started-$([guid]::NewGuid().ToString('N')).signal"
$raceSurvivedFile = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-race-survived-$([guid]::NewGuid().ToString('N')).signal"
$startupProbe = $env:GRAPHHELM_PS_POOL_PID_STARTUP_PROBE -eq '1'
# Hold the wrapper behind an explicit release file. The old 400 ms sleep made this cell a timing
# lottery: on a loaded host the wrapper could exit before Stop-SuiteProcessTree reached taskkill,
# so the intended PID-not-found branch was never exercised. The fake taskkill below releases the
# wrapper only after the process-tree snapshot has run, preserving the race shape with no clock race.
$raceCommand = if ($startupProbe) {
    "`$child = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30; Set-Content -LiteralPath `"$raceSurvivedFile`" -Value survived'; Set-Content -LiteralPath '$raceStartedFile' -Value started; while (-not (Test-Path -LiteralPath '$raceReleaseFile')) { Start-Sleep -Milliseconds 25 }"
} else {
    "`$child = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30'; Set-Content -LiteralPath '$racePidFile' -Value `$child.Id; while (-not (Test-Path -LiteralPath '$raceReleaseFile')) { Start-Sleep -Milliseconds 25 }"
}
$raceProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-NonInteractive', '-Command', $raceCommand

# A missing PID is a harness failure, not a PowerShell exception. This fixture keeps the regression
# deterministic: the wrapper starts a child, exits before publishing the production PID file, and
# the child would leave a marker if the real remembered-tree cleanup failed after wrapper exit.
$missingPidPath = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-missing-$([guid]::NewGuid().ToString('N')).pid"
$missingChildPidPath = "$missingPidPath.child"
$missingStartedPath = "$missingPidPath.started"
$missingSurvivedPath = "$missingPidPath.survived"
$missingChildScriptPath = "$missingPidPath.child.ps1"
$missingWrapperScriptPath = "$missingPidPath.wrapper.ps1"
[System.IO.File]::WriteAllText($missingChildScriptPath, @"
[System.IO.File]::WriteAllText('$($missingChildPidPath.Replace("'", "''"))', [string]`$PID)
Start-Sleep -Seconds 3
[System.IO.File]::WriteAllText('$($missingSurvivedPath.Replace("'", "''"))', 'descendant survived')
"@, $utf8NoBom)
[System.IO.File]::WriteAllText($missingWrapperScriptPath, @"
`$null = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-NonInteractive', '-File', '$($missingChildScriptPath.Replace("'", "''"))'
[System.IO.File]::WriteAllText('$($missingStartedPath.Replace("'", "''"))', 'started')
exit 0
"@, $utf8NoBom)
$missingTreeProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-NonInteractive', '-File', $missingWrapperScriptPath
$missingTreeDeadline = [DateTime]::UtcNow.AddSeconds(10)
while (-not (Test-Path -LiteralPath $missingStartedPath) -and [DateTime]::UtcNow -lt $missingTreeDeadline) {
    Start-Sleep -Milliseconds 25
}
$missingTreeStarted = Test-Path -LiteralPath $missingStartedPath
$missingChildReady = Wait-ForPidFile -Path $missingChildPidPath -TimeoutMilliseconds 10000
$missingChildProcess = $null
$missingChildExited = $false
if ($missingChildReady.Succeeded) {
    try {
        $missingChildProcess = Microsoft.PowerShell.Management\Get-Process -Id $missingChildReady.ProcessId
        $null = $missingChildProcess.Handle
    } catch {
        $script:fixtureBroken = $true
    }
}
$missingWrapperExited = $missingTreeProcess.WaitForExit(5000)
$missingTreeStopped = $false
if ($missingTreeStarted -and $missingChildReady.Succeeded -and $missingWrapperExited -and $null -ne $missingChildProcess) {
    $missingTreeStopped = Stop-SuiteProcessTree -Process $missingTreeProcess
}
if ($null -ne $missingChildProcess) {
    try { $missingChildExited = $missingChildProcess.WaitForExit(5000) } catch { $missingChildExited = $false }
}
Start-Sleep -Seconds 1
$missingTreeSurvived = Test-Path -LiteralPath $missingSurvivedPath
try {
    if ($null -ne $missingChildProcess) {
        if (-not $missingChildExited -and -not $missingChildProcess.HasExited) {
            try { taskkill.exe /PID $missingChildReady.ProcessId /T /F 2>$null | Out-Null } catch { }
            $missingChildExited = $missingChildProcess.WaitForExit(5000)
        }
    }
} catch { $script:fixtureBroken = $true }
$missingPidPublished = Test-Path -LiteralPath $missingPidPath
Remove-Item -LiteralPath $missingPidPath, $missingChildPidPath, $missingStartedPath, $missingSurvivedPath, `
    $missingChildScriptPath, $missingWrapperScriptPath -Force -ErrorAction SilentlyContinue
$missingPidProbe = [pscustomobject]@{
    Succeeded = $missingTreeStarted -and $missingChildReady.Succeeded -and $missingWrapperExited -and $missingChildExited
    ProcessId = $missingChildReady.ProcessId
    Published = $missingPidPublished
    ChildExited = $missingChildExited
    Reason = if ($missingTreeStopped -and -not $missingTreeSurvived) { $null } else { 'missing PID child was not vouched for and reaped' }
}

$raceChildId = $null
$raceChildProcess = $null
$raceStopped = $false
$raceChildExited = $false
$raceCleanupVouched = $false
try {
    $racePidTimeout = if ($startupProbe) { 3000 } else { 30000 }
    $raceReady = Wait-ForPidFile -Path $racePidFile -TimeoutMilliseconds $racePidTimeout
    if (-not $raceReady.Succeeded) {
        $script:fixtureBroken = $true
        Write-Host "HARNESS-BROKE: $($raceReady.Reason)" -ForegroundColor Magenta
    } else {
        try {
            $raceChildId = $raceReady.ProcessId
            $raceChildProcess = Microsoft.PowerShell.Management\Get-Process -Id $raceChildId
            $null = $raceChildProcess.Handle
            $script:raceProcess = $raceProcess
            function Start-Process {
                param([string] $FilePath, [switch] $PassThru, [System.Diagnostics.ProcessWindowStyle] $WindowStyle, [object[]] $ArgumentList)
                if ([string]$ArgumentList[1] -eq [string]$script:raceProcess.Id) {
                    Set-Content -LiteralPath $raceReleaseFile -Value 'release' -Encoding ASCII
                    $null = $script:raceProcess.WaitForExit(5000)
                    $fake = [pscustomobject]@{ Handle = 1; ExitCode = 128 }
                    $fake | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($Milliseconds) return $true }
                    $fake | Add-Member -MemberType ScriptMethod -Name Kill -Value { }
                    return $fake
                }
                return Microsoft.PowerShell.Management\Start-Process -FilePath $FilePath -PassThru -WindowStyle $WindowStyle -ArgumentList $ArgumentList
            }
            $raceStopped = Stop-SuiteProcessTree -Process $raceProcess
        } finally {
            Remove-Item function:Start-Process -Force -ErrorAction SilentlyContinue
        }
        $raceChildExited = try { $raceChildProcess.HasExited } catch { $false }
        if (-not $raceChildExited) {
            # Cleanup only. The process may exit between the liveness read and taskkill; that benign race
            # must not abort the harness before the assertion reports the production result.
            try { taskkill.exe /PID $raceChildId /T /F 2>$null | Out-Null } catch { }
        }
    }
} finally {
    # Always run the real remembered-tree cleanup, including after the wrapper has already exited.
    # A taskkill guarded by HasExited would skip the only operation that can vouch an unknown child.
    try {
        if ($null -ne $raceProcess) {
            $raceCleanupVouched = Stop-SuiteProcessTree -Process $raceProcess
            if (-not $raceCleanupVouched) {
                $script:fixtureBroken = $true
            }
        }
    } catch { $script:fixtureBroken = $true }
    Remove-Item -LiteralPath $racePidFile -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $raceReleaseFile -Force -ErrorAction SilentlyContinue
}
Assert-True ($missingPidProbe.Succeeded -and -not $missingPidProbe.Published -and $missingTreeStopped -and $missingPidProbe.ChildExited -and -not $missingTreeSurvived) `
    'a wrapper that exits after starting a child without publishing its pid is cleaned through the remembered tree, with no unclassified Get-Content exception'
Assert-True ($raceStopped -and $raceChildExited) `
    "a wrapper that exits while taskkill reports PID-not-found still has every remembered descendant stopped (stopped=$raceStopped childExited=$raceChildExited)"

if ($startupProbe) {
    $probeOutcomePath = $env:GRAPHHELM_PS_POOL_PID_STARTUP_OUTCOME
    if (-not [string]::IsNullOrWhiteSpace($probeOutcomePath)) {
        $probeWrapperExited = try { $raceProcess.HasExited } catch { $false }
        $probeChildStarted = Test-Path -LiteralPath $raceStartedFile
        $probeChildSurvived = Test-Path -LiteralPath $raceSurvivedFile
        [System.IO.File]::WriteAllText($probeOutcomePath, "cleanup=$raceCleanupVouched`nwrapperExited=$probeWrapperExited`nchildStarted=$probeChildStarted`nchildSurvived=$probeChildSurvived`nfixtureBroken=$script:fixtureBroken", $utf8NoBom)
    }
    Remove-Item -LiteralPath $raceStartedFile, $raceSurvivedFile -Force -ErrorAction SilentlyContinue
    if ($script:fixtureBroken) { exit 2 }
    exit 0
}

# Invoke this same suite through its real PID-admission branch. The child deliberately omits the
# production PID publication, so a passing result requires the actual startup failure wiring to
# report exit 2 and the actual finally block to vouch cleanup.
$probeOutcomePath = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-pool-startup-outcome-$([guid]::NewGuid().ToString('N')).txt"
$previousProbe = $env:GRAPHHELM_PS_POOL_PID_STARTUP_PROBE
$previousOutcome = $env:GRAPHHELM_PS_POOL_PID_STARTUP_OUTCOME
$env:GRAPHHELM_PS_POOL_PID_STARTUP_PROBE = '1'
$env:GRAPHHELM_PS_POOL_PID_STARTUP_OUTCOME = $probeOutcomePath
try {
    $probeProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
        -ArgumentList '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath
    # #1256: the child re-runs every cell above the probe branch before it exits -- 10.2 s measured
    # on an idle box -- so 15 s failed under gate load with exited=False. This bound is failure-only
    # (a green run returns as soon as the child exits), so it is sized for load, not for the idle box.
    $probeExited = $probeProcess.WaitForExit(120000)
    if ($probeExited) {
        $probeProcess.Refresh()
    } else {
        # Never leave the probe child (and whatever it started) running past its verdict.
        try { taskkill.exe /PID $probeProcess.Id /T /F 2>$null | Out-Null } catch { }
    }
    $probeExitCode = if ($probeExited) { $probeProcess.ExitCode } else { $null }
    $probeOutcome = if (Test-Path -LiteralPath $probeOutcomePath) { [System.IO.File]::ReadAllText($probeOutcomePath) } else { '' }
} finally {
    $env:GRAPHHELM_PS_POOL_PID_STARTUP_PROBE = $previousProbe
    $env:GRAPHHELM_PS_POOL_PID_STARTUP_OUTCOME = $previousOutcome
}
Assert-True ($probeExited -and $probeExitCode -eq 2 -and $probeOutcome -match 'fixtureBroken=True') `
    "a child invocation with missing PID publication reports bounded startup failure as exit 2 (exited=$probeExited exit=$probeExitCode)"
Assert-True ($probeOutcome -match 'cleanup=True' -and $probeOutcome -match 'wrapperExited=True' -and $probeOutcome -match 'childStarted=True' -and $probeOutcome -match 'childSurvived=False') `
    "the same child invocation vouches wrapper and descendant cleanup (outcome='$probeOutcome')"
Remove-Item -LiteralPath $probeOutcomePath, $raceStartedFile, $raceSurvivedFile -Force -ErrorAction SilentlyContinue

# Stage a PID whose CIM identity and current Process identity disagree. Numeric equality alone must
# not authorize a kill: it can describe a descendant that exited and an unrelated process that
# inherited the same number before Get-Process pinned the handle.
$identityProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 30'
$script:identityProcess = $identityProcess
$script:identityRoot = 424242
function Get-CimInstance {
    param([string] $ClassName, [string[]] $Property, [object] $ErrorAction)
    return @(
        [pscustomobject]@{ ProcessId = $script:identityRoot; ParentProcessId = 0; CreationDate = [DateTime]::UtcNow.AddMinutes(-2) },
        [pscustomobject]@{ ProcessId = $script:identityProcess.Id; ParentProcessId = $script:identityRoot; CreationDate = [DateTime]::UtcNow.AddMinutes(-1) }
    )
}
function Get-Process {
    param([int] $Id, [object] $ErrorAction)
    return Microsoft.PowerShell.Management\Get-Process -Id $Id -ErrorAction SilentlyContinue
}
try {
    $identitySnapshot = Get-SuiteProcessTreeSnapshot -RootProcessId $script:identityRoot
} finally {
    Remove-Item function:Get-CimInstance -Force
    Remove-Item function:Get-Process -Force
    try { $identityProcess.Kill() } catch { }
    $null = $identityProcess.WaitForExit(5000)
}
Assert-True (-not $identitySnapshot.Succeeded) `
    'a reused PID is rejected when the CIM creation identity differs from the handle-backed Process start time'

# If the initial tree snapshot fails, killing only the wrapper proves nothing about descendants.
# The cleanup result must stay false even when taskkill reports that the wrapper itself was killed.
$snapshotFailureProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 30'
$script:snapshotFailureProcess = $snapshotFailureProcess
function Get-SuiteProcessTreeSnapshot {
    param([int] $RootProcessId)
    return [pscustomobject]@{ Succeeded = $false; Descendants = @() }
}
function Invoke-SuiteTaskkill {
    param([int] $ProcessId)
    try { $script:snapshotFailureProcess.Kill() } catch { }
    $null = $script:snapshotFailureProcess.WaitForExit(5000)
    return $true
}
try {
    $snapshotFailureStopped = Stop-SuiteProcessTree -Process $snapshotFailureProcess
} finally {
    Remove-Item function:Get-SuiteProcessTreeSnapshot -Force
    Remove-Item function:Invoke-SuiteTaskkill -Force
}
Assert-True (-not $snapshotFailureStopped) `
    'a failed descendant snapshot fails closed even when the wrapper kill succeeds, because surviving children were never observed'

# The stub names come from the runner's own inventory, never retyped: a hand-written name that
# drifted from the pin would make every cell below red for the inventory check instead of for the
# thing it is testing.
$pinned = @([regex]::Matches($runnerText, "(?m)^\s*'([a-z0-9-]+\.tests\.ps1)',?\s*$") |
        ForEach-Object { $_.Groups[1].Value })

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-pool-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

function Invoke-Pool {
    <#
        Writes one stub per PINNED name -- so the inventory check passes -- giving each the exit
        code, sleep or stream behaviour the caller asked for, then runs the real runner over them.
    #>
    param(
        [hashtable] $ExitCodes = @{},
        [hashtable] $SleepSeconds = @{},
        [hashtable] $DescendantMarkers = @{},
        [hashtable] $StderrText = @{},
        [string] $Throttle = '6',
        [string] $TimeoutSeconds = '1800'
    )
    Get-ChildItem -LiteralPath $fixtureRoot -Filter '*.tests.ps1' -File -ErrorAction SilentlyContinue | Remove-Item -Force
    Copy-Item -LiteralPath $runnerPath -Destination (Join-Path $fixtureRoot 'run-ps-suites.ps1') -Force
    foreach ($name in $pinned) {
        $lines = New-Object System.Collections.Generic.List[string]
        if ($DescendantMarkers.ContainsKey($name)) {
            $marker = [string]$DescendantMarkers[$name]
            $childPath = Join-Path $fixtureRoot "$name.child.ps1"
            $readyPath = "$marker.ready"
            $releasePath = "$marker.release"
            # The child confirms it has started before the timeout can test process-tree cleanup.
            # A release signal delays the survival marker until after the pool has reported its
            # verdict, so a longer startup allowance cannot make a healthy child mark survival.
            $child = @"
[System.IO.File]::WriteAllText('$($readyPath.Replace("'", "''"))', 'ready')
`$deadline = [DateTime]::UtcNow.AddSeconds(180)
while (-not (Test-Path -LiteralPath '$($releasePath.Replace("'", "''"))') -and [DateTime]::UtcNow -lt `$deadline) { Start-Sleep -Milliseconds 50 }
if (Test-Path -LiteralPath '$($releasePath.Replace("'", "''"))') {
    Start-Sleep -Seconds 3
    [System.IO.File]::WriteAllText('$($marker.Replace("'", "''"))', 'descendant survived')
}
"@
            [System.IO.File]::WriteAllText($childPath, $child, $utf8NoBom)
            $lines.Add("`$child = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', '$($childPath.Replace("'", "''"))'")
            $lines.Add("[System.IO.File]::WriteAllText('$($marker.Replace("'", "''")).pid', [string]`$child.Id)")
        }
        if ($SleepSeconds.ContainsKey($name)) { $lines.Add("Start-Sleep -Seconds $($SleepSeconds[$name])") }
        if ($StderrText.ContainsKey($name)) { $lines.Add("[Console]::Error.WriteLine('$($StderrText[$name])')") }
        $lines.Add("Write-Host 'stub $name speaking'")
        $code = 0
        if ($ExitCodes.ContainsKey($name)) { $code = $ExitCodes[$name] }
        $lines.Add("exit $code")
        [System.IO.File]::WriteAllText((Join-Path $fixtureRoot $name), ($lines -join [Environment]::NewLine), $utf8NoBom)
    }
    $previousThrottle = $env:GRAPHHELM_PS_SUITES_THROTTLE
    $previousTimeout = $env:GRAPHHELM_PS_SUITE_TIMEOUT_SECONDS
    $env:GRAPHHELM_PS_SUITES_THROTTLE = $Throttle
    $env:GRAPHHELM_PS_SUITE_TIMEOUT_SECONDS = $TimeoutSeconds
    try {
        $out = @(& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass `
                -File (Join-Path $fixtureRoot 'run-ps-suites.ps1') 2>&1 | ForEach-Object { [string]$_ })
        return [ordered]@{ exitCode = $LASTEXITCODE; text = ($out -join "`n") }
    } finally {
        $env:GRAPHHELM_PS_SUITES_THROTTLE = $previousThrottle
        $env:GRAPHHELM_PS_SUITE_TIMEOUT_SECONDS = $previousTimeout
    }
}

try {
    Write-Host ''
    Write-Host '-- the fixture drives the real runner --' -ForegroundColor Cyan
    Assert-True ($pinned.Count -ge 10 -and (Test-Path -LiteralPath $runnerPath)) `
        "ARRANGEMENT: $($pinned.Count) pinned suite names read out of the runner itself, so the stubs cannot drift from the inventory check that runs before them"

    $green = Invoke-Pool
    Assert-True ($green.exitCode -eq 0) `
        "CONTROL: a pool where every stub exits 0 gives exit 0 (got $($green.exitCode)) -- every red below is measured against this"

    # A pwsh parent can pass its PowerShell Core module directory through Start-Process into a
    # Windows PowerShell child. The Core Utility module shadows the Windows one, so Get-FileHash
    # disappears. Launch the copied runner through the same Start-Process boundary as gate.ps1
    # and require an actual suite child to resolve the native command.
    $moduleStub = @'
$ErrorActionPreference = 'Stop'
$command = Get-Command Get-FileHash -ErrorAction Stop
if ($command.Module.Path -notmatch '[\\/]WindowsPowerShell[\\/]') { exit 1 }
Write-Host 'NATIVE-WINDOWS-UTILITY'
exit 0
'@
    [System.IO.File]::WriteAllText((Join-Path $fixtureRoot $pinned[0]), $moduleStub, $utf8NoBom)
    $moduleOut = Join-Path $fixtureRoot 'module-probe.out'
    $moduleErr = Join-Path $fixtureRoot 'module-probe.err'
    $modulePathBeforeProbe = [Environment]::GetEnvironmentVariable('PSModulePath', 'Process')
    try {
        # A Windows PowerShell parent normally repairs its own path. Recreate the Core-first
        # path explicitly so this regression stays observable from either parent host.
        $pwshCommand = Get-Command pwsh -ErrorAction SilentlyContinue
        if ($null -ne $pwshCommand) {
            $coreModules = Join-Path (Split-Path $pwshCommand.Source -Parent) 'Modules'
            if (Test-Path -LiteralPath (Join-Path $coreModules 'Microsoft.PowerShell.Utility')) {
                [Environment]::SetEnvironmentVariable('PSModulePath', "$coreModules;$modulePathBeforeProbe", 'Process')
            }
        }
        $moduleProcess = Start-Process -FilePath 'powershell.exe' -PassThru -Wait -NoNewWindow `
            -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $fixtureRoot 'run-ps-suites.ps1')) `
            -RedirectStandardOutput $moduleOut -RedirectStandardError $moduleErr
    } finally {
        [Environment]::SetEnvironmentVariable('PSModulePath', $modulePathBeforeProbe, 'Process')
    }
    $moduleText = [System.IO.File]::ReadAllText($moduleOut)
    Assert-True ($moduleProcess.ExitCode -eq 0 -and $moduleText.Contains('NATIVE-WINDOWS-UTILITY')) `
        'a real Start-Process suite child resolves the native Windows PowerShell Utility module'

    Write-Host ''
    Write-Host '-- the three states survive aggregation --' -ForegroundColor Cyan
    $failing = $pinned[0]
    $one = Invoke-Pool -ExitCodes @{ $failing = 1 }
    Assert-True ($one.exitCode -eq 1) "a stub exiting 1 gives exit 1 (got $($one.exitCode))"
    Assert-True ($one.text -match [regex]::Escape($failing)) "and the refusal names $failing"

    $broke = $pinned[1]
    $two = Invoke-Pool -ExitCodes @{ $broke = 2 }
    Assert-True ($two.exitCode -eq 2) "a stub exiting 2 gives exit 2 (got $($two.exitCode))"
    Assert-True ($two.text -match 'HARNESS-BROKE in:') `
        'and the HARNESS-BROKE wording survives -- gate-background-stage-evidence asserts that sentence as TEXT'

    # 2 BEATS 1, and the serial loop got that for free from ordering. A pool has to state it.
    $both = Invoke-Pool -ExitCodes @{ $failing = 1; $broke = 2 }
    Assert-True ($both.exitCode -eq 2) `
        "a pool holding BOTH a failure and a harness break exits 2, not 1 (got $($both.exitCode)) -- the third state does not collapse into the second"
    Assert-True ($both.text -match [regex]::Escape($failing) -and $both.text -match [regex]::Escape($broke)) `
        'and both suites are named, so the one that lost the precedence contest is still visible'

    Write-Host ''
    Write-Host '-- a suite with no colour is a refusal, not a pass --' -ForegroundColor Cyan
    $slow = $pinned[2]
    $timed = Invoke-Pool -SleepSeconds @{ $slow = 30 } -TimeoutSeconds '1'
    Assert-True ($timed.exitCode -eq 2) `
        "a stub that outlives its deadline is killed and gives exit 2 (got $($timed.exitCode)) -- a hung suite has no colour, and silence must not read as green"
    Assert-True ($timed.text -match [regex]::Escape($slow) -and $timed.text -match 'exceeded 1 s') `
        'and the refusal names the suite and the ceiling it was given'

    $descendantMarker = Join-Path $fixtureRoot 'timed-descendant.marker'
    $descendantExitedBeforeRelease = $false
    try {
        # A one-second deadline can expire while PowerShell is still starting the child on a
        # loaded host. Allow its bounded startup, then require the child's own ready receipt;
        # otherwise the absence of a survival marker would be a vacuous pass.
        $timedTree = Invoke-Pool -SleepSeconds @{ $slow = 90 } `
            -DescendantMarkers @{ $slow = $descendantMarker } -TimeoutSeconds '30'
        $childPid = Wait-ForPidFile -Path "$descendantMarker.pid" -TimeoutMilliseconds 5000
        if (-not $childPid.Succeeded) {
            $script:fixtureBroken = $true
            Write-Host "HARNESS-BROKE: $($childPid.Reason)" -ForegroundColor Magenta
        } else {
            try {
                # The child waits for the release signal, so a live PID here means the pool
                # returned before terminating it. PID reuse can only cause a false refusal.
                $observedChild = [System.Diagnostics.Process]::GetProcessById($childPid.ProcessId)
                $descendantExitedBeforeRelease = $observedChild.HasExited
            } catch [System.ArgumentException] {
                $descendantExitedBeforeRelease = $true
            } catch {
                $script:fixtureBroken = $true
                Write-Host 'HARNESS-BROKE: timed descendant process identity could not be read' -ForegroundColor Magenta
            }
        }
    } finally {
        [System.IO.File]::WriteAllText("$descendantMarker.release", 'release')
    }
    Assert-True (Test-Path -LiteralPath "$descendantMarker.ready") `
        'the timed descendant started before the pool tested process-tree cleanup'
    Assert-True ($timedTree.exitCode -eq 2 -and $timedTree.text -match [regex]::Escape($slow)) `
        'the pool refused the wrapper after its deadline rather than reporting a clean exit'
    Assert-True $descendantExitedBeforeRelease `
        'the timed descendant process exited before its release signal, independent of marker scheduling'
    Start-Sleep -Seconds 4
    Assert-True (-not (Test-Path -LiteralPath $descendantMarker)) `
        'the timeout kills the whole process tree before a descendant can outlive its suite wrapper'

    # THE MEASURED PROTOTYPE DEFECT. Staged by reading, because a stub can choose its exit code but
    # not whether Windows reports one; the timeout cell above proves the string-typed path executes.
    Assert-True ($runnerText -match "if \(\`$null -eq \`$code\) \{ \`$ledger\[\`$entry\.Name\] = 'unreadable' \}") `
        'SOURCE: an exit code that reads back $null becomes ''unreadable'' -- not 0, which is the prototype defect that reported 46 green children'
    Assert-True ($runnerText -match "else \{ \`$broke\.Add\(`"\`$name \(exit code unreadable\)`"\) \}") `
        'and ''unreadable'' lands in the HARNESS-BROKE list, so an unanswerable child refuses instead of congratulating'

    Assert-True ($runnerText -match '\$strayKeys = @\(\$ledger\.Keys \| Where-Object \{ \$discovered -notcontains \$_ \}\)' -and
        $runnerText -match 'a result exists for a suite nobody discovered') `
        'the ledger is set-compared against the discovered set in BOTH directions, so a suite that was never scheduled cannot look like one that passed'

    Write-Host ''
    Write-Host '-- the transcript stays attributable --' -ForegroundColor Cyan
    $noisy = $pinned[3]
    $stderr = Invoke-Pool -StderrText @{ $noisy = 'STDERR-ONLY-MARKER' }
    Assert-True ($stderr.text -match 'STDERR-ONLY-MARKER') `
        'a suite that writes only to stderr still has that text replayed, so a diagnosis printed on the wrong stream is not lost'
    Assert-True ($stderr.text -match "\[suite\] $([regex]::Escape($noisy)) \([0-9.]+s\)") `
        'and each replayed suite carries its own wall time, so the pool can say which member is its floor without being re-instrumented'

    Write-Host ''
    Write-Host '-- redirected streams close on their own clock --' -ForegroundColor Cyan
    $readHelper = Get-Command Read-SuiteStream -ErrorAction SilentlyContinue
    if ($null -eq $readHelper) {
        Assert-True $false 'a briefly locked transcript is retried and read after its writer releases it'
        Assert-True $false 'a transcript that stays locked fails closed instead of hanging or disappearing'
    } else {
        $transientPath = Join-Path $fixtureRoot 'transient-stream.txt'
        $transientReady = Join-Path $fixtureRoot 'transient-stream.ready'
        [System.IO.File]::WriteAllText($transientPath, 'eventual transcript', $utf8NoBom)
        $lockerScript = @"
`$stream = [System.IO.File]::Open('$($transientPath.Replace("'", "''"))', 'Open', 'ReadWrite', 'None')
[System.IO.File]::WriteAllText('$($transientReady.Replace("'", "''"))', 'ready')
Start-Sleep -Milliseconds 500
`$stream.Dispose()
"@
        $locker = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
            -ArgumentList '-NoProfile', '-NonInteractive', '-Command', $lockerScript
        $readyDeadline = (Get-Date).AddSeconds(5)
        while (-not (Test-Path -LiteralPath $transientReady) -and (Get-Date) -lt $readyDeadline) {
            Start-Sleep -Milliseconds 25
        }
        $lockerReady = Test-Path -LiteralPath $transientReady
        $transient = Read-SuiteStream -Path $transientPath -TimeoutMilliseconds 3000
        $lockerExited = $locker.WaitForExit(5000)
        if (-not $lockerExited) { try { $locker.Kill() } catch { } }
        Assert-True ($lockerReady -and $lockerExited -and $transient.Succeeded -and $transient.Text -eq 'eventual transcript') `
            'a briefly locked transcript is retried and read after its writer releases it'

        $persistentPath = Join-Path $fixtureRoot 'persistent-stream.txt'
        [System.IO.File]::WriteAllText($persistentPath, 'must not disappear', $utf8NoBom)
        $persistentLock = [System.IO.File]::Open($persistentPath, 'Open', 'ReadWrite', 'None')
        try {
            $persistent = Read-SuiteStream -Path $persistentPath -TimeoutMilliseconds 150
        } finally {
            $persistentLock.Dispose()
        }
        Assert-True (-not $persistent.Succeeded -and $persistent.Reason -match 'remained locked') `
            'a transcript that stays locked fails closed instead of hanging or disappearing'
    }

    Write-Host ''
    Write-Host '-- an unreadable throttle widens to serial, never wider --' -ForegroundColor Cyan
    $bad = Invoke-Pool -Throttle 'not-a-number'
    Assert-True ($bad.exitCode -eq 0 -and $bad.text -match 'running 1 at a time') `
        'an unparseable throttle runs SERIALLY -- every way of being unsure about the number resolves to the behaviour this file had before the pool'
    Assert-True ($bad.text -match 'is not a positive integer; running SERIALLY') `
        'and it says so, rather than silently choosing a number nobody asked for'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}
if ($script:fixtureBroken) {
    Write-Host 'HARNESS-BROKE: fixture startup or cleanup could not be vouched for.' -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
