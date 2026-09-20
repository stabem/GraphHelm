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
$ExpectedAssertionCount = 100

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

    $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state 2>&1
    Assert-True -Condition (-not (Test-Path -LiteralPath $mergedPath)) `
        'a MERGED pull request is DROPPED rather than benched -- its branch still points at the enqueued head, so the head check passes'
    $mergedStatus = $(
        $sp = [System.IO.Path]::ChangeExtension($mergedPath, '.status')
        if (Test-Path -LiteralPath $sp) { Get-Content -LiteralPath $sp -Raw } else { '' })
    Assert-True -Condition ($mergedStatus -match 'MERGED') `
        "and the status names the state it was dropped for (status: '$($mergedStatus.Trim())')"

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

    # THE PROBE IS OVERRIDDEN so disk pressure can be staged; the same shape this suite already uses
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
    $liveLog = & powershell -NoProfile -ExecutionPolicy Bypass -File $runner `
        -Slot HDD -SlotRoot $slotRoot -Once -QueueDirectory $queue -StateDirectory $state `
        -TargetRoot $fixtureTargets -BenchRoot $fixtureBenches 2>&1
    $liveCode = $LASTEXITCODE
    $liveText = ($liveLog | Out-String)
    Assert-True -Condition ($liveText -match 'is held \(live\); waiting' -and $liveCode -eq 1) `
        -Message "CONTROL: a lock naming this live process is waited on (rc=$liveCode)"

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
    function Invoke-External {
        param([string] $File, [string[]] $Arguments, [switch] $CaptureError)
        return [pscustomobject]@{ Code = $script:listCode; Output = $script:listOutput }
    }
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
    $benchRoot = Join-Path $fixture 'benches'
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
    Set-Content -LiteralPath (Join-Path $author 'ci/gate.ps1') -Value 'exit 0' -Encoding ASCII
    $null = & git -C $author add -A 2>$null
    $null = & git -C $author commit -q -m 'fixture: a gate that exits 0'
    $fixtureHead = (& git -C $author rev-parse HEAD).Trim()
    Assert-True -Condition ($fixtureHead -match '^[0-9a-f]{40}$') `
        -Message "CONTROL: the fixture commit exists (got '$fixtureHead')"
    $null = & git -C $author push -q $origin "HEAD:refs/heads/$serverBranch" 2>$null
    $null = & git clone -q --origin origin $origin $clone 2>$null
    $configured = & git -C $clone config --get "branch.$serverBranch.merge" 2>$null
    Assert-True -Condition ($LASTEXITCODE -ne 0 -and [string]::IsNullOrEmpty($configured)) `
        -Message 'CONTROL: the runner clone holds no tracking config for the branch before the run'

    $trackingShim = $strictShim.Replace($serverHead, $fixtureHead)
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
    $benchHead = if (Test-Path -LiteralPath $bench) { (& git -C $bench rev-parse HEAD 2>$null) } else { '' }
    Assert-True -Condition ("$benchHead".Trim() -eq $fixtureHead) `
        -Message "the bench sits at the entry head (expected $($fixtureHead.Substring(0, 8)), got '$("$benchHead".Trim().Substring(0, [Math]::Min(8, "$benchHead".Trim().Length)))')"
    $upstreamCode = 128
    $upstream = if (Test-Path -LiteralPath $bench) { $u = (& git -C $bench rev-parse --abbrev-ref '@{upstream}' 2>$null); $upstreamCode = $LASTEXITCODE; $u } else { '' }
    Assert-True -Condition ($upstreamCode -eq 0 -and "$upstream".Trim() -eq "origin/$serverBranch") `
        -Message "the bench branch tracks origin, so the gate can answer pushed from it (expected origin/$serverBranch, got rc=$upstreamCode '$("$upstream".Trim())')"

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
    $movedShim = $strictShim.Replace($serverHead, $fixtureHead).Replace($serverBranch, $movedBranch)
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
    $narrowShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $movedBranch)
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
    $dashShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $dashBranch)
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
    $ambiguousShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $ambiguousBranch)
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
    $liarShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $liarBranch)
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
    $lockShim = $strictShim.Replace($serverHead, $movedHead).Replace($serverBranch, $lockBranch)
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
