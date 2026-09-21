# #1053 item 8: the PowerShell suites run in a pool, and this file is what the pool had to earn.
#
# The stage was 658.2 s on the gate that first measured the rest of #1053 -- larger than
# `workspace tests` -- purely because `ci/run-ps-suites.ps1` walked a `foreach`. Running the same
# 48 children six at a time took it to ~365-490 s, and the remaining floor is one suite:
# `merge-proof.tests.ps1` alone measured 487.7 s against a pool wall of 488.6 s. The pool is
# therefore already optimal; what is left is inside that suite, not in this scheduler.
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
$ExpectedAssertionCount = 23

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$runnerPath = Join-Path $PSScriptRoot 'run-ps-suites.ps1'
$runnerText = [System.IO.File]::ReadAllText($runnerPath)
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

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
# Hold the wrapper behind an explicit release file. The old 400 ms sleep made this cell a timing
# lottery: on a loaded host the wrapper could exit before Stop-SuiteProcessTree reached taskkill,
# so the intended PID-not-found branch was never exercised. The fake taskkill below releases the
# wrapper only after the process-tree snapshot has run, preserving the race shape with no clock race.
$raceCommand = "`$child = Start-Process powershell.exe -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30'; Set-Content -LiteralPath '$racePidFile' -Value `$child.Id; while (-not (Test-Path -LiteralPath '$raceReleaseFile')) { Start-Sleep -Milliseconds 25 }"
$raceProcess = Microsoft.PowerShell.Management\Start-Process powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList '-NoProfile', '-NonInteractive', '-Command', $raceCommand
$deadline = (Get-Date).AddSeconds(10)
while (-not (Test-Path -LiteralPath $racePidFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
$raceChildId = [int](Get-Content -LiteralPath $racePidFile -Raw)
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
try {
    $raceStopped = Stop-SuiteProcessTree -Process $raceProcess
} finally {
    Remove-Item function:Start-Process -Force
}
$raceChildExited = try { $raceChildProcess.HasExited } catch { $false }
if (-not $raceChildExited) {
    # Cleanup only. The process may exit between the liveness read and taskkill; that benign race
    # must not abort the harness before the assertion reports the production result.
    try { taskkill.exe /PID $raceChildId /T /F 2>$null | Out-Null } catch { }
}
Remove-Item -LiteralPath $racePidFile -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $raceReleaseFile -Force -ErrorAction SilentlyContinue
Assert-True ($raceStopped -and $raceChildExited) `
    "a wrapper that exits while taskkill reports PID-not-found still has every remembered descendant stopped (stopped=$raceStopped childExited=$raceChildExited)"

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
            $child = "Start-Sleep -Seconds 3`n[System.IO.File]::WriteAllText('$($marker.Replace("'", "''"))', 'descendant survived')"
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
    $timedTree = Invoke-Pool -SleepSeconds @{ $slow = 30 } `
        -DescendantMarkers @{ $slow = $descendantMarker } -TimeoutSeconds '1'
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
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
