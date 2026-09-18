# #904: isolated tests for ci/crate-input-hash.ps1.
#
# Dot-sources ONLY crate-input-hash.ps1, never gate.ps1. The repository-backed cells build their
# own throwaway git repository under the OS temp root and remove it, so running this needs no
# slot, no cargo, no toolchain and no coordination with any other lane.
#
# THE HASH IS A CACHE KEY, so the two failure directions are not symmetric and the cells are
# written to say which one they are about. A hash that changes when it should not costs a rebuild.
# A hash that STAYS THE SAME when an input changed lets a stale green replay against different
# bytes -- #904's own threat assessment. Every cell below that asserts "different" is guarding the
# unsafe direction; the ones that assert "same" are guarding usefulness.
#
# Every count is wrapped in @(). PowerShell collapses an empty result to $null, so `.Count` throws
# under StrictMode and a genuine zero cannot be asserted.
#
# The declared total is Assert-* CALLS reached at runtime; Assert-Equal delegating to Assert-True
# fires once per call, not twice. A cell that stops running while its neighbours stay green shows
# up here as a miscount rather than as quiet success.
$ExpectedAssertionCount = 64

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:total = 0
$script:failed = 0

function Assert-True {
    param([Parameter(Mandatory)][bool] $Condition, [Parameter(Mandatory)][string] $Label)
    $script:total++
    if ($Condition) { Write-Host "PASS  $Label" } else { $script:failed++; Write-Host "FAIL  $Label" }
}

function Assert-Equal {
    param([Parameter(Mandatory)][AllowNull()] $Expected, [Parameter(Mandatory)][AllowNull()] $Actual, [Parameter(Mandatory)][string] $Label)
    Assert-True -Condition ("$Expected" -eq "$Actual") -Label "$Label (expected '$Expected', got '$Actual')"
}

function Assert-NotEqual {
    # The label is built BEFORE the outcome is known, so it must read correctly on both paths. An
    # earlier spelling said "both were 'X'", which is true only when the cell FAILS -- every green
    # line then claimed a collision that had not happened. A label that is false on the passing
    # path is a small thing that teaches a wrong reading of a whole log.
    param([Parameter(Mandatory)][AllowNull()] $Unexpected, [Parameter(Mandatory)][AllowNull()] $Actual, [Parameter(Mandatory)][string] $Label)
    Assert-True -Condition ("$Unexpected" -ne "$Actual") -Label "$Label (must not be '$Unexpected'; was '$Actual')"
}

. "$PSScriptRoot/crate-input-hash.ps1"

# A baseline component tuple every composition cell varies ONE field of. Sharing it is what makes
# "this field is read" a measurement rather than a coincidence: if a cell changed two fields, a
# differing hash would not say which one the function looked at.
function New-Baseline {
    return @{
        Crate             = 'graphhelm-events'
        TreeObject        = '4f2d1c9a6b3e8d70f1a2b3c4d5e6f708192a3b4c'
        LockSlice         = 'serde 1.0.0; thiserror 2.0.0'
        WorkspaceManifest = '4e208752960763478eefe306a1bb05aeffa9725f'
        ToolchainId       = 'rustc 1.97.1 (abcdef012 2026-01-01)'
        Features          = @('std', 'postgres')
        DependencyHashes  = @('aaaaaaaaaaaa-1111', 'bbbbbbbbbbbb-2222')
        BuildScriptInputs = @('build.rs')
    }
}

function Get-BaselineHash {
    param([hashtable] $Overrides = @{})
    $b = New-Baseline
    foreach ($key in $Overrides.Keys) { $b[$key] = $Overrides[$key] }
    return Get-CrateInputHash @b
}

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("crate-input-hash-tests-" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $root | Out-Null

try {
    # ---------------------------------------------------------------- composition, no repository

    # Determinism first. Without it every "different" assertion below could pass by accident.
    Assert-Equal -Expected (Get-BaselineHash) -Actual (Get-BaselineHash) `
        -Label 'the same components hash to the same key'

    Assert-Equal -Expected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ Features = @('postgres', 'std') }) `
        -Label 'feature ORDER does not change the key (cargo promises no order)'

    Assert-Equal -Expected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ DependencyHashes = @('bbbbbbbbbbbb-2222', 'aaaaaaaaaaaa-1111') }) `
        -Label 'dependency-hash ORDER does not change the key'

    Assert-Equal -Expected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ BuildScriptInputs = @('build.rs') }) `
        -Label 'the same build-script inputs hash the same'

    # One field at a time, each guarding the UNSAFE direction: if any of these came back equal, a
    # shard proved with one value would be reused for another.
    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ TreeObject = '0000000000000000000000000000000000000000' }) `
        -Label 'a different source tree changes the key'

    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ LockSlice = 'serde 1.0.1; thiserror 2.0.0' }) `
        -Label 'a different lockfile slice changes the key'

    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ Features = @('std') }) `
        -Label 'a different feature SET changes the key (unification is an input)'

    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ DependencyHashes = @('aaaaaaaaaaaa-1111', 'cccccccccccc-3333') }) `
        -Label 'a dependency whose own key moved changes this key (escalation)'

    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ BuildScriptInputs = @('build.rs', 'proto/schema.proto') }) `
        -Label 'a declared build-script input changes the key'

    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ Crate = 'graphhelm-governor' }) `
        -Label 'two crates sharing every other component do not share a key'

    # THE ARMING SITE. Two cells, because the property has two halves and one of them cannot be
    # asserted the obvious way.
    #
    # `$WorkspaceManifest` shipped as `= ''` -- the only optional input in a function where every
    # other one is Mandatory, and the UNSAFE one. A caller who forgets it gets the empty-manifest
    # key, byte-identical for every possible root manifest: the collision this field exists to
    # close, re-entering through a parameter default.
    #
    # `[Parameter(Mandatory)]` is NOT the remedy, and that is measured rather than argued. Under the
    # gate's own invocation -- `powershell -NoProfile -ExecutionPolicy Bypass -File`, with no
    # `-NonInteractive` -- a missing Mandatory parameter PROMPTS: rc=124 after 40s, output cut off
    # at the first line. It would hang the gate rather than fail it. Nor does adding a throw BESIDE
    # Mandatory help: the binder raises ParameterBindingException before the body runs, so the
    # throw is unreachable and a cell asserting its message would be vacuous. So the guard is the
    # throw ALONE, and this cell can therefore reach it.
    foreach ($fn in @('New-HashPreimage', 'Get-CrateInputHash')) {
        $threw = $false
        $message = ''
        try {
            & $fn -Crate 'c' -TreeObject 't' -LockSlice 'l' -ToolchainId 'tc' | Out-Null
        } catch {
            $threw = $true
            $message = "$($_.Exception.Message)"
        }
        Assert-True -Condition $threw -Label "$fn refuses to run when the workspace manifest is not supplied"
        Assert-True -Condition ($message -like '*WorkspaceManifest*') -Label "  and the failure names the input it wanted ($fn)"
    }

    # The other half, on the FORM: a DEFAULT is what silently stands in for a missing argument, so
    # no scalar input may carry one. Read from the AST, so a sixth field added later with `= ''`
    # is caught without anyone remembering to extend a list -- which is exactly how this defect
    # arrived. The arrangement guard first: a parse that found no parameters would pass vacuously.
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        (Join-Path $PSScriptRoot 'crate-input-hash.ps1'), [ref]$null, [ref]$null)
    foreach ($fn in @('New-HashPreimage', 'Get-CrateInputHash')) {
        $target = $ast.FindAll({
            param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $fn
        }, $true)
        Assert-Equal -Expected 1 -Actual @($target).Count -Label "ARRANGEMENT: exactly one $fn is defined"
        # The type comes from the TypeConstraintAst among the parameter's attributes, not from
        # `.StaticType` -- with several attributes on one parameter that property came back empty
        # here and the filter selected NOTHING. The arrangement guard below is what caught it: the
        # "no default" assertion had passed over an empty set, which is vacuously true and proves
        # nothing at all.
        $scalars = @(
            $target[0].Body.ParamBlock.Parameters | Where-Object {
                @($_.Attributes | Where-Object {
                    $_ -is [System.Management.Automation.Language.TypeConstraintAst] -and
                    "$($_.TypeName.FullName)" -eq 'string'
                }).Count -gt 0
            }
        )
        Assert-True -Condition ($scalars.Count -ge 5) `
            -Label "ARRANGEMENT: $fn's scalar inputs are visible in the AST (found $($scalars.Count))"
        $withDefaults = @(
            $scalars | Where-Object { $null -ne $_.DefaultValue } |
                ForEach-Object { $_.Name.VariablePath.UserPath }
        )
        Assert-Equal -Expected '' -Actual (@($withDefaults) -join ',') `
            -Label "no scalar input of $fn carries a default: a default silently stands in for a missing argument"
    }

    # The workspace ROOT manifest: an input in NO crate's tree (ISSUES 4's finding on #915).
    Assert-NotEqual -Unexpected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ WorkspaceManifest = '0000000000000000000000000000000000000000' }) `
        -Label 'a changed workspace root manifest changes the key (profiles live there, in no crate)'
    Assert-Equal -Expected (Get-BaselineHash) `
        -Actual (Get-BaselineHash @{ WorkspaceManifest = (New-Baseline).WorkspaceManifest }) `
        -Label 'the same workspace manifest hashes the same'

    # The generation, and what it is FOR: a toolchain bump must not reuse across compilers.
    $bumped = Get-BaselineHash @{ ToolchainId = 'rustc 1.98.0 (999888777 2026-06-01)' }
    Assert-NotEqual -Unexpected (Get-BaselineHash) -Actual $bumped `
        -Label 'a toolchain bump changes the key'
    Assert-NotEqual -Unexpected ((Get-BaselineHash) -split '-')[0] -Actual ($bumped -split '-')[0] `
        -Label 'a toolchain bump changes the GENERATION prefix, not only the digest'
    Assert-Equal -Expected (Get-Sha256Hex -Text (New-Baseline).ToolchainId).Substring(0, 12) `
        -Actual ((Get-BaselineHash) -split '-')[0] `
        -Label 'the generation is the first 12 hex of the toolchain id digest'

    # INJECTIVITY at the FIELD level. Kept, and its weight stated honestly: cutting the field-level
    # length prefix out of the implementation leaves these three GREEN, because the fixed field
    # names and fixed arity already separate the tuples. So this cell does NOT pin that prefix --
    # it pins that a field-split cannot merge, which is a property of the whole rendering. The
    # prefix that a sabotage does redden is the one inside the sets, four cells below.
    # The third assertion is the control for the first two: it shows the naive concatenation of
    # these two tuples really is one string, so they are not passing for an unrelated reason.
    $left = Get-CrateInputHash -Crate 'ab' -TreeObject 'c' -LockSlice 'x' -WorkspaceManifest '' -ToolchainId 't'
    $right = Get-CrateInputHash -Crate 'a' -TreeObject 'bc' -LockSlice 'x' -WorkspaceManifest '' -ToolchainId 't'
    Assert-NotEqual -Unexpected $left -Actual $right `
        -Label 'a field-split that a naive concatenation would merge does NOT collide'
    $leftPre = New-HashPreimage -Crate 'ab' -TreeObject 'c' -LockSlice 'x' -WorkspaceManifest '' -ToolchainId 't'
    $rightPre = New-HashPreimage -Crate 'a' -TreeObject 'bc' -LockSlice 'x' -WorkspaceManifest '' -ToolchainId 't'
    Assert-NotEqual -Unexpected $leftPre -Actual $rightPre -Label 'and their preimages differ too'
    Assert-Equal -Expected ('ab' + 'c') -Actual ('a' + 'bc') `
        -Label 'CONTROL: the unprefixed concatenation of those two tuples IS one string'

    # THE COLLISION THAT ACTUALLY EXISTS, found because a sabotage on the field-level length prefix
    # reddened NOTHING. With a fixed field count and fixed field names, the outer join is already
    # injective and that prefix is belt-and-braces. The ambiguity is one level in: the SETS are
    # joined with a comma, and a single element containing a comma is indistinguishable from two
    # elements. A build-script input `proto/a,b.proto` and the pair `proto/a` + `b.proto` are
    # different inputs and were one key -- measured on this file before this cell existed.
    Assert-NotEqual `
        -Unexpected (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -BuildScriptInputs @('a,b')) `
        -Actual (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -BuildScriptInputs @('a', 'b')) `
        -Label 'one build-script input containing a comma is not two inputs'
    Assert-NotEqual `
        -Unexpected (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -DependencyHashes @('x,y')) `
        -Actual (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -DependencyHashes @('x', 'y')) `
        -Label 'one dependency key containing a comma is not two dependencies'
    Assert-NotEqual `
        -Unexpected (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -Features @('p,q')) `
        -Actual (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -Features @('p', 'q')) `
        -Label 'one feature containing a comma is not two features'
    # And the empty set is not the set holding one empty string: `deps=` would spell both.
    Assert-NotEqual `
        -Unexpected (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -DependencyHashes @()) `
        -Actual (Get-CrateInputHash -Crate 'c' -TreeObject 't' -LockSlice 'l' -WorkspaceManifest '' -ToolchainId 'tc' -DependencyHashes @('')) `
        -Label 'no dependencies is not one empty dependency'

    # The preimage names its fields, so a reader can see what went in without inverting a digest.
    $baselineForPreimage = New-Baseline
    $preimage = New-HashPreimage @baselineForPreimage
    foreach ($field in @('crate=', 'tree=', 'lock=', 'toolchain=', 'features=', 'deps=', 'buildinputs=')) {
        Assert-True -Condition ($preimage -like "*$field*") -Label "the preimage names the field '$field'"
    }

    # ---------------------------------------------------------------- repository-backed

    $repo = Join-Path $root 'repo'
    New-Item -ItemType Directory -Path $repo | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repo 'core/leaf/src') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repo 'core/other/src') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $repo '.factory/gate-runs') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/src/lib.rs') -Value 'pub fn one() -> u8 { 1 }' -Encoding utf8
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/Cargo.toml') -Value '[package]' -Encoding utf8
    Set-Content -LiteralPath (Join-Path $repo 'core/other/src/lib.rs') -Value 'pub fn two() -> u8 { 2 }' -Encoding utf8
    Set-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Encoding utf8 -Value @(
        '[workspace]'
        'members = ["core/leaf", "core/other"]'
        '[profile.test]'
        'debug = 1'
    )

    $git = {
        param([string[]] $Arguments)
        $global:LASTEXITCODE = 0
        $out = & git -C $repo @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) { throw "HARNESS-BROKE: git $($Arguments -join ' ') exited $LASTEXITCODE ($out)" }
        return $out
    }
    & $git @('init', '--quiet') | Out-Null
    & $git @('config', 'user.email', 'tests@example.invalid') | Out-Null
    & $git @('config', 'user.name', 'crate-input-hash tests') | Out-Null
    # DECLARED, NOT INHERITED (#981). `git config` reads the user and system files too, so a host
    # with `commit.gpgSign = true` and no reachable key -- or a global `core.hooksPath` whose
    # pre-commit hook fails -- cannot make the seven commits below, and this suite would report
    # whatever the missing commit broke. The same two lines, for the same reason, are in
    # ci/normalize-script-eol.tests.ps1 and ci/gate-manifest-provenance.tests.ps1.
    #
    # The hooks path points INSIDE the fixture at a directory that is never created: git treats a
    # missing hooks directory as no hooks, and a path under $repo cannot collide with anything the
    # host has. `--quiet` and `| Out-Null` swallow stdout only, and this suite's `$git` wrapper
    # throws with the captured `2>&1` output, so a signing failure would still name itself -- the
    # point of these two lines is that it does not happen at all.
    & $git @('config', 'commit.gpgSign', 'false') | Out-Null
    & $git @('config', 'core.hooksPath', (Join-Path $repo '.no-hooks')) | Out-Null
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'base') | Out-Null
    $base = ([string](& $git @('rev-parse', 'HEAD'))).Trim()

    $leafAtBase = Get-CrateTreeObject -RepositoryRoot $repo -Revision $base -CratePath 'core/leaf'
    Assert-True -Condition ($leafAtBase -match '^[0-9a-f]{40}$') `
        -Label 'HARNESS: the tree object is a full object id'

    # Acceptance 1a: an unrelated crate moving must not move this one, or nothing is ever reused.
    Set-Content -LiteralPath (Join-Path $repo 'core/other/src/lib.rs') -Value 'pub fn two() -> u8 { 22 }' -Encoding utf8
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'change the OTHER crate') | Out-Null
    $afterOther = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    Assert-NotEqual -Unexpected $base -Actual $afterOther -Label 'HARNESS: the commit landed'
    Assert-Equal -Expected $leafAtBase `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterOther -CratePath 'core/leaf') `
        -Label 'ACCEPTANCE 1a: an unrelated crate changing leaves the leaf tree object alone'
    Assert-NotEqual `
        -Unexpected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $base -CratePath 'core/other') `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterOther -CratePath 'core/other') `
        -Label 'CONTROL for 1a: the crate that DID change moved (the instrument can see a change)'

    # Acceptance 2: the gate's own manifest-only commit, the exact shape that kills proofs today.
    Set-Content -LiteralPath (Join-Path $repo '.factory/gate-runs/run.json') -Value '{"status":"GREEN"}' -Encoding utf8
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'gate: run manifest') | Out-Null
    $afterManifest = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    Assert-NotEqual -Unexpected $afterOther -Actual $afterManifest -Label 'HARNESS: the manifest commit landed'
    Assert-Equal -Expected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterOther -CratePath 'core/leaf') `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterManifest -CratePath 'core/leaf') `
        -Label 'ACCEPTANCE 2: a manifest-only commit does not move the leaf tree object'
    Assert-Equal -Expected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterOther -CratePath 'core/other') `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterManifest -CratePath 'core/other') `
        -Label 'ACCEPTANCE 2: nor any other crate''s'

    # Acceptance 1b: one byte of the leaf's own source.
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/src/lib.rs') -Value 'pub fn one() -> u8 { 2 }' -Encoding utf8
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'one byte of the leaf') | Out-Null
    $afterLeaf = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    Assert-NotEqual -Unexpected $leafAtBase `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterLeaf -CratePath 'core/leaf') `
        -Label 'ACCEPTANCE 1b: one byte of the leaf''s own source moves its tree object'

    # Acceptance 3: build.rs. It is a FILE NOBODY LISTED -- the point of hashing the whole tree.
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/build.rs') -Value 'fn main() {}' -Encoding utf8
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'add build.rs') | Out-Null
    $afterBuild = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    $treeWithBuild = Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuild -CratePath 'core/leaf'
    Assert-NotEqual -Unexpected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterLeaf -CratePath 'core/leaf') `
        -Actual $treeWithBuild -Label 'ACCEPTANCE 3: adding build.rs moves the tree object'
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/build.rs') -Value 'fn main() { println!("cargo:rerun-if-changed=x"); }' -Encoding utf8
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'change build.rs') | Out-Null
    $afterBuildEdit = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    Assert-NotEqual -Unexpected $treeWithBuild `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf') `
        -Label 'ACCEPTANCE 3: EDITING build.rs moves it too, with no file list to update'

    # The object database, not the working tree: an uncommitted edit must not move a committed key,
    # or two lanes with different dirty trees would disagree about a proof of the same commit.
    $beforeDirtying = Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf'
    Set-Content -LiteralPath (Join-Path $repo 'core/leaf/src/lib.rs') -Value 'pub fn one() -> u8 { 99 } // uncommitted' -Encoding utf8
    Assert-True -Condition ((Get-Content -LiteralPath (Join-Path $repo 'core/leaf/src/lib.rs') -Raw) -like '*uncommitted*') `
        -Label 'HARNESS: the working tree really was dirtied'
    Assert-Equal -Expected $beforeDirtying `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf') `
        -Label 'an UNCOMMITTED edit does not move the key: the read is from the object database'
    Assert-NotEqual -Unexpected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf') `
        -Actual (([string](& $git @('rev-parse', 'HEAD'))).Trim()) `
        -Label 'HARNESS: a tree object is not the commit id it was read from'

    # Trailing separators and backslashes are the same crate. A key that differed by spelling would
    # miss its own shard depending on who called it.
    Assert-Equal -Expected (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf') `
        -Actual (Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core\leaf\') `
        -Label 'a crate path spelled with backslashes and a trailing separator is the same crate'

    # A missing path must throw and NAME what it could not read, not return an empty string that a
    # caller would hash into a perfectly stable key for a crate that does not exist.
    $threw = $false
    $message = ''
    try {
        Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/absent' | Out-Null
    } catch {
        $threw = $true
        $message = "$($_.Exception.Message)"
    }
    Assert-True -Condition $threw -Label 'a crate path that does not exist throws rather than answering'
    Assert-True -Condition ($message -like '*core/absent*') -Label 'and the failure names the path'
    Assert-True -Condition ($message -like "*$afterBuildEdit*") -Label 'and the revision it looked at'

    # Acceptance 4, on real tree objects rather than on the fixture: a toolchain bump re-generations
    # EVERY crate at once, which is what "fresh generation instead of reusing" means.
    $toolchainA = 'rustc 1.97.1 (abcdef012 2026-01-01)'
    $toolchainB = 'rustc 1.98.0 (999888777 2026-06-01)'
    $keysA = @()
    $keysB = @()
    foreach ($crate in @('core/leaf', 'core/other')) {
        $tree = Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath $crate
        $keysA += Get-CrateInputHash -Crate $crate -TreeObject $tree -LockSlice '' -WorkspaceManifest '' -ToolchainId $toolchainA
        $keysB += Get-CrateInputHash -Crate $crate -TreeObject $tree -LockSlice '' -WorkspaceManifest '' -ToolchainId $toolchainB
    }
    Assert-Equal -Expected 2 -Actual @($keysA).Count -Label 'HARNESS: two crates were keyed'
    $sharedGeneration = @($keysA | ForEach-Object { ($_ -split '-')[0] } | Select-Object -Unique)
    Assert-Equal -Expected 1 -Actual @($sharedGeneration).Count `
        -Label 'ACCEPTANCE 4: one toolchain gives every crate ONE generation'
    $crossed = @($keysA | Where-Object { @($keysB) -contains $_ })
    Assert-Equal -Expected 0 -Actual @($crossed).Count `
        -Label 'ACCEPTANCE 4: after a bump, NO key from the old generation survives'

    Assert-NotEqual -Unexpected $keysA[0] -Actual $keysA[1] `
        -Label 'two crates under one toolchain still get distinct keys'

    # ISSUES 4's scenario, end to end. The PAIR is the point: the crate's tree object does NOT move
    # when a build profile changes, so a key built from the tree object alone would have said
    # "nothing changed" about a workspace that now compiles differently. That is the unsafe
    # direction, and this is the cell that catches it.
    # The working tree is deliberately dirty at this point -- an earlier cell left an uncommitted
    # edit in core/leaf/src/lib.rs to prove the read comes from the object database. Restore it
    # FIRST: this cell's whole claim is that the manifest is the ONLY thing that changed, and a
    # stray file in the commit would move the tree object and make the cell prove the opposite.
    # (It did, on the first run: the trap cell failed because `git add -A` swept that edit in.)
    & $git @('checkout', '--', 'core/leaf/src/lib.rs') | Out-Null
    $manifestBefore = Get-WorkspaceManifestBlob -RepositoryRoot $repo -Revision $afterBuildEdit
    $leafTreeBefore = Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterBuildEdit -CratePath 'core/leaf'
    Set-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Encoding utf8 -Value @(
        '[workspace]'
        'members = ["core/leaf", "core/other"]'
        '[profile.test]'
        'debug = 0'
    )
    & $git @('add', '-A') | Out-Null
    & $git @('commit', '--quiet', '-m', 'flip [profile.test] debug 1 -> 0') | Out-Null
    $afterProfile = ([string](& $git @('rev-parse', 'HEAD'))).Trim()
    $manifestAfter = Get-WorkspaceManifestBlob -RepositoryRoot $repo -Revision $afterProfile
    $leafTreeAfter = Get-CrateTreeObject -RepositoryRoot $repo -Revision $afterProfile -CratePath 'core/leaf'

    Assert-Equal -Expected $leafTreeBefore -Actual $leafTreeAfter `
        -Label 'THE TRAP: a build-profile change leaves the crate tree object untouched'
    Assert-NotEqual -Unexpected $manifestBefore -Actual $manifestAfter `
        -Label 'but the workspace manifest blob moves'
    $keyBefore = Get-CrateInputHash -Crate 'core/leaf' -TreeObject $leafTreeBefore -LockSlice '' `
        -WorkspaceManifest $manifestBefore -ToolchainId $toolchainA
    $keyAfter = Get-CrateInputHash -Crate 'core/leaf' -TreeObject $leafTreeAfter -LockSlice '' `
        -WorkspaceManifest $manifestAfter -ToolchainId $toolchainA
    Assert-NotEqual -Unexpected $keyBefore -Actual $keyAfter `
        -Label 'so the KEY moves: a profile change cannot replay a stale proof (ISSUES 4, #915)'

    # A missing root manifest must throw, not answer an empty string that hashes stably.
    $threwManifest = $false
    try { Get-WorkspaceManifestBlob -RepositoryRoot $repo -Revision $afterProfile -ManifestPath 'no-such.toml' | Out-Null }
    catch { $threwManifest = $true }
    Assert-True -Condition $threwManifest -Label 'a workspace manifest that is not there throws rather than answering'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, $($script:total) ran" -ForegroundColor Magenta
    Write-Host '  A cell that stopped running is indistinguishable from a cell that passed. Refusing.' -ForegroundColor Magenta
    exit 2
}
if ($script:failed -gt 0) {
    Write-Host "FAILED: $($script:failed) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "OK: $($script:total) assertions passed" -ForegroundColor Green
exit 0
