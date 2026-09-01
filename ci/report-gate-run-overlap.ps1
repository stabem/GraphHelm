# #638: report gate runs that overlapped in time.
#
#   powershell -File ci/report-gate-run-overlap.ps1                       # the committed manifests
#   powershell -File ci/report-gate-run-overlap.ps1 -Directory D:/graphhelm-slot/gate-runs
#
# REPORTS, does not gate. Wiring this into gate.ps1 as a blocking stage would turn it red on
# history the moment it landed -- the machine store already holds overlapping pairs -- and a gate
# nobody can make green gets disabled rather than fixed. Whether overlap becomes blocking, and from
# which date forward, is a decision for #638, not a side effect of adding the reader.
#
# Exit codes: 0 clean, 3 overlaps found, 4 indeterminate manifests present with no overlaps. A
# non-zero for indeterminate is deliberate -- "I could not read some of the population" must not
# leave the same trace as "I read all of it and found nothing".

[CmdletBinding()]
param(
    # Resolved in the body, not here: a param default is evaluated in the CALLER's scope, where
    # $PSScriptRoot is empty, and `Join-Path` then fails before the script has run a line.
    [string] $Directory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($Directory)) {
    $Directory = Join-Path $PSScriptRoot '../.factory/gate-runs'
}

. "$PSScriptRoot/gate-run-overlap.ps1"

$read = Read-GateRunWindows -Directory $Directory
$overlaps = Find-OverlappingRuns -Windows $read.Windows

Write-Host "gate-run manifests in $Directory"
Write-Host "  windows read   : $($read.Windows.Count)"
Write-Host "  indeterminate  : $($read.Indeterminate.Count)"
Write-Host "  overlapping    : $($overlaps.Count)"

if ($read.Indeterminate.Count -gt 0) {
    Write-Host ""
    Write-Host "INDETERMINATE -- these files are part of the population and could not be read;"
    Write-Host "they are neither evidence of overlap nor evidence of its absence:"
    foreach ($item in $read.Indeterminate) {
        Write-Host ("  {0,-44} {1}" -f (Format-UntrustedValue $item.Name), (Format-UntrustedValue $item.Reason))
    }
}

if ($overlaps.Count -gt 0) {
    $samePath = @($overlaps | Where-Object { $_.Sharing -eq 'unknown-same-path-text' })
    $diffPath = @($overlaps | Where-Object { $_.Sharing -eq 'unknown-different-path-text' })
    $noTarget = @($overlaps | Where-Object { $_.Sharing -eq 'unknown-no-target-dir' })

    Write-Host ""
    Write-Host "OVERLAPPING RUNS -- two gates whose windows intersect. That is a fact about TIME."
    Write-Host "Whether they could contaminate each other is a separate question with its own"
    Write-Host "evidence, and ci/gate.ps1 explicitly permits a run to isolate its target dir:"
    Write-Host ("  unknown-same-path-text      : {0}" -f $samePath.Count)
    Write-Host ("  unknown-different-path-text : {0}" -f $diffPath.Count)
    Write-Host ("  unknown-no-target-dir       : {0}" -f $noTarget.Count)
    Write-Host ""
    Write-Host "EVERY BUCKET BEGINS WITH unknown, AND THAT IS THE FINDING:"
    Write-Host "  Equal path text does not prove one directory: no manifest field records a host,"
    Write-Host "  so one name on two machines is two directories."
    Write-Host "  UNEQUAL path text does not prove two either: C:/target and C:\target are one"
    Write-Host "  directory spelt twice, and so are a trailing separator, a .., a symlink, or two"
    Write-Host "  mounts of one share. gate.ps1 records the RAW environment value, and"
    Write-Host "  ci/gate-target-dir.tests.ps1 deliberately ACCEPTS both spellings."
    Write-Host "  Canonicalising the text would not help: resolving a symlink or a mount needs the"
    Write-Host "  filesystem that wrote it, on a host this artefact does not name."
    Write-Host ""
    Write-Host "  So the contamination question is UNANSWERABLE from these manifests today. These"
    Write-Host "  labels report the EVIDENCE, never a conclusion. Read them as cannot-be-established,"
    Write-Host "  never as measured-absent: the same counts on the page, opposite claims underneath."
    Write-Host ""
    Write-Host "THREE LIMITS, all structural, none inferable away:"
    Write-Host "  WHO ran is not recorded, so an overlap is never evidence of two owners."
    Write-Host "  WHICH HOST is not recorded, so a matching target path proves nothing."
    Write-Host "  Manifest names were head+second until #667, so two runs sharing both overwrote"
    Write-Host "  each other at the producer -- counts over that period are a LOWER BOUND."

    foreach ($pair in $overlaps) {
        Write-Host ("  [{0}] {1}s  {2}  ||  {3}" -f $pair.Sharing, $pair.OverlapSeconds, (Format-UntrustedValue $pair.A), (Format-UntrustedValue $pair.B))
        Write-Host ("        {0} -> {1}" -f $pair.WindowStartUtc, $pair.WindowEndUtc)
        if ($pair.Sharing -eq 'unknown-same-path-text') {
            Write-Host ("        both name {0} -- same text, unknown whether same directory" -f (Format-UntrustedValue $pair.TargetDirA))
        }
    }
    exit 3
}

if ($read.Indeterminate.Count -gt 0) { exit 4 }
Write-Host ""
Write-Host "no overlapping gate runs in this population"
