# #643: the gate hands script paths to `powershell -File`. A path that does not resolve makes the
# stage die at exit 127 -- loud, but only on a FULL gate run, which is the one thing nobody does
# while iterating. This suite reads those paths straight out of ci/gate.ps1 and resolves them, so a
# typo in the wire is caught by a suite that costs seconds instead of by a gate that costs an hour.
#
# It exists because a real one shipped: `'ci\run-ps-suites.ps1'` was written through a layer that
# interprets escapes, `\r` became a literal CARRIAGE RETURN, and the stage pointed at
# `ci<CR>un-ps-suites.ps1`. Five cells had verified the runner and not one had verified the WIRE.
#
# The population is DERIVED from the file, never a hand list: a hand list is born correct and rots
# on the next site somebody adds, which is the same defect one generation later.

$ExpectedAssertionCount = 14
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

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# Two shapes, because the gate uses both and they never share a token:
#   -File (Join-Path $repositoryRoot 'ci/run-ps-suites.ps1')   <- the stage invocation
#   -File ci/gate.ps1                                          <- bare, in the module documentation
# The two shapes are counted SEPARATELY, and that is the whole point of the split. Pooling them
# lets the documentation matches -- which are prose and can never break the gate -- keep the total
# non-zero while the EXECUTABLE match silently stops being observed. A count that pools a load-
# bearing population with a decorative one answers a question nobody asked. (Found by Codex on
# review of the first version, which pooled them.)
$executableSites = New-Object System.Collections.Generic.List[string]
foreach ($match in [regex]::Matches($gateText, "-File\s+\(Join-Path\s+\`$repositoryRoot\s+'([^']+)'\)")) {
    $executableSites.Add($match.Groups[1].Value)
}
$documentationSites = New-Object System.Collections.Generic.List[string]
foreach ($match in [regex]::Matches($gateText, "-File\s+([A-Za-z0-9_./\-]+\.ps1)")) {
    $documentationSites.Add($match.Groups[1].Value)
}
$sites = New-Object System.Collections.Generic.List[string]
$sites.AddRange($executableSites)
$sites.AddRange($documentationSites)

# The vacuity controls, first: every assertion below quantifies over a list, and every one of them
# is trivially true of an empty list. A regex that stops matching would otherwise report the gate
# as clean -- the loudest possible green for the emptiest possible measurement.
Assert-True -Condition ($executableSites.Count -gt 0) `
    -Message "the extraction found at least one EXECUTABLE -File site in ci/gate.ps1 (found $($executableSites.Count))"

Assert-True -Condition ($documentationSites.Count -gt 0) `
    -Message "the extraction found at least one documented -File path in ci/gate.ps1 (found $($documentationSites.Count))"

$controlChars = @($sites | Where-Object { $_ -match '[\x00-\x1f]' })
Assert-True -Condition ($controlChars.Count -eq 0) `
    -Message "no -File path carries a control character (offending: $($controlChars.Count))"

# Test-Path THROWS on a path containing a control character rather than returning $false, and a
# throw here kills the script mid-run: it exits 1 without ever reaching the assertion-count guard,
# so "the harness broke" arrives dressed as "an assertion failed". Measured on the real defect
# bytes. A path the filesystem refuses to even evaluate is a path that does not resolve.
$missing = @($sites | Where-Object {
        try { -not (Test-Path -LiteralPath (Join-Path $repositoryRoot $_)) } catch { $true }
    })
Assert-True -Condition ($missing.Count -eq 0) `
    -Message "every -File path in ci/gate.ps1 resolves to a file (missing: $($missing -join ', '))"


# ---------------------------------------------------------------------------------------------
# #903: THE RUNNER MUST ACTUALLY CALL THE SELECTOR. The scope machinery landed with #928 and for a
# day nothing invoked it: every manifest said `FULL: no scope selection was given`, which is the
# correct default AND the whole feature not running. A producer nobody calls is indistinguishable
# from no producer, and it is invisible because the safe default is also the silent one.
$runnerText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'gate-runner.ps1'))
Assert-True -Condition ($runnerText.IndexOf('ci/select-scope.ps1', [System.StringComparison]::Ordinal) -ge 0) `
    -Message 'the runner invokes ci/select-scope.ps1, so a derived scope reaches a real gate rather than sitting unused'
Assert-True -Condition ($runnerText.IndexOf('-ScopeSelection', [System.StringComparison]::Ordinal) -ge 0) `
    -Message 'and passes -ScopeSelection to the gate it launches'


# ---------------------------------------------------------------------------------------------
# #925: EVERY SPAWN CARRIES -NonInteractive, because a missing argument must FAIL, never PROMPT.
#
# A PowerShell script invoked without a value for one of its `[Parameter(Mandatory)]` inputs does
# not exit non-zero -- it asks for the value and waits. In a child the gate is waiting on, that has
# no colour: the stage does not redden, does not go green, it stops, and the run ends with no
# verdict at all. There are 130 `[Parameter(Mandatory)]` declarations across 17 production scripts
# under ci/; each is that hang waiting for the first caller that omits an argument.
#
# THE POPULATION COMES FROM THE PARSER, NOT FROM A REGEX, and that distinction is the whole reason
# these cells can exist. `-NoProfile` appears 18 times under ci/ and eleven of them are prose --
# `.EXAMPLE` blocks in comment-based help, a usage line built into a string. A text scan would
# demand the flag in documentation, and the natural fix for that noise is a hand-maintained
# exclusion list, which is the same defect one generation later. The tokenizer already knows the
# difference: a comment is ONE comment token and a here-string is ONE string token, so neither can
# ever equal the argument `-NoProfile`. Six sites survive that filter, and they are exactly the six
# real spawns.
#
# `*.tests.ps1` is excluded from the population deliberately, and the cells below prove that rather
# than assume it: this very file spawns PowerShell WITHOUT the flag on purpose, to measure the hang.
# A population that reached the suites would make this suite redden itself.

$productionScripts = @(
    Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -File |
        Where-Object { $_.Name -notlike '*.tests.ps1' } |
        Sort-Object -Property Name
)

# The smallest enclosing command or assignment -- never the first one FindAll returns, which is the
# OUTERMOST. `Invoke-Stage 'ci powershell suites' { & powershell ... }` is a command containing a
# command; taking the outer one would search a whole scriptblock for the flag and call a site
# covered because some unrelated neighbour inside the same block carried it.
function Get-SmallestEnclosingNode {
    param($Ast, $Extent)
    $candidates = @($Ast.FindAll({
        param($n)
        ($n -is [System.Management.Automation.Language.CommandAst] -or
         $n -is [System.Management.Automation.Language.AssignmentStatementAst]) -and
        $n.Extent.StartOffset -le $Extent.StartOffset -and $n.Extent.EndOffset -ge $Extent.EndOffset
    }, $true))
    if ($candidates.Count -eq 0) { return $null }
    $sorted = @($candidates | Sort-Object -Property { $_.Extent.EndOffset - $_.Extent.StartOffset })
    return $sorted[0]
}

function Get-SpawnArgumentSites {
    param([Parameter(Mandatory)] $Files)
    $sites = New-Object System.Collections.Generic.List[object]
    foreach ($file in $Files) {
        $tokens = $null
        $errors = $null
        $fileAst = [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
        foreach ($token in $tokens) {
            # Two shapes and no third: a bare parameter (-NoProfile) and a quoted element of an
            # argument array. A comment or here-string CONTAINING the word is a single token whose
            # text is the whole paragraph, so it matches neither.
            $isArgument = ($token.Text -eq '-NoProfile') -or
                (($token -is [System.Management.Automation.Language.StringToken]) -and ($token.Value -eq '-NoProfile'))
            if (-not $isArgument) { continue }
            $node = Get-SmallestEnclosingNode -Ast $fileAst -Extent $token.Extent
            $sites.Add([pscustomobject]@{
                File = $file.Name
                Line = $token.Extent.StartLineNumber
                Text = $(if ($node) { $node.Extent.Text } else { '' })
            })
        }
    }
    return $sites
}

$spawnSites = @(Get-SpawnArgumentSites -Files $productionScripts)

# VACUITY FIRST. Every assertion below quantifies over this list and every one of them is trivially
# true of an empty one. A tokenizer change, a renamed flag, a directory that stops being walked --
# each would report the gate as clean by measuring nothing.
Assert-True -Condition ($spawnSites.Count -ge 6) `
    -Message "the parser found at least 6 PowerShell spawn sites under ci/ (found $($spawnSites.Count))"

# THE POSITIVE CONTROL. A filter that silently narrowed to one file would still satisfy the floor
# above once a sixth site appeared in it, so the four files that actually spawn are named.
$spawningFiles = @($spawnSites | ForEach-Object { $_.File } | Sort-Object -Unique)
$expectedSpawners = @('gate-runner.ps1', 'gate.ps1', 'merge-proof-from-main.ps1', 'run-ps-suites.ps1')
$absentSpawners = @($expectedSpawners | Where-Object { $spawningFiles -notcontains $_ })
Assert-True -Condition ($absentSpawners.Count -eq 0) `
    -Message "every known spawning file is represented in the population (absent: $($absentSpawners -join ', '))"

# THE NEGATIVE CONTROL, and it is what makes the parser worth using. These two scripts carry
# -NoProfile in .EXAMPLE blocks of their comment-based help and spawn nothing. A text scan finds
# three matches across them; the parser must find none, or the rule below is demanding a flag in
# prose that nobody will ever execute.
$prosePopulation = @($spawnSites | Where-Object { @('normalize-script-eol.ps1', 'closing-keywords.ps1') -contains $_.File })
Assert-True -Condition ($prosePopulation.Count -eq 0) `
    -Message "documented example lines are not mistaken for spawns (found $($prosePopulation.Count) in help text)"

# AND THE SUITE DOES NOT MEASURE ITSELF. The behavioural cells below spawn PowerShell deliberately
# without the flag; if *.tests.ps1 ever entered the population this file would fail its own rule.
# Asserted from the file's own bytes rather than trusted to the filter above.
$ownText = [System.IO.File]::ReadAllText($PSCommandPath)
$ownSites = @($spawnSites | Where-Object { $_.File -eq (Split-Path -Leaf $PSCommandPath) })
Assert-True -Condition (($ownSites.Count -eq 0) -and ($ownText -match '(?<![A-Za-z])-NoProfile(?![A-Za-z])')) `
    -Message 'this suite spawns PowerShell without the flag on purpose and is itself outside the population'

# THE RULE. Every real spawn site must carry -NonInteractive alongside -NoProfile.
$unflagged = @($spawnSites | Where-Object { $_.Text -notmatch '(?<![A-Za-z])-NonInteractive(?![A-Za-z])' })
$unflaggedNames = @($unflagged | ForEach-Object { "$($_.File):$($_.Line)" })
Assert-True -Condition ($unflagged.Count -eq 0) `
    -Message "every PowerShell spawn under ci/ carries -NonInteractive (missing at: $($unflaggedNames -join ', '))"

# THE COMPLEMENT, so the rule cannot be escaped by dropping the token it keys on. A new spawn
# written without -NoProfile would leave the population above and be governed by nothing. Splatted
# invocations are followed to the array they splat: `& powershell.exe @arguments` carries its flags
# in an assignment several lines up, and that assignment is itself one of the sites checked above.
$spawnsMissingNoProfile = New-Object System.Collections.Generic.List[string]
foreach ($file in $productionScripts) {
    $tokens = $null
    $errors = $null
    $fileAst = [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
    foreach ($command in $fileAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true)) {
        $commandName = $command.GetCommandName()
        if (-not $commandName -or $commandName -notmatch '^(powershell|pwsh)(\.exe)?$') { continue }
        $argumentText = $command.Extent.Text
        foreach ($element in $command.CommandElements) {
            if (($element -is [System.Management.Automation.Language.VariableExpressionAst]) -and $element.Splatted) {
                foreach ($assignment in $fileAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] }, $true)) {
                    if ($assignment.Left.Extent.Text -eq ('$' + $element.VariablePath.UserPath)) {
                        $argumentText += [System.Environment]::NewLine + $assignment.Extent.Text
                    }
                }
            }
        }
        if ($argumentText -notmatch '(?<![A-Za-z])-NoProfile(?![A-Za-z])') {
            $spawnsMissingNoProfile.Add("$($file.Name):$($command.Extent.StartLineNumber)")
        }
    }
}
Assert-True -Condition ($spawnsMissingNoProfile.Count -eq 0) `
    -Message "every named powershell/pwsh invocation carries -NoProfile, so none escapes the rule above (bare at: $($spawnsMissingNoProfile -join ', '))"

# ---------------------------------------------------------------------------------------------
# AND THE FLAG HAS TO EARN ITS PLACE. The five assertions above are a spelling rule; on their own
# they would pass just as well if -NonInteractive did nothing at all. These two measure the
# behaviour the rule exists for, against a fixture, with no production file involved.
#
# THE BOUND IS WHAT KEEPS THIS CELL FROM BECOMING THE HANG IT TESTS FOR. The two waits are
# deliberately asymmetric: the flagged spawn exits in ~200 ms measured, so 15 s gives it 70x of
# headroom under a loaded gate, and the unflagged one has to still be running at 8 s -- 38x past
# the flagged time -- before this calls it a hang.
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("gate-script-paths-925-" + [System.Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $fixtureRoot
try {
    $fixture = Join-Path $fixtureRoot 'needs-an-argument.ps1'
    [System.IO.File]::WriteAllText($fixture, "param([Parameter(Mandatory)][string] `$Needed)`nexit 0`n")

    function Invoke-BoundedSpawn {
        param([Parameter(Mandatory)][string[]] $Flags, [Parameter(Mandatory)][int] $BoundMilliseconds)
        $arguments = @($Flags) + @('-File', $fixture)
        $process = Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -PassThru -WindowStyle Hidden
        try {
            $exited = $process.WaitForExit($BoundMilliseconds)
            $code = $(if ($exited) { $process.ExitCode } else { $null })
            return [pscustomobject]@{ Exited = $exited; Code = $code }
        } finally {
            # Unconditional: the whole point of the first arm is a child that does not end on its
            # own, and a suite that leaves one behind has added a stuck process to every gate.
            if (-not $process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                $null = $process.WaitForExit(5000)
            }
        }
    }

    $withoutFlag = Invoke-BoundedSpawn -Flags @('-NoProfile', '-ExecutionPolicy', 'Bypass') -BoundMilliseconds 8000
    Assert-True -Condition (-not $withoutFlag.Exited) `
        -Message 'WITHOUT -NonInteractive a missing Mandatory parameter does not fail: the child is still waiting at a prompt after 8 s'

    $withFlag = Invoke-BoundedSpawn -Flags @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass') -BoundMilliseconds 15000
    Assert-True -Condition ($withFlag.Exited -and ($withFlag.Code -ne 0)) `
        -Message "WITH -NonInteractive the same script fails instead of prompting (exited: $($withFlag.Exited), code: $($withFlag.Code))"
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}


Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
