# #753: the sweep is an instrument, so it gets a suite of its own.
#
# It was born inside ci/classify-run.tests.ps1 as a few lines of AST walking, and the reviewer
# condition that made it a committed tool -- "a reviewer has to be able to re-run it" -- is the same
# reason it needs cells that are about IT rather than about the file it was first pointed at.
#
# EVERY DETECTOR HERE IS PROVED AGAINST A SYNTHETIC POSITIVE. There is no natural corpus for some of
# these: `switch` clauses exist in nine places today and existed in NONE when the widening was first
# proposed, so a sweep reporting zero would have been indistinguishable from a sweep that cannot
# see them. A fixture written for the purpose is the only control that survives the corpus moving.
#
# And every detector has its NEGATIVE control beside it, because the expensive failure for a tool
# like this is not a miss -- it is a false positive, which is how a sweep gets deleted three months
# later by somebody who stopped believing it.
$ExpectedAssertionCount = 49

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

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$toolPath = Join-Path $scriptDir 'find-culture-comparisons.ps1'
if (-not (Test-Path -LiteralPath $toolPath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $toolPath" -ForegroundColor Magenta
    exit 2
}
# Dot-sourced, so the cells call the SAME function an operator runs. A copy of the logic here would
# be a suite proving the copy.
. $toolPath

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("sweep-tests-" + [Guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($sandbox) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function New-Fixture {
    param([Parameter(Mandatory)] [string] $Name, [Parameter(Mandatory)] [string] $Body)
    $path = Join-Path $sandbox $Name
    [System.IO.File]::WriteAllText($path, $Body, $utf8NoBom)
    return $path
}

function Get-Kinds {
    param([Parameter(Mandatory)] [string] $File, [string] $Include = 'all')
    return @(Find-CultureComparison -File $File -Include $Include)
}

try {
    Write-Host "`n-- the premise the detectors rest on, measured here rather than quoted --"

    # THE TOOL'S DESIGN RESTS ON WHICH FORMS FOLD AN IGNORABLE CODE POINT. That belongs in a cell:
    # a comment saying `.Equals` is ordinal is a claim, and this file is the only place the claim can
    # be checked against the language the gate actually runs on.
    $w = [string][char]0xFE00
    $v = 'GREEN' + $w
    Assert-True -Condition ($(switch ($v) { 'GREEN' { $true } default { $false } })) `
        -Message 'a switch clause matches a value carrying an ignorable code point'
    $table = @{ GREEN = 1 }
    Assert-True -Condition ($null -ne $table[$v]) `
        -Message 'and a hashtable lookup by that value hits the key that does not contain it'
    Assert-True -Condition (('GREEN').StartsWith($v) -and ('GREEN').EndsWith($v) -and (('GREEN').IndexOf($v) -eq 0)) `
        -Message 'and StartsWith, EndsWith and IndexOf all say yes to it'
    # The negative half, which is what keeps the detector list short.
    Assert-True -Condition ((-not ('GREEN').Equals($v)) -and (-not ('GREEN').Contains($v)) -and
        (('GREEN').Replace($v, 'X') -ceq 'GREEN')) `
        -Message 'while Equals, Contains and Replace are ordinal and do NOT'
    # The control, so the four above are not passing because everything says yes.
    Assert-True -Condition ((-not ('GREEN').StartsWith('GREENX')) -and (('GREEN').IndexOf('GREENX') -eq -1) -and
        (-not ('GREEN').Contains('GREENX'))) `
        -Message 'CONTROL: a visible difference is refused by all of them'

    Write-Host "`n-- the binary detector, and what it leaves alone --"

    $binary = New-Fixture -Name 'binary.ps1' -Body @'
if ($a -eq 'green') { 1 }
if ($null -eq $b) { 2 }
if ($c -eq 3) { 3 }
if ($d -eq $true) { 4 }
'@
    $rows = Get-Kinds -File $binary
    Assert-True -Condition (@($rows | Where-Object { $_.Kind -ceq 'binary' }).Count -eq 1) `
        -Message "one binary comparison is found (got $(@($rows | Where-Object { $_.Kind -ceq 'binary' }).Count))"
    Assert-True -Condition (@($rows).Count -eq 1) `
        -Message 'and the null, numeric and boolean comparisons beside it are not listed'

    Write-Host "`n-- the switch detector, proved against a synthetic positive --"

    $switchFixture = New-Fixture -Name 'switchy.ps1' -Body @'
switch ($status) {
    'GREEN' { 1 }
    'RED' { 2 }
    default { 3 }
}
switch ($n) {
    1 { 'one' }
    default { 'other' }
}
'@
    $rows = Get-Kinds -File $switchFixture
    Assert-True -Condition (@($rows | Where-Object { $_.Kind -ceq 'switch' }).Count -eq 2) `
        -Message "both string clauses are found (got $(@($rows | Where-Object { $_.Kind -ceq 'switch' }).Count))"
    Assert-True -Condition (@($rows | Where-Object { $_.Text -cmatch '^1$' }).Count -eq 0) `
        -Message 'and a numeric clause is not one of them'

    Write-Host "`n-- the index detector lists variable keys and leaves literals alone --"

    # A LITERAL KEY CANNOT CARRY A SMUGGLED CODE POINT: it is written by the author, in the file the
    # reader is looking at. A VARIABLE key is whatever arrived. Listing both would bury the second in
    # the first -- there are three literals and sixty-eight variables in this repository today.
    $indexFixture = New-Fixture -Name 'indexy.ps1' -Body @'
$byVariable = $table[$key]
$byLiteral = $table['status']
$byNumber = $array[0]
'@
    $rows = Get-Kinds -File $indexFixture
    Assert-True -Condition (@($rows | Where-Object { $_.Kind -ceq 'index' }).Count -eq 1) `
        -Message "the variable key is listed (got $(@($rows | Where-Object { $_.Kind -ceq 'index' }).Count))"
    Assert-True -Condition (@($rows | Where-Object { $_.Text -cmatch "'status'" }).Count -eq 0) `
        -Message 'and the literal key is not'
    Assert-True -Condition (@($rows | Where-Object { $_.Text -cmatch '\[0\]' }).Count -eq 0) `
        -Message 'and an array subscript is not a comparison at all'

    Write-Host "`n-- the member detector, with the ordinal methods deliberately absent --"

    $memberFixture = New-Fixture -Name 'membery.ps1' -Body @'
$a.StartsWith('x')
$b.EndsWith('y')
$c.IndexOf('z')
$d.LastIndexOf('w')
$e.CompareTo('v')
$f.StartsWith('x', [System.StringComparison]::Ordinal)
$g.Equals('x')
$h.Contains('x')
$i.Replace('x', 'y')
'@
    $rows = @(Get-Kinds -File $memberFixture | Where-Object { $_.Kind -ceq 'member' })
    Assert-True -Condition (@($rows).Count -eq 5) `
        -Message "the five culture-sensitive calls are found (got $(@($rows).Count))"
    Assert-True -Condition (@($rows | Where-Object { $_.Text -cmatch 'StringComparison' }).Count -eq 0) `
        -Message 'and a call that passes StringComparison explicitly is not among them'
    Assert-True -Condition (@($rows | Where-Object { $_.Operator -cmatch '^(Equals|Contains|Replace)$' }).Count -eq 0) `
        -Message 'and Equals, Contains and Replace are not listed, because they are already ordinal'

    Write-Host "`n-- the default stays narrow, so the headline number does not move --"

    $mixed = New-Fixture -Name 'mixed.ps1' -Body @'
if ($a -eq 'green') { 1 }
switch ($b) { 'GREEN' { 2 } }
$c = $table[$key]
$d.StartsWith('x')
'@
    $wide = Get-Kinds -File $mixed -Include 'all'
    $narrow = Get-Kinds -File $mixed -Include 'binary'
    Assert-True -Condition (@($wide).Count -eq 4) `
        -Message "-Include all finds all four families (got $(@($wide).Count))"
    Assert-True -Condition (@($narrow).Count -eq 1 -and @($narrow)[0].Kind -ceq 'binary') `
        -Message 'and the default finds the operator alone, so an old number stays comparable'

    Write-Host "`n-- what the tool refuses, and what it refuses to call zero --"

    $missing = Join-Path $sandbox 'not-here.ps1'
    $threw = $false
    try { Find-CultureComparison -File $missing | Out-Null } catch { $threw = $true }
    Assert-True -Condition $threw `
        -Message 'a path that is not there is an error, not an empty list'

    # A FILE THAT DOES NOT PARSE IS NOT A FILE WITH NO COMPARISONS. The tool warns and counts it as
    # unread; a cell holds that, because the alternative is a clean-looking zero over a broken file.
    $broken = New-Fixture -Name 'broken.ps1' -Body @'
function Oops {
    if ($a -eq 'green') { 1 }
'@
    $warned = $null
    $rows = @(Find-CultureComparison -File $broken -Include 'all' -WarningVariable warned -WarningAction SilentlyContinue)
    Assert-True -Condition (@($rows).Count -eq 0 -and @($warned).Count -ge 1) `
        -Message 'an unparseable file yields no rows AND a warning, rather than a silent zero'
    Assert-True -Condition (@($warned)[0] -cmatch 'does not parse') `
        -Message 'and the warning says which file could not be read'

    Write-Host "`n-- the closing caveat is derived from the mode that ran --"

    # THE TOOL'S CLAIM ABOUT ITS OWN SCOPE HAS TO MOVE WITH ITS SCOPE. Both modes used to end with
    # the same fixed sentence -- "switch, -match and -like are not looked for" -- which is false in
    # `-Include all`, in the very output that lists switch findings. A reader who trusted it there
    # would conclude the sweep is blind to exactly the family it had just reported. (Found by D in
    # review; this cell is why it cannot come back.)
    #
    # Run as a child process, because the caveat is printed rather than returned: the cells above
    # dot-source the function, and this one has to see what an operator sees.
    $narrowOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $toolPath -Path $binary 2>&1 |
            ForEach-Object { [string]$_ }) -join "`n"
    $wideOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $toolPath -Path $binary -Include all 2>&1 |
            ForEach-Object { [string]$_ }) -join "`n"
    Assert-True -Condition ($narrowOut -cmatch 'not looked for in this mode: switch clauses') `
        -Message 'the default mode says switch clauses are not looked for'
    Assert-True -Condition ($wideOut -cnotmatch 'not looked for in this mode: switch') `
        -Message 'and the wide mode does NOT, because there it looks for them'
    # The control: both modes still name what neither of them looks for, so the derivation did not
    # simply drop the caveat in one branch.
    Assert-True -Condition (($narrowOut -cmatch 'not looked for in this mode: [^\n]*-match') -and
        ($wideOut -cmatch 'not looked for in this mode: [^\n]*-match')) `
        -Message 'CONTROL: both modes still name -match, which neither looks for'

    Write-Host "`n-- the tool holds itself to its own rule --"

    Assert-True -Condition (@(Find-CultureComparison -File $toolPath -Include 'binary').Count -eq 0) `
        -Message 'the sweep reports no culture-aware binary comparison in itself'
    $toolSource = [System.IO.File]::ReadAllText($toolPath)
    $stoppingPipes = @(($toolSource -split "`n") | Where-Object {
            $_ -match '&\s+git' -and $_ -match '\|\s*(Select-Object|Where-Object)' -and $_ -notmatch '^\s*#'
        })
    Assert-True -Condition ($stoppingPipes.Count -eq 0) `
        -Message ('and no native command in it is piped before its exit code is read' +
            $(if ($stoppingPipes.Count -gt 0) { ': ' + (($stoppingPipes | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))
    $canaryLine = '    $root = (& git rev-parse --show-toplevel 2>$null | Select-Object -First 1)'
    Assert-True -Condition (($canaryLine -match '&\s+git') -and ($canaryLine -match '\|\s*(Select-Object|Where-Object)')) `
        -Message 'and that pattern recognises the shape it is looking for when one is put in front of it'
} finally {
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------------------------------------
# #835: the number arrives with the tree it came from.
#
# The incident: this sweep reported `3 over 3` from a lane's session worktree and `354 over 36`
# from a fresh worktree of the same head. Exit 0 both times. A twelve-fold under-report is
# indistinguishable from a clean repository unless the output says which tree it read.
#
# DRIVEN WITH HAND-BUILT INPUTS, NOT WITH THIS SUITE'S OWN CHECKOUT. A cell that asked
# `Get-TreeProvenance` about the tree it happens to run in would assert whatever that tree is today
# -- green in a worktree, green in the main checkout, and silent about the distinction it exists to
# make. `Format-TreeProvenance` takes the four facts as parameters for exactly this reason.
Write-Host "#835: the summary names the tree it read"

$mainCheckout = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0
Assert-True -Condition ($mainCheckout -like '*the MAIN checkout*') `
    'equal --git-dir and --git-common-dir is the MAIN checkout -- decided by the two paths, never by a file count, because ls-files there is scoped to the CWD prefix and answers 0'

$worktree = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git/worktrees/w1' -GitCommonDir 'C:/repo/.git' -Behind 0
Assert-True -Condition ($worktree -like '*a linked worktree*') `
    'a git-dir under the common dir is a linked worktree'

$behind = Format-TreeProvenance -Head 'efd85d07' -GitDir 'C:/repo/.git/worktrees/w1' -GitCommonDir 'C:/repo/.git' -Behind 374
Assert-True -Condition ($behind -like '*374 commit(s) BEHIND*') `
    'a tree behind origin/main says so with the count -- 374 is the real number one of the fleet worktrees carried'

$level = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0
Assert-True -Condition (($level -like '*level with origin/main*') -and ($level -notlike '*UNKNOWN*')) `
    'level is said only when BOTH sides are zero and the tree is clean'

# THE REGRESSION CELL for the #1006 review's sharpest finding. `rev-list --count HEAD..origin/main`
# counts what origin/main has and HEAD does not, so a branch AHEAD of main and missing nothing
# answers 0 -- and the first version printed "level with origin/main" for it. That line was quoted
# as evidence in this PR's own body while being false about the bench it came from.
$aheadOnly = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git/worktrees/w1' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 3 -Dirty 0
Assert-True -Condition (($aheadOnly -notlike '*level with origin/main*') -and ($aheadOnly -like '*0 commits behind*') -and ($aheadOnly -like '*3 ahead*')) `
    "a branch AHEAD of main is never called level: it reads 0 behind and names the ahead count (got: $aheadOnly)"

# The scan reads WORKING-COPY bytes, so the commit alone labels two different inputs the same way.
$dirty = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 2
# NOT "a dirty tree must not say level": level in COMMITS and dirty in the WORKING TREE are both
# true at once, and the honest line says both. My first version of this assertion denied that and
# failed against correct output -- the cell was wrong, not the code.
Assert-True -Condition (($dirty -like '*2 uncommitted change(s) were SCANNED*') -and ($dirty -like '*level with origin/main*')) `
    "a dirty tree says so BESIDE the distance, because the commit is level and the bytes scanned are not it (got: $dirty)"

# The shape must ACCEPT the dirty suffix on every distance, including `level`. Without this the
# mandatory suite went red on any clean-but-dirty checkout, and this bench passed only because it
# happens to be ahead/behind -- alternatives that already carried a tail.
$levelDirty = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 2
Assert-True -Condition ($levelDirty -match '^Tree: [0-9a-f]{7,40} in (a linked worktree|the MAIN checkout), (level with origin/main.*|0 commits behind origin/main.*|\d+ commit\(s\) BEHIND origin/main.*|distance from origin/main: UNKNOWN \(.+)$') `
    "the documented shape accepts a level distance carrying a dirty suffix (got: $levelDirty)"

# An index flag makes the dirty count UNRELIABLE rather than wrong, and the line has to say which:
# a reader who sees "0 uncommitted" on a tree with a hidden edit is misled by silence.
$hiddenLine = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -Hidden 1
Assert-True -Condition (($hiddenLine -like '*hidden from*index flag*') -and ($hiddenLine -like '*NOT reliable*')) `
    "a scanned file hidden from git status by an index flag makes the line say the count cannot be trusted (got: $hiddenLine)"

$dirtyUnknown = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty $null
Assert-True -Condition ($dirtyUnknown -like '*working tree: UNKNOWN*') `
    'and an unmeasurable working tree says UNKNOWN rather than passing for clean -- the same rule the distance already follows'

# THE CELL THE OTHERS EXIST FOR. `0` and "I could not ask" are different facts; collapsing them
# would put the reassuring zero back, one layer up.
$unknown = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind $null -BehindReason 'origin/main is not present in this checkout'
Assert-True -Condition (($unknown -like '*UNKNOWN*') -and ($unknown -like '*origin/main is not present*')) `
    'an unmeasurable distance says UNKNOWN and names the reason -- never 0, which is what a clean tree looks like'

$undetermined = Format-TreeProvenance -Head '' -GitDir '' -GitCommonDir '' -Behind $null -BehindReason 'no git'
Assert-True -Condition (($undetermined -like '*could NOT be determined*') -and ($undetermined -like '*UNKNOWN head*')) `
    'with nothing measured, the line says so on both axes rather than printing a confident blank'

# THE HALF THE CELLS ABOVE DO NOT REACH, and E found it by sabotage on the review of this PR:
# every assertion above drives `Format-TreeProvenance`, the PURE half. `Get-TreeProvenance` -- the
# half that actually runs `git rev-parse` and `git rev-list` -- was covered by nothing. E replaced
# its whole body with an invented string and this suite stayed 33/33 green.
#
# That is this pull request's own subject, inside the mechanism meant to prevent it: an instrument
# that reports a tree, with the part that asks the tree unguarded.
#
# SHAPE PLUS THE HEAD, NOT THE WHOLE SENTENCE. The suite must pass in a linked worktree, in the main
# checkout, level or behind, with or without an `origin/main` -- so pinning the text would pin this
# bench. The shape is asserted for any of those, and the sha is compared against a freshly-asked
# `git rev-parse --short HEAD`, which is bench-independent and is what an invented-but-plausible
# string fails: a hardcoded line can be given the right SHAPE, it cannot be given this checkout's
# head.
Write-Host "#835: the git half answers the git, not a story about it"

$realProvenance = Get-TreeProvenance -Root $PSScriptRoot
# EVERY alternative ends in `.*`, and the reason is a red this suite would have produced on
# somebody else's machine: the dirty suffix (`, and N uncommitted change(s)...`) is appended to
# whichever distance text was chosen, so `level` and `UNKNOWN` without a tail made a clean-but-dirty
# checkout fail the MANDATORY suite for a reason that has nothing to do with provenance. This bench
# passed only because it happens to be ahead/behind, whose alternatives already carried `.*` --
# green by the accident of where it was run (review of #1006).
$shape = '^Tree: [0-9a-f]{7,40} in (a linked worktree|the MAIN checkout), (level with origin/main.*|0 commits behind origin/main.*|\d+ commit\(s\) BEHIND origin/main.*|distance from origin/main: UNKNOWN \(.+)$'
Assert-True -Condition ($realProvenance -match $shape) `
    "the real git half returns the documented shape on whatever checkout this suite runs in (got: $realProvenance)"

$headOutput = @(& git -C $PSScriptRoot rev-parse --short HEAD 2>$null)
$headExit = $LASTEXITCODE
$actualHead = if ($headExit -eq 0) { ([string](@($headOutput | Where-Object { $_ } | Select-Object -First 1))).Trim() } else { '' }
Assert-True -Condition (($actualHead.Length -gt 0) -and $realProvenance.Contains($actualHead)) `
    "and it names THIS checkout's head ($actualHead), which a plausible hardcoded string cannot -- the discriminating half of this cell"

# H's gap on #1006, and it is a real one: reverting the invocation to the two-dot
# `rev-list --count HEAD..origin/main` -- the exact Codex defect -- left the suite 39/39 GREEN.
# The property cells feed `Format-TreeProvenance` numbers BY HAND, so they never exercise the
# counting; the real-git cell asserts shape and head, which a one-sided count satisfies too.
# Every cell was about a half that was not broken.
#
# So this one BUILDS a tree whose two sides differ and drives the real `Get-TreeProvenance` at it.
# A throwaway repository, not this bench: the bench's own divergence changes with every push, and a
# cell whose answer moves with the checkout is not a cell.
Write-Host "#1006: the count reads BOTH sides, against a tree built to have them differ"

# ONE FORM FOR THE CLASS, not three hardened call sites (L, reviewing #1006). Three findings in a
# row were the same shape -- inherited git configuration reaching a synthetic fixture and reddening
# a MANDATORY suite for something about the developer's machine: `commit.gpgSign` with no key, then
# `core.hooksPath` with a failing hook. Hand-repeating the flags answers the two that were found;
# a helper answers the next one too, because there is one place to add it.
#
# `core.hooksPath=` (empty) AND `--no-verify`, deliberately both: `--no-verify` is what the
# reviewer reproduced and covers pre-commit and commit-msg, and the empty hooksPath covers the
# hooks it does not (a `prepare-commit-msg` that exits non-zero still fails the commit).
function Invoke-FixtureGit {
    param([Parameter(Mandatory)] [string] $Repo, [Parameter(Mandatory)] [string[]] $CommitArgs)
    & git -C $Repo `
        -c user.name=cell -c user.email=cell@test `
        -c commit.gpgSign=false -c core.hooksPath= `
        commit --no-verify @CommitArgs 2>$null
}

$aheadRepo = Join-Path ([System.IO.Path]::GetTempPath()) ("sweep-ahead-" + [Guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($aheadRepo) | Out-Null
try {
    & git -C $aheadRepo init --quiet 2>$null
    # -c commit.gpgSign=false: a global signing setting with no usable key in the gate environment
    # made both commits fail, leaving $baseSha empty and reddening a mandatory suite for a reason
    # that has nothing to do with what it measures (review of #1006).
    Invoke-FixtureGit -Repo $aheadRepo -CommitArgs @('--quiet', '--allow-empty', '-m', 'base')
    $baseSha = (& git -C $aheadRepo rev-parse HEAD 2>$null | Select-Object -First 1)
    # origin/main pinned at the base; HEAD then moves ahead of it. Nothing is behind.
    & git -C $aheadRepo update-ref refs/remotes/origin/main $baseSha 2>$null
    Invoke-FixtureGit -Repo $aheadRepo -CommitArgs @('--quiet', '--allow-empty', '-m', 'ahead')

    $aheadLine = Get-TreeProvenance -Root $aheadRepo

    # THE CONTROL: the fixture really is one-ahead-none-behind, asked of git directly rather than
    # assumed from the commands above. Without it a failed `init` or `update-ref` would make the
    # assertion below fail for a reason that has nothing to do with the counting.
    $sides = (& git -C $aheadRepo rev-list --left-right --count origin/main...HEAD 2>$null | Select-Object -First 1)
    Assert-True -Condition ([string]$sides -match '^0\s+1$') `
        "CONTROL: the throwaway tree is 0 behind and 1 ahead before anything is asserted about the line (got '$sides')"

    Assert-True -Condition (($aheadLine -like '*0 commits behind*') -and ($aheadLine -like '*1 ahead*') -and ($aheadLine -notlike '*level with origin/main*')) `
        "the real git half reports BOTH sides: a two-dot count answers 0 here and would print level (got: $aheadLine)"

    # THE DIRTY COUNT IS ABOUT WHAT WAS SCANNED, not about the repository. `git status` covers the
    # whole tree, so an unrelated modified file made the line say it "was SCANNED" -- a false
    # sentence in the line that exists to stop false sentences (review of #1006). The pair below is
    # what discriminates: the SAME dirty file, once outside the scanned set and once inside it.
    Set-Content -LiteralPath (Join-Path $aheadRepo 'README.md') -Value 'one' -Encoding utf8
    Set-Content -LiteralPath (Join-Path $aheadRepo 'a.ps1') -Value '$x = 1' -Encoding utf8
    & git -C $aheadRepo add -A 2>$null
    Invoke-FixtureGit -Repo $aheadRepo -CommitArgs @('--quiet', '-m', 'files')
    Set-Content -LiteralPath (Join-Path $aheadRepo 'README.md') -Value 'two' -Encoding utf8

    $outsideLine = Get-TreeProvenance -Root $aheadRepo -ScannedPaths @((Join-Path $aheadRepo 'a.ps1'))
    Assert-True -Condition ($outsideLine -notlike '*uncommitted change*') `
        "a dirty file OUTSIDE the scanned set is not reported as scanned input (got: $outsideLine)"

    Set-Content -LiteralPath (Join-Path $aheadRepo 'a.ps1') -Value '$x = 2' -Encoding utf8
    $insideLine = Get-TreeProvenance -Root $aheadRepo -ScannedPaths @((Join-Path $aheadRepo 'a.ps1'))
    Assert-True -Condition ($insideLine -like '*1 uncommitted change(s) were SCANNED*') `
        "and a dirty file INSIDE it is -- the control that makes the line above a measurement (got: $insideLine)"

    # THE CASE THE REVIEWER REPRODUCED (#1006): `--assume-unchanged` makes `git status` omit the
    # file while the parser still reads its edited bytes. The dirty count drops to 0 and the tree
    # reads clean -- the false-clean this line exists to prevent, hiding behind an index flag.
    # Same file, same edit, one `update-index` call between the two readings.
    & git -C $aheadRepo update-index --assume-unchanged 'a.ps1' 2>$null
    $hiddenRepoLine = Get-TreeProvenance -Root $aheadRepo -ScannedPaths @((Join-Path $aheadRepo 'a.ps1'))
    Assert-True -Condition ($hiddenRepoLine -notlike '*uncommitted change*') `
        "CONTROL: with the flag set, git status really does report the edit as absent (got: $hiddenRepoLine)"
    Assert-True -Condition ($hiddenRepoLine -like '*hidden from*index flag*') `
        "and the line says so, instead of letting the silence read as a clean tree (got: $hiddenRepoLine)"
    & git -C $aheadRepo update-index --no-assume-unchanged 'a.ps1' 2>$null
} finally {
    Remove-Item -Recurse -Force -LiteralPath $aheadRepo -ErrorAction SilentlyContinue
}

# THE MODE NOBODY LOOKS AT. `-AsJson` returned before the provenance existed, so a machine caller
# got findings from a stale checkout with no head and no distance -- the false reading this change
# removes, surviving in the one path with no human reading the output. (#1006 review.)
$jsonOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $toolPath -Path $toolPath -AsJson 2>&1 | Out-String)
$jsonParsed = $null
try { $jsonParsed = $jsonOut | ConvertFrom-Json } catch { $jsonParsed = $null }
Assert-True -Condition (($null -ne $jsonParsed) -and ($jsonParsed.PSObject.Properties.Name -contains 'tree') -and ($jsonParsed.PSObject.Properties.Name -contains 'sites')) `
    'the -AsJson contract carries the tree beside the sites, and still parses as one JSON document'

# And the line actually reaches the output. Driven through the real script, the same way the mode
# cells above drive it, so a function nobody calls cannot pass this suite.
$provenanceOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $toolPath -Path $toolPath 2>&1 |
    Out-String)
Assert-True -Condition ($provenanceOut -like '*explicit -Path*') `
    'with -Path the tool says the count is of the files named on the command line, rather than describing a checkout those files may not live in'

# Source guard, same shape as the ordinal one above: a fixture commit that inherits a global
# `core.hooksPath` runs somebody's pre-commit hook and fails, reddening a MANDATORY suite for a
# local configuration. Checked in the source because the behavioural version would need a global
# hook installed on this machine to reproduce (#1006 review, reproduced by the reviewer).
$suiteText = [System.IO.File]::ReadAllText($PSCommandPath)
$viaHelper = @([regex]::Matches($suiteText, 'Invoke-FixtureGit -Repo')).Count
Assert-True -Condition ($viaHelper -ge 3) `
    "the fixture commits go through the helper at all ($viaHelper) -- the control for the absence below"

# The absence that matters is a commit built by HAND, which is how the hardening drifts back out.
Assert-True -Condition (@([regex]::Matches($suiteText, '& git [^
]*\scommit\s')).Count -eq 0) `
    'no fixture commit is spelled by hand: one form carries gpgSign, hooksPath, --no-verify and identity, so the next inherited-config hazard has one place to be fixed'

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount. A case vanished or was added without updating the declared total." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total)/$($script:total)" -ForegroundColor Green
