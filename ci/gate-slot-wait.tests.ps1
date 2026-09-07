# #956: a manifest's `runStartUtc` was stamped when the gate BEGAN WAITING for a slot, not when it
# claimed one. #982's record read 64.8 min for a 38.4-minute run; the 26.5 min between was a
# SLOT-WAIT line in the ledger, and anyone summing manifests into a wall-clock table got the wrong
# number by exactly that. The instant the machine started working is the claim; the instant the
# gate started waiting is a different fact, worth keeping under its own name.
#
# Two claims here. The derivation `Get-SlotWaitSecs` is cut out of ci/gate.ps1 and driven with real
# instants. The PLACEMENT is a containment claim, never a position claim: the run-start stamp lives
# INSIDE the claim block (between Enter-GateSlot and the first snapshot after it) and the queued
# stamp lives OUTSIDE it -- a sabotage that moves the stamp back above the wait reddens both.

$ExpectedAssertionCount = 12
$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
if (-not (Test-Path -LiteralPath $gatePath)) {
    Write-Host 'HARNESS-BROKE: ci/gate.ps1 is not beside this suite' -ForegroundColor Magenta
    exit 2
}
$gateText = [System.IO.File]::ReadAllText($gatePath)

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End, [switch] $IncludeEnd)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) { throw "HARNESS-BROKE: slice anchors did not match for [$Start]" }
    $end = if ($IncludeEnd) { $j + $End.Length } else { $j }
    return $gateText.Substring($i, $end - $i)
}

try {
    Invoke-Expression (Get-GateSlice -Start 'function Get-SlotWaitSecs {' -End "`n}" -IncludeEnd)

    $q = [DateTime]::Parse('2026-09-07T15:26:23Z').ToUniversalTime()
    $s = [DateTime]::Parse('2026-09-07T15:52:56Z').ToUniversalTime()
    Assert-True ((Get-SlotWaitSecs -QueuedUtc $q -StartUtc $s) -eq 1593) `
        "#982's own instants: queued 15:26:23, claimed 15:52:56 -> 1593 s of slot wait, the 26.5 minutes the manifest hid"
    Assert-True ((Get-SlotWaitSecs -QueuedUtc $s -StartUtc $s) -eq 0) `
        'a run that claimed at once waits 0 s -- every #958 datapoint reads this way against the ledger'
    Assert-True ($null -eq (Get-SlotWaitSecs -QueuedUtc $s -StartUtc $q)) `
        'a start before its own queue instant is published as ABSENT -- skew is unknown, and 0 would pass for an instant claim'
    Assert-True ((Get-SlotWaitSecs -QueuedUtc $q -StartUtc $s.AddMilliseconds(400)) -eq 1593.4) `
        'the wait keeps milliseconds, the same grain as wallTimeSecs'

    # PLACEMENT, BY CONTAINMENT. The claim block starts where the slot is asked for and ends at the
    # first snapshot taken after it; the run-start stamp must be inside it and the queued stamp must
    # not be. Position comparisons cannot say this -- the writer is defined above the call site, so
    # the wrong arrangement can have the smaller index -- but a slice can.
    $claimBlock = Get-GateSlice -Start 'Enter-GateSlot -Path $slotLockPath' -End '$slotLockAtStart = Read-SlotLockSnapshot'
    $beforeClaim = $gateText.Substring(0, $gateText.IndexOf('Enter-GateSlot -Path $slotLockPath', [System.StringComparison]::Ordinal))
    Assert-True ($claimBlock.Contains('$runStartUtc = [DateTime]::UtcNow')) `
        'runStartUtc is stamped INSIDE the claim block -- after the slot is held, so it is the instant the machine starts working'
    Assert-True (-not $beforeClaim.Contains('$runStartUtc = [DateTime]::UtcNow')) `
        'and NOT before it -- the stamp that used to sit at the top of the file, ahead of the wait, is gone'
    # ADJACENCY, not precedence. A stamp anywhere before the claim satisfied the first draft, and the
    # dot-sourcing and git probes between it and Enter-GateSlot put a median 0.274 s (max 1.49 s over
    # 98 runs) inside every free-slot wait. The last non-comment statement before the call must be it.
    $linesBeforeClaim = @($beforeClaim -split "`n")
    $linesBeforeClaim = $linesBeforeClaim[0..($linesBeforeClaim.Count - 2)]   # the last element is the call's own line, cut mid-way
    $lastBeforeClaim = ($linesBeforeClaim | Where-Object { $_.Trim() -ne '' -and -not $_.Trim().StartsWith('#') } | Select-Object -Last 1).Trim()
    Assert-True ($lastBeforeClaim -eq '$runQueuedUtc = [DateTime]::UtcNow') `
        "the queued instant is the LAST statement before Enter-GateSlot, so a free slot reads 0 and nothing but the wait is inside the number (last statement: $lastBeforeClaim)"

    $manifest = Get-GateSlice -Start 'function Write-RunManifest {' -End "`n}"
    # THE ASSIGNMENT SHAPE, not the bare name. The first draft asked only whether the manifest text
    # CONTAINED 'Get-SlotWaitSecs', and a sabotage that replaced the derivation with inline
    # subtraction stayed green -- the comment above the field still named the function. A cell
    # satisfied by its own comment guards nothing; this one wants the call on the field's line.
    Assert-True ($manifest -match '(?m)^\s*slotWaitSecs\s*=\s*Get-SlotWaitSecs\s+-QueuedUtc\s+\$runQueuedUtc\s+-StartUtc\s+\$runStartUtc') `
        'the manifest derives slotWaitSecs through Get-SlotWaitSecs on the field line itself, not a second arithmetic beside a comment that names it'
    Assert-True ($manifest.Contains('queuedUtc')) `
        'and the queued instant itself, so a reader can re-derive the wait rather than trust the field'

    # THE FRESHNESS BOUNDARY (ISSUES 1 on #985 and #987). Get-TestArtifactManifest judges every
    # artefact against $runStartUtc, and with the sentinel now $null a call before the claim would
    # read EVERY artefact as fresh -- silent, total, flattering. Three cells: the comparison is
    # against the claim instant; the call site sits after the claim restamp (searched from the
    # claim onwards, so the queued stamp cannot satisfy it); and the guard actually throws on null,
    # driven at runtime rather than read.
    $freshness = Get-GateSlice -Start 'function Get-TestArtifactManifest {' -End "`n}" -IncludeEnd
    Assert-True ($freshness -match '(?m)^\s*\$freshBuild\s*=\s*\(\$mtimeUtc\s+-ge\s+\$runStartUtc\)') `
        'the freshness cross-check compares against the CLAIM instant'
    $claimCallAt = $gateText.IndexOf('Enter-GateSlot -Path $slotLockPath', [System.StringComparison]::Ordinal)
    $claimStampAt = $gateText.IndexOf('$runStartUtc = [DateTime]::UtcNow', $claimCallAt, [System.StringComparison]::Ordinal)
    Assert-True ($claimStampAt -ge 0 -and $gateText.Substring($claimStampAt).Contains('$artifactManifest = Get-TestArtifactManifest')) `
        'and the artefact manifest is BUILT after that stamp -- before it, $runStartUtc is $null and every artefact reads fresh'
    Invoke-Expression $freshness
    $runStartUtc = $null
    $toolchain = '+956-no-such-toolchain'   # if the guard is missing, cargo must fail fast, not compile the workspace into this suite
    $threw = $false
    try { $null = Get-TestArtifactManifest } catch { $threw = $_.Exception.Message.Contains('before the slot claim') }
    Assert-True $threw `
        'called with a null claim instant, Get-TestArtifactManifest THROWS before asking cargo anything -- the silent all-fresh reading is not reachable'
} finally {
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-slot-wait: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-slot-wait: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
