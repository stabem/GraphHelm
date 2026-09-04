# #810: isolated tests for ci/gate-evidence.ps1 - which lines of a red stage's transcript survive
# into the run manifest.
#
# Dot-sources ONLY gate-evidence.ps1, never gate.ps1. Select-GateEvidenceLines is a pure function
# of a string list and two integers: no cargo, no slot, no repository, no temp files. Same
# homegrown PASS/FAIL/HARNESS-BROKE harness as ci/slot-lock.tests.ps1 (#200/#228) - this
# repository carries no Pester dependency and one issue's worth of pure functions is not the
# occasion to add one.
#
# 21 runtime assertions: 16 direct Assert-True calls + 5 Assert-Equal calls. Assert-Equal
# delegates to Assert-True, so it fires ONCE at runtime, not twice; a naive grep over this file
# also counts the two function DEFINITION lines and the delegation inside Assert-Equal's body,
# which are source text rather than runtime assertions.
$ExpectedAssertionCount = 21

$ErrorActionPreference = 'Stop'
# 2.0, not Latest: this is what ci/gate.ps1 sets before it dot-sources gate-evidence.ps1, so
# this is the strictness Select-GateEvidenceLines actually runs under.
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'gate-evidence.ps1')

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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -ceq $Actual) -Message "$Message (expected '$Expected', got '$Actual')"
}

# The transcript this issue is about, in the shape the real manifest recorded it: a genuine
# failure early, then one trailing summary per test binary that --no-fail-fast kept running.
# The passing-summary count is deliberately larger than the budget, which is the entire point -
# with 60 of them after it, no fixed-size window measured from the END can reach the failure.
$FailingTestName = 'events::local::tests::rejects_unregistered_root'
$OkSummary = 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'

function New-WorkspaceTranscript {
    param([int] $OkSummaryCount = 60)
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($i in 1..8) { $lines.Add("test some::passing::case_$i ... ok") }
    $lines.Add('failures:')
    $lines.Add("    $FailingTestName")
    $lines.Add('')
    $lines.Add("thread 'events::local::tests::rejects_unregistered_root' panicked at core/events/src/local.rs:412:9:")
    $lines.Add('assertion `left == right` failed')
    $lines.Add('test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    return $lines.ToArray()
}

Write-Host ''
Write-Host 'the workspace transcript --no-fail-fast produces' -ForegroundColor Cyan

$transcript = New-WorkspaceTranscript
$selected = @(Select-GateEvidenceLines -Lines $transcript)

Assert-True -Condition ($selected -join "`n").Contains($FailingTestName) -Message 'the excerpt names the test that failed'

# THE CONTROL THAT MAKES THE ONE ABOVE MEAN SOMETHING. Without it, a Select-GateEvidenceLines that
# simply returned the whole transcript would satisfy the assertion above and look like the fix.
# This is the OLD behaviour - the last 40 lines - applied to the SAME input, and it must NOT find
# the failure. If this ever passes, the fixture stopped reproducing the defect and every other
# assertion in this file is measuring nothing.
$oldSlice = $transcript[($transcript.Count - 40)..($transcript.Count - 1)]
Assert-True -Condition (-not ($oldSlice -join "`n").Contains($FailingTestName)) -Message 'CONTROL: slicing the last 40 lines of the SAME transcript does not reach the failure'

Assert-True -Condition (@($oldSlice | Where-Object { $_ -ceq $OkSummary }).Count -eq 40) -Message 'CONTROL: those 40 lines are FULL, not empty - all of them are other binaries reporting ok'

Assert-True -Condition ($selected.Count -le 40) -Message 'the excerpt still fits the 40-line budget'

Assert-Equal -Expected $transcript[$transcript.Count - 1] -Actual $selected[$selected.Count - 1] -Message "the stage's own last line is kept"

Assert-True -Condition (@($selected | Where-Object { $_ -clike '*lines omitted*' }).Count -eq 1) -Message 'exactly one notice says the excerpt is not contiguous'

$notice = @($selected | Where-Object { $_ -clike '*lines omitted*' })[0]
Assert-True -Condition ($notice -cmatch '\s(\d+) lines omitted' -and [int]$Matches[1] -gt 0) -Message 'the notice names a positive number of omitted lines'

Assert-True -Condition ($selected -join "`n").Contains('panicked at core/events/src/local.rs:412:9') -Message 'the panic site travels with the failure name'

Write-Host ''
Write-Host 'more than one failure, which --no-fail-fast makes the EXPECTED case' -ForegroundColor Cyan

# `--no-fail-fast` is the instruction to keep going after a binary fails, so a red workspace run
# routinely names SEVERAL. The window anchors on the first and never looks again -- correct, and
# incomplete. Without a count in the notice the excerpt names one failure and the omitted region
# reads as filler, so a reader triages one, fixes it, and meets the next on the re-run. Found in
# review of #812 by the ISSUES 2 lane, who ran the shipped function rather than reading it.
$multi = New-Object System.Collections.Generic.List[string]
$multi.Add('failures:'); $multi.Add('    suite_a::the_first_one')
foreach ($i in 1..200) { $multi.Add($OkSummary) }
$multi.Add('failures:'); $multi.Add('    suite_b::the_second_one')
foreach ($i in 1..30) { $multi.Add($OkSummary) }
$multi.Add('error: test failed, to rerun pass `-p graphhelm-b`')
$multiSelected = @(Select-GateEvidenceLines -Lines $multi.ToArray())
$multiNotice = @($multiSelected | Where-Object { $_ -clike '*lines omitted*' })[0]

# The count is pulled out ONCE, into a local, rather than read from the automatic `$Matches`
# in the assertion below. `$Matches` survives a FAILED `-cmatch` holding the previous
# statement's captures, so an assertion reading it is reading a value some OTHER line set --
# under the sabotage that removes the count entirely, that spelling reported `got 27` from a
# match made twenty lines earlier. The red was still a red, but for a number from nowhere.
$multiCount = if ($multiNotice -cmatch 'INCLUDING (\d+) further line') { [int]$Matches[1] } else { -1 }

Assert-True -Condition ($multiCount -gt 0) -Message 'the notice says the omitted region ALSO names failures, instead of reading as filler'

# THE COUNT IS ASSERTED, not merely its presence -- and asserting it caught the fixture's author
# rather than the code. I expected 2, on the belief that `failures:` AND the indented test name
# both match. Only `failures:` does: the name line matches no marker, and the trailing `error:`
# line sits in the reserved tail rather than in the gap. So the omitted region holds exactly ONE
# marker line, and a notice claiming 2 would be as wrong as one claiming none.
#
# An assertion on the phrase's PRESENCE would have accepted 1, 2 or 9 without complaint, and the
# wrong expectation would have shipped as a passing test.
Assert-Equal -Expected 1 -Actual $multiCount -Message 'and it names HOW MANY, counted over the omitted region only'

# CONTROL: a run with exactly ONE named failure must NOT claim further ones. Without this, a
# notice that always appended the phrase would satisfy the assertion above and look like the fix.
$singleNotice = @($selected | Where-Object { $_ -clike '*lines omitted*' })[0]
Assert-True -Condition (-not ($singleNotice -clike '*INCLUDING*')) -Message 'CONTROL: a single-failure run does not claim further failures it does not have'

Write-Host ''
Write-Host 'when nothing names a failure' -ForegroundColor Cyan

# CASE DISCRIMINATION, and the reason the marker pattern is applied with -cmatch. Every line here
# contains the word `failed`, in the phrase `0 failed;` that cargo prints for a binary that
# PASSED. A case-insensitive marker matches all 60 and would anchor the window on line 0, making
# every transcript "informative" and the selection meaningless.
$allPassing = @(1..60 | ForEach-Object { $OkSummary })
$fallback = @(Select-GateEvidenceLines -Lines $allPassing)

Assert-Equal -Expected 40 -Actual $fallback.Count -Message 'a transcript of only passing summaries falls back to the last 40 lines'

Assert-True -Condition (@($fallback | Where-Object { $_ -clike '*lines omitted*' }).Count -eq 0) -Message 'the fallback is contiguous, so it carries no omission notice'

Write-Host ''
Write-Host 'the OTHER stage class: a PowerShell suite, whose vocabulary is not cargo' -ForegroundColor Cyan

# #810's population was `workspace tests` only, so the marker set was chosen entirely from cargo's
# output. `ci/gate.ps1`'s `ci powershell suites` stage prints a different vocabulary: a failing
# assertion is `  FAIL: <message>`, and the run ends with `FAILED in: <suite> (exit 1)`. Only the
# second matched, and it sits four lines from the end -- so the window anchored there, ran to the
# end, and handed back the tail. Found in review; this fixture is that stage's shape.
$SuiteFailure = 'the manifest was committed, so the head moved'
$suiteRun = New-Object System.Collections.Generic.List[string]
foreach ($i in 1..6) { $suiteRun.Add("  PASS: an early assertion $i") }
$suiteRun.Add("  FAIL: $SuiteFailure")
foreach ($i in 1..60) { $suiteRun.Add("  PASS: a later assertion $i") }
$suiteRun.Add('')
$suiteRun.Add('FAILED in: gate-manifest-provenance.tests.ps1 (exit 1)')
$suite = @(Select-GateEvidenceLines -Lines $suiteRun.ToArray())

Assert-True -Condition ($suite -join "`n").Contains($SuiteFailure) -Message 'the excerpt names WHICH assertion failed in a PowerShell suite, not just that one did'

# CONTROL, and it is the same one the cargo fixture carries: anchoring on `FAILED in:` alone puts
# the window four lines from the end, which IS the tail. If this ever stops holding, the fixture
# no longer reproduces the defect the marker was added for.
$suiteOldSlice = $suiteRun.ToArray()[($suiteRun.Count - 40)..($suiteRun.Count - 1)]
Assert-True -Condition (-not ($suiteOldSlice -join "`n").Contains($SuiteFailure)) -Message 'CONTROL: the last 40 lines of the SAME suite run do not reach the failing assertion'

# `HARNESS-BROKE` is deliberately NOT a marker: it is the third outcome, not a failing test, and
# anchoring on it would report a suite that could not vouch for its run as a suite that failed.
$brokeRun = New-Object System.Collections.Generic.List[string]
foreach ($i in 1..60) { $brokeRun.Add("  PASS: an assertion $i") }
$brokeRun.Add('HARNESS-BROKE: ran 15 assertions, expected 99.')
$broke = @(Select-GateEvidenceLines -Lines $brokeRun.ToArray())
Assert-Equal -Expected 40 -Actual $broke.Count -Message 'a HARNESS-BROKE run names no failing assertion, so it falls back rather than anchoring on the third outcome'

Write-Host ''
Write-Host 'edges' -ForegroundColor Cyan

$short = @('one', 'two', 'three')
$kept = @(Select-GateEvidenceLines -Lines $short)
Assert-Equal -Expected 'one|two|three' -Actual ($kept -join '|') -Message 'a transcript shorter than the budget is kept whole'

Assert-True -Condition (@(Select-GateEvidenceLines -Lines @()).Count -eq 0) -Message 'an empty transcript selects nothing'

Assert-True -Condition (@(Select-GateEvidenceLines -Lines $null).Count -eq 0) -Message 'a null transcript selects nothing rather than throwing'

# The failure lands inside the reserved tail: window and tail are contiguous, so the result is one
# unbroken run and must not claim anything was omitted.
$lateFailure = New-Object System.Collections.Generic.List[string]
foreach ($i in 1..60) { $lateFailure.Add("test some::passing::case_$i ... ok") }
$lateFailure.Add('test result: FAILED. 60 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
$late = @(Select-GateEvidenceLines -Lines $lateFailure.ToArray())

Assert-True -Condition (@($late | Where-Object { $_ -clike '*lines omitted*' }).Count -eq 0) -Message 'a failure at the very end yields a contiguous excerpt'

Assert-True -Condition ($late.Count -le 40 -and ($late -join "`n").Contains('FAILED')) -Message 'and that excerpt is within budget and still names the failure'

Write-Host ''

if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    Write-Host 'Coverage changed silently - a block was skipped, commented out, or lost in a merge.' -ForegroundColor Magenta
    Write-Host 'If this is a deliberate new test, update $ExpectedAssertionCount at the top of this file.' -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
