# #1053 item 4: incremental compilation is off for the GATE, and only for the gate.
#
# Incremental buys a second build of a tree that CHANGED. This gate builds a tree that will not
# change again -- one pass, then the run ends -- so every incremental artefact it writes is work
# done for a reuse that never comes. It is not free: measured on this workspace, alternating
# on->off->on->off with a cold target each time, `cargo test --workspace --all-features --no-run`
#
#     CARGO_INCREMENTAL=1   77 s, 69 s    30 403 files in the target, 25 871 of them incremental
#     CARGO_INCREMENTAL=0   63 s, 58 s     4 532 files in the target,      0 of them incremental
#
# ~12 s and 26 000 fewer files per run. The file count is the half that matters most here: the
# target inventory already measures a real target at 23.1 GB across 68 364 files, and until #1053
# item 3 those files were landing on a mechanical disk that sampled at 1005 % disk time.
#
# IT LIVES IN ci/gate.ps1 AND NOT IN .cargo/config.toml, and the placement is the subject of the
# last cell in this file. Incremental is pure overhead for a one-shot verification and pure benefit
# for an interactive edit loop, so moving it into the committed cargo config would take it away from
# every developer to solve a gate-only cost. That is a plausible "tidy-up" that would cost something
# real and redden nothing, which is exactly what a cell is for.
#
# A REFUSED ALTERNATIVE, RECORDED SO IT IS NOT RE-DERIVED. The same investigation proposed linking
# with `rust-lld` instead of MSVC's `link.exe`. It works at the pinned 1.97.1, it keeps backtraces
# (a panic in an lld-linked binary still named `src\main.rs:1:13`), and it was measured on this
# workspace, alternating, both arms on the same NVMe:
#
#     link.exe   52 s, 51 s          rust-lld   50 s, 48 s        (291 crates, 267 binaries, 0 Fresh)
#
# ~3 s, on a build pass that is ~0.2 % of a gate. That does not buy a new failure mode in the one
# instrument this project has, so it was NOT shipped. The gate-under-five-minutes plan (see
# docs/process/DELIVERY.md, "History") carries the numbers. Note also that
# `-C linker-features=+lld`, which reads like the modern spelling, is UNSTABLE at 1.97.1 and
# refuses without `-Z unstable-options`.
#
# Same homegrown PASS/FAIL harness as the sibling gate suites; this repository carries no Pester.
# Exit codes: 0 all passed, 1 an assertion failed, 2 the harness could not vouch for the run.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   1  ARRANGEMENT: ci/gate.ps1 is readable and its first `cargo $toolchain` invocation is locatable
#   1  gate.ps1 sets CARGO_INCREMENTAL to '0'
#   1  at TOP LEVEL (column 0), so file order is execution order for it
#   1  and textually BEFORE the first cargo invocation
#   1  CONTROL: the ordering check can fail -- proved on hand-built text with the order reversed
#   1  CONTROL: the setter is not satisfied by the name appearing in a comment
#   1  the setting is NOT in .cargo/config.toml, where it would cost every interactive build
$ExpectedAssertionCount = 7

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

Write-Host ''
Write-Host '-- incremental is off for the gate, and only for the gate --' -ForegroundColor Cyan

$repoRoot = Split-Path -Parent $PSScriptRoot
$gateText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate.ps1'))

function Get-IncrementalOrdering {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Text)
    # ANCHORED AT COLUMN 0 IN THE SEARCH ITSELF, so a mention inside a comment -- this file's own
    # header names the variable six times, and gate.ps1's does too -- cannot satisfy it.
    $setMatch = [regex]::Match($Text, "(?m)^\`$env:CARGO_INCREMENTAL = '0'")
    # `cargo $toolchain` is how EVERY cargo invocation in gate.ps1 is spelled, so the first one is
    # the first real build. Anchoring on the bare word `cargo` would match that file's prose.
    $cargoAt = $Text.IndexOf('cargo $toolchain', [System.StringComparison]::Ordinal)
    $setAt = if ($setMatch.Success) { $setMatch.Index } else { -1 }
    return [pscustomobject]@{ SetAt = $setAt; CargoAt = $cargoAt; Ordered = ($setAt -ge 0 -and $cargoAt -gt $setAt) }
}

$ordering = Get-IncrementalOrdering -Text $gateText
Assert-True ($ordering.CargoAt -ge 0) `
    "ARRANGEMENT: gate.ps1's first ``cargo `$toolchain`` invocation is locatable at offset $($ordering.CargoAt), so the ordering cell below has a second point to measure against"
Assert-True ($ordering.SetAt -ge 0) `
    'gate.ps1 sets CARGO_INCREMENTAL to ''0'', so the authoritative gate does not write 26 000 files for a reuse that never comes'
Assert-True ($gateText -match "(?m)^\`$env:CARGO_INCREMENTAL = '0'") `
    'and does it at top level (column 0), so its position in the file IS its position in the run'
Assert-True ($ordering.Ordered) `
    "and textually before that first cargo invocation (set at $($ordering.SetAt), cargo at $($ordering.CargoAt)), so the contamination canary is compiled under the same configuration as everything it vouches for"

# WITHOUT THESE TWO THE CELLS ABOVE COULD BE VACUOUS: one passes on any file containing both
# strings in any order, the other on any file that merely discusses the variable.
$reversed = "cargo `$toolchain test -p ci-canary`n`$env:CARGO_INCREMENTAL = '0'`n"
Assert-True (-not (Get-IncrementalOrdering -Text $reversed).Ordered) `
    'CONTROL: the ordering check fails on hand-built text with the two the wrong way round'
$commentOnly = "# `$env:CARGO_INCREMENTAL = '0'  <- removed, see #NNNN`ncargo `$toolchain test`n"
Assert-True (-not (Get-IncrementalOrdering -Text $commentOnly).Ordered) `
    'CONTROL: the setting named only inside a COMMENT does not satisfy it, so a removal that leaves the explanation behind still reds'

# THE PLACEMENT IS A CLAIM IN ITS OWN RIGHT. `.cargo/config.toml` does not exist in this repository
# today, and if one is ever added this setting must not migrate into it.
$cargoConfigPath = Join-Path $repoRoot '.cargo\config.toml'
$cargoConfigText = if (Test-Path -LiteralPath $cargoConfigPath) { [System.IO.File]::ReadAllText($cargoConfigPath) } else { '' }
Assert-True ($cargoConfigText.IndexOf('CARGO_INCREMENTAL', [System.StringComparison]::Ordinal) -lt 0) `
    'CARGO_INCREMENTAL is not set in .cargo/config.toml: there it would take incremental away from every interactive build to solve a gate-only cost'

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
