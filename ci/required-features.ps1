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

    A SCOPED RUN NEVER BUILDS AN EXCLUDED CRATE, AND THAT IS NOT #207'S FAILURE (#207 follow-up,
    the required-features stage's own first scoped run). Checking every workspace-gated target
    against a transcript that a scope selection deliberately narrowed made every scoped run that
    excludes `adapters/postgres-event-store` red, on any diff, for a crate the run never intended to
    touch -- burning the queue's single runner on a defect that is not there. `-InScopeCrates` (or
    `-Full`) narrows the CHECKED population to what this run actually selected; the DERIVED
    population from `cargo metadata` is unchanged, so an excluded target is named as excluded rather
    than silently dropped.
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
    # the gate runner's own header (retired 2026-09-24).
    [string] $TranscriptPath,
    # Package names this run actually selected (`ci/gate.ps1`'s `$script:gateScope.crates`), passed
    # rather than re-read from the scope selection file: that parsing already happened once in
    # `Read-ScopeSelection`, and this script re-deriving it from the raw JSON would be a second copy
    # of the same rule, free to drift from the first. Omitted, or `-Full`, means every crate is in
    # scope -- unchanged from before this parameter existed.
    #
    # ONE COMMA-JOINED STRING, NOT `[string[]]`. Measured invoking this script the way `ci/gate.ps1`
    # does, through `-File` across a process boundary: a `[string[]]` bound only the FIRST element
    # of a two-element array and silently dropped the rest -- no error, no warning, a population
    # this stage exists to get right that was wrong from the first real call. A single string this
    # script splits itself has one, unambiguous shape on the command line.
    [string] $InScopeCrates,
    [switch] $Full,
    # Where to write which gated targets this run excluded, as JSON, for the caller's manifest.
    # Omitted when the caller does not need the record (e.g. `-File` from a terminal).
    [string] $ScopeReportPath
)

Set-StrictMode -Version 2.0

# WHY THIS FILE RETURNS EARLY WHEN DOT-SOURCED. The suite needs the two functions without running
# anything, and a file that does its work on load cannot be tested without doing that work. Measured
# in both directions on the target-inventory tool (#940, retired 2026-09-24) after the same problem:
# a guard that silently skipped its body under `-File` would make the tool print nothing and exit 0,
# which reads exactly like a clean answer.
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

<#
.SYNOPSIS
    Which of a gated population this run's scope actually selected (#207 follow-up).

.DESCRIPTION
    `$Full`, or omitting `$InScopeCrates` altogether, checks everyone -- the behaviour before this
    function existed, unchanged. A named `$InScopeCrates` narrows `Checked` to targets whose
    PACKAGE is in that list; everything else lands in `Excluded`, named rather than dropped, so a
    caller can print or record which targets this run did not judge and why.

    Comparison is by PACKAGE, ordinal: package names in `cargo metadata` and in a scope selection's
    `crates` list are both written by tooling, never typed by a person mid-review, so there is no
    case here for a culture-aware comparer to earn its keep and every case for it to fold two
    distinct crate names together by accident.

    AN EMPTY `$InScopeCrates` IS FULL, THE SAME AS OMITTING IT -- not "select nothing". A scope
    selection whose crate list is empty already means "no Rust narrowing" one layer up
    (`Read-ScopeSelection`'s own doc), and repeating that rule here as "exclude everything" would
    invert it: the one shape this function must never produce is every gated target reported as
    excluded because the caller passed `@()` for "I did not compute a list".
#>
function Split-RequiredFeatureTargetsByScope {
    [CmdletBinding()]
    param(
        # `[AllowEmptyCollection()]`, not just `[AllowNull()]` (Codex, review of #1019): a mandatory
        # `[object[]]` parameter rejects an EMPTY array by default, distinctly from rejecting `$null`
        # -- a workspace with no `required-features`-gated targets at all makes `Get-RequiredFeatureTargets`
        # return `@()` legitimately (not an error; `$null` means unreadable, `@()` means "read, and
        # there is nothing"), and this function terminated on that real, valid input instead of
        # reporting a successful check of zero targets.
        [Parameter(Mandatory)] [AllowNull()] [AllowEmptyCollection()] [object[]] $Targets,
        [string[]] $InScopeCrates,
        [switch] $Full
    )

    $all = @($Targets)
    if ($Full -or -not $InScopeCrates -or @($InScopeCrates).Count -eq 0) {
        return [pscustomobject]@{ Checked = $all; Excluded = @() }
    }
    $inScope = [System.Collections.Generic.HashSet[string]]::new(
        [string[]] $InScopeCrates, [System.StringComparer]::Ordinal)
    $checked = @($all | Where-Object { $inScope.Contains([string] $_.Package) })
    $excluded = @($all | Where-Object { -not $inScope.Contains([string] $_.Package) })
    return [pscustomobject]@{ Checked = $checked; Excluded = $excluded }
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

    $inScopeCratesArray = @()
    if (-not [string]::IsNullOrWhiteSpace($InScopeCrates)) {
        $inScopeCratesArray = @($InScopeCrates -split ',' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    }
    $split = Split-RequiredFeatureTargetsByScope -Targets $targets -InScopeCrates $inScopeCratesArray -Full:$Full
    if ($split.Excluded.Count -gt 0) {
        $excludedNames = @($split.Excluded | ForEach-Object { "$($_.Package)/$($_.Target)" }) -join ', '
        Write-Host "[required-features] NOTE: SCOPED run, $($split.Excluded.Count) gated target(s) outside this run's selection are not checked: $excludedNames"
    }
    if ($ScopeReportPath) {
        # WRITTEN EVEN WHEN EMPTY, so a caller reading this file after a FULL run can tell "excluded:
        # none" from "this run never wrote the field" -- the same absence-is-not-zero rule the rest
        # of this file exists to enforce, applied to its own output.
        $report = [ordered]@{
            excluded = @($split.Excluded | ForEach-Object {
                    [ordered]@{ package = $_.Package; target = $_.Target; features = $_.Features }
                })
        }
        ($report | ConvertTo-Json -Depth 4) | Out-File -LiteralPath $ScopeReportPath -Encoding utf8 -Force
    }

    if (-not $TranscriptPath) { exit 0 }

    if (-not (Test-Path -LiteralPath $TranscriptPath)) {
        # The transcript is the EVIDENCE. Missing, this script knows nothing about coverage, and
        # saying so is the whole point of the exercise -- an absent transcript and a clean run are
        # the same silence otherwise.
        Write-Host "[required-features] the transcript at $TranscriptPath does not exist, so coverage is UNKNOWN."
        exit 2
    }
    $verdict = Test-RequiredFeatureTargetsRan -Targets $split.Checked `
        -Transcript ([System.IO.File]::ReadAllText($TranscriptPath))
    if (-not $verdict.Ok) {
        Write-Host "[required-features] these gated targets built nothing in this run: $($verdict.Missing -join '; ')"
        Write-Host '[required-features] a target whose features are off is never built, so it prints nothing and the run stays green. That is the state this stage exists to refuse.'
        exit 1
    }
    Write-Host "[required-features] all $($verdict.Checked) gated target(s) built and ran in this transcript."
    exit 0
}
