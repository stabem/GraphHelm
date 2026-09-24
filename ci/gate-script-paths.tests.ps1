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

# 26 IS THE EXACT NUMBER THIS SUITE RUNS, and every one of the 26 can fail. There is no longer a
# gap between the declared count and the live coverage.
#
# AND A CELL NOW HOLDS THIS SENTENCE TO THAT VARIABLE, because a count in prose is a claim nothing
# checks. Four times in one day an edit changed a number and left a sentence quoting the old one --
# twice inside the very commit that was fixing the previous instance. Grepping for every dependent
# sentence is the right habit and a human running it by hand misses an instance roughly every time.
#
# The cell is deliberately NARROW. It pins THIS sentence only, not every `N/N` in the file: the
# other eight are receipts of measurements taken at earlier heads, and they are correct as written.
# A guard that swept them would have to tell a receipt from a claim by vocabulary, which is a
# closed-vocabulary guard over prose -- it would age against the words it guards and redden on the
# next honest receipt. One sentence, pinned exactly, beats a sweep that must be argued with.
#
# A DECLARED GAP USED TO LIVE HERE and no longer does. One assertion was a tautology that could not
# fail, so the declared total exceeded the live coverage by one. That cell is deleted, and the
# declaration it needed went with it rather than standing as a claim about a file that had moved.
#
# The total is 29 again only because the pinning cell above is itself an assertion. It is not the
# old number returning, and the old gap is not back: every one of the 29 can fail, including that one.
#
# The habit it came from is worth keeping: state a limit HERE and not only in a commit message. A
# reader of `git log` finds that one; a reader of this file -- the person about to trust the number
# -- does not. Three reviewers went looking in the file after I claimed to have declared it, and
# none of them found it, because I had written it somewhere they were not looking.
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

# THE DECLARATION AT THE TOP OF THIS FILE IS NOW HELD TO THE VARIABLE IT DESCRIBES.
#
# The sentence "<n> IS THE EXACT NUMBER THIS SUITE RUNS" is a claim about $ExpectedAssertionCount
# written in prose, and prose is not checked by anything. Four times in one day an edit moved a
# count and left a sentence quoting the old one -- twice inside the commit that was fixing the
# previous instance, and once in a commit whose entire subject was documentation.
#
# The cell reads THIS FILE's own text rather than trusting the author to have kept the two in step.
# It anchors on the sentence's distinctive phrase, so rewording the paragraph around it is free and
# changing the number is not.
$selfText = [System.IO.File]::ReadAllText($PSCommandPath)
$declaredMatch = [regex]::Match($selfText, '#\s*(\d+)\s+IS THE EXACT NUMBER THIS SUITE RUNS')
Assert-True -Condition ($declaredMatch.Success -and [int]$declaredMatch.Groups[1].Value -eq $ExpectedAssertionCount) `
    -Message "the count declared in this file's own prose matches `$ExpectedAssertionCount (prose says '$(if ($declaredMatch.Success) { $declaredMatch.Groups[1].Value } else { 'NO DECLARATION FOUND' })', variable says $ExpectedAssertionCount)"

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
# above once a sixth site appeared in it, so the files that actually spawn are named (the runner and
# merge-proof-from-main were retired on 2026-09-24, #1266).
$spawningFiles = @($spawnSites | ForEach-Object { $_.File } | Sort-Object -Unique)
$expectedSpawners = @('gate.ps1', 'run-ps-suites.ps1')
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
# `ci/gate-runner.ps1`'s hidden gate (the runner was retired on 2026-09-24). Both carried
# `-NoProfile`, so the RULE above governed them;
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
# PowerShell host, and all three shapes resolve (the positional one lived in the retired runner; a
# fixture below keeps it covered):
#
#   -FilePath 'powershell.exe'    a literal
#   Start-Process powershell      the first positional argument -- ci/gate-runner.ps1:335 (retired)
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
    -Message 'the host given POSITIONALLY resolves too -- the shape the retired gate runner used, and -FilePath alone would miss it'

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
Assert-True -Condition ($startProcessFiles -contains 'gate.ps1') `
    -Message "the gate's own stage spawn is IN the complement now (found: $($startProcessFiles -join ', '))"

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
# #902: THE OTHER WAY THE WIRE BREAKS -- an argument PowerShell 5.1 mangles before the program ever
# sees it. `gh ... --jq '.headRefName + " " + .headRefOid'` reaches gh as TWO arguments:
#
#     accepts at most 1 arg(s), received 2      exit 1
#
# Measured on this machine. In ci/gate-runner.ps1 (retired 2026-09-24) that made EVERY queue entry unresolvable -- the
# runner reported "waiting: pull request not resolvable" for all of them, so the queue accepted work
# and delivered none, and the failure looked like patience rather than a defect (X, on #928).
#
# THE CAUSE IS THE EMBEDDED QUOTE, NOT THE SPACE, and the first version of this banner said
# otherwise. Passing each shape to a real native executable and printing $args on 5.1.26100.9444:
#
#     '[.headRefName, .headRefOid]|@tsv'   ->  ONE argument, unsplit           SAFE
#     '.headRefName + " " + .headRefOid'   ->  the " are STRIPPED, so the two  BREAKS
#                                              bare spaces then split it
#
# PowerShell quotes an argument containing spaces FOR you; it does not escape an embedded double
# quote. The example above breaks because of its quotes, and its spaces are a consequence of losing
# them -- which is why a whitespace predicate both refused safe filters and passed corrupting ones.
# The corrected account and its receipts are at the predicate below.
#
# WHAT TO WRITE INSTEAD, since a guard is only as good as this sentence:
#
#   `$ENV.name`    jq reads the value from the environment, so no quote crosses the command line.
#                  BYTE-IDENTICAL output -- the faithful escape, and the one to reach for:
#                      $env:SEP = ' '
#                      gh ... --jq '.headRefName + $ENV.SEP + .headRefOid'
#
#                  SET THE VARIABLE IN THE SAME SCOPE AS THE CALL. This is the one recommendation
#                  here with a QUIETER failure mode than the shape the guard forbids, and that is
#                  worth stating plainly rather than burying.
#
#                  With `$env:SEP` unset, jq's `$ENV.SEP` is null, `"a" + null` is `"a"`, and the
#                  separator VANISHES. Measured at the live call site with nothing defined:
#
#                      issue-902-queue-deliversb6ad1616...     EXIT=0
#
#                  Two strings run together, no error, no non-zero exit, and THIS GUARD GREEN --
#                  because the hazard is at runtime and the guard reads source text.
#
#                  (No tally here on purpose. A count in prose goes stale on the next cell added or
#                  removed, and nothing connects the two; the outcome -- green -- is the durable
#                  half and it is true at every count.)
#
#                  Compare the `\"` form this guard REJECTS, which SUCCEEDS -- measured by passing
#                  each shape to a native executable and printing $args:
#
#                      '.a+\"-\"+.b'   ->  [.a+"-"+.b]   INTACT,  exit 0
#                      '.a+"-"+.b'     ->  [.a+-+.b]     quotes stripped
#
#                  So the guard forbids the form that WORKS and recommends one that can corrupt
#                  silently. That is the trade this paragraph exists to make visible, and an earlier
#                  version of these lines said the opposite -- that `\"` failed loudly with
#                  `accepts at most 1 arg(s), received 2`. It does not; the BARE form does, after
#                  its quotes are stripped and the bare spaces split it. A reviewer caught the
#                  inversion, in the commit whose whole subject was this trade.
#   `@tsv`         passes the guard, but CHANGES THE OUTPUT: the delimiter becomes a tab, not a
#                  space. Fine when nothing downstream parses the separator; silent data change when
#                  something does. Named here because this file used to prescribe it without saying so.
#   `\"` escaping  works against real gh and reaches jq byte-identical -- but THIS GUARD REJECTS
#                  IT, measured by planting it at ci/merge-proof.ps1:633 (since retired). Known limitation, not an
#                  oversight: the scanner reads source TEXT and cannot tell an escaped quote from a
#                  bare one.
#
#                  The count that receipt used to quote is deliberately gone. It quoted a tally that
#                  went wrong the moment a cell was deleted -- and the sentence explaining that went
#                  stale itself on the very next count change, which is the joke and also the point.
#                  A receipt embedded in prose decays on every count change and nothing connects the
#                  two. The line number and the outcome are the durable half;
#                  the tally was the perishable half, and it is not worth carrying.
#
# The population is DERIVED, like the paths above: every static `--jq`/`-q` argument in every
# ci/*.ps1, so the next one somebody writes is covered without anyone extending a list.
function Add-StaticArgumentTokens {
    param(
        [Parameter(Mandatory)] $Node,
        [System.Collections.Generic.List[object]] $Tokens
    )
    if ($Node -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
        $null = $Tokens.Add([pscustomobject]@{ Known = $true; Value = [string]$Node.Value })
    } elseif ($Node -is [System.Management.Automation.Language.CommandParameterAst]) {
        # BARE `-q`, which is the spelling everybody actually writes. PowerShell parses a single-dash
        # token as a CommandParameterAst, NOT as a string constant, so before this branch `gh ... -q
        # '<filter>'` was invisible to the scanner however the matcher was spelled -- and the
        # population silently fell by one rather than reporting a miss. `--jq` survives as a bare word
        # only because `--` cannot begin a parameter name, which is why the long form looked fine.
        #
        # Emitted with its dash restored so the caller compares like with like. `-q:'<filter>'` binds
        # the value to the parameter instead of leaving it as the next element, so that form is
        # unpacked here too rather than losing the filter.
        $null = $Tokens.Add([pscustomobject]@{ Known = $true; Value = "-$($Node.ParameterName)" })
        if ($null -ne $Node.Argument) { Add-StaticArgumentTokens -Node $Node.Argument -Tokens $Tokens }
    } elseif ($Node -is [System.Management.Automation.Language.ArrayLiteralAst]) {
        foreach ($child in @($Node.Elements)) { Add-StaticArgumentTokens -Node $child -Tokens $Tokens }
    } elseif ($Node -is [System.Management.Automation.Language.ArrayExpressionAst]) {
        foreach ($statement in @($Node.SubExpression.Statements)) {
            foreach ($pipelineElement in @($statement.PipelineElements)) {
                if ($pipelineElement -is [System.Management.Automation.Language.CommandExpressionAst]) {
                    Add-StaticArgumentTokens -Node $pipelineElement.Expression -Tokens $Tokens
                } else {
                    $null = $Tokens.Add([pscustomobject]@{ Known = $false; Value = $null })
                }
            }
        }
    } else {
        $null = $Tokens.Add([pscustomobject]@{ Known = $false; Value = $null })
    }
}

function Get-StaticExternalArgumentTokens {
    param([Parameter(Mandatory)] $Command)
    $elements = @($Command.CommandElements)
    $firstArgument = $null
    if ([string]::Equals([string]$Command.GetCommandName(), 'gh', [System.StringComparison]::OrdinalIgnoreCase)) {
        $firstArgument = 1
    } elseif ([string]::Equals([string]$Command.GetCommandName(), 'Invoke-External', [System.StringComparison]::OrdinalIgnoreCase) -and $elements.Count -gt 1 `
        -and $elements[1] -is [System.Management.Automation.Language.StringConstantExpressionAst] `
        -and [string]::Equals([string]$elements[1].Value, 'gh', [System.StringComparison]::OrdinalIgnoreCase)) {
        $firstArgument = 2
    }
    if ($null -eq $firstArgument) { return }
    $tokens = New-Object System.Collections.Generic.List[object]
    for ($i = $firstArgument; $i -lt $elements.Count; $i++) {
        Add-StaticArgumentTokens -Node $elements[$i] -Tokens $tokens
    }
    return ,$tokens
}

function Find-StaticJqArguments {
    # $ExcludePath is GONE, not merely unused. It had one caller, which passed $PSCommandPath to hide
    # this file from its own guard; three arms showed that hid real offenders and prevented nothing,
    # so the caller went. Leaving the parameter behind would leave the next author a documented way
    # to re-open the blind spot -- a retired mechanism that still works is an invitation, not dead code.
    param(
        [Parameter(Mandatory)] [string] $Directory
    )
    $arguments = New-Object System.Collections.Generic.List[object]
    $parseErrors = New-Object System.Collections.Generic.List[string]
    foreach ($file in @(Get-ChildItem -LiteralPath $Directory -Filter '*.ps1' -File)) {
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
        foreach ($error in @($errors)) {
            $parseErrors.Add("$($file.Name):$($error.Extent.StartLineNumber): $($error.Message)")
        }
        foreach ($command in @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.CommandAst] }, $true))) {
            $values = Get-StaticExternalArgumentTokens -Command $command
            if ($null -eq $values) { continue }
            for ($i = 0; $i -lt $values.Count; $i++) {
                # BOTH SPELLINGS. `gh` accepts `-q` as well as `--jq`, and the merge checklist (retired
                # 2026-09-24) wrote `-q` five times with spaces and embedded quotes. Matching only the
                # long form left the identical hazard invisible under the short one.
                if ($values[$i].Known -and $values[$i].Value -cin @('--jq', '-q') -and $i + 1 -lt $values.Count `
                    -and $values[$i + 1].Known) {
                    $arguments.Add([pscustomobject]@{
                        File = $file.Name
                        Line = $command.Extent.StartLineNumber
                        # THE SPELLING AS WRITTEN, so the diagnostic quotes the source. Hard-coding
                        # `--jq` in the message sent an author of a `-q` site looking for text their
                        # file does not contain -- a report that is true about the hazard and wrong
                        # about where to find it.
                        Flag = [string]$values[$i].Value
                        Filter = [string]$values[$i + 1].Value
                    })
                }
            }
        }
    }
    if ($parseErrors.Count -gt 0) { $arguments.Clear() }
    [pscustomobject]@{ Arguments = $arguments.ToArray(); ParseErrors = $parseErrors.ToArray() }
}

# THE SELF-EXEMPTION IS GONE, and it was measured out rather than argued out. `-ExcludePath
# $PSCommandPath` hid this whole file from its own guard. Three arms settle what it bought:
#
#   plant a real offender INSIDE the exempted file      invisible, 29/29
#   remove the exclusion, same plant                    CAUGHT, 28/29 EXIT=1
#   remove the exclusion, no plant                      still 29/29, no false positive
#
# So it hid real offenders and prevented nothing. The mechanism is that the predicate applies to
# `$_.Filter`, which only ever holds values the AST classifier already admitted as `gh` arguments --
# and this file's quote-bearing literals are comments, here-strings and `Assert-True` strings, which
# the classifier never admits. A first reading of this called the exclusion inert under the OLD
# whitespace predicate; the concern that the new `"` predicate would make it load-bearing was
# reasonable and was re-measured, not assumed, and it is not.
$jqScan = Find-StaticJqArguments -Directory $PSScriptRoot
# THE CELL THAT USED TO SIT HERE IS GONE, and removing it is the point rather than tidying.
#
# It asserted that a list built by filtering out $PSCommandPath contains no $PSCommandPath -- a
# tautology that could not fail -- and its MESSAGE said "the jq population excludes this test file".
# That message is now FALSE: the exclusion was deleted a few lines above, because three arms showed
# it hid real offenders and bought nothing.
#
# So the fix for one false in-file claim created another, in the same file, within one commit. That
# is the whole lesson of the last two rounds arriving a third time: when a claim goes stale, grep for
# every sentence that depended on it rather than editing the one you were shown. A reviewer found
# this one; I did not.
#
# $ExpectedAssertionCount drops 29 -> 28 with it, which also retires the "29 against 28 live cells"
# declaration -- the count and the live cells are now the same number.
Assert-True -Condition ($jqScan.ParseErrors.Count -eq 0) `
    -Message "every production PowerShell file parsed before jq arguments were classified (errors: $($jqScan.ParseErrors -join '; '))"
$jqTotal = $jqScan.Arguments.Count
# THE PREDICATE MEASURED THE WRONG PROPERTY, and it was wrong in BOTH directions. Measured on
# Windows PowerShell 5.1.26100.9444 by passing each shape to a native executable and printing $args:
#
#   '[.headRefName, .headRefOid]|@tsv'   arrives as ONE argument, unsplit -- SAFE, and the old
#                                        whitespace predicate REFUSED it
#   '.a+"-"+.b'                          arrives as  .a+-+.b  -- the quotes are STRIPPED, silently
#                                        corrupting the filter, and the old predicate passed it GREEN
#
# PowerShell quotes an argument containing spaces for you; it does NOT escape an embedded double
# quote. So whitespace is not the hazard and never was -- the guard blocked correct code and waved
# through the shape that actually breaks, which is assurance pointing the wrong way.
#
# ONE PREDICATE, THREE CONSUMERS -- and that is the point of the function, not tidiness. The two
# discriminating controls below kept their OWN private copy of the old `-match '\s'`, so the
# shipped predicate could be replaced with `-match 'ZZZNEVERMATCHES'` and the suite still passed
# 29/29: nothing in the file covered the thing the file exists to enforce. Both reviewing lanes
# found that independently, by different routes.
#
# CALLING THIS FUNCTION WAS NOT SUFFICIENT, and the first version of this comment said it was.
# A reviewer measured what it actually bought:
#
#   never-match sabotage          27/29  EXIT=1   the controls react
#   revert this predicate to '\s' 29/29  GREEN    the controls do NOT
#
# Because every control fixture carried quotes AND whitespace, the old and the new predicate scored
# IDENTICALLY on all of them. So the correction this file exists to make could be reverted with
# nothing going red, while this comment told the next reader the opposite. That is the same defect
# class a BLOCK was raised over on #992 three PRs ago -- a document asserting a coverage it does not
# have -- and it does not become a smaller defect because the author of it is me.
#
# The discriminator is a fixture carrying QUOTES WITHOUT WHITESPACE: an offender under `"` and not
# an offender under `\s`. With it in the population, reverting the predicate changes the count and
# the control fails. That fixture is the `.a+"-"+.b` line below, and the counts it moves are named
# in the assertion so a future edit cannot quietly drop it.
function Test-JqFilterHazardous {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Filter)
    return $Filter -match '"'
}
$jqOffenders = @($jqScan.Arguments | Where-Object { Test-JqFilterHazardous -Filter $_.Filter } |
    ForEach-Object { "$($_.File):$($_.Line)  $($_.Flag) '$($_.Filter)'" })
# NO ARRANGEMENT CELL SINCE #1266. Every production --jq/-q site lived in the runner, queue and
# merge-proof scripts retired on 2026-09-24, so the real population is now EMPTY and the rule below
# holds vacuously today; it governs the next site anyone writes. That the scanner still SEES the
# shape is proven by the discriminating controls below (multiline, mixed-case, quoted-data), which
# run it against planted files rather than against the tree. Measured population now: $jqTotal.
Assert-True -Condition ($jqOffenders.Count -eq 0) `
    -Message "no single-quoted --jq or -q argument contains a double quote, which PowerShell 5.1 strips from the native command line before gh sees it; write it as `$ENV.<name> to keep the output byte-identical ($($jqOffenders.Count) offender(s): $($jqOffenders -join '; '))"

# Discriminating multiline control: run the same scanner against a temporary production file. It
# covers both direct `gh` and the helper's static `Invoke-External 'gh'` array, while comments stay
# outside the command AST.
$multiRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-jq-$PID"
New-Item -ItemType Directory -Path $multiRoot -Force | Out-Null
try {
    Set-Content -LiteralPath (Join-Path $multiRoot 'multiline.ps1') -Encoding utf8 -Value @"
# gh --jq 'bad comment text'
gh pr view 1 -R owner/repo @(
    '--jq', '.headRefName + " " + .headRefOid'
)
Invoke-External 'gh' @(
    'api',
    '--jq', '.owner + " " + .repo'
)
gh pr view 2 -R owner/repo '--jq' '.headRefName'
gh pr view 3 -R owner/repo '--jq' '.a+"-"+.b'
'quoted data: gh --jq ''.fake + " " + .value'''
"@
    $multiScan = Find-StaticJqArguments -Directory $multiRoot
    # THE SHIPPED PREDICATE, not a private copy of it (see Test-JqFilterHazardous). This control kept
    # its own `-match '\s'`, which is why the production predicate could be deleted outright without
    # the suite noticing.
    #
    # THE DISCRIMINATING FIXTURE IS `.a+"-"+.b`: quotes, NO whitespace. Every other fixture here
    # carries both, so every other fixture scores identically under `"` and under `\s` -- which is
    # why calling the shared function was not by itself enough to pin the predicate. This one is an
    # offender under the shipped predicate and NOT an offender under the retired one, so reverting
    # `\s` drops the offender count to 2 and this assertion fails.
    #
    # It is also the exact shape measured to corrupt at runtime: `.a+"-"+.b` reaches jq as `.a+-+.b`.
    $multiOffenders = @($multiScan.Arguments | Where-Object { Test-JqFilterHazardous -Filter $_.Filter })
    Assert-True -Condition ($multiScan.ParseErrors.Count -eq 0 -and $multiScan.Arguments.Count -eq 4 -and $multiOffenders.Count -eq 3) `
        -Message "the production scanner observes three hazardous and one clean direct/helper argument, and the quotes-without-whitespace fixture is what makes this count depend on the shipped predicate rather than the retired one (arguments=$($multiScan.Arguments.Count), offenders=$($multiOffenders.Count), errors=$($multiScan.ParseErrors.Count))"
    Assert-True -Condition (@($multiScan.Arguments | Where-Object { $_.Filter -eq '.fake + " " + .value' }).Count -eq 0) `
        -Message 'quoted data containing gh --jq text is not classified as a command argument'
} finally {
    Remove-Item -LiteralPath $multiRoot -Recurse -Force -ErrorAction SilentlyContinue
}

# Command names are case-insensitive in PowerShell. Keep this control in the same production
# scanner path so a case-sensitive AST guard cannot silently lose mixed-case direct/helper calls.
$mixedRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-jq-mixed-$PID"
New-Item -ItemType Directory -Path $mixedRoot -Force | Out-Null
try {
    Set-Content -LiteralPath (Join-Path $mixedRoot 'mixed.ps1') -Encoding utf8 -Value @"
GH pr view 1 @('--jq', '.headRefName + " " + .headRefOid')
iNvOkE-ExTeRnAl 'GH' @('--jq', '.owner + " " + .repo')
"@
    $mixedScan = Find-StaticJqArguments -Directory $mixedRoot
    $mixedOffenders = @($mixedScan.Arguments | Where-Object { Test-JqFilterHazardous -Filter $_.Filter })
    Assert-True -Condition ($mixedScan.ParseErrors.Count -eq 0 -and $mixedScan.Arguments.Count -eq 2 -and $mixedOffenders.Count -eq 2) `
        -Message "the production scanner recognizes mixed-case direct gh and static Invoke-External gh arguments (arguments=$($mixedScan.Arguments.Count), offenders=$($mixedOffenders.Count), errors=$($mixedScan.ParseErrors.Count))"
} finally {
    Remove-Item -LiteralPath $mixedRoot -Recurse -Force -ErrorAction SilentlyContinue
}

# A parse error must refuse the whole population rather than let a partial AST produce a green zero.
$brokenRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-jq-broken-$PID"
New-Item -ItemType Directory -Path $brokenRoot -Force | Out-Null
try {
    Set-Content -LiteralPath (Join-Path $brokenRoot 'broken.ps1') -Encoding utf8 -Value "gh pr view 1 @('--jq', 'unterminated"
    $brokenScan = Find-StaticJqArguments -Directory $brokenRoot
    Assert-True -Condition ($brokenScan.ParseErrors.Count -gt 0 -and $brokenScan.Arguments.Count -eq 0) `
        -Message "a production parse error refuses the jq population (errors=$($brokenScan.ParseErrors.Count), arguments=$($brokenScan.Arguments.Count))"
} finally {
    Remove-Item -LiteralPath $brokenRoot -Recurse -Force -ErrorAction SilentlyContinue
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
