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
        [int] $TailReserve = 5
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

    # One line of the budget pays for the notice that says lines were dropped, so a reader never
    # has to wonder whether the excerpt is contiguous.
    $windowBudget = $Budget - $TailReserve - 1
    $windowEnd = [Math]::Min($first + $windowBudget - 1, $count - 1)
    $tailStart = $count - $TailReserve

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
    $furtherNamed = 0
    for ($k = $windowEnd + 1; $k -lt $tailStart; $k++) {
        if ($Lines[$k] -cmatch $script:GateEvidenceFailureMarkers) { $furtherNamed++ }
    }
    $notice = "[gate] ... $omitted lines omitted between the first named failure above and the end of the stage"
    if ($furtherNamed -gt 0) {
        $notice += ", INCLUDING $furtherNamed further line(s) naming a failure -- this excerpt is NOT the whole story"
    }
    $notice += ' ...'

    $selected = New-Object System.Collections.Generic.List[string]
    $selected.AddRange([string[]] $Lines[$first..$windowEnd])
    $selected.Add($notice)
    $selected.AddRange([string[]] $Lines[$tailStart..($count - 1)])
    return $selected.ToArray()
}
