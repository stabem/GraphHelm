# #956: the gate runs its stages one after another, and four of them are 81% of a 32-minute run.
#
# `ci powershell suites` touches no cargo -- it is `powershell -File ci/run-ps-suites.ps1` -- so it
# shares no target-directory lock, no port and no temp cluster with any Rust stage. Overlapping that
# one stage with the Rust half is ~5 minutes off every FULL run and is the smallest change that
# moves the wall clock.
#
# THE RISK THE CHANGE CREATES, and what these cells are for. Concurrency that quietly stops being
# concurrent is invisible: the run still passes, the manifest still lists every stage, and the only
# symptom is minutes nobody is measuring. So the gate CHECKS ITS OWN OVERLAP from the timestamps it
# already records, and reddens when the claim it makes about itself is false. These cells drive that
# predicate directly, and the source cells pin the arrangement it depends on.
#
# The predicate is cut out of ci/gate.ps1 by anchor and never retyped: a copy here would pass while
# the gate's own copy drifted.

$ExpectedAssertionCount = 22
$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
if (-not (Test-Path -LiteralPath $gatePath)) {
    Write-Host 'HARNESS-BROKE: ci/gate.ps1 is not beside this suite' -ForegroundColor Magenta
    exit 2
}
$gateText = [System.IO.File]::ReadAllText($gatePath)

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End, [switch] $IncludeEnd)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) { throw "HARNESS-BROKE: slice anchors did not match for [$Start]" }
    $end = if ($IncludeEnd) { $j + $End.Length } else { $j }
    return $gateText.Substring($i, $end - $i)
}

try {
    Invoke-Expression (Get-GateSlice -Start 'function Test-StageOverlapped {' -End "`n}" -IncludeEnd)

    function New-Record {
        param([string] $Name, [string] $Started, [string] $Ended)
        return [ordered]@{ name = $Name; startedUtc = $Started; endedUtc = $Ended }
    }

    $overlapping = @(
        (New-Record -Name 'workspace tests'      -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:06:00Z'),
        (New-Record -Name 'ci powershell suites' -Started '2026-09-06T21:01:00Z' -Ended '2026-09-06T21:05:30Z')
    )
    Assert-True (Test-StageOverlapped -Records $overlapping -Name 'ci powershell suites') `
        'a stage whose interval intersects another stage is reported as overlapped'

    $serial = @(
        (New-Record -Name 'workspace tests'      -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:06:00Z'),
        (New-Record -Name 'ci powershell suites' -Started '2026-09-06T21:06:00Z' -Ended '2026-09-06T21:11:00Z')
    )
    Assert-True (-not (Test-StageOverlapped -Records $serial -Name 'ci powershell suites')) `
        'a stage that starts when the previous one ends is NOT overlapped -- the serial regression is visible'

    $touching = @(
        (New-Record -Name 'a' -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:06:00Z'),
        (New-Record -Name 'ci powershell suites' -Started '2026-09-06T21:06:00Z' -Ended '2026-09-06T21:06:00Z')
    )
    Assert-True (-not (Test-StageOverlapped -Records $touching -Name 'ci powershell suites')) `
        'a zero-length stage at the boundary is not counted as overlap'

    Assert-True (-not (Test-StageOverlapped -Records @((New-Record -Name 'workspace tests' -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:06:00Z')) -Name 'ci powershell suites')) `
        'a stage that is not in the records is not overlapped'

    Assert-True (-not (Test-StageOverlapped -Records @((New-Record -Name 'ci powershell suites' -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:05:00Z')) -Name 'ci powershell suites')) `
        'a stage alone in the records is not overlapped with itself'

    $noStamps = @(
        [ordered]@{ name = 'workspace tests' },
        [ordered]@{ name = 'ci powershell suites' }
    )
    Assert-True (-not (Test-StageOverlapped -Records $noStamps -Name 'ci powershell suites')) `
        'records without timestamps answer NOT overlapped rather than throwing or guessing'

    # THE CALL, not the name. The first draft of this cell searched for 'Start-BackgroundStage' and
    # stayed GREEN after the early start was deleted outright, because the function DEFINITION also
    # sits above the first cargo stage. A cell that a sabotage cannot redden is measuring the wrong
    # thing, so it matches the ASSIGNMENT the join later reads.
    $startIndex = $gateText.IndexOf('$script:psSuitesStarted = Start-BackgroundStage', [System.StringComparison]::Ordinal)
    $rustfmtIndex = $gateText.IndexOf("Invoke-Stage 'rustfmt'", [System.StringComparison]::Ordinal)
    Assert-True ($startIndex -ge 0 -and $rustfmtIndex -ge 0 -and $startIndex -lt $rustfmtIndex) `
        'the background stage is STARTED before the first cargo stage, which is what makes the overlap possible'

    # INSIDE the block, not merely after it in the file: an index comparison would be satisfied by
    # the function DEFINITION sitting anywhere above, and by a join moved to any later stage. The
    # slice is the claim -- the work is accounted for under the name it has always had.
    # #1053 item 7 added `-AlwaysRun` to this call so a started background child is still reaped
    # after a fail-fast abort. The anchor moved with the line; the claim it supports did not.
    $joinIndex = $gateText.IndexOf("Invoke-Stage 'ci powershell suites' -AlwaysRun {", [System.StringComparison]::Ordinal)
    $joinBlock = ''
    if ($joinIndex -ge 0) {
        $blockEnd = $gateText.IndexOf("`n    } | Out-Null", $joinIndex, [System.StringComparison]::Ordinal)
        if ($blockEnd -gt $joinIndex) { $joinBlock = $gateText.Substring($joinIndex, $blockEnd - $joinIndex) }
    }
    Assert-True ($joinBlock.Contains('Complete-BackgroundStage -Started $script:psSuitesStarted')) `
        'and JOINED inside the stage it has always been recorded as, so the manifest keeps one shape and one name'

    Assert-True ($gateText.IndexOf('Test-StageOverlapped -Records', [System.StringComparison]::Ordinal) -ge 0) `
        'and the gate asks the predicate about its OWN records, so a run that quietly went serial is recorded'

    # ---------------------------------------------------------------------------------------------
    # A CONCURRENT FAILURE MUST NOT BE SWALLOWED, and it must still be READABLE.
    #
    # This is the half ci/gate-stage-reddens.tests.ps1 cannot reach. That suite slices the stage
    # block and runs it with no child started, so it exercises the INLINE fallback and would stay
    # green while every background failure went missing. A child's exit code lives on a Process
    # object and its output lives in two files; nothing about `& $Body` carries either. So the pair
    # is driven here for real, against a real process that really fails.
    Invoke-Expression (Get-GateSlice -Start 'function Start-BackgroundStage {' -End "`n}" -IncludeEnd)
    Invoke-Expression (Get-GateSlice -Start 'function Complete-BackgroundStage {' -End "`n}" -IncludeEnd)

    $childScript = "Write-Output 'OVERLAP-CHILD-STDOUT'; [Console]::Error.WriteLine('OVERLAP-CHILD-STDERR'); exit 3"
    $started = Start-BackgroundStage -Name 'probe' -FilePath 'powershell' `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $childScript) `
        -WorkingDirectory $PSScriptRoot
    if ($null -eq $started) { throw 'HARNESS-BROKE: the probe child could not be started at all' }

    $global:LASTEXITCODE = 0
    # `6>&1` MERGES THE INFORMATION STREAM, and its absence is what let these two cells stay green
    # over a blind gate. The join used to re-emit the child's lines with `Write-Output`, so a BARE
    # call like the one here saw them on the success stream -- but the gate's own call site assigns
    # the join (`$joined = Complete-BackgroundStage ...`), and an assignment consumes that stream
    # whole. The arrangement here was therefore not the arrangement at the firing site, and the two
    # cells below certified a property the real gate did not have: on #1009's red the runner printed
    # `HARNESS-BROKE in: <name> (exit 2)` and the manifest recorded `<absent: ... no output ...>`.
    #
    # The join now publishes with `Write-Host`, which an assignment cannot swallow, and this line
    # reads the same stream `Invoke-Stage` merges. The property that the lines reach the RECORD is
    # pinned at the firing site by ci/gate-background-stage-evidence.tests.ps1; what these two cells
    # keep is that the join emits them at all.
    $childLines = @(Complete-BackgroundStage -Started $started 6>&1)
    # The join's own return value is the last element; everything before it is the child's output.
    $returned = $childLines[-1]
    $childText = ($childLines | ForEach-Object { [string]$_ }) -join "`n"

    Assert-True ($returned -eq 3) `
        'a background stage that fails hands its exit code back to the join, so the stage cannot pass on a dead child'
    Assert-True ($global:LASTEXITCODE -eq 3) `
        'and sets $LASTEXITCODE, which is the value Invoke-Stage reads to decide the stage -- a mute join would inherit the neighbour verdict'
    Assert-True ($childText.Contains('OVERLAP-CHILD-STDOUT')) `
        "and the child's stdout reaches the stage, so a red still names what failed"
    Assert-True ($childText.Contains('OVERLAP-CHILD-STDERR')) `
        "and its stderr too -- that is where a failing suite prints the assertion, and the tail is worthless without it"

    # FAIL INTO THE SLOW PATH, never into a skipped stage. If the early start cannot happen, the
    # join must answer null so the block runs the suites in line: the run is exactly as long as it
    # was before #956, and it is still a real run.
    $impossible = Start-BackgroundStage -Name 'probe' -FilePath 'graphhelm-no-such-executable-956' `
        -ArgumentList @('-x') -WorkingDirectory $PSScriptRoot
    Assert-True ($null -eq $impossible) `
        'a start that cannot happen answers null rather than throwing, which is what lets the join fall back to running the suites in line'

    # ---------------------------------------------------------------------------------------------
    # THE METER MUST STILL READ. `wallTimeSecs` is the field #956 sums to measure its own progress,
    # and the join's stopwatch measures the join -- about zero seconds -- not the work. A change that
    # buys five minutes and leaves the meter unreadable has not been measured, it has been believed.
    $script:stageStartedOverrideUtc = $null
    $script:stageEndedOverrideUtc = $null
    $slow = Start-BackgroundStage -Name 'probe-slow' -FilePath 'powershell' `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', 'Start-Sleep -Seconds 2') `
        -WorkingDirectory $PSScriptRoot
    if ($null -eq $slow) { throw 'HARNESS-BROKE: the slow probe child could not be started at all' }
    $null = Complete-BackgroundStage -Started $slow
    $spanSecs = -1
    if ($script:stageStartedOverrideUtc -and $script:stageEndedOverrideUtc) {
        $spanSecs = ([DateTime]::Parse($script:stageEndedOverrideUtc).ToUniversalTime() -
            [DateTime]::Parse($script:stageStartedOverrideUtc).ToUniversalTime()).TotalSeconds
    }
    Assert-True ($spanSecs -ge 1.5) `
        "the joined stage's interval spans the CHILD's two seconds, not the join's instant, so wallTimeSecs stays readable (measured $([math]::Round($spanSecs, 3))s)"

    # A CHECK HAS TO RUN BEFORE THE VALUE IT EXISTS TO CHANGE. The first draft ran the overlap check
    # inside Write-RunManifest, which is called AFTER `$status` is decided: the run still exited 1 --
    # `$failed` is the same variable -- but the manifest it had already been handed said GREEN while
    # its own overallPassed said false.
    #
    # THE FIRST VERSION OF THIS CELL COMPARED FILE POSITIONS and could not be reddened by putting the
    # check back: Write-RunManifest is DEFINED above the call site, so the offending arrangement has
    # a smaller index than the correct one. File order is not execution order. The claim that can be
    # made about text is CONTAINMENT -- the check is not in the writer's body -- and that one a
    # sabotage does move.
    $writerBody = Get-GateSlice -Start 'function Write-RunManifest {' -End "`n}" -IncludeEnd
    Assert-True (-not $writerBody.Contains('Test-StageOverlapped -Records')) `
        'the overlap check is NOT inside the manifest writer, which is handed a status that was already decided'

    # NOT A STAGE. Nothing ran under the alarm's name: it has no record and no output tail, so a
    # reader who finds it listed among real stages goes looking for a transcript that does not exist.
    # #822 settled this for the freshness cross-check; the same shape, the same treatment.
    Invoke-Expression (Get-GateSlice -Start 'function Get-GateVerdictLines {' -End "`n}" -IncludeEnd)
    # BOTH STRINGS READ OUT OF THE FILE, never one variable handed to both sides. The first version
    # of these cells built the failed list and the frame name from a single local, which mirrors the
    # contract instead of measuring it: it stays green for EVERY possible pair, so it could not see
    # that the alarm site appended 'stage overlap' while the constant said 'stage overlap regression'
    # -- and on a real regression the verdict would print the alarm as a PEER STAGE with the #822
    # frame nowhere, the exact reader-hunts-a-missing-transcript failure the framing prevents.
    # (Found by lane orchestrator's pass at 0519817b -- PR #958, issuecomment-5563495547 -- not by
    # this suite. Attributed here to the pass that made the measurement, because a comment is read
    # for years by people who cannot go back and check who relayed it.)
    # THE TYPE THE GATE ACTUALLY PASSES, which is not the type these cells were passing.
    #
    # `$script:stageRecords` is a System.Collections.Generic.List[object]; every cell above builds a
    # plain PowerShell array. Under this machine's Windows PowerShell 5.1, `@(<a List[object]
    # VARIABLE>)` throws "Argument types do not match" UNCONDITIONALLY -- ci/gate.ps1 documents that
    # trap at its own `stages` field -- and the predicate wrapped its parameter in exactly that. So
    # the function throws on any run that COMPLETES its stages, between the last stage and the
    # manifest, and these cells were green throughout because an array fixture cannot reach a
    # List-only defect. Not a hypothetical: it is the whole run's evidence, lost silently.
    #
    # It is also what killed the two FULL runs at this head, established after the fact rather than
    # assumed: both died with the cast absent, the run with it completed 57 stages GREEN, and each
    # dead log's final line is line 5689 of 5693 of the successful one -- four lines short of the
    # manifest. The first reading of those logs called it unrelated, because a PASSING STAGE PRINTS
    # NO COMPLETION LINE and the silence looked like an unfinished stage.
    #
    # A fixture of the wrong TYPE is the same failure as a fixture that supplies both sides of a
    # contract: it is shaped so the defect cannot arrive.
    $realList = New-Object System.Collections.Generic.List[object]
    $realList.Add((New-Record -Name 'workspace tests'      -Started '2026-09-06T21:00:00Z' -Ended '2026-09-06T21:06:00Z'))
    $realList.Add((New-Record -Name 'ci powershell suites' -Started '2026-09-06T21:01:00Z' -Ended '2026-09-06T21:05:30Z'))
    $listAnswer = $null
    $listThrew = $false
    try { $listAnswer = Test-StageOverlapped -Records $realList -Name 'ci powershell suites' } catch { $listThrew = $true }
    Assert-True (-not $listThrew) `
        'the predicate accepts the List[object] the gate actually hands it, rather than throwing after the last stage and before the manifest'
    Assert-True ($listAnswer -eq $true) `
        'and answers the same for a List as for the array the other cells build'

    # A NOTE, NEVER A VERDICT (#956, 2026-09-07). The first version of the alarm appended a frame to
    # $failed and turned a 59-of-59 HDD run RED (#979's manifest 66259741ff2a-20260907T212946.729Z:
    # failed stages [], psSuitesStartedEarly true, psSuitesOverlapped false). On the HDD the first
    # build pass outlasts the suites, so "serial" is the common case and says nothing about the tree.
    # Three claims, each about text the sabotage moves: the note site exists and is a NOTE; between
    # the note and the status derivation nothing touches $failed; and no stage name is appended for
    # the overlap anywhere -- status is derived from stages alone.
    $noteAnchor = 'NOTE: ci powershell suites started early but overlapped no other stage'
    $noteIndex = $gateText.IndexOf($noteAnchor, [System.StringComparison]::Ordinal)
    Assert-True ($noteIndex -ge 0) `
        'a run that started early and overlapped nothing is reported as a NOTE on the console -- the saving that did not happen, named'
    # ANCHOR ON THE SHAPE, NOT ON ONE SPELLING. This searched for the literal
    # `$status = if ($failed.Count -eq 0)`, so #455 rewriting that derivation as a call to
    # `Get-GateStatus` broke the cell -- and the PROPERTY it measures ("nothing appends to $failed
    # between the note and the derivation") was untouched. A cell that fails when the code is
    # correctly refactored spends a reviewer's attention on itself. `$status = ` searched forward
    # FROM THE NOTE is form-agnostic: the note sits near the end of the file, so the earlier
    # `$status = @(& git status ...)` cannot be reached from here.
    $statusIndex = if ($noteIndex -ge 0) { $gateText.IndexOf("`$status = ", $noteIndex, [System.StringComparison]::Ordinal) } else { -1 }
    $noteToStatus = if ($noteIndex -ge 0 -and $statusIndex -gt $noteIndex) { $gateText.Substring($noteIndex, $statusIndex - $noteIndex) } else { '<no slice>' }
    Assert-True ($statusIndex -gt $noteIndex -and $noteToStatus -notmatch '\$failed\s*\+=') `
        'and between that note and the status derivation nothing is appended to $failed -- the status is derived from the stages alone'
    Assert-True ($gateText -notmatch '\$failed\s*\+=\s*(\$OverlapStageName|''stage overlap regression'')') `
        'and no overlap name is appended to $failed anywhere, under the old constant or its literal -- the frame that reddened #979 is gone'

    # THE FIELDS STAY. The measurement is the point; only the verdict was wrong.
    $writer = Get-GateSlice -Start 'function Write-RunManifest {' -End "`n}" -IncludeEnd
    Assert-True ($writer.Contains('psSuitesStartedEarly') -and $writer.Contains('psSuitesOverlapped')) `
        'the manifest still carries psSuitesStartedEarly and psSuitesOverlapped, so a decayed arrangement is visible to a reader even though it no longer reddens'
} finally {
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-stage-overlap: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-stage-overlap: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
