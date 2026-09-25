# #822: a stale test binary is not a third failure. The freshness cross-check says the other stages
# measured a DIFFERENT PROGRAM than this tree, so their reds are not findings about this head -- and
# the old verdict line printed it as a peer of them ("failed stages: workspace tests, binary
# freshness cross-check"), which sent a reader to debug a failure that was never observed.
#
# The decision is `Get-GateVerdictLines`, cut out of ci/gate.ps1 by anchor text and dot-sourced
# alone: running gate.ps1 would run the gate. Same homegrown harness as the other gate suites.
$ExpectedAssertionCount = 22

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# THE NAME COMES FROM THE SUBJECT, NOT FROM THIS FILE. Retyping the literal here made the cells
# prove the decision while leaving the AGREEMENT between the arming site and the filter unmeasured:
# renaming only the arming literal kept this suite at 18/18 with #822's defect restored. Read out of
# gate.ps1 by its constant, the cells now fail if the two ever spell it differently -- and with the
# constant used at both sites in the subject, they cannot.
# Single-quoted: in double quotes PowerShell expands `$FreshnessStageName`, the name this
# pattern is looking FOR, and the match then searches for an empty string.
$nameMatch = [regex]::Match($gateText, '(?m)^\$FreshnessStageName = ''([^'']+)''')
Assert-True ($nameMatch.Success) 'ARRANGEMENT: the cross-check name is a constant in gate.ps1, so the cells can read the real one'
$freshnessName = $nameMatch.Groups[1].Value
# COUNTED WITH A QUOTE CLASS, NOT WITH PLICAS. The first version of this cell wrapped the name in
# single quotes and counted that: a second copy written with DOUBLE quotes -- `$failed += "binary
# freshness cross-check"`, ordinary PowerShell that behaves identically -- was not a literal this
# cell could see, so #822's defect came back at 20/20 green. Found by K on 57e76b72.
#
# The counter is a function so the cells can FEED it text instead of only reading the subject
# through it: a counter that is blind to a quoting style reports 1 on the real file either way, and
# nothing about the real file can tell the two apart.
function Measure-NameLiterals {
    param(
        [Parameter(Mandatory)] [string] $Text,
        [Parameter(Mandatory)] [string] $Name
    )
    $quote = '[' + [char]39 + [char]34 + ']'
    @([regex]::Matches($Text, $quote + [regex]::Escape($Name) + $quote)).Count
}

# The counter sees BOTH quoting styles -- proved on hand-built text, because the subject contains
# only one of them and cannot distinguish a working counter from a half-blind one.
$twoStyles = '$failed += ' + [char]39 + $freshnessName + [char]39 + [char]10 +
             '$other  += ' + [char]34 + $freshnessName + [char]34
Assert-True ((Measure-NameLiterals -Text $twoStyles -Name $freshnessName) -eq 2) `
    'the counter sees literals in BOTH quoting styles, so a second copy written with " cannot restore #822 unseen'
Assert-True ((Measure-NameLiterals -Text ('# ' + $freshnessName + ' in prose') -Name $freshnessName) -eq 0) `
    'CONTROL: an unquoted mention is not counted, so the count above is about literals and not about the words appearing at all'

$literalCount = Measure-NameLiterals -Text $gateText -Name $freshnessName
Assert-True ($literalCount -eq 1) `
    "the name is written ONCE in gate.ps1, so an arming site and a filter cannot drift apart (found $literalCount literal occurrences)"


# ARRANGEMENT first: a gate.ps1 that failed to parse would yield an empty slice, and every cell
# below would pass or fail about a program never read.
$parseErrors = $null
$gateAst = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors)
Assert-True ($parseErrors.Count -eq 0) 'ARRANGEMENT: gate.ps1 parses, so what follows is measured rather than empty'

$start = $gateText.IndexOf('function Get-GateVerdictLines {', [System.StringComparison]::Ordinal)
$end = $gateText.IndexOf('# (end #822)', [System.StringComparison]::Ordinal)
Assert-True ($start -ge 0 -and $end -gt $start) 'ARRANGEMENT: the decision was found between its anchors, so the subject exists'
. ([scriptblock]::Create($gateText.Substring($start, $end - $start)))

Write-Host ''
Write-Host '-- CONTROL: with fresh binaries the verdict reads exactly as it always did --' -ForegroundColor Cyan
$plain = @(Get-GateVerdictLines -FreshnessStageName $freshnessName -Failed @('workspace tests', 'clippy (deny warnings)') -StaleBinaryCount 0)
Assert-True ($plain.Count -eq 1) "one line for an ordinary red (got $($plain.Count))"
Assert-True ($plain[0] -ceq '[gate] RED - failed stages: workspace tests, clippy (deny warnings)') "and it is the unchanged wording (got '$($plain[0])')"

Write-Host ''
Write-Host '-- the cross-check is the FRAME, never a peer --' -ForegroundColor Cyan
$stale = @(Get-GateVerdictLines -FreshnessStageName $freshnessName -Failed @('workspace tests', $freshnessName) -StaleBinaryCount 41)
Assert-True ($stale.Count -eq 2) "two lines: the frame, then the reds read inside it (got $($stale.Count))"
Assert-True ($stale[0].StartsWith('[gate] NOT A MEASUREMENT: 41 test binary(ies)', [System.StringComparison]::Ordinal)) "the first line names the condition and the count (got '$($stale[0])')"
Assert-True ($stale[0].IndexOf('DIFFERENT PROGRAM', [System.StringComparison]::Ordinal) -ge 0) 'and says the stages measured a different program'
Assert-True ($stale[0].IndexOf('not readable as results for this head', [System.StringComparison]::Ordinal) -ge 0) 'and says what that does to the sibling results'
Assert-True ($stale[1].IndexOf('workspace tests', [System.StringComparison]::Ordinal) -ge 0) 'the second line still lists the stage that went red'
Assert-True ($stale[1].IndexOf('not findings about this head', [System.StringComparison]::Ordinal) -ge 0) 'and says that red is not a finding about this head'
Assert-True (@($stale | Where-Object { $_.IndexOf($freshnessName, [System.StringComparison]::Ordinal) -ge 0 }).Count -eq 0) 'THE DEFECT: the cross-check never appears as a peer entry in any line'
Assert-True (@($stale | Where-Object { $_.StartsWith('[gate] RED - failed stages:', [System.StringComparison]::Ordinal) }).Count -eq 0) 'and the peer-list form is not printed at all when the run was not a measurement'

Write-Host ''
Write-Host '-- stale binaries and nothing else red: still RED, and it says why --' -ForegroundColor Cyan
$only = @(Get-GateVerdictLines -FreshnessStageName $freshnessName -Failed @($freshnessName) -StaleBinaryCount 3)
Assert-True ($only.Count -eq 2 -and $only[1] -ceq '[gate] RED - no stage failed, and none of them measured this head.') "no stage failed, none measured this head (got '$($only[-1])')"

Write-Host ''
Write-Host '-- the gate USES the decision, rather than repeating the old join --' -ForegroundColor Cyan
$calls = $gateAst.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -eq 'Get-GateVerdictLines'
    }, $true)
Assert-True ($calls.Count -ge 1) "gate.ps1 calls Get-GateVerdictLines (found $($calls.Count))"
Assert-True ($gateText.IndexOf("[gate] RED - failed stages: `$(`$failed -join ', ')", [System.StringComparison]::Ordinal) -lt 0) 'and the inline peer-list join is gone from the verdict block'
Assert-True ($gateText.IndexOf('$staleBinaryCount = $staleCount', [System.StringComparison]::Ordinal) -ge 0) 'and the cross-check feeds its count to the decision'

Write-Host ''
Write-Host '-- the cross-check message itself says what it does to its siblings --' -ForegroundColor Cyan
$message = [regex]::Match($gateText, 'FRESHNESS CROSS-CHECK: [^"]+')
Assert-True ($message.Success -and $message.Value.IndexOf('DIFFERENT PROGRAM', [System.StringComparison]::Ordinal) -ge 0) 'the printed cross-check names the different program'
Assert-True ($message.Success -and $message.Value.IndexOf('not readable as results for this head', [System.StringComparison]::Ordinal) -ge 0) 'and that the sibling results are not readable for this head'

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
