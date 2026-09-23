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
    # PER-SLOT CARGO HOME (#988). One CARGO_HOME shared by every slot is ONE `.package-cache` lock
    # and one registry directory that every concurrent gate writes through, whatever their
    # CARGO_TARGET_DIR is -- so the per-PR target directories already in place do not remove this
    # contention, they only move it. #988 measured 39.3 minutes of ONE run in which no stage ran
    # at all, three `Blocking waiting for file lock on package cache` lines in the log, and the
    # lost overlap then tripped ci/gate.ps1's #956 honesty guard into a RED with an EMPTY
    # failing-stage list. ci/merge-proof.ps1 refuses such a manifest at `:369`, so the cost is not
    # slow runs, it is runs nobody can press on.
    #
    # THE DEFAULT IS PER SLOT, NOT OPT-IN. A knob every caller has to remember is a feature that
    # is off in production; `-SharedCargoHome` is the way back to the machine-wide home.
    #
    # THEY GO ON `D:` BECAUSE IT IS THE ONLY DISK ABOVE ITS OWN FLOOR WITH ROOM FOR TWO HOMES --
    # not because it has the most free space, which was the first version of this comment and is
    # the wrong axis. Enumerated against `Get-TargetRootFloorGB` and AGENTS.md rather than eyeballed:
    #
    #   C:  101.1 GB free, floor 100  ->  1.1 GB headroom: fits ONE home, not two, and a home grows
    #   D:  432.7 GB free, floor  30  ->  402.7 GB headroom: the only one that holds both
    #   E:   29.9 GB free, floor  30  ->  BELOW ITS FLOOR already, with nothing on it
    #   F:   73.5 GB free, floor  30  ->  forbidden outright: AGENTS.md says "Never F:", the repo disk
    #
    # THE HONEST COST OF THAT, STATED RATHER THAN HIDDEN: `D:` is a surveillance-class platter and
    # #1053 item 3 moved gate TARGETS off it for a measured reason (median 1866 s there against
    # 1427 s on `E:`, over 370 records). Putting the cargo homes there is compliant and not ideal.
    # `registry/src` is ~30k small files, which is the shape a saturated platter handles worst, so
    # the moment `E:` is back above its floor these belong there instead -- one line, this branch.
    #
    # A NEW HOME IS COLD, and that is the cost #988 names: the first run per slot re-fetches the
    # registry (the machine-wide home measures 0.94 GB). It is deliberately NOT seeded by copying
    # -- a half-copied registry is a worse failure than one slow first build.
    #
    # MEASURED LIMIT, AND IT IS NOT IN THIS FILE. A cargo SUBCOMMAND installed by `cargo install`
    # lives in the OLD home's `bin`, and a cold home has none. `cargo nextest --version` under an
    # empty CARGO_HOME still answered `0.9.145` here -- but because `C:\Users\gabri\.cargo\bin` is on
    # PATH, not because the home carried it. On a machine where that directory is not on PATH the
    # gate's nextest stages would fail under a cold per-slot home.
    [string] $CargoHome,
    # THE WAY BACK, so a caller wanting the old behaviour does not have to know which path the
    # default above would have chosen.
    [switch] $SharedCargoHome,
    # #902: KEEP A WARM TARGET ACROSS RUNS OF ONE PULL REQUEST. OFF BY DEFAULT, and the default is a
    # measurement, not a preference. Across the 49 receipts of 2026-09-22/23, runner targets kept by
    # `Test-TargetShouldBeKept` (#1053) - state `complete` - proved 0 artefacts in 10 of 10 re-runs and
    # built in a median of ~2141 s (1354-14289); fresh targets built in a median of ~381 s (297-1134).
    # A controlled pair: two FULL gates started at 04:57Z on the same disk, 2734 s on a kept target
    # and 321 s on a fresh one. The ledger itself works (same-head re-runs proved 211-218 artefacts in
    # 53-256 s); the runner's re-runs follow a head move, usually a merge of main, which moves core
    # crates, so every dependent's crateInputHash changes and nothing proves - and rebuilding inside
    # a large stale target on this disk costs several times a cold build. The switch keeps #1053's
    # path for a caller that has measured a case where it pays.
    [switch] $KeepWarmTargets,
    # #902: HOW OFTEN A RUNNING GATE'S PULL REQUEST IS RE-READ. A gate for a head the branch no longer
    # names produces a receipt nobody can use; measured 2026-09-23, one slot spent ~45 min on #1214's
    # dead head. Every this-many seconds the runner asks the server again and cancels the run when the
    # head moved or the pull request closed. An unresolvable answer never cancels (see
    # Get-EntryStaleReason). 0 disables the check.
    [int] $HeadCheckSeconds = 120,
    [string] $StateDirectory,
    [switch] $Once,
    [int] $MaxIterations = 0,
    [int] $PollSeconds = 20,
    # HOW MANY PULL REQUEST DIRECTORIES THE STARTUP REAP MAY CONSIDER IN ONE PASS (#1085). The
    # bound is here and not inside the loop because an unbounded walk of a root that held ~150
    # per-PR targets would spend one `gh pr view` per directory before the first gate started, and
    # a runner that looks hung at startup gets killed rather than waited for. A truncated pass is
    # ANNOUNCED -- a silent cap reads as "nothing to clean", which is the state this exists to end.
    [int] $ReapCeiling = 60
)

$ErrorActionPreference = 'Stop'

if (-not $QueueDirectory) { $QueueDirectory = 'D:\graphhelm-slot\queue' }
if (-not $StateDirectory) { $StateDirectory = 'D:\graphhelm-slot\runner' }
if (-not $BenchRoot) { $BenchRoot = if ($Slot -eq 'SSD') { 'D:\runner-ssd' } else { 'D:\runner-hdd' } }
# #1053 item 3: NEITHER SLOT BUILDS ON THE PLATTER ANY MORE. `D:` is a WDC WD20PURZ -- a
# surveillance-class mechanical disk -- and it held one of the two gate targets. Measured over the
# 370 records in .factory/gate-runs: runs whose `cargoTargetDir` was on `D:` median 1866 s (n=156)
# against 1427 s on `E:` (n=183), and the per-stage split lands where it should, with compile-bound
# stages roughly halving (clippy 52 -> 25 s, schema catalog 116 -> 61 s) while execution-bound
# `workspace tests` barely moves (373 -> 366 s). Sampled live with two gates running: `D:` at
# 1005 % disk time with a queue depth of 8, `E:` at 3.8 %, `C:` at 14.3 %.
#
# The slot is still NAMED 'HDD'. The name is the QUEUE's, not the spindle's, and renaming it would
# move `GRAPHHELM_SLOT_DIR` and `$slotRoot` -- paths `.factory/tools/slot-claim.sh` and ci/gate.ps1
# read -- for no gain. AGENTS.md says so in the same paragraph that records this change.
#
# `C:` IS THE SYSTEM DISK AND THAT IS NOT TAKEN LIGHTLY. AGENTS.md read "never a gate target" until
# #1053, on the argument that a full `C:` takes the machine down. The argument stands; what changed
# is that the floor now has a caller (`Get-TargetRootFloorGB`, 100 GB) instead of being prose --
# and prose was not holding, because at the time this landed `C:` carried SEVENTEEN lane target
# directories against a rule saying at most two. An enforced floor on one gate target is a stricter
# regime than an unenforced rule on seventeen lane targets.
if (-not $TargetRoot) { $TargetRoot = if ($Slot -eq 'SSD') { 'E:\runner-targets\ssd' } else { 'C:\runner-targets\hdd' } }
# #988, resolved here and beside $TargetRoot because it is the same KIND of decision: a per-slot
# path this runner owns. The directory is NOT created here -- Write-Note does not exist yet at
# this point in the file, and a creation that cannot announce its own failure is the shape this
# change exists to end. Invoke-OneEntry creates it, once, and says so.
# BOTH AT ONCE IS A CONTRADICTION, NOT A PRECEDENCE QUESTION. The first version of this let
# -SharedCargoHome win silently, which buys the worst outcome available: the operator named a home,
# did not get it, and reads `cargoHome=shared` as the feature being broken rather than as their own
# mistake. There is no reading of the pair that is more likely than the other, so guessing one is
# strictly worse than refusing. It fails HERE, at launch, because a runner loop that starts and
# then quietly ignores an argument is the shape this whole change exists to end.
if ($SharedCargoHome -and $CargoHome) {
    throw "-CargoHome '$CargoHome' and -SharedCargoHome were both given and they ask for opposite things. Pass one: -CargoHome <path> for that home, -SharedCargoHome for the machine-wide one, or neither for this slot's default."
}
if ($SharedCargoHome) { $CargoHome = '' }
elseif (-not $CargoHome) {
    # A RUNNER TOLD WHERE ITS SLOT LIVES KEEPS ITS CACHE THERE. `-SlotRoot` is already this file's
    # test seam (`:117`: "exists so a TEST can point this at a throwaway directory"), and without
    # this branch every existing runner regression would silently create and populate the REAL
    # `D:\cargo-homes\hdd` -- a suite reaching outside its own temporary root.
    $CargoHome = if ($SlotRoot) { Join-Path $SlotRoot 'cargo-home' }
                 elseif ($Slot -eq 'SSD') { 'D:\cargo-homes\ssd' }
                 else { 'D:\cargo-homes\hdd' }
}
$script:CargoHomeChecked = $false
$script:CargoHomeUsable = $false

# THE RUNNER DOES NOT CLAIM FOR A BUILD. It sets the per-spindle slot paths and lets `ci/gate.ps1`
# claim through its own `Enter-GateSlot` (`:1953`). The startup terminal reaper briefly uses this
# SAME lock with CreateNew, because deleting a live gate's bench must share the gate exclusion.
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
    $view = & $Invoker 'gh' @('pr', 'view', "$PullRequest", '--json', 'headRefName,headRefOid,state,baseRefName,baseRefOid')
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
    $baseRef = if ($parsed | Get-Member -Name 'baseRefName' -MemberType NoteProperty) { [string] $parsed.baseRefName } else { '' }
    $baseSha = if ($parsed | Get-Member -Name 'baseRefOid' -MemberType NoteProperty) { [string] $parsed.baseRefOid } else { '' }
    return [pscustomobject]@{ Branch = $branch; Head = $resolved; State = $state; BaseRef = $baseRef; BaseSha = $baseSha }
}

function Test-HeadSha {
    param([string] $Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    return ($Value -cmatch '^[0-9a-f]{40}$')
}

function Test-PublicationEligibility {
    param(
        [object] $PullRequestState,
        [Parameter(Mandatory)] [string] $ExpectedHead,
        [string] $ExpectedBranch,
        [string] $ExpectedBaseRef,
        [string] $ExpectedBaseSha
    )
    if (-not $PullRequestState) {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request state unavailable' }
    }
    $state = [string]$PullRequestState.State
    if ($state -eq 'MERGED' -or $state -eq 'CLOSED') {
        return [pscustomobject]@{ Allowed = $false; Reason = "pull request became terminal ($state)" }
    }
    if ($state -ne 'OPEN') {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request open state unavailable' }
    }
    if ([string]$PullRequestState.Head -cne $ExpectedHead) {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request head moved during gate' }
    }
    if ($ExpectedBranch -and [string]$PullRequestState.Branch -cne $ExpectedBranch) {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request branch renamed during gate' }
    }
    if ($ExpectedBaseRef -and [string]$PullRequestState.BaseRef -cne $ExpectedBaseRef) {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request base branch changed during gate' }
    }
    if ($ExpectedBaseSha -and [string]$PullRequestState.BaseSha -cne $ExpectedBaseSha) {
        return [pscustomobject]@{ Allowed = $false; Reason = 'pull request base revision changed during gate' }
    }
    return [pscustomobject]@{ Allowed = $true; Reason = 'open at expected head' }
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

# WHO HOLDS THIS SPINDLE, read from the ONE lock the gate itself uses. This helper only reads. The
# build claim remains inside `ci/gate.ps1`; the startup reaper separately uses the same SLOT.lock
# with CreateNew rather than inventing a second lock that would not exclude a gate.
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

function Enter-TerminalReapSlot {
    param([Parameter(Mandatory)] [string] $Path)

    # Terminal cleanup removes gate benches and targets, so it participates in the same exclusion
    # as a gate. A read-then-delete check is only advisory and races a new gate claim. CreateNew is
    # the atomic boundary: either this cleanup owns the slot, or it changes nothing.
    $parent = Split-Path -Path $Path -Parent
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        $null = New-Item -ItemType Directory -Path $parent -Force
    }
    $start = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
    $content = @(
        "HELD by gate runner terminal reaper | $([DateTime]::UtcNow.ToString('o')) | STATUS: terminal cleanup",
        'Claimed through create-or-fail: the kernel refused every gate while cleanup owned this slot.',
        "holder: pid=$PID start=$start"
    ) -join "`n"
    try {
        $encoding = New-Object System.Text.UTF8Encoding($false)
        $bytes = $encoding.GetBytes($content)
        $stream = New-Object System.IO.FileStream($Path, [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write, [System.IO.FileShare]::None, 4096,
            [System.IO.FileOptions]::WriteThrough)
        try {
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Flush($true)
        } finally {
            $stream.Dispose()
        }
        return [pscustomobject]@{ Acquired = $true; Content = $content }
    } catch [System.IO.IOException] {
        return [pscustomobject]@{ Acquired = $false; Content = '' }
    } catch {
        Write-Note "reaper: could not claim the slot ($($_.Exception.Message)); skipping destructive cleanup"
        return [pscustomobject]@{ Acquired = $false; Content = '' }
    }
}

function Exit-TerminalReapSlot {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $ExpectedContent
    )
    try {
        if ((Test-Path -LiteralPath $Path) -and
            [System.String]::Equals([System.IO.File]::ReadAllText($Path), $ExpectedContent,
                [System.StringComparison]::Ordinal)) {
            Remove-Item -LiteralPath $Path -Force
        } else {
            Write-Note 'reaper: slot content changed while cleanup owned it; refusing to remove the lock'
        }
    } catch {
        Write-Note "reaper: could not release its slot claim ($($_.Exception.Message))"
    }
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
        [Parameter(Mandatory)] [string] $BenchPath,
        [switch] $FailClosed
    )
    $listed = Invoke-External 'git' @('-C', $RepositoryRoot, 'worktree', 'list', '--porcelain')
    if ($listed.Code -ne 0) {
        # The legacy fallback treats an unreadable list as registered so it will not recursively
        # delete an unowned directory. Terminal cleanup passes -FailClosed: unknown ownership must
        # retain the bench, never become permission to remove it.
        return (-not $FailClosed)
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
    if (-not $sawAnyWorktree) { return (-not $FailClosed) }
    return $false
}

# THE GATE'S OWN CANARY, NAMED ONCE HERE AND COMPARED AGAINST gate.ps1 BY THE SUITE.
# `ci/gate.ps1` builds this path at two sites and `Write-CanaryNonce` rewrites the TRACKED file
# before every run by design (#152), so EVERY bench a gate has ever run in is permanently modified
# in exactly this one file. Measured 2026-09-20 on the four real runner benches for merged pull
# requests: three read ` M tools/ci-canary/src/nonce.rs` and nothing else, with zero unpushed
# commits, and only the fourth -- which happened to be clean -- could be removed without --force.
# That is why the terminal cleanup below had to learn this path before it could reclaim anything.
# ONE CONSTANT, NOT A LITERAL AT EACH CALL SITE: a third copy would drift against gate.ps1's two in
# silence, and the drift's direction is a reaper that retains every bench forever while reporting
# success -- safe, and invisible.
$script:GateOwnCanaryPath = 'tools/ci-canary/src/nonce.rs'

function Test-BenchRemovalIsSafe {
    <#
    .SYNOPSIS
        May this bench be force-removed, or does it hold something only its owner can judge?
    .DESCRIPTION
        BOTH HALVES MUST HOLD, and each answers a different way to lose work.

        THE DIRT HALF. `git status --porcelain` must list nothing, or nothing except an UNSTAGED
        modification to the gate's own canary. Any untracked file, any other modified path, any
        staged change retains the bench. The canary excuse is exactly one path and exactly one
        column wide: a STAGED canary ('M ' or 'MM') is not the gate's doing, because
        `Write-CanaryNonce` writes the file and never touches the index.

        THE COMMIT HALF. The bench must hold no commit the server does not. A tree can be spotless
        and still carry an hour of work in a commit that was never pushed, and the dirt half is
        blind to it -- so a bench on a branch is compared against `refs/remotes/origin/<branch>`
        and must be zero commits ahead, and a DETACHED bench must have its HEAD reachable from some
        remote-tracking branch. Anything this cannot establish -- git refusing, a missing remote
        ref, a count that will not parse -- RETAINS. An unanswerable question is never permission
        to force-delete somebody's worktree.
    #>
    param(
        [Parameter(Mandatory)] [string] $BenchPath
    )
    $retain = { param($Why) [pscustomobject]@{ Safe = $false; Reason = $Why } }

    $status = Invoke-External 'git' @('-C', $BenchPath, 'status', '--porcelain') -CaptureError
    if ($status.Code -ne 0) {
        $said = (@($status.Output) | Where-Object { $_ } | Select-Object -First 1)
        return (& $retain "git status exited $($status.Code) in this bench ($said), so its contents are unknown")
    }
    $foreign = New-Object System.Collections.Generic.List[string]
    $sawCanary = $false
    foreach ($line in @($status.Output)) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line.Length -lt 4) {
            $foreign.Add($line.Trim())
            continue
        }
        $index = $line[0]
        $worktree = $line[1]
        # A rename prints `XY <old> -> <new>`; both ends count, because a rename INTO the canary
        # path from elsewhere is still not something this script wrote.
        $paths = @(($line.Substring(3) -split ' -> ') | ForEach-Object { ($_.Trim('"')) -replace '\\', '/' })
        $isGateOwn = ($index -eq ' ' -and $worktree -eq 'M' -and $paths.Count -eq 1 -and
            [string]::Equals($paths[0], $script:GateOwnCanaryPath, [System.StringComparison]::Ordinal))
        if ($isGateOwn) {
            $sawCanary = $true
            continue
        }
        foreach ($candidate in $paths) {
            if (-not $foreign.Contains($candidate)) { $foreign.Add($candidate) }
        }
    }
    if ($foreign.Count -gt 0) {
        return (& $retain ("it holds changes that are not the gate's canary: " + (($foreign | Sort-Object) -join ', ')))
    }

    $symbolic = Invoke-External 'git' @('-C', $BenchPath, 'symbolic-ref', '--quiet', '--short', 'HEAD') -CaptureError
    $branch = ''
    if ($symbolic.Code -eq 0) {
        $branch = [string] (@($symbolic.Output) | Where-Object { $_ } | Select-Object -First 1)
        $branch = $branch.Trim()
    }
    $vouched = ''
    if ($branch) {
        $remoteRef = "refs/remotes/origin/$branch"
        $verify = Invoke-External 'git' @('-C', $BenchPath, 'rev-parse', '--verify', '--quiet', $remoteRef) -CaptureError
        if ($verify.Code -ne 0) {
            return (& $retain "this clone has no $remoteRef to compare its commits against")
        }
        $ahead = Invoke-External 'git' @('-C', $BenchPath, 'rev-list', '--count', "$remoteRef..HEAD") -CaptureError
        $count = 0
        $counted = ($ahead.Code -eq 0 -and
            [int]::TryParse([string] (@($ahead.Output) | Where-Object { $_ } | Select-Object -First 1), [ref] $count))
        if (-not $counted) {
            return (& $retain "git could not count what this bench holds past $remoteRef")
        }
        if ($count -gt 0) {
            return (& $retain "it holds $count commit(s) that $remoteRef does not")
        }
        $vouched = "nothing ahead of $remoteRef"
    } else {
        $contains = Invoke-External 'git' @('-C', $BenchPath, 'branch', '--remotes', '--contains', 'HEAD') -CaptureError
        $reachable = @(@($contains.Output) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
        if ($contains.Code -ne 0 -or $reachable.Count -eq 0) {
            return (& $retain 'its HEAD is detached and sits on no remote-tracking branch, so the server may not have it')
        }
        $vouched = "detached HEAD reachable from $($reachable[0].Trim())"
    }

    $dirt = if ($sawCanary) { "its only difference from HEAD is the gate's own $script:GateOwnCanaryPath (#152)" } else { 'its working tree is clean' }
    return [pscustomobject]@{ Safe = $true; Reason = "$dirt, and $vouched" }
}

# THE HAZARD THIS CHANGE INTRODUCED, AND THE GUARD THAT ANSWERS IT (#1085).
# Before the startup reap, `Remove-TerminalRunnerBench` ran only when a queue entry for an already
# terminal pull request was SELECTED, and that never collided with a live gate on the same bench,
# because the selector was the same loop that would otherwise have been gating. The reap widened
# that: EVERY runner startup now walks the roots, and a pull request can merge WHILE another runner
# process is gating in `pr<N>`. A `git worktree remove --force` on that directory mid-compile
# destroys the run and the receipt it was about to push.
#
# FOUR SHAPES WERE CONSIDERED.
#   1. Skip a bench some live process is "working in", by walking processes and comparing working
#      directories. REJECTED: the gate child's cwd is the bench, but its descendants -- cargo,
#      rustc, link.exe -- are the ones that live longest; a process working directory is not
#      readable for every process without elevation; and a read that fails would have to fail SAFE,
#      which here means retaining every bench on any machine where the query is refused. A guard
#      that cannot answer on the ordinary machine turns back into "retain everything", which is the
#      defect #1085 exists to remove.
#   2. Skip when the slot lock names a live holder whose cwd is that bench. REJECTED for the same
#      cwd problem and for a worse one: the slot lock is per SPINDLE, one lock for many benches, so
#      it can say "somebody is gating" and not "in this bench". It would either retain far too much
#      (any live gate anywhere retaining every bench on that spindle) or, with a cwd read that
#      failed open, retain nothing.
#   3. Simply not reaping the bench THIS runner is about to use. REJECTED as insufficient rather
#      than wrong: it is already true -- the reap runs before selection and never touches an OPEN
#      pull request -- and it says nothing about the OTHER runner process, which is the whole
#      hazard.
#   4. A CLAIM FILE THE WORKING RUNNER WRITES. CHOSEN. The claim is authored by the only process
#      that knows the answer, it is a plain file so reading it needs no privilege, and it carries a
#      process identity -- a pid AND that pid's start time -- so a claim left behind by a crashed
#      runner is provably stale rather than eternal.
#
# ONE CLAIM PER RUNNER PROCESS, NOT ONE PER BENCH. `Invoke-OneEntry` overwrites this process's claim
# with the pull request it is about to work, so moving on to the next entry releases the previous
# bench by itself. There is no disarm to forget and no `finally` to skip. A runner that is idle
# leaves a claim naming a pull request that is still open, which nothing reaps anyway; a runner that
# DIED leaves a claim whose pid is dead, which this reads as stale and deletes.
#
# IT LIVES BESIDE THE BENCH, NEVER INSIDE IT. A file under the worktree would appear in
# `git status --porcelain` as an untracked path, and `Test-BenchRemovalIsSafe` would then retain
# that bench forever -- safe, and permanently. The name is not `pr<digits>` and it is a file rather
# than a directory, so the reaper's own walk never mistakes a claim for a candidate.
$script:RunnerClaimPrefix = '.runner-claim-'

function Get-RunnerProcessStamp {
    <#
    .SYNOPSIS
        A process identity that survives pid reuse: the pid, and that pid's start time.
    .DESCRIPTION
        A bare pid is not an identity. Windows reuses pids, and a claim naming only a number would
        eventually read a completely unrelated process as "the runner still gating here" and retain
        the bench forever. The start time pins WHICH process wore that number.
    #>
    param([Parameter(Mandatory)] [int] $ProcessId)
    $process = Get-Process -Id $ProcessId -ErrorAction Stop
    return [pscustomobject]@{
        ProcessId    = $ProcessId
        StartedTicks = $process.StartTime.ToUniversalTime().Ticks
    }
}

function Set-RunnerWorkClaim {
    <#
    .SYNOPSIS
        Record that THIS runner process is working pull request N, in every managed root.
    .DESCRIPTION
        Written into each root that exists, because the bench and the target can live on different
        spindles and a reaper pointed at only one of them must still see the claim. A claim that
        cannot be written is NOTED and never fatal: this runner's job is to gate, and the cost of a
        missing claim is a reap that may take this bench -- which is the state before this guard
        existed, not a new one.
    #>
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Roots
    )
    $stamp = $null
    try { $stamp = Get-RunnerProcessStamp -ProcessId $PID } catch { $stamp = $null }
    if (-not $stamp) {
        Write-Note "could not read this runner's own process start time; the bench claim for #$PullRequest is not being written"
        return
    }
    $payload = (ConvertTo-Json ([ordered]@{
                pid          = $stamp.ProcessId
                startedTicks = $stamp.StartedTicks
                pullRequest  = "$PullRequest"
                writtenUtc   = (Get-Date).ToUniversalTime().ToString('o')
            }) -Compress)
    foreach ($rootPath in @($Roots)) {
        if ([string]::IsNullOrWhiteSpace($rootPath) -or -not (Test-Path -LiteralPath $rootPath)) { continue }
        $claim = Join-Path $rootPath ("{0}{1}.json" -f $script:RunnerClaimPrefix, $PID)
        try {
            [System.IO.File]::WriteAllText($claim, $payload)
        } catch {
            Write-Note "could not write the bench claim $claim ($($_.Exception.Message)); a concurrent reap would not see this run"
        }
    }
}

function Test-PullRequestHasLiveWorker {
    <#
    .SYNOPSIS
        Is some LIVE runner process working this pull request right now?
    .DESCRIPTION
        FAIL SAFE MEANS $true. Every answer this cannot establish -- a claim directory that will not
        list, a claim file that will not read, JSON that will not parse, a field that is not there,
        a pid whose start time the OS refuses to disclose -- returns "yes, somebody is working
        here", and the caller retains. The only route to $false is to read every claim in every root
        and find each one either naming a different pull request or naming a process that provably
        no longer exists.

        THE ONE ESTABLISHED NEGATIVE is `Get-Process -Id` reporting that the OS has no such process.
        A claim whose process is gone is STALE, and stale claims are deleted as they are found, so a
        machine that reboots mid-gate does not accumulate files that mean nothing.
    #>
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Roots
    )
    $unknown = { param($Why) [pscustomobject]@{ Live = $true; Reason = $Why } }
    foreach ($rootPath in @($Roots)) {
        if ([string]::IsNullOrWhiteSpace($rootPath) -or -not (Test-Path -LiteralPath $rootPath)) { continue }
        $claims = @()
        try {
            $claims = @(Get-ChildItem -LiteralPath $rootPath -File -Filter ("{0}*.json" -f $script:RunnerClaimPrefix) -ErrorAction Stop)
        } catch {
            return (& $unknown "the claims in $rootPath could not be listed ($($_.Exception.Message))")
        }
        foreach ($claimFile in $claims) {
            $parsed = $null
            try {
                $parsed = (Get-Content -LiteralPath $claimFile.FullName -Raw -ErrorAction Stop) | ConvertFrom-Json
            } catch {
                return (& $unknown "$($claimFile.Name) could not be read or parsed ($($_.Exception.Message)), so what it claims is unknown")
            }
            $claimedPr = if ($parsed) { [string] $parsed.pullRequest } else { '' }
            $claimedPid = 0
            $claimedTicks = [int64] 0
            $wellFormed = ($parsed -and
                -not [string]::IsNullOrWhiteSpace($claimedPr) -and
                [int]::TryParse([string] $parsed.pid, [ref] $claimedPid) -and
                [int64]::TryParse([string] $parsed.startedTicks, [ref] $claimedTicks))
            if (-not $wellFormed) {
                return (& $unknown "$($claimFile.Name) does not carry both a pull request and a process identity, so it cannot be ruled stale")
            }
            $stamp = $null
            $missing = $false
            try {
                $stamp = Get-RunnerProcessStamp -ProcessId $claimedPid
            } catch [Microsoft.PowerShell.Commands.ProcessCommandException] {
                $missing = $true
            } catch {
                return (& $unknown "the process $claimedPid named by $($claimFile.Name) could not be interrogated ($($_.Exception.Message))")
            }
            if ($missing -or -not $stamp -or $stamp.StartedTicks -ne $claimedTicks) {
                Remove-Item -LiteralPath $claimFile.FullName -Force -ErrorAction SilentlyContinue
                continue
            }
            if ([string]::Equals($claimedPr.Trim(), "$PullRequest".Trim(), [System.StringComparison]::Ordinal)) {
                return [pscustomobject]@{ Live = $true; Reason = "runner process $claimedPid is working #$PullRequest right now ($($claimFile.Name))" }
            }
        }
    }
    return [pscustomobject]@{ Live = $false; Reason = 'no live runner process claims this pull request' }
}


function Remove-PublishedRunnerTarget {
    param(
        [Parameter(Mandatory)] [string] $TargetRoot,
        [Parameter(Mandatory)] [string] $PullRequest,
        # #902: the keep policy applies only when the runner was told to keep warm targets.
        [switch] $KeepWarm
    )
    # The PR number was validated before this function is called. Construct the only target this
    # runner owns from that number; never accept a path from the queue or from a remote response.
    $target = Join-Path $TargetRoot ("pr{0}" -f $PullRequest)
    if (-not (Test-Path -LiteralPath $target)) { return }

    # #1053 AND #1085 MEET HERE, AND NEITHER ONE WINS BY BEING LOUDER.
    #   #1085 (this function) says: build artefacts may disappear ONLY after the receipt is
    #     provably on the server. That is a PRECONDITION on removal, and the caller enforces it.
    #   #1053 (`Test-TargetShouldBeKept`) says: a target whose last build FINISHED and vouched,
    #     on a root that is still above its free-space floor, is a cache worth keeping -- measured
    #     at ~565 s of cold build saved per re-run.
    # Composed: the receipt buys the RIGHT to remove, the keep policy decides whether removing is
    # WORTH it. Removing unconditionally here would make #1053's warm reuse dead code, because
    # every successful run ends in this branch. Keeping unconditionally would bring back the litter
    # the owner's standing order is about: a target for a pull request that never re-runs is never
    # visited by the run-start eviction, so it is evicted here instead.
    if ($KeepWarm -and (Test-TargetShouldBeKept -TargetDir $target -TargetRoot $TargetRoot)) {
        Write-Note "published receipt for #$PullRequest; keeping the runner target $target -- its last build finished and vouched and $TargetRoot is above its floor (#1053)"
        return
    }
    Write-Note "published receipt for #$PullRequest; removing the runner target $target (build cache is not evidence)"
    Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $target) {
        Write-Note "WARNING $target could not be removed after the receipt push; the target remains as a backstop"
    }
}

function Get-ReceiptCommitProof {
    param(
        [Parameter(Mandatory)] [string] $BenchPath,
        [Parameter(Mandatory)] [string] $ExpectedParent
    )
    # A successful push is not evidence that this run produced a receipt: pushing an unchanged
    # branch is a successful no-op. Require one new commit directly on the queued head, and require
    # every path in that commit to be in the durable gate-run store.
    $tip = Invoke-External 'git' @('-C', $BenchPath, 'rev-parse', '--verify', 'HEAD')
    if ($tip.Code -ne 0 -or $tip.Output.Count -eq 0) { return $null }
    $tipSha = ([string] $tip.Output[0]).Trim()
    if (-not (Test-HeadSha $tipSha) -or $tipSha -eq $ExpectedParent) { return $null }
    $parent = Invoke-External 'git' @('-C', $BenchPath, 'rev-parse', '--verify', "$tipSha^")
    if ($parent.Code -ne 0 -or $parent.Output.Count -eq 0 -or ([string]$parent.Output[0]).Trim() -ne $ExpectedParent) { return $null }
    $changed = Invoke-External 'git' @('-C', $BenchPath, 'diff-tree', '--no-commit-id', '--name-status', '-r', $tipSha)
    if ($changed.Code -ne 0 -or $changed.Output.Count -eq 0) { return $null }
    $paths = @()
    foreach ($line in $changed.Output) {
        $parts = ([string]$line).Split("`t")
        if ($parts.Count -ne 2 -or $parts[0] -cne 'A' -or $parts[1] -notlike '.factory/gate-runs/*') { return $null }
        $paths += $parts[1]
    }
    if ($paths.Count -eq 0) { return $null }
    $receipt = Invoke-External 'git' @('-C', $BenchPath, 'ls-tree', '-r', '--name-only', $tipSha, '--', '.factory/gate-runs')
    if ($receipt.Code -ne 0 -or @($receipt.Output | Where-Object { ([string]$_).Trim() -in $paths }).Count -eq 0) { return $null }
    return $tipSha
}

function Publish-Receipt {
    param(
        [Parameter(Mandatory)] [string] $BenchPath,
        [Parameter(Mandatory)] [string] $ExpectedParent,
        [Parameter(Mandatory)] [string] $Branch
    )
    $receiptTip = Get-ReceiptCommitProof -BenchPath $BenchPath -ExpectedParent $ExpectedParent
    if (-not $receiptTip) { return $false }
    $push = Invoke-External 'git' @('-C', $BenchPath, 'push', 'origin', "HEAD:$Branch")
    if ($push.Code -ne 0) { return $false }
    $visible = Invoke-External 'git' @('-C', $BenchPath, 'ls-remote', 'origin', "refs/heads/$Branch")
    $serverTip = if ($visible.Code -eq 0 -and $visible.Output.Count -gt 0) { (([string]$visible.Output[0]) -split "`t")[0].Trim() } else { '' }
    if ($serverTip -ceq $receiptTip) { return $true }
    Write-Note "receipt push returned 0 but server head was '$serverTip', expected receipt $receiptTip; retaining target"
    return $false
}

function Remove-TerminalRunnerBench {
    param(
        [Parameter(Mandatory)] [string] $RepositoryRoot,
        [Parameter(Mandatory)] [string] $BenchRoot,
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [string] $State
    )
    if ($State -ne 'MERGED' -and $State -ne 'CLOSED') { return }
    $bench = Join-Path $BenchRoot ("pr{0}" -f $PullRequest)
    if (-not (Test-Path -LiteralPath $bench)) { return }

    # A terminal PR is not permission to delete an arbitrary directory. Only remove a worktree
    # that this clone can enumerate at this exact path, and do not force removal: a dirty or active
    # worktree is retained for its owner to inspect.
    if (-not (Test-BenchIsRegistered -RepositoryRoot $RepositoryRoot -BenchPath $bench -FailClosed)) {
        Write-Note "terminal pull request #$PullRequest has an unregistered bench at $bench; leaving it alone"
        return
    }
    # WITHOUT --force THIS FUNCTION RECLAIMED ALMOST NOTHING, AND THAT IS MEASURED, NOT FEARED.
    # `git worktree remove` refuses any bench with a modified or untracked file, and the gate
    # rewrites the tracked canary in EVERY bench it runs in (#152) -- so three of the four real
    # benches for merged pull requests on this machine on 2026-09-20 were retained by a rule
    # written for somebody's unfinished work, and only the one clean bench was taken. The
    # discriminator above is what makes --force here narrower than the un-forced call it replaces:
    # it refuses on the untracked file and on the unpushed commit that `worktree remove` cannot see
    # at all, and it excuses exactly one path this script's sibling wrote itself.
    $safety = Test-BenchRemovalIsSafe -BenchPath $bench
    if (-not $safety.Safe) {
        Write-Note "pull request #$PullRequest is $State but its bench was retained ($($safety.Reason))"
        return
    }
    $remove = Invoke-External 'git' @('-C', $RepositoryRoot, 'worktree', 'remove', '--force', $bench) -CaptureError
    if ($remove.Code -eq 0 -and -not (Test-Path -LiteralPath $bench)) {
        # THE LOG NAMES WHICH CONDITION LICENSED THE FORCE, because "removed with --force" alone
        # would be indistinguishable from the unconditional delete this must never become.
        Write-Note "pull request #$PullRequest is $State; removed its runner bench $bench -- $($safety.Reason)"
        return
    }
    $why = (@($remove.Output) | Where-Object { $_ } | Select-Object -First 2) -join ' | '
    if ([string]::IsNullOrWhiteSpace($why)) { $why = "git exited $($remove.Code)" }
    Write-Note "pull request #$PullRequest is $State but its bench was retained ($why)"
}

function Remove-TerminalRunnerTarget {
    param(
        [Parameter(Mandatory)] [string] $TargetRoot,
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [string] $State
    )
    if ($State -ne 'MERGED' -and $State -ne 'CLOSED') { return }
    # A terminal PR cannot be retried. Its target is therefore no longer a useful warm cache, and
    # unlike a published receipt there is no future open run that can evict it. Derive the exact
    # runner-owned path from the validated PR number; never accept a queue or server path here.
    $target = Join-Path $TargetRoot ("pr{0}" -f $PullRequest)
    if (-not (Test-Path -LiteralPath $target)) { return }
    Write-Note "pull request #$PullRequest is $State; removing its terminal runner target $target (no retry can use this cache)"
    Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $target) {
        Write-Note "WARNING $target could not be removed after terminal pull request cleanup; the target remains"
    }
}

function Invoke-TerminalRunnerReap {
    <#
    .SYNOPSIS
        Remove the benches and targets of pull requests that are already MERGED or CLOSED.
    .DESCRIPTION
        WHY A SEPARATE PASS EXISTS AT ALL. AGENTS.md says this runner "removes its own benches under
        -BenchRoot once their pull request is no longer open". Until #1085 the only callers of
        `Remove-TerminalRunnerBench` and `Remove-TerminalRunnerTarget` were inside `Invoke-OneEntry`'s
        MERGED/CLOSED queue-drop path, which runs only when a queue entry for that pull request is
        SELECTED AFTER it became terminal. The ordinary lifecycle -- the run publishes its receipt,
        the entry is deleted, the pull request merges an hour later -- never re-enters that path, so
        the bench and the target simply survived. Measured 2026-09-20 ~00:20Z: about 150 per-PR
        targets, almost all terminal and up to 21 GB each, and about 90 stale benches, with `E:`
        below the 30 GB floor this repository's own document sets. The sentence was true of the
        document and false of the disk.

        THE NUMBER COMES FROM THE DIRECTORY NAME AND FROM NOTHING ELSE. `pr<digits>`, directly under
        a managed root, never recursively, never from a queue entry and never from the server: this
        function force-removes worktrees, and the only defensible input to that is a name this
        script's own code wrote.

        AN OPEN PULL REQUEST IS NEVER TOUCHED -- the removers return early on any state that is not
        MERGED or CLOSED -- and that is precisely why this may run BEFORE the selection loop: it
        cannot take the bench or the target of the entry this run is about to pick. An unresolvable
        pull request, or one whose state the server did not say, is SKIPPED rather than read as
        terminal, which is the same choice `Resolve-PullRequestBranch` already documents for the
        same reason: the decision on an unknown state must never be the destructive one.

        ONE BAD DIRECTORY MUST NOT STOP THE PASS OR THE RUNNER. Every candidate is wrapped, because
        the whole point of reaping at startup is that the gate runs afterwards.
    #>
    param(
        [Parameter(Mandatory)] [string] $RepositoryRoot,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $BenchRoot,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $TargetRoot,
        [int] $Ceiling = 60,
        # A SEAM, matching `Resolve-PullRequestBranch`'s own `-Invoker`: the suite drives this
        # against real directories with a resolver it controls, because a cell that had to reach
        # GitHub would measure whatever those pull requests happen to be today.
        [scriptblock] $Resolver
    )
    if (-not $Resolver) { $Resolver = { param($PullRequest) Resolve-PullRequestBranch -PullRequest "$PullRequest" } }

    $found = New-Object System.Collections.Generic.List[int]
    foreach ($rootPath in @($BenchRoot, $TargetRoot)) {
        if ([string]::IsNullOrWhiteSpace($rootPath) -or -not (Test-Path -LiteralPath $rootPath)) { continue }
        $children = @()
        try {
            $children = @(Get-ChildItem -LiteralPath $rootPath -Directory -ErrorAction Stop)
        } catch {
            Write-Note "reaper: could not list $rootPath ($($_.Exception.Message)); leaving that root alone this pass"
            continue
        }
        foreach ($child in $children) {
            $named = [regex]::Match($child.Name, '^pr(\d+)$')
            if (-not $named.Success) { continue }
            $number = 0
            if (-not [int]::TryParse($named.Groups[1].Value, [ref] $number)) { continue }
            if (-not $found.Contains($number)) { $found.Add($number) }
        }
    }

    $sorted = @($found | Sort-Object)
    $truncated = ($Ceiling -gt 0 -and $sorted.Count -gt $Ceiling)
    $considered = if ($truncated) { @($sorted | Select-Object -First $Ceiling) } else { $sorted }
    if ($truncated) {
        Write-Note ("reaper: $($sorted.Count) pr<N> directories under the managed roots against a ceiling of " +
            "$Ceiling; reaping the $Ceiling lowest-numbered this pass and leaving $($sorted.Count - $Ceiling) " +
            'for the next startup -- this pass is TRUNCATED, not empty')
    }

    $removed = 0
    $skipped = 0
    foreach ($number in @($considered)) {
        try {
            $resolved = & $Resolver "$number"
            $state = if ($resolved) { [string] $resolved.State } else { '' }
            if ([string]::IsNullOrWhiteSpace($state)) {
                $skipped++
                Write-Note "reaper: the server did not say what #$number is; skipping it, because an unknown state must not read as terminal"
                continue
            }
            if ($state -ne 'MERGED' -and $state -ne 'CLOSED') {
                $skipped++
                continue
            }
            # A TERMINAL PULL REQUEST CAN STILL HAVE A GATE RUNNING IN ITS BENCH. The merge happens
            # on the server while the compile happens here, and this pass is the frequent path to a
            # `--force` on that directory. Unknown answers retain (`Test-PullRequestHasLiveWorker`
            # returns live on everything it cannot establish), so a claim this cannot read costs a
            # reap and never a run.
            $worker = Test-PullRequestHasLiveWorker -PullRequest "$number" -Roots @($BenchRoot, $TargetRoot)
            if ($worker.Live) {
                $skipped++
                Write-Note "reaper: #$number is $state but its bench is in use ($($worker.Reason)); leaving bench and target alone this pass"
                continue
            }
            $bench = Join-Path $BenchRoot ("pr{0}" -f $number)
            $target = Join-Path $TargetRoot ("pr{0}" -f $number)
            $had = @($bench, $target) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
            Remove-TerminalRunnerBench -RepositoryRoot $RepositoryRoot -BenchRoot $BenchRoot -PullRequest "$number" -State $state
            Remove-TerminalRunnerTarget -TargetRoot $TargetRoot -PullRequest "$number" -State $state
            $removed += @(@($had) | Where-Object { -not (Test-Path -LiteralPath $_) }).Count
        } catch {
            $skipped++
            Write-Note "reaper: #$number could not be reaped ($($_.Exception.Message)); the pass continues"
        }
    }
    Write-Note "reaper: $($sorted.Count) pr<N> directories found, $(@($considered).Count) considered, $removed directory/directories reclaimed, $skipped skipped"
    return [pscustomobject]@{ Found = $sorted.Count; Considered = @($considered).Count; Removed = $removed; Skipped = $skipped; Truncated = [bool] $truncated }
}

function Test-TargetBuildFinished {
    <#
    .SYNOPSIS
        Did the last gate build in this target directory FINISH, so the next run may keep it?
    .DESCRIPTION
        #943 gave every run a cold target because the freshness instrument of the day was
        TIMESTAMP-based: a reused binary predates the run that reuses it, so every warm target read
        as stale and the runner -- which, unlike a human, cannot rename its way out -- hit it on
        every re-run. Its CONTENT-based replacement landed on 2026-09-16 (#904/#1038, "a reused test
        binary passes on content, never on cargo's fingerprint") and was extended on 2026-09-19
        (#1007). The cold-target workaround outlived its cause by three days, and it is expensive:
        across the 71 records in .factory/gate-runs carrying `buildPassSecs`, 67 read "no target
        directory yet, so nothing is being reused" and the cold median is 565.7 s, against 65.459 s
        for the one FULL run in that population that did reuse (324 artefacts proven, 1 rebuilt).

        FAIL-CLOSED, AND CLOSED MEANS TODAY'S BEHAVIOUR. Only one answer preserves a directory: a
        marker that exists, parses, and records `complete`. Absent, unreadable, unparseable, missing
        the field, `building`, `unproven`, or any word this function does not know all answer
        $false, and $false is the unconditional removal this repository has been doing since #943.
        A new state added to the gate therefore costs a cold build until someone teaches this
        function about it -- which is the safe direction for an instrument whose other direction is
        "keep a target whose build died half-written".

        WHY `unproven` IS NOT PRESERVED even though the gate does not abort on it. The gate treats
        it as contaminated-but-not-suspect and proves or rebuilds each binary itself, so keeping the
        directory would be sound. It is still removed here, because the predicate a cell can state
        in one sentence -- "the last build finished AND vouched" -- is the one that stays true as
        the state vocabulary grows.

        WHAT THIS FUNCTION DELIBERATELY DOES NOT DO: decide whether the preserved target is SAFE to
        build in. That is `Get-TargetBuildState` in ci/gate.ps1, which the gate calls before its
        first compile and which aborts the run on `interrupted` and `concurrent` (#455). This
        function only answers whether the runner should spend minutes deleting tens of gigabytes
        first. The two must not be collapsed: this one runs in a different process, cannot see
        gate.ps1's functions, and errs toward deleting; that one errs toward refusing to build.

        The marker's name is repeated here rather than shared, for the reason ci/gate.ps1 gives for
        repeating it between its own reader and writer: each function is lifted out of its file
        alone by a suite. `ci/gate-runner.tests.ps1` reads the literal out of gate.ps1 and asserts
        this file carries the same one, so the two copies cannot drift apart in silence.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $TargetDir)

    if ([string]::IsNullOrWhiteSpace($TargetDir)) { return $false }
    $markerPath = Join-Path $TargetDir '.graphhelm-build-state.json'
    if (-not (Test-Path -LiteralPath $markerPath)) { return $false }

    $marker = $null
    try {
        $marker = [System.IO.File]::ReadAllText($markerPath) | ConvertFrom-Json
    } catch {
        # A marker this runner cannot read is not a marker saying `complete`.
        return $false
    }
    if ($null -eq $marker) { return $false }

    # BY INDEX, never `$marker.state`. Under `Set-StrictMode` a dotted read of an absent property
    # THROWS, and a throw here would kill the runner over a malformed file written by something
    # else -- turning "delete this target" into "stop processing the queue".
    $stateProperty = $marker.PSObject.Properties['state']
    if ($null -eq $stateProperty) { return $false }
    return [string]::Equals([string]$stateProperty.Value, 'complete', [System.StringComparison]::Ordinal)
}

function Get-TargetRootFloorGB {
    <#
    .SYNOPSIS
        How much free space must remain on the disk holding this target root.
    .DESCRIPTION
        The numbers are AGENTS.md's own, not new ones: `C:` (the system disk) keeps 100 GB, every
        other spindle keeps 30 GB. Until #1053 those floors were prose, and prose does not hold --
        at the time this landed `C:` carried SEVENTEEN lane target directories against a rule that
        says at most two. This is the same rule with a caller.

        AN UNRECOGNISED ROOT GETS THE NON-ZERO FLOOR, never 0. A floor of 0 is "fill the disk",
        which is the one outcome this function exists to prevent, and it must not be what an
        unexpected path silently buys.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)

    if (-not [string]::IsNullOrWhiteSpace($Path) -and
        $Path.TrimStart().StartsWith('C:', [System.StringComparison]::OrdinalIgnoreCase)) {
        return 100
    }
    return 30
}

function Get-TargetRootFreeGB {
    <#
    .SYNOPSIS
        Free gigabytes on the drive holding this path, or $null when that cannot be answered.
    .DESCRIPTION
        $null IS A THIRD ANSWER AND EVERY CALLER MUST TREAT IT AS ONE. Returning 0 for "unknown"
        would read as a full disk, and returning a large number would read as an empty one; both
        are claims this function has not earned. The caller below turns $null into "do not keep the
        target", which is the pre-#1053 behaviour and therefore the safe direction.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)

    if ([string]::IsNullOrWhiteSpace($Path)) { return $null }
    try {
        $qualifier = Split-Path -Qualifier $Path -ErrorAction Stop
    } catch {
        return $null
    }
    $name = $qualifier.TrimEnd(':')
    $drive = Get-PSDrive -Name $name -PSProvider FileSystem -ErrorAction SilentlyContinue
    # `Free` is $null on a provider that does not report it, and that is not zero either.
    if ($null -eq $drive -or $null -eq $drive.Free) { return $null }
    return [math]::Round(([double]$drive.Free) / 1GB, 2)
}

function Test-TargetShouldBeKept {
    <#
    .SYNOPSIS
        Both halves of the keep decision: the last build finished AND the disk can afford to keep it.
    .DESCRIPTION
        #1053 stopped the runner deleting a target before every run, which is worth ~500 s on a
        re-run. It also removed the only thing that bounded a target's size, because cargo never
        collects stale artefacts: one target measured 23.1 GB across 68 364 files, and a pull
        request re-run ten times would keep growing it. That bound is restored HERE rather than by
        going back to the unconditional delete -- under disk pressure the preserved target is a
        cache, and a cache gets evicted.

        The two halves fail in the same direction on purpose: any doubt about either one deletes,
        and deleting is exactly what this runner did before #1053.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $TargetDir,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $TargetRoot
    )

    if (-not (Test-TargetBuildFinished -TargetDir $TargetDir)) { return $false }
    $free = Get-TargetRootFreeGB -Path $TargetRoot
    if ($null -eq $free) { return $false }
    return ($free -ge (Get-TargetRootFloorGB -Path $TargetRoot))
}

function Get-EntryStaleReason {
    <#
    .SYNOPSIS
        Why a RUNNING entry no longer deserves its slot, or $null when it still does.
    .DESCRIPTION
        Asked while the gate runs, with the same resolver that admitted the entry, so the two
        answers cannot disagree about what "the head" means. Three answers cancel: the pull request
        is MERGED or CLOSED, or its head is not the one being gated. EVERY OTHER ANSWER KEEPS THE
        RUN, including "the server could not be asked": a gate killed on a network blip loses
        forty minutes of good work, and a gate left running on a head that moved loses at most the
        same forty minutes - so the unknown case falls on the side that can only waste, never
        destroy.
    #>
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [string] $Head,
        [scriptblock] $Invoker
    )
    $resolved = Resolve-PullRequestBranch -PullRequest $PullRequest -Invoker $Invoker
    if (-not $resolved) { return $null }
    if ($resolved.State -eq 'MERGED' -or $resolved.State -eq 'CLOSED') {
        return "pull request is $($resolved.State)"
    }
    if ($resolved.Head -and $resolved.Head -ne $Head) {
        return "head moved to $($resolved.Head.Substring(0, [Math]::Min(8, $resolved.Head.Length)))"
    }
    return $null
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
    #     pr = '\..\..\..'    Join-Path -> C:\runner-targets\hdd\pr\..\..\..   GetFullPath -> C:\
    #   Since #1053 item 3 that escape lands on the SYSTEM disk's root, so this refusal is more
    #   load-bearing now, not less, than when the target sat on the platter.
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
        Remove-TerminalRunnerBench -RepositoryRoot $repoRoot -BenchRoot $BenchRoot -PullRequest "$pr" -State $resolvedPr.State
        Remove-TerminalRunnerTarget -TargetRoot $TargetRoot -PullRequest "$pr" -State $resolvedPr.State
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
    # ARM THE CLAIM AT THE ARMING SITE, NOT AT THE FIRING SITE. It goes here, the instant this
    # process commits to a bench path, rather than beside `Start-Process` below: the window a
    # concurrent reap must not enter starts when the worktree is prepared, not when the compiler
    # starts. Overwriting this process's single claim is also what RELEASES the previous entry's
    # bench, so there is no disarm that a `return` further down could skip.
    Set-RunnerWorkClaim -PullRequest "$pr" -Roots @($BenchRoot, $TargetRoot)
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
    # #901, owner order 2026-09-23: A CHANGE THAT IS ONLY MARKDOWN NO CODE READS IS NOT BUILT.
    # `.factory/MERGE-CHECKLIST.md`'s docs-only exception already lets such a pull request merge
    # without a receipt; before this block the runner spent a full gate on it anyway (#1214: about
    # seventeen minutes of the slot for four Markdown files). `ci/docs-only.ps1` takes the same
    # decision from the diff, with a positive control on its own grep, and the runner acts ONLY on
    # its exit 0. Every other answer -- 1 not docs-only, 2 undecided, a crash, no merge base --
    # falls through to the build below, so a failure here costs time and never a gate.
    #
    # THE DECISION IS THE RUNNER'S OWN COPY, NEVER THE PULL REQUEST'S (review of #1216 by lane
    # 5bdc38). Read from the bench, a pull request could ship a `ci/docs-only.ps1` that exits 0 and
    # skip its own gate, and the only defence left was a presser remembering to re-run main's copy.
    # The bench supplies DATA -- its diff and its tree -- and the script that judges it comes from the
    # runner's checkout, the way merge-proof runs from origin/main's. A runner whose checkout predates
    # the script has no copy, and falls through to the build: the fail-safe direction again.
    $docsVerdictFile = Join-Path $StateDirectory ("$pr-$runId.docs-only.json")
    $docsOnlyScript = Join-Path $PSScriptRoot 'docs-only.ps1'
    try {
        $docsBase = Invoke-External 'git' @('-C', $bench, 'merge-base', 'origin/main', $resolvedPr.Head)
        if ((Test-Path -LiteralPath $docsOnlyScript -PathType Leaf) -and $docsBase.Code -eq 0 -and $docsBase.Output.Count -gt 0) {
            $docsVerdict = Invoke-External 'powershell' @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-File', $docsOnlyScript,
                '-MergeBase', ([string]$docsBase.Output[0]).Trim(),
                '-Head', $resolvedPr.Head, '-RepoRoot', $bench)
            if ($docsVerdict.Code -eq 0 -and $docsVerdict.Output.Count -gt 0) {
                [System.IO.File]::WriteAllText($docsVerdictFile, (($docsVerdict.Output | ForEach-Object { [string]$_ }) -join "`n"))
                Write-Note "entry $($Candidate.File.Name): docs-only, not built (#901) -> $docsVerdictFile"
                Set-EntryStatus -EntryPath $entryPath -State "skipped: docs-only, no code reads the changed Markdown; not built (#901) verdict=$docsVerdictFile"
                Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
                return 'dropped'
            }
            Write-Note "entry $($Candidate.File.Name): docs-only decision exited $($docsVerdict.Code); building"
        }
    } catch {
        Write-Note "entry $($Candidate.File.Name): docs-only decision failed ($($_.Exception.Message)); building"
    }
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

    # PR-BASE PROOF IS AN INPUT TO THE GATE, NOT SOMETHING A PRESS CAN RECONSTRUCT LATER. Capture
    # GitHub's baseRefOid from the same live PR view as the head, fetch that ref, and prove the
    # exact object exists before handing the snapshot to gate.ps1. Any unanswered step widens to
    # local-only evidence and says so in the runner log; it never invents a base from origin/main.
    $snapshotArgument = ''
    $snapshotCaptured = $false
    $snapshotFile = Join-Path $StateDirectory ("$pr-$runId.landing.json")
    $baseRef = [string]$resolvedPr.BaseRef
    $baseSha = [string]$resolvedPr.BaseSha
    try {
        $baseFormat = if ([string]::IsNullOrWhiteSpace($baseRef)) {
            [pscustomobject]@{ Code = 1; Output = @() }
        } else {
            Invoke-External 'git' @('-C', $bench, 'check-ref-format', "refs/heads/$baseRef")
        }
        $fetchBase = if ($baseFormat.Code -eq 0) {
            Invoke-External 'git' @('-C', $bench, 'fetch', '--quiet', 'origin', "+refs/heads/$baseRef`:refs/remotes/origin/$baseRef")
        } else {
            [pscustomobject]@{ Code = 1; Output = @() }
        }
        $baseObject = if ($fetchBase.Code -eq 0 -and (Test-HeadSha $baseSha)) {
            Invoke-External 'git' @('-C', $bench, 'cat-file', '-e', "${baseSha}^{commit}")
        } else {
            [pscustomobject]@{ Code = 1; Output = @() }
        }
        if ($baseFormat.Code -eq 0 -and $fetchBase.Code -eq 0 -and $baseObject.Code -eq 0 -and
            (Test-HeadSha $baseSha)) {
            $snapshot = [ordered]@{
                head = [string]$resolvedPr.Head
                sha = $baseSha
                ref = $baseRef
                pullRequest = [int]$pr
            } | ConvertTo-Json -Compress
            [System.IO.File]::WriteAllText($snapshotFile, $snapshot, (New-Object System.Text.UTF8Encoding($false)))
            $snapshotArgument = " -LandingSnapshotPath '$snapshotFile'"
            $snapshotCaptured = $true
            Write-Note "entry $($Candidate.File.Name): landing snapshot -> $snapshotFile $snapshot"
        } else {
            Write-Note "entry $($Candidate.File.Name): landing snapshot not captured (ref $($baseFormat.Code), fetch $($fetchBase.Code), object $($baseObject.Code)); refusing to start the gate"
        }
    } catch {
        Write-Note "entry $($Candidate.File.Name): landing snapshot failed ($($_.Exception.Message)); refusing to start the gate"
    }
    if (-not $snapshotCaptured) {
        Set-EntryStatus -EntryPath $entryPath -State 'waiting: landing snapshot unavailable'
        return 'stalled'
    }

    # Touch the build cache only after the PR-base evidence is complete. A missing landing snapshot
    # must leave the previous target intact for retry and diagnosis; deleting it before this point
    # turns a network/provenance refusal into needless cold work.
    if (Test-Path -LiteralPath $target) {
        if ($KeepWarmTargets -and (Test-TargetShouldBeKept -TargetDir $target -TargetRoot $TargetRoot)) {
            Write-Note "entry $($Candidate.File.Name): keeping $target -- its last build finished and vouched, so the gate proves each binary on content (#1053, #904)"
        } elseif (-not $KeepWarmTargets) {
            Write-Note "entry $($Candidate.File.Name): removing the previous run's target $target -- warm runner targets proved 0 artefacts in 10 of 10 re-runs and built ~5x slower than fresh ones (#902; -KeepWarmTargets restores #1053's keep)"
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $target) {
                Write-Note "entry $($Candidate.File.Name): WARNING $target could not be fully removed; this run may redden on the canary staleness cell (#943)"
            }
        } elseif (Test-TargetBuildFinished -TargetDir $target) {
            $freeNote = Get-TargetRootFreeGB -Path $TargetRoot
            $freeText = if ($null -eq $freeNote) { 'unknown' } else { "$freeNote GB" }
            Write-Note "entry $($Candidate.File.Name): removing $target -- its last build DID finish, but $TargetRoot has $freeText free against a floor of $(Get-TargetRootFloorGB -Path $TargetRoot) GB, so the reuse is evicted rather than grown (#1053)"
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $target) {
                Write-Note "entry $($Candidate.File.Name): WARNING $target could not be fully removed; this run may redden on the canary staleness cell (#943)"
            }
        } else {
            Write-Note "entry $($Candidate.File.Name): removing the previous run's target $target (#943: the last build did not finish and vouch, so this run must be cold)"
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $target) {
                Write-Note "entry $($Candidate.File.Name): WARNING $target could not be fully removed; this run may redden on the canary staleness cell (#943)"
            }
        }
    }

    # The gate is an untrusted child from the runner's point of view. A terminating PowerShell
    # error used to skip the final `$LASTEXITCODE` write, leaving the parent with `rc=unknown` and
    # no way to tell a wrapper failure from a still-running child. Keep the wrapper verdict
    # explicit, append the exception to the same bounded transcript, and write the rc in `finally`
    # so every child outcome has a durable answer.
    # #988: SET IT IN THE CHILD, not merely in this process. Inheritance would carry it too, but
    # then the home a run used is invisible in the log and a future reader cannot tell which
    # cache produced a result -- the same reason CARGO_TARGET_DIR is written here, not exported.
    #
    # AND WE CREATE THE DIRECTORY EVEN THOUGH CARGO WOULD. Measured: `CARGO_HOME` pointed at a
    # path that does not exist, `cargo fetch` -> rc=0 and cargo creates the home itself, with
    # `.package-cache`, `.global-cache` and `registry` in it. So this block is not required for
    # the feature to work. It earns its place twice over anyway: it ANNOUNCES the one-off cold
    # fetch at the moment it is incurred rather than leaving a reader to wonder why the first
    # run on a slot was slow, and it detects an unusable path BEFORE the gate starts -- turning
    # what would otherwise be a failed gate into a slower run on the machine-wide home.
    $cargoHomeAssignment = ''
    if ($CargoHome) {
        if (-not $script:CargoHomeChecked) {
            $script:CargoHomeChecked = $true
            try {
                if (-not (Test-Path -LiteralPath $CargoHome)) {
                    New-Item -ItemType Directory -Path $CargoHome -Force -ErrorAction Stop | Out-Null
                    Write-Note "created the per-slot cargo home $CargoHome; its first run pays one cold registry fetch (#988)"
                }
                $script:CargoHomeUsable = $true
            } catch {
                # ANNOUNCED. A silent fall back to the machine-wide home is indistinguishable
                # from the feature working, and an unannounced fallback turns a gap into drift:
                # every later run would contend on the shared lock while the log said nothing.
                Write-Note "WARNING could not create the per-slot cargo home $CargoHome ($($_.Exception.Message)); running with the machine-wide home instead"
                $script:CargoHomeUsable = $false
            }
        }
        if ($script:CargoHomeUsable) {
            $cargoHomeAssignment = "`$env:CARGO_HOME='$CargoHome'; "
            Write-Note "entry $($Candidate.File.Name): CARGO_HOME -> $CargoHome"
        }
    }
    $inner = "Set-Location '$bench'; " +
        "`$env:CARGO_TARGET_DIR='$target'; " +
        $cargoHomeAssignment +
        "`$env:GRAPHHELM_SLOT_LOCK_PATH='$slotLock'; " +
        "`$wrapperRc = 99; " +
        "try { & ./ci/gate.ps1$scopeArgument$snapshotArgument *> '$logFile'; " +
        "`$wrapperRc = if (`$null -eq `$LASTEXITCODE) { 99 } else { [int]`$LASTEXITCODE } } " +
        "catch { Add-Content -LiteralPath '$logFile' -Value ('[runner] gate child threw: ' + `$_.Exception.Message); `$wrapperRc = 99 } " +
        "finally { if (`$null -eq `$wrapperRc) { `$wrapperRc = 99 }; " +
        "Set-Content -LiteralPath '$rcFile' -Value ([string]`$wrapperRc) }"
    # -NonInteractive here is the worst case of #925, not merely another instance of it: this child
    # runs in a HIDDEN window, so a prompt from a missing `[Parameter(Mandatory)]` in ci/gate.ps1 --
    # which has seventeen of them -- would be waiting on a console no operator can see or answer. The
    # watchdog below would then find a flat log and, correctly by its own rule, only record a
    # suspicion after twenty minutes. The selector call above already carried this flag; the run it
    # launches did not.
    $proc = Start-Process powershell -ArgumentList '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', $inner -PassThru -WindowStyle Hidden
    # Cache the native handle immediately while the process is live. Without this touch, .NET can
    # return an empty ExitCode after a fast wrapper exits, which would hide the child result.
    $procHandle = $proc.Handle
    "$($proc.Id)" | Set-Content -LiteralPath $pidFile

    # PROOF OF LIFE IS THE LOG GROWING, and "wedged" is never a short reading. A healthy gate sits
    # flat between stages -- 45 s measured, with one descendant and no compiler.
    $lastSize = -1
    $flatSince = Get-Date
    $headCheckedAt = Get-Date
    while (-not (Test-Path -LiteralPath $rcFile)) {
        Start-Sleep -Seconds $PollSeconds
        # #902: A RUN FOR A HEAD THE BRANCH NO LONGER NAMES IS CANCELLED, NOT FINISHED. The receipt
        # it would publish certifies a commit nobody can merge, and the slot it holds is the scarcest
        # thing on the machine. Checked on a clock, not every poll, so the server is asked a few
        # times an hour; an unresolvable answer keeps the run (Get-EntryStaleReason).
        if ($HeadCheckSeconds -gt 0 -and ((Get-Date) - $headCheckedAt).TotalSeconds -ge $HeadCheckSeconds) {
            $headCheckedAt = Get-Date
            $staleReason = Get-EntryStaleReason -PullRequest "$pr" -Head $head
            if ($staleReason) {
                Write-Note "pr $pr cancelled while building: $staleReason; stopping the gate process tree $($proc.Id)"
                # The whole tree: the gate's cargo, rustc, nextest and PostgreSQL children would
                # otherwise keep the disk busy for the next entry. The slot lock the gate held is
                # left for the gate's own dead-holder reclaim (#902), which the next run performs.
                $null = & taskkill.exe /PID $proc.Id /T /F 2>&1
                try { $null = $proc.WaitForExit(30000) } catch { }
                Set-EntryStatus -EntryPath $entryPath -State "cancelled: $staleReason (log=$logFile)"
                Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction SilentlyContinue
                Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
                return 'cancelled'
            }
        }
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
    $childExit = $null
    try {
        if ($proc.HasExited) { $childExit = $proc.ExitCode }
    } catch {
        $childExit = $null
    }
    $rc = if (Test-Path -LiteralPath $rcFile) { (Get-Content -LiteralPath $rcFile -Raw).Trim() } else { '99' }
    if ([string]::IsNullOrWhiteSpace($rc) -or $rc -eq 'unknown') { $rc = '99' }
    $childExitText = if ($null -eq $childExit) { 'null' } else { [string]$childExit }

    # Re-read the live PR immediately before publication. A receipt pushed after the PR became
    # terminal is unreachable from the merged main; a receipt pushed after the head moved certifies
    # a commit the branch no longer names. Retain the queue, bench, target and local receipt in both
    # races so the evidence remains diagnosable instead of being deleted as if publication worked.
    $publicationState = Resolve-PullRequestBranch -PullRequest "$pr"
    $publicationEligibility = Test-PublicationEligibility -PullRequestState $publicationState -ExpectedHead $head `
        -ExpectedBranch $branch -ExpectedBaseRef $baseRef -ExpectedBaseSha $baseSha
    if (-not $publicationEligibility.Allowed) {
        Write-Note "entry $($Candidate.File.Name): receipt not published: $($publicationEligibility.Reason); retaining queue, bench and target"
        Set-EntryStatus -EntryPath $entryPath -State ("waiting: receipt publication refused: {0}" -f $publicationEligibility.Reason)
        return 'stalled'
    }

    # PUSH THE MANIFEST THE INSTANT IT EXISTS. What survives an app restart is what is on the server.
    $pushed = Publish-Receipt -BenchPath $bench -ExpectedParent $head -Branch $branch

    # THE RECEIPT MUST BE ON THE SERVER BEFORE BUILD ARTEFACTS DISAPPEAR. A failed push leaves both
    # target and bench available as the backstop for diagnosis and retry. Re-read after the push as
    # well: the PR may merge in the narrow interval between the pre-push read and the network write.
    # In that race the receipt exists on the branch but not in merged main, so it is not publication
    # for cleanup purposes and every local retry/debug artefact must remain.
    if ($pushed) {
        $publishedTip = Get-ReceiptCommitProof -BenchPath $bench -ExpectedParent $head
        $postPublicationState = Resolve-PullRequestBranch -PullRequest "$pr"
        $postPublicationEligibility = if ($publishedTip) {
            Test-PublicationEligibility -PullRequestState $postPublicationState -ExpectedHead $publishedTip `
                -ExpectedBranch $branch -ExpectedBaseRef $baseRef -ExpectedBaseSha $baseSha
        } else {
            [pscustomobject]@{ Allowed = $false; Reason = 'published receipt tip could not be reproved' }
        }
        # GitHub may briefly return the pre-push head while the PR remains OPEN. Publish-Receipt
        # already proved the remote branch is exactly $publishedTip, so that one stale head value is
        # safe to tolerate; any third head, unknown state, or terminal state still refuses cleanup.
        if (-not $postPublicationEligibility.Allowed -and $postPublicationState -and
            $postPublicationState.Head -ceq $head) {
            $postPublicationEligibility = Test-PublicationEligibility -PullRequestState $postPublicationState `
                -ExpectedHead $head -ExpectedBranch $branch -ExpectedBaseRef $baseRef -ExpectedBaseSha $baseSha
            if ($postPublicationEligibility.Allowed) {
                $postPublicationEligibility = [pscustomobject]@{ Allowed = $true; Reason = 'open; PR head propagation still shows queued head' }
            }
        }
        if (-not $postPublicationEligibility.Allowed) {
            Write-Note "entry $($Candidate.File.Name): receipt push raced with $($postPublicationEligibility.Reason); retaining queue, bench and target"
            Set-EntryStatus -EntryPath $entryPath -State ("waiting: pushed receipt is not merge-reachable: {0}" -f $postPublicationEligibility.Reason)
            return 'stalled'
        }
        Remove-PublishedRunnerTarget -TargetRoot $TargetRoot -PullRequest "$pr" -KeepWarm:$KeepWarmTargets
    }

    # #988: WHICH CACHE PRODUCED THIS. The gate manifest records cargoTargetDir and not the home,
    # so without this field the only trace of the cache a run used dies with the runner's notes
    # -- and `cargoHome=shared` is how an operator sees the per-slot home did NOT take effect.
    $homeUsed = if ($cargoHomeAssignment) { $CargoHome } else { 'shared' }
    Set-EntryStatus -EntryPath $entryPath -State ("finished rc=$rc childExit=$childExitText pushed=$pushed cargoHome=$homeUsed log=$logFile")
    Write-Note "pr $pr finished rc=$rc childExit=$childExitText pushed=$pushed cargoHome=$homeUsed"
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

Write-Note "repository $repoRoot; queue $QueueDirectory; slot lock $slotLock (temporarily claimed only for terminal cleanup; gate owns it during builds)"

# ONCE PER RUNNER PROCESS, AT STARTUP, BEFORE THE FIRST SELECTION. Cleanup first atomically owns the
# same slot a gate must own. A pull request can become terminal while its gate is still running;
# OPEN-state filtering alone cannot protect that live gate's bench or target.
$reapClaim = Enter-TerminalReapSlot -Path $slotLock
if ($reapClaim.Acquired) {
    try {
        $null = Invoke-TerminalRunnerReap -RepositoryRoot $repoRoot -BenchRoot $BenchRoot -TargetRoot $TargetRoot -Ceiling $ReapCeiling
    } finally {
        Exit-TerminalReapSlot -Path $slotLock -ExpectedContent $reapClaim.Content
    }
} else {
    Write-Note 'terminal reaper skipped: the gate slot is already held or could not be claimed safely'
}

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
