# #676: the five measured failure modes of the copy-paste EOL recipe, each one a throwaway
# repository, each run twice -- once against the recipe that was drafted, once against
# ci/normalize-script-eol.ps1.
#
# The recipe's damage is asymmetric and that is what sets the bar for these cells: an operator who
# runs NOTHING keeps CRLF, which is visible and reversible; an operator who runs the wrong thing
# loses work, and in mode 5 loses a file outright. So every cell asserts TWO things -- that the
# recipe does the damage the issue measured, and that the program does not -- because "the program
# passed" says nothing unless the same fixture is known to break something.
#
# The fixtures are real git repositories rather than mocks: the whole subject is git's own
# interaction between a `text eol=lf` attribute, a stat-clean path and the index, and no mock of
# that would be evidence about it.
#
# Measured on git 2.47.1.windows.1 and Windows PowerShell 5.1.26100.9168. The version matters here:
# #676 records `git checkout --force` behaving differently on 2.43.0, so a cell that asserts what
# the RECIPE does is a claim about a git version and says so.

$ExpectedAssertionCount = 125
# 'Continue', not 'Stop'. This suite RUNS the failing recipe on purpose, and under Windows
# PowerShell 5.1 a native command's redirected stderr becomes a NativeCommandError that 'Stop'
# promotes to a terminating error -- so `git checkout` printing "did not match any file" would kill
# the suite at the exact cell whose subject is that message. gate.ps1's Invoke-Stage documents the
# same trap and takes the same way out: judge native commands by their exit code and their output,
# never by whether they wrote to stderr. The assertion-count guard below is what still catches a
# suite that dies early.
$ErrorActionPreference = 'Continue'
$script:total = 0
$script:skipped = 0
$script:skipReasons = @()
# A SUM CANNOT SEE A REDISTRIBUTION BETWEEN ITS TERMS, and neither can a COUNT of skips:
# turning a real `Assert-True` into a `Skip-Assertion` leaves `total + skipped` intact and
# produces a skip count identical to a legitimate one (measured on a sibling suite by a
# reviewing lane: 23 of 24, 1 skipped, rc=0, GREEN). What separates them is the REASON, so
# every legitimate skip site is declared here and the tail refuses anything else.
#
# WHAT THIS DOES NOT DO, so it is not read as stronger than it is: it does not catch a skip
# whose message was COPIED from a legitimate site, and it does not notice a legitimate site
# firing twice. The threat it is sized for is accidental degradation, which does not inherit
# a legal reason string; it is not a defence against someone deliberately forging one.
$AllowedSkipReasons = @(
    'and the ambiguity refusal is not exercised here: the volume, not the program, decided'
)
$script:failures = 0

function Test-Reported {
    <#
        Does the diagnosis name THIS PATH as one it would change?

        The assertions used to match `would normalise` anywhere in the output -- and the summary
        line prints `would normalise N file(s)` ALWAYS, zero included. So a classifier that decided
        every CRLF file was already LF passed the whole suite: the promise this program exists to
        keep could fail completely and the authoritative suite saw 65 of 65. Found by review, in
        the ten assertions written to catch exactly that.

        The per-path line is the one that carries a claim about a file. A count is not a claim
        about anything.
    #>
    param([Parameter(Mandatory)] [string] $Text, [Parameter(Mandatory)] [string] $Path)
    return $Text -cmatch ('would normalise\s+' + [regex]::Escape($Path))
}

function Observe {
    <#
        A measurement that is RECORDED but does not gate.

        The cells that assert what the RECIPE does are claims about a git version: #676 records
        `checkout --force` behaving differently on 2.43.0 than on the 2.47.1 this was measured on.
        Gating on them would make this suite -- which runs inside the gate -- fail on a developer's
        machine for a reason that has nothing to do with the change in front of them, and a gate
        that fails for unrelated reasons is one that gets switched off rather than fixed.

        So the recipe's behaviour is printed with the git version beside it, as context for the
        program's behaviour, and only the PROGRAM's behaviour is asserted. The evidence that the
        recipe does the damage lives in the pull request, measured once on a named version, which
        is where a claim about someone else's git belongs.
    #>
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $mark = if ($Condition) { 'as measured' } else { 'DIFFERS HERE' }
    $colour = if ($Condition) { 'DarkGray' } else { 'Yellow' }
    Write-Host "  note ($mark, $script:gitVersion): $Message" -ForegroundColor $colour
}

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


# The SUITE has to listen in UTF-8 too, for the same reason the program does: git and the child
# process write path bytes as UTF-8, and PowerShell decodes a native command's output with the
# CONSOLE's encoding. Without this the accented cell compared `café.sh` against `cafÃ©.sh` and
# failed -- the per-path predicate working correctly on a name the harness had mis-decoded.
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { }
$script:gitVersion = ((& git --version) -join ' ').Trim()
$programPath = Join-Path $PSScriptRoot 'normalize-script-eol.ps1'
$programText = [System.IO.File]::ReadAllText($programPath)

# THE ORDINAL HELPERS, EXTRACTED FROM THE PROGRAM AND PREPENDED TO EVERY SEAM THAT USES THEM.
#
# `Test-SameText` and `Test-InSet` sit above the functions the seams cut out, so an extraction
# anchored at one of those functions no longer carries its own dependency -- which is exactly what
# happened when they were introduced: nine cells went red at once, in three different seams, on a
# change that had not altered a single decision.
#
# Extracted rather than redefined here. A copy in this file would let a sabotage of the REAL
# `Test-SameText` -- ordinal back to `-eq`, say -- leave every seam cell green, which is the same
# hole as a suite holding its own copy of a keyword list.
$helperStart = $programText.IndexOf('function Test-SameText {')
$helperEnd = $programText.IndexOf('function Resolve-ScopePath {')
if ($helperStart -lt 0 -or $helperEnd -le $helperStart) {
    Write-Host 'HARNESS-BROKE: the ordinal helpers were not found between their anchors' -ForegroundColor Magenta
    exit 2
}
$OrdinalHelpers = $programText.Substring($helperStart, $helperEnd - $helperStart)
if (-not (Test-Path -LiteralPath $programPath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $programPath" -ForegroundColor Magenta
    exit 2
}

$Latin1 = [System.Text.Encoding]::GetEncoding(28591)
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-eol-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$script:fixtureCount = 0
$script:removedLocalConfigCalls = 0
$script:configProofBeforeBytes = $null

function ConvertTo-FixtureGitConfigPath {
    param([Parameter(Mandatory)] [string] $Path)
    $normalized = [System.IO.Path]::GetFullPath($Path).Replace('\', '/')
    return [string][char]34 + $normalized.Replace([string][char]34, '\"') + [string][char]34
}

function Write-FixtureLocalConfig {
    param([Parameter(Mandatory)] [string] $Repo)
    $configPath = Join-Path $Repo '.git\config'
    $existing = [System.IO.File]::ReadAllText($configPath)
    $newline = if ($existing.Contains("`r`n")) { "`r`n" } else { "`n" }
    $separator = if ($existing.Length -gt 0 -and -not $existing.EndsWith("`r`n") -and -not $existing.EndsWith("`n")) { $newline } else { '' }
    $hooksPath = ConvertTo-FixtureGitConfigPath (Join-Path $Repo '.no-hooks')
    $settings = @(
        '[user]'
        '    email = fixture@example.invalid'
        '    name = fixture'
        '[core]'
        '    autocrlf = false'
        "    hooksPath = $hooksPath"
        '[commit]'
        '    gpgSign = false'
        '[tag]'
        '    gpgSign = false'
    ) -join $newline
    [System.IO.File]::AppendAllText($configPath, $separator + $settings + $newline, $Utf8NoBom)
}

function Invoke-FixtureGit {
    param([Parameter(Mandatory)] [string[]] $Arguments)
    $output = @(& git @Arguments 2>&1)
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        $detail = (@($output | ForEach-Object { [string] $_ }) -join ' | ')
        throw "HARNESS-BROKE: fixture git failed (exit $code): git $($Arguments -join ' ') -- $detail"
    }
}

function New-Fixture {
    <#
        A repository in exactly the state this migration exists for: the attribute is in force, the
        index blob is LF, and the working tree still holds CRLF -- with `git status` CLEAN, which is
        why nothing tells the operator anything is wrong.
    #>
    param([Parameter(Mandatory)] [string] $Name, [string[]] $Files = @('a.sh'))

    $repo = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory($repo) | Out-Null
    $script:fixtureCount++
    $script:removedLocalConfigCalls += 6
    Push-Location $repo
    try {
        Invoke-FixtureGit -Arguments @('init', '--quiet')
        $configPath = Join-Path $repo '.git\config'
        if ($Name -like '__config-proof*') {
            $script:configProofBeforeBytes = [System.IO.File]::ReadAllBytes($configPath)
        }
        Write-FixtureLocalConfig -Repo $repo
        # A developer with `commit.gpgSign=true` and no usable key cannot commit here. The fixture
        # declares its own signing configuration rather than inheriting whatever the machine has.
        # And no inherited hooks: a global core.hooksPath whose pre-commit fails would make the
        # fixture commit fail. The fixture-owned hooks path keeps that machine setting out of the
        # repository for the same reason as the signing setting.
        [System.IO.File]::WriteAllText((Join-Path $repo '.gitattributes'), "*.sh text eol=lf`n*.ps1 text eol=lf`n*.py text eol=lf`n", $Latin1)
        foreach ($name in $Files) {
            [System.IO.File]::WriteAllText((Join-Path $repo $name), "echo one`necho two`n", $Latin1)
        }
        Invoke-FixtureGit -Arguments @('add', '-A')
        Invoke-FixtureGit -Arguments @('commit', '-m', 'fixture', '--quiet')
        # Now put CRLF back in the WORKING TREE only. The index keeps LF, and because the attribute
        # normalises on comparison, git reports the tree as clean.
        foreach ($name in $Files) {
            [System.IO.File]::WriteAllText((Join-Path $repo $name), "echo one`r`necho two`r`n", $Latin1)
        }
    } finally {
        Pop-Location
    }
    return $repo
}

$configProof = New-Fixture -Name ('__config-proof space ' + [string][char]0x00E9)
$configAfterPath = Join-Path $configProof '.git\config'
$configAfter = [System.IO.File]::ReadAllText($configAfterPath)
$configAfterBytes = [System.IO.File]::ReadAllBytes($configAfterPath)
$prefixPreserved = $configAfterBytes.Length -ge $script:configProofBeforeBytes.Length
if ($prefixPreserved) {
    for ($byteIndex = 0; $byteIndex -lt $script:configProofBeforeBytes.Length; $byteIndex++) {
        if ($configAfterBytes[$byteIndex] -ne $script:configProofBeforeBytes[$byteIndex]) { $prefixPreserved = $false; break }
    }
}
$configValues = @(& git -C $configProof config --local --list)
$configReadExit = $LASTEXITCODE
$hooksValue = ([System.IO.Path]::GetFullPath((Join-Path $configProof '.no-hooks'))).Replace('\', '/')
$expectedConfigValues = @(
    'user.email=fixture@example.invalid'
    'user.name=fixture'
    'core.autocrlf=false'
    "core.hookspath=$hooksValue"
    'commit.gpgsign=false'
    'tag.gpgsign=false'
)
Assert-True -Condition ($configReadExit -eq 0 -and ($expectedConfigValues | Where-Object { $configValues -cnotcontains $_ }).Count -eq 0) `
    -Message 'fixture local config exposes all six deterministic values through git'
Assert-True -Condition ($null -ne $script:configProofBeforeBytes -and $prefixPreserved) `
    -Message 'fixture config append preserves the bytes generated by git init'
Assert-True -Condition ($configAfter -cmatch '(?im)^\s*(repositoryformatversion|filemode|bare|ignorecase)\s*=') `
    -Message 'fixture config retains unrelated init-generated core settings'
$quotedPath = ConvertTo-FixtureGitConfigPath ('C:\fixture space\' + [string][char]0x00E9 + '\deep\dir\.no-hooks')
Assert-True -Condition ($quotedPath -ceq ('"C:/fixture space/' + [string][char]0x00E9 + '/deep/dir/.no-hooks"')) `
    -Message 'fixture config path serialization uses a quoted value with spaces, non-ASCII, and normalized backslashes'
$configHead = ((& git -C $configProof rev-parse HEAD) -join '').Trim()
$configHeadExit = $LASTEXITCODE
Assert-True -Condition ($configHeadExit -eq 0 -and $configHead -match '^[0-9a-f]{40}$') `
    -Message 'fixture native init, add, and commit produced a real HEAD'

function Get-WorktreeEol {
    param([Parameter(Mandatory)] [string] $Repo, [Parameter(Mandatory)] [string] $File)
    Push-Location $Repo
    try {
        $line = @(& git ls-files --eol -- $File)[0]
        if ($null -eq $line) { return 'absent-from-index' }
        if ($line -match 'w/(\S+)') { return $Matches[1] }
        return 'unparsed'
    } finally {
        Pop-Location
    }
}

function Invoke-Program {
    <#
        -DryRun by default, because the program has no other working mode in this pull request:
        without it every call would meet the write-mode refusal and every cell below would be
        asserting the same sentence. A caller that wants the refusal asks for it with -NoDryRun,
        and exactly one cell does.
    #>
    param([Parameter(Mandatory)] [string] $Repo, [string[]] $ExtraArgs = @(), [switch] $NoDryRun)
    if (-not $NoDryRun -and ($ExtraArgs -cnotcontains '-DryRun')) { $ExtraArgs = @('-DryRun') + $ExtraArgs }
    Push-Location $Repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $programPath @ExtraArgs 2>&1 |
                ForEach-Object { [string]$_ })
        return [ordered]@{ exitCode = $LASTEXITCODE; text = $out -join "`n" }
    } finally {
        Pop-Location
    }
}

try {
    # ---- MODE 1: renormalize + checkout does nothing, because checkout skips a stat-clean path.
    Write-Host ''
    Write-Host '-- mode 1: `git add --renormalize . && git checkout -- .` --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'mode1'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'ARRANGEMENT: the fixture really starts with a CRLF working tree'
    Push-Location $repo
    try {
        & git add --renormalize . 2>&1 | Out-Null
        & git checkout -- . 2>&1 | Out-Null
    } finally { Pop-Location }
    Observe -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'the recipe leaves the file CRLF: it does nothing at all'
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'a.sh') -and ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf')) `
        -Message 'and the diagnosis says it WOULD normalise it, while the file stays CRLF'

    # ---- MODE 2: a pathspec matching nothing makes checkout abort the whole command.
    Write-Host ''
    Write-Host '-- mode 2: a repository with no .ps1 --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'mode2'
    Push-Location $repo
    try {
        $checkout = @(& git checkout --force -- '*.sh' '*.ps1' '*.py' 2>&1 | ForEach-Object { [string]$_ })
    } finally { Pop-Location }
    Observe -Condition (($checkout -join "`n") -match "did not match any file") `
        -Message 'the recipe errors on the extension that is absent'
    Observe -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'and rewrites NOTHING: the .sh that does exist is still CRLF'
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'a.sh') -and ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf')) `
        -Message 'and reports the file that IS there, absent extensions being an ordinary empty case'

    # ---- MODE 3 and 4: an uncommitted edit inside a script.
    # These are one fixture because they are one hazard measured at two points: mode 3 is a guard
    # that warns and proceeds, mode 4 is what proceeding costs. The cell asserts the program's
    # guard DECIDES -- it returns before touching anything -- and that the edit survives.
    Write-Host ''
    Write-Host '-- modes 3+4: an uncommitted edit, and a guard that must decide rather than warn --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'mode4'
    $edited = Join-Path $repo 'a.sh'
    [System.IO.File]::WriteAllText($edited, "echo one`r`necho two`r`necho THE OPERATOR EDIT`r`n", $Latin1)
    Push-Location $repo
    try {
        $statusBefore = @(& git status --porcelain)
        & git checkout --force -- '*.sh' 2>&1 | Out-Null
        $statusAfter = @(& git status --porcelain)
    } finally { Pop-Location }
    Assert-True -Condition (($statusBefore -join '') -match 'a\.sh') `
        -Message 'ARRANGEMENT: the edit is visible to git before the recipe runs'
    Observe -Condition ([System.IO.File]::ReadAllText($edited) -notmatch 'THE OPERATOR EDIT') `
        -Message 'the recipe DESTROYS the uncommitted edit'
    Observe -Condition ($statusAfter.Count -eq 0) `
        -Message 'and `git status` is clean afterwards, so nothing records the loss'

    $repo = New-Fixture -Name 'mode4b'
    $edited = Join-Path $repo 'a.sh'
    [System.IO.File]::WriteAllText($edited, "echo one`r`necho two`r`necho THE OPERATOR EDIT`r`n", $Latin1)
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program REFUSES (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'a\.sh') -Message 'and names the file it refused over'
    Assert-True -Condition ([System.IO.File]::ReadAllText($edited) -match 'THE OPERATOR EDIT') `
        -Message 'and the edit is still there, byte for byte'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'and it touched nothing else either: the refusal happens before any write'

    # ---- MODE 5: skip-worktree, the one that ends with a file gone.
    Write-Host ''
    Write-Host '-- mode 5: a skip-worktree script in the sweep --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'mode5' -Files @('a.sh', 'b.sh')
    Push-Location $repo
    try {
        & git update-index --skip-worktree b.sh 2>&1 | Out-Null
        # The delete-then-restore form the recipe reached for once the earlier fixes were in.
        Remove-Item -LiteralPath (Join-Path $repo 'a.sh') -Force
        Remove-Item -LiteralPath (Join-Path $repo 'b.sh') -Force
        & git checkout -- a.sh b.sh 2>&1 | Out-Null
    } finally { Pop-Location }
    Observe -Condition (-not (Test-Path -LiteralPath (Join-Path $repo 'a.sh'))) `
        -Message "the recipe leaves a.sh DELETED -- the restore refused the whole batch over b.sh"

    $repo = New-Fixture -Name 'mode5b' -Files @('a.sh', 'b.sh')
    Push-Location $repo
    try { & git update-index --skip-worktree b.sh 2>&1 | Out-Null } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program REFUSES (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'b\.sh') -Message 'and names the flagged path'
    Assert-True -Condition ((Test-Path -LiteralPath (Join-Path $repo 'a.sh')) -and (Test-Path -LiteralPath (Join-Path $repo 'b.sh'))) `
        -Message 'and BOTH files still exist: there is no delete step to fail halfway through'

    # ---- The flag predicate must be case-SENSITIVE. git marks an ordinary cached file `H`, and
    # PowerShell's -match is case-insensitive, so `[a-z]` matches `H` and every normal file reads as
    # flagged. The first run of the program refused an entire repository this way, with a
    # word-perfect message. A cell for it, because the defect is invisible in a passing run.
    Write-Host ''
    Write-Host '-- the flag predicate does not read an ordinary file as flagged --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'flags'
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-DryRun')
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "an unflagged repository is not refused (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -notmatch 'skip-worktree') `
        -Message 'and no skip-worktree refusal is printed for files git marks H'

    # ---- The rule's own coverage decides, not this script's pattern list.
    Write-Host ''
    Write-Host '-- a matching extension the attribute does not cover is left alone --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'exempt'
    [System.IO.File]::WriteAllText((Join-Path $repo '.gitattributes'), "*.sh text eol=lf`nexempt.sh -text`n", $Latin1)
    [System.IO.File]::WriteAllText((Join-Path $repo 'exempt.sh'), "echo one`r`necho two`r`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m 'exempt' --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'exempt.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ([System.IO.File]::ReadAllBytes((Join-Path $repo 'exempt.sh')) -contains 13) `
        -Message 'and exempt.sh keeps its CRLF: git''s attribute answer decides, not the extension'

    # ---- The six review findings on the first version, each with the cell it should have had.
    # Four of them were one defect in the write step and share this fixture; two are scope.
    Write-Host '-- -Path scopes the flag refusal, not just the sweep --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'scope'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'sub')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'sub/inside.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m scope --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'sub/inside.sh'), "echo one`r`necho two`r`n", $Latin1)
        # The flagged script is OUTSIDE the requested path.
        & git update-index --skip-worktree a.sh 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'sub')
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "a flagged script outside -Path does not abort the run (exit $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'sub/inside.sh') -and ((Get-WorktreeEol -Repo $repo -File 'sub/inside.sh') -eq 'crlf')) `
        -Message 'and the file inside -Path is reported'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'while the file outside it is left exactly as it was'

    Write-Host ''
    Write-Host '-- launched from a subdirectory, paths still resolve --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'cwd'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'nested')) | Out-Null
    Push-Location (Join-Path $repo 'nested')
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $programPath -DryRun 2>&1 | ForEach-Object { [string]$_ })
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    $text = $out -join "`n"
    Assert-True -Condition ($code -eq 0) -Message "the program exits 0 from a subdirectory (got $code)"
    Assert-True -Condition (Test-Reported -Text $text -Path 'a.sh') `
        -Message 'and reaches a file at the repository ROOT: ls-files paths are repository-relative'

    Write-Host ''
    Write-Host '-- a script beyond the read bound is refused, not read --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'huge'
    $huge = Join-Path $repo 'huge.sh'
    $line = ('#' * 99) + "`n"
    [System.IO.File]::WriteAllText($huge, ($line * 90000), $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m huge --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText($huge, ($line * 90000).Replace("`n", "`r`n"), $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program refuses (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'huge\.sh') -Message 'and names the oversized script'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf') `
        -Message 'and reported nothing else: the bound is checked before any other file is read'

    Write-Host ''
    Write-Host '-- an edit that changes only letter casing is still an edit --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'casing'
    $cased = Join-Path $repo 'a.sh'
    # Same bytes, same length, same line endings-to-be: only the capitals differ. PowerShell's -ne
    # is case-insensitive, so the comparison that protects an operator's work called this identical
    # and rewrote the file.
    [System.IO.File]::WriteAllText($cased, "ECHO ONE`r`nECHO TWO`r`n", $Latin1)
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) `
        -Message "a case-only edit is refused (exit 1, got $($result.exitCode))"
    Assert-True -Condition ([System.IO.File]::ReadAllText($cased) -cmatch 'ECHO ONE') `
        -Message 'and the capitals survive: the refusal happened before the rewrite'

    Write-Host ''
    Write-Host '-- outside a working tree it reports HARNESS-BROKE, not a crash --' -ForegroundColor Cyan
    $bare = Join-Path $fixtureRoot 'not-a-repo'
    [System.IO.Directory]::CreateDirectory($bare) | Out-Null
    Push-Location $bare
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $programPath 2>&1 | ForEach-Object { [string]$_ })
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    Assert-True -Condition ($code -eq 2) `
        -Message "the instrument being unusable is exit 2, not 1 (got $code)"
    Assert-True -Condition (($out -join "`n") -match 'HARNESS-BROKE') `
        -Message 'and it says so: `git rev-parse` writing to stderr must not kill the diagnostic'

    Write-Host ''
    Write-Host '-- -Path is a directory the operator typed, not a pathspec pattern --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'literal'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'scope[1]')) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'scope1')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'scope[1]/inside.sh'), "echo one`necho two`n", $Latin1)
    [System.IO.File]::WriteAllText((Join-Path $repo 'scope1/outside.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m literal --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'scope[1]/inside.sh'), "echo one`r`necho two`r`n", $Latin1)
        [System.IO.File]::WriteAllText((Join-Path $repo 'scope1/outside.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'scope[1]')
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'scope[1]/inside.sh') -and ((Get-WorktreeEol -Repo $repo -File 'scope[1]/inside.sh') -eq 'crlf')) `
        -Message 'the file in the directory that was ASKED for is reported'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'scope1/outside.sh') -eq 'crlf') `
        -Message 'and the one in the directory the pattern would have matched instead is untouched'

    Write-Host '-- a script whose NAME looks like an environment variable --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'envname'
    $envName = '%PATH%.sh'
    [System.IO.File]::WriteAllText((Join-Path $repo $envName), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m envname --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo $envName), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "the program exits 0 rather than refusing a valid checkout (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path $envName) -and ((Get-WorktreeEol -Repo $repo -File $envName) -eq 'crlf')) `
        -Message 'and reports it: no shell expanded %PATH% inside the object argument'

    Write-Host ''
    Write-Host '-- an index blob past the bound, with a small working copy --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'bigblob'
    $big = Join-Path $repo 'big.sh'
    [System.IO.File]::WriteAllText($big, ((('#' * 99) + "`n") * 90000), $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m big --quiet 2>&1 | Out-Null
        # The working copy is now SMALL, so the directory entry says nothing about the 8 MiB the
        # index still holds. Without a size probe on the blob, it is materialised in full.
        [System.IO.File]::WriteAllText($big, "echo tiny`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program refuses (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'index blob') `
        -Message 'and says it was the INDEX side that was over the bound, not the working file'

    Write-Host ''
    Write-Host '-- a flagged script with a non-ASCII name, under core.quotePath --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'quotepath'
    $accented = [char]0x63 + [char]0x61 + [char]0x66 + [char]0xE9 + '.sh'   # cafe with an acute e
    [System.IO.File]::WriteAllText((Join-Path $repo $accented), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git config core.quotePath true 2>&1 | Out-Null
        & git add -A 2>&1 | Out-Null
        & git commit -m accented --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo $accented), "echo one`r`necho two`r`n", $Latin1)
        & git update-index --skip-worktree $accented 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    # The exit code CANNOT discriminate here and asserting it alone was a wasted cell: with the
    # non-NUL listing the flag refusal is skipped and the run refuses anyway, for a different and
    # misleading reason. The message is the assertion.
    Assert-True -Condition ($result.exitCode -eq 1) `
        -Message "the run refuses (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'skip-worktree') `
        -Message 'and it is the SKIP-WORKTREE refusal, not some other refusal that happens to exit 1'
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File $accented) -eq 'crlf') `
        -Message 'and the flagged file was not touched'

    Write-Host ''
    Write-Host '-- a non-ASCII name is decoded as UTF-8, not as the console code page --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'utf8name'
    $accented2 = [char]0x63 + [char]0x61 + [char]0x66 + [char]0xE9 + '.sh'
    [System.IO.File]::WriteAllText((Join-Path $repo $accented2), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m accented --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo $accented2), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "an accented filename is not refused as missing (exit $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path $accented2) -and ((Get-WorktreeEol -Repo $repo -File $accented2) -eq 'crlf')) `
        -Message 'and it is reported: git speaks UTF-8 and the program now listens in UTF-8'

    Write-Host ''
    Write-Host '-- a checkout path and a script name that both contain spaces --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'a repo with spaces'
    $spaced = 'my script.sh'
    [System.IO.File]::WriteAllText((Join-Path $repo $spaced), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m spaced --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo $spaced), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "a valid checkout with spaces is not refused (exit $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path $spaced) -and ((Get-WorktreeEol -Repo $repo -File $spaced) -eq 'crlf')) `
        -Message 'and the script whose NAME has a space is reported'

    Write-Host ''
    Write-Host '-- an uppercase extension is in the sweep, not outside it --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'upper'
    [System.IO.File]::WriteAllText((Join-Path $repo 'BUILD.PS1'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m upper --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'BUILD.PS1'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'BUILD.PS1') -and ((Get-WorktreeEol -Repo $repo -File 'BUILD.PS1') -eq 'crlf')) `
        -Message 'BUILD.PS1 is reported: the attribute covers it, so the sweep must list it'

    Write-Host ''
    Write-Host '-- a UTF-16 script is refused, never reported as already normalised --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'utf16'
    $u16 = Join-Path $repo 'wide.ps1'
    [System.IO.File]::WriteAllText($u16, "echo one`r`necho two`r`n", (New-Object System.Text.UnicodeEncoding($false, $true)))
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m wide --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program refuses (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'UTF-16') -Message 'and says the encoding is why'
    Assert-True -Condition ([System.IO.File]::ReadAllBytes($u16)[0] -eq 0xFF) `
        -Message 'and the file still has its byte-order mark: nothing was rewritten'

    Write-Host ''
    Write-Host '-- an index that was never renormalised is a repository problem, and says so --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'crlfindex'
    Push-Location $repo
    try {
        # Commit the CRLF bytes with no attribute in force, THEN add the rule: the index keeps
        # CRLF, which is the state a checkout is left in when the rule lands after the content.
        [System.IO.File]::WriteAllText((Join-Path $repo '.gitattributes'), "", $Latin1)
        [System.IO.File]::WriteAllText((Join-Path $repo 'stale.sh'), "echo one`r`necho two`r`n", $Latin1)
        & git add -A 2>&1 | Out-Null
        & git commit -m stale --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo '.gitattributes'), "*.sh text eol=lf`n", $Latin1)
        & git add .gitattributes 2>&1 | Out-Null
        & git commit -m rule --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) -Message "the program refuses (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -match 'renormalis') `
        -Message 'and points at `git add --renormalize`, not at the working tree'

    Write-Host ''
    Write-Host '-- -Path in the wrong case still names the directory --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'pathcase'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'ci')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'ci/inside.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m pathcase --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/inside.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'CI')
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ((Test-Reported -Text $result.text -Path 'ci/inside.sh') -and ((Get-WorktreeEol -Repo $repo -File 'ci/inside.sh') -eq 'crlf')) `
        -Message '-Path CI reaches ci/: exiting 0 having REPORTED nothing is the worst way to be wrong'

    Write-Host ''
    Write-Host '-- the count bound is in scope where it is tested --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'countbound'
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-DryRun')
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "an ordinary repository is not refused by the path-count ceiling (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -cnotmatch 'beyond the  this program') `
        -Message 'and no refusal is printed with an EMPTY ceiling in it: a bound out of scope reads as zero'

    Write-Host ''
    Write-Host '-- a tracked script replaced by a directory --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'asdir'
    Remove-Item -LiteralPath (Join-Path $repo 'a.sh') -Force
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'a.sh')) | Out-Null
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 1) `
        -Message "the program refuses rather than throwing (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -cnotmatch 'UnauthorizedAccessException') `
        -Message 'and the refusal is a message, not a stack trace out of File.Open'

    Write-Host '-- an empty script has no line endings, which is not a broken index --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'noeol'
    [System.IO.File]::WriteAllText((Join-Path $repo 'empty.sh'), '', $Latin1)
    [System.IO.File]::WriteAllText((Join-Path $repo 'oneline.sh'), 'echo one', $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m noeol --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "a file git reports as i/none is not refused (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -cnotmatch 'renormalis') `
        -Message 'and nobody is sent to renormalise a file that has no line endings to normalise'

    Write-Host ''
    Write-Host '-- without -DryRun the program WRITES, and verifies what it wrote --' -ForegroundColor Cyan
    #
    # This cell used to assert the OPPOSITE: #677 shipped the diagnosis alone and refused here, and
    # the byte-comparison below carried the label "ARMED for #693: nothing here can write, so this
    # cannot fail yet". #693 is this change, so the arming condition is met and the assertion is
    # inverted rather than deleted. It is worth saying which cell was rewritten and why: a test that
    # certified an interim refusal is exactly the kind that gets quietly dropped when the refusal
    # goes, taking its byte-level comparison with it.
    $repo = New-Fixture -Name 'write'
    $before = [System.IO.File]::ReadAllBytes((Join-Path $repo 'a.sh'))
    Assert-True -Condition (($before -contains 13) -and ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'crlf')) `
        -Message 'ARRANGEMENT: the fixture starts with CR bytes on disk and git agrees it is crlf'

    $result = Invoke-Program -Repo $repo -NoDryRun
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "the write mode succeeds (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -cmatch 'normalised\s+a\.sh' -and $result.text -cnotmatch 'would normalise\s+a\.sh') `
        -Message 'and reports the PAST tense, which a run that had not written would be lying with'

    # git's own eol column, not this suite's opinion of the bytes. `git status` was clean before and
    # is clean after -- that is the whole reason #676 needs a program -- so the instrument has to be
    # the one that can tell.
    Assert-True -Condition ((Get-WorktreeEol -Repo $repo -File 'a.sh') -eq 'lf') `
        -Message 'and git now reads the working tree as lf'

    # BYTES, not length. A partial conversion is different bytes of the same length, which a length
    # comparison passes. The content must be exactly the CRLF content with the CRs removed -- not
    # merely CR-free, which an empty file also is.
    $after = [System.IO.File]::ReadAllBytes((Join-Path $repo 'a.sh'))
    $expected = [System.Text.Encoding]::GetEncoding(28591).GetBytes(
        [System.Text.Encoding]::GetEncoding(28591).GetString($before).Replace("`r`n", "`n"))
    $exact = $after.Length -eq $expected.Length
    if ($exact) {
        for ($i = 0; $i -lt $expected.Length; $i++) {
            if ($after[$i] -ne $expected[$i]) { $exact = $false; break }
        }
    }
    Assert-True -Condition $exact `
        -Message 'and the bytes are the previous bytes with CR removed, byte for byte -- not merely CR-free'

    # THE VERIFIER RAN AND SAID SO. Without this the cell passes over a program that wrote correctly
    # and skipped its own verification, which is the state #676 names as the one worth having: the
    # write is the easy half, and "did it land" is the question `git status` cannot answer.
    Assert-True -Condition ($result.text -cmatch 'verified: every rewritten file now reads w/lf') `
        -Message 'and the run says its verification re-read observed the result'

    # No sidecar survives the happy path. The staging file, the backup and the rescue file are all
    # GUID-named, so a leak is invisible to `git status` -- untracked content this program created
    # and did not clean up, in a checkout it just told the operator is correct.
    $sidecars = @(Get-ChildItem -LiteralPath $repo -Filter '*.eol-*' -Force -ErrorAction SilentlyContinue)
    Assert-True -Condition ($sidecars.Count -eq 0) `
        -Message "and leaves no .eol-tmp, .eol-backup or .eol-rescued behind (found $($sidecars.Count))"

    Write-Host ''
    Write-Host '-- the replacement does not clobber a sidecar it did not create --' -ForegroundColor Cyan
    #
    # The first version staged through a PREDICTABLE name and truncated whatever already had it. A
    # file called `a.sh.eol-migration.tmp` is legal in a repository, does not end in `.sh`, and is
    # therefore invisible to every check above -- so the program would have destroyed a file it was
    # never asked to look at. The staging name now carries a GUID and is opened CreateNew, which
    # throws rather than truncating.
    $repo = New-Fixture -Name 'sidecar'
    $bystander = Join-Path $repo 'a.sh.eol-migration.tmp'
    [System.IO.File]::WriteAllText($bystander, 'THE BYSTANDER', $Latin1)
    $result = Invoke-Program -Repo $repo -NoDryRun
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "the run succeeds with an unrelated sidecar present (exit $($result.exitCode))"
    Assert-True -Condition ((Test-Path -LiteralPath $bystander) -and ([System.IO.File]::ReadAllText($bystander) -ceq 'THE BYSTANDER')) `
        -Message 'and the bystander file is untouched, content and all'

    Write-Host ''
    Write-Host '-- a run that converts nothing still verifies what it classified --' -ForegroundColor Cyan
    #
    # The verifier covers ALREADY-LF paths too, and this is the cell that keeps it that way. The
    # classification comes from bytes read earlier in the run; a checkout filter or an editor
    # writing CRLF after that read would leave the file skipped on a stale snapshot, and a run that
    # converted nothing else would then skip verification entirely and report success twice over --
    # once in the summary, once by exiting 0.
    $repo = New-Fixture -Name 'nothing'
    [System.IO.File]::WriteAllText((Join-Path $repo 'a.sh'), "echo one`necho two`n", $Latin1)
    $result = Invoke-Program -Repo $repo -NoDryRun
    Assert-True -Condition ($result.exitCode -eq 0) `
        -Message "a run with nothing to convert succeeds (exit $($result.exitCode))"
    Assert-True -Condition ($result.text -cmatch 'normalised 0 file\(s\); 1 already LF') `
        -Message 'and reports one already-LF file and no conversions'
    Assert-True -Condition ($result.text -cmatch 'verified: every rewritten file now reads w/lf') `
        -Message 'and STILL verifies, because an already-LF classification is a claim about the disk too'

    # THE CLASS CELL, and the one that survives any future narrowing of this program.
    #
    # A diagnosis fails in a way a converter does not: not by a wrong verdict, but by a file that
    # got NO verdict and was therefore never mentioned. "Nothing to do" where the recipe would
    # have destroyed something is worse than having no tool, because it tells the operator that
    # everything is fine. So the residue is an OUTPUT LINE, not silence -- and this cell holds
    # whatever the buckets become.
    Write-Host ''
    Write-Host '-- every covered path gets a verdict, and a residue is reported rather than dropped --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'residue' -Files @('a.sh', 'b.sh', 'c.sh')
    $result = Invoke-Program -Repo $repo
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the program exits 0 (got $($result.exitCode))"
    Assert-True -Condition ($result.text -cmatch 'every covered path has a verdict: 3 classified') `
        -Message 'all three covered paths are accounted for by name, not by absence of complaint'

    Write-Host ''
    Write-Host '-- an ambiguous NESTED scope is refused, not silently widened --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'nested'
    foreach ($d in @('parent/ci', 'parent/CI')) {
        [System.IO.Directory]::CreateDirectory((Join-Path $repo $d)) | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo "$d/x.sh"), "echo one`necho two`n", $Latin1)
    }
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m nested --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    $tracked = @(& git -C $repo ls-files -- 'parent/*')
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'parent/Ci')
    if ($tracked.Count -ge 2) {
        # The volume distinguishes the two spellings, so the request really is ambiguous.
        Assert-True -Condition ($result.exitCode -eq 1) `
            -Message "an ambiguous nested -Path is refused (exit $($result.exitCode))"
        Assert-True -Condition ($result.text -cmatch 'parent/ci' -and $result.text -cmatch 'parent/CI') `
            -Message 'and both spellings are named, so the operator picks rather than the program'
    } else {
        # A case-insensitive volume collapsed the two directories into one, so there is nothing
        # ambiguous to refuse. Asserting the refusal here would fail for a property of the volume.
        Assert-True -Condition ($result.exitCode -eq 0) `
            -Message "on a case-insensitive volume the two spellings are one directory, so nothing is ambiguous (exit $($result.exitCode))"
        Skip-Assertion `
            -Message 'and the ambiguity refusal is not exercised here: the volume, not the program, decided'
    }

    # ---- The spelling collapse, fed STRINGS, so the verdict is the same on every filesystem.
    #
    # This is the cell that turns a declaration into a measurement. The ambiguity refusal was
    # unreachable on a case-insensitive volume, so the sabotage that removes `-CaseSensitive` proved
    # nothing on the machine this suite runs on -- a guard whose test depended on the developer's
    # disk. The decision is over STRINGS from `git ls-files`, never over the disk, so the seam takes
    # a list and the cell supplies one.
    Write-Host ''
    Write-Host '-- the spelling collapse distinguishes case, on any filesystem --' -ForegroundColor Cyan
    $seam = Join-Path $fixtureRoot 'seam.ps1'
    $seamStart = $programText.IndexOf('function Get-DistinctScopeSpellings {')
    $seamEnd = $programText.IndexOf('function Read-BoundedGit {')
    if ($seamStart -lt 0 -or $seamEnd -le $seamStart) {
        Write-Host 'HARNESS-BROKE: Get-DistinctScopeSpellings was not found between its anchors' -ForegroundColor Magenta
        exit 2
    }
    [System.IO.File]::WriteAllText($seam, $programText.Substring($seamStart, $seamEnd - $seamStart) + @'

$two = Get-DistinctScopeSpellings -Paths @('parent/ci/x.sh', 'parent/CI/x.sh') -Depth 2
$one = Get-DistinctScopeSpellings -Paths @('parent/ci/x.sh', 'parent/ci/y.sh') -Depth 2
Write-Output "two=$($two.Count) one=$($one.Count)"
'@, $Latin1)
    $seamOut = (@(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $seam 2>&1 |
                ForEach-Object { [string]$_ }) -join "`n")
    Assert-True -Condition ($seamOut -cmatch 'two=2') `
        -Message 'two spellings that differ only in case are TWO -- which is what makes the refusal fire'
    Assert-True -Condition ($seamOut -cmatch 'one=1') `
        -Message 'and two paths under the same spelling are ONE, so the refusal does not cry wolf'

    Write-Host ''
    Write-Host '-- a directory that exists and holds nothing tracked is an empty scope --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'emptyscope'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'Ci')) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'ci')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'ci/tracked.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m emptyscope --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/tracked.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    $result = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'Ci')
    Assert-True -Condition ($result.exitCode -eq 0) -Message "the run succeeds (exit $($result.exitCode))"
    Assert-True -Condition (-not (Test-Reported -Text $result.text -Path 'ci/tracked.sh')) `
        -Message 'and does NOT report a file from a directory the operator did not name'

    # ---- #699 finding 1: `.` and `..` inside -Path, folded before the disk is asked about it.
    #
    # git normalises a pathspec itself and `Test-ExactDirectory` did not, so the two halves of one
    # run disagreed about what -Path named -- and the disagreement is not symmetric: the disk half
    # answering "absent" is what drops the run into the case-insensitive fallback.
    #
    # The seam takes STRINGS, like the spelling collapse above and for the same reason. `a/../../x`
    # is a claim about a path's SHAPE; a cell that had to build each shape on disk would be a claim
    # about the machine that built it instead.
    Write-Host ''
    Write-Host '-- the scope path folds . and .., and refuses one that climbs out --' -ForegroundColor Cyan
    $scopeSeam = Join-Path $fixtureRoot 'scope-seam.ps1'
    $scopeStart = $programText.IndexOf('function Resolve-ScopePath {')
    $scopeEnd = $programText.IndexOf('function Test-ExactDirectory {')
    if ($scopeStart -lt 0 -or $scopeEnd -le $scopeStart) {
        Write-Host 'HARNESS-BROKE: Resolve-ScopePath was not found between its anchors' -ForegroundColor Magenta
        exit 2
    }
    [System.IO.File]::WriteAllText($scopeSeam, $OrdinalHelpers + $programText.Substring($scopeStart, $scopeEnd - $scopeStart) + @'

foreach ($case in @('./ci', 'ci/../ci', 'a/./b/../c', '.', '../x', 'a/../../x')) {
    $verdict = Resolve-ScopePath -RelativePath $case
    Write-Output ('<' + $case + '> ok=' + $verdict.ok + ' path=<' + $verdict.path + '>')
}

# The ordinal comparisons, exercised through the REAL helpers this seam carries.
$vs = [char]0x0FE00
Write-Output ('culture-eq=' + (('GREEN' + $vs) -eq 'GREEN'))
Write-Output ('culture-ceq=' + (('GREEN' + $vs) -ceq 'GREEN'))
Write-Output ('ordinal-differs=' + (Test-SameText ('GREEN' + $vs) 'GREEN'))
Write-Output ('ordinal-same=' + (Test-SameText 'GREEN' 'GREEN'))
Write-Output ('ordinal-case=' + (Test-SameText 'GREEN' 'green'))
Write-Output ('culture-empty=' + (("$vs") -eq ''))
Write-Output ('ordinal-empty=' + (Test-SameText "$vs" ''))
Write-Output ('inset-differs=' + (Test-InSet @('a.sh') ('a.sh' + $vs)))
Write-Output ('inset-same=' + (Test-InSet @('a.sh') 'a.sh'))
$weighted = Resolve-ScopePath -RelativePath "$vs"
Write-Output ('weightless-scope=<' + $weighted.path + '>')
'@, $Latin1)
    $scopeOut = (@(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scopeSeam 2>&1 |
                ForEach-Object { [string]$_ }) -join "`n")
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<./ci> ok=True path=<ci>')) `
        -Message 'a leading ./ is dropped, so the disk is asked about the directory git was asked about'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<ci/../ci> ok=True path=<ci>')) `
        -Message 'and a .. pops the segment before it rather than being handed to the disk as a name'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<a/./b/../c> ok=True path=<a/c>')) `
        -Message 'both, interleaved, in one path'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<.> ok=True path=<>')) `
        -Message 'the repository root reduces to the empty scope, which is the whole repository and not an error'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<../x> ok=False')) `
        -Message 'a path that climbs above the root is REFUSED here, where the answer is known'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('<a/../../x> ok=False')) `
        -Message 'and climbing out after descending is the same refusal, not a shape that slips past'

    # ---- #753's class, in this program's own comparisons. Found by running #759's AST detector
    # (`ci/find-culture-comparisons.ps1`) against this file while reviewing that pull request: 17
    # sites, of which the one below undoes this very fix.
    #
    # The first two assertions are ARRANGEMENT and they are the reason the rest matter: they show
    # the operators this program used to reach for answering YES to two strings that differ.
    Assert-True -Condition ($scopeOut -cmatch 'culture-eq=True') `
        -Message 'ARRANGEMENT: -eq calls two DIFFERENT strings equal, because a variation selector has no culture weight'
    Assert-True -Condition ($scopeOut -cmatch 'culture-ceq=True') `
        -Message 'ARRANGEMENT: and so does -ceq -- case-sensitivity and culture-awareness are ORTHOGONAL'

    Assert-True -Condition ($scopeOut -cmatch 'ordinal-differs=False') `
        -Message 'the ordinal comparison tells them apart, which is the whole fix'
    Assert-True -Condition ($scopeOut -cmatch 'ordinal-same=True') `
        -Message 'CONTROL: and still calls two identical strings equal, or it is a comparer that never matches'
    Assert-True -Condition ($scopeOut -cmatch 'ordinal-case=False') `
        -Message 'and keeps the CASE-sensitivity every -ceq here intended: ordinal removes the culture, not the case'

    # THE SITE THAT UNDID THE FIX. `-eq ''` is how this program asked whether -Path reduced to the
    # repository root, and a string of Length 1 answered YES -- so a directory named with a
    # zero-weight code point swept the WHOLE REPOSITORY instead of that one directory. The scope
    # widening this pull request exists to close, through the other door.
    Assert-True -Condition ($scopeOut -cmatch 'culture-empty=True') `
        -Message 'ARRANGEMENT: a string of Length 1 compares -eq to the empty string'
    Assert-True -Condition ($scopeOut -cmatch 'ordinal-empty=False') `
        -Message 'ordinally it does not, so an emptiness guard stops answering yes about a non-empty value'
    Assert-True -Condition ($scopeOut -cmatch [regex]::Escape('weightless-scope=<') -and $scopeOut -cnotmatch [regex]::Escape('weightless-scope=<>')) `
        -Message 'END TO END: a -Path of one zero-weight character is a DIRECTORY, not the whole repository'

    Assert-True -Condition ($scopeOut -cmatch 'inset-differs=False') `
        -Message 'set membership tells the two paths apart too -- the residue guard fails OPEN otherwise'
    Assert-True -Condition ($scopeOut -cmatch 'inset-same=True') `
        -Message 'CONTROL: and still finds a path that really is in the set'

    Write-Host ''
    Write-Host '-- ./sub and sub/../sub reach what sub reaches, and none takes the fallback --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'dotscope'
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'sub')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'sub/inside.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m dotscope --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'sub/inside.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }
    # A directory that EXISTS with the spelling asked for and holds nothing tracked. This is the
    # branch the fallback steals: `Test-ExactDirectory` deciding the spelling is present is the only
    # thing that stops the run widening to `:(literal,icase)`, and a `.` segment made it decide
    # absent for a directory that is right there.
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'bare')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'bare/untracked.txt'), "not tracked`n", $Latin1)
    $plain = Invoke-Program -Repo $repo -ExtraArgs @('-Path', 'bare')
    Assert-True -Condition ($plain.text -cmatch 'exists and holds no tracked scripts') `
        -Message 'ARRANGEMENT: the plain spelling reaches the exists-and-empty branch, so there is a branch to lose'
    foreach ($spelling in @('./bare', 'bare/../bare')) {
        $dotted = Invoke-Program -Repo $repo -ExtraArgs @('-Path', $spelling)
        Assert-True -Condition ($dotted.text -cmatch 'exists and holds no tracked scripts') `
            -Message "-Path '$spelling' reaches the same branch instead of falling back to the case-insensitive scope"
    }
    # THE PREMISE, PINNED -- and it does not redden under the sabotage that removes the folding,
    # which is the honest reading of it. Both of these pass on the unfixed program too, because git
    # normalises the PATHSPEC itself (`:(literal)./ci` lists what `:(literal)ci` lists, measured on
    # 2.47.1.windows.1). That asymmetry between git and the disk is the whole defect, so the half
    # this repository does not own is worth a pin: if a future git stopped folding, the fix above
    # would be normalising one side of a disagreement that had moved.
    $viaDot = Invoke-Program -Repo $repo -ExtraArgs @('-Path', './sub')
    Assert-True -Condition ($viaDot.exitCode -eq 0) `
        -Message "PREMISE: git folds the pathspec, so a dotted -Path over a populated directory succeeds (exit $($viaDot.exitCode))"
    Assert-True -Condition (Test-Reported -Text $viaDot.text -Path 'sub/inside.sh') `
        -Message 'PREMISE: and reaches the same file -Path sub reaches -- the sweep half was never the broken one'

    # ---- #699 finding 2: the ceiling counts what the program KEEPS, not what git prints.
    #
    # Driven at the seam with a ceiling of 200 characters, because the real one is 20,000,000 and a
    # fixture that reached it would need about a million paths. The ceiling is a NUMBER the function
    # reads; the cell supplies a different number and the function is otherwise the one that ships.
    #
    # The first read is the ARRANGEMENT and it has to overflow, or the second read proves nothing:
    # a filtered read that does not overflow says something only when the same listing, unfiltered,
    # does.
    Write-Host ''
    Write-Host '-- the enumeration ceiling counts kept records, not printed ones --' -ForegroundColor Cyan
    $bounded = New-Fixture -Name 'bounded'
    [System.IO.Directory]::CreateDirectory((Join-Path $bounded 'n')) | Out-Null
    foreach ($i in 1..400) {
        [System.IO.File]::WriteAllText((Join-Path $bounded ('n/{0:D4}.txt' -f $i)), "x`n", $Latin1)
    }
    [System.IO.File]::WriteAllText((Join-Path $bounded 'keep.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $bounded
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m bounded --quiet 2>&1 | Out-Null
    } finally { Pop-Location }

    $readerSeam = Join-Path $fixtureRoot 'reader-seam.ps1'
    $readerStart = $programText.IndexOf('function Read-BoundedGit {')
    $readerEnd = $programText.IndexOf('function Invoke-Git {')
    if ($readerStart -lt 0 -or $readerEnd -le $readerStart) {
        Write-Host 'HARNESS-BROKE: Read-BoundedGit was not found between its anchors' -ForegroundColor Magenta
        exit 2
    }
    [System.IO.File]::WriteAllText($readerSeam, '$MaxEnumerationChars = 200' + "`n" + $OrdinalHelpers +
        $programText.Substring($readerStart, $readerEnd - $readerStart) + @'

$repo = $args[0]
$listing = @('-C', $repo, 'ls-files', '-z', '--')
$all = Read-BoundedGit -GitArgs $listing
$scripts = Read-BoundedGit -GitArgs $listing -KeepRecord { param($Record) $Record.ToLowerInvariant().EndsWith('.sh') }
$names = @($scripts.records)
Write-Output ("unfiltered-overflowed=" + $all.overflowed)
Write-Output ("filtered-overflowed=" + $scripts.overflowed)
Write-Output ("kept=" + ($names -join ','))
Write-Output ("all-kept-are-scripts=" + (($names.Count -gt 0) -and (@($names | Where-Object { -not $_.ToLowerInvariant().EndsWith('.sh') }).Count -eq 0)))
'@, $Latin1)
    $readerOut = (@(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $readerSeam $bounded 2>&1 |
                ForEach-Object { [string]$_ }) -join "`n")
    Assert-True -Condition ($readerOut -cmatch 'unfiltered-overflowed=True') `
        -Message 'ARRANGEMENT: unfiltered, this listing really does pass the ceiling'
    Assert-True -Condition ($readerOut -cmatch 'filtered-overflowed=False') `
        -Message 'and filtered it does NOT: the 400 non-scripts are dropped as they are read, never collected'
    Assert-True -Condition ($readerOut -cmatch 'kept=[^\r\n]*keep\.sh') `
        -Message 'while the script that IS in scope survives the same read'
    # The emptiness is part of this condition on purpose: "every kept record is a script" is TRUE of
    # a read that kept nothing, so without it the assertion survives a ceiling that overflowed and
    # threw the whole listing away -- a green over an empty set, which is the shape that makes a
    # suite look like coverage while measuring nothing.
    Assert-True -Condition ($readerOut -cmatch 'all-kept-are-scripts=True') `
        -Message 'and nothing the predicate refused was kept, over a set that is not empty'

    # ---- The predicate the enumeration hands it, which decides two different things.
    Write-Host ''
    Write-Host '-- the kept-record predicate keeps scripts AND anything it cannot parse --' -ForegroundColor Cyan
    $predicateSeam = Join-Path $fixtureRoot 'predicate-seam.ps1'
    $predicateStart = $programText.IndexOf('$KeepScriptRecord = {')
    $predicateEnd = $programText.IndexOf('$enumeration = Read-BoundedGit')
    if ($predicateStart -lt 0 -or $predicateEnd -le $predicateStart) {
        Write-Host 'HARNESS-BROKE: $KeepScriptRecord was not found between its anchors' -ForegroundColor Magenta
        exit 2
    }
    [System.IO.File]::WriteAllText($predicateSeam, '$extensions = @(''.sh'', ''.ps1'', ''.py'')' + "`n" +
        $programText.Substring($predicateStart, $predicateEnd - $predicateStart) + @'

Write-Output ("upper=" + (& $KeepScriptRecord "i/lf w/lf attr/`tsub/BUILD.PS1"))
Write-Output ("other=" + (& $KeepScriptRecord "i/lf w/lf attr/`tsub/notes.txt"))
Write-Output ("notab=" + (& $KeepScriptRecord "a record with no tab at all"))
'@, $Latin1)
    $predicateOut = (@(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $predicateSeam 2>&1 |
                ForEach-Object { [string]$_ }) -join "`n")
    Assert-True -Condition ($predicateOut -cmatch 'upper=True') `
        -Message 'an uppercase extension is still in the sweep: the extension test stayed case-insensitive when it moved'
    Assert-True -Condition ($predicateOut -cmatch 'other=False') `
        -Message 'and a tracked non-script is refused, which is the whole point of filtering at the read'
    Assert-True -Condition ($predicateOut -cmatch 'notab=True') `
        -Message 'but an UNPARSABLE record is kept, so the refusal that names it is not retired by the filter'

    Write-Host ''
    Write-Host '-- a -Path that climbs out of the repository is a refusal, not a broken instrument --' -ForegroundColor Cyan
    # Exit 2 says the tool could not answer. Here it answered: the path is outside the repository,
    # which is the operator's mistake and reversible by retyping it. Reporting HARNESS-BROKE for
    # that spends the one signal that means "do not trust what you just read".
    $climb = Invoke-Program -Repo $repo -ExtraArgs @('-Path', '../outside')
    Assert-True -Condition ($climb.exitCode -eq 1) `
        -Message "the run REFUSES rather than reporting a broken instrument (exit $($climb.exitCode); before this change the probe's non-zero exit read as HARNESS-BROKE, 2)"
    Assert-True -Condition ($climb.text -cmatch 'climbs above the repository root' -and $climb.text -cmatch [regex]::Escape('../outside')) `
        -Message 'and says why, naming the path the operator typed'

    # ---- #698: a parent directory replaced by a junction, and the program reporting on what is on
    # the other side of it.
    #
    # `Test-Path`, `File::Exists` and `File::Open` all TRAVERSE a reparse point without saying so,
    # so the enumeration came from the index and the bytes came from somewhere else. The verdict it
    # produced is the dangerous one -- not a wrong refusal but "already LF", a clean bill of health
    # for a file that still holds CRLF on the other side of the link.
    #
    # `New-Item -ItemType Junction` needs no administrator rights, which is why the cell is a
    # junction and not a symlink. A machine that cannot build one cannot verify this guard at all,
    # so that is HARNESS-BROKE rather than a pass: nothing would have been measured.
    Write-Host ''
    Write-Host '-- a path reached through a junction is refused, not reported on --' -ForegroundColor Cyan
    $linked = New-Fixture -Name 'linked'
    [System.IO.Directory]::CreateDirectory((Join-Path $linked 'real')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $linked 'real/inside.sh'), "echo one`necho two`n", $Latin1)
    Push-Location $linked
    try {
        & git add -A 2>&1 | Out-Null
        & git commit -m linked --quiet 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $linked 'real/inside.sh'), "echo one`r`necho two`r`n", $Latin1)
    } finally { Pop-Location }

    # The negative control runs FIRST, on this same fixture before the junction exists. Without it a
    # guard that refused every path would pass the two assertions below and look like a fix.
    $ordinary = Invoke-Program -Repo $linked
    Assert-True -Condition ($ordinary.exitCode -eq 0 -and (Test-Reported -Text $ordinary.text -Path 'real/inside.sh')) `
        -Message "CONTROL: with an ordinary directory the same path is reported, not refused (exit $($ordinary.exitCode))"

    # The decoy is byte-identical to the INDEX blob, so the program reading through the junction
    # sees a file that is already LF and says so. A decoy with different content would have been
    # caught by the uncommitted-edit refusal for the wrong reason, and a refusal is not the failure
    # this cell is about.
    [System.IO.Directory]::CreateDirectory((Join-Path $linked 'decoy')) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $linked 'decoy/inside.sh'), "echo one`necho two`n", $Latin1)
    Rename-Item -LiteralPath (Join-Path $linked 'real') -NewName 'real-actual'
    $junction = Join-Path $linked 'real'
    $madeJunction = $null -ne (New-Item -ItemType Junction -Path $junction -Value (Join-Path $linked 'decoy') -ErrorAction SilentlyContinue)
    if (-not $madeJunction) {
        Write-Host 'HARNESS-BROKE: this machine would not create a junction, so the reparse-point guard was not measured' -ForegroundColor Magenta
        exit 2
    }
    try {
        Assert-True -Condition ([System.IO.File]::ReadAllText((Join-Path $linked 'real/inside.sh')) -cnotmatch "`r`n") `
            -Message 'ARRANGEMENT: the junction really redirects -- the tracked path now reads the decoy, which is LF'
        Assert-True -Condition ([System.IO.File]::ReadAllText((Join-Path $linked 'real-actual/inside.sh')) -cmatch "`r`n") `
            -Message 'ARRANGEMENT: while the file the repository actually has still holds CRLF, so a clean bill of health is a lie'

        $through = Invoke-Program -Repo $linked
        Assert-True -Condition ($through.exitCode -eq 1) `
            -Message "the run REFUSES rather than reporting on the other side (exit $($through.exitCode))"
        Assert-True -Condition ($through.text -cmatch 'reparse point' -and $through.text -cmatch [regex]::Escape("real/inside.sh") -and $through.text -cmatch "'real'") `
            -Message 'and names both the tracked path and the segment that redirected it'
        Assert-True -Condition ($through.text -cnotmatch 'already LF') `
            -Message 'and does NOT reach the summary that called it already LF: the refusal lands before any verdict'

        # The scope half of the same question. A junction NAMED by -Path redirects the whole sweep,
        # so it is refused before the probes rather than once per file.
        $scoped = Invoke-Program -Repo $linked -ExtraArgs @('-Path', 'real')
        Assert-True -Condition ($scoped.exitCode -eq 1) `
            -Message "a -Path that names a junction is refused too (exit $($scoped.exitCode))"
        Assert-True -Condition ($scoped.text -cmatch 'reparse point' -and $scoped.text -cmatch "-Path 'real'") `
            -Message 'and says so as a scope refusal, before any probe or enumeration runs'
        # ---- The walk ASKED about the zero-weight segment: observed at the seam, no filesystem.
        #
        # The cells on the base branch establish that -eq and -ceq disagree with an ordinal
        # comparison about a zero-weight code point. That is a claim about the COMPARISON. What this
        # guard promises is that it asks about EVERY SEGMENT, which is a different subject, and the
        # gap between the two is where the defect lived.
        #
        # The cache is the observation point, and it works because the function takes it rather than
        # holding one: it is keyed by the repository-relative prefix, so a segment that was asked
        # about leaves its key behind and a segment that was dropped does not. Under the old
        # comparison the middle segment never appears. (Method from the GraphHelm ISSUES 4 lane.)
        #
        # No filesystem: an attributes read on a path that does not exist throws, is caught, and is
        # cached as "not a reparse point" -- so the walk still visits every segment and the cache
        # still records what it visited.
        $guardSeam = Join-Path $fixtureRoot 'guard-seam.ps1'
        $guardStart = $programText.IndexOf('function Find-ReparsePointSegment {')
        $guardEnd = $programText.IndexOf('function Get-DistinctScopeSpellings {')
        if ($guardStart -lt 0 -or $guardEnd -le $guardStart) {
            Write-Host 'HARNESS-BROKE: Find-ReparsePointSegment was not found between its anchors' -ForegroundColor Magenta
            exit 2
        }
        [System.IO.File]::WriteAllText($guardSeam, $OrdinalHelpers +
            $programText.Substring($guardStart, $guardEnd - $guardStart) + @'

$vs = [string][char]0x0FE00
$cache = @{}
$null = Find-ReparsePointSegment -Root 'C:\does-not-exist' -RelativePath ("a/" + $vs + "/b") -Cache $cache
Write-Output ('asked=' + (($cache.Keys | Sort-Object) -join '|'))
'@, $Latin1)
        $guardOut = (@(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $guardSeam 2>&1 |
                    ForEach-Object { [string]$_ }) -join "`n")
        $vsKey = 'a/' + [string][char]0x0FE00
        Assert-True -Condition ($guardOut -cmatch [regex]::Escape($vsKey)) `
            -Message 'the walk ASKED about a zero-weight segment -- it left its key in the cache instead of being dropped'
        Assert-True -Condition ($guardOut -cmatch [regex]::Escape($vsKey + '/b')) `
            -Message 'and carried it into the prefix of the segment below, so the path it asks about is the path it was given'

        # ---- The segment the walk used to DROP (#753 in #698's own guard).
        #
        # `Find-ReparsePointSegment` filtered its segments with `$_ -ne ''`, and a segment that is a
        # lone zero-weight code point compares -eq to the empty string. So it was dropped from the
        # walk, the guard never asked about it, and the traversal this whole pull request refuses
        # went through -- on exactly the path shape somebody would pick to defeat it. The guard
        # failing OPEN, in the guard's own file.
        #
        # NTFS accepts the name and git tracks the path (measured: `d<U+FE00>/a.sh` appears in
        # `git ls-files`), so this is constructible rather than theoretical.
        $weightless = New-Fixture -Name 'weightless'
        $vs = [string][char]0x0FE00
        # A LONE zero-weight segment, not "d$vs". The first version of this cell used a letter
        # followed by the code point -- which is not empty-equivalent, so nothing dropped it and the
        # cell passed under the sabotage it was written to catch. Measured, not reasoned: the
        # sabotage reddened the seam cells and left this one green until the name changed.
        $weightlessDir = Join-Path $weightless $vs
        [System.IO.Directory]::CreateDirectory($weightlessDir) | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $weightlessDir 'inside.sh'), "echo one`necho two`n", $Latin1)
        Push-Location $weightless
        try {
            & git add -A 2>&1 | Out-Null
            & git commit -m weightless --quiet 2>&1 | Out-Null
            [System.IO.File]::WriteAllText((Join-Path $weightlessDir 'inside.sh'), "echo one`r`necho two`r`n", $Latin1)
        } finally { Pop-Location }

        # -z, so git does not apply core.quotePath and hand back an escaped spelling of the name --
        # the same reason the program itself reads with -z. Asserting the PATH rather than a count:
        # a count is satisfied by the fixture's own two files and would have passed without the
        # zero-weight path ever being tracked, which is exactly what it did on the first run.
        $trackedRaw = (& git -C $weightless ls-files -z) -join ''
        $weightlessPath = [string][char]0x0FE00 + "/inside.sh"
        Assert-True -Condition ($trackedRaw -cmatch [regex]::Escape($weightlessPath)) `
            -Message 'ARRANGEMENT: git really tracks a path THROUGH a zero-weight directory name'

        # Replace that directory with a junction, exactly as the cell above does with an ASCII name.
        [System.IO.Directory]::CreateDirectory((Join-Path $weightless 'decoy')) | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $weightless 'decoy/inside.sh'), "echo one`necho two`n", $Latin1)
        Rename-Item -LiteralPath $weightlessDir -NewName "actual$vs"
        $weightlessJunction = Join-Path $weightless $vs
        $madeSecond = $null -ne (New-Item -ItemType Junction -Path $weightlessJunction -Value (Join-Path $weightless 'decoy') -ErrorAction SilentlyContinue)
        if (-not $madeSecond) {
            Write-Host 'HARNESS-BROKE: a junction with a zero-weight name could not be created' -ForegroundColor Magenta
            exit 2
        }
        try {
            $weightlessRun = Invoke-Program -Repo $weightless
            Assert-True -Condition ($weightlessRun.exitCode -eq 1 -and $weightlessRun.text -cmatch 'reparse point') `
                -Message "the guard walks a zero-weight segment instead of dropping it, and refuses (exit $($weightlessRun.exitCode))"
        } finally {
            [System.IO.Directory]::Delete($weightlessJunction)
        }

    } finally {
        # The reparse point is removed by itself, before the suite's own recursive delete reaches
        # it. Directory.Delete on a junction removes the LINK; a recursive remove that followed it
        # would be deleting through it, which is the very behaviour this cell exists to name.
        [System.IO.Directory]::Delete($junction)
    }
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
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
    Write-Host "HARNESS-BROKE: ran $script:total assertions and skipped $script:skipped, expected population $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "fixture setup count: $script:fixtureCount (35 pre-existing + 1 config proof); local config setup launches avoided: 210 for the pre-existing fixtures, $script:removedLocalConfigCalls equivalent for this run"
Write-Host "$passed/$script:total passed ($script:skipped skipped)" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
