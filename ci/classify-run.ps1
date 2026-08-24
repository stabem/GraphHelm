<#
.SYNOPSIS
    Settle a gate run's CLASS after the holder has read the failure. #199.

.DESCRIPTION
    `status: RED` records that a run failed and nothing about whether the failure means anything.
    Three classes are indistinguishable in an exit code:

        real-red       the code failed something. The only class that is evidence about code.
        dead           killed by a lock collision, a clean underneath it, an aborted slot.
                       Says nothing at all.

    `instrument-red` USED TO BE A THIRD CLASS AND IS NOT ONE ANY MORE. Real data would not fit it:
    A Agent's `gate.log` failed `rustfmt` and `clippy` -- CODE -- while carrying four stale binaries
    -- INSTRUMENT -- in the SAME run. Both true at once, and an exclusive class forces one label,
    erasing the half that decides whether the red is citable.

    So "was the instrument broken?" is now the manifest's own DERIVED field, `instrumentSuspect`,
    computed by `gate.ps1` from `staleArtifactCount` and `canaryPassed` at the moment those numbers
    are produced. This script only READS it. It does not compute it, deliberately: a value
    calculated at classification time would attach itself to runs nobody measured.

    ABSENT IS NOT FALSE. A pre-taxonomy manifest has no `instrumentSuspect`, and that means NOT
    MEASURED -- never "the instrument was healthy".

    Measured on 2026-08-20: one lane produced three runs, one of each, and ZERO verdicts between
    them - while a census that counted "two complete runs, both RED" had no way to tell them apart.

    The gate cannot decide this. A GREEN run classifies itself; a RED one is a judgement the holder
    makes once they have read the failure, so the manifest ships `runClass: UNCLASSIFIED` and this
    script is how that value stops being UNCLASSIFIED.

    Both the in-repo manifest and the durable copy are updated when both are present, because a
    class that lands on one of two copies is worse than no class: the two then disagree.

.PARAMETER Manifest
    Path to the run manifest, in the repo or in the slot directory.

.PARAMETER Class
    real-red | instrument-red | dead

.PARAMETER Because
    One line of why. Required: a class with no reason is a label, and the next reader cannot check
    a label.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Manifest,
    [Parameter(Mandatory)] [ValidateSet('real-red', 'dead')] [string] $Class,
    [Parameter(Mandatory)] [string] $Because,

    # #199: declare the failure UNRELATED to the diff under judgement. All three are required
    # together, because each alone is cheap to assert and worthless on its own:
    #   -UnrelatedTestFile   the failing test's file. VERIFIED HERE against the diff -- this is the
    #                        one condition a script can check, so it is checked rather than trusted.
    #   -UnrelatedIssue      the issue carrying the evidence. A failure nobody filed is a failure
    #                        nobody will look at again.
    #   -FailThenPassObserved  the holder saw it fail and then pass IN THE SAME RUN. Not checkable
    #                        from here; required explicitly so that claiming it is a deliberate act.
    [string] $UnrelatedTestFile,
    [string] $UnrelatedIssue,
    [switch] $FailThenPassObserved,

    # #199: the OTHER answer, and it deliberately carries NO bar.
    #
    # KNOWN AND DELIBERATE ASYMMETRY, written here so a later census is not surprised by it:
    # `relatedToDiff = $true` travels with only `runClassBecause`, while `$false` travels with three
    # evidence fields. That is not an oversight -- the bar guards the CHEAP claim ("not mine"), and
    # the expensive one needs no guard -- but it means a future count will find `true` unstructured
    # and `false` structured. Anyone aggregating these must not read the missing fields beside a
    # `true` as a weaker record; there is nothing to record. (Observed by C Agent in review.)
    #
    # The three conditions above guard the CHEAP sentence ("not mine"); this is the expensive one,
    # and a holder who reads the failure and concludes it belongs to their own diff must be able to
    # SAY so. Without this switch the field has no path to $true at all, so "judged, and it is mine"
    # would be indistinguishable from "nobody judged" -- the exact collapse this field was added to
    # prevent, on its own axis.
    [switch] $Mine
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $Manifest)) {
    throw "no manifest at $Manifest"
}

# ReadAllText, then a strict parse: this file exists to be machine-read, and a BOM or a provider-
# decorated string would defeat that downstream. Same discipline as the writer in gate.ps1.
$json = [System.IO.File]::ReadAllText($Manifest)
$run = $json | ConvertFrom-Json

# #199: ERA MARKER. A manifest written before this change has NO `runClass` property at all, and
# PowerShell reads an absent property as falsy -- so the guard below waved it straight through and
# this script would have written a class onto a pre-#202 manifest, producing a record
# INDISTINGUISHABLE from one the new gate had produced. The run it describes was never judged
# against these categories, and nothing in the file would say so afterwards. (Found in review by
# L Agent.) Refused by NAME rather than by value, because absent and UNCLASSIFIED are different
# facts: one means "not judged yet", the other means "this gate could not have judged it".
if (-not ($run.PSObject.Properties.Name -contains 'runClass')) {
    throw "this manifest has no runClass field, so it predates the taxonomy (#202). Classifying it would produce a record that looks like a judged run of the new gate and is not one. Re-run the gate, or annotate the file by hand and say it was pre-taxonomy."
}

if ($run.runClass -and $run.runClass -ne 'UNCLASSIFIED') {
    throw "this run is already classified as '$($run.runClass)'. Classifying twice would overwrite a judgement someone already made; edit deliberately if that is what you mean."
}

# #199: attribution, and the gate that keeps "not mine" from being the cheapest sentence to write.
#
# The default is $null -- UNKNOWN -- and stays there unless all three conditions are supplied. Any
# ONE of them alone is an assertion; together they are a record someone else can re-check. The
# boundary is deliberate: WITHOUT THE THREE, A RED IS A RED.
$relatedToDiff = $null
if ($Mine -and ($UnrelatedTestFile -or $UnrelatedIssue -or $FailThenPassObserved)) {
    throw "-Mine and the unrelated-* switches are opposite answers to the same question. Pick one."
}
if ($Mine) {
    $relatedToDiff = $true
}
if ($UnrelatedTestFile -or $UnrelatedIssue -or $FailThenPassObserved) {
    if (-not ($UnrelatedTestFile -and $UnrelatedIssue -and $FailThenPassObserved)) {
        throw "declaring a failure unrelated to the diff needs ALL THREE: -UnrelatedTestFile, -UnrelatedIssue and -FailThenPassObserved. One of them alone is the cheapest sentence in this taxonomy to write and the least checkable."
    }

    # The condition a script CAN check, so it is checked. `git log --name-only` over the range plus
    # the uncommitted diff: if the failing test's file is in either, the failure is inside the work
    # under judgement and this claim is refused rather than recorded.
    # $ErrorActionPreference='Continue' around every native call, and NO `2>$null`. Caught by this
    # script's own live test: under Windows PowerShell 5.1 a redirected native stderr line becomes a
    # NativeCommandError, which 'Stop' promotes to a TERMINATING error -- so git's routine
    # "LF will be replaced by CRLF" warning aborted the check. Worse than aborting: the abort looked
    # like the refusal this block exists to produce, and the negative test PASSED FOR THE WRONG
    # REASON. Same hazard `Invoke-Stage` in `gate.ps1` handles the same way.
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $repoRoot = (git rev-parse --show-toplevel)
    if (-not $repoRoot) { $ErrorActionPreference = $previousPreference; throw "not inside a git repository: cannot verify that $UnrelatedTestFile is outside the diff" }
    Push-Location -LiteralPath $repoRoot
    try {
        $touched = @()
        $touched += (git diff --name-only)
        $touched += (git diff --name-only --cached)
        $touched += (git log --format= --name-only 'origin/main..HEAD')
        $needle = $UnrelatedTestFile.Replace('\', '/')
        $hit = $touched | Where-Object { $_ -and ($_.Replace('\', '/') -eq $needle) }
        if ($hit) {
            throw "$UnrelatedTestFile IS touched by this diff (found in the range or the working tree), so a failure in it is not unrelated. Refused."
        }
    } finally {
        Pop-Location
        $ErrorActionPreference = $previousPreference
    }

    $relatedToDiff = $false
    # The GRADE travels with the value, or it is lost in the sum. Written flat, these three read as
    # siblings and a later reader cannot tell that only ONE of them passed through an instrument.
    # The decisive condition -- fail-then-pass in the same run -- is the one this script CANNOT
    # check, so the record says so in the value itself rather than only in a comment nobody reads
    # beside the JSON.
    $run | Add-Member -NotePropertyName unrelatedTestFile -NotePropertyValue $UnrelatedTestFile -Force
    $run | Add-Member -NotePropertyName unrelatedTestFileVerifiedAgainstDiff -NotePropertyValue $true -Force
    $run | Add-Member -NotePropertyName unrelatedIssue -NotePropertyValue $UnrelatedIssue -Force
    $run | Add-Member -NotePropertyName failThenPassObserved -NotePropertyValue 'asserted-by-holder' -Force
}

$run | Add-Member -NotePropertyName runClass -NotePropertyValue $Class -Force
$run | Add-Member -NotePropertyName relatedToDiff -NotePropertyValue $relatedToDiff -Force
$run | Add-Member -NotePropertyName runClassBecause -NotePropertyValue $Because -Force
$run | Add-Member -NotePropertyName runClassAtUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o')) -Force

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$out = $run | ConvertTo-Json -Depth 8

$written = New-Object System.Collections.Generic.List[string]
$slotDir = if ($env:GRAPHHELM_SLOT_DIR) { $env:GRAPHHELM_SLOT_DIR } else { 'D:/graphhelm-slot' }
$fileName = Split-Path -Leaf $Manifest

# ORDER IS THE MITIGATION, and it is chosen rather than defaulted. Two copies cannot be written
# atomically, so one of them is written first and the question is WHICH DIVERGENCE IS SURVIVABLE if
# the second write dies. This file's own doc says a class landing on one of two copies is worse than
# no class, because the two then disagree -- so:
#   DURABLE first, COMMITTABLE second.
# A crash between them leaves the committable copy UNCLASSIFIED, which reads as "nobody judged yet"
# and is true. The reverse order leaves a committed, classified manifest whose durable twin says
# otherwise -- a disagreement someone would have to arbitrate with no way to tell which is right.
#
# Both writes are wrapped and both REPORT: the earlier version left the first write bare under
# 'Stop', so a failure on the second aborted the script with the first already changed and nothing
# printed -- the operator saw an exception and never learned that one copy had moved. (Found in
# review by C Agent, who also noted it made this PR's security-review sentence about "both write
# paths are wrapped" false. It is true now.)
$targets = New-Object System.Collections.Generic.List[string]
$durable = [System.IO.Path]::Combine([System.IO.Path]::Combine($slotDir, 'gate-runs'), $fileName)
if ((Test-Path -LiteralPath $durable) -and ((Resolve-Path -LiteralPath $durable).Path -ne (Resolve-Path -LiteralPath $Manifest).Path)) {
    $targets.Add($durable)
}
$targets.Add($Manifest)

foreach ($target in $targets) {
    try {
        [System.IO.File]::WriteAllText($target, $out, $utf8NoBom)
        $written.Add($target)
    } catch {
        Write-Host "ERROR: could not write $target : $($_.Exception.Message)" -ForegroundColor Red
        if ($written.Count -gt 0) {
            Write-Host "  ALREADY WRITTEN, so the copies now DISAGREE: $($written -join ', ')" -ForegroundColor Red
            Write-Host "  Reconcile before citing either." -ForegroundColor Red
        }
        throw
    }
}

try {
    # .NET APIs, not New-Item/Join-Path: both fail NON-TERMINATINGLY, printing a raw error and
    # then handing the catch a misleading one. Found by gate.ps1's negative control, applied here.
    [System.IO.Directory]::CreateDirectory($slotDir) | Out-Null
    $line = '{0} | gate | RUN-CLASSIFIED | class={1} manifest={2} because={3}' -f [DateTime]::UtcNow.ToString('o'), $Class, $fileName, $Because
    [System.IO.File]::AppendAllText([System.IO.Path]::Combine($slotDir, 'SLOT.log'), $line + "`n", $utf8NoBom)
} catch {
    Write-Host "WARNING: could not append to SLOT.log: $($_.Exception.Message)" -ForegroundColor Yellow
}

# Read, never computed here. Three states, and the third is the point: absent means the run predates
# the measurement, which is a different fact from "the instrument was fine".
if ($run.PSObject.Properties.Name -contains 'instrumentSuspect') {
    if ($run.instrumentSuspect) {
        Write-Host "  NOTE: instrumentSuspect = TRUE (staleArtifactCount=$($run.staleArtifactCount), canaryPassed=$($run.canaryPassed))." -ForegroundColor Yellow
        Write-Host "  A '$Class' verdict can be true AT THE SAME TIME as a broken instrument -- that is why this is a field and not a class." -ForegroundColor Yellow
        Write-Host "  Say in -Because whether the code failure stands on its own." -ForegroundColor Yellow
    } else {
        Write-Host "  instrumentSuspect = false (measured by the gate)."
    }
} else {
    Write-Host "  instrumentSuspect: NOT MEASURED (this manifest predates the field). Absent is not false." -ForegroundColor Yellow
}

Write-Host "classified as $Class"
foreach ($path in $written) { Write-Host "  updated: $path" }
