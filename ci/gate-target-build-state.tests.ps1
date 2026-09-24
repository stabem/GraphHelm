# #455: a contaminated target must be knowable BEFORE the first compile, and a clean one must not
# be flagged for it.
#
# THE TRAP THIS SUITE EXISTS TO HOLD OPEN. The late flag (`ci/gate.ps1:1240`,
# `$freshBuild = ($mtimeUtc -ge $runStartUtc)`) is correct and useless early: at the instant before
# a build, NOTHING has been rebuilt, so every artefact of every reused target predates the run's
# start. Measured on this box: 77 of 77 test executables in `D:/graphhelm-target-g` fail that
# predicate at run start, and across the 128 gate receipts committed at the time (the receipt
# store was retired 2026-09-24) only 9 of 27 observed reuses were actually contaminated. An early
# check keyed on age -- or on the canary nonce, which `Write-CanaryNonce` rotates unconditionally
# every run -- flags all 27 and is wrong two times in three. A guard that cries wolf two times in
# three is one the fleet learns to skip, and it then occupies the place where the real check would go.
#
# So the property under test is DISCRIMINATION, not detection. A function that returns
# "contaminated" always detects every contamination and is exactly the instrument measured above;
# the cell that kills it is the one asserting a clean reuse comes back CLEAN.
#
# The distinction the issue's own evidence names is not age and not the nonce -- it is whether the
# previous build FINISHED. #455: "reused D:/graphhelm-target-issue-453 after an earlier gate
# ATTEMPT". A target left mid-build carries cargo fingerprints claiming freshness for binaries
# whose source has since moved; a target left by a run that completed does not.
#
# NO MOCKED LIVENESS. The concurrent arm uses $PID -- this very process, genuinely alive -- and the
# interrupted arm spawns a real process and waits for it to exit, so the "is the owner still there"
# question is answered by the operating system in both cells rather than by a stub that would agree
# with whatever the implementation happened to do.

$ExpectedAssertionCount = 55
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) {
        Write-Host "  PASS: $Message" -ForegroundColor Green
    } else {
        $script:failures++
        Write-Host "  FAIL: $Message" -ForegroundColor Red
    }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'

# ARRANGEMENT FIRST. A file that stopped parsing, or a function that was renamed, yields no subject
# at all -- and every assertion below would then be about a program that was never read, in the
# same green as a passing run.
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: gate.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

# RETURNS THE TEXT; the caller dot-sources it. Dot-sourcing inside this function would define the
# subject in THIS function's scope, which vanishes on return -- measured, and it presents as
# "Get-TargetBuildState is not recognized" at the first call site, several screens away from the
# import that looked like it worked.
function Get-GateFunctionText {
    param([Parameter(Mandatory)] [string] $Name)
    $fn = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $Name
        }, $true)
    if ($null -eq $fn) {
        # NAMED, because "expected 18 assertions, ran 1" says nothing about what is missing.
        Write-Host "HARNESS-BROKE: $Name was not found in gate.ps1" -ForegroundColor Magenta
        exit 2
    }
    return $fn.Extent.Text
}

. ([scriptblock]::Create((Get-GateFunctionText -Name 'Get-TargetBuildState')))
. ([scriptblock]::Create((Get-GateFunctionText -Name 'Write-TargetBuildState')))

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-target-state-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

# A target directory that LOOKS like a reused one: it has build output in it. The artefact matters,
# because "no marker" must condemn a directory that already holds binaries and must NOT condemn an
# empty one -- otherwise failing closed quietly becomes failing always.
function New-TargetDir {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [switch] $Empty,
        [string] $MarkerContent
    )
    $dir = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory((Join-Path $dir 'debug')) | Out-Null
    if (-not $Empty) {
        [System.IO.File]::WriteAllText((Join-Path $dir 'debug\some_test-abc123.exe'), 'not a real binary')
    }
    if ($PSBoundParameters.ContainsKey('MarkerContent')) {
        [System.IO.File]::WriteAllText((Join-Path $dir '.graphhelm-build-state.json'), $MarkerContent)
    }
    return $dir
}

function New-Marker {
    param([Parameter(Mandatory)] [string] $State, [Parameter(Mandatory)] [int] $ProcessId)
    return (@{
            state       = $State
            processId   = $ProcessId
            head        = 'be7330796771763371a411c3578fd8f131e144d'
            startedUtc  = [DateTime]::UtcNow.ToString('o')
        } | ConvertTo-Json -Compress)
}

# A REAL dead process id, and one the OS CANNOT RECYCLE while this suite runs (Codex on #1009).
#
# Spawning and waiting is not enough on its own: after the child exits its pid is free, and under
# process churn Windows can hand it to something else before the assertion runs -- the fixture would
# then read as `concurrent` instead of `interrupted` and redden every authoritative gate,
# intermittently, for a reason no cell states.
#
# The fix is NOT a mock. Touching `.Handle` before the process exits keeps the process OBJECT open,
# and Windows does not reuse a pid while a handle to it is held -- so the number stays reserved and
# unusable by anyone else. Measured: with the handle held, `HasExited` is True and
# `Get-Process -Id` reports NOT FOUND, which is exactly the reading the cell needs. The operating
# system still answers the liveness question; it simply cannot answer it about a different process.
# THE HELD HANDLE IS THE MECHANISM, NOT A LEAK -- said plainly because the next reader will see a
# `Process` object deliberately kept alive and reach for the obvious cleanup. Disposing it, or
# letting it fall out of scope, hands the pid straight back to the OS and restores the flake this
# comment exists to explain. The handles live in a script-scope list for exactly as long as the
# suite does, and the process itself is already gone: what is retained is a few bytes of kernel
# bookkeeping per fixture, released when the suite exits.
$script:deadProcessHandles = New-Object System.Collections.Generic.List[object]
function Get-DeadProcessId {
    $p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', 'exit', '0' -PassThru -WindowStyle Hidden
    $null = $p.Handle
    $p.WaitForExit()
    $script:deadProcessHandles.Add($p)
    return $p.Id
}

try {
    Write-Host '-- the clean reuse, which is the cell an always-contaminated function fails --' -ForegroundColor Cyan

    $complete = New-TargetDir -Name 'complete' -MarkerContent (New-Marker -State 'complete' -ProcessId (Get-DeadProcessId))
    $completeRead = Get-TargetBuildState -TargetDir $complete
    Assert-True -Condition ($completeRead.contaminated -eq $false) `
        "a target whose last build COMPLETED is a legitimate reuse, not contamination (got contaminated=$($completeRead.contaminated), state=$($completeRead.state))"
    Assert-True -Condition ($completeRead.state -ceq 'complete') `
        "and its state is named 'complete' (got '$($completeRead.state)')"

    Write-Host ''
    Write-Host '-- the case #455 describes: an earlier gate ATTEMPT --' -ForegroundColor Cyan

    $deadPid = Get-DeadProcessId
    $interrupted = New-TargetDir -Name 'interrupted' -MarkerContent (New-Marker -State 'building' -ProcessId $deadPid)
    $interruptedRead = Get-TargetBuildState -TargetDir $interrupted
    Assert-True -Condition ($interruptedRead.contaminated -eq $true) `
        "a target left mid-build by a process that is GONE is contaminated (pid $deadPid; got contaminated=$($interruptedRead.contaminated))"
    Assert-True -Condition ($interruptedRead.state -ceq 'interrupted') `
        "and the state says the build was interrupted rather than merely 'unknown' (got '$($interruptedRead.state)')"
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace([string]$interruptedRead.reason)) `
        "and it carries a reason a reader can act on (got '$($interruptedRead.reason)')"

    Write-Host ''
    Write-Host '-- two gates in one target dir: today nothing sees this at all --' -ForegroundColor Cyan

    # $PID is THIS process: alive by construction, no stub involved.
    $concurrent = New-TargetDir -Name 'concurrent' -MarkerContent (New-Marker -State 'building' -ProcessId $PID)
    $concurrentRead = Get-TargetBuildState -TargetDir $concurrent
    Assert-True -Condition ($concurrentRead.contaminated -eq $true) `
        "a target whose recorded builder is STILL ALIVE is contaminated (pid $PID; got contaminated=$($concurrentRead.contaminated))"
    Assert-True -Condition ($concurrentRead.state -ceq 'concurrent') `
        "and it is called out as concurrent use, not as an interrupted build -- the remedies differ (got '$($concurrentRead.state)')"

    Write-Host ''
    Write-Host '-- absence and damage: fail closed, but only where there is something to condemn --' -ForegroundColor Cyan

    $noMarker = New-TargetDir -Name 'no-marker'
    $noMarkerRead = Get-TargetBuildState -TargetDir $noMarker
    Assert-True -Condition ($noMarkerRead.contaminated -eq $true) `
        "a NON-EMPTY target with no marker has unknown provenance, and unknown fails closed (got contaminated=$($noMarkerRead.contaminated))"
    Assert-True -Condition ($noMarkerRead.state -ceq 'unknown') `
        "and says 'unknown' rather than inventing a cause (got '$($noMarkerRead.state)')"

    # THE OTHER SIDE OF FAILING CLOSED. Without this cell, "absent means contaminated" makes every
    # first run on a new target dir a contaminated one, and the fix becomes the naive check again.
    $emptyDir = New-TargetDir -Name 'empty' -Empty
    $emptyRead = Get-TargetBuildState -TargetDir $emptyDir
    Assert-True -Condition ($emptyRead.contaminated -eq $false) `
        "an EMPTY target directory is not a reuse and must not be flagged (got contaminated=$($emptyRead.contaminated), state=$($emptyRead.state))"

    $missingRead = Get-TargetBuildState -TargetDir (Join-Path $fixtureRoot 'never-created')
    Assert-True -Condition ($missingRead.contaminated -eq $false) `
        "a target directory that does not exist yet is not a reuse either (got contaminated=$($missingRead.contaminated), state=$($missingRead.state))"

    $malformed = New-TargetDir -Name 'malformed' -MarkerContent '{ not json'
    $malformedRead = Get-TargetBuildState -TargetDir $malformed
    Assert-True -Condition ($malformedRead.contaminated -eq $true) `
        "a marker that does not parse is unknown provenance, not an absent marker on an empty dir (got contaminated=$($malformedRead.contaminated))"

    # A marker that parses but carries no state is a THIRD thing from one that does not parse and
    # from one that is missing -- valid JSON is not a valid marker.
    $shapeless = New-TargetDir -Name 'shapeless' -MarkerContent '{"head":"abc"}'
    $shapelessRead = Get-TargetBuildState -TargetDir $shapeless
    Assert-True -Condition ($shapelessRead.contaminated -eq $true) `
        "a marker that parses but names no state is unknown provenance (got contaminated=$($shapelessRead.contaminated))"

    Write-Host ''
    Write-Host '-- discrimination: the property, stated as one assertion --' -ForegroundColor Cyan

    # THE CELL THAT KILLS THE 67%-WRONG FUNCTION. Every contamination cell above is satisfied by
    # `return @{contaminated=$true}`; this one is not, and neither is it satisfied by the inverse.
    Assert-True -Condition ($completeRead.contaminated -ne $interruptedRead.contaminated) `
        'a completed build and an interrupted one get DIFFERENT answers -- a function that condemns every reuse fails here'
    Assert-True -Condition ($interruptedRead.state -cne $concurrentRead.state) `
        'and the two contaminated causes are told apart, so the diagnostic names which one happened'

    Write-Host ''
    Write-Host '-- the writer, and the round trip that proves the two halves agree --' -ForegroundColor Cyan

    # HAND-BUILT MARKERS PROVE THE READER; ONLY THE WRITER'S OWN OUTPUT PROVES THE WIRING. Every
    # cell above feeds text this file composed, so a writer that emits a different field name would
    # leave all of them green while the gate silently read 'unknown' on every run.
    $roundTrip = New-TargetDir -Name 'round-trip' -Empty
    Write-TargetBuildState -TargetDir $roundTrip -State 'building' -Head 'deadbeef'
    $duringOwn = Get-TargetBuildState -TargetDir $roundTrip
    Assert-True -Condition ($duringOwn.state -ceq 'concurrent') `
        "a marker this process wrote as 'building' reads back as concurrent while this process is alive (got '$($duringOwn.state)')"

    Write-TargetBuildState -TargetDir $roundTrip -State 'complete' -Head 'deadbeef'
    $afterOwn = Get-TargetBuildState -TargetDir $roundTrip
    Assert-True -Condition ($afterOwn.contaminated -eq $false -and $afterOwn.state -ceq 'complete') `
        "and reads back as a clean COMPLETE reuse once the writer says the build finished (got contaminated=$($afterOwn.contaminated), state='$($afterOwn.state)')"

    # The marker must not become build input. A file cargo hashes would change the fingerprint of
    # every crate and defeat the caching this whole guard exists to keep.
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $roundTrip '.graphhelm-build-state.json')) `
        'the marker lives at the target root, outside debug/ and release/, where no cargo fingerprint reaches it'

    # #1007: A RUN THAT FINISHED BUT COULD NOT VOUCH is stamped `unproven`, and the reader must tell
    # it from a run that DIED. Before this state existed the gate left `building` in place on that
    # path, the next run found the owner gone, read `interrupted` -- suspect -- and aborted before its
    # first compile, sticky. The two arms below are the distinction: `unproven` reuses nothing on
    # trust (contaminated, like `interrupted`) and does NOT abort (not suspect, unlike `interrupted`).
    Write-TargetBuildState -TargetDir $roundTrip -State 'unproven' -Head 'deadbeef'
    $afterUnproven = Get-TargetBuildState -TargetDir $roundTrip
    Assert-True -Condition ($afterUnproven.state -ceq 'unproven' -and $afterUnproven.contaminated -eq $true -and $afterUnproven.suspect -eq $false) `
        "a finished run that could not vouch reads back as UNPROVEN: contaminated (no clean reuse) and NOT suspect (no abort) (got state='$($afterUnproven.state)', contaminated=$($afterUnproven.contaminated), suspect=$($afterUnproven.suspect))"
    Assert-True -Condition ($afterUnproven.suspect -ne $interruptedRead.suspect -and $afterUnproven.contaminated -eq $interruptedRead.contaminated) `
        'and it differs from INTERRUPTED on exactly the axis that decides the abort: same contamination, opposite suspicion'

    Write-Host ''
    Write-Host '-- the marker is UNTRUSTED INPUT: another process wrote it (Codex on #1009) --' -ForegroundColor Cyan

    # This file lives in a target directory the gate does not own, and it is read BEFORE the first
    # compile in the only authoritative gate. Two things follow, and neither was true of the first
    # draft: it must be bounded before it is parsed, and nothing out of it may be echoed verbatim
    # into a log or a COMMITTED manifest.
    $huge = New-TargetDir -Name 'huge'
    [System.IO.File]::WriteAllText((Join-Path $huge '.graphhelm-build-state.json'), ('{"state":"complete","pad":"' + ('A' * 70000) + '"}'))
    $hugeRead = Get-TargetBuildState -TargetDir $huge
    Assert-True -Condition ($hugeRead.state -ceq 'unknown' -and $hugeRead.contaminated -eq $true) `
        "an oversized marker is refused before it is parsed, not read into the parser (got '$($hugeRead.state)')"
    # AND IT IS REFUSED FOR ITS SIZE, not by falling through to a JSON error -- the payload above is
    # VALID JSON naming a legitimate state, so a bound that did not fire would return 'complete'.
    Assert-True -Condition ($hugeRead.reason -match 'bytes') `
        "and the reason names the size rather than blaming the JSON (got '$($hugeRead.reason)')"

    # THE ECHO. A hostile state carries a newline and terminal control text; the reason is printed to
    # the gate console and persisted into `targetBuildState` in a manifest that is committed.
    $evil = "complete`" `n`e[31mFAKE GREEN`e[0m`n$(('x' * 200))"
    $hostile = New-TargetDir -Name 'hostile' -MarkerContent (@{ state = $evil; processId = 1 } | ConvertTo-Json -Compress)
    $hostileRead = Get-TargetBuildState -TargetDir $hostile
    Assert-True -Condition ($hostileRead.state -ceq 'unknown') `
        "an unrecognised state is unknown (got '$($hostileRead.state)')"
    Assert-True -Condition ($hostileRead.reason -notmatch 'FAKE GREEN' -and $hostileRead.reason -notmatch "`n") `
        "and NOTHING of it reaches the reason -- no injected text, no newline (got '$($hostileRead.reason)')"
    # THE OTHER SIDE, or the cell above is satisfied by a diagnostic that says nothing at all: an
    # ordinary unknown state -- a typo, or a state a newer gate writes -- is still quoted back, or
    # the reader cannot act on it.
    $typo = New-TargetDir -Name 'typo' -MarkerContent (@{ state = 'completed'; processId = 1 } | ConvertTo-Json -Compress)
    $typoRead = Get-TargetBuildState -TargetDir $typo
    Assert-True -Condition ($typoRead.reason -match 'completed') `
        "a plain identifier IS named, so the safe form did not cost the diagnostic (got '$($typoRead.reason)')"

    # THE SCAN IS BOUNDED IN DEPTH (Codex on #1009). `-Recurse` with `Select-Object -First 1` stops
    # only once a FILE has been emitted, so a target whose tree begins with directories is walked in
    # full -- before the first compile, in the only authoritative gate.
    Assert-True -Condition ((Get-GateFunctionText -Name 'Get-TargetBuildState') -notmatch '-Recurse') `
        'the marker-absence scan does not recurse: its depth is fixed, not merely short-circuited'

    # A deep tree with no files anywhere the probe looks is NOT output, and answering that must not
    # require walking it.
    $deep = Join-Path $fixtureRoot 'deep'
    [System.IO.Directory]::CreateDirectory((Join-Path $deep 'a\b\c\d\e\f\g')) | Out-Null
    $deepRead = Get-TargetBuildState -TargetDir $deep
    Assert-True -Condition ($deepRead.contaminated -eq $false -and $deepRead.state -ceq 'fresh') `
        "a directory tree with no build output is fresh, whatever its shape (got '$($deepRead.state)')"

    # A SCAN THAT RAN OUT IS NOT A SCAN THAT FOUND NOTHING (Codex on #1009). 600 directories and no
    # files: the probe hits its 512-entry bound before it can see anything, and "I stopped looking"
    # must not be recorded as "there is nothing here" -- that would skip the whole check on precisely
    # the target too wide to inspect.
    $wide = Join-Path $fixtureRoot 'wide'
    [System.IO.Directory]::CreateDirectory($wide) | Out-Null
    foreach ($n in 1..600) { [System.IO.Directory]::CreateDirectory((Join-Path $wide $n)) | Out-Null }
    $wideRead = Get-TargetBuildState -TargetDir $wide
    Assert-True -Condition ($wideRead.state -ceq 'unknown' -and $wideRead.contaminated -eq $true) `
        "a probe that exhausts its bound is UNKNOWN, never fresh (got '$($wideRead.state)')"
    Assert-True -Condition ($wideRead.reason -match '512') `
        "and the reason names the bound, so a reader knows it stopped rather than finished (got '$($wideRead.reason)')"

    # AND A PROBE WE COULD NOT READ IS NOT A PROBE THAT FOUND NOTHING. A real directory, denied to
    # this very user with icacls -- not a stub, because this suite's whole discipline is that the OS
    # answers. The arrangement is asserted FIRST: if the deny does not bite on some machine, the cell
    # must say so rather than quietly measure a readable directory and pass.
    $denied = Join-Path $fixtureRoot 'denied'
    [System.IO.Directory]::CreateDirectory((Join-Path $denied 'debug')) | Out-Null
    $deniedProbe = Join-Path $denied 'debug'
    $null = icacls $deniedProbe /deny ('{0}:(RX)' -f $env:USERNAME) 2>&1
    $reallyUnreadable = $false
    try { (New-Object System.IO.DirectoryInfo $deniedProbe).EnumerateFileSystemInfos() | Select-Object -First 1 | Out-Null }
    catch { $reallyUnreadable = $true }
    Assert-True -Condition $reallyUnreadable `
        'ARRANGEMENT: the probed directory really is unreadable, or the two cells below measure nothing'
    $deniedRead = Get-TargetBuildState -TargetDir $denied
    Assert-True -Condition ($deniedRead.state -ceq 'unknown' -and $deniedRead.contaminated -eq $true) `
        "a probe that could not be READ is unknown, never fresh (got '$($deniedRead.state)')"
    Assert-True -Condition ($deniedRead.reason -match 'could not be read') `
        "and the reason says it could not be read, not that it was empty (got '$($deniedRead.reason)')"
    $null = icacls $deniedProbe /remove:d $env:USERNAME 2>&1

    # AND THE ONE I BROKE FIXING THE ABOVE, kept as its own cell: a target used only for `cargo test`
    # can have nothing at the top of `debug` and every binary one level down in `deps`. A probe that
    # stopped at `debug` would call that target empty and skip the whole check.
    $depsOnly = Join-Path $fixtureRoot 'deps-only'
    [System.IO.Directory]::CreateDirectory((Join-Path $depsOnly 'debug\deps')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $depsOnly 'debug\deps\some_test-abc.exe'), 'x')
    $depsRead = Get-TargetBuildState -TargetDir $depsOnly
    Assert-True -Condition ($depsRead.contaminated -eq $true -and $depsRead.state -ceq 'unknown') `
        "output only in debug/deps IS build output, so an unmarked target holding it is unknown, not fresh (got '$($depsRead.state)')"

    Write-Host ''
    Write-Host '-- what REDDENS is narrower than what is not clean (X on #1009) --' -ForegroundColor Cyan

    # TWO QUESTIONS, TWO FIELDS. `contaminated` means "not a proven-clean reuse"; `suspect` means
    # "this run's instrument cannot be vouched for", and only the second forbids a GREEN.
    #
    # The first draft used one field for both, and the cost is measurable rather than theoretical:
    # 178 of 178 cargo-shaped target directories on this machine's D: and E: hold build output and
    # no marker, so every one of them would have cost a non-GREEN hand-launched run on first reuse.
    # For a guard whose whole argument is that a check firing on clean runs gets ignored, that is
    # the guard arguing against itself. It also bought nothing: an unknown target that really is
    # stale still fails the late staleArtifactCount check, which is how #455 was caught originally.
    Assert-True -Condition ($noMarkerRead.suspect -eq $false) `
        "an UNKNOWN target is a note, not a verdict -- it does not redden the run (got suspect=$($noMarkerRead.suspect))"
    Assert-True -Condition ($malformedRead.suspect -eq $false -and $shapelessRead.suspect -eq $false) `
        "and neither does a damaged marker (malformed suspect=$($malformedRead.suspect), shapeless suspect=$($shapelessRead.suspect))"

    # THE OTHER SIDE, or the field above would be satisfied by returning false always.
    Assert-True -Condition ($interruptedRead.suspect -eq $true -and $concurrentRead.suspect -eq $true) `
        "a build that demonstrably BROKE does redden it (interrupted suspect=$($interruptedRead.suspect), concurrent suspect=$($concurrentRead.suspect))"
    Assert-True -Condition ($completeRead.suspect -eq $false -and $emptyRead.suspect -eq $false) `
        'and a clean reuse and an empty directory still redden nothing'

    Write-Host ''
    Write-Host '-- the STATUS, which is the only field anything downstream reads --' -ForegroundColor Cyan

    # THE CELL THE FIRST DRAFT DID NOT HAVE, and its absence let a false claim ship twice. That
    # draft fed the verdict into `instrumentSuspect` and printed "the run continues and cannot
    # report GREEN". Measured after Codex asked: `merge-proof.ps1` (retired 2026-09-24) required
    # `status == GREEN` and mentioned `instrumentSuspect` ZERO times, and the status was derived
    # from the failed-stage count alone -- so a contaminated target with every stage passing
    # produced GREEN, exit 0, and a manifest merge-proof accepted. The flag was recorded, not
    # enforced, and nothing here could tell the two apart because no cell called the rule that
    # decides.
    . ([scriptblock]::Create((Get-GateFunctionText -Name 'Get-GateStatus')))

    Assert-True -Condition ((Get-GateStatus -FailedStageCount 0 -TargetSuspect $false) -ceq 'GREEN') `
        'a clean run on a sound target is GREEN'
    Assert-True -Condition ((Get-GateStatus -FailedStageCount 0 -TargetSuspect $true) -cne 'GREEN') `
        'and a suspect target is NOT GREEN -- the field anything downstream reads'
    Assert-True -Condition ((Get-GateStatus -FailedStageCount 0 -TargetSuspect $true) -ceq 'HARNESS-BROKE') `
        'it is HARNESS-BROKE, not RED: the tree did not fail, the run could not vouch for what it measured'
    # Failure is the more specific fact, and this also stops the rule from being satisfied by
    # returning HARNESS-BROKE whenever anything is wrong.
    Assert-True -Condition ((Get-GateStatus -FailedStageCount 1 -TargetSuspect $true) -ceq 'RED') `
        'a failed stage outranks a suspect target -- RED names the more specific fact'

    Write-Host ''
    Write-Host '-- the release must not erase the proof of what the canary detected --' -ForegroundColor Cyan

    # MY OWN DEFECT, from the commit that fixed the canary deadlock. The release was UNCONDITIONAL,
    # and `ABORTED-BY-CANARY` means the canary DETECTED contaminated output -- so stamping `complete`
    # there told the next run the target was a legitimate reuse, and it would skip the early abort
    # this whole PR adds. A mechanism that erases the proof of exactly what it detects.
    # Read here rather than relying on `$gateText`, which the wiring section below sets AFTER this
    # point -- measured: the suite threw "cannot be retrieved because it has not been set" and
    # stopped at 34 of 46. The catch that names a crash is what made that one line long.
    $gateSource = [System.IO.File]::ReadAllText($gatePath)
    $canaryRelease = [regex]::Match($gateSource, "if \(\`$canaryOutcome\.cargoLockObserved\) \{[^}]*Write-TargetBuildState[^}]*\}", 'Singleline')
    Assert-True -Condition ($canaryRelease.Success) `
        'the canary path releases the target ONLY on the lock branch, which never compiled'
    # And the sticky mark needs a way out, or the next run meets a dead end it cannot read its way
    # out of: an abort that names only the cause reads as a bug the second time you see it.
    Assert-True -Condition ($gateSource -cmatch 'To proceed: use a FRESH target directory') `
        'and the abort tells the operator how to leave the state it puts them in'

    # THE THIRD INSTANCE OF ONE SHAPE: releasing the mark before the answer is known. The canary path
    # had it, I fixed that and left the ordinary one -- a run finishing with STALE artefacts still
    # stamped `complete`, so the next run trusted the target and spent a full gate rediscovering the
    # same stale output.
    # #904 RESPELLED THE POPULATION, NOT THE PROPERTY. The condition used to be `$staleAtEnd`,
    # every artefact whose mtime predates the run's start; it is now `$unprovenAtEnd`, every
    # artefact this run can neither say it built nor prove unchanged against the target's ledger.
    # The claim this cell makes -- the stamp is CONDITIONAL on the run being able to vouch for what
    # it measured -- is the same one, and a cell that failed on a correct rename would spend a
    # reviewer's attention on itself.
    Assert-True -Condition ($gateSource -cmatch '\$unprovenAtEnd -eq 0') `
        'the end-of-stages stamp is written only over a run that can vouch for every artefact it measured'

    # And the enumeration is bounded, not only the loop body: PowerShell's `foreach` statement
    # materialises its collection before the body runs once, so a counter inside the body bounds
    # nothing. Measured on 5000 entries: Get-ChildItem + break 76 ms, the lazy enumerator 9 ms.
    # CODE ONLY, NOT COMMENTS -- third time tonight that my own explanation decoyed my own cell. The
    # comment beside the fix quotes `Get-ChildItem` to record the measurement that motivated it, so a
    # naive `-notmatch` over the whole function fails on the sentence explaining why it passes.
    $probeFn = Get-GateFunctionText -Name 'Get-TargetBuildState'
    $probeCode = (($probeFn -split "`n") | Where-Object { -not ($_.TrimStart().StartsWith('#')) }) -join "`n"
    Assert-True -Condition ($probeCode -cmatch 'EnumerateFileSystemInfos' -and $probeCode -notmatch 'Get-ChildItem') `
        'the marker-absence probe streams the directory rather than materialising it'

    Write-Host ''
    Write-Host '-- the wiring: a detector nothing calls detects nothing --' -ForegroundColor Cyan

    # These four read gate.ps1 as TEXT. That is an arrangement check and not an execution check --
    # it proves the call sites exist, not that they run -- and it is here because the failure it
    # rules out is the one that leaves every cell above green while the gate never asks the
    # question: a producer with no consumer.
    $gateText = [System.IO.File]::ReadAllText($gatePath)
    $callSites = @([regex]::Matches($gateText, 'Get-TargetBuildState\s+-TargetDir')).Count
    Assert-True -Condition ($callSites -ge 1) `
        "gate.ps1 CALLS Get-TargetBuildState, not merely defines it ($callSites call site(s))"

    # ORDER, NOT PRESENCE (H on #1009). The first version asserted the two calls with two unrelated
    # -cmatch and would have stayed green with the marks SWAPPED -- a gate that stamps 'complete'
    # before building and 'building' after it satisfies both halves and inverts the whole guard.
    # Counting the conjuncts is not testing them.
    $iBuilding = [regex]::Match($gateText, "Write-TargetBuildState\s+-TargetDir[^\r\n]*-State\s+'building'").Index
    $iComplete = [regex]::Match($gateText, "Write-TargetBuildState\s+-TargetDir[^\r\n]*-State\s+'complete'").Index
    # LastIndexOf, not IndexOf: the FIRST occurrence of this text is inside the comment a few lines
    # above the statement, which quotes the pinned literal to explain why the order matters. My own
    # explanation became a decoy for my own cell -- the third time in this file that a lookup found
    # the same shape in a different role.
    $iStagesDone = $gateText.LastIndexOf('$script:stagesCompleted = $true', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($iBuilding -gt 0 -and $iComplete -gt $iBuilding) `
        "the claim precedes the completion stamp (building at $iBuilding, complete at $iComplete)"
    # And the stamp is on the completion path rather than an exit path: a 'complete' written in the
    # finally would say complete for the interrupted run too, which is the one case that matters.
    #
    # HONEST ABOUT ITS REACH: these are claims about the ORDER OF THE SOURCE, not about execution.
    # They would not catch a `complete` write moved into a function called earlier. H measured the
    # execution order by instants on a bench; this is the cheap standing check beside it.
    # TWO STAMPS NOW WEAR THIS SHAPE -- the canary's release and the end-of-stages one -- so the
    # first draft of this cell measured the wrong instance the moment the second was added. Take
    # the LAST, which is the end-of-stages stamp, and require it immediately before the flag.
    $iCompleteLast = $gateText.LastIndexOf("Write-TargetBuildState -TargetDir `$actualTargetDir -State 'complete'", [System.StringComparison]::Ordinal)
    Assert-True -Condition ($iStagesDone -gt 0 -and $iCompleteLast -gt 0 -and $iStagesDone -gt $iCompleteLast -and ($iStagesDone - $iCompleteLast) -lt 400) `
        "the end-of-stages stamp is written immediately before the completion flag, on the path that finished (stamp $iCompleteLast, flag $iStagesDone)"

    # THE CELL THAT MAKES THE VERDICT LOAD-BEARING. Without this the reading is a line of console
    # output nobody acts on; with it a contaminated target cannot come out GREEN, through the
    # machinery that already forbids one.
    Assert-True -Condition ($gateText -cmatch '\$instrumentSuspect\s*=[^\r\n]*\$targetSuspect') `
        'only the SUSPECT half feeds $instrumentSuspect -- an unknown target is recorded, not reddened'

    Assert-True -Condition ($gateText -cmatch 'targetBuildState\s*=\s*\$script:targetBuildState') `
        'and the whole reading -- state and reason, not a boolean -- reaches the manifest a presser reads'

    # ORDER, and these three are the ones the first two drafts got wrong in both directions.
    $iAbort = $gateText.IndexOf('[gate] ABORTING: $($script:targetBuildState.reason)', [System.StringComparison]::Ordinal)
    $iClaim = [regex]::Match($gateText, "Write-TargetBuildState\s+-TargetDir[^\r\n]*-State\s+'building'").Index
    $iNonce = $gateText.IndexOf('    Write-CanaryNonce', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($iAbort -gt 0 -and $iNonce -gt $iAbort) `
        "a suspect target ends the run BEFORE the first compile, which is the word #455 asked for (abort $iAbort, canary $iNonce)"
    # THE ONE THAT COST EVIDENCE. Claiming the directory before the abort overwrites the very marker
    # just read, so the interrupted run's death is erased by the run that detected it.
    Assert-True -Condition ($iClaim -gt $iAbort) `
        "and the run claims the target only AFTER that abort, so detecting contamination cannot destroy its own evidence (claim $iClaim)"
    Assert-True -Condition ($gateText -cmatch "HARNESS-BROKE: every stage passed, but") `
        'the terminal verdict also refuses a suspect target -- the exit code, not only the manifest'

    # READ BEFORE USE, second instance in this file in one night. `Write-RunManifest` reads both
    # overlap fields unconditionally, and the early abort calls it long before the stage block
    # assigns them -- under StrictMode that is a THROW, so the abort would write no manifest and
    # never reach its own exit. The abort would have looked implemented and done nothing.
    $iInit = $gateText.IndexOf('$script:psSuitesStartedEarly = $false', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($iInit -gt 0 -and $iInit -lt $iAbort) `
        "the overlap fields are initialised before the early abort can serialize a manifest (init $iInit, abort $iAbort)"

    # THE DEADLOCK. The canary's own exit sits between the two stamps, so without a release there the
    # marker stays `building` -- and the next run reads `interrupted` and refuses before the canary,
    # making the "re-run when the lock releases" instruction impossible to follow.
    $iCanaryRelease = $gateText.IndexOf("Write-TargetBuildState -TargetDir `$actualTargetDir -State 'complete'", [System.StringComparison]::Ordinal)
    Assert-True -Condition ($iCanaryRelease -gt $iClaim -and $iCanaryRelease -lt $iCompleteLast) `
        "the canary's own exit releases the target BETWEEN the claim and the end-of-stages stamp, so a documented re-run is not locked out by this run's marker (claim $iClaim, release $iCanaryRelease, end $iCompleteLast)"
} catch {
    # A suite that dies must SAY what killed it. Without this, the finally below reports
    # "ran 0 assertions" -- a red that names a count and not a cause, which is the same
    # unreadable colour this repository keeps replacing everywhere else.
    Write-Host "HARNESS-BROKE: the suite threw before finishing -- $($_.Exception.Message)" -ForegroundColor Magenta
    Write-Host "  line $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim())" -ForegroundColor Magenta
} finally {
    Write-Host ''
    try { [System.IO.Directory]::Delete($fixtureRoot, $true) } catch { }
    if ($script:total -ne $ExpectedAssertionCount) {
        # A suite that stops running some of its cells must not report the colour of the cells it
        # did run: the count is the only thing that can tell a green from a green-shaped gap.
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-target-build-state: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-target-build-state: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
