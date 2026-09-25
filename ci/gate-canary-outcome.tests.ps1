# #751: the canary's two arms. "The canary caught contamination" and "the canary never ran"
# are opposite claims about the same run, and the durable manifest is what #674(b) reads to decide
# a merge -- so recording one as the other is not a cosmetic mislabel.
#
# Extracts the pure functions from ci/gate.ps1 by text and dot-sources NOTHING else: running
# gate.ps1 would run the gate. Same homegrown harness as ci/gate-evidence.tests.ps1.
$ExpectedAssertionCount = 34

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

$scriptDir = $PSScriptRoot
$gatePath = Join-Path $scriptDir 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# ARRANGEMENT before the assertions that rest on it: a file that failed to parse would yield an
# empty extraction, and every cell below would pass or fail about a program never read.
$parseErrors = $null
$gateAst = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors)
Assert-True ($parseErrors.Count -eq 0) 'ARRANGEMENT: gate.ps1 parses, so what follows is measured rather than empty'

function Get-Slice {
    param([string] $From, [string] $To)
    $a = $gateText.IndexOf($From, [System.StringComparison]::Ordinal)
    $b = $gateText.IndexOf($To, [System.StringComparison]::Ordinal)
    if ($a -lt 0 -or $b -le $a) { throw "could not slice $From .. $To out of gate.ps1" }
    return $gateText.Substring($a, $b - $a)
}

$predicates = Get-Slice -From 'function Test-GateHardCargoLock {' -To 'function Invoke-Stage {'
$decision = Get-Slice -From 'function Get-CanaryOutcome {' -To '# #152: rewrites the canary'
Assert-True ($predicates.Length -gt 0 -and $decision.Length -gt 0) 'ARRANGEMENT: the predicates and the decision were extracted, so the subject exists'
. ([scriptblock]::Create($predicates + "`n" + $decision))

# The transcripts. Kept DISJOINT so a detector that leaned one way fails one of the cells.
$HardLock = @(
    'error: failed to open: D:\codex-targets\debug\.cargo-build-lock',
    'Caused by:',
    '  The process cannot access the file because it is being used by another process. (os error 32)'
)
$WaitedThenBuilt = @(
    '    Blocking waiting for file lock on build directory',
    '   Compiling ci-canary v0.1.0'
)
$RealContamination = @(
    'running 1 test',
    'test hashing::tests::src_tree_hash_matches_build ... FAILED',
    'failures:',
    'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out'
)
# THE CASE THE FIRST VERSION GOT WRONG (found in review of #833): cargo queued, got the lock, ran,
# and the canary found real dirt. Both signatures in one transcript.
$WaitedThenContaminated = $WaitedThenBuilt + $RealContamination

Write-Host ''
Write-Host '-- the two lock shapes are told apart, because they are different claims --' -ForegroundColor Cyan
Assert-True (Test-GateHardCargoLock -Lines $HardLock) 'the hard shape -- cargo could not open the lock -- is a hard lock'
Assert-True (-not (Test-GateWaitedForCargoLock -Lines $HardLock)) 'and it is not a wait: cargo never queued, it gave up'
Assert-True (Test-GateWaitedForCargoLock -Lines $WaitedThenBuilt) 'the cooperative shape -- Blocking waiting for file lock -- is a wait'
Assert-True (-not (Test-GateHardCargoLock -Lines $WaitedThenBuilt)) 'and a wait is not a hard lock'
Assert-True ((-not (Test-GateHardCargoLock -Lines $RealContamination)) -and (-not (Test-GateWaitedForCargoLock -Lines $RealContamination))) 'a contaminated-build transcript carries neither signature: the control that makes the cells above mean something'
# THE LOCK AN ISOLATED TARGET DOES NOT REMOVE (raised on #833 after G's gate hung): cargo also locks
# ~/.cargo/.package-cache, which is GLOBAL TO THE MACHINE. A matcher keyed to `.cargo-build-lock`
# covered only the lock an isolated CARGO_TARGET_DIR already removes -- so the likeliest contention
# on a five-gate host read as contamination.
$PackageCacheHard = @(
    'error: failed to open: C:\Users\example\.cargo\.package-cache',
    'Caused by:',
    '  The process cannot access the file because it is being used by another process. (os error 32)'
)
$PackageCacheWait = @(
    '    Blocking waiting for file lock on package cache',
    '   Downloading crates ...'
)
# THE CONTROL FOR THE BROADENING ITSELF: `failed to open` with no contention behind it is a plain
# I/O failure, not a lock, and must NOT be read as one. Without this the widened matcher would be
# free to call any open failure a lock.
$PlainOpenFailure = @(
    'error: failed to open: /nonexistent/Cargo.toml',
    'Caused by:',
    '  The system cannot find the path specified. (os error 3)'
)

Write-Host ''
Write-Host '-- the machine-global package cache lock, which an isolated target does NOT remove --' -ForegroundColor Cyan
Assert-True (Test-GateHardCargoLock -Lines $PackageCacheHard) 'a hard failure on the package cache is a cargo lock, not contamination'
Assert-True (Test-GateWaitedForCargoLock -Lines $PackageCacheWait) 'and a wait on the package cache is a wait: the prefix is cut before the lock name'
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $PackageCacheHard).status -ceq 'HARNESS-BROKE') 'so a package-cache lock reaches the arm as HARNESS-BROKE'
Assert-True (-not (Test-GateHardCargoLock -Lines $PlainOpenFailure)) 'CONTROL: failed-to-open with no contention is NOT a lock, so the widened matcher stays a lock detector'
# It is HARNESS-BROKE, but for the OTHER reason, and the distinction is the point: not because a
# lock was seen -- none was -- but because nothing reported. The predicate assertion above is what
# keeps the lock detector honest; this one records that the arm no longer needs it to be a lock.
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $PlainOpenFailure).status -ceq 'HARNESS-BROKE') 'a plain open failure reported nothing either, so it is a non-run and NOT a lock'
Assert-True (-not (Get-CanaryOutcome -ExitCode 101 -Lines $PlainOpenFailure).cargoLockObserved) 'and the record does not blame a lock that was never there'

Write-Host ''
Write-Host '-- did the canary get far enough to report? that is what turns a wait into a run --' -ForegroundColor Cyan
Assert-True (Test-GateCanaryProducedResult -Lines $RealContamination) 'a transcript with a test result reported one'
Assert-True (-not (Test-GateCanaryProducedResult -Lines $HardLock)) 'a transcript that never opened the lock reported nothing'
Assert-True ((-not (Test-GateCanaryProducedResult -Lines @())) -and (-not (Test-GateCanaryProducedResult -Lines $null))) 'and empty and null report nothing rather than throwing'

Write-Host ''
Write-Host '-- ARM ONE: a lock that stopped the run is HARNESS-BROKE --' -ForegroundColor Cyan
$onHard = Get-CanaryOutcome -ExitCode 101 -Lines $HardLock
Assert-True ($onHard.status -ceq 'HARNESS-BROKE') "a canary that could not take the lock is HARNESS-BROKE (got '$($onHard.status)')"
$onWaitNoResult = Get-CanaryOutcome -ExitCode 101 -Lines $WaitedThenBuilt
Assert-True ($onWaitNoResult.status -ceq 'HARNESS-BROKE') "a canary that waited and never reported is HARNESS-BROKE (got '$($onWaitNoResult.status)')"

Write-Host ''
Write-Host '-- ARM TWO: a canary that RAN and found dirt is ABORTED-BY-CANARY, even if it queued first --' -ForegroundColor Cyan
$onDirt = Get-CanaryOutcome -ExitCode 101 -Lines $RealContamination
Assert-True ($onDirt.status -ceq 'ABORTED-BY-CANARY') "a canary that ran and reported dirt is ABORTED-BY-CANARY (got '$($onDirt.status)')"
# THE FINDING FROM #833's REVIEW: reading the wait alone filed this as 'never ran'. It ran. It found
# dirt. HARNESS-BROKE here would tell the operator to re-run on a stable host, which is the wrong
# instruction for a contaminated one.
$onWaitThenDirt = Get-CanaryOutcome -ExitCode 101 -Lines $WaitedThenContaminated
Assert-True ($onWaitThenDirt.status -ceq 'ABORTED-BY-CANARY') "a canary that WAITED, ran, and found dirt is still ABORTED-BY-CANARY (got '$($onWaitThenDirt.status)')"
Assert-True (-not $onWaitThenDirt.cargoLockObserved) 'and it does not blame the lock for a finding it actually made'
Write-Host ''
Write-Host '-- a canary that never REPORTED made no finding, however it ended --' -ForegroundColor Cyan
# The third non-run shape, named by the review of #833: the launcher kill, which carries no cargo
# text at all. Filed as contamination, it would accuse an environment nothing measured.
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines @()).status -ceq 'HARNESS-BROKE') 'a non-zero exit with NO output is a run that never reported, not a finding'
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $null).status -ceq 'HARNESS-BROKE') 'and a null transcript is the same claim'
$CompileFailure = @(
    'error[E0433]: failed to resolve: use of undeclared crate or module `nonce`',
    'error: could not compile `ci-canary` (lib test) due to 1 previous error'
)
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $CompileFailure).status -ceq 'HARNESS-BROKE') 'a canary crate that did not build reported nothing, so it found nothing'
# THE CONTROL FOR THIS BROADENING: `running N tests` is printed BEFORE the tests execute, so a
# canary that starts and aborts hard -- no `test result:` line at all -- still RAN and is still a
# finding. Without this cell, "no result line" could swallow a genuine contaminated abort.
$AbortedMidTest = @(
    'running 1 test',
    'error: test failed, to rerun pass `-p ci-canary --lib`'
)
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $AbortedMidTest).status -ceq 'ABORTED-BY-CANARY') 'CONTROL: a canary that started and aborted hard still RAN, so it is still a finding'

Write-Host ''
Write-Host '-- ordering: REPORTING decides, because the lock phrases are matched over the whole transcript --' -ForegroundColor Cyan
# THIS CELL INVERTED, and the reason belongs here rather than only in the commit. It used to assert
# that a hard-lock signature beat a result line. It does not any more, and C is right: the hard
# matcher accumulates `failed to open` and `being used by another process` over the WHOLE
# transcript, so it cannot tell a lock's own two lines from two unrelated ones -- and a genuine
# contamination whose panic says "failed to open the fixture" would have been filed as "never ran".
# The result line already answers the question the accumulation is guessing at.
$both = $HardLock + $RealContamination
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $both).status -ceq 'ABORTED-BY-CANARY') 'a transcript that REPORTED is a finding, even beside lock words'
# C's own transcript, constructed rather than observed: the two phrases of the hard shape arriving
# on unrelated lines of a real failure. The detector has to discriminate, not accumulate.
$DirtyCarryingLockWords = @(
    'running 1 test',
    'test hashing::tests::src_tree_hash_matches_build ... FAILED',
    'failures:',
    '  thread panicked: failed to open the fixture file',
    '  caused by: the file is being used by another process',
    'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out'
)
Assert-True (Test-GateHardCargoLock -Lines $DirtyCarryingLockWords) 'ARRANGEMENT: the accumulating matcher DOES fire on these unrelated lines -- that is the window'
Assert-True ((Get-CanaryOutcome -ExitCode 101 -Lines $DirtyCarryingLockWords).status -ceq 'ABORTED-BY-CANARY') 'and the arm closes it anyway, because the canary reported'
Assert-True (-not (Get-CanaryOutcome -ExitCode 101 -Lines $DirtyCarryingLockWords).cargoLockObserved) 'and the record does not blame a lock for a finding the canary made'

Write-Host ''
Write-Host '-- and a canary that passed is untouched --' -ForegroundColor Cyan
$onGreen = Get-CanaryOutcome -ExitCode 0 -Lines $WaitedThenBuilt
Assert-True ($onGreen.passed -and $onGreen.status -ceq 'GREEN') 'exit 0 passes even when cargo queued for the lock -- waiting is not failing'
Assert-True ($onHard.status -cne $onDirt.status) 'THE ARMS ARE DISTINGUISHED: the same exit 101 produces two different statuses'

Write-Host ''
Write-Host '-- the arm in gate.ps1 USES this decision, rather than repeating it --' -ForegroundColor Cyan
$calls = $gateAst.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -eq 'Get-CanaryOutcome'
    }, $true)
Assert-True ($calls.Count -ge 1) "gate.ps1 calls Get-CanaryOutcome (found $($calls.Count))"
Assert-True ($gateText.IndexOf("-Status 'ABORTED-BY-CANARY'", [System.StringComparison]::Ordinal) -lt 0) 'and no call site still passes the literal ABORTED-BY-CANARY status, which would pin one arm'
Assert-True ($gateText.IndexOf("'HARNESS-BROKE'", [System.StringComparison]::Ordinal) -ge 0) 'and HARNESS-BROKE is a status the manifest writer accepts'

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