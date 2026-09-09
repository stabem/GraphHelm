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
#   1  the studio-stage join branch is locatable in gate.ps1
#   1  and it still calls Complete-BackgroundStage, not a deleted stand-in
#   1  the joined child's STAGE_FAILED_AT line reaches Invoke-Stage's own capture
#   1  and so does an ordinary diagnostic line from the same join
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
$ExpectedAssertionCount = 30

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
    try {
        $wrapper = Start-Process -FilePath 'powershell.exe' `
            -ArgumentList '-NoProfile', '-File', $wrapperScript -PassThru
        $null = $wrapper.Handle
        $deadline = (Get-Date).AddSeconds(10)
        while (-not (Test-Path -LiteralPath $pidFile) -and (Get-Date) -lt $deadline) {
            Start-Sleep -Milliseconds 100
        }
        Assert-True -Condition (Test-Path -LiteralPath $pidFile) `
            'ARRANGEMENT: the synthetic wrapper spawned its own child and recorded its pid'
        $childProcessId = [int](Get-Content -LiteralPath $pidFile -Raw).Trim()
        Assert-True -Condition ($null -ne (Get-Process -Id $childProcessId -ErrorAction SilentlyContinue)) `
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
Write-Host '-- the joined studio child''s own output reaches the manifest, not just its exit code (Codex, review of #1003) --' -ForegroundColor Cyan

# `$joined = Complete-BackgroundStage -Started ...` would capture EVERY line the join emits -- the
# child's stdout and stderr, `STAGE_FAILED_AT` included -- into `$joined` instead of letting it flow
# through this scriptblock's own output stream, which is what `Invoke-Stage`'s own
# `& $Body 2>&1 6>&1 | ForEach-Object {...}` captures into `$capturedLines` / `outputTail`. A failed
# Studio stage would then record an empty tail and nothing else. Extracted by text, driven with a
# stand-in `Complete-BackgroundStage` that emits known lines, so this proves the lines actually leave
# the block rather than trusting a read of the shape.
$joinBlock = Get-Block -Opening 'if ($null -eq $script:studioStarted) {'
Assert-True -Condition ($null -ne $joinBlock) `
    'the studio-stage join branch is locatable in gate.ps1'
Assert-True -Condition ($joinBlock -and $joinBlock.Contains('Complete-BackgroundStage')) `
    'and it still calls Complete-BackgroundStage, not a deleted stand-in'

if ($joinBlock) {
    function Complete-BackgroundStage {
        param([Parameter(Mandatory)] [AllowNull()] $Started)
        Write-Output 'STAGE_FAILED_AT=typecheck'
        Write-Output 'npm ERR! synthetic diagnostic line for this test'
        return 1
    }
    $script:studioStarted = [pscustomobject]@{ Name = 'synthetic stage' }

    $joinedOutput = New-Object System.Collections.Generic.List[string]
    . ([scriptblock]::Create($joinBlock)) | ForEach-Object { $joinedOutput.Add([string]$_) }

    Assert-True -Condition ($joinedOutput -contains 'STAGE_FAILED_AT=typecheck') `
        "the joined child's STAGE_FAILED_AT line reaches Invoke-Stage's own capture (got: $($joinedOutput -join ' | '))"
    Assert-True -Condition ($joinedOutput -contains 'npm ERR! synthetic diagnostic line for this test') `
        'and so does an ordinary diagnostic line from the same join'
}

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
