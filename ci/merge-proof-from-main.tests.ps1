# #733: run from the candidate's own checkout, the pull request supplies the predicate that judges
# it. `ci/merge-proof-from-main.ps1` is the procedure made executable, and the property worth a cell
# is the one the issue says a cell cannot reach:
#
#   THE BYTES EXECUTED ARE MAIN'S, NOT THE CANDIDATE'S.
#
# The issue is right that a cell cannot exercise the bypass by running a candidate which deleted the
# comparison -- that measures a different program. But it CAN exercise the wrapper: sabotage the
# candidate's copy to `exit 0`, which certifies anything, and observe that the verdict is unmoved.
# The sabotage is the real one, not a stand-in.
#
# Each cell builds a THROWAWAY CLONE with `git clone --local`, so `origin` points at this
# repository and the wrapper's `git fetch origin main` works with no network. The candidate's copy
# is edited in the clone's WORKING TREE, which is exactly where a hostile author would edit it, and
# exactly what the wrapper must ignore because it reads the object store.
#
# Measured on Windows PowerShell 5.1.

$ExpectedAssertionCount = 13
$ErrorActionPreference = 'Continue'
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

$wrapper = Join-Path $PSScriptRoot 'merge-proof-from-main.ps1'
if (-not (Test-Path -LiteralPath $wrapper)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $wrapper" -ForegroundColor Magenta
    exit 2
}
$repository = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path

function Invoke-Wrapper {
    param([Parameter(Mandatory)] [string] $Root, [int] $Number = 1)
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $wrapper,
        '-PullRequest', "$Number", '-RepositoryRoot', $Root)
    $out = @(& powershell.exe @arguments 2>&1 | ForEach-Object { [string]$_ })
    $joined = ($out -join " ")
    # WHITESPACE COLLAPSED BEFORE MATCHING. `Write-Host` wraps at the console width, so a long
    # line arrives split mid-token and a substring assertion fails for a reason that has nothing
    # to do with the subject -- which is how the first version of this suite reported that the
    # wrapper never named the file it ran, while the blob id on the same line matched.
    $flat = [System.Text.RegularExpressions.Regex]::Replace($joined, '\s+', ' ')
    return [ordered]@{ code = $LASTEXITCODE; text = $flat }
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "mpfm-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

try {
    Write-Host ''
    Write-Host '-- the clone the cells judge --' -ForegroundColor Cyan

    # A BARE ORIGIN BUILT ON PURPOSE, not a clone of this checkout, and the reason is a trap worth
    # recording. `git clone` copies the SOURCE'S LOCAL BRANCHES into the clone's `origin/*`, so a
    # clone of this worktree gets the local `main` -- which here is months stale and predates
    # `ci/merge-proof.ps1` existing. Repairing that ref afterwards is not enough either: THE
    # WRAPPER'S OWN `git fetch origin main` PUTS THE STALE ONE BACK, because that is what the
    # source's `main` says. So the fixture's origin must genuinely hold the commit under test.
    $origin = Join-Path $fixtureRoot 'origin.git'
    & git init --quiet --bare "$origin" 2>&1 | Out-Null
    & git push --quiet "$origin" 'origin/main:refs/heads/main' 2>&1 | Out-Null
    $pushed = $LASTEXITCODE

    $clone = Join-Path $fixtureRoot 'candidate'
    # `--branch main` because `git init --bare` points HEAD at `master`, so a plain clone of this
    # origin checks out NOTHING and leaves a directory holding only `.git`.
    & git clone --quiet --branch main "$origin" "$clone" 2>&1 | Out-Null
    Assert-True -Condition ($pushed -eq 0 -and $LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath (Join-Path $clone 'ci/merge-proof.ps1'))) `
        -Message 'ARRANGEMENT: a bare origin holds this main, and the candidate clone carries the verifier'

    $expected = (& git -C "$clone" rev-parse 'origin/main:ci/merge-proof.ps1' 2>$null | Select-Object -First 1)
    # A FORTY-HEX BLOB, not merely non-empty. `git rev-parse` ECHOES ITS ARGUMENT when it cannot
    # resolve, so `-not IsNullOrWhiteSpace` accepted the string `origin/main:ci/merge-proof.ps1`
    # as a blob id and this arrangement passed while the fixture was broken. An arrangement check
    # that accepts the failure mode is worse than none: it makes the cells below look attributable.
    Assert-True -Condition ([string]$expected -cmatch '^[0-9a-f]{40}$') `
        -Message "ARRANGEMENT: origin/main holds a verifier blob ($expected)"

    Write-Host ''
    Write-Host '-- the wrapper names the bytes it runs, and they are main s --' -ForegroundColor Cyan

    $clean = Invoke-Wrapper -Root $clone
    # NOT an assertion about the VERDICT. Whether a bogus pull request is ABSENT or HARNESS-BROKE
    # depends on whether `gh` can reach GitHub, and this suite must not decide differently offline.
    # What is invariant is which file ran.
    Assert-True -Condition ($clean.text -cmatch 'running origin/main:ci/merge-proof\.ps1') `
        -Message 'the wrapper says which copy it is running'
    # THE BLOB MUST APPEAR IN THE RUNNING LINE, not merely somewhere in the output. The first
    # version asserted only that the id was present, and it PASSED while the wrapper was refusing
    # -- because the refusal message quotes the blob it expected. An assertion satisfied by the
    # failure it is meant to exclude is the shape this whole PR is about.
    Assert-True -Condition ($clean.text -cmatch ('running origin/main:ci/merge-proof\.ps1 \(blob ' + [regex]::Escape($expected))) `
        -Message 'and the RUNNING line carries main s blob id'

    # CONTROL, and the cell below is worthless without it: the unsabotaged run must NOT already be
    # exit 0. If it were, "still not 0 under sabotage" would be true of a wrapper that does nothing.
    Assert-True -Condition ($clean.code -ne 0) `
        -Message "CONTROL: a bogus pull request does not certify, so 0 below would mean something (exit $($clean.code))"

    Write-Host ''
    Write-Host '-- THE CELL: the candidate cannot supply its own predicate --' -ForegroundColor Cyan

    # The real sabotage, not a stand-in: a verifier that certifies ANYTHING. Written into the
    # clone's WORKING TREE, which is where a hostile author edits, and which the wrapper must
    # ignore because it reads the object store.
    [System.IO.File]::WriteAllText((Join-Path $clone 'ci/merge-proof.ps1'), "exit 0`n",
        (New-Object System.Text.UTF8Encoding($false)))

    $direct = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $clone 'ci/merge-proof.ps1'))
    & powershell.exe @direct 2>&1 | Out-Null
    Assert-True -Condition ($LASTEXITCODE -eq 0) `
        -Message 'CONTROL: run directly, the sabotaged copy certifies anything (exit 0) -- so the sabotage is real'

    $sabotaged = Invoke-Wrapper -Root $clone
    Assert-True -Condition ($sabotaged.code -ne 0) `
        -Message "the wrapper is UNMOVED by a candidate that replaced the verifier with ``exit 0`` (exit $($sabotaged.code))"
    Assert-True -Condition ($sabotaged.text -cmatch ('running origin/main:ci/merge-proof\.ps1 \(blob ' + [regex]::Escape($expected))) `
        -Message 'and still RAN main s blob, so it read the object store rather than the working tree'

    Write-Host ''
    Write-Host '-- the bootstrap spelling the docs must NOT give (#811 review) --' -ForegroundColor Cyan

    # PINNED AGAINST THE REAL SHELL, because three documents' warnings rest on this being true.
    # `>` in Windows PowerShell 5.1 is `Out-File`, which re-encodes: the extracted runner is not
    # main's bytes and still parses, which is the failure the blob check exists for occurring in
    # the layer ABOVE the blob check. If a future PowerShell stops re-encoding, the warnings in
    # AGENTS.md, merge-proof.ps1's .EXAMPLE and this wrapper's header become wrong, and the next
    # reader should find that out here rather than by trusting a stale caution.
    $redirected = Join-Path $fixtureRoot 'redirected.ps1'
    & git -C "$clone" show 'origin/main:ci/merge-proof.ps1' > $redirected
    $viaRedirect = (& git -C "$clone" hash-object --no-filters -- $redirected 2>$null | Select-Object -First 1)
    Assert-True -Condition ([string]$viaRedirect -cmatch '^[0-9a-f]{40}$' -and -not [string]::Equals([string]$viaRedirect, $expected, [System.StringComparison]::Ordinal)) `
        -Message "``git show ... > file`` does NOT reproduce main's bytes (main $expected, redirect $viaRedirect)"

    # AND IT STILL PARSES, which is why nothing downstream would notice. A corrupted runner that
    # failed to load would be a loud problem; one that loads is the quiet one.
    $parseErrors = $null
    $null = [System.Management.Automation.Language.Parser]::ParseFile($redirected, [ref]$null, [ref]$parseErrors)
    Assert-True -Condition (@($parseErrors).Count -eq 0) `
        -Message 'and the re-encoded copy PARSES CLEANLY, so the corruption is silent'

    Write-Host ''
    Write-Host '-- it refuses rather than judging the wrong tree --' -ForegroundColor Cyan

    $missing = Invoke-Wrapper -Root (Join-Path $fixtureRoot 'no-such-checkout')
    Assert-True -Condition ($missing.text -cmatch 'HARNESS-BROKE') `
        -Message 'a -RepositoryRoot that does not exist says HARNESS-BROKE'

    # THE NUMBER MUST AGREE WITH THE WORD, and the first version of this script failed exactly
    # here: it printed HARNESS-BROKE and exited 2, which is `ci/merge-proof.ps1`'s code for NOT --
    # a judgement about the candidate. `ci/merge-proof.tests.ps1` pins 2 to the NOT verdict, so a
    # caller reading the number was told to fix a branch that was fine. Asserted as an EQUALITY
    # rather than as "non-zero", because non-zero is what the defect satisfied.
    Assert-True -Condition ($missing.code -eq 1) `
        -Message "and exits 1, the verifier's HARNESS-BROKE code, never 2 which means NOT (got $($missing.code))"

    Assert-True -Condition ($missing.code -ne 2) `
        -Message 'and specifically not 2, which an existing cell in merge-proof.tests.ps1 pins to NOT'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = 'Green'
if ($script:failures -gt 0) { $color = 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
