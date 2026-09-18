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
$ExpectedAssertionCount = 90

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

# -HeadBefore matches -Head deliberately: this cell asserts that the distance is known and the
# three head readings agree; working-copy continuity is a separate, explicitly UNKNOWN fact.
$level = Format-TreeProvenance -Head 'abc12345' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'abc12345' -HeadAfterAll 'abc12345'
Assert-True -Condition (($level -like '*level with origin/main*') -and ($level -like '*working-copy continuity: UNKNOWN*') -and ($level -notlike '*distance from origin/main: UNKNOWN*')) `
    'level is said only when BOTH sides are zero, while working-copy continuity remains explicitly UNKNOWN'

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

$rewrittenSameCount = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 7 }) -ReflogAfter ([pscustomobject]@{ Top = 'bbbb2222'; Count = 7 })
Assert-True -Condition (($rewrittenSameCount -like '*reflog was REWRITTEN*') -and ($rewrittenSameCount -like '*whether an entry was appended is UNKNOWN*') -and ($rewrittenSameCount -like '*whether HEAD moved is UNKNOWN*') -and ($rewrittenSameCount -notlike '*, and HEAD MOVED*') -and ($rewrittenSameCount -notlike '*MOVED AND RETURNED*')) `
    "an OID change with no net reflog growth keeps append and movement UNKNOWN, not inferred (got: $rewrittenSameCount)"

Assert-True -Condition ($level -like '*working-copy continuity: UNKNOWN*') `
    "a clean trailing status does not claim atomic working-copy contents (got: $level)"

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

# EVERY FIELD IN THIS LINE IS MEASURED AFTER THE SCAN, and the scan is not instant. The file list
# comes from the index at T0, the bytes are read across [T0,T1], and the head, distance and dirty
# count are asked at T1 -- so a checkout landing inside that window makes all of them name a tree
# that did not produce the findings printed beside them. The line would be confidently wrong,
# which is the one failure this line exists to prevent (Codex P2 on #1006).
#
# THE NEGATIVE IS PRINTED TOO. Reporting movement only when it happens leaves a quiet line
# ambiguous between "did not move" and "was never asked", and the quiet reading is the
# reassuring one. It also makes the wiring provable end to end below: only a run that really
# took a head before the listing can print the unchanged sentence.
$moved = Format-TreeProvenance -Head 'bbbb2222' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'bbbb2222'
# THE SENTENCE REPORTS THE DISAGREEMENT, and stops there. It used to say `NO head here describes
# what was read`, which is a denial the readings do not support: a checkout landing between the
# first head read and the listing leaves the whole scan produced by the NEW head, which the two
# later readings name (Codex P2 on #1024). What is observed is that the boundaries disagree, so
# that is what is said.
Assert-True -Condition (($moved -like '*the tree MOVED*') -and ($moved -like '*aaaa1111*') -and ($moved -like '*bbbb2222*') -and ($moved -like '*cannot say which*') -and ($moved -notlike '*NO head*')) `
    "a head that differs across the boundaries says the readings DISAGREE and that the line cannot say which produced the findings, rather than denying that any did (got: $moved)"

# ABBREVIATIONS ARE FOR DISPLAY ONLY. `rev-parse --short` lengthens an abbreviation to stay
# unique, so a concurrent fetch or object write changes the text without changing HEAD -- a
# reviewer reproduced the same commit reading `23b4` then `23b41` at `core.abbrev=4`, which
# every comparison here would have called a move. Full ids are read; the line shortens them.
$fullShas = Format-TreeProvenance -Head '1111111111111111111111111111111111111111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore '2222222222222222222222222222222222222222' -HeadAfterAll '1111111111111111111111111111111111111111'
Assert-True -Condition ($fullShas -like 'Tree: 11111111 in *') `
    "the head is NAMED with an 8-character abbreviation, which is all a single id needs (got: $fullShas)"

# EIGHT CHARACTERS NAMES ONE COMMIT AND CANNOT TELL TWO APART. The disagreement branch is the
# only place this line prints ids for a reader to COMPARE, and two ids sharing an 8-character
# prefix would print identically there -- a line saying the readings DISAGREE while showing the
# same value three times, leaving the operator unable to identify what was observed (Codex P2 on
# #1024). That branch prints them whole; every other place still names one id in eight.
Assert-True -Condition ($fullShas -like '*2222222222222222222222222222222222222222 ->*') `
    "and the disagreement prints the ids WHOLE, because that is the branch a reader compares (got: $fullShas)"

# THE COLLISION ITSELF, not an argument about it: two ids sharing their first eight characters.
$collide = Format-TreeProvenance -Head 'abcd1234aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'abcd1234bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' -HeadAfterAll 'abcd1234aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
Assert-True -Condition (($collide -like '*abcd1234bbbb*') -and ($collide -like '*abcd1234aaaa*')) `
    "two ids sharing an 8-character prefix stay distinguishable in the disagreement (got: $collide)"

# Read here, not borrowed: the later cells define their own copy further down the file, and a
# variable from below is not in scope yet -- StrictMode says so rather than treating it as empty.
$headReadLines = @([System.IO.File]::ReadAllText($toolPath) -split "`r?`n")
$shortReads = @($headReadLines | Where-Object { $_.Contains('rev-parse --short HEAD') })
$fullReads = @($headReadLines | Where-Object { $_.Contains('rev-parse HEAD 2>$null') })
Assert-True -Condition (@($fullReads).Count -eq 3) `
    "CONTROL: all three head readings are present and ask for the full id (got $(@($fullReads).Count))"

Assert-True -Condition (@($shortReads).Count -eq 0) `
    'and none of them asks for an abbreviation, which can lengthen under a concurrent write and read as a move'

# THE SWEEP BY PROPERTY, because the last three fixes were one-sided: the twin head, the pinned
# distance while status and ls-files -v were not, and full ids for `rev-parse` while the reflog
# still abbreviated. Each was applied to the instrument under discussion instead of to every
# instrument with the property (J on #1024). The property: ANY git output this tool compares as
# TEXT must be the full object id, because abbreviations lengthen under a concurrent object
# write. Enumerated with a COUNT control, so adding a git call to this file reds this cell and
# whoever adds it has to decide whether their output is compared.
$gitCalls = @($headReadLines | Where-Object { $_.Contains('& git ') -and $_.Contains(' -C ') })
Assert-True -Condition (@($gitCalls).Count -eq 9) `
    "CONTROL: the tool makes 9 git calls (got $(@($gitCalls).Count)) -- a new one must be classified, not inherited"

Assert-True -Condition (@($gitCalls | Where-Object { $_.Contains('--short') }).Count -eq 0) `
    'no git call asks for an abbreviated object id'

# A COMMAND WHOSE OUTPUT IS PARSED MUST NOT BE CONFIGURABLE BY WHOEVER RUNS IT. This file
# inherited a developer's git configuration three separate times -- core.logAllRefUpdates left
# the fixture with no reflog, GIT_DEFAULT_HASH made its object ids 64 characters, and
# color.ui=always wrapped the parsed reflog column in ANSI -- each time turning the authoritative
# gate red over something that has nothing to do with what is measured. Each was reported
# separately because each was fixed separately; the property is swept here so the fourth one
# cannot arrive on a call that simply was not the one under discussion.
Assert-True -Condition (@($gitCalls | Where-Object { $_.Contains('-c color.ui=false') }).Count -eq @($gitCalls).Count) `
    "every git call pins color.ui, so a developer's color.ui=always cannot decorate a value this tool parses (got $(@($gitCalls | Where-Object { $_.Contains('-c color.ui=false') }).Count) of $(@($gitCalls).Count))"

$reflogCalls = @($gitCalls | Where-Object { $_.Contains('reflog') })
# `--no-color` AS WELL AS the config pin: `color.diff` is more specific than `color.ui` and wins
# for the log family that `reflog show` belongs to. Measured: `-c color.diff=always -c
# color.ui=false` still returns the oid wrapped in ESC[33m; the command's own option returns it
# clean. A blanket pin covers the general case, and where the command has its own option, the
# option is the authority -- a subordinate setting can always override the general one.
Assert-True -Condition ($reflogCalls[0].Contains('--no-color')) `
    'the reflog read passes --no-color, because a more specific color setting overrides the config pin for the log family'

Assert-True -Condition ((@($reflogCalls).Count -eq 1) -and $reflogCalls[0].Contains('--no-abbrev')) `
    'and the reflog read asks for full ids too -- its first column abbreviates by default (8 characters against 40) and that column is compared with String::Equals'

$stable = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 7 }) -ReflogAfter ([pscustomobject]@{ Top = 'aaaa1111'; Count = 7 })
Assert-True -Condition (($stable -like '*head unchanged at all 3 readings*') -and ($stable -like '*reflog SNAPSHOTS are equal*') -and ($stable -like '*not proof that no update happened*') -and ($stable -notlike '*tree MOVED*') -and ($stable -notlike '*MOVED AND RETURNED*')) `
    "an unchanged head says so explicitly rather than by silence, and NAMES the interval the reflog covers rather than claiming the whole run (got: $stable)"

# AND IT SAYS WHAT IT DOES NOT COVER. Something is read last, and whatever it is has an
# unobserved tail: an A->B->A completed between the last reflog snapshot and the final head read
# is invisible to every reading here, and a further snapshot would only move the tail rather
# than remove it (Codex P2 on #1024). The remedy available to a line is to stop claiming past
# its instrument.
# `-notlike '*MOVED*'` above became `'*tree MOVED*'` because the sentence now contains the word
# reMOVED, and a case-insensitive substring negative cannot tell one from the other -- the
# absence would have fired on prose that says nothing about movement.
Assert-True -Condition ($stable -like '*Nothing here observes what happens after the last snapshot*') `
    "and it states the tail it cannot see, instead of reading as a guarantee over the whole run (got: $stable)"

# THE LIMIT OF SAMPLING, said in the line rather than left for a reader to work out. Three
# equal readings establish that three instants agreed, not that nothing happened between them
# (Codex P2 on #1024). Without the reflog the sentence has to claim the narrower fact.
$sampledOnly = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111'
Assert-True -Condition (($sampledOnly -like '*which are SAMPLES*') -and ($sampledOnly -notlike '*reflog records no HEAD update*')) `
    "with no reflog the line says the 3 readings are SAMPLES and that a move and a move back would not have been seen (got: $sampledOnly)"

# AND THE DETECTOR EARNS ITS PLACE: equal heads at every boundary, and the reflog says HEAD was
# updated anyway. No number of head samples reports this; the reflog does, because git appends
# an entry for every update whether or not the sha comes back.
$returned = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 7 }) -ReflogAfter ([pscustomobject]@{ Top = 'bbbb2222'; Count = 9 })
Assert-True -Condition (($returned -like '*MOVED AND RETURNED*') -and ($returned -like '*bbbb2222*') -and ($returned -like '*aaaa1111*') -and ($returned -notlike '*unchanged*')) `
    "three identical heads and a reflog whose newest entry names a DIFFERENT commit is a proven move and return -- nothing but HEAD standing elsewhere writes another oid there (got: $returned)"

# AND IT SURVIVES PRUNING, which a counter does not. `git reflog expire` running in another lane
# can remove more old entries than an A->B->A checkout appended, leaving the count equal or
# LOWER while HEAD moved -- a count-based detector then reports no movement (Codex P2 on #1024).
# Pruning never removes the newest entry, and every HEAD update appends one, so the top line
# decides and the count only describes.
$pruned = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 40 }) -ReflogAfter ([pscustomobject]@{ Top = 'bbbb2222'; Count = 6 })
# AN OID THAT CHANGED WITHOUT AN APPEND IS A REWRITE, NOT A MOVE. `git reflog delete HEAD@{0}`
# removes the newest entry and EXPOSES an older one with a different oid while HEAD never moves;
# an expire that reaches the newest entry does the same. A move APPENDS, so an oid change with no
# growth in the count cannot be told from a rewrite, and UNKNOWN is the honest answer (Codex P2
# on #1024). This cell used to assert the opposite -- that the oid alone proved the move.
Assert-True -Condition (($pruned -like '*reflog was REWRITTEN*') -and ($pruned -like '*bbbb2222*') -and ($pruned -like '*UNKNOWN*') -and ($pruned -notlike '*MOVED AND RETURNED*')) `
    "an oid that changed while the count FELL is reported as a rewrite of unknown meaning, not as a proven move (got: $pruned)"

# AND THE PRUNE THAT LEAVES THE NEWEST ENTRY ALONE. Both marks stay usable, neither signal
# fires, and the line used to say the snapshots were equal with the 'same count' -- contradicted
# by the measurement it was reporting (Codex P2 on #1024).
$prunedOnly = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 40 }) -ReflogAfter ([pscustomobject]@{ Top = 'aaaa1111'; Count = 6 })
Assert-True -Condition (($prunedOnly -like '*reflog was PRUNED*') -and ($prunedOnly -like '*40 to 6*') -and ($prunedOnly -notlike '*SNAPSHOTS are equal*')) `
    "a count that fell with the newest entry unchanged is reported as a prune, with both counts, instead of being called equal snapshots (got: $prunedOnly)"

# AND IT CLAIMS NO ABSENCE OF APPENDS. A NET decrease cannot rule out concurrent appends: an
# in-place operation such as a hard reset onto the same commit appends an entry with the SAME
# oid, and a prune of two older ones hides it in the net -- top equal, count down, an append that
# happened (Codex P2 on #1024). The line reports the two things it saw and marks the third
# unknown.
Assert-True -Condition (($prunedOnly -like '*whether anything was APPENDED in between is*') -and ($prunedOnly -like '*UNKNOWN*') -and ($prunedOnly -notlike '*no HEAD update was appended*')) `
    "and it says whether anything was appended is UNKNOWN, rather than reading a net decrease as proof that nothing was (got: $prunedOnly)"

# AND THE TOP LINE IS NOT A UNIQUE IDENTITY, which is the top-line detector's own blind spot. The
# same B->A checkout performed twice writes byte-identical top text, so a marker built only on it
# reports no movement while the count climbs -- reproduced by a reviewer at 5 to 7 with the top
# unchanged (Codex P2 on #1024). Each signal covers what the other misses: the top survives
# pruning, the count survives repetition, so EITHER is movement. Requiring both would be an AND of
# two partial detectors, which detects neither.
$repeated = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 5 }) -ReflogAfter ([pscustomobject]@{ Top = 'aaaa1111'; Count = 7 })
Assert-True -Condition (($repeated -like '*HEAD OPERATION was RECORDED*') -and ($repeated -like '*gained 2 entr*') -and ($repeated -notlike '*MOVED AND RETURNED*') -and ($repeated -notlike '*unchanged*')) `
    "a same-oid reflog append reports an OPERATION, never a move: `git reset --hard HEAD` writes an entry with the oid HEAD already had, and the count alone cannot tell that from a move and a return (got: $repeated)"

# THE ORDER OF THE LAST TWO READINGS, asserted on the source because a race cannot be staged from
# a cell. Something has to be read last. When the REFLOG was last, a checkout landing between the
# final head read and it left all three head samples equal and the reflog changed, and the line
# said MOVED AND RETURNED for a HEAD that moved and did NOT return. With the head last, a move in
# that same gap makes the heads DISAGREE, which is a true sentence (Codex P2 on #1024).
# Read here rather than borrowed from a later cell: a variable defined further down the file is
# not in scope yet, and StrictMode says so rather than treating it as empty.
$toolLines = @([System.IO.File]::ReadAllText($toolPath) -split "`r?`n")
$reflogReadIndex = @(0..($toolLines.Count - 1) | Where-Object { $toolLines[$_].Contains('$reflogAfter = Get-HeadReflogMark') })
$finalHeadIndex = @(0..($toolLines.Count - 1) | Where-Object { $toolLines[$_].Contains('$endOutput = @(& git') -and $toolLines[$_].Contains('rev-parse') })
Assert-True -Condition ((@($reflogReadIndex).Count -eq 1) -and (@($finalHeadIndex).Count -eq 1)) `
    "CONTROL: one trailing reflog read and one final head read (got $(@($reflogReadIndex).Count) and $(@($finalHeadIndex).Count))"

Assert-True -Condition ($reflogReadIndex[0] -lt $finalHeadIndex[0]) `
    'the reflog is read BEFORE the final head, so a checkout in the gap between them disagrees on the head instead of being reported as a move that returned'

# THE CASE ONLY THREE BOUNDARIES CATCH: moved and moved back. The first and last readings agree,
# so a two-boundary check calls this stable, while the distance was counted against a head that
# neither of them names.
$thereAndBack = Format-TreeProvenance -Head 'bbbb2222' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111'
Assert-True -Condition (($thereAndBack -like '*the tree MOVED*') -and ($thereAndBack -notlike '*unchanged*')) `
    "a tree that moved and moved back is MOVED, not stable -- the outer boundaries agree and the middle one does not (got: $thereAndBack)"

# THE THIRD STATE, which is the one a convenience default would erase: if the head could not be
# read before the scan, an empty string must NOT compare equal to the head read after it and
# report a stable tree. Absence and agreement are different facts.
$unaskedMove = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore '' -HeadAfterAll 'aaaa1111'
Assert-True -Condition (($unaskedMove -like '*whether the tree MOVED is UNKNOWN (1 of the 3*') -and ($unaskedMove -notlike '*unchanged*')) `
    'an unread before-head reports UNKNOWN movement, never a stable tree -- the empty string must not read as equal to the head measured at the end'

# THE MIRROR, and it did not exist until a reviewer asked for it. The cell above guards the
# BEFORE head; the AFTER head reaches the same comparison through $headText, which is the sha OR
# the literal 'an UNKNOWN head'. A real sha is never equal to that literal, so a failed post-scan
# read printed "MOVED during the scan (<sha> -> an UNKNOWN head)" -- a movement nobody observed,
# manufactured out of a failure, in the line whose whole job is to refuse exactly that. I applied
# the discipline to the parameter I was adding and not to its twin (J on #1024). Either head
# missing means the question was not answered.
# UNKNOWN IS FOR WHEN NOTHING WAS ESTABLISHED, not for whenever something was missed. A failed
# reading removes evidence; it does not remove the evidence that survived. With two readings
# that DISAGREE and a third unread, movement is already proven, and reporting UNKNOWN throws
# away a fact the run holds -- in the flattering direction, because UNKNOWN reads as 'probably
# fine' where MOVED reads as 'do not trust these findings' (Codex P2 on #1024).
$provenDespiteUnread = Format-TreeProvenance -Head 'bbbb2222' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll ''
Assert-True -Condition (($provenDespiteUnread -like '*the head readings DISAGREE*') -and ($provenDespiteUnread -like '*1 of the 3 head readings failed*') -and ($provenDespiteUnread -notlike '*whether the tree MOVED is UNKNOWN*')) `
    "two readings that disagree PROVE movement even with the third unread, and the line reports the movement while still naming the failed boundary (got: $provenDespiteUnread)"

# The same rule for the OTHER independent witness: a changed reflog oid is proof that does not
# come from the head samples at all, so an unread head must not suppress it either.
$reflogProofDespiteUnread = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll '' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 5 }) -ReflogAfter ([pscustomobject]@{ Top = 'bbbb2222'; Count = 6 })
Assert-True -Condition (($reflogProofDespiteUnread -like '*HEAD MOVED during the run*') -and ($reflogProofDespiteUnread -like '*independent of the head*') -and ($reflogProofDespiteUnread -notlike '*whether the tree MOVED is UNKNOWN*')) `
    "a changed reflog oid is proof independent of the head readings, so one unread head does not suppress it (got: $reflogProofDespiteUnread)"

# A same-oid reflog append is still a known operation when one head boundary is unread. The
# operation evidence comes from the two reflog snapshots; only the separate question of whether
# the tree moved remains UNKNOWN. Losing the operation sentence in this state would turn a known
# `git reset --hard HEAD` into a generic sample failure.
$sameOidOperationDespiteUnread = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll '' -ReflogBefore ([pscustomobject]@{ Top = 'aaaa1111'; Count = 5 }) -ReflogAfter ([pscustomobject]@{ Top = 'aaaa1111'; Count = 6 })
Assert-True -Condition (($sameOidOperationDespiteUnread -like '*HEAD OPERATION was RECORDED*') -and ($sameOidOperationDespiteUnread -like '*gained 1 entr*') -and ($sameOidOperationDespiteUnread -like '*whether the tree MOVED is UNKNOWN*') -and ($sameOidOperationDespiteUnread -like '*1 of the 3 head readings failed*')) `
    "a same-oid reflog append remains recorded evidence while an unread head keeps movement UNKNOWN (got: $sameOidOperationDespiteUnread)"

# AND THE PAIR PRINTED FOR COMPARISON IS WHOLE HERE TOO. The full-id rule was applied to the
# disagreement branch alone and left this one abbreviating two DISTINCT oids, which print
# identically when they share eight characters -- a line claiming HEAD stood on another commit
# while showing the same value twice. Same property, one branch later.
# The fixture names the SAME commit in the head readings and in the opening marker, because the
# marker check added beside this cell makes an inconsistent fixture exercise a different branch:
# a marker that disagrees with HEAD at the first reading is now reported as out of step, and a
# cell about printing a movement pair whole would silently stop reaching the movement branch at
# all. A fixture has to be a tree that could exist.
$reflogCollide = Format-TreeProvenance -Head 'abcd1234bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'abcd1234bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' -HeadAfterAll 'abcd1234bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' -ReflogBefore ([pscustomobject]@{ Top = 'abcd1234bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'; Count = 5 }) -ReflogAfter ([pscustomobject]@{ Top = 'abcd1234aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'; Count = 6 })
Assert-True -Condition (($reflogCollide -like '*MOVED AND RETURNED*') -and ($reflogCollide -like '*abcd1234bbbb*') -and ($reflogCollide -like '*abcd1234aaaa*')) `
    "the reflog-movement branch prints its two oids whole, so a shared 8-character prefix does not make it claim a move while showing one value twice (got: $reflogCollide)"

# THE INITIAL MARKER IS ITSELF A READING, and until this cell it was trusted as an axiom. The
# whole oid-changed inference reads "the newest entry named X before and Y after, so HEAD stood
# somewhere else in between" -- which only follows if the entry named where HEAD WAS at the
# start. `git reflog delete HEAD@{0}` without `--updateref` removes the newest entry and leaves
# HEAD exactly where it is, so the marker exposes an OLDER commit while HEAD never moved. A
# later in-place `git reset --hard HEAD` then appends an entry naming HEAD: oid changed, count
# grew, and every one of the three head readings is the same value -- the exact input shape of
# MOVED AND RETURNED, produced by a tree that never left the commit (Codex P2 on #1024). The
# rewrite happens BEFORE the first snapshot, so the delta table sees ordinary growth and cannot
# catch it; the marker has to be checked against the head read at the same instant instead.
$desyncedMarker = Format-TreeProvenance -Head 'aaaa1111' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111' -ReflogBefore ([pscustomobject]@{ Top = 'bbbb2222'; Count = 2 }) -ReflogAfter ([pscustomobject]@{ Top = 'aaaa1111'; Count = 3 })
Assert-True -Condition (($desyncedMarker -notlike '*MOVED AND RETURNED*') -and ($desyncedMarker -notlike '*, and HEAD MOVED*') -and ($desyncedMarker -like '*out of step with HEAD*') -and ($desyncedMarker -like '*whether HEAD MOVED is UNKNOWN*') -and ($desyncedMarker -like '*bbbb2222*') -and ($desyncedMarker -like '*aaaa1111*')) `
    "a reflog marker that already disagreed with HEAD at the first reading cannot carry the movement inference: the line says the log was out of step and reports movement as UNKNOWN (got: $desyncedMarker)"

$unreadAfter = Format-TreeProvenance -Head '' -GitDir 'C:/repo/.git' -GitCommonDir 'C:/repo/.git' -Behind 0 -Ahead 0 -Dirty 0 -HeadBefore 'aaaa1111' -HeadAfterAll 'aaaa1111'
Assert-True -Condition (($unreadAfter -like '*whether the tree MOVED is UNKNOWN*') -and ($unreadAfter -notlike '*aaaa1111 ->*')) `
    "an unreadable head AFTER the scan reports UNKNOWN movement too, never a move from the head that was read into a failure (got: $unreadAfter)"

# And the distance is pinned to the sha that was READ, not asked of HEAD a second time: a
# checkout between the two calls would leave the head and the distance describing different
# commits, inside the one line whose job is to say which commit these findings came from. The
# behaviour cannot be forced from a cell without a race, so the SHAPE is asserted against the
# tool's own source, with the read proved first.
$toolText = [System.IO.File]::ReadAllText($toolPath)
Assert-True -Condition ($toolText.Length -gt 10000) `
    "CONTROL: the tool source was read ($($toolText.Length) chars), so the absence below is about a file that exists"

# ASSERTED ON THE COMMAND LINE, NOT ON THE FILE. The comments above that call deliberately
# QUOTE "origin/main...HEAD" to explain what the two-dot form got wrong, so a file-wide absence
# check fails on the sentence describing the fix. Same trap, second suite.
$revListLines = @($toolText -split "`r?`n" | Where-Object { $_.Contains('-C $Root rev-list') -and (-not $_.TrimStart().StartsWith('#')) })
Assert-True -Condition (@($revListLines).Count -eq 1) `
    "CONTROL: exactly one rev-list call in the tool (got $(@($revListLines).Count)), so the assertion below is about a known line"

Assert-True -Condition ($revListLines[0].Contains('origin/main...$head') -and (-not $revListLines[0].Contains('origin/main...HEAD'))) `
    "the distance is counted against the sha already read, not against HEAD again -- the same window one level down (got: $($revListLines[0].Trim()))"

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

# NOT `--short`: its width is `core.abbrev`, and the formatter prints a fixed 8, so at
# `core.abbrev=12` this cell demanded a longer value than the line can contain and reddened the
# authoritative gate over a developer's configuration (Codex P2 on #1024). The FOURTH time this
# pair of files inherited a git setting, and the first one inside the tests, which is why the
# tool-side property guard could not see it -- a sweep is only as wide as the population it
# walks. The rule the formatter applies is applied here too: read the full id, take eight.
$headOutput = @(& git -c color.ui=false -C $PSScriptRoot rev-parse HEAD 2>$null)
$headExit = $LASTEXITCODE
$fullHead = if ($headExit -eq 0) { ([string](@($headOutput | Where-Object { $_ } | Select-Object -First 1))).Trim() } else { '' }
$actualHead = if ($fullHead.Length -ge 8) { $fullHead.Substring(0, 8) } else { $fullHead }
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
    # THE HASH IS STATED AT INIT, not configured afterwards: `extensions.objectFormat` needs
    # repositoryformatversion 1 and setting it on a v0 repo makes every later git call answer
    # `fatal: repo version is 0, but v1-only extension found` -- measured, on the first attempt.
    # `GIT_DEFAULT_HASH=sha256` in a developer's environment otherwise makes an unqualified init
    # produce 64-character ids, and the shape assertion below -- correctly reading a FULL oid --
    # would fail for a configuration unrelated to what is measured (Codex P2 on #1024). Same
    # class as the reflog setting: the fixture states what it needs.
    & git -C $aheadRepo init --quiet --object-format=sha1 2>$null
    # REFLOGS ON, EXPLICITLY. A developer with global `core.logAllRefUpdates=false` gives this
    # throwaway repository no reflog at all: `git reflog show HEAD` succeeds with zero lines,
    # `Get-HeadReflogMark` correctly answers $null, and the end-to-end assertion below fails --
    # the authoritative local gate red for a configuration that has nothing to do with what is
    # being measured (Codex P2 on #1024). The fixture states what it needs instead of inheriting
    # it, the same reason the commits here pass their own identity and signing settings.
    & git -C $aheadRepo config core.logAllRefUpdates true 2>$null
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

    # THE MARK IS AN OID, and nothing proved it until a sabotage stayed green: every formatter cell
    # hands Top a value that already looks like an oid, so returning the whole reflog LINE instead
    # would have satisfied all of them. The line carries a message, and two operations leaving HEAD
    # on the same commit write different messages -- comparing lines would call that a move. Asked
    # of the real repository, where the answer is git's and not the cell's.
    $realMark = Get-HeadReflogMark -Root $aheadRepo
    Assert-True -Condition ($null -ne $realMark) `
        'CONTROL: the throwaway tree has a usable reflog, so the shape assertion below is about a value that exists'

    Assert-True -Condition ($realMark.Top -match '^[0-9a-f]{40}$|^[0-9a-f]{64}$') `
        "the mark's Top is a FULL object id and nothing else -- 40 for SHA-1, 64 for SHA-256, and neither is a display line (got: $($realMark.Top))"

    # THE HOSTILE CONFIGURATION, BUILT RATHER THAN ASSUMED. Setting `color.diff always` on the
    # bench does not reach this fixture -- a throwaway `git init` inherits global and system
    # config, not another repository's local file -- so an attempt to reproduce the reviewer's
    # case from the bench reddened only the source assertion and left this cell green. The
    # condition has to be constructed WHERE the reading happens.
    #
    # `color.diff` is more specific than `color.ui` and wins for the log family that
    # `reflog show` belongs to, so the `-c color.ui=false` pin does not suppress it and the oid
    # arrives wrapped in ESC[33m (Codex P2 on #1024).
    & git -C $aheadRepo config color.diff always 2>$null
    $colouredMark = Get-HeadReflogMark -Root $aheadRepo
    & git -C $aheadRepo config --unset color.diff 2>$null

    Assert-True -Condition ($null -ne $colouredMark) `
        'CONTROL: the reflog is still readable with color.diff=always, so the shape check below is about decoration and not about a failed read'

    Assert-True -Condition ($colouredMark.Top -match '^[0-9a-f]{40}$|^[0-9a-f]{64}$') `
        "a repository with color.diff=always still yields a bare object id, because the reflog read passes --no-color and not only the config pin (got: $($colouredMark.Top))"

    # THE WIRING, END TO END, and the reason the unchanged sentence is printed at all. Every
    # movement cell above drives the PURE formatter with hand-made shas; none of them proves the
    # SCRIPT reads a head before its file listing. This runs the real tool in repository-wide
    # mode against the throwaway tree -- the only mode where provenance is asked -- and the only
    # way its output can carry the unchanged sentence is if that read actually happened. Delete
    # the capture and this cell reads UNKNOWN.
    Push-Location -LiteralPath $aheadRepo
    try {
        $sweepOut = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $toolPath 2>&1 | Out-String)
    } finally {
        Pop-Location
    }
    Assert-True -Condition ($sweepOut -like '*Tree:*') `
        "CONTROL: the repository-wide run reached the provenance line at all (got: $sweepOut)"
    Assert-True -Condition ($sweepOut -like '*head unchanged at all 3 readings*') `
        "and the script really takes a head BEFORE its listing: a tree that did not move says so, which an unread before-head cannot print (got: $sweepOut)"

    # AND THE DETECTOR IS WIRED, not only implemented. This tree has a reflog, so a run that
    # really read it before and after says so; a run that never called the helper falls to the
    # SAMPLES sentence and reds here.
    Assert-True -Condition ($sweepOut -like '*reflog SNAPSHOTS are equal*') `
        "and the reflog readings are taken by the real run, not just accepted as parameters (got: $sweepOut)"
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
