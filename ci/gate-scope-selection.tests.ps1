# #903: the gate reads a scope selection, and every way of failing to read one runs the FULL gate.
#
# `ci/select-scope.ps1` decides what a change can reach; this is the other half -- what the GATE
# does with that decision, and specifically what it does when the decision is missing, unreadable
# or malformed. That is the half where a scoped gate becomes dangerous: a selection that cannot be
# read must widen the run, because the alternative is a gate that quietly runs less and stays green.
#
# The subject is `Read-ScopeSelection`, cut out of ci/gate.ps1 by anchor text and never retyped --
# running gate.ps1 would run the gate.

$ExpectedAssertionCount = 28
$ErrorActionPreference = 'Stop'
# THE SUITE RUNS UNDER THE GATE'S OWN RULES. ci/gate.ps1:88 sets `Set-StrictMode -Version 2.0`,
# and this file did not: the cells exercised the extracted functions under LAXER rules than
# production, so `[string]$dict.missingKey` -- which THROWS under 2.0 and returns $null without
# it -- passed here and killed every gate run at the manifest write. The cell was not missing;
# it was blind, because the instrument did not reproduce the environment it claims to measure.
Set-StrictMode -Version 2.0
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

$parseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: gate.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

# Cut a function out of a file by anchor. NAMED, because the first version of this arrangement
# extracted three functions by hand and a fourth (`New-FullScope`) was added to gate.ps1 without a
# fourth block here: the suite then died as "expected 26 assertions, ran 1", which says nothing
# about what is missing. A dependency that is absent should name itself.
function Import-FunctionFrom {
    param(
        [Parameter(Mandatory)] [string] $Text,
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $From
    )
    $anchor = "function $Name {"
    $i = $Text.IndexOf($anchor, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $Text.IndexOf("`n}", $i, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) {
        Write-Host "HARNESS-BROKE: $Name was not found in $From" -ForegroundColor Magenta
        exit 2
    }
    return $Text.Substring($i, $j - $i + 2)
}

$argsStart = $gateText.IndexOf('function Get-ScopePackageArgs {', [System.StringComparison]::Ordinal)
$argsEnd = if ($argsStart -ge 0) { $gateText.IndexOf("`n}", $argsStart, [System.StringComparison]::Ordinal) } else { -1 }
if ($argsStart -lt 0 -or $argsEnd -le $argsStart) {
    Write-Host 'HARNESS-BROKE: Get-ScopePackageArgs was not found in gate.ps1' -ForegroundColor Magenta
    exit 2
}
. ([scriptblock]::Create($gateText.Substring($argsStart, $argsEnd - $argsStart + 2)))

$recordStart = $gateText.IndexOf('function Get-ScopeRecord {', [System.StringComparison]::Ordinal)
$recordEnd = if ($recordStart -ge 0) { $gateText.IndexOf("`n}", $recordStart, [System.StringComparison]::Ordinal) } else { -1 }
if ($recordStart -lt 0 -or $recordEnd -le $recordStart) {
    Write-Host 'HARNESS-BROKE: Get-ScopeRecord was not found in gate.ps1' -ForegroundColor Magenta
    exit 2
}
. ([scriptblock]::Create($gateText.Substring($recordStart, $recordEnd - $recordStart + 2)))

. ([scriptblock]::Create((Import-FunctionFrom -Text $gateText -Name 'New-FullScope' -From 'gate.ps1')))

$start = $gateText.IndexOf('function Read-ScopeSelection {', [System.StringComparison]::Ordinal)
$end = if ($start -ge 0) { $gateText.IndexOf("`n}", $start, [System.StringComparison]::Ordinal) } else { -1 }
if ($start -lt 0 -or $end -le $start) {
    Write-Host 'HARNESS-BROKE: Read-ScopeSelection was not found in gate.ps1' -ForegroundColor Magenta
    exit 2
}
. ([scriptblock]::Create($gateText.Substring($start, $end - $start + 2)))


# #903: the presser's half lives in ci/merge-proof.ps1, so it is cut from THAT file by anchor.
$proofPath = Join-Path $PSScriptRoot 'merge-proof.ps1'
$proofText = [System.IO.File]::ReadAllText($proofPath)
$noteStart = $proofText.IndexOf('function Format-ScopeNote {', [System.StringComparison]::Ordinal)
$noteEnd = if ($noteStart -ge 0) { $proofText.IndexOf("`n}", $noteStart, [System.StringComparison]::Ordinal) } else { -1 }
if ($noteStart -lt 0 -or $noteEnd -le $noteStart) {
    Write-Host 'HARNESS-BROKE: Format-ScopeNote was not found in merge-proof.ps1' -ForegroundColor Magenta
    exit 2
}
. ([scriptblock]::Create($proofText.Substring($noteStart, $noteEnd - $noteStart + 2)))

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-gate-scope-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function New-SelectionFile {
    param([Parameter(Mandatory)] [string] $Name, [Parameter(Mandatory)] [string] $Content)
    $path = Join-Path $fixtureRoot $Name
    [System.IO.File]::WriteAllText($path, $Content, $utf8NoBom)
    return $path
}

try {
    Assert-True ($gateText.Substring($start, $end - $start).Length -gt 100) `
        "arrangement: Read-ScopeSelection was cut from gate.ps1 ($($end - $start) chars)"

    # ---- every way of NOT having a selection is a FULL run -------------------------------------
    # These are the cells that matter. A scoped gate that narrows on a bad selection runs fewer
    # stages and reports the same green, and nothing downstream can tell the two apart.
    $none = Read-ScopeSelection -Path ''
    Assert-True ($none.full -eq $true) 'no selection at all is a FULL run'
    Assert-True (-not [string]::IsNullOrWhiteSpace([string]$none.reason)) `
        "and says why, rather than leaving a bare true (got '$($none.reason)')"

    $missing = Read-ScopeSelection -Path (Join-Path $fixtureRoot 'not-there.json')
    Assert-True ($missing.full -eq $true) 'a selection path that does not exist is a FULL run, not an empty selection'
    Assert-True ($missing.reason -match 'not found|unreadable') `
        "and the reason names the absence (got '$($missing.reason)')"

    $broken = Read-ScopeSelection -Path (New-SelectionFile -Name 'broken.json' -Content '{ not json')
    Assert-True ($broken.full -eq $true) 'a selection that does not parse is a FULL run'

    # SHAPE, not just syntax: a document that parses but carries no crate list is not a narrow run.
    $shapeless = Read-ScopeSelection -Path (New-SelectionFile -Name 'shapeless.json' -Content '{"escalated":false}')
    Assert-True ($shapeless.full -eq $true) `
        'a selection that parses but carries no crates is a FULL run -- valid JSON is not a valid selection'

    # An empty crate list is the most dangerous narrow selection of all: it runs NOTHING and passes.
    $empty = Read-ScopeSelection -Path (New-SelectionFile -Name 'empty.json' -Content '{"escalated":false,"crates":[],"matrix":false}')
    Assert-True ($empty.full -eq $true) `
        'a selection with an EMPTY crate list is a FULL run -- selecting nothing would run nothing and pass'

    # ---- the selector escalating is carried, with its rule --------------------------------------
    $escalated = Read-ScopeSelection -Path (New-SelectionFile -Name 'esc.json' `
            -Content '{"escalated":true,"escalationRule":"ci/","crates":[],"matrix":true,"matrixReason":"FULL run: ci/ changed"}')
    Assert-True ($escalated.full -eq $true) 'a selection that escalated is a FULL run'
    Assert-True ($escalated.reason -match 'ci/') `
        "and the gate carries WHICH rule escalated it into its own reason (got '$($escalated.reason)')"

    # ---- the one narrow case ---------------------------------------------------------------------
    $narrow = Read-ScopeSelection -Path (New-SelectionFile -Name 'ok.json' `
            -Content '{"escalated":false,"crates":["core-leaf","core-mid"],"matrix":false,"matrixReason":"skipped: nothing reaches the adapter"}')
    Assert-True ($narrow.full -eq $false -and @($narrow.crates).Count -eq 2) `
        "a well-formed narrow selection is carried (full=$($narrow.full), crates=$(@($narrow.crates) -join ','))"
    Assert-True ($narrow.matrix -eq $false) 'and the matrix decision travels with it'

    # ---- the manifest's `scope` object -------------------------------------------------------
    # A scoped run that does not RECORD what it skipped is unauditable: `merge-proof` and the
    # presser would read a GREEN that covered a fraction of the workspace and could not tell.
    $fullRecord = Get-ScopeRecord -Selection (Read-ScopeSelection -Path '')
    Assert-True ($fullRecord.full -eq $true) 'the record says a FULL run was full'
    Assert-True (-not [string]::IsNullOrWhiteSpace([string]$fullRecord.reason)) `
        'and carries the reason, so a reader never has to guess why it was full'
    Assert-True (@($fullRecord.crates).Count -eq 0) `
        'a FULL run records no crate list, because there was no selection'

    $narrowRecord = Get-ScopeRecord -Selection (Read-ScopeSelection -Path (New-SelectionFile -Name 'rec.json' `
                -Content '{"escalated":false,"crates":["core-leaf","core-mid"],"matrix":false,"matrixReason":"skipped: nothing reaches the adapter"}'))
    Assert-True ($narrowRecord.full -eq $false -and @($narrowRecord.crates).Count -eq 2) `
        "a scoped run records the crates it ran (full=$($narrowRecord.full), crates=$(@($narrowRecord.crates) -join ','))"
    Assert-True ($narrowRecord.matrix -eq $false -and -not [string]::IsNullOrWhiteSpace([string]$narrowRecord.matrixReason)) `
        'and records the matrix decision WITH its reason, not a bare false'


    # ---- what the PRESSER is told, which is the point of recording it at all -------------------
    # ABSENT IS NOT FULL. A manifest written before #903 has no scope object, and inventing
    # "FULL" for it would be a reassurance nobody measured -- the same shape as a bare `false`.
    Assert-True ([string]::IsNullOrEmpty((Format-ScopeNote -Body ('{"status":"GREEN"}' | ConvertFrom-Json)))) `
        'a manifest with no scope object produces NO note, rather than an invented FULL'
    Assert-True ([string]::IsNullOrEmpty((Format-ScopeNote -Body $null))) `
        'and a null body produces no note rather than throwing at the presser'

    $fullNote = Format-ScopeNote -Body ('{"scope":{"full":true,"reason":"FULL: no scope selection was given","crates":[],"matrix":true}}' | ConvertFrom-Json)
    Assert-True ($fullNote -match 'FULL' -and $fullNote -match 'no scope selection') `
        "a full run's note says FULL and carries the reason (got '$fullNote')"

    $scopedNote = Format-ScopeNote -Body ('{"scope":{"full":false,"reason":"SCOPED","crates":["a","b"],"matrix":false,"matrixReason":"nothing reaches the adapter"}}' | ConvertFrom-Json)
    Assert-True ($scopedNote -match '2 crate' -and $scopedNote -match 'not the workspace' -and $scopedNote -match 'nothing reaches the adapter') `
        "a scoped run's note names the count AND says the GREEN is not the workspace (got '$scopedNote')"


    # ---- what cargo is actually told -----------------------------------------------------------
    # A FULL run must keep `--workspace`, NOT an enumeration of every crate: an enumeration stops
    # covering a crate added after the selection was computed, and does it silently.
    Assert-True (@(Get-ScopePackageArgs -Scope (Read-ScopeSelection -Path '')).Count -eq 0) `
        'a FULL run passes NO -p arguments, so the caller keeps --workspace'

    $selArgs = @(Get-ScopePackageArgs -Scope (Read-ScopeSelection -Path (New-SelectionFile -Name 'args.json' `
                    -Content '{"escalated":false,"crates":["core-leaf","core-mid"],"matrix":false}')))
    Assert-True ($selArgs.Count -eq 4) "a scoped run passes one -p per crate (got $($selArgs.Count) tokens: $($selArgs -join ' '))"
    Assert-True (($selArgs[0] -eq '-p') -and ($selArgs[2] -eq '-p')) `
        'and each crate is preceded by its own -p, so cargo sees packages and not a single joined value'
    Assert-True (($selArgs -contains 'core-leaf') -and ($selArgs -contains 'core-mid')) `
        'and every selected crate is present'

    # THE FEATURE UNION IS NOT THE SELECTION'S BUSINESS. A scoped run that also narrowed features
    # would skip exactly the code a feature-gated change alters, and report the same green. Asserted
    # against the gate's own text so the invocation cannot drift away from the claim.
    $scopedInvocation = $gateText.IndexOf('cargo $toolchain test @scopeArgs', [System.StringComparison]::Ordinal)
    Assert-True ($scopedInvocation -ge 0 -and `
            $gateText.Substring($scopedInvocation, 120).IndexOf('--all-features', [System.StringComparison]::Ordinal) -ge 0) `
        'the SCOPED cargo invocation still passes --all-features: the selection narrows packages, never cfg'


    # ---- EACH HALF OF THE STRICTMODE FIX, ASSERTED ALONE ---------------------------------------
    # The fix has two halves: the constructor always emits `matrixReason`, and the reader indexes
    # instead of dotting. Measured: sabotaging EITHER half alone leaves this suite at 26/26 green,
    # because the other half compensates. Belt-and-braces is right for production and blind for a
    # test -- so each half is asserted on its own subject, where the other cannot cover for it.
    Assert-True ((New-FullScope -Reason 'probe').Contains('matrixReason')) `
        'the FULL constructor emits matrixReason on every branch it builds (asserted on the producer, not through a reader that would hide its absence)'

    # And the reader survives a selection that LACKS the key -- hand-built, because every selection
    # the constructor makes has it, so the real producer cannot exercise this branch.
    $withoutKey = [ordered]@{ full = $true; reason = 'hand-built, no matrixReason'; crates = @(); matrix = $true }
    $survived = $true
    try { [void](Get-ScopeRecord -Selection $withoutKey) } catch { $survived = $false }
    Assert-True $survived `
        'and the record reader survives a selection with no matrixReason at all -- under StrictMode a dotted read THROWS, which killed the manifest write on every run'

} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-scope-selection: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-scope-selection: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
