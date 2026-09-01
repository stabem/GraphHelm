# #639: isolated tests for ci/classify-run.ps1 -- the green side of the taxonomy.
#
# WHAT THIS FILE EXISTS TO PROVE. A gate manifest recorded `runClass: green` for a run whose own
# author measured it as luck (`171e4fdadb93`, PASS=4 FAIL=1, written into the NEXT commit's message
# because there was nowhere else to put it). Three sites made that unrecordable: `gate.ps1` assigns
# `green` automatically, this script REFUSES anything already classified, and its vocabulary was
# red-side only. So no person could mark that green, and a census reading colour read it as proof.
#
# The distinction that fixes it without breaking anything: the refusal at the re-classification
# guard protects a HUMAN JUDGEMENT from being overwritten. An automatic `green` is not a judgement --
# it is the absence of failures. So the guard refuses on the ORIGIN of the class, not on its
# existence, and an automatic green may be refined ONCE by a person.
#
# Homegrown PASS/FAIL/HARNESS-BROKE harness, same discipline as ci/slot-lock.tests.ps1: this
# repository carries no Pester dependency and this issue is not the place to add one. The declared
# total is what separates "everything passed" from "half the file vanished in a merge".
#
# 22 = 22 Assert-True calls. Assert-Equal is not used here; every case asserts a boolean outcome or
# a string equality expressed through Assert-True, so the naive grep and the runtime count agree.
$ExpectedAssertionCount = 22

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

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$classify = Join-Path $scriptDir 'classify-run.ps1'

# A throwaway store. A probe written into the real slot directory is litter that a later census
# cannot tell from a real run, so this file never touches D:/graphhelm-slot.
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("classify-tests-" + [Guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($sandbox) | Out-Null
$previousSlotDir = $env:GRAPHHELM_SLOT_DIR
$env:GRAPHHELM_SLOT_DIR = $sandbox

function New-Manifest {
    param([Parameter(Mandatory)] [hashtable] $Properties)
    $path = Join-Path $sandbox ((New-Guid).ToString('N').Substring(0, 12) + "-20260828T000000Z.json")
    # Overrides, not a hashtable sum: `@{} + @{}` throws on a duplicate key, and a fixture that
    # cannot restate a default cannot express the case where the default is the DEFECT.
    $body = @{ headSha = 'a' * 40; status = 'GREEN'; overallPassed = $true }
    foreach ($key in $Properties.Keys) { $body[$key] = $Properties[$key] }
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($path, ($body | ConvertTo-Json -Depth 8), $utf8NoBom)
    return $path
}

# Never lets a failure escape as a terminating error: a red must land on an assertion below, not
# abort the file and leave the remaining cases unrun and unreported.
function Invoke-Classify {
    param([Parameter(Mandatory)] [string] $Path, [Parameter(Mandatory)] [string] $Class, [string] $Because = 'test')
    try {
        & $classify -Manifest $Path -Class $Class -Because $Because *> $null
        return @{ Ok = $true; Error = '' }
    } catch {
        return @{ Ok = $false; Error = $_.Exception.Message }
    }
}

function Read-Manifest {
    param([Parameter(Mandatory)] [string] $Path)
    return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json)
}

try {
    Write-Host "`n-- an automatic green can be refined, once, by a person --"

    $green = New-Manifest -Properties @{ runClass = 'green' }
    $refine = Invoke-Classify -Path $green -Class 'green-by-luck' -Because 'ran alone five times: PASS=4 FAIL=1'
    Assert-True $refine.Ok "an automatic green accepts the class green-by-luck (was: $($refine.Error))"

    $after = Read-Manifest -Path $green
    Assert-True ($after.runClass -eq 'green-by-luck') "the refined class is recorded"
    Assert-True ($after.runClassRefinedFrom -eq 'green') "the record still says it WAS green, so a census can see the history"
    Assert-True ($after.runClassOrigin -eq 'human') "the refined class is marked as a human judgement"
    Assert-True ([string]::IsNullOrEmpty($after.runClassBecause) -eq $false) "the refinement carries its reason"

    $flaky = New-Manifest -Properties @{ runClass = 'green' }
    $second = Invoke-Classify -Path $flaky -Class 'flaky-observed' -Because 'observed failing then passing'
    Assert-True $second.Ok "flaky-observed is also available on the green side (was: $($second.Error))"

    Write-Host "`n-- a human judgement stays irreversible --"

    $again = Invoke-Classify -Path $green -Class 'flaky-observed' -Because 'second opinion'
    # One assertion, not two: with the human check disarmed the green-side rule refuses this call
    # anyway (the refined class is no longer `green`, so there is "nothing to refine"), and a bare
    # -not Ok passes for that wrong reason. Second time this file has been masked by a neighbouring
    # guard -- the cure is the same, assert the MESSAGE.
    Assert-True ((-not $again.Ok) -and ($again.Error -match 'already classified')) "a green already refined by a person refuses a second refinement, naming the prior judgement"

    $red = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED' }
    $judged = Invoke-Classify -Path $red -Class 'real-red' -Because 'the code failed'
    Assert-True $judged.Ok "UNCLASSIFIED still accepts a first human judgement (was: $($judged.Error))"

    $overwrite = Invoke-Classify -Path $red -Class 'dead' -Because 'changed my mind'
    # The MESSAGE, not merely the refusal: with the human-origin check disarmed, the green-side
    # check below it still refuses this call -- so asserting only "it threw" passes for the wrong
    # reason and the guard under test could be deleted without reddening. Found by sabotage.
    Assert-True ((-not $overwrite.Ok) -and ($overwrite.Error -match 'by a person')) "a human real-red cannot be overwritten, and the refusal names the human judgement"

    # A manifest written BEFORE runClassOrigin existed carries a human class and no origin field.
    # The derivation must reach 'human' from the value alone, or every legacy judgement in the
    # ledger becomes overwritable the moment this change lands -- the widest way this could go wrong.
    $legacy = New-Manifest -Properties @{ runClass = 'dead' }
    $legacyOverwrite = Invoke-Classify -Path $legacy -Class 'real-red' -Because 'no origin field on this one'
    Assert-True ((-not $legacyOverwrite.Ok) -and ($legacyOverwrite.Error -match 'by a person')) "a legacy human class with no origin field is still protected, by the same guard"

    Write-Host "`n-- a green is not a failure, and cannot be relabelled as one --"

    $mislabel = New-Manifest -Properties @{ runClass = 'green' }
    $asRed = Invoke-Classify -Path $mislabel -Class 'real-red' -Because 'wrong direction'
    Assert-True (-not $asRed.Ok) "an automatic green refuses a red-side class: the run did not fail"
    Assert-True ($asRed.Error -match 'refined but not contradicted') "and the refusal names the reason, not merely that a class exists"
    Assert-True ((Read-Manifest -Path $mislabel).runClass -eq 'green') "and the refused call left the class untouched"

    # #640 review (K): the MIRROR of the rule above, and it was missing. `UNCLASSIFIED` is what the
    # gate writes when the run did NOT pass everything, so gating the green-side rule on
    # "not UNCLASSIFIED" let a FAILED run be recorded as flaky-observed -- as a human judgement
    # nothing afterwards can overwrite. A green-side class refines an observed pass; where the gate
    # never observed one, there is nothing to refine.
    $notPassing = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED' }
    $greenOnFail = Invoke-Classify -Path $notPassing -Class 'flaky-observed' -Because 'this run did not pass everything'
    Assert-True ((-not $greenOnFail.Ok) -and ($greenOnFail.Error -match 'nothing to refine')) "a run that did not pass everything refuses a green-side class, and the refusal says why"
    Assert-True ((Read-Manifest -Path $notPassing).runClass -eq 'UNCLASSIFIED') "and the refused call left it unclassified"

    # #640 review: the derivation reads ANY value outside {green, UNCLASSIFIED} as `human`, and a
    # human class is unoverwritable -- so a malformed or untrusted manifest carrying a class no
    # vocabulary contains would LOCK that run out of classification forever. Refuse instead of
    # widening: a value in neither vocabulary falls outside the justification the derivation rests
    # on, and saying so is cheaper than guessing which half wrote it.
    $bogus = New-Manifest -Properties @{ runClass = 'not-a-class' }
    $onBogus = Invoke-Classify -Path $bogus -Class 'real-red' -Because 'the class here belongs to no vocabulary'
    # One assertion: the OLD message also quotes the offending value, so a separate "quotes the
    # value" check passes before the fix exists. Third time a cell in this file would have passed
    # for a neighbouring reason -- that is a pattern in this taxonomy, not three accidents.
    Assert-True ((-not $onBogus.Ok) -and ($onBogus.Error -match 'belongs to neither') -and ($onBogus.Error -match 'not-a-class')) "an unknown class value is refused rather than read as human, and the refusal quotes it"

    # #640 review, and these are ONE CLASS with the two already fixed: a manifest field believed
    # without the fields that corroborate it. `gate.ps1:441` writes green only when
    # $Status -eq 'GREEN' AND $passedEverything, so a `green` whose status disagrees was never
    # written by that rule and cannot be refined as if it had been.
    $lying = New-Manifest -Properties @{ runClass = 'green'; status = 'RED'; overallPassed = $false }
    $onLying = Invoke-Classify -Path $lying -Class 'green-by-luck' -Because 'the class says green and the status does not'
    Assert-True ((-not $onLying.Ok) -and ($onLying.Error -match 'not corroborated')) "a green whose status contradicts it is refused, and the refusal names the contradiction"

    # Absent is a THIRD fact, not false -- this file's own doctrine for instrumentSuspect. A green
    # nobody can corroborate is not a green anyone should refine.
    $utf8NoBomLocal = New-Object System.Text.UTF8Encoding($false)
    $bare = Join-Path $sandbox 'bbbbbbbbbbbb-20260828T000000Z.json'
    [System.IO.File]::WriteAllText($bare, (@{ headSha = 'b' * 40; runClass = 'green' } | ConvertTo-Json -Depth 8), $utf8NoBomLocal)
    $onBare = Invoke-Classify -Path $bare -Class 'flaky-observed' -Because 'no status field at all'
    Assert-True ((-not $onBare.Ok) -and ($onBare.Error -match 'not corroborated')) "a green with no status field is refused too: absent is not agreement"

    # The durable twin. This script already KNOWS the twin exists -- it writes to it -- but decided
    # the refinement from the committable copy alone. A partial write leaves the committable copy
    # automatic while the durable one already carries a human judgement.
    $twinName = 'cccccccccccc-20260828T000000Z.json'
    $committable = Join-Path $sandbox $twinName
    [System.IO.File]::WriteAllText($committable, (@{ headSha = 'c' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green' } | ConvertTo-Json -Depth 8), $utf8NoBomLocal)
    [System.IO.Directory]::CreateDirectory((Join-Path $sandbox 'gate-runs')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $sandbox 'gate-runs') $twinName), (@{ headSha = 'c' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green-by-luck'; runClassOrigin = 'human' } | ConvertTo-Json -Depth 8), $utf8NoBomLocal)
    $onSplit = Invoke-Classify -Path $committable -Class 'flaky-observed' -Because 'the twin already carries a human judgement'
    Assert-True ((-not $onSplit.Ok) -and ($onSplit.Error -match 'durable twin')) "a refinement is refused when the durable twin already holds a human judgement, and the refusal names the twin"

    # Control: an AGREEING twin must not block anything, or the guard above would simply forbid
    # refinement wherever a twin exists -- a guard that refuses everything proves nothing.
    $okName = 'dddddddddddd-20260828T000000Z.json'
    $okCommittable = Join-Path $sandbox $okName
    [System.IO.File]::WriteAllText($okCommittable, (@{ headSha = 'd' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green' } | ConvertTo-Json -Depth 8), $utf8NoBomLocal)
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $sandbox 'gate-runs') $okName), (@{ headSha = 'd' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green' } | ConvertTo-Json -Depth 8), $utf8NoBomLocal)
    $onAgree = Invoke-Classify -Path $okCommittable -Class 'green-by-luck' -Because 'twin agrees'
    Assert-True $onAgree.Ok "an agreeing durable twin does not block the refinement (was: $($onAgree.Error))"

    Write-Host "`n-- the pre-taxonomy refusal still stands --"

    $ancient = New-Manifest -Properties @{}
    $preTaxonomy = Invoke-Classify -Path $ancient -Class 'real-red' -Because 'no runClass field at all'
    Assert-True (-not $preTaxonomy.Ok) "a manifest with no runClass field is still refused (#199 era marker)"
    Assert-True ($preTaxonomy.Error -match 'predates the taxonomy') "and the refusal names the era, not the value"
} finally {
    $env:GRAPHHELM_SLOT_DIR = $previousSlotDir
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount. A case vanished or was added without updating the declared total." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total)/$($script:total)" -ForegroundColor Green
