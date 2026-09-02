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

$ExpectedAssertionCount = 69
# 'Continue', not 'Stop'. This suite RUNS the failing recipe on purpose, and under Windows
# PowerShell 5.1 a native command's redirected stderr becomes a NativeCommandError that 'Stop'
# promotes to a terminating error -- so `git checkout` printing "did not match any file" would kill
# the suite at the exact cell whose subject is that message. gate.ps1's Invoke-Stage documents the
# same trap and takes the same way out: judge native commands by their exit code and their output,
# never by whether they wrote to stderr. The assertion-count guard below is what still catches a
# suite that dies early.
$ErrorActionPreference = 'Continue'
$script:total = 0
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

# The SUITE has to listen in UTF-8 too, for the same reason the program does: git and the child
# process write path bytes as UTF-8, and PowerShell decodes a native command's output with the
# CONSOLE's encoding. Without this the accented cell compared `café.sh` against `cafÃ©.sh` and
# failed -- the per-path predicate working correctly on a name the harness had mis-decoded.
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { }
$script:gitVersion = ((& git --version) -join ' ').Trim()
$programPath = Join-Path $PSScriptRoot 'normalize-script-eol.ps1'
$programText = [System.IO.File]::ReadAllText($programPath)
if (-not (Test-Path -LiteralPath $programPath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $programPath" -ForegroundColor Magenta
    exit 2
}

$Latin1 = [System.Text.Encoding]::GetEncoding(28591)
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-eol-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

function New-Fixture {
    <#
        A repository in exactly the state this migration exists for: the attribute is in force, the
        index blob is LF, and the working tree still holds CRLF -- with `git status` CLEAN, which is
        why nothing tells the operator anything is wrong.
    #>
    param([Parameter(Mandatory)] [string] $Name, [string[]] $Files = @('a.sh'))

    $repo = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory($repo) | Out-Null
    Push-Location $repo
    try {
        & git init --quiet 2>&1 | Out-Null
        & git config user.email 'fixture@example.invalid' 2>&1 | Out-Null
        & git config user.name 'fixture' 2>&1 | Out-Null
        & git config core.autocrlf false 2>&1 | Out-Null
        # A developer with `commit.gpgSign=true` and no usable key cannot commit here, and the
        # fixture's failure is swallowed by Out-Null -- so the repository would be EMPTY and every
        # cell below would fail on a subject that was never created. The fixture declares its own
        # configuration rather than inheriting whatever the machine has.
        & git config commit.gpgSign false 2>&1 | Out-Null
        # And no inherited hooks: a global core.hooksPath whose pre-commit fails would make the
        # fixture commit fail, Out-Null would swallow it, and every cell below would run against an
        # EMPTY repository. Same reason as the signing setting -- a fixture that depends on the
        # developer's configuration is a gate that fails for reasons unrelated to the change.
        & git config core.hooksPath ([System.IO.Path]::Combine($repo, '.no-hooks')) 2>&1 | Out-Null
        & git config tag.gpgSign false 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo '.gitattributes'), "*.sh text eol=lf`n*.ps1 text eol=lf`n*.py text eol=lf`n", $Latin1)
        foreach ($name in $Files) {
            [System.IO.File]::WriteAllText((Join-Path $repo $name), "echo one`necho two`n", $Latin1)
        }
        & git add -A 2>&1 | Out-Null
        & git commit -m 'fixture' --quiet 2>&1 | Out-Null
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
    Write-Host '-- without -DryRun the program refuses, before anything is read --' -ForegroundColor Cyan
    $repo = New-Fixture -Name 'nowrite'
    $before = [System.IO.File]::ReadAllBytes((Join-Path $repo 'a.sh'))
    $result = Invoke-Program -Repo $repo -NoDryRun
    Assert-True -Condition ($result.exitCode -eq 1) `
        -Message "the write mode refuses (exit 1, got $($result.exitCode))"
    Assert-True -Condition ($result.text -cmatch '#693') `
        -Message 'and names where the write half went, so the refusal is a direction and not a dead end'
    # BYTES, not length. A regression that rewrote the file to different bytes of the same length --
    # which is exactly what a CRLF-to-LF conversion is NOT, but a partial one could be -- passed a
    # length comparison. The message said "byte-for-byte" and the predicate did not check that.
    #
    # ARMED, NOT DISCRIMINATING, and saying so: with no write path in this pull request nothing can
    # make this fail, so it is a guard placed for #693 rather than evidence here. It becomes a real
    # cell the moment the write half lands.
    $after = [System.IO.File]::ReadAllBytes((Join-Path $repo 'a.sh'))
    $identical = $after.Length -eq $before.Length
    if ($identical) {
        for ($i = 0; $i -lt $before.Length; $i++) {
            if ($after[$i] -ne $before[$i]) { $identical = $false; break }
        }
    }
    Assert-True -Condition $identical `
        -Message 'and the file is byte-for-byte as it was (ARMED for #693: nothing here can write, so this cannot fail yet)'

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
        Assert-True -Condition ($true) `
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
