# #903: the gate reads a scope selection, and every way of failing to read one runs the FULL gate.
#
# `ci/select-scope.ps1` decides what a change can reach; this is the other half -- what the GATE
# does with that decision, and specifically what it does when the decision is missing, unreadable
# or malformed. That is the half where a scoped gate becomes dangerous: a selection that cannot be
# read must widen the run, because the alternative is a gate that quietly runs less and stays green.
#
# The subject is `Read-ScopeSelection`, cut out of ci/gate.ps1 by anchor text and never retyped --
# running gate.ps1 would run the gate.

$ExpectedAssertionCount = 55
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


# #901 slice 2: the suites decision and the coverage record, cut by the same anchor rule.
. ([scriptblock]::Create((Import-FunctionFrom -Text $gateText -Name 'Get-PsSuitesScope' -From 'gate.ps1')))
. ([scriptblock]::Create((Import-FunctionFrom -Text $gateText -Name 'Get-RunCoverage' -From 'gate.ps1')))

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
    # A scoped run that does not RECORD what it skipped is unauditable: a reader of its manifest
    # would see a GREEN that covered a fraction of the workspace and could not tell.
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
    # #1053 item 4 renamed the runner on this line -- `test` became `nextest run` -- so the ANCHOR
    # moved with it. The claim it supports did not: a scoped run still narrows packages and never
    # cfg, whichever tool executes it. The anchor caught the rename by going red rather than by
    # quietly finding -1, which is what `$scopedInvocation -ge 0` in the cell below is for.
    $scopedInvocation = $gateText.IndexOf('cargo $toolchain nextest run @scopeArgs', [System.StringComparison]::Ordinal)
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


    # ---- THE MATRIX DECISION IS ACTED ON, AND A SKIP IS NOT A GREEN -----------------------------
    # Measured over 169 manifests: the two PostgreSQL matrices are 10.3 min of a 26.2 min run, 35%
    # of all gate time, and four of one day's seven PRs touched neither Rust nor SQL and paid it (X).
    # Recording the decision without acting on it is the whole saving not happening, and it looks
    # exactly like the saving happening -- the manifest says `matrix: false` either way.
    Assert-True ($gateText.IndexOf('$script:matrixSkipped', [System.StringComparison]::Ordinal) -ge 0) `
        'the gate computes a single matrix decision from the scope, rather than recording one and ignoring it'

    # The stage block must branch on it, not on -SkipPostgres alone.
    $branch = $gateText.IndexOf('if ($script:matrixSkipped) {', [System.StringComparison]::Ordinal)
    Assert-True ($branch -ge 0) 'and the PostgreSQL stage block branches on that decision'

    # A SKIPPED MATRIX IS NOT A GREEN MATRIX. `Get-RunCoverage` must be fed the EFFECTIVE skip, or a
    # scoped run records `coverage.complete = true` while never having touched persistence -- a
    # completeness claim nobody measured, in the field a later reader trusts most.
    Assert-True ($gateText.IndexOf('Get-RunCoverage -SkipPostgres ([bool]($script:matrixSkipped -or $script:postgresMatrixUnavailable))', [System.StringComparison]::Ordinal) -ge 0) `
        'coverage is computed from the EFFECTIVE skip, including an unavailable artifact build, so a scope-skipped run reports complete=false'

    # And the console says WHICH reason: a reader who cannot tell a scoped skip from a broken one
    # cannot act on either, and both leave the same hole in the record.
    Assert-True ($gateText.IndexOf('SKIPPED by scope', [System.StringComparison]::Ordinal) -ge 0) `
        'and a scope-skipped matrix names its reason rather than printing a bare SKIPPED'

    # ---- THE KNOWN-EMPTY SELECTION (#903, X): empty because nothing Rust changed ------------------
    # Two empties that used to be one. "No crate list" and "an empty list from a diff nobody could
    # explain" still mean FULL -- those are the guards below and they must not move. This one carries
    # `rustInputsChanged: false`, which is the producer saying the emptiness is a RESULT and not a gap.
    $known = Read-ScopeSelection -Path (New-SelectionFile -Name 'known-empty.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"matrixReason":"skipped: no Rust build input changed","rustInputsChanged":false}')
    Assert-True ($known.full -eq $false) `
        'a selection that is empty BECAUSE no Rust input changed is not a FULL run'
    Assert-True ($known.matrix -eq $false) `
        'and its matrix decision is honoured, which is the whole saving'
    Assert-True (@(Get-ScopePackageArgs -Scope $known).Count -eq 0) `
        'and it passes no -p arguments, so no stage can mistake it for a selection of crates'
    # #901 slice 2: THE SAVING, and the one place it may appear.
    Assert-True ($known['rust'] -eq $false) `
        'a selection that is empty BECAUSE no Rust input changed runs no Rust stage (rust=false)'
    Assert-True ((Get-ScopeRecord -Selection $known).rust -eq $false) `
        'and the RECORD says so, so the receipt cannot read as a Rust run'

    # CONTROL: the same shape WITHOUT the field is the old empty list, and still means FULL.
    $unexplained = Read-ScopeSelection -Path (New-SelectionFile -Name 'unexplained-empty.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"matrixReason":"skipped"}')
    Assert-True ($unexplained.full -eq $true) `
        'CONTROL: an empty crate list with no explanation is still FULL -- selecting nothing would run nothing and pass'
    Assert-True ($unexplained.matrix -eq $true) `
        'CONTROL: and a FULL run runs the matrix, whatever the selection asked for'

    # CONTROL: the field cannot be used to claim the emptiness when Rust DID change.
    $claimed = Read-ScopeSelection -Path (New-SelectionFile -Name 'claimed-empty.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"rustInputsChanged":true}')
    Assert-True ($claimed.full -eq $true) `
        'CONTROL: rustInputsChanged TRUE with an empty list is the unexplained case again, so FULL'

    # CONTROL: A WRONG TYPE MUST WIDEN. `-eq $false` alone coerced the right operand's type onto the
    # left, so the string 'false' and the integer 0 both satisfied it and NARROWED -- while the
    # string '0' widened, so the coercion was not even uniform. That inverts the rule this whole
    # state was added under: every way of being unsure runs everything. A producer emitting a JSON
    # string after a refactor, or a hand-written selection file, would have run fewer stages and
    # reported the same green. These three pin the two directions apart.
    # (Found by the second pass on PR #952 at c0f9504c, not by this suite.)
    #
    # Both fixtures carry `matrixReason` deliberately. The narrow branch reads it, so a fixture
    # without it makes the UNGUARDED code THROW rather than answer -- and a throw aborts the suite
    # into HARNESS-BROKE, which hides which cell moved. The sabotage has to produce a clean red.
    $stringFalse = Read-ScopeSelection -Path (New-SelectionFile -Name 'string-false.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"matrixReason":"skipped","rustInputsChanged":"false"}')
    Assert-True ($stringFalse.full -eq $true) `
        'CONTROL: the STRING "false" is not the producer saying no Rust input changed -- a wrong type is a way of being unsure, so FULL'

    $zero = Read-ScopeSelection -Path (New-SelectionFile -Name 'zero-false.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"matrixReason":"skipped","rustInputsChanged":0}')
    Assert-True ($zero.full -eq $true) `
        'CONTROL: the integer 0 is not a boolean false either, and it narrowed before the -is [bool] guard'

    $boolFalse = Read-ScopeSelection -Path (New-SelectionFile -Name 'bool-false.json' `
        -Content '{"escalated":false,"crates":[],"matrix":false,"matrixReason":"skipped","rustInputsChanged":false}')
    Assert-True ($boolFalse.full -eq $false) `
        'and the REAL boolean false still narrows -- the guard rejects wrong types without rejecting the case it exists for'

    # ---- #901 slice 2: `rust` IS TRUE EVERYWHERE ELSE ----------------------------------------
    # CONTROLS for the cell above: every other branch of Read-ScopeSelection must run Rust, or the
    # saving has leaked into a state that did not prove "no Rust input changed".
    Assert-True ($unexplained['rust'] -eq $true) 'CONTROL: an unexplained empty list runs Rust (FULL)'
    Assert-True ($claimed['rust'] -eq $true) 'CONTROL: rustInputsChanged TRUE runs Rust'
    Assert-True ($stringFalse['rust'] -eq $true -and $zero['rust'] -eq $true) 'CONTROL: a wrong-typed false runs Rust'
    Assert-True ((Read-ScopeSelection -Path '')['rust'] -eq $true) 'CONTROL: no selection at all runs Rust'
    $crateScope = Read-ScopeSelection -Path (New-SelectionFile -Name 'rust-crates.json' `
        -Content '{"escalated":false,"crates":["core-leaf"],"matrix":false,"matrixReason":"x","rustInputsChanged":false}')
    Assert-True ($crateScope['rust'] -eq $true) `
        'CONTROL: a selection that NAMES crates runs Rust even if it also claims rustInputsChanged false'
    Assert-True ((Get-ScopeRecord -Selection ([ordered]@{ full = $false; reason = 'r'; crates = @(); matrix = $false; matrixReason = '' })).rust -eq $true) `
        'CONTROL: a record built from a selection WITHOUT the key says Rust ran -- absent never reads as skipped'

    # ---- #901 slice 2: the PowerShell suites decision fails WIDE -------------------------------
    $suitesNone = Get-PsSuitesScope -Path ''
    Assert-True ($suitesNone.included -eq $true) 'no selection runs every suite'
    $suitesNo = Get-PsSuitesScope -Path (New-SelectionFile -Name 'suites-no.json' `
        -Content '{"escalated":false,"crates":["a"],"psSuites":false,"psSuitesReason":"skipped: nothing under ci/ names it"}')
    Assert-True ($suitesNo.included -eq $false) 'a real boolean psSuites=false narrows the suites stage'
    Assert-True ($suitesNo.reason -match 'nothing under ci/ names it') 'and carries the selector''s own reason'
    $suitesString = Get-PsSuitesScope -Path (New-SelectionFile -Name 'suites-string.json' `
        -Content '{"escalated":false,"crates":["a"],"psSuites":"false"}')
    Assert-True ($suitesString.included -eq $true) 'CONTROL: the STRING "false" runs every suite'
    $suitesAbsent = Get-PsSuitesScope -Path (New-SelectionFile -Name 'suites-absent.json' -Content '{"escalated":false,"crates":["a"]}')
    Assert-True ($suitesAbsent.included -eq $true) 'CONTROL: a selection without the field runs every suite'
    $suitesEsc = Get-PsSuitesScope -Path (New-SelectionFile -Name 'suites-esc.json' `
        -Content '{"escalated":true,"escalationRule":"ci/","crates":[],"psSuites":false}')
    Assert-True ($suitesEsc.included -eq $true) 'CONTROL: an escalated selection runs every suite whatever psSuites says'
    $suitesBroken = Get-PsSuitesScope -Path (New-SelectionFile -Name 'suites-broken.json' -Content '{ nope')
    Assert-True ($suitesBroken.included -eq $true) 'CONTROL: an unreadable selection runs every suite'

    # ---- #901 slice 2: coverage says what was skipped, and `complete` takes every skip -----------
    $covAll = Get-RunCoverage -SkipPostgres $false -BuildMode 'cold'
    Assert-True ($covAll.complete -eq $true -and $covAll.rust -eq 'included' -and $covAll.psSuites -eq 'included') `
        'CONTROL: a run that skipped nothing is complete, with Rust and suites included'
    $covNoRust = Get-RunCoverage -SkipPostgres $true -BuildMode 'unknown' -SkipRust $true
    Assert-True ($covNoRust.complete -eq $false -and $covNoRust.rust -eq 'skipped') `
        'a run with no Rust stage records rust=skipped and is not complete'
    $covNarrow = Get-RunCoverage -SkipPostgres $false -BuildMode 'cold' -SkipPsSuites $true
    Assert-True ($covNarrow.complete -eq $false -and $covNarrow.psSuites -eq 'narrowed') `
        'a run with narrowed suites records psSuites=narrowed and is not complete'

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
