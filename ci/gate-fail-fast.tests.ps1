# #1053 item 7: the gate stops paying for stages nobody will read, and records that it did.
#
# Measured over the 370 gate receipts committed before the receipt store was retired (2026-09-24):
# 39 % of runs are RED; a red run medians 1734 s against GREEN's 1664 s; the FIRST failing stage
# ends at a median of 795 s. So a red gate spends a further ~940 s after its answer is known. 56 %
# of red runs have exactly ONE failing stage, so for most of them nothing a reader would have used
# is skipped.
#
# THE DANGER IS NOT THE SKIP, IT IS HOW THE SKIP IS RECORDED. `passed = $null` with `notRun = $true`
# is the whole design: `$false` fabricates a failure the run never observed, `$true` fabricates a
# green, and omitting the entry makes "not run" indistinguishable from "does not exist in this
# build". The cells below fix all three of those, in both directions -- a green run must gain no
# `notRun` key at all, or the flag would be decoration that a future edit could leave always-on
# without reddening anything.
#
# `Invoke-Stage` is cut out of ci/gate.ps1 by anchor and dot-sourced alone, the way
# ci/gate-verdict.tests.ps1 and ci/gate-stage-reddens.tests.ps1 already cut their subjects: running
# gate.ps1 would run the gate.
#
# Same homegrown PASS/FAIL harness as the sibling gate suites; this repository carries no Pester.
# Exit codes: 0 all passed, 1 an assertion failed, 2 the harness could not vouch for the run.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   1  ARRANGEMENT: the slice carries Invoke-Stage, the skip and the arming
#   1  NEGATIVE CONTROL: with every stage passing, all four run
#   1  and NOT ONE record carries a notRun key -- the flag is not always-on decoration
#   1  a failure in position 2 of 4 still records stage 2 as a real failure
#   1  and stages 3 and 4 are PRESENT in the record rather than missing
#   1  with notRun = $true
#   1  and notRunCause naming the stage that failed
#   1  their `passed` is $null -- not $false, which would fabricate a failure
#   1  and not $true, which would fabricate a green
#   1  the skipped bodies DID NOT RUN (the side effect they would have had is absent)
#   1  -AlwaysRun still runs after an abort, so a started background child is still reaped
#   1  CONTROL: the arming really is what stops them -- clearing it lets the next stage run
#   1  an UNARMED slice never skips at all -- a slice is not a run
$ExpectedAssertionCount = 13

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

$gateText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate.ps1'))

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) {
        # An anchor that stopped matching must NOT read as a passing test: the slice would be empty
        # and every assertion below would be about nothing.
        throw "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]"
    }
    return $gateText.Substring($i, $j - $i)
}

$sliceHelpers = Get-GateSlice -Start '$failed = @()' -End '# #152: rewrites the canary'
Assert-True ($sliceHelpers -match 'function Invoke-Stage \{' -and
    $sliceHelpers -match 'SKIPPED: \$Name' -and
    $sliceHelpers -match '\$script:abortAfterStage = \$Name') `
    "ARRANGEMENT: the slice carries Invoke-Stage, its skip and its arming ($($sliceHelpers.Length) chars), so these cells drive the real function"

# Rebuilt for each scenario, so no cell can inherit another's state.
function New-StageBench {
    param([switch] $NoFailFastRequested)
    $bench = [pscustomobject]@{ Ran = (New-Object System.Collections.Generic.List[string]) }
    $script:benchRan = $bench.Ran
    Remove-Variable -Name abortAfterStage -Scope Script -ErrorAction SilentlyContinue
    # ARMED EXPLICITLY, because a SLICE is not a run. gate.ps1 arms this once above `$failed = @()`,
    # which is where this and every sibling suite starts its slice -- so an unarmed slice never
    # skips. That boundary is what keeps the five suites which drive Invoke-Stage across several
    # INDEPENDENT scenarios (gate-stage-reddens, gate-stage-overlap, gate-postgres-evidence,
    # gate-background-stage-evidence, gate-stage-stderr-evidence) from having scenario one's
    # deliberate failure silently skip scenario two.
    $script:failFastEnabled = (-not $NoFailFastRequested)
    $script:stageRecords = New-Object System.Collections.Generic.List[object]
    $script:failed = @()
    return $bench
}

function Get-Record {
    param([Parameter(Mandatory)] [string] $Name)
    return @($script:stageRecords | Where-Object { $_.name -eq $Name })[0]
}

function Test-HasNotRun {
    <#
        BY THE DICTIONARY'S KEYS, NOT BY `PSObject.Properties`. A stage record is an
        `[ordered]` hashtable, and `PSObject.Properties` on one of those lists the HASHTABLE's own
        members (Count, Keys, Values, IsReadOnly) -- never its entries. Written that way this
        helper answered `$false` for every record ever passed to it, which made the green-run
        NEGATIVE CONTROL below pass vacuously: it asserted "no record carries a notRun key" against
        a predicate that could not have found one. Caught because the positive cell using the same
        helper went red while `notRunCause`, read directly off the same record, was fine.
    #>
    param([Parameter(Mandatory)] $Record)
    if ($null -eq $Record) { return $false }
    return (@($Record.Keys) -contains 'notRun')
}

# WRITTEN TO A FILE, NOT `[scriptblock]::Create`. The slice dot-sources `gate-evidence.ps1` through
# `$PSScriptRoot`, and inside a scriptblock that variable is EMPTY -- `Join-Path` then refuses and
# the suite dies on its harness instead of on its subject. Giving the slice a real path gives it a
# real `$PSScriptRoot`, and the dependency is copied next to it for the same reason
# ci/gate-stage-reddens.tests.ps1 copies it into its fixture.
$sliceRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-failfast-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($sliceRoot) | Out-Null
$slicePath = Join-Path $sliceRoot 'slice.ps1'
[System.IO.File]::WriteAllText($slicePath, $sliceHelpers, (New-Object System.Text.UTF8Encoding($false)))
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'gate-evidence.ps1') -Destination (Join-Path $sliceRoot 'gate-evidence.ps1') -Force

# READ OUT OF THE SUBJECT, NEVER RETYPED. `$MuteStageExitCode` is defined above this slice's start
# anchor, so the slice uses it without declaring it. Retyping `99` here would let the two drift the
# day someone changes it in gate.ps1, and the drift would be invisible: the cells would still pass
# while driving a function that mutes with a different code than the gate does.
$muteMatch = [regex]::Match($gateText, '(?m)^\$MuteStageExitCode = (\d+)')
if (-not $muteMatch.Success) { throw 'HARNESS-BROKE: $MuteStageExitCode is no longer a literal in gate.ps1' }
$MuteStageExitCode = [int]$muteMatch.Groups[1].Value

. $slicePath

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- NEGATIVE CONTROL: a green run gains no notRun key at all --' -ForegroundColor Cyan
$bench = New-StageBench
foreach ($n in @('one', 'two', 'three', 'four')) {
    Invoke-Stage $n { $script:benchRan.Add($n); & cmd /c 'exit 0' } | Out-Null
}
Assert-True ($bench.Ran.Count -eq 4) `
    "with every stage passing, all four bodies run (ran $($bench.Ran.Count))"
Assert-True (-not (@($script:stageRecords | Where-Object { Test-HasNotRun -Record $_ }).Count -gt 0)) `
    'and NOT ONE record carries a notRun key -- so the flag is a real state and not decoration a future edit could leave always-on'

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- a failure in position 2 of 4 skips the rest, and says so --' -ForegroundColor Cyan
$bench = New-StageBench
Invoke-Stage 'one' { $script:benchRan.Add('one'); & cmd /c 'exit 0' } | Out-Null
Invoke-Stage 'two' { $script:benchRan.Add('two'); & cmd /c 'exit 3' } | Out-Null
Invoke-Stage 'three' { $script:benchRan.Add('three'); & cmd /c 'exit 0' } | Out-Null
Invoke-Stage 'four' { $script:benchRan.Add('four'); & cmd /c 'exit 0' } | Out-Null

$two = Get-Record -Name 'two'
Assert-True ($null -ne $two -and $two.passed -eq $false -and $two.exitCode -eq 3) `
    'the stage that failed is still recorded as a real failure with its exit code -- fail-fast does not swallow the finding'

$three = Get-Record -Name 'three'
$four = Get-Record -Name 'four'
Assert-True ($null -ne $three -and $null -ne $four) `
    'stages 3 and 4 are PRESENT in the record -- omitting them would make "not run" indistinguishable from "does not exist in this build"'
Assert-True ((Test-HasNotRun -Record $three) -and $three.notRun -eq $true -and (Test-HasNotRun -Record $four) -and $four.notRun -eq $true) `
    'each carries notRun = $true'
Assert-True ($three.notRunCause -eq 'two' -and $four.notRunCause -eq 'two') `
    'and notRunCause naming the stage that failed, so a reader knows WHY without reconstructing the order'

# THE LOAD-BEARING DISTINCTION, asserted in both wrong directions rather than just the right one.
Assert-True ($null -eq $three.passed -and $null -eq $four.passed) `
    'their `passed` is $null, not $false -- $false would fabricate a failure this run never observed'
Assert-True (-not ($three.passed -eq $true) -and -not ($four.passed -eq $true)) `
    'and not $true -- which would fabricate a green for a stage that never ran'

Assert-True (($bench.Ran -contains 'one') -and ($bench.Ran -contains 'two') -and
    (-not ($bench.Ran -contains 'three')) -and (-not ($bench.Ran -contains 'four'))) `
    "the skipped bodies DID NOT RUN -- the side effect they would have had is absent (ran: $($bench.Ran -join ', '))"

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- a started background child is still reaped --' -ForegroundColor Cyan
$bench = New-StageBench
Invoke-Stage 'boom' { & cmd /c 'exit 1' } | Out-Null
Invoke-Stage 'the join' -AlwaysRun { $script:benchRan.Add('the join'); & cmd /c 'exit 0' } | Out-Null
Assert-True ($bench.Ran -contains 'the join') `
    '-AlwaysRun runs after an abort, so the three joins of background children still reap them instead of leaking a process past the run (#1003)'

# CONTROL: without this the cells above could be passing because the bodies never ran for some
# other reason entirely.
Remove-Variable -Name abortAfterStage -Scope Script -ErrorAction SilentlyContinue
Invoke-Stage 'after the flag is cleared' { $script:benchRan.Add('after'); & cmd /c 'exit 0' } | Out-Null
Assert-True ($bench.Ran -contains 'after') `
    'CONTROL: clearing the arming lets the very next stage run again, so it really is the flag that stopped them'

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- an UNARMED slice never skips: a slice is not a run --' -ForegroundColor Cyan
$bench = New-StageBench
Remove-Variable -Name failFastEnabled -Scope Script -ErrorAction SilentlyContinue
Invoke-Stage 'unarmed boom' { $script:benchRan.Add('unarmed boom'); & cmd /c 'exit 1' } | Out-Null
Invoke-Stage 'the next scenario' { $script:benchRan.Add('the next scenario'); & cmd /c 'exit 0' } | Out-Null
Assert-True ($bench.Ran -contains 'the next scenario') `
    'with fail-fast NEVER ARMED, a failing stage does not skip the next one -- this is the regression that reddened five sibling suites on their first gate, and the boundary that fixes it'

Remove-Item -LiteralPath $sliceRoot -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
