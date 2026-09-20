# #1053 item 4: `workspace tests` runs `cargo nextest run`, and this file is what that had to earn.
#
# MEASURED BEFORE A LINE WAS WRITTEN, alternating on one warm target, idle box:
#   cargo test    364 s, 346 s        nextest run    90 s, 99 s
#   burn-in, five consecutive nextest runs: 91 / 92 / 85 / 88 / 93 s, 3049 passed every time
# Seven green runs, zero flakes, so no `.config/nextest.toml` test-group ships with this: a group
# nothing has been observed to need is a guard that only fires because of a defect nobody measured.
#
# AND THE POPULATION IS IDENTICAL, which is the claim that makes the time claim mean anything.
# `cargo nextest list --message-format json` and `cargo test -- --list` each enumerate 3098 tests;
# a name-by-name multiset diff in BOTH directions is empty; 3049 run and 49 ignored on each side.
#
# TWO WAYS THIS COULD SILENTLY STOP RUNNING SOMETHING, and both have a cell below.
#
#   1. THE RUNNER GOES MISSING AND THE GATE FALLS BACK. A gate that quietly ran `cargo test`
#      instead would publish a green from a DIFFERENT INSTRUMENT than the one measured above, on a
#      machine nobody would think to ask about. The gate refuses instead, before its first stage,
#      on absent OR mismatched -- pinned exactly, like the Rust toolchain, because two nextest
#      versions can disagree about defaults that decide a verdict.
#
#   2. A RUST DOCTEST IS ADDED. nextest does not run doctests at all. Today that costs nothing --
#      every doc-comment fence in this workspace opens ```text -- but nothing MADE that true and
#      nothing would announce the day it stops. The scanner below is that announcement, and it
#      fails CLOSED: a fence tag it does not recognise as inert counts as a Rust doctest, so a new
#      tag has to be classified deliberately rather than inherited by silence.
#
# Same homegrown PASS/FAIL harness as the sibling gate suites; this repository carries no Pester.
# Exit codes: 0 all passed, 1 an assertion failed, 2 the harness could not vouch for the run.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   1  ARRANGEMENT: ci/tool-versions.json is readable and pins a cargo-nextest version
#   1  gate.ps1 reads that pin        1  and refuses on a version mismatch
#   1  and refuses when the runner is absent
#   1  the refusal happens BEFORE the first cargo invocation
#   1  CONTROL: the ordering check can fail, proved on hand-built text
#   1  `workspace tests` runs `nextest run` on the FULL branch
#   1  and on the SCOPED branch -- both, so a scoped run is not quietly measured by another tool
#   1  NO `cargo $toolchain test --workspace` survives anywhere: there is no fallback path
#   1  ARRANGEMENT: the doctest scanner found .rs files to read
#   1  DOCTEST GUARD: no Rust doctest fence exists anywhere in the workspace
#   1  CONTROL: the scanner FLAGS a hand-built ```rust fence
#   1  CONTROL: and a bare ``` opening fence, which rustdoc also runs as Rust
#   1  CONTROL: it does NOT flag ```text, or the bare ``` that CLOSES it
#   1  CONTROL: it ignores fences in ordinary comments and in code, not just doc comments
$ExpectedAssertionCount = 15

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

$repoRoot = Split-Path -Parent $PSScriptRoot
$gateText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate.ps1'))

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- the runner is pinned, and its absence refuses rather than falling back --' -ForegroundColor Cyan

$pinPath = Join-Path $PSScriptRoot 'tool-versions.json'
$pinned = ''
if (Test-Path -LiteralPath $pinPath) {
    try { $pinned = [string](([System.IO.File]::ReadAllText($pinPath) | ConvertFrom-Json).'cargo-nextest') } catch { $pinned = '' }
}
Assert-True ($pinned -match '^[0-9]+\.[0-9]+\.[0-9]+$') `
    "ARRANGEMENT: ci/tool-versions.json pins a cargo-nextest version ('$pinned'), so the cells below read a real pin"

Assert-True ($gateText.IndexOf("'cargo-nextest'", [System.StringComparison]::Ordinal) -ge 0 -and
    $gateText.IndexOf('tool-versions.json', [System.StringComparison]::Ordinal) -ge 0) `
    'gate.ps1 reads the pin out of ci/tool-versions.json rather than carrying a version of its own'
Assert-True ($gateText -match 'REFUSED: cargo-nextest \$nextestFound is installed') `
    'and REFUSES on a version mismatch -- "close enough" is a runner nobody named'
Assert-True ($gateText -match 'REFUSED: cargo-nextest is not installed') `
    'and refuses when the runner is absent, naming the install command'

# ORDERING: the refusal must precede every cargo invocation, because every stage after it is
# measured by this tool.
function Get-RefusalOrdering {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Text)
    $refuseAt = $Text.IndexOf('REFUSED: cargo-nextest is not installed', [System.StringComparison]::Ordinal)
    $cargoAt = $Text.IndexOf('cargo $toolchain', [System.StringComparison]::Ordinal)
    return [pscustomobject]@{ Ordered = ($refuseAt -ge 0 -and $cargoAt -gt $refuseAt); RefuseAt = $refuseAt; CargoAt = $cargoAt }
}
$ordering = Get-RefusalOrdering -Text $gateText
Assert-True ($ordering.Ordered) `
    "the refusal sits before the first ``cargo `$toolchain`` invocation (refusal at $($ordering.RefuseAt), cargo at $($ordering.CargoAt))"
Assert-True (-not (Get-RefusalOrdering -Text "cargo `$toolchain test`nREFUSED: cargo-nextest is not installed`n").Ordered) `
    'CONTROL: the ordering check fails on hand-built text with the two the wrong way round'

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- both branches of the stage use the pinned runner, and no fallback survives --' -ForegroundColor Cyan

$stageStart = $gateText.IndexOf("Invoke-Stage 'workspace tests' {", [System.StringComparison]::Ordinal)
$stageEnd = if ($stageStart -ge 0) { $gateText.IndexOf('} | Out-Null', $stageStart, [System.StringComparison]::Ordinal) } else { -1 }
$stageBody = if ($stageStart -ge 0 -and $stageEnd -gt $stageStart) { $gateText.Substring($stageStart, $stageEnd - $stageStart) } else { '' }

Assert-True ($stageBody -match 'cargo \$toolchain nextest run --workspace --all-features') `
    'the FULL branch runs `nextest run --workspace`'
Assert-True ($stageBody -match 'cargo \$toolchain nextest run @scopeArgs --all-features') `
    'and the SCOPED branch does too -- a scoped run must not be quietly measured by a different tool'

# NO FALLBACK ANYWHERE. `cargo $toolchain test` still appears for the per-suite cli loop and for the
# artefact build pass, which is correct; what must not exist is a WORKSPACE-wide `cargo test`, the
# shape a fallback would take.
Assert-True ($gateText.IndexOf('cargo $toolchain test --workspace', [System.StringComparison]::Ordinal) -lt 0) `
    'no workspace-wide `cargo $toolchain test` survives anywhere in gate.ps1 -- there is no fallback path to fall down'

# ------------------------------------------------------------------------------------------------
Write-Host ''
Write-Host '-- nextest does not run doctests, so a Rust doctest must be impossible to add quietly --' -ForegroundColor Cyan

# FAILS CLOSED. Anything not on this list counts as a Rust doctest, so a tag nobody classified is a
# red rather than an inheritance. rustdoc runs a fence whose info string is empty or names a Rust
# attribute (rust, should_panic, no_run, compile_fail, edition2021, ...); it leaves the rest alone.
$InertFenceTags = @('text', 'ignore', 'json', 'yaml', 'toml', 'console', 'sh', 'bash', 'powershell', 'diff', 'md', 'markdown', 'csv', 'ini')

function Find-RustDoctestFences {
    <#
        Walks doc-comment lines only (`///` and `//!`), tracking open/closed so the bare ``` that
        CLOSES a ```text block is not mistaken for an opening one.
    #>
    # NOT `[Parameter(Mandatory)]`, and that is not laziness. An EMPTY .rs file makes
    # `ReadAllLines` return an empty array; PowerShell unrolls that to nothing, Mandatory then
    # reports "cannot bind argument ... because it is an empty string", and the sweep DIES on that
    # file. A scanner that dies partway leaves the rest of the workspace unread while the cell above
    # it has already reported how many files it "found" -- a guard reporting on a population it
    # never finished walking. `[AllowEmptyCollection()]` does not fix it; dropping Mandatory does.
    param(
        [string[]] $Lines = @(),
        [string] $Path = '(text)'
    )
    $hits = New-Object System.Collections.Generic.List[string]
    $open = $false
    for ($i = 0; $i -lt $Lines.Count; $i++) {
        $line = $Lines[$i].Trim()
        if (-not ($line.StartsWith('///') -or $line.StartsWith('//!'))) { continue }
        $body = $line.Substring(3).Trim()
        if (-not $body.StartsWith('```')) { continue }
        if ($open) { $open = $false; continue }
        $open = $true
        $tag = $body.Substring(3).Trim().ToLowerInvariant()
        # An info string can carry several comma-separated words; the first decides the language.
        $first = ($tag -split '[,\s]')[0]
        if ([string]::IsNullOrEmpty($first) -or ($InertFenceTags -notcontains $first)) {
            $shown = if ([string]::IsNullOrEmpty($first)) { '(bare fence)' } else { $first }
            $hits.Add("$Path`:$($i + 1) -> $shown")
        }
    }
    return @($hits.ToArray())
}

$rustFiles = @(Get-ChildItem -LiteralPath $repoRoot -Recurse -Filter '*.rs' -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '[\\/](target|node_modules|\.git)[\\/]' })
Assert-True ($rustFiles.Count -gt 100) `
    "ARRANGEMENT: the scanner found $($rustFiles.Count) .rs files to read, so the guard below is not passing over an empty sweep"

$doctestHits = New-Object System.Collections.Generic.List[string]
foreach ($file in $rustFiles) {
    $rel = $file.FullName.Substring($repoRoot.Length).TrimStart('\', '/')
    foreach ($hit in (Find-RustDoctestFences -Lines ([System.IO.File]::ReadAllLines($file.FullName)) -Path $rel)) {
        $doctestHits.Add($hit)
    }
}
Assert-True ($doctestHits.Count -eq 0) `
    "DOCTEST GUARD: no Rust doctest fence exists in the workspace, so nextest run loses nothing by not running doctests. Offenders: $(if ($doctestHits.Count) { ($doctestHits -join ' | ') } else { 'none' }). If one is added deliberately, the gate needs a cargo test --doc stage BEFORE this cell is relaxed."

# CONTROLS. Without these the guard above passes on a scanner that finds nothing by construction.
Assert-True ((@(Find-RustDoctestFences -Lines @('/// ```rust', '/// let x = 1;', '/// ```'))).Count -eq 1) `
    'CONTROL: the scanner FLAGS a ```rust fence'
Assert-True ((@(Find-RustDoctestFences -Lines @('/// ```', '/// let x = 1;', '/// ```'))).Count -eq 1) `
    'CONTROL: and a BARE ``` fence, which rustdoc also compiles and runs as Rust'
Assert-True ((@(Find-RustDoctestFences -Lines @('/// ```text', '/// not rust', '/// ```'))).Count -eq 0) `
    'CONTROL: it does NOT flag ```text, nor read the bare ``` that closes it as a new opening'
Assert-True ((@(Find-RustDoctestFences -Lines @('// ```rust', '   let s = "```";'))).Count -eq 0) `
    'CONTROL: a fence in an ordinary comment or inside code is not a doc comment and is not flagged'

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
