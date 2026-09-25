# The three schema stages are `cargo run`, and `cargo run` builds under the DEV profile.
#
# `Cargo.toml` declares `[profile.test] debug = 1`. The default dev profile carries `debug = 2`. A
# differing `-C debuginfo` is a different compiled unit to cargo, so each of these three stages --
# running immediately after a `cli:` loop that just built this exact crate's whole dependency chain
# under the TEST profile -- recompiled it all again to change one debuginfo level.
#
# Measured over the 370 gate receipts committed before the receipt store was retired (2026-09-24):
# `schema catalog` n=358, p10 51.7 s, median 73.2 s, p90 153.3 s -- while `schema baseline
# compatibility` (0.7 s) and `schema conformance` (0.5 s), which reuse the binary `schema catalog`
# just built, are free. That asymmetry is the proof the cost is a BUILD and not schema work: the
# same program answering three questions cannot be a hundred times slower on the first one for any
# other reason.
#
# `--profile test` on all three makes them reuse the loop's unit. It changes exactly one compiler
# flag (`-C debuginfo=2` -> `-C debuginfo=1`); feature resolution, opt-level, debug-assertions,
# overflow-checks and panic are identical.
#
# WHAT THIS SUITE IS FOR, AND WHY GUARD B IS THE LOAD-BEARING HALF. Guard A pins the flag onto the
# three command lines. That alone would rot: the saving exists only while `[profile.test]` differs
# from dev in a way that costs nothing semantically. The day someone adds `opt-level` or
# `debug-assertions` to that table, `--profile test` stops meaning "the same program, cheaper" and
# starts meaning "a DIFFERENT program answers the schema stages than the one the gate tested".
# Guard B fails on that day and names the key, so the flag is removed deliberately rather than
# carried forward as a silent change of subject.
#
# Same homegrown PASS/FAIL harness as the sibling gate suites; this repository carries no Pester.
# Exit codes: 0 all passed, 1 an assertion failed, 2 the harness could not vouch for the run.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   1  ARRANGEMENT: gate.ps1 is readable and holds exactly three `cargo $toolchain run` lines
#   3  each schema sub-command sits on exactly ONE of them (the anchor is unique, per sub-command)
#   3  GUARD A: each of those three lines carries `--profile test`
#   1  CONTROL: the flag detector reports a hand-built line WITHOUT the flag as missing
#   1  ARRANGEMENT: Cargo.toml is readable and `[profile.test]` is locatable
#   1  GUARD B: `[profile.test]`'s key set is exactly {debug}
#   1  CONTROL: the table parser SEES a second key in hand-built text
#   1  CONTROL: the parser reads the `test` table and not merely "a profile table"
#   1  CONTROL: the real `[profile.release]` carries keys, so the decoy above separates two states
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

# ------------------------------------------------------------------------------------------------
# Guard A -- the flag is on all three stage command lines
# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- guard A: the three schema stages build under the test profile --' -ForegroundColor Cyan

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# THE POPULATION IS ASSERTED, NOT ASSUMED. Three is the number of `cargo run` invocations this gate
# makes; a fourth added later is a stage this suite has never seen and must not silently pass as
# covered. The count fires in BOTH directions, which a per-line check alone cannot do.
$runLines = @(($gateText -split "`r?`n") | Where-Object { $_ -match 'cargo\s+\$toolchain\s+run\b' })
Assert-True ($runLines.Count -eq 3) `
    "ARRANGEMENT: gate.ps1 holds exactly three ``cargo `$toolchain run`` lines, the population this suite covers (found $($runLines.Count))"

# ANCHORED ON THE SUB-COMMAND, NEVER ON `cargo $toolchain run`. That string matches all three lines
# and would prove nothing about any one of them: a flag present on `schema catalog` alone would
# satisfy a check written against the shared prefix while two stages still recompiled.
function Get-StageLine {
    param([Parameter(Mandatory)] [string[]] $Lines, [Parameter(Mandatory)] [string] $SubCommand)
    return @($Lines | Where-Object { $_.IndexOf("-- schema $SubCommand", [System.StringComparison]::Ordinal) -ge 0 })
}

function Test-CarriesTestProfile {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Line)
    return ($Line.IndexOf('--profile test', [System.StringComparison]::Ordinal) -ge 0)
}

foreach ($sub in @('catalog', 'check', 'conformance')) {
    $matched = @(Get-StageLine -Lines $runLines -SubCommand $sub)
    Assert-True ($matched.Count -eq 1) `
        "the ``schema $sub`` stage sits on exactly one ``cargo run`` line, so the cell below names one command and not a class (found $($matched.Count))"
}

foreach ($sub in @('catalog', 'check', 'conformance')) {
    $matched = @(Get-StageLine -Lines $runLines -SubCommand $sub)
    $line = if ($matched.Count -eq 1) { [string]$matched[0] } else { '' }
    Assert-True (Test-CarriesTestProfile -Line $line) `
        "GUARD A: the ``schema $sub`` stage carries ``--profile test``, so it reuses the unit the cli loop already built instead of recompiling the dependency chain for a debuginfo level"
}

# WITHOUT THIS THE THREE CELLS ABOVE COULD BE VACUOUS. A detector that answered true for everything
# would pass them on a file that never carried the flag.
Assert-True (-not (Test-CarriesTestProfile -Line 'cargo $toolchain run --locked -q -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json')) `
    'CONTROL: the detector reports a hand-built command line WITHOUT the flag as missing, so the three cells above are measuring its presence'

# ------------------------------------------------------------------------------------------------
# Guard B -- the flag keeps meaning what it means only while [profile.test] is debuginfo-only
# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- guard B: [profile.test] stays a debuginfo-only difference from dev --' -ForegroundColor Cyan

# Reads ONE named table and stops at the next header. A parser that ran to end-of-file would fold
# `[profile.release]`'s keys into the answer and report `lto` as a key of `[profile.test]`, which is
# the decoy two cells below.
function Get-ProfileTableKeys {
    param([Parameter(Mandatory)] [string] $Text, [Parameter(Mandatory)] [string] $Table)
    $keys = New-Object System.Collections.Generic.List[string]
    $inTable = $false
    foreach ($raw in ($Text -split "`r?`n")) {
        $line = $raw.Trim()
        if ($line.StartsWith('[')) {
            $inTable = [string]::Equals($line, "[$Table]", [System.StringComparison]::Ordinal)
            continue
        }
        if (-not $inTable) { continue }
        if ($line.Length -eq 0 -or $line.StartsWith('#')) { continue }
        $eq = $line.IndexOf('=', [System.StringComparison]::Ordinal)
        if ($eq -gt 0) { $keys.Add($line.Substring(0, $eq).Trim()) }
    }
    return @($keys.ToArray())
}

$cargoPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'
$cargoText = [System.IO.File]::ReadAllText($cargoPath)
$testKeys = @(Get-ProfileTableKeys -Text $cargoText -Table 'profile.test')
Assert-True ($testKeys.Count -ge 1) `
    "ARRANGEMENT: Cargo.toml is readable and [profile.test] is locatable, so the cell below is reading the real table (keys: $(if ($testKeys.Count) { $testKeys -join ', ' } else { 'none' }))"

$unexpected = @($testKeys | Where-Object { -not [string]::Equals($_, 'debug', [System.StringComparison]::Ordinal) })
Assert-True ($unexpected.Count -eq 0) `
    "GUARD B: [profile.test] carries only ``debug``, so ``--profile test`` is a debuginfo difference and nothing else. Offending key(s): $(if ($unexpected.Count) { $unexpected -join ', ' } else { 'none' }). If a semantic key is added here, the three schema stages must DROP --profile test: they would otherwise run a different program than the one the gate tested."

# The parser must be able to SEE a second key, or Guard B passes on a table it cannot read.
$twoKeys = "[profile.test]`ndebug = 1`nopt-level = 1`n"
Assert-True ((@(Get-ProfileTableKeys -Text $twoKeys -Table 'profile.test')).Count -eq 2) `
    'CONTROL: the parser sees a second key in hand-built text, so Guard B would fail the day one is added'

# THE ANCHOR IS THE `test` TABLE, NOT "A PROFILE TABLE". Without this decoy a parser that ran past
# the header would read `[profile.release]`'s keys and Guard B would fail for the wrong reason --
# or, with the tables the other way round, pass while blind.
$decoy = "[profile.release]`nlto = `"thin`"`ncodegen-units = 1`n`n[profile.test]`ndebug = 1`n"
$decoyKeys = @(Get-ProfileTableKeys -Text $decoy -Table 'profile.test')
Assert-True ($decoyKeys.Count -eq 1 -and [string]::Equals($decoyKeys[0], 'debug', [System.StringComparison]::Ordinal)) `
    "CONTROL: a neighbouring [profile.release] with two keys does not leak into the answer (got: $(if ($decoyKeys.Count) { $decoyKeys -join ', ' } else { 'none' }))"

# And the decoy separates two real states only if the repository's own release table has keys.
$releaseKeys = @(Get-ProfileTableKeys -Text $cargoText -Table 'profile.release')
Assert-True ($releaseKeys.Count -ge 1) `
    "CONTROL: the repository's own [profile.release] carries keys ($(if ($releaseKeys.Count) { $releaseKeys -join ', ' } else { 'none' })), so the decoy above is separating two states that both occur here"

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
