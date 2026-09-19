# #638: isolated tests for ci/gate-run-overlap.ps1.
#
# Dot-sources ONLY gate-run-overlap.ps1, never gate.ps1. Every case builds its own manifests under
# a throwaway temp directory this script creates and removes itself, so running it needs no slot,
# no cargo, and no coordination with any other lane.
#
# Homegrown PASS/FAIL/HARNESS-BROKE harness with a declared expected count, matching
# ci/slot-lock.tests.ps1: PASS/FAIL alone cannot tell a complete run from one where a block was
# silently dropped by a bad merge, so a mismatch against the declared total is its own third
# outcome rather than a green with fewer assertions in it.
#
# 34 runtime assertions: 34 Assert-* CALLS below. (Was 23; three cells arrived with the
# second review round. The count has now caught a miscount of mine three times in one day,
# which is the argument for declaring it. Originally declared 25 from counting the
# review-driven cells as nine when they are seven; the harness refused with HARNESS-BROKE
# rather than passing 23 of them quietly, which is the whole reason the count is declared.) The two function DEFINITIONS are not calls, and
# Assert-Equal's internal delegation to Assert-True fires once per Assert-Equal call rather than as
# an independent assertion -- so a naive grep over this file returns a larger, wrong number.
$ExpectedAssertionCount = 34

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:total = 0
$script:failed = 0

function Assert-True {
    param([Parameter(Mandatory)][bool] $Condition, [Parameter(Mandatory)][string] $Label)
    $script:total++
    if ($Condition) { Write-Host "PASS  $Label" } else { $script:failed++; Write-Host "FAIL  $Label" }
}

function Assert-Equal {
    param([Parameter(Mandatory)][AllowNull()] $Expected, [Parameter(Mandatory)][AllowNull()] $Actual, [Parameter(Mandatory)][string] $Label)
    Assert-True -Condition ("$Expected" -eq "$Actual") -Label "$Label (expected '$Expected', got '$Actual')"
}

. "$PSScriptRoot/gate-run-overlap.ps1"

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("gate-run-overlap-tests-" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $root | Out-Null

function New-ManifestWithTarget {
    param([string] $Directory, [string] $Name, [string] $Start, [string] $End, [string] $TargetDir)
    $body = [ordered]@{ status = 'GREEN'; headSha = 'deadbeef'; runStartUtc = $Start; runEndUtc = $End; cargoTargetDir = $TargetDir }
    [System.IO.File]::WriteAllText((Join-Path $Directory $Name), ($body | ConvertTo-Json -Depth 4), (New-Object System.Text.UTF8Encoding($false)))
}

function New-Manifest {
    param([string] $Directory, [string] $Name, [string] $Start, [string] $End, [string] $Sha = 'deadbeef')
    $body = [ordered]@{ status = 'GREEN'; headSha = $Sha; runStartUtc = $Start; runEndUtc = $End }
    $json = $body | ConvertTo-Json -Depth 4
    [System.IO.File]::WriteAllText((Join-Path $Directory $Name), $json, (New-Object System.Text.UTF8Encoding($false)))
}

try {
    # ---- overlapping pair is accused -------------------------------------------------------
    $case = Join-Path $root 'overlapping'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'a.json' '2026-08-28T07:01:49Z' '2026-08-28T07:40:24Z' 'aaaa'
    New-Manifest $case 'b.json' '2026-08-28T07:20:00Z' '2026-08-28T08:00:00Z' 'bbbb'
    $read = Read-GateRunWindows -Directory $case
    $found = Find-OverlappingRuns -Windows $read.Windows
    Assert-Equal 2 $read.Windows.Count 'two well-formed manifests are read'
    Assert-Equal 1 $found.Count 'an intersecting pair is accused'
    Assert-Equal 1224 $found[0].OverlapSeconds 'the accusation carries the measured overlap, not just a flag'

    # ---- NEGATIVE CONTROL: touching windows are not an overlap -----------------------------
    # Without this case the guard could flag every adjacent pair and still look correct above.
    $case = Join-Path $root 'touching'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'a.json' '2026-08-28T07:00:00Z' '2026-08-28T07:30:00Z'
    New-Manifest $case 'b.json' '2026-08-28T07:30:00Z' '2026-08-28T08:00:00Z'
    $read = Read-GateRunWindows -Directory $case
    $found = Find-OverlappingRuns -Windows $read.Windows
    Assert-Equal 2 $read.Windows.Count 'both touching manifests are read'
    Assert-Equal 0 $found.Count 'windows that merely touch are NOT accused'

    # ---- NEGATIVE CONTROL: plainly separate windows ----------------------------------------
    $case = Join-Path $root 'separate'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'a.json' '2026-08-20T10:00:00Z' '2026-08-20T11:00:00Z'
    New-Manifest $case 'b.json' '2026-08-20T14:00:00Z' '2026-08-20T15:00:00Z'
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 0 (Find-OverlappingRuns -Windows $read.Windows).Count 'separate windows are not accused'

    # ---- a broken manifest is a THIRD STATE, never a silent skip ---------------------------
    # A file that cannot be read must not quietly shrink the population: that is exactly how a
    # broken instrument comes back as a reassuring zero.
    $case = Join-Path $root 'indeterminate'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'good.json' '2026-08-20T10:00:00Z' '2026-08-20T11:00:00Z'
    [System.IO.File]::WriteAllText((Join-Path $case 'broken.json'), '{ not json', (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText((Join-Path $case 'nofield.json'), '{ "status": "GREEN" }', (New-Object System.Text.UTF8Encoding($false)))
    [System.IO.File]::WriteAllText((Join-Path $case 'badstamp.json'), '{ "runStartUtc": "not-a-time", "runEndUtc": "also-not" }', (New-Object System.Text.UTF8Encoding($false)))
    New-Manifest $case 'backwards.json' '2026-08-20T11:00:00Z' '2026-08-20T10:00:00Z'
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 1 $read.Windows.Count 'only the well-formed manifest becomes a window'
    Assert-Equal 4 $read.Indeterminate.Count 'all four broken manifests are reported, not dropped'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'broken.json' }).Reason -eq 'unparseable json') -Label 'unparseable json is named as such'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'nofield.json' }).Reason -like 'missing*') -Label 'a missing window field is named'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'badstamp.json' }).Reason -eq 'unparseable timestamp') -Label 'an unparseable timestamp is named'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'backwards.json' }).Reason -eq 'end precedes start') -Label 'a window that runs backwards is named'

    # ---- an empty population returns a real zero, not $null --------------------------------
    $case = Join-Path $root 'empty'; New-Item -ItemType Directory -Path $case | Out-Null
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 0 $read.Windows.Count 'an empty directory yields zero windows'
    Assert-Equal 0 (Find-OverlappingRuns -Windows $read.Windows).Count 'zero overlaps is a countable zero, not null'

    # ---- three mutually overlapping runs are three pairs -----------------------------------
    $case = Join-Path $root 'triple'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'a.json' '2026-08-30T01:00:00Z' '2026-08-30T02:00:00Z'
    New-Manifest $case 'b.json' '2026-08-30T01:30:00Z' '2026-08-30T02:30:00Z'
    New-Manifest $case 'c.json' '2026-08-30T01:45:00Z' '2026-08-30T02:15:00Z'
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 3 (Find-OverlappingRuns -Windows $read.Windows).Count 'three mutually overlapping runs report three pairs'

    # ---- an oversized manifest is indeterminate, and is never READ -------------------------
    # The bound has to be checked before the read, or the reader pays the cost it exists to avoid.
    $case = Join-Path $root 'oversized'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'small.json' '2026-08-20T10:00:00Z' '2026-08-20T11:00:00Z'
    $big = '{"runStartUtc":"2026-08-20T10:00:00Z","runEndUtc":"2026-08-20T11:00:00Z","pad":"' + ('x' * 5MB) + '"}'
    [System.IO.File]::WriteAllText((Join-Path $case 'huge.json'), $big, (New-Object System.Text.UTF8Encoding($false)))
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 1 $read.Windows.Count 'an oversized manifest does not become a window'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'huge.json' }).Reason -like 'exceeds*') -Label 'an oversized manifest is reported as indeterminate, not skipped'

    # ---- valid json that is not an object ---------------------------------------------------
    # Each of these parses. None is a manifest. Before the guard, the property read threw OUTSIDE
    # the catch and killed the whole report -- one bad file silencing every good one.
    $case = Join-Path $root 'nonobject'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'good.json' '2026-08-20T10:00:00Z' '2026-08-20T11:00:00Z'
    foreach ($pair in @(@('null.json', 'null'), @('array.json', '[]'), @('number.json', '42'), @('string.json', '"x"'))) {
        [System.IO.File]::WriteAllText((Join-Path $case $pair[0]), $pair[1], (New-Object System.Text.UTF8Encoding($false)))
    }
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 1 $read.Windows.Count 'only the real manifest becomes a window'
    Assert-Equal 4 $read.Indeterminate.Count 'null, [], 42 and "x" are each reported rather than throwing'

    # ---- an overlap is classified by target-dir EVIDENCE, not by time -----------------------
    # Time proves concurrency. It does not prove contamination, and gate.ps1 permits isolation.
    $case = Join-Path $root 'sharing'; New-Item -ItemType Directory -Path $case | Out-Null
    New-ManifestWithTarget $case 'a.json' '2026-08-28T07:00:00Z' '2026-08-28T08:00:00Z' 'D:/same'
    New-ManifestWithTarget $case 'b.json' '2026-08-28T07:30:00Z' '2026-08-28T08:30:00Z' 'D:/same'
    $found = Find-OverlappingRuns -Windows (Read-GateRunWindows -Directory $case).Windows
    Assert-Equal 'unknown-same-path-text' $found[0].Sharing 'matching path TEXT is unknown, not proof of a shared directory'

    $case = Join-Path $root 'sharing-isolated'; New-Item -ItemType Directory -Path $case | Out-Null
    New-ManifestWithTarget $case 'a.json' '2026-08-28T07:00:00Z' '2026-08-28T08:00:00Z' 'D:/one'
    New-ManifestWithTarget $case 'b.json' '2026-08-28T07:30:00Z' '2026-08-28T08:30:00Z' 'D:/two'
    $found = Find-OverlappingRuns -Windows (Read-GateRunWindows -Directory $case).Windows
    Assert-Equal 'unknown-different-path-text' $found[0].Sharing 'UNEQUAL path text is also unknown -- one directory can be spelt two ways'

    $case = Join-Path $root 'sharing-unknown'; New-Item -ItemType Directory -Path $case | Out-Null
    New-ManifestWithTarget $case 'a.json' '2026-08-28T07:00:00Z' '2026-08-28T08:00:00Z' 'D:/one'
    New-Manifest $case 'b.json' '2026-08-28T07:30:00Z' '2026-08-28T08:30:00Z'
    $found = Find-OverlappingRuns -Windows (Read-GateRunWindows -Directory $case).Windows
    Assert-Equal 'unknown-no-target-dir' $found[0].Sharing 'a missing cargoTargetDir is its own unknown, never folded into an answer'

    # ---- the two spellings the repo's OWN test accepts are not two directories -------------
    # ci/gate-target-dir.tests.ps1:108-115 deliberately accepts `C:/explicit-isolated-target` AND
    # `C:\explicit-isolated-target`, and gate.ps1 records the raw environment value, so the
    # producer is documented to emit aliases of one path. Reading them as separate would be the
    # mirror of the shared-target overclaim this file already removed.
    $case = Join-Path $root 'aliases'; New-Item -ItemType Directory -Path $case | Out-Null
    New-ManifestWithTarget $case 'a.json' '2026-08-28T07:00:00Z' '2026-08-28T08:00:00Z' 'C:/explicit-isolated-target'
    New-ManifestWithTarget $case 'b.json' '2026-08-28T07:30:00Z' '2026-08-28T08:30:00Z' 'C:\explicit-isolated-target'
    $found = Find-OverlappingRuns -Windows (Read-GateRunWindows -Directory $case).Windows
    Assert-Equal 'unknown-different-path-text' $found[0].Sharing 'two spellings the producer is documented to accept are not called isolated'

    # ---- a duration beyond Int32 does not kill the report ----------------------------------
    # Manifests are untrusted; a [int] cast on centuries of seconds throws and takes the whole run.
    $case = Join-Path $root 'huge-window'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'a.json' '1900-01-01T00:00:00Z' '2100-01-01T00:00:00Z'
    New-Manifest $case 'b.json' '1901-01-01T00:00:00Z' '2099-01-01T00:00:00Z'
    $found = Find-OverlappingRuns -Windows (Read-GateRunWindows -Directory $case).Windows
    Assert-Equal 1 $found.Count 'a centuries-long overlap is reported rather than throwing'
    Assert-True -Condition ($found[0].OverlapSeconds -gt 2147483647) -Label 'the duration survives past the Int32 ceiling'

    # ---- the discovered population is bounded before anything is read ----------------------
    # If every file is malformed none becomes a window, so the window-count guard never fires --
    # the advertised aggregate bound was walked around by a sufficiently broken directory.
    $case = Join-Path $root 'flood'; New-Item -ItemType Directory -Path $case | Out-Null
    1..($script:MaxRunsForPairwise + 1) | ForEach-Object {
        [System.IO.File]::WriteAllText((Join-Path $case "$_.json"), 'not json', (New-Object System.Text.UTF8Encoding($false)))
    }
    $threw = $false
    $message = ''
    try { Read-GateRunWindows -Directory $case | Out-Null } catch { $threw = $true; $message = $_.Exception.Message }
    Assert-True -Condition $threw -Label 'an over-large directory of MALFORMED files is refused before being read'
    Assert-True -Condition ($message -like '*before reading them*') -Label 'the refusal says it happened before the read, not after'

    # ---- a timestamp with no explicit offset is indeterminate, never guessed ---------------
    # `[datetimeoffset]::Parse` on a naive string adopts the READING machine's local zone, so the
    # same manifest would yield a different window on two machines -- in a tool whose entire output
    # is the intersection of windows, that is the input to the conclusion being silently wrong.
    $case = Join-Path $root 'nooffset'; New-Item -ItemType Directory -Path $case | Out-Null
    New-Manifest $case 'good.json' '2026-08-20T10:00:00Z' '2026-08-20T11:00:00Z'
    New-Manifest $case 'naive.json' '2026-08-20T10:00:00' '2026-08-20T11:00:00'
    New-Manifest $case 'offset.json' '2026-08-20T10:00:00+02:00' '2026-08-20T11:00:00+02:00'
    $read = Read-GateRunWindows -Directory $case
    Assert-Equal 2 $read.Windows.Count 'Z and a numeric offset are both accepted'
    Assert-True -Condition (($read.Indeterminate | Where-Object { $_.Name -eq 'naive.json' }).Reason -like '*no explicit UTC offset*') -Label 'a naive timestamp is indeterminate and says why'

    # ---- the population is bounded BEFORE the quadratic scan -------------------------------
    # The per-file bound does not bound this: N mutually overlapping windows allocate N(N-1)/2
    # results, so a directory of small valid manifests still exhausts the reader.
    $many = @(1..($script:MaxRunsForPairwise + 1) | ForEach-Object {
        [pscustomobject]@{
            Name = "w$_.json"; HeadSha = 'a'; Status = 'GREEN'; TargetDir = $null
            Start = [datetimeoffset]::Parse('2026-08-20T10:00:00Z'); End = [datetimeoffset]::Parse('2026-08-20T11:00:00Z')
        }
    })
    $threw = $false
    try { Find-OverlappingRuns -Windows $many | Out-Null } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'an over-limit population refuses out loud instead of half-reporting'

    # ---- a forged log line in a target dir is neutralised before printing ------------------
    # ci/gate-target-dir.tests.ps1:124-137 proves a CRLF payload PASSES validation and is persisted
    # exactly, so this reaches the report as ordinary data. Printing it raw would let a manifest
    # write its own lines into the output someone reads to judge a run.
    $forged = "D:\safe`r`n2026-01-01 | gate | FORGED | payload"
    $rendered = Format-UntrustedValue $forged
    Assert-True -Condition ($rendered -notmatch "`r" -and $rendered -notmatch "`n") -Label 'a CRLF payload cannot break the report onto its own line'
    Assert-True -Condition ($rendered -like '*<U+000D>*') -Label 'the control character is shown, not silently dropped'
    Assert-True -Condition ($rendered -like '*FORGED*') -Label 'the payload text is still visible so the reader sees what was attempted'

    # ---- a missing directory is an error, never an empty result ----------------------------
    $threw = $false
    try { Read-GateRunWindows -Directory (Join-Path $root 'does-not-exist') | Out-Null } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'a missing directory throws instead of reporting zero runs'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount"
    exit 2
}
if ($script:failed -gt 0) { Write-Host "FAILED: $script:failed of $script:total"; exit 1 }
Write-Host "PASSED: $script:total of $ExpectedAssertionCount"
