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

    EXIT CODES: 3 a `-Once` run looped past the size of its own queue (the skip list is not
    excluding a stalled entry, and spinning is worse than saying so), 0 the loop ended as asked (iteration ceiling reached, or the queue drained with
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
    param(
        [Parameter(Mandatory)] [string] $File,
        [string[]] $Arguments = @(),
        # Merge stderr into Output. OFF by default and deliberately so: a tool's progress chatter on
        # stderr would otherwise be parsed as its answer by every caller that reads Output[0].
        #
        # It is ON for the one call whose FAILURE TEXT is the answer -- `git worktree add`, where
        # the reason lives entirely on stderr and discarding it is what made three different causes
        # read as one sentence (#902). Under Windows PowerShell 5.1 a redirected native stderr line
        # arrives as an ErrorRecord rather than a string; `$ErrorActionPreference` is already
        # `Continue` here so it does not terminate, and the records are flattened to text below so
        # a caller cannot tell the two streams apart by accident.
        [switch] $CaptureError
    )
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = if ($CaptureError) { & $File @Arguments 2>&1 } else { & $File @Arguments 2>$null }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ Code = $code; Output = @(@($out) | ForEach-Object { [string] $_ }) }
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
    # `state` costs nothing here -- it is the same call -- and it is the difference between a
    # queue that spends a slot on a merged pull request and one that does not (#902). The head
    # comparison below cannot catch that case: a merged branch still exists and still points at the
    # head the lane enqueued, so the entry looks perfectly current.
    $view = & $Invoker 'gh' @('pr', 'view', "$PullRequest", '--json', 'headRefName,headRefOid,state')
    if (-not $view -or $view.Code -ne 0 -or $view.Output.Count -eq 0) { return $null }
    $parsed = $null
    try { $parsed = (($view.Output -join "`n")) | ConvertFrom-Json } catch { return $null }
    if (-not $parsed) { return $null }
    $branch = [string] $parsed.headRefName
    $resolved = [string] $parsed.headRefOid
    if ([string]::IsNullOrWhiteSpace($branch) -or [string]::IsNullOrWhiteSpace($resolved)) { return $null }
    # ABSENT rather than empty when the server did not say: an unknown state must not read as OPEN,
    # because the caller's decision on it is "spend a gate".
    $state = if ($parsed | Get-Member -Name 'state' -MemberType NoteProperty) { [string] $parsed.state } else { '' }
    return [pscustomobject]@{ Branch = $branch; Head = $resolved; State = $state }
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
    param(
        [string] $Directory,
        # Entries this iteration has already tried and could not move (#902). Without it the
        # ordering below hands back the same file every time: a blocked entry keeps its creation
        # time, so it stays the oldest, and everything behind it waits on a lane that is not
        # coming. Measured 2026-09-06: the same entry picked on two invocations back to back with
        # eight others queued behind it.
        [string[]] $Exclude = @()
    )
    $files = @(Get-ChildItem -LiteralPath $Directory -Filter '*.json' -File -ErrorAction SilentlyContinue |
        Where-Object { $Exclude -notcontains $_.Name })
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
# #902: the liveness reader the gate uses, so the runner's answer about a lock is the gate's answer.
# Beside this file when it runs from ci/; from the repository it probes when it runs as a COPY
# elsewhere (the suite's sabotage cells WRITE a mutated copy of this file into a scratch directory
# -- `WriteAllText`, not `Copy-Item` -- and run it from there), so a relocated runner keeps the
# same answer instead of dying on a missing dot-source.
# DOT-SOURCED AT SCRIPT SCOPE, never inside a function: a function that dot-sources defines the
# helpers in its own scope and they vanish when it returns (measured: the first draft did exactly
# that, and `Get-SlotHolderPairFromContent` was unknown at the call site). This only RESOLVES the
# path; the dot-source is the caller's, at top level.
function Resolve-SlotLockReaderPath {
    param([AllowNull()] [AllowEmptyString()] [string] $RepositoryRoot)
    $beside = Join-Path $PSScriptRoot 'slot-lock.ps1'
    if (Test-Path -LiteralPath $beside) { return $beside }
    if (-not [string]::IsNullOrWhiteSpace($RepositoryRoot)) {
        $inRepo = Join-Path $RepositoryRoot 'ci/slot-lock.ps1'
        if (Test-Path -LiteralPath $inRepo) { return $inRepo }
    }
    throw "ci/slot-lock.ps1 was found neither beside this runner ($PSScriptRoot) nor in the repository ($RepositoryRoot); the liveness reader is required"
}

function Get-SlotHolder {
    param([string] $Path)
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try { return (Get-Content -LiteralPath $Path -Raw).Trim() } catch { return $null }
}

# #902: A LOCK WHOSE HOLDER IS DEAD IS NOT A REASON TO WAIT. On 2026-09-08 the HDD gate for #979
# died at ~04:12Z without its `finally`, its pair stayed in `D:/graphhelm-slot/SLOT.lock`, and the
# relaunched runner sat on "the HDD slot lock is held" for good -- `Get-SlotHolder` reads CONTENT
# and never asks whether the process still exists. The gate's own `Enter-GateSlot` reclaims exactly
# that pair (`ci/slot-lock.ps1`: a pair the OS says is dead), but the gate is never launched while
# this loop waits, so the reclaim that would have freed the slot could not run: a deadlock between
# a reader that defers to the exclusion and an exclusion that needs the reader to launch it.
#
# The question is the same one the gate asks, through the same function: `Test-SlotHolderLiveness`
# over the `holder: pid=… start=…` pair. `dead` proceeds -- the gate is the exclusion and reclaims
# it; `live` waits; `indeterminate` waits too, because a pair this cannot judge is one the gate
# will not reclaim either, and preparing a bench for a gate that will refuse is the cost this
# branch exists to avoid. Still ADVISORY: nothing here writes the lock.
function Get-SlotHolderLiveness {
    param([AllowNull()] [AllowEmptyString()] [string] $Content)
    $pair = Get-SlotHolderPairFromContent -Content ([string]$Content)
    return Test-SlotHolderLiveness -HolderPid ([string]$pair.pid) -HolderStartUtc ([string]$pair.startUtc)
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
        return 'dropped'
    }

    # AND `pr` IS VALIDATED FOR THE SAME REASON `head` IS -- which became true only when this file
    # started DELETING things (#943, found by ISSUES 2 reviewing the change that added the delete).
    #
    # `$pr` is interpolated straight into a path that is then removed recursively and forcibly:
    #
    #     $target = Join-Path $TargetRoot ("pr{0}" -f $pr)
    #     Remove-Item -LiteralPath $target -Recurse -Force
    #
    # Measured on this machine, because `-LiteralPath` sounds like it forbids traversal and DOES NOT:
    #
    #     pr = '\..\..\..'    Join-Path -> D:\runner-targets\hdd\pr\..\..\..   GetFullPath -> D:\
    #     Remove-Item -LiteralPath <that> -Recurse -Force -WhatIf
    #         -> "Performing the operation "Remove Directory" on target "<the resolved parent>"
    #
    # The whole drive, unattended, on the machine holding every lane's bench and every gate target.
    #
    # `Resolve-PullRequestBranch` already returns early for a non-numeric value -- verified, `gh pr
    # view "../../.."` exits 1 -- so the hazard is not reachable today. THAT IS INCIDENTAL, NOT
    # STRUCTURAL: it makes the safety of a recursive force-delete rest on the error behaviour of an
    # unrelated network call, three checks away, and a reorder for any other reason removes it
    # silently. The refusal belongs here, beside the one for `head`, where a reader looks for it.
    #
    # This also restores the claim the comment below makes about the queue being safe to leave
    # world-writable. That sentence was written when the worst a poisoned entry could do was waste a
    # build; adding the delete made it false, and this makes it true again.
    if ("$pr" -notmatch '^[0-9]+$') {
        Write-Note "entry $($Candidate.File.Name): pr is not a number; dropping"
        Set-EntryStatus -EntryPath $entryPath -State 'refused: malformed pr'
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
        return 'stalled'
    }
    $branch = $resolvedPr.Branch
    $serverHead = $resolvedPr.Head

    # A MERGED PULL REQUEST IS NOT A HEAD THAT MOVED, and the check below cannot see it (#902).
    # Its branch still exists and still points at the enqueued head, so the entry reads as current:
    # it is picked, benched, gated, and a manifest is published onto a branch nobody will merge.
    # Measured on 2026-09-06: #935 and #937 merged at 16:27Z and 16:23Z and their entries sat at the
    # head of the queue afterwards, ahead of four open pull requests.
    #
    # CLOSED is dropped for the same reason and OPEN is the only state that spends a slot. An
    # UNKNOWN state -- the server did not say, or said something this build has not heard of -- is
    # deliberately NOT dropped: a queue that discards entries it cannot classify loses work
    # silently, which is a worse failure than spending one gate on a stale one.
    if ($resolvedPr.State -eq 'MERGED' -or $resolvedPr.State -eq 'CLOSED') {
        Write-Note "entry $($Candidate.File.Name): pull request $pr is $($resolvedPr.State); dropping"
        Set-EntryStatus -EntryPath $entryPath -State "dropped: pull request is $($resolvedPr.State)"
        Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
        return 'dropped'
    }

    if ($serverHead -ne $head) {
        Write-Note "entry $($Candidate.File.Name): head moved on the server ($($serverHead.Substring(0,8)) != $($head.Substring(0,8))); dropping"
        Set-EntryStatus -EntryPath $entryPath -State "dropped: head moved to $($serverHead.Substring(0,8))"
        Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
        return 'dropped'
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

    # #943: EVERY RUN GETS A COLD TARGET, and this runner is the one place that can guarantee it.
    #
    # `$target` is `pr<N>` -- stable across every run of the same pull request, and removed by
    # nothing. So the second gate on any PR built into the first one's artefacts, and a warm target
    # reddens `workspace tests` on the gate's OWN canary: the canary rewrites
    # `tools/ci-canary/src/nonce.rs` as the FIRST stage, `graphhelm.exe` does not depend on
    # ci-canary so nothing forces it to relink, and on a warm target the binary still on disk is the
    # PREVIOUS run's. That is a source newer than the binary, which the staleness instrument
    # correctly refuses. On a cold target the binary does not exist yet, so the window never opens.
    #
    # Four lanes reached "use a fresh target for the second run" independently and left the evidence
    # in their directory names -- `issues1-920b-targets`, `g-220b-targets`, `c-753-target2` -- with
    # no note saying why, so each rediscovery cost a wasted gate. A hand-run can rename its target.
    # THE RUNNER CANNOT: without this it hits the bug on every re-run, deterministically, and the
    # only symptom is a red that is not about the tree.
    #
    # REPORTED, NEVER SILENT, AND THE RUN CONTINUES EITHER WAY. A removal that cannot finish leaves
    # a target warmer than nothing, and the honest thing is to say so and let the gate speak: a red
    # carrying this note in its log is diagnosable, whereas failing the entry here would trade a
    # wasted run for no run at all.
    if (Test-Path -LiteralPath $target) {
        Write-Note "entry $($Candidate.File.Name): removing the previous run's target $target (#943: a re-run must be cold)"
        Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $target) {
            Write-Note "entry $($Candidate.File.Name): WARNING $target could not be fully removed; this run may redden on the canary staleness cell (#943)"
        }
    }

    # THE BENCH IS THE PULL REQUEST'S REAL BRANCH. Not an alias: the manifest commit is written to
    # whatever branch the bench holds, so an alias lands the record on a branch nobody merges.
    Set-EntryStatus -EntryPath $entryPath -State 'preparing bench'
    $null = Invoke-External 'git' @('-C', $repoRoot, 'fetch', '-q', 'origin', "$branch")
    if (Test-Path -LiteralPath $bench) {
        $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench)
    }
    $add = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'add', '-B', $branch, $bench, $head) -CaptureError
    if ($add.Code -ne 0) {
        # SAY WHICH OF THE THREE, because only one of them has an action attached (#902). A disk
        # failure, a head this repository does not have, and a branch another worktree holds all
        # produced the same sentence, and the third is the ordinary case: a lane's own worktree
        # holds its branch a second after it opens the pull request, and `git worktree add -B`
        # refuses while it does. git's own stderr already names the branch and the worktree, so
        # passing it through turns a dead end into an instruction.
        $why = (@($add.Output) | Where-Object { $_ } | Select-Object -First 2) -join ' | '
        if ([string]::IsNullOrWhiteSpace($why)) { $why = "git exited $($add.Code) and said nothing" }
        Write-Note "entry $($Candidate.File.Name): the bench could not be prepared -- $why"
        Set-EntryStatus -EntryPath $entryPath -State "waiting: bench could not be prepared -- $why"
        return 'stalled'
    }
    $symbolic = Invoke-External 'git' @('-C', $bench, 'symbolic-ref', '--quiet', 'HEAD')
    if ($symbolic.Code -ne 0 -or $symbolic.Output.Count -eq 0) {
        # A DETACHED BENCH RUNS EVERYTHING AND PUBLISHES NOTHING. Refuse before spending the run,
        # not after: the information is here, thirty minutes before the publication step needs it.
        Write-Note 'the bench is detached; refusing to spend a run that cannot publish'
        Set-EntryStatus -EntryPath $entryPath -State 'refused: detached bench'
        return 'stalled'
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
    # #903: DERIVE THE SCOPE, AND FALL TO FULL ON ANY DOUBT. The machinery landed with #928 and
    # nothing called it: every manifest since says `FULL: no scope selection was given`, which is
    # the correct default and also the whole feature not running. This is the caller.
    #
    # The selection is written next to the run's other artefacts, OUTSIDE the bench: a file inside
    # it would dirty the tree, `dirtyDiffHash` would go non-null and the gate would refuse to
    # publish its manifest -- a green run ending `status: RED, manifest not published`.
    #
    # EVERY failure here is silence, not a refusal: if the selector cannot be run, or exits
    # non-zero, or writes nothing, the gate is launched with NO `-ScopeSelection` and runs FULL.
    # That is the same direction `Read-ScopeSelection` already takes for a selection it cannot
    # read, and it is the only safe direction: a scoped run that narrowed on a bad derivation
    # would run fewer stages and report the same green.
    $scopeArgument = ''
    $scopeFile = Join-Path $StateDirectory ("$pr-$runId.scope.json")
    try {
        $mergeBase = Invoke-External 'git' @('-C', $bench, 'merge-base', 'origin/main', $resolvedPr.Head)
        if ($mergeBase.Code -eq 0 -and $mergeBase.Output.Count -gt 0) {
            $selection = Invoke-External 'powershell' @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-File', (Join-Path $bench 'ci/select-scope.ps1'),
                '-MergeBase', ([string]$mergeBase.Output[0]).Trim(),
                '-Head', $resolvedPr.Head, '-RepoRoot', $bench)
            if ($selection.Code -eq 0 -and $selection.Output.Count -gt 0) {
                [System.IO.File]::WriteAllText($scopeFile, (($selection.Output | ForEach-Object { [string]$_ }) -join "`n"))
                $scopeArgument = " -ScopeSelection '$scopeFile'"
                Write-Note "entry $($Candidate.File.Name): scope derived -> $scopeFile"
            } else {
                Write-Note "entry $($Candidate.File.Name): scope derivation exited $($selection.Code); running FULL"
            }
        } else {
            Write-Note "entry $($Candidate.File.Name): no merge-base for $($resolvedPr.Head); running FULL"
        }
    } catch {
        Write-Note "entry $($Candidate.File.Name): scope derivation failed ($($_.Exception.Message)); running FULL"
    }

    $inner = "Set-Location '$bench'; " +
        "`$env:CARGO_TARGET_DIR='$target'; " +
        "`$env:GRAPHHELM_SLOT_LOCK_PATH='$slotLock'; " +
        "& ./ci/gate.ps1$scopeArgument *> '$logFile'; " +
        "`$LASTEXITCODE | Set-Content '$rcFile'"
    # -NonInteractive here is the worst case of #925, not merely another instance of it: this child
    # runs in a HIDDEN window, so a prompt from a missing `[Parameter(Mandatory)]` in ci/gate.ps1 --
    # which has seventeen of them -- would be waiting on a console no operator can see or answer. The
    # watchdog below would then find a flat log and, correctly by its own rule, only record a
    # suspicion after twenty minutes. The selector call above already carried this flag; the run it
    # launches did not.
    $proc = Start-Process powershell -ArgumentList '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', $inner -PassThru -WindowStyle Hidden
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
    return 'built'
}

# ---------------------------------------------------------------------------------------------

$probe = Invoke-External 'git' @('rev-parse', '--show-toplevel')
if ($probe.Code -ne 0 -or $probe.Output.Count -eq 0) {
    Write-Note 'not inside a git repository; the runner has nothing to build from'
    exit 2
}
$repoRoot = ([string]$probe.Output[0]).Trim()
. (Resolve-SlotLockReaderPath -RepositoryRoot $repoRoot)

if (-not (Test-Path -LiteralPath $StateDirectory)) { $null = New-Item -ItemType Directory -Path $StateDirectory -Force }

Write-Note "repository $repoRoot; queue $QueueDirectory; slot lock $slotLock (claimed by the gate, not by this)"

$iterations = 0
# A `-Once` RUN CANNOT LOOP MORE TIMES THAN THE QUEUE HAS ENTRIES, PLUS ONE.
#
# Every iteration under `-Once` must either finish the work (build or drop, which break) or skip a
# stalled entry, and a skipped entry is excluded from the next selection -- so the loop is bounded
# by the queue. If it is not, the exclusion is not working and the runner spins forever at
# `$PollSeconds` a turn.
#
# THE FAILURE MODE THIS REPLACES WAS A HANG, and it was found by X reviewing #951: they removed
# `-Exclude $stalled` from the selection -- the correct sabotage -- and the suite did not go red, it
# stopped returning. Two of those ran for two hours. A hang is the worst outcome a guard can have:
# a red says the fix is gone, a hang says nothing and looks like work in progress for as long as
# nobody looks. This turns that into exit 3 with a sentence.
$onceCeiling = 0
# Entries this pass has already found unmovable. Cleared whenever the queue is exhausted, because
# what blocks an entry is somebody else's next action rather than a property of the entry.
$stalled = @()
try {
    while ($true) {
        $iterations++
        if ($MaxIterations -gt 0 -and $iterations -gt $MaxIterations) {
            Write-Note "iteration ceiling $MaxIterations reached"
            break
        }
        if ($Once) {
            if ($onceCeiling -le 0) {
                $onceCeiling = 1 + @(Get-ChildItem -LiteralPath $QueueDirectory -Filter '*.json' -File -ErrorAction SilentlyContinue).Count
            }
            if ($iterations -gt $onceCeiling) {
                # NOT a break. Breaking would report success for a run that made no progress and
                # could not say why -- the same silence the exit code exists to replace.
                Write-Note ("a single-entry run took $iterations turns over a queue of " +
                    "$($onceCeiling - 1); an entry is being re-picked, so the skip list is not " +
                    'excluding it. Refusing to spin.')
                exit 3
            }
        }

        $candidate = $null
        if (Test-Path -LiteralPath $QueueDirectory) {
            $candidate = Get-NextEntry -Directory $QueueDirectory -Exclude $stalled
        }
        if (-not $candidate) {
            if ($Once) {
                if ($stalled.Count -gt 0) {
                    Write-Note "every remaining entry is stalled ($($stalled -join ', ')); nothing this runner can move"
                } else {
                    Write-Note 'queue empty'
                }
                break
            }
            # A STALL IS NOT PERMANENT, so the skip list is cleared before waiting. The thing that
            # blocks an entry -- a lane holding its branch, a pull request the server would not
            # resolve -- is somebody else's next action, and a runner that remembered the stall
            # forever would need restarting to notice it had ended.
            $stalled = @()
            Start-Sleep -Seconds $PollSeconds
            continue
        }

        # ADVISORY ONLY. If the lock is held, waiting is cheaper than preparing a bench for a gate
        # that will refuse -- but this is not the exclusion. The exclusion is `Enter-GateSlot` inside
        # the gate, and it runs whether this read said anything or not. A reader that treated its own
        # answer as permission would be the second claimant all over again.
        $holder = Get-SlotHolder -Path $slotLock
        if ($holder) {
            $liveness = Get-SlotHolderLiveness -Content $holder
            # ORDINAL (C on #1022): -eq is culture-sensitive, and the unsafe direction of a folded compare
            # is the one that reads a live holder as dead and proceeds past it.
            if ([string]::Equals($liveness, 'dead', [System.StringComparison]::Ordinal)) {
                # #902: the pair in the lock names a process the OS no longer has. The gate reclaims
                # such a pair on entry; this loop only had to stop treating it as a holder.
                Write-Note "the $Slot slot lock names a dead holder ($(([string]$holder -split "`n" | Select-Object -Last 1).Trim())); the gate reclaims it, preparing a bench"
            } else {
                Write-Note "the $Slot slot lock is held ($liveness); waiting rather than preparing a bench"
                if ($Once) { exit 1 }
                Start-Sleep -Seconds $PollSeconds
                continue
            }
        }

        $outcome = Invoke-OneEntry -Candidate $candidate

        # A STALLED ENTRY IS SKIPPED, NOT WAITED ON (#902). `-Once` means one gate, not one
        # attempt: an entry nobody can bench must not consume the invocation that a buildable one
        # behind it was waiting for. `built` and `dropped` both count as having done the work --
        # the second because the entry is gone and the queue moved.
        if ($outcome -eq 'stalled') {
            $stalled += $candidate.File.Name
            continue
        }

        if ($Once) { break }
    }
} finally {
    # NOTHING TO RELEASE. This process never claimed the slot -- the gate did, and the gate releases
    # it. A `finally` that removed `SLOT.lock` here would delete another run's claim whenever this
    # loop exited while a gate it launched was still going, which is precisely the crash the lock is
    # there to survive.
}

exit 0
