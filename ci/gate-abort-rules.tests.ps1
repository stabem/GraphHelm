# #191: the two decisions in ci/gate.ps1 that nothing could redden.
#
# #191 states its own test: "a change that made the door refuse unconditionally would pass every
# test in this repository -- because no test runs the script. The same is true of the manifest rule
# and of the canary's abort branch." I ran it against `origin/main` at 21e370b0, each mutation
# alone, the file restored by byte copy and sha-compared, the verdict taken from the whole pinned
# suite list rather than from the suite I expected to catch it:
#
#   the CARGO_TARGET_DIR door refuses unconditionally   ci/gate-target-dir.tests.ps1  14/29  CAUGHT
#   the manifest rule deleted                           30 discovered, 30 passed      NOT CAUGHT
#   the canary's abort made unreachable                 30 discovered, 30 passed      NOT CAUGHT
#
# The door is covered. These two are the remainder, and neither is a small branch: one turns a green
# run red when its manifest never reached the server, the other stops the whole gate before any
# stage can produce evidence from a build environment the run cannot trust.
#
# THE BLOCKS ARE CUT OUT OF gate.ps1 AND DRIVEN, never re-typed here. Running the file would run the
# gate, and a copy of the rule in this suite would be a second oracle that agrees until the day it
# does not -- the shape ci/gate-verdict.tests.ps1 had to fix twice.
#
# THE SLICING IS STRUCTURAL, NOT BY OFFSET. A fixed character window over a subject that grows is
# how ci/gate-slot-claim.tests.ps1 came to pass while the block it measured had been emptied: the
# window overshot into the next statement. These find the opening line and then the closing brace at
# the SAME indent, so an edit inside the block moves nothing.
#
# DECLARED ASSERTION COUNT, derived by counting the calls:
#   2  the manifest block is locatable and is not empty
#   1  an unpublished manifest reaches $failed
#   1  by the name the verdict prints
#   1  CONTROL: a published one does not
#   2  the canary block is locatable and is not empty
#   1  a failed canary exits non-zero
#   1  CONTROL: a passing canary does not exit at all
#   1  and the abort says which of its two voices it used
#   1  the reap loop is locatable in gate.ps1
#   1  and it still uses taskkill, not a bare Process.Kill() that reaches only the wrapper
#   1  ARRANGEMENT: the synthetic wrapper spawned its own child and recorded its pid
#   1  ARRANGEMENT: the child is alive before the reap runs
#   1  the reap ends the wrapper process
#   1  and the wrapper's own child too -- the whole tree
#   1  the in-line Studio stage is locatable in gate.ps1
#   1  and it still invokes the fail-closed Studio script
#   1  it no longer joins a background Studio process
#   1  no early Studio process can survive into abort cleanup
#   1  the manifest's requiredFeaturesExcluded expression is locatable
#   1  ARRANGEMENT: gate.ps1's early init lines for both fields are found, extracted not retyped
#   1  on the CANARY-ABORT PATH -- only the early inits set -- it does not throw under StrictMode
#   1  and reads as UNREADABLE (null), not as an empty array dressed up as "the stage ran"
#   1  a report that WAS read and named nothing excluded reads as an empty array, not null
#   1  NEGATIVE CONTROL: with NEITHER variable set at all, the same expression DOES throw -- proving
#      the cells above measure the early init, not a StrictMode-tolerant accident
#   1  the required-features argument-building block is locatable in gate.ps1
#   1  and it no longer emits an explicit-value -Full: token
#   1  a FULL-scope run reaches required-features.ps1 and writes its scope report
#   1  a SCOPED run (excluding the gating crate) reaches required-features.ps1 and writes it too
#   1  NEGATIVE CONTROL: the finally-detector names the variable the shipped cleanup threw on
#   1  DECOY: one added init silences it, so it measures the init and not the name
#   1  no finally in this suite reads a variable its try may never have assigned
$ExpectedAssertionCount = 37

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0

function Read-PidFile {
    <#
    .SYNOPSIS
        The pid written in a file, or $null while there is not one yet.
    .DESCRIPTION
        A PRESENT FILE IS NOT A WRITTEN FILE. The distinction is the whole point: waiting on
        `Test-Path` returns the instant the path appears, which for `Out-File` is before the bytes
        land. Everything this returns is a pid a caller can use; every other state -- absent,
        empty, half-written, not a number -- is $null, so one wait loop covers all of them.
    #>
    param([Parameter(Mandatory)] [string] $Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $raw = try { Get-Content -LiteralPath $Path -Raw -ErrorAction Stop } catch { return $null }
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    $parsed = 0
    if ([int]::TryParse($raw.Trim(), [ref] $parsed) -and $parsed -gt 0) { return $parsed }
    return $null
}

function Wait-ProcessFromPidFile {
    <#
    .SYNOPSIS
        Waits for a pid file and for that pid to identify a live process.
    .DESCRIPTION
        A readable pid is not yet a usable process identity. Under load the wrapper can publish
        the pid just before the child is visible to Get-Process, and a child that exited quickly
        must not be confused with a successful arrangement. Keep both observations inside one
        bounded wait so the caller never probes Get-Process with a null identity.
    #>
    param(
        [Parameter(Mandatory)] [string] $Path,
        [int] $TimeoutSeconds = 10
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $processId = Read-PidFile -Path $Path
        if ($null -ne $processId) {
            $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
            if ($null -ne $process) { return $process }
        }
        Start-Sleep -Milliseconds 100
    }
    return $null
}

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateLines = [System.IO.File]::ReadAllLines($gatePath)

# Find a block by its opening line and the closing brace at the same indent.
function Get-Block {
    param([Parameter(Mandatory)] [string] $Opening)
    for ($i = 0; $i -lt $gateLines.Count; $i++) {
        if ($gateLines[$i].Trim() -ne $Opening) { continue }
        $indent = $gateLines[$i].Length - $gateLines[$i].TrimStart().Length
        $close = (' ' * $indent) + '}'
        for ($j = $i + 1; $j -lt $gateLines.Count; $j++) {
            if ($gateLines[$j].TrimEnd() -eq $close) {
                return ($gateLines[$i..$j] -join "`n")
            }
        }
        return $null
    }
    return $null
}

Write-Host ''
Write-Host '-- an unpublished manifest turns a green run red (#152, uncovered until now) --' -ForegroundColor Cyan

$manifestBlock = Get-Block -Opening 'if ($script:manifestNotPublished) {'
Assert-True -Condition ($null -ne $manifestBlock) `
    'the manifest rule is locatable in gate.ps1, or every assertion below is about an empty string'
# NOT JUST LOCATABLE. A block that had been emptied would still be found and would still "pass" a
# cell that only checked it exists -- the exact way a sibling suite stayed green over a deleted call.
Assert-True -Condition ($manifestBlock -and $manifestBlock.Contains('$failed')) `
    'and it still adds to $failed -- a located but emptied block is the failure this cell exists for'

if ($manifestBlock) {
    $script:manifestNotPublished = $true
    $failed = @()
    # DOT-SOURCED, not called. `&` runs the block in a child scope, so its `$failed +=` creates a
    # local and the assertion below reads an untouched array -- green for the wrong reason if the
    # assertion had been `is-empty`. Measured: with `&` the count was 0 while the block's own
    # Write-Host proved it had run.
    . ([scriptblock]::Create($manifestBlock)) | Out-Null
    Assert-True -Condition ($failed.Count -eq 1) `
        'a run whose manifest never reached the server reaches $failed, so the verdict is RED'
    # The NAME matters as much as the count: `Get-GateVerdictLines` prints the list, and a stage
    # named something else sends a reader somewhere else.
    Assert-True -Condition ($failed -contains 'run manifest not published') `
        "and by the name the verdict prints (got: $($failed -join ', '))"

    # CONTROL. Without it a block that appended unconditionally would satisfy both assertions above,
    # and unconditional is what a careless edit produces.
    $script:manifestNotPublished = $false
    $failed = @()
    . ([scriptblock]::Create($manifestBlock)) | Out-Null
    Assert-True -Condition ($failed.Count -eq 0) `
        'CONTROL: a run whose manifest WAS published adds nothing -- the rule discriminates'
}

Write-Host ''
Write-Host '-- a failed canary aborts the gate before any stage can be read as evidence (#152) --' -ForegroundColor Cyan

$canaryBlock = Get-Block -Opening 'if (-not $canaryPassed) {'
Assert-True -Condition ($null -ne $canaryBlock) `
    'the canary abort is locatable in gate.ps1'
Assert-True -Condition ($canaryBlock -and $canaryBlock.Contains('exit 1')) `
    'and it still exits -- a block that only PRINTS lets 56 stages run on a tree it does not trust'

if ($canaryBlock) {
    # A CHILD PROCESS, because the block's whole point is `exit`, and an `exit` in this process ends
    # the harness instead of being observed. The exit code IS the assertion; everything the block
    # calls is stubbed so nothing reaches a repository.
    $harness = @'
$ErrorActionPreference = 'Stop'
function Write-Host { param([Parameter(ValueFromRemainingArguments)] $Rest) }
function Write-RunManifest { param([Parameter(ValueFromRemainingArguments)] $Rest) 'stub-manifest' }
function Read-SlotLockSnapshot { $null }
# #455 added a target-marker release inside the canary block. Stubbed here rather than lifted,
# because the subject of this suite is the ABORT RULE: the block must still exit, whatever it calls
# on the way out. A block that grows a call this harness does not know goes red as
# "term not recognized" with no count line -- which is what happened, and is why the stub is named
# in the same breath as the two variables the call needs.
function Write-TargetBuildState { param([Parameter(ValueFromRemainingArguments)] $Rest) }
$actualTargetDir = 'stub-target'
$gatedHeadAtStart = 'stubhead'
$emptyArtifacts = @()
$slotLockAtStart = $null
$canaryOutcome = [pscustomobject]@{ passed = $CANARY; status = 'RED'; cargoLockObserved = $LOCKED }
$canaryPassed = $canaryOutcome.passed
BLOCK
exit 0
'@
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    try {
        $failScript = Join-Path $tempDir 'fail.ps1'
        [System.IO.File]::WriteAllText($failScript,
            $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$false').Replace('$LOCKED', '$false'))
        & powershell -NoProfile -ExecutionPolicy Bypass -File $failScript 2>&1 | Out-Null
        Assert-True -Condition ($LASTEXITCODE -eq 1) `
            "a failed canary exits 1, so nothing past it is reported as evidence (got $LASTEXITCODE)"

        # CONTROL: the same block, the same harness, a canary that passed. If this also exited, the
        # cell above would be measuring the harness rather than the rule.
        $passScript = Join-Path $tempDir 'pass.ps1'
        [System.IO.File]::WriteAllText($passScript,
            $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$true').Replace('$LOCKED', '$false'))
        & powershell -NoProfile -ExecutionPolicy Bypass -File $passScript 2>&1 | Out-Null
        Assert-True -Condition ($LASTEXITCODE -eq 0) `
            "CONTROL: a canary that passed falls through and the gate goes on (got $LASTEXITCODE)"

        # THE TWO VOICES. #751 split this: a canary that never RAN because another run holds the
        # build lock is a QUEUE, not a finding about this head, and it is spoken in magenta with
        # different words. Both still exit 1 -- the distinction is what the operator reads, and a
        # merge of the two branches would send somebody to debug a contamination that was never
        # observed.
        $queuedScript = Join-Path $tempDir 'queued.ps1'
        $queuedText = $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$false').Replace('$LOCKED', '$true')
        # This one keeps Write-Host, so the words can be read.
        $queuedText = $queuedText.Replace("function Write-Host { param([Parameter(ValueFromRemainingArguments)] `$Rest) }", '')
        [System.IO.File]::WriteAllText($queuedScript, $queuedText)
        $spoken = (& powershell -NoProfile -ExecutionPolicy Bypass -File $queuedScript 2>&1 | Out-String)
        Assert-True -Condition ($spoken -match 'did not run' -and $spoken -notmatch 'ABORTING') `
            'a canary blocked by another run says QUEUE, not contamination -- the two voices stay apart (#751)'
    } finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
Write-Host '-- a background stage reaped after an abort takes its children with it (#1003 review, X) --' -ForegroundColor Cyan

# `.Process.Kill()` on PowerShell 5.1 ends only the `powershell.exe` WRAPPER `Start-BackgroundStage`
# launches, not the `npm`/`node` children it spawns to run the stage -- a killed wrapper leaves
# `npm ci` still writing into `apps/studio/node_modules`, exactly the interference this reap exists
# to prevent, one process down. Extracted by text, not retyped: the fix is `taskkill /T`, and this
# proves that flag against a REAL two-level process tree, not against a claim about what it does.
$reapBlock = Get-Block -Opening 'foreach ($started in @($script:studioStarted, $script:psSuitesStarted)) {'
Assert-True -Condition ($null -ne $reapBlock) `
    'the reap loop is locatable in gate.ps1'
Assert-True -Condition ($reapBlock -and $reapBlock.Contains('taskkill')) `
    'and it still uses taskkill, not a bare Process.Kill() that reaches only the wrapper'

if ($reapBlock) {
    $wrapperScript = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-wrapper-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.ps1')
    $pidFile = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-child-pid-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt')
    [System.IO.File]::WriteAllText($wrapperScript, @"
`$child = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120' -PassThru
`$child.Id | Out-File -LiteralPath '$pidFile'
Start-Sleep -Seconds 120
"@)
    # BOTH OF THESE ARE READ BY THE `finally` BELOW, SO BOTH MUST EXIST BEFORE THE `try` CAN FAIL.
    # Under StrictMode reading an unassigned variable is a TERMINATING error, so a `finally` that
    # touches a variable the `try` had not reached yet throws a SECOND error over the first -- and
    # the second one is what reaches the log. On 2026-09-11 that turned a missed 10-second
    # arrangement deadline into `The variable '$childProcessId' cannot be retrieved`, and the
    # sentence naming the real cause was never printed at all (#1043). The `if ($leftover)` guard
    # below already treats $null correctly, so initialising costs nothing and buys the message.
    $wrapper = $null
    $childProcessId = $null
    try {
        $wrapper = Start-Process -FilePath 'powershell.exe' `
            -ArgumentList '-NoProfile', '-File', $wrapperScript -PassThru
        $null = $wrapper.Handle
        # WAIT FOR A PID, NOT FOR A PATH. `Out-File` creates the file and then writes it, so
        # `Test-Path` goes true while the contents are still empty -- and the next line's `[int]`
        # cast on an empty string THROWS. `Assert-True` records and returns rather than halting, so
        # execution reached that cast every time; what the log then showed was the ARRANGEMENT cell
        # PASSING (the path really did exist) immediately followed by the cleanup complaining that
        # `$childProcessId` was never set. Both true, and together they are the signature of this
        # race. Measured on #1042: the identical blob 769195eb went GREEN on one head and RED on the
        # next with nothing under `ci/` changed between them.
        # WAIT FOR A PID AND A LIVE PROCESS, NOT FOR A PATH. `Out-File` creates the file before
        # its bytes land, and under load Windows can publish the pid just before Get-Process can
        # observe the child. If the identity never becomes usable, this is a broken observer, not
        # a production verdict. Exit 2 after the finally below so the suite reports HARNESS-BROKE
        # and never calls Get-Process with a null id.
        $childProcess = Wait-ProcessFromPidFile -Path $pidFile -TimeoutSeconds 10
        if ($null -eq $childProcess) {
            Write-Host 'HARNESS-BROKE: the synthetic child pid never became a live process identity within 10 seconds.' -ForegroundColor Magenta
            exit 2
        }
        $childProcessId = [int]$childProcess.Id
        Assert-True -Condition ($childProcessId -gt 0) `
            'ARRANGEMENT: the synthetic wrapper spawned its own child and recorded a READABLE pid'
        Assert-True -Condition (-not $childProcess.HasExited) `
            "ARRANGEMENT: the child ($childProcessId) is alive before the reap runs, or the cells below prove nothing"

        # THE SUBJECT: gate.ps1's own reap block, driven with a real Started-shaped object.
        $script:studioStarted = [pscustomobject]@{ Name = 'synthetic stage'; Process = $wrapper }
        $script:psSuitesStarted = $null
        . ([scriptblock]::Create($reapBlock)) | Out-Null

        $wrapperGone = $false
        $childGone = $false
        $waitDeadline = (Get-Date).AddSeconds(10)
        while ((Get-Date) -lt $waitDeadline) {
            $wrapperGone = ($null -eq (Get-Process -Id $wrapper.Id -ErrorAction SilentlyContinue))
            $childGone = ($null -eq (Get-Process -Id $childProcessId -ErrorAction SilentlyContinue))
            if ($wrapperGone -and $childGone) { break }
            Start-Sleep -Milliseconds 200
        }
        Assert-True -Condition $wrapperGone `
            'the reap ends the wrapper process'
        # NEGATIVE CONTROL: if the reap regresses to killing only the wrapper, $wrapperGone is
        # true but this surviving descendant remains false, so the fixture fails closed.
        Assert-True -Condition $childGone `
            "and the wrapper's own child ($childProcessId) too -- the whole tree, not just the process gate.ps1 held a handle to"
    } finally {
        Remove-Item -LiteralPath $wrapperScript -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
        foreach ($leftover in @($wrapper.Id, $childProcessId)) {
            if ($leftover) {
                try { & taskkill /T /F /PID $leftover 2>&1 | Out-Null } catch {}
            }
        }
    }
}

Write-Host ''
Write-Host '-- the Studio stage remains fail-closed after leaving the background (#1102) --' -ForegroundColor Cyan

# #1102 removed only the early process. The same child script still runs inside Invoke-Stage, so
# its stdout, stderr and native exit code take the ordinary capture path. Four source assertions
# replace the four join assertions above: the old join no longer exists by design.
$studioBlock = Get-Block -Opening "Invoke-Stage 'apps/studio (npm)' {"
$gateText = $gateLines -join "`n"
Assert-True -Condition ($null -ne $studioBlock) `
    'the in-line Studio stage is locatable in gate.ps1'
Assert-True -Condition ($studioBlock -and $studioBlock.Contains("-File (Join-Path `$repositoryRoot 'ci/studio-stage.ps1')")) `
    'and it still invokes the fail-closed Studio script'
Assert-True -Condition ($studioBlock -and -not $studioBlock.Contains('Complete-BackgroundStage')) `
    'the Studio stage no longer joins a background process'
Assert-True -Condition (-not $gateText.Contains("Start-BackgroundStage -Name 'apps/studio (npm)'")) `
    'no early Studio process can survive into abort cleanup'

Write-Host ''
Write-Host '-- #1019 review (B): the manifest field must not throw before the stage that fills it runs --' -ForegroundColor Cyan

# EXTRACTED BY TEXT, one line: the required-features stage runs long after the canary can already
# have aborted, and a manifest write on THAT path evaluates this expression with only gate.ps1's
# own top-of-file inits (if any) in scope -- never the stage's own assignments.
$excludedLine = @($gateLines | Where-Object { $_.TrimStart().StartsWith('requiredFeaturesExcluded = if (') }) |
Select-Object -First 1
Assert-True -Condition ($null -ne $excludedLine) `
    'the manifest''s requiredFeaturesExcluded expression is locatable'

if ($excludedLine) {
    $expr = $excludedLine.Trim().Substring('requiredFeaturesExcluded = '.Length)

    # A REAL CHILD PROCESS FOR EACH CASE, the same isolation the canary cells above use and for the
    # same reason: `$script:` inside a dynamically created scriptblock binds to whichever scope
    # invokes it, so running the early-init case and the bare case back to back IN this file's own
    # process would leak the first case's assignments into the second and prove nothing about either.
    function Test-RequiredFeaturesExpression {
        param([string] $Prelude)
        # `$ErrorActionPreference = 'Stop'`, MATCHING gate.ps1's OWN SETTING (near its top): without
        # it a StrictMode violation is a non-terminating error under PowerShell's own default --
        # printed, not thrown, and the script that hit it still exits 0. Omitting this line here
        # measured a claim about a script that behaves differently from the one under test.
        $body = "Set-StrictMode -Version 2.0`n`$ErrorActionPreference = 'Stop'" + "`n" + $Prelude + "`n" +
        '$r = ' + $expr + "`n" +
        'if ($null -eq $r) { Write-Host "RESULT:NULL" } else { Write-Host "RESULT:COUNT=$(@($r).Count)" }'
        $tempScript = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-expr-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.ps1')
        [System.IO.File]::WriteAllText($tempScript, $body)
        # NOT `2>&1` -- the bare case is BUILT to throw, and under this file's own
        # `$ErrorActionPreference = 'Stop'` (top of file), merging the child's stderr into the
        # success stream turns that expected stderr line into a terminating NativeCommandError here,
        # crashing the harness instead of letting it observe the exit code it exists to check
        # (measured live while writing this cell: the exact hazard #1003 names in `ci/gate.ps1`'s
        # own `Invoke-Stage` comment, reproduced by this suite about a different subject). Scoped
        # `Continue`, the same remedy, so stdout and stderr stay two separate streams read after the
        # process ends rather than one that can abort mid-read.
        $previousPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = (& powershell -NoProfile -ExecutionPolicy Bypass -File $tempScript 2>&1 | Out-String)
            $exitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $previousPreference
            Remove-Item -LiteralPath $tempScript -Force -ErrorAction SilentlyContinue
        }
        return [pscustomobject]@{ ExitCode = $exitCode; Output = $output }
    }

    # THE CANARY-ABORT PATH: only gate.ps1's early, top-of-file inits have run by the time this
    # expression would be reached there -- the required-features STAGE that would otherwise fill
    # these in is never started on this path.
    #
    # THE PRELUDE IS EXTRACTED, NOT RETYPED (#1019 review, B, second pass caught a defect the first
    # version of this cell had by construction: it hardcoded `$script:requiredFeaturesReportUnreadable
    # = $true` as "what the early init should say" rather than reading what gate.ps1's early init
    # ACTUALLY says, so a regression on gate.ps1's own line -- exactly B's finding -- would have gone
    # on passing this cell forever). The two assignment lines are found by their variable name and
    # used verbatim.
    # THE FIRST OCCURRENCE OF EACH, not every occurrence: both variables are also reset locally
    # right before the stage runs (so a fresh run's manifest, if the stage DOES complete, is not
    # reading a previous run's leftovers), and `Unreadable` is set again inside three failure
    # branches -- all of them later in the file than the early init this cell is about. File order is
    # the ordering that matters here, because it is the ordering gate.ps1 itself executes in.
    $excludedInitLine = @($gateLines | Where-Object {
            $_.TrimStart().StartsWith('$script:requiredFeaturesExcluded = @()')
        }) | Select-Object -First 1
    $unreadableInitLine = @($gateLines | Where-Object {
            $_.TrimStart().StartsWith('$script:requiredFeaturesReportUnreadable = ')
        }) | Select-Object -First 1
    Assert-True -Condition ($null -ne $excludedInitLine -and $null -ne $unreadableInitLine) `
        'ARRANGEMENT: gate.ps1 has an early init line for both fields, or the cells below measure a prelude nobody''s early init actually has'

    $abort = Test-RequiredFeaturesExpression -Prelude ($excludedInitLine.Trim() + "`n" + $unreadableInitLine.Trim())
    Assert-True -Condition ($abort.ExitCode -eq 0) `
        "on the CANARY-ABORT PATH -- only the early inits set -- it does not throw under StrictMode (exit $($abort.ExitCode): $($abort.Output))"
    Assert-True -Condition ($abort.Output -like '*RESULT:NULL*') `
        "and reads as UNREADABLE (null), not as an empty array dressed up as `"the stage ran and excluded nothing`" (got: $($abort.Output))"

    # THE CASE THE OLD CELL WAS ACTUALLY NAMING, restored under its own name: a report WAS read
    # (`Unreadable` goes `$false` only at the one call site that follows a successful parse) and it
    # genuinely named nothing excluded.
    $readEmpty = Test-RequiredFeaturesExpression -Prelude (
        '$script:requiredFeaturesExcluded = @()' + "`n" + '$script:requiredFeaturesReportUnreadable = $false')
    Assert-True -Condition ($readEmpty.Output -like '*RESULT:COUNT=0*') `
        "a report that WAS read and named nothing excluded reads as an empty array, not null (got: $($readEmpty.Output))"

    # NEGATIVE CONTROL: neither variable defined at all -- the shape B actually found (the field
    # assigned only inside the stage; a manifest write the canary-abort path reaches first sees
    # neither). If this does NOT throw, the cells above are not measuring the early init.
    $bare = Test-RequiredFeaturesExpression -Prelude ''
    Assert-True -Condition ($bare.ExitCode -ne 0) `
        'NEGATIVE CONTROL: with NEITHER variable set at all, the same expression DOES throw -- proving the cells above measure the early init, not a StrictMode-tolerant accident'
}

Write-Host ''
Write-Host '-- #1019 follow-up: the required-features argument list survives the -File boundary in both scope states --' -ForegroundColor Cyan

# Two real defects lived in the inline argument list this block replaces, both measured directly
# against the real `ci/required-features.ps1` across the same `-File` process boundary this suite
# always drives through: `-Full:([bool]$script:gateScope.full)` -- an explicit value on a `[switch]`
# parameter -- arrives as a bare STRING under `-File` and `[switch]` refuses to convert it ("Cannot
# convert value \"System.String\" to type \"...SwitchParameter\""), exit 1, every run; and
# `-InScopeCrates ''` -- an EMPTY STRING argument -- is dropped entirely at the boundary rather than
# received as empty, so the NEXT token is read as `-InScopeCrates`'s value and PowerShell reports
# "Missing an argument for parameter 'InScopeCrates'" instead. A FULL run hit the second defect
# before the first was ever reached, because `$inScopeCratesArg` is always empty on FULL.
#
# NOT `Get-Block`: this is a flat sequence of statements inside `Invoke-Stage`'s scriptblock, not a
# brace-delimited block of its own, so it is sliced by start/end line text instead.
function Get-Slice {
    param([string] $StartsWith, [string] $EndsWith)
    $startIndex = -1
    for ($i = 0; $i -lt $gateLines.Count; $i++) {
        if ($gateLines[$i].TrimStart().StartsWith($StartsWith)) { $startIndex = $i; break }
    }
    if ($startIndex -lt 0) { return $null }
    for ($j = $startIndex; $j -lt $gateLines.Count; $j++) {
        if ($gateLines[$j].TrimStart().StartsWith($EndsWith)) {
            return ($gateLines[$startIndex..$j] -join "`n")
        }
    }
    return $null
}

$argsBlock = Get-Slice -StartsWith '$requiredFeaturesArgs = @(' -EndsWith '@requiredFeaturesArgs'
Assert-True -Condition ($null -ne $argsBlock) `
    'the required-features argument-building block is locatable in gate.ps1'
Assert-True -Condition ($argsBlock -and -not $argsBlock.Contains('-Full:')) `
    'and it no longer emits an explicit-value -Full: token, the shape that could never bind under -File'

if ($argsBlock) {
    # A REAL CHILD LAUNCH, not a claim about the argument list: this drives the extracted block
    # verbatim, which itself invokes the real `ci/required-features.ps1` through the real `-File`
    # boundary -- the only way either defect above was ever actually caught.
    function Invoke-RequiredFeaturesArgsBlock {
        param([bool] $Full, [string[]] $Crates)
        $transcriptPath = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-rf-transcript-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt')
        $requiredFeaturesScopeReportPath = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-rf-report-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json')
        [System.IO.File]::WriteAllText($transcriptPath, 'nothing built in this synthetic transcript')
        try {
            $repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
            $script:gateScope = [pscustomobject]@{ full = $Full; crates = $Crates }
            . ([scriptblock]::Create($argsBlock)) | Out-Null
            return [pscustomobject]@{
                ExitCode      = $LASTEXITCODE
                ReportWritten = (Test-Path -LiteralPath $requiredFeaturesScopeReportPath)
            }
        } finally {
            Remove-Item -LiteralPath $transcriptPath -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $requiredFeaturesScopeReportPath -Force -ErrorAction SilentlyContinue
        }
    }

    $fullResult = Invoke-RequiredFeaturesArgsBlock -Full $true -Crates @()
    Assert-True -Condition $fullResult.ReportWritten `
        "a FULL-scope run reaches required-features.ps1 and writes its scope report (exit $($fullResult.ExitCode))"

    # Excludes the gating crate deliberately -- the shape #990 measured broken before this PR's own
    # scoping fix existed, driven here through the SAME argument list the switch/empty-string defects
    # lived in.
    $scopedResult = Invoke-RequiredFeaturesArgsBlock -Full $false -Crates @('some-other-crate')
    Assert-True -Condition $scopedResult.ReportWritten `
        "a SCOPED run (excluding the gating crate) reaches required-features.ps1 and writes its scope report too (exit $($scopedResult.ExitCode))"
}

Write-Host ''
Write-Host '-- a `finally` must not read a variable the `try` may never have reached (#1043) --' -ForegroundColor Cyan

# THE DETECTOR IS STRUCTURAL, NOT LEXICAL. Grepping for a name would answer a question about this
# one defect; the class is "any variable a finally reads that the try had not assigned yet", and it
# arrives under a different name each time. This walks the AST: for every try/finally, every
# variable READ in the finally must have an assignment somewhere BEFORE the try begins -- unless the
# finally assigns it itself (its own foreach variable, for instance).
#
# DECLARED LIMIT: assignment is matched by name across the whole file, not by scope, so a same-named
# assignment inside an unrelated function would satisfy this check. That is the direction that fails
# SAFE for a guard whose job is to catch an omission, and narrowing it would cost more than it buys.
function Get-FinallyVariablesNotPreInitialised {
    param([Parameter(Mandatory)] [string] $Source)

    $errors = $null
    $tokens = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($Source, [ref]$tokens, [ref]$errors)
    if ($errors -and $errors.Count -gt 0) {
        throw "the subject does not parse, so a clean result would mean nothing: $($errors[0].Message)"
    }

    # $_ and friends are always bound; naming them would make every finally an offender.
    $automatic = @('_', 'PSItem', 'null', 'true', 'false', 'args', 'this', 'PSCmdlet', 'PSBoundParameters',
                   'ErrorActionPreference', 'LASTEXITCODE', 'PWD', 'Host', 'MyInvocation')

    $offenders = New-Object System.Collections.Generic.List[string]
    $tries = $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.TryStatementAst] -and $null -ne $n.Finally }, $true)
    foreach ($try in $tries) {
        $tryStart = $try.Extent.StartOffset

        $assignedHere = @()
        $assignedHere += @($try.Finally.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] }, $true) |
            ForEach-Object { $_.Left } |
            Where-Object { $_ -is [System.Management.Automation.Language.VariableExpressionAst] } |
            ForEach-Object { $_.VariablePath.UserPath })
        $assignedHere += @($try.Finally.FindAll({ param($n) $n -is [System.Management.Automation.Language.ForEachStatementAst] }, $true) |
            ForEach-Object { $_.Variable.VariablePath.UserPath })

        $preInit = @($ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] }, $true) |
            Where-Object { $_.Extent.StartOffset -lt $tryStart } |
            ForEach-Object { $_.Left } |
            Where-Object { $_ -is [System.Management.Automation.Language.VariableExpressionAst] } |
            ForEach-Object { $_.VariablePath.UserPath })

        foreach ($use in $try.Finally.FindAll({ param($n) $n -is [System.Management.Automation.Language.VariableExpressionAst] }, $true)) {
            $name = $use.VariablePath.UserPath
            if ($automatic -contains $name) { continue }
            if ($assignedHere -contains $name) { continue }
            if ($preInit -contains $name) { continue }
            if (-not $offenders.Contains($name)) { $offenders.Add($name) }
        }
    }
    return @($offenders.ToArray())
}

# NEGATIVE CONTROL FIRST, and it carries the full payload: this is the shape that shipped, reduced
# to the two statements that matter. If the detector cannot name `childProcessId` here, its silence
# on the real file below proves nothing.
$sick = @'
try {
    $wrapper = Start-Process -FilePath 'powershell.exe' -PassThru
    Assert-True -Condition $false 'ARRANGEMENT: the wrapper recorded its pid'
    $childProcessId = 42
} finally {
    foreach ($leftover in @($wrapper.Id, $childProcessId)) {
        if ($leftover) { taskkill /T /F /PID $leftover }
    }
}
'@
$sickOffenders = Get-FinallyVariablesNotPreInitialised -Source $sick
Assert-True -Condition ($sickOffenders -contains 'childProcessId') `
    "NEGATIVE CONTROL: the detector names the variable the cleanup would have thrown on (got: $($sickOffenders -join ', '))"

# `.Count` is read through @() on purpose: under StrictMode an empty result unrolls to $null and
# `$null.Count` is a terminating error -- the same family of trap as the defect this cell guards,
# met while writing the guard for it.
# THE DECOY: the same source, one line added. A detector that still complains is measuring the
# presence of the name rather than the presence of the initialisation.
$cured = "`$wrapper = `$null`n`$childProcessId = `$null`n" + $sick
$curedOffenders = Get-FinallyVariablesNotPreInitialised -Source $cured
Assert-True -Condition (@($curedOffenders).Count -eq 0) `
    "DECOY: initialising both before the try silences it, so it measures the init and not the name (got: $($curedOffenders -join ', '))"

# THE SUBJECT.
$selfOffenders = Get-FinallyVariablesNotPreInitialised -Source ([System.IO.File]::ReadAllText($PSCommandPath))
Assert-True -Condition (@($selfOffenders).Count -eq 0) `
    "no finally in this suite reads a variable its try may never have assigned (got: $($selfOffenders -join ', '))"

Write-Host ''
Write-Host '-- a PRESENT pid file is not a WRITTEN one (#1043, measured on #1042) --' -ForegroundColor Cyan

# THE DISCRIMINATING CASE IS THE EMPTY-BUT-PRESENT FILE. That is the state `Out-File` passes
# through, it is what `Test-Path` could not tell apart, and it is the one an assertion on the path
# reports as a healthy arrangement one line before the cast on its contents throws.
$pidProbe = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-pidprobe-" + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt')
try {
    Assert-True -Condition ($null -eq (Read-PidFile -Path $pidProbe)) `
        'an ABSENT pid file reads as no pid yet, so the wait keeps waiting'

    Set-Content -LiteralPath $pidProbe -Value '' -Encoding ASCII
    Assert-True -Condition ((Test-Path -LiteralPath $pidProbe) -and $null -eq (Read-PidFile -Path $pidProbe)) `
        'THE RACE: a file that EXISTS but is empty still reads as no pid -- the state Test-Path called ready'

    Set-Content -LiteralPath $pidProbe -Value 'not-a-pid' -Encoding ASCII
    Assert-True -Condition ($null -eq (Read-PidFile -Path $pidProbe)) `
        'and a half-written or garbage value is no pid either, rather than an exception one line later'

    Set-Content -LiteralPath $pidProbe -Value '4321' -Encoding ASCII
    Assert-True -Condition ((Read-PidFile -Path $pidProbe) -eq 4321) `
        'CONTROL: a written pid IS returned, so the three cells above are not passing on a reader that never succeeds'
} finally {
    Remove-Item -LiteralPath $pidProbe -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "INCOMPLETE: ran $script:total assertions, expected $ExpectedAssertionCount" -ForegroundColor Yellow
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total of $ExpectedAssertionCount" -ForegroundColor Green
exit 0
