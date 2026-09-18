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
# fact: `ci/gate.ps1:1242` reads `GRAPHHELM_SLOT_DIR`, and `.factory/tools/slot-claim.sh:132` reads
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
# The pass predicate lives in its own file so the guard cells can load it directly: this script
# has top-level code and cannot be dot-sourced by a test without running a gate (#1133).
#
# IT IS A SIBLING, AND COPYING THIS FILE ALONE BREAKS IT. The test harness copies the runner into a
# temp directory to sabotage one line, and the first version of this dot-source failed there with a
# bare exit 1 -- a missing dependency wearing a runner error's clothes. Whoever copies this script
# must copy `gate-passes.ps1` beside it, and if they do not, this says so by name.
$passesModule = Join-Path $PSScriptRoot 'gate-passes.ps1'
if (-not (Test-Path -LiteralPath $passesModule)) {
    Write-Error "gate-runner requires ci/gate-passes.ps1 beside it (looked in '$PSScriptRoot'); copy the sibling or run from the repository"
    exit 2
}
. $passesModule

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
        # PASSES, NOT REVIEW OBJECTS (#1133). The line here was
        # `gh api pulls/N/reviews --jq length`, which counted objects with no filter on verdict,
        # head, author or surface -- so a score rose monotonically over a branch's life, never
        # decayed across a rebase, and an old pull request outranked a ready one permanently.
        # `Get-LivePassCount` counts distinct reviewer sessions carrying a verdict word AND this
        # entry's head, excluding the lane that enqueued it. See ci/gate-passes.ps1.
        $passes = 0
        $lane = ''
        if ($entry | Get-Member -Name 'lane' -MemberType NoteProperty) { $lane = [string] $entry.lane }
        $entryHead = ''
        if ($entry | Get-Member -Name 'head' -MemberType NoteProperty) { $entryHead = [string] $entry.head }
        if (Test-HeadSha $entryHead) {
            # A GATE RECEIPT MOVES THE HEAD WITHOUT INVALIDATING A PASS (#1133, found in review).
            # The tip a gate commits touches only `.factory/gate-runs/`, so the passes name the
            # parent. Measured on #1125: 0 at the manifest tip, 2 at its parent. Without this a
            # re-gate after a flake costs a pull request the position its reviewers earned.
            # `Get-LivePassCount` resolves the manifest-only parent itself when the argument is
            # absent, so this call site does not have to remember -- a caller who forgot was the
            # second half of the finding that added it.
            $tally = Get-LivePassCount -PullRequest "$($entry.pr)" -Head $entryHead -AuthorLane $lane
            $passes = $tally.Count
            # A ZERO FROM A FAILED READ IS NOT A ZERO FROM AN UNREVIEWED PULL REQUEST, and the old
            # scorer could not say which it was holding. Ordering still proceeds -- age breaks the
            # tie and nothing starves -- but the log names the difference.
            if (-not $tally.Measured) {
                Write-Note "pass count for #$($entry.pr) is PARTIAL: a surface did not answer; ordering with $passes"
            }
        } else {
            # An entry with no usable head cannot have its passes measured against anything. It is
            # not dropped here: the head validation downstream owns that decision and says why.
            Write-Note "pass count for #$($entry.pr) skipped: entry carries no valid head"
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

function Test-BenchIsRegistered {
    <#
    .SYNOPSIS
        Is this path a worktree THIS clone knows about?
    .DESCRIPTION
        Compared on normalised text, because the two sides disagree about spelling: `worktree list
        --porcelain` prints forward slashes, the bench path is built with `Join-Path` and carries
        backslashes, and Windows is case-insensitive. Comparing the raw strings answers "not
        registered" for every bench, which would turn the reclaim below into an unconditional
        delete -- the one outcome worse than the defect it repairs.
    #>
    param(
        [Parameter(Mandatory)] [string] $RepositoryRoot,
        [Parameter(Mandatory)] [string] $BenchPath
    )
    $listed = Invoke-External 'git' @('-C', $RepositoryRoot, 'worktree', 'list', '--porcelain')
    if ($listed.Code -ne 0) {
        # UNREADABLE IS NOT UNREGISTERED. If the question cannot be answered, the safe answer is
        # the one that deletes nothing.
        return $true
    }
    $normalise = { param($p) ($p.Replace([char]92, [char]47)).TrimEnd('/').ToLowerInvariant() }
    $wanted = & $normalise $BenchPath
    # AN EMPTY LIST THAT EXITED 0 IS ALSO AN UNANSWERABLE QUESTION, and without this counter it
    # answered "not registered" for every bench on the machine -- authorising the delete below on
    # the strength of an external tool always printing a line. `git worktree list --porcelain` does
    # print the main worktree today, so it is not reachable; that is INCIDENTAL, NOT STRUCTURAL, and
    # it is the same objection this file already makes twenty lines up about `$pr`: the safety of a
    # recursive force-delete must not rest on the output behaviour of a tool three checks away.
    $sawAnyWorktree = $false
    foreach ($line in @($listed.Output)) {
        if ($line -match '^worktree (.+)$') {
            $sawAnyWorktree = $true
            if ((& $normalise $Matches[1].Trim()) -eq $wanted) { return $true }
        }
    }
    if (-not $sawAnyWorktree) { return $true }
    return $false
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

    # A BRANCH NAME THAT BEGINS WITH A DASH IS REFUSED, NOT HANDLED. `git check-ref-format` accepts
    # `--upload-pack=nope`, and every git command below that receives the name as a word reads it as
    # an option: the fetch without `--` RAN the value as its upload-pack (measured in this suite),
    # and `worktree add` cannot create such a branch at all (its internal checkout reads it as an
    # option even behind `-B<name>` and `--`). No queued branch has ever been named this way; the
    # refusal names the reason so the entry is not left stalled behind a sentence nobody can read.
    # The terminal-state check above intentionally comes first: a CLOSED or MERGED entry is safe to
    # discard even when its historical branch name begins with a dash, and must not remain queued
    # forever behind the named refusal.
    $branch = $resolvedPr.Branch
    if ("$branch" -match '^-') {
        Write-Note "entry $($Candidate.File.Name): the branch name '$branch' begins with a dash and would be read as an option; refusing"
        Set-EntryStatus -EntryPath $entryPath -State "refused: branch name begins with a dash ($branch)"
        return 'stalled'
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
    # THE REFSPEC IS MADE TO COVER THE BRANCH, and only then is the branch fetched. A bare
    # `fetch origin <branch>` updates `refs/remotes/origin/<branch>` only when the configured refspec
    # covers it: a clone made with `--single-branch` (Codex P2 on #979) fetches the tip into
    # FETCH_HEAD and nothing else, and every feature branch stalls quietly, all at once. Naming the
    # destination in the fetch is not enough either, and neither are the two tracking keys:
    # `@{upstream}` maps `refs/heads/<branch>` through the CONFIGURED refspec and under a narrow one
    # answers "not stored as a remote-tracking branch" -- a bench that builds, a manifest that says
    # pushed: null again (measured by ISSUES 4 on #979). THE WIDENING IS THE WILDCARD, added once
    # when absent -- never a branch-specific line: that line outlives the branch, and once the branch
    # is deleted after its merge every plain `git fetch origin` in this clone dies with "couldn't
    # find remote ref" until someone repairs the config by hand (Codex on #979).
    $wildcard = '+refs/heads/*:refs/remotes/origin/*'
    $refspecs = Invoke-External 'git' @('-C', $repoRoot, 'config', '--get-all', 'remote.origin.fetch')
    if (@(@($refspecs.Output) | Where-Object { $_ -eq $wildcard }).Count -eq 0) {
        $null = Invoke-External 'git' @('-C', $repoRoot, 'config', '--add', 'remote.origin.fetch', $wildcard)
    }
    # AND THE FETCH MUST SUCCEED BEFORE ITS REF IS READ. `refs/remotes/origin/<branch>` may still hold
    # what an EARLIER run fetched; a fetch that fails now (the branch deleted between the `gh` lookup
    # and here) would leave that stale ref in place, the runner would gate the stale commit and, at
    # the end, push to recreate a branch somebody deleted (Codex on #979). Stalled, naming the fetch.
    # `--` BEFORE THE BRANCH: a branch name is allowed to begin with `--` (`git check-ref-format`
    # accepts `--upload-pack=x`), and without the separator git reads it as an option -- the fetch
    # fails and the entry stalls for a reason nobody can see (Codex on #979).
    # FULLY QUALIFIED: a bare name is resolved tags-first (gitrevisions), so a branch that shares its
    # name with a tag would fetch the tag, exit 0, and leave origin/<branch> untouched (Codex on #979).
    # `refs/heads/<branch>` names the branch and nothing else; the widened wildcard refspec above is
    # what lets git update the remote-tracking ref for an explicitly named source.
    $fetch = Invoke-External 'git' @('-C', $repoRoot, 'fetch', '-q', 'origin', '--', "refs/heads/${branch}") -CaptureError
    if ($fetch.Code -ne 0) {
        $why = (@($fetch.Output) | Where-Object { $_ } | Select-Object -First 2) -join ' | '
        if ([string]::IsNullOrWhiteSpace($why)) { $why = "git exited $($fetch.Code) and said nothing" }
        # The same prefix as the add's failure below, so the reader has ONE sentence to search for
        # and git's own words follow it either way (#951's contract); the fetch is named after it.
        Write-Note "entry $($Candidate.File.Name): the bench could not be prepared -- fetch of origin/$branch failed: $why; leaving it queued"
        Set-EntryStatus -EntryPath $entryPath -State "waiting: bench could not be prepared -- fetch of origin/$branch failed: $why"
        return 'stalled'
    }
    if (Test-Path -LiteralPath $bench) {
        # THE REMOVE'S EXIT CODE IS THE ANSWER, AND IT USED TO BE DISCARDED (#1045).
        #
        # `git worktree remove` fails when the bench is not registered IN THIS CLONE, which is the
        # ordinary state of every bench a previous runner instance created from a different repo
        # root -- registration lives in the clone that made it. With the code thrown away that
        # failure surfaced one line later as `worktree add`'s `already exists`, so the operator was
        # handed the WRONG STEP: the remove that quietly did nothing was never mentioned, and the
        # block below -- whose entire purpose is to say which of three causes fired -- named a
        # cause created two lines above it. `pr997` sat in `waiting: bench could not be prepared`
        # for three days that way, retrying every poll against something that never heals.
        #
        # THE FALLBACK ASKS `worktree list` BEFORE IT DELETES, and that guard is not ceremony: a
        # remove can also fail because something HOLDS a live bench, and deleting a bench out from
        # under a running gate is far worse than the stall this repairs. Unregistered is the only
        # case with an action attached.
        $remove = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench) -CaptureError
        if ($remove.Code -ne 0 -and (Test-Path -LiteralPath $bench)) {
            if (Test-BenchIsRegistered -RepositoryRoot $repoRoot -BenchPath $bench) {
                Write-Note "entry $($Candidate.File.Name): $bench is a REGISTERED worktree that would not remove; leaving it alone"
            }
            else {
                $why = @($remove.Output) | Where-Object { $_ } | Select-Object -First 1
                if ([string]::IsNullOrWhiteSpace($why)) { $why = "git exited $($remove.Code) and said nothing" }
                Write-Note "entry $($Candidate.File.Name): $bench is not registered by this clone -- $why; reclaiming the directory"
                Remove-Item -LiteralPath $bench -Recurse -Force -ErrorAction SilentlyContinue
                $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'prune')
            }
        }
    }
    # AND THE BRANCH MUST TRACK ORIGIN, OR THE RECORD CANNOT VOUCH. `ci/gate.ps1` answers `pushed`
    # from `@{upstream}`; `worktree add -B <branch> <bench> <sha>` creates a branch with neither
    # `branch.<b>.remote` nor `branch.<b>.merge`, so every manifest from a bench for a branch this
    # clone had never held came back `pushed: null` and merge-proof refused it (#939, #947, #967,
    # #968 from D:/orch-runner-clone, 2026-09-07). The author's own repository hid the defect:
    # there the branch pre-existed with tracking, and `-B` leaves branch config alone.
    #
    # THE TWO KEYS ARE WRITTEN, NOT INFERRED. `--track` (and the default when the start point is a
    # remote-tracking ref) decides trackability from the CONFIGURED refspec, so under a
    # `--single-branch` clone it refuses even when the ref exists ("starting point is not a
    # branch", measured by ISSUES 4 on #979). The bench therefore starts from the queued SHA --
    # always resolvable -- and the upstream is set as the two config keys `@{upstream}` reads.
    # `-B<branch>` ATTACHED, for the same reason the fetch carries `--`: a branch named
    # `--upload-pack=nope` handed to `-B` as a separate word is read as an option ("unknown option
    # `upload-pack=nope'", measured), and the fetch without `--` went further and RAN it.
    $add = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'add', "-B${branch}", $bench, $head) -CaptureError
    if ($add.Code -ne 0) {
        # SAY WHICH OF THE THREE, because only one of them has an action attached (#902). A disk
        # failure, an origin branch this repository has not fetched, and a branch another worktree holds all
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
    # THE BENCH BRANCH MUST TRACK ORIGIN, or the gate has no server to ask (#1040). `worktree add -B`
    # creates the branch with no upstream; `ci/gate.ps1` answers `pushed` by asking the server named
    # by `<branch>@{upstream}`, so without this the runner's receipts all carry `pushed: null` and
    # `ci/merge-proof.ps1` refuses them. A bench that cannot be wired is refused here, before the run
    # is spent, the way a detached bench is refused below.
    #
    # THE TWO KEYS ARE WRITTEN FIRST, AND `--set-upstream-to` IS THE REPAIR. `@{upstream}` is read
    # out of exactly this pair, and both values name a FULL ref path, so they cannot be steered by
    # what else the clone happens to hold. `git branch --set-upstream-to origin/<branch>` resolves
    # its argument through the ordinary ref-name rules instead, and in a clone that also holds a
    # LOCAL branch called `origin/<branch>` -- the state Codex named on this pull request -- it dies
    # with "ambiguous object name" on a bench that is perfectly wirable. So the unambiguous write
    # goes first and git's own wiring is what answers when the write did not take (`.git/config`
    # locked for an instant, Codex on #979): the fallback is where the diagnosis lives, and a run on
    # a bench that cannot vouch is never spent either way.
    $null = Invoke-External 'git' @('-C', $bench, 'config', "branch.${branch}.remote", 'origin')
    $null = Invoke-External 'git' @('-C', $bench, 'config', "branch.${branch}.merge", "refs/heads/${branch}")
    # AND THE UPSTREAM IS READ BACK, NOT ASSUMED -- both writes above discard their exit codes, and
    # the question the gate will ask is asked here first.
    #
    # THE READ-BACK ASKS FOR THE FULL REF, NOT THE ABBREVIATION (Codex P2 on this pull request).
    # `rev-parse --abbrev-ref @{upstream}` is documented to return a NON-AMBIGUOUS short name, not a
    # fixed spelling: in a clone that also holds a local branch named `origin/<branch>` it answers
    # `remotes/origin/<branch>`, and an equality against `origin/<branch>` would then reject a bench
    # that is correctly configured -- removing it and stalling that entry on every retry.
    # `--symbolic-full-name` has one spelling for one ref, so the comparison cannot be fooled.
    $wanted = "refs/remotes/origin/${branch}"
    $upstream = Invoke-External 'git' @('-C', $bench, 'rev-parse', '--symbolic-full-name', '@{upstream}') -CaptureError
    $upstreamName = if ($upstream.Code -eq 0 -and $upstream.Output.Count -gt 0) { ([string]$upstream.Output[0]).Trim() } else { '' }
    if ($upstreamName -ne $wanted) {
        $track = Invoke-External 'git' @('-C', $bench, 'branch', '--set-upstream-to', "origin/$branch", $branch) -CaptureError
        if ($track.Code -ne 0) {
            $why = (@($track.Output) | Where-Object { $_ } | Select-Object -First 2) -join ' | '
            if ([string]::IsNullOrWhiteSpace($why)) { $why = "git exited $($track.Code) and said nothing" }
            Write-Note "entry $($Candidate.File.Name): the bench branch cannot track origin/$branch -- $why"
            # THE BENCH GOES WITH THE REFUSAL, for the reason the head-mismatch block below gives:
            # left behind it holds the branch, and the next entry for it fails its own add with the
            # wrong reason.
            $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench)
            Set-EntryStatus -EntryPath $entryPath -State "refused: bench branch has no upstream -- $why"
            return 'stalled'
        }
        $upstream = Invoke-External 'git' @('-C', $bench, 'rev-parse', '--symbolic-full-name', '@{upstream}') -CaptureError
        $upstreamName = if ($upstream.Code -eq 0 -and $upstream.Output.Count -gt 0) { ([string]$upstream.Output[0]).Trim() } else { '' }
    }
    if ($upstreamName -ne $wanted) {
        $why = (@($upstream.Output) | Where-Object { $_ } | Select-Object -First 2) -join ' | '
        if ([string]::IsNullOrWhiteSpace($why)) { $why = "@{upstream} answered '$upstreamName'" }
        Write-Note "entry $($Candidate.File.Name): the bench could not be prepared -- upstream not configured: $why; leaving it queued"
        $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench)
        Set-EntryStatus -EntryPath $entryPath -State "waiting: bench could not be prepared -- upstream not configured: $why"
        return 'stalled'
    }
    # THE SERVER MUST BE AT THE QUEUED HEAD. The entry was resolved against `gh` a moment ago and
    # the fetch above brought the tracking ref to the server tip; if the two disagree the branch
    # moved in between, and a run on the queued sha would vouch for a commit the server no longer
    # names as the tip. Refuse, and let the resolve run again next time.
    $originRef = Invoke-External 'git' @('-C', $repoRoot, 'rev-parse', '--verify', '--quiet', "refs/remotes/origin/${branch}")
    $benchHeadSha = if ($originRef.Code -eq 0 -and $originRef.Output.Count -gt 0) { ([string]$originRef.Output[0]).Trim() } else { '' }
    if ($benchHeadSha -ne $head) {
        Write-Note "entry $($Candidate.File.Name): origin/$branch is at '$benchHeadSha', not at the queued head $head; leaving it queued"
        # THE REFUSED BENCH GOES FIRST, because it holds the branch: left behind, the next entry for
        # that branch fails its own add with "already checked out" and reports the wrong reason.
        # Bench before status, and the order is the order: a crash between the two leaves a removed
        # bench with no status, which is simply re-picked and re-prepared next run; the other order
        # leaves a status-written bench still holding the branch -- the jam this block removes.
        $null = Invoke-External 'git' @('-C', $repoRoot, 'worktree', 'remove', '--force', $bench)
        Set-EntryStatus -EntryPath $entryPath -State 'waiting: origin branch is not at the queued head'
        # 'stalled', not nothing: the outer loop skips an entry only on that word (#951), and a
        # refusal that returns no outcome is re-selected at once and wedges the line (Codex, #979).
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
    # `.factory/tools/slot-claim.sh:132` and by nothing on this path -- the runner launches
    # `ci/gate.ps1` directly, never the shell wrapper -- so it LOOKED like it was doing work it was
    # not (ISSUES 4). And `GRAPHHELM_SLOT_DIR` moves more than the lock: `ci/gate.ps1:4258` builds
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
