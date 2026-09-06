<#
.SYNOPSIS
    The long-lived gate runner: it owns one slot, drains the queue, and publishes manifests (#902).

.DESCRIPTION
    ONE RUNNER OWNS A SLOT. Lanes enqueue with `ci/gate-queue.ps1` and go back to their next issue;
    nobody else launches a gate. That is the whole of the change -- what the gate proves is
    untouched (deliverable 2 of #901), and so is the shard store (deliverable 3).

    WHY IT LIVES OUTSIDE A SESSION. Measured 2026-09-05 ~04:11Z: a fleet recycle killed every gate on
    the machine, including the ones launched detached with `Start-Process`. Detached from a SESSION
    is not detached from the APP. Registered as a scheduled task, this process is a child of the
    task host and survives the app entirely -- which is the only way a thirty-minute run can outlive
    a five-minute conversation.

    WHAT IT REFUSES TO TRUST. The queue file is untrusted input: it is written by another process and
    carries no evidence of who wrote it. Every field is validated again here, and the BRANCH NAME is
    never read from it at all -- the runner resolves the branch from the pull request number through
    `gh pr view` and then requires the resolved head to equal the head in the entry. An entry naming
    a head the server no longer has is dropped, not built: between enqueue and drain a branch can be
    force-pushed, and the sha a lane meant can stop existing while still existing in some local
    object database.

    WHY THE BENCH IS ON A BRANCH. `ci/gate.ps1` reads `git symbolic-ref --quiet HEAD` ONCE at startup
    (`:1819`) and publishes through `git update-ref ... $branchRef` (`:1124`) using that capture. A
    DETACHED bench therefore runs every stage and publishes nothing -- the manifest is written inside
    the worktree and lands nowhere. Measured: a main-gate run that way ended `status: RED` with
    `failed stages: []`, every stage green and no record. Switching branch mid-run does not help
    either; the capture already happened.

    AND WHY THE PID/LOG/RC FILES LIVE OUTSIDE THE WORKTREE. Inside it they dirty the tree,
    `dirtyDiffHash` goes non-null, and the gate correctly refuses to commit a manifest into a tree
    holding changes it did not make -- a run with every stage green ends RED for "run manifest not
    published". Measured twice on 2026-09-05, by two lanes, on the same day.

    PROOF OF LIFE IS THE LOG GROWING. Not a process count: the counting pattern misses a wrapper
    launch (a command line that never says `gate.ps1`) and over-counts its own reader, and two lanes
    measuring zero in the same minute both launched. The slot CLAIM is the handshake; the count is
    only a check. And "wedged" is never a thirty-second reading: a healthy gate sits flat for tens of
    seconds between stages with no compiler running (measured: 45 s, `desc=1`).

    EXIT CODES: 0 the loop ended as asked (iteration ceiling reached, or the queue drained with
    -Once), 1 the slot could not be claimed, 2 the runner could not read or write its own state.
    Callers act only on 0.

.PARAMETER Slot
    SSD or HDD. Decides the claim file and the default target root; the two slots are independent and
    a second runner may hold the other one.

.PARAMETER Once
    Drain at most one entry and return. Used by the tests and by a human checking the wiring.

.PARAMETER MaxIterations
    A CEILING, not a schedule. An outward loop needs a ceiling more than it needs a correct exit
    condition: without one, a runner whose queue never empties never returns, and a hang has no
    colour. Zero means "until the queue is empty" when -Once is absent.
#>
[CmdletBinding()]
param(
    [ValidateSet('SSD', 'HDD')] [string] $Slot = 'SSD',
    [string] $SlotRoot,
    [string] $QueueDirectory,
    [string] $BenchRoot,
    [string] $TargetRoot,
    [string] $StateDirectory,
    [switch] $Once,
    [int] $MaxIterations = 0,
    [int] $PollSeconds = 20
)

$ErrorActionPreference = 'Stop'

if (-not $QueueDirectory) { $QueueDirectory = 'D:\graphhelm-slot\queue' }
if (-not $StateDirectory) { $StateDirectory = 'D:\graphhelm-slot\runner' }
if (-not $BenchRoot) { $BenchRoot = if ($Slot -eq 'SSD') { 'D:\runner-ssd' } else { 'D:\runner-hdd' } }
if (-not $TargetRoot) { $TargetRoot = if ($Slot -eq 'SSD') { 'E:\runner-targets\ssd' } else { 'D:\runner-targets\hdd' } }

# THE RUNNER DOES NOT CLAIM. It sets the per-spindle slot paths and lets `ci/gate.ps1` claim through
# its own `Enter-GateSlot` (`:1953`), which is the single arbiter since #892 and #905.
#
# The first version of this file invented a second ledger (`E:\SLOT-SSD.claim`) and claimed it here.
# That is worse than a wrong path: TWO CLAIMANTS WITH TWO LOCKS DO NOT EXCLUDE EACH OTHER. A runner
# holding its own file and a hand-launched gate holding `SLOT.lock` would each read a free slot and
# both build on the same spindle -- the exact double-claim #892 closed. Found in review of #911 by
# the G lane, against a branch cut before #892 landed.
#
# BOTH VARIABLES, PER SLOT, because two different programs read two different names for the same
# fact: `ci/gate.ps1:387` reads `GRAPHHELM_SLOT_DIR`, and `.factory/tools/slot-claim.sh:50` reads
# `SLOT_LOCK`. Exporting only one was measured on #871: a run on the SSD claimed the HDD's lock as
# well, and the HDD queue stood still for 27 minutes.
# `-SlotRoot` exists so a TEST can point this at a throwaway directory. It is not an operational
# knob: every real invocation leaves it unset and gets the per-spindle path below, because two
# runners on one spindle with different roots do not exclude each other -- the same defect the
# comment above records from #871, one level up.
$slotRoot = if ($SlotRoot) { $SlotRoot } else { if ($Slot -eq 'SSD') { 'E:/graphhelm-slot' } else { 'D:/graphhelm-slot' } }
$slotLock = "$slotRoot/SLOT.lock"
$repoRoot = $null

function Write-Note {
    param([Parameter(Mandatory)] [string] $Message)
    $stamp = (Get-Date).ToUniversalTime().ToString('HH:mm:ss')
    Write-Host "[runner $Slot $stamp] $Message"
}

# NOTHING THAT CAN STOP THE PIPELINE MAY SIT BETWEEN A NATIVE COMMAND AND THE READ OF ITS EXIT CODE
# (#762). Under 5.1 a native command's stderr returns wrapped in a NativeCommandError, and under
# `$ErrorActionPreference = 'Stop'` that record is terminating -- so a `git` or `gh` that refuses
# would kill this runner instead of being observed. Every external call goes through here.
function Invoke-External {
    param([Parameter(Mandatory)] [string] $File, [string[]] $Arguments = @())
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $File @Arguments 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ Code = $code; Output = @($out) }
}

# THE `--jq` ARGUMENT CANNOT CARRY A QUOTED SPACE THROUGH WINDOWS POWERSHELL 5.1, AND THAT IS WHY
# THIS ASKS FOR JSON AND PARSES IT HERE. The first version of this function passed
# `--jq '.headRefName + " " + .headRefOid'`. PowerShell re-parses a native command's arguments and
# strips the inner double quotes, so `gh` received `.headRefName`, `+`, ` `, `+`, `.headRefOid` as
# five arguments and answered `accepts at most 1 arg(s), received 2` with exit 1. Measured on this
# machine, in the array-splat form this file uses, not only in a hand-typed line.
#
# The consequence was total and silent: EVERY entry read back "gh could not resolve pull request N;
# leaving it queued", so the queue never drained a single gate from the day it merged. The refusal
# text described the world -- a pull request that cannot be resolved is a real state -- which is
# exactly the shape #889 names, a path failure wearing a domain failure's words.
#
# `--json` alone carries no spaces and survives. The parse moves into PowerShell, where the quoting
# is ours rather than the argument passer's.
function Resolve-PullRequestBranch {
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [scriptblock] $Invoker
    )
    if (-not $Invoker) { $Invoker = { param($file, $arguments) Invoke-External $file $arguments } }
    $view = & $Invoker 'gh' @('pr', 'view', "$PullRequest", '--json', 'headRefName,headRefOid')
    if (-not $view -or $view.Code -ne 0 -or $view.Output.Count -eq 0) { return $null }
    $parsed = $null
    try { $parsed = (($view.Output -join "`n")) | ConvertFrom-Json } catch { return $null }
    if (-not $parsed) { return $null }
    $branch = [string] $parsed.headRefName
    $resolved = [string] $parsed.headRefOid
    if ([string]::IsNullOrWhiteSpace($branch) -or [string]::IsNullOrWhiteSpace($resolved)) { return $null }
    return [pscustomobject]@{ Branch = $branch; Head = $resolved }
}

function Test-HeadSha {
    param([string] $Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    return ($Value -cmatch '^[0-9a-f]{40}$')
}

function Set-EntryStatus {
    param([Parameter(Mandatory)] [string] $EntryPath, [Parameter(Mandatory)] [string] $State)
    $statusFile = [System.IO.Path]::ChangeExtension($EntryPath, '.status')
    $line = "{0} {1}" -f (Get-Date).ToUniversalTime().ToString('o'), $State
    Set-Content -LiteralPath $statusFile -Value $line -NoNewline
    Write-Note "status: $State"
}

# READINESS ORDER, NOT ARRIVAL ORDER. A pull request with two passes is the one whose gate result
# someone is waiting on; one with none may still be edited. Age breaks ties, so nothing starves.
function Get-NextEntry {
    param([string] $Directory)
    $files = @(Get-ChildItem -LiteralPath $Directory -Filter '*.json' -File -ErrorAction SilentlyContinue)
    if ($files.Count -eq 0) { return $null }
    $scored = foreach ($f in $files) {
        $entry = $null
        try { $entry = (Get-Content -LiteralPath $f.FullName -Raw) | ConvertFrom-Json } catch { }
        if (-not $entry) { continue }
        $passes = 0
        $probe = Invoke-External 'gh' @('api', "repos/stabem/GraphHelm/pulls/$($entry.pr)/reviews", '--jq', 'length')
        if ($probe.Code -eq 0 -and $probe.Output.Count -gt 0) {
            $parsed = 0
            if ([int]::TryParse(([string]$probe.Output[0]).Trim(), [ref] $parsed)) { $passes = $parsed }
        }
        [pscustomobject]@{ File = $f; Entry = $entry; Passes = $passes; Age = $f.CreationTimeUtc }
    }
    $ordered = @($scored | Sort-Object -Property @{ Expression = 'Passes'; Descending = $true }, @{ Expression = 'Age'; Descending = $false })
    if ($ordered.Count -eq 0) { return $null }
    return $ordered[0]
}

# WHO HOLDS THIS SPINDLE, read from the ONE lock the gate itself uses. This is a READ, never a
# claim: `Enter-GateSlot` inside `ci/gate.ps1` is the only thing that writes `SLOT.lock`, and adding
# a second writer here is what the first version of this file got wrong.
#
# The answer is advisory. It exists so the runner can say "held, waiting" in a status file instead of
# spending a bench preparation on a gate that will refuse -- not so it can decide. The decision is
# the gate's, atomically, thirty seconds later.
function Get-SlotHolder {
    param([string] $Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try { return (Get-Content -LiteralPath $Path -Raw).Trim() } catch { return $null }
}

function Invoke-OneEntry {
    param([Parameter(Mandatory)] $Candidate)

    $entryPath = $Candidate.File.FullName
    $entry = $Candidate.Entry
    $pr = $entry.pr
    $head = [string] $entry.head

    if (-not (Test-HeadSha $head)) {
        Write-Note "entry $($Candidate.File.Name): head is not 40 lowercase hex; dropping"
        Set-EntryStatus -EntryPath $entryPath -State 'refused: malformed head'
        return
    }

    # THE BRANCH COMES FROM THE SERVER, NEVER FROM THE FILE, and the resolved head must equal the one
    # the lane enqueued. This is the check that makes the queue safe to leave world-writable: the
    # worst a poisoned entry can do is name a pull request whose real head disagrees, and that is
    # dropped rather than built.
    $resolvedPr = Resolve-PullRequestBranch -PullRequest "$pr"
    if (-not $resolvedPr) {
        Write-Note "entry $($Candidate.File.Name): gh could not resolve pull request $pr; leaving it queued"
        Set-EntryStatus -EntryPath $entryPath -State 'waiting: pull request not resolvable'
        return
    }
    $branch = $resolvedPr.Branch
    $serverHead = $resolvedPr.Head
    if ($serverHead -ne $head) {
        Write-Note "entry $($Candidate.File.Name): head moved on the server ($($serverHead.Substring(0,8)) != $($head.Substring(0,8))); dropping"
        Set-EntryStatus -EntryPath $entryPath -State "dropped: head moved to $($serverHead.Substring(0,8))"
        Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
        return
    }

    $bench = Join-Path $BenchRoot ("pr{0}" -f $pr)
    $target = Join-Path $TargetRoot ("pr{0}" -f $pr)
    $runId = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmss')
    $logFile = Join-Path $StateDirectory ("$pr-$runId.log")
    $rcFile = Join-Path $StateDirectory ("$pr-$runId.rc")
    $pidFile = Join-Path $StateDirectory ("$pr-$runId.pid")

    foreach ($d in @($StateDirectory, $BenchRoot, $TargetRoot)) {
        if (-not (Test-Path -LiteralPath $d)) { $null = New-Item -ItemType Directory -Path $d -Force }
    }

    # THE BENCH IS THE PULL REQUEST'S REAL BRANCH. Not an alias: the manifest commit is written to
    # whatever branch the bench holds, so an alias lands the record on a branch nobody merges.
    Set-EntryStatus -EntryPath $entryPath -State 'preparing bench'
    $null = Invoke-External 'git' @('-C', $repoRoot, 'fetch', '-q', 'origin', "$branch")
    if (Test-Path -LiteralPath $bench) {
        $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench)
    }
    $add = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'add', '-B', $branch, $bench, $head)
    if ($add.Code -ne 0) {
        Write-Note "entry $($Candidate.File.Name): the bench could not be prepared; leaving it queued"
        Set-EntryStatus -EntryPath $entryPath -State 'waiting: bench could not be prepared'
        return
    }
    $symbolic = Invoke-External 'git' @('-C', $bench, 'symbolic-ref', '--quiet', 'HEAD')
    if ($symbolic.Code -ne 0 -or $symbolic.Output.Count -eq 0) {
        # A DETACHED BENCH RUNS EVERYTHING AND PUBLISHES NOTHING. Refuse before spending the run,
        # not after: the information is here, thirty minutes before the publication step needs it.
        Write-Note 'the bench is detached; refusing to spend a run that cannot publish'
        Set-EntryStatus -EntryPath $entryPath -State 'refused: detached bench'
        return
    }

    Set-EntryStatus -EntryPath $entryPath -State "building on $branch"
    # ONE VARIABLE, AND IT IS THE ONE THAT NAMES THE FILE. Two review findings on #911 land on this
    # line and they have a single answer.
    #
    # The first version exported `GRAPHHELM_SLOT_DIR` and `SLOT_LOCK`. `SLOT_LOCK` is read by
    # `.factory/tools/slot-claim.sh:50` and by nothing on this path -- the runner launches
    # `ci/gate.ps1` directly, never the shell wrapper -- so it LOOKED like it was doing work it was
    # not (ISSUES 4). And `GRAPHHELM_SLOT_DIR` moves more than the lock: `ci/gate.ps1:1976` builds
    # the durable ledger as `<slot dir>/gate-runs`, while `ci/merge-proof.ps1` sweeps only
    # `D:\graphhelm-slot\gate-runs`. Pointing the slot dir at `E:` would have parked every SSD run's
    # manifest where the verifier does not look, and each would have earned "no independent record
    # corroborates it" however sound the run was (measured by M on #916: `e7b9915e3db7` -> 0 files
    # on D:, 5 on E:).
    #
    # `GRAPHHELM_SLOT_LOCK_PATH` separates the two facts, which is what it exists for
    # (`ci/slot-lock.ps1:14`: "points DIRECTLY at the lock file itself, no directory to Join-Path
    # against"). `Get-SlotLockPath` returns it when set (`:345`), and `ci/gate.ps1:2230` defaults it
    # from `Get-SlotDir` only when unset. So the LOCK becomes per-spindle and the LEDGER stays where
    # the verifier sweeps: one export, both findings.
    $inner = "Set-Location '$bench'; " +
        "`$env:CARGO_TARGET_DIR='$target'; " +
        "`$env:GRAPHHELM_SLOT_LOCK_PATH='$slotLock'; " +
        "& ./ci/gate.ps1 *> '$logFile'; " +
        "`$LASTEXITCODE | Set-Content '$rcFile'"
    $proc = Start-Process powershell -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $inner -PassThru -WindowStyle Hidden
    "$($proc.Id)" | Set-Content -LiteralPath $pidFile

    # PROOF OF LIFE IS THE LOG GROWING, and "wedged" is never a short reading. A healthy gate sits
    # flat between stages -- 45 s measured, with one descendant and no compiler.
    $lastSize = -1
    $flatSince = Get-Date
    while (-not (Test-Path -LiteralPath $rcFile)) {
        Start-Sleep -Seconds $PollSeconds
        $size = if (Test-Path -LiteralPath $logFile) { (Get-Item -LiteralPath $logFile).Length } else { 0 }
        if ($size -ne $lastSize) {
            $lastSize = $size
            $flatSince = Get-Date
            Set-EntryStatus -EntryPath $entryPath -State ("building on {0}: log {1} bytes" -f $branch, $size)
        } elseif (((Get-Date) - $flatSince).TotalMinutes -ge 20) {
            Write-Note 'the log has not grown in twenty minutes; recording the suspicion, not acting on it'
            Set-EntryStatus -EntryPath $entryPath -State 'suspect: log flat 20 min'
            $flatSince = Get-Date
        }
        if (-not (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)) { break }
    }
    Start-Sleep -Seconds 2
    $rc = if (Test-Path -LiteralPath $rcFile) { (Get-Content -LiteralPath $rcFile -Raw).Trim() } else { 'unknown' }

    # PUSH THE MANIFEST THE INSTANT IT EXISTS. What survives an app restart is what is on the server.
    $push = Invoke-External 'git' @('-C', $bench, 'push', 'origin', "HEAD:$branch")
    $pushed = ($push.Code -eq 0)

    Set-EntryStatus -EntryPath $entryPath -State ("finished rc=$rc pushed=$pushed log=$logFile")
    Write-Note "pr $pr finished rc=$rc pushed=$pushed"
    Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------------------------

$probe = Invoke-External 'git' @('rev-parse', '--show-toplevel')
if ($probe.Code -ne 0 -or $probe.Output.Count -eq 0) {
    Write-Note 'not inside a git repository; the runner has nothing to build from'
    exit 2
}
$repoRoot = ([string]$probe.Output[0]).Trim()

if (-not (Test-Path -LiteralPath $StateDirectory)) { $null = New-Item -ItemType Directory -Path $StateDirectory -Force }

Write-Note "repository $repoRoot; queue $QueueDirectory; slot lock $slotLock (claimed by the gate, not by this)"

$iterations = 0
try {
    while ($true) {
        $iterations++
        if ($MaxIterations -gt 0 -and $iterations -gt $MaxIterations) {
            Write-Note "iteration ceiling $MaxIterations reached"
            break
        }

        $candidate = $null
        if (Test-Path -LiteralPath $QueueDirectory) { $candidate = Get-NextEntry -Directory $QueueDirectory }
        if (-not $candidate) {
            if ($Once) { Write-Note 'queue empty'; break }
            Start-Sleep -Seconds $PollSeconds
            continue
        }

        # ADVISORY ONLY. If the lock is held, waiting is cheaper than preparing a bench for a gate
        # that will refuse -- but this is not the exclusion. The exclusion is `Enter-GateSlot` inside
        # the gate, and it runs whether this read said anything or not. A reader that treated its own
        # answer as permission would be the second claimant all over again.
        $holder = Get-SlotHolder -Path $slotLock
        if ($holder) {
            Write-Note "the $Slot slot lock is held; waiting rather than preparing a bench"
            if ($Once) { exit 1 }
            Start-Sleep -Seconds $PollSeconds
            continue
        }

        Invoke-OneEntry -Candidate $candidate

        if ($Once) { break }
    }
} finally {
    # NOTHING TO RELEASE. This process never claimed the slot -- the gate did, and the gate releases
    # it. A `finally` that removed `SLOT.lock` here would delete another run's claim whenever this
    # loop exited while a gate it launched was still going, which is precisely the crash the lock is
    # there to survive.
}

exit 0
