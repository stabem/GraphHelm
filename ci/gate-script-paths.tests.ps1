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

$ExpectedAssertionCount = 21
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
#
# #963: `Start-Process` IS one of those spawns, and the complement skipped every one of them.
# `GetCommandName()` returns `Start-Process`, never the host it launches, so a name filter alone
# never saw `ci/gate.ps1`'s stage spawn -- the single most important spawn in this repository -- nor
# `ci/gate-runner.ps1`'s hidden gate. Both carry `-NoProfile` today, so the RULE above governs them;
# take that token away and, before this widening, nothing did. Measured, with a seventh spawn site
# present so the vacuity floor kept its count of six:
#
#   scan 1 (keys on the -NoProfile token)   0 sites    -> not in the population
#   scan 2 (keys on GetCommandName)         0 matched  -> not seen
#   the suite                               14/14 GREEN, stage spawn ungoverned
#
# THE TARGET IS RESOLVED, NOT ASSUMED, and that is the half that keeps this from swallowing the
# tree. `ci/` legitimately starts non-PowerShell processes -- `ci/postgres.ps1` spawns `pg_ctl` in
# this exact shape -- and a widening that counted every `Start-Process` would end up demanding
# `-NoProfile` from `pg_ctl`. So a `Start-Process` is a spawn only when its target RESOLVES to a
# PowerShell host, and all three shapes in this tree resolve:
#
#   -FilePath 'powershell.exe'    a literal
#   Start-Process powershell      the first positional argument -- ci/gate-runner.ps1:335
#   -FilePath $hostExe            a variable, followed to its assignment, which in ci/gate.ps1 is
#                                 `if (...) { 'pwsh' } else { 'powershell' }`: two constants, and
#                                 one branch naming a host is enough
#
# A target that resolves to NOTHING is not a spawn. That is the conservative direction deliberately:
# an unresolvable variable is unknown, and a cell that fired on unknown would fire on `$pgCtl`.
#
# The positional shape is read only at element 1, immediately after the command name.
# `Start-Process -Verb runas powershell` is therefore MISSED rather than mis-resolved to `runas`.
# That shape is not in this tree, and a narrow answer is better than a wrong one here, because the
# wrong one is what gets an exclusion list written for it.

function Resolve-TargetNames {
    param($Expression, $Ast)
    $names = New-Object System.Collections.Generic.List[string]
    if ($null -eq $Expression) { return $names.ToArray() }
    if (($Expression -is [System.Management.Automation.Language.StringConstantExpressionAst]) -or
        ($Expression -is [System.Management.Automation.Language.ExpandableStringExpressionAst])) {
        $names.Add([string] $Expression.Value)
    } elseif ($Expression -is [System.Management.Automation.Language.VariableExpressionAst]) {
        # Every assignment to that name, and every string constant anywhere inside it. A conditional
        # assignment yields one constant per branch, which is exactly ci/gate.ps1's `$hostExe`, and
        # one branch naming a host is enough -- the run takes one of them.
        $wanted = '$' + $Expression.VariablePath.UserPath
        foreach ($assignment in $Ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] }, $true)) {
            if ($assignment.Left.Extent.Text -ne $wanted) { continue }
            foreach ($constant in $assignment.Right.FindAll({ param($n) $n -is [System.Management.Automation.Language.StringConstantExpressionAst] }, $true)) {
                $names.Add([string] $constant.Value)
            }
        }
    }
    return $names.ToArray()
}

function Test-TargetIsPowerShellHost {
    param([string[]] $Names)
    foreach ($name in @($Names)) {
        if ([string]::IsNullOrWhiteSpace($name)) { continue }
        # The LEAF, so a fully qualified path to the host still counts and a bare name is its own
        # leaf. GetFileName throws on invalid path characters, and an argument that cannot be a path
        # is not a host either.
        $leaf = $name
        try { $leaf = [System.IO.Path]::GetFileName($name) } catch { $leaf = $name }
        if ($leaf -match '^(powershell|pwsh)(\.exe)?$') { return $true }
    }
    return $false
}

function Get-StartProcessTargetExpression {
    param($Command)
    $elements = @($Command.CommandElements)
    for ($i = 1; $i -lt $elements.Count; $i++) {
        $element = $elements[$i]
        if ($element -is [System.Management.Automation.Language.CommandParameterAst]) {
            # PowerShell binds parameters by unambiguous prefix and `-File` is one for Start-Process,
            # so the name is matched as a prefix rather than spelled out in full.
            $parameterName = [string] $element.ParameterName
            if (($parameterName.Length -ge 2) -and
                ('FilePath'.StartsWith($parameterName, [System.StringComparison]::OrdinalIgnoreCase))) {
                if ($null -ne $element.Argument) { return $element.Argument }
                if (($i + 1) -lt $elements.Count) { return $elements[$i + 1] }
            }
            continue
        }
        if ($i -eq 1) { return $element }
    }
    return $null
}

<#
.SYNOPSIS
    Every command in one script that starts a PowerShell host, and whether it carries -NoProfile.

.DESCRIPTION
    Takes TEXT rather than a path so a fixture can drive it. The real-tree cells below are the only
    ones that read this checkout: without them the widening would be measured only against strings
    written in this file, and without the fixtures an empty result would be indistinguishable from a
    matcher that can no longer find anything.
#>
function Get-HostSpawnCommands {
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $Text,
        [Parameter(Mandatory)] [string] $Name
    )
    $tokens = $null
    $errors = $null
    $fileAst = [System.Management.Automation.Language.Parser]::ParseInput($Text, [ref]$tokens, [ref]$errors)
    $found = New-Object System.Collections.Generic.List[object]

    foreach ($command in $fileAst.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true)) {
        $commandName = $command.GetCommandName()
        if (-not $commandName) { continue }

        $kind = $null
        if ($commandName -match '^(powershell|pwsh)(\.exe)?$') {
            $kind = 'named'
        } elseif ($commandName -match '^Start-Process$') {
            $target = Get-StartProcessTargetExpression -Command $command
            if (Test-TargetIsPowerShellHost -Names (Resolve-TargetNames -Expression $target -Ast $fileAst)) {
                $kind = 'start-process'
            }
        }
        if (-not $kind) { continue }

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

        $found.Add([pscustomobject]@{
            Name         = $Name
            Line         = $command.Extent.StartLineNumber
            Kind         = $kind
            HasNoProfile = ($argumentText -match '(?<![A-Za-z])-NoProfile(?![A-Za-z])')
        })
    }
    # `.ToArray()`, NOT the usual `return , ([object[]] $found)`. The comma idiom stops a
    # ONE-element result being unwrapped, and it is correct where the caller ASSIGNS the result --
    # `ci/required-features.ps1` does exactly that and reads 0 for an empty population, measured.
    # It breaks where the caller wraps in `@()`, which is how every cell below calls this: `@()`
    # enumerates the pipeline, the pipeline emits the wrapped array as one object, and the count is
    # 1. An empty result becomes one row that does not exist.
    #
    #   comma idiom, empty    $x = f  -> 0     @(f).Count = 1   <- the row it invents
    #   .ToArray(), empty     $x = g  -> 0     @(g).Count = 0
    #   either one, ONE element                @(f).Count = 1   <- why the idiom looks correct
    #
    # The row it invented was the negative control's: a Start-Process that targets no host must
    # produce NOTHING, and it produced one empty something instead. `.ToArray()` is right under
    # both call shapes, so the collector does not depend on how it is read.
    return $found.ToArray()
}

# THE MATCHER FIRST, against fixtures, because once this lands the real tree is clean and a clean
# result proves nothing about whether the matcher can still find anything.

$literalSpawn = @(Get-HostSpawnCommands -Name 'fixture.ps1' -Text "Start-Process -FilePath 'powershell.exe' -ArgumentList @('-File', `$x) -PassThru")
Assert-True -Condition (($literalSpawn.Count -eq 1) -and (-not $literalSpawn[0].HasNoProfile)) `
    -Message 'a Start-Process spawning a literal powershell.exe with no -NoProfile is REPORTED (this is #963)'

# THE NEGATIVE CONTROL, and it is the half that would bite: ci/ starts real non-PowerShell
# processes, and a widening that caught them would demand -NoProfile from pg_ctl.
$foreignSpawn = @(Get-HostSpawnCommands -Name 'fixture.ps1' -Text "Start-Process -FilePath 'robocopy.exe' -ArgumentList @('a', 'b') -Wait")
Assert-True -Condition ($foreignSpawn.Count -eq 0) `
    -Message 'and a Start-Process whose target is not a host is not in the population at all'

$positionalSpawn = @(Get-HostSpawnCommands -Name 'fixture.ps1' -Text "`$proc = Start-Process powershell -ArgumentList '-ExecutionPolicy', 'Bypass', '-Command', `$inner -PassThru")
Assert-True -Condition (($positionalSpawn.Count -eq 1) -and (-not $positionalSpawn[0].HasNoProfile)) `
    -Message 'the host given POSITIONALLY resolves too -- that is ci/gate-runner.ps1 shape, and -FilePath alone would miss it'

# ci/gate.ps1's own shape, reduced: the target is a variable whose assignment is a conditional, so
# resolving it means reading both branches.
$variableSpawnText = @(
    "`$hostExe = if (`$PSVersionTable.PSEdition -eq 'Core') { 'pwsh' } else { 'powershell' }",
    "`$process = Start-Process -FilePath `$hostExe -PassThru -NoNewWindow -ArgumentList @('-File', `$ScriptPath)"
) -join [System.Environment]::NewLine
$variableSpawn = @(Get-HostSpawnCommands -Name 'fixture.ps1' -Text $variableSpawnText)
Assert-True -Condition (($variableSpawn.Count -eq 1) -and (-not $variableSpawn[0].HasNoProfile)) `
    -Message 'a target reached through a variable and a conditional resolves -- this is ci/gate.ps1:448 with the flag taken out'

# AND THE CONTROL FOR THE FLAG ITSELF. Without it, a widening that reported every resolved spawn as
# unflagged would satisfy all four cells above.
$compliantSpawn = @(Get-HostSpawnCommands -Name 'fixture.ps1' -Text "Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-NonInteractive', '-File', `$x)")
Assert-True -Condition (($compliantSpawn.Count -eq 1) -and $compliantSpawn[0].HasNoProfile) `
    -Message 'a compliant Start-Process is in the population AND reads as flagged -- the cells above are about the flag, not about Start-Process'

# ---- and only now the real tree.

$hostSpawns = New-Object System.Collections.Generic.List[object]
foreach ($file in $productionScripts) {
    foreach ($row in (Get-HostSpawnCommands -Name $file.Name -Text ([System.IO.File]::ReadAllText($file.FullName)))) {
        $hostSpawns.Add($row)
    }
}

# THE JUNCTION. The fixtures prove the matcher can see this shape; the rule below proves the tree is
# clean. Neither says the matcher is pointed AT the two spawns #963 is about. This does.
$startProcessSpawns = @($hostSpawns | Where-Object { $_.Kind -eq 'start-process' })
$startProcessFiles = @($startProcessSpawns | ForEach-Object { $_.Name } | Sort-Object -Unique)
Assert-True -Condition (($startProcessFiles -contains 'gate.ps1') -and ($startProcessFiles -contains 'gate-runner.ps1')) `
    -Message "the gate's own stage spawn and the runner's hidden gate are IN the complement now (found: $($startProcessFiles -join ', '))"

Assert-True -Condition (@($startProcessSpawns | Where-Object { $_.Name -eq 'postgres.ps1' }).Count -eq 0) `
    -Message 'and pg_ctl is not: ci/postgres.ps1 starts a non-PowerShell process in the same shape and must stay out'

$spawnsMissingNoProfile = @($hostSpawns | Where-Object { -not $_.HasNoProfile } | ForEach-Object { "$($_.Name):$($_.Line)" })
Assert-True -Condition ($spawnsMissingNoProfile.Count -eq 0) `
    -Message "every invocation that starts a PowerShell host carries -NoProfile, so none escapes the rule above (bare at: $($spawnsMissingNoProfile -join ', '))"

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
