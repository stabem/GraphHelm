# #883: a detached HEAD used to be discovered only at publish time, after all 56 stages had
# already run -- 40 minutes spent on a run that could never commit its manifest. The gate now
# refuses (or, with -AllowDetachedHead, warns and continues) before the first stage. This test
# proves the refusal fires before any stage banner, that -AllowDetachedHead is the only escape
# hatch, and that a normal branch checkout is unaffected -- using the real gate.ps1 subprocess
# for the refusal path (safe: it exits before the machine-wide slot is ever claimed) and an
# isolated precondition slice for the allowed-and-warns path (running the real gate that far
# would claim the shared slot and run the full suite, which a unit test must not do).

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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected [$Expected], got [$Actual])"
}

function Invoke-ChildPowerShell {
    param([Parameter(Mandatory)] [string] $ScriptPath, [Parameter(Mandatory)] [string] $WorkingDirectory)

    Push-Location -LiteralPath $WorkingDirectory
    try {
        $output = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $ScriptPath 2>&1 |
                ForEach-Object { [string]$_ })
        return [ordered]@{
            exitCode = $LASTEXITCODE
            output   = $output
            text     = $output -join "`n"
        }
    } finally {
        Pop-Location
    }
}

function New-FixtureRepo {
    param([Parameter(Mandatory)] [string] $Path)

    New-Item -ItemType Directory -Path $Path -Force | Out-Null
    Push-Location -LiteralPath $Path
    try {
        & git init --quiet . 2>&1 | Out-Null
        & git config user.email 'gate-detached-head-test@example.invalid' 2>&1 | Out-Null
        & git config user.name 'gate-detached-head-test' 2>&1 | Out-Null
        'fixture' | Out-File -LiteralPath (Join-Path $Path 'fixture.txt') -Encoding utf8
        & git add -A 2>&1 | Out-Null
        & git commit --quiet -m 'fixture commit' 2>&1 | Out-Null
        return (& git rev-parse HEAD).Trim()
    } finally {
        Pop-Location
    }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateLines = @(Get-Content -LiteralPath $gatePath)
$sliceStart = [Array]::IndexOf($gateLines, '$repositoryRoot = Split-Path -Parent $PSScriptRoot')
$sliceEnd = [Array]::IndexOf($gateLines, "        'manifest will not be committed (#883).') -ForegroundColor Yellow")
if ($sliceStart -lt 0 -or $sliceEnd -le $sliceStart) {
    throw 'HARNESS-BROKE: detached-HEAD precondition boundaries were not found in ci/gate.ps1'
}
# $sliceEnd names the last line of the block's BODY; the block's closing brace is one line further.
$closeBraceIndex = $sliceEnd + 1
if ($gateLines[$closeBraceIndex].Trim() -ne '}') {
    throw 'HARNESS-BROKE: expected a closing brace immediately after the detached-HEAD block body'
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-gate-detached-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$previousTargetDir = $env:CARGO_TARGET_DIR

try {
    $env:CARGO_TARGET_DIR = Join-Path $fixtureRoot 'throwaway-target'

    $branchRepo = Join-Path $fixtureRoot 'branch-repo'
    New-FixtureRepo -Path $branchRepo | Out-Null
    $detachedRepo = Join-Path $fixtureRoot 'detached-repo'
    $detachedSha = New-FixtureRepo -Path $detachedRepo
    Push-Location -LiteralPath $detachedRepo
    try { & git checkout --quiet $detachedSha 2>&1 | Out-Null } finally { Pop-Location }

    function New-PreconditionFixture {
        param([Parameter(Mandatory)] [string] $Path, [Parameter(Mandatory)] [bool] $AllowFlag)

        $body = @(
            'Set-StrictMode -Version 2.0'
            '$ErrorActionPreference = ''Stop'''
            '$PSScriptRoot = ' + "'$($PSScriptRoot.Replace("'", "''"))'"
            '$AllowDetachedHead = $' + $(if ($AllowFlag) { 'true' } else { 'false' })
            $gateLines[$sliceStart..$closeBraceIndex]
            'Write-Output ''SLICE-REACHED-END'''
            'exit 0'
        ) -join "`r`n"
        [System.IO.File]::WriteAllText($Path, $body, (New-Object System.Text.UTF8Encoding($false)))
    }

    $noFlagFixture = Join-Path $fixtureRoot 'no-flag.ps1'
    $allowFixture = Join-Path $fixtureRoot 'allow.ps1'
    New-PreconditionFixture -Path $noFlagFixture -AllowFlag $false
    New-PreconditionFixture -Path $allowFixture -AllowFlag $true

    Write-Host "`n=== Isolated precondition slice ==="

    $branchNoFlag = Invoke-ChildPowerShell -ScriptPath $noFlagFixture -WorkingDirectory $branchRepo
    Assert-Equal -Expected 0 -Actual $branchNoFlag.exitCode -Message 'a normal branch checkout, no flag, is not refused'
    Assert-True -Condition ($branchNoFlag.text -notlike '*REFUSED*') -Message 'a normal branch checkout emits no refusal'
    Assert-True -Condition ($branchNoFlag.text -notlike '*detached*') -Message 'a normal branch checkout emits no detached-HEAD warning either'

    $detachedNoFlag = Invoke-ChildPowerShell -ScriptPath $noFlagFixture -WorkingDirectory $detachedRepo
    Assert-Equal -Expected 1 -Actual $detachedNoFlag.exitCode -Message 'a detached checkout, no flag, is refused'
    Assert-True -Condition ($detachedNoFlag.text -match '\[gate\] REFUSED: HEAD is detached') -Message 'the refusal is named and legible'
    Assert-True -Condition ($detachedNoFlag.text -notlike '*SLICE-REACHED-END*') -Message 'the refusal exits before the slice''s own end sentinel'

    $detachedAllowed = Invoke-ChildPowerShell -ScriptPath $allowFixture -WorkingDirectory $detachedRepo
    Assert-Equal -Expected 0 -Actual $detachedAllowed.exitCode -Message 'a detached checkout with -AllowDetachedHead is not refused'
    Assert-True -Condition ($detachedAllowed.text -notlike '*REFUSED*') -Message '-AllowDetachedHead emits no refusal'
    Assert-True -Condition ($detachedAllowed.text -match '\[gate\] HEAD is detached and -AllowDetachedHead was passed') -Message '-AllowDetachedHead still names the fact as a warning'
    Assert-True -Condition ($detachedAllowed.text -like '*SLICE-REACHED-END*') -Message '-AllowDetachedHead lets the run continue past this check'

    Write-Host "`n=== Real gate subprocess, refusal path (BARRED from the stages, not merely expected to stop) ==="
    # THE CHILD RUNS THE REAL GATE, so what stops it must not be the guard under test. If the
    # detached-HEAD check is removed or weakened -- the regression this cell exists to catch -- the
    # child does not fail an assertion: it walks on, claims the MACHINE-WIDE slot, rewrites the
    # tracked `tools/ci-canary/src/nonce.rs` (the gate resolves its repository from its own script
    # path, not from this fixture cwd) and compiles for tens of minutes. A failing test may not cost
    # a gate slot and mutate the checkout, and the sabotage run on this PR did exactly that.
    #
    # THE BARRIER: point the slot lock at a path whose PARENT DOES NOT EXIST. `ci/gate.ps1` defaults
    # `GRAPHHELM_SLOT_LOCK_PATH` only when it is unset, and an `Enter-GateSlot` outcome that is not
    # claimed/inherited exits 1 at `[gate] NO SLOT (...)` -- before the first stage. A missing parent
    # is the deterministic form: no waiting, no contention, no handle to hold and no race with one.
    # `GRAPHHELM_SLOT_WAIT_MINUTES = 0` removes the budget for the contention case as well.
    #
    # On the path this cell asserts today the barrier is never reached, and that is the point: it
    # costs nothing while the guard holds and is the only thing standing there when it does not.
    $barredSlotLock = Join-Path (Join-Path $fixtureRoot 'no-such-slot-dir') 'SLOT.lock'
    $realGateScript = @(
        "`$env:GRAPHHELM_SLOT_LOCK_PATH = '$($barredSlotLock.Replace("'", "''"))'"
        "`$env:GRAPHHELM_SLOT_WAIT_MINUTES = '0'"
        "& '$($gatePath.Replace("'", "''"))' -SkipPostgres"
        'exit $LASTEXITCODE'
    ) -join "`r`n"
    $realGateFixture = Join-Path $fixtureRoot 'real-gate.ps1'
    [System.IO.File]::WriteAllText($realGateFixture, $realGateScript, (New-Object System.Text.UTF8Encoding($false)))
    $realDetached = Invoke-ChildPowerShell -ScriptPath $realGateFixture -WorkingDirectory $detachedRepo
    Assert-Equal -Expected 1 -Actual $realDetached.exitCode -Message 'the real gate refuses a detached checkout'
    Assert-True -Condition ($realDetached.text -match '\[gate\] REFUSED: HEAD is detached') -Message 'the real gate names the detached-HEAD refusal'
    Assert-True -Condition ($realDetached.text -notlike '*contamination canary*') -Message 'the real gate refuses before the first stage banner'
    # AND THE BARRIER MUST NOT BE WHAT REFUSED, or this cell would pass with the guard gone.
    Assert-True -Condition ($realDetached.text -notlike '*NO SLOT*') -Message 'the refusal came from the detached-HEAD check, not from the barred slot lock'

    Write-Host "`n=== Source contract ==="
    $gateText = $gateLines -join "`n"
    Assert-True -Condition ($gateText -match '\[switch\]\s*\$AllowDetachedHead') -Message 'the param block declares -AllowDetachedHead'
    $checkIndex = $gateText.IndexOf('if (-not $gatedBranchAtStart) {')
    $slotClaimIndex = $gateText.IndexOf('$script:slotOutcome = Enter-GateSlot')
    Assert-True -Condition ($checkIndex -ge 0 -and $slotClaimIndex -gt $checkIndex) -Message 'the detached-HEAD check sits before the slot is ever claimed'
    Assert-True -Condition ($gateText -match '(?s)gatedBranchExit = \$LASTEXITCODE.{0,80}Select-Object -First 1') -Message 'the capture the check reads follows the capture-then-reduce shape (#762)'
} finally {
    if ($null -eq $previousTargetDir) {
        Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    } else {
        $env:CARGO_TARGET_DIR = $previousTargetDir
    }
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
