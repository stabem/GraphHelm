# #1217 focused tests. They observe accounting and disclosure at the reader boundary.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:total = 0
$script:failed = 0
$ExpectedAssertionCount = 30

function Assert-True {
    param([bool] $Condition, [string] $Label)
    $script:total++
    if ($Condition) { Write-Host "PASS  $Label" } else { $script:failed++; Write-Host "FAIL  $Label" }
}
function Assert-Equal {
    param($Expected, $Actual, [string] $Label)
    Assert-True ("$Expected" -eq "$Actual") "$Label (expected '$Expected', got '$Actual')"
}
function Write-Receipt {
    param([string] $Directory, [string] $Name, [hashtable] $Values)
    $json = ($Values | ConvertTo-Json -Depth 8)
    [IO.File]::WriteAllText((Join-Path $Directory $Name), $json, (New-Object Text.UTF8Encoding($false)))
}

. (Join-Path $PSScriptRoot 'delivery-time.ps1')
$root = Join-Path ([IO.Path]::GetTempPath()) ('delivery-time-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
try {
    # Overlap is unioned once, while execution remains additive. The second GREEN does not erase RED.
    Write-Receipt $root 'a.json' @{ status='RED'; pullRequest=1204; headSha='a'; runClass='red'; runStartUtc='2026-09-23T00:00:00Z'; runEndUtc='2026-09-23T00:10:00Z'; slotWaitSecs=2.5; coverage=@{postgres='included'} }
    Write-Receipt $root 'b.json' @{ status='GREEN'; pullRequest=1204; headSha='b'; runClass='green'; runStartUtc='2026-09-23T00:05:00Z'; runEndUtc='2026-09-23T00:20:00Z'; slotWaitSecs=1.5; coverage=@{postgres='included'} }
    $read = Read-DeliveryReceipts $root
    $report = Measure-DeliveryTime $read
    Assert-Equal 2 $report.population.validReceipts 'two valid receipts are selected'
    Assert-Equal 1500 $report.totals.executionSumSecs 'execution sum remains additive'
    Assert-Equal 1200 $report.totals.unionWallTimeSecs 'overlap is counted once in union wall time'
    Assert-Equal 1 $report.totals.statuses.RED 'RED history remains visible beside GREEN'
    Assert-Equal 1 $report.totals.statuses.GREEN 'GREEN history remains visible'
    Assert-Equal 4 $report.totals.slotWaitSecs 'slot wait is summed from slotWaitSecs only'
    Assert-Equal 'unknown: receipts record slotWaitSecs, not queue wait' $report.unknowns.queueWaitSecs 'queue wait is unknown'
    Assert-Equal 'unknown: receipts do not link later runs to earlier failures' $report.unknowns.retryCausality 'retry causality is unknown'
    Assert-Equal 1 $report.totals.additionalRuns 'one additional run is reported without calling it rework'
    Assert-Equal 'green' $report.pullRequests[0].receipts[1].runClass 'run class provenance is retained'

    # A nested interval still makes the observed span reach the outer end.
    $nested = Join-Path $root 'nested'; New-Item -ItemType Directory -Path $nested | Out-Null
    Write-Receipt $nested 'outer.json' @{ status='GREEN'; pullRequest=1300; runStartUtc='2026-09-23T02:00:00Z'; runEndUtc='2026-09-23T02:01:40Z'; slotWaitSecs=0 }
    Write-Receipt $nested 'inner.json' @{ status='GREEN'; pullRequest=1300; runStartUtc='2026-09-23T02:00:10Z'; runEndUtc='2026-09-23T02:00:20Z'; slotWaitSecs=0 }
    $nestedReport = Measure-DeliveryTime (Read-DeliveryReceipts $nested)
    Assert-Equal 100 $nestedReport.totals.observedSpanSecs 'nested intervals retain the outer observed span'
    Assert-Equal 100 $nestedReport.totals.unionWallTimeSecs 'nested intervals union to the outer duration'

    # Missing slot wait is unavailable evidence, never zero.
    $missingSlot = Join-Path $root 'missing-slot'; New-Item -ItemType Directory -Path $missingSlot | Out-Null
    Write-Receipt $missingSlot 'one.json' @{ status='GREEN'; pullRequest=1301; runStartUtc='2026-09-23T03:00:00Z'; runEndUtc='2026-09-23T03:01:00Z' }
    $missingSlotReport = Measure-DeliveryTime (Read-DeliveryReceipts $missingSlot)
    Assert-True ($null -eq $missingSlotReport.totals.slotWaitSecs) 'missing slot wait remains null'
    Assert-Equal 1 $missingSlotReport.totals.slotWaitMissingCount 'missing slot wait count is explicit'

    # Exact byte copies are one run with both source paths, not two executions.
    Copy-Item (Join-Path $root 'b.json') (Join-Path $root 'copy.json')
    $read = Read-DeliveryReceipts $root
    $report = Measure-DeliveryTime (Read-DeliveryReceipts $root)
    Assert-Equal 1 $report.population.duplicates 'exact content copies are deduplicated'
    Assert-Equal 2 $report.pullRequests[0].receipts[1].provenance.Count 'duplicate provenance keeps both paths'

    # Filtered reports still disclose malformed and unattributable evidence.
    Write-Receipt $root 'other-pr.json' @{ status='GREEN'; pullRequest=999; runStartUtc='2026-09-23T01:00:00Z'; runEndUtc='2026-09-23T01:01:00Z' }
    [IO.File]::WriteAllText((Join-Path $root 'broken.json'), '{', (New-Object Text.UTF8Encoding($false)))
    Write-Receipt $root 'unattributed.json' @{ status='GREEN'; pullRequest=$null; runStartUtc='2026-09-23T01:00:00Z'; runEndUtc='2026-09-23T01:01:00Z' }
    Write-Receipt $root 'missing-stamp.json' @{ status='GREEN'; pullRequest=1204; runEndUtc='2026-09-23T01:01:00Z' }
    $read = Read-DeliveryReceipts $root 1204
    Assert-Equal 2 @($read.receipts).Count 'PR filter keeps only the requested PR'
    Assert-Equal 3 @($read.indeterminate).Count 'malformed, unattributed, and missing timestamp files remain disclosed under filter'

    $json = & (Join-Path $PSScriptRoot 'report-delivery-time.ps1') -Directory $root -PullRequest 1204
    Assert-True (($json | ConvertFrom-Json).schema -eq 'graphhelm.delivery-time.v1') 'entrypoint emits JSON report'

    # Status keys and all other output must be byte-stable across separate processes.
    $repeat = Join-Path $root 'repeat'; New-Item -ItemType Directory -Path $repeat | Out-Null
    Write-Receipt $repeat 'z.json' @{ status='RED'; pullRequest=1500; runStartUtc='2026-09-23T05:00:00Z'; runEndUtc='2026-09-23T05:01:00Z' }
    Write-Receipt $repeat 'a.json' @{ status='GREEN'; pullRequest=1500; runStartUtc='2026-09-23T05:02:00Z'; runEndUtc='2026-09-23T05:03:00Z' }
    $one = Join-Path $root 'repeat-one.json'; $two = Join-Path $root 'repeat-two.json'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'report-delivery-time.ps1') -Directory $repeat > $one
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'report-delivery-time.ps1') -Directory $repeat > $two
    Assert-Equal (Get-FileHash -Algorithm SHA256 -LiteralPath $one).Hash (Get-FileHash -Algorithm SHA256 -LiteralPath $two).Hash 'report bytes are stable across processes'

    # Network/device paths and reparse-point directories are refused before receipt enumeration.
    $uncRejected = $false; try { Read-DeliveryReceipts '\\127.0.0.1\unreachable' | Out-Null } catch { $uncRejected = $true }
    $deviceRejected = $false; try { Read-DeliveryReceipts '\\.\PIPE\delivery-time' | Out-Null } catch { $deviceRejected = $true }
    Assert-True ($uncRejected -and $deviceRejected) 'UNC and device paths are rejected before access'
    $junctionTarget = Join-Path $root 'junction-target'; $junction = Join-Path $root 'junction-input'
    New-Item -ItemType Directory -Path $junctionTarget | Out-Null
    New-Item -ItemType Junction -Path $junction -Target $junctionTarget | Out-Null
    $junctionRejected = $false; try { Read-DeliveryReceipts $junction | Out-Null } catch { $junctionRejected = $true }
    Assert-True $junctionRejected 'reparse-point directory is rejected before enumeration'
    $ancestorChild = Join-Path $junction 'child-that-must-not-be-looked-up'
    $ancestorRejected = $false; $ancestorReason = ''
    try { Read-DeliveryReceipts $ancestorChild | Out-Null } catch { $ancestorRejected = $true; $ancestorReason = $_.Exception.Message }
    Assert-True ($ancestorRejected -and $ancestorReason -like '*reparse point*') 'ancestor reparse point is rejected before child lookup'

    # A time-only value must stay indeterminate instead of acquiring today's date.
    $timeOnly = Join-Path $root 'time-only'; New-Item -ItemType Directory -Path $timeOnly | Out-Null
    Write-Receipt $timeOnly 'time-only.json' @{ status='GREEN'; pullRequest=1600; runStartUtc='12:00Z'; runEndUtc='2026-09-23T12:01:00Z' }
    $timeOnlyRead = Read-DeliveryReceipts $timeOnly
    Assert-True (@($timeOnlyRead.indeterminate | Where-Object { $_.reason -like '*timestamp*' }).Count -eq 1) 'time-only timestamp is rejected as indeterminate'

    # Finite per-receipt waits whose aggregate would overflow must remain unavailable and JSON-safe.
    $overflow = Join-Path $root 'overflow'; New-Item -ItemType Directory -Path $overflow | Out-Null
    Write-Receipt $overflow 'one.json' @{ status='GREEN'; pullRequest=1601; runStartUtc='2026-09-23T13:00:00Z'; runEndUtc='2026-09-23T13:01:00Z'; slotWaitSecs=1e308 }
    Write-Receipt $overflow 'two.json' @{ status='GREEN'; pullRequest=1601; runStartUtc='2026-09-23T13:02:00Z'; runEndUtc='2026-09-23T13:03:00Z'; slotWaitSecs=1e308 }
    $overflowText = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'report-delivery-time.ps1') -Directory $overflow) -join "`n"
    $overflowJson = $overflowText | ConvertFrom-Json -ErrorAction Stop
    Assert-True (($null -eq $overflowJson.totals.slotWaitSecs) -and ($overflowText -notmatch 'Infinity')) 'overflowing slot wait stays unavailable in strict JSON'

    # Empty input is valid, but its unavailable timing metrics are null rather than zero.
    $empty = Join-Path $root 'empty'; New-Item -ItemType Directory -Path $empty | Out-Null
    $emptyReport = Measure-DeliveryTime (Read-DeliveryReceipts $empty)
    Assert-True ($null -eq $emptyReport.totals.executionSumSecs) 'empty input has no execution sum'
    Assert-True ($null -eq $emptyReport.totals.unionWallTimeSecs) 'empty input has no union wall time'
    Assert-True ($null -eq $emptyReport.totals.observedSpanSecs) 'empty input has no observed span'

    # Small fixtures exercise both population bounds without allocating large input.
    $bounded = Join-Path $root 'bounded'; New-Item -ItemType Directory -Path $bounded | Out-Null
    Write-Receipt $bounded 'a.json' @{ status='GREEN'; pullRequest=1400; runStartUtc='2026-09-23T04:00:00Z'; runEndUtc='2026-09-23T04:01:00Z' }
    Write-Receipt $bounded 'b.json' @{ status='GREEN'; pullRequest=1400; runStartUtc='2026-09-23T04:00:00Z'; runEndUtc='2026-09-23T04:01:00Z' }
    $oldFileLimit = $script:MaxDeliveryReceiptFiles
    $script:MaxDeliveryReceiptFiles = 1
    $fileLimitThrew = $false
    try { Read-DeliveryReceipts $bounded | Out-Null } catch { $fileLimitThrew = $true }
    $script:MaxDeliveryReceiptFiles = $oldFileLimit
    Assert-True $fileLimitThrew 'receipt population count is bounded before reading'
    $oldByteLimit = $script:MaxDeliveryReceiptBytes
    $script:MaxDeliveryReceiptBytes = 10
    $byteLimited = Read-DeliveryReceipts (Join-Path $root 'bounded')
    $script:MaxDeliveryReceiptBytes = $oldByteLimit
    Assert-True (@($byteLimited.indeterminate | Where-Object { $_.reason -like 'exceeds*' }).Count -gt 0) 'oversized receipt is disclosed without parsing'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $script:total" -ForegroundColor Magenta
    exit 2
}
if ($script:failed -gt 0) { Write-Host "FAILED: $script:failed/$script:total" -ForegroundColor Red; exit 1 }
Write-Host "PASSED: $script:total/$ExpectedAssertionCount" -ForegroundColor Green
exit 0
