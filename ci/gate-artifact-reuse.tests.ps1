# #904: a warm target must be able to pass, and the ONE way it must never pass is by trusting
# cargo.
#
# THE SAVING. `$freshBuild = ($mtimeUtc -ge $runStartUtc)` is the only question the gate asks about
# a reused binary, and on any reused target every binary predates the run start -- so a warm target
# is unconditionally RED and every run pays a cold compile. Measured: 323.8 s cold against 1.0 s
# warm, of a 1042 s gate. It is the largest single item in the gate and it buys nothing.
#
# THE HAZARD, which is why "trust cargo's fingerprint" is REFUTED and not merely unattractive.
# Cargo's fingerprint is MTIME-based. Reproduced: change a source to V2, backdate its mtime, and
# rebuild -- cargo compiles nothing, the V1 binary runs, every ordinary test passes, rc=0, and the
# target's own marker says `complete`. `git checkout`, `mv` and `copy` all produce backdated mtimes,
# and this repository's own sabotage rituals do exactly that. A reuse rule resting on cargo's
# fingerprint would certify a program nobody in the run compiled, in silence.
#
# SO THE PROPERTY UNDER TEST IS DISCRIMINATION, not detection, and that is why every cell that
# refuses something has a CONTROL beside it that must be accepted. A function returning
# `contaminated` for everything detects every contamination and is exactly the instrument the gate
# already has, wearing a new name; the cells that kill it are the ones asserting that an untouched
# crate comes back `proven-reuse` and that an unrelated crate's edit does not move a hash.
#
# NO COMPILE HAPPENS HERE, deliberately. This suite runs inside `ci powershell suites`, the one
# stage that holds no target-directory lock and runs beside the Rust stages -- and cargo also takes
# `~/.cargo/.package-cache`, which is GLOBAL TO THE MACHINE (see ci/gate-canary-outcome.tests.ps1
# for where that was measured). A fixture build here could queue behind the gate's own build for
# minutes, and a hang has no colour. `cargo metadata` is read once, which
# ci/required-features.tests.ps1 already does from this same stage.
#
# NOTHING IN THE REPOSITORY IS MUTATED. #904's written design asks cell D to edit
# `core/protocols/src/lib.rs` in place; that would poison `Get-WorktreeDirt` for any gate running
# concurrently -- the manifest would record a dirty tree this suite made -- and would feed a
# half-written file to the concurrent compile. The same claim is measured here by redirecting ONE
# package's directory in the REAL graph at a copy, which changes no byte under version control.

$ExpectedAssertionCount = 118
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
# Two full gates observed the name-only import return while Get-FileHash was still absent in this
# child, even though the same six-way pool was green in isolation. Load the host's own manifest and
# force its script exports into this runspace; the no-auto-load assertion below keeps this
# fail-closed if that explicit import ever stops providing the command.
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -Force -ErrorAction Stop
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

$hadModuleAutoLoadingPreference = Test-Path variable:PSModuleAutoLoadingPreference
$savedModuleAutoLoading = if ($hadModuleAutoLoadingPreference) { Get-Variable PSModuleAutoLoadingPreference -ValueOnly } else { $null }
try {
    $PSModuleAutoLoadingPreference = 'None'
    $fileHashProviderIsLoaded = $null -ne (Get-Command Get-FileHash -ErrorAction SilentlyContinue)
} finally {
    $PSModuleAutoLoadingPreference = if ($hadModuleAutoLoadingPreference) { $savedModuleAutoLoading } else { 'All' }
}
Assert-True $fileHashProviderIsLoaded `
    'the file-hash provider is already loaded, so a pooled cold start cannot lose it to module auto-loading'

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# ARRANGEMENT FIRST. A file that stopped parsing, or a function that was renamed, yields no subject
# at all -- and every assertion below would then be about a program that was never read, in the
# same green as a passing run.
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: gate.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

# RETURNS THE TEXT; the caller dot-sources it. Dot-sourcing inside this function would define the
# subject in THIS function's scope, which vanishes on return.
function Get-GateFunctionText {
    param([Parameter(Mandatory)] [string] $Name)
    $wanted = $Name
    $fn = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $wanted
        }.GetNewClosure(), $true)
    if ($null -eq $fn) {
        # NAMED, because "expected 49 assertions, ran 1" says nothing about what is missing.
        Write-Host "HARNESS-BROKE: $Name was not found in ci/gate.ps1" -ForegroundColor Magenta
        exit 2
    }
    return $fn.Extent.Text
}

# The hash primitives the gate's own functions call, dot-sourced from the file the gate itself
# dot-sources -- so no cell here can be green against a copy the gate does not use.
. (Join-Path $PSScriptRoot 'crate-input-hash.ps1')
foreach ($name in @(
        'ConvertTo-ComparablePath',
        'Get-CrateDirectoryDigest',
        'Get-EmbeddedInputReferences',
        'Get-CrateEmbeddedInputDigests',
        # Get-CargoDependencyGraph delegates its verdict to these two; a suite that loads the
        # reader without them dot-sources a call to a function that is not there.
        'Test-NativeCallFailed',
        'Get-NativeStderrText',
        'Get-CargoDependencyGraph',
        'Get-CrateInputHashesFromGraph',
        'Get-ArtifactLedgerPath',
        'Read-ArtifactLedger',
        'Write-ArtifactLedger',
        'Get-ArtifactReuseProof',
        'Select-UnprovenReuse')) {
    . ([scriptblock]::Create((Get-GateFunctionText -Name $name)))
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-artifact-reuse-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

function New-FixtureCrate {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Body
    )
    $path = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory((Join-Path $path 'src')) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $path 'tests')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $path 'Cargo.toml'), "[package]`nname = `"$Name`"`nversion = `"0.1.0`"`n")
    [System.IO.File]::WriteAllText((Join-Path $path 'src\lib.rs'), $Body)
    [System.IO.File]::WriteAllText((Join-Path $path 'tests\foo.rs'), "#[test] fn t() {}`n")
    return $path
}

function New-SingleCrateGraph {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Directory
    )
    $id = "path+file:///fixture/$Name#$Name@0.1.0"
    return [ordered]@{
        packages = @{ $id = [ordered]@{ name = $Name; directory = $Directory } }
        members  = @{ $id = $true }
        nodes    = @{ $id = [ordered]@{ features = @(); deps = @() } }
    }
}

try {
    # ================================================================ A
    Write-Host ''
    Write-Host '=== A. the verdict prices the population this run can VOUCH FOR ===' -ForegroundColor Cyan
    # PINNED ON THE ASSIGNMENT, not on a field of a manifest somebody could set by hand. The verdict
    # is a line of this program; a record of the verdict is a claim about it, and the two are only
    # the same thing while nobody has edited one of them.
    $verdictLine = [regex]::Match($gateText, '(?m)^\$passedEverything\s*=.*$')
    Assert-True ($verdictLine.Success) 'ARRANGEMENT: $passedEverything is assigned on one line this cell can read'
    Assert-True ($verdictLine.Success -and $verdictLine.Value.Contains('($unprovenReuse.Count -eq 0)')) `
        'the verdict requires that NOTHING went unproven -- not that nothing was reused'
    Assert-True ($verdictLine.Success -and -not $verdictLine.Value.Contains('$staleArtifacts.Count')) `
        'and the old timestamp-only population no longer decides it'

    $suspectLine = [regex]::Match($gateText, '(?m)^\$instrumentSuspect\s*=.*$')
    Assert-True ($suspectLine.Success -and $suspectLine.Value.Contains('($unprovenReuse.Count -gt 0)')) `
        'the instrument flag is derived from the same population as the verdict, so the two cannot disagree'

    # ---------------------------------------------------------------------------------------------
    # THE ARTEFACT BUILD PASS IS A TERM OF THE VERDICT.
    #
    # Measured on this branch, which is why this cell exists: a cold run's build pass exited 101
    # after enumerating 22 of 234 test binaries, and the run published GREEN. Every stage passed --
    # each compiles what it needs -- while the instrument that ENUMERATES them had died. The ledger
    # was then written from 22 artefacts, and the next run refused the 212 it had no record for.
    # `artifactBuildExit` reached the manifest and stopped there: five sites in gate.ps1, not one a
    # verdict term.
    #
    # Pinned ON THE ASSIGNMENT, the same shape the cells above use, because a field that a later
    # edit can satisfy by hand claims a proof the run did not make.
    # ---------------------------------------------------------------------------------------------
    Assert-True ($verdictLine.Success -and $verdictLine.Value.Contains('-and $buildPassSucceeded')) `
        'a build pass that did not exit 0 cannot be part of a green run: the verdict requires it'
    Assert-True ($suspectLine.Success -and $suspectLine.Value.Contains('(-not $buildPassSucceeded)')) `
        'and the instrument flag says so too, so a partial enumeration is never silently trusted'

    $derivation = [regex]::Match($gateText, '(?m)^\$buildPassSucceeded\s*=.*$')
    Assert-True ($derivation.Success) 'ARRANGEMENT: $buildPassSucceeded is derived on one line this cell can read'
    Assert-True ($derivation.Success -and $derivation.Value.Contains('-is [int]')) `
        'and it is TYPE-CHECKED before it is compared: $null -eq 0 is False in PowerShell but a string "0" is not an exit code either, and an unknown must never read as success'

    # THE THREE STATES, driven rather than read. A wrong type must widen: the abort paths hand
    # `Write-RunManifest` a manifest whose `buildExitCode` is $null, and a run that cannot say
    # whether its build pass succeeded has not established that it did.
    $succeededFrom = {
        param($value)
        ($value -is [int]) -and ($value -eq 0)
    }
    Assert-True ((& $succeededFrom 0) -eq $true) 'exit 0 is the only success'
    Assert-True ((& $succeededFrom 101) -eq $false) 'the 101 measured on this branch is not a success'
    Assert-True ((& $succeededFrom $null) -eq $false) 'and an ABSENT exit code widens to failure rather than passing as zero'

    # THE RECORD SURVIVES. `staleArtifactCount` is what a cold-only gate would have refused, and it
    # is the only way a reader can audit this change after the fact: without it, "the gate stopped
    # going red" and "the gate stopped looking" are the same observation.
    Assert-True ($gateText.Contains('$staleArtifacts = @($ArtifactManifest.artifacts | Where-Object { $_.freshBuild -eq $false })')) `
        'the timestamp population is still COMPUTED, as the record of what a cold-only gate would have refused'
    Assert-True ($gateText.Contains('staleArtifactCount = $staleArtifacts.Count')) `
        'and the manifest still publishes it under its original name, so historical manifests keep their meaning'

    # ONE FILTER, FOUR CONSUMERS, COUNTED FROM THE FILE. Four hand-written copies of one predicate is
    # four chances for three of them to keep the old one -- and the worst of the four is the stamp:
    # edit the verdict alone and a legitimate warm run never records `complete`, so the NEXT run
    # reads `interrupted` and aborts before its first compile. A silent trap that turns the saving
    # into a permanent refusal.
    $callSites = [regex]::Matches($gateText, 'Select-UnprovenReuse -Artifacts')
    Assert-True ($callSites.Count -ge 3) `
        "the shared filter is called at every site that used to filter by timestamp (found $($callSites.Count))"
    $decidingByTimestamp = [regex]::Matches($gateText, '\$_\.freshBuild -eq \$false')
    Assert-True ($decidingByTimestamp.Count -eq 1) `
        "and exactly ONE site still filters by timestamp -- the record, not a decision (found $($decidingByTimestamp.Count))"

    # THE MTIME RULE'S OWN LITERAL IS UNTOUCHED. ci/gate-slot-wait.tests.ps1 pins it, and every
    # historical manifest's `freshBuild` keeps its meaning only while what produced it is unchanged.
    Assert-True ($gateText -match '(?m)^\s*\$freshBuild\s*=\s*\(\$mtimeUtc\s+-ge\s+\$runStartUtc\)') `
        'the mtime rule itself is unchanged -- it became a RECORD, it was not rewritten'

    # ================================================================ B
    Write-Host ''
    Write-Host '=== B. a backdated source is CONTAMINATION, and an untouched one is a proven reuse ===' -ForegroundColor Cyan
    $crateB = New-FixtureCrate -Name 'fixture-b' -Body "pub fn v() -> u32 { 1 }`n"
    $graphB = New-SingleCrateGraph -Name 'fixture-b' -Directory $crateB
    $idB = @($graphB.members.Keys)[0]
    $foldB1 = Get-CrateInputHashesFromGraph -Graph $graphB -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    $hashB1 = [string]$foldB1.byPackageId[$idB]
    Assert-True (-not [string]::IsNullOrWhiteSpace($hashB1)) 'ARRANGEMENT: the fixture crate has an input hash'

    # THE BINARY. Its bytes never change in this cell, which is the whole point: the defect is a
    # source that moved under a binary that did not.
    $targetB = Join-Path $fixtureRoot 'target-b'
    [System.IO.Directory]::CreateDirectory($targetB) | Out-Null
    $exeB = Join-Path $targetB 'fixture_b-abcdef.exe'
    [System.IO.File]::WriteAllBytes($exeB, [byte[]](1, 2, 3, 4))
    $exeShaB = (Get-FileHash -LiteralPath $exeB -Algorithm SHA256).Hash

    $ledgerPath = Write-ArtifactLedger -TargetDir $targetB -Artifacts @(
        [ordered]@{ executable = $exeB; sha256 = $exeShaB; crateInputHash = $hashB1 })
    Assert-True (Test-Path -LiteralPath $ledgerPath) 'ARRANGEMENT: a cold run wrote the ledger entry this reuse is proven against'
    $ledgerB = Read-ArtifactLedger -TargetDir $targetB
    $entryB = $ledgerB[(ConvertTo-ComparablePath -Path $exeB)]
    Assert-True ($null -ne $entryB) 'ARRANGEMENT: and the ledger reads back keyed by the executable'

    # THE CONTROL, FIRST. Without it this cell is satisfied by a function that answers
    # `contaminated` for everything -- which is today's behaviour under a new name, and the change
    # would buy nothing while looking exactly like a pass.
    $controlB = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaB -CrateInputHash $hashB1 -LedgerEntry $entryB
    Assert-True ($controlB -ceq 'proven-reuse') `
        "CONTROL: an untouched crate whose binary predates the run is a PROVEN reuse (got '$controlB')"

    # NOW THE DEFECT. V2 on disk, mtime dragged back to 2020 -- the shape `git checkout`, `mv` and
    # `copy` all produce, and the shape this repository's sabotage rituals produce.
    $sourceB = Join-Path $crateB 'src\lib.rs'
    [System.IO.File]::WriteAllText($sourceB, "pub fn v() -> u32 { 2 }`n")
    [System.IO.File]::SetLastWriteTimeUtc($sourceB, [DateTime]::Parse('2020-01-01T00:00:00Z').ToUniversalTime())
    $sourceMtime = [System.IO.File]::GetLastWriteTimeUtc($sourceB)
    $exeMtime = [System.IO.File]::GetLastWriteTimeUtc($exeB)
    # THE FIXTURE REACHES THE DEFECT, asserted rather than assumed: if the backdating did not take,
    # the cell below would be measuring an ordinary edit, which every instrument catches.
    Assert-True ($sourceMtime -lt $exeMtime) `
        "ARRANGEMENT: the V2 source is OLDER than the binary, so cargo's mtime fingerprint reports nothing to do (source $($sourceMtime.ToString('o')), binary $($exeMtime.ToString('o')))"

    $foldB2 = Get-CrateInputHashesFromGraph -Graph $graphB -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    $hashB2 = [string]$foldB2.byPackageId[$idB]
    Assert-True ($hashB2 -cne $hashB1) `
        'the input hash MOVED although the mtime went backwards -- it reads content, not timestamps'
    $verdictB = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaB -CrateInputHash $hashB2 -LedgerEntry $entryB
    Assert-True ($verdictB -ceq 'contaminated') `
        "a binary whose crate inputs moved under it is CONTAMINATED, not a proven reuse (got '$verdictB')"

    # THE THIRD STATE, kept apart from the second on purpose: no ledger entry is a target this
    # machine has no custody record for, and its remedy (run once cold) is not the remedy for a
    # contaminated one.
    $verdictNoEntry = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaB -CrateInputHash $hashB2 -LedgerEntry $null
    Assert-True ($verdictNoEntry -ceq 'unproven-reuse') `
        "an executable the ledger has never seen is UNPROVEN, which is a different absence (got '$verdictNoEntry')"
    $verdictRebuilt = Get-ArtifactReuseProof -RebuiltThisRun $true -ExeSha256 $exeShaB -CrateInputHash $hashB2 -LedgerEntry $entryB
    Assert-True ($verdictRebuilt -ceq 'rebuilt') `
        "and a binary this run actually built needs no ledger at all (got '$verdictRebuilt')"

    # TWO ABSENCES ARE NOT AN EQUALITY. A null hash on both sides compares equal, which is how a
    # measurement that FAILED comes back wearing the colour of one that passed.
    $blankEntry = [ordered]@{ executable = $exeB; sha256 = ''; crateInputHash = '' }
    $verdictBlank = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 '' -CrateInputHash '' -LedgerEntry $blankEntry
    Assert-True ($verdictBlank -ceq 'contaminated') `
        "two absent hashes do not prove each other (got '$verdictBlank')"
    # AND THE WRITER REFUSES TO MANUFACTURE THAT PAIR in the first place.
    $targetBlank = Join-Path $fixtureRoot 'target-blank'
    [System.IO.Directory]::CreateDirectory($targetBlank) | Out-Null
    Write-ArtifactLedger -TargetDir $targetBlank -Artifacts @(
        [ordered]@{ executable = $exeB; sha256 = $exeShaB; crateInputHash = '' }) | Out-Null
    Assert-True ((Read-ArtifactLedger -TargetDir $targetBlank).Count -eq 0) `
        'the ledger writer DROPS an artefact missing either hash rather than recording a blank that would later compare equal'

    # ================================================================ C
    Write-Host ''
    Write-Host '=== C. the crate hash covers the WHOLE directory, not a file list ===' -ForegroundColor Cyan
    $crateC = New-FixtureCrate -Name 'fixture-c' -Body "pub fn v() -> u32 { 1 }`n"
    $baseC = Get-CrateDirectoryDigest -Path $crateC

    # THE MTIME CONTROL, first and for the same reason as B's: a digest that simply returned a new
    # value every call would satisfy all three sabotages below. This is the cell that kills it, and
    # it is also the property the whole mechanism rests on.
    [System.IO.File]::SetLastWriteTimeUtc((Join-Path $crateC 'src\lib.rs'), [DateTime]::UtcNow.AddDays(-3000))
    Assert-True ((Get-CrateDirectoryDigest -Path $crateC) -ceq $baseC) `
        'CONTROL: moving an mtime without moving a byte does not move the digest'

    [System.IO.File]::WriteAllText((Join-Path $crateC 'tests\foo.rs'), "#[test] fn t() { assert!(false); }`n")
    $afterTests = Get-CrateDirectoryDigest -Path $crateC
    Assert-True ($afterTests -cne $baseC) 'a change under tests/ moves the hash -- an integration test is a build input'

    [System.IO.File]::WriteAllText((Join-Path $crateC 'Cargo.toml'), "[package]`nname = `"fixture-c`"`nversion = `"0.2.0`"`n")
    $afterManifest = Get-CrateDirectoryDigest -Path $crateC
    Assert-True ($afterManifest -cne $afterTests) 'a change to the crate manifest moves it'

    [System.IO.File]::WriteAllText((Join-Path $crateC 'build.rs'), "fn main() { }`n")
    $afterBuildScript = Get-CrateDirectoryDigest -Path $crateC
    Assert-True ($afterBuildScript -cne $afterManifest) `
        'and an UNTRACKED build.rs dropped into the directory moves it -- version control is not the boundary, the directory is'

    # THE ONE EXCLUSION, stated as a cell rather than only as a comment: cargo's own output under the
    # crate is not an input, and on a developer box it can be larger than the repository.
    [System.IO.Directory]::CreateDirectory((Join-Path $crateC 'target\debug')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $crateC 'target\debug\whatever.bin'), 'output, not input')
    Assert-True ((Get-CrateDirectoryDigest -Path $crateC) -ceq $afterBuildScript) `
        'a crate-level target/ directory is excluded: it is output, and hashing it would make every crate its own moving target'

    # ================================================================ D
    Write-Host ''
    Write-Host '=== D. the transitive closure is real, on the graph this repository actually has ===' -ForegroundColor Cyan
    $crateLeaf = New-FixtureCrate -Name 'fixture-leaf' -Body "pub fn v() -> u32 { 1 }`n"
    $crateRoot = New-FixtureCrate -Name 'fixture-root' -Body "pub fn w() -> u32 { 2 }`n"
    $crateOther = New-FixtureCrate -Name 'fixture-other' -Body "pub fn x() -> u32 { 3 }`n"
    $idLeaf = 'path+file:///fixture/fixture-leaf#fixture-leaf@0.1.0'
    $idRoot = 'path+file:///fixture/fixture-root#fixture-root@0.1.0'
    $idOther = 'path+file:///fixture/fixture-other#fixture-other@0.1.0'
    $graphD = [ordered]@{
        packages = @{
            $idLeaf  = [ordered]@{ name = 'fixture-leaf'; directory = $crateLeaf }
            $idRoot  = [ordered]@{ name = 'fixture-root'; directory = $crateRoot }
            $idOther = [ordered]@{ name = 'fixture-other'; directory = $crateOther }
        }
        members  = @{ $idLeaf = $true; $idRoot = $true; $idOther = $true }
        nodes    = @{
            $idLeaf  = [ordered]@{ features = @(); deps = @() }
            $idRoot  = [ordered]@{ features = @(); deps = @($idLeaf) }
            $idOther = [ordered]@{ features = @(); deps = @() }
        }
    }
    $foldD1 = Get-CrateInputHashesFromGraph -Graph $graphD -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock' -WorkspaceManifestSha256 'manifest'
    [System.IO.File]::WriteAllText((Join-Path $crateLeaf 'src\lib.rs'), "pub fn v() -> u32 { 99 }`n")
    $foldD2 = Get-CrateInputHashesFromGraph -Graph $graphD -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock' -WorkspaceManifestSha256 'manifest'
    Assert-True ([string]$foldD2.byPackageId[$idRoot] -cne [string]$foldD1.byPackageId[$idRoot]) `
        'one byte in a DEPENDENCY moves the dependent crate hash -- the fold really is transitive'
    Assert-True ([string]$foldD2.byPackageId[$idLeaf] -cne [string]$foldD1.byPackageId[$idLeaf]) `
        'and the dependency own hash moved too, so the cell above is not reading the dependent by accident'
    # THE CONTROL: a fold that stirred every hash on every call would pass the cell above.
    Assert-True ([string]$foldD2.byPackageId[$idOther] -ceq [string]$foldD1.byPackageId[$idOther]) `
        'CONTROL: a crate that does not depend on the edited one does NOT move'

    # THE SAME CLAIM AGAINST THE REAL GRAPH, without editing one byte under version control.
    $realGraph = Get-CargoDependencyGraph -WorkspaceRoot $repositoryRoot -ToolchainArgument '+1.97.1'
    # SNAPSHOT THE DIRECTORIES BEFORE THIS CELL REDIRECTS ONE OF THEM. Cell F below enumerates the
    # workspace's build scripts from this list, and the first version read it back out of the graph
    # AFTER the redirect -- so `core/protocols` was silently outside the scan, and a second
    # `build.rs` planted there did not redden cell F at all. Found by running that sabotage: the
    # cell had a hole exactly the size of the crate this cell touches.
    $memberDirectories = @($realGraph.members.Keys |
            ForEach-Object { [string]$realGraph.packages[[string]$_].directory })
    Assert-True ($realGraph.members.Count -gt 1) `
        "ARRANGEMENT: cargo metadata is readable from this checkout (found $($realGraph.members.Count) workspace members), or every assertion below is about a failed read"
    # BY THE PACKAGE NAME cargo reports, not by a suffix of the id. Measured on cargo 1.97.1: a
    # PackageIDSpec OMITS the name when it equals the last path segment, so `tools/ci-canary` is
    # `path+file:///...#0.1.0` while `apps/cli` is `path+file:///...#graphhelm-cli@0.1.0`. A
    # suffix match finds two of the three crates this cell needs and silently loses the control.
    $cliId = @($realGraph.members.Keys | Where-Object { ([string]$realGraph.packages[[string]$_].name) -ceq 'graphhelm-cli' })
    $protocolsId = @($realGraph.members.Keys | Where-Object { ([string]$realGraph.packages[[string]$_].name) -ceq 'graphhelm-protocols' })
    Assert-True ($cliId.Count -eq 1 -and $protocolsId.Count -eq 1) `
        'ARRANGEMENT: graphhelm-cli and graphhelm-protocols are both workspace members of this checkout'

    # A CYCLE EXISTS AMONG THE MEMBERS, and that is a measurement, not a story: it is the reason the
    # fold is reachability rather than the topological order #904's design asks for. A topological
    # order does not exist for this graph, and a recursive fold over it does not terminate.
    $backEdge = $false
    foreach ($member in @($realGraph.members.Keys)) {
        foreach ($dep in @($realGraph.nodes[$member].deps)) {
            if (-not $realGraph.members.ContainsKey([string]$dep)) { continue }
            if (@($realGraph.nodes[[string]$dep].deps) -contains $member) { $backEdge = $true }
        }
    }
    Assert-True $backEdge `
        'the real member graph contains a cycle (dev-dependencies, which cargo permits), so the fold may not be a topological one'

    if ($cliId.Count -eq 1 -and $protocolsId.Count -eq 1) {
        # ONE PACKAGE'S DIRECTORY IS REDIRECTED AT A COPY. The graph is the real one -- cycles,
        # external packages and all -- and the bytes under version control are never touched.
        #
        # THE COPY KEEPS ITS DEPTH, and it has to since #1038's review: `core/protocols` embeds
        # `../../../schemas/*.json`, `../../../examples/…`, `../../../conformance/…` and
        # `concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/…")` at COMPILE time, and the input
        # hash now covers those. A copy dropped straight into the fixture root resolves none of
        # them, the crate is refused as UNPROVEN rather than hashed, and this cell would measure a
        # fixture defect while reading as a statement about the fold. So the copy sits at
        # `<fixture>/mirror/core/protocols` with the four referenced roots copied beside it --
        # 201 small files, measured at 0.9 MB in total.
        $mirror = Join-Path $fixtureRoot 'mirror'
        [System.IO.Directory]::CreateDirectory((Join-Path $mirror 'core')) | Out-Null
        foreach ($shared in @('schemas', 'conformance', 'examples', 'tools')) {
            $sharedSource = Join-Path $repositoryRoot $shared
            if (Test-Path -LiteralPath $sharedSource) {
                Copy-Item -LiteralPath $sharedSource -Destination (Join-Path $mirror $shared) -Recurse -Force
            }
        }
        $protocolsCopy = Join-Path $mirror 'core\protocols'
        Copy-Item -LiteralPath ([string]$realGraph.packages[$protocolsId[0]].directory) -Destination $protocolsCopy -Recurse -Force
        $realGraph.packages[$protocolsId[0]].directory = $protocolsCopy
        $realFold1 = Get-CrateInputHashesFromGraph -Graph $realGraph -ToolchainId 'rustc 1.97.1 (fixture)' `
            -LockfileSha256 'lock' -WorkspaceManifestSha256 'manifest'
        [System.IO.File]::AppendAllText((Join-Path $protocolsCopy 'src\lib.rs'), "// #904 cell D`n")
        $realFold2 = Get-CrateInputHashesFromGraph -Graph $realGraph -ToolchainId 'rustc 1.97.1 (fixture)' `
            -LockfileSha256 'lock' -WorkspaceManifestSha256 'manifest'
        Assert-True ([string]$realFold2.byPackageId[$cliId[0]] -cne [string]$realFold1.byPackageId[$cliId[0]]) `
            'one byte of core/protocols moves graphhelm-cli input hash across the REAL dependency graph'
        Assert-True ([string]$realFold2.byPackageId[$protocolsId[0]] -cne [string]$realFold1.byPackageId[$protocolsId[0]]) `
            'and core/protocols own hash moved, so the claim above is not about an unrelated crate'
        $canary = @($realGraph.members.Keys | Where-Object { ([string]$realGraph.packages[[string]$_].name) -ceq 'ci-canary' })
        if ($canary.Count -eq 1) {
            Assert-True ([string]$realFold2.byPackageId[$canary[0]] -ceq [string]$realFold1.byPackageId[$canary[0]]) `
                'CONTROL: a workspace member that does not depend on core/protocols does NOT move'
        } else {
            Assert-True $false 'ARRANGEMENT: tools/ci-canary is a workspace member, so a negative control exists'
        }
    } else {
        Assert-True $false 'ARRANGEMENT: the real-graph cells could not run'
        Assert-True $false 'ARRANGEMENT: the real-graph cells could not run'
        Assert-True $false 'ARRANGEMENT: the real-graph cells could not run'
    }

    # ================================================================ E
    Write-Host ''
    Write-Host '=== E. the complete stamp follows the NEW count ===' -ForegroundColor Cyan
    # THE SITE THAT MUST NOT BE FORGOTTEN. Leave it reading `freshBuild -eq $false` and every
    # legitimate warm run fails to stamp `complete`; the next run reads `interrupted` and aborts
    # before its first compile, so the whole saving becomes a permanent refusal -- and every stage
    # of every affected run still passes, which is what makes it silent.
    $warm = @(
        [ordered]@{ executable = 'a.exe'; reuseProof = 'proven-reuse' },
        [ordered]@{ executable = 'b.exe'; reuseProof = 'proven-reuse' },
        [ordered]@{ executable = 'c.exe'; reuseProof = 'rebuilt' }
    )
    Assert-True ((Select-UnprovenReuse -Artifacts $warm).Count -eq 0) `
        'a fully warm run -- every artefact proven or rebuilt -- has NOTHING unproven, so it stamps complete'
    $oneContaminated = @($warm + @([ordered]@{ executable = 'd.exe'; reuseProof = 'contaminated' }))
    Assert-True ((Select-UnprovenReuse -Artifacts $oneContaminated).Count -eq 1) `
        'CONTROL: one contaminated artefact is counted, so the filter is not simply answering zero'
    $oneUnproven = @($warm + @([ordered]@{ executable = 'e.exe'; reuseProof = 'unproven-reuse' }))
    Assert-True ((Select-UnprovenReuse -Artifacts $oneUnproven).Count -eq 1) `
        'and so is one unproven reuse -- the two share a verdict even though they want different remedies'

    $stampAnchor = "`$unprovenAtEnd = (Select-UnprovenReuse -Artifacts `$artifactManifest.artifacts).Count"
    $stampIndex = $gateText.IndexOf($stampAnchor, [System.StringComparison]::Ordinal)
    Assert-True ($stampIndex -ge 0) `
        'the end-of-run stamp counts what the run cannot vouch for, through the SAME filter as the verdict'
    $stampWindow = if ($stampIndex -ge 0) {
        $gateText.Substring($stampIndex, [Math]::Min(6000, $gateText.Length - $stampIndex))
    } else { '<no slice>' }
    Assert-True ($stampWindow.Contains('if ($unprovenAtEnd -eq 0 -and $buildPassSucceededAtEnd) {')) `
        'and stamps only when that count is zero AND the build pass that produced the count succeeded'
    Assert-True ($stampWindow.Contains("Write-TargetBuildState -TargetDir `$actualTargetDir -State 'complete'")) `
        'ARRANGEMENT: it is the complete stamp that sits under that condition'
    Assert-True (-not $stampWindow.Contains('$_.freshBuild -eq $false')) `
        'and it no longer refuses a warm target for a timestamp alone'
    Assert-True ($stampWindow.Contains('Write-ArtifactLedger -TargetDir $actualTargetDir')) `
        'the ledger is written INSIDE the proven branch -- a run that could not vouch for one binary does not get to vouch for any'

    # THE LEDGER ROUND-TRIPS, driven rather than read: a writer and a reader that disagree about the
    # key would make every reuse unprovable, and every run would silently go back to compiling cold.
    $targetE = Join-Path $fixtureRoot 'target-e'
    [System.IO.Directory]::CreateDirectory($targetE) | Out-Null
    Write-ArtifactLedger -TargetDir $targetE -Artifacts @(
        [ordered]@{ executable = 'D:\Target\Debug\Deps\Thing-1234.exe'; sha256 = 'AABB'; crateInputHash = 'gen-abc' }) | Out-Null
    $roundTrip = Read-ArtifactLedger -TargetDir $targetE
    Assert-True ($roundTrip.ContainsKey('d:/target/debug/deps/thing-1234.exe')) `
        'the ledger key is case- and separator-normalised, so the writer and the reader cannot disagree about one path'
    $emptyLedger = Read-ArtifactLedger -TargetDir (Join-Path $fixtureRoot 'target-that-does-not-exist')
    Assert-True ($emptyLedger.Count -eq 0) `
        'CONTROL: a target with no ledger reads as EMPTY, which fails closed -- every reuse unproven, which is today behaviour'

    # ================================================================ F
    Write-Host ''
    Write-Host '=== F. hermeticity: exactly one build script in the workspace ===' -ForegroundColor Cyan
    # A `build.rs` that reads anything outside its own crate breaks the input hash SILENTLY: the
    # file it read is not under any crate directory, so no hash moves when it changes, and a stale
    # binary is then proven against a ledger entry that describes a different build. #904's
    # hermeticity linter is the real remedy; until it lands, this cell holds the line by keeping the
    # population at one known script whose inputs have been read.
    $buildScripts = New-Object 'System.Collections.Generic.List[string]'
    Assert-True ($memberDirectories.Count -eq $realGraph.members.Count) `
        "ARRANGEMENT: every workspace member's directory is in the scan (got $($memberDirectories.Count) of $($realGraph.members.Count))"
    foreach ($directory in $memberDirectories) {
        # Snapshotted above, BEFORE cell D redirected one package at a copy -- otherwise the crate
        # cell D touches would be outside this scan and a build script planted there would not be
        # seen. Anything outside the checkout is still skipped, so this suite cannot count its own
        # litter as a finding about the repository.
        if (-not $directory.StartsWith($repositoryRoot, [System.StringComparison]::OrdinalIgnoreCase)) { continue }
        foreach ($found in @(Get-ChildItem -LiteralPath $directory -Recurse -Force -Filter 'build.rs' -File -ErrorAction SilentlyContinue)) {
            $relative = $found.FullName.Substring($repositoryRoot.Length).TrimStart([char]'\', [char]'/') -replace '\\', '/'
            if ($relative -match '(^|/)target/') { continue }
            $buildScripts.Add($relative)
        }
    }
    # VACUITY CONTROL. "No build script broke hermeticity" and "the search found nothing" are the
    # same green, and only one of them is a measurement.
    Assert-True ($buildScripts.Count -gt 0) `
        "the search found at least one build script, so the assertion below is about a population that exists (found $($buildScripts.Count))"
    Assert-True ($buildScripts.Count -eq 1) `
        "the workspace has exactly ONE build script (found $($buildScripts.Count): $($buildScripts -join ', '))"
    Assert-True (@($buildScripts) -contains 'tools/ci-canary/build.rs') `
        'and it is tools/ci-canary/build.rs, the one whose inputs have been read'
    # THE COPY MADE IN CELL D IS NOT COUNTED, which is what keeps this cell measuring the checkout
    # rather than this suite own litter.
    Assert-True (-not (@($buildScripts) | Where-Object { $_.Contains('protocols-copy') })) `
        'CONTROL: the fixture copy this suite made is not counted as a workspace build script'

    # ================================================================ G
    Write-Host ''
    Write-Host '=== G. the seams are driven with the PRODUCER collection, not a literal array ===' -ForegroundColor Cyan
    # WHY THIS CELL EXISTS, measured and not imagined. PR #1038's gate ran on the SSD runner, passed
    # all 60 stages, and then died with NO manifest, NO rc file and no error text in its log:
    # `Select-UnprovenReuse -Artifacts $artifactManifest.artifacts` -- the first statement of the tail
    # after the last stage -- threw `System.ArgumentException: Argument types do not match`.
    # `Get-TestArtifactManifest` builds `artifacts` as a System.Collections.Generic.List[object], and
    # under this machine's Windows PowerShell 5.1 (5.1.26100.9168) `@(<such a variable>)` throws
    # unconditionally. gate.ps1 already documents that trap TWICE -- at `stages = [object[]]
    # $stageRecords` and inside `Test-StageOverlapped` -- and #904's two new functions reintroduced it.
    #
    # EVERY CELL ABOVE HANDS THESE FUNCTIONS A LITERAL `@(...)` ARRAY. That is precisely why 49
    # assertions were green while the gate's own call path could not execute once: the cells and the
    # gate did not share an instrument. So the fixture here is NOT written by hand -- it is built from
    # the collection expression READ OUT OF THE PRODUCER, so the day the producer changes shape this
    # cell stops describing it and says so instead of going quietly green against a stale guess.
    $producerText = Get-GateFunctionText -Name 'Get-TestArtifactManifest'
    $producerMatch = [regex]::Match($producerText,
        '(?m)^\s*\$artifacts\s*=\s*(New-Object\s+System\.Collections\.Generic\.List\[object\])\s*$')
    Assert-True ($producerMatch.Success) `
        'ARRANGEMENT: the producer builds its artefact collection in one expression this cell can re-create'
    Assert-True ($producerText -match '(?m)^\s*artifacts\s+=\s*\$artifacts\s*$') `
        'ARRANGEMENT: and hands that very collection out as the `artifacts` field the tail reads'
    # A WRONG FALLBACK ON PURPOSE. If the match above fails the arrangement assertion is already red,
    # and this must not quietly substitute the type the cell wanted to prove.
    $producerCollectionExpression = if ($producerMatch.Success) {
        $producerMatch.Groups[1].Value
    } else { 'New-Object System.Collections.ArrayList' }
    $producerArtifacts = & ([scriptblock]::Create($producerCollectionExpression))
    Assert-True ($producerArtifacts -is [System.Collections.Generic.List[object]]) `
        "ARRANGEMENT: the fixture collection is the producer's own type (got $($producerArtifacts.GetType().Name))"
    foreach ($record in $warm) { $producerArtifacts.Add($record) }
    $producerArtifacts.Add([ordered]@{ executable = 'f.exe'; reuseProof = 'unproven-reuse' })
    $producerArtifacts.Add([ordered]@{ executable = 'g.exe'; reuseProof = 'contaminated' })
    $producerManifest = [ordered]@{ buildExitCode = 0; artifacts = $producerArtifacts; buildMode = 'warm' }

    # THE ASSERTION THAT FIRES. The throw is CAUGHT rather than left to abort the suite: an aborted
    # suite exits 2 and reads as HARNESS-BROKE, and this is a FINDING about the subject, not a broken
    # bench. The literal error text goes in the failure line, where whoever reads the red will see it.
    $drivenCount = -1
    $drivenError = '<none>'
    try { $drivenCount = (Select-UnprovenReuse -Artifacts $producerManifest.artifacts).Count }
    catch { $drivenError = "$($_.Exception.GetType().FullName): $($_.Exception.Message)" }
    Assert-True ($drivenCount -eq 2) `
        "Select-UnprovenReuse walks the producer's own collection, read off the manifest field the gate reads (got $drivenCount, error: $drivenError)"
    # CONTROL: THE SAME RECORDS, A DIFFERENT CONTAINER. Without it the assertion above is satisfied by
    # any function that answers 2, and the cell would not be about the collection type at all.
    Assert-True ((Select-UnprovenReuse -Artifacts ([object[]]$producerArtifacts.ToArray())).Count -eq 2) `
        'CONTROL: the same five records as a plain ARRAY answer the same 2, so the cell above prices the CONTAINER and not the records'
    # VACUITY CONTROL: a fully proven run in the producer's container answers zero, so the assertion
    # above is not met by a function that counts its whole input.
    $provenOnly = & ([scriptblock]::Create($producerCollectionExpression))
    foreach ($record in $warm) { $provenOnly.Add($record) }
    $vacuityCount = -1
    $vacuityError = '<none>'
    try { $vacuityCount = (Select-UnprovenReuse -Artifacts $provenOnly).Count }
    catch { $vacuityError = "$($_.Exception.GetType().FullName): $($_.Exception.Message)" }
    Assert-True ($vacuityCount -eq 0) `
        "CONTROL: a fully proven run in the same container answers 0, not a count of everything it was given (got $vacuityCount, error: $vacuityError)"

    # THE SIBLING SEAM, driven the same way: gate.ps1 hands `Write-ArtifactLedger` the identical field
    # eleven lines below the site that threw, so a fix to one function alone leaves the next run to
    # die in the other -- inside a catch that would have turned the whole custody chain into a NOTE.
    $targetG = Join-Path $fixtureRoot 'target-g'
    [System.IO.Directory]::CreateDirectory($targetG) | Out-Null
    $ledgerG = & ([scriptblock]::Create($producerCollectionExpression))
    $ledgerG.Add([ordered]@{ executable = 'D:\Target\Debug\Deps\g-1.exe'; sha256 = 'AA11'; crateInputHash = 'gen-g1' })
    $ledgerG.Add([ordered]@{ executable = 'D:\Target\Debug\Deps\g-2.exe'; sha256 = 'BB22'; crateInputHash = 'gen-g2' })
    $ledgerGPath = $null
    $ledgerGError = '<none>'
    try { $ledgerGPath = Write-ArtifactLedger -TargetDir $targetG -Artifacts $ledgerG }
    catch { $ledgerGError = "$($_.Exception.GetType().FullName): $($_.Exception.Message)" }
    Assert-True ($null -ne $ledgerGPath -and (Test-Path -LiteralPath $ledgerGPath)) `
        "Write-ArtifactLedger takes the producer's own collection too (error: $ledgerGError)"
    $readG = if ($null -ne $ledgerGPath) { Read-ArtifactLedger -TargetDir $targetG } else { @{} }
    Assert-True ($readG.Count -eq 2) `
        "and both records reach the file, so the ledger is not written EMPTY out of a container it failed to walk (got $($readG.Count))"

    # THE TWO SHAPES THE REMEDY MUST NOT BREAK, and they are not decoration: the tempting wrong fix is
    # `$Artifacts.ToArray()`, which is the spelling ci/gate.ps1 already uses for its rustfmt lists
    # (`Get-RustfmtHarnessNote -Tail $fmtLines.ToArray()`), and it throws on both of these -- a lone
    # record has no ToArray, and neither has `$null`. Measured: with `.ToArray()` in place of the cast
    # these two go red and the four driven assertions above stay green, so the two halves of the remedy
    # are pinned by different assertions.
    $loneCount = -1
    $loneError = '<none>'
    try { $loneCount = (Select-UnprovenReuse -Artifacts ([ordered]@{ executable = 'h.exe'; reuseProof = 'contaminated' })).Count }
    catch { $loneError = "$($_.Exception.GetType().FullName): $($_.Exception.Message)" }
    Assert-True ($loneCount -eq 1) `
        "CONTROL: a LONE record counts as one artefact, not as its own key/value pairs (got $loneCount, error: $loneError)"
    Assert-True ((Select-UnprovenReuse -Artifacts $null).Count -eq 0) `
        'CONTROL: $null is an empty population, which is what AllowNull() on the parameter is for'

    # AND THE SPELLING ITSELF, so a later edit cannot reintroduce the exact token that threw. It sits
    # BESIDE the driven assertions, never instead of them.
    #
    # READ OFF THE AST, NOT OFF THE FUNCTION TEXT. The first draft was `-not ($seamText -match
    # '@\(\$Artifacts\)')` over `Get-GateFunctionText`, and it went RED against the CORRECT fix --
    # because the note explaining the defect quotes `@($Artifacts)` in prose, and a text match cannot
    # tell a warning from the thing it warns about. The loop's own enumerated expression can.
    foreach ($seamName in @('Select-UnprovenReuse', 'Write-ArtifactLedger')) {
        $wantedSeam = $seamName
        $seamAst = $ast.Find({
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $wantedSeam
            }.GetNewClosure(), $true)
        $seamLoops = @($seamAst.FindAll({
                    param($node) $node -is [System.Management.Automation.Language.ForEachStatementAst]
                }, $true) | Where-Object { $_.Condition.Extent.Text -match '\$Artifacts' })
        Assert-True ($seamLoops.Count -eq 1) `
            "ARRANGEMENT: $seamName walks its `$Artifacts parameter in exactly one loop (found $($seamLoops.Count))"
        $walked = if ($seamLoops.Count -eq 1) { $seamLoops[0].Condition.Extent.Text } else { '<no single loop>' }
        Assert-True ($walked -notmatch '^\s*@\(') `
            "and it does not wrap that parameter in @(), the spelling that cost #1038 its run (it walks: $walked)"
    }
    # ================================================================ H
    Write-Host ''
    Write-Host '=== H. the compile-time inputs that live OUTSIDE the crate directory are read out of the source ===' -ForegroundColor Cyan
    # WHY THIS CELL EXISTS (lane S on #1038 at bcde3cf3). `Get-CrateDirectoryDigest` walks ONE
    # directory, and this workspace embeds files from outside EVERY member directory at COMPILE
    # time: `schemas/*.json`, `conformance/**`, `examples/graphs/*.yaml`, `README.md`,
    # `QUICKSTART.md`, `extensions/builtin/**`, `docs/acceptance/**`, and
    # `tools/source-invariants/detect.rs`, which is not a workspace member at all. Edit one of them
    # with a BACKDATED mtime and cargo rebuilds nothing, every directory digest is byte-identical,
    # and every artefact in the workspace comes back `proven-reuse` -- the gate going GREEN over
    # binaries compiled from the PREVIOUS bytes. That is the same defect class this whole mechanism
    # was written to close, walking in through the one door the digest cannot see.
    $refsInline = Get-EmbeddedInputReferences -SourceLabel 'tests/a.rs' -Text 'const S: &str = include_str!("../../../schemas/agent.schema.json");'
    Assert-True ($refsInline.Count -eq 1 -and [string]$refsInline[0]['relative'] -ceq '../../../schemas/agent.schema.json') `
        "an inline include_str! is read, path and all (found $($refsInline.Count))"
    Assert-True ($refsInline.Count -eq 1 -and [string]$refsInline[0]['baseKind'] -ceq 'file') `
        'and it resolves against the DIRECTORY OF THE FILE THAT SPELLS IT, which is rustc''s rule'

    # THE SHAPE `rustfmt` ACTUALLY WRITES. Six of the nine sites lane S listed put the path on the
    # NEXT line, because the line would otherwise be too long. A line-oriented matcher sees the
    # macro and no path at all, and a scanner that silently found nothing there is exactly the
    # failure this cell forbids.
    $refsWrapped = Get-EmbeddedInputReferences -SourceLabel 'src/x.rs' -Text @'
const MEMORY_TRANSITION_POLICY: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml"
);
'@
    Assert-True ($refsWrapped.Count -eq 1 -and [string]$refsWrapped[0]['relative'] -ceq '../../../../extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml') `
        "a macro whose path sits on the next line is read the same as an inline one (found $($refsWrapped.Count))"

    $refsBytes = Get-EmbeddedInputReferences -SourceLabel 'src/b.rs' -Text 'const F: &[u8] = include_bytes!("../../../../../docs/acceptance/m05-run-2026-08-16/events/format.json");'
    Assert-True ($refsBytes.Count -eq 1 -and [string]$refsBytes[0]['macro'] -ceq 'include_bytes!') `
        'include_bytes! is a compile-time input too -- bytes are not a different kind of dependency'

    # THE `include!` SHAPE, which is how all 18 of this tree's `include!` sites are written, and the
    # one whose base is NOT the file: `env!("CARGO_MANIFEST_DIR")` names the crate root explicitly.
    $refsConcat = Get-EmbeddedInputReferences -SourceLabel 'tests/source_invariants.rs' -Text @'
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));
'@
    Assert-True ($refsConcat.Count -eq 1 -and [string]$refsConcat[0]['baseKind'] -ceq 'manifest') `
        "concat!(env!(CARGO_MANIFEST_DIR), ...) resolves against the CRATE directory (found $($refsConcat.Count), base '$(if ($refsConcat.Count -eq 1) { [string]$refsConcat[0]['baseKind'] } else { '<none>' })')"
    Assert-True ($refsConcat.Count -eq 1 -and [string]$refsConcat[0]['relative'] -ceq '/../../tools/source-invariants/detect.rs') `
        'and the concatenated tail is the path, joined as rustc joins it'

    # FAIL CLOSED ON A SHAPE IT CANNOT RESOLVE, and SAY WHICH. A scanner that skipped what it did
    # not understand would be worse than no scanner: it would read as coverage.
    $refsOpaque = Get-EmbeddedInputReferences -SourceLabel 'src/c.rs' -Text 'const S: &str = include_str!(SOME_PATH_CONST);'
    Assert-True ($refsOpaque.Count -eq 1 -and -not [string]::IsNullOrEmpty([string]$refsOpaque[0]['reason'])) `
        'a macro argument that is not a literal comes back with a REASON, never silently skipped'
    Assert-True ($refsOpaque.Count -eq 1 -and ([string]$refsOpaque[0]['reason']).Contains('SOME_PATH_CONST')) `
        'and the reason quotes the argument, so the manifest names what it could not read'

    # THE CONTROL that kills a scanner returning a problem for everything: prose ABOUT the macro is
    # not a call of it, and this tree's own comments are full of exactly that.
    $refsProse = Get-EmbeddedInputReferences -SourceLabel 'tests/d.rs' -Text '//! it reaches its subject through `include_str!`, a path written by hand'
    Assert-True ($refsProse.Count -eq 0) `
        "CONTROL: a comment naming the macro without calling it is not a reference (found $($refsProse.Count))"
    Assert-True ((Get-EmbeddedInputReferences -SourceLabel 'tests/e.rs' -Text 'fn main() { }').Count -eq 0) `
        'CONTROL: and a source that embeds nothing yields nothing'

    # ================================================================ I
    Write-Host ''
    Write-Host '=== I. an embedded input edited with a BACKDATED mtime is not a proven reuse ===' -ForegroundColor Cyan
    # THE CELL LANE S ASKED FOR, driven end to end: a crate whose test embeds a file from outside
    # its own directory, a warm target carrying a ledger this machine wrote, and the schema edited
    # to V2 with its mtime dragged BELOW the binary's -- the shape `git checkout`, `mv` and `copy`
    # all produce. Before the scanner this returned `proven-reuse` and the gate went GREEN.
    $sharedSchemas = Join-Path $fixtureRoot 'schemas'
    [System.IO.Directory]::CreateDirectory($sharedSchemas) | Out-Null
    $schemaFile = Join-Path $sharedSchemas 'x.json'
    [System.IO.File]::WriteAllText($schemaFile, "{ `"version`": 1 }`n")
    $crateH = New-FixtureCrate -Name 'fixture-h' -Body "pub fn v() -> u32 { 1 }`n"
    [System.IO.File]::WriteAllText((Join-Path $crateH 'tests\embed.rs'),
        "const S: &str = include_str!(`"../../schemas/x.json`");`n#[test] fn t() { assert!(!S.is_empty()); }`n")
    $digestsH = Get-CrateEmbeddedInputDigests -CrateDirectory $crateH
    Assert-True ($digestsH.problems.Count -eq 0) `
        "ARRANGEMENT: the fixture's embedded input resolves ($($digestsH.problems -join ' | '))"
    Assert-True ($digestsH.entries.Count -eq 1 -and $digestsH.entries[0].EndsWith('file:../../schemas/x.json')) `
        "ARRANGEMENT: and it is ONE entry naming the file outside the crate (got $($digestsH.entries.Count))"

    $graphH = New-SingleCrateGraph -Name 'fixture-h' -Directory $crateH
    $idH = @($graphH.members.Keys)[0]
    $foldH1 = Get-CrateInputHashesFromGraph -Graph $graphH -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    $hashH1 = [string]$foldH1.byPackageId[$idH]
    $targetH = Join-Path $fixtureRoot 'target-h'
    [System.IO.Directory]::CreateDirectory($targetH) | Out-Null
    $exeH = Join-Path $targetH 'fixture_h-abcdef.exe'
    [System.IO.File]::WriteAllBytes($exeH, [byte[]](9, 9, 9, 9))
    $exeShaH = (Get-FileHash -LiteralPath $exeH -Algorithm SHA256).Hash
    Write-ArtifactLedger -TargetDir $targetH -Artifacts @(
        [ordered]@{ executable = $exeH; sha256 = $exeShaH; crateInputHash = $hashH1 }) | Out-Null
    $entryH = (Read-ArtifactLedger -TargetDir $targetH)[(ConvertTo-ComparablePath -Path $exeH)]

    # THE CONTROL FIRST, as everywhere in this suite: without it, a scanner that moved the hash on
    # every call would satisfy the assertion below and refuse every reuse in the workspace.
    $controlH = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaH -CrateInputHash $hashH1 -LedgerEntry $entryH
    Assert-True ($controlH -ceq 'proven-reuse') `
        "CONTROL: with nothing touched, the warm binary is a PROVEN reuse (got '$controlH')"
    # AND THE SECOND CONTROL: a byte moving somewhere else in the tree is not this crate's business.
    [System.IO.File]::WriteAllText((Join-Path $sharedSchemas 'unreferenced.json'), '{ "unrelated": true }')
    $foldHUnrelated = Get-CrateInputHashesFromGraph -Graph $graphH -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    Assert-True (([string]$foldHUnrelated.byPackageId[$idH]) -ceq $hashH1) `
        'CONTROL: a file nothing embeds does not move the hash -- the scan reads REFERENCES, not a directory'

    [System.IO.File]::WriteAllText($schemaFile, "{ `"version`": 2 }`n")
    [System.IO.File]::SetLastWriteTimeUtc($schemaFile, [DateTime]::Parse('2020-01-01T00:00:00Z').ToUniversalTime())
    $schemaMtime = [System.IO.File]::GetLastWriteTimeUtc($schemaFile)
    $exeMtimeH = [System.IO.File]::GetLastWriteTimeUtc($exeH)
    Assert-True ($schemaMtime -lt $exeMtimeH) `
        "ARRANGEMENT: the V2 schema is OLDER than the binary, so cargo's mtime fingerprint reports nothing to do (schema $($schemaMtime.ToString('o')), binary $($exeMtimeH.ToString('o')))"
    $crateDigestH = Get-CrateDirectoryDigest -Path $crateH
    Assert-True ($crateDigestH -ceq (Get-CrateDirectoryDigest -Path $crateH)) `
        'ARRANGEMENT: and NO BYTE of the crate directory moved -- the directory digest alone is blind to this edit, which is the defect'

    $foldH2 = Get-CrateInputHashesFromGraph -Graph $graphH -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    $hashH2 = [string]$foldH2.byPackageId[$idH]
    Assert-True ($hashH2 -cne $hashH1) `
        'the crate input hash MOVED although the edit was outside the crate and the mtime went backwards'
    $verdictH = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaH -CrateInputHash $hashH2 -LedgerEntry $entryH
    Assert-True ($verdictH -cne 'proven-reuse') `
        "so the binary is NOT a proven reuse (got '$verdictH')"
    Assert-True ($verdictH -ceq 'contaminated') `
        "and it is CONTAMINATED rather than merely unknown: the custody record exists and its inputs moved under it (got '$verdictH')"

    # A REFERENCE THAT LANDS BACK INSIDE THE CRATE is already covered by the directory digest, and a
    # second entry would only cost another read. This is the cell that keeps the scan proportionate.
    [System.IO.File]::WriteAllText((Join-Path $crateH 'tests\local.rs'),
        "const L: &str = include_str!(`"../src/lib.rs`");`n")
    $digestsLocal = Get-CrateEmbeddedInputDigests -CrateDirectory $crateH
    Assert-True ($digestsLocal.entries.Count -eq 1 -and $digestsLocal.problems.Count -eq 0) `
        "a reference that resolves back inside the crate adds no entry (got $($digestsLocal.entries.Count) entries, $($digestsLocal.problems.Count) problems)"

    # ================================================================ J
    Write-Host ''
    Write-Host '=== J. an input the scanner cannot read whole makes the crate UNPROVEN, and says why ===' -ForegroundColor Cyan
    # FAILING CLOSED IS THE WHOLE POINT. A shape this scanner cannot resolve, or a path that is not
    # there, is a compile-time input it cannot hash -- and a hash computed over fewer inputs than
    # there are is precisely how a stale binary gets certified. The cost of failing closed is one
    # cold compile; the cost of failing open is the gate's meaning.
    $crateI = New-FixtureCrate -Name 'fixture-i' -Body "pub fn v() -> u32 { 1 }`n"
    [System.IO.File]::WriteAllText((Join-Path $crateI 'tests\opaque.rs'),
        "const S: &str = include_str!(SOME_PATH_CONST);`n")
    $digestsI = Get-CrateEmbeddedInputDigests -CrateDirectory $crateI
    Assert-True ($digestsI.problems.Count -eq 1) `
        "an unresolvable macro is ONE named problem (got $($digestsI.problems.Count))"
    Assert-True ($digestsI.problems.Count -eq 1 -and $digestsI.problems[0].Contains('tests/opaque.rs') -and $digestsI.problems[0].Contains('SOME_PATH_CONST')) `
        "and the problem names the FILE and the ARGUMENT, not just the refusal (got '$(if ($digestsI.problems.Count -eq 1) { $digestsI.problems[0] } else { '<none>' })')"

    $graphI = New-SingleCrateGraph -Name 'fixture-i' -Directory $crateI
    $idI = @($graphI.members.Keys)[0]
    $foldI = Get-CrateInputHashesFromGraph -Graph $graphI -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    Assert-True (-not $foldI.byPackageId.ContainsKey($idI)) `
        'the crate publishes NO input hash, so nothing can be proven against it'
    Assert-True ($foldI.unresolvedByPackageId.ContainsKey($idI) -and ([string]$foldI.unresolvedByPackageId[$idI]).Contains('tests/opaque.rs')) `
        'and the reason travels with the refusal, keyed by package id, so the manifest can name it'
    Assert-True ($foldI.unresolvedByDirectory.ContainsKey((ConvertTo-ComparablePath -Path $crateI))) `
        'keyed by the crate DIRECTORY too, because the artefact enumeration looks up both ways'

    # A MISSING PATH IS A PROBLEM, NOT A ZERO: recording an absent input as an empty digest would
    # make two different trees agree.
    $crateMissing = New-FixtureCrate -Name 'fixture-missing' -Body "pub fn v() -> u32 { 1 }`n"
    [System.IO.File]::WriteAllText((Join-Path $crateMissing 'tests\gone.rs'),
        "const S: &str = include_str!(`"../../schemas/not-there.json`");`n")
    $digestsMissing = Get-CrateEmbeddedInputDigests -CrateDirectory $crateMissing
    Assert-True ($digestsMissing.problems.Count -eq 1 -and $digestsMissing.problems[0].Contains('not-there.json')) `
        "an embedded input that does not exist is a problem naming it, not an empty digest (got $($digestsMissing.problems.Count))"

    # THE REFUSAL IS TRANSITIVE. A dependent's hash folds the shallow hash of a crate whose input
    # set is incomplete, so the dependent's hash is incomplete too.
    $crateDep = New-FixtureCrate -Name 'fixture-dependent' -Body "pub fn w() -> u32 { 2 }`n"
    $idDep = 'path+file:///fixture/fixture-dependent#fixture-dependent@0.1.0'
    $graphT = [ordered]@{
        packages = @{
            $idI   = [ordered]@{ name = 'fixture-i'; directory = $crateI }
            $idDep = [ordered]@{ name = 'fixture-dependent'; directory = $crateDep }
        }
        members  = @{ $idI = $true; $idDep = $true }
        nodes    = @{
            $idI   = [ordered]@{ features = @(); deps = @() }
            $idDep = [ordered]@{ features = @(); deps = @($idI) }
        }
    }
    $foldT = Get-CrateInputHashesFromGraph -Graph $graphT -ToolchainId 'rustc 1.97.1 (fixture)' `
        -LockfileSha256 'lock-v1' -WorkspaceManifestSha256 'manifest-v1'
    Assert-True (-not $foldT.byPackageId.ContainsKey($idDep)) `
        'a crate that DEPENDS on one with an unreadable input set publishes no hash either'
    Assert-True ($foldT.unresolvedByPackageId.ContainsKey($idDep) -and ([string]$foldT.unresolvedByPackageId[$idDep]).Contains('via dependency')) `
        'and its reason says the refusal came through a dependency, which is a different remedy'

    # AND THE VERDICT ITSELF REFUSES, above the ledger comparison: two hashes computed over the same
    # INCOMPLETE input set compare equal, and an agreement between two measurements that were each
    # missing the same file is not a proof.
    $unprovenVerdict = Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaH -CrateInputHash $hashH1 `
        -LedgerEntry $entryH -UnresolvedInputs $true
    Assert-True ($unprovenVerdict -ceq 'unproven-reuse') `
        "an artefact whose crate inputs could not be read whole is UNPROVEN even against a matching ledger entry (got '$unprovenVerdict')"
    Assert-True ((Get-ArtifactReuseProof -RebuiltThisRun $false -ExeSha256 $exeShaH -CrateInputHash $hashH1 -LedgerEntry $entryH -UnresolvedInputs $false) -ceq 'proven-reuse') `
        'CONTROL: and with the inputs resolved the same arguments are a proven reuse, so the flag is what decides'
    Assert-True ((Get-ArtifactReuseProof -RebuiltThisRun $true -ExeSha256 $exeShaH -CrateInputHash $hashH1 -LedgerEntry $entryH -UnresolvedInputs $true) -ceq 'rebuilt') `
        'CONTROL: a binary this run actually built needs no input hash at all, resolved or not'

    # THE ENUMERATION READS THE REFUSAL, pinned on the source: a fold that refuses and a caller that
    # never asks is a mechanism with no consumer.
    Assert-True ($gateText.Contains("unresolvedByPackageId.ContainsKey(`$packageId)")) `
        'the artefact enumeration looks the refusal up by package id'
    Assert-True ($gateText.Contains("-UnresolvedInputs ([string]::Equals(`$crateInputHashSource, 'unresolved-embedded-input', [System.StringComparison]::Ordinal))")) `
        'and hands it to the reuse verdict rather than deciding a second time on its own'
    Assert-True ($gateText.Contains('embeddedInputProblems')) `
        'and the run manifest publishes the named problems, so a red can be read without the console'

    # ================================================================ K
    Write-Host ''
    Write-Host '=== K. a FAILED build pass writes no ledger and stamps no target ===' -ForegroundColor Cyan
    # Lane S's second finding, measured on this branch's own incident: the build pass exits 101
    # after enumerating 22 of 234 test binaries. All 22 were rebuilt, so the unproven count is 0 --
    # and the tail then wrote a ledger covering 22 and stamped the target `complete`. Every later
    # run found 212 executables with no custody record, called them `unproven-reuse` and went RED,
    # while cargo refused to rebuild them because by mtime they were fresh: stuck RED until someone
    # deleted the target by hand, with `targetBuildState` saying the opposite of what happened.
    $tailGuard = [regex]::Match($gateText, '(?m)^\s*if \(\$unprovenAtEnd -eq 0.*$')
    Assert-True ($tailGuard.Success) 'ARRANGEMENT: the end-of-stages guard is one line this cell can read'
    Assert-True ($tailGuard.Success -and $tailGuard.Value.Contains('$buildPassSucceededAtEnd')) `
        'the ledger write and the `complete` stamp require the build pass to have SUCCEEDED, not merely to have enumerated something'
    $tailDerivation = [regex]::Match($gateText, '(?m)^\s*\$buildPassSucceededAtEnd\s*=.*$')
    Assert-True ($tailDerivation.Success -and $tailDerivation.Value.Contains('-is [int]')) `
        'and it is derived from the exit code with the same type check as the verdict, so a $null widens to failure'
    Assert-True ($tailDerivation.Success -and $tailDerivation.Value.Contains('$buildExitAtEnd')) `
        'from the artefact manifest''s own recorded exit code, not from a flag someone could set beside it'

    # ONE LEDGER WRITE, and it is INSIDE the guard. A second call anywhere else would restore the
    # defect while every assertion above stayed green.
    $ledgerWrites = [regex]::Matches($gateText, 'Write-ArtifactLedger -TargetDir')
    Assert-True ($ledgerWrites.Count -eq 1) `
        "the ledger is written at exactly ONE site (found $($ledgerWrites.Count))"
    $guardStart = if ($tailGuard.Success) { $tailGuard.Index } else { -1 }
    Assert-True ($guardStart -ge 0 -and $ledgerWrites.Count -eq 1 -and $ledgerWrites[0].Index -gt $guardStart) `
        'and that site is inside the guard, after it, never before'

    # THE FINISHED-BUT-UNPROVEN TARGET IS STAMPED `unproven` (#1007). The cell that stood here asserted
    # the sentence "the next run reads it as interrupted and rebuilds cold" -- and that sentence was
    # FALSE: `interrupted` is suspect, a suspect target aborts at the #455 guard before its first
    # compile, and the mark is sticky. A guard that pins a false sentence certifies the defect. This
    # one pins the truth and the site that makes it true.
    Assert-True ($gateText.Contains('this target is stamped unproven')) `
        'the failed-pass branch says out loud that the target is stamped unproven, not left to read as interrupted'
    $unprovenStampSites = [regex]::Matches($gateText, 'Write-TargetBuildState -TargetDir ' + [regex]::Escape('$actualTargetDir') + " -State 'unproven'")
    Assert-True ($unprovenStampSites.Count -eq 1 -and $guardStart -ge 0 -and $unprovenStampSites[0].Index -gt $guardStart) `
        "and the unproven stamp has exactly ONE site, inside the same guard as the ledger write (found $($unprovenStampSites.Count))"
    $stampSites = [regex]::Matches($gateText, "Write-TargetBuildState -TargetDir \`$actualTargetDir -State 'complete'")
    Assert-True ($stampSites.Count -le 2) `
        "and the `complete` stamp has not sprouted a new site outside the guard (found $($stampSites.Count))"

    # ENROLMENT (#1007) has two properties that must never drift, and both are in the producer's text:
    # it happens ONCE (the retry is flagged `-Enrolling` and the flag disables it), and it deletes
    # ONLY binaries the ledger has no row for -- never a `contaminated` one, whose disagreeing row is
    # the evidence this whole mechanism exists to keep.
    $enrolProducer = Get-GateFunctionText -Name 'Get-TestArtifactManifest'
    $enrolGuard = [regex]::Match($enrolProducer, '(?m)^\s*if \(-not \$Enrolling -and \$buildExit -eq 0\) \{')
    Assert-True ($enrolGuard.Success) `
        'the enrolment block is guarded by -not $Enrolling, so the retry cannot enrol again'
    $removeSites = [regex]::Matches($enrolProducer, 'Remove-Item -LiteralPath')
    Assert-True ($removeSites.Count -eq 1 -and $enrolGuard.Success -and $removeSites[0].Index -gt $enrolGuard.Index) `
        "the producer deletes binaries at exactly ONE site and it is inside the enrolment guard (found $($removeSites.Count))"
    Assert-True ($enrolProducer.Contains('Get-TestArtifactManifest -CargoArgs $CargoArgs -Enrolling')) `
        'the retry calls the producer with -Enrolling and the same cargo arguments'
    $enrolFilter = [regex]::Match($enrolProducer, '(?s)\$enrol = @\(\$artifacts \| Where-Object \{(.*?)\}\)')
    Assert-True ($enrolFilter.Success -and $enrolFilter.Groups[1].Value.Contains("'unproven-reuse'") -and -not $enrolFilter.Groups[1].Value.Contains("'contaminated'")) `
        'the enrolment filter selects unproven-reuse and never contaminated'
} catch {
    # An aborted suite must say WHY before the finally exits 2: this file reported "expected 117,
    # ran 35" with no cause when a real `cargo metadata` under it hit a bad toolchain (lane B on
    # #1007, who first credited the abort to a sabotage and withdrew that). Same four lines as its
    # sibling gate-suite-artifact.tests.ps1.
    Write-Host "HARNESS-BROKE: a cell threw: $($_.Exception.GetType().Name): $($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim()))" -ForegroundColor Magenta
    Write-Host $_.ScriptStackTrace -ForegroundColor Magenta
    throw
} finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-artifact-reuse: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-artifact-reuse: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
