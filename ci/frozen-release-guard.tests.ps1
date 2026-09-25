# #194: isolated tests for ci/frozen-release-guard.ps1.
#
# The subject is a rewrite that leaves no trace. `schemas/releases/*` is a frozen copy of a schema
# that four readers pin, and the gesture that destroys it -- regenerate the schema set -- is the
# same gesture that maintains the live one. Nothing goes red today; the damage surfaces when
# somebody migrates old history against a release that no longer says what it said.
#
# The commits are INJECTED, so no case builds a repository. Two cells at the end do read the real
# tree, and they are marked, because a suite whose only input is this checkout can be observed only
# on the day it fires and this guard is meant never to fire.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   3  the frozen prefix: inside it, the live schema beside it, a look-alike sibling
#   1  a marker that begins a line and names a decision counts
#   1  ADR-nnn counts too
#   1  the same words mid-sentence do NOT
#   1  a marker naming no decision does NOT
#   1  a commit touching a frozen path without the marker is an offence, and names the path
#   1  the same commit with the marker is not
#   1  PER COMMIT: one marked commit does not license an unmarked one in the same range
#   1  a commit touching nothing frozen is never an offence
#   1  a null range is null offences -- unread is not clean
#   1  A CREATION of a new pin with no trailer is SILENT -- it rewrites nothing
#   1  and a MODIFICATION of an existing one with no trailer is still an offence
#   1  a DELETION is an offence too: removing a pin is the loss, not just changing it
#   1  AN EMPTY RANGE IS EMPTY, NOT UNREAD -- the two must not become one verdict
#   1  and zero offences over zero commits is COVERAGE ZERO, never a clean tree
#   1  A RENAME OUT of a frozen path parses to D old + A new (#978)
#   1  and it is an OFFENCE, named by the path it LEFT
#   1  a rename INTO a frozen path is exempt -- arriving somewhere empty is a first publication
#   1  a COPY out of one is not an offence: the source is still there
#   1  CONTROL: an ordinary M line still parses to exactly one change
#   2  REAL TREE: the frozen directory exists, and this branch rewrites nothing in it
$ExpectedAssertionCount = 24

$ErrorActionPreference = 'Stop'
$script:total = 0
$script:skipped = 0
$script:skipReasons = @()
# A SUM CANNOT SEE A REDISTRIBUTION BETWEEN ITS TERMS, and neither can a COUNT of skips:
# turning a real `Assert-True` into a `Skip-Assertion` leaves `total + skipped` intact and
# produces a skip count identical to a legitimate one (measured on this suite by a
# reviewing lane: 23 of 24, 1 skipped, rc=0, GREEN). What separates them is the REASON, so
# every legitimate skip site is declared here and the tail refuses anything else.
#
# WHAT THIS DOES NOT DO, so it is not read as stronger than it is: it does not catch a skip
# whose message was COPIED from a legitimate site, and it does not notice a legitimate site
# firing twice. The threat it is sized for is accidental degradation, which does not inherit
# a legal reason string; it is not a defence against someone deliberately forging one.
$AllowedSkipReasons = @(
    'no merge base against origin/main in this checkout, so the range is unknown rather than clean'
)
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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected '$Expected', got '$Actual')"
}

function Skip-Assertion {
    param([Parameter(Mandatory)] [string] $Message)
    $script:skipped++
    $script:skipReasons += $Message
    Write-Host "  SKIP: $Message" -ForegroundColor Yellow
}

# HARNESS SELF-CHECK, armed on every host: a SKIP must not move the pass counter. These suites
# were rewritten so a skipped check stops counting as a pass, and the tail guard is
# `total + skipped == expected` -- a SUM, which any redistribution between its two terms
# satisfies. This exercises the skip path directly and leaves no residue, so it fires wherever
# the suite runs rather than only on a host that happens to take the skip branch.
$__selfTotal = $script:total; $__selfSkipped = $script:skipped
Skip-Assertion 'harness self-check: a skip must not be counted as a pass'
if ($script:total -ne $__selfTotal) {
    Write-Host "HARNESS-BROKE: Skip-Assertion advanced the pass counter, so a skipped check is being reported as a pass" -ForegroundColor Red
    exit 2
}
$script:total = $__selfTotal; $script:skipped = $__selfSkipped
$script:skipReasons = @($script:skipReasons | Select-Object -First $__selfSkipped)


. (Join-Path $PSScriptRoot 'frozen-release-guard.ps1')

# `Status` defaults to 'M' because a MODIFICATION is what this guard is about; an addition is the
# case that must be spared and it is spelled out at every call site that needs it, so no cell can
# exercise the exemption by forgetting a parameter.
function New-Commit {
    param([string] $Sha, [string] $Message, [string[]] $Paths, [string] $Status = 'M')
    $changes = @($Paths | ForEach-Object { [pscustomobject]@{ Status = $Status; Path = $_ } })
    return [pscustomobject]@{ Sha = $Sha; Message = $Message; Changes = $changes }
}

# The same shape as New-Commit, but built from RAW `--name-status` lines, so a cell can exercise the
# parser and the predicate together rather than hand-writing the Changes the parser would produce.
function New-CommitFromLines {
    param([string] $Sha, [string] $Message, [string[]] $Lines)
    return [pscustomobject]@{ Sha = $Sha; Message = $Message; Changes = @(ConvertTo-CommitChanges -Lines $Lines) }
}

$marked = "fix(schema): reissue 1.0.0`n`nRewrites-Release: D-042 -- 1.0.0 reissued because the sealed key id was wrong"
$frozenPath = 'schemas/releases/1.0.0/event-envelope.schema.json'

Write-Host ''
Write-Host '-- what counts as frozen, and what only looks like it --' -ForegroundColor Cyan

Assert-True -Condition (Test-FrozenReleasePath -Path $frozenPath) `
    'a file under schemas/releases/ is frozen'
Assert-True -Condition (-not (Test-FrozenReleasePath -Path 'schemas/event-envelope.schema.json')) `
    'the LIVE schema beside it is not -- that one is what a vocabulary change is supposed to touch'
Assert-True -Condition (-not (Test-FrozenReleasePath -Path 'schemas/releases-old/1.0.0/x.json')) `
    'and a sibling directory whose name merely starts the same way is not swept in'

Write-Host ''
Write-Host '-- the marker is a trailer that names a decision, not a sentence about one --' -ForegroundColor Cyan

Assert-True -Condition (Test-ReleaseRewriteMarker -Message $marked) `
    'a trailer beginning a line and naming D-nnn counts'
Assert-True -Condition (Test-ReleaseRewriteMarker -Message "x`n  Rewrites-Release: ADR-013 reissue") `
    'ADR-nnn counts too, and leading whitespace is allowed because tools indent bodies'
Assert-True -Condition (-not (Test-ReleaseRewriteMarker -Message 'this commit does not use Rewrites-Release: D-042 anywhere meaningful')) `
    'the same words mid-sentence do NOT count: prose ABOUT the rule is not the rule being invoked'
Assert-True -Condition (-not (Test-ReleaseRewriteMarker -Message "reissue`n`nRewrites-Release: because I meant to")) `
    'and a marker naming no decision record does not count -- "I meant to" is not a reason'

Write-Host ''
Write-Host '-- the offence, and the licence --' -ForegroundColor Cyan

$unmarked = @(New-Commit -Sha 'aaaaaaaa1111' -Message 'chore(schema): regenerate the schema set' -Paths @($frozenPath, 'schemas/event-envelope.schema.json'))
$offences = Get-FrozenReleaseOffences -Commits $unmarked
Assert-True -Condition (($offences.Count -eq 1) -and ($offences[0] -like "*aaaaaaaa*$frozenPath*")) `
    'a bulk regeneration that also rewrote the frozen copy is an offence, and the message names the file'

$licensed = @(New-Commit -Sha 'bbbbbbbb2222' -Message $marked -Paths @($frozenPath))
Assert-Equal 0 (Get-FrozenReleaseOffences -Commits $licensed).Count `
    'the same rewrite with the decision named is allowed: a reissue is somebody entitled to decide it'

# THE DISCRIMINATING CELL. A guard that asked "does this RANGE carry the marker" would pass here,
# and the unmarked commit is exactly the accident this exists for -- a branch that reissues a
# release deliberately and, three commits later, regenerates it again by mistake.
$mixed = @(
    (New-Commit -Sha 'bbbbbbbb2222' -Message $marked -Paths @($frozenPath)),
    (New-Commit -Sha 'cccccccc3333' -Message 'chore(schema): regenerate' -Paths @($frozenPath))
)
$mixedOffences = Get-FrozenReleaseOffences -Commits $mixed
Assert-True -Condition (($mixedOffences.Count -eq 1) -and ($mixedOffences[0] -like 'cccccccc*')) `
    'PER COMMIT: one marked commit does not license an unmarked one in the same range'

Assert-Equal 0 (Get-FrozenReleaseOffences -Commits @(New-Commit -Sha 'dddddddd4444' -Message 'feat: unrelated' -Paths @('core/events/src/local.rs'))).Count `
    'a commit touching nothing frozen is never an offence, marker or not'

Assert-True -Condition ($null -eq (Get-FrozenReleaseOffences -Commits $null)) `
    'a range that could not be read is null, never an empty list that reads as clean'

Write-Host ''
Write-Host '-- a creation is not a rewrite, and the letter is what tells them apart --' -ForegroundColor Cyan

# #947's review: the guard read `git show --name-only`, which carries no status, so the FIRST
# publication of a new pin was an offence unless it carried a trailer. That refuses the touch where
# the doc promises to refuse the silence, and no cell saw it because none varied the change type.
$created = @(New-Commit -Sha 'eeeeeeee5555' -Message 'feat(schema): publish the 2.0.0 release' `
    -Paths @('schemas/releases/2.0.0/event-envelope.schema.json') -Status 'A')
Assert-Equal 0 (Get-FrozenReleaseOffences -Commits $created).Count `
    'CREATING a new pin with no trailer is silent: it destroys nothing, and requiring a decision for it would refuse the ordinary way a release is cut'

$modified = @(New-Commit -Sha 'ffffffff6666' -Message 'chore(schema): regenerate' `
    -Paths @($frozenPath) -Status 'M')
Assert-Equal 1 (Get-FrozenReleaseOffences -Commits $modified).Count `
    'and MODIFYING an existing one with no trailer is still an offence -- the exemption is about A, not about the directory'

# Deliberately NOT exempt. Removing a pin is a bigger loss than changing it: four readers resolve
# `schemas/releases/{from_version}/catalog.json` and a deletion takes the path away entirely.
$deleted = @(New-Commit -Sha '111111117777' -Message 'chore(schema): drop 1.0.0' `
    -Paths @($frozenPath) -Status 'D')
Assert-Equal 1 (Get-FrozenReleaseOffences -Commits $deleted).Count `
    'and DELETING one is an offence too -- the exemption is for what was not there before, not for every non-modification'

Write-Host ''
Write-Host '-- a rename is a deletion and a creation, and the letter alone could not say so (#978) --' -ForegroundColor Cyan

# The parser is driven with the exact bytes `git show --name-status` emits. Until #978 these six
# lines lived inside Get-RangeCommits, which needs a repository, so the one shape that mattered had
# no cell -- and the comment beside the code claimed a discrimination the code did not make.
$renamedOut = @(ConvertTo-CommitChanges -Lines @("R100`t$frozenPath`tdocs/old-catalog.json"))
Assert-Equal 'D|A' (($renamedOut | ForEach-Object { $_.Status }) -join '|') `
    'a rename parses to TWO changes, a deletion of the old path and a creation of the new'

Assert-True -Condition ((Get-FrozenReleaseOffences -Commits @(New-CommitFromLines -Sha 'aaaa11112222' -Message 'chore: tidy' -Lines @("R100`t$frozenPath`tdocs/old-catalog.json"))) -join ' ').Contains($frozenPath) `
    'moving a pin OUT of the frozen directory is an offence, and the message names the path it LEFT'

Assert-Equal 0 (Get-FrozenReleaseOffences -Commits @(New-CommitFromLines -Sha 'bbbb22223333' -Message 'feat: publish 2.0.0' -Lines @("R100`tdocs/draft.json`tschemas/releases/2.0.0/catalog.json"))).Count `
    'and moving one IN is not: arriving where nothing was is a first publication, the same case A is exempt for'

# A COPY leaves the source in place. Emitting a D for it would refuse a commit that took nothing
# away -- #947 was sent back for exactly that over-refusal in the other letter.
Assert-Equal 0 (Get-FrozenReleaseOffences -Commits @(New-CommitFromLines -Sha 'cccc33334444' -Message 'chore: snapshot' -Lines @("C100`t$frozenPath`tdocs/copy.json"))).Count `
    'a COPY out of a frozen path is not an offence: the pin is still where its readers look'

# CONTROL, because a parser that split everything into two would satisfy the first cell.
Assert-Equal 1 @(ConvertTo-CommitChanges -Lines @("M`t$frozenPath")).Count `
    'CONTROL: an ordinary modification still parses to exactly one change'

Write-Host ''
Write-Host '-- and the real tree, so the subject is not only a fixture --' -ForegroundColor Cyan

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Assert-True -Condition (Test-Path -LiteralPath (Join-Path $root 'schemas/releases/1.0.0')) `
    'ARRANGEMENT: the frozen release this guard protects exists in this checkout'

# ---- AN EMPTY RANGE IS NOT A CLEAN ONE. Asked on #947 by the orchestrator, measured here rather
# than answered in prose: on a gate run whose HEAD is a main commit (#752's shape, now real),
# `merge-base HEAD origin/main` IS that commit, so the range is `X..X` and this guard sees no
# commits at all. Zero offences over zero commits is not a statement about the tree.
$sameSha = (& git -C $root rev-parse HEAD 2>$null).Trim()
$emptyRange = Get-RangeCommits -Root $root -From $sameSha -To $sameSha
Assert-True -Condition (($null -ne $emptyRange) -and (@($emptyRange).Count -eq 0)) `
    'X..X is an EMPTY range, and empty is not the same value as unread -- a failed read is $null and must stay distinguishable'
Assert-Equal 0 (Get-FrozenReleaseOffences -Commits $emptyRange).Count `
    'and it yields zero offences -- which is COVERAGE ZERO, not a clean tree: nothing was examined'

# The range this branch adds. It must rewrite nothing frozen -- if it did, this guard would be
# refusing its own commit, which is the right answer and worth seeing rather than assuming.
$base = (& git -C $root merge-base HEAD origin/main 2>$null)
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($base)) {
    # No merge base is a legitimate state for a checkout with no `origin/main` -- and it is UNKNOWN,
    # so it is reported rather than counted as clean.
    Skip-Assertion 'no merge base against origin/main in this checkout, so the range is unknown rather than clean'
} else {
    $commits = Get-RangeCommits -Root $root -From $base.Trim() -To 'HEAD'
    $count = @($commits).Count
    if ($count -eq 0) {
        # THE #752 SHAPE. Reported in its own words rather than borrowing the clean one: this run
        # measured no commits, and saying "rewrites no pinned release" about it would be the exact
        # substitution -- coverage zero wearing a verdict -- that this file exists to refuse.
        Assert-True -Condition ($null -ne $commits) `
            "HEAD is at or below origin/main, so the range is EMPTY and this stage examined nothing (still a list, not a failed read)"
    } else {
        $real = Get-FrozenReleaseOffences -Commits $commits
        Assert-Equal 0 $real.Count `
            "this branch rewrites no pinned release ($($base.Trim().Substring(0,8))..HEAD, $count commit(s) examined)"
    }
}

Write-Host ''
foreach ($__reason in @($script:skipReasons)) {
    $__ok = $false
    foreach ($__allowed in $AllowedSkipReasons) { if ($__reason -like $__allowed) { $__ok = $true; break } }
    if (-not $__ok) {
        Write-Host "HARNESS-BROKE: a check was SKIPPED with an undeclared reason, which is how a real assertion gets downgraded to a skip without changing any count: $__reason" -ForegroundColor Red
        exit 2
    }
}
if (($script:total + $script:skipped) -ne $ExpectedAssertionCount) {
    Write-Host "INCOMPLETE: ran $script:total assertions and skipped $script:skipped, expected population $ExpectedAssertionCount" -ForegroundColor Yellow
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total of $ExpectedAssertionCount ($script:skipped skipped)" -ForegroundColor Green
exit 0
