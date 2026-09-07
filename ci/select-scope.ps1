<#
.SYNOPSIS
    Derive the gate's scope from what the change can REACH (#903, epic #901, deliverable 2).

.DESCRIPTION
    Maps every changed path to its crate, expands to all transitive dependents over BOTH
    `dependencies` and `dev-dependencies`, and prints the selection as JSON for the gate and for
    `merge-proof` to record. It runs nothing and decides nothing about colour: it answers "which
    crates can this change reach", and the caller runs them.

    IT FAILS CLOSED, AND THAT IS THE WHOLE DESIGN. The escalation list below is a deny-list over a
    CLASS -- "a path whose change can invalidate the dependency graph this selection is derived
    FROM" -- and a list of names cannot see the next member of its class. So every state this
    script cannot map widens the run instead of narrowing it: a path that matches no crate, a
    metadata document that does not parse, a manifest it cannot read. A selector that guesses
    narrow is a selector that silently stops running the stage that would have gone red, and the
    failure is invisible because the remaining stages pass.

    The compiled graph sees COMPILE-TIME coupling only. Semantic drift between `core/protocols` and
    `core/schema` does not appear in it at all -- the board already carries that lesson as "compile
    radius is not semantic radius" -- so those paths are escalation rules rather than graph nodes.

.PARAMETER ChangedFiles
    ';'-separated repo-relative paths. Omitted, they come from `git diff --name-only`.

.PARAMETER MetadataPath
    A `cargo metadata --format-version 1` document. Omitted, cargo is asked directly. Present, no
    cargo runs -- which is what lets the cells build a graph containing the dev-dependency edge and
    the unmapped path that this workspace does not happen to have today.
#>
param(
    [string] $ChangedFiles,
    [string] $MetadataPath,
    [string] $RepoRoot = '.',
    [string] $MergeBase,
    [string] $Head = 'HEAD'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Each rule is a NAME and a predicate, so the selection can report WHICH one fired. A boolean would
# be enough to widen the run and useless to a reader: a selector that escalates for the wrong reason
# keeps escalating, keeps passing, and stops the day that reason moves -- with nothing to notice it.
$EscalationRules = @(
    @{ Name = 'ci/'; Test = { param($p) $p -like 'ci/*' } }
    @{ Name = 'schemas/'; Test = { param($p) $p -like 'schemas/*' } }
    @{ Name = 'core/protocols/'; Test = { param($p) $p -like 'core/protocols/*' } }
    @{ Name = 'Cargo.toml'; Test = { param($p) $p -eq 'Cargo.toml' -or $p -like '*/Cargo.toml' } }
    @{ Name = 'Cargo.lock'; Test = { param($p) $p -eq 'Cargo.lock' -or $p -like '*/Cargo.lock' } }
    @{ Name = 'build.rs'; Test = { param($p) $p -eq 'build.rs' -or $p -like '*/build.rs' } }
    @{ Name = 'rust-toolchain'; Test = { param($p) $p -like 'rust-toolchain*' -or $p -like '*/rust-toolchain*' } }
)

function ConvertTo-RepoPath {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    # `TrimStart('./')` takes a CHAR ARRAY, not a prefix: it strips every leading '.' AND '/',
    # so `.factory/gate-runs/x.json` came back as `factory/...` -- a path matching no crate and
    # no escalation rule, which then escalated every run as 'unmapped'. Measured on the real
    # repository; no fixture had a dotfile, so no cell could have caught it.
    $normalised = $Path -replace '\\', '/'
    while ($normalised.StartsWith('./')) { $normalised = $normalised.Substring(2) }
    return $normalised
}

function Write-Selection {
    param(
        [Parameter(Mandatory)] [bool] $Escalated,
        [AllowNull()] [string] $Rule,
        [string[]] $Crates = @(),
        [string[]] $Changed = @(),
        [string[]] $Unmapped = @(),
        [bool] $Matrix = $true,
        [string] $MatrixReason = '',
        # TRUE unless a caller says otherwise: an omission must read as "Rust input may have
        # changed", which is the answer that widens.
        [bool] $RustInputsChanged = $true
    )
    # `crates` is EMPTY on an escalation on purpose: a FULL run has no selection, and printing the
    # partial one the script had computed invites a caller to use it.
    $document = [ordered]@{
        escalated      = $Escalated
        escalationRule = $Rule
        crates         = @($Crates | Sort-Object -Unique)
        changedFiles   = @($Changed)
        unmapped       = @($Unmapped)
        matrix         = $Matrix
        matrixReason   = $MatrixReason
        rustInputsChanged = $RustInputsChanged
    }
    Write-Output ($document | ConvertTo-Json -Depth 6 -Compress)
}

# ---- the changed set -----------------------------------------------------------------------
if ($PSBoundParameters.ContainsKey('ChangedFiles') -and -not [string]::IsNullOrWhiteSpace($ChangedFiles)) {
    $changed = @($ChangedFiles -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { ConvertTo-RepoPath -Path $_.Trim() })
} else {
    if ([string]::IsNullOrWhiteSpace($MergeBase)) {
        Write-Selection -Escalated $true -Rule 'no-merge-base' -MatrixReason 'FULL run: no merge base to diff against'
        exit 0
    }
    $diff = @(& git -C $RepoRoot diff --name-only $MergeBase $Head 2>&1)
    if ($LASTEXITCODE -ne 0) {
        # The diff failing is not "nothing changed". An empty changed set would select nothing and
        # run nothing, which is the widest possible failure wearing the narrowest possible output.
        Write-Selection -Escalated $true -Rule 'diff-failed' -MatrixReason 'FULL run: git diff failed, so the changed set is unknown'
        exit 0
    }
    $changed = @($diff | ForEach-Object { ConvertTo-RepoPath -Path ([string]$_) } | Where-Object { $_ })
}

if ($changed.Count -eq 0) {
    Write-Selection -Escalated $true -Rule 'empty-diff' -Changed $changed `
        -MatrixReason 'FULL run: the changed set is empty, which is a question about the diff and not an answer about scope'
    exit 0
}

# ---- KNOWN AND NOT BUILD INPUT --------------------------------------------------------------
#
# A THIRD state, and it exists because the first two collapse two different claims into one output.
# `unmapped-path` means "this path is in no class I know" and `crates: []` means "the selection is
# empty"; both widen, correctly, because an unknown must never narrow. But a `ci/*.tests.ps1` file
# is neither unknown nor build input: it is a PowerShell suite FOR the gate, discovered by
# `ci/run-ps-suites.ps1` and run in a stage that is unconditional. It cannot change what a Rust
# stage measures.
#
# MEASURED BEFORE IT WAS DESIGNED (X, 2026-09-06): the selector was run against the real diff of
# every pull request the fleet handled that day. One of eight skipped the PostgreSQL matrices; five
# of the other seven were exactly this class, escalating under the `ci/` deny-list. The naive
# repair -- exempting suites from that rule -- was measured by K and buys nothing: the path then
# reaches the unmapped rule and escalates for a different reason. So the class has to be NAMED, and
# the emptiness it produces has to be distinguishable from the emptiness nobody can explain.
#
# THE MEMBERSHIP IS DELIBERATELY NARROW. `ci/gate.ps1`, `ci/merge-proof.ps1` and `ci/select-scope.ps1`
# stay on the escalation list: a change to the gate changes what every other stage measures, which
# is a different claim from "a suite for the gate changed". Adding a member here is loosening an
# escalation, so it is an edit a reviewer sees and a cell has to survive.
$KnownNonBuildInput = @(
    @{ Name = 'ci-suite'; Test = { param($p) $p -like 'ci/*.tests.ps1' } }
)

function Test-KnownNonBuildInput {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    foreach ($class in $KnownNonBuildInput) { if (& $class.Test $Path) { return $true } }
    return $false
}

# The escalation rules run FIRST and this class is subtracted from what they see, so a suite file
# does not trip `ci/`. Everything else in the diff still reaches every rule below unchanged.
# Run manifests are subtracted here TOO, but only so they cannot make a suite-only diff look mixed.
# They keep their own handling further down (`manifest-only` escalates to FULL), and this block
# refuses to fire on them alone: a diff of nothing but receipts is a question about the diff, and
# the answer to that question is already written below.
$RunManifestPrefix = '.factory/gate-runs/'
$suitePaths = @($changed | Where-Object { Test-KnownNonBuildInput -Path $_ })
$buildInput = @($changed | Where-Object {
    -not (Test-KnownNonBuildInput -Path $_) -and
    -not $_.StartsWith($RunManifestPrefix, [System.StringComparison]::Ordinal)
})

if ($buildInput.Count -eq 0 -and $suitePaths.Count -gt 0) {
    # KNOWN empty, and the reason says which class made it empty. `rustInputsChanged` is the field
    # the gate reads to tell this apart from an empty list it could not explain -- see the guard in
    # `Read-ScopeSelection`, which still answers FULL when the field is absent or true.
    Write-Selection -Escalated $false -Rule '' -Changed $changed -Crates @() `
        -Matrix $false `
        -MatrixReason "skipped: no Rust build input changed -- every changed path is a known non-build class (ci-suite): $($suitePaths -join ', ')" `
        -RustInputsChanged $false
    exit 0
}

# ---- escalation, BEFORE any selection ------------------------------------------------------
foreach ($rule in $EscalationRules) {
    foreach ($path in $buildInput) {
        if (& $rule.Test $path) {
            Write-Selection -Escalated $true -Rule $rule.Name -Changed $changed `
                -MatrixReason "FULL run: $($rule.Name) changed ($path)"
            exit 0
        }
    }
}

# ---- the graph -----------------------------------------------------------------------------
try {
    if ($PSBoundParameters.ContainsKey('MetadataPath') -and -not [string]::IsNullOrWhiteSpace($MetadataPath)) {
        $metadata = [System.IO.File]::ReadAllText($MetadataPath) | ConvertFrom-Json
    } else {
        $raw = & cargo metadata --format-version 1 --no-deps --manifest-path (Join-Path $RepoRoot 'Cargo.toml') 2>&1
        if ($LASTEXITCODE -ne 0) { throw "cargo metadata exited $LASTEXITCODE" }
        $metadata = ($raw -join "`n") | ConvertFrom-Json
    }
    if ($null -eq $metadata -or -not $metadata.packages) { throw 'metadata carried no packages' }
} catch {
    # FAIL CLOSED. A graph that could not be read cannot narrow anything, and the reason travels
    # with the escalation so nobody has to guess whether cargo was missing or the JSON was damaged.
    Write-Selection -Escalated $true -Rule 'metadata-unreadable' -Changed $changed `
        -MatrixReason "FULL run: the dependency graph could not be read ($($_.Exception.Message))"
    exit 0
}

$rootPrefix = (ConvertTo-RepoPath -Path ((Resolve-Path -LiteralPath $RepoRoot -ErrorAction SilentlyContinue).Path)) + '/'
$crates = @{}
foreach ($package in $metadata.packages) {
    $manifest = ConvertTo-RepoPath -Path ([string]$package.manifest_path)
    if ($manifest.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $manifest = $manifest.Substring($rootPrefix.Length)
    }
    $directory = ($manifest -replace '/Cargo\.toml$', '')
    if ($directory -eq $manifest) { $directory = '' }
    $crates[[string]$package.name] = [ordered]@{
        name      = [string]$package.name
        directory = $directory
        # BOTH kinds, and the dev edge is the one that matters: integration tests live in
        # dev-dependencies, so a `dependencies`-only walk drops exactly the crate whose tests
        # exercise the change -- silently, and with every remaining stage green.
        deps      = @(if ($package.dependencies) { $package.dependencies | ForEach-Object { [string]$_.name } } else { @() })
    }
}

# THE GATE'S OWN RECEIPT IS NOT BUILD INPUT -- the same reasoning #899 used for the freeze rule.
# #674(a) makes every authoritative run commit its manifest under this prefix onto the branch it
# judged, so from a branch's SECOND run on the store is always in the diff. Left to the unmapped
# rule below it maps to no crate and escalates the run to FULL, every time, for ever.
#
# Measured on #919's real range, where the only other changed file was one `apps/cli` test:
#   escalationRule "unmapped-path", unmapped [".factory/gate-runs/a50e7d0a24ee-....json"]
#
# A scope selector that escalates on its own gate's receipt can never narrow anything -- this
# deliverable failing completely, with every existing cell still green, because no fixture
# contained a manifest path. The exemption is this prefix and nothing wider: any other file under
# `.factory/` still reaches the unmapped rule and still escalates.
$RunManifestStore = '.factory/gate-runs/'
$changed = @($changed | Where-Object { -not $_.StartsWith($RunManifestStore, [System.StringComparison]::Ordinal) })
if ($changed.Count -eq 0) {
    Write-Selection -Escalated $true -Rule 'manifest-only' -Changed @() `
        -MatrixReason 'FULL run: the only changes are run manifests, which are not build input, so there is nothing to derive a scope from'
    exit 0
}

# ---- map changed paths to crates, by LONGEST directory prefix -------------------------------
$seeds = New-Object System.Collections.Generic.HashSet[string]
$unmapped = New-Object System.Collections.Generic.List[string]
foreach ($path in $buildInput) {
    $best = $null
    $bestLength = -1
    foreach ($crate in $crates.Values) {
        if ([string]::IsNullOrEmpty($crate.directory)) { continue }
        $prefix = $crate.directory + '/'
        if ($path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) -and $prefix.Length -gt $bestLength) {
            $best = $crate.name
            $bestLength = $prefix.Length
        }
    }
    if ($null -eq $best) { $unmapped.Add($path) } else { [void]$seeds.Add($best) }
}

if ($unmapped.Count -gt 0) {
    # A path nobody owns is not a path that changes nothing. It is the case the deny-list above did
    # not anticipate, which is the one state where narrowing is least defensible.
    Write-Selection -Escalated $true -Rule 'unmapped-path' -Changed $changed -Unmapped $unmapped.ToArray() `
        -MatrixReason "FULL run: $($unmapped.Count) changed path(s) map to no crate: $($unmapped -join ', ')"
    exit 0
}

# ---- expand to transitive DEPENDENTS (reverse edges) ----------------------------------------
$selected = New-Object System.Collections.Generic.HashSet[string]
foreach ($seed in $seeds) { [void]$selected.Add($seed) }
$changedInPass = $true
while ($changedInPass) {
    $changedInPass = $false
    foreach ($crate in $crates.Values) {
        if ($selected.Contains($crate.name)) { continue }
        foreach ($dependency in $crate.deps) {
            if ($selected.Contains($dependency)) {
                [void]$selected.Add($crate.name)
                $changedInPass = $true
                break
            }
        }
    }
}

# ---- the PostgreSQL matrix -------------------------------------------------------------------
$matrix = $false
$matrixReason = 'skipped: nothing in the selection reaches adapters/postgres-event-store/'
foreach ($name in $selected) {
    $directory = $crates[$name].directory
    if ($directory -like 'adapters/postgres-event-store*') {
        $matrix = $true
        $matrixReason = "ran: $name is in the selection"
        break
    }
}

Write-Selection -Escalated $false -Rule $null -Crates @($selected) -Changed $changed `
    -Matrix $matrix -MatrixReason $matrixReason
exit 0
