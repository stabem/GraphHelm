# #909: isolated tests for ci/test-count.ps1.
#
# Dot-sources ONLY test-count.ps1, which defines two pure functions and does nothing else -- no
# cargo, no cluster, no slot, no repository. Every fixture below is a hand-written string array, so
# these cells are about the PARSE and the three-state verdict, never about a real run.
#
# WHY THIS FILE EXISTS AT ALL, stated where the next reader will find it. On 2026-09-05 a lane read
# the last 80 lines of a PostgreSQL stage, saw fifteen `running 0 tests` summaries, and published
# "zero tests executed, the stage passes green". It was wrong: `Invoke-Postgres` publishes
# `Get-Content -Tail 80`, twenty-four doc-test targets sit in that tail, and a crate with no doc tests
# prints exactly that summary. The retraction is on #752 and #909. The defect that made the mistake
# possible is real and is not the mistake: nothing counted, so "ran everything" and "ran nothing" had
# the same artefact. Cell (f) is the one that would have caught the wrong reading.
#
# Homegrown PASS/FAIL harness with a declared total, the same discipline the rest of ci/*.tests.ps1
# uses: a block silently dropped by a bad merge leaves every remaining assertion green, and
# "N/N passed" reads healthy to anyone who does not independently know what N should be.
# 26, and I did not get this right by counting the file: I wrote 24, and the harness answered
# HARNESS-BROKE with 26. The (e) block is a loop over THREE cases contributing two assertions each,
# which reads as one block. That is precisely the arithmetic a declared total exists to catch, and it
# caught its own author on the first run.
$ExpectedAssertionCount = 26

$ErrorActionPreference = 'Stop'
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
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected [$Expected], got [$Actual])"
}

. (Join-Path $PSScriptRoot 'test-count.ps1')

Write-Host "`n=== (a) a passing summary: executed is passed + failed, not the whole line ==="
$a = Get-ExecutedTestCount -Lines @(
    'test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.78s'
)
Assert-Equal -Expected 34 -Actual $a.executed -Message '(a1) 34 executed'
Assert-Equal -Expected 1 -Actual $a.groups -Message '(a2) one summary is one group'
Assert-Equal -Expected 'measured' -Actual (Get-TestExecutionVerdict -Totals $a) -Message '(a3) verdict is measured'

Write-Host "`n=== (b) a FAILED summary still executed its tests ==="
$b = Get-ExecutedTestCount -Lines @(
    'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 11 filtered out; finished in 10.61s'
)
Assert-Equal -Expected 2 -Actual $b.executed -Message '(b1) a failure ran: 1 passed + 1 failed = 2 executed'
Assert-Equal -Expected 11 -Actual $b.filteredOut -Message '(b2) filtered out is recorded and is not executed'
Assert-Equal -Expected 'measured' -Actual (Get-TestExecutionVerdict -Totals $b) -Message '(b3) a red run still measured'

Write-Host "`n=== (c) ignored and filtered tests did NOT run ==="
$c = Get-ExecutedTestCount -Lines @(
    'test result: ok. 0 passed; 0 failed; 13 ignored; 0 measured; 42 filtered out; finished in 0.01s'
)
Assert-Equal -Expected 0 -Actual $c.executed -Message '(c1) 13 ignored + 42 filtered out is ZERO executed'
Assert-Equal -Expected 13 -Actual $c.ignored -Message '(c2) ignored is carried, so the reason is legible'
Assert-Equal -Expected 'none' -Actual (Get-TestExecutionVerdict -Totals $c) -Message '(c3) a group that ran nothing is none, not unknown'

Write-Host "`n=== (d) several binaries sum ==="
$d = Get-ExecutedTestCount -Lines @(
    'test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s',
    'noise between the summaries, which is what a real log looks like',
    'test result: FAILED. 2 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.20s'
)
Assert-Equal -Expected 10 -Actual $d.executed -Message '(d1) 5 + (2+3) = 10 executed across two groups'
Assert-Equal -Expected 2 -Actual $d.groups -Message '(d2) two groups'
Assert-Equal -Expected 3 -Actual $d.failed -Message '(d3) failures are carried'

Write-Host "`n=== (e) THE DISTINCTION: nothing said 'test result:' is UNKNOWN, never zero ==="
foreach ($case in @(
    @{ name = 'empty array'; lines = @() },
    @{ name = 'null'; lines = $null },
    @{ name = 'output with no summary at all'; lines = @('Compiling graphhelm-events', 'Running tests\x.rs') }
)) {
    $e = Get-ExecutedTestCount -Lines $case.lines
    Assert-Equal -Expected 0 -Actual $e.groups -Message "(e) $($case.name): no groups seen"
    Assert-Equal -Expected 'unknown' -Actual (Get-TestExecutionVerdict -Totals $e) `
        -Message "(e) $($case.name): verdict is unknown -- a reader that saw nothing must not report a zero"
}

Write-Host "`n=== (f) the exact shape that produced the wrong reading on 2026-09-05 ==="
# Fifteen doc-test targets, each legitimately empty. This is what the tail showed, and reading it as
# "the stage ran nothing" was the error. The COUNT says the honest thing: these groups ran nothing,
# and because the head of the stream was cut, they are not the whole run.
$docTail = 1..15 | ForEach-Object {
    'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
}
$f = Get-ExecutedTestCount -Lines $docTail
Assert-Equal -Expected 15 -Actual $f.groups -Message '(f1) fifteen groups reported'
Assert-Equal -Expected 0 -Actual $f.executed -Message '(f2) and every one of them executed nothing'
Assert-Equal -Expected 'none' -Actual (Get-TestExecutionVerdict -Totals $f) -Message '(f3) verdict none'
# The guard that matters: a partial view plus a real run must NOT read as none. One real summary
# anywhere in the same stream flips it, which is what would have happened had the head not been cut.
$g = Get-ExecutedTestCount -Lines (@('test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out') + $docTail)
Assert-Equal -Expected 'measured' -Actual (Get-TestExecutionVerdict -Totals $g) `
    -Message '(f4) one real summary among fifteen empty ones is MEASURED, not none'

Write-Host "`n=== (g) anchor: a test that prints the words is not a summary ==="
$h = Get-ExecutedTestCount -Lines @(
    'stdout: the fixture echoed test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
    'thread panicked while formatting test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'
)
Assert-Equal -Expected 0 -Actual $h.groups -Message '(g1) mid-line matches are not libtest summaries'
Assert-Equal -Expected 0 -Actual $h.executed -Message '(g2) and contribute no executed tests'
Assert-Equal -Expected 'unknown' -Actual (Get-TestExecutionVerdict -Totals $h) -Message '(g3) so the verdict stays unknown'

Write-Host "`n=== (h) a leading-whitespace summary is still a summary ==="
$i = Get-ExecutedTestCount -Lines @('   test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')
Assert-Equal -Expected 7 -Actual $i.executed -Message '(h1) indentation added by a wrapper does not hide the count'

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount" -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "$($script:total - $script:failures)/$($script:total) passed, $($script:failures) FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
exit 0
