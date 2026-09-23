# #1217: bounded, offline accounting of gate receipt time.
# This file is a reader and calculator only. Receipt JSON is untrusted input and is never executed.

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script:MaxDeliveryReceiptBytes = 8MB
$script:MaxDeliveryReceiptFiles = 2048
$script:MaxDeliveryReceiptPopulationBytes = 256MB

function New-DeliveryIndeterminate {
    param([string] $Name, [string] $Reason, [int] $PullRequest = -1)
    [ordered]@{
        file = $Name
        reason = $Reason
        pullRequest = if ($PullRequest -ge 0) { $PullRequest } else { $null }
    }
}

function ConvertTo-DeliveryTimestamp {
    param([object] $Value)
    if ($Value -isnot [string] -or [string]::IsNullOrWhiteSpace($Value)) { return $null }
    # A date-only or time-only value would make the reader invent a calendar context. The producer
    # writes complete ISO date-times with an explicit UTC offset.
    if ($Value -notmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})$') { return $null }
    $parsed = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse($Value, [Globalization.CultureInfo]::InvariantCulture,
        [Globalization.DateTimeStyles]::RoundtripKind, [ref] $parsed)) { return $null }
    return $parsed.ToUniversalTime()
}

function ConvertTo-DeliveryNumber {
    param([object] $Value)
    if ($null -eq $Value -or $Value -is [bool]) { return $null }
    $number = 0.0
    if (-not [double]::TryParse([string]$Value, [Globalization.NumberStyles]::Float,
        [Globalization.CultureInfo]::InvariantCulture, [ref] $number)) { return $null }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or $number -lt 0) { return $null }
    return $number
}

function Read-DeliveryFileBytes {
    param([string] $Path, [int64] $MaxBytes, [int64] $RemainingBytes)
    $stream = $null; $memory = New-Object IO.MemoryStream; $readTotal = 0L; $tooLarge = $false; $reason = $null
    try {
        $stream = [IO.File]::OpenRead($Path)
        $buffer = New-Object byte[] 65536
        while ($true) {
            $remainingLimit = [Math]::Min($MaxBytes + 1L, $RemainingBytes + 1L) - $readTotal
            if ($remainingLimit -le 0) { $tooLarge = $true; $reason = if ($RemainingBytes -le $readTotal) { 'receipt population byte budget exceeded' } else { "exceeds $MaxBytes bytes" }; break }
            $count = [int][Math]::Min($buffer.Length, $remainingLimit)
            $n = $stream.Read($buffer, 0, $count)
            if ($n -eq 0) { break }
            $memory.Write($buffer, 0, $n); $readTotal += $n
            if ($readTotal -gt $MaxBytes) { $tooLarge = $true; $reason = "exceeds $MaxBytes bytes"; break }
            if ($readTotal -gt $RemainingBytes) { $tooLarge = $true; $reason = 'receipt population byte budget exceeded'; break }
        }
        [pscustomobject]@{ bytes = if ($tooLarge) { $null } else { $memory.ToArray() }; readBytes = $readTotal; tooLarge = $tooLarge; reason = $reason }
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
        $memory.Dispose()
    }
}

function Assert-DeliverySafeDirectory {
    param([Parameter(Mandatory)][string] $Directory)
    if ($Directory -match '^(\\\\|//|\\\\[?.])') { throw 'unsafe receipt path: UNC or device path' }
    $full = [IO.Path]::GetFullPath($Directory)
    if ($full -match '^(\\\\|//|\\\\[?.])') { throw 'unsafe receipt path: UNC or device path' }
    $root = [IO.Path]::GetPathRoot($full)
    if ([string]::IsNullOrWhiteSpace($root)) { throw 'unsafe receipt path: no local root' }
    try {
        $drive = New-Object IO.DriveInfo($root)
        if ($drive.DriveType -eq [IO.DriveType]::Network) { throw 'unsafe receipt path: network drive' }
    } catch [IO.IOException] { throw 'unsafe receipt path: drive could not be classified' }
    # Walk downward from the already-classified local root. Looking up the full leaf first would
    # allow a junction/symlink ancestor to redirect the lookup before the ancestor is inspected.
    $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop
    if (($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'unsafe receipt path: directory ancestor is a reparse point' }
    $relative = $full.Substring($root.Length).TrimStart('\', '/')
    $currentPath = $root.TrimEnd('\') + '\'
    if ([string]::IsNullOrWhiteSpace($relative)) { return $currentPath }
    foreach ($part in ($relative -split '[\\/]+' | Where-Object { $_ -ne '' })) {
        $candidate = Join-Path $currentPath $part
        try { $current = Get-Item -LiteralPath $candidate -Force -ErrorAction Stop } catch { throw "receipt directory does not exist: $Directory" }
        if (($current.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'unsafe receipt path: directory ancestor is a reparse point' }
        $currentPath = $current.FullName.TrimEnd('\') + '\'
    }
    if (-not ($current.PSIsContainer)) { throw "receipt directory does not exist: $Directory" }
    return $current.FullName
}

function Assert-DeliverySafeFile {
    param([Parameter(Mandatory)][string] $Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'unsafe receipt input: file is a reparse point' }
}

function Read-DeliveryReceipts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string] $Directory,
        [Nullable[int]] $PullRequest
    )

    $safeDirectory = Assert-DeliverySafeDirectory $Directory
    $files = New-Object System.Collections.Generic.List[string]
    foreach ($path in [IO.Directory]::EnumerateFiles($safeDirectory, '*.json')) {
        if ($files.Count -ge $script:MaxDeliveryReceiptFiles) { throw "receipt population exceeds $($script:MaxDeliveryReceiptFiles) files before reading them" }
        $files.Add($path)
    }
    $files = @($files | Sort-Object)
    $receipts = New-Object System.Collections.Generic.List[object]
    $indeterminate = New-Object System.Collections.Generic.List[object]
    $duplicates = New-Object System.Collections.Generic.List[object]
    $seen = @{}
    $actualBytesRead = 0L
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        foreach ($file in $files) {
            Assert-DeliverySafeFile $file
            $fileName = [IO.Path]::GetFileName($file)
            $loaded = Read-DeliveryFileBytes $file $script:MaxDeliveryReceiptBytes ($script:MaxDeliveryReceiptPopulationBytes - $actualBytesRead)
            $actualBytesRead += $loaded.readBytes
            if ($loaded.tooLarge) {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName $loaded.reason))
                continue
            }
            $bytes = $loaded.bytes
            $digest = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
            if ($seen.ContainsKey($digest)) {
                $seen[$digest].provenance.Add($file)
                $duplicates.Add([ordered]@{ file = $file; duplicateOf = $seen[$digest].file; sha256 = $digest })
                continue
            }
            try {
                $text = [Text.Encoding]::UTF8.GetString($bytes)
                $raw = $text | ConvertFrom-Json -ErrorAction Stop
                if ($null -eq $raw -or $raw -is [array] -or $raw -isnot [psobject]) { throw 'JSON root is not an object' }
            } catch {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName 'unparseable json')); continue
            }

            $prValue = $null
            if ($raw.PSObject.Properties.Name -contains 'pullRequest') { $prValue = $raw.pullRequest }
            $pr = 0
            if ($null -eq $prValue -or -not [int]::TryParse([string]$prValue, [ref]$pr) -or $pr -lt 1) {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName 'unattributable pull request')); continue
            }
            $startValue = if ($raw.PSObject.Properties.Name -contains 'runStartUtc') { $raw.runStartUtc } else { $null }
            $endValue = if ($raw.PSObject.Properties.Name -contains 'runEndUtc') { $raw.runEndUtc } else { $null }
            $start = ConvertTo-DeliveryTimestamp $startValue
            $end = ConvertTo-DeliveryTimestamp $endValue
            if ($null -eq $start -or $null -eq $end) {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName 'missing or invalid UTC timestamp') ); continue
            }
            if ($end -lt $start) {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName 'end precedes start' $pr)); continue
            }
            $status = if ($raw.PSObject.Properties.Name -contains 'status') { [string]$raw.status } else { '' }
            if ([string]::IsNullOrWhiteSpace($status)) {
                $indeterminate.Add((New-DeliveryIndeterminate $fileName 'missing status' $pr)); continue
            }
            $slot = $null
            if ($raw.PSObject.Properties.Name -contains 'slotWaitSecs') { $slot = ConvertTo-DeliveryNumber $raw.slotWaitSecs }
            $record = [pscustomobject][ordered]@{
                file = $file
                sha256 = $digest
                provenance = (New-Object System.Collections.Generic.List[string])
                pullRequest = $pr
                headSha = if ($raw.PSObject.Properties.Name -contains 'headSha') { [string]$raw.headSha } else { $null }
                runClass = if ($raw.PSObject.Properties.Name -contains 'runClass') { [string]$raw.runClass } else { $null }
                coverage = if ($raw.PSObject.Properties.Name -contains 'coverage') { $raw.coverage } else { $null }
                status = $status
                startedUtc = $start.ToString('o')
                endedUtc = $end.ToString('o')
                durationSecs = ($end - $start).TotalSeconds
                slotWaitSecs = $slot
            }
            $record.provenance.Add($file)
            $seen[$digest] = $record
            if ($null -eq $PullRequest -or ([int]$PullRequest) -eq $pr) { $receipts.Add($record) }
        }
    } finally { $sha.Dispose() }

    $selected = $null
    if ($null -ne $PullRequest) { $selected = [int]$PullRequest }
    [ordered]@{
        receipts = $receipts.ToArray()
        indeterminate = $indeterminate.ToArray()
        duplicates = $duplicates.ToArray()
        filesRead = $files.Count
        selectedPullRequest = $selected
    }
}

function Get-DeliveryFiniteSum {
    param([object[]] $Values)
    $sum = 0.0
    foreach ($value in $Values) {
        $candidate = $sum + [double]$value
        if ([double]::IsNaN($candidate) -or [double]::IsInfinity($candidate)) {
            return [pscustomobject]@{ Value = $null; Overflow = $true }
        }
        $sum = $candidate
    }
    [pscustomobject]@{ Value = $sum; Overflow = $false }
}

function Measure-DeliveryTime {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object] $ReadResult)

    $groups = @($ReadResult.receipts | Group-Object -Property pullRequest | Sort-Object { [int]$_.Name })
    $perPr = New-Object System.Collections.Generic.List[object]
    $allIntervals = New-Object System.Collections.Generic.List[object]
    $sum = 0.0; $slotValues = New-Object System.Collections.Generic.List[double]; $slotObservedCount = 0; $slotOverflow = $false
    $statusTotals = @{}
    foreach ($group in $groups) {
        $items = @($group.Group | Sort-Object startedUtc, file)
        $intervals = @($items | ForEach-Object { [pscustomobject]@{ Start = [DateTimeOffset]::Parse($_.startedUtc); End = [DateTimeOffset]::Parse($_.endedUtc) } })
        foreach ($r in $items) {
            $sum += [double]$r.durationSecs
            if ($null -ne $r.slotWaitSecs) { $slotValues.Add([double]$r.slotWaitSecs); $slotObservedCount++ }
            $key = [string]$r.status
            if (-not $statusTotals.ContainsKey($key)) { $statusTotals[$key] = 0 }
            $statusTotals[$key]++
            $allIntervals.Add($intervals[$items.IndexOf($r)])
        }
        $union = Get-DeliveryUnionSeconds $intervals
        $span = Get-DeliverySpan $intervals
        $groupSlotItems = @($items | Where-Object { $null -ne $_.slotWaitSecs })
        $groupSlot = $null
        if ($groupSlotItems.Count -gt 0) {
            $groupSlotResult = Get-DeliveryFiniteSum @($groupSlotItems | Select-Object -ExpandProperty slotWaitSecs)
            $groupSlot = $groupSlotResult.Value
            if ($groupSlotResult.Overflow) { $slotOverflow = $true }
        }
        $perPr.Add([pscustomobject][ordered]@{
            pullRequest = [int]$group.Name
            runCount = $items.Count
            additionalRuns = [Math]::Max(0, $items.Count - 1)
            executionSumSecs = $items | Measure-Object durationSecs -Sum | Select-Object -ExpandProperty Sum
            unionWallTimeSecs = $union
            observedSpanSecs = $span
            slotWaitSecs = $groupSlot
            slotWaitObservedCount = $groupSlotItems.Count
            statuses = [ordered]@{ green = @($items | Where-Object status -eq 'GREEN').Count; red = @($items | Where-Object status -eq 'RED').Count; other = @($items | Where-Object { $_.status -notin @('GREEN','RED') }).Count }
            receipts = @($items)
        })
    }
    $slotTotal = $null
    if ($slotObservedCount -gt 0 -and -not $slotOverflow) {
        $totalSlotResult = Get-DeliveryFiniteSum $slotValues.ToArray()
        $slotTotal = $totalSlotResult.Value
        if ($totalSlotResult.Overflow) { $slotOverflow = $true; $slotTotal = $null }
    }
    $totalUnion = $null
    $totalSpan = $null
    $totalExecution = $null
    if (@($ReadResult.receipts).Count -gt 0) {
        $totalUnion = Get-DeliveryUnionSeconds $allIntervals
        $totalSpan = Get-DeliverySpan $allIntervals
        $totalExecution = $sum
    }
    $slotState = 'observed'
    if ($slotOverflow) { $slotState = 'unavailable: observed slot waits exceed finite numeric range' }
    elseif ($slotObservedCount -eq 0) { $slotState = 'unavailable: no slot wait observations' }
    $statusOutput = [ordered]@{}
    foreach ($statusKey in @('GREEN', 'RED') + @($statusTotals.Keys | Sort-Object | Where-Object { $_ -notin @('GREEN', 'RED') })) {
        if ($statusTotals.ContainsKey($statusKey)) { $statusOutput[$statusKey] = $statusTotals[$statusKey] }
    }
    [ordered]@{
        schema = 'graphhelm.delivery-time.v1'
        population = [ordered]@{ filesRead = $ReadResult.filesRead; validReceipts = @($ReadResult.receipts).Count; indeterminate = @($ReadResult.indeterminate).Count; duplicates = @($ReadResult.duplicates).Count }
        filter = [ordered]@{ pullRequest = $ReadResult.selectedPullRequest }
        totals = [ordered]@{ executionSumSecs = $totalExecution; unionWallTimeSecs = $totalUnion; observedSpanSecs = $totalSpan; slotWaitSecs = $slotTotal; slotWaitObservedCount = $slotObservedCount; slotWaitMissingCount = @($ReadResult.receipts).Count - $slotObservedCount; slotWaitState = $slotState; statuses = $statusOutput; additionalRuns = [Math]::Max(0, @($ReadResult.receipts).Count - $groups.Count) }
        pullRequests = $perPr.ToArray()
        indeterminate = @($ReadResult.indeterminate)
        duplicates = @($ReadResult.duplicates)
        unknowns = [ordered]@{ queueWaitSecs = 'unknown: receipts record slotWaitSecs, not queue wait'; retryCausality = 'unknown: receipts do not link later runs to earlier failures'; endToEndDeliverySecs = 'unknown: no delivery observer is present' }
    }
}

function Get-DeliveryUnionSeconds {
    param([object[]] $Intervals)
    if ($null -eq $Intervals -or $Intervals.Count -eq 0) { return 0.0 }
    $ordered = @($Intervals | Sort-Object Start, End); $start = $ordered[0].Start; $end = $ordered[0].End; $total = 0.0
    if ($ordered.Count -eq 1) { return ($end - $start).TotalSeconds }
    foreach ($i in $ordered[1..($ordered.Count - 1)]) {
        if ($i.Start -gt $end) { $total += ($end - $start).TotalSeconds; $start = $i.Start; $end = $i.End }
        elseif ($i.End -gt $end) { $end = $i.End }
    }
    return $total + ($end - $start).TotalSeconds
}

function Get-DeliverySpan {
    param([object[]] $Intervals)
    if ($null -eq $Intervals -or $Intervals.Count -eq 0) { return 0.0 }
    $ordered = @($Intervals | Sort-Object Start, End)
    $maxEnd = $ordered[0].End
    foreach ($item in $ordered) { if ($item.End -gt $maxEnd) { $maxEnd = $item.End } }
    return ($maxEnd - $ordered[0].Start).TotalSeconds
}
