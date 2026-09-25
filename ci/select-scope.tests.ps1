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

$ExpectedAssertionCount = 56
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
# A GIT REPOSITORY WITH NOTHING TRACKED (#901 slice 2): every class path is now asked which Rust files
# read it, through `git grep`, and a root where git cannot answer widens by design. An empty index
# answers "no reader", which is the premise the cells below were written under. The no-git answer
# has its own cell, on a root outside this one.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
& git -C $fixtureRoot init -q 2>&1 | Out-Null
$ErrorActionPreference = $previousPreference
$nonRepoRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-select-scope-nogit-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($nonRepoRoot) | Out-Null
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
        [string] $Metadata = $metadataPath,
        [string] $Root = $fixtureRoot
    )
    $json = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath `
        -MetadataPath $Metadata -ChangedFiles ($ChangedFiles -join ';') -RepoRoot $Root 2>&1
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


    # ---- KNOWN-EMPTY: a change that is known and reaches no Rust (#903, after X's measurement) ---
    # MEASURED FIRST, NOT DESIGNED FIRST: the selector was run against the real diff of every pull
    # request this fleet handled on 2026-09-06, and one of eight skipped the matrix. Five of the
    # other seven were `ci/*.tests.ps1` changes -- a PowerShell suite for the gate, which cannot
    # change what a Rust test measures. They escalated because `ci/` is a deny-list entry.
    #
    # The naive repair (exempt tests from the `ci/` rule) buys nothing: K measured that the path
    # then falls to the unmapped rule and escalates for a different reason. So this is a THIRD
    # state, and its whole difficulty is that it must not weaken the two guards beside it --
    # "no crate list" and "an empty list from an unknown diff" both still mean FULL.
    $suiteOnly = Invoke-Select -ChangedFiles @('ci/gate-verdict.tests.ps1')
    Assert-True ($null -ne $suiteOnly.result -and $suiteOnly.result.escalated -eq $false) `
        "a change to a ci/*.tests.ps1 suite alone does NOT escalate (got escalated=$(if ($null -eq $suiteOnly.result) { 'unparseable' } else { $suiteOnly.result.escalated }), rule '$(if ($null -eq $suiteOnly.result) { '' } else { $suiteOnly.result.escalationRule })')"
    Assert-True ($null -ne $suiteOnly.result -and $suiteOnly.result.rustInputsChanged -eq $false) `
        'and says the emptiness is KNOWN -- rustInputsChanged is false, which is what separates it from an empty list nobody could explain'
    Assert-True ($null -ne $suiteOnly.result -and $suiteOnly.result.matrix -eq $false) `
        'so the PostgreSQL matrix is skipped'
    Assert-True ($null -ne $suiteOnly.result -and $suiteOnly.result.matrixReason -match 'no Rust') `
        "and the reason names the class rather than leaving a bare false (got '$(if ($null -eq $suiteOnly.result) { '' } else { $suiteOnly.result.matrixReason })')"

    # CONTROL 1: the gate's OWN scripts keep escalating. A change to ci/gate.ps1 changes what every
    # other stage measures, which is a different claim from "a suite for the gate changed".
    $gateScript = Invoke-Select -ChangedFiles @('ci/gate.ps1')
    Assert-True ($null -ne $gateScript.result -and $gateScript.result.escalated -eq $true -and $gateScript.result.escalationRule -eq 'ci/') `
        'CONTROL: ci/gate.ps1 still escalates under the ci/ rule, so the new state did not swallow the old one'

    # CONTROL 2: a suite change WITH a Rust change is an ordinary selection, not the known-empty
    # state. Without this, a rule that fires on "any suite file present" would pass every cell above.
    $mixed = Invoke-Select -ChangedFiles @('ci/gate-verdict.tests.ps1', 'core/leaf/src/lib.rs')
    Assert-True ($null -ne $mixed.result -and $mixed.result.escalated -eq $false -and @($mixed.result.crates).Count -gt 0) `
        "CONTROL: a suite change alongside a Rust change selects crates normally (got $(@(if ($null -ne $mixed.result) { $mixed.result.crates }).Count))"
    Assert-True ($null -ne $mixed.result -and $mixed.result.rustInputsChanged -eq $true) `
        'CONTROL: and reports that Rust input DID change, so the known-empty state cannot be reached with Rust in the diff'

    # CONTROL 3: an unmapped path still escalates. The new state is a NAMED class, not "anything
    # the mapper could not place".
    $stillUnmapped = Invoke-Select -ChangedFiles @('deploy/restore-vps.sh')
    Assert-True ($null -ne $stillUnmapped.result -and $stillUnmapped.result.escalated -eq $true -and $stillUnmapped.result.escalationRule -eq 'unmapped-path') `
        "CONTROL: a path in no class still escalates as unmapped (got rule '$(if ($null -eq $stillUnmapped.result) { '' } else { $stillUnmapped.result.escalationRule })')"

    # ---- FAIL CLOSED: the two ways the derivation can be wrong -------------------------------
    # The escalation list is a DENY-LIST over a class ("a path whose change can invalidate the graph
    # the selection is derived from"), and a list of names cannot see the next member. So anything
    # the selector cannot map must widen the run, never narrow it.
    $unmapped = Invoke-Select -ChangedFiles @('docs/whatever.yaml', 'core/leaf/src/lib.rs')
    Assert-True ($null -ne $unmapped.result -and $unmapped.result.escalated -eq $true) `
        'a changed path that maps to NO crate escalates to FULL rather than being reported and skipped'

    $badMetadataPath = Join-Path $fixtureRoot 'broken.json'
    [System.IO.File]::WriteAllText($badMetadataPath, '{ this is not json', $utf8NoBom)
    $bad = Invoke-Select -ChangedFiles @('core/leaf/src/lib.rs') -Metadata $badMetadataPath
    Assert-True ($bad.exitCode -ne 0 -or ($null -ne $bad.result -and $bad.result.escalated -eq $true)) `
        "metadata that does not parse refuses or escalates -- it never yields a narrow selection (exit $($bad.exitCode))"
    Assert-True ($bad.text -notmatch 'core-leaf' -or ($null -ne $bad.result -and $bad.result.escalated -eq $true)) `
        'and does not emit a crate list derived from a graph it could not read'

    # ---- DOTFILES AND THE GATE'S OWN RECEIPT ---------------------------------------------------
    # Both of these were found by running the selector on REAL history, not by a cell: no fixture
    # in this file had a path beginning with a dot, so no cell could have caught either.
    #
    # 1. `TrimStart('./')` takes a char array and stripped the leading dot, turning
    #    `.factory/gate-runs/x.json` into `factory/...` -- a path matching no crate and no rule.
    # 2. Even spelled correctly it maps to no crate, so the unmapped rule escalated EVERY run:
    #    #674(a) puts a manifest in every branch's diff from its second run on. A selector that
    #    escalates on its own gate's receipt can never narrow anything.
    $manifestPath = '.factory/gate-runs/a50e7d0a24ee-20260906T045907.929Z-9d2df338.json'

    $withReceipt = Invoke-Select -ChangedFiles @($manifestPath, 'core/leaf/src/lib.rs')
    Assert-True ($null -ne $withReceipt.result -and $withReceipt.result.escalated -eq $false) `
        "a run manifest beside a crate change does NOT escalate: the receipt is not build input (rule: '$(if ($null -eq $withReceipt.result) { 'unparseable' } else { $withReceipt.result.escalationRule })')"
    Assert-True (@(if ($null -ne $withReceipt.result) { $withReceipt.result.crates }) -contains 'core-leaf') `
        'and the crate beside it is still selected'

    # The exemption is the STORE, not the whole `.factory/` tree.
    $otherFactory = Invoke-Select -ChangedFiles @('.factory/orchestrator-board.yaml', 'core/leaf/src/lib.rs')
    Assert-True ($null -ne $otherFactory.result -and $otherFactory.result.escalated -eq $true) `
        'a NON-manifest file under .factory/ still escalates -- the exemption is the store, not the tree'

    # A diff that is only receipts has nothing to derive a scope from, and says so.
    $onlyReceipt = Invoke-Select -ChangedFiles @($manifestPath)
    Assert-True ($null -ne $onlyReceipt.result -and $onlyReceipt.result.escalated -eq $true -and `
            $onlyReceipt.result.escalationRule -eq 'manifest-only') `
        "a diff of run manifests alone is FULL, by its own named rule (got '$(if ($null -eq $onlyReceipt.result) { '' } else { $onlyReceipt.result.escalationRule })')"


    # ---- #901 slice 2: WHAT IS NOT RUST BUILD INPUT, and what still is -------------------------
    # A REAL git repository this time, because two of the rules read the tree: the gate's own
    # `Join-Path` calls decide which `ci/` files the gate RUNS, and `git grep` decides who reads a
    # changed Markdown file. The graph is two crates, so "selected the reader" and "selected
    # everything" cannot pass the same cell.
    $repo = Join-Path $fixtureRoot 'repo'
    foreach ($dir in @('ci', 'core/leaf/src', 'tools/unrelated/src', 'apps/studio/src', 'docs')) {
        [System.IO.Directory]::CreateDirectory((Join-Path $repo $dir)) | Out-Null
    }
    $files = [ordered]@{
        # The gate RUNS helper.ps1, which RUNS deep.ps1. commented.ps1 is named only in a comment.
        'ci/gate.ps1'           = ". (Join-Path `$PSScriptRoot 'helper.ps1')`n# . (Join-Path `$PSScriptRoot 'commented.ps1')`n"
        'ci/helper.ps1'         = "& powershell -File (Join-Path `$repositoryRoot 'ci/deep.ps1')`n"
        'ci/deep.ps1'           = "Write-Host deep`n"
        'ci/commented.ps1'      = "Write-Host commented`n"
        'ci/tool.ps1'           = "Write-Host a verifier the gate never runs`n"
        'ci/x.tests.ps1'        = "# this suite reads docs/named-by-suite.md`n"
        'core/leaf/src/lib.rs'  = "pub const R: &str = include_str!(`"../../../README.md`");`n"
        'tools/unrelated/src/lib.rs' = "pub fn f() {}`n"
        'README.md'             = "# read by core/leaf`n"
        'docs/lonely.md'        = "nobody reads this`n"
        'docs/named-by-suite.md' = "a suite reads this`n"
        'apps/studio/src/a.ts'  = "export const a = 1`n"
    }
    foreach ($name in $files.Keys) { [System.IO.File]::WriteAllText((Join-Path $repo $name), $files[$name], $utf8NoBom) }
    # Native stderr under 'Stop' is a terminating error in Windows PowerShell 5.1 (git prints hints
    # on init), which would end the suite in HARNESS-BROKE rather than in a verdict.
    $ErrorActionPreference = 'Continue'
    & git -C $repo init -q 2>&1 | Out-Null
    & git -C $repo add -A 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
    $repoRootText = $repo.Replace('\', '/')
    $repoMetadata = Join-Path $fixtureRoot 'repo-metadata.json'
    $repoGraph = [ordered]@{
        packages = @(
            [ordered]@{ name = 'core-leaf'; id = 'core-leaf 0.1.0'; manifest_path = "$repoRootText/core/leaf/Cargo.toml"; dependencies = @() },
            [ordered]@{ name = 'unrelated'; id = 'unrelated 0.1.0'; manifest_path = "$repoRootText/tools/unrelated/Cargo.toml"; dependencies = @() },
            [ordered]@{ name = 'protocols'; id = 'protocols 0.1.0'; manifest_path = "$repoRootText/core/protocols/Cargo.toml"; dependencies = @() }
        )
    }
    [System.IO.File]::WriteAllText($repoMetadata, ($repoGraph | ConvertTo-Json -Depth 6), $utf8NoBom)

    $tool = Invoke-Select -ChangedFiles @('ci/tool.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $tool.result -and $tool.result.escalated -eq $false -and $tool.result.rustInputsChanged -eq $false) `
        "a ci/ file the gate never runs is not Rust build input (escalated=$(if ($tool.result) { $tool.result.escalated }), rust=$(if ($tool.result) { $tool.result.rustInputsChanged }), reason '$(if ($tool.result) { $tool.result.matrixReason })')"
    Assert-True ($null -ne $tool.result -and $tool.result.matrixReason -match 'ci-tool' -and $tool.result.psSuites -eq $true) `
        'and it names the class ci-tool, and its suites still run'
    $deep = Invoke-Select -ChangedFiles @('ci/deep.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $deep.result -and $deep.result.escalated -eq $true -and $deep.result.escalationRule -eq 'ci/') `
        'CONTROL: a ci/ file the gate runs TRANSITIVELY (gate -> helper -> deep) still escalates'
    $commented = Invoke-Select -ChangedFiles @('ci/commented.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $commented.result -and $commented.result.escalated -eq $false) `
        'a ci/ file named only in a COMMENT of the gate is not run by it'
    $noGate = Invoke-Select -ChangedFiles @('ci/tool.ps1')
    Assert-True ($null -ne $noGate.result -and $noGate.result.escalated -eq $true) `
        'CONTROL: where ci/gate.ps1 cannot be read, no ci/ file is a tool -- the closure fails wide'

    $studio = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $studio.result -and $studio.result.escalated -eq $false -and $studio.result.rustInputsChanged -eq $false) `
        'an apps/studio change is not Rust build input'
    Assert-True ($null -ne $studio.result -and $studio.result.psSuites -eq $false) `
        "and nothing under ci/ names it, so the suites narrow (reason '$(if ($studio.result) { $studio.result.psSuitesReason })')"
    $studioMixed = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts', 'core/leaf/src/lib.rs') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $studioMixed.result -and $studioMixed.result.rustInputsChanged -eq $true -and @($studioMixed.result.crates) -contains 'core-leaf') `
        'CONTROL: studio beside a crate change still selects the crate'

    $lonely = Invoke-Select -ChangedFiles @('docs/lonely.md') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $lonely.result -and $lonely.result.escalated -eq $false -and $lonely.result.rustInputsChanged -eq $false) `
        "Markdown nothing names is not Rust build input (reason '$(if ($lonely.result) { $lonely.result.matrixReason })')"
    Assert-True ($null -ne $lonely.result -and $lonely.result.matrixReason -match 'markdown') 'and it names the class markdown'
    $readme = Invoke-Select -ChangedFiles @('README.md') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $readme.result -and $readme.result.escalated -eq $false -and @($readme.result.crates) -contains 'core-leaf') `
        'Markdown a crate reads (include_str!) selects THAT crate'
    Assert-True ($null -ne $readme.result -and -not (@($readme.result.crates) -contains 'unrelated')) `
        'CONTROL: and not a crate that does not read it'
    $bySuite = Invoke-Select -ChangedFiles @('docs/named-by-suite.md') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $bySuite.result -and $bySuite.result.rustInputsChanged -eq $false -and $bySuite.result.psSuites -eq $true) `
        'CONTROL: Markdown only a suite names is not Rust input, and the suites run for it'
    $fixtureMd = Invoke-Select -ChangedFiles @('core/leaf/tests/fixtures/tree/notes.md') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $fixtureMd.result -and $fixtureMd.result.rustInputsChanged -eq $true -and @($fixtureMd.result.crates) -contains 'core-leaf') `
        "Markdown under a tests/fixtures tree is build input for the crate that holds it -- a test may walk it without naming it (rust=$(if ($fixtureMd.result) { $fixtureMd.result.rustInputsChanged }))"
    Assert-True ($null -ne $fixtureMd.result -and -not (@($fixtureMd.result.crates) -contains 'unrelated')) `
        'CONTROL: and only that crate'
    $mdNoRepo = Invoke-Select -ChangedFiles @('docs/lonely.md') -Root $nonRepoRoot
    Assert-True ($null -ne $mdNoRepo.result -and $mdNoRepo.result.escalated -eq $true) `
        'CONTROL: where git grep cannot answer, Markdown stays build input and escalates'
    $suiteNoRepo = Invoke-Select -ChangedFiles @('ci/x.tests.ps1') -Root $nonRepoRoot
    Assert-True ($null -ne $suiteNoRepo.result -and $suiteNoRepo.result.escalated -eq $true) `
        'CONTROL: where git grep cannot say which Rust files read a ci/ suite, the suite escalates'

    $crateOnly = Invoke-Select -ChangedFiles @('tools/unrelated/src/lib.rs') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $crateOnly.result -and $crateOnly.result.psSuites -eq $false) `
        'a crate change nothing under ci/ names narrows the suites'
    $ciMention = Invoke-Select -ChangedFiles @('core/leaf/src/lib.rs') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $ciMention.result -and $ciMention.result.psSuites -eq $false -and @($ciMention.result.crates) -contains 'core-leaf') `
        'and a crate change still selects its crate while the suites narrow'
    $escalatedSuites = Invoke-Select -ChangedFiles @('ci/deep.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $escalatedSuites.result -and $escalatedSuites.result.psSuites -eq $true) `
        'CONTROL: an escalated run runs every suite'

    # ---- BLOCK on #1220 (lane 3f90d6): a Rust test that WALKS ci/ is a reader of every ci/ file ----
    # Added LAST, because every ci/ cell above is about a tree where no Rust file names `ci`.
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'tools/unrelated/tests')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'tools/unrelated/tests/walk.rs'),
        "fn walk() { let ci = root.join(`"ci`"); let _ = std::fs::read_dir(&ci); }`n", $utf8NoBom)
    $ErrorActionPreference = 'Continue'
    & git -C $repo add -A 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
    $walkedTool = Invoke-Select -ChangedFiles @('ci/tool.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $walkedTool.result -and $walkedTool.result.rustInputsChanged -eq $true) `
        "a ci/ tool change is Rust input once a Rust test walks ci/ (rust=$(if ($walkedTool.result) { $walkedTool.result.rustInputsChanged }))"
    Assert-True ($null -ne $walkedTool.result -and @($walkedTool.result.crates) -contains 'unrelated') `
        'and the WALKER''s crate is selected'
    Assert-True ($null -ne $walkedTool.result -and -not (@($walkedTool.result.crates) -contains 'core-leaf')) `
        'CONTROL: and a crate that does not walk ci/ is not'
    $walkedSuite = Invoke-Select -ChangedFiles @('ci/x.tests.ps1') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $walkedSuite.result -and @($walkedSuite.result.crates) -contains 'unrelated') `
        'a ci/ SUITE change selects the walker too -- the class that existed before this slice'
    # A READER IS NOT A CHANGE: a walker under an escalation directory selects its crate and does not
    # escalate the run -- and the same file CHANGED still escalates.
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'core/protocols/tests')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'core/protocols/tests/walk_studio.rs'),
        "fn w() { let s = root.join(`"apps/studio`"); }`n", $utf8NoBom)
    $ErrorActionPreference = 'Continue'
    & git -C $repo add -A 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
    $readByProtocols = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $readByProtocols.result -and $readByProtocols.result.escalated -eq $false -and $readByProtocols.result.rustInputsChanged -eq $true) `
        "a reader under core/protocols/ makes studio Rust input WITHOUT escalating the run, selecting protocols=$(if ($readByProtocols.result) { @($readByProtocols.result.crates) -contains 'protocols' }) (escalated=$(if ($readByProtocols.result) { $readByProtocols.result.escalated }), reason '$(if ($readByProtocols.result) { $readByProtocols.result.escalationRule })')"
    $protocolsChanged = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts', 'core/protocols/tests/walk_studio.rs') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $protocolsChanged.result -and $protocolsChanged.result.escalated -eq $true -and $protocolsChanged.result.escalationRule -eq 'core/protocols/') `
        'CONTROL: the same reader, CHANGED in the diff, still escalates under its rule'
    $cliLiteral = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $cliLiteral.result -and -not (@($cliLiteral.result.crates) -contains 'core-leaf')) `
        'CONTROL: a crate that names neither the path nor its directory is not selected'
    Remove-Item -LiteralPath (Join-Path $repo 'core/protocols/tests/walk_studio.rs') -Force
    $ErrorActionPreference = 'Continue'
    & git -C $repo add -A 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
    $studioStill = Invoke-Select -ChangedFiles @('apps/studio/src/a.ts') -Metadata $repoMetadata -Root $repo
    Assert-True ($null -ne $studioStill.result -and $studioStill.result.rustInputsChanged -eq $false) `
        'CONTROL: a walker of ci/ does not make studio Rust input'

    # ---- A RENAME SHOWS BOTH SIDES (review of #1216 by lane b9deb2) ----------------------------
    # `git mv tools/unrelated/src/lib.rs docs/moved.md` must not read as "one Markdown file changed".
    $ErrorActionPreference = 'Continue'
    & git -C $repo -c user.email=t@t -c user.name=t commit -q -m base 2>&1 | Out-Null
    $renameBase = (& git -C $repo rev-parse HEAD 2>$null | Select-Object -First 1)
    & git -C $repo mv tools/unrelated/src/lib.rs docs/moved.md 2>&1 | Out-Null
    & git -C $repo -c user.email=t@t -c user.name=t commit -q -m rename 2>&1 | Out-Null
    $ErrorActionPreference = 'Stop'
    $renameJson = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath `
        -MetadataPath $repoMetadata -RepoRoot $repo -MergeBase ([string]$renameBase).Trim() -Head HEAD 2>&1
    $renamed = $null
    try { $renamed = (($renameJson | ForEach-Object { [string]$_ }) -join "`n") | ConvertFrom-Json } catch { }
    Assert-True ($null -ne $renamed -and @($renamed.changedFiles) -contains 'tools/unrelated/src/lib.rs') `
        "a rename lists the Rust path it removed, not only the Markdown it created (changed: $(if ($renamed) { @($renamed.changedFiles) -join ',' }))"
    Assert-True ($null -ne $renamed -and $renamed.rustInputsChanged -eq $true -and @($renamed.crates) -contains 'unrelated') `
        'and the crate that lost the file is selected'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $nonRepoRoot -Recurse -Force -ErrorAction SilentlyContinue
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
