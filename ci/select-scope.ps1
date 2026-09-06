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
    return ($Path -replace '\\', '/').TrimStart('./')
}

function Write-Selection {
    param(
        [Parameter(Mandatory)] [bool] $Escalated,
        [AllowNull()] [string] $Rule,
        [string[]] $Crates = @(),
        [string[]] $Changed = @(),
        [string[]] $Unmapped = @(),
        [bool] $Matrix = $true,
        [string] $MatrixReason = ''
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

# ---- escalation, BEFORE any selection ------------------------------------------------------
foreach ($rule in $EscalationRules) {
    foreach ($path in $changed) {
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

# ---- map changed paths to crates, by LONGEST directory prefix -------------------------------
$seeds = New-Object System.Collections.Generic.HashSet[string]
$unmapped = New-Object System.Collections.Generic.List[string]
foreach ($path in $changed) {
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
