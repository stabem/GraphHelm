# #909: how many tests a run actually EXECUTED, from libtest's own summary lines.
#
# WHY A COUNT AND NOT A TRANSCRIPT. `Invoke-Postgres` publishes `Get-Content -Tail 80` of each of the
# child's two streams, and the temp files are deleted afterwards. That bound is deliberate and tested
# (`gate-postgres-evidence.tests.ps1` asserts the human log stays at or under 80 noise lines), and it
# means the head of a PostgreSQL stage's output is gone before any reader sees it. A stage that ran
# every ignored test and a stage that ran none therefore produce THE SAME ARTEFACT: 80 trailing lines,
# `outputTail: null` for a passing stage, and no count anywhere. Nothing downstream can tell them
# apart, and on 2026-09-05 a reader (me) read one as the other and published it.
#
# A count survives a tail. A transcript does not. This is the smallest thing that makes the question
# answerable at all.
#
# THREE STATES, NOT TWO. The caller must be able to distinguish:
#
#     groups = 0                 nothing said `test result:` -- the READER saw no summary at all.
#                                Unknown. Never report this as "zero tests ran": that is exactly the
#                                inference the tail already invited once.
#     groups > 0, executed = 0   every binary reported, and every one of them ran nothing.
#                                THIS is a genuine zero, and it is the state #909 is about.
#     executed > 0               the run measured something.
#
# `passed + failed` is what "executed" means here. `ignored` did not run -- that is the whole point of
# the ignored matrix, where the tests are meant to STOP being ignored. `filtered out` did not run
# either, and `measured` is benchmarks.

Set-StrictMode -Version 2.0

# libtest's summary, anchored. A test that PRINTS the words "test result:" in its own stdout must not
# be counted: only a line that begins with them is libtest's own. `^` matters more than it looks --
# the ignored matrix runs with --test-threads=1 precisely so output interleaving is predictable, and a
# fixture that echoes its input is a normal thing for this repository's suites to do.
#
# ORDINAL digits: `\d` in .NET regex is Unicode-aware and would accept non-ASCII digit forms that
# [int]::Parse under an invariant culture then rejects, turning a parse into a throw inside a counter
# whose entire job is to be boring. [0-9] says what is meant.
$script:TestResultPattern =
    '^test result: \S+\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out'

function Get-ExecutedTestCount {
    <#
    .SYNOPSIS
        Executed-test totals parsed from libtest summary lines.
    .OUTPUTS
        [ordered] with: executed, passed, failed, ignored, filteredOut, groups.
    #>
    param(
        [AllowEmptyCollection()] [AllowNull()] [string[]] $Lines
    )

    $totals = [ordered]@{
        executed    = 0
        passed      = 0
        failed      = 0
        ignored     = 0
        filteredOut = 0
        groups      = 0
    }
    if ($null -eq $Lines) { return $totals }

    foreach ($line in $Lines) {
        if ($null -eq $line) { continue }
        # TrimStart only: libtest indents nothing, but a caller that read the line through a wrapper
        # may have. Trailing content after "filtered out" (the `finished in 0.78s` tail) is left
        # unmatched on purpose -- pinning it would make the parser fail on a libtest that stops
        # printing the duration.
        $match = [regex]::Match($line.TrimStart(), $script:TestResultPattern)
        if (-not $match.Success) { continue }

        $totals.groups++
        $inv = [System.Globalization.CultureInfo]::InvariantCulture
        $passed = [int]::Parse($match.Groups[1].Value, $inv)
        $failed = [int]::Parse($match.Groups[2].Value, $inv)
        $totals.passed += $passed
        $totals.failed += $failed
        $totals.ignored += [int]::Parse($match.Groups[3].Value, $inv)
        $totals.filteredOut += [int]::Parse($match.Groups[5].Value, $inv)
        $totals.executed += $passed + $failed
    }

    return $totals
}

# The verdict the gate acts on, kept beside the count so the three states are named in one place
# rather than re-derived at each call site.
#
# 'unknown' is NOT a pass and NOT a failure: it says the reader could not see. A gate that treats it
# as either is making up the answer, which is the defect this file exists to end.
function Get-TestExecutionVerdict {
    param([Parameter(Mandatory)] $Totals)

    if ($Totals.groups -eq 0) { return 'unknown' }
    if ($Totals.executed -eq 0) { return 'none' }
    return 'measured'
}
