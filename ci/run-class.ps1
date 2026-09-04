# #644: the ONE place the gate's class rule lives.
#
# `gate.ps1` decides the class when a run ends; `classify-run.ps1` recomputes it to check that a
# manifest's class is corroborated by the fields the gate wrote it from. Those were two copies of the
# same sentence, and the second copy was written to catch a manifest lying about the first -- an
# oracle duplicated is an oracle that can disagree with itself, and the copy that drifts is the one
# nobody is looking at. Pinning them with a guard would police the divergence; extracting removes it.
#
# Dot-sourced by both, the same way `gate.ps1` already dot-sources `slot-lock.ps1`. No cargo
# dependency, no slot dependency, no repository dependency: one pure function over two values.

function Get-RunClassFrom {
    <#
    .SYNOPSIS
        The class the gate assigns from a finished run's two verdicts. #644.

    .DESCRIPTION
        ONE rule with TWO outcomes, and both halves matter to callers. `green` is the absence of
        failures across BOTH verdicts -- the stage tally and the stricter combined verdict that also
        weighs the canary and stale artifacts. Anything else is `UNCLASSIFIED`, which is not a
        judgement either: it means nobody has read the failure yet.

        A caller that mirrors only the green half leaves `UNCLASSIFIED` beside a fully passing status
        unexamined -- a pair this function cannot produce, and exactly the defect that motivated
        putting the rule in one place.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [AllowNull()] [string] $Status,
        [Parameter(Mandatory)] [bool] $PassedEverything
    )

    # ORDINAL, BECAUSE -eq IS NOT. PowerShell's comparison operators are culture aware, and a
    # culture comparison gives some code points no weight at all: measured, 'GREEN' plus U+FE00,
    # U+00AD or U+FFFD each comes back -eq 'GREEN'. This one line decides the class the GATE writes
    # and the class `classify-run.ps1` recomputes to check it, so a status carrying an invisible
    # code point was classified `green` in both places at once -- the duplication this file removed
    # would at least have needed two mistakes.
    #
    # Ordinal and not OrdinalIgnoreCase: the gate writes GREEN in capitals, `classify-run.ps1`
    # refuses any status outside its closed vocabulary, and a lowercase `green` status is not a
    # value either producer emits. Case-insensitivity here would widen the rule, not preserve it.
    if ([string]::Equals($Status, 'GREEN', [System.StringComparison]::Ordinal) -and $PassedEverything) { 'green' }
    else { 'UNCLASSIFIED' }
}
