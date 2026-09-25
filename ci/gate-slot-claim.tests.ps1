# ci/gate-slot-claim.tests.ps1 -- #700: the gate CLAIMS the machine-wide slot before it runs.
#
# Text pins over ci/gate.ps1, because the gate cannot be run from a cell. What they pin is the
# WIRING: that the claim precedes the first ledger line, that the lock path has one spelling, that
# a run leaving through the outer finally records RUN-ABORT and releases its own claim, and that a
# slot not obtained is an exit, not a shrug. The BEHAVIOUR of the wait is measured in
# ci/slot-lock.tests.ps1 against Enter-GateSlot with a real holder process.
$ExpectedAssertionCount = 6
$ErrorActionPreference = 'Stop'
$script:total = 0; $script:failures = 0
function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green } else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)
$tokens = $null; $errors = $null
[System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$tokens, [ref]$errors) | Out-Null
Assert-True -Condition (@($errors).Count -eq 0) -Message 'ARRANGEMENT: gate.ps1 parses, so the pins below read a script and not a fragment'

$ord = [System.StringComparison]::Ordinal
# ANCHOR ON THE CALL, NEVER ON THE NAME. `Enter-GateSlot` occurs first inside a COMMENT near the
# top of gate.ps1, so anchoring on the bare name measured a window of prose: this pin failed for a
# reason unrelated to the wiring, and the RUN-START ordering pin below PASSED for one -- a comment
# is above every line in the file. Found by this suite reddening on a correct implementation.
# The assignment is unambiguous: prose does not assign.
$claimAt = $gateText.IndexOf('$script:slotOutcome = Enter-GateSlot', $ord)
$runStartAt = $gateText.IndexOf("Write-SlotEvent -Event 'RUN-START'", $ord)
Assert-True -Condition ($gateText.IndexOf('Enter-GateSlot', $ord) -lt $claimAt) `
    -Message 'CONTROL: the bare name occurs before the call site (in prose), which is why these pins anchor on the assignment'
Assert-True -Condition ($claimAt -ge 0 -and $runStartAt -ge 0 -and $claimAt -lt $runStartAt) `
    -Message '#700 the slot is claimed BEFORE the RUN-START line: a run that never got the slot never starts the ledger pair'
Assert-True -Condition ($gateText.IndexOf('Get-SlotLockPath -SlotDir (Get-SlotDir)', $ord) -ge 0) `
    -Message '#700 the lock path is Get-SlotLockPath over Get-SlotDir: one spelling, and GRAPHHELM_SLOT_LOCK_PATH defaulted for the reader'

# The outer finally: the block that ends with Pop-Location. Both the abort line and the release live there.
$finallyAt = $gateText.IndexOf("} finally {`n    Pop-Location", $ord)
if ($finallyAt -lt 0) { $finallyAt = $gateText.IndexOf("} finally {`r`n    Pop-Location", $ord) }
# #755: BOUNDED BY THE BLOCK, AND BLIND TO COMMENTS. A fixed 1600-character window overshot the
# finally's real end by 383 characters and reached the next block; and a plain text search matched
# the function's name where it appears in a COMMENT inside the block. Measured: deleting the
# block's only real call left this cell green on both counts. Cut to the closing brace, then drop
# comment and blank lines before searching -- a mention is not a call (the hazard K wrote up on #833).
$finallyEndAt = if ($finallyAt -ge 0) { $gateText.IndexOf("`n}`n", $finallyAt) } else { -1 }
if ($finallyEndAt -lt 0) { $finallyEndAt = if ($finallyAt -ge 0) { $gateText.IndexOf("`r`n}`r`n", $finallyAt) } else { -1 } }
$finallyBlock = if ($finallyAt -ge 0 -and $finallyEndAt -gt $finallyAt) {
    (($gateText.Substring($finallyAt, $finallyEndAt - $finallyAt) -split "`r?`n") |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and $_.TrimStart() -notmatch '^#' }) -join "`n"
} else { '' }
# #755: THE PROPERTY IS "THIS BLOCK REACHES RUN-ABORT", not "this block contains the literal".
# The event is written through `Write-RunAbort` now -- one writer for both exits, so the stage
# block and the manifest catch cannot drift apart -- and the literal moved with it. Accepting
# either spelling keeps this cell about the finally; that `Write-RunAbort` actually emits
# RUN-ABORT is pinned separately, in ci/gate-run-abort.tests.ps1, against the real function.
$reachesAbort = ($finallyBlock.IndexOf("'RUN-ABORT'", $ord) -ge 0) -or ($finallyBlock.IndexOf('Write-RunAbort', $ord) -ge 0)
Assert-True -Condition ($reachesAbort -and $finallyBlock.IndexOf('Remove-SlotClaim', $ord) -ge 0) `
    -Message '#700 the outer finally writes RUN-ABORT when RUN-END was not reached, and releases the run''s own claim'

$afterClaim = if ($claimAt -ge 0) { $gateText.Substring($claimAt, [Math]::Min(2400, $gateText.Length - $claimAt)) } else { '' }
Assert-True -Condition ($afterClaim.IndexOf("'claimed'", $ord) -ge 0 -and $afterClaim.IndexOf("'inherited'", $ord) -ge 0 -and $afterClaim -cmatch "(?s)'inherited'.{0,900}?exit 1") `
    -Message '#700 claimed and inherited both proceed; a slot not obtained (expired or path-fault) exits 1 before any stage: no slot, no run'

if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta; exit 2
}
$passed = $script:total - $script:failures
Write-Host "$passed/$script:total passed" -ForegroundColor $(if ($script:failures -eq 0) { 'Green' } else { 'Red' })
if ($script:failures -gt 0) { exit 1 }
exit 0
