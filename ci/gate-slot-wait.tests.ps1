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

$ExpectedAssertionCount = 9
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
    Assert-True ((Get-SlotWaitSecs -QueuedUtc $s -StartUtc $q) -eq 0) `
        'a start before its own queue instant is clamped to 0, never negative -- clock skew cannot mint a saving'
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
    Assert-True ($beforeClaim.Contains('$runQueuedUtc = [DateTime]::UtcNow')) `
        'the queued instant is stamped before the wait, under its own name, so the wait is a fact the manifest can carry'

    $manifest = Get-GateSlice -Start 'function Write-RunManifest {' -End "`n}"
    # THE ASSIGNMENT SHAPE, not the bare name. The first draft asked only whether the manifest text
    # CONTAINED 'Get-SlotWaitSecs', and a sabotage that replaced the derivation with inline
    # subtraction stayed green -- the comment above the field still named the function. A cell
    # satisfied by its own comment guards nothing; this one wants the call on the field's line.
    Assert-True ($manifest -match '(?m)^\s*slotWaitSecs\s*=\s*Get-SlotWaitSecs\s+-QueuedUtc\s+\$runQueuedUtc\s+-StartUtc\s+\$runStartUtc') `
        'the manifest derives slotWaitSecs through Get-SlotWaitSecs on the field line itself, not a second arithmetic beside a comment that names it'
    Assert-True ($manifest.Contains('queuedUtc')) `
        'and the queued instant itself, so a reader can re-derive the wait rather than trust the field'
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
