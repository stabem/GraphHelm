# #205: isolated tests for ci/event-contract-sweep.ps1.
#
# Every instrument the project has for the event contract is intra-branch: the parity checkers
# compare `event.rs` against the envelope schema WITHIN one checkout, and the gate runs one branch.
# Two open branches can declare the same wire tag with different payloads; git merges the regions
# cleanly and the second to land forks every journal the first already wrote. This suite exercises
# the sweep that looks at every open branch at once, with INJECTED declarations and an injected
# ancestry oracle, so no cell builds a repository. Three cells read the real remote (the three `Get-ContractPopulation -Root $root -Remote` sites, ~185 s each; no line numbers here on purpose -- a citation in a file you are editing is stale when written), and
# they are marked, because a guard meant never to fire can only be observed on the day it does.
#
# WHAT THE ASSERTIONS COVER (a topic list, not a tally: several lines below are loop bodies, so summing the numbers gives ~75 while the suite runs 102 -- a hand-sum produced a wrong $ExpectedAssertionCount once on #1005; the count is derived from a run and pinned at :84):
#   1  canonical form: key order is not a payload difference
#   1  applicable root validation constraints remain part of each tag contract
#   1  canonical form: a change inside a $ref'd definition IS one
#   1  declared kinds are read off the oneOf variants, keyed by the kind's const, payload included
#   1  a schema with no variants reads as UNKNOWN (null), never as zero kinds
#   2  a nested external reference resolves against its own document's directory, not the envelope's
#   1  a relative reference inside an array carries its external document directory
#   1  a reference cycle between external documents is UNKNOWN and prompt, never a hang
#   1  the base envelope is readable by this sweep (a tree property; reddens if the nested-$ref fix is reverted)
#   2  REAL REMOTE floors by plain git: HeadCount matches ls-remote (race tolerance 2); Touched >= heads whose envelope differs
#   1  two lineages, divergent payloads: COLLISION
#   1  two lineages, identical payloads: RE-DECLARATION
#   1  a descendant collapses into its ancestor: one lineage, SINGLE
#   1  the SAME commit under two names is one lineage, not zero (mutual ancestry keeps one)
#   1  three lineages, two identical and one divergent: COLLISION naming all three
#   1  the collapse consults the injected oracle, not name order (descendant listed first)
#   1  zero declarations: an empty list of verdicts, not null
#   1  the head is party to a collision: refused
#   1  two OTHER branches collide: the verdict is listed but this head is not refused
#   1  a re-declaration is never a refusal
#   2  REAL REMOTE: the reading resolves or names its UNKNOWN; this head is party to no collision
#      (or, with the remote mute, a NOTE naming the reason -- never a red for the tree)
#   1  the manifest record rule is a function in gate.ps1 (lifted by AST, never retyped)
#   1  a MEASURED marker in the stage capture records 'measured'
#   1  a NOT MEASURED marker records 'notMeasured' with the reason verbatim
#   2  THE WIRE: the real join + the real stage block, sliced from gate.ps1, with a child printing the
#      marker: the capture reads 'measured', and the join's exit code still reaches the stage
#   1  the marker THIS run printed is read back as the state this run had (no string drift)
#   3  CHECKED-IN ENVELOPE: arrangement; a payload-definition change moves the tag's canonical value;
#      two branches with different payloads under one tag are a COLLISION that refuses HEAD
#   1  required/enum are sets: the same keys in another order are the same payload
#   1  case-only payload differences are two variants (ordinal), COLLISION
#   1  a zero-weight code point (U+FE00) is a different contract under Test-SameContract, equal under -eq
#   1  a $ref with a sibling keyword still expands its target
#   1  description/title are annotations, not contract
#   1  a reference cycle past the depth bound is UNKNOWN, never a shared value
#   1  HEAD is never collapsed as the ancestor of another head (working tree is its own lineage)
#   1  a spent whole-sweep budget is UNKNOWN naming the budget
#   1  two remote heads differing only by case are two heads (ordinal dictionary)
#   1  two tags differing only by case are two declared kinds (ordinal dictionaries)
#   1  oneOf/anyOf/allOf are sets; prefixItems is not
#   1  a git blob with non-ASCII text reads back as UTF-8 (real temp commit)
#   1  ancestry is precomputed and memoised inside the budget: no git after the population returns
#   1  a payload FIELD named description keeps its schema (annotation filtering is for keywords, not map members)
#   1  a $ref with JSON Pointer escapes or a deeper path resolves to its target
#   1  type arrays are sets
#   1  object keys sort ordinally: case-distinct keys in opposite orders are the same value, or UNKNOWN both ways (PS 5.1 refuses them)
#   1  a CLEAN HEAD that is an ancestor collapses like any ancestor (SINGLE, no refusal)
#   1  a 1 s whole-sweep budget bounds the first git call too (hang fixture, 20 s per-call timeout)
#   1  dependentRequired is a name map (a field named description keeps its companions)
#   1  refusal membership is ordinal (a remote branch named 'head' is not this gate)
#   1  HEAD declares only what it touched (three-dot vs base) or what the working tree changed
#   1  const values are literal JSON (members named like annotations are data)
#   1  a sole $ref canonicalises as its target (inline == factored)
#   1  Read-RefText re-asks the remaining time before each of its two git calls
#   1  a $ref to a $anchor fragment expands
#   1  a credential-bearing remote is redacted in every reason
#   1  ancestry names are ordinal: Foo/foo in one lineage collapse to SINGLE
#   1  an exponentially expanding (acyclic) schema is UNKNOWN under a total work budget
#   1  a $dynamicRef to a $dynamicAnchor expands
#   1  dependentRequired companion arrays are sets
#   1  the ancestry memo key is injective ((a,b>c) vs (a>b,c))
#   1  a remote that accepts and never answers is cut by the sweep's own bound (TCP listener fixture)
#   1  and the timed-out fetch leaves NO process behind (the bound kills the tree, not one pid)
#   1  an oversized working-tree schema is UNKNOWN before it is read
#   1  a repository root with a space reaches git as one argument
#   1  external checked-in refs contribute their target content
#   1  unreachable local definitions do not change the contract
#   1  a non-predicate ancestry git error reads UNKNOWN
#   1  anchor lookup ignores literal JSON data
#   1  anchor lookup spends the shared canonicalisation budget
#   1  credentials in remote query/path text are redacted
#   1  external reference fragments select the referenced schema node
#   1  common credential query/path names, including percent-encoded forms, are redacted
#   1  the independent floor observer remains bounded after the Known reading
#   4  an unavailable independent listing is named (timeout, bound, exit code), a successful one -- even empty -- is not (#1247)
$ExpectedAssertionCount = 121

$ErrorActionPreference = 'Stop'
$script:total = 0
$script:skipped = 0
$script:skipReasons = @()
# A SUM CANNOT SEE A REDISTRIBUTION BETWEEN ITS TERMS, and neither can a COUNT of skips:
# turning a real `Assert-True` into a `Skip-Assertion` leaves `total + skipped` intact and
# produces a skip count identical to a legitimate one (measured on this suite by a
# reviewing lane: 23 of 24, 1 skipped, rc=0, GREEN). What separates them is the REASON, so
# every legitimate skip site is declared here and the tail refuses anything else.
#
# WHAT THIS DOES NOT DO, so it is not read as stronger than it is: it does not catch a skip
# whose message was COPIED from a legitimate site, and it does not notice a legitimate site
# firing twice. The threat it is sized for is accidental degradation, which does not inherit
# a legal reason string; it is not a defence against someone deliberately forging one.
$AllowedSkipReasons = @(
    'NOT MEASURED: the collision cell is skipped with its reason named (*'
    'NOT MEASURED: the HeadCount floor is skipped with the same reason; this is a note, not a pass'
    'NOT MEASURED: the Touched floor is skipped with the same reason; this is a note, not a pass'
    # #1247: the population is Known but the independent listing the floors need did not answer.
    'NOT MEASURED: the HeadCount floor needs an independent ls-remote listing, unavailable after 3 attempts (*); this is a note, not a pass'
    'NOT MEASURED: the Touched floor is counted over the same listing (*); this is a note, not a pass'
)
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
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected '$Expected', got '$Actual')"
}

function Skip-Assertion {
    param([Parameter(Mandatory)] [string] $Message)
    $script:skipped++
    $script:skipReasons += $Message
    Write-Host "  SKIP: $Message" -ForegroundColor Yellow
}

# HARNESS SELF-CHECK, armed on every host: a SKIP must not move the pass counter. These suites
# were rewritten so a skipped check stops counting as a pass, and the tail guard is
# `total + skipped == expected` -- a SUM, which any redistribution between its two terms
# satisfies. This exercises the skip path directly and leaves no residue, so it fires wherever
# the suite runs rather than only on a host that happens to take the skip branch.
$__selfTotal = $script:total; $__selfSkipped = $script:skipped
Skip-Assertion 'harness self-check: a skip must not be counted as a pass'
if ($script:total -ne $__selfTotal) {
    Write-Host "HARNESS-BROKE: Skip-Assertion advanced the pass counter, so a skipped check is being reported as a pass" -ForegroundColor Red
    exit 2
}
$script:total = $__selfTotal; $script:skipped = $__selfSkipped
$script:skipReasons = @($script:skipReasons | Select-Object -First $__selfSkipped)


. (Join-Path $PSScriptRoot 'event-contract-sweep.ps1')

<#
.SYNOPSIS
    Why an independent `git ls-remote` probe gave no usable listing, or $null when it did.
.DESCRIPTION
    #1247: the HeadCount floor re-lists the remote as a second instrument. When that listing is
    unavailable the floors are UNMEASURED, which this file records as a skip with its reason --
    never a red for the tree (the policy above `Get-ContractPopulation` below), and never a
    `.Count` on `$null` under StrictMode, which crashed the suite on #1233's gate and hid the
    reason. A probe that ANSWERED is available even with zero lines: an empty listing beside a
    sweep that measured heads is a disagreement the floor must report, not a skip.
#>
function Get-ListingUnavailableReason {
    param([AllowNull()] $Probe)
    if ($null -eq $Probe) { return 'the probe returned nothing' }
    if ($Probe.TimedOut) { return 'timed out' }
    if (($Probe.PSObject.Properties.Name -contains 'OutputExceeded') -and [bool]$Probe.OutputExceeded) { return 'output bound exceeded' }
    if ($Probe.Code -ne 0) { return "git exited $($Probe.Code)" }
    return $null
}

# Four pure cells: the reason is named for each unavailable shape, and an answered listing is not
# unavailable even when it is empty (the case a skip must never swallow).
Assert-Equal 'timed out' (Get-ListingUnavailableReason ([pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $true })) `
    '#1247: a timed-out listing is unavailable, and the reason says so'
Assert-Equal 'output bound exceeded' (Get-ListingUnavailableReason ([pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $false; OutputExceeded = $true })) `
    '#1247: an over-bound listing is unavailable, and the reason says so'
Assert-Equal 'git exited 128' (Get-ListingUnavailableReason ([pscustomobject]@{ Code = 128; Lines = @(); TimedOut = $false })) `
    '#1247: a failed git exit is unavailable, and the reason carries the code'
Assert-True -Condition ($null -eq (Get-ListingUnavailableReason ([pscustomobject]@{ Code = 0; Lines = @(); TimedOut = $false }))) `
    '#1247: an ANSWERED empty listing is available, so the floor still reds on it instead of skipping'

function New-Declaration {
    param([string] $Branch, [string] $Tag, [string] $Canonical, [switch] $Remote)
    $workingTree = (-not $Remote -and [string]::Equals($Branch, 'HEAD', [System.StringComparison]::Ordinal))
    return [pscustomobject]@{ Branch = $Branch; Identity = if ($workingTree) { '@working-tree' } else { $Branch }; IsWorkingTree = $workingTree; Tag = $Tag; Canonical = $Canonical }
}
# The ancestry oracle is a SET OF EDGES, so a cell states exactly which lineages descend from which
# and nothing is inferred from names. Equal names are equal commits.
function New-Oracle {
    param([string[]] $Edges = @())
    $set = @($Edges)
    return { param($a, $b)
        $left = [string]$a
        $right = [string]$b
        $same = [string]::Equals($left, $right, [System.StringComparison]::Ordinal)
        $direct = "$left>$right"
        # Existing fixture edges use HEAD for the working tree. Keep that spelling as a
        # compatibility edge only; declaration identity/equality remains explicit, so a remote
        # branch literally named HEAD cannot alias @working-tree.
        $legacyLeft = if ([string]::Equals($left, '@working-tree', [System.StringComparison]::Ordinal)) { 'HEAD' } else { $left }
        $legacyRight = if ([string]::Equals($right, '@working-tree', [System.StringComparison]::Ordinal)) { 'HEAD' } else { $right }
        $legacy = "$legacyLeft>$legacyRight"
        $same -or (@($set | Where-Object {
            [string]::Equals([string] $_, $direct, [System.StringComparison]::Ordinal) -or
            [string]::Equals([string] $_, $legacy, [System.StringComparison]::Ordinal)
        }).Count -gt 0)
    }.GetNewClosure()
}

# The envelope's REAL shape, in miniature (C's block on #1005 caught the first version reading only
# the top-level variant): the top-level `oneOf` variant carries the tag and the scope, and the
# PAYLOAD is selected separately by `$defs.eventKind.oneOf`, whose `data` references the payload
# definition. A canonical value that stops at the top-level variant is a function of (tag, scope)
# and never sees the payload -- the exact class this sweep exists for.
$schemaA = '{"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}},"scope":{"$ref":"#/$defs/scope"}}}],"$defs":{"scope":{"required":["executionId"]},"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"$ref":"#/$defs/sweep"}},"additionalProperties":false}]},"sweep":{"required":["executionId","asOf"],"additionalProperties":false}}}'
$schemaAReordered = '{"$defs":{"sweep":{"additionalProperties":false,"required":["executionId","asOf"]},"eventKind":{"oneOf":[{"additionalProperties":false,"properties":{"data":{"$ref":"#/$defs/sweep"},"type":{"const":"sweep_performed"}},"required":["type","data"],"type":"object"}]},"scope":{"required":["executionId"]}},"oneOf":[{"properties":{"scope":{"$ref":"#/$defs/scope"},"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}]}'
$schemaADefChanged = '{"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}},"scope":{"$ref":"#/$defs/scope"}}}],"$defs":{"scope":{"required":["executionId"]},"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"$ref":"#/$defs/sweep"}},"additionalProperties":false}]},"sweep":{"required":["executionId","asOf","caller"],"additionalProperties":false}}}'

Write-Host ''
Write-Host '-- what a payload difference is --' -ForegroundColor Cyan
$kindsA = Get-DeclaredKinds -SchemaText $schemaA
$kindsReordered = Get-DeclaredKinds -SchemaText $schemaAReordered
$kindsDefChanged = Get-DeclaredKinds -SchemaText $schemaADefChanged
Assert-True -Condition ($null -ne $kindsA -and $null -ne $kindsReordered -and $kindsA['sweep_performed'] -eq $kindsReordered['sweep_performed']) `
    'key order and formatting are not a payload difference'
Assert-True -Condition ($null -ne $kindsA -and $null -ne $kindsDefChanged -and $kindsA['sweep_performed'] -ne $kindsDefChanged['sweep_performed']) `
    'a change inside a $ref-ed definition IS one: the variant is compared with its references expanded'
Assert-True -Condition ($null -ne $kindsA -and @($kindsA.Keys).Count -eq 1 -and $kindsA.ContainsKey('sweep_performed')) `
    'declared kinds are read off the oneOf variants, keyed by the kind const'
Assert-True -Condition ($null -eq (Get-DeclaredKinds -SchemaText '{"type":"object"}')) `
    'a schema with no variants reads as UNKNOWN, never as zero kinds'

# Root validation applies to every envelope instance, even when a tag variant only constrains its
# discriminator. Dropping that context makes two tags with different payload validity look equal.
$rootConstraintTemplate = '{"type":"object","properties":{"kind":{"type":"object","properties":{"data":{"type":"string","maxLength":MAX_LENGTH}}}},"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}],"$defs":{"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"type":"string"}}}]}}}'
$rootConstraint10 = $rootConstraintTemplate.Replace('MAX_LENGTH','10')
$rootConstraint20 = $rootConstraintTemplate.Replace('MAX_LENGTH','20')
$rootKinds10 = Get-DeclaredKinds -SchemaText $rootConstraint10
$rootKinds20 = Get-DeclaredKinds -SchemaText $rootConstraint20
$rootConstraintVerdict = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'root-10' -Tag 'sweep_performed' -Canonical $rootKinds10['sweep_performed']),
    (New-Declaration -Branch 'root-20' -Tag 'sweep_performed' -Canonical $rootKinds20['sweep_performed'])
) -IsAncestor (New-Oracle)
Assert-True -Condition ($null -ne $rootKinds10 -and $null -ne $rootKinds20 -and $rootKinds10['sweep_performed'] -cne $rootKinds20['sweep_performed'] -and $rootConstraintVerdict[0].Category -ceq 'COLLISION') `
    'applicable root validation constraints remain in the tag contract, so maxLength 10 and 20 are a COLLISION'

# Root composition and reference keywords are validation context too. The old root whitelist
# silently dropped these, making incompatible envelopes look identical to the sweep.
$rootKeywordTemplate = '{"type":"object","properties":{"kind":{"$ref":"#/$defs/eventKind"}},ROOT_CONSTRAINT,"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}],"$defs":{"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"type":"string"}}}]},"rootConstraint":{"minProperties":1}}}'
$rootKeywordPairs = @(
    @('allOf', '"allOf":[{"minProperties":1}]', '"allOf":[{"minProperties":2}]'),
    @('anyOf', '"anyOf":[{"minProperties":1}]', '"anyOf":[{"minProperties":2}]'),
    @('not', '"not":{"required":["forbidden"]}', '"not":{"required":["different"]}'),
    @('if', '"if":{"properties":{"mode":{"const":"a"}}}', '"if":{"properties":{"mode":{"const":"b"}}}'),
    @('then', '"then":{"required":["mode"]}', '"then":{"required":["other"]}'),
    @('else', '"else":{"required":["mode"]}', '"else":{"required":["other"]}'),
    @('$ref', '"$ref":"#/$defs/rootConstraint"', '"$ref":"#/$defs/rootConstraintChanged"')
)
$rootKeywordDifferences = @()
foreach ($pair in $rootKeywordPairs) {
    $leftText = $rootKeywordTemplate.Replace('ROOT_CONSTRAINT', $pair[1])
    $rightText = $rootKeywordTemplate.Replace('ROOT_CONSTRAINT', $pair[2])
    if ($pair[0] -ceq '$ref') {
        $rightText = $rightText.Replace('"rootConstraint":{"minProperties":1}', '"rootConstraint":{"minProperties":1},"rootConstraintChanged":{"minProperties":2}')
    }
    $leftKinds = Get-DeclaredKinds -SchemaText $leftText
    $rightKinds = Get-DeclaredKinds -SchemaText $rightText
    $rootKeywordDifferences += ($null -ne $leftKinds -and $null -ne $rightKinds -and $leftKinds['sweep_performed'] -cne $rightKinds['sweep_performed'])
}
Assert-True -Condition (@($rootKeywordDifferences | Where-Object { -not $_ }).Count -eq 0) `
    'root allOf/anyOf/not/if/then/else/$ref constraints remain in each tag contract'

# A sole ref to a narrower kind schema is not the canonical event-kind union. Replacing every
# sole ref with the matching eventKind entry erased this extra constraint and hid collisions.
$narrowKindTemplate = '{"type":"object","properties":{"kind":{"$ref":"#/$defs/narrowKind"}},"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}],"$defs":{"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"type":"string"}}}]},"narrowKind":{"type":"object","properties":{"data":{"type":"string","maxLength":LIMIT}}}}}'
$narrowKind10 = Get-DeclaredKinds -SchemaText $narrowKindTemplate.Replace('LIMIT','10')
$narrowKind20 = Get-DeclaredKinds -SchemaText $narrowKindTemplate.Replace('LIMIT','20')
Assert-True -Condition ($null -ne $narrowKind10 -and $null -ne $narrowKind20 -and $narrowKind10['sweep_performed'] -cne $narrowKind20['sweep_performed'] -and (ConvertFrom-Json $narrowKind10['sweep_performed']).root.properties.kind.allOf.Count -eq 2) `
    'a sole ref to a narrower kind definition stays a conjunction with eventKind, preserving its constraint'

# Constraints on the eventKind container apply to every selected oneOf entry. Dropping the
# container and retaining only the chosen entry made maxLength 10 and 20 appear identical.
$eventKindContainerTemplate = '{"type":"object","properties":{"kind":{"$ref":"#/$defs/eventKind"}},"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}],"$defs":{"eventKind":{"properties":{"data":{"type":"string","maxLength":LIMIT}},"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"type":"string"}}}]}}}'
$eventKindContainer10 = Get-DeclaredKinds -SchemaText $eventKindContainerTemplate.Replace('LIMIT','10')
$eventKindContainer20 = Get-DeclaredKinds -SchemaText $eventKindContainerTemplate.Replace('LIMIT','20')
$eventKindContainerControl = Get-DeclaredKinds -SchemaText $eventKindContainerTemplate.Replace('LIMIT','10')
Assert-True -Condition ($null -ne $eventKindContainer10 -and $null -ne $eventKindContainer20 -and $eventKindContainer10['sweep_performed'] -cne $eventKindContainer20['sweep_performed']) `
    'eventKind container constraints remain in the selected tag contract, so maxLength 10 and 20 differ'
Assert-True -Condition ($null -ne $eventKindContainer10 -and $null -ne $eventKindContainerControl -and $eventKindContainer10['sweep_performed'] -ceq $eventKindContainerControl['sweep_performed']) `
    'identical eventKind container constraints retain identical canonical contracts'
$eventKindAnnotationOnlyTemplate = '{"type":"object","properties":{"kind":{"$ref":"#/$defs/eventKind"}},"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}}}}],"$defs":{"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"type":"string"}}}]}}}'
$eventKindAnnotationOnlyText = $eventKindAnnotationOnlyTemplate.Replace('"oneOf"', '"description":"annotation only","oneOf"')
$eventKindAnnotationOnly = Get-DeclaredKinds -SchemaText $eventKindAnnotationOnlyTemplate
$eventKindAnnotated = Get-DeclaredKinds -SchemaText $eventKindAnnotationOnlyText
$eventKindAnnotationVerdict = @(Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'without-description' -Tag 'sweep_performed' -Canonical $eventKindAnnotationOnly['sweep_performed']),
    (New-Declaration -Branch 'with-description' -Tag 'sweep_performed' -Canonical $eventKindAnnotated['sweep_performed'])
) -IsAncestor (New-Oracle))
Assert-True -Condition ($null -ne $eventKindAnnotationOnly -and $null -ne $eventKindAnnotated -and $eventKindAnnotationOnly['sweep_performed'] -ceq $eventKindAnnotated['sweep_performed']) `
    'an annotation-only eventKind container sibling does not change the canonical contract'
Assert-True -Condition ($eventKindAnnotationVerdict.Count -eq 1 -and $eventKindAnnotationVerdict[0].Category -ceq 'RE-DECLARATION') `
    'absent versus present eventKind description is a RE-DECLARATION, not a COLLISION'

Write-Host ''
Write-Host '-- the CHECKED-IN envelope: the payload is what the canonical value sees --' -ForegroundColor Cyan
# Not a hand-built miniature: the real schema, loaded from this checkout, and one payload definition
# tightened by one required key. If the canonical value stops at the top-level variant, the two
# texts collapse to the same value and two branches with different payloads read as agreement.
$envelopePath = Join-Path (Join-Path $PSScriptRoot '..') 'schemas/event-envelope.schema.json'
$envelopeText = [IO.File]::ReadAllText($envelopePath)
$envelope = ConvertFrom-Json -InputObject $envelopeText
$firstTag = [string] $envelope.oneOf[0].properties.kind.properties.type.const
$firstKind = @($envelope.'$defs'.eventKind.oneOf | Where-Object { $_.properties.type.const -eq $firstTag })[0]
$payloadName = ([string] $firstKind.properties.data.'$ref').Substring(8)
$payloadRequired = @($envelope.'$defs'.$payloadName.required)
$tightened = $envelopeText -replace ('"' + $payloadName + '":\s*\{'), ('"' + $payloadName + '":{"x-tightened-by-the-suite":true,')
Assert-True -Condition ($tightened -ne $envelopeText -and $payloadRequired.Count -gt 0) `
    "ARRANGEMENT: the checked-in envelope declares '$firstTag' with payload '$payloadName' (required: $($payloadRequired -join ', ')) and the tightened copy differs from it"
# The complete envelope includes imported resources. Without their content it must be UNKNOWN.
Assert-True ($null -eq (Get-DeclaredKinds -SchemaText $envelopeText)) 'the full checked-in envelope without external resource content is UNKNOWN'
# Keep this observer at its promised boundary: the actual first variant and its complete local
# definition closure, not an assertion that unavailable imports were compared for all variants.
function Select-FirstLocalContract {
    param([string]$Text)
    $document = ConvertFrom-Json $Text
    $variant = $document.oneOf[0]
    $tag = $variant.properties.kind.properties.type.const
    $entry = @($document.'$defs'.eventKind.oneOf | Where-Object {$_.properties.type.const -ceq $tag})[0]
    $definitions = [ordered]@{eventKind=[pscustomobject]@{oneOf=@($entry)}}
    $pending = @(Get-ReferencedDefinitions -Node $variant) + @(Get-ReferencedDefinitions -Node $entry)
    while ($pending.Count -gt 0) {
        $name = $pending[0]; $pending = @($pending | Select-Object -Skip 1)
        if ($definitions.Contains($name)) { continue }
        $value = $document.'$defs'.PSObject.Properties[$name].Value
        $definitions[$name] = $value
        $pending += @(Get-ReferencedDefinitions -Node $value)
    }
    return ConvertTo-Json -InputObject ([pscustomobject]@{oneOf=@($variant); '$defs'=[pscustomobject]$definitions}) -Depth 100 -Compress
}
$realFixtureResolver = New-ExternalResolver -Root (Split-Path -Parent $PSScriptRoot) -SchemaPath 'schemas/event-envelope.schema.json' -SourceRef HEAD -RemainingSeconds {60} -WorkingTree
$realKinds = Get-DeclaredKinds -SchemaText (Select-FirstLocalContract $envelopeText) -ExternalResolver $realFixtureResolver
$tightenedKinds = Get-DeclaredKinds -SchemaText (Select-FirstLocalContract $tightened) -ExternalResolver $realFixtureResolver
Assert-True -Condition ($null -ne $realKinds -and $null -ne $tightenedKinds -and $realKinds[$firstTag] -ne $tightenedKinds[$firstTag] -and @($realKinds.Keys).Count -eq 1) `
    "the real schema first-kind closure reads one tag, and a change INSIDE '$firstTag''s payload definition changes its canonical value"
$realCollision = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'HEAD' -Tag $firstTag -Canonical $realKinds[$firstTag]),
    (New-Declaration -Branch 'k-162' -Tag $firstTag -Canonical $tightenedKinds[$firstTag])
) -IsAncestor (New-Oracle)
Assert-True -Condition ($realCollision.Count -eq 1 -and $realCollision[0].Category -eq 'COLLISION' -and (Get-ContractRefusals -Verdicts $realCollision -Head 'HEAD').Count -eq 1) `
    "two branches declaring '$firstTag' with the checked-in payload and the tightened one are a COLLISION, and this head is refused for it"

# An external relative reference is a checked-in contract dependency, not just an inert string.
# The resolver is injected here so this cell stays offline; the population supplies the real
# checked-in/remote resolver when it reads the envelope.
$schemaExternalRef = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"checked-target.schema.json"}')
$externalResolverA = { param([string] $Reference) if ($Reference -ceq 'checked-target.schema.json') { return '{"type":"string"}' } }.GetNewClosure()
$externalResolverB = { param([string] $Reference) if ($Reference -ceq 'checked-target.schema.json') { return '{"type":"number"}' } }.GetNewClosure()
$externalKindsA = try { Get-DeclaredKinds -SchemaText $schemaExternalRef -ExternalResolver $externalResolverA } catch { $null }
$externalKindsB = try { Get-DeclaredKinds -SchemaText $schemaExternalRef -ExternalResolver $externalResolverB } catch { $null }
Assert-True -Condition ($null -ne $externalKindsA -and $null -ne $externalKindsB -and $externalKindsA['sweep_performed'] -ne $externalKindsB['sweep_performed']) `
    'a relative external reference includes its checked-in target, so string-equal refs with different targets are different contracts'
$schemaExternalFragmentA = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"checked-target.schema.json#/$defs/a"}')
$schemaExternalFragmentB = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"checked-target.schema.json#/$defs/b"}')
$externalFragmentTarget = '{"$defs":{"a":{"type":"string"},"b":{"type":"number"}}}'
$externalFragmentResolver = { param([string] $Reference) return $externalFragmentTarget }.GetNewClosure()
$externalFragmentKindsA = try { Get-DeclaredKinds -SchemaText $schemaExternalFragmentA -ExternalResolver $externalFragmentResolver } catch { $null }
$externalFragmentKindsB = try { Get-DeclaredKinds -SchemaText $schemaExternalFragmentB -ExternalResolver $externalFragmentResolver } catch { $null }
Assert-True -Condition ($null -ne $externalFragmentKindsA -and $null -ne $externalFragmentKindsB -and $externalFragmentKindsA['sweep_performed'] -ne $externalFragmentKindsB['sweep_performed']) `
    'an external JSON Pointer fragment selects its target node, so different fragments in one checked-in file are different contracts'

# A relative ref INSIDE an external document has that document as its URI base. The old resolver
# reused the envelope directory, which could silently select a different checked-in file. Until the
# resolver carries the external document directory, this must be UNKNOWN rather than a false
# canonical equivalence. The injected target and nested sibling deliberately have different types.
$schemaNestedExternal = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"sub/checked-target.schema.json"}')
$nestedExternalResolver = {
    param([string] $Reference)
    if ($Reference -ceq 'sub/checked-target.schema.json') { return '{"$ref":"other.json"}' }
    if ($Reference -ceq 'other.json') { return '{"type":"number"}' }
    return $null
}.GetNewClosure()
$nestedExternalKinds = try { Get-DeclaredKinds -SchemaText $schemaNestedExternal -ExternalResolver $nestedExternalResolver } catch { $null }
Assert-True -Condition ($null -eq $nestedExternalKinds) `
    'a relative reference inside an external document is UNKNOWN until its document base is carried (never resolved against the envelope directory)'

    # The document base IS carried now (#1005): a nested reference resolves against the directory of the
    # document that contains it, and the loaded document is found by path. So the cell above refuses for
    # the RIGHT reason -- `sub/other.json` was never loaded -- and this cell proves the positive half: when
    # BOTH `sub/other.json` (number) and an envelope-level `other.json` (string decoy) are loaded, the
    # nested `other.json` must select the sibling, never the decoy. Lane A built the failing case on #1005.
    $schemaNestedSibling = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"sub/checked-target.schema.json"},"decoy":{"$ref":"other.json"},"sibling":{"$ref":"sub/other.json"}')
    $schemaNestedControl = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"sub/other.json"},"decoy":{"$ref":"other.json"},"sibling":{"$ref":"sub/other.json"}')
    $schemaNestedWrong   = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"other.json"},"decoy":{"$ref":"other.json"},"sibling":{"$ref":"sub/other.json"}')
    $nestedSiblingResolver = {
        param([string] $Reference)
        if ($Reference -ceq 'sub/checked-target.schema.json') { return '{"$ref":"other.json"}' }
        if ($Reference -ceq 'sub/other.json') { return '{"type":"number"}' }
        if ($Reference -ceq 'other.json') { return '{"type":"string"}' }
        return $null
    }.GetNewClosure()
    $nestedSiblingKinds = try { Get-DeclaredKinds -SchemaText $schemaNestedSibling -ExternalResolver $nestedSiblingResolver } catch { $null }
    $nestedControlKinds = try { Get-DeclaredKinds -SchemaText $schemaNestedControl -ExternalResolver $nestedSiblingResolver } catch { $null }
    $nestedWrongKinds   = try { Get-DeclaredKinds -SchemaText $schemaNestedWrong   -ExternalResolver $nestedSiblingResolver } catch { $null }
    Assert-True -Condition ($null -ne $nestedSiblingKinds -and $null -ne $nestedControlKinds -and $nestedSiblingKinds['sweep_performed'] -ceq $nestedControlKinds['sweep_performed']) `
        'a relative reference inside an external document resolves against THAT document''s directory (sub/t -> other.json is sub/other.json)'
    # `$null -ne $nestedSiblingKinds` is repeated on purpose: when the cell above fails this one must
    # FAIL too, not throw "Cannot index into a null array" and take the summary with it (lane B).
    Assert-True -Condition ($null -ne $nestedSiblingKinds -and $null -ne $nestedWrongKinds -and $nestedSiblingKinds['sweep_performed'] -cne $nestedWrongKinds['sweep_performed']) `
        'and it is not the envelope-level file of the same name (the decoy would have made the two canonicals equal)'

    # Arrays are schema nodes too. The document base must survive the array descent before an
    # item-level external reference is resolved; otherwise `other.json` is silently read from the
    # envelope directory and the string decoy wins over the numeric sibling.
    $schemaArrayNested = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"sub/checked-target.schema.json"},"decoy":{"$ref":"other.json"},"sibling":{"$ref":"sub/other.json"}')
    $arrayNestedResolver = {
        param([string] $Reference)
        if ($Reference -ceq 'sub/checked-target.schema.json') { return '[{"$ref":"other.json"}]' }
        if ($Reference -ceq 'sub/other.json') { return '{"type":"number"}' }
        if ($Reference -ceq 'other.json') { return '{"type":"string"}' }
        return $null
    }.GetNewClosure()
    $arrayNestedKinds = try { Get-DeclaredKinds -SchemaText $schemaArrayNested -ExternalResolver $arrayNestedResolver } catch { $null }
    $arrayCanonical = if ($null -ne $arrayNestedKinds) { ConvertFrom-Json $arrayNestedKinds['sweep_performed'] } else { $null }
    $arrayPayload = if ($null -ne $arrayCanonical) { $arrayCanonical.kind.properties.data.'$ref' } else { $null }
    Assert-True -Condition ($null -ne $arrayPayload -and @($arrayPayload).Count -eq 1 -and $arrayPayload[0].type -ceq 'number') `
        'a relative reference inside an external array item resolves against that document''s directory (the sibling wins over the envelope decoy)'

    # A cycle between external documents throws under the Depth/Nodes caps rather than hanging. Argued from
    # the code on #1005 and then built by lane A (A->B->A in 84 ms); this cell keeps it built.
    $schemaCycle = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"cyc-a.json"},"other":{"$ref":"cyc-b.json"}')
    $cycleResolver = {
        param([string] $Reference)
        if ($Reference -ceq 'cyc-a.json') { return '{"$ref":"cyc-b.json"}' }
        if ($Reference -ceq 'cyc-b.json') { return '{"$ref":"cyc-a.json"}' }
        return $null
    }.GetNewClosure()
    $cycleWatch = [Diagnostics.Stopwatch]::StartNew()
    $cycleKinds = try { Get-DeclaredKinds -SchemaText $schemaCycle -ExternalResolver $cycleResolver } catch { $null }
    $cycleWatch.Stop()
    Assert-True -Condition ($null -eq $cycleKinds -and $cycleWatch.Elapsed.TotalSeconds -lt 10) `
        "a reference cycle between external documents is UNKNOWN and returns promptly, not a hang (took $([int]$cycleWatch.ElapsedMilliseconds) ms)"

Write-Host ''
Write-Host '-- what the canonical value must and must not distinguish (C, third pass) --' -ForegroundColor Cyan
# `required` and `enum` are SETS: two branches that list the same keys in a different order accept
# exactly the same instances and must not be a COLLISION.
$schemaARequiredSwapped = $schemaA.Replace('"required":["executionId","asOf"]', '"required":["asOf","executionId"]')
$swappedKinds = Get-DeclaredKinds -SchemaText $schemaARequiredSwapped
Assert-True -Condition ($schemaARequiredSwapped -ne $schemaA -and $null -ne $swappedKinds -and $swappedKinds['sweep_performed'] -eq $kindsA['sweep_performed']) `
    'the same required keys in a different order are the same payload (required and enum are sets)'
# An unused helper under a reached definition is not part of the accepted contract.
$schemaUnusedA = $schemaA.Replace('"sweep":{"required', '"sweep":{"$defs":{"unused":{"const":"A"}},"required')
$schemaUnusedB = $schemaA.Replace('"sweep":{"required', '"sweep":{"$defs":{"unused":{"const":"B"}},"required')
$unusedKindsA = Get-DeclaredKinds -SchemaText $schemaUnusedA
$unusedKindsB = Get-DeclaredKinds -SchemaText $schemaUnusedB
Assert-True -Condition ($null -ne $unusedKindsA -and $null -ne $unusedKindsB -and $unusedKindsA['sweep_performed'] -eq $unusedKindsB['sweep_performed']) `
    'an unreachable local $defs member does not change the payload contract'
# JSON Schema is case-sensitive: a `const` that differs only by case is a different contract, and a
# culture-aware, case-insensitive distinct count would fold the two into one.
$caseOnly = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'issue-160' -Tag 'sweep_performed' -Canonical '{"const":"Done"}'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical '{"const":"done"}')
) -IsAncestor (New-Oracle)
Assert-True -Condition ($caseOnly.Count -eq 1 -and $caseOnly[0].Category -eq 'COLLISION' -and $caseOnly[0].Variants -eq 2) `
    'two payloads differing only by case are two variants, compared ordinally: COLLISION, not RE-DECLARATION'

# The comparison that decides whether a head DECLARES a tag (its canonical value differs from the
# base's) must be ordinal: `-eq`, `-ceq` and their kin are culture-aware and give ZERO weight to
# U+00AD, U+200D, U+2060, U+FE00, U+FEFF, U+FFFD (#753, measured in #830), so a payload that
# differs from main's by one such code point would read as "unchanged" and never be swept.
$sameUnderCulture = ('{"const":"done"}' -eq ('{"const":"done' + [char]0xFE00 + '"}'))
$sameContract = if (Get-Command Test-SameContract -ErrorAction SilentlyContinue) { Test-SameContract '{"const":"done"}' ('{"const":"done' + [char]0xFE00 + '"}') } else { 'no ordinal comparison exists' }
Assert-True -Condition ($sameUnderCulture -eq $true -and $sameContract -eq $false) `
    'two canonical values differing by a zero-weight code point (U+FE00) are DIFFERENT contracts under the ordinal rule, where -eq calls them equal'

Write-Host ''
Write-Host '-- five more edges of the canonical value and the population (C, fourth pass) --' -ForegroundColor Cyan
# (a) `$ref` with sibling keywords is valid draft-2020-12 and must still expand the target.
$schemaRefSibling = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"#/$defs/sweep","description":"the sweep payload"}')
$schemaRefSiblingChanged = $schemaRefSibling.Replace('"required":["executionId","asOf"],"additionalProperties":false', '"required":["executionId","asOf","caller"],"additionalProperties":false')
$k1 = Get-DeclaredKinds -SchemaText $schemaRefSibling
$k2 = Get-DeclaredKinds -SchemaText $schemaRefSiblingChanged
Assert-True -Condition ($schemaRefSibling -ne $schemaA -and $schemaRefSiblingChanged -ne $schemaRefSibling -and $null -ne $k1 -and $null -ne $k2 -and $k1['sweep_performed'] -ne $k2['sweep_performed']) `
    'a $ref carrying a sibling keyword still expands its target, so a change behind it is a payload change'
# (b) non-validating annotations are not contract: two schemas differing only in `description`
#     accept exactly the same journals and must canonicalise identically.
$schemaAnnotated = $schemaA.Replace('"sweep":{"required"', '"sweep":{"description":"one sweep","title":"Sweep","required"')
$kAnn = Get-DeclaredKinds -SchemaText $schemaAnnotated
Assert-True -Condition ($schemaAnnotated -ne $schemaA -and $null -ne $kAnn -and $kAnn['sweep_performed'] -eq $kindsA['sweep_performed']) `
    'description/title on a payload do not change its canonical value (annotations are not constraints)'
# (c) a reference cycle past the canonicalisation depth is UNKNOWN, never a synthesized equal value.
$schemaCycle = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"required":["executionId"],"properties":{"again":{"$ref":"#/$defs/sweep"}}}')
$kCycle = Get-DeclaredKinds -SchemaText $schemaCycle
Assert-True -Condition ($schemaCycle -ne $schemaA -and $null -eq $kCycle) `
    'a payload that recurses past the depth bound reads as UNKNOWN (null), not as a value two different schemas could share'
# (d) the working tree is a lineage of its own: HEAD is never collapsed as the ANCESTOR of another
#     head, because its declaration may be uncommitted and was never part of that descendant.
$headAncestor = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'HEAD' -Tag 'sweep_performed' -Canonical 'WORKING-TREE'),
    (New-Declaration -Branch 'feat/descendant' -Tag 'sweep_performed' -Canonical 'B')
) -IsAncestor (New-Oracle -Edges @('HEAD>feat/descendant'))
Assert-True -Condition ($headAncestor.Count -eq 1 -and $headAncestor[0].Category -eq 'COLLISION' -and (Get-ContractRefusals -Verdicts $headAncestor -Head 'HEAD').Count -eq 1) `
    'HEAD whose commit is an ancestor of another head keeps its own (working-tree) declaration: a different payload there is a COLLISION refusing HEAD, not SINGLE'
# (e) one wall-clock budget over the whole population: per-call timeouts do not compose a bound.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exhausted = try { Get-ContractPopulation -Root $root -BudgetSeconds 0 } catch { [pscustomobject]@{ Known = $false; Reason = "no whole-sweep budget parameter exists ($($_.Exception.Message))" } }
Assert-True -Condition ($null -ne $exhausted -and -not $exhausted.Known -and $exhausted.Reason -like '*whole-sweep budget of 0s*') `
    "a whole-sweep budget that is already spent reads as UNKNOWN naming the budget (got [$($exhausted.Reason)])"

# Names are CASE-SENSITIVE in git and in JSON Schema; a PowerShell `@{}` is not, so two heads
# `Foo` / `foo` (git accepts both) or two tags differing by case would fold into one and the second
# would never be inspected (Codex P2 on #1005).
$caseHeads = ConvertTo-HeadShas -Listing @("1111111111111111111111111111111111111111`trefs/heads/Foo", "2222222222222222222222222222222222222222`trefs/heads/foo")
Assert-True -Condition ($null -ne $caseHeads -and $caseHeads.Count -eq 2 -and $caseHeads['Foo'] -like '1111*' -and $caseHeads['foo'] -like '2222*') `
    "two remote heads differing only by case are two heads (got $($caseHeads.Count))"
$schemaCaseTags = $schemaA.Replace('"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}},"scope":{"$ref":"#/$defs/scope"}}}]', '"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"sweep_performed"}}},"scope":{"$ref":"#/$defs/scope"}}},{"properties":{"kind":{"properties":{"type":{"const":"Sweep_Performed"}}},"scope":{"$ref":"#/$defs/scope"}}}]').Replace('"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"$ref":"#/$defs/sweep"}},"additionalProperties":false}]}', '"eventKind":{"oneOf":[{"type":"object","required":["type","data"],"properties":{"type":{"const":"sweep_performed"},"data":{"$ref":"#/$defs/sweep"}},"additionalProperties":false},{"type":"object","required":["type","data"],"properties":{"type":{"const":"Sweep_Performed"},"data":{"$ref":"#/$defs/sweep"}},"additionalProperties":false}]}')
$caseTags = Get-DeclaredKinds -SchemaText $schemaCaseTags
Assert-True -Condition ($schemaCaseTags -ne $schemaA -and $null -ne $caseTags -and @($caseTags.Keys).Count -eq 2) `
    "two tags differing only by case are two declared kinds (got $(if ($null -ne $caseTags) { @($caseTags.Keys).Count } else { 'null' }))"

Write-Host ''
Write-Host '-- C at db470cda: combinators are sets, blobs are UTF-8, ancestry is inside the budget --' -ForegroundColor Cyan
# (a) `oneOf` / `anyOf` / `allOf` alternatives are order-independent; `prefixItems` is not.
$schemaOneOf = $schemaA.Replace('"scope":{"required":["executionId"]}', '"scope":{"oneOf":[{"type":"null"},{"required":["executionId"]}]}')
$schemaOneOfSwapped = $schemaA.Replace('"scope":{"required":["executionId"]}', '"scope":{"oneOf":[{"required":["executionId"]},{"type":"null"}]}')
$kO1 = Get-DeclaredKinds -SchemaText $schemaOneOf
$kO2 = Get-DeclaredKinds -SchemaText $schemaOneOfSwapped
$schemaPrefix = $schemaA.Replace('"scope":{"required":["executionId"]}', '"scope":{"prefixItems":[{"type":"null"},{"type":"string"}]}')
$schemaPrefixSwapped = $schemaA.Replace('"scope":{"required":["executionId"]}', '"scope":{"prefixItems":[{"type":"string"},{"type":"null"}]}')
$kP1 = Get-DeclaredKinds -SchemaText $schemaPrefix
$kP2 = Get-DeclaredKinds -SchemaText $schemaPrefixSwapped
Assert-True -Condition ($schemaOneOf -ne $schemaA -and $null -ne $kO1 -and $null -ne $kO2 -and $kO1['sweep_performed'] -eq $kO2['sweep_performed'] -and $null -ne $kP1 -and $null -ne $kP2 -and $kP1['sweep_performed'] -ne $kP2['sweep_performed']) `
    'oneOf alternatives in another order are the same contract, while prefixItems in another order are not'
# (b) a blob read out of git must be decoded as UTF-8, or a non-ASCII const differs from the
#     working-tree read and two identical payloads collide. A real commit, in a temp repository.
$utfRepo = Join-Path ([IO.Path]::GetTempPath()) ("ecs-utf8-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $utfRepo | Out-Null
& git -C $utfRepo init -q 2>$null
& git -C $utfRepo config user.email 'suite@example.invalid' 2>$null
& git -C $utfRepo config user.name 'suite' 2>$null
$utfText = '{"const":"caf' + [char]0x00E9 + ' ' + [char]0x2014 + ' ' + [char]0x4E2D + '"}'
[IO.File]::WriteAllText((Join-Path $utfRepo 'u.json'), $utfText, (New-Object System.Text.UTF8Encoding($false)))
& git -C $utfRepo add u.json 2>$null
& git -C $utfRepo commit -q -m 'utf8' 2>$null
$fromBlob = Read-RefText -Root $utfRepo -Ref 'HEAD' -Path 'u.json'
Remove-Item -LiteralPath $utfRepo -Recurse -Force -ErrorAction SilentlyContinue
Assert-True -Condition ($null -ne $fromBlob -and [string]::Equals($fromBlob.Trim(), $utfText, [System.StringComparison]::Ordinal)) `
    "a blob with non-ASCII text reads back from git byte-for-byte as UTF-8 (got [$fromBlob])"
# (c) the ancestry oracle a population hands out must have done its git work INSIDE the budget:
#     no `merge-base` after the population returns, whatever the caller asks.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# The same remote knob as the REAL REMOTE cells, and the same rule: a remote that does not answer is
# a NOTE with its reason, never a red for the tree (Codex P1 on #1005: this cell used to require
# `Known` and reddened the suite offline before the NOT MEASURED path could run).
$memoRemote = if ([string]::IsNullOrWhiteSpace($env:GH_EVENT_CONTRACT_REMOTE)) { 'origin' } else { $env:GH_EVENT_CONTRACT_REMOTE }
$counted = Get-ContractPopulation -Root $root -Remote $memoRemote
$counterVar = Get-Variable -Scope Script -Name AncestryGitCalls -ErrorAction SilentlyContinue
$before = if ($null -ne $counterVar) { [int] $counterVar.Value } else { -1 }
if ($null -ne $counted -and $counted.Known) { $null = & $counted.IsAncestor 'HEAD' 'no-such-head-1'; $null = & $counted.IsAncestor 'no-such-head-1' 'no-such-head-2' }
$after = if ($null -ne $counterVar) { [int] (Get-Variable -Scope Script -Name AncestryGitCalls).Value } else { -2 }
if ($null -ne $counted -and -not $counted.Known) {
    Write-Host "  NOTE: [event-contract] population UNKNOWN -- $($counted.Reason). The memoisation cell did NOT measure." -ForegroundColor Yellow
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($counted.Reason)) `
        "NOT MEASURED: the memoisation cell is skipped with its reason named ($($counted.Reason)); this is a note, not a pass"
} else {
    Assert-True -Condition ($null -ne $counted -and $counted.Known -and ($counted.PSObject.Properties.Name -contains 'AncestryPairs') -and $before -ge 0 -and $before -eq $after) `
        "ancestry is precomputed under the sweep's budget and memoised: calling IsAncestor after the population returned made no git call (calls before $before, after $after; pairs $(if ($null -ne $counted -and ($counted.PSObject.Properties.Name -contains 'AncestryPairs')) { $counted.AncestryPairs } else { 'n/a' }))"
}

# `merge-base --is-ancestor` has exactly two predicate exits: 0 and 1. A repository/object error
# must not be converted into a false edge and then into a fabricated collision.
$oldInvokeBoundedGit = (Get-Item Function:\Invoke-BoundedGit).ScriptBlock
Set-Item Function:\Invoke-BoundedGit -Value { param($Root, $Arguments, $TimeoutSeconds) [pscustomobject]@{ Code = 2; Lines = @(); TimedOut = $false } }
$ancestryError = try { Invoke-AncestryProbe -Root $root -A 'a' -B 'b' -TimeoutSeconds 1 } catch { $null }
Set-Item Function:\Invoke-BoundedGit -Value $oldInvokeBoundedGit
Assert-True -Condition ($null -ne $ancestryError -and -not $ancestryError.Known -and $ancestryError.Reason -like '*exited 2*') `
    'a nonzero merge-base error other than the documented predicate exits is UNKNOWN, not false ancestry'

# A payload FIELD named `description` is a member of a `properties` map, not the annotation
# keyword: its schema is contract and must not be dropped (Codex P1 on #1005, 2c7da1a2).
$schemaFieldDesc = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"required":["description"],"properties":{"description":{"type":"string"}},"additionalProperties":false}')
$schemaFieldDescNum = $schemaFieldDesc.Replace('"description":{"type":"string"}', '"description":{"type":"number"}')
$kD1 = Get-DeclaredKinds -SchemaText $schemaFieldDesc
$kD2 = Get-DeclaredKinds -SchemaText $schemaFieldDescNum
Assert-True -Condition ($schemaFieldDesc -ne $schemaA -and $schemaFieldDescNum -ne $schemaFieldDesc -and $null -ne $kD1 -and $null -ne $kD2 -and $kD1['sweep_performed'] -ne $kD2['sweep_performed']) `
    'a payload field NAMED description keeps its schema: string vs number there is a different contract'
# A `$ref` is a JSON Pointer: `~1` is `/`, `~0` is `~`, and it may go deeper than one segment.
$schemaEscapedRef = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"#/$defs/sweep~1v2"}').Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep/v2":{"required":["executionId","asOf"],"additionalProperties":false}')
$schemaEscapedRefChanged = $schemaEscapedRef.Replace('"sweep/v2":{"required":["executionId","asOf"]', '"sweep/v2":{"required":["executionId","asOf","caller"]')
$kR1 = Get-DeclaredKinds -SchemaText $schemaEscapedRef
$kR2 = Get-DeclaredKinds -SchemaText $schemaEscapedRefChanged
$schemaDeepRef = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"#/$defs/wrap/properties/inner"}').Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"wrap":{"properties":{"inner":{"required":["executionId","asOf"],"additionalProperties":false}}}')
$schemaDeepRefChanged = $schemaDeepRef.Replace('"inner":{"required":["executionId","asOf"]', '"inner":{"required":["executionId","asOf","caller"]')
$kQ1 = Get-DeclaredKinds -SchemaText $schemaDeepRef
$kQ2 = Get-DeclaredKinds -SchemaText $schemaDeepRefChanged
Assert-True -Condition ($schemaEscapedRef -ne $schemaA -and $null -ne $kR1 -and $null -ne $kR2 -and $kR1['sweep_performed'] -ne $kR2['sweep_performed'] -and $schemaDeepRef -ne $schemaA -and $null -ne $kQ1 -and $null -ne $kQ2 -and $kQ1['sweep_performed'] -ne $kQ2['sweep_performed']) `
    'a $ref with a JSON Pointer escape (~1) or a deeper path (/properties/inner) is resolved, so a change behind it is a payload change'

Write-Host ''
Write-Host '-- C at 2c7da1a2: the budget bounds every call, a clean HEAD collapses, keys and type arrays are sets --' -ForegroundColor Cyan
# (a) `type` arrays are sets.
$schemaTypeArr = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"type":["string","null"]}')
$schemaTypeArrSwapped = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"type":["null","string"]}')
$kT1 = Get-DeclaredKinds -SchemaText $schemaTypeArr
$kT2 = Get-DeclaredKinds -SchemaText $schemaTypeArrSwapped
Assert-True -Condition ($schemaTypeArr -ne $schemaA -and $null -ne $kT1 -and $null -ne $kT2 -and $kT1['sweep_performed'] -eq $kT2['sweep_performed']) `
    'a type array in another order is the same contract (type arrays are sets)'
# (b) object keys are sorted ORDINALLY: `Foo` and `foo` written in opposite orders are the same object.
$schemaKeys = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"properties":{"Foo":{"type":"string"},"foo":{"type":"number"}}}')
$schemaKeysSwapped = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"properties":{"foo":{"type":"number"},"Foo":{"type":"string"}}}')
$kK1 = Get-DeclaredKinds -SchemaText $schemaKeys
$kK2 = Get-DeclaredKinds -SchemaText $schemaKeysSwapped
# MEASURED on this host: Windows PowerShell 5.1's ConvertFrom-Json REFUSES an object whose keys differ
# only by case ("contains the duplicated keys 'Foo' and 'foo'"), so such a schema cannot be read at
# all here and must be UNKNOWN both ways -- never two different values, and never one value that a
# case-folding parser invented. On a host that can parse it, the ordinal key sort makes both orders
# the same value. Either outcome is honest; two different values is the defect.
$sameOrBothUnknown = (($null -eq $kK1) -and ($null -eq $kK2)) -or (($null -ne $kK1) -and ($null -ne $kK2) -and $kK1['sweep_performed'] -eq $kK2['sweep_performed'])
Assert-True -Condition ($schemaKeys -ne $schemaA -and $sameOrBothUnknown) `
    "case-distinct keys in opposite orders: the same value both ways, or UNKNOWN both ways (here: $(if ($null -eq $kK1) { 'UNKNOWN, the host parser refuses case-distinct keys' } else { 'same value' }))"
# (c) a CLEAN HEAD (working tree == checked-out commit) that is an ancestor of another head
#     collapses like any ancestor; only a DIRTY HEAD is its own lineage.
$cleanHead = @()
try {
    $cleanHead = Get-ContractVerdicts -Declarations @(
        (New-Declaration -Branch 'HEAD' -Tag 'sweep_performed' -Canonical 'A'),
        (New-Declaration -Branch 'feat/descendant' -Tag 'sweep_performed' -Canonical 'B')
    ) -IsAncestor (New-Oracle -Edges @('HEAD>feat/descendant')) -HeadDirty:$false
} catch { $cleanHead = @() }
Assert-True -Condition ($cleanHead.Count -eq 1 -and $cleanHead[0].Category -eq 'SINGLE' -and (@($cleanHead[0].Branches) -join ',') -eq 'feat/descendant') `
    'a clean HEAD that is the ancestor of another head collapses into it: SINGLE, no refusal of the parent branch'
# (d) the whole-sweep budget bounds EVERY git call by the time that remains, from the first one:
#     a remote that accepts and never answers, -BudgetSeconds 1 -TimeoutSeconds 20, ends in seconds.
$listener2 = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener2.Start()
$port2 = ([System.Net.IPEndPoint] $listener2.LocalEndpoint).Port
$budgetWatch = [System.Diagnostics.Stopwatch]::StartNew()
$budgeted = Get-ContractPopulation -Root $root -Remote "git://127.0.0.1:$port2/x.git" -TimeoutSeconds 20 -BudgetSeconds 1
$budgetWatch.Stop()
$listener2.Stop()
Start-Sleep -Milliseconds 500
$orphans2 = @(Get-CimInstance Win32_Process -Filter "Name='git.exe'" | Where-Object { $_.CommandLine -match ('git://127\.0\.0\.1:' + $port2 + '/') })
Assert-True -Condition ($null -ne $budgeted -and -not $budgeted.Known -and $budgetWatch.Elapsed.TotalSeconds -lt 10 -and $orphans2.Count -eq 0) `
    "a 1 s budget bounds the FIRST call too: UNKNOWN '$($budgeted.Reason)' after $([math]::Round($budgetWatch.Elapsed.TotalSeconds,1)) s with a 20 s per-call timeout, no orphan"

Write-Host ''
Write-Host '-- C at 1a3cb2c4: dependentRequired is a name map, HEAD declares only what it touched, refusal membership is ordinal --' -ForegroundColor Cyan
# (a) `dependentRequired` keys are instance property names: a field named `description` keeps its
#     companion constraint.
$schemaDepReq = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"dependentRequired":{"description":["executionId"]}}')
$schemaDepReqChanged = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"dependentRequired":{"description":["asOf"]}}')
$kDR1 = Get-DeclaredKinds -SchemaText $schemaDepReq
$kDR2 = Get-DeclaredKinds -SchemaText $schemaDepReqChanged
Assert-True -Condition ($schemaDepReq -ne $schemaA -and $null -ne $kDR1 -and $null -ne $kDR2 -and $kDR1['sweep_performed'] -ne $kDR2['sweep_performed']) `
    'dependentRequired keyed by a field named description keeps its companion list: different companions are a different contract'
# (b) refusal membership is ordinal: a remote branch literally named `head` is not this gate.
$lowerHead = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'head' -Tag 'sweep_performed' -Canonical 'A'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical 'B')
) -IsAncestor (New-Oracle)
Assert-True -Condition ($lowerHead.Count -eq 1 -and $lowerHead[0].Category -eq 'COLLISION' -and (Get-ContractRefusals -Verdicts $lowerHead -Head 'HEAD').Count -eq 0) `
    "a collision between a remote branch named 'head' and another is listed but does not refuse HEAD (ordinal membership)"
# A remote ref may also be named exactly `HEAD`; its explicit identity must not alias the
# working-tree declaration that is displayed as HEAD.
$remoteHead = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'HEAD' -Remote -Tag 'sweep_performed' -Canonical 'REMOTE'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical 'OTHER'),
    (New-Declaration -Branch 'HEAD' -Tag 'sweep_performed' -Canonical 'WORKING-TREE')
) -IsAncestor (New-Oracle)
Assert-True -Condition ($remoteHead.Count -eq 1 -and $remoteHead[0].Category -eq 'COLLISION' -and (Get-ContractRefusals -Verdicts $remoteHead -Head 'HEAD' -WorkingTree).Count -eq 1) `
    'a remote branch named HEAD remains distinct from the working tree and does not alias its refusal identity'
Assert-True -Condition (@($remoteHead[0].Branches | Where-Object { [string]::Equals($_, 'HEAD', [System.StringComparison]::Ordinal) }).Count -eq 2) `
    'the remote HEAD collision retains both the remote HEAD and working-tree declarations as separate branches'
# Candidate external paths are JSON-schema paths, so membership is ordinal and case-sensitive.
Assert-True -Condition ((Test-OrdinalPathContains -Paths @('schemas/foo.json') -Value 'schemas/FOO.json') -eq $false) `
    'external candidate path membership keeps foo.json and FOO.json distinct'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$memoRemote = if ([string]::IsNullOrWhiteSpace($env:GH_EVENT_CONTRACT_REMOTE)) { 'origin' } else { $env:GH_EVENT_CONTRACT_REMOTE }
# (c) HEAD declares only what it touched: a branch that never touched the schema (three-dot against
#     the base) with a clean working tree contributes NO declarations, even where main has moved.
$touchedPop = Get-ContractPopulation -Root $root -Remote $memoRemote
if ($null -ne $touchedPop -and -not $touchedPop.Known) {
    Write-Host "  NOTE: [event-contract] population UNKNOWN -- $($touchedPop.Reason). The touched-HEAD cell did NOT measure." -ForegroundColor Yellow
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($touchedPop.Reason)) `
        "NOT MEASURED: the touched-HEAD cell is skipped with its reason named ($($touchedPop.Reason)); this is a note, not a pass"
} else {
    $hasFields = ($null -ne $touchedPop) -and ($touchedPop.PSObject.Properties.Name -contains 'HeadTouched') -and ($touchedPop.PSObject.Properties.Name -contains 'HeadDeclarations')
    $headDecls = @($touchedPop.Declarations | Where-Object { Test-SameContract $_.Branch 'HEAD' }).Count
    Assert-True -Condition ($hasFields -and (($touchedPop.HeadTouched -or $touchedPop.HeadDirty) -or ($headDecls -eq 0 -and $touchedPop.HeadDeclarations -eq 0))) `
        "HEAD contributes declarations only when it touched the schema or the tree is dirty (touched: $(if ($hasFields) { $touchedPop.HeadTouched } else { 'n/a' }), dirty: $($touchedPop.HeadDirty), HEAD declarations: $headDecls)"
}

Write-Host ''
Write-Host '-- C at 874db014: const is literal, one deadline per blob read, a sole $ref is its target --' -ForegroundColor Cyan
# (a) the VALUE of `const` is literal JSON: an object member named `description` there is data.
$schemaConstA = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"const":{"description":"A"}}')
$schemaConstB = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"const":{"description":"B"}}')
$kC1 = Get-DeclaredKinds -SchemaText $schemaConstA
$kC2 = Get-DeclaredKinds -SchemaText $schemaConstB
Assert-True -Condition ($schemaConstA -ne $schemaA -and $null -ne $kC1 -and $null -ne $kC2 -and $kC1['sweep_performed'] -ne $kC2['sweep_performed']) `
    'an object-valued const keeps every member: {"description":"A"} and {"description":"B"} are different contracts'
# (b) a SOLE `$ref` is its target: a payload written inline and the same payload factored into $defs
#     behind a lone reference are one contract.
$schemaInline = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"required":["executionId","asOf"],"additionalProperties":false}')
$kI = Get-DeclaredKinds -SchemaText $schemaInline
Assert-True -Condition ($schemaInline -ne $schemaA -and $null -ne $kI -and $kI['sweep_performed'] -eq $kindsA['sweep_performed']) `
    'a payload inline and the same payload behind a sole $ref canonicalise identically'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# (c) Read-RefText asks for the time left BEFORE EACH of its two git calls, not once for both.
$script:askedProbe = 0
$probeRead = $null
try { $probeRead = Read-RefText -Root $root -Ref 'HEAD' -Path 'schemas/event-envelope.schema.json' -RemainingSeconds { $script:askedProbe += 1; 30 } } catch { $script:askedProbe = -1 }
Assert-True -Condition ($null -ne $probeRead -and $script:askedProbe -eq 2) `
    "Read-RefText re-asks the remaining time before each git call: asked $($script:askedProbe) time(s) for cat-file + show"

Write-Host ''
Write-Host '-- C at 411016a6: anchor refs expand, remotes are redacted, ancestry names are ordinal --' -ForegroundColor Cyan
# (a) `$ref: "#name"` resolves to the node carrying `$anchor: name`.
$schemaAnchor = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$ref":"#sweepAnchor"}').Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"$anchor":"sweepAnchor","required":["executionId","asOf"],"additionalProperties":false}')
$schemaAnchorChanged = $schemaAnchor.Replace('"required":["executionId","asOf"],"additionalProperties":false}', '"required":["executionId","asOf","caller"],"additionalProperties":false}')
$kAn1 = Get-DeclaredKinds -SchemaText $schemaAnchor
$kAn2 = Get-DeclaredKinds -SchemaText $schemaAnchorChanged
Assert-True -Condition ($schemaAnchor -ne $schemaA -and $schemaAnchorChanged -ne $schemaAnchor -and $null -ne $kAn1 -and $null -ne $kAn2 -and $kAn1['sweep_performed'] -ne $kAn2['sweep_performed']) `
    'a $ref to a $anchor fragment is expanded, so a change behind the anchor is a payload change'
$anchorDecoy = ConvertFrom-Json '{"const":{"$anchor":"target","marker":"decoy"},"properties":{"actual":{"$anchor":"target","type":"string"}}}'
$anchorHit = Find-Anchor -Node $anchorDecoy -Name 'target'
Assert-True -Condition ($null -ne $anchorHit -and $null -ne $anchorHit.Value.PSObject.Properties['type'] -and $null -eq $anchorHit.Value.PSObject.Properties['marker']) `
    'anchor lookup traverses schema locations and ignores a decoy inside const data'
$oldMaxCanonicalNodes = $script:MaxCanonicalNodes
$script:CanonicalNodes = 0
$script:MaxCanonicalNodes = 2
$anchorBudgetResult = 'returned'
try { $null = Find-Anchor -Node (ConvertFrom-Json '{"properties":{"a":{"properties":{"b":{"type":"string"}}}}}') -Name 'missing' } catch { $anchorBudgetResult = 'exhausted' }
$script:MaxCanonicalNodes = $oldMaxCanonicalNodes
Assert-True -Condition ($anchorBudgetResult -ceq 'exhausted') `
    'anchor traversal charges every visited schema node against the shared canonicalisation budget'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# (b) a credential in the remote URL never reaches a reason (the suite prints it; the manifest keeps it).
$leakRead = Get-ContractPopulation -Root $root -Remote 'https://user:sekrit-token@127.0.0.1:9/x.git' -TimeoutSeconds 5 -BudgetSeconds 5
Assert-True -Condition ($null -ne $leakRead -and -not $leakRead.Known -and -not [string]::IsNullOrWhiteSpace($leakRead.Reason) -and $leakRead.Reason -notmatch 'sekrit-token' -and $leakRead.Reason -notmatch 'user:') `
    "a credential-bearing remote is redacted in the reason (got [$($leakRead.Reason)])"
$leakQuery = Get-ContractPopulation -Root $root -Remote 'https://127.0.0.1:9/x.git/token/sekrit-path?access_token=sekrit-query-token' -TimeoutSeconds 5 -BudgetSeconds 5
Assert-True -Condition ($null -ne $leakQuery -and $leakQuery.Reason -notlike '*sekrit-query-token*' -and $leakQuery.Reason -notlike '*sekrit-path*') `
    "credentials in query/path remote text never reach the reason (got [$($leakQuery.Reason)])"
$credentialLabels = @(
    (Get-RemoteLabel 'https://host/repo.git?api_key=sekrit-api-key'),
    (Get-RemoteLabel 'https://host/repo.git?client_secret=sekrit-client-secret'),
    (Get-RemoteLabel 'https://host/repo.git?oauth_token=sekrit-oauth-token'),
    (Get-RemoteLabel 'https://host/repo.git/private_token/sekrit-private-token'),
    (Get-RemoteLabel 'https://host/repo.git?client%5Fsecret=sekrit-encoded-key'),
    (Get-RemoteLabel 'https://host/repo.git/private%5Ftoken/sekrit-encoded-path'),
    (Get-RemoteLabel 'https://host/repo.git?api_key=sekrit%2Dencoded%2Dvalue')
)
$credentialValues = @('sekrit-api-key', 'sekrit-client-secret', 'sekrit-oauth-token', 'sekrit-private-token', 'sekrit-encoded-key', 'sekrit-encoded-path', 'sekrit%2Dencoded%2Dvalue')
$credentialLeaks = @($credentialValues | Where-Object { $needle = $_; @($credentialLabels | Where-Object { $_ -like "*$needle*" }).Count -gt 0 })
Assert-True -Condition ($credentialLeaks.Count -eq 0) `
    "common credential query/path names and percent-encoded names are redacted (leaked: $($credentialLeaks -join ', '); labels: $($credentialLabels -join ' | '))"
# (c) ancestry is ordinal: `Foo` and `foo` are two refs; when one descends from the other they are
#     ONE lineage (SINGLE), which a case-folding uniqueness or `-eq` would report as two.
$caseLineage = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'Foo' -Tag 'sweep_performed' -Canonical 'A'),
    (New-Declaration -Branch 'foo' -Tag 'sweep_performed' -Canonical 'A')
) -IsAncestor (New-Oracle -Edges @('Foo>foo'))
Assert-True -Condition ($caseLineage.Count -eq 1 -and $caseLineage[0].Category -eq 'SINGLE' -and (@($caseLineage[0].Branches) -join ',') -eq 'foo') `
    "case-distinct refs in one lineage collapse ordinally: SINGLE, survivor foo (got $($caseLineage[0].Category) / $(@($caseLineage[0].Branches) -join ','))"

Write-Host ''
Write-Host '-- C at 663ee579: a total expansion budget, $dynamicRef, dependentRequired companions as sets, an injective memo key --' -ForegroundColor Cyan
# (a) an ACYCLIC schema under the depth cap can still expand exponentially: d0 = allOf of 6 refs to
#     d1, ... d5 = allOf of 6 refs to d6. 6^6 = 46656 leaf expansions must be REFUSED (UNKNOWN)
#     by the canonical-node budget. A wall-clock threshold is not a valid observer because machine
#     load changes it; the product promise is bounded node work, not latency.
$wide = ''
for ($i = 0; $i -lt 6; $i++) { $wide += '"d' + $i + '":{"allOf":[' + ((1..6 | ForEach-Object { '{"$ref":"#/$defs/d' + ($i + 1) + '"}' }) -join ',') + ']},' }
$wide += '"d6":{"type":"string"}'
$schemaWide = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"$ref":"#/$defs/d0"},' + $wide)
$oldMaxCanonicalNodes = $script:MaxCanonicalNodes
$script:MaxCanonicalNodes = 1000
$script:CanonicalNodes = 0
$controlKinds = Get-DeclaredKinds -SchemaText $schemaA
$controlNodes = $script:CanonicalNodes
$script:CanonicalNodes = 0
$kWide = Get-DeclaredKinds -SchemaText $schemaWide
 $wideNodes = $script:CanonicalNodes
$script:MaxCanonicalNodes = $oldMaxCanonicalNodes
Assert-True -Condition ($null -ne $controlKinds -and $controlNodes -lt 1000 -and $schemaWide -ne $schemaA -and $null -eq $kWide -and $wideNodes -eq 1001) `
    "an exponentially expanding schema is UNKNOWN after exactly 1001 node visits under a 1000-node budget (control nodes $controlNodes, wide nodes $wideNodes)"
# (b) `$dynamicRef` to a `$dynamicAnchor` resolves like a reference.
$schemaDyn = $schemaA.Replace('"data":{"$ref":"#/$defs/sweep"}', '"data":{"$dynamicRef":"#sweepDyn"}').Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"$dynamicAnchor":"sweepDyn","required":["executionId","asOf"],"additionalProperties":false}')
$schemaDynChanged = $schemaDyn.Replace('"required":["executionId","asOf"],"additionalProperties":false}', '"required":["executionId","asOf","caller"],"additionalProperties":false}')
$kDy1 = Get-DeclaredKinds -SchemaText $schemaDyn
$kDy2 = Get-DeclaredKinds -SchemaText $schemaDynChanged
Assert-True -Condition ($schemaDyn -ne $schemaA -and $schemaDynChanged -ne $schemaDyn -and $null -ne $kDy1 -and $null -ne $kDy2 -and $kDy1['sweep_performed'] -ne $kDy2['sweep_performed']) `
    'a $dynamicRef to a $dynamicAnchor is expanded, so a change behind it is a payload change'
# (c) dependentRequired companion arrays are sets.
$schemaCompA = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"dependentRequired":{"x":["a","b"]}}')
$schemaCompB = $schemaA.Replace('"sweep":{"required":["executionId","asOf"],"additionalProperties":false}', '"sweep":{"dependentRequired":{"x":["b","a"]}}')
$kCo1 = Get-DeclaredKinds -SchemaText $schemaCompA
$kCo2 = Get-DeclaredKinds -SchemaText $schemaCompB
Assert-True -Condition ($schemaCompA -ne $schemaA -and $null -ne $kCo1 -and $null -ne $kCo2 -and $kCo1['sweep_performed'] -eq $kCo2['sweep_performed']) `
    'dependentRequired companions in another order are the same contract (companion arrays are sets)'
# (d) the ancestry memo is keyed by an injective pair: (a, b>c) and (a>b, c) are different pairs.
$memoOk = $false
try {
    $memo = New-AncestryMemo
    Set-AncestryMemo -Memo $memo -A 'a' -B 'b>c' -Value $true
    $memoOk = ((Test-AncestryMemo -Memo $memo -A 'a' -B 'b>c') -eq $true) -and ($null -eq (Test-AncestryMemo -Memo $memo -A 'a>b' -B 'c'))
} catch { $memoOk = $false }
Assert-True -Condition $memoOk `
    "the ancestry memo keys pairs injectively: (a, b>c) is recorded and (a>b, c) is still unknown"

Write-Host ''
Write-Host '-- the three categories --' -ForegroundColor Cyan
$divergent = @(
    (New-Declaration -Branch 'issue-160' -Tag 'sweep_performed' -Canonical 'A'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical 'B')
)
$v = Get-ContractVerdicts -Declarations $divergent -IsAncestor (New-Oracle)
Assert-True -Condition ($v.Count -eq 1 -and $v[0].Category -eq 'COLLISION') `
    'two lineages with divergent payloads under one tag: COLLISION'

$identical = @(
    (New-Declaration -Branch 'issue-160' -Tag 'completion_claimed' -Canonical 'A'),
    (New-Declaration -Branch 'issue-201' -Tag 'completion_claimed' -Canonical 'A')
)
$identicalVerdicts = Get-ContractVerdicts -Declarations $identical -IsAncestor (New-Oracle)
Assert-True -Condition ($identicalVerdicts.Count -eq 1 -and $identicalVerdicts[0].Category -eq 'RE-DECLARATION') `
    'two lineages with identical payloads: RE-DECLARATION, informational'

$descended = @(
    (New-Declaration -Branch 'j-161-guards' -Tag 'clearance_identity_registered' -Canonical 'A'),
    (New-Declaration -Branch 'issue-161-registry' -Tag 'clearance_identity_registered' -Canonical 'A')
)
$v = Get-ContractVerdicts -Declarations $descended -IsAncestor (New-Oracle -Edges @('j-161-guards>issue-161-registry'))
Assert-True -Condition ($v.Count -eq 1 -and $v[0].Category -eq 'SINGLE') `
    'a preserved ref and its descendant are one lineage: SINGLE, and nothing to delete'

$sameCommit = @(
    (New-Declaration -Branch 'HEAD' -Tag 'dlq_routed' -Canonical 'A'),
    (New-Declaration -Branch 'feat/dlq' -Tag 'dlq_routed' -Canonical 'A')
)
$v = Get-ContractVerdicts -Declarations $sameCommit -IsAncestor (New-Oracle -Edges @('HEAD>feat/dlq', 'feat/dlq>HEAD'))
Assert-True -Condition ($v.Count -eq 1 -and $v[0].Category -eq 'SINGLE' -and (@($v[0].Branches) -join ',') -eq 'HEAD') `
    'the same commit under two names (a pushed head) is ONE lineage, and the survivor is HEAD even when the alias sorts first -- a refusal is only ever addressed to HEAD'

$threeWay = @(
    (New-Declaration -Branch 'issue-160' -Tag 'overdue_exception' -Canonical 'A'),
    (New-Declaration -Branch 'issue-201' -Tag 'overdue_exception' -Canonical 'A'),
    (New-Declaration -Branch 'k-162' -Tag 'overdue_exception' -Canonical 'B')
)
$v = Get-ContractVerdicts -Declarations $threeWay -IsAncestor (New-Oracle)
Assert-True -Condition ($v.Count -eq 1 -and $v[0].Category -eq 'COLLISION' -and (@($v[0].Branches) -join ',') -eq 'issue-160,issue-201,k-162') `
    'three lineages, two identical and one divergent: COLLISION naming all three (a copy resolves with its original by declaration, never by measurement)'

$decoyOrder = @(
    (New-Declaration -Branch 'issue-161-registry' -Tag 'clearance_identity_revoked' -Canonical 'A'),
    (New-Declaration -Branch 'j-161-guards' -Tag 'clearance_identity_revoked' -Canonical 'B')
)
$v = Get-ContractVerdicts -Declarations $decoyOrder -IsAncestor (New-Oracle -Edges @('j-161-guards>issue-161-registry'))
Assert-True -Condition ($v.Count -eq 1 -and $v[0].Category -eq 'SINGLE' -and (@($v[0].Branches) -join ',') -eq 'issue-161-registry') `
    'the collapse consults the oracle: the descendant listed FIRST still survives and the ancestor drops'

$none = Get-ContractVerdicts -Declarations @() -IsAncestor (New-Oracle)
Assert-True -Condition ($null -ne $none -and @($none).Count -eq 0) `
    'zero declarations is an empty list of verdicts, not null'

Write-Host ''
Write-Host '-- who is refused --' -ForegroundColor Cyan
$party = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'HEAD' -Tag 'sweep_performed' -Canonical 'A'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical 'B')
) -IsAncestor (New-Oracle)
Assert-Equal 1 (Get-ContractRefusals -Verdicts $party -Head 'HEAD').Count `
    'this head declares a tag another lineage declares differently: refused'

$bystander = Get-ContractVerdicts -Declarations @(
    (New-Declaration -Branch 'issue-160' -Tag 'sweep_performed' -Canonical 'A'),
    (New-Declaration -Branch 'k-162' -Tag 'sweep_performed' -Canonical 'B'),
    (New-Declaration -Branch 'HEAD' -Tag 'dlq_routed' -Canonical 'C')
) -IsAncestor (New-Oracle)
Assert-True -Condition (@($bystander | Where-Object { $_.Category -eq 'COLLISION' }).Count -eq 1 -and (Get-ContractRefusals -Verdicts $bystander -Head 'HEAD').Count -eq 0) `
    'two OTHER branches collide: listed, but this head is not refused for their fight'

Assert-Equal 0 (Get-ContractRefusals -Verdicts $identicalVerdicts -Head 'issue-160').Count `
    'a re-declaration is never a refusal'

Write-Host ''
Write-Host '-- the bounds are the sweep''s own, not the operating system''s (C, third pass) --' -ForegroundColor Cyan
# A remote that ACCEPTS the connection and says nothing: the OS connect timeout never fires (that
# one dies alone at ~21 s and agreed with the 20 s bound by coincidence), a plain `git fetch` was
# still running at 75 s, and only the sweep's own bound can end it. Measured by C at 69be5317 with
# this exact fixture; kept here so the receipt is the suite's.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([System.Net.IPEndPoint] $listener.LocalEndpoint).Port
$hangWatch = [System.Diagnostics.Stopwatch]::StartNew()
$hung = Get-ContractPopulation -Root $root -Remote "git://127.0.0.1:$port/x.git" -TimeoutSeconds 2
$hangWatch.Stop()
$listener.Stop()
Assert-True -Condition ($null -ne $hung -and -not $hung.Known -and $hung.Reason -like '*did not finish within*' -and $hangWatch.Elapsed.TotalSeconds -lt 30) `
    "a remote that accepts and never answers is cut by the sweep's own bound: UNKNOWN '$($hung.Reason)' after $([math]::Round($hangWatch.Elapsed.TotalSeconds,1)) s, no hang"
# The bound is not a bound if the process it timed out is still alive: an orphaned `git fetch`
# inherits the runner's stdout handle and `ci/run-ps-suites.ps1` then prints its summary and never
# exits -- measured 2026-09-08 on two runs of this very suite, released the instant the orphans were
# killed by hand. `Process.Kill()` ends `git.exe` and not the child `git` forks for `git://`, so the
# timeout path must kill the TREE, and this cell is the one that would have caught it.
Start-Sleep -Milliseconds 500
$orphans = @(Get-CimInstance Win32_Process -Filter "Name='git.exe'" | Where-Object { $_.CommandLine -match ('git://127\.0\.0\.1:' + $port + '/') })
Assert-True -Condition ($orphans.Count -eq 0) `
    "the timed-out fetch left no process behind (found $($orphans.Count): $(($orphans | ForEach-Object { $_.ProcessId }) -join ', '))"

# The working tree is an input too: an oversized envelope on disk is refused before it is read,
# exactly like a remote blob.
$bigDir = Join-Path ([IO.Path]::GetTempPath()) ("ecs-big-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $bigDir | Out-Null
$bigFile = Join-Path $bigDir 'big.json'
$stream = [IO.File]::Create($bigFile); $stream.SetLength($script:MaxSchemaBlobBytes + 1); $stream.Close()
$bigRead = if (Get-Command Read-WorkingTreeText -ErrorAction SilentlyContinue) { Read-WorkingTreeText -Path $bigFile } else { 'no bounded reader exists' }
Assert-True -Condition ($null -eq $bigRead) `
    "a working-tree schema over $script:MaxSchemaBlobBytes bytes reads as UNKNOWN (null), never as text to parse"
Remove-Item -LiteralPath $bigDir -Recurse -Force -ErrorAction SilentlyContinue

# A repository root with a space in it must reach git as ONE argument through Start-Process.
$spacedRoot = Join-Path ([IO.Path]::GetTempPath()) ("ecs root " + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $spacedRoot | Out-Null
& git -C $spacedRoot init -q 2>$null
$spaced = Invoke-BoundedGit -Root $spacedRoot -Arguments @('rev-parse', '--is-inside-work-tree') -TimeoutSeconds 20
Remove-Item -LiteralPath $spacedRoot -Recurse -Force -ErrorAction SilentlyContinue
Assert-True -Condition ($null -ne $spaced -and $spaced.Code -eq 0 -and (@($spaced.Lines) -join '') -eq 'true') `
    "a repository root containing a space is passed to git whole (code $($spaced.Code), lines [$(@($spaced.Lines) -join '|')])"

Write-Host ''
# These controls call the production paths; the mutable reader is used only while capturing.
$script:snapshotPayload = '{"type":"string"}'
$script:snapshotReads = 0
$snapshotReader = { param($Reference) $script:snapshotReads++; return $script:snapshotPayload }
$snapshot = $null; $snapshotAgain = $null; $snapshotChanged = $null
if (Get-Command New-ContractSnapshot -ErrorAction SilentlyContinue) {
    $snapshot = New-ContractSnapshot -SchemaText $schemaExternalRef -ExternalResolver $snapshotReader
    $snapshotAgain = Get-DeclaredKinds -Snapshot $snapshot
    $script:snapshotPayload = '{"type":"number"}'
    $sameSnapshot = Get-DeclaredKinds -Snapshot $snapshot
    $snapshotChanged = New-ContractSnapshot -SchemaText $schemaExternalRef -ExternalResolver $snapshotReader
    $changedSnapshot = Get-DeclaredKinds -Snapshot $snapshotChanged
}
Assert-True ($null -ne $snapshot -and $snapshotAgain['sweep_performed'] -ceq $sameSnapshot['sweep_performed'] -and $script:snapshotReads -eq 2) `
    'a captured reference snapshot is immutable and repeated canonicalization makes no reader calls'
Assert-True ($null -ne $snapshotChanged -and $snapshotAgain['sweep_performed'] -cne $changedSnapshot['sweep_performed']) `
    'a fresh snapshot observes changed referenced payload content'
$canonicalCommand = Get-Command ConvertTo-ContractCanonical
Assert-True (-not $canonicalCommand.Parameters.ContainsKey('ExternalResolver') -and -not $canonicalCommand.Parameters.ContainsKey('ExternalDocuments')) `
    'the pure canonicalizer exposes no external resolver or resource registry'
$noExternal = Get-ExternalReferencePaths -SchemaText $schemaA -SchemaPath 'schemas/event-envelope.schema.json'
Assert-True ($null -ne $noExternal -and @($noExternal).Count -eq 0) `
    'zero external dependencies is a known empty set, not UNKNOWN'
$savedBounded = (Get-Command Invoke-BoundedGit).ScriptBlock
$savedWorking = (Get-Command Read-WorkingTreeText).ScriptBlock
$script:expiredReads = 0
try {
    Set-Item Function:\Invoke-BoundedGit -Value { param($Root,$Arguments,$TimeoutSeconds) $script:expiredReads++; [pscustomobject]@{Code=0;Lines=@('2');TimedOut=$false} }
    $expiredRead = Read-RefText -Root $root -Ref HEAD -Path 'schemas/x.json' -RemainingSeconds { 0 }
    Assert-True ($null -eq $expiredRead -and $script:expiredReads -eq 0) 'an expired read launches neither size nor content Git command'
    $script:expiredReads = 0; $script:remainingChecks = 0
    $midRead = Read-RefText -Root $root -Ref HEAD -Path 'schemas/x.json' -RemainingSeconds { $script:remainingChecks++; if ($script:remainingChecks -eq 1) {1} else {0} }
    Assert-True ($null -eq $midRead -and $script:expiredReads -eq 1 -and $script:remainingChecks -eq 2) 'expiry after the size probe prevents the content Git command'
    $script:expiredReads = 0
    Set-Item Function:\Read-WorkingTreeText -Value { param($Path) $script:expiredReads++; '{}' }
    $expiredResolver = New-ExternalResolver -Root $root -SchemaPath 'schemas/event-envelope.schema.json' -SourceRef HEAD -RemainingSeconds {0} -WorkingTree
    $expiredTarget = & $expiredResolver 'x.json'
    Assert-True ($null -eq $expiredTarget -and $script:expiredReads -eq 0) 'an expired external capture performs no working-tree read'
} finally {
    Set-Item Function:\Invoke-BoundedGit -Value $savedBounded
    Set-Item Function:\Read-WorkingTreeText -Value $savedWorking
}

# Real local Git population: neither branch edits the envelope, only its referenced payload.
$contractFixture = Join-Path ([IO.Path]::GetTempPath()) ('ecs-dependency-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($contractFixture)
$fixtureRepo = Join-Path $contractFixture 'work'
$fixtureRemote = Join-Path $contractFixture 'remote.git'
$fixtureHooks = Join-Path $contractFixture 'hooks'
[void][IO.Directory]::CreateDirectory($fixtureHooks)
function Invoke-ContractFixtureGit {
    param([string[]]$GitArgs)
    $out = @(& git -c commit.gpgsign=false -c core.hooksPath=$fixtureHooks -c user.name=Fixture -c user.email=fixture@example.invalid @GitArgs 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "fixture git failed: $($out -join ' ')" }
}
try {
    Invoke-ContractFixtureGit @('init','--bare','-q',$fixtureRemote)
    Invoke-ContractFixtureGit @('init','-q','-b','main',$fixtureRepo)
    [void][IO.Directory]::CreateDirectory((Join-Path $fixtureRepo 'schemas'))
    $fixtureEnvelope = Join-Path $fixtureRepo 'schemas/event-envelope.schema.json'
    $fixturePayload = Join-Path $fixtureRepo 'schemas/checked-target.schema.json'
    [IO.File]::WriteAllText($fixtureEnvelope,$schemaExternalRef)
    [IO.File]::WriteAllText($fixturePayload,'{"type":"string"}')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'add','.')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qm','baseline')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'remote','add','origin',$fixtureRemote)
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','main')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-qb','alpha')
    [IO.File]::WriteAllText($fixturePayload,'{"type":"number"}')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qam','alpha payload')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','alpha')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-qb','beta','main')
    [IO.File]::WriteAllText($fixturePayload,'{"type":"boolean"}')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qam','beta payload')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','beta')
    $dependencyPopulation = Get-ContractPopulation -Root $fixtureRepo -BudgetSeconds 120
    $dependencyRefusals = @()
    if ($dependencyPopulation.Known) {
        $dependencyVerdicts = Get-ContractVerdicts -Declarations $dependencyPopulation.Declarations -IsAncestor $dependencyPopulation.IsAncestor -HeadDirty $dependencyPopulation.HeadDirty
        $dependencyRefusals = @(Get-ContractRefusals -Verdicts $dependencyVerdicts -Head HEAD)
    }
    Assert-True ($dependencyPopulation.Known -and $dependencyPopulation.HeadTouched -and $dependencyRefusals.Count -eq 1) `
        'referenced-file-only changes enter both remote and clean HEAD populations and produce their collision'

    # Exercise the real native runner with a small retained-output budget; neither stream may
    # become a successful reading when its cap is crossed. No hostile remote or huge file needed.
    $script:MaxGitOutputBytes = 16
    try {
        $tooMuchOut = Invoke-BoundedGit -Root $fixtureRepo -Arguments @('show','HEAD:schemas/event-envelope.schema.json')
        Assert-True ($tooMuchOut.Code -eq -1 -and @($tooMuchOut.Lines).Count -eq 0) 'native stdout over budget is refused before materialization'
        $tooMuchErr = Invoke-BoundedGit -Root $fixtureRepo -Arguments @('rev-parse','--verify','definitely-missing-fixture-ref')
        Assert-True ($tooMuchErr.Code -eq -1 -and @($tooMuchErr.Lines).Count -eq 0) 'native stderr over budget is refused too'
    } finally { $script:MaxGitOutputBytes = 8MB }
    $oversizedListing = @(1..4097 | ForEach-Object { ('a' * 40) + "`trefs/heads/fixture-$_" })
    Assert-True ($null -eq (ConvertTo-HeadShas -Listing $oversizedListing)) 'an oversized remote ref population is UNKNOWN before ancestry work'

    # main changes X after stale forks; stale changes only Y. Its old X is not a declaration.
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-q','main')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-qb','stale')
    $withY = $schemaExternalRef | ConvertFrom-Json
    $yVariant = $withY.oneOf[0] | ConvertTo-Json -Depth 30 | ConvertFrom-Json
    $yVariant.properties.kind.properties.type.const = 'unrelated_y'
    $yKind = $withY.'$defs'.eventKind.oneOf[0] | ConvertTo-Json -Depth 30 | ConvertFrom-Json
    $yKind.properties.type.const = 'unrelated_y'
    $withY.oneOf = @($withY.oneOf) + @($yVariant)
    $withY.'$defs'.eventKind.oneOf = @($withY.'$defs'.eventKind.oneOf) + @($yKind)
    [IO.File]::WriteAllText($fixtureEnvelope,($withY | ConvertTo-Json -Depth 30))
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qam','stale adds Y only')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','stale')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-q','main')
    [IO.File]::WriteAllText($fixturePayload,'{"type":"number"}')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qam','main changes X')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','main')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-qb','current')
    [IO.File]::WriteAllText($fixturePayload,'{"type":"boolean"}')
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'commit','-qam','current changes X')
    # Remove earlier independent test branches so they cannot legitimately collide with current.
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'push','-q','origin','--delete','alpha','beta')
    $divergence = Get-ContractPopulation -Root $fixtureRepo -BudgetSeconds 120
    $staleX = @($divergence.Declarations | Where-Object { $_.Branch -eq 'stale' -and $_.Tag -eq 'sweep_performed' })
    $staleY = @($divergence.Declarations | Where-Object { $_.Branch -eq 'stale' -and $_.Tag -eq 'unrelated_y' })
    Assert-True ($divergence.Known -and $staleX.Count -eq 0 -and $staleY.Count -eq 1) 'a stale branch declares only tags changed against its own merge base'
    $divergenceVerdicts = Get-ContractVerdicts -Declarations $divergence.Declarations -IsAncestor $divergence.IsAncestor -HeadDirty $divergence.HeadDirty
    $divergenceRefusals = Get-ContractRefusals -Verdicts $divergenceVerdicts -Head HEAD
    Assert-True (@($divergenceRefusals).Count -eq 0) 'the stale unrelated tag does not falsely refuse the current X edit'
    Invoke-ContractFixtureGit @('-C',$fixtureRepo,'checkout','-q','stale')
    $staleHead = Get-ContractPopulation -Root $fixtureRepo -BudgetSeconds 120
    $headX = @($staleHead.Declarations | Where-Object { $_.Branch -eq 'HEAD' -and $_.Tag -eq 'sweep_performed' })
    Assert-True ($staleHead.Known -and $headX.Count -eq 0 -and $staleHead.HeadDeclarations -eq 1) 'a stale HEAD also declares only its own changed tag'
} finally {
    $resolvedFixture = [IO.Path]::GetFullPath($contractFixture)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolvedFixture.StartsWith($expectedParent,[StringComparison]::OrdinalIgnoreCase)) { throw 'unsafe fixture cleanup path' }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
}

# A dirty working-tree schema can be readable while the checked-out HEAD has no schema blob. The
# population must refuse as UNKNOWN before passing the null committed text to a mandatory string
# parameter; a thrown parameter-binding error is a broken instrument, not a measured refusal.
$missingHeadRoot = Join-Path ([IO.Path]::GetTempPath()) ('ecs-missing-head-' + [guid]::NewGuid().ToString('N'))
$missingHeadRepo = Join-Path $missingHeadRoot 'work'
$missingHeadRemote = Join-Path $missingHeadRoot 'origin.git'
New-Item -ItemType Directory -Path $missingHeadRepo -Force | Out-Null
& git -C $missingHeadRepo init -q -b main 2>$null
& git -C $missingHeadRepo config user.email 'suite@example.invalid' 2>$null
& git -C $missingHeadRepo config user.name 'suite' 2>$null
$missingHeadSchema = Join-Path $missingHeadRepo 'schemas/event-envelope.schema.json'
New-Item -ItemType Directory -Path (Split-Path -Parent $missingHeadSchema) -Force | Out-Null
[IO.File]::WriteAllText($missingHeadSchema, $schemaA, (New-Object System.Text.UTF8Encoding($false)))
& git -C $missingHeadRepo add schemas/event-envelope.schema.json 2>$null
& git -C $missingHeadRepo commit -q -m baseline 2>$null
& git init --bare -q $missingHeadRemote 2>$null
& git -C $missingHeadRepo remote add origin $missingHeadRemote 2>$null
& git -C $missingHeadRepo push -q origin main 2>$null
& git -C $missingHeadRepo checkout -qb candidate 2>$null
Remove-Item -LiteralPath $missingHeadSchema -Force
& git -C $missingHeadRepo add -u schemas/event-envelope.schema.json 2>$null
& git -C $missingHeadRepo commit -q -m 'remove committed schema' 2>$null
[IO.File]::WriteAllText($missingHeadSchema, $schemaA, (New-Object System.Text.UTF8Encoding($false)))
$missingHeadPopulation = try { Get-ContractPopulation -Root $missingHeadRepo -BudgetSeconds 120 } catch { $null }
Assert-True -Condition ($null -ne $missingHeadPopulation -and -not $missingHeadPopulation.Known -and $missingHeadPopulation.Reason -like '*HEAD:schemas/event-envelope.schema.json*') `
    'a readable dirty schema with no committed HEAD blob is UNKNOWN with a named reason, not a parameter-binding crash'
$resolvedMissingHead = [IO.Path]::GetFullPath($missingHeadRoot)
$expectedMissingParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
if (-not $resolvedMissingHead.StartsWith($expectedMissingParent,[StringComparison]::OrdinalIgnoreCase)) { throw 'unsafe missing-head fixture cleanup path' }
Remove-Item -LiteralPath $resolvedMissingHead -Recurse -Force

Write-Host '-- REAL REMOTE (observable only on the day it fires) --' -ForegroundColor Cyan
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# `GH_EVENT_CONTRACT_REMOTE` lets a reader point this cell at a remote that does not exist or does
# not answer, to watch the UNKNOWN path produce its note instead of a red. Unset, it is `origin`.
$remoteName = if ([string]::IsNullOrWhiteSpace($env:GH_EVENT_CONTRACT_REMOTE)) { 'origin' } else { $env:GH_EVENT_CONTRACT_REMOTE }
    # THE BASE ENVELOPE IS READABLE BY THIS SWEEP, asserted as a property of the TREE, before and apart from
    # the remote. Lane B on #1005 reverted the nested-`$ref` fix and this suite stayed 96/96: the real-remote
    # cell below turns every UNKNOWN into a note, so a sweep that cannot read its own base schema was
    # indistinguishable from a remote that did not answer. A reason string could tell them apart, but a
    # vocabulary guard over prose ages against the prose; this asks the question directly instead.
    # origin/main is the base the sweep judges against; a checkout without it asks HEAD, which must be
    # readable for the same reason and is never a remote question.
    $baseRef = 'origin/main'
    try { & git -C $root rev-parse -q --verify 'origin/main^{commit}' 2>$null | Out-Null; if ($LASTEXITCODE -ne 0) { $baseRef = 'HEAD' } } catch { $baseRef = 'HEAD' }
    $baseEnvelopeText = Read-RefText -Root $root -Ref $baseRef -Path 'schemas/event-envelope.schema.json' -RemainingSeconds { 60 }
    $baseResolver = New-ExternalResolver -Root $root -SchemaPath 'schemas/event-envelope.schema.json' -SourceRef $baseRef -RemainingSeconds { 60 }
    $baseKinds = if ($null -ne $baseEnvelopeText) { try { Get-DeclaredKinds -SchemaText $baseEnvelopeText -ExternalResolver $baseResolver } catch { $null } } else { $null }
    Assert-True -Condition ($null -ne $baseKinds -and @($baseKinds.Keys).Count -gt 0) `
        "the sweep can read the declared kinds off ${baseRef}:schemas/event-envelope.schema.json, nested external references included (got $(if ($null -ne $baseKinds) { @($baseKinds.Keys).Count } else { 'UNKNOWN' }) kinds); a sweep that cannot read its own base measures nothing, and that is a fact about the tree, not the remote"

$population = Get-ContractPopulation -Root $root -Remote $remoteName
# The property under test is declaration parity across branches, not this tree's code. A remote
# that does not answer is a NOTE with its reason and an UNKNOWN in the record -- never a red for
# the tree, and never a wait: every remote call inside the sweep is bounded (20 s).
Assert-True -Condition ($null -ne $population -and ($population.Known -or -not [string]::IsNullOrWhiteSpace($population.Reason))) `
    'the reading is either a resolved population or an UNKNOWN that names its reason -- silence is neither'
if ($null -ne $population -and $population.Known) {
    $real = Get-ContractVerdicts -Declarations $population.Declarations -IsAncestor $population.IsAncestor -HeadDirty $population.HeadDirty
    $refused = (Get-ContractRefusals -Verdicts $real -Head 'HEAD')
    # The marker the gate reads out of this stage's capture into the manifest (`eventContractSweep`).
    $script:printedMarker = "[event-contract] MEASURED -- $($population.HeadCount) remote head(s); $(@($population.Touched).Count) touched the envelope since their base; $(@($population.Declarations).Count) declaration(s) differ from $($population.Base)"
    Write-Host $script:printedMarker
    Assert-Equal 0 $refused.Count `
        "this head is party to no collision ($($population.HeadCount) remote heads, $(@($population.Touched).Count) touched the envelope since their base, $(@($population.Declarations).Count) declaration(s) differ from $($population.Base))"
        # FLOORS BY A DIFFERENT INSTRUMENT. Lane B blinded the diff-first shortcut (every head reads
        # unchanged) and the sweep still printed MEASURED with 0 touched and this cell stayed green. The
        # floors below are computed with plain git in this cell, not with the sweep's own functions, so
        # they cannot share its blindness. They are BOUNDS, not equalities, because an oracle with the
        # sweep's exact semantics would be the sweep: HeadCount must match ls-remote to within a race
        # (a push between the sweep's listing and this one), and Touched must be at least the number of
        # heads whose ENVELOPE FILE differs from base three-dot -- SchemaPath is always in the sweep's
        # diff list, so every such head is touched by construction. A blinded diff reports 0 and sinks.
        # #1247: up to three bounded attempts, because this second listing is an instrument and one
        # slow answer is not a fact about the tree. Still unavailable after them, both floors are
        # SKIPPED with the reason -- counted, never passed -- matching the NOT MEASURED arm below.
        $lsProbe = $null
        $lsUnavailable = 'not attempted'
        foreach ($lsAttempt in 1..3) {
            $lsProbe = Invoke-BoundedGit -Root $root -Arguments @('ls-remote', '--heads', $remoteName) -TimeoutSeconds 20
            $lsUnavailable = Get-ListingUnavailableReason $lsProbe
            if ($null -eq $lsUnavailable) { break }
        }
        if ($null -ne $lsUnavailable) {
            Write-Host "[event-contract/floors] NOT MEASURED -- the independent ls-remote listing was unavailable after 3 attempts ($lsUnavailable)" -ForegroundColor Yellow
            Skip-Assertion `
                "NOT MEASURED: the HeadCount floor needs an independent ls-remote listing, unavailable after 3 attempts ($lsUnavailable); this is a note, not a pass"
            Skip-Assertion `
                "NOT MEASURED: the Touched floor is counted over the same listing ($lsUnavailable); this is a note, not a pass"
        } else {
        $lsHeads = @($lsProbe.Lines)
        $headGap = [Math]::Abs([int]$population.HeadCount - $lsHeads.Count)
        Assert-True -Condition ($lsHeads.Count -gt 0 -and $headGap -le 2) `
            "the population's HeadCount ($($population.HeadCount)) is the remote's head listing ($($lsHeads.Count) by bounded git ls-remote, gap $headGap, race tolerance 2)"
        $envelopeFloor = 0
        foreach ($listed in $lsHeads) {
            $sha = ($listed -split '\s+')[0]
            if ($sha -notmatch '^[0-9a-f]{40}$') { continue }
            $diffProbe = Invoke-BoundedGit -Root $root -Arguments @('diff', '--quiet', "origin/main...$sha", '--', 'schemas/event-envelope.schema.json') -TimeoutSeconds 20
            $diffOutputExceeded = $null -ne $diffProbe -and ($diffProbe.PSObject.Properties.Name -contains 'OutputExceeded') -and [bool]$diffProbe.OutputExceeded
            if ($null -ne $diffProbe -and -not $diffProbe.TimedOut -and -not $diffOutputExceeded -and $diffProbe.Code -eq 1) { $envelopeFloor++ }
        }
        Assert-True -Condition (@($population.Touched).Count -ge $envelopeFloor) `
            "Touched ($(@($population.Touched).Count)) is at least the number of heads whose envelope file differs from origin/main three-dot ($envelopeFloor, counted here with plain git); fewer means the sweep skipped heads it had to examine"
        }
} else {
    $reason = if ($null -ne $population) { $population.Reason } else { 'the sweep returned nothing' }
    # The marker the gate reads out of this stage's capture into the manifest (`eventContractSweep`).
    $script:printedMarker = "[event-contract] NOT MEASURED -- $reason"
    Write-Host $script:printedMarker -ForegroundColor Yellow
    Skip-Assertion `
        "NOT MEASURED: the collision cell is skipped with its reason named ($reason); this is a note, not a pass"
    # Three remote checks are unmeasured; keep them in the expected population but out of the passed count.
    Skip-Assertion `
        'NOT MEASURED: the HeadCount floor is skipped with the same reason; this is a note, not a pass'
    Skip-Assertion `
        'NOT MEASURED: the Touched floor is skipped with the same reason; this is a note, not a pass'
}

# The independent control must not be allowed to wedge after a Known population. Exercise the
# same bounded reader against a silent git transport only after the normal Known/UNKNOWN branch
# above has completed; this is the Known-first/hung-second observer for the suite harness.
$floorListener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$floorListener.Start()
$floorPort = ([System.Net.IPEndPoint] $floorListener.LocalEndpoint).Port
$floorProbeWatch = [System.Diagnostics.Stopwatch]::StartNew()
$floorProbe = Invoke-BoundedGit -Root $root -Arguments @('ls-remote', '--heads', "git://127.0.0.1:$floorPort/x.git") -TimeoutSeconds 2
$floorProbeWatch.Stop()
$floorListener.Stop()
Assert-True -Condition ($null -ne $floorProbe -and $floorProbe.TimedOut -and $floorProbeWatch.Elapsed.TotalSeconds -lt 30) `
    'the independent floor observer stays bounded after the Known reading when the second git operation hangs'

Write-Host ''
Write-Host '-- the manifest record: what the gate writes down about this reading --' -ForegroundColor Cyan
# `Get-EventContractSweepRecord` lives in ci/gate.ps1 beside `Get-RunCoverage`, and is lifted out of
# it by AST rather than retyped, because a copy passes while the original rots.
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$recordFn = ([System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$null)).Find({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -ceq 'Get-EventContractSweepRecord'
    }, $true)
Assert-True -Condition ($null -ne $recordFn) `
    'ARRANGEMENT: the sweep record rule is a function in gate.ps1 this cell can call'
if ($null -ne $recordFn) {
    . ([scriptblock]::Create($recordFn.Extent.Text))
    $measured = Get-EventContractSweepRecord -Lines @('[gate] something else', '[event-contract] MEASURED -- 300 remote head(s); 13 touched', 'PASSED: 19 of 19')
    Assert-True -Condition ($measured.state -ceq 'measured' -and $measured.reason -ceq '') `
        "a MEASURED marker in the stage capture records state 'measured' with no reason (got [$($measured.state)] [$($measured.reason)])"
    $mute = Get-EventContractSweepRecord -Lines @('[event-contract] NOT MEASURED -- git fetch origin did not finish within 20s', 'PASSED: 19 of 19')
    Assert-True -Condition ($mute.state -ceq 'notMeasured' -and $mute.reason -ceq 'git fetch origin did not finish within 20s') `
        "a NOT MEASURED marker records state 'notMeasured' and carries the reason verbatim (got [$($mute.state)] [$($mute.reason)])"
    # Producer and parser are bound by a string, and a string can be renamed on one side only.
    # This feeds the rule the line this very run PRINTED, so a rename that leaves every other cell
    # green cannot leave the manifest saying `absent` (K's residual on #1005).
    $printed = Get-EventContractSweepRecord -Lines @($script:printedMarker)
    $expectedState = if ($null -ne $population -and $population.Known) { 'measured' } else { 'notMeasured' }
    Assert-True -Condition ($printed.state -ceq $expectedState) `
        "the marker this run actually printed is read back as '$expectedState' (got [$($printed.state)] from [$($script:printedMarker)])"
} else {
    Assert-True -Condition $false 'the measured-marker cell could not run: no function to call'
    Assert-True -Condition $false 'the not-measured-marker cell could not run: no function to call'
}

Write-Host ''
Write-Host '-- the WIRE: a marker printed by the background suites stage reaches the stage capture --' -ForegroundColor Cyan
# Ten landed manifests carried 0 outputTail lines for `ci powershell suites`: the stage block did
# `$joined = Complete-BackgroundStage ...`, and the join re-emits the child's log on the success
# stream, so every line was swallowed into `$joined` and the marker was structurally unobservable
# (the 5fc672e8 gate wrote `eventContractSweep: absent` on a run whose suite had printed it). This
# cell drives the REAL join and the REAL stage block, sliced out of gate.ps1 by anchor, with a
# child that prints the marker, and reads the record back through the capture Invoke-Stage keeps.
$gateText = [IO.File]::ReadAllText($gatePath)
function Get-SweepGateSlice {
    param([string] $Start, [string] $End, [switch] $IncludeEnd)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -lt 0) { throw "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]" }
    $end = if ($IncludeEnd) { $j + $End.Length } else { $j }
    return $gateText.Substring($i, $end - $i)
}
Invoke-Expression (Get-SweepGateSlice -Start 'function Start-BackgroundStage {' -End "`n}" -IncludeEnd)
Invoke-Expression (Get-SweepGateSlice -Start 'function Complete-BackgroundStage {' -End "`n}" -IncludeEnd)
$stageBlock = Get-SweepGateSlice -Start "    Invoke-Stage 'ci powershell suites' -AlwaysRun {" -End "`n    } | Out-Null" -IncludeEnd
# The stub keeps exactly the half of Invoke-Stage this cell is about: the body's success and
# information streams are the capture. Everything else the real one does (timing, evidence,
# redaction) is not the subject here.
$script:probeCaptured = New-Object System.Collections.Generic.List[string]
function Invoke-Stage {
    param([string] $Name, [switch] $AlwaysRun, [scriptblock] $Body)
    & $Body 2>&1 6>&1 | ForEach-Object { $script:probeCaptured.Add([string] $_) }
}
$repositoryRoot = $root
$script:psSuitesStarted = Start-BackgroundStage -Name 'ci powershell suites' -FilePath 'powershell' `
    -ArgumentList @('-NoProfile', '-Command', "Write-Host '[event-contract] MEASURED -- probe child'; exit 0") `
    -WorkingDirectory $PSScriptRoot
if ($null -eq $script:psSuitesStarted) { throw 'HARNESS-BROKE: the probe child could not be started' }
$global:LASTEXITCODE = 99
Invoke-Expression $stageBlock
$wired = Get-EventContractSweepRecord -Lines $script:probeCaptured.ToArray()
Assert-True -Condition ($wired.state -ceq 'measured') `
    "the marker the background suites stage printed reaches the stage capture and reads back as 'measured' (got [$($wired.state)] from $($script:probeCaptured.Count) captured line(s))"
# `$joined` lives in the block's own scope; what Invoke-Stage READS to decide the stage is
# `$LASTEXITCODE`, which the join sets. Seeded with 99 so a join that never ran cannot pass.
Assert-True -Condition ($global:LASTEXITCODE -eq 0) `
    "and the join's exit code still reaches `$LASTEXITCODE, which is what Invoke-Stage reads (got [$global:LASTEXITCODE])"

# The actual capture path must not let later suite fixture output replace an incomplete reading.
$script:probeCaptured.Clear()
$script:psSuitesStarted = Start-BackgroundStage -Name 'ci powershell suites' -FilePath 'powershell' `
    -ArgumentList @('-NoProfile', '-Command', "Write-Host '[event-contract] NOT MEASURED -- offline'; Write-Host '[event-contract] MEASURED -- later fixture'; exit 0") `
    -WorkingDirectory $PSScriptRoot
if ($null -eq $script:psSuitesStarted) { throw 'HARNESS-BROKE: ambiguity child could not start' }
Invoke-Expression $stageBlock
$ambiguous = Get-EventContractSweepRecord -Lines $script:probeCaptured.ToArray()
Assert-True ($ambiguous.state -ceq 'notMeasured') 'actual background stage capture refuses conflicting sweep markers'
foreach ($pair in @(@('MEASURED','NOT MEASURED'), @('MEASURED','MEASURED'), @('NOT MEASURED','NOT MEASURED'))) {
    $record = Get-EventContractSweepRecord -Lines @($pair | ForEach-Object { "[event-contract] $_ -- fixture" })
    Assert-True ($record.state -ceq 'notMeasured' -and $record.reason -match 'ambiguous') "duplicate markers refuse measurement: $($pair -join ', ')"
}

$inlineSchema = '{"oneOf":[{"properties":{"kind":{"properties":{"type":{"const":"probe"}}}}}],"$defs":{"eventKind":{"oneOf":[{"properties":{"type":{"const":"probe"},"data":PAYLOAD}}]}}}'
foreach ($sample in @(
    @{Ref='payload.json'; Resource='{"type":"string"}'; Inline='{"type":"string"}'},
    @{Ref='payload.json#/$defs/x'; Resource='{"$defs":{"x":{"type":"string"}}}'; Inline='{"type":"string"}'},
    @{Ref='payload.json#/$defs/x'; Resource='{"$defs":{"x":{"$ref":"#/$defs/y"},"y":{"type":"string"}}}'; Inline='{"type":"string"}'},
    @{Ref='payload.json'; Resource='false'; Inline='false'}
)) {
    $resource = $sample.Resource
    $reader = {param($reference) $resource}.GetNewClosure()
    $externalSchema = $inlineSchema.Replace('PAYLOAD', ('{"$ref":"' + $sample.Ref + '"}'))
    $snapshot = New-ContractSnapshot -SchemaText $externalSchema -ExternalResolver $reader
    $externalKinds = Get-DeclaredKinds -Snapshot $snapshot
    $inlineKinds = Get-DeclaredKinds -SchemaText $inlineSchema.Replace('PAYLOAD',$sample.Inline)
    $verdict = @(Get-ContractVerdicts -Declarations @(
        (New-Declaration -Branch 'HEAD' -Tag 'probe' -Canonical $externalKinds['probe']),
        (New-Declaration -Branch 'other' -Tag 'probe' -Canonical $inlineKinds['probe'])
    ) -IsAncestor (New-Oracle))
    Assert-True ($externalKinds['probe'] -ceq $inlineKinds['probe'] -and $verdict[0].Category -cne 'COLLISION') "snapshot and classifier agree for external target $($sample.Ref) = $resource"
}
$siblingSnapshot = New-ContractSnapshot -SchemaText '{"$ref":"payload.json","maxLength":2}' -ExternalResolver {param($r) '{"type":"string"}'}
$siblingNode = $siblingSnapshot.SchemaText | ConvertFrom-Json
$script:CanonicalNodes=0
$siblingCanonical = ConvertTo-ContractCanonical -Node $siblingNode -Definitions $siblingNode
Assert-True ($siblingCanonical.Contains('"maxLength":2') -and $siblingCanonical.Contains('"$ref":')) 'expanded reference keeps semantic wrapper siblings'
$literalNode = '{"properties":{"$ref":{"type":"string"}},"const":{"$ref":{"type":"number"}}}' | ConvertFrom-Json
$script:CanonicalNodes=0
$literalCanonical = ConvertTo-ContractCanonical -Node $literalNode -Definitions $literalNode
Assert-True ($literalCanonical.Contains('"properties":{"$ref":') -and $literalCanonical.Contains('"const":{"$ref":')) 'reference-shaped map entries and literal values retain their keys'

# THE MERGE-BASE CONTRACT IS READ ONCE PER FORK POINT, NOT ONCE PER HEAD.
#
# Reading one is a `merge-base` sha, a blob, and every `$ref` under it expanded. Measured on this
# repository: ~10.5 s per touched head, 14 touched heads, ~147 s of a 197.8 s sweep -- the dominant
# cost of the stage, and the sweep runs twice. Branches cut from the same point have the SAME fork
# contract by construction, so recomputing it per head is pure repetition: of 14 touched heads here,
# only 10 fork points were distinct.
#
# A SLOW SUITE IS NOT RED, which is why this cell exists at all. Nothing else in this file would
# notice the memo being dropped.
$memoFixture = Join-Path ([IO.Path]::GetTempPath()) ('ecs-forkmemo-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($memoFixture)
$memoRepo = Join-Path $memoFixture 'work'
$memoBare = Join-Path $memoFixture 'remote.git'
$memoHookDir = Join-Path $memoFixture 'hooks'
[void][IO.Directory]::CreateDirectory($memoHookDir)
function Invoke-MemoFixtureGit {
    param([string[]]$GitArgs)
    $out = @(& git -c commit.gpgsign=false -c core.hooksPath=$memoHookDir -c user.name=Fixture -c user.email=fixture@example.invalid @GitArgs 2>&1)
    if ($LASTEXITCODE -ne 0) { throw "memo fixture git failed: $($out -join ' ')" }
}
try {
    Invoke-MemoFixtureGit @('init','--bare','-q',$memoBare)
    Invoke-MemoFixtureGit @('init','-q','-b','main',$memoRepo)
    [void][IO.Directory]::CreateDirectory((Join-Path $memoRepo 'schemas'))
    $memoEnvelope = Join-Path $memoRepo 'schemas/event-envelope.schema.json'
    $memoPayload = Join-Path $memoRepo 'schemas/checked-target.schema.json'
    [IO.File]::WriteAllText($memoEnvelope, $schemaExternalRef)
    [IO.File]::WriteAllText($memoPayload, '{"type":"string"}')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'add','.')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'commit','-qm','baseline')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'remote','add','origin',$memoBare)
    Invoke-MemoFixtureGit @('-C',$memoRepo,'push','-q','origin','main')
    # THREE heads cut from ONE commit, each touching the contract differently.
    foreach ($pair in @(@('m-a','{"type":"number"}'), @('m-b','{"type":"boolean"}'), @('m-c','{"type":"integer"}'))) {
        Invoke-MemoFixtureGit @('-C',$memoRepo,'checkout','-qb',$pair[0],'main')
        [IO.File]::WriteAllText($memoPayload, $pair[1])
        Invoke-MemoFixtureGit @('-C',$memoRepo,'commit','-qam',("payload " + $pair[0]))
        Invoke-MemoFixtureGit @('-C',$memoRepo,'push','-q','origin',$pair[0])
    }
    # Move main, then cut a FOURTH head from the new tip: a second, genuinely distinct fork point.
    Invoke-MemoFixtureGit @('-C',$memoRepo,'checkout','-q','main')
    [IO.File]::WriteAllText((Join-Path $memoRepo 'unrelated.txt'), 'moves main without touching the contract')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'add','.')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'commit','-qm','advance main')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'push','-q','origin','main')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'checkout','-qb','m-d','main')
    [IO.File]::WriteAllText($memoPayload, '{"type":"null"}')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'commit','-qam','payload m-d')
    Invoke-MemoFixtureGit @('-C',$memoRepo,'push','-q','origin','m-d')

    $memoForks = @(@('m-a','m-b','m-c','m-d') | ForEach-Object {
            (@(& git -C $memoRepo merge-base 'origin/main' $_ 2>$null) | Select-Object -First 1)
        })
    Assert-True (@($memoForks | Sort-Object -Unique).Count -eq 2) `
        "ARRANGEMENT: the four heads share exactly two fork points (found $(@($memoForks | Sort-Object -Unique).Count)), or the count below measures nothing"

    $readsBefore = [int] (Get-Variable -Scope Script -Name ForkContractReads).Value
    $memoPopulation = Get-ContractPopulation -Root $memoRepo -BudgetSeconds 120
    $readsAfter = [int] (Get-Variable -Scope Script -Name ForkContractReads).Value

    Assert-True ($memoPopulation.Known -and @($memoPopulation.Touched).Count -eq 4) `
        "ARRANGEMENT: all four heads are touched and the population is Known (touched=$(@($memoPopulation.Touched).Count), Known=$($memoPopulation.Known)), or the delta below is about heads that never took the expensive path"

    # NOT `-le 4`: an upper bound is satisfied by doing the work four times. The number IS the claim.
    Assert-True (($readsAfter - $readsBefore) -eq 2) `
        "the fork contract is read once per FORK POINT: 4 touched heads over 2 fork points cost $($readsAfter - $readsBefore) read(s), expected 2"
} finally {
    $resolvedMemo = [IO.Path]::GetFullPath($memoFixture)
    $memoParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolvedMemo.StartsWith($memoParent,[StringComparison]::OrdinalIgnoreCase)) { throw 'unsafe memo fixture cleanup path' }
    Remove-Item -LiteralPath $resolvedMemo -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
foreach ($__reason in @($script:skipReasons)) {
    $__ok = $false
    foreach ($__allowed in $AllowedSkipReasons) { if ($__reason -like $__allowed) { $__ok = $true; break } }
    if (-not $__ok) {
        Write-Host "HARNESS-BROKE: a check was SKIPPED with an undeclared reason, which is how a real assertion gets downgraded to a skip without changing any count: $__reason" -ForegroundColor Red
        exit 2
    }
}
if (($script:total + $script:skipped) -ne $ExpectedAssertionCount) {
    Write-Host "INCOMPLETE: ran $script:total assertions and skipped $script:skipped, expected population $ExpectedAssertionCount" -ForegroundColor Yellow
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total of $ExpectedAssertionCount ($script:skipped skipped)" -ForegroundColor Green
exit 0
