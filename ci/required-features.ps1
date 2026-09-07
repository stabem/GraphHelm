<#
.SYNOPSIS
    The targets a manifest hides behind a feature, and whether a run actually built them (#207).

.DESCRIPTION
    `adapters/postgres-event-store/Cargo.toml` declares `concurrency` and `repository_conformance`
    behind `required-features = ["test-support"]`, and `test-support` is not in `default`. They run
    today only because `ci/gate.ps1` passes `--all-features` -- a flag that is there to compile
    everything, not to cover `required-features`.

    **THE FAILURE MODE IS AN ABSENCE.** A target whose features are not enabled is not skipped with
    a message; it is never built, so it contributes no per-test lines at all and the suite stays
    green. "0 tests from a target that never built" is byte-identical to "a target with nothing to
    run", and nothing in a green run distinguishes them. Narrowing the gate's feature flags for
    speed is the named trigger.

    THE LIST IS DERIVED, NEVER HAND-MAINTAINED, and #207 makes that the condition for building this
    at all. `cargo metadata` already reports every target with its `required-features`, so a target
    added tomorrow is checked tomorrow. A hardcoded list is not a weaker version of this: it fails
    in the opposite direction, staying green about targets nobody added it to.

    THE ANCHOR IS THE BINARY PATH, NOT THE TARGET NAME, and that is measured rather than preferred.
    Searching a transcript for `concurrency` is satisfied by `development_concurrency` and
    `read_concurrency` -- three targets in this workspace share that substring, and two of them have
    no required features at all, so a name search would report the guarded target as present on a
    run that never built it. Cargo prints the test binary as `…deps<sep><name>-<hash>.exe`, and
    `deps<sep><name>-` is unique to the target because the hash suffix follows the full name.

    BOTH SEPARATORS, because the transcript carries the host's. A gate on Windows prints `deps\`
    and one on Linux prints `deps/`; checking only the local one would make this pass vacuously on
    the other platform, which is the failure this file exists to refuse.
#>

[CmdletBinding()]
param(
    # A file holding the run's transcript. Given, this script CHECKS; omitted, it only lists what is
    # gated. A path rather than the text itself: a gate transcript is megabytes, and a command line
    # is not where megabytes belong.
    #
    # The caller writes it OUTSIDE the repository. A temp file inside a worktree makes
    # `dirtyDiffHash` non-null and the gate correctly refuses to commit a manifest into a tree
    # holding changes it did not make -- measured twice on 2026-09-05, by two lanes, and recorded in
    # ci/gate-runner.ps1's own header.
    [string] $TranscriptPath
)

Set-StrictMode -Version 2.0

# WHY THIS FILE RETURNS EARLY WHEN DOT-SOURCED. The suite needs the two functions without running
# anything, and a file that does its work on load cannot be tested without doing that work. Measured
# in both directions on `.factory/tools/target-inventory.ps1` (#940) after the same problem: a guard
# that silently skipped its body under `-File` would make the tool print nothing and exit 0, which
# reads exactly like a clean answer.
$script:RequiredFeaturesDotSourced = $MyInvocation.InvocationName -eq '.'

<#
.SYNOPSIS
    Every target in this workspace that a manifest gates behind a feature.

.DESCRIPTION
    Returns objects carrying `Package`, `Target`, `Kind` and `Features`. An empty result is a
    legitimate answer -- a workspace may gate nothing -- and it is NOT the same as a failed read,
    which is why `-RequireAny` exists and why the caller is told which of the two it got.
#>
function Get-RequiredFeatureTargets {
    [CmdletBinding()]
    param(
        [string] $WorkspaceRoot = (Get-Location).Path,
        [scriptblock] $MetadataSource
    )

    if (-not $MetadataSource) {
        $MetadataSource = {
            param($root)
            Push-Location -LiteralPath $root
            try {
                # `--no-deps` keeps this to the workspace's own manifests and makes the call cheap:
                # it reads Cargo.toml files and compiles nothing.
                $previous = $ErrorActionPreference
                $ErrorActionPreference = 'Continue'
                try {
                    $out = & cargo metadata --format-version 1 --no-deps 2>$null
                    $code = $LASTEXITCODE
                } finally {
                    $ErrorActionPreference = $previous
                }
                if ($code -ne 0) { return $null }
                return ($out -join "`n")
            } finally {
                Pop-Location
            }
        }
    }

    $json = & $MetadataSource $WorkspaceRoot
    if ([string]::IsNullOrWhiteSpace($json)) { return $null }
    $metadata = $null
    try { $metadata = $json | ConvertFrom-Json } catch { return $null }
    if (-not $metadata) { return $null }
    if (-not ($metadata | Get-Member -Name 'packages' -MemberType NoteProperty)) { return $null }

    $found = @()
    foreach ($package in @($metadata.packages)) {
        foreach ($target in @($package.targets)) {
            $features = @()
            if ($target | Get-Member -Name 'required-features' -MemberType NoteProperty) {
                $features = @($target.'required-features')
            }
            if ($features.Count -eq 0) { continue }
            $found += [pscustomobject]@{
                Package  = [string] $package.name
                Target   = [string] $target.name
                Kind     = (@($target.kind) -join ',')
                Features = ($features -join ',')
            }
        }
    }
    # An ARRAY always, even for one row: PowerShell unwraps a single-element array on return, and a
    # caller doing `.Count` on the bare object would read 1 for a string of length 1.
    return , ([object[]] $found)
}

<#
.SYNOPSIS
    Which of those targets a transcript does NOT show a binary for.

.DESCRIPTION
    `Ok` is false when any target is missing OR when the target list itself could not be read, and
    those are different states carried in `Reason`: `unread` cannot be answered and must never be
    reported as `all ran`. The two are the same absence in the transcript and must not be the same
    verdict -- the whole subject of #207 is an absence that reads as success.
#>
function Test-RequiredFeatureTargetsRan {
    [CmdletBinding()]
    param(
        [AllowNull()] $Targets,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $Transcript
    )

    if ($null -eq $Targets) {
        return [pscustomobject]@{
            Ok = $false; Reason = 'unread'; Missing = @(); Checked = 0
        }
    }

    $rows = @($Targets)
    $missing = @()
    foreach ($row in $rows) {
        $name = [string] $row.Target
        if ([string]::IsNullOrWhiteSpace($name)) { continue }
        $windows = 'deps\' + $name + '-'
        $unix = 'deps/' + $name + '-'
        if (($Transcript.IndexOf($windows, [System.StringComparison]::Ordinal) -lt 0) -and
            ($Transcript.IndexOf($unix, [System.StringComparison]::Ordinal) -lt 0)) {
            $missing += "$($row.Package)/$($row.Target) (required-features: $($row.Features))"
        }
    }

    if ($missing.Count -gt 0) {
        return [pscustomobject]@{
            Ok = $false; Reason = 'missing'; Missing = $missing; Checked = $rows.Count
        }
    }
    return [pscustomobject]@{
        Ok = $true; Reason = 'ran'; Missing = @(); Checked = $rows.Count
    }
}

if (-not $script:RequiredFeaturesDotSourced) {
    $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $targets = Get-RequiredFeatureTargets -WorkspaceRoot $root
    if ($null -eq $targets) {
        Write-Host '[required-features] cargo metadata could not be read; the population is UNKNOWN, not empty.'
        exit 2
    }
    Write-Host "[required-features] $($targets.Count) target(s) gated behind a feature:"
    foreach ($row in $targets) {
        Write-Host "  $($row.Package)/$($row.Target)  [$($row.Kind)]  required-features: $($row.Features)"
    }
    if (-not $TranscriptPath) { exit 0 }

    if (-not (Test-Path -LiteralPath $TranscriptPath)) {
        # The transcript is the EVIDENCE. Missing, this script knows nothing about coverage, and
        # saying so is the whole point of the exercise -- an absent transcript and a clean run are
        # the same silence otherwise.
        Write-Host "[required-features] the transcript at $TranscriptPath does not exist, so coverage is UNKNOWN."
        exit 2
    }
    $verdict = Test-RequiredFeatureTargetsRan -Targets $targets `
        -Transcript ([System.IO.File]::ReadAllText($TranscriptPath))
    if (-not $verdict.Ok) {
        Write-Host "[required-features] these gated targets built nothing in this run: $($verdict.Missing -join '; ')"
        Write-Host '[required-features] a target whose features are off is never built, so it prints nothing and the run stays green. That is the state this stage exists to refuse.'
        exit 1
    }
    Write-Host "[required-features] all $($verdict.Checked) gated target(s) built and ran in this transcript."
    exit 0
}
