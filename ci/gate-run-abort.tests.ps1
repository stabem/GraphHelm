# #755: a RUN-START with no RUN-END was two things -- a run still going and a run that died -- and
# the ledger could not tell them apart. `Write-RunAbort` writes the terminal line the dead run
# never wrote, from the stage loop's `finally` and from the manifest write's `catch`.
#
# The helper region of ci/gate.ps1 is cut by anchor text and dot-sourced; `Write-SlotEvent` is then
# replaced by a stub that records what would have reached SLOT.log. Nothing here runs the gate.
$ExpectedAssertionCount = 19

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
$gateText = [System.IO.File]::ReadAllText($gatePath)
$nl = if ($gateText.Contains("`r`n")) { "`r`n" } else { "`n" }

$parseErrors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors)
Assert-True ($parseErrors.Count -eq 0) 'ARRANGEMENT: gate.ps1 parses, so what follows is measured rather than empty'

function Get-Slice {
    param([string] $From, [string] $To)
    $a = $gateText.IndexOf($From, [System.StringComparison]::Ordinal)
    $b = if ($a -ge 0) { $gateText.IndexOf($To, $a + $From.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($a -lt 0 -or $b -le $a) { throw "HARNESS-BROKE: could not slice [$From] .. [$To] out of gate.ps1" }
    return $gateText.Substring($a, $b - $a)
}

# From the field encoder up to (not including) gate.ps1's own dot-source of gate-evidence.ps1,
# which needs a real $PSScriptRoot; everything the writer uses is inside, except Write-SlotEvent,
# which is stubbed below on purpose.
$helpers = Get-Slice -From 'function ConvertTo-SlotEventField {' -To '. (Join-Path $PSScriptRoot ''gate-evidence.ps1'')'
Assert-True ($helpers.IndexOf('function Write-RunAbort {', [System.StringComparison]::Ordinal) -ge 0) 'ARRANGEMENT: the writer lives in the helper region the suites slice, so it is the production text under test'
. ([scriptblock]::Create($helpers))

# The stub: what would have reached SLOT.log, kept in order.
$script:ledger = New-Object System.Collections.Generic.List[string]
function Write-SlotEvent {
    param([Parameter(Mandatory)] [string] $Event, [string] $Detail = '')
    $script:ledger.Add("$Event | $Detail")
}
function Decode-Field { param([string] $Base64) return [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String($Base64)) }

Write-Host ''
Write-Host '-- a run that never reached RUN-END gets its terminal line, with head and reason --' -ForegroundColor Cyan
$script:slotRunEnded = $false
$gatedHeadAtStart = 'abc123def456'
Write-RunAbort -Reason 'the stage block was left before it finished'
Assert-True ($script:ledger.Count -eq 1) "exactly one line written (got $($script:ledger.Count))"
Assert-True ($script:ledger[0].StartsWith('RUN-ABORT | head=abc123def456 reasonBase64=', [System.StringComparison]::Ordinal)) "it is RUN-ABORT and names the head (got '$($script:ledger[0])')"
$encoded = ($script:ledger[0] -split 'reasonBase64=')[1]
Assert-True ((Decode-Field $encoded) -ceq 'the stage block was left before it finished') 'and the reason round-trips through the same base64 field encoding RUN-START uses'
Assert-True $script:slotRunEnded 'and the run is now marked ended, so no second terminal line can follow'

Write-Host ''
Write-Host '-- one terminal line per run, never two --' -ForegroundColor Cyan
Write-RunAbort -Reason 'a second path reached the writer'
Assert-True ($script:ledger.Count -eq 1) "a second call after the first writes nothing (got $($script:ledger.Count))"
$script:ledger.Clear(); $script:slotRunEnded = $true
Write-RunAbort -Reason 'after RUN-END'
Assert-True ($script:ledger.Count -eq 0) 'a run that reached RUN-END gets no RUN-ABORT: the guard is the flag RUN-END sets'

Write-Host ''
Write-Host '-- the reason cannot break the line format --' -ForegroundColor Cyan
$script:ledger.Clear(); $script:slotRunEnded = $false
Write-RunAbort -Reason "two`nlines | with a pipe"
Assert-True ($script:ledger.Count -eq 1 -and $script:ledger[0].IndexOf("`n") -lt 0 -and (($script:ledger[0] -split ' \| ').Count -eq 2)) 'a newline or a pipe in the reason is encoded, not written raw'
$script:ledger.Clear(); $script:slotRunEnded = $false; $gatedHeadAtStart = $null
Write-RunAbort -Reason 'no head known'
Assert-True ($script:ledger[0].IndexOf('head=unknown ', [System.StringComparison]::Ordinal) -ge 0) 'a run whose head could not be read says unknown rather than an empty field'

Write-Host ''
Write-Host '-- THE FINALLY ITSELF: the production text of the stage loop''s finally, run against a throw --' -ForegroundColor Cyan
# Cut from `} finally {` + Pop-Location up to the closing brace, so the assertion is on gate.ps1's
# own finally rather than a copy of it.
# STRUCTURAL, NOT ADJACENT. The first version anchored on `} finally {` + Pop-Location + the `if`
# as ONE literal, so #905 landing a five-line comment between Pop-Location and the guard broke a
# cell that was about neither. Anchor on the block's opening, take it to its closing brace, and
# assert what the REGION contains -- the same correction K made to one of my cells on #882.
$finallyStart = $gateText.IndexOf('} finally {' + $nl + '    Pop-Location', [System.StringComparison]::Ordinal)
Assert-True ($finallyStart -ge 0) 'ARRANGEMENT: the stage block''s finally was found'
$finallyEnd = $gateText.IndexOf($nl + '}' + $nl, $finallyStart)
$finallyText = $gateText.Substring($finallyStart, $finallyEnd + $nl.Length + 1 - $finallyStart)
Assert-True ($finallyText.IndexOf('Write-RunAbort -Reason', [System.StringComparison]::Ordinal) -ge 0) 'and it reaches the ledger through the one writer, not an inline event'
Assert-True ($finallyText.IndexOf('if (-not $script:stagesCompleted)', [System.StringComparison]::Ordinal) -ge 0) 'guarded by the completion flag the try sets last'
function Invoke-StageLoopShape {
    param([bool] $Throw)
    $script:ledger.Clear(); $script:slotRunEnded = $false; $gatedHeadAtStart = 'feedface'
    $body = '$script:stagesCompleted = $false' + $nl + 'Push-Location .' + $nl + 'try {' + $nl +
        $(if ($Throw) { "    throw 'a stage blew up'" + $nl } else { '' }) +
        '    $script:stagesCompleted = $true' + $nl + $finallyText
    try { . ([scriptblock]::Create($body)) } catch { }
    return $script:ledger.Count
}
Assert-True ((Invoke-StageLoopShape -Throw $true) -eq 1) 'a throw inside the stage loop leaves one RUN-ABORT behind'
Assert-True ($script:ledger[0].IndexOf('reasonBase64=', [System.StringComparison]::Ordinal) -ge 0 -and (Decode-Field (($script:ledger[0] -split 'reasonBase64=')[1])) -ceq 'the stage block was left before it finished') 'and it says the loop was left before it finished'
Assert-True ((Invoke-StageLoopShape -Throw $false) -eq 0) 'CONTROL: a stage loop that runs to its end writes no RUN-ABORT'

Write-Host ''
Write-Host '-- the wiring, read from the file: the flag is the LAST statement of the try, RUN-END sets the guard, the manifest catch aborts --' -ForegroundColor Cyan
Assert-True ($gateText.IndexOf('    $script:stagesCompleted = $true' + $nl + '} finally {', [System.StringComparison]::Ordinal) -ge 0) 'the completion flag is set as the last statement before the finally, so nothing after it can be skipped'
$runEndAt = $gateText.IndexOf("Write-SlotEvent -Event 'RUN-END'", [System.StringComparison]::Ordinal)
$runEndedAt = $gateText.IndexOf('$script:slotRunEnded = $true', $runEndAt, [System.StringComparison]::Ordinal)
Assert-True ($runEndAt -ge 0 -and $runEndedAt -gt $runEndAt -and $runEndedAt - $runEndAt -lt 200) 'RUN-END sets the guard immediately after it is written'
Assert-True ($gateText.IndexOf('Write-RunAbort -Reason "the run manifest could not be written', [System.StringComparison]::Ordinal) -ge 0) 'a manifest write that fails is the other way a run ends without RUN-END, and it aborts by name'

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
