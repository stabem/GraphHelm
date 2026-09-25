<#
.SYNOPSIS
    Settle a gate run's CLASS after the holder has read the failure. #199.

.DESCRIPTION
    `status: RED` records that a run failed and nothing about whether the failure means anything.
    Three classes are indistinguishable in an exit code:

        real-red       the code failed something. The only class that is evidence about code.
        dead           killed by a lock collision, a clean underneath it, an aborted slot.
                       Says nothing at all.

    `instrument-red` USED TO BE A THIRD CLASS AND IS NOT ONE ANY MORE. Real data would not fit it:
    A Agent's `gate.log` failed `rustfmt` and `clippy` -- CODE -- while carrying four stale binaries
    -- INSTRUMENT -- in the SAME run. Both true at once, and an exclusive class forces one label,
    erasing the half that decides whether the red is citable.

    So "was the instrument broken?" is now the manifest's own DERIVED field, `instrumentSuspect`,
    computed by `gate.ps1` from `artifactsUnprovenReuse` -- the artefacts this run could not vouch
    for by CONTENT, which since #904 is the population that decides, not the mtime-derived
    `staleArtifactCount` that only records what a cold-only gate would have refused -- together with
    `canaryPassed`, `targetBuildState` and the artefact build pass's own exit, at the moment those
    numbers are produced. This script only READS it. It does not compute it, deliberately: a value
    calculated at classification time would attach itself to runs nobody measured.

    ABSENT IS NOT FALSE. A pre-taxonomy manifest has no `instrumentSuspect`, and that means NOT
    MEASURED -- never "the instrument was healthy".

    Measured on 2026-08-20: one lane produced three runs, one of each, and ZERO verdicts between
    them - while a census that counted "two complete runs, both RED" had no way to tell them apart.

    The gate cannot decide this. A GREEN run classifies itself; a RED one is a judgement the holder
    makes once they have read the failure, so the manifest ships `runClass: UNCLASSIFIED` and this
    script is how that value stops being UNCLASSIFIED.

    Both the in-repo manifest and the durable copy are updated when both are present, because a
    class that lands on one of two copies is worse than no class: the two then disagree.

.PARAMETER Manifest
    Path to the run manifest, in the repo or in the slot directory.

.PARAMETER Class
    real-red | dead | green-by-luck | flaky-observed

    The first two judge a RED. The last two refine an automatic GREEN, and exist because a green
    that passed by luck was previously unrecordable (#639). `instrument-red` is named here no
    longer: this file's own description removed it as a class, and the line that still listed it
    was stale. Corrected rather than left to contradict the paragraph above it.

.PARAMETER Because
    One line of why. Required: a class with no reason is a label, and the next reader cannot check
    a label.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Manifest,
    [Parameter(Mandatory)] [ValidateSet('real-red', 'dead', 'green-by-luck', 'flaky-observed')] [string] $Class,
    [Parameter(Mandatory)] [string] $Because,

    # #199: declare the failure UNRELATED to the diff under judgement. All three are required
    # together, because each alone is cheap to assert and worthless on its own:
    #   -UnrelatedTestFile   the failing test's file. VERIFIED HERE against the diff -- this is the
    #                        one condition a script can check, so it is checked rather than trusted.
    #   -UnrelatedIssue      the issue carrying the evidence. A failure nobody filed is a failure
    #                        nobody will look at again.
    #   -FailThenPassObserved  the holder saw it fail and then pass IN THE SAME RUN. Not checkable
    #                        from here; required explicitly so that claiming it is a deliberate act.
    [string] $UnrelatedTestFile,
    [string] $UnrelatedIssue,
    [switch] $FailThenPassObserved,

    # #199: the OTHER answer, and it deliberately carries NO bar.
    #
    # KNOWN AND DELIBERATE ASYMMETRY, written here so a later census is not surprised by it:
    # `relatedToDiff = $true` travels with only `runClassBecause`, while `$false` travels with three
    # evidence fields. That is not an oversight -- the bar guards the CHEAP claim ("not mine"), and
    # the expensive one needs no guard -- but it means a future count will find `true` unstructured
    # and `false` structured. Anyone aggregating these must not read the missing fields beside a
    # `true` as a weaker record; there is nothing to record. (Observed by C Agent in review.)
    #
    # The three conditions above guard the CHEAP sentence ("not mine"); this is the expensive one,
    # and a holder who reads the failure and concludes it belongs to their own diff must be able to
    # SAY so. Without this switch the field has no path to $true at all, so "judged, and it is mine"
    # would be indistinguishable from "nobody judged" -- the exact collapse this field was added to
    # prevent, on its own axis.
    [switch] $Mine
)

. (Join-Path $PSScriptRoot 'run-class.ps1')
$ErrorActionPreference = 'Stop'

# ORDINAL, BECAUSE -eq AND -in ARE NOT. PowerShell's comparison operators are case-insensitive AND
# CULTURE aware, and a culture comparison gives some code points no weight at all. Measured:
#     ('GREEN' + [char]0xFE00) -eq 'GREEN'                     -> True
#     ('green' + [char]0xFE00) -in @('green', 'UNCLASSIFIED')  -> True
#     [string]::Equals('GREEN' + [char]0xFE00, 'GREEN', 'Ordinal')  -> False
# U+FE00 is a variation selector: category Mn, ordinary text. The answer is not to refuse the
# character -- that is a deny-list growing by one code point per review -- but to stop comparing
# approximately.
#
# This matters here more than almost anywhere, because this file's guards are CLOSED VOCABULARIES
# and its own comment at the vocabulary check states the property the comparer breaks: "a value
# outside the set compares unequal to every member". It does not. A manifest whose `status` was
# GREEN plus an invisible code point passed the closed set, was recomputed as an automatic `green`,
# and became refinable -- and the same shape let a `runClass` and a stated `runClassOrigin` through
# their own vocabularies.
#
# Case matters in every vocabulary here (`GREEN`, `green`, `human`), so these are Ordinal. The two
# PATH comparisons further down are OrdinalIgnoreCase instead, because Windows paths are
# case-insensitive and making them case-sensitive would be a different defect wearing this fix.
function Test-SameText {
    param(
        [Parameter(Position = 0)] [AllowNull()] [object] $Left,
        [Parameter(Position = 1)] [AllowNull()] [object] $Right
    )
    # Null is not the empty string: `[string] $null` is '', so a typed parameter would make an
    # absent value equal to an empty one. Both nulls are answered before any cast.
    if ($null -eq $Left -or $null -eq $Right) { return ($null -eq $Left -and $null -eq $Right) }
    return [string]::Equals([string]$Left, [string]$Right, [System.StringComparison]::Ordinal)
}

function Test-InVocabulary {
    param([Parameter()] [AllowNull()] [object] $Value, [Parameter(Mandatory)] [string[]] $Vocabulary)
    foreach ($member in $Vocabulary) { if (Test-SameText $Value $member) { return $true } }
    return $false
}

function Test-NameIsPresent {
    param([Parameter(Mandatory)] $Object, [Parameter(Mandatory)] [string] $Name)
    foreach ($property in $Object.PSObject.Properties.Name) { if (Test-SameText $property $Name) { return $true } }
    return $false
}

function Test-SamePath {
    # OrdinalIgnoreCase, deliberately. What is removed is the CULTURE, not the case-insensitivity --
    # and the justification is not the same at all three call sites, so it is written at each one
    # rather than assumed from the name:
    #   :318 and :453 compare two RESOLVED filesystem paths, where Windows itself is
    #        case-insensitive and two spellings name one file;
    #   :399 compares paths that came out of GIT, whose index IS case-sensitive, so that argument
    #        does not hold there. It stays OrdinalIgnoreCase for the opposite reason, written at
    #        the site: over-matching there REFUSES.
    # (The third site was mine to justify and I had not; found by J in review of #759.)
    param([Parameter(Mandatory)] [string] $Left, [Parameter(Mandatory)] [string] $Right)
    return [string]::Equals($Left, $Right, [System.StringComparison]::OrdinalIgnoreCase)
}

if (-not (Test-Path -LiteralPath $Manifest)) {
    throw "no manifest at $Manifest"
}

# ReadAllText, then a strict parse: this file exists to be machine-read, and a BOM or a provider-
# decorated string would defeat that downstream. Same discipline as the writer in gate.ps1.
$json = [System.IO.File]::ReadAllText($Manifest)
$run = $json | ConvertFrom-Json

# #199: ERA MARKER. A manifest written before this change has NO `runClass` property at all, and
# PowerShell reads an absent property as falsy -- so the guard below waved it straight through and
# this script would have written a class onto a pre-#202 manifest, producing a record
# INDISTINGUISHABLE from one the new gate had produced. The run it describes was never judged
# against these categories, and nothing in the file would say so afterwards. (Found in review by
# L Agent.) Refused by NAME rather than by value, because absent and UNCLASSIFIED are different
# facts: one means "not judged yet", the other means "this gate could not have judged it".
if (-not (Test-NameIsPresent -Object $run -Name 'runClass')) {
    throw "this manifest has no runClass field, so it predates the taxonomy (#202). Classifying it would produce a record that looks like a judged run of the new gate and is not one. Re-run the gate, or annotate the file by hand and say it was pre-taxonomy."
}

# #639: the refusal is about ORIGIN, not existence. The invariant this guard protects is that A
# HUMAN JUDGEMENT IS NOT OVERWRITTEN -- and an automatic `green` is not a judgement. It is what
# `gate.ps1` writes when no stage failed: the absence of failures, not anyone's reading of them.
# Refusing on existence made a green TERMINAL, so a green whose own author had measured it as luck
# (PASS=4 FAIL=1) stayed recorded as proof, with the refutation stranded in a commit message the
# ledger does not link to.
$GreenSideClasses = @('green-by-luck', 'flaky-observed')

# THE CLASS these guards belong to, written once so the fourth site is not rediscovered by a sweep:
# a manifest field is believed only alongside the fields that CORROBORATE it. Enumerated over every
# site in this file that reads a field and acts on it -- five, of which two were already sound
# (:104 acts on ABSENCE, which is the fact itself; the instrumentSuspect block only PRINTS) and
# three needed this: the class needs `runClassOrigin`, an automatic class needs `status` +
# `overallPassed`, and a refinement needs the DURABLE TWIN. (Found in review on #640.)
#
# THE PREDICATE, corrected on #644, because the first version of this rule was too weak: it is not
# enough that the corroborating field EXISTS and holds a legal value. IT MUST AGREE. A stated
# `runClassOrigin: automatic` beside `runClass: real-red` is present, spelled correctly, and false --
# and taken raw it walked straight past the human-judgement guard. Every pair below is therefore
# checked for AGREEMENT, and where the two disagree the manifest is refused rather than believed.

# Absent `runClassOrigin` is DERIVED, and the derivation is sound rather than a guess: the two
# producers have disjoint, closed vocabularies. `green` and `UNCLASSIFIED` can only come from
# `gate.ps1`; every other class can only come from THIS script, whose ValidateSet cannot emit them.
# So the value determines the origin for every manifest written before the field existed.
# SHAPE IS ASSERTED ONCE, AT THE READ. In PowerShell `-eq` and `-ne` against an array are FILTERS
# returning a collection, and BOTH are truthy -- so no comparison written after an untrusted field is
# read is a scalar test, whichever way its sign points. Worse, a ONE-ELEMENT array compares equal to
# its own element, so `@('human')` behaves exactly like `'human'` until something enumerates it.
#
# Checking the shape here, once, is what makes every `if` below an ordinary scalar comparison. The
# alternative -- hardening each comparison -- leaves the next one added by the next author unhardened.
# (Found in review by K on #645; the previous code refused these by luck, because the vocabulary
# check happens to reject a collection, not because anything tested the shape.)
function Read-ScalarField {
    param(
        [Parameter(Mandatory)] $Run,
        [Parameter(Mandatory)] [string] $Name,
        # When the gate can only write a CLOSED set of values, give it here. A value outside the set
        # is refused at the read rather than carried into a comparison that cannot tell it apart:
        # `'banana' -ne 'GREEN'` yields exactly what `'RED' -ne 'GREEN'` yields, so the refusal comes
        # out plausible and for the wrong reason. Fifth instance of that pattern in this pair of
        # files. (Found in review on #645.)
        [string[]] $Vocabulary
    )

    if (-not (Test-NameIsPresent -Object $Run -Name $Name)) { return $null }
    $value = $Run.$Name
    if (($value -is [System.Array]) -or ($value -is [System.Collections.IList])) {
        throw "this manifest's $Name is not a single value: it holds a collection of $($value.Count). PowerShell's -eq and -ne both return truthy against an array, so no comparison below could test it. Fix the manifest."
    }
    # PRESENT-BUT-NULL is refused, and that is the whole distinction: ABSENT means the manifest
    # predates the field and is tolerated; NULL means something wrote nothing where a value belongs.
    # The earlier version made this distinction for `status`, through the vocabulary, and NOT for
    # `overallPassed`, whose type check was guarded by `$null -ne $field` -- false for a present null,
    # so the check was skipped and `[bool]$null` coerced silently to false. A null corroborator then
    # agreed with a RED status. PowerShell coerces null to false, to '' and to 0 without complaining,
    # so the only safe place to stop it is before any comparison. (Found in review on #645 -- the
    # sixth instance of this file's own pattern, and the second where the fix was in place for one
    # field and not its neighbour.)
    #
    # Vocabulary first, so a field WITH a closed set keeps the more specific message.
    if ($Vocabulary -and -not (Test-InVocabulary -Value $value -Vocabulary $Vocabulary)) {
        $seen = if ($null -eq $value) { 'null' } else { "'$value'" }
        throw "this manifest's $Name is $seen, which is not a $Name the gate writes. The gate's vocabulary is closed: $($Vocabulary -join ', '). Refused at the read, because a value outside the set compares unequal to every member and would produce a refusal for the wrong reason."
    }
    if ($null -eq $value) {
        throw "this manifest's $Name is present and null. Absent would mean the manifest predates the field, which is tolerated; present and null means something wrote nothing where a value belongs. PowerShell coerces null to false, to '' and to 0 without complaint, so it is refused here rather than carried into a comparison that cannot tell it from a real answer."
    }
    return $value
}

$slotDir = if ($env:GRAPHHELM_SLOT_DIR) { $env:GRAPHHELM_SLOT_DIR } else { 'D:/graphhelm-slot' }
$fileName = Split-Path -Leaf $Manifest
$existingClass = Read-ScalarField -Run $run -Name 'runClass'
# gate.ps1's [ValidateSet] on Write-RunManifest -Status, and the AST confirms it is the only
# ValidateSet in that file, so this is the whole vocabulary rather than a sample of it.
# THE COUPLING WAS A COMMENT AND IS NOW A CELL (#751): this list and that ValidateSet are two
# spellings of one closed set, and nothing re-derived one from the other. A status added on
# one side only makes this file refuse every manifest carrying it, with the message 'not a
# status the gate writes' -- which would be false. classify-run.tests.ps1 now reads the set
# out of gate.ps1's AST and compares, so the two cannot drift again.
$GateStatuses = @('GREEN', 'RED', 'ABORTED-BY-CANARY', 'HARNESS-BROKE')
$statusField = Read-ScalarField -Run $run -Name 'status' -Vocabulary $GateStatuses
$passedField = Read-ScalarField -Run $run -Name 'overallPassed'

# The TYPE is part of the shape. `[bool]` on a non-empty string is TRUE in PowerShell, so the string
# "false" where the gate writes a Boolean would read as success -- and nothing later would notice.
if ($null -ne $passedField -and $passedField -isnot [bool]) {
    throw "this manifest's overallPassed is not a Boolean: it holds '$passedField' of type $($passedField.GetType().Name). PowerShell casts any non-empty string to true, so this cannot be read as a verdict. Fix the manifest."
}

# A PAIR THE GATE CANNOT EMIT, refused before anything is recomputed from it. `gate.ps1:720` makes
# the status RED only when `$failed.Count` is nonzero, and `:441` requires that same count to be ZERO
# for `overallPassed` -- so `overallPassed` true implies status GREEN, always. Recomputing the class
# from an impossible pair yields a PLAUSIBLE answer, which is the dangerous kind: UNCLASSIFIED, which
# then corroborates and lets impossible evidence become an irreversible human judgement.
#
# Checked for EVERY class rather than inside the automatic branch below: the manifest that gets
# persisted as `real-red` never reaches that branch. (Found in review on #645.)
if ($null -ne $statusField -and $true -eq $passedField -and -not (Test-SameText $statusField 'GREEN')) {
    throw "this manifest says overallPassed = true beside status = '$statusField', which gate.ps1 cannot emit: status is non-GREEN only when a stage failed, and overallPassed requires that same count to be zero. Refused before recomputing, because the pair is impossible rather than merely unusual."
}
$AutomaticClasses = @('green', 'UNCLASSIFIED')
$HumanClasses = @('real-red', 'dead', 'green-by-luck', 'flaky-observed')
$OriginVocabulary = @('automatic', 'human')
$existingOrigin = if (Test-InVocabulary -Value $existingClass -Vocabulary $AutomaticClasses) {
    'automatic'
} elseif (Test-InVocabulary -Value $existingClass -Vocabulary $HumanClasses) {
    'human'
} else {
    # REFUSED, not widened. The derivation rests on the two producers having disjoint CLOSED
    # vocabularies; a value in neither falls outside that justification entirely. Reading it as
    # `human` -- the earlier behaviour -- was the expensive guess: a human class cannot be
    # overwritten, so a malformed or untrusted manifest would lock that run out of classification
    # permanently. Saying "I cannot place this" costs one line and locks nothing.
    # (Found in review on #640.)
    throw "this manifest's runClass is '$existingClass', which belongs to neither vocabulary: gate.ps1 writes $($AutomaticClasses -join ', ') and this script writes $($HumanClasses -join ', '). Its origin cannot be derived, so it is refused rather than guessed. Fix the manifest, or say by hand what wrote it."
}

# A STATED origin is CROSS-CHECKED against the class, never taken raw. The class determines the
# origin on its own -- the producers' vocabularies are disjoint -- so a stored `runClassOrigin` is a
# second witness, and a second witness that disagrees is the whole reason to have one. Taking it raw
# made it a way to OVERRIDE the derivation: `real-red` + `automatic` skipped the human-judgement
# guard entirely. (Found in review on #644.)
if (Test-NameIsPresent -Object $run -Name 'runClassOrigin') {
    $statedOrigin = Read-ScalarField -Run $run -Name 'runClassOrigin'
    if (-not (Test-InVocabulary -Value $statedOrigin -Vocabulary $OriginVocabulary)) {
        $seen = if ($null -eq $statedOrigin) { 'null' } else { "'$statedOrigin'" }
        throw "this manifest states runClassOrigin = $seen, which is neither 'automatic' nor 'human'. An origin outside its own vocabulary cannot corroborate anything, so it is refused rather than trusted."
    }
    if (-not (Test-SameText $statedOrigin $existingOrigin)) {
        throw "this manifest's runClass '$existingClass' and its stated runClassOrigin '$statedOrigin' disagree: '$existingClass' can only have been written by a $existingOrigin producer. Present and legal is not the same as agreeing. Reconcile the manifest before classifying."
    }
}

# `gate.ps1:441` is ONE rule with two outcomes: green when $Status is GREEN AND $passedEverything,
# UNCLASSIFIED otherwise. The first version of this check mirrored only the green half, so
# `UNCLASSIFIED` beside a fully passing status -- a pair the gate cannot emit -- passed unexamined.
# Mirroring the whole rule costs the same and leaves no half unchecked. (#644)
#
# ABSENT counts as not corroborated, deliberately: absent and false are different facts, and this
# file already treats them so for `instrumentSuspect`. A class nobody can corroborate is not one
# anybody should act on.
if (Test-InVocabulary -Value $existingClass -Vocabulary $AutomaticClasses) {
    $hasStatus = Test-NameIsPresent -Object $run -Name 'status'
    $hasPassed = Test-NameIsPresent -Object $run -Name 'overallPassed'
    if (-not ($hasStatus -and $hasPassed)) {
        $sawStatus = if ($hasStatus) { "'$($run.status)'" } else { 'ABSENT' }
        $sawPassed = if ($hasPassed) { "$($run.overallPassed)" } else { 'ABSENT' }
        throw "this manifest's runClass is '$existingClass' but that class is not corroborated: status = $sawStatus, overallPassed = $sawPassed. Both are needed to say which class gate.ps1 would have written."
    }
    $classTheGateWouldWrite = Get-RunClassFrom -Status $statusField -PassedEverything ([bool]$passedField)
    if (-not (Test-SameText $classTheGateWouldWrite $existingClass)) {
        throw "this manifest's runClass is '$existingClass' but that class is not corroborated by the fields the gate writes it from: status = '$($run.status)', overallPassed = $($run.overallPassed). From those, gate.ps1 would have written '$classTheGateWouldWrite'. Refused rather than believed."
    }
}

# The DURABLE TWIN. This script writes both copies and has always known the twin exists; it decided
# the refinement from the committable copy alone. Its own doc explains the surviving divergence after
# a partial write -- committable UNCLASSIFIED, durable classified -- and the same crash also leaves
# the committable copy an automatic `green` while the durable one already holds a human judgement.
# Reading the twin is what makes "refine once" true across both copies rather than per file.
$durableTwin = [System.IO.Path]::Combine([System.IO.Path]::Combine($slotDir, 'gate-runs'), $fileName)
if ((Test-Path -LiteralPath $durableTwin) -and
    -not (Test-SamePath (Resolve-Path -LiteralPath $durableTwin).Path (Resolve-Path -LiteralPath $Manifest).Path)) {
    # THE SAME READER AS THE PRIMARY COPY, and this line was not. `Get-Content -Raw` in Windows
    # PowerShell 5.1 decodes with the ANSI code page unless told otherwise, while the gate WRITES
    # UTF-8 without a BOM and this script reads the committable copy with `File.ReadAllText`, which
    # is UTF-8. So the two copies of one run were being decoded two different ways, and any byte
    # above 7F in the twin came back as two or three characters that were never in the file.
    #
    # Measured while writing the ordinal cells: a twin whose runClass carried U+FE00 came back
    # eight characters long instead of six, so the disagreement check fired on the DECODER rather
    # than on the values -- an encoding defect standing in front of a comparison defect and making
    # the comparison look sound. The refusal was right by accident, and the diagnostic printed
    # mojibake at the operator.
    $twin = [System.IO.File]::ReadAllText($durableTwin) | ConvertFrom-Json
    $twinClass = if (Test-NameIsPresent -Object $twin -Name 'runClass') { $twin.runClass } else { 'ABSENT' }
    if (-not (Test-SameText $twinClass $existingClass)) {
        throw "this copy and its durable twin disagree: this one says '$existingClass', the durable twin at $durableTwin says '$twinClass'. Classifying from one copy would overwrite whatever the other already records. Reconcile them before classifying."
    }
}

if ($existingClass -and -not (Test-SameText $existingClass 'UNCLASSIFIED') -and (Test-SameText $existingOrigin 'human')) {
    throw "this run is already classified as '$existingClass' by a person. Classifying twice would overwrite a judgement someone already made; edit deliberately if that is what you mean."
}

# Both rules below anchor on the class being EXACTLY an automatic `green`, and that precision is the
# whole of them. An earlier version gated on `-ne 'UNCLASSIFIED'`, which reads as "nothing failed" and
# is not: `UNCLASSIFIED` is what `gate.ps1` writes when the run did NOT pass everything. So a FAILED
# run could be persisted as `flaky-observed` -- as a HUMAN judgement nothing afterwards overwrites.
# (Found in review by K on #640. It is this file's own rule with the sign flipped, which is why the
# approximate anchor passed a reading and failed a test.)

# A green-side class REFINES an observed pass. Where the gate never observed one, there is nothing
# to refine -- the mirror of the rule below it.
if ((Test-InVocabulary -Value $Class -Vocabulary $GreenSideClasses) -and -not (Test-SameText $existingClass 'green')) {
    $seen = if ($existingClass) { "'$existingClass'" } else { 'no class' }
    throw "'$Class' refines an automatic green, and this run carries $seen. The gate never recorded a passing run here, so there is nothing to refine. A run that failed is judged with: real-red, dead."
}

# An automatic green may be refined but not CONTRADICTED. `real-red` and `dead` are readings of a
# FAILURE this run did not have; applying one would make the ledger assert something the gate never
# observed, which is the defect this change exists to remove rather than to mirror.
if ((Test-SameText $existingClass 'green') -and -not (Test-InVocabulary -Value $Class -Vocabulary $GreenSideClasses)) {
    throw "'$existingClass' was assigned automatically because no stage failed, so it can be refined but not contradicted: '$Class' is a reading of a failure this run did not have. Use one of: $($GreenSideClasses -join ', ')."
}

# #199: attribution, and the gate that keeps "not mine" from being the cheapest sentence to write.
#
# The default is $null -- UNKNOWN -- and stays there unless all three conditions are supplied. Any
# ONE of them alone is an assertion; together they are a record someone else can re-check. The
# boundary is deliberate: WITHOUT THE THREE, A RED IS A RED.
$relatedToDiff = $null
if ($Mine -and ($UnrelatedTestFile -or $UnrelatedIssue -or $FailThenPassObserved)) {
    throw "-Mine and the unrelated-* switches are opposite answers to the same question. Pick one."
}
if ($Mine) {
    $relatedToDiff = $true
}
if ($UnrelatedTestFile -or $UnrelatedIssue -or $FailThenPassObserved) {
    if (-not ($UnrelatedTestFile -and $UnrelatedIssue -and $FailThenPassObserved)) {
        throw "declaring a failure unrelated to the diff needs ALL THREE: -UnrelatedTestFile, -UnrelatedIssue and -FailThenPassObserved. One of them alone is the cheapest sentence in this taxonomy to write and the least checkable."
    }

    # The condition a script CAN check, so it is checked. `git log --name-only` over the range plus
    # the uncommitted diff: if the failing test's file is in either, the failure is inside the work
    # under judgement and this claim is refused rather than recorded.
    # $ErrorActionPreference='Continue' around every native call, and NO `2>$null`. Caught by this
    # script's own live test: under Windows PowerShell 5.1 a redirected native stderr line becomes a
    # NativeCommandError, which 'Stop' promotes to a TERMINATING error -- so git's routine
    # "LF will be replaced by CRLF" warning aborted the check. Worse than aborting: the abort looked
    # like the refusal this block exists to produce, and the negative test PASSED FOR THE WRONG
    # REASON. Same hazard `Invoke-Stage` in `gate.ps1` handles the same way.
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $repoRoot = (git rev-parse --show-toplevel)
    if (-not $repoRoot) { $ErrorActionPreference = $previousPreference; throw "not inside a git repository: cannot verify that $UnrelatedTestFile is outside the diff" }
    Push-Location -LiteralPath $repoRoot
    try {
        # EACH CALL'S STATUS IS READ, and that is the other half of the hazard the comment above
        # describes. The 'Continue' window stops a CHATTY git from aborting this block; it does not
        # make a git that FAILED look like anything other than a git that found nothing. `git log
        # origin/main..HEAD` exits 128 with empty output wherever refs/remotes/origin/main is absent
        # -- the gate runner (retired 2026-09-24) repaired exactly that for `--single-branch` clones
        # in this same factory -- and the committed range then contributes nothing to $touched.
        #
        # WHICH DIRECTION THAT MISTAKE FALLS IS THE POINT, and the comment below on
        # OrdinalIgnoreCase already names it: "under-matching certifies a touched file as untouched,
        # and this check exists precisely to stop a failure in a file the diff touches being called
        # unrelated". An unread range is under-matching at its widest, and the script would go on to
        # RECORD `unrelatedTestFileVerifiedAgainstDiff = $true` for a file the branch did modify.
        #
        # NO `2>&1` ON THESE, deliberately, for the reason the comment above gives: the redirect is
        # what turns git's routine stderr into a NativeCommandError. Unredirected, git's stderr goes
        # to the console and $LASTEXITCODE is the honest answer about whether it succeeded. Read
        # through Get-Variable because a command that never launched leaves it unassigned.
        $touched = @()
        foreach ($probe in @(
                @{ Label = 'the working tree'; Arguments = @('diff', '--name-only') },
                @{ Label = 'the index'; Arguments = @('diff', '--name-only', '--cached') },
                @{ Label = 'the commit range origin/main..HEAD'; Arguments = @('log', '--format=', '--name-only', 'origin/main..HEAD') })) {
            $probeLines = & git @($probe.Arguments)
            $probeCode = Get-Variable -Name 'LASTEXITCODE' -ValueOnly -ErrorAction SilentlyContinue
            if ($probeCode -ne 0) {
                throw ("cannot verify that $UnrelatedTestFile is unrelated: 'git $($probe.Arguments -join ' ')' " +
                    "failed (exit $probeCode), so $($probe.Label) is UNREADABLE rather than empty. Refused.")
            }
            $touched += $probeLines
        }
        $needle = $UnrelatedTestFile.Replace('\', '/')
        # ORDINALIGNORECASE HERE FAILS CLOSED, and that is the reason -- not the Windows one. These
        # paths come from `git diff --name-only`, and git's index is case-sensitive: `Foo.rs` and
        # `foo.rs` can both exist. So the usual argument for ignoring case does not apply.
        #
        # What decides it is the DIRECTION of the mistake in this block, which throws when it finds
        # a match. Over-matching refuses a run that might have been fine; under-matching certifies a
        # touched file as untouched, and this check exists precisely to stop a failure in a file the
        # diff touches being called unrelated. Ordinal would take the second kind of mistake.
        $hit = $touched | Where-Object { $_ -and (Test-SamePath ($_.Replace('\', '/')) $needle) }
        if ($hit) {
            throw "$UnrelatedTestFile IS touched by this diff (found in the range or the working tree), so a failure in it is not unrelated. Refused."
        }
    } finally {
        Pop-Location
        $ErrorActionPreference = $previousPreference
    }

    $relatedToDiff = $false
    # The GRADE travels with the value, or it is lost in the sum. Written flat, these three read as
    # siblings and a later reader cannot tell that only ONE of them passed through an instrument.
    # The decisive condition -- fail-then-pass in the same run -- is the one this script CANNOT
    # check, so the record says so in the value itself rather than only in a comment nobody reads
    # beside the JSON.
    $run | Add-Member -NotePropertyName unrelatedTestFile -NotePropertyValue $UnrelatedTestFile -Force
    $run | Add-Member -NotePropertyName unrelatedTestFileVerifiedAgainstDiff -NotePropertyValue $true -Force
    $run | Add-Member -NotePropertyName unrelatedIssue -NotePropertyValue $UnrelatedIssue -Force
    $run | Add-Member -NotePropertyName failThenPassObserved -NotePropertyValue 'asserted-by-holder' -Force
}

# The class this replaced, so the ledger is self-describing: a census can see that the run was
# green AND that a person refined it, without having to find the commit that says so.
if (Test-SameText $existingClass 'green') {
    $run | Add-Member -NotePropertyName runClassRefinedFrom -NotePropertyValue $existingClass -Force
}
$run | Add-Member -NotePropertyName runClass -NotePropertyValue $Class -Force
$run | Add-Member -NotePropertyName runClassOrigin -NotePropertyValue 'human' -Force
$run | Add-Member -NotePropertyName relatedToDiff -NotePropertyValue $relatedToDiff -Force
$run | Add-Member -NotePropertyName runClassBecause -NotePropertyValue $Because -Force
$run | Add-Member -NotePropertyName runClassAtUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o')) -Force

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$out = $run | ConvertTo-Json -Depth 8

$written = New-Object System.Collections.Generic.List[string]

# ORDER IS THE MITIGATION, and it is chosen rather than defaulted. Two copies cannot be written
# atomically, so one of them is written first and the question is WHICH DIVERGENCE IS SURVIVABLE if
# the second write dies. This file's own doc says a class landing on one of two copies is worse than
# no class, because the two then disagree -- so:
#   DURABLE first, COMMITTABLE second.
# A crash between them leaves the committable copy UNCLASSIFIED, which reads as "nobody judged yet"
# and is true. The reverse order leaves a committed, classified manifest whose durable twin says
# otherwise -- a disagreement someone would have to arbitrate with no way to tell which is right.
#
# Both writes are wrapped and both REPORT: the earlier version left the first write bare under
# 'Stop', so a failure on the second aborted the script with the first already changed and nothing
# printed -- the operator saw an exception and never learned that one copy had moved. (Found in
# review by C Agent, who also noted it made this PR's security-review sentence about "both write
# paths are wrapped" false. It is true now.)
$targets = New-Object System.Collections.Generic.List[string]
$durable = [System.IO.Path]::Combine([System.IO.Path]::Combine($slotDir, 'gate-runs'), $fileName)
if ((Test-Path -LiteralPath $durable) -and
    -not (Test-SamePath (Resolve-Path -LiteralPath $durable).Path (Resolve-Path -LiteralPath $Manifest).Path)) {
    $targets.Add($durable)
}
$targets.Add($Manifest)

foreach ($target in $targets) {
    try {
        [System.IO.File]::WriteAllText($target, $out, $utf8NoBom)
        $written.Add($target)
    } catch {
        Write-Host "ERROR: could not write $target : $($_.Exception.Message)" -ForegroundColor Red
        if ($written.Count -gt 0) {
            Write-Host "  ALREADY WRITTEN, so the copies now DISAGREE: $($written -join ', ')" -ForegroundColor Red
            Write-Host "  Reconcile before citing either." -ForegroundColor Red
        }
        throw
    }
}

try {
    # .NET APIs, not New-Item/Join-Path: both fail NON-TERMINATINGLY, printing a raw error and
    # then handing the catch a misleading one. Found by gate.ps1's negative control, applied here.
    [System.IO.Directory]::CreateDirectory($slotDir) | Out-Null
    $line = '{0} | gate | RUN-CLASSIFIED | class={1} manifest={2} because={3}' -f [DateTime]::UtcNow.ToString('o'), $Class, $fileName, $Because
    [System.IO.File]::AppendAllText([System.IO.Path]::Combine($slotDir, 'SLOT.log'), $line + "`n", $utf8NoBom)
} catch {
    Write-Host "WARNING: could not append to SLOT.log: $($_.Exception.Message)" -ForegroundColor Yellow
}

# Read, never computed here. Three states, and the third is the point: absent means the run predates
# the measurement, which is a different fact from "the instrument was fine".
if (Test-NameIsPresent -Object $run -Name 'instrumentSuspect') {
    if ($run.instrumentSuspect) {
        # #904: NAME THE POPULATION THAT ACTUALLY DRIVES THE FLAG. `staleArtifactCount` no longer
        # decides it -- it is the record of what a cold-only gate would have refused -- and a note
        # explaining a flag with a number that cannot set it sends the reader to the wrong subsystem.
        # ABSENT IS NOT ZERO: a manifest written before #904 has no such field, and says so.
        $unproven = if (Test-NameIsPresent -Object $run -Name 'artifactsUnprovenReuse') { $run.artifactsUnprovenReuse } else { 'NOT MEASURED' }
        Write-Host "  NOTE: instrumentSuspect = TRUE (artifactsUnprovenReuse=$unproven, staleArtifactCount=$($run.staleArtifactCount), canaryPassed=$($run.canaryPassed))." -ForegroundColor Yellow
        Write-Host "  A '$Class' verdict can be true AT THE SAME TIME as a broken instrument -- that is why this is a field and not a class." -ForegroundColor Yellow
        Write-Host "  Say in -Because whether the code failure stands on its own." -ForegroundColor Yellow
    } else {
        Write-Host "  instrumentSuspect = false (measured by the gate)."
    }
} else {
    Write-Host "  instrumentSuspect: NOT MEASURED (this manifest predates the field). Absent is not false." -ForegroundColor Yellow
}

Write-Host "classified as $Class"
foreach ($path in $written) { Write-Host "  updated: $path" }
