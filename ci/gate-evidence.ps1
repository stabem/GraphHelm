# #810: choosing which lines of a failed stage's transcript the run manifest keeps.
#
# Split out of gate.ps1 for the same reason ci/slot-lock.ps1 was (#200): the decision is a pure
# function of a list of strings and a budget, so it can be exercised with constructed transcripts
# in ci/gate-evidence.tests.ps1 without running cargo, touching a slot, or having a repository.
# gate.ps1 dot-sources this file.

# No Set-StrictMode here, deliberately, and the same choice ci/slot-lock.ps1 makes. gate.ps1
# sets `-Version 2.0` before it dot-sources this file; a `-Version Latest` here would raise
# the strictness of THE ENTIRE REST OF THE GATE as a side effect of loading one helper, which
# is a behaviour change to every stage and has nothing to do with what this file does. The
# tests set 2.0 explicitly so the function is exercised under the strictness it runs under in
# production.

# Lines that NAME a failure, as opposed to lines that merely accompany one.
#
# CASE IS LOAD-BEARING and this is the whole reason the pattern is applied with -cmatch. cargo's
# per-binary summary line for a binary that passed reads
#
#     test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
#
# so a case-INSENSITIVE `failed` matches every one of those and marks the entire transcript as
# informative, which is the same as marking none of it. The failing binary says `FAILED`, in
# capitals, and that is what discriminates. ci/gate-evidence.tests.ps1 pins this with a transcript
# built only from passing summaries: it must yield NO markers.
# `^\s*FAIL:` is NOT cargo's vocabulary and is here for a stage cargo never runs. `ci/gate.ps1`'s
# `ci powershell suites` stage runs ci/run-ps-suites.ps1, whose suites print `  FAIL: <message>`
# for a failing assertion. `FAIL:` is not `FAILED`, so without this the ONLY line the selector
# could see in that stage is `FAILED in: <suite> (exit 1)` at run-ps-suites.ps1:95 -- four lines
# from the end. The window would anchor there, run to the end, and return the tail: exactly the
# behaviour this file replaces, arriving through the "a failure was named" branch instead of the
# fallback. gate-manifest-provenance.tests.ps1 alone carries 165 assertions; the `FAIL:` line
# saying WHICH one broke is the line a reader needs.
#
# #810's measurement was taken over `workspace tests` only, so this stage class was never in the
# population the marker set was chosen from. Found in review of that PR.
#
# The anchor is deliberately NOT `HARNESS-BROKE`: that is the third outcome, "the suite could not
# vouch for its own run", and it is not a failing test. Anchoring on it would be a category error.
$script:GateEvidenceFailureMarkers =
    '^failures:|^error(\[|:)|panicked at|FAILED|^Diff in |assertion .*failed|^\s*FAIL:'

# #917: the markers above are two different KINDS of line, and which one the window opens on
# decides whether the excerpt can be diagnosed.
#
# libtest announces a failure the moment it happens (`test x ... FAILED`) and explains it only at
# the END, in the `failures:` section that carries `panicked at <file>:<line>` and the assertion.
# Anchoring on the first marker of any kind opens the window on the ANNOUNCEMENT, spends the budget
# on the `... ok` lines that follow it, and leaves the explanation inside the omitted gap. Measured
# on two manifests committed to main: PR #871's `workspace tests` kept its panic site because the
# failure happened to be late in the binary; PR #854's `cli: api_http` lost both the site and the
# assertion because it happened to be early. Whether a run is diagnosable from its record must not
# depend on where in the binary the failing cell sits.
#
# So: prefer an EXPLANATION marker, and fall back to the full set when a stage announces a failure
# and never explains it (a binary that exits non-zero printing nothing). `^\s*FAIL:` is here rather
# than in the announcement set because a PowerShell suite's `  FAIL: <message>` IS the explanation --
# that stage prints its reason and its `FAILED in:` line separately, and #810 anchored it correctly.
#
# The announcement is not lost: `^failures:` and the stage's own summary sit inside $TailReserve.
$script:GateEvidenceExplanationMarkers =
    '^failures:|^error(\[|:)|panicked at|^Diff in |assertion .*failed|^\s*FAIL:'

<#
.SYNOPSIS
Pick the lines of a failed stage's output that the manifest should keep.

.DESCRIPTION
The manifest keeps a bounded excerpt of a red stage's transcript so that "a reader sees the named
assertion in the JSON itself, not only in a console scrollback that may already be gone"
(ci/gate.ps1). Keeping the LAST N lines satisfies that only when the failure is the last thing the
stage printed.

The `workspace tests` stage runs `cargo test --workspace --no-fail-fast`, and --no-fail-fast is
precisely the instruction to KEEP GOING after a binary fails. Every remaining test binary then
prints its own trailing summary, so the failing binary's `failures:` block ends up separated from
the end of the transcript by one summary block per binary that ran afterwards. Measured across 29
manifests (#810): single-binary stages named their failure in 10 of 10 captured tails, the
workspace stage in 2 of 12 -- and the ten uninformative ones were not empty but FULL, forty lines
of other binaries reporting `0 passed; 0 failed`.

No constant fixes that, because the distance depends on how many binaries follow the failing one.
So the excerpt is SELECTED rather than sliced: anchor on the first line that names a failure, keep
a window forward from there, and always keep the last few lines so the stage's own ending is still
visible. When nothing names a failure the last $Budget lines are kept, which is exactly the old
behaviour and is the right answer for the stages where it already worked.

The result never exceeds $Budget lines, so the compiler-error wall the original comment set out to
keep out of every red manifest still stays out.

.OUTPUTS
System.String[]. PowerShell unrolls a returned array, so call sites wrap in @().
#>
function Select-GateEvidenceLines {
    [CmdletBinding()]
    [OutputType([string[]])]
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [AllowNull()]
        # A real transcript contains BLANK LINES -- cargo prints one before `failures:`
        # and around a panic message. Without this, PowerShell's Mandatory validation
        # refuses the whole array the moment one element is empty, so the function would
        # have thrown on every genuine stage transcript while passing every fixture that
        # happened not to contain one. ci/gate-evidence.tests.ps1 keeps that blank line.
        [AllowEmptyString()]
        [string[]] $Lines,

        # Total lines the excerpt may occupy. 40 is gate.ps1's long-standing budget and this
        # change deliberately does not move it: the defect is where the window is AIMED, not how
        # wide it is, and widening it would trade one silent failure for the wall of compiler
        # output the budget exists to exclude.
        [int] $Budget = 40,

        # How many lines at the very end are kept regardless of where the failure is, so the
        # stage's own final summary and exit are always in the record.
        [int] $TailReserve = 5,

        # #917: how many lines the ANNOUNCEMENT region keeps when the tail is grown backwards to
        # reach an explanation.
        #
        # This replaces a fixed `$ExplanationReserve = 14` cap, which ISSUES 3 measured as the wrong
        # instrument: an explanation further from the end than 14 lines was not reached at all, so on
        # a transcript with two failing cells and ordinary multi-line panic output (82 lines) the fix
        # kept NEITHER explanation -- the exact case `--no-fail-fast` makes routine.
        #
        # The tail's real bound is what the budget leaves, not a constant. Whatever the window and
        # the notice do not need belongs to the explanation region, and the window needs very
        # little: in the #854 manifest 33 of its 40 lines were `... ok` from cells that PASSED.
        # Three lines carry the announcement; the rest is evidence.
        [int] $AnnouncementKeep = 3
    )

    if ($null -eq $Lines) { return @() }
    $count = $Lines.Count
    if ($count -le $Budget) { return $Lines }

    $first = -1
    for ($i = 0; $i -lt $count; $i++) {
        if ($Lines[$i] -cmatch $script:GateEvidenceFailureMarkers) { $first = $i; break }
    }

    # Nothing names a failure. The last $Budget lines are as good an excerpt as exists, and this is
    # what the stage would have recorded before this change.
    if ($first -lt 0) {
        return $Lines[($count - $Budget)..($count - 1)]
    }

    # #917: WHERE THE RESERVED TAIL STARTS, not where the window opens.
    #
    # The anchor stays on the first marker of any kind, because for libtest that is
    # `test x ... FAILED` and it is the ONLY line carrying the failing cell's name -- moving the
    # anchor to the explanation loses it (measured: it reddens
    # `gate-postgres-evidence.tests.ps1`'s "outputTail carries test name"). But the explanation --
    # `panicked at <file>:<line>` and the assertion -- is printed at the END, past a window sized
    # for the announcement, so a fixed five-line tail lands after it and keeps only the summary.
    #
    # So the tail is grown BACKWARDS to begin at the explanation when one exists beyond the window,
    # bounded by $ExplanationReserve so a stage that explains itself early cannot swallow the
    # budget. The window still opens on the name; the gap and its notice still sit between them.
    $explain = -1
    for ($i = $first + 1; $i -lt $count; $i++) {
        if ($Lines[$i] -cmatch $script:GateEvidenceExplanationMarkers) { $explain = $i; break }
    }

    $tailStart = $count - $TailReserve

    # GROW ONLY WHEN GROWING REACHES THE EXPLANATION. The first spelling of this said
    # `Max($explain, $count - $allowance)`, which silently picked the floor whenever the explanation
    # was further from the end than the allowance -- landing the tail in a region of passing
    # summaries that explains nothing, AND shrinking the window that had been reaching the
    # explanation on its own. It reddened four cells on the original fixture, where the panic sits
    # three lines after the anchor and the window already covered it.
    #
    # A rule that fires when it cannot achieve its purpose is worse than one that does not fire: the
    # `Max` made the excerpt worse than `main` for the shape `main` already handled.
    $allowance = $Budget - $AnnouncementKeep - 1
    if ($allowance -lt $TailReserve) { $allowance = $TailReserve }
    if ($explain -ge 0 -and $explain -lt $tailStart -and ($count - $explain) -le $allowance) {
        $tailStart = $explain
    }

    # One line of the budget pays for the notice that says lines were dropped, so a reader never
    # has to wonder whether the excerpt is contiguous.
    $windowBudget = $Budget - ($count - $tailStart) - 1
    if ($windowBudget -lt 1) { $windowBudget = 1 }
    $windowEnd = [Math]::Min($first + $windowBudget - 1, $count - 1)

    # The window already runs into the reserved tail: no gap to announce, so emit one run of lines.
    # This branch yields at most $Budget - 1 lines (it is only reached when $first is late enough).
    if ($windowEnd -ge $tailStart - 1) {
        return $Lines[$first..($count - 1)]
    }

    $omitted = $tailStart - ($windowEnd + 1)

    # HOW MANY MORE FAILURES ARE IN THE GAP, because `--no-fail-fast` makes more than one the
    # EXPECTED case rather than an edge -- it is the instruction to keep going after a binary
    # fails. The window anchors on the first named failure and never looks again, so without this
    # the excerpt names one failure and the notice reads as though what it dropped were filler:
    # "N lines omitted ... and the end of the stage" invites a reader to triage one failure, fix
    # it, and meet the next on the re-run, or to report that the run had one.
    #
    # A record that reads as complete while being partial is worse than one that admits its
    # shape. The count does not make the excerpt bigger -- widening it would trade this for the
    # compiler wall the budget excludes -- it makes the excerpt honest about what it is not.
    # #238: COUNT WHAT THE READER IS ASKING ABOUT. The previous wording -- "N further line(s)
    # naming a failure" -- is literally true and reads as N further FAILURES. Measured on #920's
    # gate: the notice said 6, and the omitted region held ONE additional failing test. Cargo
    # spends about six marker lines per failing target (`failures:` twice, `panicked at`,
    # `test result: FAILED`, `error: test failed`, and the `test <name> ... FAILED` line), so the
    # number over-states by roughly that factor. A lane read it as six problems and ordered a
    # 7100-line log decode to find them; the run had two failures in total, one root cause.
    #
    # So: where libtest named the failing tests, report THOSE -- distinct, because one test can
    # be named on several lines. Where it did not (a PowerShell suite, a compiler wall, a stage
    # that fails without libtest), fall back to the marker count and SAY it is markers. Both
    # branches answer the question the reader actually has, and neither invents a number the
    # transcript cannot back.
    $furtherNamed = 0
    $furtherTests = New-Object System.Collections.Generic.List[string]
    for ($k = $windowEnd + 1; $k -lt $tailStart; $k++) {
        $line = $Lines[$k]
        if ($line -cmatch $script:GateEvidenceFailureMarkers) { $furtherNamed++ }
        # ORDINAL and case-sensitive, like every other match in this file: libtest writes
        # `test <name> ... FAILED` and nothing else in a transcript legitimately looks like it.
        $named = [regex]::Match([string]$line, '^test\s+(\S+)\s+\.\.\.\s+FAILED')
        if ($named.Success -and -not $furtherTests.Contains($named.Groups[1].Value)) {
            $furtherTests.Add($named.Groups[1].Value)
        }
    }
    $notice = "[gate] ... $omitted lines omitted between the first named failure above and the end of the stage"
    # BOTH CLAUSES, NEVER ONE INSTEAD OF THE OTHER (second pass on this PR, focused-nightingale).
    # The first version made these an if/elseif, so a region holding BOTH libtest failures and
    # marker lines libtest did not name -- a PowerShell suite's `FAIL:` lines, a compiler wall, any
    # stage that fails without libtest -- reported the test names and dropped the marker count in
    # silence.
    #
    # AND THE DIRECTION WAS THE WORSE ONE. The defect this function fixes OVER-stated: a reader
    # chased six problems that were one, which costs time and reaches no wrong conclusion. Dropping
    # the marker clause UNDER-states: the reader is told "1 further failing test" and concludes the
    # excerpt is nearly the whole story while the region also held failures of another kind. An
    # instrument that errs high wastes an hour; one that errs low loses a failure. Shipping the
    # second inside the fix for the first would have been incoherent.
    #
    # The marker count is NOT presented as additional to the named tests -- the named tests spend
    # marker lines of their own, and a second number claiming to be extra would be the original
    # over-statement wearing a different label. It says what it counted, and what that is.
    $clauses = @()
    if ($furtherTests.Count -gt 0) {
        $clauses += "$($furtherTests.Count) further failing test(s): $($furtherTests -join ', ')"
    }
    if ($furtherNamed -gt 0) {
        $clauses += "$furtherNamed further line(s) MATCHING THE FAILURE MARKER (marker lines, not distinct failures)"
    }
    if ($clauses.Count -gt 0) {
        $notice += ", INCLUDING " + ($clauses -join ', and ') + " -- this excerpt is NOT the whole story"
    }
    $notice += ' ...'

    $selected = New-Object System.Collections.Generic.List[string]
    $selected.AddRange([string[]] $Lines[$first..$windowEnd])
    $selected.Add($notice)
    $selected.AddRange([string[]] $Lines[$tailStart..($count - 1)])
    return $selected.ToArray()
}
