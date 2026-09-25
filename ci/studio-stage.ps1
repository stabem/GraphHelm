<#
.SYNOPSIS
    Runs `apps/studio`'s own test suite and build as one gate-callable unit (#464).

.DESCRIPTION
    Installs locked dependencies, runs the Studio test suite, and builds TypeScript/Vite. Stops on
    the first nonzero exit.

    NODE ABSENCE IS NOT A FAILURE. `node`/`npm` are not a declared prerequisite anywhere in this
    repository today (#464's own text), so a machine without them must not turn red for a gap the
    gate itself decided to accept. This script exits 0 and prints the sentinel line
    `[studio] NODE_ABSENT` in that case; the CALLER decides what that means for the manifest
    (`studioNodePresent`), and it must never mean "passed" -- gate.ps1 records the sentinel as a
    NOTE and does not create a stage for it, so there is no green entry a reader could mistake for
    tests having run.

    Three steps, each named on failure: `npm ci`, `npm test`, `npm run build`. The Studio build
    runs its TypeScript project build before Vite, as defined by apps/studio/package.json.
    Cache-first, explicit (item 4 of #464): `npm ci` against whatever `apps/studio/package-lock.json`
    and the local npm cache already hold. No network fetch is forced beyond what `npm ci` itself
    needs when the cache is cold; this script does not vendor `node_modules` or add an offline
    flag on its own authority, because that is a decision for whoever owns the CI machine's cache
    policy, not a script.

.PARAMETER StudioDir
    The npm project directory. Defaults to `apps/studio` under the repository root. Overridable so
    `ci/studio-stage.tests.ps1` can point this at a throwaway fixture project instead of the real
    one -- the whole reason this script is a separate file from an inline `Invoke-Stage` block.

.PARAMETER RepositoryRoot
    Used only to resolve the default `StudioDir`. Defaults to the parent of this script's own
    directory, i.e. the repository root when this file stays at `ci/studio-stage.ps1`.
#>
param(
    [string] $StudioDir,
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($StudioDir)) {
    $StudioDir = Join-Path $RepositoryRoot 'apps/studio'
}

# `Get-Command` over a manual PATH search: it already accounts for `PATHEXT` on Windows (`npm.cmd`
# vs a bare `npm`), which a hand-rolled `Test-Path` per PATH entry would have to reimplement and
# would get wrong the first time PATHEXT changed.
$nodeCommand = Get-Command -Name 'node' -ErrorAction SilentlyContinue
$npmCommand = Get-Command -Name 'npm' -ErrorAction SilentlyContinue
if ($null -eq $nodeCommand -or $null -eq $npmCommand) {
    Write-Host '[studio] NODE_ABSENT'
    exit 0
}

if (-not (Test-Path -LiteralPath $StudioDir)) {
    Write-Host "[studio] STAGE_FAILED_AT=missing-directory ($StudioDir)"
    exit 1
}

# Each step is named before it runs and again if it fails so captured output identifies the command.
$steps = @(
    @{ Name = 'ci'; Arguments = @('--prefix', $StudioDir, 'ci') }
    @{ Name = 'test'; Arguments = @('--prefix', $StudioDir, 'test') }
    @{ Name = 'build'; Arguments = @('--prefix', $StudioDir, 'run', 'build') }
)

foreach ($step in $steps) {
    Write-Host "[studio] npm $($step.Arguments -join ' ')"
    # NOT `2>&1`. Under `$ErrorActionPreference = 'Stop'` (set above), merging a native command's
    # stderr into the success stream turns EVERY stderr line into a terminating NativeCommandError
    # -- `ci/gate.ps1`'s own `Invoke-Stage` carries the identical hazard in its own comment, and
    # this script is the one place that hazard was not yet paid for. Measured: `npm warn deprecated`
    # on a perfectly successful install was enough to redden this stage before this line changed --
    # a step that WARNED and PASSED was reported as failed, and the real exit code was never read
    # because the terminating error unwound past it. stdout and stderr are left on their own
    # streams; the caller (gate.ps1's `Invoke-Stage`, or `Start-BackgroundStage`'s file redirection)
    # already captures both without this script needing to merge them itself.
    & npm @($step.Arguments)
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Host "[studio] STAGE_FAILED_AT=$($step.Name) (exit $code)"
        exit $code
    }
}

Write-Host '[studio] all steps passed'
exit 0
