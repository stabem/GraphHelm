# #903 (epic #901, deliverable 2): the gate runs what the change can REACH.
#
# The property under test is not "the scoped run is green". A scope selector cannot be judged by
# running it on a tree where nothing is broken -- every selection looks correct when every stage
# passes. The property is the one that can fail:
#
#     the scoped run reddens on every change the FULL run would have reddened on.
#
# Which makes the interesting cells the ones about REACH: what the selection includes, what it
# refuses to narrow, and what it escalates. So the graph here is hand-built rather than read from
# this workspace -- a fixture can contain the dev-dependency edge, the unmapped path and the
# unparseable manifest that the real repository does not have on any given day, and a selector is
# exactly the kind of program whose bugs live in the cases the corpus happens not to contain.
#
# EVERY CELL BELOW FAILS BEFORE ci/select-scope.ps1 EXISTS. That is the point of writing them
# first: a selector that was tested after it was written is tested against what it does.

$ExpectedAssertionCount = 14
$ErrorActionPreference = 'Stop'
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

$scriptPath = Join-Path $PSScriptRoot 'select-scope.ps1'

# ARRANGEMENT FIRST. Absent or unparseable, every cell below would be about a program that does not
# exist, and an $ErrorActionPreference='Stop' crash reads as a broken harness rather than as a
# verdict about the selector.
if (-not (Test-Path -LiteralPath $scriptPath)) {
    Write-Host "HARNESS-BROKE: ci/select-scope.ps1 does not exist yet (red-first: this is the expected first failure)" -ForegroundColor Magenta
    exit 2
}
$parseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: select-scope.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-select-scope-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# A hand-built `cargo metadata --format-version 1` graph. Four crates and one edge of each kind that
# matters:
#
#     core-leaf   <- core-mid          (dependencies:      a normal edge)
#     core-leaf   <- cli-tests         (dev-dependencies:  the edge an integration test travels)
#     unrelated                        (no path to core-leaf at all -- the negative control)
#
# `unrelated` is what makes the selection cells falsifiable: without a crate that must NOT be
# selected, "expand to dependents" and "select everything" produce identical passes.
function New-MetadataFixture {
    param([Parameter(Mandatory)] [string] $Path)
    $root = $fixtureRoot.Replace('\', '/')
    $metadata = [ordered]@{
        packages = @(
            [ordered]@{ name = 'core-leaf'; id = 'core-leaf 0.1.0'; manifest_path = "$root/core/leaf/Cargo.toml"
                dependencies = @()
            },
            [ordered]@{ name = 'core-mid'; id = 'core-mid 0.1.0'; manifest_path = "$root/core/mid/Cargo.toml"
                dependencies = @([ordered]@{ name = 'core-leaf'; kind = $null })
            },
            [ordered]@{ name = 'cli-tests'; id = 'cli-tests 0.1.0'; manifest_path = "$root/apps/cli/Cargo.toml"
                dependencies = @([ordered]@{ name = 'core-leaf'; kind = 'dev' })
            },
            [ordered]@{ name = 'unrelated'; id = 'unrelated 0.1.0'; manifest_path = "$root/tools/unrelated/Cargo.toml"
                dependencies = @()
            },
            [ordered]@{ name = 'postgres-event-store'; id = 'pes 0.1.0'; manifest_path = "$root/adapters/postgres-event-store/Cargo.toml"
                dependencies = @([ordered]@{ name = 'core-mid'; kind = $null })
            }
        )
        workspace_members = @('core-leaf 0.1.0', 'core-mid 0.1.0', 'cli-tests 0.1.0', 'unrelated 0.1.0', 'pes 0.1.0')
    }
    [System.IO.File]::WriteAllText($Path, ($metadata | ConvertTo-Json -Depth 8), $utf8NoBom)
}

$metadataPath = Join-Path $fixtureRoot 'metadata.json'
New-MetadataFixture -Path $metadataPath

function Invoke-Select {
    param(
        [Parameter(Mandatory)] [string[]] $ChangedFiles,
        [string] $Metadata = $metadataPath
    )
    $json = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath `
        -MetadataPath $Metadata -ChangedFiles ($ChangedFiles -join ';') -RepoRoot $fixtureRoot 2>&1
    $exit = $LASTEXITCODE
    $text = ($json | ForEach-Object { [string]$_ }) -join "`n"
    $parsed = $null
    try { $parsed = $text | ConvertFrom-Json } catch { }
    return [ordered]@{ exitCode = $exit; text = $text; result = $parsed }
}

try {
    # ---- ESCALATION: the named paths, and the RULE that fired -------------------------------
    # A selector that escalated for the wrong reason will stop escalating when that reason moves,
    # and nothing will notice: the run stays FULL and stays green. So each cell reads the rule name.
    $ci = Invoke-Select -ChangedFiles @('ci/gate.ps1')
    Assert-True ($null -ne $ci.result -and $ci.result.escalated -eq $true) `
        "a change under ci/ escalates to FULL (got escalated=$(if ($null -eq $ci.result) { 'unparseable output' } else { $ci.result.escalated }))"
    Assert-True ($null -ne $ci.result -and $ci.result.escalationRule -eq 'ci/') `
        "and names WHICH rule fired (got '$(if ($null -eq $ci.result) { '' } else { $ci.result.escalationRule })')"

    $lock = Invoke-Select -ChangedFiles @('Cargo.lock')
    Assert-True ($null -ne $lock.result -and $lock.result.escalated -eq $true -and $lock.result.escalationRule -eq 'Cargo.lock') `
        "Cargo.lock escalates and names its rule (got '$(if ($null -eq $lock.result) { '' } else { $lock.result.escalationRule })')"

    # ---- SELECTION: reach, and the negative control -----------------------------------------
    $leaf = Invoke-Select -ChangedFiles @('core/leaf/src/lib.rs')
    $leafCrates = @(if ($null -ne $leaf.result) { $leaf.result.crates })
    Assert-True ($null -ne $leaf.result -and $leaf.result.escalated -eq $false) `
        'a change in one leaf crate does NOT escalate'
    Assert-True ($leafCrates -contains 'core-leaf') `
        "the changed crate itself is selected (got: $($leafCrates -join ', '))"
    Assert-True ($leafCrates -contains 'core-mid') `
        'a normal `dependencies` dependent is selected'
    Assert-True ($leafCrates -contains 'cli-tests') `
        'a DEV-dependency dependent is selected -- the edge an integration test travels, and the one a dependencies-only walk drops in silence'
    # THE NEGATIVE CONTROL. Without it, "select everything" passes every cell above.
    Assert-True (-not ($leafCrates -contains 'unrelated')) `
        "a crate with no path to the change is NOT selected (got: $($leafCrates -join ', '))"

    # ---- THE POSTGRES MATRIX: only when reached ---------------------------------------------
    Assert-True ($null -ne $leaf.result -and $leaf.result.matrix -eq $true) `
        'the PostgreSQL matrix runs when the adapter is among the dependents'
    $unrelated = Invoke-Select -ChangedFiles @('tools/unrelated/src/main.rs')
    Assert-True ($null -ne $unrelated.result -and $unrelated.result.matrix -eq $false) `
        'and does NOT run when nothing in the selection reaches the adapter'
    Assert-True ($null -ne $unrelated.result -and -not [string]::IsNullOrWhiteSpace([string]$unrelated.result.matrixReason)) `
        'and the manifest says WHY the matrix was skipped, rather than leaving a silent false'

    # ---- FAIL CLOSED: the two ways the derivation can be wrong -------------------------------
    # The escalation list is a DENY-LIST over a class ("a path whose change can invalidate the graph
    # the selection is derived from"), and a list of names cannot see the next member. So anything
    # the selector cannot map must widen the run, never narrow it.
    $unmapped = Invoke-Select -ChangedFiles @('docs/whatever.md', 'core/leaf/src/lib.rs')
    Assert-True ($null -ne $unmapped.result -and $unmapped.result.escalated -eq $true) `
        'a changed path that maps to NO crate escalates to FULL rather than being reported and skipped'

    $badMetadataPath = Join-Path $fixtureRoot 'broken.json'
    [System.IO.File]::WriteAllText($badMetadataPath, '{ this is not json', $utf8NoBom)
    $bad = Invoke-Select -ChangedFiles @('core/leaf/src/lib.rs') -Metadata $badMetadataPath
    Assert-True ($bad.exitCode -ne 0 -or ($null -ne $bad.result -and $bad.result.escalated -eq $true)) `
        "metadata that does not parse refuses or escalates -- it never yields a narrow selection (exit $($bad.exitCode))"
    Assert-True ($bad.text -notmatch 'core-leaf' -or ($null -ne $bad.result -and $bad.result.escalated -eq $true)) `
        'and does not emit a crate list derived from a graph it could not read'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "select-scope: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "select-scope: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
