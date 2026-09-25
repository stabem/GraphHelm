# #643: does a FAILING PowerShell suite actually redden the GATE?
#
# The runner's own table proves it exits 1 when a suite fails, and the wire proves the gate can
# reach the runner. Neither answers K's question, which is one floor up: the stage invocation ends
# in `| Out-Null`, and a pipe is one keystroke away from discarding the verdict it was meant to
# carry. A stage that runs, fails, and is not counted is the same defect this PR exists to close --
# a check that does not gate -- wearing the gate's own colours.
#
# The gate itself is far too expensive to run per cell, so this suite executes a VERBATIM SLICE of
# ci/gate.ps1: its failure accounting, its real Invoke-Stage, the stage block as written, and its
# final verdict block. Every one of those is cut out of the file by anchor text and never retyped,
# because retyping the invocation is exactly what broke it once already.
#
# The suites themselves are STUBS here. The subject is the exit code's journey from a child process
# to the gate's verdict, so a stub that exits with a chosen code is the fixture that isolates it --
# and no file in the real ci/ directory is touched.

$ExpectedAssertionCount = 7
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

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

function Get-GateSlice {
    param(
        [Parameter(Mandatory)] [string] $Start,
        [Parameter(Mandatory)] [string] $End,
        [switch] $IncludeEnd
    )
    $i = $gateText.IndexOf($Start)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length) } else { -1 }
    if ($i -lt 0 -or $j -le $i) {
        # Anchors that stop matching must NOT read as a passing test: the slice would simply be
        # empty and every assertion below would be about nothing.
        throw "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]"
    }
    $end = if ($IncludeEnd) { $j + $End.Length } else { $j }
    return $gateText.Substring($i, $end - $i)
}

$sliceHelpers = Get-GateSlice -Start '$failed = @()' -End '# #152: rewrites the canary'
# The end anchor is the block terminator, indentation included. Anchoring on a bare '| Out-Null'
# matched the FIRST one instead, so any pipe added INSIDE the stage body cut the slice mid-block and
# the fixture became unparseable -- the harness breaking while wearing an ordinary red.
# #1053 item 7 moved `-AlwaysRun` onto this call, so the START anchor moved with the line. The
# assertions below are unchanged: this suite still slices the same stage and still asks whether it
# reddens. An anchor tracks the text it names; it does not get to be stale and still be trusted --
# and it proved that here by REFUSING (HARNESS-BROKE) rather than passing over an empty slice.
$sliceStage = Get-GateSlice -Start "    Invoke-Stage 'ci powershell suites' -AlwaysRun {" -End "`n    } | Out-Null" -IncludeEnd
$sliceVerdict = Get-GateSlice -Start 'if ($failed.Count -gt 0) {' -End "Write-Host '[gate] GREEN"

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-stage-reddens-$([guid]::NewGuid().ToString('N'))"
$fixtureCi = Join-Path $fixtureRoot 'ci'
[System.IO.Directory]::CreateDirectory($fixtureCi) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Set-FixtureSuites {
    param([Parameter(Mandatory)] [hashtable] $ExitCodes)

    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'run-ps-suites.ps1') -Destination (Join-Path $fixtureCi 'run-ps-suites.ps1') -Force
    Get-ChildItem -LiteralPath $fixtureCi -Filter '*.tests.ps1' -File | Remove-Item -Force
    foreach ($name in $ExitCodes.Keys) {
        $code = $ExitCodes[$name]
        $stub = "Write-Host 'stub $name speaking'" + [Environment]::NewLine + "exit $code" + [Environment]::NewLine
        [System.IO.File]::WriteAllText((Join-Path $fixtureCi $name), $stub, $utf8NoBom)
    }
}

function Invoke-GateSlice {
    $slicePath = Join-Path $fixtureRoot 'gate-slice.ps1'
    # The helpers slice contains Invoke-Stage, and Invoke-Stage dot-sources gate-evidence.ps1
    # for Select-GateEvidenceLines (#810). The slice is written to $fixtureRoot, so that is
    # where its $PSScriptRoot points and where the dependency has to be -- the same reason
    # run-ps-suites.ps1 is copied into the fixture. Without this the slice fails to load and
    # the suite reddens for a missing file rather than for anything it is testing.
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'gate-evidence.ps1') -Destination (Join-Path $fixtureRoot 'gate-evidence.ps1') -Force
    $preamble = @(
        "`$ErrorActionPreference = 'Stop'"
        "`$repositoryRoot = '$($fixtureRoot.Replace("'", "''"))'"
    ) -join [Environment]::NewLine
    $script = @($preamble, $sliceHelpers, $sliceStage, $sliceVerdict, "Write-Host '[gate] GREEN - every stage passed.'", 'exit 0') -join [Environment]::NewLine
    [System.IO.File]::WriteAllText($slicePath, $script, $utf8NoBom)

    $output = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $slicePath 2>&1 | ForEach-Object { [string]$_ })
    return [ordered]@{ exitCode = $LASTEXITCODE; text = $output -join "`n" }
}

# The pinned inventory lives in the runner, so the stubs must carry exactly those names or the
# runner refuses before any of them runs -- which would redden the gate for the wrong reason.
$pinned = @([regex]::Matches(
        [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'run-ps-suites.ps1')),
        "'([A-Za-z0-9.-]+\.tests\.ps1)'") | ForEach-Object { $_.Groups[1].Value })

try {
    Assert-True -Condition ($pinned.Count -gt 0) `
        -Message "the runner's pinned inventory was read for the stub names (found $($pinned.Count))"

    # CONTROL FIRST: with every stub green the slice must go GREEN. Without this, a slice that is
    # red for some unrelated reason would make all three red cells pass while proving nothing.
    $allGreen = @{}
    foreach ($n in $pinned) { $allGreen[$n] = 0 }
    Set-FixtureSuites -ExitCodes $allGreen
    $green = Invoke-GateSlice
    Assert-True -Condition ($green.exitCode -eq 0) `
        -Message "control: every suite green leaves the gate slice at exit 0 (got $($green.exitCode))"
    Assert-True -Condition ($green.text -match 'GREEN - every stage passed') `
        -Message 'control: the gate slice reports GREEN when nothing failed'

    # A suite fails. The stage is invoked through `| Out-Null`; the verdict must survive the pipe.
    $oneRed = @{}
    foreach ($n in $pinned) { $oneRed[$n] = 0 }
    $oneRed[$pinned[0]] = 1
    Set-FixtureSuites -ExitCodes $oneRed
    $red = Invoke-GateSlice
    Assert-True -Condition ($red.exitCode -eq 1) `
        -Message "a failing suite makes the gate slice exit 1, not 0 (got $($red.exitCode))"
    Assert-True -Condition ($red.text -match 'RED - failed stages: .*ci powershell suites') `
        -Message 'the gate slice NAMES the stage in its RED line'
    Assert-True -Condition ($red.text -match [regex]::Escape($pinned[0])) `
        -Message "the failing suite's own name reaches the gate output, not only its exit code"

    # HARNESS-BROKE at the suite. The runner keeps 2 as 2; the gate has no third state and collapses
    # it to red. That is the correct direction -- it is asserted here so the collapse is a RECORDED
    # decision rather than something a reader has to discover.
    $broke = @{}
    foreach ($n in $pinned) { $broke[$n] = 0 }
    $broke[$pinned[0]] = 2
    Set-FixtureSuites -ExitCodes $broke
    $harness = Invoke-GateSlice
    Assert-True -Condition ($harness.exitCode -eq 1 -and $harness.text -match 'HARNESS-BROKE') `
        -Message "a suite exiting 2 still reddens the gate, and the harness-broke wording survives into the gate output (exit $($harness.exitCode))"
} finally {
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
