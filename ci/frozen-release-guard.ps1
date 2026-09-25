<#
.SYNOPSIS
    A pinned release under `schemas/releases/` is history, and rewriting it needs saying so (#194).

.DESCRIPTION
    `schemas/releases/1.0.0/event-envelope.schema.json` is a frozen copy of a schema. A vocabulary
    change must touch the LIVE schema and never the frozen one -- but the natural gesture for "the
    schema changed" is to regenerate the schema set, and a bulk regeneration rewrites the frozen
    release silently. Nothing goes red. The damage surfaces later, when someone replays or migrates
    old history against a release that no longer says what it said.

    **THE DESTRUCTIVE GESTURE IS THE MAINTENANCE GESTURE**, which is what makes this the sharp end
    of the golden-file channel rather than an alarm.

    Four readers depend on that pin: `apps/cli/src/commands/schema/migrate.rs` builds
    `schemas/releases/{from_version}/catalog.json` to migrate FROM, `apps/cli/tests/schema_cli.rs`
    distinguishes snapshot paths from the live catalog, and
    `adapters/postgres-event-store/src/backup.rs` pins the catalog twice for backup and restore.
    Before this file, nothing made a diff under `schemas/releases/*` red: no test asserted those
    files unchanged, no gate stage checked them, and the freeze was a convention held in the heads
    of people who knew it.

    WHAT THIS REFUSES, AND WHAT IT DOES NOT. It refuses a commit that touches a frozen release
    WITHOUT saying it meant to. It does not refuse the rewrite itself: a release genuinely being
    reissued is a decision somebody is entitled to make, and a guard that could not be satisfied
    would be routed around within a week. The marker is a trailer on the commit that does it:

        Rewrites-Release: D-042 -- 1.0.0 reissued because <reason>

    The value must name a decision record (`D-nnn` or `ADR-nnn`), because "I meant to" is not a
    reason and #194 asks for an explicit reference rather than a bare acknowledgement.

    PER COMMIT, NOT PER RANGE. A branch where one commit carries the marker does not license a
    second commit that rewrote a frozen file by accident -- and the accidental one is the whole
    population this exists for. Each commit that touches a frozen path answers for itself.

    THE MARKER IS NOT PARSED OUT OF MARKDOWN, deliberately. #873 is open about
    `ci/closing-keywords.ps1` flagging a keyword inside a code span that GitHub does not link, and
    building a second markdown reader here would be a second oracle for the same unsettled question.
    Instead the marker must BEGIN a line, which is what a trailer does. The residual case -- someone
    quoting the trailer at the start of a line inside a fenced block, in the same commit that
    rewrites a frozen release -- is stated rather than handled, and it is a strictly smaller
    population than the one a markdown parser would get wrong in the other direction.
#>

[CmdletBinding()]
param(
    [string] $RepoRoot = (Get-Location).Path,
    # The range to judge. Omitted, nothing is judged and this says so -- an unexamined range and a
    # clean one are the same silence otherwise, which is the shape #194 is about one level up.
    [string] $MergeBase,
    [string] $Head = 'HEAD'
)

Set-StrictMode -Version 2.0

$script:FrozenGuardDotSourced = $MyInvocation.InvocationName -eq '.'

# The prefix is a PATH, matched with a separator, so a sibling directory named `schemas/releases-old`
# is not swept into the freeze by accident. `git diff --name-only` always prints forward slashes,
# on every platform, so only one form has to be matched here.
$script:FrozenPrefix = 'schemas/releases/'

<#
.SYNOPSIS
    Whether a repo-relative path names a frozen release file.
#>
function Test-FrozenReleasePath {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    return $Path.StartsWith($script:FrozenPrefix, [System.StringComparison]::Ordinal)
}

<#
.SYNOPSIS
    Whether a commit message carries the trailer, with a decision record named in it.
#>
function Test-ReleaseRewriteMarker {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Message)
    foreach ($line in ($Message -split "`r?`n")) {
        # BEGINS the line. Leading whitespace is allowed because commit bodies get indented by
        # tools, but the trailer may not be buried mid-sentence -- prose ABOUT the rule is not the
        # rule being invoked, and this repository's commit bodies are full of prose about rules.
        $trimmed = $line.TrimStart()
        if (-not $trimmed.StartsWith('Rewrites-Release:', [System.StringComparison]::Ordinal)) { continue }
        $value = $trimmed.Substring('Rewrites-Release:'.Length)
        if ($value -cmatch '\b(D|ADR)-\d{2,}\b') { return $true }
    }
    return $false
}

<#
.SYNOPSIS
    The commits that rewrote a frozen release without saying they meant to.

.DESCRIPTION
    `Commits` is a sequence of objects carrying `Sha`, `Message` and `Paths`. Injected rather than
    read here, so the cells can drive this without building a repository per case.
#>
function Get-FrozenReleaseOffences {
    param([AllowNull()] $Commits)

    if ($null -eq $Commits) { return $null }
    $offences = @()
    foreach ($commit in @($Commits)) {
        # A CREATION IS NOT A REWRITE, and this is the line #947's review turned the PR back for.
        # `A` is the first publication of a pin -- `schemas/releases/2.0.0/…` appearing for the
        # first time -- and it destroys nothing, so requiring a trailer for it would refuse the
        # ordinary way a release is cut. Every other letter stays an offence, and the asymmetry is
        # deliberate: `M` rewrites the bytes, `D` removes the pin four readers depend on, `R` moves
        # it out from under them, and each of those is the loss this guard exists to make loud.
        # Only `A` adds something that was not there.
        $frozen = @(@($commit.Changes) | Where-Object {
            (Test-FrozenReleasePath -Path ([string] $_.Path)) -and (([string] $_.Status) -ne 'A')
        })
        if ($frozen.Count -eq 0) { continue }
        if (Test-ReleaseRewriteMarker -Message ([string] $commit.Message)) { continue }
        $sha = [string] $commit.Sha
        $short = if ($sha.Length -ge 8) { $sha.Substring(0, 8) } else { $sha }
        $named = @($frozen | ForEach-Object { "$($_.Status) $($_.Path)" })
        $offences += "$short rewrote $($named -join ', ')"
    }
    return , ([object[]] $offences)
}

<#
.SYNOPSIS
    Read the range from git. `$null` means the range could not be read, never that it was clean.
#>
<#
.SYNOPSIS
    The commits in `From..To`, each with its message and its changes.

.DESCRIPTION
    WHAT THIS MEASURES WHEN THE SUBJECT IS `main` ITSELF (#947, asked by the orchestrator).
    Callers derive `From` as `git merge-base HEAD origin/main`. On a pull request head that is the
    branch point and the range is the branch's own commits, which is the case this guard is for. On
    a gate run whose HEAD IS a main commit -- #752's shape -- the merge base is that same commit,
    so the range is `X..X` and **this guard examines nothing**.

    That is not a hole in the merge path, and the reason is measurable: this repository
    squash-merges, and the squash CARRIES the branch's commit bodies. `de67cf46` on main holds the
    full text of its branch commit, so a `Rewrites-Release:` trailer written on a branch is present
    in main's history afterwards. The licence survives the merge; what an empty range means is that
    a main-head run RE-CHECKS nothing, not that anything went unchecked.

    An empty range returns an EMPTY LIST. A failed read returns `$null`. Those must never collapse
    into one value -- `ci/frozen-release-guard.tests.ps1` asserts both, because "zero offences over
    zero commits" and "zero offences over three commits" are the same number and not the same
    statement.
#>
<#
.SYNOPSIS
    One `git show --name-status` line per change, with the letter kept.

.DESCRIPTION
    A FUNCTION over TEXT so fixtures can drive it. Until #978 this was six lines inline inside
    `Get-RangeCommits`, which needs a repository and a commit to exercise, so the one shape that
    mattered had no cell and the comment beside it described a discrimination the code did not make.

    A RENAME IS A DELETION AND A CREATION, which is how git itself models it, and modelling it that
    way here makes both directions fall out of the existing predicate with no new branch (#978):

        git mv schemas/releases/1.0.0/x.json docs/x.json    ->  D on a frozen path   OFFENCE
        git mv docs/x.json schemas/releases/2.0.0/x.json    ->  A on a frozen path   exempt

    Reading only the LAST field, as this did, records the destination. For a move OUT of a frozen
    directory the destination is not frozen, so the change never entered the population at all and
    the pin four readers depend on could be moved away in silence.

    A COPY IS NOT A LOSS. `C100<TAB>old<TAB>new` leaves the source where it was, so only the
    destination is new and there is nothing to license. Emitting a `D` for it would refuse a commit
    that took nothing away -- the same over-refusal #947 was sent back for, in the other letter.
#>
function ConvertTo-CommitChanges {
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Lines)
    $changes = New-Object System.Collections.Generic.List[object]
    foreach ($line in @($Lines)) {
        # `A<TAB>path`; a rename or copy is `R100<TAB>old<TAB>new`.
        $fields = ([string] $line) -split "`t"
        if ($fields.Count -lt 2) { continue }
        $letter = ([string] $fields[0]).Substring(0, 1)
        $last = [string] $fields[$fields.Count - 1]
        if (($fields.Count -ge 3) -and ($letter -eq 'R')) {
            $changes.Add([pscustomobject]@{ Status = 'D'; Path = [string] $fields[1] })
            $changes.Add([pscustomobject]@{ Status = 'A'; Path = $last })
            continue
        }
        if (($fields.Count -ge 3) -and ($letter -eq 'C')) {
            $changes.Add([pscustomobject]@{ Status = 'A'; Path = $last })
            continue
        }
        $changes.Add([pscustomobject]@{ Status = $letter; Path = $last })
    }
    # `.ToArray()`, not `return , ([object[]] …)`: every caller wraps in `@()`, and under that shape
    # the comma idiom reads an EMPTY list as one row.
    return $changes.ToArray()
}

function Get-RangeCommits {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $From,
        [Parameter(Mandatory)] [string] $To
    )
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $shas = @(& git -C $Root rev-list "$From..$To" 2>$null)
        if ($LASTEXITCODE -ne 0) { return $null }
    } finally {
        $ErrorActionPreference = $previous
    }

    $commits = @()
    foreach ($sha in $shas) {
        if ([string]::IsNullOrWhiteSpace($sha)) { continue }
        $ErrorActionPreference = 'Continue'
        $message = (& git -C $Root show -s --format=%B $sha 2>$null) -join "`n"
        if ($LASTEXITCODE -ne 0) { return $null }
        # `--name-status`, not `--name-only`, and the letter is the whole point (#947's review).
        # CREATING `schemas/releases/2.0.0/…` rewrites nothing -- it is the first publication of a
        # new pin, which is the ordinary way a release is cut. Without the status letter this guard
        # refused the TOUCH where its own doc promises to refuse the SILENCE, and no cell could see
        # it because none varied the change type.
        $raw = @(& git -C $Root show --name-status --format= $sha 2>$null | Where-Object { $_ })
        if ($LASTEXITCODE -ne 0) { return $null }
        $ErrorActionPreference = $previous
        $changes = @(ConvertTo-CommitChanges -Lines $raw)
        $commits += [pscustomobject]@{ Sha = $sha; Message = $message; Changes = $changes }
    }
    return , ([object[]] $commits)
}

if (-not $script:FrozenGuardDotSourced) {
    if ([string]::IsNullOrWhiteSpace($MergeBase)) {
        Write-Host '[frozen-release] no merge base was given, so no range was examined. This is UNKNOWN, not clean.'
        exit 2
    }
    $commits = Get-RangeCommits -Root $RepoRoot -From $MergeBase -To $Head
    if ($null -eq $commits) {
        Write-Host "[frozen-release] the range $MergeBase..$Head could not be read; coverage is UNKNOWN."
        exit 2
    }
    $offences = Get-FrozenReleaseOffences -Commits $commits
    Write-Host "[frozen-release] $($commits.Count) commit(s) examined in $MergeBase..$Head."
    if ($offences.Count -gt 0) {
        Write-Host "[frozen-release] a pinned release was rewritten without saying so:"
        foreach ($line in $offences) { Write-Host "  $line" }
        Write-Host '[frozen-release] `schemas/releases/*` is history: a release copy is what old data was migrated FROM, and four readers pin it.'
        Write-Host '[frozen-release] If the rewrite is deliberate, name the decision in the commit that does it:'
        Write-Host '[frozen-release]     Rewrites-Release: D-0nn -- <version> reissued because <reason>'
        Write-Host '[frozen-release] If it is not, the usual cause is a bulk schema regeneration that touched the frozen copy as well as the live one.'
        exit 1
    }
    Write-Host '[frozen-release] no commit in this range rewrote a pinned release without naming a decision.'
    exit 0
}
