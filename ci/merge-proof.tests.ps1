# #674(b): does a recorded run vouch for the head a pull request would merge?
#
# The subject is ci/merge-proof.ps1, run against real throwaway repositories with real commits and
# real manifest files. The whole question is about git's own answers -- what the parent of a head
# is, what a tip touched -- and no mock of that would be evidence about it.
#
# Every cell drives the script with -Head and -RepositoryRoot so nothing here needs `gh` or a
# network: the lookup this replaces is the one thing that cannot be exercised offline, and a suite
# that needs the network is a gate that fails for reasons unrelated to the change.

# Exit codes are the consumer's scheme, agreed with the desk that calls this: 0 SATISFIED,
# 1 THE TOOL BROKE, 2 NOT, 3 ABSENT. 1 is reserved for a broken tool so a caller treating
# "non-zero" as "refused" can never refuse a merge because this script failed to run.
$ExpectedAssertionCount = 146
# 'Continue', not 'Stop': these cells run git and the subject against fixtures that are meant to
# fail, and under Windows PowerShell 5.1 a native command's redirected stderr becomes a
# NativeCommandError that 'Stop' promotes to a terminating error.
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

try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { }

$subjectPath = Join-Path $PSScriptRoot 'merge-proof.ps1'
if (-not (Test-Path -LiteralPath $subjectPath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $subjectPath" -ForegroundColor Magenta
    exit 2
}

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-mergeproof-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function New-Repo {
    <#
        A repository with a commit, and a `ci/gate.ps1` that decides the MODE.

        The advisory/blocking switch is read from the repository under test, not from a flag, so a
        fixture chooses its mode by whether its gate carries (a)'s function -- which is the same
        fact the real switch reads. A fixture that set the mode some other way would be testing a
        different program.
    #>
    param([Parameter(Mandatory)] [string] $Name, [switch] $ProvenanceInGate)

    $repo = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory((Join-Path $repo 'ci')) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $repo '.factory/gate-runs')) | Out-Null
    Push-Location $repo
    try {
        & git init --quiet 2>&1 | Out-Null
        & git config user.email 'fixture@example.invalid' 2>&1 | Out-Null
        & git config user.name 'fixture' 2>&1 | Out-Null
        # Declared, never inherited: a developer with commit.gpgSign and no key, or a global
        # pre-commit hook that fails, would otherwise leave an EMPTY repository here and every cell
        # below would fail on a subject that was never created.
        & git config commit.gpgSign false 2>&1 | Out-Null
        & git config core.hooksPath ([System.IO.Path]::Combine($repo, '.no-hooks')) 2>&1 | Out-Null
        $gate = if ($ProvenanceInGate) { "function Get-HeadProvenance { }`n" } else { "# no provenance here`n" }
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/gate.ps1'), $gate, $utf8NoBom)
        [System.IO.File]::WriteAllText((Join-Path $repo 'work.txt'), "one`n", $utf8NoBom)
        & git add -A 2>&1 | Out-Null
        & git commit -m 'fixture' --quiet 2>&1 | Out-Null
        # AN ORIGIN WITH A MAIN, because the subject asks main whether (a) has landed -- not the
        # working tree. A fixture without one made every cell HARNESS-BROKE, which is the correct
        # behaviour for a tool that cannot read the fact it needs, and it is why this setup is part
        # of the subject's contract rather than scaffolding.
        $bare = "$repo.origin.git"
        & git init --bare --quiet $bare 2>&1 | Out-Null
        & git remote add origin $bare 2>&1 | Out-Null
        & git push --quiet origin HEAD:refs/heads/main 2>&1 | Out-Null
        & git fetch --quiet origin 2>&1 | Out-Null
    } finally { Pop-Location }
    return $repo
}

function Add-Manifest {
    <#
        Writes a manifest AND COMMITS IT, because that is what the gate does and what the subject
        reads. The first version of this suite only wrote the file, and every cell passed against a
        manifest that was never committed -- which was the whole finding: the subject was reading a
        working tree, so an uncommitted file satisfied a proof about a commit. `-Uncommitted` exists
        for the two cells whose subject IS that distinction.
    #>
    param(
        [Parameter(Mandatory)] [string] $Repo, [Parameter(Mandatory)] [hashtable] $Body,
        [string] $Name = 'run.json', [switch] $Uncommitted,
        # The bytes to write, INSTEAD of serializing $Body. An unpaired surrogate does not survive
        # ConvertTo-Json plus a UTF-8 write -- it comes back as nothing at all, so the fixture that
        # was meant to carry one wrote a clean manifest and the cell passed for the wrong reason.
        # A manifest is a FILE, and a file can hold a JSON escape that no PowerShell string round
        # trip will preserve.
        [string] $RawJson
    )
    # `dirtyDiffHash` is written by the gate on EVERY run -- null when the worktree was clean -- so
    # a fixture without it is not a manifest the gate could have produced. Defaulted here rather than
    # in every cell, and overridable by the cells whose subject IS this field.
    if (-not $Body.ContainsKey('dirtyDiffHash')) { $Body['dirtyDiffHash'] = $null }
    $path = Join-Path (Join-Path $Repo '.factory/gate-runs') $Name
    $text = if ($PSBoundParameters.ContainsKey('RawJson')) { $RawJson } else { $Body | ConvertTo-Json -Depth 6 }
    [System.IO.File]::WriteAllText($path, $text, $utf8NoBom)
    if (-not $Uncommitted) {
        Push-Location $Repo
        try {
            & git add -- ".factory/gate-runs/$Name" 2>&1 | Out-Null
            & git commit --quiet -m "gate: run manifest" 2>&1 | Out-Null
        } finally { Pop-Location }
    }
    return $path
}

function Commit-Store {
    <# A pure tip: the manifest store and nothing else, exactly as (a) commits it. #>
    param([Parameter(Mandatory)] [string] $Repo, [string] $Message = 'gate: run manifest')
    Push-Location $Repo
    try {
        & git add -- '.factory/gate-runs' 2>&1 | Out-Null
        & git commit --quiet -m $Message 2>&1 | Out-Null
        return (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
}

function Invoke-Proof {
    param([Parameter(Mandatory)] [string] $Repo, [Parameter(Mandatory)] [int] $PullRequest, [Parameter(Mandatory)] [string] $Head, [switch] $Json)
    # Two explicit call forms rather than one with a splatted argument array. With `@extra` the
    # -Json run came back EMPTY with a verdict exit code, while the identical command typed by hand
    # produced 647 bytes of JSON -- so the splat, not the subject, was the difference. A harness
    # that invokes the subject differently from the way an operator does is measuring a different
    # program, and that is worth two lines to avoid.
    $out = if ($Json) {
        @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
                -PullRequest $PullRequest -Head $Head -RepositoryRoot $Repo -Json 2>&1 | ForEach-Object { [string]$_ })
    } else {
        @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
                -PullRequest $PullRequest -Head $Head -RepositoryRoot $Repo 2>&1 | ForEach-Object { [string]$_ })
    }
    return [ordered]@{ exitCode = $LASTEXITCODE; text = $out -join "`n" }
}

try {
    # ---- The head itself is named: the simple case, and the control for every other cell.
    Write-Host ''
    Write-Host '-- a manifest naming the head PARENT, with a pure tip --' -ForegroundColor Cyan
    # THE LABEL WAS WRONG AND THE MECHANISM IS WORTH MORE THAN THE LABEL. `Add-Manifest` COMMITS,
    # so the head passed here is the manifest commit and the manifest names its parent: this cell
    # has always exercised the parent-with-pure-tip rule, never the equality one.
    #
    # And the equality branch cannot be reached by any store git can produce. It fires when a
    # manifest names the very commit whose tree contains it, and a commit's sha is computed FROM
    # that tree -- so the record would have to name a sha it is an input to. The gate's design is
    # the consequence, not a choice: it commits the manifest naming the PARENT, which is exactly why
    # the parent rule exists.
    #
    # The branch stays: it costs one comparison and it is the honest reading of "does this record
    # vouch for this head". What changes is that no cell claims to exercise it, and this note says
    # why none can.
    $repo = New-Repo -Name 'parentpure' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $head } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($tip -cne $head) `
        -Message 'ARRANGEMENT: the manifest commit is the head, and the record names its parent'
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0) -Message "SATISFIED exits 0 (got $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'SATISFIED') -Message 'and says SATISFIED'
    Assert-True -Condition ($r.text -cmatch 'it names the parent') `
        -Message 'and the reason names the PARENT rule, which is the branch this fixture can reach'
    Assert-True -Condition ($r.text -cmatch 'BLOCKING') `
        -Message 'and reports BLOCKING mode, because this fixture gate records provenance'

    # ---- The real shape: the manifest names the PARENT, because committing it moved the head.
    Write-Host ''
    Write-Host '-- the manifest names the parent, and the tip adds only the record --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'parent' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    # Add-Manifest commits, so the manifest lives in the tip and names its parent -- the shape (a)
    # actually produces.
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0 -and $r.text -cmatch 'SATISFIED') `
        -Message "a parent manifest with a pure tip is SATISFIED (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'names the parent') `
        -Message 'and the reason says so, so nobody has to infer which branch of the rule applied'

    # ---- The exception is earned by the tip being pure, and by nothing else.
    Write-Host ''
    Write-Host '-- a tip that also carries other work is NOT vouched for --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'impure' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    # One commit carrying BOTH: the manifest names this commit's parent and the tip is impure.
    # Committing the manifest first would make it the grandparent, and the cell would be about a
    # different rule.
    Add-Manifest -Repo $repo -Uncommitted -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $repo 'work.txt'), "the author's change`n", $utf8NoBom)
    Push-Location $repo
    try {
        & git add -A 2>&1 | Out-Null
        & git commit --quiet -m 'manifest AND work' 2>&1 | Out-Null
        $tip = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'NOT') `
        -Message "an impure tip is NOT (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'work\.txt') `
        -Message 'and the file that spoiled it is named, not merely counted'
    Assert-True -Condition ($r.text -cmatch 'M work\.txt' -or $r.text -cmatch 'A work\.txt') `
        -Message 'with the CHANGE KIND beside it, which is what makes a deletion distinguishable from an addition'

    # ---- An ancestor is not a parent. This is the cell the is-ancestor sabotage must redden.
    Write-Host ''
    Write-Host '-- a manifest naming a GRANDPARENT is not vouching for the head --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'grandparent' -ProvenanceInGate
    $grand = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $grand } | Out-Null
    $middle = Commit-Store -Repo $repo -Message 'gate: first'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 99; headSha = $middle } -Name 'other.json' | Out-Null
    $tip = Commit-Store -Repo $repo -Message 'gate: second'
    # Both halves measured separately: a semicolon inside a parenthesised condition is not an
    # expression in PowerShell, and cramming them together was a parse error rather than a test.
    & git -C $repo merge-base --is-ancestor $grand $tip 2>$null | Out-Null
    $isAncestor = ($LASTEXITCODE -eq 0)
    $tipParent = (& git -C $repo rev-parse "${tip}^").Trim()
    Assert-True -Condition ($isAncestor -and ($grand -cne $tipParent)) `
        -Message 'ARRANGEMENT: the grandparent IS an ancestor of the head and is NOT its parent'
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2) -Message "it is NOT (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'neither the head nor its parent') `
        -Message 'and says exactly that: equality against the parent, never an ancestor test'

    # ---- pushed: the closed vocabulary.
    Write-Host ''
    Write-Host '-- pushed false, and pushed null, are both refusals --' -ForegroundColor Cyan
    foreach ($case in @(@{ v = $false; word = 'false'; phrase = 'never reached the server' },
                        @{ v = $null; word = 'null'; phrase = 'nobody could tell' })) {
        $repo = New-Repo -Name "pushed-$($case.word)" -ProvenanceInGate
        $head = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $case.v; pullRequest = 42; headSha = $head } | Out-Null
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch [regex]::Escape($case.phrase)) `
            -Message "pushed=$($case.word) is NOT, and the reason names it (exit $($r.exitCode))"
    }

    # ---- status, and a manifest that belongs to a different pull request.
    Write-Host ''
    Write-Host '-- a red run does not vouch, and another PR''s manifest is not this one''s --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'red' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'RED'; pushed = $true; pullRequest = 42; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch "not GREEN") `
        -Message "a RED run is NOT, naming the status (exit $($r.exitCode))"

    $repo = New-Repo -Name 'otherpr' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 99; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "a manifest for ANOTHER pull request leaves this one ABSENT, not NOT (exit $($r.exitCode))"

    # ---- null is not a wildcard.
    Write-Host ''
    Write-Host '-- a manifest with no pull request number does not match every number --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'nullpr' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = $null; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "pullRequest null does not vouch for #42: it is ABSENT, not a wildcard (exit $($r.exitCode))"

    # ---- ABSENT and NOT are different answers.
    Write-Host ''
    Write-Host '-- an empty store is ABSENT, with its own exit code --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'empty' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $head
    Assert-True -Condition ($r.exitCode -eq 3) -Message "ABSENT has its own exit code (got $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'ABSENT' -and $r.text -cmatch 'run the gate') `
        -Message 'and says what to do about it, which is the opposite of what NOT asks for'

    # ---- The mode is read from the repository, and advisory cannot block.
    Write-Host ''
    Write-Host '-- before (a) lands, the verdict is reported and not enforced --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'advisory'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'RED'; pushed = $false; pullRequest = 42; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($r.text -cmatch 'ADVISORY') `
        -Message 'a gate without provenance puts the tool in advisory mode'
    Assert-True -Condition ($r.text -cmatch 'NOT') `
        -Message 'the verdict is still computed and reported'
    # The exit code is the VERDICT, in every mode. Advisory used to force 0, which made the code
    # lie about what was found; whether to enforce it is the caller's decision, and the mode is
    # what tells them -- as a separate fact, in the text and in the JSON.
    Assert-True -Condition ($r.exitCode -eq 2) `
        -Message "the exit code still reports the verdict (exit $($r.exitCode)), and the MODE is the separate fact that says not to enforce it"

    # ABSENT in advisory too, because the two verdicts leave by different doors and only one of
    # them was covered. A fix that returns 0 for ABSENT-in-advisory passed the cell above.
    $repo = New-Repo -Name 'advisory-absent'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $head
    Assert-True -Condition ($r.exitCode -eq 3 -and $r.text -cmatch 'ADVISORY') `
        -Message "ABSENT keeps its own code in advisory mode as well (exit $($r.exitCode))"

    # ---- An unreadable manifest is not an absent one.
    Write-Host ''
    Write-Host '-- one corrupt file does not decide the verdict for the others --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'corrupt' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    # Both files in ONE commit, so the good manifest names that commit's parent and the broken one
    # sits beside it at the same head.
    [System.IO.File]::WriteAllText((Join-Path $repo '.factory/gate-runs/broken.json'), "{ not json", $utf8NoBom)
    Add-Manifest -Repo $repo -Uncommitted -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $head } | Out-Null
    Push-Location $repo
    try {
        & git add -- '.factory/gate-runs' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: run manifest' 2>&1 | Out-Null
        $tip = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0) `
        -Message "the good manifest still decides (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'does not parse') `
        -Message 'and the unreadable one is reported rather than silently skipped'

    Write-Host ''
    Write-Host '-- a corrupt manifest that is the ONLY record is not "run the gate" --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'corruptonly' -ProvenanceInGate
    [System.IO.File]::WriteAllText((Join-Path $repo '.factory/gate-runs/broken.json'), "{ not json", $utf8NoBom)
    Push-Location $repo
    try {
        & git add -- '.factory/gate-runs' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: a broken manifest' 2>&1 | Out-Null
        $tip = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2) `
        -Message "it is NOT, not ABSENT (exit $($r.exitCode)): a record exists and cannot be read"
    Assert-True -Condition ($r.text -cnotmatch 'run the gate') `
        -Message 'and it does not say "run the gate", which would write a second file beside a broken one'

    Write-Host ''
    Write-Host '-- a tip that DELETES a manifest is not a manifest-only tip --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'deleting' -ProvenanceInGate
    # The tip must be a commit that BOTH records a run for its own parent AND deletes an older
    # record. Only then is the manifest-vs-head arithmetic satisfied and the sole thing left to
    # judge is the deletion -- which is what makes this cell about the deletion and nothing else.
    Add-Manifest -Repo $repo -Name 'old.json' -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = 'a' * 40 } | Out-Null
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Name 'current.json' -Uncommitted -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    Push-Location $repo
    try {
        & git rm --quiet -- '.factory/gate-runs/old.json' 2>&1 | Out-Null
        & git add -- '.factory/gate-runs' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: a record added and another erased' 2>&1 | Out-Null
        $tip = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2) `
        -Message "a tip that also DELETES under the store is not pure (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'old\.json') `
        -Message 'and the deletion is named -- a commit that erases evidence must not certify like one that adds it'

    # ---- The machine record, for the caller that pastes a verdict into a merge comment.
    Write-Host ''
    Write-Host '-- -Json carries the same verdict as a record, not as prose --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'json' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    # Add-Manifest commits, so the manifest lives in the tip and names its parent -- the shape (a)
    # actually produces.
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip -Json
    $parsed = $null
    $lines = @($r.text -split "`n")
    $first = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l.TrimStart().StartsWith('{') })
    if ($first -ge 0) {
        try { $parsed = ($lines[$first..($lines.Count - 1)] -join "`n") | ConvertFrom-Json } catch { }
    }
    Assert-True -Condition ($null -ne $parsed) -Message 'the output parses as JSON and nothing else is printed to stdout'
    Assert-True -Condition ($parsed.state -ceq 'SATISFIED' -and $parsed.matched -ceq 'parent') `
        -Message 'it carries the state AND which side matched, so nobody re-derives it from the sentence'
    Assert-True -Condition ($parsed.mode -ceq 'blocking' -and -not [string]::IsNullOrWhiteSpace($parsed.modeReason)) `
        -Message 'and the mode WITH its reason, so a merge comment never has to infer why'
    Assert-True -Condition (@($parsed.tip) -ccontains 'A	.factory/gate-runs/run.json' -or
            @($parsed.tip | Where-Object { $_ -cmatch '\.factory/gate-runs/run\.json' }).Count -gt 0) `
        -Message 'and the tip it judged, by path and change kind'

    # ---- A manifest for this head under another number is ABSENT, and says so by name.
    Write-Host ''
    Write-Host '-- the near miss is named rather than hidden behind "nothing" --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'nearmiss' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 677; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "it is ABSENT: nothing recorded a run for THIS pull request (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'near miss' -and $r.text -cmatch '#677') `
        -Message 'and the manifest that nearly matched is named, so "nothing" does not hide it'

    # ---- THE CENTRAL ONE: the store is read at the HEAD, not from whatever tree the caller is in.
    #
    # Both directions, because they fail differently and only one of them is loud. A committed
    # manifest missing from the working tree used to read ABSENT -- a certified head refused. A
    # stale file present only in the working tree used to read SATISFIED -- an uncertified head
    # vouched for, which is the one that lets an ungated commit through.
    Write-Host ''
    Write-Host '-- a committed manifest counts even when the working tree does not have it --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'committed-only' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    Remove-Item -LiteralPath (Join-Path $repo '.factory/gate-runs/run.json') -Force
    Assert-True -Condition (-not (Test-Path -LiteralPath (Join-Path $repo '.factory/gate-runs/run.json'))) `
        -Message 'ARRANGEMENT: the manifest is committed and absent from the working tree'
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0) `
        -Message "it is still SATISFIED (exit $($r.exitCode)): the proof is about a commit, not about a disk"

    Write-Host ''
    Write-Host '-- a manifest that was never committed proves nothing --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'worktree-only' -ProvenanceInGate
    $head = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Uncommitted -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $head } | Out-Null
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $head
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "it is ABSENT (exit $($r.exitCode)): a file nobody committed is not a record of anything"

    Write-Host ''
    Write-Host '-- the ledger not being there is not the ledger being empty --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'noledger' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory (Join-Path $fixtureRoot 'no-such-ledger') 2>&1 |
            ForEach-Object { [string]$_ })
    Assert-True -Condition (($out -join "`n") -cmatch 'no ledger found at') `
        -Message 'a ledger that is not there says so, rather than reporting zero runs it never looked for'

    # ---- A manifest is untrusted input, and `pushed` is a closed vocabulary of THREE values.
    Write-Host ''
    Write-Host '-- pushed must BE a boolean, not merely compare like one --' -ForegroundColor Cyan
    foreach ($bad in @(@{ v = 'true'; word = 'the string "true"' }, @{ v = 1; word = 'the number 1' })) {
        $repo = New-Repo -Name "notbool-$($bad.v)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $bad.v; pullRequest = 42; headSha = $gated } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'not a boolean') `
            -Message "$($bad.word) does not vouch (exit $($r.exitCode)): PowerShell would compare it equal to true"
    }

    # ---- The candidate must not be able to downgrade the mode that judges it.
    Write-Host ''
    Write-Host '-- a branch that removes the provenance function does not turn blocking off --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'downgrade' -ProvenanceInGate
    Push-Location $repo
    try {
        # The BRANCH loses the function; origin/main keeps it. If the mode were read from the
        # checkout, a pull request could disable the check that judges it -- by editing itself.
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/gate.ps1'), "# removed`n", $utf8NoBom)
        & git commit -am 'drop provenance from this branch' --quiet 2>&1 | Out-Null
        $tip = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.text -cmatch 'BLOCKING') `
        -Message 'the mode stays BLOCKING: it is a fact about main, which the candidate cannot edit'
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "and the verdict is still reported (exit $($r.exitCode)), not downgraded to a pass"

    # ---- JSON mode emits ONE document, including when the tool cannot answer.
    Write-Host ''
    Write-Host '-- -Json stays a single document even for HARNESS-BROKE --' -ForegroundColor Cyan
    $bare = Join-Path $fixtureRoot 'notarepo'
    [System.IO.Directory]::CreateDirectory($bare) | Out-Null
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head ('a' * 40) -RepositoryRoot $bare -Json 2>&1 | ForEach-Object { [string]$_ })
    $broke = $null
    try { $broke = ($out -join "`n") | ConvertFrom-Json } catch { }
    Assert-True -Condition ($null -ne $broke -and $broke.state -ceq 'HARNESS-BROKE') `
        -Message 'a tool that cannot answer says so IN the document, so a caller piping it gets a state and not a syntax error'

    Write-Host ''
    Write-Host '-- the pull-request field is a scalar integer, not anything that compares like one --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'prstring' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = '42'; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "a pullRequest of '42' as a STRING does not name pull request 42 (exit $($r.exitCode))"

    $repo = New-Repo -Name 'prarray' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = @(42, 99); headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 3) `
        -Message "an ARRAY of numbers does not vouch for any of them (exit $($r.exitCode)): -eq over a collection is a filter, not a comparison"

    Write-Host ''
    Write-Host '-- a SATISFIED verdict with no second witness says so --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'nowitness' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory (Join-Path $fixtureRoot 'no-such-ledger') 2>&1 |
            ForEach-Object { [string]$_ })
    Assert-True -Condition (($out -join "`n") -cmatch 'rests on the committed manifest alone') `
        -Message 'the verdict names its own weakness: the manifest is written by the author of the code it vouches for'

    Write-Host ''
    Write-Host '-- headSha has to be one string, and a whole object id --' -ForegroundColor Cyan
    # `-cne` OVER A COLLECTION IS A FILTER. An empty array filters to empty, which is false, so the
    # whole mismatch block was skipped and a manifest that named NO head vouched for the head it was
    # asked about -- and for any other. The singleton walked the same path with a real value, which
    # is the shape an author would actually write by hand.
    foreach ($bad in @(
            @{ label = 'an empty array'; value = @() },
            @{ label = 'a singleton array holding the right sha'; value = $null },
            @{ label = 'a short string'; value = 'abc123' })) {
        $repo = New-Repo -Name "shashape-$($bad.label.Split(' ')[1])" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        $value = if ($null -eq $bad.value -and $bad.label -cmatch 'singleton') { ,@($gated) } else { $bad.value }
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $value } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2) `
            -Message "$($bad.label) is not a head this manifest vouches for (exit $($r.exitCode))"
    }

    Write-Host ''
    Write-Host '-- the default repository is the one the SCRIPT sits in --' -ForegroundColor Cyan
    # The parameter is documented as "the repository this script sits in", and `rev-parse` in the
    # CALLER's directory answered a different question. Run by absolute path from another checkout
    # it judged that other repository, and the verdict still read as a verdict about this one.
    $home1 = New-Repo -Name 'default-home' -ProvenanceInGate
    $elsewhere = New-Repo -Name 'default-elsewhere' -ProvenanceInGate
    Copy-Item -LiteralPath $subjectPath -Destination (Join-Path $home1 'ci/merge-proof.ps1') -Force
    $homeGated = (& git -C $home1 rev-parse HEAD).Trim()
    Add-Manifest -Repo $home1 -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $homeGated } | Out-Null
    $homeTip = (& git -C $home1 rev-parse HEAD).Trim()
    Push-Location $elsewhere
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $home1 'ci/merge-proof.ps1') `
                -PullRequest 42 -Head $homeTip -LedgerDirectory (Join-Path $fixtureRoot 'no-such-ledger') 2>&1 |
                ForEach-Object { [string]$_ })
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    Assert-True -Condition ($code -eq 0) `
        -Message "with no -RepositoryRoot the script judges its OWN repository, not the caller's (exit $code)"

    Write-Host ''
    Write-Host '-- a pull request is looked up in the repository being judged --' -ForegroundColor Cyan
    # `gh` resolves a number against the CURRENT DIRECTORY's repository. Without a slug the lookup
    # answers with some other project's head, and every line below it would be about a commit from a
    # repository nobody asked about. No slug means no lookup -- not a lookup somewhere else.
    $repo = New-Repo -Name 'noslug' -ProvenanceInGate
    $r = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -RepositoryRoot $repo 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 1 -and (($r -join "`n") -cmatch 'could not derive owner/repo')) `
        -Message "an origin that is not GitHub refuses the lookup instead of resolving it elsewhere (exit $code)"

    Write-Host ''
    Write-Host '-- a tip too large to list is not a pure tip --' -ForegroundColor Cyan
    # The bound fails CLOSED. An untrusted commit chooses how many paths it touches, and the answer
    # to "I stopped reading" cannot be "it touches only the store".
    $repo = New-Repo -Name 'bigtip' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } -Name 'a.json' | Out-Null
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 99; headSha = $gated } -Name 'b.json' | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -MaxTipEntries 0 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 2) `
        -Message "a tip whose listing hit the bound is NOT accepted as touching only the store (exit $code)"

    Write-Host ''
    Write-Host '-- two attempts at one commit are not two copies of one run --' -ForegroundColor Cyan
    # The gate writes the durable copy and the committed copy of ONE run under the same name, and a
    # retry on the same commit leaves an earlier RED and a later GREEN in the ledger. Joined by head
    # those two attempts looked like one run recorded twice and disagreeing, so a correct SATISFIED
    # reported store corruption that was really a retry.
    $repo = New-Repo -Name 'retry' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } -Name 'second.json' | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    # `<HEAD12>-<timestamp>.json`, which is what the gate writes. A fixture under any other name was
    # never a manifest the gate could have produced, and the scan that looks for corroboration finds
    # runs by that prefix -- so the old names were testing a path production never takes.
    $ledger = Join-Path $fixtureRoot 'retry-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    $prefix = $gated.Substring(0, 12)
    [System.IO.File]::WriteAllText((Join-Path $ledger "$prefix-2026-09-02T10-00-00.json"),
        # `dirtyDiffHash` present and null, because the gate writes it on every run: a ledger
        # record without it was not written by the gate, and the witness predicate is the same
        # one the committed store is held to.
        (@{ status = 'RED'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $ledger "$prefix-2026-09-02T11-00-00.json"),
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 0) `
        -Message "the retry does not turn a good verdict into a refusal (exit $code)"
    Assert-True -Condition (($out -join "`n") -cnotmatch 'do not agree') `
        -Message 'an earlier RED attempt on the same commit is not reported as a disagreeing copy of the GREEN run'

    Write-Host ''
    Write-Host '-- a run on a dirty tree measured a tree no commit holds --' -ForegroundColor Cyan
    # GREEN under uncommitted edits says the tests passed on something that was never committed, and
    # committing only the manifest afterwards leaves that difference in place.
    $repo = New-Repo -Name 'dirty' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
        dirtyDiffHash = 'b4f1c2e9a7d64c1b8e3f0a5d2c7b9e14f6a8d3b0c5e2f719a4d8c6b3e0f5a1d72' } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'uncommitted edits') `
        -Message "a GREEN run with a non-null dirtyDiffHash does not vouch for the head (exit $($r.exitCode))"

    # And ABSENT is not clean. Every manifest the gate writes carries the field; one without it was
    # written by something else, and "the field I would have checked is missing" is not a pass.
    #
    # THE FIELD IS DELIBERATELY MISSING HERE. A sweep that added `dirtyDiffHash = $null` to every
    # hand-written fixture in this file put it here too and turned the cell green against a subject
    # that no longer checked -- the cell whose whole subject is the field being absent. Caught by
    # its own red on the next run; left with a sign on it.
    $repo = New-Repo -Name 'nodirtyfield' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $path = Join-Path (Join-Path $repo '.factory/gate-runs') 'run.json'
    [System.IO.File]::WriteAllText($path,
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    Push-Location $repo
    try {
        & git add -- '.factory/gate-runs/run.json' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: run manifest' 2>&1 | Out-Null
    } finally { Pop-Location }
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'no dirtyDiffHash field') `
        -Message "a manifest with no dirtyDiffHash field is not a clean run (exit $($r.exitCode))"

    Write-Host ''
    Write-Host '-- the verifier is part of the candidate --' -ForegroundColor Cyan
    # The documented invocation runs THIS file out of a checkout of the branch being judged, so the
    # pull request supplies its own predicate. Reading the gate from main and trusting this file was
    # half a check.
    $repo = New-Repo -Name 'selfdiff' -ProvenanceInGate
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/merge-proof.ps1'), "# not the verifier`n", $utf8NoBom)
        & git add -- 'ci/merge-proof.ps1' 2>&1 | Out-Null
        & git commit --quiet -m 'a different verifier on main' 2>&1 | Out-Null
        & git push --quiet origin HEAD:refs/heads/main 2>&1 | Out-Null
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 1 -and $r.text -cmatch 'not the one at origin/main') `
        -Message "a verifier that differs from main's copy refuses to judge (exit $($r.exitCode))"

    # BOOTSTRAP IS A REAL STATE AND IT IS NAMED. Until (b) lands there is no copy on main to compare
    # against, and every cell above runs in exactly that state -- so this is the control that says
    # the cell above reddens for the difference and not merely for the comparison existing.
    $repo = New-Repo -Name 'selfbootstrap' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0 -and $r.text -cmatch 'does not exist at origin/main') `
        -Message "with no copy on main the run says so instead of pretending to have checked (exit $($r.exitCode))"

    Write-Host ''
    Write-Host '-- the ledger is a store like the other one --' -ForegroundColor Cyan
    # It gains a file per gate run forever, so "many files" is its normal state, not an attack. An
    # oversized entry used to be read whole before anything asked whether it was even about this
    # head -- a corroboration source able to stop the verdict from being returned.
    $repo = New-Repo -Name 'bigledger' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'big-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + '-2026-09-02T10-00-00.json')),
        ('{"status":"GREEN","filler":"' + ('x' * 1100000) + '"}'), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 0 -and (($out -join "`n") -cmatch 'beyond the \d+ this tool will read')) `
        -Message "an oversized ledger file is named and stepped over, not read (exit $code)"

    Write-Host ''
    Write-Host '-- no field is read as a collection, and the list is the whole manifest --' -ForegroundColor Cyan
    # THE SAME DEFECT ARRIVED FOUR TIMES IN FOUR FIELDS, and the fourth arrived in the same commit
    # that fixed the second. Guarding field by field loses to fields being added, so this cell is
    # written over the FIELD LIST rather than over the fields somebody remembered: every field the
    # verifier reads, each in the singleton shape that a hand-written guard lets through.
    foreach ($field in @(
            @{ name = 'status'; value = ,@('GREEN'); code = 2 },
            @{ name = 'pushed'; value = ,@($true); code = 2 },
            @{ name = 'headSha'; value = 'SELF'; code = 2 },
            @{ name = 'dirtyDiffHash'; value = ,@(); code = 2 },
            @{ name = 'dirtyDiffHash'; value = ,@($null); code = 2 },
            # A pullRequest nobody can read does not name this pull request at all, so the answer is
            # ABSENT rather than NOT -- a different verdict for the same shape, and the cell says so
            # instead of averaging the two.
            @{ name = 'pullRequest'; value = ,@(42); code = 3 })) {
        $repo = New-Repo -Name "shape-$($field.name)-$($field.code)-$([guid]::NewGuid().ToString('N').Substring(0,6))" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        $body = @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null }
        $body[$field.name] = if ($field.value -ceq 'SELF') { ,@($gated) } else { $field.value }
        Add-Manifest -Repo $repo -Body $body | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq $field.code) `
            -Message "$($field.name) as a one-element collection is not a value (exit $($r.exitCode), wanted $($field.code))"
    }

    Write-Host ''
    Write-Host '-- a passing retry does not erase the committed failure before it --' -ForegroundColor Cyan
    # The verdict used to be written the moment a good manifest was found, discarding what the loop
    # had already collected: an earlier committed RED for the same pull request vanished from a
    # SATISFIED answer, and the retry read as an unqualified first-time pass. The good record still
    # decides; the others are reported beside it.
    $repo = New-Repo -Name 'retrystore' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    # BOTH IN ONE COMMIT, on purpose: committed separately the second manifest would name a
    # grandparent and the cell would be measuring the parent rule instead of this one.
    Add-Manifest -Repo $repo -Body @{ status = 'RED'; pushed = $true; pullRequest = 42; headSha = $gated } -Name 'attempt-1.json' -Uncommitted | Out-Null
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } -Name 'attempt-2.json' -Uncommitted | Out-Null
    Push-Location $repo
    try {
        & git add -- '.factory/gate-runs/attempt-1.json' '.factory/gate-runs/attempt-2.json' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: two attempts' 2>&1 | Out-Null
    } finally { Pop-Location }
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 0) `
        -Message "the GREEN retry still decides the verdict (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'attempt-1\.json' -and $r.text -cmatch "do NOT vouch") `
        -Message 'and the earlier RED attempt is named in the SATISFIED answer instead of being discarded'

    Write-Host ''
    Write-Host '-- valid JSON that is not an object is a corrupt record, not an absent one --' -ForegroundColor Cyan
    # `"garbage"` and `[1,2]` parse PERFECTLY. The value went into the store, had no readable
    # pullRequest, and the answer became ABSENT -- "nothing recorded a run for this pull request,
    # run the gate" -- for a store that holds a structurally corrupt file. Re-running fixes an
    # absent record and writes a second file beside a broken one.
    foreach ($root in @('"garbage"', '[1,2]', '42')) {
        $repo = New-Repo -Name "nonobject-$([guid]::NewGuid().ToString('N').Substring(0,6))" -ProvenanceInGate
        $path = Join-Path (Join-Path $repo '.factory/gate-runs') 'run.json'
        [System.IO.File]::WriteAllText($path, $root, $utf8NoBom)
        Push-Location $repo
        try {
            & git add -- '.factory/gate-runs/run.json' 2>&1 | Out-Null
            & git commit --quiet -m 'gate: run manifest' 2>&1 | Out-Null
        } finally { Pop-Location }
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'not an object') `
            -Message "a JSON root of $root is unreadable, not absent (exit $($r.exitCode))"
    }

    Write-Host ''
    Write-Host '-- no manifest field is read except through the reader --' -ForegroundColor Cyan
    # THE GUARD FOR THE GENERAL FORM. One reader is only worth having if nothing walks around it,
    # and the next field will be added by somebody who did not sit through the four rounds that
    # produced it. This asserts over the SOURCE: outside Read-ManifestField, no line reaches into a
    # parsed manifest body by property.
    #
    #   Select-String -Path ci/merge-proof.ps1 -Pattern '\$m\.|\$_\.body\.|\$body\.[a-z]|\$committed\.body\.'
    #
    # The two `$candidate.name`/`$committed.name` reads are the STORE's own filename, not a field
    # from the manifest, so the pattern is written to leave them alone deliberately.
    $subjectText = [System.IO.File]::ReadAllText($subjectPath)
    $readerStart = $subjectText.IndexOf('function Read-ManifestField {')
    $readerEnd = $subjectText.IndexOf('function Get-RemoteSlug {')
    Assert-True -Condition ($readerStart -ge 0 -and $readerEnd -gt $readerStart) `
        -Message 'the reader is where this cell expects it (otherwise the sweep below proves nothing)'
    $outsideReader = $subjectText.Remove($readerStart, $readerEnd - $readerStart)
    $bypasses = @(($outsideReader -split "`n") | Where-Object {
            $_ -cmatch '\$m\.[A-Za-z]|\$body\.[A-Za-z]|\$_\.body\.[A-Za-z]|\$committed\.body\.[A-Za-z]' -and
            $_ -cnotmatch 'PSObject\.Properties'
        })
    Assert-True -Condition ($bypasses.Count -eq 0) `
        -Message ("no field is read outside the reader" + $(if ($bypasses.Count -gt 0) { ": " + (($bypasses | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))

    # And the sweep is not vacuous: the same pattern over a line that DOES bypass the reader has to
    # find it, or the zero above is a zero from a pattern that matches nothing.
    $canary = '    if ($m.status -cne ''GREEN'') { $why += "no" }'
    $found = @(($canary -split "`n") | Where-Object {
            $_ -cmatch '\$m\.[A-Za-z]|\$body\.[A-Za-z]|\$_\.body\.[A-Za-z]|\$committed\.body\.[A-Za-z]' -and
            $_ -cnotmatch 'PSObject\.Properties'
        })
    Assert-True -Condition ($found.Count -eq 1) `
        -Message 'and the pattern that found nothing does find a bypass when one is put in front of it'

    Write-Host ''
    Write-Host '-- a note about the scan is not a record --' -ForegroundColor Cyan
    # THE REGRESSION CELL. The previous commit added an unconditional sentence describing how the
    # ledger was scanned, into the same list the verdict tests for emptiness -- so "no independent
    # record corroborates this" stopped being true, and the #709 warning vanished exactly when the
    # ledger directory EXISTED and held nothing matching. The `nowitness` cell above did not catch
    # it because it points at a directory that is not there at all.
    $repo = New-Repo -Name 'emptyledger' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'ledger-that-exists'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 0 -and (($out -join "`n") -cmatch 'rests on the committed manifest alone')) `
        -Message "an existing ledger holding nothing is still no witness, and the verdict says so (exit $code)"

    Write-Host ''
    Write-Host '-- twins are compared on every field the proof uses --' -ForegroundColor Cyan
    # `status` and `headSha` agreeing while the rest disagrees is the interesting case, and the one
    # an author would produce: flip the committed copy's pull request to the requested number and
    # `pushed` to true, and SATISFIED came back while the independent record said otherwise.
    $repo = New-Repo -Name 'twinfields' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $twinName = $gated.Substring(0, 12) + '-2026-09-02T10-00-00.json'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } -Name $twinName | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'twin-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger $twinName),
        (@{ status = 'GREEN'; pushed = $false; pullRequest = 99; headSha = $gated; dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    $text = ($out -join "`n")
    Assert-True -Condition ($text -cmatch 'disagree on pullRequest') `
        -Message 'a twin naming a different pull request is a disagreement, not a silence'
    Assert-True -Condition ($text -cmatch 'disagree on pushed') `
        -Message 'and so is a twin that says the head never reached the server'

    Write-Host ''
    Write-Host '-- a corrupt ledger entry for THIS head is named --' -ForegroundColor Cyan
    # The durable copy is the only independent witness this tool has. Swallowing a corrupt one is
    # worst under SATISFIED, where the committed copy is accepted and nobody looks again.
    $repo = New-Repo -Name 'badledger' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'corrupt-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + '-2026-09-02T10-00-00.json')),
        '{ this is not json', $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cmatch 'does not parse as JSON') `
        -Message 'a ledger entry that names this head and does not parse is reported, not skipped'

    Write-Host ''
    Write-Host '-- a witness has to AGREE before it counts as one --' -ForegroundColor Cyan
    # Everything the ledger scan touched used to land in one list, and SATISFIED suppressed the
    # "committed manifest alone" warning whenever that list was non-empty. So a file too big to read,
    # an entry with an unreadable head, or a RED record saying the opposite all made an
    # author-written GREEN manifest look independently corroborated. Listing a record is not
    # agreeing with it.
    foreach ($case in @(
            @{ name = 'oversized'; body = ('{"status":"GREEN","filler":"' + ('x' * 1100000) + '"}') },
            @{ name = 'red'; body = $null },
            @{ name = 'unreadablehead'; body = '{"status":"GREEN","pushed":true,"headSha":[]}' })) {
        $repo = New-Repo -Name "witness-$($case.name)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $ledger = Join-Path $fixtureRoot "witness-$($case.name)-ledger"
        [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
        $body = if ($null -eq $case.body) {
            (@{ status = 'RED'; pushed = $true; pullRequest = 42; headSha = $gated } | ConvertTo-Json)
        } else { $case.body }
        [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + '-2026-09-02T10-00-00.json')), $body, $utf8NoBom)
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
                -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
        Assert-True -Condition ((($out -join "`n")) -cmatch 'rests on the committed manifest alone') `
            -Message "a $($case.name) ledger entry does not corroborate anything, and the verdict still says so"
    }

    # The control: a ledger record that DOES agree removes the warning. Without it the assertions
    # above would pass against a tool that never printed the warning at all.
    $repo = New-Repo -Name 'witness-good' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'witness-good-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + '-2026-09-02T10-00-00.json')),
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cnotmatch 'rests on the committed manifest alone') `
        -Message 'CONTROL: a GREEN ledger record for this head IS a second witness, and the warning goes'

    Write-Host ''
    Write-Host '-- a witness is held to the whole proof, not to two of its fields --' -ForegroundColor Cyan
    # The ledger is a store like the other one, so a record in it is a manifest and is judged by the
    # same predicate. GREEN and pushed were enough to suppress the "committed manifest alone"
    # warning, which made the ledger the back door to everything the committed store had closed.
    foreach ($case in @(
            @{ name = 'otherpr'; body = @{ status = 'GREEN'; pushed = $true; pullRequest = 99; headSha = 'HEAD'; dirtyDiffHash = $null } },
            @{ name = 'dirty'; body = @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = 'HEAD'
                    dirtyDiffHash = 'f00dfacef00dfacef00dfacef00dfacef00dfacef00dfacef00dfacef00dface' } },
            @{ name = 'nodirty'; body = @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = 'HEAD' } })) {
        $repo = New-Repo -Name "witnessfull-$($case.name)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $ledger = Join-Path $fixtureRoot "witnessfull-$($case.name)-ledger"
        [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
        $body = @{} + $case.body
        $body['headSha'] = $gated
        [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + '-2026-09-02T10-00-00.json')),
            ($body | ConvertTo-Json), $utf8NoBom)
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
                -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
        Assert-True -Condition ((($out -join "`n")) -cmatch 'rests on the committed manifest alone') `
            -Message "a ledger record that fails the proof on $($case.name) is not a second witness"
    }

    Write-Host ''
    Write-Host '-- the NOT verdict carries what it knows about the store --' -ForegroundColor Cyan
    # ABSENT and SATISFIED both report unreadable records; NOT did not. A store holding a broken
    # manifest beside a merely-wrong one reported the wrong one and hid the broken one entirely --
    # in the human output and in the JSON -- so the operator is told to look at the wrong thing.
    $repo = New-Repo -Name 'notunreadable' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = ('c' * 40) } -Name 'wrong.json' | Out-Null
    $broken = Join-Path (Join-Path $repo '.factory/gate-runs') 'broken.json'
    [System.IO.File]::WriteAllText($broken, '{ not json at all', $utf8NoBom)
    Push-Location $repo
    try {
        & git add -- '.factory/gate-runs/broken.json' 2>&1 | Out-Null
        & git commit --quiet -m 'gate: a broken record' 2>&1 | Out-Null
    } finally { Pop-Location }
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2) `
        -Message "the verdict is still NOT, decided by the manifest that does not vouch (exit $($r.exitCode))"
    Assert-True -Condition ($r.text -cmatch 'broken\.json') `
        -Message 'and the record that could not be read at all is named beside it, not hidden'

    Write-Host ''
    Write-Host '-- a truncated ledger scan says it was truncated --' -ForegroundColor Cyan
    # Taking exactly the ceiling keeps an unspecified provider-ordered subset and says nothing: a
    # corroborating GREEN retry or a disagreeing RED attempt can vanish, and the answer changes with
    # enumeration order while claiming no witness was found. Absence that was never looked for must
    # not read as absence that was looked for and missing.
    $repo = New-Repo -Name 'truncated' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'truncated-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    foreach ($n in 1..3) {
        [System.IO.File]::WriteAllText((Join-Path $ledger ($gated.Substring(0, 12) + "-2026-09-02T1$n-00-00.json")),
            (@{ status = 'RED'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    }
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger -MaxManifests 2 2>&1 |
            ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cmatch 'scan was TRUNCATED') `
        -Message 'the scan says it stopped short, instead of reporting what it happened to reach'

    Write-Host ''
    Write-Host '-- twins are compared before the head filter, not after --' -ForegroundColor Cyan
    # Twins are matched by NAME -- the run identity the gate writes both copies under -- while the
    # head filter answers a different question: which record can corroborate THIS verdict. Running
    # the filter first discarded the most interesting thing the scan can find: two copies of ONE run
    # disagreeing about which commit it judged.
    $repo = New-Repo -Name 'twinbeforefilter' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $twinName = $gated.Substring(0, 12) + '-2026-09-02T10-00-00.json'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } -Name $twinName | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'twinbefore-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    # SAME NAME, DIFFERENT HEAD. Under the old order this entry was dropped by the head filter and
    # the pair's disagreement was never printed -- the SATISFIED verdict went out clean.
    [System.IO.File]::WriteAllText((Join-Path $ledger $twinName),
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = ('d' * 40); dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    $text = ($out -join "`n")
    Assert-True -Condition ($text -cmatch 'disagree on headSha') `
        -Message 'the two copies of one run naming different heads is reported'
    Assert-True -Condition ($text -cmatch 'rests on the committed manifest alone') `
        -Message 'and the entry still does not corroborate the verdict, because it is about another head'

    Write-Host ''
    Write-Host '-- the store has a total ceiling, not only a per-file one --' -ForegroundColor Cyan
    # Two thousand manifests of just under a megabyte each pass the per-file ceiling one at a time
    # and add up to two gigabytes. The per-file limit reads as a bound and bounds only the worst
    # SINGLE entry -- the same shape as a count ceiling with unbounded files behind it.
    $repo = New-Repo -Name 'bigstore' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    foreach ($n in 1..3) {
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
            filler = ('x' * 2000) } -Name "run-$n.json" | Out-Null
    }
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -MaxStoreBytes 3000 2>&1 | ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 1 -and (($out -join "`n") -cmatch 'total more than')) `
        -Message "a store too large in aggregate is HARNESS-BROKE, not a verdict (exit $code)"

    # The control: the same store under a ceiling that fits still answers. Without it the assertion
    # above would pass against a tool that refused every store it was ever shown.
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -MaxStoreBytes 1000000 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ($LASTEXITCODE -ne 1) `
        -Message "CONTROL: the same store under a ceiling that fits is judged normally (exit $LASTEXITCODE)"

    Write-Host ''
    Write-Host '-- one byte budget, spent by both stores --' -ForegroundColor Cyan
    # The aggregate ceiling bounded the committed store and left the ledger unbounded: two thousand
    # entries just under the per-file ceiling, in EACH of two prefixes (head and parent), is four
    # gigabytes read before a verdict. Per-file versus per-run, one directory over -- and two
    # ceilings that are each individually true.
    $repo = New-Repo -Name 'ledgerbudget' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $parent = (& git -C $repo rev-parse "$tip^").Trim()
    $ledger = Join-Path $fixtureRoot 'budget-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    # BOTH prefixes, because the scan reads the head's and the parent's, and a budget checked in
    # only one of them would pass this cell while the other spent freely.
    foreach ($prefix in @($tip.Substring(0, 12), $parent.Substring(0, 12))) {
        [System.IO.File]::WriteAllText((Join-Path $ledger "$prefix-2026-09-02T10-00-00.json"),
            (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated; dirtyDiffHash = $null
                filler = ('y' * 1500) } | ConvertTo-Json), $utf8NoBom)
    }
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger -MaxStoreBytes 2000 2>&1 |
            ForEach-Object { [string]$_ })
    $code = $LASTEXITCODE
    Assert-True -Condition ($code -eq 1 -and (($out -join "`n") -cmatch 'across the committed store and the ledger')) `
        -Message "the ledger spends the same budget as the store, and overflowing it is HARNESS-BROKE (exit $code)"

    # The control: under a budget that fits, the same repository and ledger answer normally.
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger -MaxStoreBytes 1000000 2>&1 |
            ForEach-Object { [string]$_ })
    Assert-True -Condition ($LASTEXITCODE -ne 1) `
        -Message "CONTROL: the same pair of stores under a budget that fits is judged normally (exit $LASTEXITCODE)"

    Write-Host ''
    Write-Host '-- a manifest string cannot carry control characters into the verdict --' -ForegroundColor Cyan
    # The manifest is candidate input and its values are interpolated into the human verdict and
    # into -Json. A newline in `status` forges extra verdict lines, a carriage return overwrites the
    # line above, and an ESC colours or moves the cursor in whoever's terminal reads the answer.
    #
    # Refused at the READER rather than escaped at each print site: escaping is a list of sites to
    # keep complete, and this file has learned twice what happens to lists of sites.
    foreach ($case in @(
            @{ name = 'newline'; value = "GREEN`nSATISFIED: forged" },
            @{ name = 'carriage-return'; value = "GREEN`r[merge-proof] SATISFIED" },
            @{ name = 'escape'; value = "GREEN$([char]27)[31m" })) {
        $repo = New-Repo -Name "control-$($case.name)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = $case.value; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'contains a control character') `
            -Message "a $($case.name) in status is refused by the reader (exit $($r.exitCode))"
        # And the refusal does not echo the value back: printing it is the thing being prevented.
        Assert-True -Condition ($r.text -cnotmatch 'forged' -and -not $r.text.Contains([string][char]27)) `
            -Message "and the refusal does not print the payload while refusing it"
    }

    Write-Host ''
    Write-Host '-- one family per cell: the refused population is a Unicode CATEGORY --' -ForegroundColor Cyan
    # The previous guard was ASCII C0 plus DEL: the family I had in mind, not the class. These are
    # the ones that actually deceive a reader, and none of them is in that range. Written as code
    # points rather than as literal characters, so the byte census over this file stays green and so
    # a reviewer can see WHICH character each cell is about.
    foreach ($case in @(
            @{ slug = 'rlo'; what = 'a right-to-left override'; payload = [string][char]0x202E
                expect = 'U+202E'; phrase = 'invisible formatting' },
            @{ slug = 'isolate'; what = 'a bidi isolate'; payload = [string][char]0x2066
                expect = 'U+2066'; phrase = 'invisible formatting' },
            @{ slug = 'zwsp'; what = 'a zero-width space'; payload = [string][char]0x200B
                expect = 'U+200B'; phrase = 'invisible formatting' },
            @{ slug = 'bom'; what = 'a byte order mark'; payload = [string][char]0xFEFF
                expect = 'U+FEFF'; phrase = 'invisible formatting' },
            @{ slug = 'shy'; what = 'a soft hyphen'; payload = [string][char]0x00AD
                expect = 'U+00AD'; phrase = 'invisible formatting' },
            @{ slug = 'lsep'; what = 'a line separator'; payload = [string][char]0x2028
                expect = 'U+2028'; phrase = 'line-separating' },
            @{ slug = 'psep'; what = 'a paragraph separator'; payload = [string][char]0x2029
                expect = 'U+2029'; phrase = 'paragraph-separating' },
            # Above the BMP, so it arrives as two surrogates: the cell that says the reader asks
            # about CODE POINTS and not about UTF-16 units.
            @{ slug = 'astral'; what = 'an astral format character'; payload = [char]::ConvertFromUtf32(0x110BD)
                expect = 'U+110BD'; phrase = 'invisible formatting' },
            @{ slug = 'shy2'; what = 'a zero-width joiner'; payload = [string][char]0x200D
                expect = 'U+200D'; phrase = 'invisible formatting' })) {
        $repo = New-Repo -Name "deceptive-$($case.slug)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = "GREEN$($case.payload)"; pushed = $true; pullRequest = 42
            headSha = $gated } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text.Contains($case.expect) -and $r.text -cmatch $case.phrase) `
            -Message "$($case.what) in status is refused, by code point (exit $($r.exitCode))"
        Assert-True -Condition (-not $r.text.Contains($case.payload)) `
            -Message "and the refusal names the code point without carrying the character into the output"
    }

    # WHAT THE ESCAPE ACTUALLY BECOMES, measured rather than assumed. A manifest CAN carry
    # "GREEN" plus a lone surrogate escape, because a file can hold what a PowerShell string cannot
    # -- but ConvertFrom-Json replaces it with U+FFFD before any reader sees it, so the refusal this
    # cell was written to prove is unreachable through a manifest and the branch says so.
    #
    # What is left is the fact that DOES decide: the value is no longer GREEN, and the run must not
    # be satisfied by it. Under a culture-aware `-ceq` it was: exit 0, SATISFIED, on a status that
    # differs from GREEN by a code point the comparer gives no weight.
    $repo = New-Repo -Name 'deceptive-surrogate' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $escaped = ([char]92) + 'ud83d'
    $rawBody = @{ status = 'GREENSURROGATEHERE'; pushed = $true; pullRequest = 42; headSha = $gated
        dirtyDiffHash = $null } | ConvertTo-Json -Depth 6
    Add-Manifest -Repo $repo -Body @{} -RawJson ($rawBody -replace 'SURROGATEHERE', $escaped) | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'not GREEN') `
        -Message "a status that is GREEN plus a lone surrogate escape does not vouch (exit $($r.exitCode), NOT is 2)"
    # The fixture has to still be carrying it: the first version of this cell wrote the payload as a
    # PowerShell string, the write path deleted it, and the manifest said plain GREEN.
    $onDisk = [System.IO.File]::ReadAllText((Join-Path $repo '.factory/gate-runs/run.json'))
    Assert-True -Condition ($onDisk.Contains($escaped)) `
        -Message 'and the manifest on disk really holds the escape, so the cell is about what it says'

    Write-Host ''
    Write-Host '-- the equalities that decide are ordinal, because -ceq is not --' -ForegroundColor Cyan
    # A VARIATION SELECTOR IS NOT IN THE REFUSED CLASS AND MUST NOT BE: U+FE00 is Mn, it is ordinary
    # text, and a guard that refused it would be a deny-list growing by one code point per review.
    # It is also weightless in a culture comparison, so 'GREEN' + U+FE00 was -ceq 'GREEN' and the run
    # came back SATISFIED. The comparison stops being approximate; the alphabet stays as it is.
    $repo = New-Repo -Name 'ordinal-status' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN' + [char]0xFE00; pushed = $true; pullRequest = 42
        headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'not GREEN') `
        -Message "GREEN plus a weightless variation selector does not vouch (exit $($r.exitCode), NOT is 2)"

    # The twin comparison is decided by the same operator, and there it decides whether two records
    # of ONE run are reported as disagreeing. Two values that differ only by U+FE00 were passed over
    # as identical by the check whose entire job is noticing they are not.
    $repo = New-Repo -Name 'ordinal-twin' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $twinName = $gated.Substring(0, 12) + '-2026-09-03T11-00-00.json'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } `
        -Name $twinName | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'ordinal-twin-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger $twinName),
        (@{ status = 'GREEN' + [char]0xFE00; pushed = $true; pullRequest = 42; headSha = $gated
            dirtyDiffHash = $null } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cmatch 'disagree') `
        -Message 'two twin values differing only by a weightless code point are reported as disagreeing'

    # THE SWEEP, because the fix is a rule about every equality in the file and the cells above reach
    # three of them. `-ceq` and `-cne` are the operators that look case-strict and are still culture
    # aware; `-ccontains` and `-cnotcontains` are the same comparer over a list.
    $subjectLines = @([System.IO.File]::ReadAllText($subjectPath) -split "`n")
    $inDocBlock = $false
    $cultureOps = @(foreach ($line in $subjectLines) {
            $trimmed = $line.Trim()
            if ($trimmed -cmatch '<#') { $inDocBlock = $true }
            if ($trimmed -cmatch '#>') { $inDocBlock = $false; continue }
            if ($inDocBlock -or $trimmed.StartsWith('#')) { continue }
            if ($line -cmatch '-ceq |-cne |-ccontains |-cnotcontains ') { $trimmed }
        })
    Assert-True -Condition ($cultureOps.Count -eq 0) `
        -Message ('no equality that decides uses the culture-aware operators' +
            $(if ($cultureOps.Count -gt 0) { ': ' + (($cultureOps | Select-Object -First 3) -join ' | ') } else { '' }))
    # Not vacuous: the sweep finds one when it is put in front of it, and is not fooled by the
    # doc comment in Test-SameText that discusses the operator by name.
    $canaryFound = @(foreach ($line in @('    if ($a -cne $b) {', '    # $a -ceq $b in a comment')) {
            $trimmed = $line.Trim()
            if ($trimmed.StartsWith('#')) { continue }
            if ($line -cmatch '-ceq |-cne |-ccontains |-cnotcontains ') { $trimmed }
        })
    Assert-True -Condition ($canaryFound.Count -eq 1) `
        -Message 'and the sweep finds a culture-aware comparison when one is put in front of it, while ignoring a comment'

    # THE CONTROL, and it is the half that decides whether the guard is usable: a value that is
    # merely NOT ASCII is not deceptive. An astral emoji and an accented letter pass the reader --
    # this run still fails, because the status is not GREEN, but it fails for what it SAYS rather
    # than for what it is made of. A guard that refused these would be refused itself.
    $repo = New-Repo -Name 'deceptive-control' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $harmless = [char]::ConvertFromUtf32(0x1F600) + [string][char]0x00E9
    Add-Manifest -Repo $repo -Body @{ status = "GREEN$harmless"; pushed = $true; pullRequest = 42
        headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.text -cnotmatch 'no field this tool reads may hold' -and $r.text -cnotmatch 'unpaired surrogate') `
        -Message 'CONTROL: an astral emoji and an accented letter are not refused by the reader'

    Write-Host ''
    Write-Host '-- the verdict does not print a ledger path under the user profile --' -ForegroundColor Cyan
    # The house rule is that output exposes no home paths, and a verdict is output like any other.
    # Where the ledger is OUTSIDE home the path is still named, because "absence at a path nobody
    # can see" is the defect that made this scan report zero runs it never looked for.
    $repo = New-Repo -Name 'homeledger' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $homeLedger = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'graphhelm-ledger-that-is-not-there'
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $homeLedger 2>&1 |
            ForEach-Object { [string]$_ })
    $text = ($out -join "`n")
    Assert-True -Condition ($text -cnotmatch [regex]::Escape([Environment]::GetFolderPath('UserProfile'))) `
        -Message 'the home path does not appear in the verdict'
    Assert-True -Condition ($text -cmatch 'a path under the current user profile') `
        -Message 'and the run still says WHERE it looked, in a form that names no path'

    # The control: a ledger outside home keeps its path, because that is the diagnostic the
    # redaction must not cost.
    # NOT under $fixtureRoot: on Windows %TEMP% lives inside the user profile, so the "outside home"
    # control was pointing at a path inside home and failed on its own premise. The drive root the
    # suite runs from is outside it, and the path does not need to exist -- the message under test is
    # the one that reports absence.
    $outsideLedger = Join-Path ([System.IO.Path]::GetPathRoot($PSScriptRoot)) 'ledger-outside-home'
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $outsideLedger 2>&1 |
            ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cmatch [regex]::Escape('ledger-outside-home')) `
        -Message 'CONTROL: a ledger outside the user profile is still named in full'

    Write-Host ''
    Write-Host '-- no refusal prints the value it is refusing --' -ForegroundColor Cyan
    # The control-character check lived inside the `string` case, and `dirtyDiffHash` is read as
    # `null` -- so a non-null string carrying a newline or an ESC skipped it and was interpolated
    # RAW into the refusal. The refusal was the vector. A rule written inside one branch is a rule
    # about that branch, which is the fifth time in this pull request that one of my rules was
    # applied to the subject in front of me instead of to its subjects.
    foreach ($case in @(
            @{ name = 'newline'; value = "abc`nSATISFIED: forged" },
            @{ name = 'escape'; value = "abc$([char]27)[31m" })) {
        $repo = New-Repo -Name "dirtyctl-$($case.name)" -ProvenanceInGate
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
            dirtyDiffHash = $case.value } | Out-Null
        $tip = (& git -C $repo rev-parse HEAD).Trim()
        $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
        Assert-True -Condition ($r.exitCode -eq 2 -and $r.text -cmatch 'contains a control character') `
            -Message "dirtyDiffHash carrying a $($case.name) is refused by the reader (exit $($r.exitCode))"
        Assert-True -Condition ($r.text -cnotmatch 'forged' -and $r.text -cnotmatch '\x1b\[31m') `
            -Message "and the refusal does not print the payload it is refusing"
    }

    # THE SWEEP, over the reader rather than over the two cases above: the cells reach the kinds a
    # fixture can express, and this reaches every refusal message there is.
    $subjectText = [System.IO.File]::ReadAllText($subjectPath)
    $readerStart = $subjectText.IndexOf('function Read-ManifestField {')
    $readerEnd = $subjectText.IndexOf('function Get-RemoteSlug {')
    $readerText = $subjectText.Substring($readerStart, $readerEnd - $readerStart)
    $echoingRefusals = @(($readerText -split "`n") | Where-Object {
            # The SUCCESS line assigns `value = $v` and `why = $null` together, so a pattern that only
            # asks for both words on one line flags it -- a false positive that would have made this
            # sweep unusable and then removed. It looks at the why STRING alone.
            $whyPart = if ($_ -cmatch 'why\s*=(.*)$') { $Matches[1] } else { '' }
            $whyPart -cmatch '\$v' -and $whyPart -cnotmatch 'GetType\(\)'
        })
    Assert-True -Condition ($echoingRefusals.Count -eq 0) `
        -Message ("no refusal message in the reader interpolates the value" +
            $(if ($echoingRefusals.Count -gt 0) { ': ' + (($echoingRefusals | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))

    Write-Host ''
    Write-Host '-- the repository path is redacted like every other path --' -ForegroundColor Cyan
    # The redaction was written for the ledger and the repository root kept its own unredacted
    # interpolation one function away. Same rule, second subject, and it took a review to find it.
    $repo = New-Repo -Name 'pathredact' -ProvenanceInGate
    Push-Location $repo
    try { & git remote add origin 'C:\not-a-github-url' 2>&1 | Out-Null } finally { Pop-Location }
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -RepositoryRoot $repo 2>&1 | ForEach-Object { [string]$_ })
    $text = ($out -join "`n")
    Assert-True -Condition ($text -cmatch 'could not derive owner/repo') `
        -Message 'ARRANGEMENT: the lookup really did refuse, so there is a message to inspect'
    Assert-True -Condition ($text -cnotmatch [regex]::Escape([Environment]::GetFolderPath('UserProfile'))) `
        -Message 'and the HARNESS-BROKE reason exposes no path under the user profile'

    Write-Host ''
    Write-Host '-- no source file carries a control byte --' -ForegroundColor Cyan
    # THE INSTRUMENT FOR A DEFECT WITH NO DOWNSTREAM SYMPTOM. A quoting layer once turned the text
    # of a character class into the BYTES it names. The class matched the same set either way, so
    # every cell passed, the parser was happy, the reader really did refuse control characters, and
    # review saw nothing -- the only thing that surfaced it was `git grep` reporting "Binary file
    # matches" while somebody was looking for something else.
    #
    # Nothing structural stops the next author's heredoc from eating an escape the same way. This
    # cell is what replaces "nothing": a byte census over the sources, which fails the moment an
    # escape becomes the thing it describes, anywhere in either file.
    foreach ($sourceFile in @($subjectPath, $PSCommandPath)) {
        $bytes = [System.IO.File]::ReadAllBytes($sourceFile)
        $offenders = @()
        for ($i = 0; $i -lt $bytes.Length; $i++) {
            $b = $bytes[$i]
            # Tab, CR and LF are the only control bytes source is allowed to hold.
            if (($b -lt 9 -or ($b -ge 11 -and $b -le 12) -or ($b -ge 14 -and $b -le 31) -or $b -eq 127)) {
                $offenders += ('0x{0:X2} at byte {1}' -f $b, $i)
            }
        }
        Assert-True -Condition ($offenders.Count -eq 0) `
            -Message ("$([System.IO.Path]::GetFileName($sourceFile)) holds no control bytes outside tab/CR/LF" +
                $(if ($offenders.Count -gt 0) { ': ' + (($offenders | Select-Object -First 4) -join ', ') } else { '' }))
    }

    # Not vacuous: the same census over a buffer that DOES carry one has to find it.
    $canaryBytes = [System.Text.Encoding]::UTF8.GetBytes("ok`tok`r`nok" + [char]27 + 'x')
    $canaryHits = @()
    for ($i = 0; $i -lt $canaryBytes.Length; $i++) {
        $b = $canaryBytes[$i]
        if (($b -lt 9 -or ($b -ge 11 -and $b -le 12) -or ($b -ge 14 -and $b -le 31) -or $b -eq 127)) { $canaryHits += $i }
    }
    Assert-True -Condition ($canaryHits.Count -eq 1) `
        -Message 'and the census finds an ESC when one is put in front of it, while ignoring tab, CR and LF'

    Write-Host ''
    Write-Host '-- a mention of the function is not a definition of it --' -ForegroundColor Cyan
    # The mode decides whether a NOT is enforced, and it was decided by a regex over the TEXT of
    # main's gate: a comment, a string, or a docstring naming the function answered yes. This file's
    # own rule is that a comment is a claim rather than a fact, and the mode is the one place where
    # believing a comment changes whether a verdict binds.
    $repo = New-Repo -Name 'mentiononly'
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/gate.ps1'),
            "# the provenance work will add function Get-HeadProvenance here`n`$x = 'function Get-HeadProvenance'`n", $utf8NoBom)
        & git add -- 'ci/gate.ps1' 2>&1 | Out-Null
        & git commit --quiet -m 'a gate that only MENTIONS the function' 2>&1 | Out-Null
        & git push --quiet origin HEAD:refs/heads/main 2>&1 | Out-Null
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.text -cmatch 'ADVISORY') `
        -Message 'a comment naming Get-HeadProvenance does not make the mode BLOCKING'

    # The control: a real definition still does. Without it, "always advisory" would pass above.
    $repo = New-Repo -Name 'realdefinition'
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/gate.ps1'), "function Get-HeadProvenance { }`n", $utf8NoBom)
        & git add -- 'ci/gate.ps1' 2>&1 | Out-Null
        & git commit --quiet -m 'a gate that defines it' 2>&1 | Out-Null
        & git push --quiet origin HEAD:refs/heads/main 2>&1 | Out-Null
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.text -cmatch 'BLOCKING') `
        -Message 'CONTROL: a real definition still makes the mode BLOCKING'

    Write-Host ''
    Write-Host '-- the script path is redacted too --' -ForegroundColor Cyan
    # The message MOST likely to carry a home path, because #733's documented recovery is to extract
    # this script somewhere else and run it from there. The rule was written for the ledger, carried
    # to the repository root, and this third site was still printing raw.
    $loose = Join-Path $fixtureRoot 'loose-copy'
    [System.IO.Directory]::CreateDirectory($loose) | Out-Null
    $looseSubject = Join-Path $loose 'merge-proof.ps1'
    [System.IO.File]::Copy($subjectPath, $looseSubject, $true)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $looseSubject `
            -PullRequest 42 -Head ('a' * 40) 2>&1 | ForEach-Object { [string]$_ })
    $text = ($out -join "`n")
    Assert-True -Condition ($text -cmatch 'is not inside a git working tree') `
        -Message 'ARRANGEMENT: the copy really is outside a work tree, so the message under test appeared'
    Assert-True -Condition ($text -cnotmatch [regex]::Escape([Environment]::GetFolderPath('UserProfile'))) `
        -Message 'and it names no path under the user profile'

    Write-Host ''
    Write-Host '-- a gate that does not parse decides no mode --' -ForegroundColor Cyan
    # `ParseInput` does not THROW on a syntax error: it returns a PARTIAL ast and reports the errors
    # through its second [ref], which I discarded. So the catch written for the unparseable case
    # could never fire, and a gate carrying a real definition beside a syntax error elsewhere was
    # judged from a tree the parser had already given up on.
    $repo = New-Repo -Name 'gateunparseable'
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'ci/gate.ps1'),
            "function Get-HeadProvenance { }`nif (`$true { 'this bracket never closes'`n", $utf8NoBom)
        & git add -- 'ci/gate.ps1' 2>&1 | Out-Null
        & git commit --quiet -m 'a gate with the function AND a syntax error' 2>&1 | Out-Null
        & git push --quiet origin HEAD:refs/heads/main 2>&1 | Out-Null
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated } | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $r = Invoke-Proof -Repo $repo -PullRequest 42 -Head $tip
    Assert-True -Condition ($r.exitCode -eq 1 -and $r.text -cmatch 'does not parse') `
        -Message "a gate that does not parse is HARNESS-BROKE, not a mode (exit $($r.exitCode))"

    Write-Host ''
    Write-Host '-- two unreadable values are not agreement --' -ForegroundColor Cyan
    # When both sides fail the reader, `ok` is false on both and the equality branch never runs, so
    # the pair reads as consistent. The instrument cannot READ the values, which is a different fact
    # from the values matching -- and this file refuses to collapse those anywhere else.
    $repo = New-Repo -Name 'twinbothbad' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $twinName = $gated.Substring(0, 12) + '-2026-09-03T10-00-00.json'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
        dirtyDiffHash = 'committed-side-hash' } -Name $twinName | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'twinbothbad-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger $twinName),
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
            dirtyDiffHash = 'ledger-side-hash' } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cmatch 'the two unreadable values DIFFER') `
        -Message 'two different unreadable values are reported as a disagreement, not passed over'

    # The control: two IDENTICAL unreadable values are not a disagreement -- the pair really does
    # agree, and reporting it would be noise that teaches people to ignore the finding.
    $repo = New-Repo -Name 'twinbothsame' -ProvenanceInGate
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $twinName = $gated.Substring(0, 12) + '-2026-09-03T10-00-00.json'
    Add-Manifest -Repo $repo -Body @{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
        dirtyDiffHash = 'same-bad-hash' } -Name $twinName | Out-Null
    $tip = (& git -C $repo rev-parse HEAD).Trim()
    $ledger = Join-Path $fixtureRoot 'twinbothsame-ledger'
    [System.IO.Directory]::CreateDirectory($ledger) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $ledger $twinName),
        (@{ status = 'GREEN'; pushed = $true; pullRequest = 42; headSha = $gated
            dirtyDiffHash = 'same-bad-hash' } | ConvertTo-Json), $utf8NoBom)
    $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $subjectPath `
            -PullRequest 42 -Head $tip -RepositoryRoot $repo -LedgerDirectory $ledger 2>&1 | ForEach-Object { [string]$_ })
    Assert-True -Condition ((($out -join "`n")) -cnotmatch 'the two unreadable values DIFFER') `
        -Message 'CONTROL: two identical unreadable values are not reported as disagreeing'
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
