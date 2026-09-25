# #810: isolated tests for ci/gate-evidence.ps1 - which lines of a red stage's transcript survive
# into the run manifest.
#
# Dot-sources ONLY gate-evidence.ps1, never gate.ps1. Select-GateEvidenceLines is a pure function
# of a string list and two integers: no cargo, no slot, no repository, no temp files. Same
# homegrown PASS/FAIL/HARNESS-BROKE harness as ci/slot-lock.tests.ps1 (#200/#228) - this
# repository carries no Pester dependency and one issue's worth of pure functions is not the
# occasion to add one.
#
# 34 runtime assertions: 29 direct Assert-True calls + 5 Assert-Equal calls. Assert-Equal
# delegates to Assert-True, so it fires ONCE at runtime, not twice; a naive grep over this file
# also counts the two function DEFINITION lines and the delegation inside Assert-Equal's body,
# which are source text rather than runtime assertions.
$ExpectedAssertionCount = 43

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
# 60, not 30 (#917). This cell is about the NOTICE reporting failures it dropped, so the fixture has
# to put the second failure in the OMITTED GAP. With 30 trailing summaries the second `failures:`
# now lands 33 lines from the end, inside the reach of the explanation-seeking tail, so the excerpt
# KEEPS it -- and a notice truthfully reporting nothing further would fail a cell whose whole
# subject is the notice reporting something further.
#
# The expectation is not being lowered to match new behaviour: the count below is still 1. The
# fixture is being restored to producing the situation the cell exists to measure, and the case
# where the tail now reaches the second failure is a NEW cell below rather than an edit to this one.
foreach ($i in 1..60) { $multi.Add($OkSummary) }
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
Write-Host 'the shape real cargo prints: ANNOUNCED first, EXPLAINED last (#917)' -ForegroundColor Cyan

# The fixture above starts at `failures:` and so anchors on an EXPLANATION by accident of its own
# construction. Real libtest output does not: it says `test x ... FAILED` the moment the cell fails,
# keeps running every remaining cell, and prints `panicked at <file>:<line>` only in the trailing
# `failures:` section. That gap is where the diagnosis lives, and it is what PR #854's committed
# manifest lost while PR #871's kept -- the same function, the same budget, decided by where in the
# binary the failing cell happened to sit.
$AnnouncedSite = 'panicked at apps/cli/tests/schema_cli.rs:99:5'
$AnnouncedWhy = 'the catalog disagreed'
$announced = New-Object System.Collections.Generic.List[string]
$announced.Add('test suite::cell_a ... FAILED')
foreach ($i in 1..60) { $announced.Add("test suite::passing_$i ... ok") }
$announced.Add('failures:')
$announced.Add('')
$announced.Add('---- suite::cell_a stdout ----')
$announced.Add("thread 'suite::cell_a' panicked at apps/cli/tests/schema_cli.rs:99:5:")
$announced.Add("assertion ``left == right`` failed: $AnnouncedWhy")
$announced.Add('')
$announced.Add('failures:')
$announced.Add('    suite::cell_a')
$announced.Add('test result: FAILED. 60 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')

# ARRANGEMENT, both halves: this fixture reproduces the defect only if its first line anchors the
# window (matches the full set) while explaining nothing (does not match the explanation set). If it
# ever matched both, the tail would have nothing to be grown backwards to and the cells below would
# pass on a fixture that had quietly stopped being the real shape.
Assert-True -Condition ($announced[0] -cmatch $script:GateEvidenceFailureMarkers) `
    -Message 'ARRANGEMENT: the announcement line anchors the window, so the failing cell name is kept'
Assert-True -Condition (-not ($announced[0] -cmatch $script:GateEvidenceExplanationMarkers)) `
    -Message 'ARRANGEMENT: and it explains nothing, so the explanation is only reachable by growing the tail'

$announcedExcerpt = @(Select-GateEvidenceLines -Lines $announced.ToArray())

# RED before #917: the window opened on line 0, spent the budget on the 60 `... ok` lines, and the
# five-line tail landed AFTER the panic site, so the site fell into the omitted gap. The assertion
# text happened to survive on $TailReserve alone, which is why the loss was easy to miss -- the
# excerpt read as though it explained itself while withholding the file and line.
#
# The name below is NOT redundant with that: an earlier attempt at this fix moved the anchor to the
# explanation instead of growing the tail, which bought the site by losing the announcement -- the
# only line carrying the cell's name. `gate-postgres-evidence.tests.ps1` caught it. Both are
# asserted here so the next person cannot buy one with the other.
Assert-True -Condition ($announcedExcerpt -join "`n").Contains($AnnouncedSite) `
    -Message 'the panic SITE survives when the failure is announced early and explained late'

Assert-True -Condition ($announcedExcerpt[0] -clike '*suite::cell_a*FAILED*') `
    -Message 'and the excerpt still OPENS on the announcement, so the failing cell is named at the top'

Assert-True -Condition ($announcedExcerpt -join "`n").Contains($AnnouncedWhy) `
    -Message 'and so does the assertion that failed'

Assert-True -Condition ($announcedExcerpt -join "`n").Contains('suite::cell_a') `
    -Message 'and the excerpt still names WHICH cell failed, from the trailing failures list'

# A stage can announce a failure and never explain it: a binary that exits non-zero having printed
# nothing else. The explanation pass finds nothing, and the fallback must be the pre-#917 excerpt
# rather than an empty one -- a fix that only works when it has something to anchor on is not a fix.
$announcedOnly = New-Object System.Collections.Generic.List[string]
foreach ($i in 1..60) { $announcedOnly.Add("test suite::passing_$i ... ok") }
$announcedOnly.Add('test suite::cell_b ... FAILED')
foreach ($i in 1..8) { $announcedOnly.Add("test suite::passing_late_$i ... ok") }
$announcedOnlyExcerpt = @(Select-GateEvidenceLines -Lines $announcedOnly.ToArray())

Assert-True -Condition ($announcedOnlyExcerpt -join "`n").Contains('suite::cell_b') `
    -Message 'a stage that announces a failure and never explains it still anchors on the announcement'

Write-Host ''
Write-Host 'TWO failing cells, which --no-fail-fast makes routine (#917, found by ISSUES 3)' -ForegroundColor Cyan

# The shape the first version of this fix could not handle, and it is the common one: two cells fail,
# each printing a `---- stdout ----` block with a panic site, an assertion and a couple of context
# lines. ISSUES 3 ran the shipped function against it rather than reading the diff and found the
# first explanation still lost; measured here, BOTH were lost, because the tail's bound was a fixed
# 14 lines rather than whatever the budget left over.
$twoFailures = New-Object System.Collections.Generic.List[string]
$twoFailures.Add('test suite::cell_a ... FAILED')
foreach ($i in 1..60) { $twoFailures.Add("test suite::passing_$i ... ok") }
$twoFailures.Add('test suite::cell_b ... FAILED')
$twoFailures.Add('failures:')
$twoFailures.Add('')
$twoFailures.Add('---- suite::cell_a stdout ----')
$twoFailures.Add("thread 'a' panicked at apps/cli/tests/api_http.rs:99:5:")
$twoFailures.Add('assertion `left == right` failed: FIRST')
$twoFailures.Add('  left:  something long')
$twoFailures.Add('  right: something else')
$twoFailures.Add('note: run with RUST_BACKTRACE=1')
$twoFailures.Add('')
$twoFailures.Add('---- suite::cell_b stdout ----')
$twoFailures.Add("thread 'b' panicked at apps/cli/tests/api_http.rs:222:5:")
$twoFailures.Add('assertion `left == right` failed: SECOND')
$twoFailures.Add('  left:  another')
$twoFailures.Add('  right: other')
$twoFailures.Add('note: run with RUST_BACKTRACE=1')
$twoFailures.Add('')
$twoFailures.Add('failures:')
$twoFailures.Add('    suite::cell_a')
$twoFailures.Add('    suite::cell_b')
$twoFailures.Add('test result: FAILED. 60 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out')

# ARRANGEMENT: the first explanation must sit further from the end than the plain $TailReserve, or
# the cells below would pass on a transcript the unmodified tail already reached, and would be
# measuring nothing about the growth.
$firstExplanationIndex = 0
for ($i = 0; $i -lt $twoFailures.Count; $i++) {
    if ($twoFailures[$i] -clike '*panicked at apps/cli/tests/api_http.rs:99:5*') { $firstExplanationIndex = $i; break }
}
Assert-True -Condition (($twoFailures.Count - $firstExplanationIndex) -gt 5) `
    -Message "ARRANGEMENT: the first explanation is beyond the 5-line tail reserve ($($twoFailures.Count - $firstExplanationIndex) lines from the end)"

$twoSelected = @(Select-GateEvidenceLines -Lines $twoFailures.ToArray())

Assert-True -Condition ($twoSelected -join "`n").Contains('api_http.rs:99:5') `
    -Message 'the FIRST failing cell keeps its panic site when a second cell also fails'

Assert-True -Condition ($twoSelected -join "`n").Contains('api_http.rs:222:5') `
    -Message 'and so does the second'

Assert-True -Condition ($twoSelected[0] -clike '*suite::cell_a*FAILED*') `
    -Message 'while the excerpt still opens on the announcement, which is the only line naming the first cell'

Assert-True -Condition ($twoSelected[$twoSelected.Count - 1] -clike '*test result: FAILED*') `
    -Message "and still ends on the stage's own summary"

# The other side of the same change: when the tail reaches far enough to KEEP a second failure, the
# notice must not claim further failures were dropped. The `$multi` fixture above has the opposite
# shape on purpose -- there the second failure really is in the gap and the notice really does say so.
$nearTail = New-Object System.Collections.Generic.List[string]
$nearTail.Add('failures:'); $nearTail.Add('    suite_a::the_first_one')
foreach ($i in 1..200) { $nearTail.Add($OkSummary) }
$nearTail.Add('failures:'); $nearTail.Add('    suite_b::the_second_one')
foreach ($i in 1..30) { $nearTail.Add($OkSummary) }
$nearTail.Add('error: test failed, to rerun pass `-p graphhelm-b`')
$nearSelected = @(Select-GateEvidenceLines -Lines $nearTail.ToArray())
$nearNotice = @($nearSelected | Where-Object { $_ -clike '*lines omitted*' })[0]

Assert-True -Condition (($nearSelected -join "`n").Contains('suite_b::the_second_one') -and -not ($nearNotice -clike '*INCLUDING*')) `
    -Message 'a second failure the excerpt KEEPS is not also reported as one it dropped'

Write-Host ''

Write-Host ''
Write-Host 'the notice counts what the reader is asking about (#238)' -ForegroundColor Cyan

# A transcript whose omitted region holds TWO distinct failing tests, each spending a full cargo
# block of marker lines. This is the shape #920's gate produced, where the old notice reported the
# MARKER count and a lane read it as that many problems.
function New-TwoFailureTranscript {
    param([int] $OkSummaryCount = 60, [switch] $RepeatTheSecondName)
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($i in 1..8) { $lines.Add("test some::passing::case_$i ... ok") }
    $lines.Add("test $FailingTestName ... FAILED")
    $lines.Add('failures:')
    $lines.Add('thread ''one'' panicked at core/events/src/local.rs:412:9:')
    $lines.Add('test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    # The second failure, in the region the window drops.
    $lines.Add('test other::suite::second_cell ... FAILED')
    if ($RepeatTheSecondName) { $lines.Add('test other::suite::second_cell ... FAILED') }
    $lines.Add('failures:')
    $lines.Add('thread ''two'' panicked at core/protocols/src/lib.rs:9:1:')
    $lines.Add('test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    $lines.Add('error: test failed, to rerun pass `-p graphhelm-protocols`')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    return $lines.ToArray()
}

$twoFailures = @(Select-GateEvidenceLines -Lines (New-TwoFailureTranscript) -Budget 40)
$twoNotice = @($twoFailures | Where-Object { $_ -clike '*lines omitted*' })[0]

Assert-True -Condition ($twoNotice -cmatch 'INCLUDING 1 further failing test\(s\)') `
    -Message "the notice counts the failing TEST in the gap, not its marker lines (got: $twoNotice)"
Assert-True -Condition ($twoNotice -clike '*other::suite::second_cell*') `
    -Message 'and NAMES it, so the reader knows what they have not seen'
# THE POINT OF #238. That one failure spends six marker lines; the old wording reported six and a
# lane read it as six problems, ordering a 7100-line decode to find them.
Assert-True -Condition (-not ($twoNotice -clike '*further line(s) naming a failure*')) `
    -Message 'the old wording, which reported markers in words that read as failures, is gone'

# DISTINCT, because libtest names one failing test on more than one line and a count that grew
# with the repetition would be the same over-statement wearing a different number.
$repeated = @(Select-GateEvidenceLines -Lines (New-TwoFailureTranscript -RepeatTheSecondName) -Budget 40)
$repeatedNotice = @($repeated | Where-Object { $_ -clike '*lines omitted*' })[0]
Assert-True -Condition ($repeatedNotice -cmatch 'INCLUDING 1 further failing test\(s\)') `
    -Message "a test named twice in the gap is ONE further failing test (got: $repeatedNotice)"

# THE FALLBACK, and the control that proves it is the branch being exercised: a stage that fails
# without libtest (a PowerShell suite, a compiler wall) has marker lines and no test names, and the
# notice must then say it is counting markers rather than inventing a failure count.
function New-MarkersWithoutTestNames {
    param([int] $OkSummaryCount = 60)
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('  FAIL: the first suite refused')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    $lines.Add('  FAIL: a second suite refused, in the gap')
    $lines.Add('error: the run stopped here')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    return $lines.ToArray()
}

$markersOnly = @(Select-GateEvidenceLines -Lines (New-MarkersWithoutTestNames) -Budget 40)
$markersNotice = @($markersOnly | Where-Object { $_ -clike '*lines omitted*' })[0]
Assert-True -Condition ($markersNotice -clike '*MATCHING THE FAILURE MARKER*') `
    -Message "with no libtest names to count, the notice says it is counting markers (got: $markersNotice)"
Assert-True -Condition (-not ($markersNotice -cmatch 'further failing test')) `
    -Message 'CONTROL: and does NOT claim a failing-test count the transcript cannot back'

Write-Host ''
Write-Host 'a MIXED region reports both kinds, because dropping one under-states (#238)' -ForegroundColor Cyan

# THE SECOND PASS'S FINDING. The first version of this change made the two clauses an if/elseif, so
# a region holding BOTH libtest failures and marker lines libtest did not name reported the test
# names and dropped the marker count in SILENCE.
#
# The direction is what makes it worth a cell. The defect this file exists to fix OVER-stated: a
# reader chased six problems that were one. Dropping the marker clause UNDER-states: the reader is
# told "1 further failing test" and concludes the excerpt is nearly the whole story while the region
# also held failures of another kind. An instrument that errs high wastes an hour; one that errs low
# loses a failure.
function New-MixedRegionTranscript {
    param([int] $OkSummaryCount = 60, [switch] $LibtestOnly)
    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add("test $FailingTestName ... FAILED")
    $lines.Add('failures:')
    $lines.Add('test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    # In the gap: a libtest failure ...
    $lines.Add('test other::suite::second_cell ... FAILED')
    # ... and, unless the control suppresses them, failures from a harness that names no test.
    if (-not $LibtestOnly) {
        $lines.Add('  FAIL: a PowerShell suite refused, and libtest never saw it')
        $lines.Add('error: the ci powershell suites stage stopped here')
    }
    foreach ($i in 1..$OkSummaryCount) { $lines.Add($OkSummary) }
    return $lines.ToArray()
}

$mixed = @(Select-GateEvidenceLines -Lines (New-MixedRegionTranscript) -Budget 40)
$mixedNotice = @($mixed | Where-Object { $_ -clike '*lines omitted*' })[0]

Assert-True -Condition ($mixedNotice -cmatch 'further failing test\(s\): other::suite::second_cell') `
    -Message "the named libtest failure is still reported (got: $mixedNotice)"
Assert-True -Condition ($mixedNotice -clike '*MATCHING THE FAILURE MARKER*') `
    -Message 'AND the marker lines are reported beside it, rather than dropped in silence'

# CONTROL, and it took a correction to write honestly. My first version asserted that the same gap
# WITHOUT the unnamed markers still names the test, and called that a control against the marker
# clause becoming unconditional. It is not one: the clause IS present in both cases, because the
# named test's own `... FAILED` line is itself a marker line. An assertion that passes in both arms
# of the thing it claims to distinguish controls nothing.
#
# What actually distinguishes them is the NUMBER. The marker count must RESPOND to the unnamed
# failures in the gap -- otherwise it is a constant wearing a count's clothes, which is the exact
# family of defect this whole file is about.
$libtestOnly = @(Select-GateEvidenceLines -Lines (New-MixedRegionTranscript -LibtestOnly) -Budget 40)
$libtestOnlyNotice = @($libtestOnly | Where-Object { $_ -clike '*lines omitted*' })[0]
$mixedMarkers = if ($mixedNotice -cmatch 'INCLUDING [^,]+, and (\d+) further line') { [int]$Matches[1] } else { -1 }
$onlyMarkers = if ($libtestOnlyNotice -cmatch 'INCLUDING [^,]+, and (\d+) further line') { [int]$Matches[1] } else { -1 }
Assert-True -Condition ($mixedMarkers -gt $onlyMarkers -and $onlyMarkers -ge 0) `
    -Message "CONTROL: the marker count RESPONDS to the unnamed failures in the gap (mixed $mixedMarkers vs libtest-only $onlyMarkers)"

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
