# ci/slot-claim.tests.ps1 -- #906: the lock is derived from the target's disk.
#
# THE CONTRADICTION THIS CLOSES. `.factory/MERGE-CHECKLIST.md` has said since #892 that there is ONE
# LOCK PER DISK -- HDD at `D:/graphhelm-slot/SLOT.lock`, SSD at `E:/graphhelm-slot/SLOT.lock` -- while
# `slot-claim.sh` had ONE default and no idea which spindle its caller was about to build on. The safe
# configuration therefore required every wrapper to set `SLOT_LOCK` by hand, and the failure of any
# single one was invisible to the others.
#
# Measured instance, 2026-09-05 14:11:43Z (#906): a lane launched with `CARGO_TARGET_DIR=E:/issues3-targets`
# and no `SLOT_LOCK`, took the default, and wrote `HELD by issues3 | ... | STATUS: gate #826 E:` into
# the file that governs D: -- while another lane already held E:. The lock's own STATUS line said E:
# and the file it lived in governed D:. Two builds on one SSD, both lanes believing they held a slot,
# arriving through the DEFAULT rather than through a race.
#
# Most cells use locks under the temp directory. The derivation cell observes the canonical path
# for a drive selected as unmounted at test setup; that observation is bounded to the setup check
# and is not an absolute guarantee against a drive being mounted concurrently.
#
# AND IT INVOKES GIT BASH BY PATH. From PowerShell, bare `bash` is WSL (#906, measured): WSL does not
# inherit Windows environment variables without WSLENV, so the holder pair reads empty and the script
# correctly refuses "no holder pair was supplied" -- a true sentence about a call that never carried
# what it was supposed to. It also cannot resolve `C:/...` as a path. A suite that invoked bare `bash`
# would measure the shell, not the script.

$ExpectedAssertionCount = 31

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0
function Assert-True {
    param([bool] $Condition, [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$claimScript = Join-Path $repositoryRoot '.factory/tools/slot-claim.sh'
if (-not (Test-Path -LiteralPath $claimScript)) {
    Write-Host "HARNESS-BROKE: $claimScript is not where this suite expects it" -ForegroundColor Magenta
    exit 2
}

# GIT BASH, never bare `bash`, and FOUND FROM THE INSTALLED GIT rather than from a list of
# directories. Three hardcoded paths cover the default installer and nothing else: a per-user
# install, Scoop, or PortableGit has a perfectly good Git bash somewhere else, and this suite is
# pinned in run-ps-suites -- so a wrong guess makes the AUTHORITATIVE GATE report HARNESS-BROKE
# before it tests anything (Codex P1 on #1030). `git.exe` is on PATH by definition for anyone
# who can work here, and bash sits beside it or one level up.
#
# AND IT IS PROVED NOT TO BE WSL, because that is the whole point: from PowerShell bare `bash`
# is WSL, which inherits no Windows environment and cannot resolve `C:/...`. `uname -s` answers
# MINGW64_NT/MSYS_NT for Git bash and Linux for WSL, so the shell says which it is rather than
# the path being trusted to imply it.
function Resolve-GitBash {
    param([string] $GitPath, [string] $ExecPath)
    # A package-manager shim is not the installation: Get-Command git may return
    # scoop/shims/git.exe while Git itself knows its real libexec path. Resolve from
    # Git's --exec-path first, then walk the shim only as a diagnostic fallback.
    $candidates = New-Object System.Collections.Generic.List[string]
    foreach ($start in @($ExecPath, (Split-Path -Parent $GitPath))) {
        $walk = $start
        for ($level = 0; ($level -lt 6) -and $walk; $level++) {
            $candidates.Add((Join-Path $walk 'bash.exe'))
            $candidates.Add((Join-Path $walk 'bin\bash.exe'))
            $candidates.Add((Join-Path $walk 'usr\bin\bash.exe'))
            $walk = Split-Path -Parent $walk
        }
    }
    return ($candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1)
}

$gitCommand = Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
$gitBash = $null
if ($gitCommand) {
    $gitExecPath = (& $gitCommand.Source --exec-path 2>$null | Select-Object -First 1)
    if ($gitExecPath) { $gitExecPath = $gitExecPath.Trim() }
    $gitBash = Resolve-GitBash -GitPath $gitCommand.Source -ExecPath $gitExecPath
}
if (-not $gitBash) {
    # `$gitCommand` is null when git is absent, and under StrictMode a property read on null THROWS
    # -- so the diagnostic written for a missing bash would itself crash the suite instead of
    # printing HARNESS-BROKE (lane S on #1030).
    $gitSource = if ($gitCommand) { $gitCommand.Source } else { '<git not on PATH>' }
    Write-Host "HARNESS-BROKE: no bash found beside git ($gitSource); bare 'bash' is WSL here and cannot make a claim" -ForegroundColor Magenta
    exit 2
}
$kernel = (& $gitBash -c 'uname -s' 2>$null | Out-String).Trim()
if ($kernel -notmatch '^(MINGW|MSYS)') {
    Write-Host "HARNESS-BROKE: $gitBash reports kernel '$kernel'; that is not Git bash, and WSL cannot carry the holder pair or resolve C:/ paths" -ForegroundColor Magenta
    exit 2
}

function Invoke-Claim {
    param([hashtable] $Environment, [string[]] $ClaimArgs)
    $saved = @{}
    foreach ($name in @('SLOT_LOCK', 'SLOT_LOCK_DEFAULT', 'SLOT_LOG', 'SLOT_TARGET', 'CARGO_TARGET_DIR', 'GRAPHHELM_SLOT_LOCK_PATH', 'GRAPHHELM_HOLDER_PID', 'GRAPHHELM_HOLDER_START')) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
        [Environment]::SetEnvironmentVariable($name, $null)
    }
    foreach ($name in $Environment.Keys) { [Environment]::SetEnvironmentVariable($name, $Environment[$name]) }
    # `2>&1` ON A NATIVE COMMAND IS TERMINATING under `$ErrorActionPreference = 'Stop'`: PowerShell
    # wraps each stderr line in an ErrorRecord and throws NativeCommandError, so a script that
    # REFUSES -- which is most of what this suite measures -- would kill the suite instead of
    # returning its message. Lowered for the call and restored immediately, rather than for the
    # file, so an unexpected failure anywhere else still stops.
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $gitBash $claimScript @ClaimArgs 2>&1 | Out-String
        return [pscustomobject]@{ Output = $output; ExitCode = $LASTEXITCODE }
    } finally {
        $ErrorActionPreference = $previousPreference
        foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    }
}

function Get-DriveBackedScratchRoot {
    param([string] $TemporaryRoot, [string] $LocalApplicationData)
    # TEMP may be a valid UNC share, but this suite deliberately compares drive-letter locks.
    # Choose a local scratch root before deriving a drive instead of treating '\' as a letter.
    if ($TemporaryRoot -match '^[A-Za-z]:[\\/]') { return $TemporaryRoot }
    if ($LocalApplicationData -match '^[A-Za-z]:[\\/]') {
        return Join-Path $LocalApplicationData 'Temp'
    }
    throw 'HARNESS-BROKE: drive-letter lock controls need a drive-backed temporary directory'
}
$scratchRoot = Get-DriveBackedScratchRoot -TemporaryRoot ([System.IO.Path]::GetTempPath()) `
    -LocalApplicationData ([Environment]::GetFolderPath('LocalApplicationData'))
$scratch = Join-Path $scratchRoot ("slot-906-" + [Guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($scratch) | Out-Null
$scratchDrive = ([System.IO.Path]::GetPathRoot($scratch)).Substring(0, 1).ToUpperInvariant()
$holder = @{
    GRAPHHELM_HOLDER_PID   = "$PID"
    GRAPHHELM_HOLDER_START = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
    SLOT_LOG               = (Join-Path $scratch 'activity.log')
}

try {
    Assert-True ((Get-DriveBackedScratchRoot '\\server\share\temp' 'C:\Users\fixture\AppData\Local') -eq 'C:\Users\fixture\AppData\Local\Temp') `
        'a UNC TEMP selects local application data before extracting a drive'
    Assert-True ((Get-DriveBackedScratchRoot 'E:\temp' 'C:\Users\fixture\AppData\Local') -eq 'E:\temp') `
        'a drive-backed TEMP keeps its original drive'
    Write-Host "#906: the control -- this suite can make a claim at all"

    $lockA = (Join-Path $scratch 'A.lock') -replace '\\', '/'
    $first = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = $lockA; SLOT_TARGET = "$scratchDrive`:/anything" }) -ClaimArgs @('m', 'M', 'suite control')
    Assert-True ($first.ExitCode -eq 0) `
        "CONTROL: a claim on a temp lock succeeds (exit $($first.ExitCode)); every absence below is about a script that runs. Output: $($first.Output)"
    Assert-True (Test-Path -LiteralPath (Join-Path $scratch 'A.lock')) `
        'and the lock file exists afterwards, so the claim was a write and not a message'

    # These calls are sequential. They prove occupied-lock refusal and preserve the first holder;
    # they do not claim to be a concurrent race proof. The atomic create-or-fail primitive is the
    # production exclusion mechanism, while a real parallel stress test remains a separate check.
    Write-Host "#906: a second claim on the SAME slot is refused"

    $second = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = $lockA; SLOT_TARGET = "$scratchDrive`:/anything" }) -ClaimArgs @('other', 'X', 'second claim')
    Assert-True ($second.ExitCode -ne 0) `
        "the second claim on the same path is REFUSED (exit $($second.ExitCode)) -- absence is the only spelling of free"
    Assert-True ($second.Output -notlike '*HELD by other*') `
        'and the refusal did not overwrite the first holder'

    Write-Host "#906: two claims on DIFFERENT slots both succeed -- the positive control"

    $lockB = (Join-Path $scratch 'B.lock') -replace '\\', '/'
    $other = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = $lockB; SLOT_TARGET = "$scratchDrive`:/anything" }) -ClaimArgs @('m', 'M', 'other slot')
    Assert-True ($other.ExitCode -eq 0) `
        "a claim on a DIFFERENT path succeeds while the first is held (exit $($other.ExitCode)) -- without this, the refusal above could be a broken script rather than exclusion"

    Write-Host "#906: the lock is DERIVED from the target's drive when SLOT_LOCK is unset"

    # AN UNMOUNTED LETTER, FOUND AT RUNTIME. The first version hardcoded `Q:` and asserted it was
    # absent -- and that assertion GATED NOTHING: Assert-True records a failure and execution
    # continues, so on a host with Q: mounted the cell below would have gone on to create a real
    # `Q:/graphhelm-slot/SLOT.lock` that the cleanup does not remove (Codex P1 on #1030). A check
    # that does not gate is not a control. Scanning for a letter nobody has mounted removes the
    # assumption instead of asserting it, and the harness refuses if there is none.
    $freeLetter = ([char[]](90..65 | ForEach-Object { [char]$_ })) |
        Where-Object { -not (Test-Path -LiteralPath "$($_):\") } | Select-Object -First 1
    if (-not $freeLetter) {
        Write-Host 'HARNESS-BROKE: every drive letter is mounted; the derivation cell has nowhere safe to point' -ForegroundColor Magenta
        exit 2
    }
    Assert-True (-not (Test-Path -LiteralPath "$($freeLetter):\")) `
        "CONTROL: $($freeLetter): is not mounted, so the derivation cell cannot reach a real slot"

    # AND THE FALLBACK IS MADE HARMLESS, because a BROKEN derivation lands on it. Sabotaging the
    # derivation once sent this cell to the machine's real `D:/graphhelm-slot/SLOT.lock`; it
    # refused only because that slot happened to be held at the time. Free, it would have claimed
    # the HDD slot and blocked the fleet -- a suite whose sabotage mode is 'take the machine's
    # slot' is not one anyone should run.
    $derived = Invoke-Claim -Environment ($holder + @{ CARGO_TARGET_DIR = "$($freeLetter):/some-targets"; SLOT_LOCK_DEFAULT = (Join-Path $scratch 'fallback.lock').Replace([char]92, [char]47) }) -ClaimArgs @('m', 'M', 'derivation')
    Assert-True ($derived.Output -like "*$($freeLetter):/graphhelm-slot/SLOT.lock*") `
        "an unset SLOT_LOCK with a target on $($freeLetter): derives $($freeLetter):/graphhelm-slot/SLOT.lock (got: $($derived.Output))"
    Assert-True ($derived.Output -notlike '*D:/graphhelm-slot*') `
        'and does NOT fall back to the HDD default, which is the measured defect: a lock on D: governing a build on another disk'

    Write-Host "#906: an MSYS path names a disk too, and a target that names none is REFUSED"

    # THE FORM THIS SCRIPT IS MOST LIKELY TO BE HANDED. It runs under Git bash, where the same
    # directory is spelled /z/some-targets, and a wrapper exporting CARGO_TARGET_DIR from inside
    # bash hands over exactly that. Missing it made the drive undetectable and the lock fall back
    # to D: -- re-creating, for the one path form nobody tested, the defect this change removes,
    # and doing it while the checklist now tells wrappers NOT to name the lock (Codex P1 on #1030).
    $msys = Invoke-Claim -Environment ($holder + @{ CARGO_TARGET_DIR = "/$($freeLetter.ToString().ToLowerInvariant())/some-targets"; SLOT_LOCK_DEFAULT = (Join-Path $scratch ('fallback-msys.lock')).Replace([char]92, [char]47) }) -ClaimArgs @('m', 'M', 'msys form')
    Assert-True ($msys.Output -like "*$($freeLetter):/graphhelm-slot/SLOT.lock*") `
        "an MSYS target /$($freeLetter.ToString().ToLowerInvariant())/... derives the same lock as $($freeLetter):/... (got: $($msys.Output))"

    # AND A TARGET WHOSE DISK CANNOT BE READ IS REFUSED RATHER THAN DEFAULTED. Falling back is
    # what the checklist now forbids a wrapper from doing by hand, so the script must not do it
    # silently either. Safe because a target WAS supplied: a wrapper that supplies none still
    # gets the default and is untouched, which the cell below this one holds.
    $noDisk = Invoke-Claim -Environment ($holder + @{ CARGO_TARGET_DIR = 'relative/targets'; SLOT_LOCK_DEFAULT = (Join-Path $scratch ('fallback-nodisk.lock')).Replace([char]92, [char]47) }) -ClaimArgs @('m', 'M', 'no disk')
    Assert-True ($noDisk.ExitCode -eq 6) `
        "a target naming no disk is refused (got $($noDisk.ExitCode)) instead of quietly taking the default, which would be a claim on one disk for a build on another"
    Assert-True (($noDisk.Output -like '*no disk this script can identify*') -and ($noDisk.Output -like '*relative/targets*')) `
        "and the refusal names the target it could not place, and the forms it understands (got: $($noDisk.Output))"

    Write-Host "#906: an explicit SLOT_LOCK on a different disk than the target is REFUSED"

    foreach ($targetCase in @(
        @{ Variable = 'CARGO_TARGET_DIR'; Target = 'relative/targets'; File = 'unknown-relative.lock' },
        @{ Variable = 'SLOT_TARGET'; Target = '//server/share/targets'; File = 'unknown-unc.lock' }
    )) {
        $explicitUnknownPath = (Join-Path $scratch $targetCase.File).Replace([char]92, [char]47)
        $caseEnvironment = $holder + @{ SLOT_LOCK = $explicitUnknownPath }
        $caseEnvironment[$targetCase.Variable] = $targetCase.Target
        $explicitUnknown = Invoke-Claim -Environment $caseEnvironment -ClaimArgs @('m', 'M', 'unknown target with explicit lock')
        $created = Test-Path -LiteralPath $explicitUnknownPath
        Assert-True ($explicitUnknown.ExitCode -eq 6 -and -not $created -and $explicitUnknown.Output -like '*target names no disk*') `
            "an explicit lock cannot bypass an unknown $($targetCase.Variable) target (exit $($explicitUnknown.ExitCode), lock created: $created)"
    }

    $mismatch = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = $lockB; CARGO_TARGET_DIR = "$($freeLetter):/some-targets" }) -ClaimArgs @('m', 'M', 'mismatch')
    Assert-True ($mismatch.ExitCode -eq 6) `
        "a lock and a target on different disks is refused with its own exit code (got $($mismatch.ExitCode)): the caller stated two slots in one breath and one of them is wrong"
    Assert-True (($mismatch.Output -like '*different disks*') -and ($mismatch.Output -like "*$($freeLetter):*") -and ($mismatch.Output -like "*$scratchDrive*")) `
        "and the refusal NAMES both drives rather than saying the configuration is invalid (got: $($mismatch.Output))"

    Write-Host "#906: an explicit lock with no identifiable disk is REFUSED when the target disk is known"
    $unknownLock = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = 'relative/SLOT.lock'; CARGO_TARGET_DIR = "$($freeLetter):/some-targets" }) -ClaimArgs @('m', 'M', 'unknown lock disk')
    Assert-True ($unknownLock.ExitCode -eq 6) `
        "an explicit lock with no disk is refused (got $($unknownLock.ExitCode)) instead of silently governing the target from an unknown location"
    Assert-True (($unknownLock.Output -like '*explicit lock names no disk*') -and ($unknownLock.Output -like '*relative/SLOT.lock*')) `
        "and the refusal names the unplaced explicit lock (got: $($unknownLock.Output))"

    Write-Host "#906: two target variables that name different disks are REFUSED"

    # PRECEDENCE IS NOT AGREEMENT. `SLOT_TARGET` wins over `CARGO_TARGET_DIR`, so a wrapper that
    # INHERITS one and sets the other states two disks in one breath and the script would have
    # derived (or validated) the lock for the disk cargo is NOT building on -- the measured #906
    # shape again, reached through precedence instead of through a default (Codex P1 on #1030).
    # The lock must answer for the disk the build actually lands on, so disagreement refuses.
    $conflictLock = (Join-Path $scratch 'conflict.lock').Replace([char]92, [char]47)
    $conflict = Invoke-Claim -Environment ($holder + @{ SLOT_TARGET = "$($freeLetter):/targets"; CARGO_TARGET_DIR = "$scratchDrive`:/targets"; SLOT_LOCK = $conflictLock }) -ClaimArgs @('m', 'M', 'conflicting targets')
    Assert-True (($conflict.ExitCode -eq 6) -and (-not (Test-Path -LiteralPath $conflictLock))) `
        "SLOT_TARGET and CARGO_TARGET_DIR on different disks are refused before any claim (exit $($conflict.ExitCode))"
    Assert-True (($conflict.Output -like '*SLOT_TARGET*') -and ($conflict.Output -like '*CARGO_TARGET_DIR*') -and ($conflict.Output -like "*$($freeLetter):*") -and ($conflict.Output -like "*$scratchDrive*")) `
        "and the refusal names BOTH variables and BOTH disks rather than silently preferring one (got: $($conflict.Output))"

    # An unplaceable second variable is the same defect with one drive missing: the lock would be
    # derived from the readable one while cargo builds wherever the other resolves.
    $conflictUnknownLock = (Join-Path $scratch 'conflict-unknown.lock').Replace([char]92, [char]47)
    $conflictUnknown = Invoke-Claim -Environment ($holder + @{ SLOT_TARGET = "$scratchDrive`:/targets"; CARGO_TARGET_DIR = 'relative/targets'; SLOT_LOCK = $conflictUnknownLock }) -ClaimArgs @('m', 'M', 'unplaceable second target')
    Assert-True (($conflictUnknown.ExitCode -eq 6) -and (-not (Test-Path -LiteralPath $conflictUnknownLock))) `
        "a readable SLOT_TARGET cannot mask an unplaceable CARGO_TARGET_DIR (exit $($conflictUnknown.ExitCode), lock created: $(Test-Path -LiteralPath $conflictUnknownLock))"

    # AND AGREEMENT IS NOT PUNISHED, including across spellings: the refusal is about the DISK,
    # not about the two variables being present, and a Git bash wrapper spells the same directory
    # /x/... while a PowerShell one spells it X:/... .
    $agreeLock = (Join-Path $scratch 'agree.lock').Replace([char]92, [char]47)
    $agree = Invoke-Claim -Environment ($holder + @{ SLOT_TARGET = "$scratchDrive`:/targets"; CARGO_TARGET_DIR = "/$($scratchDrive.ToLowerInvariant())/targets"; SLOT_LOCK = $agreeLock }) -ClaimArgs @('m', 'M', 'agreeing targets')
    Assert-True ($agree.ExitCode -eq 0) `
        "the same disk spelled X:/... and /x/... in the two variables still claims (exit $($agree.ExitCode)): the check reads disks, not strings"

    Write-Host "#906: the variable the GATE reads is checked against the claim this run makes"

    # THE CLAIM IS NOT THE GATE. `ci/slot-lock.ps1:345` returns GRAPHHELM_SLOT_LOCK_PATH verbatim
    # and `ci/gate.ps1:4509` claims that file, so a wrapper that forwards the wrong one splits the
    # claim and the gate across two disks -- silently, and after this script has already said yes
    # (lane S on #1030). Every path below is a FIXTURE under the scratch directory; a real
    # D:/ or E:/graphhelm-slot path is never named here, because a suite that can take the
    # machine's slot is not one anyone should run.
    # The direction is chosen so the RED is the REAL defect: the claim lands on a writable fixture
    # while the gate's variable points at another disk. Unfixed, that exits 0 AND CREATES the lock
    # -- a claim held here and a gate waiting there. The gate-side path names an unmounted letter,
    # so even a regression cannot touch a real slot.
    $splitLock = (Join-Path $scratch 'split.lock').Replace([char]92, [char]47)
    $split = Invoke-Claim -Environment ($holder + @{ CARGO_TARGET_DIR = "$scratchDrive`:/targets"; SLOT_LOCK = $splitLock; GRAPHHELM_SLOT_LOCK_PATH = "$($freeLetter):/graphhelm-slot/SLOT.lock" }) -ClaimArgs @('m', 'M', 'split gate lock')
    Assert-True (($split.ExitCode -eq 6) -and (-not (Test-Path -LiteralPath $splitLock))) `
        "a GRAPHHELM_SLOT_LOCK_PATH on another disk than the claim is refused (exit $($split.ExitCode), lock created: $(Test-Path -LiteralPath $splitLock))"
    Assert-True (($split.Output -like '*GRAPHHELM_SLOT_LOCK_PATH*') -and ($split.Output -like "*$($freeLetter):*") -and ($split.Output -like "*$scratchDrive*")) `
        "and the refusal names the gate's variable and both disks (got: $($split.Output))"

    # Forwarding the SAME disk is the configuration the checklist requires, and it must still claim.
    $forwardLock = (Join-Path $scratch 'forward.lock').Replace([char]92, [char]47)
    $forward = Invoke-Claim -Environment ($holder + @{ CARGO_TARGET_DIR = "$scratchDrive`:/targets"; SLOT_LOCK = $forwardLock; GRAPHHELM_SLOT_LOCK_PATH = $forwardLock }) -ClaimArgs @('m', 'M', 'forwarded gate lock')
    Assert-True ($forward.ExitCode -eq 0) `
        "forwarding the same path the claim takes still claims (exit $($forward.ExitCode)): the check is about disagreement, not about the variable being set"

    Write-Host "#906: derivation, not refusal, when SLOT_LOCK is simply unset"

    # The direction matters: refusing an unset SLOT_LOCK would hard-stop every lane whose wrapper has
    # not been updated, which is a worse failure than the one this closes.
    $lockC = (Join-Path $scratch 'C.lock') -replace '\\', '/'
    $sameDisk = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = $lockC; CARGO_TARGET_DIR = "$scratchDrive`:/targets" }) -ClaimArgs @('m', 'M', 'same disk')
    Assert-True ($sameDisk.ExitCode -eq 0) `
        "a lock and a target on the SAME disk proceed (exit $($sameDisk.ExitCode)) -- the refusal fires on disagreement, not on the presence of a target"

    $noTarget = Invoke-Claim -Environment ($holder + @{ SLOT_LOCK = (Join-Path $scratch 'D.lock') -replace '\\', '/' }) -ClaimArgs @('m', 'M', 'no target')
    Assert-True ($noTarget.ExitCode -eq 0) `
        "a claim with NO target at all still works (exit $($noTarget.ExitCode)) -- a wrapper that changes nothing keeps claiming"

    Write-Host "#906: the WSL trap, pinned in this suite's own invocation"

    # Hermetic discovery proof: a Scoop shim path must resolve Bash from Git's own
    # installation path, without executing or depending on a real Scoop install.
    $scoopRoot = Join-Path $scratch 'scoop/apps/git/current'
    $scoopBash = Join-Path $scoopRoot 'bin/bash.exe'
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $scoopBash) | Out-Null
    [System.IO.File]::WriteAllText($scoopBash, 'fixture')
    $resolvedScoop = Resolve-GitBash -GitPath (Join-Path $scratch 'scoop/shims/git.exe') -ExecPath (Join-Path $scoopRoot 'mingw64/libexec/git-core')
    Assert-True ($resolvedScoop -eq $scoopBash) `
        'a Scoop shim is resolved through Git --exec-path to the real installation Bash, rather than only through the shim ancestors'
    Assert-True ((& $gitBash -c 'uname -s' 2>$null | Out-String).Trim() -match '^(MINGW|MSYS)') `
        'the selected Bash reports a Git Bash kernel, so the resolver cannot silently select WSL'

    $claimText = [System.IO.File]::ReadAllText($claimScript)
    Assert-True ($claimText.Contains('ONE LOCK PER DISK')) `
        'and the script itself names the rule it enforces, so a lane reading it meets the convention rather than a bare default'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $scratch -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount. A case vanished or was added without updating the declared total." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total/$script:total" -ForegroundColor Green
