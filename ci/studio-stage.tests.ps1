# #464: isolated tests for ci/studio-stage.ps1 and the scope gate that decides whether it runs.
#
# Two subjects, two techniques. `Test-StudioScopeChanged` is a pure function inside `ci/gate.ps1`
# and is extracted by text the same way `ci/gate-canary-outcome.tests.ps1` extracts
# `Get-CanaryOutcome` -- dot-sourcing the whole file would run the gate. `ci/studio-stage.ps1` is
# its own file with its own CLI surface (`-StudioDir`), so it is spawned as a real process against
# throwaway fixture projects, the same way `ci/gate-target-dir.tests.ps1` spawns the real gate for
# its door cell. Homegrown PASS/FAIL harness, not Pester -- see ci/slot-lock.tests.ps1 for why.
#
# Criterion this suite exists to meet (#464 item 3): prove the Studio stage can actually go RED,
# not merely that it runs. Cells 11-13 are that proof: a fixture whose `test` script exits 1 turns
# ci/studio-stage.ps1 red, names which step failed, and the step after it never runs.
$ExpectedAssertionCount = 32
# The node-dependent cells are counted SEPARATELY from the total on purpose: see the tail.
$ExpectedNodeDependentCount = 13

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0
$script:skipped = 0
$script:nodeDependent = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$scriptDir = $PSScriptRoot
$gatePath = Join-Path $scriptDir 'gate.ps1'
$stagePath = Join-Path $scriptDir 'studio-stage.ps1'

# ==== Part 1: Test-StudioScopeChanged, extracted from gate.ps1 by text =======================

$gateText = [System.IO.File]::ReadAllText($gatePath)
$parseErrors = $null
[System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors) | Out-Null
Assert-True ($parseErrors.Count -eq 0) 'ARRANGEMENT: gate.ps1 parses, so what follows is measured rather than empty'

function Get-Slice {
    param([string] $From, [string] $To)
    $a = $gateText.IndexOf($From, [System.StringComparison]::Ordinal)
    $b = $gateText.IndexOf($To, [System.StringComparison]::Ordinal)
    if ($a -lt 0 -or $b -le $a) { throw "could not slice $From .. $To out of gate.ps1" }
    return $gateText.Substring($a, $b - $a)
}

$scopeFn = Get-Slice -From 'function Test-StudioScopeChanged {' -To 'function Read-ScopeSelection {'
Assert-True ($scopeFn.Length -gt 0) 'ARRANGEMENT: Test-StudioScopeChanged was extracted, so the subject exists'
. ([scriptblock]::Create($scopeFn))

Assert-True (Test-StudioScopeChanged -Path '') 'no path given widens to true (matches Read-ScopeSelection default: absent means FULL)'
Assert-True (Test-StudioScopeChanged -Path (Join-Path $env:TEMP "studio-scope-nonexistent-$([guid]::NewGuid().ToString('N'))")) 'a path that does not exist widens to true'

$tempDir = Join-Path $env:TEMP "studio-scope-tests-$([guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
try {
    $unparseable = Join-Path $tempDir 'unparseable.json'
    Set-Content -LiteralPath $unparseable -Value 'not json {{{' -Encoding utf8
    Assert-True (Test-StudioScopeChanged -Path $unparseable) 'a file that does not parse as JSON widens to true'

    $noChangedFiles = Join-Path $tempDir 'no-changed-files.json'
    Set-Content -LiteralPath $noChangedFiles -Value '{"escalated": false}' -Encoding utf8
    Assert-True (Test-StudioScopeChanged -Path $noChangedFiles) 'a selection with no changedFiles key widens to true'

    $escalated = Join-Path $tempDir 'escalated.json'
    Set-Content -LiteralPath $escalated -Value '{"escalated": true, "changedFiles": ["core/foo.rs"]}' -Encoding utf8
    Assert-True (Test-StudioScopeChanged -Path $escalated) 'an escalated selection widens to true regardless of changedFiles content'

    $studioChanged = Join-Path $tempDir 'studio-changed.json'
    Set-Content -LiteralPath $studioChanged -Value '{"escalated": false, "changedFiles": ["apps/studio/src/App.tsx", "core/schema/src/lib.rs"]}' -Encoding utf8
    Assert-True (Test-StudioScopeChanged -Path $studioChanged) 'a changedFiles list containing an apps/studio/ path returns true'

    $rustOnly = Join-Path $tempDir 'rust-only.json'
    Set-Content -LiteralPath $rustOnly -Value '{"escalated": false, "changedFiles": ["core/schema/src/lib.rs", "adapters/tool-host/src/process.rs"]}' -Encoding utf8
    Assert-True (-not (Test-StudioScopeChanged -Path $rustOnly)) 'a changedFiles list with no apps/studio/ path returns false: a Rust-only PR never pays npm'

    $empty = Join-Path $tempDir 'empty.json'
    Set-Content -LiteralPath $empty -Value '{"escalated": false, "changedFiles": []}' -Encoding utf8
    Assert-True (-not (Test-StudioScopeChanged -Path $empty)) 'an empty, non-escalated changedFiles list returns false'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $tempDir -ErrorAction SilentlyContinue
}

# ==== Part 2: ci/studio-stage.ps1, spawned as a real process ================================

# GUARDED, NOT SKIPPED SILENTLY (Codex P1, review of #1003). `studio-stage.ps1`'s NODE_ABSENT check
# runs before it ever looks at `-StudioDir`, so on a machine with no real node/npm the green,
# red-fixture and missing-directory cells below all take that exit-0 path regardless of what their
# fixtures say -- `all steps passed` never prints, `STAGE_FAILED_AT=test` never prints, and every
# assertion that expects one of those strings fails, on a host this stage itself is documented to
# accept (`ci/studio-stage.ps1`'s own header: "node/npm are not a declared prerequisite anywhere in
# this repository today"). A SUITE THAT REQUIRES A DEPENDENCY THE STAGE DECLARES OPTIONAL is red on
# exactly the environment the stage exists to tolerate. Each guarded cell still runs -- as a named
# SKIP that passes for a stated reason -- so `$ExpectedAssertionCount` stays one fixed number on
# every host, and a cell that silently stopped running is still caught.
$script:nodeAvailable = [bool](Get-Command -Name 'node' -ErrorAction SilentlyContinue) -and
[bool](Get-Command -Name 'npm' -ErrorAction SilentlyContinue)
function Assert-NodeDependent {
    param([Parameter(Mandatory)] [scriptblock] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:nodeDependent++
    if ($script:nodeAvailable) {
        Assert-True (& $Condition) $Message
    } else {
        $script:skipped++
        Write-Host "  SKIP: node/npm not on this host, which ci/studio-stage.ps1 treats as optional: $Message" -ForegroundColor Yellow
    }
}

# HARNESS SELF-CHECK, armed on every host INCLUDING one that has node. `Assert-NodeDependent`
# takes its skip branch only when node is absent, so on the gate host that branch never executes
# and a regression folding skips back into passes would be invisible there. Forcing the flag
# exercises the branch directly. Leaves no residue.
$__selfTotal = $script:total; $__selfSkipped = $script:skipped; $__selfNode = $script:nodeAvailable
$script:nodeAvailable = $false
Assert-NodeDependent { $true } 'harness self-check: a skip must not be counted as a pass'
$script:nodeAvailable = $__selfNode
if ($script:total -ne $__selfTotal) {
    Write-Host "HARNESS-BROKE: Assert-NodeDependent advanced the pass counter on its skip branch, so a skipped check is being reported as a pass" -ForegroundColor Red
    exit 2
}
$script:total = $__selfTotal; $script:skipped = $__selfSkipped; $script:nodeDependent--


function New-StudioFixture {
    param([string] $TestExitCode = '0', [string] $BuildExitCode = '0')
    $dir = Join-Path $env:TEMP "studio-stage-fixture-$([guid]::NewGuid().ToString('N').Substring(0,8))"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $pkg = @{
        name    = 'fixture'
        version = '1.0.0'
        scripts = @{
            # The real Studio package folds tsc into build, so a passing stage proves it does not
            # invoke this standalone script.
            typecheck = 'node -e "process.exit(9)"'
            test      = "node -e `"process.exit($TestExitCode)`""
            build     = "node -e `"process.exit($BuildExitCode)`""
        }
    }
    ($pkg | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath (Join-Path $dir 'package.json') -Encoding utf8
    $lock = @{
        name            = 'fixture'
        version         = '1.0.0'
        lockfileVersion = 3
        requires        = $true
        packages        = @{ '' = @{ name = 'fixture'; version = '1.0.0' } }
    }
    ($lock | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath (Join-Path $dir 'package-lock.json') -Encoding utf8
    return $dir
}

function Invoke-StudioStage {
    param([string] $StudioDir, [hashtable] $EnvironmentOverride)
    $psExe = (Get-Command powershell.exe).Source
    $originalPath = $env:PATH
    # NOT `2>&1` on the spawn below -- this harness has the SAME hazard M found in the script it
    # tests: under `$ErrorActionPreference = 'Stop'` (this file's own top), redirecting the child's
    # stderr into the success stream turns any stderr line into a terminating NativeCommandError.
    # Measured live while adding the stderr fixtures below: this exact line raised on
    # `npm warn deprecated something` before it was changed, which would have failed the harness
    # rather than the subject it was trying to observe. `Continue`, scoped to just this call, is
    # `Invoke-Stage`'s own established remedy in `ci/gate.ps1` for the identical situation.
    $previousPreference = $ErrorActionPreference
    try {
        if ($EnvironmentOverride -and $EnvironmentOverride.ContainsKey('PATH')) {
            $env:PATH = $EnvironmentOverride['PATH']
        }
        $ErrorActionPreference = 'Continue'
        $output = & $psExe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $stagePath -StudioDir $StudioDir 2>&1
        $code = $LASTEXITCODE
    } finally {
        $env:PATH = $originalPath
        $ErrorActionPreference = $previousPreference
    }
    return [pscustomobject]@{ Code = $code; Output = ($output -join "`n") }
}

$greenFixture = New-StudioFixture -TestExitCode '0'
try {
    $result = Invoke-StudioStage -StudioDir $greenFixture
    Assert-NodeDependent { $result.Code -eq 0 } "a fixture whose steps all pass exits 0 (got $($result.Code))"
    Assert-NodeDependent { $result.Output -like '*all steps passed*' } 'a fully-green run prints the all-steps-passed line'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $greenFixture -ErrorAction SilentlyContinue
}

# THE CELL #464 EXISTS FOR: a fixture whose `test` step fails must turn this stage red, name the
# step, and never run the step after it.
$redFixture = New-StudioFixture -TestExitCode '1'
try {
    $result = Invoke-StudioStage -StudioDir $redFixture
    Assert-NodeDependent { $result.Code -ne 0 } "a fixture whose test step fails exits non-zero (got $($result.Code))"
    Assert-NodeDependent { $result.Output -like '*STAGE_FAILED_AT=test*' } 'the failure names the step that failed (test)'
    Assert-NodeDependent { $result.Output -notlike '*run build*' } 'the build step never ran after test failed -- the sequence stops at the first failure'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $redFixture -ErrorAction SilentlyContinue
}

$buildRedFixture = New-StudioFixture -BuildExitCode '7'
try {
    $result = Invoke-StudioStage -StudioDir $buildRedFixture
    Assert-NodeDependent { $result.Code -eq 7 } "a build failure exits with its real code (got $($result.Code))"
    Assert-NodeDependent { $result.Output -like '*STAGE_FAILED_AT=build*' } 'the build failure names the build step'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $buildRedFixture -ErrorAction SilentlyContinue
}

# Node absence must read as a note, never as a failure and never as a pass that ran nothing. NOT
# guarded by $script:nodeAvailable: this cell forces the CHILD's own PATH to exclude node/npm
# regardless of what the host running this suite has, so it exercises the NODE_ABSENT path either
# way and needs no host-dependent skip.
$noNodeResult = Invoke-StudioStage -StudioDir 'C:\does-not-matter' -EnvironmentOverride @{ PATH = 'C:\Windows\System32;C:\Windows' }
Assert-True ($noNodeResult.Code -eq 0) "with node/npm absent from PATH, the script exits 0, not a failure (got $($noNodeResult.Code))"
Assert-True ($noNodeResult.Output -like '*NODE_ABSENT*') 'with node/npm absent, the sentinel line is printed so the caller can tell absence from a pass'

$missingDir = Join-Path $env:TEMP "studio-stage-missing-$([guid]::NewGuid().ToString('N'))"
$missingResult = Invoke-StudioStage -StudioDir $missingDir
Assert-NodeDependent { $missingResult.Code -ne 0 } "a StudioDir that does not exist (Node present) exits non-zero (got $($missingResult.Code))"
Assert-NodeDependent { $missingResult.Output -like '*STAGE_FAILED_AT=missing-directory*' } 'a missing StudioDir names itself, not a generic npm error'

# THE CELLS M'S REVIEW OF #1003 EXISTS FOR. Under `$ErrorActionPreference = 'Stop'` (set at this
# script's top), merging a native command's stderr into the success stream via `2>&1` turns EVERY
# stderr line into a terminating NativeCommandError -- measured against real `npm`, where
# `npm warn deprecated` on a perfectly successful install was enough to redden a step that actually
# passed, and to lose the true exit code of a step that actually failed (`ci/gate.ps1`'s own
# `Invoke-Stage` names the identical hazard in its own comment). Neither of the two fixtures above
# writes to stderr, so neither could have caught this -- these two are built specifically so one
# does and the assertions are about what a bare `2>&1` would have done to each.
function New-StudioStderrFixture {
    param([string] $BuildExitCode = '0')
    $dir = Join-Path $env:TEMP "studio-stage-stderr-fixture-$([guid]::NewGuid().ToString('N').Substring(0,8))"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $pkg = @{
        name    = 'fixture'
        version = '1.0.0'
        scripts = @{
            # Writes to stderr AND signals its own real outcome via its exit code -- the two must
            # be read independently, which is exactly what a `2>&1` merge stops being true.
            typecheck = 'node -e "process.exit(9)"'
            test      = 'node -e "process.exit(0)"'
            build     = "node -e `"console.error('npm warn deprecated something'); process.exit($BuildExitCode)`""
        }
    }
    ($pkg | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath (Join-Path $dir 'package.json') -Encoding utf8
    $lock = @{
        name = 'fixture'; version = '1.0.0'; lockfileVersion = 3; requires = $true
        packages = @{ '' = @{ name = 'fixture'; version = '1.0.0' } }
    }
    ($lock | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath (Join-Path $dir 'package-lock.json') -Encoding utf8
    return $dir
}

# A step that WARNS (writes to stderr) and PASSES (exits 0) must still pass. This is the exact
# shape `npm warn deprecated` has on a real, healthy install -- if this cell is red, every real
# gate run against apps/studio would be too.
$stderrGreenFixture = New-StudioStderrFixture -BuildExitCode '0'
try {
    $result = Invoke-StudioStage -StudioDir $stderrGreenFixture
    Assert-NodeDependent { $result.Code -eq 0 } "a step that writes to stderr but exits 0 must still pass the stage (got $($result.Code))"
    Assert-NodeDependent { $result.Output -like '*all steps passed*' } 'a stderr warning on an otherwise-successful run does not stop the remaining steps from running'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $stderrGreenFixture -ErrorAction SilentlyContinue
}

# A step that WARNS (writes to stderr) and genuinely FAILS (a specific non-zero exit code) must
# report THAT code, named at the step that produced it -- not a code mangled by an intervening
# terminating error, and not silence about which step it was.
$stderrRedFixture = New-StudioStderrFixture -BuildExitCode '3'
try {
    $result = Invoke-StudioStage -StudioDir $stderrRedFixture
    Assert-NodeDependent { $result.Code -eq 3 } "a build step that writes to stderr and exits 3 must report exit 3, not a code an intervening error substituted (got $($result.Code))"
    Assert-NodeDependent { $result.Output -like '*STAGE_FAILED_AT=build*' } 'the failing build step is still named when it also wrote to stderr'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $stderrRedFixture -ErrorAction SilentlyContinue
}

# ==== Part 3: source contract -- gate.ps1 actually wires the stage in, not only defines it ===

Assert-True ($gateText -like "*Invoke-Stage 'apps/studio (npm)'*") 'gate.ps1 has an Invoke-Stage entry for the Studio suite'
Assert-True ($gateText -like '*studioNodePresent*') 'the manifest schema carries studioNodePresent'
Assert-True ($gateText -like '*studioScopeIncluded*') 'the manifest schema carries studioScopeIncluded'
Assert-True (
    $gateText.IndexOf('if ($script:studioScopeIncluded -and $script:studioNodePresent) {', [System.StringComparison]::Ordinal) -gt 0
) 'the stage entry is conditioned on scope AND node together, never created unconditionally'

# #1102: a single reused Vitest thread still missed its fixed 60-second startup wait while the
# early Studio child competed with Rust and both PostgreSQL matrices. The gate must keep the same
# fail-closed Studio stage, but start it only after the last competing matrix has been joined.
$studioStageIndex = $gateText.LastIndexOf("Invoke-Stage 'apps/studio (npm)'", [System.StringComparison]::Ordinal)
$lastPostgresJoinIndex = $gateText.LastIndexOf(
    "Complete-PostgresStage -Name 'PostgreSQL matrix under a non-C collation'",
    [System.StringComparison]::Ordinal
)
Assert-True ($studioStageIndex -gt $lastPostgresJoinIndex) `
    'the Studio stage starts after both PostgreSQL matrices have been joined, outside peak gate load'
Assert-True (-not $gateText.Contains("Start-BackgroundStage -Name 'apps/studio (npm)'")) `
    'the Studio stage has no early background start that can race the loaded gate host'

# RESTORED. This diff deleted the line below on the grounds that the behaviour fixtures cover it.
# They do not: re-adding `2>&1` to the npm call in ci/studio-stage.ps1 leaves 31/31 green and
# rc=0, because the fixtures reach the step through `npm run`, which does not propagate the
# child's stderr in the shape that trips NativeCommandError. Measured by sabotage before this
# line went back. The #1003 hazard is a SOURCE SPELLING, and a source spelling needs a witness
# that reads the source.
$stageText = [System.IO.File]::ReadAllText($stagePath)
Assert-True ($stageText -notlike '*npm @($step.Arguments) 2>&1*') 'the npm call does not merge stderr into the success stream (the #1003 hazard)'

Write-Host ''
Write-Host "$($script:total - $script:failures)/$($script:total) passed; $($script:skipped) skipped"
# A SUM CANNOT SEE A REDISTRIBUTION BETWEEN ITS TERMS. `$total + $skipped` is satisfied by
# folding every skip back into the passes, which is precisely the change this suite exists to
# forbid -- and the split was reported only by an unasserted Write-Host. These two checks read
# the terms.
if (($script:total + $script:skipped) -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount checks, ran $($script:total) assertions and skipped $($script:skipped)" -ForegroundColor Red
    exit 2
}
if ($script:nodeDependent -ne $ExpectedNodeDependentCount) {
    Write-Host "HARNESS-BROKE: expected $ExpectedNodeDependentCount node-dependent cells, reached $($script:nodeDependent)" -ForegroundColor Red
    exit 2
}
$expectedSkipped = if ($script:nodeAvailable) { 0 } else { $ExpectedNodeDependentCount }
if ($script:skipped -ne $expectedSkipped) {
    Write-Host "HARNESS-BROKE: node/npm $(if ($script:nodeAvailable) { 'present' } else { 'absent' }) on this host, so exactly $expectedSkipped cells must be SKIPPED, but $($script:skipped) were -- a skip counted as a pass is the defect this suite forbids" -ForegroundColor Red
    exit 2
}
if ($script:failures -gt 0) { exit 1 }
exit 0
