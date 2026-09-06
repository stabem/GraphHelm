<#
.SYNOPSIS
    What is on D:, who left it there, and when it was last written. Read-only. (#847)

.DESCRIPTION
    D: is the only spinning disk on this machine and every lane's `CARGO_TARGET_DIR` lives on it.
    This lists the build targets so removal can be decided BY NAME later, by somebody who can see
    what each one is. It removes nothing and writes nothing outside its own output.

    WHY IT DOES NOT RECURSE BY DEFAULT, measured rather than assumed (2026-09-05):

        Get-ChildItem D:\ -Directory                      257 entries      55 ms
        ... | Where-Object Name -like '*target*'           121 entries    1.1 s
        Get-ChildItem D:\ -Directory -Recurse -Depth 2    TIMED OUT AT 10 MINUTES

    Name and `LastWriteTime` come from the directory entry and are free. **Size does not exist as
    metadata** -- a directory's size is the sum of its contents, so asking for it means walking a
    cargo target of hundreds of thousands of small files, on a HDD, while five gates are building.
    That is the ten-minute scan above and worse. So `-WithSize` is opt-in, and the default answers
    the question that can be answered cheaply: what is here, whose it looks like, and how stale.

    A LISTING IS NOT A REMOVAL PLAN. `LastWriteTime` says when bytes last changed, never whether a
    build is running now. A target with a live process chain is in use however old it looks, and
    the process chain is what decides -- not this list, and not a log (#823).

.PARAMETER Root
    The drive or directory to inventory. Defaults to `D:\`.

.PARAMETER WithSize
    Also compute each directory's size. Expect minutes to hours under load; see above.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .factory/tools/target-inventory.ps1
#>
param(
    [string] $Root = 'D:\',
    [switch] $WithSize
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Attribution is from the NAME and says so. Every row is labelled `by-name`, because a directory
# called `c-753-target` is evidence about what somebody typed, not about who holds it now. The
# alternative -- opening handles to find the owner -- is neither read-only nor cheap.
function Get-TargetAttribution {
    param([Parameter(Mandatory)] [string] $Name)

    $lane = $null
    $ticket = $null

    # `c-753-target`, `g-220b-targets`, `m827-target`: a single letter, then a separator or digits.
    if ($Name -cmatch '^(?<lane>[a-z])[-]?(?=\d)') { $lane = $Matches['lane'].ToUpperInvariant() }
    elseif ($Name -cmatch '^(?<lane>[a-z])-') { $lane = $Matches['lane'].ToUpperInvariant() }

    # `issue438`, `pr467`, `gh-review-441-target`, `m827-target`: the first 3-4 digit run.
    if ($Name -cmatch '(?<n>\d{3,4})') { $ticket = $Matches['n'] }

    if ($null -eq $lane -and $null -eq $ticket) { return 'unattributed' }
    if ($null -eq $lane) { return "#$ticket" }
    if ($null -eq $ticket) { return "lane $lane" }
    return "lane $lane / #$ticket"
}

# ENUMERATE ONCE, AND KEEP THE ERRORS. `-ErrorAction SilentlyContinue` is the only way an entry
# can go missing here, and it is invisible by construction: what it drops never reaches a count, so
# comparing counts downstream compares two numbers that cannot differ (J's review of #850 -- my own
# first guard was exactly that, a comparison of a list against itself).
# `-ErrorVariable` keeps what `SilentlyContinue` swallows, so the loss becomes a fact the guard
# below can read instead of an absence nothing can see.
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$allDirectories = @(Get-ChildItem -LiteralPath $Root -Directory -Force `
        -ErrorAction SilentlyContinue -ErrorVariable enumerationErrors)
$found = @($allDirectories | Where-Object { $_.Name -like '*target*' })
$enumerateMs = $stopwatch.ElapsedMilliseconds

# THE TWO CONTROLS A ZERO NEEDS, because "nothing matched" and "nothing was read" print the same
# line and exit the same way (#823: a run of zero tests printed `ok` and exited 0).
#
#   1. DID THE ENUMERATION LOSE ANYTHING? Any error at all means this listing is a subset of an
#      unknown size, and a subset presented as an inventory is worse than no inventory.
#   2. DID IT SEE ANYTHING? A root that yields zero directories is not a root with no targets --
#      it is a path that is wrong, unreadable, or not there. The denominator is printed beside the
#      numerator so `0 of 257` and `0 of 0` cannot be read as the same answer.
if ($enumerationErrors -and $enumerationErrors.Count -gt 0) {
    Write-Host "[target-inventory] HARNESS-BROKE: $($enumerationErrors.Count) error(s) while reading $Root; this listing is a subset of unknown size." -ForegroundColor Magenta
    foreach ($enumerationError in $enumerationErrors | Select-Object -First 5) {
        Write-Host "    $($enumerationError.Exception.Message)" -ForegroundColor Magenta
    }
    exit 2
}
if ($allDirectories.Count -eq 0) {
    Write-Host "[target-inventory] HARNESS-BROKE: $Root yielded no directories at all. That is a bad path, not an empty disk." -ForegroundColor Magenta
    exit 2
}

$now = Get-Date
$now = Get-Date
$rows = foreach ($dir in $found) {
    $age = [int]([math]::Floor(($now - $dir.LastWriteTime).TotalDays))
    $size = $null
    if ($WithSize) {
        $bytes = (Get-ChildItem -LiteralPath $dir.FullName -File -Recurse -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
        $size = if ($null -eq $bytes) { 0 } else { [math]::Round($bytes / 1GB, 2) }
    }
    [pscustomobject]@{
        Path        = $dir.FullName
        LastWrite   = $dir.LastWriteTime.ToString('yyyy-MM-dd HH:mm')
        AgeDays     = $age
        Attribution = Get-TargetAttribution -Name $dir.Name
        SizeGB      = $size
    }
}
$rows = @($rows | Sort-Object -Property AgeDays -Descending)

$drive = Get-PSDrive -Name ($Root.Substring(0, 1)) -ErrorAction SilentlyContinue
if ($drive) {
    $freeGb = [math]::Round($drive.Free / 1GB, 1)
    $totalGb = [math]::Round(($drive.Free + $drive.Used) / 1GB, 1)
    Write-Host "[target-inventory] $Root  free ${freeGb} GB of ${totalGb} GB"
}
Write-Host "[target-inventory] enumerated in ${enumerateMs} ms; sizes: $(if ($WithSize) { 'computed' } else { 'NOT computed (pass -WithSize; expect minutes to hours)' })"
Write-Host ''

$columns = if ($WithSize) { 'AgeDays', 'LastWrite', 'SizeGB', 'Attribution', 'Path' }
           else { 'AgeDays', 'LastWrite', 'Attribution', 'Path' }
$rows | Format-Table -AutoSize -Property $columns | Out-String -Width 200 | Write-Host

# THE DENOMINATOR IS THE OTHER NUMBER, not a second count of the same thing. `$rows` is built
# one-for-one from `$found`, so `listed vs found` compares a list with itself and can never
# disagree -- it was a guard that could not fire (J, #850). What a reader needs is how many
# directories were READ to produce the matches, so `0 of 257` and `0 of 0` say different things.
Write-Host "[target-inventory] $($rows.Count) matched of $($allDirectories.Count) directories read under $Root"
$unattributed = @($rows | Where-Object { $_.Attribution -eq 'unattributed' }).Count
Write-Host "[target-inventory] $unattributed of $($rows.Count) unattributed by name"
Write-Host '[target-inventory] read-only: nothing was removed. Removal is by NAME, and never on a target whose process chain is alive.'
