# #639: isolated tests for ci/classify-run.ps1 -- the green side of the taxonomy.
#
# WHAT THIS FILE EXISTS TO PROVE. A gate manifest recorded `runClass: green` for a run its own
# author had measured as luck -- run alone five times, PASS=4 FAIL=1 -- and wrote that into a commit
# message because there was nowhere else to put it. Three sites made that unrecordable: `gate.ps1`
# assigns `green` automatically, this script REFUSES anything already classified, and its
# vocabulary was red-side only. So no person could mark that green, and a census reading colour read it as proof.
#
# TWO CORRECTIONS TO THE PARAGRAPH ABOVE, both to statements I made and both measured since (#659).
# The earlier wording said the measurement was written into the NEXT commit's message. It was not:
# it is in `171e4fdadb93`'s OWN message, lines 5-6 of its body. And `171e4fdadb93` is the commit that
# CARRIES the measurement, not the head of the run it refers to -- that commit says the lucky green
# was "already recorded" when it was written at 13:54:15Z, while the run at its own head did not
# start until 13:57:22Z and ran WITH the fix applied. The green it points at is the last one before
# the failure, `47569a13bc96` at 07:01:49Z. Recorded here because the first version of these
# sentences was cited by a review as the standard against which a correct record was judged wrong:
# a comment that misstates its own evidence does not merely fail to help, it actively misleads.
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
# 45 = 45 runtime assertions: 43 direct Assert-True calls plus the one inside the two-item
# consumer loop, which the naive grep counts once and which fires twice. Assert-Equal is not used here; every case asserts a boolean outcome or
# a string equality expressed through Assert-True, so the naive grep and the runtime count agree.
$ExpectedAssertionCount = 69

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
    # Use the framework type directly. The full gate runs this suite in a fresh background
    # PowerShell process; one loaded run could not auto-load the module that exports New-Guid and
    # stopped before any assertion. Guid.NewGuid has no command-discovery or module dependency.
    $path = Join-Path $sandbox ([Guid]::NewGuid().ToString('N').Substring(0, 12) + "-20260828T000000Z.json")
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

    $red = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false }
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
    $notPassing = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false }
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

    # #644: PRESENCE and VOCABULARY are not AGREEMENT. The enumeration on #640 found the right
    # five sites and checked that the corroborating field EXISTS and is a legal value; it never
    # checked that the corroborator SAYS THE SAME THING. A stated origin was taken raw.
    $lyingOrigin = New-Manifest -Properties @{ runClass = 'real-red'; runClassOrigin = 'automatic'; status = 'RED'; overallPassed = $false }
    $onLyingOrigin = Invoke-Classify -Path $lyingOrigin -Class 'dead' -Because 'the stated origin contradicts the class'
    Assert-True ((-not $onLyingOrigin.Ok) -and ($onLyingOrigin.Error -match 'disagree')) "a stated origin that contradicts its own class is refused, naming the disagreement"

    $bogusOrigin = New-Manifest -Properties @{ runClass = 'green'; runClassOrigin = 'banana' }
    $onBogusOrigin = Invoke-Classify -Path $bogusOrigin -Class 'green-by-luck' -Because 'the origin is not a word this taxonomy uses'
    Assert-True ((-not $onBogusOrigin.Ok) -and ($onBogusOrigin.Error -match 'neither .*automatic.* nor .*human|not a recognised origin')) "an origin outside its own vocabulary is refused rather than trusted"

    # gate.ps1:441 emits UNCLASSIFIED only when the COMBINED result did not pass. The #640 fix
    # corroborated the green side and left the red side unchecked -- half a mirror.
    # The case gate.ps1's own comment records as having happened: stages all passed, so $Status was
    # GREEN, while the stricter combined verdict was false because of stale artifacts. The two
    # verdicts DISAGREE, and only a rule that reads both gets this right. Without this fixture, a
    # sabotage that drops $PassedEverything from the rule changes no behaviour any cell observes.
    $staleArtifacts = New-Manifest -Properties @{ runClass = 'green'; status = 'GREEN'; overallPassed = $false }
    $onStale = Invoke-Classify -Path $staleArtifacts -Class 'green-by-luck' -Because 'GREEN stages, failing combined verdict'
    Assert-True ((-not $onStale.Ok) -and ($onStale.Error -match 'not corroborated')) "a GREEN status with a failing combined verdict does not corroborate a green class"

    $impossible = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'GREEN'; overallPassed = $true }
    $onImpossible = Invoke-Classify -Path $impossible -Class 'real-red' -Because 'unclassified, yet everything passed'
    Assert-True ((-not $onImpossible.Ok) -and ($onImpossible.Error -match 'not corroborated')) "UNCLASSIFIED beside a fully passing status is refused: the gate never writes that pair"

    # CONTROLS. A corroborator that AGREES must block nothing -- otherwise a guard that refuses
    # whenever the field is present passes every sabotage and proves nothing.
    $honest = New-Manifest -Properties @{ runClass = 'real-red'; runClassOrigin = 'human'; status = 'RED'; overallPassed = $false }
    $onHonest = Invoke-Classify -Path $honest -Class 'dead' -Because 'agreeing origin, still a human judgement'
    Assert-True ((-not $onHonest.Ok) -and ($onHonest.Error -match 'by a person')) "an AGREEING origin still routes to the human-judgement refusal, not to a corroboration refusal"

    $honestUnclassified = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false; runClassOrigin = 'automatic' }
    $onHonestUnclassified = Invoke-Classify -Path $honestUnclassified -Class 'real-red' -Because 'every corroborator agrees'
    Assert-True $onHonestUnclassified.Ok "a manifest whose corroborators all agree is classified normally (was: $($onHonestUnclassified.Error))"

    # #644, the debt declared on #640 and now paid. Everything above rests on ONE premise: the two
    # producers have disjoint closed vocabularies, which holds only while `gate.ps1` assigns
    # `runClass` in exactly ONE place. A second assignment could quietly win over the first and
    # nothing here would go red -- the derivation would keep answering, wrongly.
    #
    # The guard lives with the CONSUMER of the premise, not the producer: `classify-run.ps1` is what
    # breaks if the premise fails, and a guard beside the code that depends on it is the one someone
    # reads when they change that code.
    #
    # Parsed, not grepped. `^\s*\$runClass\s*=` counts source TEXT: it misses an assignment inside
    # a block on one line, and matches one inside a comment or a here-string. The AST answers about
    # the program.
    $gatePath = Join-Path $scriptDir 'gate.ps1'
    $parseErrors = $null
    $gateAst = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors)
    # The ARRANGEMENT is asserted before the assertion that depends on it: a file that did not parse
    # would report zero assignments, and a zero would read as "the premise holds" when it means
    # "nothing was measured".
    Assert-True ($parseErrors.Count -eq 0) "gate.ps1 parses, so the assignment count below is a measurement rather than a silent zero"
    $runClassAssignments = $gateAst.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -eq 'runClass'
    }, $true)
    Assert-True ($runClassAssignments.Count -eq 1) "gate.ps1 assigns runClass exactly once (found $($runClassAssignments.Count)); the origin derivation is unsound the moment there are two"
    # #751: THE COUPLING WAS A COMMENT. classify-run.ps1 carries `$GateStatuses` and gate.ps1
    # carries the [ValidateSet] that defines it, and the only thing tying them together was a
    # sentence saying so. A status added on one side alone makes this file refuse every manifest
    # that carries it, with the message "not a status the gate writes" -- which would be false, and
    # would refuse exactly the runs the new status was added to describe. Fail-closed, so not
    # dangerous; wrong, and silent until someone reads a refusal they cannot explain.
    #
    # Both sides are read from the AST, neither is run: this file must not execute gate.ps1, and a
    # regex over either would match the vocabulary quoted in a comment.
    $validateSets = $gateAst.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.AttributeAst] -and
            $node.TypeName.Name -eq 'ValidateSet'
        }, $true)
    Assert-True ($validateSets.Count -eq 1) "gate.ps1 holds exactly one ValidateSet (found $($validateSets.Count)); with two, the one read below is a sample rather than the vocabulary"
    $gateStatusValues = @($validateSets[0].PositionalArguments | ForEach-Object { $_.Value })
    $classifyAst = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $scriptDir 'classify-run.ps1'), [ref]$null, [ref]$null)
    $gateStatusesAssign = @($classifyAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
                $node.Left.VariablePath.UserPath -eq 'GateStatuses'
            }, $true))
    Assert-True ($gateStatusesAssign.Count -eq 1) "classify-run.ps1 assigns GateStatuses exactly once (found $($gateStatusesAssign.Count))"
    $classifyStatusValues = @($gateStatusesAssign[0].Right.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.StringConstantExpressionAst]
            }, $true) | ForEach-Object { $_.Value })
    $missingHere = @($gateStatusValues | Where-Object { $classifyStatusValues -cnotcontains $_ })
    $extraHere = @($classifyStatusValues | Where-Object { $gateStatusValues -cnotcontains $_ })
    Assert-True ($gateStatusValues.Count -gt 0) "ARRANGEMENT: the ValidateSet yielded values (found $($gateStatusValues.Count)), so an empty comparison below is not a silent pass"
    Assert-True (($missingHere.Count -eq 0) -and ($extraHere.Count -eq 0)) `
        ("this file's GateStatuses IS gate.ps1's ValidateSet, derived rather than copied" +
            $(if ($missingHere.Count) { " -- missing here: $($missingHere -join ', ')" } else { '' }) +
            $(if ($extraHere.Count) { " -- not in the gate: $($extraHere -join ', ')" } else { '' }))

    # #644: the gate's class rule now lives in ONE place, and these cells are what that buys.
    # Before extraction, `classify-run.ps1` recomputed the rule to catch a manifest lying about it --
    # a second copy of the oracle, written to police the first. An oracle duplicated is an oracle
    # that can disagree with itself, and the copy nobody looks at is the one that drifts.
    . (Join-Path $scriptDir 'run-class.ps1')
    Assert-True ((Get-RunClassFrom -Status 'GREEN' -PassedEverything $true) -eq 'green') "GREEN and everything passed is the only pair that yields green"
    Assert-True ((Get-RunClassFrom -Status 'GREEN' -PassedEverything $false) -eq 'UNCLASSIFIED') "a GREEN status whose combined verdict failed is UNCLASSIFIED, not green"
    Assert-True ((Get-RunClassFrom -Status 'RED' -PassedEverything $true) -eq 'UNCLASSIFIED') "a RED status is UNCLASSIFIED even when the combined verdict passed"

    # Both consumers must CALL it rather than keep a copy. Asked of the AST, not of the source text:
    # a grep for the name matches the mention in a comment, and would pass on a file that only talks
    # about the function.
    foreach ($consumer in @('gate.ps1', 'classify-run.ps1')) {
        $consumerAst = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $scriptDir $consumer), [ref]$null, [ref]$null)
        $calls = $consumerAst.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq 'Get-RunClassFrom'
        }, $true)
        Assert-True ($calls.Count -ge 1) "$consumer calls Get-RunClassFrom instead of carrying its own copy of the rule"
    }

    # #645 review (K): in PowerShell, `-eq` and `-ne` against an ARRAY are FILTERS, not tests --
    # both return a collection, and both are truthy. So no comparison written after an untrusted
    # field is read is a scalar test. Measured:
    #     @('automatic','human') -eq 'automatic'  -> truthy
    #     @('automatic','human') -ne 'automatic'  -> truthy
    #     @('human')             -eq 'human'      -> True    <- a one-element array passes AS a scalar
    # Today these are refused, but by the vocabulary check happening to reject a collection -- luck,
    # not a scalar test. The shape is asserted ONCE at the read, so no later `if` is accidentally
    # structural.
    $arrayOrigin = New-Manifest -Properties @{ runClass = 'green'; runClassOrigin = @('automatic', 'human') }
    $onArrayOrigin = Invoke-Classify -Path $arrayOrigin -Class 'green-by-luck' -Because 'the origin is an array'
    Assert-True ((-not $onArrayOrigin.Ok) -and ($onArrayOrigin.Error -match 'not a single value')) "an array-valued runClassOrigin is refused as a SHAPE error, not left to a comparison that cannot test it"

    $arrayClass = New-Manifest -Properties @{ runClass = @('green') }
    $onArrayClass = Invoke-Classify -Path $arrayClass -Class 'green-by-luck' -Because 'the class is a one-element array'
    Assert-True ((-not $onArrayClass.Ok) -and ($onArrayClass.Error -match 'not a single value')) "a one-element array runClass is refused too: it compares equal to its own element"

    # #645 review: a pair the gate cannot emit. gate.ps1:720 makes status RED only when
    # $failed.Count is nonzero, and :441 requires that same count to be ZERO for overallPassed --
    # so overallPassed true implies status GREEN, always. Recomputing the class from an impossible
    # pair yields a plausible answer (UNCLASSIFIED) and lets impossible evidence become an
    # irreversible human judgement. Checked for EVERY class, not only the automatic ones: the
    # manifest that gets persisted as real-red never reaches the automatic branch.
    $impossiblePair = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $true }
    $onImpossiblePair = Invoke-Classify -Path $impossiblePair -Class 'real-red' -Because 'RED yet everything passed'
    Assert-True ((-not $onImpossiblePair.Ok) -and ($onImpossiblePair.Error -match 'cannot emit')) "a RED status claiming overall success is refused: the gate cannot emit that pair"

    # Not special-cased to RED: the rule is that overallPassed true implies status GREEN.
    $abortedPair = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'ABORTED-BY-CANARY'; overallPassed = $true }
    $onAbortedPair = Invoke-Classify -Path $abortedPair -Class 'dead' -Because 'aborted yet everything passed'
    Assert-True ((-not $onAbortedPair.Ok) -and ($onAbortedPair.Error -match 'cannot emit')) "the same refusal covers ABORTED-BY-CANARY, because the rule is about the pair and not about RED"

    # Found while fixing the above: [bool]'false' is TRUE in PowerShell, so a string where the
    # gate writes a Boolean would read as success. The type is part of the shape.
    $stringPassed = New-Manifest -Properties @{ runClass = 'green'; status = 'GREEN'; overallPassed = 'false' }
    $onStringPassed = Invoke-Classify -Path $stringPassed -Class 'green-by-luck' -Because 'overallPassed is the STRING false'
    Assert-True ((-not $onStringPassed.Ok) -and ($onStringPassed.Error -match 'not a Boolean')) "a non-Boolean overallPassed is refused rather than cast, because [bool] on a non-empty string is always true"

    # #645 review: the FIFTH turn of the same screw in this pair of files. Shape was checked
    # (scalar), type was checked (Boolean), agreement was checked -- and a `status` that is not a
    # word the gate can write still entered the comparison. `'banana' -ne 'GREEN'` yields exactly
    # what `'RED' -ne 'GREEN'` yields, so the refusal came out PLAUSIBLE and for the wrong reason.
    # The vocabulary is closed and lives in gate.ps1:391 as [ValidateSet('GREEN','RED',
    # 'ABORTED-BY-CANARY')], confirmed by AST as the only one in that file.
    $bogusStatus = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'banana'; overallPassed = $false }
    $onBogusStatus = Invoke-Classify -Path $bogusStatus -Class 'real-red' -Because 'the status is not a word the gate writes'
    Assert-True ((-not $onBogusStatus.Ok) -and ($onBogusStatus.Error -match 'not a status the gate writes')) "a status outside the gate's vocabulary is refused AT THE READ, not left to a comparison that cannot tell it from RED"

    # Present-but-null is a third fact again: absent means the manifest predates the field, null
    # means something wrote nothing where a verdict belongs.
    $nullStatus = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = $null; overallPassed = $false }
    $onNullStatus = Invoke-Classify -Path $nullStatus -Class 'dead' -Because 'the status is present and null'
    Assert-True ((-not $onNullStatus.Ok) -and ($onNullStatus.Error -match 'not a status the gate writes')) "a present-but-null status is refused too: absent and null are different facts"

    # CONTROL: the third legal value must still pass. A vocabulary guard that only admits the two
    # values my other fixtures happen to use would pass every sabotage above and reject real runs.
    $aborted = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'ABORTED-BY-CANARY'; overallPassed = $false }
    $onAborted = Invoke-Classify -Path $aborted -Class 'dead' -Because 'a genuine canary abort'
    Assert-True $onAborted.Ok "ABORTED-BY-CANARY is a legal status and classifies normally (was: $($onAborted.Error))"

    # #645 review: PRESENT-BUT-NULL is not ABSENT, and I applied that distinction to `status` and
    # not to `overallPassed`. The type-check was guarded by `$null -ne $passedField`, which is FALSE
    # for a present null -- so the check was skipped and `[bool]$null` silently coerced to false.
    # A null corroborator then agreed with a RED status and corroborated the class.
    $nullPassed = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $null }
    $onNullPassed = Invoke-Classify -Path $nullPassed -Class 'real-red' -Because 'overallPassed is present and null'
    Assert-True ((-not $onNullPassed.Ok) -and ($onNullPassed.Error -match 'present and null')) "a present-but-null overallPassed is refused, not coerced to false"

    # CONTROL: ABSENT must still be tolerated at the read, or every manifest written before these
    # fields existed becomes unclassifiable. The distinction is the whole point of the fix.
    $utf8NoBom2 = New-Object System.Text.UTF8Encoding($false)
    $legacyBare = Join-Path $sandbox 'eeeeeeeeeeee-20260828T000000Z.json'
    [System.IO.File]::WriteAllText($legacyBare, (@{ headSha = 'e' * 40; runClass = 'real-red' } | ConvertTo-Json -Depth 8), $utf8NoBom2)
    $onLegacyBare = Invoke-Classify -Path $legacyBare -Class 'dead' -Because 'no status or overallPassed at all'
    Assert-True ((-not $onLegacyBare.Ok) -and ($onLegacyBare.Error -match 'by a person')) "a manifest with NEITHER field still reaches the human-judgement refusal: absent is tolerated, null is not"

    Write-Host "`n-- both copies of one run are read by the same reader --"

    # #753, found while writing the comparison cells. `Get-Content -Raw` decodes with the ANSI code
    # page under Windows PowerShell 5.1; the gate writes UTF-8 without a BOM and this script reads
    # the committable copy with `File.ReadAllText`. Two decoders over one record: every byte above
    # 7F in the durable twin came back as two or three characters that were never in the file, so
    # the twin check could report a disagreement between two byte-identical copies.
    #
    # Asserted over the source, and said plainly why: the values that reach the twin comparison are
    # constrained to a closed ASCII vocabulary before it, so no fixture can drive a non-ASCII value
    # THROUGH that comparison -- the defect is real and its only observable effect is on values the
    # earlier guards reject. A cell that cannot reach it structurally is the honest one.
    $classifySource = [System.IO.File]::ReadAllText($classify)
    Assert-True ($classifySource -notmatch 'Get-Content -LiteralPath \$durableTwin') `
        "the durable twin is not read with Get-Content, which decodes with the ANSI code page"
    Assert-True ($classifySource -match '\$twin = \[System\.IO\.File\]::ReadAllText\(\$durableTwin\)') `
        "it is read with the same reader as the committable copy, so one record has one decoding"
    # Not vacuous: the pattern that must find nothing does find the old form when it is present.
    Assert-True ('    $twin = Get-Content -LiteralPath $durableTwin -Raw | ConvertFrom-Json' -match 'Get-Content -LiteralPath \$durableTwin') `
        "and the sweep recognises the old reader when one is put in front of it"

    Write-Host "`n-- the comparisons that decide are ordinal, because -eq is not --"

    # #753. PowerShell's `-eq`/`-ne`/`-in`/`-notin`/`-contains` are case-insensitive AND CULTURE
    # aware, and a culture comparison gives some code points no weight at all. Measured here:
    #     ('GREEN' + [char]0xFE00) -eq 'GREEN'                     -> True
    #     ('green' + [char]0xFE00) -in @('green', 'UNCLASSIFIED')  -> True
    # U+FE00 is a variation selector: category Mn, ORDINARY TEXT. Refusing the character would be a
    # deny-list growing by one code point per review; what stops being approximate is the
    # COMPARISON. The payload is invisible on purpose -- a visible character would redden these
    # cells for a different reason and they would stop proving what they say.
    #
    # This file's own comment at the vocabulary check states the property the comparer breaks: "a
    # value outside the set compares unequal to every member". It does not.
    $weightless = [string][char]0xFE00
    Assert-True ((('GREEN' + $weightless) -eq 'GREEN') -and
        (-not [string]::Equals(('GREEN' + $weightless), 'GREEN', [System.StringComparison]::Ordinal))) `
        "ARRANGEMENT: the culture-aware operator calls these two strings equal and the ordinal one does not"

    # THE CLOSED VOCABULARY, refused at the read.
    $sneakyStatus = New-Manifest -Properties @{ runClass = 'green'; status = 'GREEN' + $weightless }
    $onSneakyStatus = Invoke-Classify -Path $sneakyStatus -Class 'green-by-luck' -Because 'the status carries a weightless code point'
    Assert-True ((-not $onSneakyStatus.Ok) -and ($onSneakyStatus.Error -match 'vocabulary is closed')) `
        "a status of GREEN plus a weightless code point is refused by the closed vocabulary (was: $($onSneakyStatus.Error))"

    # THE ORIGIN DERIVATION, which rests on two disjoint closed vocabularies.
    $sneakyClass = New-Manifest -Properties @{ runClass = 'green' + $weightless }
    $onSneakyClass = Invoke-Classify -Path $sneakyClass -Class 'green-by-luck' -Because 'the class carries a weightless code point'
    Assert-True ((-not $onSneakyClass.Ok) -and ($onSneakyClass.Error -match 'belongs to neither vocabulary')) `
        "a runClass of green plus a weightless code point belongs to neither vocabulary (was: $($onSneakyClass.Error))"

    # THE SECOND WITNESS: an origin outside its own vocabulary corroborates nothing.
    $sneakyOrigin = New-Manifest -Properties @{ runClass = 'green'; runClassOrigin = 'automatic' + $weightless }
    $onSneakyOrigin = Invoke-Classify -Path $sneakyOrigin -Class 'green-by-luck' -Because 'the origin carries a weightless code point'
    Assert-True ((-not $onSneakyOrigin.Ok) -and ($onSneakyOrigin.Error -match "neither 'automatic' nor 'human'")) `
        "a runClassOrigin of automatic plus a weightless code point is refused (was: $($onSneakyOrigin.Error))"

    # THE DURABLE TWIN. Two copies of one run differing only by a weightless code point are not in
    # agreement, and this cell could not exist before the decoder fix: reading the twin with the
    # ANSI code page turned U+FE00 into three characters, so the disagreement fired for the wrong
    # reason and the comparison underneath looked sound.
    $twinName = [Guid]::NewGuid().ToString('N').Substring(0, 12) + "-20260828T000000Z.json"
    $twinDir = Join-Path $sandbox 'gate-runs'
    [System.IO.Directory]::CreateDirectory($twinDir) | Out-Null
    $utf8NoBomTwin = New-Object System.Text.UTF8Encoding($false)
    $committablePath = Join-Path $sandbox $twinName
    [System.IO.File]::WriteAllText($committablePath,
        (@{ headSha = 'f' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green' } | ConvertTo-Json -Depth 8), $utf8NoBomTwin)
    [System.IO.File]::WriteAllText((Join-Path $twinDir $twinName),
        (@{ headSha = 'f' * 40; status = 'GREEN'; overallPassed = $true; runClass = 'green' + $weightless } | ConvertTo-Json -Depth 8), $utf8NoBomTwin)
    $onTwin = Invoke-Classify -Path $committablePath -Class 'green-by-luck' -Because 'the twin differs by a weightless code point'
    Assert-True ((-not $onTwin.Ok) -and ($onTwin.Error -match 'durable twin')) `
        "a durable twin differing only by a weightless code point is a disagreement (was: $($onTwin.Error))"

    # THE RULE ITSELF, in the file that owns it and that the GATE also dot-sources.
    Assert-True ((Get-RunClassFrom -Status ('GREEN' + $weightless) -PassedEverything $true) -eq 'UNCLASSIFIED') `
        "GREEN plus a weightless code point is not the GREEN that yields green"

    # THE SWEEP, and it runs the COMMITTED tool rather than a copy of its logic. `ci/find-culture-
    # comparisons.ps1` is dot-sourced here so the suite and a reviewer at a terminal read the same
    # answer from the same producer -- a sweep the reviewer cannot re-run is an assertion with
    # numbers in it.
    #
    # Asked of the AST, not of the source text: a regex flags the operator named inside a THROW
    # MESSAGE -- ci/classify-run.ps1 has one -- and a sweep with a false positive is a sweep somebody
    # deletes three months later. Comparisons against $null, $true and $false are exempt as identity
    # and boolean tests.
    #
    # BINARY, and the assertion says so. The sweep inspects BinaryExpressionAst and nothing else, so
    # it does not see the other idiomatic ways to dispatch on a closed vocabulary in PowerShell --
    # a `switch` clause, a hashtable lookup by key, or `.StartsWith`/`.Equals` without an explicit
    # StringComparison -- all of which are culture aware too. Neither of these two files uses any of
    # them (measured), so the sweep is complete FOR THEM and the name is narrow enough to stay
    # honest when the next file does. (Named by J in review of #759.)
    . (Join-Path $scriptDir 'find-culture-comparisons.ps1')
    foreach ($subject in @('classify-run.ps1', 'run-class.ps1')) {
        $flagged = @(Find-CultureComparison -File (Join-Path $scriptDir $subject))
        Assert-True ($flagged.Count -eq 0) `
            ("no BINARY comparison over text in $subject is left to the culture comparer" +
                $(if ($flagged.Count -gt 0) { ': ' + (($flagged | Select-Object -First 2 | ForEach-Object { $_.Text }) -join ' | ') } else { '' }))
    }

    # Not vacuous, and the canary goes through the SAME function on a real file: it must find the
    # text comparison and leave the null check alone. A canary that exercised a reimplementation
    # here would prove the reimplementation.
    $canaryFile = Join-Path $sandbox 'canary.ps1'
    [System.IO.File]::WriteAllText($canaryFile,
        "if (`$a -eq 'green') { 1 }`nif (`$null -eq `$b) { 2 }`n", (New-Object System.Text.UTF8Encoding($false)))
    $canaryFlagged = @(Find-CultureComparison -File $canaryFile)
    Assert-True ($canaryFlagged.Count -eq 1) `
        "and the sweep finds a text comparison when one is put in front of it, while leaving a null check alone"

    Write-Host "`n-- the sweep reads a native command's exit code before anything can stop the pipeline --"

    # `Select-Object -First 1` stops the pipeline once it has its item, which TERMINATES the native
    # command feeding it. On PowerShell 7 that leaves `$LASTEXITCODE` at -1 with the VALUE correct,
    # so `git rev-parse --show-toplevel | Select-Object -First 1` made the tool refuse with "not
    # inside a git working tree" while holding that tree's path. Reported by a reviewer who could not
    # run the documented invocation; NOT reproducible on this host, which is 5.1 and exits 0 both
    # ways -- so the cell is structural and says why.
    #
    # The rule is about the SHAPE: capture, read $LASTEXITCODE, then reduce. Asserted over the file
    # rather than by running it, because the behaviour that breaks it belongs to another edition.
    $sweepSource = [System.IO.File]::ReadAllText((Join-Path $scriptDir 'find-culture-comparisons.ps1'))
    $stoppingPipes = @(($sweepSource -split "`n") | Where-Object {
            $_ -match '&\s+git' -and $_ -match '\|\s*(Select-Object|Where-Object)' -and $_ -notmatch '^\s*#'
        })
    Assert-True ($stoppingPipes.Count -eq 0) `
        ("no native command in the sweep is piped before its exit code is read" +
            $(if ($stoppingPipes.Count -gt 0) { ': ' + (($stoppingPipes | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))
    # Not vacuous: the pattern finds the old line when it is put in front of it.
    $canaryLine = '    $root = (& git rev-parse --show-toplevel 2>$null | Select-Object -First 1)'
    Assert-True (($canaryLine -match '&\s+git') -and ($canaryLine -match '\|\s*(Select-Object|Where-Object)')) `
        'and the pattern recognises the shape it is looking for when one is put in front of it'

    Write-Host "`n-- a git that FAILED is not a git that found nothing --"

# THE OTHER HALF OF A HAZARD THIS BLOCK ALREADY KNEW ABOUT. classify-run.ps1 carries a comment
# explaining that a redirected native stderr becomes a NativeCommandError which 'Stop' promotes to
# a terminating error, and it opens an $ErrorActionPreference='Continue' window to survive that.
# That fixes the TERMINATION half. The STATUS half was left open: none of the three git calls read
# an exit code, so a git that ran and REFUSED contributed nothing to $touched and read exactly like
# a git that answered "this range touches no files".
#
# WHICH WAY THE MISTAKE FALLS IS THE POINT. The block's own comment says under-matching "certifies
# a touched file as untouched, and this check exists precisely to stop a failure in a file the diff
# touches being called unrelated". A failed `git log origin/main..HEAD` does exactly that, and the
# script then RECORDS `unrelatedTestFileVerifiedAgainstDiff = $true` -- a red attributed away from
# the author's own diff, in the ledger, with the verification field saying it was checked.
#
# THE MISSING REF IS NOT HYPOTHETICAL: the gate runner (retired 2026-09-24) documented and repaired
# `--single-branch` clones in this same factory, where nothing populates refs/remotes/origin/main.
function New-GitShim {
    <#
      .SYNOPSIS
        A real `git.cmd` that answers the subcommands this block issues, and fails the one named.
      .DESCRIPTION
        A REAL PROCESS on PATH, because the property under test is an EXIT CODE. A PowerShell
        function named `git` would shadow the call and could not set $LASTEXITCODE at all.
    #>
    param(
        [Parameter(Mandatory)] [string] $TopLevel,
        [AllowEmptyString()] [string] $FailSubcommand = '',
        [AllowEmptyString()] [string] $LogOutputFile = ''
    )
    $dir = Join-Path $sandbox ([Guid]::NewGuid().ToString('N'))
    [System.IO.Directory]::CreateDirectory($dir) | Out-Null
    # GOTO LABELS, not `(echo X & exit /b 0)`. cmd.exe echoes everything up to the `&` INCLUDING the
    # space before it, so the one-line form handed back a top-level path with a trailing space and
    # `Push-Location` answered "An object at the specified path does not exist" -- a harness failure
    # wearing the shape of the refusal these cells are trying to observe.
    $lines = @('@echo off')
    if ($FailSubcommand) { $lines += "if `"%1`"==`"$FailSubcommand`" goto failsub" }
    $lines += 'if "%1"=="rev-parse" goto revparse'
    if ($LogOutputFile) { $lines += 'if "%1"=="log" goto dolog' }
    $lines += 'exit /b 0'
    if ($FailSubcommand) {
        $lines += ':failsub'
        $lines += 'echo fatal: ambiguous argument 1>&2'
        $lines += 'exit /b 128'
    }
    $lines += ':revparse'
    $lines += "echo $TopLevel"
    $lines += 'exit /b 0'
    if ($LogOutputFile) {
        $lines += ':dolog'
        $lines += "type `"$LogOutputFile`""
        $lines += 'exit /b 0'
    }
    [System.IO.File]::WriteAllText((Join-Path $dir 'git.cmd'), (($lines -join "`r`n") + "`r`n"))
    return $dir
}

function Invoke-ClassifyUnrelated {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [string] $TestFile,
        [Parameter(Mandatory)] [string] $ShimDirectory
    )
    $savedPath = $env:PATH
    try {
        $env:PATH = "$ShimDirectory;$env:SystemRoot\system32"
        & $classify -Manifest $Path -Class 'real-red' -Because 'unrelated failure' `
            -UnrelatedTestFile $TestFile -UnrelatedIssue '1' -FailThenPassObserved *> $null
        return @{ Ok = $true; Error = '' }
    } catch {
        return @{ Ok = $false; Error = $_.Exception.Message }
    } finally {
        $env:PATH = $savedPath
    }
}

$shimTop = $sandbox.Replace('\', '/')

# ARRANGEMENT: with a git that answers and reports NOTHING touched, the claim is accepted. Without
# this cell the refusals below could be a script that refuses everything.
$cleanRun = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false }
$acceptShim = New-GitShim -TopLevel $shimTop
$accepted = Invoke-ClassifyUnrelated -Path $cleanRun -TestFile 'core/untouched/tests/far_away.rs' -ShimDirectory $acceptShim
Assert-True $accepted.Ok "ARRANGEMENT: a git that answers, with the file untouched, ACCEPTS the unrelated claim (was: $($accepted.Error))"
Assert-True ((Read-Manifest -Path $cleanRun).unrelatedTestFileVerifiedAgainstDiff -eq $true) `
    'and the accepted claim is what records unrelatedTestFileVerifiedAgainstDiff'

# THE TRAP: git RUNS and REFUSES on the commit range.
$failedRun = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false }
$failShim = New-GitShim -TopLevel $shimTop -FailSubcommand 'log'
$refusedRead = Invoke-ClassifyUnrelated -Path $failedRun -TestFile 'core/untouched/tests/far_away.rs' -ShimDirectory $failShim
Assert-True (-not $refusedRead.Ok) `
    "a git that EXITED 128 on the commit range refuses the claim instead of certifying it (was: $($refusedRead.Error))"
Assert-True ($refusedRead.Error -match 'UNREADABLE rather than empty') `
    'and the refusal says the range was unreadable, not that it was empty'
Assert-True ((Read-Manifest -Path $failedRun).PSObject.Properties.Name -notcontains 'unrelatedTestFileVerifiedAgainstDiff') `
    'and NOTHING is recorded -- an unverifiable claim must not leave a verification field behind'

# CONTROL: the ORIGINAL refusal must survive the new one. A git that answers and reports the file
# IN the range still refuses, with its own message -- the exit-code check must not have replaced it.
$touchedRun = New-Manifest -Properties @{ runClass = 'UNCLASSIFIED'; status = 'RED'; overallPassed = $false }
$logFile = Join-Path $sandbox 'log-output.txt'
[System.IO.File]::WriteAllText($logFile, "core/touched/tests/near.rs`r`n")
$touchedShim = New-GitShim -TopLevel $shimTop -LogOutputFile $logFile
$refusedTouch = Invoke-ClassifyUnrelated -Path $touchedRun -TestFile 'core/touched/tests/near.rs' -ShimDirectory $touchedShim
Assert-True (-not $refusedTouch.Ok -and $refusedTouch.Error -match 'IS touched by this diff') `
    "CONTROL: a file the range DOES touch is still refused by the original check (was: $($refusedTouch.Error))"

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
