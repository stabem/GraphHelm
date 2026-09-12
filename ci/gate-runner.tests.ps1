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
#   1  the target is removed between them          1  and the runner says so in the log
#   1  a traversing `pr` is refused as malformed    1  and the directory it aimed at survives
$ExpectedAssertionCount = 35

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
        if ($addAt -lt 0 -and $vectors[$i] -match "worktree add -B $serverBranch ") { $addAt = $i }
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

    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $sabotaged,
        '-Slot', 'HDD', '-SlotRoot', $slotRoot, '-Once',
        '-QueueDirectory', $queue, '-StateDirectory', $state, '-PollSeconds', '1')
    $spinner = Start-Process -FilePath 'powershell' -ArgumentList $arguments -PassThru -WindowStyle Hidden
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

    Assert-True -Condition ($beforeLaunch -match 'Remove-Item[^\r\n]*\$target') `
        -Message 'the previous run''s target is REMOVED between its allocation and the gate launch, so a re-run of the same pull request is cold (#943)'

    # AND IT SAYS SO IN THE LOG. Deleting tens of gigabytes takes minutes on the HDD, and a runner
    # that goes quiet for minutes with no line explaining why is indistinguishable from a wedged one
    # -- which is exactly the reading the liveness watchdog below is built to avoid making.
    Assert-True -Condition ($beforeLaunch -match 'Write-Note[^\r\n]*943') `
        -Message 'and the runner records that it did it, naming the issue, so a quiet minute in the log is explained rather than suspicious'


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
    #   pr = '\..\..\..'   Join-Path 'D:\runner-targets\hdd' -> D:\runner-targets\hdd\pr\..\..\..
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

} finally {
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
