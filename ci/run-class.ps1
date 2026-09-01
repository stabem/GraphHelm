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

    if ($Status -eq 'GREEN' -and $PassedEverything) { 'green' } else { 'UNCLASSIFIED' }
}
