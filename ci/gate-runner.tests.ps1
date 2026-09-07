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
#   1  ARRANGEMENT: the selection really does pass the skip list, so removing it is a sabotage
#   1  THE FAILURE MODE IS A FAILURE: with the exclusion removed, the run EXITS rather than spins
#   1  and it exits 3, the code that says the skip list stopped excluding
$ExpectedAssertionCount = 17

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
