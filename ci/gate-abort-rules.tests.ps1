# #191: the two decisions in ci/gate.ps1 that nothing could redden.
#
# #191 states its own test: "a change that made the door refuse unconditionally would pass every
# test in this repository -- because no test runs the script. The same is true of the manifest rule
# and of the canary's abort branch." I ran it against `origin/main` at 21e370b0, each mutation
# alone, the file restored by byte copy and sha-compared, the verdict taken from the whole pinned
# suite list rather than from the suite I expected to catch it:
#
#   the CARGO_TARGET_DIR door refuses unconditionally   ci/gate-target-dir.tests.ps1  14/29  CAUGHT
#   the manifest rule deleted                           30 discovered, 30 passed      NOT CAUGHT
#   the canary's abort made unreachable                 30 discovered, 30 passed      NOT CAUGHT
#
# The door is covered. These two are the remainder, and neither is a small branch: one turns a green
# run red when its manifest never reached the server, the other stops the whole gate before any
# stage can produce evidence from a build environment the run cannot trust.
#
# THE BLOCKS ARE CUT OUT OF gate.ps1 AND DRIVEN, never re-typed here. Running the file would run the
# gate, and a copy of the rule in this suite would be a second oracle that agrees until the day it
# does not -- the shape ci/gate-verdict.tests.ps1 had to fix twice.
#
# THE SLICING IS STRUCTURAL, NOT BY OFFSET. A fixed character window over a subject that grows is
# how ci/gate-slot-claim.tests.ps1 came to pass while the block it measured had been emptied: the
# window overshot into the next statement. These find the opening line and then the closing brace at
# the SAME indent, so an edit inside the block moves nothing.
#
# DECLARED ASSERTION COUNT, derived by counting the calls:
#   2  the manifest block is locatable and is not empty
#   1  an unpublished manifest reaches $failed
#   1  by the name the verdict prints
#   1  CONTROL: a published one does not
#   2  the canary block is locatable and is not empty
#   1  a failed canary exits non-zero
#   1  CONTROL: a passing canary does not exit at all
#   1  and the abort says which of its two voices it used
$ExpectedAssertionCount = 10

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateLines = [System.IO.File]::ReadAllLines($gatePath)

# Find a block by its opening line and the closing brace at the same indent.
function Get-Block {
    param([Parameter(Mandatory)] [string] $Opening)
    for ($i = 0; $i -lt $gateLines.Count; $i++) {
        if ($gateLines[$i].Trim() -ne $Opening) { continue }
        $indent = $gateLines[$i].Length - $gateLines[$i].TrimStart().Length
        $close = (' ' * $indent) + '}'
        for ($j = $i + 1; $j -lt $gateLines.Count; $j++) {
            if ($gateLines[$j].TrimEnd() -eq $close) {
                return ($gateLines[$i..$j] -join "`n")
            }
        }
        return $null
    }
    return $null
}

Write-Host ''
Write-Host '-- an unpublished manifest turns a green run red (#152, uncovered until now) --' -ForegroundColor Cyan

$manifestBlock = Get-Block -Opening 'if ($script:manifestNotPublished) {'
Assert-True -Condition ($null -ne $manifestBlock) `
    'the manifest rule is locatable in gate.ps1, or every assertion below is about an empty string'
# NOT JUST LOCATABLE. A block that had been emptied would still be found and would still "pass" a
# cell that only checked it exists -- the exact way a sibling suite stayed green over a deleted call.
Assert-True -Condition ($manifestBlock -and $manifestBlock.Contains('$failed')) `
    'and it still adds to $failed -- a located but emptied block is the failure this cell exists for'

if ($manifestBlock) {
    $script:manifestNotPublished = $true
    $failed = @()
    # DOT-SOURCED, not called. `&` runs the block in a child scope, so its `$failed +=` creates a
    # local and the assertion below reads an untouched array -- green for the wrong reason if the
    # assertion had been `is-empty`. Measured: with `&` the count was 0 while the block's own
    # Write-Host proved it had run.
    . ([scriptblock]::Create($manifestBlock)) | Out-Null
    Assert-True -Condition ($failed.Count -eq 1) `
        'a run whose manifest never reached the server reaches $failed, so the verdict is RED'
    # The NAME matters as much as the count: `Get-GateVerdictLines` prints the list, and a stage
    # named something else sends a reader somewhere else.
    Assert-True -Condition ($failed -contains 'run manifest not published') `
        "and by the name the verdict prints (got: $($failed -join ', '))"

    # CONTROL. Without it a block that appended unconditionally would satisfy both assertions above,
    # and unconditional is what a careless edit produces.
    $script:manifestNotPublished = $false
    $failed = @()
    . ([scriptblock]::Create($manifestBlock)) | Out-Null
    Assert-True -Condition ($failed.Count -eq 0) `
        'CONTROL: a run whose manifest WAS published adds nothing -- the rule discriminates'
}

Write-Host ''
Write-Host '-- a failed canary aborts the gate before any stage can be read as evidence (#152) --' -ForegroundColor Cyan

$canaryBlock = Get-Block -Opening 'if (-not $canaryPassed) {'
Assert-True -Condition ($null -ne $canaryBlock) `
    'the canary abort is locatable in gate.ps1'
Assert-True -Condition ($canaryBlock -and $canaryBlock.Contains('exit 1')) `
    'and it still exits -- a block that only PRINTS lets 56 stages run on a tree it does not trust'

if ($canaryBlock) {
    # A CHILD PROCESS, because the block's whole point is `exit`, and an `exit` in this process ends
    # the harness instead of being observed. The exit code IS the assertion; everything the block
    # calls is stubbed so nothing reaches a repository.
    $harness = @'
$ErrorActionPreference = 'Stop'
function Write-Host { param([Parameter(ValueFromRemainingArguments)] $Rest) }
function Write-RunManifest { param([Parameter(ValueFromRemainingArguments)] $Rest) 'stub-manifest' }
function Read-SlotLockSnapshot { $null }
$emptyArtifacts = @()
$slotLockAtStart = $null
$canaryOutcome = [pscustomobject]@{ passed = $CANARY; status = 'RED'; cargoLockObserved = $LOCKED }
$canaryPassed = $canaryOutcome.passed
BLOCK
exit 0
'@
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("gar-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    try {
        $failScript = Join-Path $tempDir 'fail.ps1'
        [System.IO.File]::WriteAllText($failScript,
            $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$false').Replace('$LOCKED', '$false'))
        & powershell -NoProfile -ExecutionPolicy Bypass -File $failScript 2>&1 | Out-Null
        Assert-True -Condition ($LASTEXITCODE -eq 1) `
            "a failed canary exits 1, so nothing past it is reported as evidence (got $LASTEXITCODE)"

        # CONTROL: the same block, the same harness, a canary that passed. If this also exited, the
        # cell above would be measuring the harness rather than the rule.
        $passScript = Join-Path $tempDir 'pass.ps1'
        [System.IO.File]::WriteAllText($passScript,
            $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$true').Replace('$LOCKED', '$false'))
        & powershell -NoProfile -ExecutionPolicy Bypass -File $passScript 2>&1 | Out-Null
        Assert-True -Condition ($LASTEXITCODE -eq 0) `
            "CONTROL: a canary that passed falls through and the gate goes on (got $LASTEXITCODE)"

        # THE TWO VOICES. #751 split this: a canary that never RAN because another run holds the
        # build lock is a QUEUE, not a finding about this head, and it is spoken in magenta with
        # different words. Both still exit 1 -- the distinction is what the operator reads, and a
        # merge of the two branches would send somebody to debug a contamination that was never
        # observed.
        $queuedScript = Join-Path $tempDir 'queued.ps1'
        $queuedText = $harness.Replace('BLOCK', $canaryBlock).Replace('$CANARY', '$false').Replace('$LOCKED', '$true')
        # This one keeps Write-Host, so the words can be read.
        $queuedText = $queuedText.Replace("function Write-Host { param([Parameter(ValueFromRemainingArguments)] `$Rest) }", '')
        [System.IO.File]::WriteAllText($queuedScript, $queuedText)
        $spoken = (& powershell -NoProfile -ExecutionPolicy Bypass -File $queuedScript 2>&1 | Out-String)
        Assert-True -Condition ($spoken -match 'did not run' -and $spoken -notmatch 'ABORTING') `
            'a canary blocked by another run says QUEUE, not contamination -- the two voices stay apart (#751)'
    } finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
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
