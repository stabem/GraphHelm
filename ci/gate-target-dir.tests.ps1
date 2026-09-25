# #166: focused trap for the gate's earliest precondition. The test executes only the
# precondition slice for accepted values, so proving SET-ONLY semantics never starts cargo or
# consumes the global gate slot. It also invokes the real gate once with the variable unset to
# prove refusal happens before any stage banner.
#
# WHICH SHAPE THIS IS, AND WHAT THE SHAPE CANNOT CATCH (#191). Two arms, and they fail differently:
#
#   REFUSING values (blank/empty/unset) run the REAL `ci/gate.ps1` through a wrapper. This is the
#   arm of this suite that executes the real gate rather than a slice or fixture, and it stays
#   cheap because the door exits before cargo. It therefore covers the door's WIRING --
#   that `gate.ps1` actually reaches this check first -- not merely the predicate's logic.
#
#   ACCEPTED values run a SLICE of the precondition block, not the gate. A slice proves the
#   predicate says yes; it does not execute the gate's accepted-value continuation. The source
#   contract below also checks the extracted guard, so deleting that guard is detected. The
#   accepted arm is not exercised against the real gate for a structural reason, not an oversight:
#   an accepted value means the gate proceeds, so the assertion would cost a full gate run and the
#   global slot.
#
#   Nothing in this suite executes the real gate past the door. The production runner does;
#   its invocation is not a test assertion. This suite's extracted functions and fixtures do not
#   establish that the real gate enforces later-stage behavior (the stage list, the contamination
#   canary's abort, or the manifest rule). Proving that wiring needs an observer at those decision
#   points without running the full stages. This is a limit of this suite, not a census of all CI.
#
# Sabotage receipt for the assertions below (2026-09-07, `95a7ad9d`): replacing the refusal line
# `[gate] REFUSED: CARGO_TARGET_DIR is unset or blank.` with an unnamed `[gate] Configuration
# problem.` -- keeping `exit 1` -- takes this suite from 29/29 to 26/29, and the three that fall are
# exactly the "named and legible" assertions while every exit-code assertion stays green. That is
# the reason these assert on the refusal TEXT: an exit code cannot separate "refused at the door"
# from "ran and failed", so a code-only guard passes a gate that has stopped saying why.

$ExpectedAssertionCount = 31
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
    param([Parameter(Mandatory)] [string] $ScriptPath)

    $output = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $ScriptPath 2>&1 |
            ForEach-Object { [string]$_ })
    return [ordered]@{
        exitCode = $LASTEXITCODE
        output   = $output
        text     = $output -join "`n"
    }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateLines = @(Get-Content -LiteralPath $gatePath)
$preconditionStart = [Array]::IndexOf(
    $gateLines,
    '$repositoryRoot = Split-Path -Parent $PSScriptRoot'
)
$preconditionEnd = [Array]::IndexOf($gateLines, '$toolchain = ''+1.97.1''')
if ($preconditionStart -lt 0 -or $preconditionEnd -le $preconditionStart) {
    throw 'HARNESS-BROKE: target-dir precondition boundaries were not found in ci/gate.ps1'
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-gate-target-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$preconditionFixture = Join-Path $fixtureRoot 'precondition.ps1'
$realGateFixture = Join-Path $fixtureRoot 'real-gate.ps1'
$previousTargetDir = $env:CARGO_TARGET_DIR

try {
    $precondition = @(
        "Set-StrictMode -Version 2.0"
        '$ErrorActionPreference = ''Stop'''
        '$PSScriptRoot = ' + "'$($PSScriptRoot.Replace("'", "''"))'"
        $gateLines[$preconditionStart..($preconditionEnd - 1)]
        'Write-Output (''ENCODED='' + (ConvertTo-SlotEventField -Value $env:CARGO_TARGET_DIR))'
        'exit 0'
    ) -join "`r`n"
    [System.IO.File]::WriteAllText(
        $preconditionFixture,
        $precondition,
        (New-Object System.Text.UTF8Encoding($false))
    )
    $realGateScript = @(
        "& '$($gatePath.Replace("'", "''"))' -SkipPostgres"
        'exit $LASTEXITCODE'
    ) -join "`r`n"
    [System.IO.File]::WriteAllText(
        $realGateFixture,
        $realGateScript,
        (New-Object System.Text.UTF8Encoding($false))
    )

    Write-Host "`n=== SET-ONLY acceptance ==="
    $acceptedValues = @(
        'C:/explicit-isolated-target',
        'C:\explicit-isolated-target',
        '\\server\share\graphhelm-target',
        '.\relative-target',
        'd:\MiXeD\Target',
        'D:\trailing-target\'
    )
    foreach ($acceptedValue in $acceptedValues) {
        $env:CARGO_TARGET_DIR = $acceptedValue
        $accepted = Invoke-ChildPowerShell -ScriptPath $preconditionFixture
        Assert-Equal -Expected 0 -Actual $accepted.exitCode -Message "explicit target form [$acceptedValue] passes the precondition"
        Assert-True -Condition ($accepted.text -notlike '*REFUSED*') -Message "explicit target form [$acceptedValue] emits no refusal"
    }

    Write-Host "`n=== Slot-event field encoding ==="
    $injected = "D:\safe`r`n2026-01-01 | gate | FORGED | payload"
    $env:CARGO_TARGET_DIR = $injected
    $encoded = Invoke-ChildPowerShell -ScriptPath $preconditionFixture
    Assert-Equal -Expected 0 -Actual $encoded.exitCode -Message 'a nonblank target containing line breaks still passes SET-ONLY validation'
    $encodedLines = @($encoded.output | Where-Object { $_ -like 'ENCODED=*' })
    Assert-Equal -Expected 1 -Actual $encodedLines.Count -Message 'slot-event encoding remains one physical output line'
    $decoded = if ($encodedLines.Count -eq 1) {
        [System.Text.Encoding]::UTF8.GetString(
            [System.Convert]::FromBase64String($encodedLines[0].Substring('ENCODED='.Length))
        )
    } else {
        $null
    }
    Assert-Equal -Expected $injected -Actual $decoded -Message 'slot-event encoding round-trips the exact target value'

    Write-Host "`n=== Whitespace refusal ==="
    $env:CARGO_TARGET_DIR = '   '
    $blank = Invoke-ChildPowerShell -ScriptPath $realGateFixture
    Assert-Equal -Expected 1 -Actual $blank.exitCode -Message 'whitespace-only target dir is refused'
    Assert-True -Condition ($blank.text -match '\[gate\] REFUSED:.*CARGO_TARGET_DIR') -Message 'whitespace refusal is named and legible'
    Assert-True -Condition ($blank.text -notmatch '\[gate\] contamination canary') -Message 'whitespace refusal happens before the first real stage banner'

    Write-Host "`n=== Empty refusal ==="
    $env:CARGO_TARGET_DIR = ''
    $empty = Invoke-ChildPowerShell -ScriptPath $realGateFixture
    Assert-Equal -Expected 1 -Actual $empty.exitCode -Message 'empty target dir is refused'
    Assert-True -Condition ($empty.text -match '\[gate\] REFUSED:.*CARGO_TARGET_DIR') -Message 'empty refusal is named and legible'
    Assert-True -Condition ($empty.text -notmatch '\[gate\] contamination canary') -Message 'empty refusal happens before the first real stage banner'

    Write-Host "`n=== Real gate unset refusal ==="
    Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    $unset = Invoke-ChildPowerShell -ScriptPath $realGateFixture
    Assert-Equal -Expected 1 -Actual $unset.exitCode -Message 'real gate exits nonzero when target dir is unset'
    Assert-True -Condition ($unset.text -match '\[gate\] REFUSED:.*CARGO_TARGET_DIR') -Message 'real gate names CARGO_TARGET_DIR refusal'
    Assert-True -Condition ($unset.text -notlike '*contamination canary*') -Message 'real gate refuses before canary setup'

    Write-Host "`n=== Source contract ==="
    Assert-True -Condition ($precondition -like '*IsNullOrWhiteSpace*') -Message 'precondition treats unset, empty, and whitespace as one invalid class'
# #191 REVIEW: THE LINE ABOVE DOES NOT DETECT THE GUARD'S DELETION, AND THE HEADER SAID IT DID.
# `IsNullOrWhiteSpace` occurs THREE times in the 96..332 slice -- the guard at :103 and two
# unrelated helpers at :187 and :227 -- so the wildcard is satisfied by either of the others.
# A reader deleted ci/gate.ps1:102-108 entire, REFUSED string and all, and all five source-contract
# assertions stayed green. The sentence in this file's header claimed coverage the cell did not have,
# in a paragraph whose whole subject is that guard's blind spot.
#
# ANCHORED ON THE GUARD'S OWN TEXT INSTEAD. The refusal message belongs to this guard and to nothing
# else, so its absence is the guard's absence. The line above is KEPT rather than replaced: it asserts
# a different property -- that unset, empty and whitespace are one invalid class -- and narrowing it
# would retire that claim to fix a coverage gap it never made.
#
# A DECOY FOR THE ANCHOR, because a second copy anywhere in the file would fake this green.
Assert-True -Condition ((([regex]::Matches($gateLines -join "`n", [regex]::Escape('REFUSED: CARGO_TARGET_DIR is unset or blank'))).Count) -eq 1) -Message 'the refusal message is unique in ci/gate.ps1, so the deletion check below cannot be satisfied by a copy'
Assert-True -Condition ($precondition -like '*REFUSED: CARGO_TARGET_DIR is unset or blank*') -Message 'deleting the target-dir guard IS detected: its own refusal message is asserted, not a predicate three unrelated helpers also use'
    Assert-True -Condition ($precondition -notlike '*targetDirPattern*') -Message 'precondition does not impose a path pattern'
    Assert-True -Condition ($precondition -notlike '*graphhelm-target-m10*') -Message 'precondition does not special-case retired paths'
    $gateText = $gateLines -join "`n"
    Assert-True -Condition ($gateText -match 'cargoTargetDir\s*=\s*\$actualTargetDir') -Message 'run manifest records the exact accepted target value'
    Assert-True -Condition (
        $gateText -like '*targetDirBase64=$(ConvertTo-SlotEventField -Value $actualTargetDir)*' -and
        $gateText -notlike '*targetDir=$($env:CARGO_TARGET_DIR)*'
    ) -Message 'RUN-START encodes the untrusted target instead of writing it raw'
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
