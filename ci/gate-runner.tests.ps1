# #902: isolated tests for ci/gate-runner.ps1 -- the half of the gate runner that resolves a queued
# pull request into a branch.
#
# WHY THERE IS A FAKE `gh` AND NOT A REAL ONE. The subject is how this repository's PowerShell hands
# ARGUMENTS to a native command, so the case has to observe the argument vector as the callee
# receives it. A real `gh` would answer over the network, make the suite depend on a live pull
# request, and -- worse -- hide the defect behind whatever that pull request happens to be. The fake
# accepts `gh pr view <n> --json ...` and REFUSES any vector carrying `--jq`, with the same exit
# code and the same sentence the real `gh` answered. A jq expression is exactly what cannot survive
# this repository's argument passing when it contains a quoted space, so its presence is the
# defect's signature. (Counting the arguments instead does not work: `cmd` splits on the comma in
# `headRefName,headRefOid`, so the callee sees six where PowerShell passed five.)
#
# The runner is run as a CHILD PROCESS against a throwaway queue this file creates and removes,
# matching ci/gate-queue.tests.ps1. Nothing here starts a gate: every case is an entry whose head
# disagrees with the fake server's, which the runner drops BEFORE it prepares a bench. That drop is
# only reachable once the resolve has succeeded, so it is the positive evidence.
#
# Homegrown PASS/FAIL harness, matching the sibling suites; this repository carries no Pester.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   2  the fake refuses a --jq vector, and accepts one without it (harness controls)
#   1  the runner exits 0            1  it does not report "could not resolve"
#   1  the log names the resolved head
#   1  a status file is written      1  it says the head moved
#   1  an unresolvable pull request leaves the entry queued
#   1  and that case's status is the not-resolvable one
#   1  a MERGED pull request's entry is DROPPED, not benched
#   1  and its status says which state it was
#   1  HEAD OF LINE: a stalled entry is skipped and the next one is processed in the SAME -Once run
#   1  and the stalled entry itself is still queued afterwards
#   1  the bench-failure status names git's own reason, not just "could not be prepared"
#   1  #1040: after a successful worktree add the bench branch is set to track origin/<branch>
#   1  and a refusal there is the entry status, with git's words   1  and the entry stays queued
#   1  ARRANGEMENT: the selection really does pass the skip list, so removing it is a sabotage
#   1  THE FAILURE MODE IS A FAILURE: with the exclusion removed, the run EXITS rather than spins
#   1  and it exits 3, the code that says the skip list stopped excluding
#   1  #943 arrangement: the target assignment and the launch are both found, in order
#   1  #1053: the removal is guarded by Test-TargetBuildFinished   1  and the removal still exists
#   1  and the removal sits in the guard's ELSE branch             1  and the runner says so in the log
#   1  #1053 arrangement: Test-TargetBuildFinished is locatable, so the cells below drive the real one
#   1  the runner names the same marker file gate.ps1 does (no silent drift into a permanent cold build)
#   1  `complete` is KEPT        1  `building` in the same directory is removed
#   1  `unproven` is removed     1  an unknown state word is removed
#   1  a marker with no `state` field is removed   1  a marker that does not parse is removed
#   1  a directory with build output and no marker is removed (the pre-#1053 state of every target)
#   1  #1053 item 3 arrangement: the floor, the free probe and the keep decision are all locatable
#   1  the system disk keeps a 100 GB floor   1  a non-system spindle keeps 30 GB
#   1  an unrecognised root still gets a non-zero floor (never "fill the disk")
#   1  the free probe answers a positive number for a real path
#   1  a non-existent drive answers $null -- a third state, neither 0 nor a number
#   1  a finished target with room is KEPT
#   1  the SAME target is EVICTED under the floor (the bound item 1 removed, restored)
#   1  unreadable free space is treated as no headroom
#   1  an unfinished build is removed even with room to spare (the provenance half still governs)
#   1  NEITHER slot defaults to a target on D:   1  CONTROL: the SSD slot still defaults to E:
#   1  a traversing `pr` is refused as malformed    1  and the directory it aimed at survives
#   1  CONTROL: the runner clone has no tracking config for the branch (#902 upstream cell)
#   1  the bench is prepared, not dropped     1  it sits at the entry head
#   1  its branch tracks origin/<branch>, so `pushed` can be answered
#   1  CONTROL: the fixture commit exists (signing and hooks declared off)
#   1  the gate receives a PR landing snapshot with the exact pull request and head
#   1  the snapshot names the live base ref
#   1  the snapshot names the live base commit
#   1  CONTROL: origin carries a second commit past the queued head
#   1  the entry stays queued        1  the status names the refusal
#   1  no build started on the moved branch
#   1  the entry queued behind the refused one is reached in the same run (the refusal stalls, not wedges)
#   1  CONTROL: a narrow-refspec clone has no origin/<branch> before the run
#   1  under a narrow refspec the bench is still prepared at the queued head
#   1  and its branch tracks origin (the refspec was widened to cover the branch)
#   1  CONTROL: the branch is deleted on origin
#   1  a plain fetch still succeeds afterwards (no branch-specific refspec left behind)
#   1  a failed fetch stalls the entry, naming the fetch      1  and no build starts on the stale ref
#   1  the older entry is selected before the younger one (explicit ages, not a sleep)
#   1  CONTROL: origin holds a branch named --upload-pack=nope     1  it is refused by name, no bench, no fetch
#   1  a live dash-prefixed entry stays queued     1  a terminal dash-prefixed entry is removed
#   1  terminal status names CLOSED     1  terminal dash entry never creates a bench
#   1  CONTROL: both invariance comparisons can fail
#   1  the real clone refspec is byte-identical around the reclaim cell     1  and so is its worktree list
#   1  CONTROL: a local branch shadows origin/<branch>     1  the bench is accepted and reaches the build step
#   1  CONTROL: the abbreviation really is remotes/origin/<branch>     1  the full symbolic name has one spelling
#   1  CONTROL: .git/config is locked     1  a failed config write stalls, naming the upstream     1  and the bench is removed
#   1  CONTROL: the log names both heads     1  two live passes are picked before twenty verdict-less objects (#1133)
#   1  missing landing proof leaves the entry queued   1  its status names the refusal
#   1  no receipt is published without landing proof   1  its target remains for retry
#   1  publication eligibility accepts the same open head
#   1  publication eligibility rejects a terminal PR   1  and rejects a moved head
#   1  a second eligibility read sits after push and before any evidence cleanup
#   1  publication rejects a renamed branch   1  a changed base ref   1  a changed base SHA
#   1  live branch/base mutation retains queue   1  status names refusal
#   1  no receipt reaches old branch             1  target remains
#   1  a gate child throw gets a concrete wrapper rc and childExit
#   1  the throwing child cannot report a published receipt
#   1  the throwing child's log records the exception
#   1  the throwing child's target remains for diagnosis
#   1  terminal drops call both safe bench and target cleanup in the real caller
#   1  a MERGED drop removes its explicit runner target
#   1  a live slot holder protects its terminal target from the startup reaper
#   1  the successful gate records rc=0 and childExit=0
#   -- #1085, the reaper's cells hold the properties they claim (+8 over 183):
#   1  a STAGED canary retains the bench (the column half, previously asserted by nothing)
#   0  the near misses pr1169-backup/old-pr42/pr12x never enter the CANDIDATE SET (REWRITTEN in
#      place of the old 'scratch-notes' control, which shared no form with its subject -- not a new cell)
#   1  the DEFAULT ceiling of 60, exercised without -Ceiling
#   1  a real runner process at an EMPTY queue prints the reaper's summary before 'queue empty'
#   1  a LIVE claim retains, and the same claim leaves another pull request reapable
#   1  a claim naming a really-dead process does not retain, and is swept
#   1  an unreadable and an identity-less claim both fail SAFE
#   1  a live claim survives a real reap pass while an unclaimed target in it is still reclaimed
#   1  Invoke-OneEntry's parse tree really CALLS Set-RunnerWorkClaim, between bench and launch
#   -- #1085, the detached arm of the bench-removal guard is asserted (+5 over 192):
#   1  CONTROL: git really refuses the reachability probe in the 723 clone
#   1  a DETACHED canary-dirty bench a remote-tracking branch DOES contain is REMOVABLE
#   1  a DETACHED canary-dirty bench carrying a commit no remote has is RETAINED
#   1  a DETACHED bench whose reachability question cannot be answered is RETAINED (fails SAFE)
#   1  and the BRANCH arm fails safe too: no origin/<branch> RETAINS, it does not read as zero
$ExpectedAssertionCount = 197

$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) {
        Write-Host "  PASS: $Message" -ForegroundColor Green
    } else {
        $script:failures++
        Write-Host "  FAIL: $Message" -ForegroundColor Red
    }
}

$runner = Join-Path $PSScriptRoot 'gate-runner.ps1'
$root = Join-Path ([System.IO.Path]::GetTempPath()) ("gr-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$queue = Join-Path $root 'queue'
$state = Join-Path $root 'state'
$shimDirectory = Join-Path $root 'bin'
$slotRoot = Join-Path $root 'slot'
New-Item -ItemType Directory -Path $queue -Force | Out-Null
New-Item -ItemType Directory -Path $state -Force | Out-Null
New-Item -ItemType Directory -Path $shimDirectory -Force | Out-Null
# A THROWAWAY SLOT ROOT. Without it this suite reads the machine's real lock and refuses whenever a
# lane happens to be gating -- a green suite would then mean 'nobody was busy', not 'the code works'.
New-Item -ItemType Directory -Path $slotRoot -Force | Out-Null

# The fake server's head. Distinct from the entry head below, on purpose: the runner then DROPS the
# entry instead of preparing a bench, and the drop is what proves the resolve worked.
$serverHead = 'a' * 40
$serverBranch = 'issue-902-fake-branch'
$terminalDashHead = 'd' * 40
$shimPath = Join-Path $shimDirectory 'gh.cmd'

$strictShim = @"
@echo off
setlocal enabledelayedexpansion
set "A1=%~1"
set "A2=%~2"
set "SAWJQ="
:scan
if "%~1"=="" goto scanned
if "%~1"=="--jq" set "SAWJQ=1"
shift
goto scan
:scanned
if "%A1%"=="api" (
  echo 0
  exit /b 0
)
if not "%A1%"=="pr" (
  echo unknown command 1>&2
  exit /b 1
)
if not "%A2%"=="view" (
  echo unknown command 1>&2
  exit /b 1
)
if defined SAWJQ (
  echo accepts at most 1 arg^(s^), received 2 1>&2
  exit /b 1
)
echo {"headRefName":"$serverBranch","headRefOid":"$serverHead"}
exit /b 0
"@

$refusingShim = @"
@echo off
if "%~1"=="api" (
  echo 0
  exit /b 0
)
echo accepts at most 1 arg^(s^), received 2 1>&2
exit /b 1
"@

# A shim that answers DIFFERENTLY per pull request, which is what the head-of-line case needs: one
# entry that cannot be resolved (so it stalls) and one that resolves to a moved head (so it is
# dropped). Both are cheap -- neither reaches a bench, so no gate is ever started.
$perPrShim = @"
@echo off
setlocal enabledelayedexpansion
set "A1=%~1"
set "A3=%~3"
if "%A1%"=="api" (
  echo 0
  exit /b 0
)
if "%A3%"=="100" (
  echo pull request 100 is not resolvable 1>&2
  exit /b 1
)
if "%A3%"=="200" (
  echo {"headRefName":"$serverBranch","headRefOid":"$serverHead","state":"OPEN"}
  exit /b 0
)
if "%A3%"=="300" (
  echo {"headRefName":"$serverBranch","headRefOid":"BBBBBBBB","state":"MERGED"}
  exit /b 0
)
if "%A3%"=="400" (
  echo {"headRefName":"--upload-pack=nope","headRefOid":"$terminalDashHead","state":"CLOSED"}
  exit /b 0
)
echo unknown pull request 1>&2
exit /b 1
"@

Set-Content -LiteralPath $shimPath -Value $strictShim -Encoding ASCII

$previousPath = $env:PATH
$env:PATH = "$shimDirectory;$env:PATH"

# EVERY CASE HERE EXPECTS A REFUSAL SOMEWHERE, so a child writes to stderr on purpose. Under
# Windows PowerShell 5.1 a native command's stderr becomes a NativeCommandError record, and under
# `Stop` that record is terminating -- the harness would die on the case it exists to measure
# instead of reading its exit code. The exit code is the observation; stderr is for a human.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # ---------------------------------------------------------------------------------------------
    # NO CELL RUNS IN THE DEVELOPER'S CLONE (Lane B, BLOCK on this pull request).
    #
    # Eleven of this suite's runner invocations had no `Push-Location` and no `git` shim, so their
    # cwd was the repository the suite lives in. `ci/gate-runner.ps1` widens `remote.origin.fetch`
    # there and fetches from that clone's real `origin` over the network. On THIS machine the clone
    # already carries the default wildcard and the fake heads resolve to nothing, so the damage was
    # invisible and the suite reported green while doing it; on a fresh `--single-branch` clone the
    # refspec gained the wildcard. Shimming the vectors one cell at a time is how the hole survived
    # two rounds -- it closes the cells somebody remembered. The cwd is closed instead, once, for
    # every cell, and the two facts that matter are MEASURED at the end rather than argued:
    #
    #   the developer's clone .... refspec list and worktree list byte-identical across the suite
    #   the guard clone .......... created NARROW, so the widening's arrival is observable there
    #
    # The second is what makes the first non-vacuous. A pair of equalities over a clone that already
    # holds the wildcard proves nothing; the same pair beside a clone where the wildcard demonstrably
    # ARRIVED proves the code path ran and did not run there.
    # ---------------------------------------------------------------------------------------------
    $realClone = "$(& git rev-parse --show-toplevel 2>$null)".Trim()
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($realClone)) `
        -Message "CONTROL: the suite knows which clone it is running in, or the invariance reads below measure nothing (got '$realClone')"
    $realRefspecsBefore = (& git -C $realClone config --get-all remote.origin.fetch 2>&1 | Out-String)
    $realWorktreesBefore = (& git -C $realClone worktree list --porcelain 2>&1 | Out-String)

    $guardOrigin = Join-Path $root 'guard-origin.git'
    $guardAuthor = Join-Path $root 'guard-author'
    $guardClone = Join-Path $root 'guard-clone'
    $guardBase = 'guard-base'
    $null = & git init --bare -q $guardOrigin 2>$null
    $null = & git init -q $guardAuthor 2>$null
    $null = & git -C $guardAuthor config user.email 'suite@example.invalid'
    $null = & git -C $guardAuthor config user.name 'gate-runner suite'
    $null = & git -C $guardAuthor config commit.gpgsign false
    $null = & git -C $guardAuthor config core.hooksPath ([System.IO.Path]::Combine($root, 'no-hooks'))
    Set-Content -LiteralPath (Join-Path $guardAuthor 'README') -Value 'the clone every cell runs in' -Encoding ASCII
    $null = & git -C $guardAuthor add -A 2>$null
    $null = & git -C $guardAuthor commit -q -m 'guard fixture'
    $null = & git -C $guardAuthor push -q $guardOrigin "HEAD:refs/heads/$guardBase" 2>$null
    # NARROW ON PURPOSE. `--single-branch` writes one branch-specific refspec and no wildcard, which
    # is the clone shape on which the widening is observable -- and the shape a lane's runner clone
    # actually has.
    $null = & git clone -q --origin origin --single-branch --branch $guardBase $guardOrigin $guardClone 2>$null
    # THE LIVENESS READER, BESIDE THE CLONE. `Resolve-SlotLockReaderPath` looks first next to the
    # runner and then at `<repoRoot>/ci/slot-lock.ps1`. The #951 cell runs a sabotaged COPY of the
    # runner out of the temp root, where the first lookup cannot resolve, so the guard clone has to
    # answer the second or that child dies at startup instead of reaching the ceiling it measures.
    New-Item -ItemType Directory -Path (Join-Path $guardClone 'ci') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'slot-lock.ps1') -Destination (Join-Path $guardClone 'ci/slot-lock.ps1') -Force
    $guardRefspecsBefore = (& git -C $guardClone config --get-all remote.origin.fetch 2>&1 | Out-String)
    Assert-True -Condition ($guardRefspecsBefore -match [regex]::Escape($guardBase) -and $guardRefspecsBefore -notmatch '\*') `
        -Message "CONTROL: the guard clone starts NARROW -- one branch-specific refspec and no wildcard (got '$($guardRefspecsBefore.Trim() -replace "`r?`n", ' // ')')"
    Push-Location $guardClone

    # HARNESS CONTROLS. If the fake accepted everything, every case below would pass against the
    # broken call too, and the suite would report health while measuring nothing.
    $exact = & cmd /c "`"$shimPath`" pr view 932 --json headRefName,headRefOid" 2>$null
    Assert-True -Condition ($LASTEXITCODE -eq 0 -and "$exact" -match 'headRefOid') `
        -Message 'CONTROL: the fake answers a vector with no --jq'
    $null = & cmd /c "`"$shimPath`" pr view 932 --json headRefName,headRefOid --jq .headRefName + x + .headRefOid" 2>$null
    Assert-True -Condition ($LASTEXITCODE -ne 0) `
        -Message 'CONTROL: the fake refuses a vector carrying --jq, as the real gh did'

    # ---------------------------------------------------------------------------------------------
    # The case: a queued entry whose head the fake server does not have.
    # ---------------------------------------------------------------------------------------------
    $entryHead = 'b' * 40
    $entry = [ordered]@{
        pr        = 932
        head      = $entryHead
        lane      = 'TESTS'
        timestamp = (Get-Date).ToUniversalTime().ToString('o')
    }
    $entryPath = Join-Path $queue "932-$($entryHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $entryPath -Value (ConvertTo-Json $entry) -Encoding UTF8

    $log = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1
    $code = $LASTEXITCODE
    $text = ($log | Out-String)

    Assert-True -Condition ($code -eq 0) -Message "the runner exits 0 (got $code)"
    Assert-True -Condition ($text -notmatch 'could not resolve pull request') `
        -Message 'the runner does not report the pull request as unresolvable'
    Assert-True -Condition ($text -match [regex]::Escape($serverHead.Substring(0, 8))) `
        -Message 'the log names the head the server resolved'

    $statusPath = [System.IO.Path]::ChangeExtension($entryPath, '.status')
    Assert-True -Condition (Test-Path -LiteralPath $statusPath) -Message 'a status file is written'
    $status = if (Test-Path -LiteralPath $statusPath) { (Get-Content -LiteralPath $statusPath -Raw) } else { '' }
    Assert-True -Condition ($status -match 'head moved') `
        -Message "the entry is dropped for a moved head, reachable only once the resolve succeeded (status: '$($status.Trim())')"

    # ---------------------------------------------------------------------------------------------
    # THE OLD BEHAVIOUR, PINNED. A fake that refuses stands in for a `gh` the runner calls wrongly:
    # the entry must stay QUEUED and say so, rather than being dropped or built.
    # ---------------------------------------------------------------------------------------------
    Set-Content -LiteralPath $shimPath -Value $refusingShim -Encoding ASCII
    # The case above DROPPED its entry, which is the point of it. Write a fresh one, or this case
    # would be measuring an empty queue and would pass on any code at all.
    Set-Content -LiteralPath $entryPath -Value (ConvertTo-Json $entry) -Encoding UTF8
    Remove-Item -LiteralPath $statusPath -Force -ErrorAction SilentlyContinue

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1
    Assert-True -Condition (Test-Path -LiteralPath $entryPath) `
        -Message 'an unresolvable pull request leaves the entry QUEUED, never dropped'
    $status2 = if (Test-Path -LiteralPath $statusPath) { (Get-Content -LiteralPath $statusPath -Raw) } else { '' }
    Assert-True -Condition ($status2 -match 'not resolvable') `
        -Message "and the status says so (status: '$($status2.Trim())')"

    # ---------------------------------------------------------------------------------------------
    # #902: a MERGED pull request is not a head that moved, and the head check cannot see it.
    # ---------------------------------------------------------------------------------------------
    $mergedHead = 'e' * 40
    $mergedShim = $perPrShim.Replace('BBBBBBBB', $mergedHead)
    Set-Content -LiteralPath $shimPath -Value $mergedShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force

    $mergedEntry = [ordered]@{ pr = 300; head = $mergedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $mergedPath = Join-Path $queue '300-eeeeeeee.json'
    Set-Content -LiteralPath $mergedPath -Value (ConvertTo-Json $mergedEntry) -Encoding UTF8
    $mergedTargetRoot = Join-Path $root 'merged-targets'
    New-Item -ItemType Directory -Path (Join-Path $mergedTargetRoot 'pr300') -Force | Out-Null

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state -TargetRoot $mergedTargetRoot 2>&1
    Assert-True -Condition (-not (Test-Path -LiteralPath $mergedPath)) `
        'a MERGED pull request is DROPPED rather than benched -- its branch still points at the enqueued head, so the head check passes'
    $mergedStatus = $(
        $sp = [System.IO.Path]::ChangeExtension($mergedPath, '.status')
        if (Test-Path -LiteralPath $sp) { Get-Content -LiteralPath $sp -Raw } else { '' })
    Assert-True -Condition ($mergedStatus -match 'MERGED') `
        "and the status names the state it was dropped for (status: '$($mergedStatus.Trim())')"
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $mergedTargetRoot 'pr300'))) `
        'and a MERGED drop removes only its explicit PR target cache'

    # ---------------------------------------------------------------------------------------------
    # #902: HEAD OF LINE. The older entry cannot be moved; the younger one must still be processed
    # in the SAME -Once invocation, or one lane's held branch stops the whole queue.
    # ---------------------------------------------------------------------------------------------
    Set-Content -LiteralPath $shimPath -Value $perPrShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force

    $stallPath = Join-Path $queue '100-bbbbbbbb.json'
    Set-Content -LiteralPath $stallPath -Value (ConvertTo-Json ([ordered]@{
        pr = 100; head = ('b' * 40); lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8
    # Created second, so it is YOUNGER and the ordering puts it behind the stalled one.
    Start-Sleep -Milliseconds 1100
    $movedPath = Join-Path $queue '200-bbbbbbbb.json'
    Set-Content -LiteralPath $movedPath -Value (ConvertTo-Json ([ordered]@{
        pr = 200; head = ('b' * 40); lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1

    Assert-True -Condition (-not (Test-Path -LiteralPath $movedPath)) `
        'HEAD OF LINE: the entry BEHIND an unmovable one is reached and processed in the same -Once run'
    Assert-True -Condition (Test-Path -LiteralPath $stallPath) `
        'and the unmovable entry is still queued -- skipped for this pass, not discarded'

    # ---------------------------------------------------------------------------------------------
    # #902: the bench failure says WHICH of its three causes it was.
    # ---------------------------------------------------------------------------------------------
    Set-Content -LiteralPath $shimPath -Value $perPrShim.Replace('BBBBBBBB', ('c' * 40)) -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force
    # 200 resolves to $serverHead; an entry naming that head passes the head check and then fails to
    # bench, because this repository does not have that object.
    $benchPath = Join-Path $queue '200-aaaaaaaa.json'
    Set-Content -LiteralPath $benchPath -Value (ConvertTo-Json ([ordered]@{
        pr = 200; head = $serverHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1
    $benchStatus = $(
        $sp = [System.IO.Path]::ChangeExtension($benchPath, '.status')
        if (Test-Path -LiteralPath $sp) { Get-Content -LiteralPath $sp -Raw } else { '' })
    # The assertion demands git's ACTUAL words, not merely that the message got longer. Written
    # loosely first -- "length > 60" -- it passed on the fallback text `git exited 128 and said
    # nothing`, which is the code saying it has no reason to give. A guard satisfied by the absence
    # of the thing it is guarding is the defect one level up, and this cell caught it in its own
    # subject before anyone else had to.
    Assert-True -Condition ($benchStatus -match 'bench could not be prepared --' -and $benchStatus -notmatch 'said nothing') `
        "the bench failure carries git's own words, so a reader knows which of the three causes it was (status: '$($benchStatus.Trim())')"


    # ---------------------------------------------------------------------------------------------
    # #1040: THE BENCH BRANCH TRACKS ORIGIN, and a branch that cannot is refused before the run.
    #
    # `git worktree add -B` creates the branch with NO upstream. `ci/gate.ps1` answers `pushed` by
    # asking the server named by `<branch>@{upstream}`, so a bench prepared that way publishes
    # `pushed: null` on every receipt and `ci/merge-proof.ps1` refuses all of them. The fix is one
    # `git branch --set-upstream-to origin/<branch> <branch>` after the add; this cell observes the
    # ARGUMENT VECTOR the callee receives, the same way the `gh` cases above do, because the property
    # is "which command, with which arguments, in which order".
    #
    # WHY A FAKE `git` AND NOT THE REAL ONE. A real add would check the whole tree out into the
    # bench, and a real success would carry on into a gate launch -- half an hour of cargo per
    # assertion. The fake answers `worktree add` with success WITHOUT creating anything, refuses
    # `--set-upstream-to` with the sentence the real git uses for a missing remote ref, and delegates
    # every other vector to the real binary. The refusal is what stops the runner before the launch,
    # so the success path (the add is followed by the tracking call) and the failure path (the
    # tracking call's refusal is the entry's status, and the entry stays queued) are both observable
    # from ONE run that spends nothing.
    # ---------------------------------------------------------------------------------------------
    Set-Content -LiteralPath $shimPath -Value $perPrShim.Replace('BBBBBBBB', ('c' * 40)) -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force
    # FIRST, not all: this machine has two git.exe on PATH and Get-Command returns both.
    $realGit = @(Get-Command -Name 'git.exe' -CommandType Application -ErrorAction Stop | Select-Object -First 1)[0].Source
    $vectorLog = Join-Path $root 'git-vectors.log'
    $gitShimPath = Join-Path $shimDirectory 'git.cmd'
    $gitShim = @"
@echo off
>>"$vectorLog" echo %*
if "%~3"=="fetch" exit /b 0
if "%~3"=="worktree" if "%~4"=="add" exit /b 0
if "%~3"=="branch" if "%~4"=="--set-upstream-to" (
  echo fatal: the requested upstream branch '%~5' does not exist 1>&2
  exit /b 128
)
"$realGit" %*
exit /b %ERRORLEVEL%
"@
    Set-Content -LiteralPath $gitShimPath -Value $gitShim -Encoding ASCII
    $trackEntryPath = Join-Path $queue '200-aaaaaaaa.json'
    Set-Content -LiteralPath $trackEntryPath -Value (ConvertTo-Json ([ordered]@{
        pr = 200; head = $serverHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8
    $trackBenches = Join-Path $root 'track-benches'
    $trackTargets = Join-Path $root 'track-targets'
    try {
        $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -TargetRoot $trackTargets -BenchRoot $trackBenches 2>&1
    } finally {
        # Removed HERE, not in the outer finally: every cell below must run against the real git.
        Remove-Item -LiteralPath $gitShimPath -Force -ErrorAction SilentlyContinue
    }
    $vectors = @(if (Test-Path -LiteralPath $vectorLog) { Get-Content -LiteralPath $vectorLog } else { @() })
    $addAt = -1; $trackAt = -1; $symbolicAt = -1
    for ($i = 0; $i -lt $vectors.Count; $i++) {
        # `-B` ATTACHED OR DETACHED. This branch passes `-B<branch>` as one word on purpose: handed
        # as a separate word, a branch named `--upload-pack=nope` is read by git as an option
        # (measured in the dash cell below). The property this cell owns is the ORDER of the calls,
        # not the spelling of that flag, so the pattern accepts both rather than failing on it.
        if ($addAt -lt 0 -and $vectors[$i] -match "worktree add -B ?$serverBranch ") { $addAt = $i }
        if ($trackAt -lt 0 -and $vectors[$i] -match "branch --set-upstream-to origin/$serverBranch $serverBranch$") { $trackAt = $i }
        if ($symbolicAt -lt 0 -and $vectors[$i] -match 'symbolic-ref') { $symbolicAt = $i }
    }
    # ORDER, from the callee's own log: the tracking call comes AFTER a successful add (it names the
    # bench, which does not exist before the add) and, when refused, NOTHING follows it -- the
    # detached-bench probe is the next call on the path, and it was never made.
    Assert-True -Condition ($addAt -ge 0 -and $trackAt -gt $addAt -and $symbolicAt -lt 0) `
        -Message "after a successful worktree add the runner calls git branch --set-upstream-to origin/$serverBranch $serverBranch, and a refusal there stops it before the next probe (add=$addAt, track=$trackAt, symbolic-ref=$symbolicAt)"
    $trackStatus = $(
        $sp = [System.IO.Path]::ChangeExtension($trackEntryPath, '.status')
        if (Test-Path -LiteralPath $sp) { Get-Content -LiteralPath $sp -Raw } else { '' })
    # git's OWN words, for the same reason the bench-failure cell demands them: a status that only
    # says "refused" would also be written by a runner that ran the command and discarded the answer.
    Assert-True -Condition ($trackStatus -match 'refused: bench branch has no upstream -- ' -and $trackStatus -match 'does not exist' -and $trackStatus -notmatch 'said nothing') `
        -Message "and the entry's status is the upstream refusal, carrying git's words (status: '$($trackStatus.Trim())')"
    Assert-True -Condition (Test-Path -LiteralPath $trackEntryPath) `
        -Message 'and the entry is STALLED -- still queued for the next pass, not dropped'

    # ---------------------------------------------------------------------------------------------
    # #951: THE FAILURE MODE OF THE HEAD-OF-LINE FIX MUST BE A FAILURE, NOT A HANG.
    #
    # X reviewed #951 by removing `-Exclude $stalled` from the selection -- the correct sabotage --
    # and the cell above did not go red: the child spun at $PollSeconds a turn and never returned.
    # Two of those ran for two hours on this machine. A red says the fix is gone; a hang says
    # nothing, looks like work in progress, and is only found by somebody auditing process trees.
    #
    # So this cell runs the sabotage ITSELF, against a COPY of the runner, and asserts the run ends.
    # The timeout is the assertion: without the ceiling this waits forever, and `WaitForExit` with a
    # bound is the only way a test can tell "still working" from "never finishing".
    # ---------------------------------------------------------------------------------------------
    Set-Content -LiteralPath $shimPath -Value $refusingShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force
    Set-Content -LiteralPath (Join-Path $queue '100-bbbbbbbb.json') -Value (ConvertTo-Json ([ordered]@{
        pr = 100; head = ('b' * 40); lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8

    $sabotaged = Join-Path $root 'gate-runner-without-the-exclusion.ps1'
    $runnerText = [System.IO.File]::ReadAllText($runner)
    $withExclusion = 'Get-NextEntry -Directory $QueueDirectory -Exclude $stalled'
    Assert-True -Condition ($runnerText.Contains($withExclusion)) `
        'ARRANGEMENT: the selection passes the skip list, so removing it is a sabotage and not a no-op'
    [System.IO.File]::WriteAllText($sabotaged,
        $runnerText.Replace($withExclusion, 'Get-NextEntry -Directory $QueueDirectory'))
    # The runner dot-sources `gate-passes.ps1` from beside itself (#1133), so the sabotaged copy
    # needs the sibling too. Without this the copy dies on the missing dependency and the cell
    # below reads exit 1 -- a dependency failure that would have been scored as the runner's
    # behaviour under sabotage.
    Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $runner) 'gate-passes.ps1') `
        -Destination (Join-Path (Split-Path -Parent $sabotaged) 'gate-passes.ps1') -Force

    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $sabotaged,
        '-Slot', 'HDD', '-SlotRoot', $slotRoot, '-Once',
        '-QueueDirectory', $queue, '-StateDirectory', $state, '-PollSeconds', '1')
    # -WorkingDirectory EXPLICITLY. `Start-Process` reads the .NET process cwd, which `Push-Location`
    # does not move; without this the sabotage child is the one cell that would still run in the
    # developer's clone, and it is the child that runs a MODIFIED runner.
    $spinner = Start-Process -FilePath 'powershell' -ArgumentList $arguments -PassThru -WindowStyle Hidden -WorkingDirectory $guardClone
    $ended = $spinner.WaitForExit(60000)
    if (-not $ended) {
        # Killed rather than left behind: this suite must not become the thing it is measuring.
        try { $spinner.Kill() } catch { }
    }
    Assert-True -Condition $ended `
        'with the skip list ignored the run ENDS rather than spinning -- a hang is not a red, and this is the cell that says so'
    Assert-True -Condition ($ended -and $spinner.ExitCode -eq 3) `
        "and it ends with 3, the code that says an entry is being re-picked (got $(if ($ended) { $spinner.ExitCode } else { 'no exit' }))"

    # #943: THE TARGET IS COLD BEFORE THE LAUNCH -- read from the file, and here is why it is not
    # driven as behaviour.
    #
    # Every case above reaches its assertion because the entry is DROPPED before a bench is prepared:
    # the fake server's head disagrees, the runner drops, and line for line it never reaches the
    # target allocation. Reaching it means agreeing with the fake, preparing a bench and STARTING A
    # REAL GATE -- half an hour of cargo per assertion, on the machine's only spinning disk, from a
    # suite that is supposed to cost seconds. So this is a source claim and says so, rather than
    # pretending to be a behavioural one.
    #
    # A CONTAINMENT CLAIM, NEVER A POSITION COMPARISON. `IndexOf(removal) -lt IndexOf(launch)` reads
    # as the same property and is not: file order is not execution order, and a removal moved into a
    # function defined earlier in the file would satisfy it while running after the gate started.
    # The claim that survives a sabotage is that the removal is INSIDE the region between the target
    # assignment and the launch. (#958 paid for this distinction one suite over.)
    $runnerText = [System.IO.File]::ReadAllText($runner)
    $assignAt = $runnerText.IndexOf('$target = Join-Path $TargetRoot', [System.StringComparison]::Ordinal)
    $launchAt = $runnerText.IndexOf('Start-Process powershell', [System.StringComparison]::Ordinal)

    # THE ARRANGEMENT IS ITS OWN BOOLEAN and is never inferred from the emptiness of the region: an
    # anchor that stopped matching gives `IndexOf` -1, and a region built from -1 is not "clean", it
    # is unmeasured. Two states must not share one representation.
    $anchorsFound = ($assignAt -ge 0 -and $launchAt -gt $assignAt)
    Assert-True -Condition $anchorsFound `
        -Message "ARRANGEMENT: the target assignment and the gate launch are both found, in that order (assign=$assignAt, launch=$launchAt)"

    $beforeLaunch = if ($anchorsFound) { $runnerText.Substring($assignAt, $launchAt - $assignAt) } else { '' }

    # #1053 REWROTE THIS CELL, AND THE REWRITE IS THE POINT. It used to read "the previous run's
    # target is REMOVED ... so a re-run is cold", which certified the unconditional delete. That
    # delete was #943's workaround for a TIMESTAMP-based freshness instrument, replaced on
    # 2026-09-16 by the content-based one (#904/#1038, extended #1007) -- so the sentence this cell
    # was asserting had become false about the repository's own intent, while the cell stayed green.
    # A cell whose message survives the change it was written to detect is a cell asserting the
    # defect; the claim is now the CONDITION, and the delete's continued existence is its sibling.
    Assert-True -Condition ($beforeLaunch -match 'Test-TargetBuildFinished -TargetDir \$target') `
        -Message 'the removal is GUARDED by Test-TargetBuildFinished, so a target whose last build finished and vouched survives into the next run of the same pull request (#1053)'

    Assert-True -Condition ($beforeLaunch -match 'Remove-Item[^\r\n]*\$target') `
        -Message 'and the removal itself is still there, so every other state -- unfinished, unvouched, unreadable, unknown -- is still deleted (#943)'

    # THE DELETE IS ON THE NEGATIVE BRANCH, stated as CONTAINMENT rather than as two offsets: the
    # region could hold the guard and the delete as independent statements and satisfy a pair of
    # `-match` cells while deleting unconditionally. What cannot be satisfied that way is the
    # delete sitting inside the `else` of the guard.
    Assert-True -Condition ($beforeLaunch -match '(?s)Test-TargetBuildFinished -TargetDir \$target.*?\}\s*else\s*\{.*?Remove-Item[^\r\n]*\$target') `
        -Message 'and it sits in the guard''s ELSE branch, so the two cells above cannot both pass on a runner that deletes whatever the guard answered'

    # AND IT SAYS SO IN THE LOG. Deleting tens of gigabytes takes minutes on the HDD, and a runner
    # that goes quiet for minutes with no line explaining why is indistinguishable from a wedged one
    # -- which is exactly the reading the liveness watchdog below is built to avoid making.
    Assert-True -Condition ($beforeLaunch -match 'Write-Note[^\r\n]*943') `
        -Message 'and the runner records that it did it, naming the issue, so a quiet minute in the log is explained rather than suspicious'

    # #1165: TARGETS ARE CACHE, RECEIPTS ARE EVIDENCE. The runner may remove its own target only
    # after the manifest push succeeds. These source checks are deliberately bounded to the caller
    # region: running a real gate here would make a focused PowerShell regression depend on cargo,
    # PostgreSQL and the machine's slot, while the ordering is the safety property itself.
    $publishFnAt = $runnerText.IndexOf('function Publish-Receipt', [System.StringComparison]::Ordinal)
    $pushAt = $runnerText.IndexOf('$push = Invoke-External', [System.StringComparison]::Ordinal)
    $publishGuardAt = $runnerText.IndexOf('    if ($pushed) {', [System.StringComparison]::Ordinal)
    $cleanupCallAt = $runnerText.IndexOf('Remove-PublishedRunnerTarget -TargetRoot', [System.StringComparison]::Ordinal)
    $statusAt = $runnerText.IndexOf('Set-EntryStatus -EntryPath $entryPath -State ("finished rc=', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($publishFnAt -ge 0 -and $pushAt -gt $publishFnAt -and $cleanupCallAt -gt $publishGuardAt -and $statusAt -gt $cleanupCallAt) `
        -Message '#1165: target cleanup is after the push and inside the successful-publication block, before the finished status'
    $invokeAt = $runnerText.IndexOf('function Invoke-OneEntry', [System.StringComparison]::Ordinal)
    $beforePush = if ($invokeAt -ge 0 -and $publishFnAt -gt $invokeAt) { $runnerText.Substring($invokeAt, $publishFnAt - $invokeAt) } else { '' }
    Assert-True -Condition (-not $beforePush.Contains('Remove-PublishedRunnerTarget')) `
        -Message '#1165: no target cleanup is reachable before the receipt push'
    $publishRegion = if ($publishGuardAt -ge 0 -and $statusAt -gt $publishGuardAt) { $runnerText.Substring($publishGuardAt, $statusAt - $publishGuardAt) } else { '' }
    Assert-True -Condition ($publishRegion.Contains('Remove-PublishedRunnerTarget') -and $publishRegion.Contains('Resolve-PullRequestBranch')) `
        -Message '#1165: a successful push re-reads PR state before target cleanup'
    $receiptAt = $runnerText.IndexOf('$receiptTip = Get-ReceiptCommitProof', [System.StringComparison]::Ordinal)
    $receiptRegion = if ($receiptAt -ge 0 -and $publishGuardAt -gt $receiptAt) { $runnerText.Substring($receiptAt, $publishGuardAt - $receiptAt) } else { '' }
    Assert-True -Condition ($receiptRegion.Contains('Get-ReceiptCommitProof -BenchPath $BenchPath -ExpectedParent $ExpectedParent')) `
        -Message '#1165: target cleanup requires a receipt commit directly on the queued head'
    Assert-True -Condition ($receiptRegion.Contains("'ls-remote', 'origin'") -and $receiptRegion.Contains('refs/heads/$Branch')) `
        -Message '#1165: target cleanup requires the expected receipt commit to be visible on the server'
    Assert-True -Condition ($runnerText.Contains('Write-Note "receipt push returned 0') -and $runnerText.Contains('retaining target')) `
        -Message '#1165: a successful push with the wrong server head retains the target'
    Assert-True -Condition ($runnerText.Contains('if (-not (Test-HeadSha $tipSha) -or $tipSha -eq $ExpectedParent)')) `
        -Message '#1165: no-op or child-crash runs with no new receipt commit cannot authorize cleanup'
    Assert-True -Condition ($runnerText.Contains("'--name-status', '-r', `$tipSha") -and $runnerText.Contains('$parts[0] -cne ''A''')) `
        -Message '#1165: receipt proof accepts only add-only receipt commits'
    Assert-True -Condition ($publishRegion -match 'if \(\$pushed\) \{[\s\S]*Remove-PublishedRunnerTarget') `
        -Message '#1165: a failed receipt push cannot enter the target-removal branch, so the target remains as the backstop'
    Assert-True -Condition ($runnerText.Contains('function Remove-PublishedRunnerTarget') -and $runnerText.Contains('build cache is not evidence')) `
        -Message '#1165: the cleanup helper names the evidence boundary and owns only the PR-derived target path'
    Assert-True -Condition ($runnerText.Contains('if ($State -ne ''MERGED'' -and $State -ne ''CLOSED'') { return }')) `
        -Message '#1165: bench cleanup is limited to pull requests explicitly observed as MERGED or CLOSED'
    Assert-True -Condition ($runnerText.Contains('Test-BenchIsRegistered -RepositoryRoot $RepositoryRoot -BenchPath $bench -FailClosed')) `
        -Message '#1165: terminal cleanup uses the fail-closed ownership check for the exact worktree path'
    # THIS CELL USED TO ASSERT THE ABSENCE OF --force, AND THAT IS WHY IT HAD TO CHANGE (#1085).
    # "No --force" was standing in for "cannot destroy work", and on this machine the two came
    # apart: `git worktree remove` refuses on a modified file, the gate rewrites the tracked canary
    # in every bench it runs in (#152), and so the rule retained three of the four real terminal
    # benches measured on 2026-09-20 while seeing none of the untracked file or the unpushed commit
    # it was supposed to be protecting. A test line that has to change was asserting the defect;
    # the property now pinned is the one that was meant all along -- the force is REACHED ONLY
    # THROUGH the discriminator, and the discriminator's refusal returns before it.
    $forcedBenchRemove = "'worktree', 'remove', '--force', `$bench) -CaptureError"
    $benchHelperAt = $runnerText.IndexOf('function Remove-TerminalRunnerBench', [System.StringComparison]::Ordinal)
    $benchHelperEnd = $runnerText.IndexOf('function Remove-TerminalRunnerTarget', [System.StringComparison]::Ordinal)
    $benchHelper = if ($benchHelperAt -ge 0 -and $benchHelperEnd -gt $benchHelperAt) { $runnerText.Substring($benchHelperAt, $benchHelperEnd - $benchHelperAt) } else { '' }
    $benchGuardAt = $benchHelper.IndexOf('$safety = Test-BenchRemovalIsSafe -BenchPath $bench', [System.StringComparison]::Ordinal)
    $benchRefusalAt = $benchHelper.IndexOf('if (-not $safety.Safe) {', [System.StringComparison]::Ordinal)
    $benchForceAt = $benchHelper.IndexOf($forcedBenchRemove, [System.StringComparison]::Ordinal)
    Assert-True -Condition ($benchGuardAt -ge 0 -and $benchRefusalAt -gt $benchGuardAt -and $benchForceAt -gt $benchRefusalAt -and
            ([regex]::Matches($benchHelper, [regex]::Escape($forcedBenchRemove))).Count -eq 1) `
        -Message '#1085: terminal bench cleanup reaches its single --force removal only past the discriminator and its early return, so the force is licensed by a measured answer rather than by a terminal state alone'
    Assert-True -Condition ($runnerText.Contains('WARNING $target could not be removed after the receipt push; the target remains as a backstop')) `
        -Message '#1165: a target that cannot be removed is retained and reported after a successful push'
    Assert-True -Condition ($runnerText.Contains('terminal pull request #$PullRequest has an unregistered bench') -and $runnerText.Contains('bench was retained')) `
        -Message '#1165: a terminal PR never authorizes deleting an unowned or busy bench'
    $terminalDropAt = $runnerText.IndexOf("if (`$resolvedPr.State -eq 'MERGED' -or `$resolvedPr.State -eq 'CLOSED')", [System.StringComparison]::Ordinal)
    $terminalBenchCallAt = $runnerText.IndexOf('Remove-TerminalRunnerBench -RepositoryRoot $repoRoot -BenchRoot $BenchRoot', $terminalDropAt, [System.StringComparison]::Ordinal)
    $terminalTargetCallAt = $runnerText.IndexOf('Remove-TerminalRunnerTarget -TargetRoot $TargetRoot -PullRequest', $terminalBenchCallAt, [System.StringComparison]::Ordinal)
    Assert-True -Condition ($terminalDropAt -ge 0 -and $terminalBenchCallAt -gt $terminalDropAt -and $terminalTargetCallAt -gt $terminalBenchCallAt) `
        -Message '#1165: the real MERGED/CLOSED drop path calls safe bench cleanup and explicit target cleanup in order'

    # ---------------------------------------------------------------------------------------------
    # #1085 PART A and PART B: the runner reaps its OWN terminal benches and targets, and the gate's
    # own canary dirt stops retaining them.
    #
    # WHY THIS BLOCK EXISTS. AGENTS.md says the runner "removes its own benches under -BenchRoot once
    # their pull request is no longer open". Measured on this tree, the only callers of
    # `Remove-TerminalRunnerBench`/`Remove-TerminalRunnerTarget` sit inside `Invoke-OneEntry`'s
    # MERGED/CLOSED queue-drop path, which runs only when a queue entry for that pull request is
    # SELECTED AFTER it became terminal. The ordinary lifecycle -- run publishes its receipt, entry is
    # deleted, pull request merges later -- never re-enters it, so bench and target survive forever.
    #
    # AND WHY THE OBVIOUS REAPER WOULD HAVE BEEN INERT. `Remove-TerminalRunnerBench` calls
    # `git worktree remove` WITHOUT --force, and EVERY bench a gate has ever run in is permanently
    # modified in exactly one tracked file: `ci/gate.ps1` builds `tools/ci-canary/src/nonce.rs` at two
    # sites (:1326 and :2562 here) and `Write-CanaryNonce` rewrites it every run by design (#152).
    # Measured 2026-09-20 on the four real runner benches for merged pull requests,
    # `git -C <bench> status --porcelain`:
    #   D:/runner-hdd-claude/pr1169  ->  " M tools/ci-canary/src/nonce.rs"
    #   C:/runner-c-claude/pr1174    ->  " M tools/ci-canary/src/nonce.rs"
    #   C:/runner-c-claude/pr1169    ->  " M tools/ci-canary/src/nonce.rs"
    #   D:/runner-ssd-claude/pr1169  ->  clean -- and this is the ONLY one the un-forced remove took.
    # Zero unpushed commits in each. So a reaper reusing that function unchanged would walk every
    # directory, decide correctly that each is terminal, and remove almost none of them -- landing the
    # same "the document says it cleans up, the code does not" defect it exists to remove. The cell
    # marked THE INERT ONE below is the one that fails for exactly that reason before this change.
    $reapRoot = Join-Path $root 'reap'
    $reapUpstream = Join-Path $reapRoot 'upstream'
    $reapClone = Join-Path $reapRoot 'clone'
    $reapBenchRoot = Join-Path $reapRoot 'benches'
    $reapTargetRoot = Join-Path $reapRoot 'targets'
    $null = New-Item -ItemType Directory -Path $reapUpstream -Force
    $null = New-Item -ItemType Directory -Path $reapBenchRoot -Force
    $null = New-Item -ItemType Directory -Path $reapTargetRoot -Force
    & git -C $reapUpstream init --initial-branch=main --quiet 2>&1 | Out-Null
    & git -C $reapUpstream config user.email 'suite@example.invalid' 2>&1 | Out-Null
    & git -C $reapUpstream config user.name 'gate-runner suite' 2>&1 | Out-Null
    & git -C $reapUpstream config commit.gpgsign false 2>&1 | Out-Null
    $null = New-Item -ItemType Directory -Path (Join-Path $reapUpstream 'tools\ci-canary\src') -Force
    # THE FIXTURE CARRIES THE REAL CANARY PATH, not a stand-in, because the property under test is
    # "this exact path is the gate's own artefact" and a stand-in would pass a predicate hard-coding
    # the wrong name.
    [System.IO.File]::WriteAllText((Join-Path $reapUpstream 'tools\ci-canary\src\nonce.rs'), "pub const NONCE: &str = 'seed';`n")
    [System.IO.File]::WriteAllText((Join-Path $reapUpstream 'README.md'), "seed`n")
    & git -C $reapUpstream add -A 2>&1 | Out-Null
    & git -C $reapUpstream commit -q -m 'seed' 2>&1 | Out-Null
    foreach ($n in 701, 702, 703, 704, 705, 708, 711) { & git -C $reapUpstream branch "feat$n" 2>&1 | Out-Null }
    & git clone --quiet $reapUpstream $reapClone 2>&1 | Out-Null
    & git -C $reapClone config user.email 'suite@example.invalid' 2>&1 | Out-Null
    & git -C $reapClone config user.name 'gate-runner suite' 2>&1 | Out-Null
    & git -C $reapClone config commit.gpgsign false 2>&1 | Out-Null
    $reapBenches = @{}
    foreach ($n in 701, 702, 703, 704, 705, 708) {
        $reapBenches[$n] = Join-Path $reapBenchRoot "pr$n"
        & git -C $reapClone worktree add --quiet -b "feat$n" $reapBenches[$n] "origin/feat$n" 2>&1 | Out-Null
    }
    foreach ($n in 701, 702, 703, 704, 705, 706, 707, 708) {
        $null = New-Item -ItemType Directory -Path (Join-Path $reapTargetRoot "pr$n") -Force
        [System.IO.File]::WriteAllText((Join-Path $reapTargetRoot "pr$n\libthing.rlib"), 'not empty')
    }
    # NAMES THAT ARE NOT pr<digits>, UNDER BOTH ROOTS -- AND THEY HAVE TO LOOK LIKE THE SUBJECT.
    # The original control here was `scratch-notes`, which shares no form with what it excludes:
    # loosening the anchored `^pr(\d+)$` to a bare `pr(\d+)` reddened nothing, because
    # `scratch-notes` fails EVERY spelling of the pattern. A control must be a NEAR MISS, so each of
    # these fails the real pattern on exactly one anchor and passes without it:
    #   pr1169-backup  -- a real directory shape on this machine (someone's copy of a bench), caught
    #                     only by the trailing `$`; an unanchored search matches `pr1169` inside it
    #                     and the reaper would force-remove somebody's backup.
    #   old-pr42       -- caught only by the leading `^`; an unanchored search matches `pr42` and the
    #                     reaper would read a number out of the MIDDLE of a name it never wrote.
    #   pr12x          -- caught by `$` again, and the one whose damage is quietest: it would reap
    #                     against pull request 12.
    # `scratch-notes` stays as the far control, so the cell still says something about a name that
    # shares no form at all.
    $reapNonCandidates = 'scratch-notes', 'pr1169-backup', 'old-pr42', 'pr12x'
    foreach ($nonCandidate in $reapNonCandidates) {
        $null = New-Item -ItemType Directory -Path (Join-Path $reapBenchRoot $nonCandidate) -Force
        $null = New-Item -ItemType Directory -Path (Join-Path $reapTargetRoot $nonCandidate) -Force
    }

    # 701: dirty by the GATE'S OWN canary and nothing else -- the population every real bench is in.
    [System.IO.File]::WriteAllText((Join-Path $reapBenches[701] 'tools\ci-canary\src\nonce.rs'), "pub const NONCE: &str = 'run-4242';`n")
    # 702: dirty by a DIFFERENT tracked file. Somebody's work; never removable.
    [System.IO.File]::WriteAllText((Join-Path $reapBenches[702] 'README.md'), "someone was editing this`n")
    # 703: an UNTRACKED file. A porcelain filter written only for ` M` would miss this whole column.
    [System.IO.File]::WriteAllText((Join-Path $reapBenches[703] 'notes.txt'), "unpushed thinking`n")
    # 704: CLEAN working tree but a commit the server does not have. The dirt test alone says remove;
    # only the "nothing ahead of the remote" half retains it, and losing it loses the commit.
    [System.IO.File]::WriteAllText((Join-Path $reapBenches[704] 'README.md'), "local work`n")
    & git -C $reapBenches[704] add -A 2>&1 | Out-Null
    & git -C $reapBenches[704] commit -q -m 'local commit origin does not have' 2>&1 | Out-Null
    # 705: clean, and its pull request is OPEN. This is why the reaper may run BEFORE selection.
    # 708: the canary path, the canary's own content -- and STAGED. `Write-CanaryNonce` writes the
    # file and never touches the index, so an 'M ' or 'MM' canary is NOT the gate's doing, and the
    # excuse is one column wide as well as one path wide. Until this fixture existed the column half
    # of the predicate was asserted by nothing: deleting `$index -eq ' ' -and $worktree -eq 'M'`
    # left the suite fully green, because every other fixture differs from the canary by its PATH.
    [System.IO.File]::WriteAllText((Join-Path $reapBenches[708] 'tools/ci-canary/src/nonce.rs'), "pub const NONCE: &str = 'staged-by-a-person';`n")
    & git -C $reapBenches[708] add 'tools/ci-canary/src/nonce.rs' 2>&1 | Out-Null

    # THE MEASUREMENT THAT MAKES PART A NECESSARY, RE-DERIVED HERE ON A FIXTURE rather than quoted
    # from a brief: git's un-forced `worktree remove` refuses a bench whose only difference is the
    # canary. This cell is green before and after the change; it is the control that says the
    # discriminator below is solving a real refusal and not an imagined one.
    $unforced = & git -C $reapClone worktree remove $reapBenches[701] 2>&1 | Out-String
    Assert-True -Condition ($LASTEXITCODE -ne 0 -and (Test-Path -LiteralPath $reapBenches[701]) -and $unforced -match 'modified or untracked') `
        -Message "CONTROL: an un-forced ``git worktree remove`` REFUSES a bench dirty only by the gate's own canary, so reusing it in a reaper would clean almost nothing (git said: $($unforced.Trim() -replace "`r?`n", ' // '))"

    $reapDiscriminatorMatch = [regex]::Match($runnerText, '(?ms)^function Test-BenchRemovalIsSafe \{.*?^\}')
    $reapCanaryConstMatch = [regex]::Match($runnerText, '(?m)^\$script:GateOwnCanaryPath = ''([^'']+)''')
    $reapBenchFnMatch = [regex]::Match($runnerText, '(?ms)^function Remove-TerminalRunnerBench \{.*?^\}')
    $reapTargetFnMatch = [regex]::Match($runnerText, '(?ms)^function Remove-TerminalRunnerTarget \{.*?^\}')
    $reapFnMatch = [regex]::Match($runnerText, '(?ms)^function Invoke-TerminalRunnerReap \{.*?^\}')
    # The three the reaper leans on. Loaded from the SAME text for the same reason as the rest: a
    # hand-written stand-in for `Test-BenchIsRegistered` would quietly answer "registered" and turn
    # every cell below into a measurement of the stand-in.
    # AND THE CONCURRENCY GUARD, loaded HERE rather than beside the cells that measure it: the
    # ordinary pass below already consults `Test-PullRequestHasLiveWorker`, and an UNDEFINED
    # predicate is swallowed by the reaper's per-candidate try/catch and read as "skip" -- correct
    # in production, and in a suite a hole that would leave every removal cell below vacuously green
    # about a guard nobody had loaded.
    $reapGuardMatch = [regex]::Match($runnerText, '(?ms)^function Test-PullRequestHasLiveWorker \{.*?^\}')
    $reapStampMatch = [regex]::Match($runnerText, '(?ms)^function Get-RunnerProcessStamp \{.*?^\}')
    $reapClaimWriteMatch = [regex]::Match($runnerText, '(?ms)^function Set-RunnerWorkClaim \{.*?^\}')
    $reapPrefixMatch = [regex]::Match($runnerText, '(?m)^\$script:RunnerClaimPrefix = ''([^'']+)''')
    $reapNoteMatch = [regex]::Match($runnerText, '(?ms)^function Write-Note \{.*?^\}')
    $reapExternalMatch = [regex]::Match($runnerText, '(?ms)^function Invoke-External \{.*?^\}')
    $reapRegisteredMatch = [regex]::Match($runnerText, '(?ms)^function Test-BenchIsRegistered \{.*?^\}')
    $reapGuardLoadable = $reapGuardMatch.Success -and $reapStampMatch.Success -and $reapClaimWriteMatch.Success -and $reapPrefixMatch.Success
    $reapLoadable = $reapDiscriminatorMatch.Success -and $reapCanaryConstMatch.Success -and
        $reapBenchFnMatch.Success -and $reapTargetFnMatch.Success -and $reapFnMatch.Success -and
        $reapNoteMatch.Success -and $reapExternalMatch.Success -and $reapRegisteredMatch.Success -and
        $reapGuardLoadable
    Assert-True -Condition $reapLoadable `
        -Message 'ARRANGEMENT: the canary constant, the claim prefix, the removal discriminator, the concurrency guard and its process stamp, both removers, the reaper and the three helpers they call are all locatable in gate-runner.ps1, so the cells below drive the real ones and not a copy'
    if ($reapLoadable) {
        . ([scriptblock]::Create($reapNoteMatch.Value))
        . ([scriptblock]::Create($reapExternalMatch.Value))
        . ([scriptblock]::Create($reapRegisteredMatch.Value))
        . ([scriptblock]::Create($reapCanaryConstMatch.Value))
        . ([scriptblock]::Create($reapDiscriminatorMatch.Value))
        . ([scriptblock]::Create($reapBenchFnMatch.Value))
        . ([scriptblock]::Create($reapTargetFnMatch.Value))
        . ([scriptblock]::Create($reapFnMatch.Value))
        . ([scriptblock]::Create($reapPrefixMatch.Value))
        . ([scriptblock]::Create($reapStampMatch.Value))
        . ([scriptblock]::Create($reapClaimWriteMatch.Value))
        . ([scriptblock]::Create($reapGuardMatch.Value))
    }
    # THE CONSTANT IS ROUND-TRIPPED AGAINST gate.ps1, for the same reason the build-state marker is:
    # two copies of a filename drift in silence, and a drift here would make the discriminator retain
    # every bench forever -- safe, and invisible.
    $gateTextForCanary = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate.ps1'))
    Assert-True -Condition ($reapCanaryConstMatch.Success -and
            $gateTextForCanary.IndexOf("`$gateOwnPath = '$($reapCanaryConstMatch.Groups[1].Value)'", [System.StringComparison]::Ordinal) -ge 0) `
        -Message "the runner's one canary constant names the same path gate.ps1 already excuses ('$(if ($reapCanaryConstMatch.Success) { $reapCanaryConstMatch.Groups[1].Value } else { 'UNREADABLE' })'), so the two cannot drift into a reaper that retains everything"

    $reapStates = @{ 701 = 'MERGED'; 702 = 'MERGED'; 703 = 'CLOSED'; 704 = 'MERGED'; 705 = 'OPEN'; 706 = ''; 708 = 'MERGED' }
    $reapResolver = {
        param([string] $PullRequest)
        $key = 0
        if (-not [int]::TryParse($PullRequest, [ref] $key)) { return $null }
        if (-not $reapStates.ContainsKey($key)) { return $null }
        return [pscustomobject]@{ Branch = "feat$key"; Head = ('a' * 40); State = $reapStates[$key]; BaseRef = 'main'; BaseSha = ('b' * 40) }
    }

    $reapSummary = if ($reapLoadable) {
        Invoke-TerminalRunnerReap -RepositoryRoot $reapClone -BenchRoot $reapBenchRoot -TargetRoot $reapTargetRoot -Resolver $reapResolver
    } else { $null }

    # THE INERT ONE. Before this change there is no discriminator, the reaper falls back on an
    # un-forced `git worktree remove`, and the cell above proves git refuses it. A reaper that is
    # green here is a reaper that actually reclaims the disk it was written for.
    Assert-True -Condition (-not (Test-Path -LiteralPath $reapBenches[701])) `
        -Message 'THE INERT ONE: a MERGED bench dirty ONLY by the gate''s own canary is REMOVED -- the case every real bench on this machine is in'
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $reapTargetRoot 'pr701'))) `
        -Message 'and its terminal target goes with it, from the same pass'
    Assert-True -Condition (Test-Path -LiteralPath $reapBenches[702]) `
        -Message 'a MERGED bench carrying a modification to ANY OTHER tracked path is RETAINED -- that is somebody''s work, and the canary excuse is one named path wide'
    Assert-True -Condition (Test-Path -LiteralPath $reapBenches[703]) `
        -Message 'a CLOSED bench holding an UNTRACKED file is RETAINED, so the porcelain filter reads the ?? column and not only the modified one'
    Assert-True -Condition (Test-Path -LiteralPath $reapBenches[704]) `
        -Message 'a MERGED bench whose tree is clean but which holds a commit origin does not have is RETAINED -- the dirt test alone would have destroyed that commit'
    Assert-True -Condition ((Test-Path -LiteralPath $reapBenches[705]) -and (Test-Path -LiteralPath (Join-Path $reapTargetRoot 'pr705'))) `
        -Message 'an OPEN pull request''s bench AND target both survive the pass, which is what makes running the reaper before selection safe'
    Assert-True -Condition (Test-Path -LiteralPath $reapBenches[708]) `
        -Message 'a MERGED bench whose ONLY change is the canary path but STAGED is RETAINED -- the gate writes that file and never touches the index, so a staged canary is a person''s edit and the excuse is one column wide as well as one path wide'
    $reapSurvivors = @($reapNonCandidates | Where-Object {
            (Test-Path -LiteralPath (Join-Path $reapBenchRoot $_)) -and (Test-Path -LiteralPath (Join-Path $reapTargetRoot $_))
        })
    # AND THE COUNT, WHICH IS THE HALF THAT ACTUALLY BITES. Survival of `pr1169-backup` is NOT
    # sensitive to the anchors: the reaper never removes the directory it matched, it builds
    # `pr<captured>` and removes THAT, so a lost anchor leaves the near-miss directory standing while
    # quietly admitting 1169, 42 and 12 as candidates -- numbers this script never wrote, each
    # costing a `gh pr view` and each licensing a force-removal of somebody else's `pr1169`. Measured:
    # with `^pr(\d+)$` the pass finds the eight real directories (701-708); with a bare `pr(\d+)` it
    # finds eleven. The candidate COUNT is where the anchors are observable, so the cell reads it.
    $reapExpectedCandidates = 8
    Assert-True -Condition ($reapSurvivors.Count -eq $reapNonCandidates.Count -and
            $reapSummary -and $reapSummary.Found -eq $reapExpectedCandidates) `
        -Message "a directory under either root whose name is not pr<digits> is never even a CANDIDATE -- including the NEAR MISSES a pattern without its anchors would match ($($reapNonCandidates -join ', ')): the pass found $(if ($reapSummary) { $reapSummary.Found } else { 'n/a' }) candidates against the $reapExpectedCandidates real pr<N> directories, and all $($reapSurvivors.Count) of $($reapNonCandidates.Count) near misses are untouched"
    Assert-True -Condition ((Test-Path -LiteralPath (Join-Path $reapTargetRoot 'pr706')) -and (Test-Path -LiteralPath (Join-Path $reapTargetRoot 'pr707'))) `
        -Message 'an EMPTY state (706) and an unresolvable pull request (707) are both SKIPPED rather than read as terminal -- the same choice the queue-drop path makes, for the same reason'

    # ---------------------------------------------------------------------------------------------
    # #1085: THE DETACHED ARM OF THE COMMIT HALF, WHICH NO CELL ABOVE REACHES.
    #
    # `Test-BenchRemovalIsSafe` answers the commit half two different ways. A bench on a BRANCH is
    # compared against `refs/remotes/origin/<branch>` -- fixture 704 owns that arm. A DETACHED bench
    # must instead have its HEAD reachable from some remote-tracking branch, and every bench 701-708
    # is on a branch, so the detached arm was asserted by NOTHING. Measured: replacing
    # `if ($contains.Code -ne 0 -or $reachable.Count -eq 0)` with `if ($false)` -- which makes the
    # reachability question answer "yes" when it cannot be answered at all -- left the suite at
    # PASSED: 192 of 192, while a detached bench carrying an unpushed commit flipped from retained to
    # force-removed.
    #
    # IT IS NOT AN EXOTIC SHAPE. `git worktree add <path> origin/<branch>` lands DETACHED by default,
    # and the runners on this machine detach a bench routinely to free its branch for the next run.
    #
    # THESE CELLS DRIVE THE PREDICATE DIRECTLY rather than through a reap pass, because the property
    # is the predicate's answer and its SENTENCE; the pass above already proves the wiring that calls
    # it. They live on their OWN clones so that one of them can break `git branch` for its whole
    # clone without perturbing the eight benches above.
    $reapDetachedRoot = Join-Path $reapRoot 'detached'
    $reapDetachedClone = Join-Path $reapDetachedRoot 'clone'
    $reapDetachedBenchRoot = Join-Path $reapDetachedRoot 'benches'
    $null = New-Item -ItemType Directory -Path $reapDetachedBenchRoot -Force
    & git clone --quiet $reapUpstream $reapDetachedClone 2>&1 | Out-Null
    & git -C $reapDetachedClone config user.email 'suite@example.invalid' 2>&1 | Out-Null
    & git -C $reapDetachedClone config user.name 'gate-runner suite' 2>&1 | Out-Null
    & git -C $reapDetachedClone config commit.gpgsign false 2>&1 | Out-Null
    $reapDetachedBenches = @{}
    # 721: DETACHED, canary-dirty, and its HEAD is exactly what `origin/feat711` points at -- the
    # POSITIVE half of the arm, so the pair below is not one-sided and a predicate that simply
    # retained every detached bench would be caught here.
    # 722: DETACHED, canary-dirty, and carrying one committed-but-unpushed file. This is the cell the
    # sabotage reddens: lose the retain and that commit goes with the directory.
    foreach ($n in 721, 722) {
        $reapDetachedBenches[$n] = Join-Path $reapDetachedBenchRoot "pr$n"
        & git -C $reapDetachedClone worktree add --quiet --detach $reapDetachedBenches[$n] 'origin/feat711' 2>&1 | Out-Null
    }
    [System.IO.File]::WriteAllText((Join-Path $reapDetachedBenches[722] 'unpushed.txt'), "an hour of work`n")
    & git -C $reapDetachedBenches[722] add 'unpushed.txt' 2>&1 | Out-Null
    & git -C $reapDetachedBenches[722] commit -q -m 'detached commit no remote has' 2>&1 | Out-Null
    # 724: on a LOCAL BRANCH with no `origin/feat724` at all -- the BRANCH arm's own failing-safe
    # path, which was as unasserted as the detached one: fixture 704 proves "one commit ahead"
    # retains, and nothing proved that a MISSING remote ref retains rather than reading as zero.
    $reapDetachedBenches[724] = Join-Path $reapDetachedBenchRoot 'pr724'
    & git -C $reapDetachedClone worktree add --quiet -b 'feat724' $reapDetachedBenches[724] 'origin/feat711' 2>&1 | Out-Null
    foreach ($n in 721, 722, 724) {
        [System.IO.File]::WriteAllText((Join-Path $reapDetachedBenches[$n] 'tools\ci-canary\src\nonce.rs'), "pub const NONCE: &str = 'run-$n';`n")
    }
    # 723: DETACHED and canary-dirty, in a clone where the reachability question CANNOT BE ANSWERED.
    # The mechanism is legal but wrong, and it reaches exactly the decision point: an unknown
    # `branch.sort` field makes `git branch` itself exit 128 while `git status` and
    # `git symbolic-ref` are untouched, so the bench arrives at the detached arm with a FAILED probe
    # rather than an empty one. Failing SAFE is the property; a lucky answer is not.
    $reapUnanswerableClone = Join-Path $reapDetachedRoot 'unanswerable-clone'
    & git clone --quiet $reapUpstream $reapUnanswerableClone 2>&1 | Out-Null
    $reapDetachedBenches[723] = Join-Path $reapDetachedBenchRoot 'pr723'
    & git -C $reapUnanswerableClone worktree add --quiet --detach $reapDetachedBenches[723] 'origin/feat711' 2>&1 | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $reapDetachedBenches[723] 'tools\ci-canary\src\nonce.rs'), "pub const NONCE: &str = 'run-723';`n")
    & git -C $reapUnanswerableClone config branch.sort 'no-such-field-for-git-to-sort-by' 2>&1 | Out-Null
    $reapProbeCode = 'n/a'
    $reapProbeFailed = $false
    if ($reapLoadable) {
        $reapProbeCheck = Invoke-External 'git' @('-C', $reapDetachedBenches[723], 'branch', '--remotes', '--contains', 'HEAD') -CaptureError
        $reapProbeCode = $reapProbeCheck.Code
        $reapProbeFailed = ($reapProbeCheck.Code -ne 0)
    }
    # A THROW IS A FAILED CELL, NOT AN ABORTED SUITE. With the retain removed, the arm walks on into
    # `$reachable[0].Trim()` over an empty array, which terminates under this file's
    # $ErrorActionPreference; caught here, that reads as the red it is.
    $reapSafety = {
        param([string] $Path)
        try { return Test-BenchRemovalIsSafe -BenchPath $Path }
        catch { return [pscustomobject]@{ Safe = $null; Reason = "THREW: $($_.Exception.Message)" } }
    }
    $reapDetachedVerdicts = @{}
    foreach ($n in 721, 722, 723, 724) {
        $reapDetachedVerdicts[$n] = if ($reapLoadable) { & $reapSafety $reapDetachedBenches[$n] } else { [pscustomobject]@{ Safe = $null; Reason = 'the discriminator was not loadable' } }
    }
    Assert-True -Condition ($reapProbeFailed) `
        -Message "CONTROL: in the 723 clone ``git branch --remotes --contains HEAD`` really does FAIL (exit $reapProbeCode), so that cell measures an unanswerable question and not an empty answer"
    Assert-True -Condition ($reapDetachedVerdicts[721].Safe -eq $true) `
        -Message "a MERGED bench on a DETACHED HEAD, dirty only by the gate's own canary, whose commit a remote-tracking branch DOES contain is REMOVABLE -- the positive half of the detached arm (said: Safe=$($reapDetachedVerdicts[721].Safe) :: $($reapDetachedVerdicts[721].Reason))"
    Assert-True -Condition ($reapDetachedVerdicts[722].Safe -eq $false -and
            $reapDetachedVerdicts[722].Reason -match 'detached and sits on no remote-tracking branch') `
        -Message "a DETACHED canary-dirty bench carrying a commit NO remote-tracking branch contains is RETAINED -- force-removing it loses that commit (said: Safe=$($reapDetachedVerdicts[722].Safe) :: $($reapDetachedVerdicts[722].Reason))"
    Assert-True -Condition ($reapDetachedVerdicts[723].Safe -eq $false -and
            $reapDetachedVerdicts[723].Reason -match 'detached and sits on no remote-tracking branch') `
        -Message "a DETACHED canary-dirty bench whose reachability question git REFUSES to answer is RETAINED -- an unanswerable question is never permission to force-delete somebody's worktree (said: Safe=$($reapDetachedVerdicts[723].Safe) :: $($reapDetachedVerdicts[723].Reason))"
    Assert-True -Condition ($reapDetachedVerdicts[724].Safe -eq $false -and
            $reapDetachedVerdicts[724].Reason -match 'no refs/remotes/origin/feat724') `
        -Message "and the BRANCH arm fails safe the same way: a canary-dirty bench on a branch this clone has no origin/<branch> for is RETAINED rather than counted as zero commits ahead (said: Safe=$($reapDetachedVerdicts[724].Safe) :: $($reapDetachedVerdicts[724].Reason))"


    # THE CEILING, ON ITS OWN ROOTS so that bounding the pass cannot perturb the cells above. A silent
    # cap reads as "nothing to clean", which is the failure this returns a flag for rather than only
    # logging.
    $reapCapRoot = Join-Path $reapRoot 'capped'
    $null = New-Item -ItemType Directory -Path $reapCapRoot -Force
    foreach ($n in 801, 802, 803, 804) {
        $null = New-Item -ItemType Directory -Path (Join-Path $reapCapRoot "pr$n") -Force
    }
    $reapCapResolver = { param([string] $PullRequest) return [pscustomobject]@{ Branch = 'x'; Head = ('a' * 40); State = 'OPEN'; BaseRef = 'main'; BaseSha = ('b' * 40) } }
    $reapCapped = if ($reapLoadable) {
        Invoke-TerminalRunnerReap -RepositoryRoot $reapClone -BenchRoot $reapCapRoot -TargetRoot $reapCapRoot -Ceiling 2 -Resolver $reapCapResolver
    } else { $null }
    Assert-True -Condition ($reapCapped -and $reapCapped.Truncated -and $reapCapped.Considered -eq 2 -and $reapCapped.Found -eq 4) `
        -Message "a ceiling bounds the candidates per pass and SAYS SO in its result, so a truncated sweep cannot be mistaken for an empty one (found $(if ($reapCapped) { $reapCapped.Found } else { 'n/a' }), considered $(if ($reapCapped) { $reapCapped.Considered } else { 'n/a' }))"
    Assert-True -Condition ($reapSummary -and -not $reapSummary.Truncated -and $reapSummary.Removed -ge 1) `
        -Message "the ordinary pass is NOT truncated and reports what it reclaimed (removed $(if ($reapSummary) { $reapSummary.Removed } else { 'n/a' }))"

    # PART B IS WIRED, NOT MERELY DEFINED, and it runs BEFORE the selection loop: a reaper that only
    # existed would leave the document's sentence as false as it was.
    $reapCallAnchor = 'Invoke-TerminalRunnerReap -RepositoryRoot $repoRoot'
    $reapCallCount = ([regex]::Matches($runnerText, [regex]::Escape($reapCallAnchor))).Count
    $reapDecoyCount = ([regex]::Matches(($runnerText + "`n" + $reapCallAnchor), [regex]::Escape($reapCallAnchor))).Count
    Assert-True -Condition ($reapCallCount -eq 1 -and $reapDecoyCount -eq 2) `
        -Message "DECOY: the startup call site is spelled exactly once in gate-runner.ps1 ($reapCallCount), and the same counter reads 2 over a text with one copy appended -- so the ordering cell below anchors on a unique site and not on a substring that happens to be everywhere"
    $reapFnAt = $runnerText.IndexOf('function Invoke-TerminalRunnerReap', [System.StringComparison]::Ordinal)
    $reapCallAt = $runnerText.IndexOf($reapCallAnchor, [System.StringComparison]::Ordinal)
    $reapIterationsAt = $runnerText.IndexOf('$iterations = 0', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($reapFnAt -ge 0 -and $reapCallAt -gt $reapFnAt -and $reapIterationsAt -gt $reapCallAt) `
        -Message 'the startup path CALLS the reaper once, after the function is defined and before the selection loop begins -- so it reaps once per runner process and never mid-queue'

    # THE DEFAULT CEILING, EXERCISED. The cell above passes `-Ceiling 2`, so the DEFAULT was pinned by
    # nothing: an editor could change 60 to 1, or to 0 (which this function reads as UNBOUNDED, not as
    # "reap nothing"), and no cell would notice. This drives the function with NO -Ceiling at all
    # against 61 empty pr<N> directories, which is one more than the default. What a future editor may
    # change: the number, freely, as long as this cell moves with it and the pass still ANNOUNCES the
    # truncation. What they may not do is reach 0 or a negative by accident, because both disable the
    # bound entirely and the announcement with it -- and that is the reading this cell would catch, by
    # finding Truncated false and Considered 61.
    $reapDefaultRoot = Join-Path $reapRoot 'default-ceiling'
    $null = New-Item -ItemType Directory -Path $reapDefaultRoot -Force
    foreach ($n in 900..960) { $null = New-Item -ItemType Directory -Path (Join-Path $reapDefaultRoot "pr$n") -Force }
    $reapDefaulted = if ($reapLoadable) {
        Invoke-TerminalRunnerReap -RepositoryRoot $reapClone -BenchRoot $reapDefaultRoot -TargetRoot '' -Resolver $reapCapResolver
    } else { $null }
    Assert-True -Condition ($reapDefaulted -and $reapDefaulted.Found -eq 61 -and $reapDefaulted.Truncated -and $reapDefaulted.Considered -eq 60) `
        -Message "the DEFAULT ceiling is 60 and it is the number that actually bounds a pass nobody parameterised (found $(if ($reapDefaulted) { $reapDefaulted.Found } else { 'n/a' }), considered $(if ($reapDefaulted) { $reapDefaulted.Considered } else { 'n/a' }), truncated $(if ($reapDefaulted) { $reapDefaulted.Truncated } else { 'n/a' }))"

    # ---------------------------------------------------------------------------------------------
    # #1085 THE CONCURRENCY GUARD. The hazard this change INTRODUCED, not one it inherited.
    #
    # Before the startup reap, a bench was force-removed only when a queue entry for an already
    # terminal pull request was SELECTED -- by the same loop that would otherwise have gated it, so
    # the two could not collide. Now every runner startup walks the roots, and a pull request can
    # merge WHILE another runner process is gating in `pr<N>`: the reap is the frequent path to a
    # `--force` on a directory a live compile is writing into.
    #
    # The guard is a claim file the working runner writes beside the bench, carrying a pid AND that
    # pid's start time. Each cell below drives the REAL predicate against a REAL process this suite
    # started and can kill, because the property is "does this correctly decide whether that process
    # is alive", and a fake process object would be measuring the fake.
    # ---------------------------------------------------------------------------------------------
    $reapLiveRoot = Join-Path $reapRoot 'live'
    $reapLiveBenchRoot = Join-Path $reapLiveRoot 'benches'
    $reapLiveTargetRoot = Join-Path $reapLiveRoot 'targets'
    $null = New-Item -ItemType Directory -Path $reapLiveBenchRoot -Force
    $null = New-Item -ItemType Directory -Path $reapLiveTargetRoot -Force
    # A REAL LIVE PROCESS, started by this suite and killed by this suite -- nothing else on this
    # machine is signalled. It stands in for the runner that is gating pr711 right now.
    $reapLiveProc = Start-Process -FilePath 'powershell' -ArgumentList '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 240' -PassThru -WindowStyle Hidden
    # A process that is REALLY GONE, for the stale arm. Started and waited on, so "no such pid" is a
    # fact about this machine rather than an invented number that might belong to somebody.
    $reapDeadProc = Start-Process -FilePath 'cmd' -ArgumentList '/c', 'exit', '0' -PassThru -WindowStyle Hidden
    $reapDeadStartedTicks = $reapDeadProc.StartTime.ToUniversalTime().Ticks
    $reapDeadProc.WaitForExit()
    Start-Sleep -Milliseconds 300
    $reapWriteClaim = {
        param([string] $RootPath, [string] $Name, [string] $Body)
        [System.IO.File]::WriteAllText((Join-Path $RootPath $Name), $Body)
    }
    $reapClaimBody = {
        param([int] $ClaimPid, [long] $Ticks, [string] $Pr)
        (ConvertTo-Json ([ordered]@{ pid = $ClaimPid; startedTicks = $Ticks; pullRequest = $Pr; writtenUtc = (Get-Date).ToUniversalTime().ToString('o') }) -Compress)
    }
    try {
        $reapLiveTicks = (Get-Process -Id $reapLiveProc.Id).StartTime.ToUniversalTime().Ticks
        & $reapWriteClaim $reapLiveBenchRoot ".runner-claim-$($reapLiveProc.Id).json" (& $reapClaimBody $reapLiveProc.Id $reapLiveTicks '711')
        $reapLiveVerdict = if ($reapGuardLoadable) { Test-PullRequestHasLiveWorker -PullRequest '711' -Roots @($reapLiveBenchRoot, $reapLiveTargetRoot) } else { $null }
        $reapOtherVerdict = if ($reapGuardLoadable) { Test-PullRequestHasLiveWorker -PullRequest '712' -Roots @($reapLiveBenchRoot, $reapLiveTargetRoot) } else { $null }
        Assert-True -Condition ($reapLiveVerdict -and $reapLiveVerdict.Live -and $reapOtherVerdict -and -not $reapOtherVerdict.Live) `
            -Message "a bench claimed by a LIVE runner process reads as in use, and the very same claim leaves a DIFFERENT pull request reapable -- so the guard retains the run and not the whole root (#711 live=$(if ($reapLiveVerdict) { $reapLiveVerdict.Live } else { 'n/a' }), #712 live=$(if ($reapOtherVerdict) { $reapOtherVerdict.Live } else { 'n/a' }))"

        # THE STALE ARM, and the reason the guard is not simply "a file exists". A runner killed
        # mid-gate leaves its claim behind; if a leftover file retained forever, the reaper would
        # reclaim nothing after the first crash and the disk this exists to free would fill again.
        # The pid is a REAL dead one and the ticks are the ones it really had.
        $reapStaleName = ".runner-claim-$($reapDeadProc.Id).json"
        & $reapWriteClaim $reapLiveBenchRoot $reapStaleName (& $reapClaimBody $reapDeadProc.Id $reapDeadStartedTicks '713')
        $reapStaleVerdict = if ($reapGuardLoadable) { Test-PullRequestHasLiveWorker -PullRequest '713' -Roots @($reapLiveBenchRoot, $reapLiveTargetRoot) } else { $null }
        Assert-True -Condition ($reapStaleVerdict -and -not $reapStaleVerdict.Live -and -not (Test-Path -LiteralPath (Join-Path $reapLiveBenchRoot $reapStaleName))) `
            -Message "a claim naming a process that is REALLY GONE does not retain, and the dead claim is swept as it is read, so one crashed runner cannot freeze the reaper forever (live=$(if ($reapStaleVerdict) { $reapStaleVerdict.Live } else { 'n/a' }))"

        # FAIL SAFE. A claim that cannot be parsed is not a claim that can be ignored: the process it
        # names might be compiling right now. Two shapes, because they fail at different lines --
        # bytes that are not JSON at all, and well-formed JSON with no process identity in it.
        & $reapWriteClaim $reapLiveTargetRoot '.runner-claim-garbage.json' 'this is not json {{{'
        $reapGarbageVerdict = if ($reapGuardLoadable) { Test-PullRequestHasLiveWorker -PullRequest '714' -Roots @($reapLiveBenchRoot, $reapLiveTargetRoot) } else { $null }
        Remove-Item -LiteralPath (Join-Path $reapLiveTargetRoot '.runner-claim-garbage.json') -Force -ErrorAction SilentlyContinue
        & $reapWriteClaim $reapLiveTargetRoot '.runner-claim-partial.json' '{"pullRequest":"714"}'
        $reapPartialVerdict = if ($reapGuardLoadable) { Test-PullRequestHasLiveWorker -PullRequest '714' -Roots @($reapLiveBenchRoot, $reapLiveTargetRoot) } else { $null }
        Remove-Item -LiteralPath (Join-Path $reapLiveTargetRoot '.runner-claim-partial.json') -Force -ErrorAction SilentlyContinue
        Assert-True -Condition ($reapGarbageVerdict -and $reapGarbageVerdict.Live -and $reapPartialVerdict -and $reapPartialVerdict.Live) `
            -Message "an UNREADABLE claim and a claim MISSING its process identity both read as in use -- the guard fails SAFE, because an unanswerable question is never permission to force-delete a running gate (garbage live=$(if ($reapGarbageVerdict) { $reapGarbageVerdict.Live } else { 'n/a' }), partial live=$(if ($reapPartialVerdict) { $reapPartialVerdict.Live } else { 'n/a' }))"

        # THE GUARD IS WHERE THE FORCE IS, not merely defined. Driven through the real reaper: pr711
        # is MERGED, its bench and target both exist, and the live claim above is the only thing
        # standing between them and `--force`.
        # A REAL REGISTERED WORKTREE, clean and level with origin -- the shape the discriminator
        # licenses a `--force` on. A plain directory here would have been retained by the ownership
        # check alone and the bench half of this cell would have proved nothing.
        & git -C $reapClone worktree add --quiet -b feat711 (Join-Path $reapLiveBenchRoot 'pr711') 'origin/feat711' 2>&1 | Out-Null
        $null = New-Item -ItemType Directory -Path (Join-Path $reapLiveTargetRoot 'pr711') -Force
        $null = New-Item -ItemType Directory -Path (Join-Path $reapLiveTargetRoot 'pr715') -Force
        $reapLiveResolver = { param([string] $PullRequest) return [pscustomobject]@{ Branch = "feat$PullRequest"; Head = ('a' * 40); State = 'MERGED'; BaseRef = 'main'; BaseSha = ('b' * 40) } }
        $null = if ($reapLoadable -and $reapGuardLoadable) {
            Invoke-TerminalRunnerReap -RepositoryRoot $reapClone -BenchRoot $reapLiveBenchRoot -TargetRoot $reapLiveTargetRoot -Resolver $reapLiveResolver
        } else { $null }
        Assert-True -Condition ((Test-Path -LiteralPath (Join-Path $reapLiveBenchRoot 'pr711')) -and
                (Test-Path -LiteralPath (Join-Path $reapLiveTargetRoot 'pr711')) -and
                -not (Test-Path -LiteralPath (Join-Path $reapLiveTargetRoot 'pr715'))) `
            -Message 'a MERGED pull request whose bench a LIVE runner process claims keeps BOTH its bench and its target through a real reap pass, while an unclaimed MERGED target in the same pass is still reclaimed -- so the guard is a skip and not an off switch'
    } finally {
        # ONLY WHAT THIS CELL STARTED. A gate may be running on this machine; nothing else is signalled.
        if ($reapLiveProc -and -not $reapLiveProc.HasExited) { Stop-Process -Id $reapLiveProc.Id -Force -ErrorAction SilentlyContinue }
    }

    # THE CLAIM IS ARMED AT THE ARMING SITE -- READ FROM THE AST, NOT FROM THE TEXT. `Invoke-OneEntry`
    # must write it the instant it commits to a bench path, before the worktree is prepared and not
    # beside `Start-Process`, because the window a concurrent reap must not enter opens when the
    # directory appears and not when the compiler starts. A substring search would have pinned this
    # the same way the two wiring cells above once pinned theirs, and it would have had the same
    # hole: prefixing the call with a single '#' leaves every substring and every offset exactly
    # where it was. In the parse tree a comment is not a CommandAst, so the '#' is visible.
    $reapParsed = [System.Management.Automation.Language.Parser]::ParseFile($runner, [ref] $null, [ref] $null)
    $reapOneEntryAst = $reapParsed.Find({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Invoke-OneEntry' }, $true)
    $reapArmAsts = if ($reapOneEntryAst) { @($reapOneEntryAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Set-RunnerWorkClaim' }, $true)) } else { @() }
    $reapLaunchAsts = if ($reapOneEntryAst) { @($reapOneEntryAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Start-Process' }, $true)) } else { @() }
    $reapBenchAssignAsts = if ($reapOneEntryAst) { @($reapOneEntryAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$bench' }, $true)) } else { @() }
    $reapArmOffset = if ($reapArmAsts.Count -eq 1) { $reapArmAsts[0].Extent.StartOffset } else { -1 }
    $reapBenchOffset = if ($reapBenchAssignAsts.Count -ge 1) { $reapBenchAssignAsts[0].Extent.StartOffset } else { -1 }
    $reapLaunchOffset = if ($reapLaunchAsts.Count -ge 1) { $reapLaunchAsts[0].Extent.StartOffset } else { -1 }
    Assert-True -Condition ($reapArmAsts.Count -eq 1 -and $reapBenchOffset -ge 0 -and $reapLaunchOffset -gt 0 -and
            $reapArmOffset -gt $reapBenchOffset -and $reapLaunchOffset -gt $reapArmOffset) `
        -Message "the working runner ARMS its claim -- one real Set-RunnerWorkClaim command in Invoke-OneEntry's parse tree ($($reapArmAsts.Count)), after the bench path is chosen (offset $reapBenchOffset) and before the gate child is launched (arm $reapArmOffset, launch $reapLaunchOffset)"

    # ---------------------------------------------------------------------------------------------
    # #1085 THE WIRING, OBSERVED RATHER THAN READ. The two cells above pin the call site by SUBSTRING
    # SEARCH, and a substring search cannot see a comment: prefixing the startup call with a single
    # '#' leaves both of them green, decoy and all, because the decoy proves the anchor is UNIQUE and
    # says nothing about whether it EXECUTES. This drives a real runner process at an EMPTY queue --
    # the very case the queue-drop path never reaches, and the whole reason #1085 exists -- and reads
    # the reaper's own summary out of its log, ahead of the line that says the queue was empty.
    # ---------------------------------------------------------------------------------------------
    $reapWiredQueue = Join-Path $reapRoot 'wired-queue'
    $reapWiredState = Join-Path $reapRoot 'wired-state'
    $reapWiredBenches = Join-Path $reapRoot 'wired-benches'
    $reapWiredTargets = Join-Path $reapRoot 'wired-targets'
    foreach ($d in $reapWiredQueue, $reapWiredState, $reapWiredBenches, $reapWiredTargets) { $null = New-Item -ItemType Directory -Path $d -Force }
    # EMPTY ROOTS ON PURPOSE. The property is that the pass RUNS, not what it removes, and a child
    # process pointed at the machine's real roots would reap this machine while measuring a comment.
    $reapWiredLog = (& powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $reapWiredQueue -StateDirectory $reapWiredState `
            -BenchRoot $reapWiredBenches -TargetRoot $reapWiredTargets 2>&1 | Out-String)
    $reapWiredSummaryAt = $reapWiredLog.IndexOf('reaper: 0 pr<N> directories found', [System.StringComparison]::Ordinal)
    $reapWiredEmptyAt = $reapWiredLog.IndexOf('queue empty', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($reapWiredSummaryAt -ge 0 -and $reapWiredEmptyAt -gt $reapWiredSummaryAt) `
        -Message "a REAL runner process with an EMPTY queue prints the reaper's own summary, and prints it BEFORE it reports the queue empty -- so the startup call is executed and not merely spelled (summary at $reapWiredSummaryAt, 'queue empty' at $reapWiredEmptyAt)"
    # The network state is re-read after the gate and before publication. Exercise the decision as
    # a pure boundary so terminal and moved-head races cannot be hidden by a fake push succeeding.
    $eligibilityStart = $runnerText.IndexOf('function Test-PublicationEligibility', [System.StringComparison]::Ordinal)
    $eligibilityEnd = $runnerText.IndexOf('function Set-EntryStatus', [System.StringComparison]::Ordinal)
    $eligibilityLoaded = $eligibilityStart -ge 0 -and $eligibilityEnd -gt $eligibilityStart
    if ($eligibilityLoaded) {
        . ([scriptblock]::Create($runnerText.Substring($eligibilityStart, $eligibilityEnd - $eligibilityStart)))
    }
    $expectedPublicationHead = 'a' * 40
    $openEligibility = if ($eligibilityLoaded) { Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'OPEN'; Head = $expectedPublicationHead }) -ExpectedHead $expectedPublicationHead } else { $null }
    $closedEligibility = if ($eligibilityLoaded) { Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'CLOSED'; Head = $expectedPublicationHead }) -ExpectedHead $expectedPublicationHead } else { $null }
    $movedEligibility = if ($eligibilityLoaded) { Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'OPEN'; Head = ('b' * 40) }) -ExpectedHead $expectedPublicationHead } else { $null }
    Assert-True -Condition ($openEligibility -and $openEligibility.Allowed) `
        -Message 'publication eligibility accepts the same exact head while the pull request is open'
    Assert-True -Condition ($closedEligibility -and -not $closedEligibility.Allowed -and $closedEligibility.Reason -match 'terminal') `
        -Message 'publication eligibility rejects a pull request that became terminal during the gate'
    Assert-True -Condition ($movedEligibility -and -not $movedEligibility.Allowed -and $movedEligibility.Reason -match 'moved') `
        -Message 'publication eligibility rejects a pull request whose head moved during the gate'
    $renamedEligibility = Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'OPEN'; Head = $expectedPublicationHead; Branch = 'renamed'; BaseRef = 'main'; BaseSha = ('c' * 40) }) -ExpectedHead $expectedPublicationHead -ExpectedBranch 'expected' -ExpectedBaseRef 'main' -ExpectedBaseSha ('c' * 40)
    $baseRefEligibility = Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'OPEN'; Head = $expectedPublicationHead; Branch = 'expected'; BaseRef = 'release'; BaseSha = ('c' * 40) }) -ExpectedHead $expectedPublicationHead -ExpectedBranch 'expected' -ExpectedBaseRef 'main' -ExpectedBaseSha ('c' * 40)
    $baseShaEligibility = Test-PublicationEligibility -PullRequestState ([pscustomobject]@{ State = 'OPEN'; Head = $expectedPublicationHead; Branch = 'expected'; BaseRef = 'main'; BaseSha = ('d' * 40) }) -ExpectedHead $expectedPublicationHead -ExpectedBranch 'expected' -ExpectedBaseRef 'main' -ExpectedBaseSha ('c' * 40)
    Assert-True -Condition (-not $renamedEligibility.Allowed -and $renamedEligibility.Reason -match 'renamed') `
        -Message 'publication eligibility rejects a source branch rename even when the head SHA is unchanged'
    Assert-True -Condition (-not $baseRefEligibility.Allowed -and $baseRefEligibility.Reason -match 'base branch') `
        -Message 'publication eligibility rejects a changed base branch even when the source head is unchanged'
    Assert-True -Condition (-not $baseShaEligibility.Allowed -and $baseShaEligibility.Reason -match 'base revision') `
        -Message 'publication eligibility rejects a changed base revision even when the source head is unchanged'
    $publishCallAt = $runnerText.IndexOf('$pushed = Publish-Receipt', $invokeAt, [System.StringComparison]::Ordinal)
    $postPublishReadAt = $runnerText.IndexOf('$postPublicationState = Resolve-PullRequestBranch', $publishCallAt, [System.StringComparison]::Ordinal)
    Assert-True -Condition ($publishCallAt -gt $invokeAt -and $postPublishReadAt -gt $publishCallAt -and $cleanupCallAt -gt $postPublishReadAt) `
        -Message 'a second eligibility read sits after the push and before target or bench cleanup, closing the merge-during-push race'
    # ---------------------------------------------------------------------------------------------
    # #1053: THE GUARD ITSELF, DRIVEN ON REAL DIRECTORIES.
    #
    # The cells above are source claims about WIRING; these are behaviour. The danger is the same
    # shape as Test-BenchIsRegistered's below: a predicate that answered $true for everything would
    # keep a half-written target and hand the next gate cargo fingerprints claiming freshness for
    # binaries whose source has moved -- #455's defect, re-armed by the thing meant to speed it up.
    # So every cell but the first asserts $false, and the first one exists to prove the predicate
    # can answer $true at all.
    $stateMatch = [regex]::Match($runnerText, '(?ms)^function Test-TargetBuildFinished \{.*?^\}')
    Assert-True -Condition $stateMatch.Success `
        -Message 'ARRANGEMENT: Test-TargetBuildFinished is locatable in gate-runner.ps1, so the cells below drive the real one and not a copy'
    . ([scriptblock]::Create($stateMatch.Value))

    # THE NAME OF THE MARKER IS READ OUT OF gate.ps1, NOT RETYPED HERE. The runner repeats the
    # literal because it cannot call the gate's reader, and two copies of a filename drift in
    # silence: a rename in gate.ps1 alone would make the runner answer $false forever -- safe, but a
    # permanent cold build nobody would think to look for. This is the round-trip that holds them
    # together, and it is why the fixtures below write the name this cell just read.
    $gateTextForMarker = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate.ps1'))
    $markerNameMatch = [regex]::Match($gateTextForMarker, '(?m)^\s*\$markerName = ''([^'']+)''')
    $runnerUsesSameName = ($markerNameMatch.Success -and
        $runnerText.IndexOf("'$($markerNameMatch.Groups[1].Value)'", [System.StringComparison]::Ordinal) -ge 0)
    Assert-True -Condition $runnerUsesSameName `
        -Message "the runner names the same build-state marker gate.ps1 does ('$(if ($markerNameMatch.Success) { $markerNameMatch.Groups[1].Value } else { 'UNREADABLE' })'), so the two copies of the filename cannot drift into a permanent cold build"

    $markerName = if ($markerNameMatch.Success) { $markerNameMatch.Groups[1].Value } else { '.graphhelm-build-state.json' }
    $stateDir = Join-Path $root 'targetstate'
    $null = New-Item -ItemType Directory -Path $stateDir -Force
    # A file that is not the marker, so "the directory holds build output" is true for every cell
    # below and the answers differ only by the marker's content.
    [System.IO.File]::WriteAllText((Join-Path $stateDir 'libthing.rlib'), 'not empty')
    $markerPath = Join-Path $stateDir $markerName
    function Set-Marker { param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Content)
        [System.IO.File]::WriteAllText($markerPath, $Content)
    }

    Set-Marker -Content '{"state":"complete","processId":4242,"head":"abc","startedUtc":"2026-09-19T00:00:00.0000000Z"}'
    Assert-True -Condition (Test-TargetBuildFinished -TargetDir $stateDir) `
        -Message 'a target whose marker says `complete` is KEPT -- the one answer that preserves, and the reason this change exists'

    # SAME DIRECTORY, ONE WORD CHANGED. Flipping only the state isolates the property: a cell using
    # a fresh directory per case would also be measuring the fixture.
    Set-Marker -Content '{"state":"building","processId":4242,"head":"abc","startedUtc":"2026-09-19T00:00:00.0000000Z"}'
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message 'the SAME directory with only the state word changed to `building` is removed, so the answer tracks the marker and not the fixture'

    Set-Marker -Content '{"state":"unproven","processId":4242,"head":"abc","startedUtc":"2026-09-19T00:00:00.0000000Z"}'
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message '`unproven` -- a pass that finished but could not vouch for every binary -- is removed, because the predicate is "finished AND vouched"'

    Set-Marker -Content '{"state":"aNewStateFromANewerGate","processId":4242,"head":"abc"}'
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message 'a state word this runner does not know is removed, so a vocabulary added to gate.ps1 costs a cold build rather than preserving a target nobody here can judge'

    Set-Marker -Content '{"processId":4242,"head":"abc"}'
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message 'a marker with no `state` field at all is removed, and reading it by index rather than by dot means StrictMode does not kill the runner over someone else''s file'

    Set-Marker -Content '{"state":"complete", this is not json'
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message 'a marker that does not parse is removed even though the bytes `complete` are in it, so the answer comes from a parsed field and not from a substring'

    Remove-Item -LiteralPath $markerPath -Force
    Assert-True -Condition (-not (Test-TargetBuildFinished -TargetDir $stateDir)) `
        -Message 'a directory holding build output with NO marker is removed -- the pre-#1053 state of every target on this machine, and the answer that keeps this change fail-closed'

    # ---------------------------------------------------------------------------------------------
    # #1053 item 3: the gate targets come off the mechanical disk, and the preserved target gains
    # the size bound that item 1 removed.
    #
    # Measured before this change: gate runs whose `cargoTargetDir` was on D: median 1866 s (n=156)
    # against 1427 s on E: (n=183), and a live sample with two gates running read D: at 1005 %
    # disk time with a queue depth of 8 while E: sat at 3.8 % and C: at 14.3 %. D: is a WDC
    # WD20PURZ -- a surveillance-class platter -- and it held one of the two gate slots.
    #
    # Moving a gate target onto C:, the SYSTEM disk, is only defensible with an enforced floor,
    # and it is doubly so now that item 1 preserves targets: cargo never collects stale artefacts,
    # so a preserved target grows without bound. These cells are that floor's only proof.
    $floorMatch = [regex]::Match($runnerText, '(?ms)^function Get-TargetRootFloorGB \{.*?^\}')
    $freeMatch = [regex]::Match($runnerText, '(?ms)^function Get-TargetRootFreeGB \{.*?^\}')
    $keepMatch = [regex]::Match($runnerText, '(?ms)^function Test-TargetShouldBeKept \{.*?^\}')
    Assert-True -Condition ($floorMatch.Success -and $freeMatch.Success -and $keepMatch.Success) `
        -Message 'ARRANGEMENT: the floor, the free-space probe and the keep decision are all locatable in gate-runner.ps1, so the cells below drive the real ones'
    . ([scriptblock]::Create($floorMatch.Value))
    . ([scriptblock]::Create($freeMatch.Value))
    . ([scriptblock]::Create($keepMatch.Value))

    Assert-True -Condition ((Get-TargetRootFloorGB -Path 'C:\runner-targets\hdd') -eq 100) `
        -Message 'the system disk keeps a 100 GB floor, which is AGENTS.md''s own number and the reason a gate target may live there at all'
    Assert-True -Condition ((Get-TargetRootFloorGB -Path 'E:\runner-targets\ssd') -eq 30) `
        -Message 'a non-system spindle keeps 30 GB, so the cell above is about C: and not about every path'
    # A FLOOR OF 0 IS "FILL THE DISK". An unrecognised root must not silently buy it.
    Assert-True -Condition ((Get-TargetRootFloorGB -Path '') -eq 30) `
        -Message 'an unrecognised or empty root still gets a non-zero floor, so an unexpected path cannot buy permission to fill a disk'

    Assert-True -Condition ((Get-TargetRootFreeGB -Path $root) -gt 0) `
        -Message 'the free-space probe answers a positive number for a path that really exists, so the cells below are not passing on a probe that answers nothing'
    # UNKNOWN IS NOT ZERO AND NOT INFINITY. Both of those are claims the probe has not earned.
    Assert-True -Condition ($null -eq (Get-TargetRootFreeGB -Path 'Q:\no\such\drive')) `
        -Message 'a path on a drive that does not exist answers $null -- a third state -- rather than 0, which would read as a full disk, or a number, which would read as an empty one'

    # for `Invoke-External` when it drives Test-BenchIsRegistered.
    $script:fakeFreeGB = 500.0
    function Get-TargetRootFreeGB { param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path) return $script:fakeFreeGB }

    Set-Marker -Content '{"state":"complete","processId":4242,"head":"abc"}'
    $script:fakeFreeGB = 500.0
    Assert-True -Condition (Test-TargetShouldBeKept -TargetDir $stateDir -TargetRoot 'C:\runner-targets\hdd') `
        -Message 'a finished target on a disk with 500 GB free against a 100 GB floor is KEPT -- both halves satisfied'

    # THE NEW PROPERTY, and the one that makes a gate target on the system disk defensible.
    $script:fakeFreeGB = 50.0
    Assert-True -Condition (-not (Test-TargetShouldBeKept -TargetDir $stateDir -TargetRoot 'C:\runner-targets\hdd')) `
        -Message 'the SAME finished target is EVICTED when the disk is under its floor, so item 1''s preserved target cannot grow a system disk into the ground'

    $script:fakeFreeGB = $null
    Assert-True -Condition (-not (Test-TargetShouldBeKept -TargetDir $stateDir -TargetRoot 'C:\runner-targets\hdd')) `
        -Message 'a disk whose free space cannot be read is treated as no headroom, so an unanswerable question deletes rather than keeps'

    # AND THE PROVENANCE HALF STILL GOVERNS ON ITS OWN: all the space in the world does not save a
    # target whose build never finished.
    Set-Marker -Content '{"state":"building","processId":4242,"head":"abc"}'
    $script:fakeFreeGB = 500.0
    Assert-True -Condition (-not (Test-TargetShouldBeKept -TargetDir $stateDir -TargetRoot 'C:\runner-targets\hdd')) `
        -Message 'an unfinished build is still removed on a disk with room to spare, so the space half did not quietly replace the provenance half'

    # THE DEFAULTS THEMSELVES -- the whole of item 3, stated where a reader of this suite sees it.
    $rootLine = @(($runnerText -split "`r?`n") | Where-Object { $_ -match '\$TargetRoot = if \(\$Slot -eq' })
    Assert-True -Condition ($rootLine.Count -eq 1 -and $rootLine[0] -notmatch "'D:") `
        -Message 'NEITHER gate slot defaults to a target on D: -- the WD20PURZ platter that measured 1005 % disk time with a queue of 8 while both SSDs idled'
    Assert-True -Condition ($rootLine.Count -eq 1 -and $rootLine[0] -match "'E:\\runner-targets\\ssd'") `
        -Message 'CONTROL: the SSD slot still defaults to E:, so the cell above is about moving the OTHER slot and not about the line having been emptied'

    # ---------------------------------------------------------------------------------------------
    # #1085 MEETS #1053: the post-receipt cleanup obeys the keep policy, and the keep policy never
    # runs before the receipt.
    #
    # These two rules were written on branches that could not see each other, and taken verbatim
    # they contradict: #1085's helper removes the target on every successful publication, which is
    # every successful run, so #1053's warm reuse would be dead code the day both land. The merge
    # composes them in one order -- the receipt buys the RIGHT to remove, the keep policy decides
    # whether removing is WORTH it -- and these cells are that composition's only proof. They drive
    # the real helper on real directories; the ordering cells above already pin the caller.
    $publishCleanupMatch = [regex]::Match($runnerText, '(?ms)^function Remove-PublishedRunnerTarget \{.*?^\}')
    Assert-True -Condition $publishCleanupMatch.Success `
        -Message 'ARRANGEMENT: Remove-PublishedRunnerTarget is locatable in gate-runner.ps1, so the two cells below drive the real one'
    function Write-Note { param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Message) }
    . ([scriptblock]::Create($publishCleanupMatch.Value))

    $publishRoot = Join-Path $root 'publishcleanup'
    $publishTarget = Join-Path $publishRoot 'pr4242'
    $null = New-Item -ItemType Directory -Path $publishTarget -Force
    [System.IO.File]::WriteAllText((Join-Path $publishTarget 'libthing.rlib'), 'not empty')
    $publishMarker = Join-Path $publishTarget $markerName

    # A WARM, VOUCHED, AFFORDABLE TARGET SURVIVES ITS OWN RECEIPT. Without this the merge would
    # silently revert #1053, and it would revert it invisibly: every cell #1053 shipped is about the
    # run-START eviction and would still pass.
    [System.IO.File]::WriteAllText($publishMarker, '{"state":"complete","processId":4242,"head":"abc"}')
    $script:fakeFreeGB = 500.0
    Remove-PublishedRunnerTarget -TargetRoot $publishRoot -PullRequest '4242'
    Assert-True -Condition (Test-Path -LiteralPath $publishTarget) `
        -Message '#1085 + #1053: a published receipt does NOT delete a target whose last build finished and vouched on a root above its floor, so warm reuse survives the cleanup'

    # AND THE LITTER STILL GOES. The owner''s standing order is about targets that outlive their
    # run; one whose build never vouched is exactly that, and the run-start eviction will never
    # visit it if this pull request never re-runs.
    [System.IO.File]::WriteAllText($publishMarker, '{"state":"building","processId":4242,"head":"abc"}')
    Remove-PublishedRunnerTarget -TargetRoot $publishRoot -PullRequest '4242'
    Assert-True -Condition (-not (Test-Path -LiteralPath $publishTarget)) `
        -Message '#1085 + #1053: the SAME target with only the state word changed IS removed after the receipt, so the keep is the marker''s doing and the litter rule still bites'


    # ---------------------------------------------------------------------------------------------
    # #943 / the delete's own precondition: a `pr` that is not a number never reaches a path.
    #
    # This is the one case in this suite that is BEHAVIOURAL rather than a source claim, and it can
    # be, because the refusal happens before a bench is prepared -- the same early return every other
    # case here rides on.
    #
    # WHY IT IS WORTH A CELL AT ALL. `$pr` is interpolated into a path that is then removed
    # recursively and forcibly, and `-LiteralPath` does NOT forbid traversal. Measured:
    #
    #   pr = '\..\..\..'   Join-Path 'C:\runner-targets\hdd' -> C:\runner-targets\hdd\pr\..\..\..
    #                      GetFullPath                       -> D:\
    #   Remove-Item -LiteralPath <a traversing path> -Recurse -Force -WhatIf
    #                      -> "Remove Directory" on the RESOLVED parent, not on the literal text
    #
    # THE FIXTURE IS SELF-CONTAINED ON PURPOSE. The traversal aims at a sibling of the target root
    # INSIDE this suite's own temp directory, and both roots are passed explicitly, so a run against
    # a tree with the guard removed destroys a fixture directory and nothing else. That is deliberate:
    # the red-first check for this cell must be safe to perform.
    # THE QUEUE IS EMPTIED FIRST, and this is not tidiness. `-Once` drains ONE entry, and the case
    # above deliberately leaves its entry QUEUED -- so without this the runner would process THAT
    # entry again and this cell would read an empty status and fail for the wrong reason. It did,
    # while this cell was being written.
    Remove-Item -LiteralPath $entryPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $statusPath -Force -ErrorAction SilentlyContinue
    Set-Content -LiteralPath $shimPath -Value $strictShim -Encoding ASCII

    $victim = Join-Path $root 'victim'
    $null = New-Item -ItemType Directory -Path $victim -Force
    Set-Content -LiteralPath (Join-Path $victim 'canary.txt') -Value 'must survive' -Encoding ASCII
    $fixtureTargets = Join-Path $root 'targets'
    $fixtureBenches = Join-Path $root 'benches'

    # THE HEAD MATCHES THE FAKE SERVER'S, and that is what makes this cell non-vacuous. With a
    # mismatched head the runner drops at "head moved" -- before the delete -- and the cell would
    # pass without the guard existing. Matching it means the `pr` refusal is the ONLY thing between
    # the entry and a recursive delete.
    $traversalEntry = [ordered]@{
        pr        = '\..\..\victim'
        head      = $serverHead
        lane      = 'TESTS'
        timestamp = (Get-Date).ToUniversalTime().ToString('o')
    }
    # THE QUEUE IS SHARED WITH EVERY CELL ABOVE, and `-Once` processes the entry the selection
    # RANKS FIRST, not the one this cell just wrote. Leftovers from an earlier cell therefore decide
    # whether this cell measures anything -- and the failure is silent: the runner refuses somebody
    # else's entry, no `.status` is written beside this one, and the assertion reads an empty string
    # rather than a wrong one. Found merging #951 into this branch, where the added cells above left
    # entries behind and this cell went red at a head where the guard it tests was intact.
    Get-ChildItem -LiteralPath $queue -Filter '*.json' -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue

    $traversalPath = Join-Path $queue "traversal-$($serverHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $traversalPath -Value (ConvertTo-Json $traversalEntry) -Encoding UTF8

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
        -TargetRoot $fixtureTargets -BenchRoot $fixtureBenches 2>&1

    $traversalStatusPath = [System.IO.Path]::ChangeExtension($traversalPath, '.status')
    $traversalStatus = if (Test-Path -LiteralPath $traversalStatusPath) {
        (Get-Content -LiteralPath $traversalStatusPath -Raw)
    } else { '' }
    # THE REASON, not merely "it was refused". Any of three earlier checks could also refuse this
    # entry, and a cell that accepted any refusal would pass on a runner that never learned to look
    # at `pr` at all.
    Assert-True -Condition ($traversalStatus -match 'malformed pr') `
        -Message "a `pr` that is not a number is refused AS SUCH, before anything path-shaped is built (status: '$($traversalStatus.Trim())')"

    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $victim 'canary.txt')) `
        -Message 'and the directory the traversal aimed at is still there, with its contents'


    # ---------------------------------------------------------------------------------------------
    # A LOCK WHOSE HOLDER IS DEAD (#902, 2026-09-08). The HDD gate for #979 died without its
    # finally, its pair stayed in SLOT.lock, and the relaunched runner waited on it for good: the
    # old wait branch read the lock's CONTENT and never asked whether the process existed, while the
    # gate that would have reclaimed the dead pair is never launched by a runner that is waiting.
    # Two cells against a real pid each: a process that has exited, and this very process.
    # ---------------------------------------------------------------------------------------------
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $deadEntry = [ordered]@{ pr = 932; head = $serverHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $deadEntryPath = Join-Path $queue "932-$($serverHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $deadEntryPath -Value (ConvertTo-Json $deadEntry) -Encoding UTF8
    $gone = Start-Process -FilePath 'cmd' -ArgumentList '/c', 'exit', '0' -PassThru -WindowStyle Hidden
    $goneStart = $gone.StartTime.ToUniversalTime().ToString('o')
    $null = $gone.WaitForExit(30000)
    $lockPath = Join-Path $slotRoot 'SLOT.lock'
    $null = New-Item -ItemType Directory -Force -Path $slotRoot
    Set-Content -LiteralPath $lockPath -Encoding ASCII -Value @(
        "HELD by gate | $((Get-Date).ToUniversalTime().ToString('o')) | gate cwd=D:\nowhere head=$serverHead | STATUS: gate run",
        'Claimed through create-or-fail: the kernel refused every other claimant.',
        "holder: pid=$($gone.Id) start=$goneStart"
    )
    $deadLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
        -TargetRoot $fixtureTargets -BenchRoot $fixtureBenches 2>&1
    $deadCode = $LASTEXITCODE
    $deadText = ($deadLog | Out-String)
    # THE JOURNEY, NOT THE NOTE (Codex on #1022): the note is printed before Invoke-OneEntry, so a
    # regression that keeps the note and then continues/exits would pass a note-only cell while a
    # dead lock still stops every bench. The entry's status file is written only INSIDE
    # Invoke-OneEntry ('preparing bench' first, then the bench's own outcome), so its existence is
    # the effect the promise is about.
    $deadStatusPath = [System.IO.Path]::ChangeExtension($deadEntryPath, '.status')
    $deadStatus = if (Test-Path -LiteralPath $deadStatusPath) { (Get-Content -LiteralPath $deadStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ($deadText -match 'names a dead holder' -and $deadText -notmatch 'waiting rather than preparing a bench' -and (Test-Path -LiteralPath $deadStatusPath)) `
        -Message "a lock whose holder pid $($gone.Id) has exited is not waited on: the runner says the holder is dead and Invoke-OneEntry ran -- the entry carries a status written there (rc=$deadCode; status: '$deadStatus')"

    # CONTROL: the same lock naming THIS process, which is alive, is waited on -- so the cell above
    # measured liveness and not merely the presence of a pid line.
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    Set-Content -LiteralPath $deadEntryPath -Value (ConvertTo-Json $deadEntry) -Encoding UTF8
    $selfStart = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
    Set-Content -LiteralPath $lockPath -Encoding ASCII -Value @(
        "HELD by gate | $((Get-Date).ToUniversalTime().ToString('o')) | gate cwd=D:\nowhere head=$serverHead | STATUS: gate run",
        'Claimed through create-or-fail: the kernel refused every other claimant.',
        "holder: pid=$PID start=$selfStart"
    )
    # A pull request may become terminal while its gate still owns this slot. The startup reaper
    # must not delete that live gate's target merely because the server now says MERGED/CLOSED.
    $liveGateTarget = Join-Path $fixtureTargets 'pr932'
    $null = New-Item -ItemType Directory -Path $liveGateTarget -Force
    Set-Content -LiteralPath (Join-Path $liveGateTarget 'active-gate.marker') -Value 'owned by live gate' -Encoding ASCII
    $liveLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
        -TargetRoot $fixtureTargets -BenchRoot $fixtureBenches 2>&1
    $liveCode = $LASTEXITCODE
    $liveText = ($liveLog | Out-String)
    Assert-True -Condition ($liveText -match 'is held \(live\); waiting' -and $liveCode -eq 1) `
        -Message "CONTROL: a lock naming this live process is waited on (rc=$liveCode)"
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $liveGateTarget 'active-gate.marker')) `
        -Message 'a live slot holder protects its terminal PR target from the startup reaper; cleanup waits until no gate owns the slot'

    # THE THIRD ANSWER: a holder line the reader cannot judge (a pid that is not an integer) is
    # 'indeterminate', and indeterminate is WAITED ON exactly as live is -- the gate's own
    # Enter-GateSlot will not reclaim a pair it cannot judge either, so a runner that proceeded here
    # would launch a gate that sits on the same lock. The note names the verdict.
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    Set-Content -LiteralPath $deadEntryPath -Value (ConvertTo-Json $deadEntry) -Encoding UTF8
    Set-Content -LiteralPath $lockPath -Encoding ASCII -Value @(
        "HELD by gate | $((Get-Date).ToUniversalTime().ToString('o')) | gate cwd=D:
owhere head=$serverHead | STATUS: gate run",
        'Claimed through create-or-fail: the kernel refused every other claimant.',
        "holder: pid=not-a-pid start=$selfStart"
    )
    $vagueLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
        -TargetRoot $fixtureTargets -BenchRoot $fixtureBenches 2>&1
    $vagueCode = $LASTEXITCODE
    $vagueText = ($vagueLog | Out-String)
    Assert-True -Condition ($vagueText -match 'is held \(indeterminate\); waiting' -and $vagueCode -eq 1 -and $vagueText -notmatch 'names a dead holder') `
        -Message "a lock whose holder the reader cannot judge is waited on as indeterminate, never reclaimed as dead (rc=$vagueCode)"
    Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue
    # -------------------------------------------------------------------------------------------------
    # #1045: AN UNREGISTERED BENCH IS RECLAIMED, AND A REGISTERED ONE IS NEVER TOUCHED.
    # -------------------------------------------------------------------------------------------------
    Write-Host ""
    Write-Host '-- #1045: the bench remove is checked, and the fallback asks before it deletes --' -ForegroundColor Cyan

    # THE SUBJECT IS AN UNREGISTERED DIRECTORY AT THE BENCH PATH -- the ordinary state of every bench a
    # previous runner instance created from a different repo root. `git worktree remove` fails on it,
    # and with that code discarded the stall was reported against `worktree add` one line later.
    #
    # The entry names a head this repository does not have, so `add` fails EITHER WAY and the run stalls
    # in both worlds. That is deliberate: it makes the DIRECTORY the only thing that differs, so the
    # cell cannot pass because the run happened to get further for an unrelated reason.
    # THE CELL ABOVE DELIBERATELY LEAVES AN INDETERMINATE LOCK, and the runner is right to wait on
    # it forever. Left in place, this cell's run never happens and both assertions below fail for a
    # reason that has nothing to do with their subject -- which is how they failed the first time.
    Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue

    $benchRoot = Join-Path $root 'benches'
    New-Item -ItemType Directory -Path $benchRoot -Force | Out-Null
    $squatter = Join-Path $benchRoot 'pr200'
    New-Item -ItemType Directory -Path $squatter -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $squatter '.git') -Value 'gitdir: nowhere-this-clone-knows' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $squatter 'occupant.txt') -Value 'left by a runner that is gone' -Encoding ASCII

    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force
    $reclaimPath = Join-Path $queue '200-dddddddd.json'
    Set-Content -LiteralPath $reclaimPath -Value (ConvertTo-Json ([ordered]@{
        pr = 200; head = $serverHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8

    # NO SHIM HERE ANY MORE, AND THAT IS THE POINT. This cell used to answer four git vectors
    # locally so they could not reach the developer's clone. The cwd is a guard clone now, so there
    # is nothing to protect it from -- and letting the real vectors run is what makes the suite
    # observe them. The branch is published to the guard origin a line before the run, so the fetch
    # SUCCEEDS and the widening (#979) has to happen for it to: the guard clone was cloned
    # `--single-branch`, so `origin/issue-902-fake-branch` is outside its refspec until the runner
    # widens it. The wildcard's arrival there is asserted at the end of the suite.
    #
    # `worktree add` then still fails -- the entry names $serverHead, which no repository has -- so
    # the run stalls in both worlds and the DIRECTORY stays the only thing that differs, which is
    # this cell's whole design.
    $null = & git -C $guardAuthor push -q $guardOrigin "HEAD:refs/heads/$serverBranch" 2>$null

    $reclaimLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state -BenchRoot $benchRoot 2>&1

    Assert-True -Condition (-not (Test-Path -LiteralPath $squatter)) `
        'an unregistered bench directory is RECLAIMED rather than left to stall every future run'
    Assert-True -Condition (($reclaimLog -join "`n") -match 'not registered by this clone') `
        'and the log names the REMOVE that failed, not only the add that inherited the blame'


    # THE GUARD, DRIVEN ON ITS OWN. This is where the danger lives: the fallback deletes a directory, and
    # a guard that answered "not registered" for everything would turn it into an unconditional delete --
    # worse than the defect it repairs. Lifted from the file, never retyped, so an edit there moves this.
    $runnerText = Get-Content -LiteralPath $runner -Raw
    $guardMatch = [regex]::Match($runnerText, '(?ms)^function Test-BenchIsRegistered \{.*?^\}')
    Assert-True -Condition $guardMatch.Success `
        'ARRANGEMENT: the registration guard is locatable in gate-runner.ps1, so the cells below drive the real one'

    $script:listOutput = @()
    $script:listCode = 0
    # THE FAKE GAINED FOUR ANSWERS WITH #1085, because the subject gained four questions: terminal
    # bench removal now asks `status --porcelain`, `symbolic-ref`, `rev-parse` and `rev-list` before
    # it is allowed to force anything. Answering them all out of $script:listOutput, as the first
    # version did, fed the worktree list back as a dirty path and retained every bench -- a fake
    # that mirrors the measured contract has to mirror all of it.
    $script:externalVectors = New-Object System.Collections.Generic.List[string]
    function Invoke-External {
        param([string] $File, [string[]] $Arguments, [switch] $CaptureError)
        $script:externalCalls++
        $script:externalVectors.Add(($Arguments -join ' '))
        if ($Arguments -contains 'remove') {
            Remove-Item -LiteralPath $Arguments[-1] -Recurse -Force -ErrorAction SilentlyContinue
            return [pscustomobject]@{ Code = 0; Output = @() }
        }
        if ($Arguments -contains 'status') { return [pscustomobject]@{ Code = 0; Output = @() } }
        if ($Arguments -contains 'symbolic-ref') { return [pscustomobject]@{ Code = 0; Output = @('issue-201-fixture') } }
        if ($Arguments -contains 'rev-parse') { return [pscustomobject]@{ Code = 0; Output = @('1111111111111111111111111111111111111111') } }
        if ($Arguments -contains 'rev-list') { return [pscustomobject]@{ Code = 0; Output = @('0') } }
        return [pscustomobject]@{ Code = $script:listCode; Output = $script:listOutput }
    }
    $script:externalCalls = 0
    . ([scriptblock]::Create($guardMatch.Value))

    # git prints forward slashes; the bench path is built with Join-Path and carries backslashes; and
    # Windows is case-insensitive. A raw string compare answers "not registered" for every real bench.
    $script:listOutput = @('worktree D:/benches/pr200', 'HEAD 1111111111111111111111111111111111111111')
    $script:listCode = 0
    Assert-True -Condition (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\Benches\PR200') `
        'a REGISTERED bench is recognised across slash and case differences, so the fallback leaves it alone'

    $script:listOutput = @('worktree D:/benches/pr999')
    Assert-True -Condition (-not (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\benches\pr200')) `
        'CONTROL: a bench genuinely absent from the list reads as unregistered, or the cell above proves nothing'

    # AND AN EMPTY LIST THAT SUCCEEDED IS UNANSWERABLE TOO. Without its own counter the loop fell
    # through to "not registered" here, which authorises the delete -- a recursive force-delete
    # resting on git always printing a line. Unreachable today; the cell exists because the
    # reachability, not the safety, is what would change.
    $script:listOutput = @()
    $script:listCode = 0
    Assert-True -Condition (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\benches\pr200') `
        'a list that exits 0 with NO worktree lines answers registered too, so the fail-safe is total and not conditional on git printing something'

    # UNREADABLE IS NOT UNREGISTERED: if the question cannot be answered, the safe answer deletes nothing.
    $script:listCode = 128
    $script:listOutput = @()
    Assert-True -Condition (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\benches\pr200') `
        'a worktree list that FAILS answers registered, so an unanswerable question never authorises a delete'

    Assert-True -Condition (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\benches\pr200') `
        'the legacy recovery check can still fail safe-open without granting terminal cleanup permission'
    Assert-True -Condition (-not (Test-BenchIsRegistered -RepositoryRoot 'X' -BenchPath 'D:\benches\pr200' -FailClosed)) `
        'the terminal cleanup ownership check fails closed when worktree enumeration is unreadable'

    $terminalMatch = [regex]::Match($runnerText, '(?ms)^function Remove-TerminalRunnerBench \{.*?^\}')
    function Write-Note { param([string] $Message) }
    . ([scriptblock]::Create($terminalMatch.Value))
    $terminalRoot = Join-Path $root 'terminal-benches'
    $unknownBench = Join-Path $terminalRoot 'pr200'
    New-Item -ItemType Directory -Path $unknownBench -Force | Out-Null
    $script:listCode = 128
    $script:listOutput = @()
    $script:externalCalls = 0
    Remove-TerminalRunnerBench -RepositoryRoot 'X' -BenchRoot $terminalRoot -PullRequest '200' -State 'CLOSED'
    Assert-True -Condition (Test-Path -LiteralPath $unknownBench) `
        'terminal cleanup retains the exact bench when worktree enumeration fails'
    Assert-True -Condition ($script:externalCalls -eq 1) `
        'enumeration failure stops before any worktree removal command is attempted'

    $registeredBench = Join-Path $terminalRoot 'pr201'
    New-Item -ItemType Directory -Path $registeredBench -Force | Out-Null
    $script:listCode = 0
    $script:listOutput = @("worktree $($registeredBench.Replace('\', '/'))")
    $script:externalCalls = 0
    $script:externalVectors.Clear()
    Remove-TerminalRunnerBench -RepositoryRoot 'X' -BenchRoot $terminalRoot -PullRequest '201' -State 'MERGED'
    # THE ORDER IS THE CLAIM, NOT THE COUNT. A bare call count went stale the moment the removal
    # gained a discriminator, and counting conjuncts is not testing them: what must hold is that the
    # ownership read comes FIRST and the removal comes LAST, with the safety questions in between.
    $vectors = @($script:externalVectors)
    Assert-True -Condition (-not (Test-Path -LiteralPath $registeredBench) -and $vectors.Count -ge 3 -and
            $vectors[0].Contains('worktree list') -and $vectors[-1].Contains('worktree remove --force') -and
            (@($vectors | Where-Object { $_.Contains(' status --porcelain') }).Count -eq 1)) `
        "terminal cleanup removes the exact registered terminal bench, reading ownership first and removing last, with the safety questions in between (git calls: $($vectors -join ' // '))"

    $unrelatedBench = Join-Path $terminalRoot 'pr202'
    New-Item -ItemType Directory -Path $unrelatedBench -Force | Out-Null
    $script:listCode = 0
    $script:listOutput = @("worktree $($registeredBench.Replace('\', '/'))")
    $script:externalCalls = 0
    Remove-TerminalRunnerBench -RepositoryRoot 'X' -BenchRoot $terminalRoot -PullRequest '202' -State 'CLOSED'
    Assert-True -Condition (Test-Path -LiteralPath $unrelatedBench) `
        'terminal cleanup retains a bench whose exact path is not registered, even for a terminal PR'



# #902 (runner defect: the bench branch has no upstream). A bench prepared for a branch the
    # runner's clone has never held must still be able to VOUCH: `ci/gate.ps1` answers `pushed`
    # from `@{upstream}`, and `git worktree add -B <branch> <bench> <sha>` creates the branch with no
    # `branch.<b>.remote/merge`, so every manifest from such a bench said `pushed: null` and
    # merge-proof refused it (measured on #939/#947/#967/#968 from D:/orch-runner-clone; the author's
    # own repository was the special case where the branch pre-existed with tracking). The fixture
    # is a REAL bare origin and a fresh clone of it: the branch exists on origin and nowhere in the
    # clone's config, which is the general case a queue runner meets. The fake `gh` answers the real
    # sha, so the entry is not dropped and the bench is prepared; the fixture's `ci/gate.ps1` is a
    # stub that exits 0, so no gate runs. The observation is `rev-parse @{upstream}` on the bench.
    # ---------------------------------------------------------------------------------------------
    $fixture = Join-Path $root 'fixture'
    $origin = Join-Path $fixture 'origin.git'
    $author = Join-Path $fixture 'author'
    $clone = Join-Path $fixture 'runner-clone'
    $cloneFail = Join-Path $fixture 'runner-clone-fail'
    $benchRoot = Join-Path $fixture 'benches'
    $benchFailRoot = Join-Path $fixture 'benches-fail'
    $targetRoot = Join-Path $fixture 'targets'
    New-Item -ItemType Directory -Path $fixture -Force | Out-Null
    $null = & git init --bare -q $origin 2>$null
    $null = & git init -q $author 2>$null
    $null = & git -C $author config user.email 'tests@graphhelm.invalid'
    $null = & git -C $author config user.name 'gate-runner tests'
    # Declared, not inherited: a developer with commit.gpgSign and no reachable key, or a global
    # core.hooksPath whose hook refuses, would otherwise get no fixture commit and an empty
    # $fixtureHead -- and the cell below would fail on a subject that never existed. Same shape as
    # gate-manifest-provenance.tests.ps1 and the crate-input-hash fixture (#983).
    $null = & git -C $author config commit.gpgSign false
    New-Item -ItemType Directory -Path (Join-Path $author '.no-hooks') -Force | Out-Null
    $null = & git -C $author config core.hooksPath (Join-Path $author '.no-hooks')
    New-Item -ItemType Directory -Path (Join-Path $author 'ci') -Force | Out-Null
    $fixtureGate = @'
param([string] $LandingSnapshotPath)
New-Item -ItemType Directory -Path $env:CARGO_TARGET_DIR -Force | Out-Null
if ($env:RUNNER_TEST_STATE_MARKER) { Set-Content -LiteralPath $env:RUNNER_TEST_STATE_MARKER -Value 'gate reached' -Encoding ASCII }
if ($env:RUNNER_TEST_MODE -eq 'noop') { exit 0 }
if ($env:RUNNER_TEST_MODE -eq 'crash') { exit 9 }
if ($env:RUNNER_TEST_MODE -eq 'throw') { throw 'fixture gate failed before its final statement' }
New-Item -ItemType Directory -Path '.factory/gate-runs' -Force | Out-Null
$landing = if ($LandingSnapshotPath -and (Test-Path -LiteralPath $LandingSnapshotPath)) {
    Get-Content -LiteralPath $LandingSnapshotPath -Raw
} else {
    '{"missing":true}'
}
Set-Content -LiteralPath '.factory/gate-runs/fixture.json' -Value $landing -Encoding ASCII
if ($env:RUNNER_TEST_MODE -eq 'wrong-server') { git config remote.origin.receivepack $env:RUNNER_TEST_RECEIVE_PACK }
if ($env:RUNNER_TEST_MODE -eq 'mutate') { Set-Content -LiteralPath '.factory/gate-runs/fixture.json' -Value '{"overallPassed":false}' -Encoding ASCII }
if ($env:RUNNER_TEST_MODE -eq 'delete') { Remove-Item -LiteralPath '.factory/gate-runs/fixture.json' -Force }
if ($env:RUNNER_TEST_MODE -eq 'rename') { Move-Item -LiteralPath '.factory/gate-runs/fixture.json' -Destination '.factory/gate-runs/renamed.json' }
if ($env:RUNNER_TEST_MODE -eq 'mixed') { Set-Content -LiteralPath '.factory/gate-runs/fixture.json' -Value '{"overallPassed":false}' -Encoding ASCII; Set-Content -LiteralPath '.factory/gate-runs/extra.json' -Value '{}' -Encoding ASCII }
git add -A .factory/gate-runs
git commit -q -m 'gate: fixture receipt'
if ($env:RUNNER_TEST_FAIL_PUSH -eq '1') { git remote set-url origin $env:RUNNER_TEST_BAD_ORIGIN }
exit 0
'@
    Set-Content -LiteralPath (Join-Path $author 'ci/gate.ps1') -Value $fixtureGate -Encoding ASCII
    $null = & git -C $author add -A 2>$null
    $null = & git -C $author commit -q -m 'fixture: a gate that exits 0'
    $fixtureHead = (& git -C $author rev-parse HEAD).Trim()
    Assert-True -Condition ($fixtureHead -match '^[0-9a-f]{40}$') `
        -Message "CONTROL: the fixture commit exists (got '$fixtureHead')"
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$serverBranch" 2>$null
    $null = & git -C $author push -q $origin 'HEAD:refs/heads/main' 2>$null
    $null = & git clone -q --origin origin $origin $clone 2>$null
    $configured = & git -C $clone config --get "branch.$serverBranch.merge" 2>$null
    Assert-True -Condition ($LASTEXITCODE -ne 0 -and [string]::IsNullOrEmpty($configured)) `
        -Message 'CONTROL: the runner clone holds no tracking config for the branch before the run'

    # A gate without an exact PR-base snapshot cannot produce merge evidence. Drive the real runner
    # with a base object that the origin does not contain: it must stop before the gate stub runs,
    # keep the queue entry, and retain its target so the same work can be retried.
    $missingBaseClone = Join-Path $fixture 'runner-clone-missing-base'
    $missingBaseBenchRoot = Join-Path $fixture 'benches-missing-base'
    $null = & git clone -q --origin origin $origin $missingBaseClone 2>$null
    New-Item -ItemType Directory -Path $missingBaseBenchRoot -Force | Out-Null
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $missingBaseSha = 'f' * 40
    $missingBaseShim = $strictShim.Replace($serverHead, $fixtureHead).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$missingBaseSha`"}")
    Set-Content -LiteralPath $shimPath -Value $missingBaseShim -Encoding ASCII
    $missingBaseEntry = [ordered]@{ pr = 932; head = $fixtureHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $missingBasePath = Join-Path $queue "932-$($fixtureHead.Substring(0, 8))-missing-base.json"
    Set-Content -LiteralPath $missingBasePath -Value (ConvertTo-Json $missingBaseEntry) -Encoding UTF8
    $missingBaseTarget = Join-Path $targetRoot 'pr932'
    New-Item -ItemType Directory -Path $missingBaseTarget -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $missingBaseTarget 'retry.marker') -Value 'keep me' -Encoding ASCII
    Push-Location $missingBaseClone
    try {
        $missingBaseLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $missingBaseBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally { Pop-Location }
    $missingBaseStatusPath = [System.IO.Path]::ChangeExtension($missingBasePath, '.status')
    $missingBaseStatus = if (Test-Path -LiteralPath $missingBaseStatusPath) { Get-Content -LiteralPath $missingBaseStatusPath -Raw } else { '' }
    $missingBaseRemote = (& git -C $missingBaseClone ls-remote origin "refs/heads/$serverBranch" | ForEach-Object { ([string]$_ -split "`t")[0] }).Trim()
    Assert-True -Condition (Test-Path -LiteralPath $missingBasePath) `
        -Message 'missing landing proof leaves the queue entry available for retry'
    Assert-True -Condition ($missingBaseStatus -match 'waiting: landing snapshot unavailable') `
        -Message "missing landing proof records an explicit waiting status (got '$($missingBaseStatus.Trim())')"
    Assert-True -Condition ($missingBaseRemote -eq $fixtureHead) `
        -Message 'missing landing proof publishes no receipt commit to the PR branch'
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $missingBaseTarget 'retry.marker')) `
        -Message 'missing landing proof retains the target backstop for retry'

    $trackingShim = $strictShim.Replace($serverHead, $fixtureHead).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $trackingShim -Encoding ASCII
    $trackingEntry = [ordered]@{
        pr        = 933
        head      = $fixtureHead
        lane      = 'TESTS'
        timestamp = (Get-Date).ToUniversalTime().ToString('o')
    }
    # Earlier cases leave entries queued on purpose (932's, the stalled one); with -Once the runner
    # takes the oldest, so the queue is emptied before this entry is written, or this case measures them.
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $trackingEntryPath = Join-Path $queue "933-$($fixtureHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $trackingEntryPath -Value (ConvertTo-Json $trackingEntry) -Encoding UTF8
    $unrelatedTarget = Join-Path $targetRoot 'pr-unrelated'
    New-Item -ItemType Directory -Path $unrelatedTarget -Force | Out-Null

    Push-Location $clone
    try {
        $trackingLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $trackingText = ($trackingLog | Out-String)
    $bench = Join-Path $benchRoot 'pr933'
    $trackingStatusPath = [System.IO.Path]::ChangeExtension($trackingEntryPath, '.status')
    $trackingStatus = if (Test-Path -LiteralPath $trackingStatusPath) { (Get-Content -LiteralPath $trackingStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ($trackingText -match "building on $([regex]::Escape($serverBranch))") `
        -Message "the bench was prepared and the run reached the build step (status: '$trackingStatus'; runner said: '$(($trackingText -split "`r?`n" | Where-Object { $_ -match '\S' } | Select-Object -Last 3) -join ' // ')')"
    Assert-True -Condition ($trackingStatus -match 'finished rc=0 childExit=0 pushed=True') `
        -Message "the successful gate records wrapper rc=0 and childExit=0 (status: '$trackingStatus')"
    $benchHead = if (Test-Path -LiteralPath $bench) { (& git -C $bench rev-parse HEAD 2>$null) } else { '' }
    $publishedHead = (& git -C $clone ls-remote origin "refs/heads/$serverBranch" | ForEach-Object { ([string]$_ -split "`t")[0] }).Trim()
    Assert-True -Condition ("$benchHead".Trim() -eq $publishedHead) `
        -Message "the bench contains the published receipt head (expected $($publishedHead.Substring(0, 8)), got '$("$benchHead".Trim().Substring(0, [Math]::Min(8, "$benchHead".Trim().Length)))')"
    $publishedLandingText = (& git -C $clone show "$publishedHead`:.factory/gate-runs/fixture.json" 2>$null | Out-String)
    $publishedLanding = try { $publishedLandingText | ConvertFrom-Json } catch { $null }
    Assert-True -Condition ($null -ne $publishedLanding -and -not $publishedLanding.missing -and
        [int]$publishedLanding.pullRequest -eq 933 -and [string]$publishedLanding.head -eq $fixtureHead) `
        -Message "the gate receives a PR landing snapshot for pull request 933 and head $($fixtureHead.Substring(0, 8)) (got '$($publishedLandingText.Trim())')"
    Assert-True -Condition ($null -ne $publishedLanding -and [string]$publishedLanding.ref -eq 'main') `
        -Message "the landing snapshot names the live base ref main (got '$($publishedLanding.ref)')"
    Assert-True -Condition ($null -ne $publishedLanding -and [string]$publishedLanding.sha -eq $fixtureHead) `
        -Message "the landing snapshot names the live base commit $($fixtureHead.Substring(0, 8)) (got '$($publishedLanding.sha)')"
    $upstreamCode = 128
    $upstream = if (Test-Path -LiteralPath $bench) { $u = (& git -C $bench rev-parse --abbrev-ref '@{upstream}' 2>$null); $upstreamCode = $LASTEXITCODE; $u } else { '' }
    Assert-True -Condition ($upstreamCode -eq 0 -and "$upstream".Trim() -eq "origin/$serverBranch") `
        -Message "the bench branch tracks origin, so the gate can answer pushed from it (expected origin/$serverBranch, got rc=$upstreamCode '$("$upstream".Trim())')"
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $targetRoot 'pr933'))) `
        -Message 'a successful receipt push removes only this PR target after publication'
    Assert-True -Condition (Test-Path -LiteralPath $unrelatedTarget) `
        -Message 'successful publication does not remove an unrelated target directory'

    # Change the live PR shape only after the gate stub begins. The runner must re-read the server
    # before publication and retain every local retry artefact when branch/base identity changed.
    $identityClone = Join-Path $fixture 'runner-clone-identity-race'
    $identityBenchRoot = Join-Path $fixture 'benches-identity-race'
    $identityMarker = Join-Path $fixture 'identity-race.marker'
    $null = & git clone -q --origin origin $origin $identityClone 2>$null
    New-Item -ItemType Directory -Path $identityBenchRoot -Force | Out-Null
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $identityEntry = [ordered]@{ pr = 944; head = $publishedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $identityPath = Join-Path $queue "944-$($publishedHead.Substring(0, 8))-identity.json"
    Set-Content -LiteralPath $identityPath -Value (ConvertTo-Json $identityEntry) -Encoding UTF8
    $identityShim = @"
@echo off
if exist "$identityMarker" (
  echo {"headRefName":"$serverBranch-renamed","headRefOid":"$publishedHead","state":"OPEN","baseRefName":"release","baseRefOid":"$('d' * 40)"}
) else (
  echo {"headRefName":"$serverBranch","headRefOid":"$publishedHead","state":"OPEN","baseRefName":"main","baseRefOid":"$fixtureHead"}
)
exit /b 0
"@
    Set-Content -LiteralPath $shimPath -Value $identityShim -Encoding ASCII
    $env:RUNNER_TEST_STATE_MARKER = $identityMarker
    try {
        Push-Location $identityClone
        try {
            $identityLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                -BenchRoot $identityBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
        } finally { Pop-Location }
    } finally { Remove-Item Env:RUNNER_TEST_STATE_MARKER -ErrorAction SilentlyContinue }
    $identityStatusPath = [System.IO.Path]::ChangeExtension($identityPath, '.status')
    $identityStatus = if (Test-Path -LiteralPath $identityStatusPath) { Get-Content -LiteralPath $identityStatusPath -Raw } else { '' }
    $identityRemote = (& git -C $identityClone ls-remote origin "refs/heads/$serverBranch" | ForEach-Object { ([string]$_ -split "`t")[0] }).Trim()
    Assert-True -Condition (Test-Path -LiteralPath $identityPath) `
        -Message 'a branch/base mutation during the gate retains the queue entry for diagnosis'
    Assert-True -Condition ($identityStatus -match 'publication refused: pull request branch renamed') `
        -Message "a branch/base mutation records the exact publication refusal (got '$($identityStatus.Trim())')"
    Assert-True -Condition ($identityRemote -eq $publishedHead) `
        -Message 'a branch/base mutation publishes no receipt to the old source branch'
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot 'pr944')) `
        -Message 'a branch/base mutation retains the gate target backstop'

    # The same fixture then forces the push to fail only after the receipt commit exists. The target
    # must remain so the failed publication has a diagnostic backstop and can be retried.
    $failedShim = $strictShim.Replace($serverHead, $publishedHead).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $failedShim -Encoding ASCII
    $failedEntry = [ordered]@{ pr = 933; head = $publishedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $failedEntryPath = Join-Path $queue "933-$($publishedHead.Substring(0, 8))-retry.json"
    Set-Content -LiteralPath $failedEntryPath -Value (ConvertTo-Json $failedEntry) -Encoding UTF8
    $null = & git clone -q --origin origin $origin $cloneFail 2>$null
    New-Item -ItemType Directory -Path $benchFailRoot -Force | Out-Null
    $env:RUNNER_TEST_FAIL_PUSH = '1'
    $env:RUNNER_TEST_BAD_ORIGIN = 'file:///path-that-does-not-exist'
    try {
        Push-Location $cloneFail
        try {
            $failedLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                -BenchRoot $benchFailRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
        } finally {
            Pop-Location
        }
    } finally {
        Remove-Item Env:RUNNER_TEST_FAIL_PUSH -ErrorAction SilentlyContinue
        Remove-Item Env:RUNNER_TEST_BAD_ORIGIN -ErrorAction SilentlyContinue
    }
    $failedStatusPath = [System.IO.Path]::ChangeExtension($failedEntryPath, '.status')
    $failedStatus = if (Test-Path -LiteralPath $failedStatusPath) { Get-Content -LiteralPath $failedStatusPath -Raw } else { '' }
    Assert-True -Condition ($failedStatus -match 'pushed=False') `
        -Message "a receipt push failure is recorded as pushed=False (status='$($failedStatus.Trim())'; log='$($failedLog -join ' // ')')"
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot 'pr933')) `
        -Message "a failed receipt push retains this PR target as the diagnostic backstop (log='$($failedLog -join ' // ')')"

    # A fake remote accepts push (exit 0) but reports a different server head. The publication helper
    # must refuse cleanup; the exact-head response is the happy control and permits it.
    $savedInvokeExternal = (Get-Command Invoke-External).ScriptBlock
    function Get-ReceiptCommitProof { return 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }
    $script:serverHeadProbe = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
    function Invoke-External {
        param([string] $File, [string[]] $Arguments, [switch] $CaptureError)
        if ($Arguments -contains 'push') { return [pscustomobject]@{ Code = 0; Output = @() } }
        if ($Arguments -contains 'ls-remote') { return [pscustomobject]@{ Code = 0; Output = @("$script:serverHeadProbe`trefs/heads/test") } }
        return [pscustomobject]@{ Code = 0; Output = @() }
    }
    $publishStart = $runnerText.IndexOf('function Publish-Receipt', [System.StringComparison]::Ordinal)
    $publishEnd = $runnerText.IndexOf('function Remove-TerminalRunnerBench', $publishStart, [System.StringComparison]::Ordinal)
    . ([scriptblock]::Create($runnerText.Substring($publishStart, $publishEnd - $publishStart)))
    $wrongTarget = Join-Path $targetRoot 'pr934'
    New-Item -ItemType Directory -Path $wrongTarget -Force | Out-Null
    $wrongPushed = Publish-Receipt -BenchPath 'bench' -ExpectedParent 'head' -Branch 'test'
    if ($wrongPushed) { Remove-Item -LiteralPath $wrongTarget -Recurse -Force }
    Assert-True -Condition (-not $wrongPushed) -Message 'a push returning 0 with the wrong server head is recorded as pushed=False'
    Assert-True -Condition (Test-Path -LiteralPath $wrongTarget) -Message 'a push returning 0 with the wrong server head retains the target backstop'
    $script:serverHeadProbe = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    $happyTarget = Join-Path $targetRoot 'pr935'
    New-Item -ItemType Directory -Path $happyTarget -Force | Out-Null
    $happyPushed = Publish-Receipt -BenchPath 'bench' -ExpectedParent 'head' -Branch 'test'
    if ($happyPushed) { Remove-Item -LiteralPath $happyTarget -Recurse -Force }
    Assert-True -Condition ($happyPushed -and -not (Test-Path -LiteralPath $happyTarget)) -Message 'the exact server-visible receipt head permits target cleanup'
    Set-Item -Path Function:\Invoke-External -Value $savedInvokeExternal

    # Receipt commits that modify, delete, rename, or mix paths are not append-only evidence. Each
    # shape must retain its target, while the happy path above proves an add-only receipt publishes.
    foreach ($receiptMutation in @(
        [ordered]@{ Mode = 'mutate'; Pr = 940 },
        [ordered]@{ Mode = 'delete'; Pr = 941 },
        [ordered]@{ Mode = 'rename'; Pr = 942 },
        [ordered]@{ Mode = 'mixed'; Pr = 943 }
    )) {
        $mutationClone = Join-Path $fixture ("runner-clone-{0}" -f $receiptMutation.Mode)
        $mutationBenchRoot = Join-Path $fixture ("benches-{0}" -f $receiptMutation.Mode)
        $null = & git clone -q --origin origin $origin $mutationClone 2>$null
        New-Item -ItemType Directory -Path $mutationBenchRoot -Force | Out-Null
        Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
        $mutationPath = Join-Path $queue "$($receiptMutation.Pr)-$($publishedHead.Substring(0, 8))-$($receiptMutation.Mode).json"
        $mutationEntry = [ordered]@{ pr = $receiptMutation.Pr; head = $publishedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
        Set-Content -LiteralPath $mutationPath -Value (ConvertTo-Json $mutationEntry) -Encoding UTF8
        $env:RUNNER_TEST_MODE = $receiptMutation.Mode
        try {
            Push-Location $mutationClone
            try {
                $mutationLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                    -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                    -BenchRoot $mutationBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
            } finally { Pop-Location }
        } finally { Remove-Item Env:RUNNER_TEST_MODE -ErrorAction SilentlyContinue }
        $mutationStatus = if (Test-Path -LiteralPath ([System.IO.Path]::ChangeExtension($mutationPath, '.status'))) { Get-Content -LiteralPath ([System.IO.Path]::ChangeExtension($mutationPath, '.status')) -Raw } else { '' }
        Assert-True -Condition ($mutationStatus -match 'pushed=False') `
            -Message "a $($receiptMutation.Mode) receipt commit cannot report a published receipt (status='$($mutationStatus.Trim())'; log='$($mutationLog -join ' // ')')"
        Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot ("pr{0}" -f $receiptMutation.Pr))) `
            -Message "a $($receiptMutation.Mode) receipt commit retains the target backstop (log='$($mutationLog -join ' // ')')"
    }

    # No-op and child-crash controls: both create the target but publish no receipt commit. A push
    # is therefore never attempted and the target remains available for diagnosis.
    $noOpClone = Join-Path $fixture 'runner-clone-noop'
    $noOpBenchRoot = Join-Path $fixture 'benches-noop'
    $null = & git clone -q --origin origin $origin $noOpClone 2>$null
    New-Item -ItemType Directory -Path $noOpBenchRoot -Force | Out-Null
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $noOpPath = Join-Path $queue "933-$($publishedHead.Substring(0, 8))-noop.json"
    Set-Content -LiteralPath $noOpPath -Value (ConvertTo-Json $failedEntry) -Encoding UTF8
    $env:RUNNER_TEST_MODE = 'noop'
    try {
        Push-Location $noOpClone
        try {
            $noOpLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                -BenchRoot $noOpBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
        } finally { Pop-Location }
    } finally { Remove-Item Env:RUNNER_TEST_MODE -ErrorAction SilentlyContinue }
    $noOpStatus = if (Test-Path -LiteralPath ([System.IO.Path]::ChangeExtension($noOpPath, '.status'))) { Get-Content -LiteralPath ([System.IO.Path]::ChangeExtension($noOpPath, '.status')) -Raw } else { '' }
    Assert-True -Condition ($noOpStatus -match 'pushed=False') -Message 'a no-op gate cannot report a published receipt'
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot 'pr933')) -Message 'a no-op gate retains the target backstop'

    $crashClone = Join-Path $fixture 'runner-clone-crash'
    $crashBenchRoot = Join-Path $fixture 'benches-crash'
    $null = & git clone -q --origin origin $origin $crashClone 2>$null
    New-Item -ItemType Directory -Path $crashBenchRoot -Force | Out-Null
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $crashPath = Join-Path $queue "933-$($publishedHead.Substring(0, 8))-crash.json"
    Set-Content -LiteralPath $crashPath -Value (ConvertTo-Json $failedEntry) -Encoding UTF8
    $env:RUNNER_TEST_MODE = 'crash'
    try {
        Push-Location $crashClone
        try {
            $crashLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                -BenchRoot $crashBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
        } finally { Pop-Location }
    } finally { Remove-Item Env:RUNNER_TEST_MODE -ErrorAction SilentlyContinue }
    $crashStatus = if (Test-Path -LiteralPath ([System.IO.Path]::ChangeExtension($crashPath, '.status'))) { Get-Content -LiteralPath ([System.IO.Path]::ChangeExtension($crashPath, '.status')) -Raw } else { '' }
    Assert-True -Condition ($crashStatus -match 'pushed=False') -Message 'a child crash before receipt publication cannot report a published receipt'
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot 'pr933')) -Message 'a child crash retains the target backstop'

    # A TERMINATING CHILD ERROR used to skip the wrapper's final rc write and leave the parent
    # reporting `unknown`. Drive that exact path and require both the wrapper verdict and the
    # native child exit to be named in the durable status.
    $throwClone = Join-Path $fixture 'runner-clone-throw'
    $throwBenchRoot = Join-Path $fixture 'benches-throw'
    $null = & git clone -q --origin origin $origin $throwClone 2>$null
    New-Item -ItemType Directory -Path $throwBenchRoot -Force | Out-Null
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $throwPath = Join-Path $queue "933-$($publishedHead.Substring(0, 8))-throw.json"
    Set-Content -LiteralPath $throwPath -Value (ConvertTo-Json $failedEntry) -Encoding UTF8
    $env:RUNNER_TEST_MODE = 'throw'
    try {
        Push-Location $throwClone
        try {
            $throwLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
                -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
                -BenchRoot $throwBenchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
        } finally { Pop-Location }
    } finally { Remove-Item Env:RUNNER_TEST_MODE -ErrorAction SilentlyContinue }
    $throwStatus = if (Test-Path -LiteralPath ([System.IO.Path]::ChangeExtension($throwPath, '.status'))) { Get-Content -LiteralPath ([System.IO.Path]::ChangeExtension($throwPath, '.status')) -Raw } else { '' }
    Assert-True -Condition ($throwStatus -match 'finished rc=99 childExit=\d+' -and $throwStatus -notmatch 'rc=unknown') `
        -Message "a terminating gate child records a concrete wrapper rc and childExit (status='$($throwStatus.Trim())'; log='$($throwLog -join ' // ')')"
    Assert-True -Condition ($throwStatus -match 'pushed=False') -Message 'a throwing gate child cannot report a published receipt'
    $throwLogPath = if ($throwStatus -match 'log=(\S+)') { $Matches[1] } else { '' }
    $throwTranscript = if ($throwLogPath -and (Test-Path -LiteralPath $throwLogPath)) { Get-Content -LiteralPath $throwLogPath -Raw } else { '' }
    Assert-True -Condition ($throwTranscript -match 'gate child threw: fixture gate failed before its final statement') `
        -Message "the throwing child exception is appended to its runner log (path='$throwLogPath')"
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $targetRoot 'pr933')) -Message 'a throwing gate child retains the target backstop'

    # ---------------------------------------------------------------------------------------------
    # THE REFUSAL, DRIVEN. The bench now starts from origin/<branch>; if that ref is not at the
    # queued head (a fetch that lagged, a push between resolve and prepare), the runner must leave
    # the entry queued and say so, not gate a different commit under a right-looking branch name.
    # Reviewers on #979 (X, ISSUES 3) found this branch asserted by reading only. Here origin holds
    # a SECOND commit on a second branch while the fake `gh` and the entry both name the FIRST.
    # ---------------------------------------------------------------------------------------------
    $movedBranch = 'issue-902-fake-branch-moved'
    Set-Content -LiteralPath (Join-Path $author 'ci/gate.ps1') -Value 'exit 0 # moved' -Encoding ASCII
    $null = & git -C $author add -A 2>$null
    $null = & git -C $author commit -q -m 'fixture: origin moved past the queued head'
    $movedHead = (& git -C $author rev-parse HEAD).Trim()
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$movedBranch" 2>$null
    Assert-True -Condition ($movedHead -match '^[0-9a-f]{40}$' -and $movedHead -ne $fixtureHead) `
        -Message 'CONTROL: origin now carries a second commit past the queued head'
    $movedShim = $strictShim.Replace($serverHead, $fixtureHead).Replace($serverBranch, $movedBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $movedShim -Encoding ASCII
    $movedEntry = [ordered]@{
        pr        = 934
        head      = $fixtureHead
        lane      = 'TESTS'
        timestamp = (Get-Date).ToUniversalTime().ToString('o')
    }
    $movedEntryPath = Join-Path $queue "934-$($fixtureHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $movedEntryPath -Value (ConvertTo-Json $movedEntry) -Encoding UTF8
    # EXPLICIT AGES, NOT A SLEEP (Codex on #979): coarse timestamp resolution or a clock step could
    # make the second entry tie with or precede the first, and both would then receive the same
    # status, so every assertion would pass while proving nothing about the skip. The ages are set
    # on the files, and the selection order is asserted from the log below.
    $behindEntry = [ordered]@{ pr = 935; head = $fixtureHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $behindEntryPath = Join-Path $queue "935-$($fixtureHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $behindEntryPath -Value (ConvertTo-Json $behindEntry) -Encoding UTF8
    $orderNow = (Get-Date).ToUniversalTime()
    # Get-NextEntry sorts on CreationTimeUtc; setting only LastWriteTimeUtc left the order to the
    # filesystem's creation-time grain (Codex on #979). Both stamps are set, creation first.
    foreach ($pair in @(@($movedEntryPath, -60), @($behindEntryPath, 0))) {
        $item = Get-Item -LiteralPath $pair[0]
        $item.CreationTimeUtc = $orderNow.AddSeconds($pair[1])
        $item.LastWriteTimeUtc = $orderNow.AddSeconds($pair[1])
    }
    Push-Location $clone
    try {
        $movedLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $movedText = ($movedLog | Out-String)
    $movedStatusPath = [System.IO.Path]::ChangeExtension($movedEntryPath, '.status')
    $movedStatus = if (Test-Path -LiteralPath $movedStatusPath) { (Get-Content -LiteralPath $movedStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition (Test-Path -LiteralPath $movedEntryPath) `
        -Message 'origin past the queued head: the entry stays QUEUED, neither built nor dropped'
    Assert-True -Condition ($movedStatus -match 'not at the queued head') `
        -Message "and the status names the refusal (status: '$movedStatus')"
    Assert-True -Condition ($movedText -notmatch "building on $([regex]::Escape($movedBranch))") `
        -Message 'and no build was started on the moved branch'
    # AND THE LINE MOVES PAST IT. Codex on #979 (`gate-runner.ps1:333`): a refusal that returns no
    # outcome is not `stalled`, so the outer loop never adds the entry to its skip list and selects
    # the same oldest entry again at once -- no PollSeconds, nothing behind it ever runs. A second
    # entry queued behind the refused one must receive ITS OWN status in the same -Once run: it
    # reaches the runner only if the first refusal was reported as stalled and skipped.
    $behindStatusPath = [System.IO.Path]::ChangeExtension($behindEntryPath, '.status')
    $behindStatus = if (Test-Path -LiteralPath $behindStatusPath) { (Get-Content -LiteralPath $behindStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ($behindStatus -match 'not at the queued head') `
        -Message "the entry queued BEHIND the refused one was reached in the same run, so the refusal stalls rather than wedges the line (status: '$behindStatus')"
    $first934 = $movedText.IndexOf('entry 934-', [System.StringComparison]::Ordinal)
    $first935 = $movedText.IndexOf('entry 935-', [System.StringComparison]::Ordinal)
    Assert-True -Condition ($first934 -ge 0 -and $first935 -gt $first934) `
        -Message "and the OLDER entry (934) was selected before the younger one (935), so the skip was measured and not a coincidence of order (934 at $first934, 935 at $first935)"

    # ---------------------------------------------------------------------------------------------
    # A NARROW REFSPEC (Codex P2 on #979, reproduced by ISSUES 4). A clone made with --single-branch
    # carries `+refs/heads/main:refs/remotes/origin/main` and nothing else: `fetch origin <branch>`
    # then lands in FETCH_HEAD only, `origin/<branch>` never exists, and -- worse -- the two tracking
    # keys alone do not help, because `@{upstream}` maps `refs/heads/<branch>` through the CONFIGURED
    # refspec and answers "not stored as a remote-tracking branch": a bench that builds, a gate that
    # runs, a manifest that says pushed: null. The runner must make the refspec cover the branch.
    # ---------------------------------------------------------------------------------------------
    $narrow = Join-Path $fixture 'runner-clone-narrow'
    $narrowBenches = Join-Path $fixture 'benches-narrow'
    # --single-branch on the FIRST fixture branch: the refspec then names that branch alone, and no
    # remote-tracking ref for the moved branch ever existed (a normal clone followed by a narrowed
    # refspec keeps the refs it already fetched, and --prune leaves them: measured, that control read
    # green for the wrong reason).
    $null = & git clone -q --origin origin --no-checkout --single-branch --branch $serverBranch $origin $narrow 2>$null
    $narrowRefspecs = @(& git -C $narrow config --get-all remote.origin.fetch)
    $null = (& git -C $narrow rev-parse --verify --quiet "refs/remotes/origin/$movedBranch" 2>$null); $narrowHasRefCode = $LASTEXITCODE
    Assert-True -Condition ($narrowRefspecs.Count -eq 1 -and $narrowRefspecs[0] -eq "+refs/heads/${serverBranch}:refs/remotes/origin/${serverBranch}" -and $narrowHasRefCode -ne 0) `
        -Message "CONTROL: the narrow clone's refspec covers only $serverBranch and origin/$movedBranch does not exist before the run (refspecs: $($narrowRefspecs -join ' | '))"
    $narrowShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $movedBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $narrowShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $narrowEntry = [ordered]@{ pr = 936; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $narrowEntryPath = Join-Path $queue "936-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $narrowEntryPath -Value (ConvertTo-Json $narrowEntry) -Encoding UTF8
    Push-Location $narrow
    try {
        $narrowLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $narrowBenches -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $narrowBench = Join-Path $narrowBenches 'pr936'
    $narrowStatusPath = [System.IO.Path]::ChangeExtension($narrowEntryPath, '.status')
    $narrowStatus = if (Test-Path -LiteralPath $narrowStatusPath) { (Get-Content -LiteralPath $narrowStatusPath -Raw).Trim() } else { '<no status file>' }
    $narrowBenchHead = if (Test-Path -LiteralPath $narrowBench) { (& git -C $narrowBench rev-parse HEAD 2>$null) } else { '' }
    Assert-True -Condition ("$narrowBenchHead".Trim() -eq $movedHead) `
        -Message "under a narrow refspec the bench is still prepared at the queued head (status: '$narrowStatus')"
    $narrowUpstreamCode = 128
    $narrowUpstream = if (Test-Path -LiteralPath $narrowBench) { $u = (& git -C $narrowBench rev-parse --abbrev-ref '@{upstream}' 2>$null); $narrowUpstreamCode = $LASTEXITCODE; $u } else { '' }
    Assert-True -Condition ($narrowUpstreamCode -eq 0 -and "$narrowUpstream".Trim() -eq "origin/$movedBranch") `
        -Message "and its branch tracks origin -- the refspec was made to cover the branch, so @{upstream} resolves (got rc=$narrowUpstreamCode '$("$narrowUpstream".Trim())')"

    # ---------------------------------------------------------------------------------------------
    # THE BRANCH IS DELETED ON ORIGIN (Codex on #979, two threads). (a) A widening that adds a
    # branch-specific refspec line leaves it behind for good: once the branch is deleted after a
    # merge, every plain `git fetch origin` in the runner clone fails with "couldn't find remote
    # ref" and the long-lived clone cannot refresh itself. The widening must be the wildcard.
    # (b) A fetch whose failure is ignored lets `rev-parse refs/remotes/origin/<branch>` answer with
    # the ref an EARLIER run populated: the runner would gate the stale commit and, at the end, push
    # to recreate a branch somebody deleted. A failed fetch must stall the entry, never be trusted.
    # ---------------------------------------------------------------------------------------------
    $null = & git -C $author push -q $origin --delete $movedBranch 2>$null
    $null = (& git ls-remote --heads $origin $movedBranch 2>$null); $gone = [string]::IsNullOrWhiteSpace((& git ls-remote --heads $origin $movedBranch 2>$null))
    Assert-True -Condition $gone -Message "CONTROL: $movedBranch no longer exists on origin"
    $null = & git -C $narrow fetch -q origin 2>$null
    Assert-True -Condition ($LASTEXITCODE -eq 0) `
        -Message "a plain 'git fetch origin' in the runner clone still succeeds after the branch was deleted (the widening left no branch-specific refspec behind; rc=$LASTEXITCODE)"
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $staleEntry = [ordered]@{ pr = 937; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $staleEntryPath = Join-Path $queue "937-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $staleEntryPath -Value (ConvertTo-Json $staleEntry) -Encoding UTF8
    Push-Location $narrow
    try {
        $staleLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $narrowBenches -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $staleText = ($staleLog | Out-String)
    $staleStatusPath = [System.IO.Path]::ChangeExtension($staleEntryPath, '.status')
    $staleStatus = if (Test-Path -LiteralPath $staleStatusPath) { (Get-Content -LiteralPath $staleStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ((Test-Path -LiteralPath $staleEntryPath) -and ($staleStatus -match 'fetch')) `
        -Message "a fetch that fails stalls the entry with the fetch named in its status, rather than trusting the ref an earlier run left behind (status: '$staleStatus')"
    Assert-True -Condition ($staleText -notmatch "building on $([regex]::Escape($movedBranch))") `
        -Message 'and no build was started on the stale ref'

    # ---------------------------------------------------------------------------------------------
    # A BRANCH NAME THAT BEGINS WITH `--` (Codex on #979). `git check-ref-format --branch` accepts
    # `--upload-pack=nope`; a fetch that names it without `--` reads it as an option and fails, and
    # the entry stalls for a reason nobody can see. The fixture pushes such a branch to origin.
    # ---------------------------------------------------------------------------------------------
    $dashBranch = '--upload-pack=nope'
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$dashBranch" 2>$null
    $dashOnOrigin = -not [string]::IsNullOrWhiteSpace((& git ls-remote --heads $origin "refs/heads/$dashBranch" 2>$null))
    Assert-True -Condition $dashOnOrigin -Message "CONTROL: origin holds a branch named '$dashBranch'"
    $dashShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $dashBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $dashShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $dashEntry = [ordered]@{ pr = 938; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $dashEntryPath = Join-Path $queue "938-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $dashEntryPath -Value (ConvertTo-Json $dashEntry) -Encoding UTF8
    Push-Location $clone
    try {
        $dashLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $dashStatusPath = [System.IO.Path]::ChangeExtension($dashEntryPath, '.status')
    $dashStatus = if (Test-Path -LiteralPath $dashStatusPath) { (Get-Content -LiteralPath $dashStatusPath -Raw).Trim() } else { '<no status file>' }
    # git itself cannot bench such a name (`worktree add` reads it as an option behind `-B<name>` and
    # `--` alike -- measured), so the correct behaviour is a NAMED refusal before any git command
    # receives the name as a word: no fetch (the first draft of the runner ran `nope` as its
    # upload-pack), no bench, and a status a reader can act on.
    $dashBench = Join-Path $benchRoot 'pr938'
    Assert-True -Condition (($dashStatus -match 'begins with a dash') -and -not (Test-Path -LiteralPath $dashBench)) `
        -Message "a branch whose name begins with '--' is refused by name before any git command sees it, and no bench exists for it (status: '$dashStatus')"
    Assert-True -Condition (Test-Path -LiteralPath $dashEntryPath) `
        -Message 'a live pull request with a dash-prefixed branch stays queued for an operator, rather than being deleted'

    # A TERMINAL pull request with the same hostile branch spelling is safe to discard. The state
    # check must happen before the dash refusal: MERGED/CLOSED work is no longer live, and leaving
    # it queued forever makes the runner retry a branch that cannot be merged. This is deliberately
    # a distinct pull request number and head so a missing fixture branch cannot make the assertion
    # pass by consuming the live dash entry above.
    Set-Content -LiteralPath $shimPath -Value $perPrShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $terminalDashPath = Join-Path $queue "400-$($terminalDashHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $terminalDashPath -Value (ConvertTo-Json ([ordered]@{
        pr = 400; head = $terminalDashHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8
    Push-Location $clone
    try {
        $terminalDashLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $terminalDashStatusPath = [System.IO.Path]::ChangeExtension($terminalDashPath, '.status')
    $terminalDashStatus = if (Test-Path -LiteralPath $terminalDashStatusPath) {
        (Get-Content -LiteralPath $terminalDashStatusPath -Raw).Trim()
    } else { '<no status file>' }
    Assert-True -Condition (-not (Test-Path -LiteralPath $terminalDashPath)) `
        'a CLOSED pull request with a dash-prefixed branch is removed from the queue'
    Assert-True -Condition ($terminalDashStatus -match 'dropped: pull request is CLOSED') `
        "and its status records the terminal state rather than the dash refusal (status: '$terminalDashStatus')"
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $benchRoot 'pr400'))) `
        'the terminal dash entry is dropped before any bench or git operation'

    # ---------------------------------------------------------------------------------------------
    # THE UPSTREAM READ-BACK MUST NOT DEPEND ON A SPELLING (Codex P2 on #1018).
    #
    # `git rev-parse --abbrev-ref @{upstream}` is documented to return a NON-AMBIGUOUS short name.
    # A runner clone that has ever processed a pull request whose branch was literally called
    # `origin/<something>` holds a LOCAL branch by that name, and from then on the abbreviation of
    # `refs/remotes/origin/<branch>` is `remotes/origin/<branch>`. An equality against
    # `origin/<branch>` then rejects a bench that is correctly configured, removes it, and stalls
    # that entry on every retry -- the queue jam this pull request exists to remove.
    #
    # The fixture creates exactly that clone: a local branch named `origin/<branch>` beside the
    # remote-tracking ref of the same name. The CONTROL measures the abbreviation on the prepared
    # bench, so the cell fails at the assertion rather than on an arrangement nobody checked.
    # ---------------------------------------------------------------------------------------------
    $ambiguousBranch = 'issue-902-fake-branch-ambiguous'
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$ambiguousBranch" 2>$null
    $null = & git -C $clone fetch -q origin 2>$null
    $null = & git -C $clone branch "origin/$ambiguousBranch" "refs/remotes/origin/$ambiguousBranch" 2>$null
    $localShadow = (& git -C $clone rev-parse --verify --quiet "refs/heads/origin/$ambiguousBranch" 2>$null)
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace("$localShadow")) `
        -Message "CONTROL: the runner clone holds a LOCAL branch named 'origin/$ambiguousBranch' beside the remote-tracking ref of that name"
    $ambiguousShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $ambiguousBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $ambiguousShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $ambiguousEntryPath = Join-Path $queue "940-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $ambiguousEntryPath -Value (ConvertTo-Json ([ordered]@{
        pr = 940; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8
    Push-Location $clone
    try {
        $ambiguousLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
    }
    $ambiguousBench = Join-Path $benchRoot 'pr940'
    $ambiguousAbbrev = if (Test-Path -LiteralPath $ambiguousBench) {
        "$(& git -C $ambiguousBench rev-parse --abbrev-ref '@{upstream}' 2>$null)".Trim()
    } else { '<no bench>' }
    $ambiguousFull = if (Test-Path -LiteralPath $ambiguousBench) {
        "$(& git -C $ambiguousBench rev-parse --symbolic-full-name '@{upstream}' 2>$null)".Trim()
    } else { '<no bench>' }
    $ambiguousStatusPath = [System.IO.Path]::ChangeExtension($ambiguousEntryPath, '.status')
    $ambiguousStatus = if (Test-Path -LiteralPath $ambiguousStatusPath) {
        (Get-Content -LiteralPath $ambiguousStatusPath -Raw).Trim()
    } else { '<no status file>' }
    Assert-True -Condition ((($ambiguousLog | Out-String) -match "building on $([regex]::Escape($ambiguousBranch))") -and ($ambiguousStatus -notmatch 'upstream not configured')) `
        -Message "a bench whose upstream abbreviates to 'remotes/origin/<branch>' is accepted and reaches the build step, rather than being removed as unconfigured (status: '$ambiguousStatus')"
    Assert-True -Condition ($ambiguousAbbrev -eq "remotes/origin/$ambiguousBranch") `
        -Message "CONTROL: on this bench the abbreviation really is the non-ambiguous spelling, so the assertion above measures the defect and not an absent one (got '$ambiguousAbbrev')"
    Assert-True -Condition ($ambiguousFull -eq "refs/remotes/origin/$ambiguousBranch") `
        -Message "and the full symbolic name -- the one the runner compares -- has a single spelling (got '$ambiguousFull')"

    # ---------------------------------------------------------------------------------------------
    # THE SECOND READ-BACK IS A PREDICATE OF ITS OWN, and nothing pinned it (Lane B's mutation run:
    # 13 sabotages, 10 red, 3 green -- this refusal and its bench removal were two of the green).
    #
    # The runner asks `@{upstream}` twice: once after writing the two config keys, and again after
    # `--set-upstream-to` has been used to repair a write that did not take. The lock cell below
    # drives the FIRST branch, where `--set-upstream-to` itself fails. The second branch -- the
    # wiring call reports success and the upstream STILL does not read back -- had no cell, so
    # deleting the refusal, or the `worktree remove` inside it, changed nothing any test could see.
    # That is the shape of a bench left holding a branch: the next entry for it fails its own add
    # with the wrong reason, which is the jam this pull request exists to remove.
    #
    # The fixture is a git that lies in exactly that way: `--set-upstream-to` exits 0 without doing
    # anything, and `rev-parse --symbolic-full-name` answers a ref that is not the one wanted. Every
    # other vector, `worktree add` and `worktree remove` included, is the real binary, so the bench
    # really exists before the refusal and its removal is really measured.
    # ---------------------------------------------------------------------------------------------
    $liarBranch = 'issue-902-fake-branch-liar'
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$liarBranch" 2>$null
    $liarShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $liarBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $liarShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $liarEntryPath = Join-Path $queue "941-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $liarEntryPath -Value (ConvertTo-Json ([ordered]@{
        pr = 941; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') })) -Encoding UTF8
    $realGitForLiar = @(Get-Command -Name 'git.exe' -CommandType Application -ErrorAction Stop | Select-Object -First 1)[0].Source
    $liarGitShim = Join-Path $shimDirectory 'git.cmd'
    Set-Content -LiteralPath $liarGitShim -Encoding ASCII -Value @"
@echo off
if "%~3"=="branch" if "%~4"=="--set-upstream-to" exit /b 0
if "%~3"=="rev-parse" if "%~4"=="--symbolic-full-name" (
  echo refs/remotes/origin/some-other-branch
  exit /b 0
)
"$realGitForLiar" %*
exit /b %ERRORLEVEL%
"@
    $liarBench = Join-Path $benchRoot 'pr941'
    Push-Location $clone
    try {
        $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $liarGitShim -Force -ErrorAction SilentlyContinue
    }
    $liarStatusPath = [System.IO.Path]::ChangeExtension($liarEntryPath, '.status')
    $liarStatus = if (Test-Path -LiteralPath $liarStatusPath) { (Get-Content -LiteralPath $liarStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ($liarStatus -match 'upstream not configured' -and $liarStatus -match 'some-other-branch') `
        -Message "a wiring call that reports success while the upstream still reads back wrong stalls the entry, and the status carries the ref that was actually found (status: '$liarStatus')"
    Assert-True -Condition (-not (Test-Path -LiteralPath $liarBench)) `
        -Message 'and the bench is removed there too, so it does not sit holding the branch for the next entry'
    Assert-True -Condition (Test-Path -LiteralPath $liarEntryPath) `
        -Message 'and the entry is STALLED -- still queued for the next pass, not dropped'

    # ---------------------------------------------------------------------------------------------
    # THE CONFIG WRITE FAILS (Codex on #979). `.git/config` held by another process for an instant
    # makes `git config` fail while the add succeeded; with both exit codes discarded the gate would
    # run a full gate on a bench whose @{upstream} does not resolve and publish a manifest that
    # cannot vouch. The lock is simulated by git's own lock file; the runner must read the upstream
    # back, remove the bench and stall the entry.
    # ---------------------------------------------------------------------------------------------
    $lockBranch = 'issue-902-fake-branch-locked'
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$lockBranch" 2>$null
    $lockShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $lockBranch).Replace(
        '}', ",`"state`":`"OPEN`",`"baseRefName`":`"main`",`"baseRefOid`":`"$fixtureHead`"}")
    Set-Content -LiteralPath $shimPath -Value $lockShim -Encoding ASCII
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force -ErrorAction SilentlyContinue
    $lockEntry = [ordered]@{ pr = 939; head = $movedHead; lane = 'TESTS'; timestamp = (Get-Date).ToUniversalTime().ToString('o') }
    $lockEntryPath = Join-Path $queue "939-$($movedHead.Substring(0, 8)).json"
    Set-Content -LiteralPath $lockEntryPath -Value (ConvertTo-Json $lockEntry) -Encoding UTF8
    $configLock = Join-Path $clone '.git\config.lock'
    Set-Content -LiteralPath $configLock -Value 'held by the suite' -Encoding ASCII
    Assert-True -Condition (Test-Path -LiteralPath $configLock) -Message 'CONTROL: the clone''s .git/config is locked before the run'
    Push-Location $clone
    try {
        $lockLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
            -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
            -BenchRoot $benchRoot -TargetRoot $targetRoot -PollSeconds 1 2>&1
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $configLock -Force -ErrorAction SilentlyContinue
    }
    $lockStatusPath = [System.IO.Path]::ChangeExtension($lockEntryPath, '.status')
    $lockStatus = if (Test-Path -LiteralPath $lockStatusPath) { (Get-Content -LiteralPath $lockStatusPath -Raw).Trim() } else { '<no status file>' }
    Assert-True -Condition ((Test-Path -LiteralPath $lockEntryPath) -and ($lockStatus -match 'upstream')) `
        -Message "a config write that fails stalls the entry with the upstream named, instead of spending a gate on a bench that cannot vouch (status: '$lockStatus')"
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $benchRoot 'pr939'))) `
        -Message 'and the bench that could not be configured was removed, so it holds no branch'

    # ---------------------------------------------------------------------------------------------
    # #1133 ARRIVES FROM `main` AND SITS INSIDE THE GUARD. Its runner invocation below passes no
    # `-BenchRoot` and had no `Push-Location` of its own, so on `main` it runs in the repository the
    # suite lives in -- the twelfth such invocation, and the one a side-choosing resolution would
    # have left outside the containment added above. It needs no change to be covered: the guard
    # clone is this shell's location for the whole `try`, and the two reads at the end of the file
    # are taken AFTER this cell, so the developer's clone is measured across it like every other.
    # ---------------------------------------------------------------------------------------------
    # ---------------------------------------------------------------------------------------------
    # #1133: READINESS ORDER IS MEASURED IN PASSES, NOT IN REVIEW OBJECTS. This is the acceptance
    # cell for the whole change: the queue must prefer the entry somebody is waiting on. #501 is
    # OLDER and answers with twenty comment objects that are not passes -- real bodies, identity
    # lines, no verdict word -- which is exactly the shape that scored 20 on the live queue and
    # outranked a ready pull request for two weeks. #502 is YOUNGER and carries two passes at its
    # enqueued head from two lanes, neither the author.
    #
    # The assertion is on ORDER inside one -Once drain, not on which entry runs: -Once processes
    # the queue, so the observable is which head the log names FIRST.
    # ---------------------------------------------------------------------------------------------
    Get-ChildItem -LiteralPath $queue -File | Remove-Item -Force
    $head501 = '4' * 40
    $head502 = '5' * 40
    $noisy = @(1..20 | ForEach-Object {
        [pscustomobject]@{ body = "Session: chatty-lane-$_ [aaaaaa] | Head: 44444444`n`nRe-queued, will report when the gate answers." } })
    $passes502 = @(
        [pscustomobject]@{ body = "Session: reviewer-one-aaaaaa [111111] | Head: 55555555`n`n**APPROVE** - crate tests and clippy green." },
        [pscustomobject]@{ body = "Lane: T | Session: reviewer-two-bbbbbb [222222] | Head: 55555555`n`nAPPROVE-WITH-RISK - one residual, named." })
    $json501 = Join-Path $root 'api-501.json'
    $json502 = Join-Path $root 'api-502.json'
    [System.IO.File]::WriteAllText($json501, (ConvertTo-Json @($noisy) -Depth 4 -Compress))
    [System.IO.File]::WriteAllText($json502, (ConvertTo-Json @($passes502) -Depth 4 -Compress))

    $orderShim = @"
@echo off
setlocal enabledelayedexpansion
set "A1=%~1"
set "A2=%~2"
set "A3=%~3"
if "%A1%"=="api" (
  echo %A2% | findstr /C:"/502/" >nul
  if not errorlevel 1 (
    type "$json502"
  ) else (
    type "$json501"
  )
  exit /b 0
)
if "%A3%"=="501" (
  echo {"headRefName":"issue-501-noisy","headRefOid":"$head501","state":"OPEN"}
  exit /b 0
)
if "%A3%"=="502" (
  echo {"headRefName":"issue-502-ready","headRefOid":"$head502","state":"OPEN"}
  exit /b 0
)
echo unknown pull request 1>&2
exit /b 1
"@
    Set-Content -LiteralPath $shimPath -Value $orderShim -Encoding ASCII

    # #501 is written FIRST so it is the older entry: under the old scorer it won on objects, and
    # under a scorer that ignored passes entirely it would win on age. Either way it goes first,
    # which is what makes this cell able to fail.
    $entry501 = [ordered]@{ pr = 501; head = $head501; lane = 'noisy-author-lane'; enqueued_at = (Get-Date).ToUniversalTime().ToString('o') }
    Set-Content -LiteralPath (Join-Path $queue '501-44444444.json') -Value (ConvertTo-Json $entry501) -Encoding UTF8
    Start-Sleep -Milliseconds 1200
    $entry502 = [ordered]@{ pr = 502; head = $head502; lane = 'ready-author-lane'; enqueued_at = (Get-Date).ToUniversalTime().ToString('o') }
    Set-Content -LiteralPath (Join-Path $queue '502-55555555.json') -Value (ConvertTo-Json $entry502) -Encoding UTF8

    $orderLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1
    $orderText = ($orderLog | Out-String)
    $at502 = $orderText.IndexOf($head502.Substring(0, 8), [System.StringComparison]::Ordinal)
    $at501 = $orderText.IndexOf($head501.Substring(0, 8), [System.StringComparison]::Ordinal)

    Assert-True -Condition (($at502 -ge 0) -and ($at501 -ge 0)) `
        "CONTROL: the log names both heads, so the comparison below has two operands (502 at $at502, 501 at $at501)"
    Assert-True -Condition (($at502 -ge 0) -and ($at501 -ge 0) -and ($at502 -lt $at501)) `
        'the entry with two live passes is picked before the older entry with twenty verdict-less objects (#1133)'

    # ---------------------------------------------------------------------------------------------
    # THE SUITE DID NOT TOUCH THE CLONE IT RUNS IN. Read once at the top, once here, compared byte
    # for byte across EVERY cell above -- not one of them, which is how this was missed twice.
    #
    # THE NON-VACUITY IS THE THIRD READ. Two equalities over a clone that already carries the
    # wildcard would pass on a machine where the widening ran and wrote nothing new; the guard clone
    # was created NARROW, so the wildcard arriving there is positive proof that the widening
    # EXECUTED during this run and executed somewhere else. And a KNOWN POSITIVE rides along, so a
    # comparison that could never fail cannot be mistaken for a measurement.
    # ---------------------------------------------------------------------------------------------
    $guardRefspecsAfter = (& git -C $guardClone config --get-all remote.origin.fetch 2>&1 | Out-String)
    $realRefspecsAfter = (& git -C $realClone config --get-all remote.origin.fetch 2>&1 | Out-String)
    $realWorktreesAfter = (& git -C $realClone worktree list --porcelain 2>&1 | Out-String)
    # THE WORKTREE HALF IS A FILTER, NOT AN EQUALITY, and that is not a weakening. Other lanes add
    # and remove worktrees in this clone while the suite runs -- the first version of this assertion
    # went red on a count that moved from 130 to 131 for reasons that had nothing to do with it, and
    # an assertion that fails on somebody else's work would be turned off within a week. The claim
    # that belongs to this suite is that NONE of the worktrees the clone holds is the suite's own,
    # which is exactly the leak Lane B measured and is immune to what other sessions do.
    $suiteWorktreesBefore = @(($realWorktreesBefore -split "`r?`n") | Where-Object { $_ -match '^worktree ' -and $_ -like "*$(Split-Path -Leaf $root)*" })
    $suiteWorktreesAfter = @(($realWorktreesAfter -split "`r?`n") | Where-Object { $_ -match '^worktree ' -and $_ -like "*$(Split-Path -Leaf $root)*" })
    $gainedWorktrees = @(($realWorktreesAfter -split "`r?`n") | Where-Object { $_ -match '^worktree ' -and ($realWorktreesBefore -split "`r?`n") -notcontains $_ })
    Assert-True -Condition ((($realRefspecsBefore + "`n+refs/heads/x:refs/remotes/origin/x") -cne $realRefspecsBefore) -and (@(("worktree $root\pr200", 'worktree D:/elsewhere') | Where-Object { $_ -like "*$(Split-Path -Leaf $root)*" }).Count -eq 1)) `
        'CONTROL: the refspec comparison detects an added refspec, and the worktree filter matches a bench under this suite'
    Assert-True -Condition ($guardRefspecsAfter -match '\*' -and $guardRefspecsAfter -cne $guardRefspecsBefore) `
        "POSITIVE CONTROL: the widening RAN during this suite and landed in the guard clone, which started narrow -- so the two assertions below are measuring an active code path (guard refspecs now: '$($guardRefspecsAfter.Trim() -replace "`r?`n", ' // ')')"
    Assert-True -Condition ($realRefspecsAfter -ceq $realRefspecsBefore) `
        "and the developer's clone still has the refspec list it started with, byte for byte (before: '$($realRefspecsBefore.Trim() -replace "`r?`n", ' // ')'; after: '$($realRefspecsAfter.Trim() -replace "`r?`n", ' // ')')"
    Assert-True -Condition ($suiteWorktreesBefore.Count -eq 0 -and $suiteWorktreesAfter.Count -eq 0) `
        "and it registers no worktree of this suite's, before or after -- every bench this run made lives in the guard clone (the clone gained $($gainedWorktrees.Count) worktree(s) meanwhile, none of them ours: $(if ($gainedWorktrees.Count) { ($gainedWorktrees -join ' // ') } else { 'none' }))"
} finally {
    # POPPED HERE, not beside the assertions: a cell that throws must not leave this shell parked in
    # a directory the next line deletes.
    while ((Get-Location -Stack).Count -gt 0) { Pop-Location }
    $ErrorActionPreference = $previousPreference
    $env:PATH = $previousPath
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "INCOMPLETE: ran $script:total assertions, expected $ExpectedAssertionCount" -ForegroundColor Yellow
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total of $ExpectedAssertionCount" -ForegroundColor Green
exit 0
