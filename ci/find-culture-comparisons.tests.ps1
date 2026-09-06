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
$ExpectedAssertionCount = 26

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
