# #205: THE EVENT CONTRACT IS COMPARED ACROSS OPEN BRANCHES, NOT ONLY WITHIN ONE.
#
# Every instrument this project has for the event contract is intra-branch. The schema-parity
# checkers compare `event.rs` against `schemas/event-envelope.schema.json` within one checkout,
# `cargo check` compiles one tree, the gate runs one branch. Two branches can therefore declare the
# SAME wire tag with DIFFERENT payloads, and nothing looks at both. It is not a merge conflict: the
# declarations sit in different regions of one file and git merges them cleanly. The damage is
# irreversible after the merge -- `validate_envelope` runs on READ as well as on append, both
# payloads carry `additionalProperties: false`, so a journal written under one shape fails to replay
# under the other, permanently, with a bare `Invalid`.
#
# So this sweep reads every open branch of the remote at once and judges the branch it runs on:
#
#   POPULATION  `git ls-remote --heads <remote>`, DERIVED and never written by hand, filtered to the
#               heads whose diff against their own merge base with `<base>` touches the envelope
#               schema or its checked-in reference dependencies (three-dot: a head that merely predates a change on main did not touch it).
#   DECLARATION one wire tag on one head whose payload definition is ABSENT from that head's
#               merge base with `<base>` or DIFFERS from it. Stale unchanged tags are not declarations.
#   COMPARISON  structural, never textual: the top-level `oneOf` variant (tag, scope) TOGETHER
#               with the tag's `$defs.eventKind.oneOf` entry (the `data` payload reference), every
#               `$ref` expanded, keys sorted -- so key order and formatting cannot manufacture a
#               difference, and a change inside the payload definition cannot hide one.
#   ANCESTRY    two refs are ONE lineage when one is an ancestor of the other (a preserved branch
#               and its descendant, a PR ref and its head). Collapsed BEFORE anything is called a
#               duplicate, because the natural remedy for a reported duplicate is to delete a side.
#   CATEGORIES  COLLISION       same tag, divergent payloads, independent lineages  -> refused
#               RE-DECLARATION  same tag, identical payload, independent lineages  -> informational
#               SINGLE          one lineage declares it                           -> quiet
#
# Only the branch being gated is refused, and only for a collision IT is party to. A fight between
# two other branches is listed, so the reader learns of it, and it reddens their gates, not this
# one; a stage that any pair of strangers can turn red would be routed around within a week.
#
# DECLARED LIMIT: a COPY -- content lifted from another branch, no ancestry -- is indistinguishable
# from independent agreement by any tree-based instrument (#205, second finding). It reads here as
# RE-DECLARATION when identical and COLLISION when the original has since moved; the sentence that
# resolves it ("these kinds belong to <branch>, this branch follows") is a declaration no measurement
# can produce, and this script does not pretend to.
#
# DECLARED LIMIT (host): Windows PowerShell 5.1's ConvertFrom-Json refuses an object whose keys differ
# only by case, so a schema carrying such keys reads as UNKNOWN on this host rather than as a value.
#
# Exit codes are the contract of the STANDALONE run: 0 clean, 1 this head is party to a collision,
# 2 the sweep could not vouch for its own reading (no remote answer, an unreadable schema). The
# third is NOT the first. The GATE never runs this script directly: it runs
# `ci/event-contract-sweep.tests.ps1` inside the `ci powershell suites` stage, and there a reading
# the sweep could not make is `[event-contract] NOT MEASURED -- <reason>` and THE SUITE exits 0 --
# this script, run standalone, exits 2 for the same reading (the `exit 2` right after that Write-Host); the 0 is the suite's, and
# a reader who grepped this file for `NOT MEASURED` beside `exit 0` has been misled by this very
# sentence once already. It is a fact about the INSTRUMENT, carried into the manifest as `eventContractSweep`, never a red for a tree whose
# code the property does not describe. A real collision is `[event-contract] COLLISION ...` and the
# suite's own assertion reddens. Measured 2026-09-08: suite with the remote mute -> PASSED, exit 0;
# `ci/run-ps-suites.ps1` with the same -> exit 0. Re-measured 2026-09-19 on #1005 after the suite
# grew: the UNKNOWN branch must run the SAME number of assertions as the Known branch, or a mute
# remote makes the suite INCOMPLETE (exit 2) and run-ps-suites turns that into HARNESS-BROKE -- a
# RED for every PR. And UNKNOWN is not only an outage state: on this box under ordinary parallel
# load the 20 s fetch cap is reached against a HEALTHY remote (lane A, 534 s run, "git fetch origin
# did not finish within 20 s"), so the symmetric count is load-bearing every busy evening.
param(
    [string] $RepoRoot = (Get-Location).Path,
    [string] $Remote = 'origin',
    [string] $Base = 'origin/main',
    [string] $SchemaPath = 'schemas/event-envelope.schema.json',
    # 600, not 300, and the number is arithmetic rather than taste. Measured on #1005 (lane B, 30/30
    # sampled): T ~= 13 s + 0.208 s per UNCHANGED head + 7.9 s per CHANGED head. At today's 302
    # heads and 15 changed the sweep takes ~170-190 s; at 300 s the ceiling was crossed at ~29
    # changed heads, i.e. fourteen more schema-touching branches -- and past the ceiling this is not
    # slow, it is NOT MEASURED, which merge-proof (retired 2026-09-24) turned into a refusal of
    # EVERY merge. 600 s moves the cliff to ~67 changed heads. The sensitive axis is changed heads,
    # which the cheap diff does not help; a per-changed-head cost below 7.9 s is the real fix and is
    # not in this change.
    #
    # THE TRADE, NAMED: 600 buys headroom in changed heads and raises the worst-case wall clock of
    # the `ci powershell suites` stage, which #1053 is trying to shrink. What it does NOT do is let a
    # mute or slow remote burn ten minutes: the fetch and the listing each run under Get-RemainingSeconds
    # with no -Cap, i.e. the 20 s $TimeoutSeconds default, and return NOT MEASURED on their own. The
    # budget is only ever spent by HEADS -- real work -- never by waiting on a remote.
    #
    # And the cheap diff's directory pathspec degrades in the SAFE direction: churn anywhere under
    # schemas/ marks more heads as changed, so the 287-of-302 skip rate falls toward examining more
    # heads, never toward skipping one (eloquent-jones on #1005, fifteen hostile paths, none escaped).
    [int] $BudgetSeconds = 600
)

Set-StrictMode -Version 2.0

$script:ContractSweepDotSourced = $MyInvocation.InvocationName -eq '.'
# How many `merge-base` probes this process has made; a suite reads it to prove none happen after a
# population has been returned.
$script:AncestryGitCalls = 0
# How many times the merge-base CONTRACT was read and canonicalised from scratch. One read is a
# `merge-base` sha, a blob, and every `$ref` under it expanded -- measured at ~10.5 s per touched
# head on this repository, and the dominant cost of a sweep (14 touched heads, ~147 s of a 197.8 s
# run). A suite reads this to prove heads that share a fork point pay for it ONCE.
$script:ForkContractReads = 0
# TOTAL WORK BUDGET for one schema's canonicalisation. Depth alone does not bound an acyclic schema:
# six definitions each `allOf` of eight references to the next expand 8^6 times under depth 48
# (Codex P1 on #1005). Past this many nodes the reading is refused (UNKNOWN), never computed to
# exhaustion inside the authoritative gate. The checked-in envelope costs a few thousand.
$script:MaxCanonicalNodes = 100000
$script:CanonicalNodes = 0

<#
.SYNOPSIS
    One JSON node as a canonical string: object keys sorted, `$ref`s into `#/$defs/` expanded in
    place, scalars as JSON. Two schemas that mean the same payload produce the same string.
#>
<#
.SYNOPSIS
    Resolves an internal JSON Pointer (`#/a/b~1c`) against the whole document: `~1` is `/`, `~0`
    is `~`, and the pointer may be deeper than one segment. `$null` when any segment is missing.
#>
function Find-Anchor {
    param($Node, [string] $Name, [int] $Depth = 0)
    if ($Depth -gt 64 -or $null -eq $Node -or $Node -is [string] -or $Node -is [ValueType]) { return $null }
    $script:CanonicalNodes += 1
    if ($script:CanonicalNodes -gt $script:MaxCanonicalNodes) {
        throw [System.InvalidOperationException] "anchor lookup work exceeded $script:MaxCanonicalNodes nodes"
    }
    if ($Node -is [array]) {
        foreach ($item in $Node) { $hit = Find-Anchor -Node $item -Name $Name -Depth ($Depth + 1); if ($null -ne $hit) { return $hit } }
        return $null
    }
    # `$anchor` and `$dynamicAnchor` alike: within ONE document a dynamic reference resolves to the
    # dynamic anchor in scope, and this sweep canonicalises one document at a time (Codex P1 on #1005).
    foreach ($kind in @('$anchor', '$dynamicAnchor')) {
        $anchor = $Node.PSObject.Properties[$kind]
        if ($null -ne $anchor -and [string]::Equals([string] $anchor.Value, $Name, [System.StringComparison]::Ordinal)) { return [pscustomobject]@{ Value = $Node } }
    }
    # Only schema-bearing keyword values are traversed. `const`, `default`, and `enum` contain
    # instance JSON, where a member named `$anchor` is data rather than a schema anchor (Codex P1
    # on #1005). Unknown extension keywords are deliberately opaque: guessing that their values are
    # schemas would let ordinary data shadow a real anchor.
    $schemaMaps = @('properties', 'patternProperties', '$defs', 'definitions', 'dependentSchemas')
    $schemaSingles = @('additionalProperties', 'propertyNames', 'contains', 'contentSchema', 'if', 'then', 'else', 'not', 'items', 'additionalItems', 'unevaluatedItems', 'unevaluatedProperties')
    $schemaArrays = @('prefixItems', 'allOf', 'anyOf', 'oneOf')
    foreach ($member in @($Node.PSObject.Properties)) {
        if ($schemaMaps -ccontains $member.Name) {
            if ($member.Value -is [object]) {
                foreach ($entry in @($member.Value.PSObject.Properties)) {
                    $hit = Find-Anchor -Node $entry.Value -Name $Name -Depth ($Depth + 1)
                    if ($null -ne $hit) { return $hit }
                }
            }
        } elseif ($schemaArrays -ccontains $member.Name -and $member.Value -is [array]) {
            foreach ($item in $member.Value) {
                $hit = Find-Anchor -Node $item -Name $Name -Depth ($Depth + 1)
                if ($null -ne $hit) { return $hit }
            }
        } elseif ($schemaSingles -ccontains $member.Name) {
            $hit = Find-Anchor -Node $member.Value -Name $Name -Depth ($Depth + 1)
            if ($null -ne $hit) { return $hit }
        }
    }
    return $null
}

function Resolve-InternalPointer {
    param($Root, [string] $Pointer)
    # `#name` is a plain-name fragment resolved by `$anchor` (2020-12 §8.2.2), not a JSON Pointer
    # (Codex P1 on #1005); `#/...` is a pointer. Anything else is external and stays unexpanded.
    if ($null -eq $Root -or -not $Pointer.StartsWith('#')) { return $null }
    if (-not $Pointer.StartsWith('#/')) {
        $name = $Pointer.Substring(1)
        if ([string]::IsNullOrEmpty($name)) { return [pscustomobject]@{ Value = $Root } }
        return Find-Anchor -Node $Root -Name $name
    }
    $node = $Root
    foreach ($raw in ($Pointer.Substring(2) -split '/')) {
        $token = $raw.Replace('~1', '/').Replace('~0', '~')
        if ($node -is [array]) {
            $index = 0
            if (-not [int]::TryParse($token, [ref] $index) -or $index -lt 0 -or $index -ge $node.Count) { return $null }
            $node = $node[$index]
            continue
        }
        if ($null -eq $node -or $node -is [string] -or $node -is [ValueType]) { return $null }
        $member = $node.PSObject.Properties[$token]
        if ($null -eq $member) { return $null }
        $node = $member.Value
    }
    return [pscustomobject]@{ Value = $node }
}

function Get-ReferencedDefinitions {
    param($Node, [int] $Depth = 0, [switch] $SkipDefinitionMaps)
    if ($Depth -gt 64 -or $null -eq $Node -or $Node -is [string] -or $Node -is [ValueType]) { return @() }
    if ($Node -is [array]) {
        $found = foreach ($item in $Node) { Get-ReferencedDefinitions -Node $item -Depth ($Depth + 1) -SkipDefinitionMaps:$SkipDefinitionMaps }
        return [string[]] @($found)
    }
    $found = @()
    foreach ($member in @($Node.PSObject.Properties)) {
        if ($member.Name -ceq 'const' -or $member.Name -ceq 'default' -or $member.Name -ceq 'enum' -or $member.Name -ceq 'examples') { continue }
        if ($SkipDefinitionMaps -and ($member.Name -ceq '$defs' -or $member.Name -ceq 'definitions')) { continue }
        if ($member.Name -ceq '$ref' -or $member.Name -ceq '$dynamicRef') {
            $pointer = [string] $member.Value
            if ($pointer.StartsWith('#/$defs/') -or $pointer.StartsWith('#/definitions/')) {
                $name = ($pointer.Substring($pointer.IndexOf('/', 2) + 1) -split '/')[0]
                $found += $name.Replace('~1', '/').Replace('~0', '~')
            }
        }
        $found += Get-ReferencedDefinitions -Node $member.Value -Depth ($Depth + 1) -SkipDefinitionMaps:$SkipDefinitionMaps
    }
    return [string[]] @($found)
}

# Capture is separate from canonicalization: readers finish before an immutable JSON snapshot is
# handed to the pure serializer. Unsupported nested external bases remain UNKNOWN, not guesses.
function Get-ExternalReferenceValues {
    param([Parameter(Mandatory)] [string] $SchemaText)
    try {
        $schema = ConvertFrom-Json -InputObject $SchemaText -ErrorAction Stop
        $refs = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
        $state = @{ Nodes = 0 }
        $walk = {
            param($Node, [int]$Depth, [bool]$IsMap, [bool]$Literal)
            $state.Nodes++
            if ($Depth -gt 64 -or $state.Nodes -gt 100000) { throw 'reference enumeration limit exceeded' }
            if ($Literal -or $null -eq $Node -or $Node -is [string] -or $Node -is [ValueType]) { return }
            if ($Node -is [array]) { foreach($item in $Node) { & $walk $item ($Depth+1) $false $false }; return }
            foreach($member in @($Node.PSObject.Properties)) {
                if (-not $IsMap -and $member.Name -cin @('description','title','$comment','examples','deprecated','readOnly','writeOnly')) { continue }
                if (-not $IsMap -and $member.Name -cin @('$ref','$dynamicRef')) {
                    if ($member.Value -isnot [string]) { throw 'reference is not a string' }
                    if (-not $member.Value.StartsWith('#')) { [void]$refs.Add($member.Value) }
                }
                $literal = -not $IsMap -and $member.Name -cin @('const','default','enum','dependentRequired')
                $map = -not $IsMap -and $member.Name -cin @('properties','patternProperties','$defs','definitions','dependentSchemas')
                & $walk $member.Value ($Depth+1) $map $literal
            }
        }
        & $walk $schema 0 $false $false
        $ordered = [string[]]@($refs); [Array]::Sort($ordered,[StringComparer]::Ordinal)
        return ,$ordered
    } catch { return $null }
}

function Get-ExternalReferencePaths {
    param([Parameter(Mandatory)] [string] $SchemaText, [Parameter(Mandatory)] [string] $SchemaPath)
    $refs = Get-ExternalReferenceValues -SchemaText $SchemaText
    if ($null -eq $refs) { return $null }
    $paths = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
    foreach($reference in $refs) {
        $relative = $reference.Split('#')[0]
        if ([string]::IsNullOrEmpty($relative) -or $relative -match '^[A-Za-z][A-Za-z0-9+.-]*:' -or $relative.StartsWith('/') -or $relative.StartsWith('\') -or $relative -match '(^|[/\\])\.\.([/\\]|$)') { return $null }
        [void]$paths.Add((Join-Path (Split-Path -Parent $SchemaPath) $relative).Replace('\','/'))
        if ($paths.Count -gt 128) { return $null }
    }
    $ordered = [string[]]@($paths); [Array]::Sort($ordered,[StringComparer]::Ordinal)
    return ,$ordered
}

function Expand-ExternalContractReferences {
    param($Node, $Definitions, $Documents, $Work, [int]$Depth=0, [switch]$IsMap, [switch]$Literal, [switch]$ResolveLocal, [string]$DocumentPath='')
    $Work.Nodes++
    if ($Depth -gt 48 -or $Work.Nodes -gt 100000) { throw 'reference expansion limit exceeded' }
    if ($Literal -or $null -eq $Node -or $Node -is [string] -or $Node -is [ValueType]) { return ,$Node }
    if ($Node -is [array]) {
        $items = @(foreach($item in $Node) { Expand-ExternalContractReferences -Node $item -Definitions $Definitions -Documents $Documents -Work $Work -Depth ($Depth+1) -ResolveLocal:$ResolveLocal -DocumentPath $DocumentPath })
        return ,$items
    }
    $result = [ordered]@{}
    foreach($member in @($Node.PSObject.Properties)) {
        $value = $member.Value
        if (-not $IsMap -and $member.Name -cin @('$ref','$dynamicRef')) {
            if ($value -isnot [string]) { throw 'reference is not a string' }
            if ($value.StartsWith('#')) {
                if ($ResolveLocal) {
                    $target = Resolve-InternalPointer -Root $Definitions -Pointer $value
                    if ($null -eq $target) { throw 'external resource local fragment missing' }
                    $value = Expand-ExternalContractReferences -Node $target.Value -Definitions $Definitions -Documents $Documents -Work $Work -Depth ($Depth+1) -ResolveLocal -DocumentPath $DocumentPath
                }
            } else {
                # `$ResolveLocal` was doing two jobs with one flag: "local fragments resolve against the
                # EXTERNAL root" -- which is what :255 sets it for -- and, as an accident of sitting in
                # this condition, "no further external reference may be followed from here". Only the
                # first is intended. The second made a nested external `$ref` unresolvable, and both
                # artifact-reference.schema.json and persisted-graph-version.schema.json carry one, so
                # the envelope never expanded and the whole sweep published NOT MEASURED.
                #
                # A reference is resolvable iff its document is loaded. That is the only question here.
                # Cycles are not newly possible -- they are bounded by the Depth/Nodes caps above, which
                # throw rather than hang.
                #
                # AND IT IS RESOLVED AGAINST THE DOCUMENT THAT CONTAINS IT, not against the envelope. The
                # first version of this loosening looked the raw string up in `$Documents`, which is keyed by
                # the ENVELOPE's own reference strings -- so `other.json` inside `sub/t.schema.json` found
                # the envelope's `other.json` and silently produced the wrong document (lane A on #1005,
                # before/after fixture). Every schema is flat today, which is the only reason that read
                # as correct. `$DocumentPath` is the containing document's path ('' at the envelope), the
                # relative part is joined under its directory with the same refusals :218 applies at the
                # top level, and the loaded document is found by PATH -- keys may carry a fragment.
                $hash = $value.IndexOf('#')
                $relative = if ($hash -ge 0) { $value.Substring(0, $hash) } else { $value }
                if ([string]::IsNullOrEmpty($relative) -or $relative -match '^[A-Za-z][A-Za-z0-9+.-]*:' -or $relative.StartsWith('/') -or
                    $relative.StartsWith('\') -or ($relative.Split([char[]]@('/', '\')) -ccontains '..')) { throw 'external resource base unavailable' }
                $baseDirectory = if ([string]::IsNullOrEmpty($DocumentPath)) { '' } else { Split-Path -Parent $DocumentPath }
                $resolvedPath = if ([string]::IsNullOrEmpty($baseDirectory)) { $relative } else { (Join-Path $baseDirectory $relative).Replace('\', '/') }
                $documentKey = $null
                foreach ($candidate in $Documents.Keys) {
                    $candidatePath = $candidate.Split('#')[0]
                    if ([string]::Equals($candidatePath, $resolvedPath, [System.StringComparison]::Ordinal)) { $documentKey = $candidate; break }
                }
                if ($null -eq $documentKey) { throw 'external resource base unavailable' }
                $externalRoot = ConvertFrom-Json -InputObject $Documents[$documentKey] -ErrorAction Stop
                $externalTarget = $externalRoot
                $hash = $value.IndexOf('#')
                if ($hash -ge 0) {
                    $fragment = [Uri]::UnescapeDataString($value.Substring($hash))
                    $target = Resolve-InternalPointer -Root $externalRoot -Pointer $fragment
                    if ($null -eq $target) { throw 'external resource fragment missing' }
                    $externalTarget = $target.Value
                }
                $value = Expand-ExternalContractReferences -Node $externalTarget -Definitions $externalRoot -Documents $Documents -Work $Work -Depth ($Depth+1) -ResolveLocal -DocumentPath $resolvedPath
            }
            $result[$member.Name] = $value
            continue
        }
        $literal = -not $IsMap -and $member.Name -cin @('const','default','enum','dependentRequired')
        $map = -not $IsMap -and $member.Name -cin @('properties','patternProperties','$defs','definitions','dependentSchemas')
        $result[$member.Name] = Expand-ExternalContractReferences -Node $value -Definitions $Definitions -Documents $Documents -Work $Work -Depth ($Depth+1) -IsMap:$map -Literal:$literal -ResolveLocal:$ResolveLocal -DocumentPath $DocumentPath
    }
    return [pscustomobject]$result
}

function New-ContractSnapshot {
    param([Parameter(Mandatory)] [string]$SchemaText, [scriptblock]$ExternalResolver)
    try {
        $refs = Get-ExternalReferenceValues -SchemaText $SchemaText
        if ($null -eq $refs -or $refs.Count -gt 128) { return $null }
        $documents = New-OrdinalMap
        $bytes = [Text.Encoding]::UTF8.GetByteCount($SchemaText)
        foreach($reference in $refs) {
            if ($null -eq $ExternalResolver) { return $null }
            $text = & $ExternalResolver $reference
            if ($null -eq $text -or [Text.Encoding]::UTF8.GetByteCount([string]$text) -gt $script:MaxSchemaBlobBytes) { return $null }
            $bytes += [Text.Encoding]::UTF8.GetByteCount([string]$text)
            if ($bytes -gt 16MB) { return $null }
            $documents[$reference] = [string]$text
        }
        # With no external resources there is nothing to expand; preserve the original local
        # reference model for the pure canonicalizer and avoid a second JSON serialization.
        if ($refs.Count -eq 0) { return [pscustomobject]@{SchemaText=$SchemaText} }
        $root = ConvertFrom-Json -InputObject $SchemaText -ErrorAction Stop
        $expanded = Expand-ExternalContractReferences -Node $root -Definitions $root -Documents $documents -Work @{Nodes=0}
        return [pscustomobject]@{SchemaText=(ConvertTo-Json -InputObject $expanded -Depth 100 -Compress)}
    } catch { return $null }
}


function Get-ReachableDefinitionNames {
    param($Parent, $DefinitionMap)
    $reachable = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::Ordinal)
    $pending = [System.Collections.Generic.Queue[string]]::new()
    foreach ($name in @(Get-ReferencedDefinitions -Node $Parent -SkipDefinitionMaps)) { $pending.Enqueue([string] $name) }
    while ($pending.Count -gt 0) {
        $name = $pending.Dequeue()
        if (-not $reachable.Add($name)) { continue }
        $entry = $DefinitionMap.PSObject.Properties[$name]
        if ($null -ne $entry) {
            foreach ($nested in @(Get-ReferencedDefinitions -Node $entry.Value)) { $pending.Enqueue([string] $nested) }
        }
    }
    return [string[]] @($reachable | ForEach-Object { [string] $_ })
}

function ConvertTo-ContractCanonical {
    # `$Definitions` is the WHOLE document (kept under its old name for the callers); `-IsMap` says
    # this node is a map whose keys are NAMES -- `properties`, `patternProperties`, `$defs`,
    # `definitions`, `dependentSchemas` -- so annotation filtering does not apply to its keys and
    # each value is a schema again. A payload FIELD named `description` is contract; the keyword
    # `description` on a schema is not (Codex P1 on #1005).
    param($Node, $Definitions, [int] $Depth = 0, [switch] $IsMap, [switch] $Literal, [switch] $SetMembers)
    # A self-referential definition would expand forever. Past this depth the reading is REFUSED:
    # substituting a literal here made two schemas that differ only below the bound canonicalise
    # identically (Codex P2 on #1005), and an equal value nobody computed is the false negative this
    # sweep exists to avoid. Real variants are a handful of levels deep.
    if ($Depth -gt 48) { throw [System.InvalidOperationException] 'canonicalisation depth exceeded: a reference cycle or a schema deeper than 48 levels' }
    $script:CanonicalNodes += 1
    if ($script:CanonicalNodes -gt $script:MaxCanonicalNodes) { throw [System.InvalidOperationException] "canonicalisation work exceeded $script:MaxCanonicalNodes nodes: an exponentially expanding schema" }
    if ($null -eq $Node) { return 'null' }
    if ($Node -is [string]) { return (ConvertTo-Json -InputObject $Node -Compress) }
    if ($Node -is [bool] -or $Node -is [int] -or $Node -is [long] -or $Node -is [double] -or $Node -is [decimal]) {
        return (ConvertTo-Json -InputObject $Node -Compress)
    }
    if ($Node -is [array]) {
        $items = foreach ($item in $Node) { ConvertTo-ContractCanonical -Node $item -Definitions $Definitions -Depth ($Depth + 1) -Literal:$Literal }
        return '[' + (@($items) -join ',') + ']'
    }
    # LITERAL JSON: the value of `const`, each `enum` member and `default` are DATA, not schema.
    # Every member name stays (a member named `description` is data), no `$ref` is expanded, and
    # arrays keep their order; only object key order is normalised (Codex P1 on #1005).
    if ($Literal) {
        $lit = New-Object 'System.Collections.Generic.Dictionary[string,object]' ([System.StringComparer]::Ordinal)
        foreach ($member in @($Node.PSObject.Properties)) { $lit[[string] $member.Name] = $member.Value }
        $litNames = [string[]] @($lit.Keys)
        [Array]::Sort($litNames, [System.StringComparer]::Ordinal)
        $litParts = foreach ($name in $litNames) {
            (ConvertTo-Json -InputObject $name -Compress) + ':' + (ConvertTo-ContractCanonical -Node $lit[$name] -Definitions $Definitions -Depth ($Depth + 1) -Literal)
        }
        return '{' + (@($litParts) -join ',') + '}'
    }
    # ANNOTATIONS ARE NOT CONTRACT. `description`, `title`, `$comment`, `examples`, `deprecated`,
    # `readOnly`, `writeOnly` change no instance's validity (JSON Schema 2020-12 §9), so two branches
    # that differ only there accept the same journals and must not collide (Codex P2 on #1005).
    # `default` is deliberately NOT here: it is an annotation the runtime may act on.
    $annotations = @('description', 'title', '$comment', 'examples', 'deprecated', 'readOnly', 'writeOnly')
    $maps = @('properties', 'patternProperties', '$defs', 'definitions', 'dependentSchemas', 'dependentRequired')
    # ORDINAL key order: `Sort-Object` is culture-aware and case-insensitive, so `Foo`/`foo` kept
    # their source order and the same object canonicalised two ways (Codex P2 on #1005).
    # ... and `PSObject.Properties[$name]` is case-insensitive too (it answered `Foo` for `foo`),
    # so the members are copied into an ordinal dictionary first and read back by exact name.
    # `@( if ... )`: an `if` EXPRESSION collapses a one-element array to the element and `.Count`
    # then throws under StrictMode (the same PS 5.1 trap E hit on #1019).
    $unsorted = @(if ($IsMap) { $Node.PSObject.Properties } else { $Node.PSObject.Properties | Where-Object { $annotations -cnotcontains $_.Name } })
    # A SOLE `$ref` (after annotations) IS its target: a payload written inline and the same payload
    # factored into `$defs` accept the same journals and must be one contract (Codex P1 on #1005).
    # With siblings, the target is serialised under the `$ref` key beside them (below).
    # Snapshot expansion has already replaced external reference strings with their targets.
    # Substitute only a sole schema reference here; semantic siblings and literal/map contexts
    # retain their wrapper. Canonicalization performs no external read.
    if (-not $IsMap -and $unsorted.Count -eq 1 -and
        $unsorted[0].Name -cin @('$ref', '$dynamicRef') -and
        ($unsorted[0].Value -is [pscustomobject] -or $unsorted[0].Value -is [bool])) {
        return ConvertTo-ContractCanonical -Node $unsorted[0].Value -Definitions $Definitions -Depth ($Depth + 1)
    }
    if (-not $IsMap -and $unsorted.Count -eq 1 -and ($unsorted[0].Name -ceq '$ref' -or $unsorted[0].Name -ceq '$dynamicRef') -and $null -ne $Definitions -and ([string] $unsorted[0].Value).StartsWith('#')) {
        $sole = Resolve-InternalPointer -Root $Definitions -Pointer ([string] $unsorted[0].Value)
        if ($null -eq $sole) { throw [System.InvalidOperationException] "internal reference $($unsorted[0].Value) resolves to nothing" }
        return ConvertTo-ContractCanonical -Node $sole.Value -Definitions $Definitions -Depth ($Depth + 1)
    }
    $byName = New-Object 'System.Collections.Generic.Dictionary[string,object]' ([System.StringComparer]::Ordinal)
    foreach ($member in $unsorted) { $byName[[string] $member.Name] = [pscustomobject]@{ Name = [string] $member.Name; Value = $member.Value } }
    $names = [string[]] @($byName.Keys)
    [Array]::Sort($names, [System.StringComparer]::Ordinal)
    $properties = @(foreach ($name in $names) { $byName[$name] })
    $parts = foreach ($property in $properties) {
        $value = $property.Value
        if ($IsMap -and $SetMembers -and $value -is [array]) {
            $companions = [string[]] @($value | ForEach-Object { ConvertTo-ContractCanonical -Node $_ -Definitions $Definitions -Depth ($Depth + 1) })
            [Array]::Sort($companions, [System.StringComparer]::Ordinal)
            (ConvertTo-Json -InputObject ([string] $property.Name) -Compress) + ':[' + ($companions -join ',') + ']'
            continue
        }
        if (-not $IsMap -and ($maps -ccontains $property.Name) -and $null -ne $value -and $value -isnot [string] -and $value -isnot [array] -and $value -isnot [ValueType]) {
            # `dependentRequired` members are ARRAYS OF NAMES, sets like `required` (Codex P2 on #1005).
            if ($property.Name -ceq '$defs' -or $property.Name -ceq 'definitions') {
                $selected = [ordered]@{}
                foreach ($name in @(Get-ReachableDefinitionNames -Parent $Node -DefinitionMap $value)) {
                    $entry = $value.PSObject.Properties[$name]
                    if ($null -ne $entry) { $selected[$name] = $entry.Value }
                }
                $value = [pscustomobject] $selected
            }
            (ConvertTo-Json -InputObject ([string] $property.Name) -Compress) + ':' + (ConvertTo-ContractCanonical -Node $value -Definitions $Definitions -Depth ($Depth + 1) -IsMap -SetMembers:($property.Name -ceq 'dependentRequired'))
            continue
        }
        # `$ref` is expanded IN PLACE whether or not it has siblings: draft 2020-12 allows a `$ref`
        # beside other keywords, and skipping the target when a sibling was present let two
        # branches share a reference and differ behind it (Codex P1 on #1005). The expanded target
        # is serialised under the `$ref` key, so the sibling keywords stay part of the value.
        if (-not $IsMap -and ($property.Name -ceq 'const' -or $property.Name -ceq 'default')) {
            (ConvertTo-Json -InputObject ([string] $property.Name) -Compress) + ':' + (ConvertTo-ContractCanonical -Node $value -Definitions $Definitions -Depth ($Depth + 1) -Literal)
            continue
        }
        if (-not $IsMap -and $property.Name -ceq 'enum' -and $value -is [array]) {
            $members = [string[]] @($value | ForEach-Object { ConvertTo-ContractCanonical -Node $_ -Definitions $Definitions -Depth ($Depth + 1) -Literal })
            [Array]::Sort($members, [System.StringComparer]::Ordinal)
            (ConvertTo-Json -InputObject 'enum' -Compress) + ':[' + ($members -join ',') + ']'
            continue
        }
        if (-not $IsMap -and ($property.Name -ceq '$ref' -or $property.Name -ceq '$dynamicRef') -and $null -ne $Definitions -and ([string] $value).StartsWith('#')) {
            # A full JSON Pointer against the whole document: `~1`/`~0` decoded, any depth
            # (Codex P1 on #1005). An internal pointer that resolves to nothing is REFUSED, not
            # left as a string two schemas could share: throw, and the schema reads UNKNOWN.
            $target = Resolve-InternalPointer -Root $Definitions -Pointer ([string] $value)
            if ($null -eq $target) { throw [System.InvalidOperationException] "internal reference $value resolves to nothing" }
            (ConvertTo-Json -InputObject '$ref' -Compress) + ':' + (ConvertTo-ContractCanonical -Node $target.Value -Definitions $Definitions -Depth ($Depth + 1))
            continue
        }
        if (-not $IsMap -and $property.Name -cin @('$ref', '$dynamicRef') -and $value -is [string] -and -not $value.StartsWith('#')) {
            throw 'external references must be expanded in the snapshot before canonicalization'
        }

        # the same instances, so they are sorted (ordinally) before they are compared, or two
        # branches agreeing on a payload would collide on its spelling (Codex P2 on #1005).
        # `oneOf` / `anyOf` / `allOf` alternatives are order-independent too (Codex P2 on #1005);
        # `prefixItems` and `items` are positional and stay ordered.
        # `type` in its array form is a set too (Codex P2 on #1005).
        if (($property.Name -ceq 'required' -or $property.Name -ceq 'enum' -or $property.Name -ceq 'oneOf' -or $property.Name -ceq 'anyOf' -or $property.Name -ceq 'allOf' -or $property.Name -ceq 'type') -and $value -is [array]) {
            $members = [string[]] @($value | ForEach-Object { ConvertTo-ContractCanonical -Node $_ -Definitions $Definitions -Depth ($Depth + 1) })
            [Array]::Sort($members, [System.StringComparer]::Ordinal)
            $serialized = '[' + ($members -join ',') + ']'
            (ConvertTo-Json -InputObject ([string] $property.Name) -Compress) + ':' + $serialized
            continue
        }
        (ConvertTo-Json -InputObject ([string] $property.Name) -Compress) + ':' +
            (ConvertTo-ContractCanonical -Node $value -Definitions $Definitions -Depth ($Depth + 1))
    }
    return '{' + (@($parts) -join ',') + '}'
}

<#
.SYNOPSIS
    The wire tags a schema declares, each mapped to the canonical form of its `oneOf` variant.
    `$null` when the text is not the envelope shape -- UNKNOWN is never zero kinds.
#>
function Get-DeclaredKinds {
    param([string] $SchemaText, [scriptblock] $ExternalResolver, $Snapshot)
    if ($null -eq $Snapshot) { $Snapshot = New-ContractSnapshot -SchemaText $SchemaText -ExternalResolver $ExternalResolver }
    if ($null -eq $Snapshot) { return $null }
    $SchemaText = $Snapshot.SchemaText
    $schema = $null
    try { $schema = ConvertFrom-Json -InputObject $SchemaText } catch { return $null }
    if ($null -eq $schema) { return $null }
    $variants = $schema.PSObject.Properties['oneOf']
    if ($null -eq $variants) { return $null }
    # The whole document is the resolution root for `$ref`s (kept under the name `$definitions`).
    $definitions = $schema
    # The top-level variant carries the tag and the scope. The PAYLOAD is selected separately, by
    # `$defs.eventKind.oneOf`, whose `data` references the payload definition -- so the payload
    # entry is part of what a tag DECLARES, and a canonical value that stopped at the variant was a
    # function of (tag, scope) that never saw the payload (C's block on #1005). A schema without
    # that second half is not the envelope shape, and reads as UNKNOWN.
    $kindEntries = New-OrdinalMap
    $eventKindContainerSiblings = [ordered]@{}
    try {
        $eventKindContainer = $schema.'$defs'.eventKind
        foreach ($containerMember in @($eventKindContainer.PSObject.Properties)) {
            # `oneOf` selects the tag below. Every other schema keyword on the eventKind
            # container constrains every selected kind and must remain in the conjunction.
            # Annotation keywords are deliberately omitted: the canonicalizer already treats
            # them as non-semantic, and wrapping an entry in an allOf with only annotations (or
            # an empty schema) would create a false structural difference.
            if ($containerMember.Name -cne 'oneOf' -and
                $containerMember.Name -cnotin @('description','title','$comment','examples','deprecated','readOnly','writeOnly')) {
                $eventKindContainerSiblings[$containerMember.Name] = $containerMember.Value
            }
        }
        foreach ($entry in @($eventKindContainer.oneOf)) {
            $entryTag = [string] $entry.properties.type.const
            if (-not [string]::IsNullOrEmpty($entryTag)) { $kindEntries[$entryTag] = $entry }
        }
    } catch { return $null }
    if ($kindEntries.Count -eq 0) { return $null }
    $kinds = New-OrdinalMap
    $script:CanonicalNodes = 0
    foreach ($variant in @($variants.Value)) {
        $tag = $null
        try { $tag = [string] $variant.properties.kind.properties.type.const } catch { $tag = $null }
        if ([string]::IsNullOrEmpty($tag)) { return $null }
        if (-not $kindEntries.ContainsKey($tag)) { return $null }
        $selectedKind = $kindEntries[$tag]
        if ($eventKindContainerSiblings.Count -gt 0) {
            $selectedKind = [pscustomobject]@{
                allOf = @([pscustomobject]$eventKindContainerSiblings, $kindEntries[$tag])
            }
        }
        # A oneOf branch is not the whole validation context. Root constraints still apply to
        # every instance, and a root `properties.kind` constraint can narrow the payload without
        # appearing in either the discriminator branch or the eventKind entry. Preserve the
        # applicable root keywords and combine the root kind schema with this tag's kind entry.
        $rootContext = [ordered]@{}
        # Preserve every root-level validation keyword that can constrain an envelope instance.
        # The root `oneOf` is the envelope's tag enumeration and is represented separately by
        # `envelope`; copying it into every per-tag contract would duplicate the selector rather
        # than preserve context.  `$defs`/`definitions` are declaration maps, not constraints.
        # Keep this list explicit: unknown extension members are annotations/data until a schema
        # vocabulary assigns them validation meaning.
        foreach ($rootKeyword in @(
                '$ref','$dynamicRef','type','enum','const','multipleOf','maximum','exclusiveMaximum',
                'minimum','exclusiveMinimum','maxLength','minLength','pattern','format','contentEncoding',
                'contentMediaType','contentSchema','maxItems','minItems','uniqueItems','maxContains',
                'minContains','items','prefixItems','contains','additionalItems','required','properties',
                'patternProperties','additionalProperties','propertyNames','minProperties','maxProperties',
                'dependentRequired','dependentSchemas','unevaluatedItems','unevaluatedProperties',
                'allOf','anyOf','not','if','then','else'
            )) {
            $rootProperty = $schema.PSObject.Properties[$rootKeyword]
            if ($null -ne $rootProperty -and $rootKeyword -cne 'properties') { $rootContext[$rootKeyword] = $rootProperty.Value }
        }
        $rootProperties = [ordered]@{}
        $schemaProperties = $schema.PSObject.Properties['properties']
        if ($null -ne $schemaProperties -and $null -ne $schemaProperties.Value) {
            foreach ($rootProperty in @($schemaProperties.Value.PSObject.Properties)) {
                if ($rootProperty.Name -ceq 'kind') {
                    $rootKind = $rootProperty.Value
                    $rootKindNames = @($rootKind.PSObject.Properties.Name)
                    # Only the direct canonical event-kind reference can be replaced by the
                    # tag-specific entry.  A sole `$ref` to a narrower definition is itself a
                    # contract constraint and must remain in the conjunction; treating every
                    # sole reference as the event-kind union erased that narrowing (P1 #1005).
                    $canonicalEventKindRef = $rootKind.PSObject.Properties['$ref']
                    if ($rootKindNames.Count -eq 1 -and $null -ne $canonicalEventKindRef -and
                        [string] $canonicalEventKindRef.Value -ceq '#/$defs/eventKind') {
                        $rootProperties[$rootProperty.Name] = $selectedKind
                    } else {
                        $rootProperties[$rootProperty.Name] = [pscustomobject]@{ allOf = @($rootKind, $selectedKind) }
                    }
                } else {
                    $rootProperties[$rootProperty.Name] = $rootProperty.Value
                }
            }
        }
        if ($rootProperties.Count -gt 0) { $rootContext['properties'] = [pscustomobject]$rootProperties }
        $declared = [pscustomobject]@{ root = [pscustomobject]$rootContext; envelope = $variant; kind = $selectedKind }
        try { $kinds[$tag] = ConvertTo-ContractCanonical -Node $declared -Definitions $definitions } catch { return $null }
    }
    return $kinds
}

<#
.SYNOPSIS
    One verdict per declared tag, after collapsing refs that descend from one another into one
    lineage. `IsAncestor` is a scriptblock `(a, b) -> bool`, so a suite can inject the tree.
#>
function Get-ContractVerdicts {
    # `-HeadDirty` (default on, the safe side): the working tree's schema differs from the
    # checked-out commit's, so HEAD's declaration was never carried by any descendant of that
    # commit and HEAD is its own lineage. A CLEAN HEAD is exactly its commit, and collapses into a
    # descendant like any ancestor would (Codex P1 on #1005).
    param([AllowEmptyCollection()] [object[]] $Declarations = @(), [Parameter(Mandatory)] [scriptblock] $IsAncestor, [bool] $HeadDirty = $true)
    $byTag = New-OrdinalMap
    foreach ($declaration in @($Declarations)) {
        if ($null -eq $declaration) { continue }
        if (-not $byTag.ContainsKey($declaration.Tag)) { $byTag[$declaration.Tag] = @() }
        $byTag[$declaration.Tag] += $declaration
    }
    $verdicts = @()
    $tags = [string[]] @($byTag.Keys)
    [Array]::Sort($tags, [System.StringComparer]::Ordinal)
    foreach ($tag in $tags) {
        $all = @($byTag[$tag])
        $lineages = @()
        foreach ($candidate in $all) {
            $collapsed = $false
            # The working tree is a lineage of its own: HEAD's declaration may be uncommitted, so a
            # descendant of HEAD's COMMIT never carried it, and HEAD is never collapsed as somebody's
            # ancestor (Codex P1 on #1005). A same-commit alias still yields to HEAD below.
            $candidateIsWorkingTree = (Test-IsWorkingTreeDeclaration $candidate) -and $HeadDirty
            foreach ($other in $all) {
                if ($candidateIsWorkingTree) { break }
                if (Test-SameDeclarationIdentity $other $candidate) { continue }
                if (-not (& $IsAncestor (Get-ContractIdentity $candidate) (Get-ContractIdentity $other))) { continue }
                # The other descends from this one: this one is the older copy of the same lineage.
                # When each descends from the other they are the SAME commit under two names, and
                # exactly one survives. HEAD always, when it is one of them: a refusal is only ever
                # addressed to HEAD, so an alias that sorted first would turn the branch introducing
                # the collision into a bystander of its own fight (Codex P1 on #1005). Otherwise
                # the name that sorts first.
                if (& $IsAncestor (Get-ContractIdentity $other) (Get-ContractIdentity $candidate)) {
                    # ONE rule for the literal, everywhere it is compared: ordinal (see Test-SameContract).
                    if (Test-IsWorkingTreeDeclaration $candidate) { continue }
                    if (-not (Test-IsWorkingTreeDeclaration $other) -and ([string]::CompareOrdinal([string] $candidate.Branch, [string] $other.Branch) -lt 0)) { continue }
                }
                $collapsed = $true
                break
            }
            if (-not $collapsed) { $lineages += $candidate }
        }
        # ORDINAL, and case-sensitive: JSON Schema is, and `Sort-Object -Unique` is culture-aware and
        # case-insensitive -- it folded `"Done"` and `"done"` into one variant (Codex P1 on #1005).
        $distinct = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::Ordinal)
        foreach ($lineage in $lineages) { [void] $distinct.Add([string] $lineage.Canonical) }
        $variants = @($distinct)
        $category = if ($lineages.Count -lt 2) { 'SINGLE' } elseif ($variants.Count -eq 1) { 'RE-DECLARATION' } else { 'COLLISION' }
        $verdicts += [pscustomobject]@{
            Tag      = $tag
            Category = $category
            Branches = @($lineages | ForEach-Object { $_.Branch } | Sort-Object)
            Variants = $variants.Count
            WorkingTreeParty = (@($lineages | Where-Object { Test-IsWorkingTreeDeclaration $_ }).Count -gt 0)
        }
    }
    return , ([object[]] $verdicts)
}

<#
.SYNOPSIS
    The collisions this head is party to. Only those redden the gate that runs here.
#>
function Get-ContractRefusals {
    param([AllowEmptyCollection()] [object[]] $Verdicts = @(), [Parameter(Mandatory)] [string] $Head, [switch] $WorkingTree)
    # The working tree is identified by declaration identity, never by the display name HEAD.
    # A remote branch may literally be named HEAD and must not become this gate's party.
    $refusals = @()
    foreach ($verdict in @($Verdicts)) {
        if ($null -eq $verdict -or $verdict.Category -ne 'COLLISION') { continue }
        $party = if ($WorkingTree) {
            [bool]$verdict.WorkingTreeParty
        } else {
            @($verdict.Branches | Where-Object { Test-SameContract $_ $Head }).Count -gt 0
        }
        if ($party) { $refusals += $verdict }
    }
    return , ([object[]] $refusals)
}

# Native streams are read in bounded byte chunks before decoding or splitting. The deadline
# bounds polling/admission, not OS Start/Kill/Dispose or taskkill calls; it is not a wall-time SLA.
$script:MaxGitOutputBytes = 8MB
$script:MaxGitOutputLines = 65536
function Invoke-BoundedGit {
    param([Parameter(Mandatory)] [string] $Root, [Parameter(Mandatory)] [string[]] $Arguments, [int] $TimeoutSeconds = 20)
    if ($TimeoutSeconds -le 0) { return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $true } }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = 'git'
    $info.WorkingDirectory = $Root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.Arguments = (@($Arguments | ForEach-Object {
        '"' + (($_ -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"'
    }) -join ' ')
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $started = $false
    $output = New-Object IO.MemoryStream
    try {
        $started = $process.Start()
        $null = $process.Handle # retain identity until cleanup completes
        $outBuffer = New-Object byte[] 4096
        $errBuffer = New-Object byte[] 4096
        $outRead = $process.StandardOutput.BaseStream.ReadAsync($outBuffer, 0, $outBuffer.Length)
        $errRead = $process.StandardError.BaseStream.ReadAsync($errBuffer, 0, $errBuffer.Length)
        $bytes = 0L
        while ($null -ne $outRead -or $null -ne $errRead -or -not $process.HasExited) {
            if ($watch.Elapsed.TotalSeconds -ge $TimeoutSeconds) { return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $true } }
            if ($null -ne $outRead -and $outRead.IsCompleted) {
                $count = $outRead.GetAwaiter().GetResult()
                $bytes += $count
                if ($bytes -gt $script:MaxGitOutputBytes) { return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $false; OutputExceeded = $true } }
                if ($count -eq 0) { $outRead = $null } else {
                    $output.Write($outBuffer, 0, $count)
                    $outRead = $process.StandardOutput.BaseStream.ReadAsync($outBuffer, 0, $outBuffer.Length)
                }
            }
            if ($null -ne $errRead -and $errRead.IsCompleted) {
                $count = $errRead.GetAwaiter().GetResult()
                $bytes += $count
                if ($bytes -gt $script:MaxGitOutputBytes) { return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $false; OutputExceeded = $true } }
                if ($count -eq 0) { $errRead = $null } else {
                    $errRead = $process.StandardError.BaseStream.ReadAsync($errBuffer, 0, $errBuffer.Length)
                }
            }
            Start-Sleep -Milliseconds 10
        }
        $reader = New-Object IO.StringReader ([Text.Encoding]::UTF8.GetString($output.ToArray()))
        try {
            $lines = New-Object 'Collections.Generic.List[string]'
            while ($null -ne ($line = $reader.ReadLine())) {
                if ($lines.Count -ge $script:MaxGitOutputLines) { return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $false; OutputExceeded = $true } }
                if ($line) { $lines.Add($line) }
            }
            return [pscustomobject]@{ Code = $process.ExitCode; Lines = $lines.ToArray(); TimedOut = $false }
        } finally { $reader.Dispose() }
    } catch {
        return [pscustomobject]@{ Code = -1; Lines = @(); TimedOut = $false }
    } finally {
        try {
            if ($started -and -not $process.HasExited) {
                # Retain the existing tree cleanup for git:// transport children. No completion
                # bound is claimed for these OS calls; the test observes the concrete hung fixture.
                try { & taskkill.exe /T /F /PID $process.Id 2>$null | Out-Null } catch { }
                try { if (-not $process.HasExited) { $process.Kill() } } catch { }
            }
        } finally { $process.Dispose(); $output.Dispose() }
    }
}
<#
.SYNOPSIS
    Whether two canonical contract values are the SAME contract: ordinal, never `-eq`/`-ceq`, which
    are culture-aware and give zero weight to U+00AD, U+200D, U+2060, U+FE00, U+FEFF, U+FFFD (#753).
#>
function Test-SameContract {
    param([AllowNull()] [string] $A, [AllowNull()] [string] $B)
    return [string]::Equals([string] $A, [string] $B, [System.StringComparison]::Ordinal)
}

function Test-IsWorkingTreeDeclaration {
    param($Declaration)
    return ($null -ne $Declaration -and $null -ne $Declaration.IsWorkingTree -and [bool]$Declaration.IsWorkingTree)
}

function Get-ContractIdentity {
    param($Declaration)
    if (Test-IsWorkingTreeDeclaration $Declaration) { return '@working-tree' }
    return [string]$Declaration.Branch
}

function Test-SameDeclarationIdentity {
    param($A, $B)
    if ((Test-IsWorkingTreeDeclaration $A) -or (Test-IsWorkingTreeDeclaration $B)) {
        return ((Test-IsWorkingTreeDeclaration $A) -and (Test-IsWorkingTreeDeclaration $B))
    }
    return Test-SameContract $A.Branch $B.Branch
}

<#
.SYNOPSIS
    A dictionary keyed by a NAME -- a git ref, a wire tag -- must be ordinal and case-sensitive: git
    accepts `Foo` beside `foo`, JSON Schema consts are case-sensitive, and a PowerShell `@{}` folds
    the two into one key so the second is never inspected (Codex P2 on #1005).
#>
function New-OrdinalMap {
    return New-Object 'System.Collections.Generic.Dictionary[string,object]' ([System.StringComparer]::Ordinal)
}

function Test-OrdinalPathContains {
    param([AllowEmptyCollection()] [string[]] $Paths = @(), [string] $Value)
    foreach ($path in @($Paths)) {
        if ([string]::Equals([string] $path, [string] $Value, [System.StringComparison]::Ordinal)) { return $true }
    }
    return $false
}

<#
.SYNOPSIS
    The `git ls-remote --heads` listing as name -> sha, case-sensitively.
#>
function ConvertTo-HeadShas {
    param([AllowEmptyCollection()] [string[]] $Listing = @())
    if (@($Listing).Count -gt 4096) { return $null }
    $headShas = New-OrdinalMap
    foreach ($row in @($Listing)) {
        $fields = ([string] $row) -split "`t"
        if ($fields.Count -lt 2) { continue }
        $headShas[($fields[1] -replace '^refs/heads/', '')] = $fields[0].Trim()
    }
    return $headShas
}

<#
.SYNOPSIS
    The remote as it may be WRITTEN DOWN: a URL's `user:token@` is replaced, because a reason
    reaches the suite's log and the gate's manifest (Codex P1 on #1005).
#>
function Get-RemoteLabel {
    param([AllowNull()] [string] $Remote)
    if ([string]::IsNullOrEmpty($Remote)) { return '' }
    $label = $Remote -replace '://[^/@]+@', '://<redacted>@'
    # Credentials also arrive in query strings and path segments on common Git services. Keep the
    # URL shape useful for diagnosis, but never carry the value into a suite marker or manifest.
    # The separator may itself be percent-encoded (`client%5Fsecret`); values are opaque, so the
    # replacement consumes their encoded form as one unit too. Include the common compound names,
    # not only the bare `token`/`secret` spellings, because the remote label is written into failures.
    $separator = '(?:[_-]|%5[fF]|%2[dD])'
    $credentialName = "(?:access${separator}?token|api${separator}?key|client${separator}?secret|oauth${separator}?token|private${separator}?token|token|password|passwd|pwd|secret|credential|key)"
    $label = $label -replace "(?i)([?&]$credentialName=)[^&#\s]*", '$1<redacted>'
    $label = $label -replace "(?i)(/$credentialName(?:/|%2[fF]))[^/?#\s]+", '$1<redacted>'
    return $label
}

<#
.SYNOPSIS
    The ancestry memo, keyed by a NESTED ordinal map so a pair is a pair: git accepts `>` in a
    branch name, so `"$a>$b"` was not injective -- (a, b>c) and (a>b, c) collided (Codex P1 on #1005).
#>
function New-AncestryMemo { return New-OrdinalMap }
function Set-AncestryMemo {
    param($Memo, [string] $A, [string] $B, [bool] $Value)
    if (-not $Memo.ContainsKey($A)) { $Memo[$A] = New-OrdinalMap }
    $Memo[$A][$B] = $Value
}
function Test-AncestryMemo {
    param($Memo, [string] $A, [string] $B)
    if (-not $Memo.ContainsKey($A)) { return $null }
    if (-not $Memo[$A].ContainsKey($B)) { return $null }
    return [bool] $Memo[$A][$B]
}

function Unknown {
    param([Parameter(Mandatory)] [string] $Reason)
    return [pscustomobject]@{ Known = $false; Reason = $Reason }
}

# A schema blob larger than this is not read: every open head is an untrusted input to this
# sweep, and one oversized blob must become a bounded UNKNOWN, never a parse that exhausts the
# gate (Codex P1 on #1005). The checked-in envelope is ~60 KiB.
$script:MaxSchemaBlobBytes = 4MB

function Read-RefText {
    # `-RemainingSeconds` is a SCRIPTBLOCK, asked before EACH of the two git calls: a number handed in
    # once was applied twice, so a helper started with 40 s left could spend 80 (Codex P1 on #1005).
    # `-TimeoutSeconds` remains for callers that have no deadline of their own.
    param([Parameter(Mandatory)] [string] $Root, [Parameter(Mandatory)] [string] $Ref, [Parameter(Mandatory)] [string] $Path, [int] $TimeoutSeconds = 20, [scriptblock] $RemainingSeconds)
    $ask = if ($null -ne $RemainingSeconds) { $RemainingSeconds } else { { $TimeoutSeconds }.GetNewClosure() }
    # Size first, through the same bounded runner as every other git call: an object store can be
    # slow as well as large, and a read with no clock is the half of the pipeline a 20 s fetch
    # bound does not reach (Codex P1 on #1005).
    $seconds = [int] (& $ask)
    if ($seconds -le 0) { return $null }
    $size = Invoke-BoundedGit -Root $Root -Arguments @('cat-file', '-s', "${Ref}:${Path}") -TimeoutSeconds $seconds
    if ($size.TimedOut -or $size.Code -ne 0 -or @($size.Lines).Count -eq 0) { return $null }
    if ([int64] ([string] @($size.Lines)[0]).Trim() -gt $script:MaxSchemaBlobBytes) { return $null }
    $seconds = [int] (& $ask)
    if ($seconds -le 0) { return $null }
    $show = Invoke-BoundedGit -Root $Root -Arguments @('show', "${Ref}:${Path}") -TimeoutSeconds $seconds
    if ($show.TimedOut -or $show.Code -ne 0) { return $null }
    return (@($show.Lines) -join "`n")
}

<#
.SYNOPSIS
    The working tree's schema, under the same size bound as a remote blob: the checkout is an
    input too, and an oversized envelope on disk must be UNKNOWN before it is read.
#>
function Read-WorkingTreeText {
    param([Parameter(Mandatory)] [string] $Path, [scriptblock] $RemainingSeconds)
    if ($null -ne $RemainingSeconds -and [int](& $RemainingSeconds) -le 0) { return $null }
    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    if ($null -ne $RemainingSeconds -and [int](& $RemainingSeconds) -le 0) { return $null }
    if ((Get-Item -LiteralPath $Path).Length -gt $script:MaxSchemaBlobBytes) { return $null }
    if ($null -ne $RemainingSeconds -and [int](& $RemainingSeconds) -le 0) { return $null }
    return [IO.File]::ReadAllText($Path)
}

function New-ExternalResolver {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $SchemaPath,
        [Parameter(Mandatory)] [string] $SourceRef,
        [Parameter(Mandatory)] [scriptblock] $RemainingSeconds,
        [switch] $WorkingTree
    )
    $schemaDirectory = Split-Path -Parent $SchemaPath
    $cache = New-OrdinalMap
    return {
        param([string] $Reference)
        if ($null -ne $RemainingSeconds -and [int](& $RemainingSeconds) -le 0) { return $null }
        $hash = $Reference.IndexOf('#')
        $relative = if ($hash -ge 0) { $Reference.Substring(0, $hash) } else { $Reference }
        if ([string]::IsNullOrEmpty($relative) -or $relative -match '^[A-Za-z][A-Za-z0-9+.-]*:' -or $relative.StartsWith('/') -or $relative.StartsWith('\') -or $relative -match '(^|[/\\])\.\.([/\\]|$)') {
            return $null
        }
        $target = (Join-Path $schemaDirectory $relative).Replace('\', '/')
        if ($cache.ContainsKey($target)) { return $cache[$target] }
        $text = if ($WorkingTree) {
            Read-WorkingTreeText -Path (Join-Path $Root $target) -RemainingSeconds $RemainingSeconds
        } else {
            Read-RefText -Root $Root -Ref $SourceRef -Path $target -RemainingSeconds $RemainingSeconds
        }
        $cache[$target] = $text
        return $text
    }.GetNewClosure()
}

function Invoke-AncestryProbe {
    param([Parameter(Mandatory)] [string] $Root, [Parameter(Mandatory)] [string] $A, [Parameter(Mandatory)] [string] $B, [int] $TimeoutSeconds = 20)
    $probe = Invoke-BoundedGit -Root $Root -Arguments @('merge-base', '--is-ancestor', $A, $B) -TimeoutSeconds $TimeoutSeconds
    if ($probe.TimedOut) { return Unknown "git merge-base --is-ancestor $A $B timed out" }
    if ($probe.Code -eq 0) { return [pscustomobject]@{ Known = $true; Value = $true; Reason = '' } }
    if ($probe.Code -eq 1) { return [pscustomobject]@{ Known = $true; Value = $false; Reason = '' } }
    return Unknown "git merge-base --is-ancestor $A $B exited $($probe.Code), so ancestry is UNKNOWN"
}

<#
.SYNOPSIS
    The derived population and its declarations, read from the remote and the working tree.
    `Known = $false` with a named `Reason` when the remote did not answer or a schema could not be
    read: UNKNOWN, never empty, and never a wait -- every remote call is bounded.
#>
function Get-ContractPopulation {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [string] $Remote = 'origin',
        [string] $Base = 'origin/main',
        [string] $SchemaPath = 'schemas/event-envelope.schema.json',
        [int] $TimeoutSeconds = 20,
        # ONE clock over the whole population. Per-call timeouts do not compose into a bound: 300
        # heads at just under 20 s each is hours (Codex P1 on #1005). Spent, the reading is UNKNOWN
        # naming the budget and how far it got, never a gate that runs until somebody looks.
        [int] $BudgetSeconds = 600
    )
    $budget = [System.Diagnostics.Stopwatch]::StartNew()
    $remoteLabel = Get-RemoteLabel -Remote $Remote
    if ($BudgetSeconds -le 0) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s is already spent before the first remote call" }
    # EVERY git call is capped by the time that REMAINS, never by the full per-call timeout, and
    # the budget is re-read after each one: `-BudgetSeconds 1` could otherwise spend 20 s in the
    # first fetch and a call begun just under the deadline could overrun it by a whole timeout
    # (Codex P1 on #1005). Zero time left refuses before another Git launch or file read.
    # `-Cap`: remote calls keep the per-call timeout; LOCAL object-store calls (diff, show,
    # cat-file, merge-base) take 60 s, because a loaded machine stalled a 0.2 s diff past 20 s once
    # and the whole reading became NOT MEASURED for a local stall. The budget bounds both.
    # This is an admission deadline, not a hard wall-time guarantee: OS process and filesystem
    # calls (including cleanup) can block. Expiry prevents starting the next read or Git command.
    function Get-RemainingSeconds {
        param([int] $Cap = $TimeoutSeconds)
        $left = [int] [Math]::Floor($BudgetSeconds - $budget.Elapsed.TotalSeconds)
        return [Math]::Max(0, [Math]::Min($Cap, $left))
    }
    function Test-BudgetSpent { return ($budget.Elapsed.TotalSeconds -ge $BudgetSeconds) }
    # The objects behind the listed heads have to be local to be read; one fetch, before the list,
    # so the list and the objects describe the same instant as nearly as a remote allows. Both
    # calls are BOUNDED: a remote that does not answer in seconds is UNKNOWN, with the reason
    # named, and never a wait.
    $fetch = Invoke-BoundedGit -Root $Root -Arguments @('fetch', '--quiet', $Remote) -TimeoutSeconds (Get-RemainingSeconds)
    if ($fetch.TimedOut) { return Unknown "git fetch $remoteLabel did not finish within the time left of the ${BudgetSeconds}s budget (per-call cap ${TimeoutSeconds}s)" }
    if ($fetch.Code -ne 0) { return Unknown "git fetch $remoteLabel exited $($fetch.Code)" }
    if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent by the fetch" }
    $list = Invoke-BoundedGit -Root $Root -Arguments @('ls-remote', '--heads', $Remote) -TimeoutSeconds (Get-RemainingSeconds)
    if ($list.TimedOut) { return Unknown "git ls-remote --heads $remoteLabel did not finish within the time left of the ${BudgetSeconds}s budget" }
    if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent by the listing" }
    if ($list.Code -ne 0) { return Unknown "git ls-remote --heads $remoteLabel exited $($list.Code)" }
    $listing = @($list.Lines)
    if ($listing.Count -eq 0) { return Unknown "git ls-remote --heads $remoteLabel listed no heads" }
    # ONE snapshot: the listing's own SHAs are what gets read, never `origin/<name>` from the fetch
    # a moment earlier (Codex P2 on #1005). A head pushed between the two is a SHA the fetch did
    # not bring, and reading it fails into UNKNOWN with the reason, rather than silently reading
    # the older alias.
    $headShas = ConvertTo-HeadShas -Listing $listing
    if ($null -eq $headShas) { return Unknown 'remote head listing exceeds the 4096-ref limit' }
    $heads = [string[]] @($headShas.Keys)
    [Array]::Sort($heads, [System.StringComparer]::Ordinal)

    $baseText = Read-RefText -Root $Root -Ref $Base -Path $SchemaPath -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
    if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent reading the base schema" }
    if ($null -eq $baseText) { return Unknown "git could not read ${Base}:${SchemaPath}" }
    $baseResolver = New-ExternalResolver -Root $Root -SchemaPath $SchemaPath -SourceRef $Base -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
    $baseKinds = Get-DeclaredKinds -SchemaText $baseText -ExternalResolver $baseResolver
    if ($null -eq $baseKinds) { return Unknown "${Base}:${SchemaPath} is not the envelope shape (no oneOf variants keyed by kind)" }
    $baseExternalPaths = Get-ExternalReferencePaths -SchemaText $baseText -SchemaPath $SchemaPath
    if ($null -eq $baseExternalPaths) { return Unknown "could not enumerate checked-in external references from ${Base}:${SchemaPath}" }

    function Get-ForkKinds([string] $CandidateRef) {
        $seconds = Get-RemainingSeconds -Cap 60
        if ($seconds -le 0) { return $null }
        $fork = Invoke-BoundedGit -Root $Root -Arguments @('merge-base', $Base, $CandidateRef) -TimeoutSeconds $seconds
        if ($fork.TimedOut -or $fork.Code -ne 0 -or @($fork.Lines).Count -ne 1) { return $null }
        $forkSha = ([string]$fork.Lines[0]).Trim()
        if ($forkSha -cnotmatch '^[0-9a-f]{40}$') { return $null }
        # THE MEMO SITS HERE, NOT ABOVE, and both halves of that are load-bearing. The budget guard
        # at the top still runs on every call, so an exhausted budget refuses whether or not this
        # fork was seen -- a cache must not buy the sweep time it no longer has. And the
        # `merge-base` above still runs per candidate, because its sha IS the key.
        if ($forkContracts.ContainsKey($forkSha)) { return $forkContracts[$forkSha] }
        $script:ForkContractReads += 1
        $forkText = Read-RefText -Root $Root -Ref $forkSha -Path $SchemaPath -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
        if ($null -eq $forkText -or (Test-BudgetSpent)) { return $null }
        $forkResolver = New-ExternalResolver -Root $Root -SchemaPath $SchemaPath -SourceRef $forkSha -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
        $forkKinds = Get-DeclaredKinds -SchemaText $forkText -ExternalResolver $forkResolver
        # SUCCESSES ONLY. A `$null` here is a timeout or an unreadable blob, and the caller turns it
        # into UNKNOWN, which ends the sweep -- caching it would be caching a failure, and a later
        # head would inherit a refusal it never earned.
        if ($null -ne $forkKinds) { $forkContracts[$forkSha] = $forkKinds }
        return $forkKinds
    }

    # ONE ENTRY PER FORK POINT, for this population read only. The fork contract is a pure
    # function of the fork sha (the schema path is fixed for the run), so two heads cut from the
    # same commit have the same answer by construction -- and this repository cuts many: of 14
    # touched heads measured on it, only 10 fork points were distinct.
    $forkContracts = @{}
    $declarations = @()
    $touched = @()
    $examined = 0
    foreach ($head in $heads) {
        if ($budget.Elapsed.TotalSeconds -gt $BudgetSeconds) {
            return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent after $examined of $($heads.Count) heads"
        }
        $examined += 1
        $ref = $headShas[$head]
        # CHEAP DIFF FIRST. 287 of 302 heads are unchanged over the schema paths (measured on this branch),
            # and each of them was paying cat-file -s, cat-file -p and a JSON parse to learn that -- ~0.55 s
            # per head to reach a `continue`.
            #
            # THE PATHSPEC IS THE SCHEMA'S DIRECTORY, NOT A LIST. The first version diffed SchemaPath plus
            # BASE's external paths and argued that a head referencing a file base does not reference must
            # differ in the schema text. FALSE: this diff is THREE-DOT, so it compares merge-base($Base,$ref)
            # with $ref, and a head INHERITS its reference set from its fork point -- if main later moved a
            # `$ref` from foo to bar, a branch forked before that and editing only foo differs from its own
            # merge-base in nothing the list named, reads as unchanged, and a real COLLISION becomes a clean
            # MEASURED (lane A on #1005, fixture built end to end; not live today because main has never
            # removed a `$ref`, checked across all 20 envelope commits and all 302 heads).
            #
            # Get-ExternalReferencePaths refuses absolute paths, URI schemes and `..` segments and joins
            # everything under Split-Path -Parent $SchemaPath, so every external document any head can name
            # lives under that directory. Diffing the directory is therefore sound BY CONSTRUCTION, at the
            # same spawn count, and does not depend on an argument about what heads can do.
            $schemaScope = Split-Path -Parent $SchemaPath
            if ([string]::IsNullOrEmpty($schemaScope)) { $schemaScope = '.' }
            $quickRun = Invoke-BoundedGit -Root $Root -Arguments @('diff', '--quiet', "$Base...$ref", '--', $schemaScope) -TimeoutSeconds (Get-RemainingSeconds -Cap 60)
        if ($quickRun.TimedOut) { return Unknown "git diff $Base...$ref ($head) did not finish within the time left (local cap 60s, budget ${BudgetSeconds}s)" }
        if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent after $examined of $($heads.Count) heads" }
        if ($quickRun.Code -eq 0) { continue }
        if ($quickRun.Code -ne 1) { return Unknown "git diff $Base...$ref ($head) exited $($quickRun.Code) (the listing advertised a head the fetch did not bring, or the object store is unreadable)" }
        $text = Read-RefText -Root $Root -Ref $ref -Path $SchemaPath -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
        if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent after $examined of $($heads.Count) heads" }
        if ($null -eq $text) { return Unknown "git could not read ${ref}:${SchemaPath} ($head), or the blob exceeds $script:MaxSchemaBlobBytes bytes" }
        $candidateExternalPaths = Get-ExternalReferencePaths -SchemaText $text -SchemaPath $SchemaPath
        if ($null -eq $candidateExternalPaths) { return Unknown "could not enumerate checked-in external references from ${ref}:${SchemaPath} ($head)" }
        $diffPaths = [string[]] @($SchemaPath) + @($baseExternalPaths) + @($candidateExternalPaths | Where-Object { -not (Test-OrdinalPathContains -Paths $baseExternalPaths -Value $_) })
        $diffRun = Invoke-BoundedGit -Root $Root -Arguments (@('diff', '--quiet', "$Base...$ref", '--') + $diffPaths) -TimeoutSeconds (Get-RemainingSeconds -Cap 60)
        if ($diffRun.TimedOut) { return Unknown "git diff $Base...$ref ($head) did not finish within the time left (local cap 60s, budget ${BudgetSeconds}s)" }
        $diff = $diffRun.Code
        # The budget is read right after the call, BEFORE the early continue: an unchanged head that
        # finished its diff just past the deadline must not start the next call (Codex P2 on #1005).
        if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent after $examined of $($heads.Count) heads" }
        if ($diff -eq 0) { continue }
        # A listed head whose objects are still unreadable after the fetch is a hole in the
        # population, and a population with a hole cannot vouch for anything.
        if ($diff -ne 1) { return Unknown "git diff $Base...$ref ($head) exited $diff (the listing advertised a head the fetch did not bring, or unreadable objects)" }
        $touched += $head
        $resolver = New-ExternalResolver -Root $Root -SchemaPath $SchemaPath -SourceRef $ref -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
        $kinds = Get-DeclaredKinds -SchemaText $text -ExternalResolver $resolver
        if ($null -eq $kinds) { return Unknown "${ref}:${SchemaPath} ($head) is not the envelope shape" }
        $forkKinds = Get-ForkKinds -CandidateRef $ref
        if ($null -eq $forkKinds) { return Unknown "could not read the merge-base contract for $head" }
        foreach ($tag in @($kinds.Keys)) {
            if ($forkKinds.ContainsKey($tag) -and (Test-SameContract $forkKinds[$tag] $kinds[$tag])) { continue }
            $declarations += [pscustomobject]@{ Branch = $head; Identity = $head; IsWorkingTree = $false; Tag = $tag; Canonical = $kinds[$tag] }
        }
    }

    # The branch under judgement is its WORKING TREE, under the name HEAD: an unpushed change is
    # exactly the one that has not been seen by anybody else yet.
    $headFile = Join-Path $Root $SchemaPath
    # Is the working tree's schema the checked-out commit's? A clean HEAD is its commit and may be
    # collapsed as an ancestor; a dirty one is its own lineage (see Get-ContractVerdicts).
    $committedHeadText = Read-RefText -Root $Root -Ref 'HEAD' -Path $SchemaPath -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
    if (Test-BudgetSpent) { return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent reading HEAD's committed schema" }
    if ($null -eq $committedHeadText) { return Unknown "git could not read HEAD:${SchemaPath}" }
    $headText = Read-WorkingTreeText -Path $headFile -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
    if ($null -eq $headText) { return Unknown "the working tree has no $SchemaPath, or it exceeds $script:MaxSchemaBlobBytes bytes" }
    $headResolver = New-ExternalResolver -Root $Root -SchemaPath $SchemaPath -SourceRef 'HEAD' -RemainingSeconds { Get-RemainingSeconds -Cap 60 } -WorkingTree
    $headKinds = Get-DeclaredKinds -SchemaText $headText -ExternalResolver $headResolver
    if ($null -eq $headKinds) { return Unknown "the working tree's $SchemaPath is not the envelope shape" }
    $headDirty = $true
    if ($null -ne $committedHeadText) {
        $committedResolver = New-ExternalResolver -Root $Root -SchemaPath $SchemaPath -SourceRef 'HEAD' -RemainingSeconds { Get-RemainingSeconds -Cap 60 }
        $committedKinds = Get-DeclaredKinds -SchemaText $committedHeadText -ExternalResolver $committedResolver
        if ($null -ne $committedKinds -and $committedKinds.Count -eq $headKinds.Count) {
            $headDirty = $false
            foreach ($tag in @($headKinds.Keys)) {
                if (-not $committedKinds.ContainsKey($tag) -or -not (Test-SameContract $committedKinds[$tag] $headKinds[$tag])) { $headDirty = $true; break }
            }
        }
    }
    # HEAD DECLARES ONLY WHAT IT TOUCHED. A branch that never touched the schema, with a clean tree,
    # still differs from `<base>` wherever main moved a kind after it forked -- and those stale
    # values made it party to somebody else's collision (Codex P1 on #1005). The same three-dot
    # filter the remote heads pass applies to HEAD's committed lineage; the working tree counts
    # when it is dirty.
    $workingPaths = Get-ExternalReferencePaths -SchemaText $headText -SchemaPath $SchemaPath
    $committedPaths = Get-ExternalReferencePaths -SchemaText $committedHeadText -SchemaPath $SchemaPath
    if ($null -eq $workingPaths -or $null -eq $committedPaths) { return Unknown 'HEAD reference paths could not be enumerated' }
    $headPaths = [string[]]@($SchemaPath) + @($baseExternalPaths) + @($workingPaths) + @($committedPaths)
    $headTouchRun = Invoke-BoundedGit -Root $Root -Arguments (@('diff', '--quiet', "$Base...HEAD", '--') + $headPaths) -TimeoutSeconds (Get-RemainingSeconds -Cap 60)
    if ($headTouchRun.TimedOut) { return Unknown "git diff $Base...HEAD did not finish within the time left" }
    if ($headTouchRun.Code -ne 0 -and $headTouchRun.Code -ne 1) { return Unknown "git diff $Base...HEAD exited $($headTouchRun.Code)" }
    $headTouched = ($headTouchRun.Code -eq 1)
    $headDeclarations = 0
    if ($headTouched -or $headDirty) {
        $headForkKinds = Get-ForkKinds -CandidateRef 'HEAD'
        if ($null -eq $headForkKinds) { return Unknown "could not read HEAD's merge-base contract" }
        foreach ($tag in @($headKinds.Keys)) {
            if ($headForkKinds.ContainsKey($tag) -and (Test-SameContract $headForkKinds[$tag] $headKinds[$tag])) { continue }
            $declarations += [pscustomobject]@{ Branch = 'HEAD'; Identity = '@working-tree'; IsWorkingTree = $true; Tag = $tag; Canonical = $headKinds[$tag] }
            $headDeclarations += 1
        }
    }

    # ANCESTRY INSIDE THE BUDGET. The lineage collapse asks `merge-base --is-ancestor` for every
    # ordered pair of declarations under one tag -- quadratic, and until now it ran AFTER this
    # function returned, outside the whole-sweep budget and through a bare `& git` with no clock
    # (Codex P1 on #1005). Every pair the collapse can ask about is computed HERE, under the same
    # stopwatch and the same bounded runner, and memoised; the oracle handed out below is a lookup
    # and makes no git call. A budget spent during this phase is UNKNOWN naming how far it got.
    $ancestry = New-AncestryMemo
    $byTagForAncestry = New-OrdinalMap
    foreach ($declaration in $declarations) {
        if (-not $byTagForAncestry.ContainsKey($declaration.Tag)) { $byTagForAncestry[$declaration.Tag] = @() }
        $byTagForAncestry[$declaration.Tag] += (Get-ContractIdentity $declaration)
    }
    $pairsDone = 0
    foreach ($tag in @($byTagForAncestry.Keys)) {
        $nameSet = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::Ordinal)
        foreach ($n in @($byTagForAncestry[$tag])) { [void] $nameSet.Add([string] $n) }
        $names = [string[]] @($nameSet)
        [Array]::Sort($names, [System.StringComparer]::Ordinal)
        if ($names.Count -lt 2) { continue }
        foreach ($a in $names) {
            foreach ($b in $names) {
                if (Test-SameContract $a $b) { continue }
                if ($null -ne (Test-AncestryMemo -Memo $ancestry -A $a -B $b)) { continue }
                if ($budget.Elapsed.TotalSeconds -gt $BudgetSeconds) {
                    return Unknown "the whole-sweep budget of ${BudgetSeconds}s was spent during ancestry after $pairsDone pair(s)"
                }
                $ra = if ([string]::Equals([string] $a, '@working-tree', [System.StringComparison]::Ordinal)) { 'HEAD' } else { $headShas[$a] }
                $rb = if ([string]::Equals([string] $b, '@working-tree', [System.StringComparison]::Ordinal)) { 'HEAD' } else { $headShas[$b] }
                $script:AncestryGitCalls += 1
                $probe = Invoke-AncestryProbe -Root $Root -A $ra -B $rb -TimeoutSeconds (Get-RemainingSeconds -Cap 60)
                if (-not $probe.Known) { return $probe }
                Set-AncestryMemo -Memo $ancestry -A $a -B $b -Value $probe.Value
                $pairsDone += 1
            }
        }
    }
    $memo = $ancestry
    $isAncestor = {
        param($a, $b)
        $known = Test-AncestryMemo -Memo $memo -A $a -B $b
        if ($null -ne $known) { return [bool] $known }
        # A pair nobody declared together was never a question the collapse can ask; it is not
        # an ancestor, and it is not a reason to call git after the budget closed.
        return $false
    }.GetNewClosure()

    return [pscustomobject]@{
        Known        = $true
        Reason       = ''
        Base         = $Base
        HeadCount    = $heads.Count
        AncestryPairs = $pairsDone
        HeadDirty    = $headDirty
        HeadTouched  = $headTouched
        HeadDeclarations = $headDeclarations
        Touched      = $touched
        Declarations = $declarations
        IsAncestor   = $isAncestor
    }
}

if (-not $script:ContractSweepDotSourced) {
    $population = Get-ContractPopulation -Root $RepoRoot -Remote $Remote -Base $Base -SchemaPath $SchemaPath -BudgetSeconds $BudgetSeconds
    if (-not $population.Known) {
        Write-Host "[event-contract] NOT MEASURED -- $($population.Reason). Coverage is UNKNOWN, not clean."
        exit 2
    }
    # (`$Remote` itself is never printed below; the reasons above carry the redacted label.)
    $verdicts = Get-ContractVerdicts -Declarations $population.Declarations -IsAncestor $population.IsAncestor -HeadDirty $population.HeadDirty
    $refusals = Get-ContractRefusals -Verdicts $verdicts -Head 'HEAD' -WorkingTree
    Write-Host "[event-contract] MEASURED -- $($population.HeadCount) remote head(s); $(@($population.Touched).Count) touched '$SchemaPath' since their base; $(@($population.Declarations).Count) declaration(s) differ from $Base."
    foreach ($verdict in @($verdicts | Where-Object { $_.Category -ne 'SINGLE' })) {
        Write-Host "[event-contract]   $($verdict.Category.PadRight(14)) $($verdict.Tag): $(@($verdict.Branches) -join ', ')"
    }
    if ($refusals.Count -gt 0) {
        Write-Host '[event-contract] this branch declares a wire tag that another open branch declares with a DIFFERENT payload:'
        foreach ($refusal in $refusals) {
            Write-Host "[event-contract]   $($refusal.Tag) -- $(@($refusal.Branches) -join ', ') ($($refusal.Variants) payload shapes)"
        }
        Write-Host '[event-contract] A wire tag is a published contract from the first journal that carries it; the second shape to land forks every journal written under the first.'
        Write-Host '[event-contract] Agree one payload with the other branch, or rename one tag. If the other branch carried yours as a copy, that is a declaration to make on its issue, not here.'
        exit 1
    }
    Write-Host "[event-contract] this branch is party to no collision."
    exit 0
}
