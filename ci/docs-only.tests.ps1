# ci/docs-only.ps1 decides whether the queued runner may skip building a change (#901).
#
# Every cell drives the real script against a throwaway git repository, because the decision is a
# fact about a TREE (which files name which) and a cell that fed the script strings instead of a
# repository would test the string handling and not the grep that makes the call.
#
# The failure this suite exists to catch is the one that SKIPS a gate it should not: a Markdown file
# a test reads by name, a package contribution, a non-Markdown file riding along, and an instrument
# that finds nothing because it looked at nothing. Each of those must build (exit 1 or 2), and the
# one positive case must skip (exit 0), or the script is either useless or dangerous.

$ExpectedAssertionCount = 26
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

$script:scriptPath = Join-Path $PSScriptRoot 'docs-only.ps1'
$root = Join-Path ([System.IO.Path]::GetTempPath()) ("docs-only-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $root -Force | Out-Null

function Invoke-Git {
    param([Parameter(ValueFromRemainingArguments)] [string[]] $GitArgs)
    $null = & git -C $root @GitArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw "git $($GitArgs -join ' ') exited $LASTEXITCODE" }
}

function Write-RepoFile {
    param([string] $Path, [string] $Text)
    $full = Join-Path $root $Path
    New-Item -ItemType Directory -Path (Split-Path $full -Parent) -Force | Out-Null
    [System.IO.File]::WriteAllText($full, $Text)
}

function Invoke-Decision {
    param([string] $Changed, [string] $Head = 'HEAD', [switch] $FromDiff, [string] $MergeBase)
    $arguments = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $script:scriptPath,
        '-RepoRoot', $root, '-Head', $Head)
    if ($FromDiff) { if ($MergeBase) { $arguments += @('-MergeBase', $MergeBase) } } else { $arguments += @('-ChangedFiles', $Changed) }
    $output = & powershell @arguments 2>&1
    $code = $LASTEXITCODE
    $json = $null
    try { $json = (($output | ForEach-Object { [string]$_ }) -join "`n") | ConvertFrom-Json } catch { $json = $null }
    return [pscustomobject]@{ Code = $code; Json = $json; Text = (($output | Out-String)) }
}

try {
    Invoke-Git init -q
    Invoke-Git config user.email 'docs-only@test'
    Invoke-Git config user.name 'docs-only test'
    Invoke-Git config core.autocrlf false
    Write-RepoFile 'README.md' "# project`n"
    Write-RepoFile 'docs/paper.md' "a paper nobody reads by name`n"
    Write-RepoFile 'docs/read-by-a-test.md' "a document a test includes`n"
    Write-RepoFile 'src/lib.rs' "const DOC: &str = include_str!(`"../docs/read-by-a-test.md`");`n"
    Write-RepoFile 'ci/check.ps1' "Get-Content README.md`n"
    Write-RepoFile 'extensions/pkg/skills/x/SKILL.md' "a skill`n"
    Write-RepoFile '.factory/gate-runs/abc.json' '{"mentions":"paper.md"}'
    Invoke-Git add -A
    Invoke-Git commit -q -m base
    $base = (& git -C $root rev-parse HEAD).Trim()

    # POSITIVE: a Markdown file no non-Markdown file names. The receipt that mentions it does not count.
    $r = Invoke-Decision -Changed 'docs/paper.md'
    Assert-True ($r.Code -eq 0) "an unread doc is docs-only (exit $($r.Code): $($r.Text.Trim()))"
    Assert-True ($null -ne $r.Json -and $r.Json.docsOnly -eq $true -and $r.Json.decided -eq $true) 'and the verdict says docsOnly and decided'
    Assert-True ($r.Json.files -contains 'docs/paper.md') 'and names the file it judged'

    # A receipt riding along does not make a docs change mixed.
    $r = Invoke-Decision -Changed 'docs/paper.md;.factory/gate-runs/abc.json'
    Assert-True ($r.Code -eq 0) "a gate receipt alongside an unread doc is still docs-only (exit $($r.Code))"

    # A doc that code reads by name builds, and the reader is named.
    $r = Invoke-Decision -Changed 'docs/read-by-a-test.md'
    Assert-True ($r.Code -eq 1) "a doc a Rust file includes is NOT docs-only (exit $($r.Code))"
    Assert-True ($r.Json.readers.Count -eq 1 -and $r.Json.readers[0].namedBy -contains 'src/lib.rs') "and the reader src/lib.rs is named ($($r.Text.Trim()))"

    # The README is named by a script: builds.
    $r = Invoke-Decision -Changed 'README.md'
    Assert-True ($r.Code -eq 1) "a README a script names is NOT docs-only (exit $($r.Code))"

    # One unread doc and one read doc together: builds.
    $r = Invoke-Decision -Changed 'docs/paper.md;docs/read-by-a-test.md'
    Assert-True ($r.Code -eq 1) "a mix of an unread and a read doc is NOT docs-only (exit $($r.Code))"

    # Any non-Markdown path: builds.
    $r = Invoke-Decision -Changed 'docs/paper.md;src/lib.rs'
    Assert-True ($r.Code -eq 1) "a doc with a Rust file is NOT docs-only (exit $($r.Code))"
    Assert-True ($r.Json.reason -like 'not Markdown: src/lib.rs*') "and the reason names the non-Markdown path ($($r.Json.reason))"
    $r = Invoke-Decision -Changed 'docs/notes.MD.txt'
    Assert-True ($r.Code -eq 1) "a path that only contains '.md' is not Markdown (exit $($r.Code))"

    # Package contributions are digest-bound: builds, whatever the grep says.
    $r = Invoke-Decision -Changed 'extensions/pkg/skills/x/SKILL.md'
    Assert-True ($r.Code -eq 1) "a Markdown package contribution is NOT docs-only (exit $($r.Code))"
    Assert-True ($r.Json.reason -like 'a package contribution*') "and the reason says why ($($r.Json.reason))"

    # Test data is walked, not named (review of #1216, lane 5bdc38): builds, whatever the grep says.
    $r = Invoke-Decision -Changed 'adapters/tool-host/tests/fixtures/context-quality/tree/docs/context/X.md'
    Assert-True ($r.Code -eq 1) "Markdown under a tests/ or fixtures/ tree is NOT docs-only (exit $($r.Code))"
    Assert-True ($r.Json.reason -like 'test data*') "and the reason says why ($($r.Json.reason))"
    $r = Invoke-Decision -Changed 'docs/testing-guide.md'
    Assert-True ($r.Code -eq 0) "CONTROL: a file NAMED like a test, outside a test directory, is still docs-only (exit $($r.Code))"

    # Nothing to judge: undecided, so the caller builds.
    $r = Invoke-Decision -Changed '.factory/gate-runs/abc.json'
    Assert-True ($r.Code -eq 2) "receipts alone are undecided (exit $($r.Code))"
    Assert-True ($r.Json.decided -eq $false) 'and say so in the verdict'

    # A broken instrument must not read as "nobody reads it".
    $r = Invoke-Decision -Changed 'docs/paper.md' -Head 'no-such-revision'
    Assert-True ($r.Code -eq 2) "an unresolvable head is undecided, never docs-only (exit $($r.Code))"
    Assert-True ($r.Json.reason -like 'positive control failed*') "and the positive control is what refused ($($r.Json.reason))"

    # From the diff, the way the runner calls it.
    Write-RepoFile 'docs/paper.md' "a paper nobody reads by name, revised`n"
    Invoke-Git add -A
    Invoke-Git commit -q -m 'docs change'
    $r = Invoke-Decision -FromDiff -MergeBase $base
    Assert-True ($r.Code -eq 0) "a docs-only commit is docs-only when derived from the diff (exit $($r.Code))"
    Write-RepoFile 'src/lib.rs' "const DOC: &str = include_str!(`"../docs/read-by-a-test.md`"); // edited`n"
    Invoke-Git add -A
    Invoke-Git commit -q -m 'code change'
    $r = Invoke-Decision -FromDiff -MergeBase $base
    Assert-True ($r.Code -eq 1) "the same range plus a code commit is NOT docs-only (exit $($r.Code))"
    $r = Invoke-Decision -FromDiff -MergeBase ''
    Assert-True ($r.Code -eq 2) "no merge base is undecided (exit $($r.Code))"

    # A doc named by a doc only is still unread: Markdown never reads Markdown.
    Write-RepoFile 'docs/index.md' "see paper.md`n"
    Invoke-Git add -A
    Invoke-Git commit -q -m 'index'
    $r = Invoke-Decision -Changed 'docs/paper.md'
    Assert-True ($r.Code -eq 0) "a doc linked only from another doc is docs-only (exit $($r.Code))"

    # A RENAME FROM CODE TO MARKDOWN IS A CODE CHANGE (review of #1216, lane b9deb2).
    Write-RepoFile 'src/extra.rs' "pub fn extra() {}`n"
    Invoke-Git add -A
    Invoke-Git commit -q -m 'extra module'
    $renameBase = (& git -C $root rev-parse HEAD).Trim()
    Invoke-Git mv src/extra.rs docs/extra-notes.md
    Invoke-Git commit -q -m 'rename code to markdown'
    $r = Invoke-Decision -FromDiff -MergeBase $renameBase
    Assert-True ($r.Code -eq 1) "a rename of a .rs file to a .md file is NOT docs-only (exit $($r.Code): $($r.Text.Trim()))"

    # Controls on the fixture itself, so a cell cannot pass because the fixture was empty.
    $tracked = @(& git -C $root ls-files)
    Assert-True ($tracked -contains 'src/lib.rs' -and $tracked -contains 'ci/check.ps1') 'CONTROL: the fixture carries the readers the cells rely on'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount" -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total) of $($script:total)" -ForegroundColor Green
exit 0
