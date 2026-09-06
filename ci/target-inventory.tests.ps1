# #936: isolated tests for `Get-TargetAttribution` in .factory/tools/target-inventory.ps1.
#
# Dot-sources the tool, which is why the tool now carries a dot-source guard: without one, loading
# it here would inventory the whole of D:, which is the multi-second scan the tool's own default
# exists to avoid, on the only spinning disk this machine has, while gates are building.
#
# THE SUBJECT IS THE FILE, NOT A COPY OF IT. The alternative -- re-creating the function from the
# file's AST -- tests something that merely looks like the subject, and this repository has been
# bitten by a checker comparing a message against a private copy of itself (#679).
#
# Homegrown PASS/FAIL harness with a declared expected count, matching ci/manifest-name.tests.ps1
# and ci/slot-lock.tests.ps1. The declared total is the point: a cell that stops running while its
# neighbours stay green is invisible without it, and the count has caught exactly that twice.
$ExpectedAssertionCount = 13

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

$tool = Join-Path (Split-Path -Parent $PSScriptRoot) '.factory/tools/target-inventory.ps1'
. $tool

# ARRANGEMENT, asserted rather than assumed: the dot-source guard let the definitions through and
# stopped the body. If the guard ever inverted, every cell below would still pass while the suite
# quietly spent minutes walking a disk -- so the guard is checked by the thing it enables.
Assert-True -Condition ([bool](Get-Command -Name 'Get-TargetAttribution' -ErrorAction SilentlyContinue)) `
    -Label 'ARRANGEMENT: dot-sourcing the tool defines its attribution function'

Write-Host ''
Write-Host '-- the ISSUES lanes are read as themselves, not as a lane letter --' -ForegroundColor Cyan

# THE DEFECT THIS FILE EXISTS FOR. `i3` is ISSUES 3. The single-letter arm matched the `i` and took
# the `3` as the start of a number, so the tool reported `lane I` -- a lane that appears NOWHERE in
# this repository, the only `lane I` on main being English prose in a review document. A blank is
# honest; a confident wrong owner sends whoever decides removal to ask a lane that does not exist,
# about a directory a live session owns.
Assert-Equal 'ISSUES 3 / #909' (Get-TargetAttribution -Name 'i3-909-targets') `
    'i3 is ISSUES 3, not a lane called I'
Assert-Equal 'ISSUES 1 / #119' (Get-TargetAttribution -Name 'issues1-119-targets') `
    'the issues<N> spelling reports the lane as well as the ticket'
Assert-Equal 'ISSUES 4' (Get-TargetAttribution -Name 'issues4-targets') `
    'and a lane target with no ticket still names its owner'

# NOTHING WAS TAKEN FROM A REAL LANE, and this is the cell that makes the three above a fix rather
# than a land grab. Only `i` followed IMMEDIATELY by a digit is claimed for ISSUES; `i-` with a
# separator still reaches the single-letter arm, so a lane I would be read as one if it existed.
Assert-Equal 'lane I / #909' (Get-TargetAttribution -Name 'i-909-targets') `
    'CONTROL: a hyphenated single letter is still a lane, so no lane letter was annexed'
Assert-Equal 'lane C / #753' (Get-TargetAttribution -Name 'c-753-target') `
    'CONTROL: an ordinary lane target is unchanged'
Assert-Equal 'lane M / #827' (Get-TargetAttribution -Name 'm827-target') `
    'CONTROL: and so is the letter-then-digits spelling'

Write-Host ''
Write-Host '-- a two-digit ticket is a FALLBACK, never a replacement --' -ForegroundColor Cyan

# `issues1-92-targets` is issue #92. Two digits fell below the old 3-4 floor, so the row went fully
# blank -- no lane and no ticket -- for a directory whose name carries both.
Assert-Equal 'ISSUES 1 / #92' (Get-TargetAttribution -Name 'issues1-92-targets') `
    'a two-digit issue number is read rather than dropped'

# THE PRECEDENCE, and it is what keeps this a widening. The 3-4 digit run wins wherever one exists,
# so no answer the tool already gave can change; the two-digit arm only fills blanks. Without this
# cell, a fallback that had quietly become the FIRST choice would still satisfy the assertion above
# while silently re-attributing every target on the disk.
Assert-Equal '#441' (Get-TargetAttribution -Name 'gh-review-441-target') `
    'CONTROL: a 3-digit ticket still wins, so the two-digit arm is reached only when there is none'
Assert-Equal 'lane X / #880' (Get-TargetAttribution -Name 'x12-issue880-target') `
    'CONTROL: and it wins even when a two-digit run comes FIRST in the name'

Write-Host ''
Write-Host '-- widening the reading did not turn it into guessing --' -ForegroundColor Cyan

# THE NEGATIVE. A name that genuinely carries neither a lane nor a ticket must still say so. Every
# cell above is a positive, and a function that returned an attribution for everything would pass
# all of them.
Assert-Equal 'unattributed' (Get-TargetAttribution -Name 'graphhelm-targets') `
    'a name with no lane and no ticket is still unattributed'
Assert-Equal 'unattributed' (Get-TargetAttribution -Name 'gh-targets') `
    'and so is one that is only a prefix'

# A SINGLE DIGIT IS NOT A TICKET. Lowering the floor to one would have matched half the names on
# this disk -- `c-753-target2`, `g-220b-targets` -- and turned a blank into a guess, which is the
# direction this change is explicitly not going.
Assert-Equal 'lane D' (Get-TargetAttribution -Name 'd-targets') `
    'a lane target with no digits at all reports the lane alone'

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}
if ($script:failed -gt 0) { Write-Host "FAILED: $script:failed of $script:total"; exit 1 }
Write-Host "PASSED: $script:total of $ExpectedAssertionCount"
