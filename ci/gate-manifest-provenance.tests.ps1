# #674(a): can a commit that reached main be traced back to the run that gated it?
#
# A gate run records `headSha`, and a squash merge THROWS THAT COMMIT AWAY. Measured on this
# repository before any of this was written: for the six most recent merges on main, the chain
# main commit -> `(#N)` -> the pull request's head -> a manifest naming that head resolves at every
# link except the last, which is ZERO every time. The ledger is dense and honest about runs; the
# shas it names mostly never existed anywhere but one worktree.
#
# The subject here is `Get-HeadProvenance` in ci/gate.ps1, extracted VERBATIM by anchor text and
# never retyped, together with a walk of the chain over fixture stores. Two things are asserted
# separately and must not be conflated: that the FIELDS are recorded honestly, and that the CHAIN
# they exist for can actually be walked.
#
# `pushed` is decided by EQUALITY and never by an ancestor test. An ancestor test says the remote
# holds some commit this one descends from, which is true of every unpushed commit on a tracked
# branch -- precisely the state the field exists to detect.

$ExpectedAssertionCount = 211
# 'Continue', not 'Stop': these cells run git against fixtures that deliberately have no upstream
# and no pull request, and under Windows PowerShell 5.1 a native command's redirected stderr
# becomes a NativeCommandError that 'Stop' promotes to a terminating error. Judge by exit code and
# output, never by whether git wrote to stderr.
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

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
if (-not (Test-Path -LiteralPath $gatePath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $gatePath" -ForegroundColor Magenta
    exit 2
}
$gateText = [System.IO.File]::ReadAllText($gatePath)

# The function, cut out of the gate by anchor text. Retyping it would make these cells assert a
# copy, and a copy passes while the original rots.
$start = $gateText.IndexOf('function Get-HeadProvenance {')
$end = $gateText.IndexOf('function Write-RunManifest {')
if ($start -lt 0 -or $end -le $start) {
    Write-Host 'HARNESS-BROKE: Get-HeadProvenance was not found between its anchors in ci/gate.ps1' -ForegroundColor Magenta
    exit 2
}
$subject = $gateText.Substring($start, $end - $start)

$pubStart = $gateText.IndexOf('function Publish-RunManifest {')
$pubEnd = $gateText.IndexOf('function Write-RunManifest {')
if ($pubStart -lt 0 -or $pubEnd -le $pubStart) {
    Write-Host 'HARNESS-BROKE: Publish-RunManifest was not found between its anchors in ci/gate.ps1' -ForegroundColor Magenta
    exit 2
}
$publisher = $gateText.Substring($pubStart, $pubEnd - $pubStart)

# #950: the preamble the gate gives `Publish-RunManifest`, DERIVED from `ci/gate.ps1`'s own text and
# never restated here. A hand-copied `Set-StrictMode -Version 2.0` would be correct today and stale
# the next time the gate's line changes, and the harness would go on measuring a strictness nothing
# ships under -- which is the defect this addresses.
#
# The statements come out IN THE ORDER `ci/gate.ps1` ITSELF PUTS THEM -- not an order chosen here.
# Order is semantics, but NOT for the reason this comment gave until #1169. It said the dot-source
# of `ci/manifest-name.ps1` carries its own `Set-StrictMode -Version Latest` into the CALLER's
# scope, "so whether the gate's own `Set-StrictMode` precedes or follows it decides which
# strictness the run ends up under". That sentence is FALSE, and measurably so: `ci/gate.ps1`
# dot-sources `manifest-name.ps1` (:1339) AND `crate-input-hash.ps1` (:1345), and
# `ci/crate-input-hash.ps1:21` sets `Set-StrictMode -Version Latest` in the caller's scope too.
# Moving the gate's own line across manifest-name ALONE therefore decides nothing -- the LAST
# strictness-setting statement still wins, and that is crate-input-hash. What order decides is
# which statement is last across ALL of them.
#
# So the imports reproduced here are not one hard-coded file. `manifest-name.ps1` is required
# because the publisher CALLS `Sync-ManifestCopies` out of it; on top of that, EVERY column-zero
# `. (Join-Path $PSScriptRoot '...')` in `ci/gate.ps1` whose target sets `Set-StrictMode` at column
# zero is reproduced as well. A library that starts setting strictness joins this preamble with no
# edit here; one that stops, leaves. A builder that emitted a fixed sequence, or that reproduced
# only the FIRST such import, would answer identically for a source whose LAST setter had changed,
# and the harness would go on running under a strictness production no longer has. That is not
# hypothetical: with only manifest-name reproduced, mutating `ci/crate-input-hash.ps1` from
# `Latest` to `2.0` changed the strictness `Publish-RunManifest` runs under in production and this
# suite still answered 207/207 passed, exit 0. Nothing reddened. Measured on a detached bench at
# 5d0414df before this was written.
#
# WHAT THIS VOUCHES FOR, AND WHAT IT DOES NOT, stated at the claim.
#   COVERED: column-zero statements in `ci/gate.ps1`, and column-zero `Set-StrictMode` in the
#     libraries `ci/gate.ps1` dot-sources at column zero. The cell "#950/#1169: the derived
#     preamble carries EVERY strictness-setting import" pins that boundary by name.
#   NOT COVERED: strictness set inside a function; strictness set by a library that a reproduced
#     library itself dot-sources (one level is read here, not the transitive closure); and
#     anything `ci/gate.ps1` reaches other than a column-zero dot-source. In those cases
#     production's strictness can move while this preamble does not, and no cell below will
#     redden. That half is stated, not guarded.
#
# Only column-zero lines are eligible: an indented copy inside a function is a different statement
# with a different scope, and a comment mentioning one is not a statement at all. Each of the four
# must match EXACTLY ONCE. Two column-zero `Set-StrictMode` lines is not a thing to pick from --
# the gate would run under the last, a reader here has no way to know that is what was meant, and
# an answer chosen from an ambiguous read is worse than no answer.
function Get-GateRunnerPreamble {
    param([Parameter(Mandatory)] [string] $GateText, [Parameter(Mandatory)] [string] $CiRoot)
    $lines = $GateText -split "`r?`n"
    $manifestNamePattern = "^\. \(Join-Path \`$PSScriptRoot 'manifest-name\.ps1'\)$"
    # Discovered, never listed: every OTHER column-zero dot-source whose target sets strictness at
    # column zero. A hard-coded list is exactly what went stale and let `crate-input-hash.ps1` --
    # the last setter, so the deciding one -- sit outside this preamble unnoticed.
    $strictImports = New-Object 'System.Collections.Generic.List[string]'
    foreach ($line in $lines) {
        if ($line -cmatch $manifestNamePattern) { continue }
        if ($line -cmatch "^\. \(Join-Path \`$PSScriptRoot '([^']+)'\)$") {
            $libPath = Join-Path $CiRoot $Matches[1]
            # A dot-source naming a file that is not there is an UNREADABLE source, not a library
            # that happens to set nothing. Refusing beats answering from a guess.
            if (-not (Test-Path -LiteralPath $libPath)) { return $null }
            $libLines = ([System.IO.File]::ReadAllText($libPath)) -split "`r?`n"
            if (@($libLines | Where-Object { $_ -cmatch '^Set-StrictMode -Version \S+$' }).Count -ge 1) {
                $strictImports.Add('^' + [regex]::Escape($line) + '$')
            }
        }
    }
    $patterns = @(
        '^Set-StrictMode -Version \S+$',
        "^\`$ErrorActionPreference = '\w+'$",
        $manifestNamePattern
    ) + @($strictImports.ToArray()) + @(
        # The publisher READS `$script:headMovedDuringRun` (ci/gate.ps1:4758), and under StrictMode
        # an unset variable THROWS rather than reading as $false. The gate initialises it at top
        # level before any stage runs, so a runner that omits it is not giving the publisher
        # production's state -- it is giving it a state production never has.
        "^\`$script:headMovedDuringRun = \`$(true|false)$"
    )
    # Missing OR duplicated: both are refusals, and both are counted per pattern before anything is
    # returned, so a caller never receives a preamble assembled from a source it could not read.
    foreach ($pattern in $patterns) {
        if (@($lines | Where-Object { $_ -cmatch $pattern }).Count -ne 1) { return $null }
    }
    # One walk of the source, in the source's own order. The statements come out ordered by where
    # they sit in `ci/gate.ps1`, never by their position in `$patterns`.
    $preamble = New-Object 'System.Collections.Generic.List[string]'
    foreach ($line in $lines) {
        foreach ($pattern in $patterns) {
            if ($line -cmatch $pattern) {
                # The runner is written into the fixture directory, so `$PSScriptRoot` there is NOT
                # `ci/`. The path is substituted; the statement is otherwise the gate's own bytes.
                $preamble.Add($line.Replace('$PSScriptRoot', "'" + $CiRoot + "'"))
                break
            }
        }
    }
    return @($preamble.ToArray())
}

$runnerPreambleLines = Get-GateRunnerPreamble -GateText $gateText -CiRoot $PSScriptRoot
if ($null -eq $runnerPreambleLines) {
    Write-Host 'HARNESS-BROKE: the gate preamble (StrictMode, ErrorActionPreference, the manifest-name dot-source and every other strictness-setting dot-source, headMovedDuringRun) was not readable at column zero in ci/gate.ps1: a statement is missing, or duplicated so the read is ambiguous' -ForegroundColor Magenta
    exit 2
}
$runnerPreamble = ($runnerPreambleLines -join "`n") + "`n"

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-provenance-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function New-Repo {
    param([Parameter(Mandatory)] [string] $Name)
    $repo = Join-Path $fixtureRoot $Name
    [System.IO.Directory]::CreateDirectory($repo) | Out-Null
    Push-Location $repo
    try {
        & git init --quiet 2>&1 | Out-Null
        & git config user.email 'fixture@example.invalid' 2>&1 | Out-Null
        & git config user.name 'fixture' 2>&1 | Out-Null
        # Declared, not inherited: a developer with commit.gpgSign and no key, or a global hook that
        # refuses, would otherwise get an empty repository here and every cell below would fail on a
        # subject that never existed. Keep both controls inside the fixture.
        & git config commit.gpgSign false 2>&1 | Out-Null
        & git config core.hooksPath (Join-Path $repo '.no-hooks') 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $repo 'a.txt'), "one`n", $utf8NoBom)
        & git add -A 2>&1 | Out-Null
        & git commit -m 'fixture' --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    return $repo
}

$nonceStart = $gateText.IndexOf('function Write-CanaryNonce {')
$nonceEnd = $gateText.IndexOf("`nfunction ", $nonceStart + 1)
if ($nonceStart -lt 0 -or $nonceEnd -le $nonceStart) {
    Write-Host 'HARNESS-BROKE: Write-CanaryNonce was not found between its anchors in ci/gate.ps1' -ForegroundColor Magenta
    exit 2
}
$nonceWriter = $gateText.Substring($nonceStart, $nonceEnd - $nonceStart)

function Invoke-NonceThenPublish {
    <#
        THE JOURNEY, not the destination. Every other cell here starts from a clean synthetic
        repository and calls the publisher DIRECTLY, so none of them ever travels the path a real
        invocation takes: the gate rewrites its canary nonce first, and that file is tracked. The
        composed behaviour was broken while every cell was green -- cheap cells buying the illusion
        that the expensive path was covered.

        So this one runs the gate's OWN nonce writer, verbatim out of ci/gate.ps1, and then the
        publisher, in one process, in that order.
    #>
    param(
        [Parameter(Mandatory)] [string] $Repo,
        [Parameter(Mandatory)] [string] $ManifestPath,
        [Parameter(Mandatory)] [string] $HeadSha
    )
    $runner = Join-Path $fixtureRoot 'run-journey.ps1'
    # #950: the journey runner reaches the same publisher, so it gets the same environment the gate
    # gives it -- derived, never restated.
    $script = (Get-PublisherRunnerPreamble) + "`$repositoryRoot = '$Repo'`n" + $nonceWriter + "`n" + $publisher +
        "`nWrite-CanaryNonce`nPublish-RunManifest -ManifestPath '$ManifestPath' -HeadSha '$HeadSha' -PullRequest 7 " +
        "-BranchRef '$((& git -C $Repo symbolic-ref --quiet HEAD).Trim())'`n"
    [System.IO.File]::WriteAllText($runner, $script, $utf8NoBom)
    Push-Location $Repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>&1 | ForEach-Object { [string]$_ })
    } finally { Pop-Location }
    return ($out -join "`n")
}

function Get-PublisherRunnerPreamble {
    <#
        The environment the extracted publisher runs under. #950: it is the gate's, derived.
    #>
    # #949 left a workaround here -- the library dot-source plus an explicit `Set-StrictMode -Off`,
    # which is neither production's strictness nor production's error preference. This is the gate's
    # own preamble instead, read out of `ci/gate.ps1` (see `Get-GateRunnerPreamble`), so the two
    # cannot drift: a change at the gate's column-zero lines travels here with no edit.
    return $runnerPreamble
}

function Invoke-Publisher {
    <# Runs the extracted Publish-RunManifest inside a fixture repository. #>
    param(
        [Parameter(Mandatory)] [string] $Repo,
        [Parameter(Mandatory)] [string] $ManifestPath,
        [Parameter(Mandatory)] [string] $HeadSha,
        $PullRequest,
        [string] $Content,
        [string[]] $Copies = @()
    )
    $runner = Join-Path $fixtureRoot 'run-publisher.ps1'
    $pr = if ($null -eq $PullRequest) { '$null' } else { [string]$PullRequest }
    # The flag is printed after the call: the CAS refusal has to REACH the verdict, and the only
    # way a cell can see that from outside the gate is to ask the variable the verdict reads.
    # The branch is passed in, as the gate passes it: captured before the stages, never read here.
    # A harness that let the publisher find its own branch would be testing a program the gate no
    # longer runs.
    $branch = (& git -C $Repo symbolic-ref --quiet HEAD).Trim()
    # `-Content` too, as the gate passes it: the commit must carry the bytes the RUN serialized, not
    # whatever is at the path when the publisher gets there.
    $contentArg = if ($Content) { " -Content '" + ($Content -replace "'", "''") + "'" } else { '' }
    # The copies too, as the gate passes them: reconciling only the path this function was handed is
    # exactly the defect the cell below measures.
    $copiesArg = if ($Copies -and $Copies.Count -gt 0) {
        " -Copies @(" + (($Copies | ForEach-Object { "'" + ($_ -replace "'", "''") + "'" }) -join ',') + ")"
    } else { '' }
    # #938: the publisher now CALLS `Sync-ManifestCopies`, so the extracted copy needs the same
    # library the real gate dot-sources at `ci/gate.ps1:672`. Without this the subject runs with the
    # function undefined and every reconciliation cell fails for a reason that is about the harness.
    # AND THE LIBRARY'S STRICTNESS IS NOT THE HARNESS'S. `ci/manifest-name.ps1:18` sets
    # `Set-StrictMode -Version Latest`, and a dot-source runs in the CALLER's scope -- so importing
    # it for one function silently imposed Latest on a runner that had never had StrictMode at all.
    # Measured: two head-provenance cells that pass without the library fail with it, and they have
    # nothing to do with reconciliation.
    #
    # #950 CLOSED THAT: `-Off` (half of production's preamble, a combination that exists nowhere)
    # is gone, and the runner now reproduces the gate's WHOLE preamble, read out of `ci/gate.ps1`
    # in the gate's own order by `Get-GateRunnerPreamble`. The earlier note here saying that was
    # "worth doing and is not this change" described the state before that function existed.
    $library = Get-PublisherRunnerPreamble
    $script = $library + $publisher + "`nPublish-RunManifest -ManifestPath '$ManifestPath' -HeadSha '$HeadSha' -PullRequest $pr -BranchRef '$branch'$contentArg$copiesArg" +
        "`nWrite-Output ('HEADMOVEDFLAG=' + [bool]`$script:headMovedDuringRun)`n"
    # The bytes the subprocess is actually handed, kept so a cell can assert what environment the
    # publisher ran under. Asserting the BUILDER instead would pass while the runner got something
    # else.
    $script:lastPublisherScript = $script
    [System.IO.File]::WriteAllText($runner, $script, $utf8NoBom)
    Push-Location $Repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>&1 | ForEach-Object { [string]$_ })
    } finally { Pop-Location }
    return ($out -join "`n")
}

function New-GhStub {
    <#
        A `gh` on PATH that answers with a canned `pr list` result.

        The lookup this guards cannot be reached with the real tool: a fixture repository has no
        GitHub remote, so every cell would end at "nobody could look" and the SELECTION -- which of
        several pull requests sharing a branch name belongs to this head -- would never run. The
        stub answers exactly what `gh pr list --json number,headRefOid` answers and nothing else,
        so what is being tested is the gate's choice among the candidates, not gh.
    #>
    param(
        [Parameter(Mandatory)] [string] $Name, [Parameter(Mandatory)] [string] $Json,
        # The `--search` answer, when it differs from the `--head` one. The two queries are the
        # point of the renamed-branch cells: the branch name finds nothing and the head still does.
        [string] $SearchJson,
        # Make one query FAIL rather than answer. A transiently failing search beside a
        # successful-but-empty branch query is the case that used to read as "no open pull request".
        [ValidateSet('none', 'search', 'head')] [string] $Fail = 'none',
        # Fail `auth status` unless it names github.com -- the shape of a gh that holds a dead token
        # for another host while github.com is fine.
        [switch] $FailUnscopedAuth,
        # Answer only when the `--head` argument names THIS branch. The assertion is then about
        # which name reached gh, not merely about the number that came back.
        [string] $RequireHead
    )

    $dir = Join-Path $fixtureRoot "ghstub-$Name"
    [System.IO.Directory]::CreateDirectory($dir) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $dir 'response.json'), $Json, $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $dir 'search.json'), $(if ($SearchJson) { $SearchJson } else { $Json }), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $dir 'fail.txt'), $Fail, $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $dir 'authscope.txt'), $(if ($FailUnscopedAuth) { 'scoped' } else { 'any' }), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $dir 'requirehead.txt'), $RequireHead, $utf8NoBom)

    # IT HAS TO ANSWER BOTH SUBJECTS, or the red it produces is red for the wrong reason. The
    # previous revision asks for `--jq '.[0].number'` and this one asks for the whole array, so a
    # stub that ignored its arguments handed raw JSON to a caller expecting one number: the old
    # code then failed to PARSE, the cell went red, and nothing was shown about the defect the cell
    # is named after -- picking the first pull request in the list. Emulating `--jq` is what makes
    # the same cell red on the old subject for the SELECTION and green here.
    $stub = @'
param()
$argline = ($args -join ' ')
if ($argline -cmatch '(^|\s)auth(\s|$)') {
    $scope = ([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'authscope.txt'))).Trim()
    if ($scope -ceq 'scoped' -and $argline -cnotmatch '--hostname\s+github\.com') { exit 1 }
    exit 0
}
$fail = ([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fail.txt'))).Trim()
if ($fail -ceq 'search' -and $argline -cmatch '--search') { exit 3 }
if ($fail -ceq 'head' -and $argline -cmatch '--head') { exit 3 }
$requireHead = ([System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'requirehead.txt'))).Trim()
if ($requireHead -and $argline -cmatch '--head' -and $argline -cnotmatch ('--head\s+' + [regex]::Escape($requireHead) + '(\s|$)')) {
    Write-Output '[]'
    exit 0
}
$file = if ($argline -cmatch '--search') { 'search.json' } else { 'response.json' }
$json = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot $file))
if ($argline -cmatch '--jq') {
    # Exactly what `--jq '.[0].number'` answers, including its silence on an empty array.
    $parsed = @(($json | ConvertFrom-Json) | ForEach-Object { $_ })
    if ($parsed.Count -gt 0) { Write-Output ([string]$parsed[0].number) }
    exit 0
}
Write-Output $json
exit 0
'@
    [System.IO.File]::WriteAllText((Join-Path $dir 'gh.ps1'), $stub, $utf8NoBom)
    # A `.cmd` because that is what a bare `gh` resolves to on PATH here; it forwards its arguments
    # so the script above can see which shape of answer is being asked for.
    $cmd = @'
@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0gh.ps1" %*
exit /b %ERRORLEVEL%
'@
    [System.IO.File]::WriteAllText((Join-Path $dir 'gh.cmd'), $cmd, $utf8NoBom)
    return $dir
}

function Format-ProvenanceForFailure {
    <#
        WHAT THE RECORD SAID, in the failure message. `(pushed=)` is an empty that does not
        distinguish "the upstream never resolved" from "the server did not answer" -- the same
        collapse this whole pull request is closing, in the cells that measure it. A reviewer on
        another machine spent a run per hypothesis because the message carried no reason.
    #>
    param([AllowNull()] $Result)
    if ($null -eq $Result) { return 'no result at all' }
    return ("pushed=[$($Result.pushed)] upstreamSha=[$($Result.upstreamSha)] " +
        "reason=[$($Result.pushedReason)]")
}

function Assert-UpstreamArranged {
    <#
        THE ARRANGEMENT ANSWERS BEFORE THE ASSERTION. Every cell below that measures `pushed` first
        builds a fixture WITH an upstream -- push -u, set-upstream-to, fetch -- and those three
        commands have their output sent to Out-Null. So on a machine where they fail, the cell that
        follows measures a repository with no upstream and reports it as a defect in the subject.

        Measured, not imagined: a reviewer on another machine had EVERY upstream-dependent cell fail
        and every other cell pass, and could not tell whether the fixture or the function was at
        fault. This asserts the fixture, and prints what git says when it is not there.
    #>
    param([Parameter(Mandatory)] [string] $Repo, [Parameter(Mandatory)] [string] $What)
    $branch = (& git -C $Repo symbolic-ref --quiet --short HEAD 2>&1 | ForEach-Object { [string]$_ } | Select-Object -First 1)
    $resolved = @(& git -C $Repo rev-parse --symbolic-full-name "$branch@{upstream}" 2>&1 | ForEach-Object { [string]$_ })
    $exit = $LASTEXITCODE
    $ref = @($resolved | Where-Object { $_ -cmatch '^refs/' } | Select-Object -First 1)
    Assert-True -Condition ($exit -eq 0 -and $ref) `
        -Message "ARRANGEMENT: $What -- the fixture really has an upstream (branch [$branch], git exited $exit and said: $(($resolved | Select-Object -First 2) -join ' | '))"
}

function Invoke-ProvenanceRaw {
    <#
        The same extracted function, run for its TEXT rather than its result.

        `Invoke-Provenance` parses stdout as JSON, so a run that REFUSES -- which is the subject of
        the malformed-bound cells -- would fail there with a JSON error naming nothing. This one
        returns whatever the run wrote, so the assertion can be about the refusal's own words.
    #>
    param([Parameter(Mandatory)] [string] $Repo)

    $runner = Join-Path $fixtureRoot 'run-provenance-raw.ps1'
    $script = $subject + @'

$head = (git rev-parse HEAD).Trim()
$branch = (git symbolic-ref --quiet HEAD).Trim()
Get-HeadProvenance -HeadSha $head -BranchRef $branch | ConvertTo-Json -Compress
'@
    [System.IO.File]::WriteAllText($runner, $script, $utf8NoBom)
    Push-Location $Repo
    try {
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>&1
        return [ordered]@{ text = (@($out | ForEach-Object { [string]$_ }) -join "`n") }
    } finally { Pop-Location }
}

function Invoke-Provenance {
    <# Runs the extracted function inside a fixture repository and returns its result as JSON. #>
    param([Parameter(Mandatory)] [string] $Repo, [string] $StubDir, [string] $PathOverride,
        # The head to ask about. Default: the repository's own HEAD, which is what the gate passes.
        # A cell overrides it to hand the function a head that differs from the server's tip only by
        # a code point the culture comparer ignores -- the one input that tells the two comparers
        # apart at the comparison this whole field rests on.
        [string] $HeadShaOverride)

    $runner = Join-Path $fixtureRoot 'run-provenance.ps1'
    # The branch is handed in, as the gate hands it: captured before the stages. A harness that let
    # the function read its own branch would be exercising a program the gate no longer runs -- and
    # it is the read that a checkout onto another branch at the SAME commit silently redirects.
    $script = $subject + @'

$head = (git rev-parse HEAD).Trim()
if ($env:GATE_TEST_HEAD_OVERRIDE) { $head = $env:GATE_TEST_HEAD_OVERRIDE }
$branch = (git symbolic-ref --quiet HEAD).Trim()
Get-HeadProvenance -HeadSha $head -BranchRef $branch | ConvertTo-Json -Compress
'@
    [System.IO.File]::WriteAllText($runner, $script, $utf8NoBom)
    $previousPath = $env:PATH
    # `PathOverride` REPLACES the path rather than prepending to it: the only way to measure an
    # absent `gh` is a PATH that does not contain one, and git has to stay reachable, so the cell
    # passes a directory holding neither and the runner is invoked by absolute path.
    if ($PathOverride) { $env:PATH = $PathOverride }
    elseif ($StubDir) { $env:PATH = "$StubDir;$env:PATH" }
    $previousHeadOverride = $env:GATE_TEST_HEAD_OVERRIDE
    if ($HeadShaOverride) { $env:GATE_TEST_HEAD_OVERRIDE = $HeadShaOverride }
    Push-Location $Repo
    try {
        # `2>&1`, not `2>$null`: this harness proves that the subject keeps git's words, and it was
        # dropping the subject's own. Measured by J as NOT the cause of his failures -- he swapped it
        # and nothing changed -- so it is kept for the day it is, not sold as a fix.
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>&1 |
                ForEach-Object { [string]$_ })
    } finally {
        Pop-Location
        $env:PATH = $previousPath
        if ($null -eq $previousHeadOverride) { Remove-Item Env:\GATE_TEST_HEAD_OVERRIDE -ErrorAction SilentlyContinue }
        else { $env:GATE_TEST_HEAD_OVERRIDE = $previousHeadOverride }
    }
    $json = ($out | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1)
    if (-not $json) { return $null }
    return $json | ConvertFrom-Json
}

try {
    # ---- No upstream at all: `pushed` is UNKNOWN. This cell used to assert FALSE, and the
    # assertion was wrong: `for-each-ref ... refs/remotes` reads the LOCAL ref database, and a
    # commit can be on the server with no local ref naming it -- `git push https://.../r.git
    # HEAD:feature` creates none. "No local ref contains it" is not "the server never got it", and
    # recording false asserted something this machine cannot see. Rewriting a cell that certified
    # the old behaviour is worth saying out loud: the behaviour was the defect, not the cell.
    Write-Host ''
    Write-Host '-- a branch with no upstream is UNKNOWN, not unpushed --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'noupstream'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -ne $result) -Message 'the function returns a result at all'
    Assert-True -Condition ($null -eq $result.pushed -and $result.pushedReason -cmatch 'no server to ask') `
        -Message 'pushed is null, and the reason says there was no server to ask'
    Assert-True -Condition ($null -eq $result.upstreamSha) `
        -Message 'and upstreamSha is null rather than a guess'

    # ---- HEAD equal to upstream: pushed.
    Write-Host ''
    Write-Host '-- HEAD equal to its upstream is pushed --' -ForegroundColor Cyan
    $origin = New-Repo -Name 'origin-bare'
    $repo = New-Repo -Name 'tracked'
    Push-Location $repo
    try {
        & git remote add origin (Join-Path $fixtureRoot 'origin-bare') 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
    } finally { Pop-Location }
    Assert-UpstreamArranged -Repo $repo -What 'HEAD equal to its upstream'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -ne $result -and $result.pushed -eq $true) `
        -Message "HEAD identical to upstream reads as pushed ($(Format-ProvenanceForFailure -Result $result))"
    Assert-True -Condition ($null -ne $result.upstreamSha) `
        -Message "and the upstream sha is recorded beside it, so the claim can be re-checked ($(Format-ProvenanceForFailure -Result $result))"

    # ---- One commit past the upstream: the state this field exists for.
    #
    # This is where an ancestor test would say YES: the remote holds a commit this one descends
    # from. Equality says no, which is the truth the button will need.
    Write-Host ''
    Write-Host '-- one commit past the upstream is NOT pushed, though it descends from it --' -ForegroundColor Cyan
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'a.txt'), "two`n", $utf8NoBom)
        & git commit -am 'unpushed' --quiet 2>&1 | Out-Null
        $ancestor = & git merge-base --is-ancestor '@{upstream}' HEAD 2>$null
        $ancestorSaysYes = ($LASTEXITCODE -eq 0)
    } finally { Pop-Location }
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($ancestorSaysYes) `
        -Message 'ARRANGEMENT: an ancestor test DOES say yes here -- that is why equality is the test'
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message 'and equality does not say YES: the fast path stays silent instead of certifying'
    Assert-True -Condition ($result.upstreamSha -cne (& git -C $repo rev-parse HEAD).Trim()) `
        -Message 'the recorded upstream sha is the OTHER commit, so the disagreement is legible'

    # ---- The pull request number: recorded or explained, never invented.
    Write-Host ''
    Write-Host '-- a branch with no pull request records why, rather than a guess --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'nopr'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -eq $result.pullRequest) `
        -Message 'pullRequest is null when none can be looked up'
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($result.pullRequestReason)) `
        -Message 'and a reason is recorded: "unknown" and "absent" are different states'
    Assert-True -Condition ($result.pullRequestReason -cnotmatch 'fixture|nopr') `
        -Message 'and the reason is not the branch name dressed up as an answer'

    # ---- THE CHAIN, walked over a fixture store.
    #
    # The criterion frozen on #674 is a chain, not a lookup: main commit -> `(#N)` -> the pull
    # request's head -> a manifest naming that head. Written as a lookup it would be RED FOREVER,
    # because the commit on main is the squash and no manifest written before the merge can carry
    # it. Here the chain is walked over a fixture store so the cell can be green for a real head
    # today and red for each broken link by name.
    Write-Host ''
    Write-Host '-- the chain resolves, and names which link is missing when one is --' -ForegroundColor Cyan
    $store = Join-Path $fixtureRoot 'store'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    # THE SHAPE A PUBLISHED RUN ACTUALLY HAS. The fixture used to make the gated head and the pull
    # request head the same sha, which is the one shape a published run never has: the gate commits
    # the manifest, so the head becomes that COMMIT and the manifest names its PARENT. A chain test
    # that compares the two directly would report "no manifest names the head" for every real
    # publication, and it was passing because the fixture had removed the difference it exists to
    # cross.
    $gatedHead = 'a' * 40      # what the stages ran against, and what the manifest names
    $publishedTip = 'b' * 40   # the manifest commit itself, which is where the branch now points
    [System.IO.File]::WriteAllText((Join-Path $store 'run.json'),
        (@{ headSha = $gatedHead; pullRequest = 4242; pushed = $true; status = 'GREEN' } | ConvertTo-Json), $utf8NoBom)

    function Resolve-Chain {
        param(
            [string] $Subject, [hashtable] $PrHeads, [string] $StoreDir,
            # tip -> its parent, and whether that tip touches only the manifest store. Both are
            # facts git answers for a real head; here they are the fixture's job.
            [hashtable] $Parents = @{}, [hashtable] $TipIsStoreOnly = @{}
        )
        if ($Subject -cnotmatch '\(#(\d+)\)\s*$') { return 'no (#N) in the merge title' }
        $number = [int]$Matches[1]
        if (-not $PrHeads.ContainsKey($number)) { return "no head recorded for pull request #$number" }
        $head = $PrHeads[$number]
        $records = @(Get-ChildItem -LiteralPath $StoreDir -Filter '*.json' -File |
                ForEach-Object { (Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json) })
        if (@($records | Where-Object { $_.headSha -ceq $head })) { return 'certified' }
        # The parent is accepted ONLY when the tip adds nothing but the record -- the same rule the
        # checker applies, and the reason the manifest commit does not break the chain.
        $parent = if ($Parents.ContainsKey($head)) { $Parents[$head] } else { $null }
        if ($parent -and @($records | Where-Object { $_.headSha -ceq $parent })) {
            if ($TipIsStoreOnly[$head]) { return 'certified' }
            return "the manifest names the parent of pull request #$number, but its tip touches more than the store"
        }
        return "no manifest names the head of pull request #$number"
    }

    $heads = @{ 4242 = $publishedTip }
    $parents = @{ $publishedTip = $gatedHead }
    $pureTips = @{ $publishedTip = $true }
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing (#4242)' -PrHeads $heads -StoreDir $store `
                -Parents $parents -TipIsStoreOnly $pureTips) -ceq 'certified') `
        -Message 'a published run -- tip is the manifest commit, manifest names its parent -- is certified end to end'

    # And the parent is not a free pass: the same shape with a tip that touches more than the store
    # is refused, which is the whole reason the parent rule is safe to have.
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing (#4242)' -PrHeads $heads -StoreDir $store `
                -Parents $parents -TipIsStoreOnly @{ $publishedTip = $false }) -cmatch 'touches more than the store') `
        -Message 'a tip that is NOT store-only cannot borrow its parent manifest'

    # The un-published shape still works, because a run that never committed its manifest is the
    # state this ticket exists to make visible rather than a state that must resolve.
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing (#4242)' -PrHeads @{ 4242 = $gatedHead } -StoreDir $store) -ceq 'certified') `
        -Message 'and a head that IS the gated sha still resolves directly'
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing' -PrHeads $heads -StoreDir $store) -cmatch 'no \(#N\)') `
        -Message 'a title with no (#N) names THAT link, not "no run found"'
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing (#9999)' -PrHeads $heads -StoreDir $store) -cmatch 'no head recorded') `
        -Message 'an unknown pull request names THAT link'
    $heads2 = @{ 4242 = ('b' * 40) }
    Assert-True -Condition ((Resolve-Chain -Subject 'feat: a thing (#4242)' -PrHeads $heads2 -StoreDir $store) -cmatch 'no manifest names') `
        -Message 'a head with no manifest names THAT link -- three defects, three repairs, three messages'

    # ---- The manifest is COMMITTED, alone, and the gated head is the parent of that commit.
    #
    # The Observer's structural finding: the commit that records the run BECOMES the head, so
    # `headSha == head` is false by construction and the button's rule has to be
    # `headSha == parent(head)` with the tip proved to add nothing but the record. These cells are
    # what make that provable with git alone.
    Write-Host ''
    Write-Host '-- the manifest commit is pure, and the gated head is its parent --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'publish'
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 4242

    $newHead = (& git -C $repo rev-parse HEAD).Trim()
    Assert-True -Condition ($newHead -cne $gated) `
        -Message "the manifest was committed, so the head moved$(if ($newHead -ceq $gated) { " -- the publisher said: $out" })"
    Assert-True -Condition ((& git -C $repo rev-parse 'HEAD~1').Trim() -ceq $gated) `
        -Message 'and the GATED head is the parent of it -- which is what the button compares against'
    $touched = @(& git -C $repo diff-tree --no-commit-id --name-only -r HEAD)
    Assert-True -Condition ($touched.Count -gt 0 -and -not @($touched | Where-Object { -not $_.StartsWith('.factory/gate-runs/') })) `
        -Message "the tip touches ONLY the manifest store (touched: $($touched -join ', '))"
    Assert-True -Condition ((& git -C $repo log -1 --format=%s).Trim() -ceq "gate: run manifest for $gated (#4242)") `
        -Message 'and the message has the fixed form a checker can parse'

    Write-Host ''
    Write-Host '-- a tree holding the author work is NOT committed into --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'dirty'
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    [System.IO.File]::WriteAllText((Join-Path $repo 'a.txt'), "the author was in the middle of this`n", $utf8NoBom)
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest $null

    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
        -Message 'the head did NOT move: nothing was committed'
    Assert-True -Condition ($out -cmatch 'NOT committed') `
        -Message 'and the gate says so rather than passing over it in silence'
    Assert-True -Condition ([System.IO.File]::ReadAllText((Join-Path $repo 'a.txt')) -cmatch 'in the middle of this') `
        -Message "and the author's uncommitted work is untouched, which is the point of the refusal"

    # ---- Pushed WITHOUT an upstream: on the server, and no tracking branch to prove it.
    Write-Host ''
    Write-Host '-- a commit pushed by explicit refspec has no upstream and IS on the remote --' -ForegroundColor Cyan
    $bare = Join-Path $fixtureRoot 'bare-two.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'norefspec'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        # No -u: this is exactly how a contributor pushes a one-off branch.
        & git push origin HEAD:refs/heads/feature --quiet 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        $hasUpstream = (& git rev-parse '@{upstream}' 2>$null); $upstreamExists = ($LASTEXITCODE -eq 0)
    } finally { Pop-Location }
    Assert-True -Condition (-not $upstreamExists) `
        -Message 'ARRANGEMENT: there really is no upstream, which is what made equality answer wrongly'
    $result = Invoke-Provenance -Repo $repo
    # THIS CELL USED TO ASSERT TRUE, AND THE TRUE WAS READ OUT OF A LOCAL CACHE. The commit really
    # is on the server here -- the fixture pushed it -- but nothing in this repository MEASURED that:
    # `for-each-ref refs/remotes` reads what the last fetch left behind, and the same reading is
    # produced by a server that has since dropped the branch. Containment cannot be asked of tips,
    # so the honest answer is the third state, and the record says the local ref is memory.
    #
    # The cost is stated rather than hidden: a contributor who pushes by explicit refspec now gets
    # `pushed: null` and #674(b) will ask instead of certifying. That is the direction this field is
    # supposed to fail in.
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message 'the commit reads as UNKNOWN: no tracked branch means no server was asked'
    Assert-True -Condition ($result.pushedReason -cmatch 'memory from the last fetch') `
        -Message 'and the reason says the local ref that names it is memory, not a reading of the server'

    # ---- A LOCAL REF IS MEMORY. This is the root: `refs/remotes/*` is what the last successful
    # fetch left behind, and both certifying paths read it. Delete or force-push the upstream
    # afterwards and the local ref still equals HEAD while the server does not have that commit as
    # any tip -- and because #674(b) refuses a manifest whose `pushed` is not TRUE, a stale true is
    # PERMISSION TO MERGE granted on evidence this machine cannot see.
    #
    # The fixture does not simulate staleness: it MAKES it, by deleting the branch on a real bare
    # remote after the fetch that recorded it.
    Write-Host ''
    Write-Host '-- a deleted upstream leaves a local ref that still equals HEAD --' -ForegroundColor Cyan
    $bare = Join-Path $fixtureRoot 'bare-stale.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'stalelocal'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        # The server drops the branch. Nothing tells this repository, and nothing is supposed to.
        & git --git-dir $bare update-ref -d refs/heads/fixture 2>&1 | Out-Null
        $head = (& git rev-parse HEAD).Trim()
        $localRef = (& git rev-parse refs/remotes/origin/fixture 2>$null | Select-Object -First 1)
        $serverHas = @(& git ls-remote $bare refs/heads/fixture 2>$null | Where-Object { $_ })
    } finally { Pop-Location }
    Assert-UpstreamArranged -Repo $repo -What 'a deleted upstream'
    Assert-True -Condition (([string]$localRef).Trim() -ceq $head) `
        -Message 'ARRANGEMENT: the local remote-tracking ref still equals HEAD, which is what used to certify'
    Assert-True -Condition ($serverHas.Count -eq 0) `
        -Message 'ARRANGEMENT: and the server no longer has that branch at all'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message "a deleted upstream does not certify a push ($(Format-ProvenanceForFailure -Result $result))"
    Assert-True -Condition ($result.pushedReason -cmatch 'has no branch fixture') `
        -Message 'and the reason says the server has no such branch, rather than repeating the local ref'
    Assert-True -Condition ($result.pushedReason -cmatch 'memory from the last fetch') `
        -Message 'while still naming the local ref as memory, because hiding it would lose the diagnosis'

    Write-Host ''
    Write-Host '-- a force-pushed upstream leaves the same stale equality --' -ForegroundColor Cyan
    # The other half of the same defect, and the one that leaves the server ANSWERING: the branch
    # exists, its tip is something else, and the gated commit is gone from it.
    $bare = Join-Path $fixtureRoot 'bare-forced.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'forcedlocal'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        $head = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    # THE FORCE-PUSH COMES FROM ANOTHER CLONE, because a push from THIS repository updates its own
    # remote-tracking ref -- the first version of this cell force-pushed from the fixture and then
    # asserted that the local ref was stale, which its own arrangement had just repaired. Staleness
    # is what happens when somebody ELSE moves the branch.
    $other = Join-Path $fixtureRoot 'forced-other'
    $inertHooks = Join-Path $fixtureRoot 'inert-clone-hooks'
    [System.IO.Directory]::CreateDirectory($inertHooks) | Out-Null
    $cloneOutput = @(& git -c "core.hooksPath=$inertHooks" clone --quiet $bare $other 2>&1)
    $cloneCode = $LASTEXITCODE
    $cloneReady = ($cloneCode -eq 0 -and (Test-Path -LiteralPath $other -PathType Container))
    Assert-True -Condition ($cloneCode -eq 0) `
        -Message "the fixture clone succeeds with its inert hooks path (rc=$cloneCode; output: $($cloneOutput -join ' '))"
    Assert-True -Condition $cloneReady `
        -Message 'the clone destination exists before the fixture enters it'
    if ($cloneReady) {
        Push-Location -LiteralPath $other -ErrorAction Stop
        try {
            & git config user.email 'fixture@example.invalid' 2>&1 | Out-Null
            & git config user.name 'Fixture' 2>&1 | Out-Null
            & git config commit.gpgSign false 2>&1 | Out-Null
            & git config core.hooksPath (Join-Path $other '.no-hooks') 2>&1 | Out-Null
            & git checkout --quiet -B fixture origin/fixture 2>&1 | Out-Null
            [System.IO.File]::WriteAllText((Join-Path $other 'a.txt'), "forced`n", $utf8NoBom)
            & git commit -am 'forced' --quiet 2>&1 | Out-Null
            $forcedSha = (& git rev-parse HEAD).Trim()
            & git push --force origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        } finally { Pop-Location }
    }
    $localRef = (& git -C $repo rev-parse refs/remotes/origin/fixture 2>$null | Select-Object -First 1)
    $serverTip = @(& git ls-remote $bare refs/heads/fixture 2>$null | Where-Object { $_ })
    Assert-UpstreamArranged -Repo $repo -What 'a force-pushed upstream'
    Assert-True -Condition (([string]$localRef).Trim() -ceq $head) `
        -Message 'ARRANGEMENT: the local ref still equals HEAD after somebody else force-pushed over the branch'
    Assert-True -Condition ($serverTip.Count -eq 1 -and $serverTip[0].StartsWith($forcedSha)) `
        -Message 'ARRANGEMENT: and the server tip really is the other commit now'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message "a force-pushed upstream does not certify a push ($(Format-ProvenanceForFailure -Result $result))"
    Assert-True -Condition ($result.pushedReason.Contains($forcedSha)) `
        -Message 'and the reason names the tip the server actually reported, so the two can be compared'

    # A GLOBAL reference-transaction hook can reject the clone's initial ref update. The clone
    # command must be judged by its own exit code and destination before any Push-Location: when it
    # fails, the old sequence continued in the caller's repository and ran its fixture config writes
    # there. This hostile configuration is isolated to the throwaway child command and is restored
    # before the normal force-push clone below.
    $hostileHooks = Join-Path $fixtureRoot 'forced-global-hooks'
    $hostileGlobal = Join-Path $fixtureRoot 'forced-global.config'
    [System.IO.Directory]::CreateDirectory($hostileHooks) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $hostileHooks 'reference-transaction'), "#!/bin/sh`nexit 1`n", $utf8NoBom)
    $hostileHooksConfig = $hostileHooks -replace '\\', '/'
    [System.IO.File]::WriteAllText($hostileGlobal, "[core]`n    hooksPath = $hostileHooksConfig`n", $utf8NoBom)
    $hostileOther = Join-Path $fixtureRoot 'forced-hostile-other'
    $callerConfigBefore = [string]((& git -C $repo config --local --list 2>$null) -join "`n")
    $previousGlobalConfig = $env:GIT_CONFIG_GLOBAL
    try {
        $env:GIT_CONFIG_GLOBAL = $hostileGlobal
        $hostileCloneOutput = @(& git clone --quiet $bare $hostileOther 2>&1)
        $hostileCloneCode = $LASTEXITCODE
        $callerConfigAfter = [string]((& git -C $repo config --local --list 2>$null) -join "`n")
    } finally {
        if ($null -eq $previousGlobalConfig) { Remove-Item Env:GIT_CONFIG_GLOBAL -ErrorAction SilentlyContinue }
        else { $env:GIT_CONFIG_GLOBAL = $previousGlobalConfig }
    }
    Assert-True -Condition (Test-Path -LiteralPath (Join-Path $hostileHooks 'reference-transaction')) `
        -Message 'ARRANGEMENT: the hostile global reference-transaction hook exists'
    Assert-True -Condition (Test-Path -LiteralPath $hostileGlobal) `
        -Message 'ARRANGEMENT: the hostile global config is the one supplied to clone'
    Assert-True -Condition ($hostileCloneCode -ne 0) `
        -Message "a rejecting global reference-transaction hook makes clone fail (rc=$hostileCloneCode; output: $($hostileCloneOutput -join ' '))"
    Assert-True -Condition (-not (Test-Path -LiteralPath $hostileOther)) `
        -Message 'a failed clone leaves no destination for a later Push-Location'
    Assert-True -Condition ($callerConfigAfter -ceq $callerConfigBefore) `
        -Message 'a failed clone does not change the caller repository config'
    $testSource = [System.IO.File]::ReadAllText($PSCommandPath)
    $cloneBlockStart = $testSource.IndexOf("`$other = Join-Path `$fixtureRoot 'forced-other'")
    $cloneBlockEnd = $testSource.IndexOf("`n        try {", $cloneBlockStart)
    $cloneBlock = if ($cloneBlockStart -ge 0 -and $cloneBlockEnd -gt $cloneBlockStart) {
        $testSource.Substring($cloneBlockStart, $cloneBlockEnd - $cloneBlockStart)
    } else { '' }
    Assert-True -Condition ($cloneBlock -match 'git -c .*core\.hooksPath=.* clone') `
        -Message 'the production-shaped fixture clone overrides hostile global hooks with an inert hooks path'
    Assert-True -Condition ($cloneBlock -match '\$cloneCode\s*=\s*\$LASTEXITCODE[\s\S]*Test-Path -LiteralPath \$other') `
        -Message 'the fixture checks clone exit and destination before Push-Location can edit the caller'
    Assert-True -Condition ($cloneBlock -match 'Push-Location -LiteralPath \$other -ErrorAction Stop') `
        -Message 'the fixture enters the verified clone destination with a literal, terminating location change'

    Write-Host ''
    Write-Host '-- a server that cannot be reached is not an answer --' -ForegroundColor Cyan
    # The direction that matters: a failed lookup must not read as false OR as true. The remote is
    # made unreachable by removing it, which is the cheapest honest version of "the network is down".
    $bare = Join-Path $fixtureRoot 'bare-gone.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'unreachable'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    Assert-UpstreamArranged -Repo $repo -What 'an unreachable server'
    Remove-Item -LiteralPath $bare -Recurse -Force -ErrorAction SilentlyContinue
    # The arrangement, asserted rather than assumed: a Remove-Item that loses to a file still held
    # open leaves a REACHABLE server, and this cell would then measure the ordinary path while
    # reading as proof about the unreachable one.
    Assert-True -Condition (-not (Test-Path -LiteralPath $bare)) `
        -Message 'ARRANGEMENT: the remote really is gone from disk'
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message "an unreachable server leaves pushed unknown ($(Format-ProvenanceForFailure -Result $result))"
    Assert-True -Condition ($result.pushedReason -cmatch 'asking the server failed') `
        -Message 'and the reason says the lookup failed, not that the commit is missing'

    Write-Host ''
    Write-Host '-- and the wait is bounded, with the bound proved rather than described --' -ForegroundColor Cyan
    # A timeout is the third state too. Proved by shrinking the bound to zero rather than by waiting
    # out the default: a cell that pays 30 seconds to prove a timeout is a tax on every run.
    $bare = Join-Path $fixtureRoot 'bare-slow.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'bounded'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    Assert-UpstreamArranged -Repo $repo -What 'the bounded lookup'
    $previousTimeout = $env:GATE_LS_REMOTE_TIMEOUT_SECONDS
    $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = '0'
    try { $result = Invoke-Provenance -Repo $repo } finally { $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = $previousTimeout }
    Assert-True -Condition ($null -eq $result.pushed -and $result.pushedReason -cmatch 'did not answer within') `
        -Message "a lookup that runs out of time is unknown, not an answer ($(Format-ProvenanceForFailure -Result $result))"
    # The control, and it is the cell that says the one above measured the BOUND and not the fixture:
    # the same repository, with the bound restored, certifies.
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($result.pushed -eq $true) `
        -Message "CONTROL: the same repository under the normal bound reads the server and certifies ($(Format-ProvenanceForFailure -Result $result))"

    Write-Host ''
    Write-Host '-- a bound the environment cannot turn back into no bound --' -ForegroundColor Cyan
    # The variable was moved INSIDE the function so the extracted cells could not leave it $null.
    # The operator could still hand it one: `[int]'abc'` under `Continue` writes an error and leaves
    # the assignment undone, so a malformed override restored the unbounded wait the move was made
    # to prevent -- `Wait-Job -Timeout $null` does not fail, it waits. Refused by name now.
    #
    # `0` is NOT refused and this cell says so, because the suite above uses it to reach the timeout
    # branch without paying a real bound. A refusal that broke that seam is what the first version of
    # this fix did, and its own suite caught it.
    $previousTimeout = $env:GATE_LS_REMOTE_TIMEOUT_SECONDS
    $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = 'abc'
    try { $malformed = Invoke-ProvenanceRaw -Repo $repo } finally { $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = $previousTimeout }
    Assert-True -Condition ($malformed.text -cmatch 'GATE_LS_REMOTE_TIMEOUT_SECONDS is \[abc\]') `
        -Message "a non-numeric bound is refused BY NAME rather than falling through to no bound (got: $($malformed.text))"
    $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = '-5'
    try { $negative = Invoke-ProvenanceRaw -Repo $repo } finally { $env:GATE_LS_REMOTE_TIMEOUT_SECONDS = $previousTimeout }
    Assert-True -Condition ($negative.text -cmatch 'GATE_LS_REMOTE_TIMEOUT_SECONDS is \[-5\]') `
        -Message "and so is a negative one, which is not a bound at all (got: $($negative.text))"

    Write-Host ''
    Write-Host '-- there is exactly one way to reach pushed = true, and it goes through the server --' -ForegroundColor Cyan
    # The sweep, because the fix is a rule about the whole function and the cells above reach the
    # paths a fixture can build. If a second assignment appears, it will be a second definition of
    # what `pushed` means.
    $trueAssignments = @(($subject -split "`n") | Where-Object { $_ -cmatch '^\s*\$pushed = \$true\s*$' })
    Assert-True -Condition ($trueAssignments.Count -eq 1) `
        -Message "pushed is set true in exactly one place (found $($trueAssignments.Count))"
    Assert-True -Condition ($subject -cmatch '(?s)ls-remote[\s\S]{0,2000}?\[string\]::Equals\(\$tip, \$HeadSha[\s\S]{0,200}?\$pushed = \$true') `
        -Message 'and that place is the branch comparing the server tip with this head'
    Assert-True -Condition ($subject -cnotmatch '(?s)for-each-ref[\s\S]{0,400}?\$pushed = \$true') `
        -Message 'while the local ref search no longer leads to a true'

    # ---- THE COMPARISONS THAT DECIDE ARE ORDINAL, BECAUSE -ceq IS NOT.
    Write-Host ''
    Write-Host '-- a copy differing by a weightless code point is not the same copy --' -ForegroundColor Cyan
    # PowerShell's case-sensitive operators are still CULTURE aware, and a culture comparison gives
    # some code points no weight: 'GREEN' plus U+FE00 is -ceq 'GREEN'. This file had that operator in
    # ten places, one of them the compare-and-swap that guards publication and one of them the
    # reconciliation below -- so a manifest on disk that differs from what the run published by a
    # variation selector was read as identical and left there.
    #
    # The character is not the point and refusing it would be a deny-list: U+FE00 is Mn, ordinary
    # text. What changed is that the COMPARISON stopped being approximate.
    $repo = New-Repo -Name 'ordinalcopy'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    $honest = (@{ headSha = $gated; status = 'GREEN' } | ConvertTo-Json -Compress)
    # The same record plus one weightless code point. Written to the file, never to this source.
    [System.IO.File]::WriteAllText($manifest, ($honest + [char]0xFE00), $utf8NoBom)
    $cultureSaysSame = (($honest + [char]0xFE00) -ceq $honest)
    Assert-True -Condition ($cultureSaysSame) `
        -Message 'ARRANGEMENT: the culture-aware operator really does call these two strings equal'
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7 -Content $honest
    $onDisk = [System.IO.File]::ReadAllText($manifest)
    Assert-True -Condition ([string]::Equals($onDisk, $honest, [System.StringComparison]::Ordinal)) `
        -Message "the copy is reconciled with what was published rather than passed over as identical (length $($onDisk.Length) vs $($honest.Length))"

    Write-Host ''
    Write-Host '-- and no comparison in the gate is left approximate --' -ForegroundColor Cyan
    # The sweep, because the cell above reaches one of the ten. `-ceq` and `-cne` look case-strict
    # and are culture aware; `-ccontains` and `-cnotcontains` are the same comparer over a list.
    $gateLines = @($gateText -split "`n")
    $inDocBlock = $false
    $cultureOps = @(foreach ($line in $gateLines) {
            $trimmed = $line.Trim()
            if ($trimmed -cmatch '<#') { $inDocBlock = $true }
            if ($trimmed -cmatch '#>') { $inDocBlock = $false; continue }
            if ($inDocBlock -or $trimmed.StartsWith('#')) { continue }
            if ($line -cmatch '-ceq |-cne |-ccontains |-cnotcontains ') { $trimmed }
        })
    Assert-True -Condition ($cultureOps.Count -eq 0) `
        -Message ('no comparison that decides uses the culture-aware operators' +
            $(if ($cultureOps.Count -gt 0) { ': ' + (($cultureOps | Select-Object -First 3) -join ' | ') } else { '' }))
    # Not vacuous, and not fooled by the comments that discuss the operator by name.
    $canaryFound = @(foreach ($line in @('    if ($a -cne $b) {', '    # $a -ceq $b in a comment')) {
            $trimmed = $line.Trim()
            if ($trimmed.StartsWith('#')) { continue }
            if ($line -cmatch '-ceq |-cne |-ccontains |-cnotcontains ') { $trimmed }
        })
    Assert-True -Condition ($canaryFound.Count -eq 1) `
        -Message 'and the sweep finds a culture-aware comparison when one is put in front of it, while ignoring a comment'

    # ---- What links a manifest to the evidence that its run FINISHED.
    Write-Host ''
    Write-Host '-- the two comparisons that DECIDE are fed an input that tells the comparers apart --' -ForegroundColor Cyan
    # A reviewer swapped all eleven ordinal comparisons in ci/gate.ps1 to InvariantCulture and ran
    # this suite: exactly ONE cell reddened. That does not mean ten are untested -- it means no cell
    # hands them an input that DISTINGUISHES the two comparers. The gap is in inputs, not in lines,
    # and these two are where it costs: the comparison that turns an `ls-remote` answer into
    # `pushed = TRUE` (this file calls that PERMISSION TO MERGE), and the one that decides what the
    # durable record says about a run whose head moved.
    #
    # The payload is a code point the culture comparer FOLDS. Measured rather than assumed, because
    # the obvious guess is wrong: U+00AD, U+200D, U+2060, U+FE00, U+FEFF and U+FFFD fold; U+200B does
    # NOT. The class is "ignorable to the comparer", never "invisible" -- and the difference decides
    # what the next reader builds.
    $ignorable = [string][char]0xFE00
    Assert-True -Condition ((('GREEN' + $ignorable) -ceq 'GREEN') -and
        -not [string]::Equals(('GREEN' + $ignorable), 'GREEN', [System.StringComparison]::Ordinal) -and
        -not (('GREEN' + [string][char]0x200B) -ceq 'GREEN')) `
        -Message 'ARRANGEMENT: U+FE00 is folded by the culture comparer and not by the ordinal one, and U+200B by neither'

    # THE PERMISSION-TO-MERGE COMPARISON. The server answers with the real tip; the head handed in
    # differs from it by one ignorable code point. A culture-aware comparison calls those equal and
    # writes `pushed: true` -- a certificate about a commit the server never named.
    $bare = Join-Path $fixtureRoot 'bare-ignorable.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'ignorablehead'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/fixture --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/fixture 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
    } finally { Pop-Location }
    Assert-UpstreamArranged -Repo $repo -What 'the permission-to-merge comparison'
    $trueHead = (& git -C $repo rev-parse HEAD).Trim()
    $result = Invoke-Provenance -Repo $repo -HeadShaOverride ($trueHead + $ignorable)
    Assert-True -Condition ($result.pushed -ne $true) `
        -Message "a head differing from the server tip by an ignorable code point is NOT certified as pushed ($(Format-ProvenanceForFailure -Result $result))"
    # The control says the cell measured the COMPARISON and not the fixture: same repository, same
    # server, real head -- certified.
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($result.pushed -eq $true) `
        -Message "CONTROL: the same repository with the real head is certified ($(Format-ProvenanceForFailure -Result $result))"

    # THE RECORDED STATUS. `Write-RunManifest` drives the whole gate and cannot be run from a cell,
    # so the rule was extracted into a function this cell dot-sources out of the subject by AST --
    # not retyped, because a copy passes while the original rots.
    $statusFn = ([System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$null)).Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -ceq 'Get-RecordedStatus'
        }, $true)
    Assert-True -Condition ($null -ne $statusFn) `
        -Message 'ARRANGEMENT: the recorded-status rule is a function this cell can call'
    . ([scriptblock]::Create($statusFn.Extent.Text))
    Assert-True -Condition ((Get-RecordedStatus -Status 'GREEN' -HeadMoved $true) -ceq 'RED') `
        -Message 'a GREEN whose head moved is recorded as RED'
    Assert-True -Condition ((Get-RecordedStatus -Status ('GREEN' + $ignorable) -HeadMoved $true) -cne 'RED') `
        -Message 'and a status that is GREEN plus an ignorable code point is not treated as GREEN, so the record keeps a value somebody has to explain'
    Assert-True -Condition ((Get-RecordedStatus -Status 'GREEN' -HeadMoved $false) -ceq 'GREEN') `
        -Message 'CONTROL: a GREEN whose head did not move is left alone'

    # AND THE REST IS DECLARED RATHER THAN LEFT LOOKING COVERED. The other nine ordinal comparisons
    # in ci/gate.ps1 -- the pull-request head match, the gate's own path, the signing flag, the
    # compare-and-swap re-read, the manifest content check, the two branch checks, the head-moved
    # comparison and the startup snapshot -- are ordinal BY READING. No cell hands any of them an
    # input that would tell the two comparers apart, and the sweep further down asserts only that
    # they are not written with the culture operators. Saying so is the difference between a gap and
    # a gap that reads as coverage.
    $ordinalUses = ([regex]::Matches($gateText, 'StringComparison\]::Ordinal')).Count
    Assert-True -Condition ($ordinalUses -ge 11) `
        -Message "the file still holds the ordinal comparisons this declaration is about (found $ordinalUses)"

    Write-Host ''
    Write-Host '-- the name is an index and the identity is inside the file --' -ForegroundColor Cyan
    # A manifest is named `<first 12 of the head>-<timestamp>.json`. That prefix is a search key, not
    # an identity: twelve characters of a sha, in a name. This cell holds the sentence that says so,
    # because the arrangement invites the mistake -- the glob is the obvious search, it returns
    # something, and the something looks right. (Reported by L, who made exactly that mistake on
    # #639 and reported the wrong run.)
    Assert-True -Condition ($gateText -cmatch 'THE NAME IS AN INDEX; THE IDENTITY IS') `
        -Message 'the gate says in the file that the name is an index and headSha is the identity'
    Assert-True -Condition ($gateText -cmatch 'matched, headSha differs') `
        -Message 'and that a glob that matches nothing and a glob that matches the wrong file are different facts'

    Write-Host ''
    Write-Host '-- RUN-END names the manifest, which is the only witness that the run ended --' -ForegroundColor Cyan
    # `runEndUtc` is stamped when the record is SERIALIZED, and everything that can still turn the
    # run RED happens after that: the pair is written, the compare-and-swap can refuse, the
    # correction can fail. So the field is not proof of completion, and a consumer that treats it as
    # proof under #674(b) would be trusting a timestamp written before the run could still fail.
    #
    # The proof is in the ledger: RUN-END carries `manifest=<basename>`, so a manifest with no
    # RUN-END naming it belongs to a RUN-START with no end -- a dead run under #199. The link is
    # one-directional by construction (the name is chosen after the object is built, so the record
    # cannot name itself), it is currently asserted only by a comment, and a comment is a claim.
    $runEndDetail = 'endDetail = "status=$endStatus class=$endClass head=$($headSha.Substring(0, 12)) manifest=$fileName"'
    Assert-True -Condition ($gateText.Contains($runEndDetail)) `
        -Message 'RUN-END carries the manifest basename, so a run can be joined to its record'
    $serializedBeforePublication = $gateText.IndexOf('runEndUtc          = [DateTime]::UtcNow')
    $pairWritten = $gateText.IndexOf('$written = Write-GateManifestPair')
    Assert-True -Condition ($serializedBeforePublication -gt 0 -and $pairWritten -gt $serializedBeforePublication) `
        -Message 'and runEndUtc is stamped BEFORE the writes it is read as having outlived'

    # ---- The reasons reach the manifest, not just the function.
    Write-Host ''
    Write-Host '-- unknown and absent stay distinguishable in the RECORD --' -ForegroundColor Cyan
    Assert-True -Condition ($gateText -cmatch 'pullRequestReason\s*=\s*\$provenance\.pullRequestReason') `
        -Message 'the manifest carries the pull-request reason, so a null number is explained'
    Assert-True -Condition ($gateText -cmatch 'pushedReason\s*=\s*\$provenance\.pushedReason') `
        -Message 'and the pushed reason, so a false is not confused with a lookup that failed'

    # ---- A tracked branch is not the only route to the server.
    Write-Host ''
    Write-Host '-- pushed by an alternate ref while the upstream stays behind --' -ForegroundColor Cyan
    $bare = Join-Path $fixtureRoot 'bare-three.git'
    & git init --bare --quiet $bare 2>&1 | Out-Null
    $repo = New-Repo -Name 'altref'
    Push-Location $repo
    try {
        & git remote add origin $bare 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/feature --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/feature 2>&1 | Out-Null
        # A new commit, pushed under a DIFFERENT ref. The upstream stays where it was.
        [System.IO.File]::WriteAllText((Join-Path $repo 'a.txt'), "two`n", $utf8NoBom)
        & git commit -am 'second' --quiet 2>&1 | Out-Null
        & git push origin HEAD:refs/heads/review --quiet 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        $upstream = (& git rev-parse '@{upstream}' 2>$null | Select-Object -First 1)
        $head = (& git rev-parse HEAD).Trim()
    } finally { Pop-Location }
    Assert-True -Condition (([string]$upstream).Trim() -cne $head) `
        -Message 'ARRANGEMENT: the upstream really is behind, so equality alone would say unpushed'
    $result = Invoke-Provenance -Repo $repo
    # Same correction as the cell above: the alternate ref is a LOCAL ref here. The server is asked
    # about the branch this run tracked, that branch's tip is the older commit, and "not the tip" is
    # not "not on the server" -- so this is null with a reason that says which tip was read.
    Assert-True -Condition ($null -eq $result.pushed) `
        -Message 'the commit reads as UNKNOWN: the tracked branch tip is not it'
    Assert-True -Condition ($result.pushedReason -cmatch 'not thereby absent from the server') `
        -Message 'and the reason names the tip that was read and refuses to conclude absence from it'

    # ---- "gh cannot answer" and "gh says none" are different states.
    Write-Host ''
    Write-Host '-- the tool being unusable is not the answer "no pull request" --' -ForegroundColor Cyan
    Assert-True -Condition ($gateText -cmatch 'gh auth status') `
        -Message 'the tool is asked whether it can answer at all before its silence is read as none'
    Assert-True -Condition (($gateText -cmatch 'nobody could look') -and ($gateText -cmatch 'has no open pull request')) `
        -Message 'and the two states carry different reasons, which is the whole point of recording one'

    Write-Host ''
    Write-Host '-- a change already staged INSIDE the store is not swept in --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'prestaged'
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    # A file the author staged, inside the store: the foreign-path filter is RIGHT to allow it --
    # it is not outside -- so only the commit's own pathspec can keep it out of this commit.
    [System.IO.File]::WriteAllText((Join-Path $store 'someone-elses.json'), '{"mine":false}', $utf8NoBom)
    Push-Location $repo
    try { & git add -- '.factory/gate-runs/someone-elses.json' 2>&1 | Out-Null } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7

    $touched = @(& git -C $repo diff-tree --no-commit-id --name-only -r HEAD)
    Assert-True -Condition ($touched -ccontains '.factory/gate-runs/run.json') `
        -Message 'the manifest this run wrote IS committed'
    Assert-True -Condition (-not ($touched -ccontains '.factory/gate-runs/someone-elses.json')) `
        -Message "and the file someone else staged is NOT, though the filter rightly allowed it"

    Write-Host ''
    Write-Host '-- a commit that fails leaves the index as it was found --' -ForegroundColor Cyan
    $repo = New-Repo -Name 'failcommit'
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    Push-Location $repo
    try {
        # Signing demanded with no usable signer: `git commit` refuses and `--no-verify` does not
        # bypass it. This is the case the reviewer named -- a machine with signing configured and
        # no key -- and it is the only refusal shape that survives whatever the developer's global
        # config happens to say, which the identity route did not (git found a global identity and
        # committed anyway, so the cell was green for the wrong reason until this was measured).
        & git config commit.gpgsign true 2>&1 | Out-Null
        & git config gpg.program ([System.IO.Path]::Combine($repo, 'no-such-gpg.exe')) 2>&1 | Out-Null
    } finally { Pop-Location }
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7

    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
        -Message 'the head did not move: the commit really did fail'
    $staged = @(& git -C $repo diff --cached --name-only)
    Assert-True -Condition (-not ($staged -ccontains '.factory/gate-runs/run.json')) `
        -Message "and the manifest is NOT left staged, so the author's next commit does not carry it"

    # ---- The pull request has to name the head that was gated.
    Write-Host ''
    Write-Host '-- a branch name is not an identifier; the head is --' -ForegroundColor Cyan
    # `gh pr list --head` filters by BRANCH NAME ONLY (gh's own help says the owner:branch form is
    # unsupported), so two forks using `fix-thing`, or a branch renamed since its pull request was
    # opened, both answer with somebody else's pull request. Taking `.[0]` recorded that number as
    # this run's provenance.
    $repo = New-Repo -Name 'prpick'
    $head = (& git -C $repo rev-parse HEAD).Trim()

    $stub = New-GhStub -Name 'twobranches' -Json ('[{"number":11,"headRefOid":"' + ('a' * 40) + '"},{"number":22,"headRefOid":"' + $head + '"}]')
    $r = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($r.pullRequest -eq 22) `
        -Message "the pull request naming THIS head is the one recorded, not the first in the list (got $($r.pullRequest))"

    # Same list, the other order: a cell that only ever sees the answer in position two would pass
    # on `.[0]` for the wrong reason the moment the list came back sorted differently.
    $stub = New-GhStub -Name 'twobranchesrev' -Json ('[{"number":22,"headRefOid":"' + $head + '"},{"number":11,"headRefOid":"' + ('a' * 40) + '"}]')
    $r = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($r.pullRequest -eq 22) `
        -Message "and the order of the list does not decide it (got $($r.pullRequest))"

    $stub = New-GhStub -Name 'nomatch' -Json ('[{"number":11,"headRefOid":"' + ('b' * 40) + '"}]')
    $r = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $r.pullRequest -and $r.pullRequestReason -cmatch 'none of them names') `
        -Message 'a pull request on the same branch name that does NOT name this head is refused, with the reason'

    $stub = New-GhStub -Name 'ambiguous' -Json ('[{"number":11,"headRefOid":"' + $head + '"},{"number":22,"headRefOid":"' + $head + '"}]')
    $r = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $r.pullRequest -and $r.pullRequestReason -cmatch 'cannot be decided') `
        -Message 'two pull requests naming the same head is a refusal, not a choice between strangers'

    $stub = New-GhStub -Name 'empty' -Json '[]'
    $r = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $r.pullRequest -and $r.pullRequestReason -cmatch 'no open pull request') `
        -Message 'an empty answer still means "this branch has no open pull request", not a failure'

    # ---- The head can move between the capture and the commit.
    Write-Host ''
    Write-Host '-- a manifest is never committed onto a head the run did not judge --' -ForegroundColor Cyan
    # `$HeadSha` is captured before the run and a network-backed lookup happens in between, so
    # another shell has a wide window to advance the branch. The tree can be perfectly clean on the
    # new head, so nothing else here would notice.
    $repo = New-Repo -Name 'headmoved'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'other.txt'), "someone else`n", $utf8NoBom)
        & git add -- 'other.txt' 2>&1 | Out-Null
        & git commit --quiet -m 'another shell moved the branch' 2>&1 | Out-Null
    } finally { Pop-Location }
    $moved = (& git -C $repo rev-parse HEAD).Trim()

    $manifestDir = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($manifestDir) | Out-Null
    $manifestPath = Join-Path $manifestDir 'moved.json'
    [System.IO.File]::WriteAllText($manifestPath, '{"status":"GREEN"}', $utf8NoBom)

    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifestPath -HeadSha $gated -PullRequest 42
    # The refusal moved from a check in front of the commit to git's own compare-and-swap, so the
    # words changed with it: `update-ref <new> <old>` is what now says no, and the message names the
    # ref and the head this run judged. Same refusal, decided one layer down where there is no
    # instant between looking and acting.
    Assert-True -Condition ($out -cmatch 'no longer points at') `
        -Message 'the publisher refuses, and names the head it was judging'
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $moved) `
        -Message 'no commit is created on the head this run never judged'
    $tracked = @(& git -C $repo ls-files -- '.factory/gate-runs/moved.json')
    Assert-True -Condition ($tracked.Count -eq 0) `
        -Message 'and the manifest is left written but untracked, for somebody to look at'
    $staged = @(& git -C $repo diff --cached --name-only)
    Assert-True -Condition ($staged.Count -eq 0) `
        -Message "and the index is left as it was found, not carrying the gate's file into the author's next commit"

    # ---- gh being ABSENT is an exception, not an exit code.
    Write-Host ''
    Write-Host '-- gh missing from PATH is a reason, not a crash --' -ForegroundColor Cyan
    # Measured before writing this: with the command unresolvable, `&` does not launch a process and
    # sets no exit code -- it throws CommandNotFoundException, `2>$null` does not silence it (the
    # command never ran, so that text is not its stderr), and the `try` here has only a `finally`.
    # The line that reads $LASTEXITCODE is never REACHED, so a cell written against a stale exit
    # code would observe an exception instead and pass for the wrong reason. This is written against
    # the escape: the function has to come back with a reason.
    $repo = New-Repo -Name 'noghatall'
    # git's own directory and nothing else: git has to stay reachable (the function asks it three
    # questions before it ever mentions gh), and gh has to be gone. A blank PATH would have removed
    # both and the cell would redden on the wrong tool.
    # git's directory and PowerShell's own, and nothing else. git has to stay reachable (the
    # function asks it three questions before it ever mentions gh) and so does powershell.exe --
    # the runner is launched with it, and the first attempt at this cell died on THAT rather than on
    # gh, which is the failure this arrangement check now makes impossible to mistake.
    $gitDir = Split-Path -Parent (Get-Command git).Source
    $restricted = "$gitDir;$PSHOME"
    $ghStillThere = @(@($gitDir, $PSHOME) | ForEach-Object {
            Get-ChildItem -LiteralPath $_ -Filter 'gh.*' -File -ErrorAction SilentlyContinue })
    Assert-True -Condition ($ghStillThere.Count -eq 0) `
        -Message 'ARRANGEMENT: gh is in neither directory, so restricting PATH to them really does remove gh'
    $result = Invoke-Provenance -Repo $repo -PathOverride $restricted
    # WHAT THIS CELL CAN AND CANNOT SEE. It calls the extracted function from a runner's top level,
    # with no enclosing try -- so it observes that the function RETURNS. In the gate the call sits
    # inside `Write-RunManifest`, which the main script wraps in a try/catch, and there the same
    # error PROPAGATES and the run goes RED for "run-manifest write". Measured both ways after two
    # readings disagreed; the message says what this structure shows and claims nothing about the
    # other one.
    Assert-True -Condition ($null -ne $result) `
        -Message 'the provenance function returns instead of dying on an absent gh'
    Assert-True -Condition ($null -eq $result.pullRequest -and $result.pullRequestReason -cmatch 'not installed') `
        -Message 'and it is recorded as "nobody could look", which is what the docstring promises'

    # ---- The candidate set is not pinned to the branch name.
    Write-Host ''
    Write-Host '-- a renamed branch does not lose its pull request --' -ForegroundColor Cyan
    # `--head <name>` chooses who is in the room; `headRefOid` only decides among them. A branch
    # renamed after its pull request was opened returns an EMPTY list, and empty read as "this
    # branch has no open pull request" -- the same fail-open, one layer further out.
    $repo = New-Repo -Name 'renamed'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $stub = New-GhStub -Name 'renamed' -Json '[]' -SearchJson ('[{"number":77,"headRefOid":"' + $head + '"}]')
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($result.pullRequest -eq 77) `
        -Message "the pull request is found by HEAD when the branch name finds nothing (got $($result.pullRequest))"

    # And the search is not allowed to invent one: an answer that names another head is still
    # filtered out by `headRefOid`, which is the guard the wider query must not weaken.
    $stub = New-GhStub -Name 'searchstranger' -Json '[]' -SearchJson ('[{"number":78,"headRefOid":"' + ('c' * 40) + '"}]')
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $result.pullRequest -and $result.pullRequestReason -cmatch 'none of them names') `
        -Message 'a wider search still cannot smuggle in a pull request that names another head'

    # ---- A failed query is not an answer.
    Write-Host ''
    Write-Host '-- an empty branch query beside a FAILED search is unknown --' -ForegroundColor Cyan
    # The search is the only query that can find a pull request whose branch was renamed. An empty
    # `--head` answer beside a failed search therefore establishes nothing, and recording "this
    # branch has no open pull request" would be the fail-open this whole thread chain is about.
    $repo = New-Repo -Name 'searchfailed'
    $stub = New-GhStub -Name 'searchfailed' -Json '[]' -Fail 'search'
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $result.pullRequest -and $result.pullRequestReason -cmatch 'nobody could look') `
        -Message 'the reason is "nobody could look", not "this branch has no open pull request"'

    # And a query that fails while the OTHER one answers is still an answer: refusing here would
    # throw away a pull request that was found, which is the opposite error.
    $repo = New-Repo -Name 'searchfailedbutfound'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $stub = New-GhStub -Name 'searchfailedbutfound' -Json ('[{"number":55,"headRefOid":"' + $head + '"}]') -Fail 'search'
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($result.pullRequest -eq 55) `
        -Message "a match that survived the failure is still recorded (got $($result.pullRequest))"

    # ---- The publication is one operation, and it never touches the author's index.
    Write-Host ''
    Write-Host '-- the manifest commit is built on the gated head, beside the author work --' -ForegroundColor Cyan
    # The old form staged into the REAL index and had to undo that on every failure path. Building
    # the commit from a temporary index means the author's staged work is never involved at all --
    # so this asserts what is now structurally true rather than what a `git reset` remembered to
    # clean up.
    $repo = New-Repo -Name 'authorindex'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    # INSIDE the store, deliberately. A change staged OUTSIDE it makes the foreign-path filter
    # refuse to publish at all -- correctly, and that is a different cell. The interesting case is
    # the one the filter allows through: a file already staged under the store, which an
    # unqualified commit would sweep into the gate's message. That is what `--only` guarded and
    # what a temporary index now makes structurally impossible.
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $store 'author.json'), "{}`n", $utf8NoBom)
        & git add -- '.factory/gate-runs/author.json' 2>&1 | Out-Null
    } finally { Pop-Location }

    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    $newHead = (& git -C $repo rev-parse HEAD).Trim()
    # THE PUBLISHER'S WORDS, HERE TOO. This assertion has failed three times in runs that did not
    # reproduce, and each time the log carried only `HEAD~1 unknown` because this cell threw away
    # the one output that names which step gave way.
    Assert-True -Condition ($newHead -cne $gated) `
        -Message "the manifest was published$(if ($newHead -ceq $gated) { " -- the publisher said: $out" })"
    Assert-True -Condition ((& git -C $repo rev-parse 'HEAD^').Trim() -ceq $gated) `
        -Message 'and its parent is EXACTLY the head the run judged'
    $committedFiles = @(& git -C $repo diff-tree --no-commit-id --name-only -r HEAD)
    Assert-True -Condition ($committedFiles.Count -eq 1 -and $committedFiles[0] -ceq '.factory/gate-runs/run.json') `
        -Message "the commit holds the manifest and nothing else ($($committedFiles -join ', '))"
    $stagedAfter = @(& git -C $repo diff --cached --name-only)
    Assert-True -Condition ($stagedAfter.Count -eq 1 -and $stagedAfter[0] -ceq '.factory/gate-runs/author.json') `
        -Message "the author's staged file is still staged, and only that ($($stagedAfter -join ', '))"

    # ---- Movement reaches the VERDICT, not only the manifest.
    Write-Host ''
    Write-Host '-- HEAD moving during the run is a gate failure --' -ForegroundColor Cyan
    # Running the whole gate here is not possible -- it compiles the workspace -- so this is
    # asserted over the SOURCE, and said plainly rather than dressed up: it proves the detection is
    # WIRED to the verdict, not that a real run fails. The three facts a wiring defect would break
    # are each named separately, so a partial rewire cannot pass.
    $gateText = [System.IO.File]::ReadAllText($gatePath)
    Assert-True -Condition ($gateText -cmatch '\$script:headMovedDuringRun = \$true') `
        -Message 'the movement is recorded in a script-scope flag, not only in the manifest body'
    Assert-True -Condition ($gateText -cmatch 'if \(\$script:headMovedDuringRun\) \{') `
        -Message 'and the final verdict reads that flag'
    # Single-quoted, so PowerShell does not consume the escapes before the regex engine sees them:
    # the double-quoted form turned `\$` into a backslash plus an interpolation and the pattern
    # failed to parse at all -- a cell that cannot run is not a cell that passes.
    $wired = '(?s)if \(\$script:headMovedDuringRun\) \{.{0,600}?\$failed \+='
    Assert-True -Condition ($gateText -cmatch $wired) `
        -Message 'and what it does with it is add a failure, so the gate cannot exit 0'

    # ---- The gate's own nonce is not the author's work.
    Write-Host ''
    Write-Host '-- the refusal does not fire on the file the gate itself rewrites --' -ForegroundColor Cyan
    # `Write-CanaryNonce` rewrites tools/ci-canary/src/nonce.rs before every run, and that file is
    # TRACKED -- so `git status` reports it on every normal invocation and the foreign-change
    # refusal fired every time. A guard that refuses on its own artefact does not protect anything;
    # it just never lets the thing it guards happen, which is #674(a) committing no manifest on any
    # ordinary run at all.
    $repo = New-Repo -Name 'noncedirty'
    $nonceDir = Join-Path $repo 'tools/ci-canary/src'
    [System.IO.Directory]::CreateDirectory($nonceDir) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $nonceDir 'nonce.rs'), "pub const RUN_NONCE: &str = `"a`";`n", $utf8NoBom)
    Push-Location $repo
    try {
        & git add -- 'tools/ci-canary/src/nonce.rs' 2>&1 | Out-Null
        & git commit --quiet -m 'the nonce, tracked like it is in the real repository' 2>&1 | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $nonceDir 'nonce.rs'), "pub const RUN_NONCE: &str = `"b`";`n", $utf8NoBom)
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -cne $gated) `
        -Message 'a run with only the gate nonce modified still publishes its manifest'

    # The control, because an exception is only safe if it is the ONLY exception: any other tracked
    # file the author is holding still stops the publication.
    $repo = New-Repo -Name 'otherdirty'
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'work.txt'), "the author is mid-edit`n", $utf8NoBom)
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ($out -cmatch 'changes outside' -and (& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
        -Message "CONTROL: any other change outside the store still refuses, so the exception is one path and not a hole"

    # ---- The journey a real invocation takes.
    Write-Host ''
    Write-Host '-- the gate writes its nonce and then publishes, in one run --' -ForegroundColor Cyan
    # The reviewer's point, and it is the one that matters: the cell above writes a dirty nonce BY
    # HAND, which reproduces the condition but not the path. Here the gate's own Write-CanaryNonce
    # runs first, verbatim out of ci/gate.ps1, and the publisher follows it in the same process --
    # so a future edit that changes WHERE the nonce is written, or adds a second file the gate
    # rewrites, reddens this instead of passing while production breaks.
    $repo = New-Repo -Name 'journey'
    $nonceDir = Join-Path $repo 'tools/ci-canary/src'
    [System.IO.Directory]::CreateDirectory($nonceDir) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $nonceDir 'nonce.rs'), "pub const RUN_NONCE: &str = `"seed`";`n", $utf8NoBom)
    Push-Location $repo
    try {
        & git add -- 'tools/ci-canary/src/nonce.rs' 2>&1 | Out-Null
        & git commit --quiet -m 'the nonce, tracked as it is in the real repository' 2>&1 | Out-Null
    } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)

    $out = Invoke-NonceThenPublish -Repo $repo -ManifestPath $manifest -HeadSha $gated
    $journeyHead = (& git -C $repo rev-parse HEAD).Trim()
    $dirtyAfter = @(& git -C $repo status --porcelain -- 'tools/ci-canary/src/nonce.rs')
    Assert-True -Condition ($dirtyAfter.Count -eq 1) `
        -Message 'ARRANGEMENT: the gate really did rewrite the nonce, so this cell is on the path it claims'
    Assert-True -Condition ($journeyHead -cne $gated) `
        -Message "and the manifest is committed anyway: the run publishes on the path a real invocation takes$(if ($journeyHead -ceq $gated) { " -- the publisher said: $out" })"
    $committed = @(& git -C $repo diff-tree --no-commit-id --name-only -r HEAD)
    Assert-True -Condition ($committed.Count -eq 1 -and $committed[0] -ceq '.factory/gate-runs/run.json') `
        -Message "and the nonce is NOT swept into that commit ($($committed -join ', '))"

    # ---- The refusal reaches the verdict.
    Write-Host ''
    Write-Host '-- a refused publication is a RED run, not a log line --' -ForegroundColor Cyan
    # The compare-and-swap failing means the branch moved during the run. That was reaching nobody:
    # the publisher returned, the caller still got a path, the flag stayed false, and the gate
    # printed GREEN and exited 0 for a head the stages never tested. Detecting a race and not acting
    # on it is worse than not detecting it, because the detection reads as coverage.
    $repo = New-Repo -Name 'casverdict'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    Push-Location $repo
    try {
        [System.IO.File]::WriteAllText((Join-Path $repo 'work.txt'), "someone else`n", $utf8NoBom)
        & git add -- 'work.txt' 2>&1 | Out-Null
        & git commit --quiet -m 'another shell moved the branch' 2>&1 | Out-Null
    } finally { Pop-Location }
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ($out -cmatch 'HEADMOVEDFLAG=True') `
        -Message 'the refusal sets the flag the final verdict reads'
    Assert-True -Condition ($out -cmatch 'This run is RED') `
        -Message 'and it says RED rather than filing a warning nobody acts on'

    # The control: a publication that SUCCEEDS must not set it, or every run would be red.
    $repo = New-Repo -Name 'casverdictok'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ($out -cmatch 'HEADMOVEDFLAG=False') `
        -Message 'CONTROL: a publication that succeeds leaves the verdict alone'

    # ---- The RECORD does not say GREEN about a run whose head moved.
    Write-Host ''
    Write-Host '-- the durable record is corrected before it is written --' -ForegroundColor Cyan
    # Asserted over the SOURCE, and said plainly: Write-RunManifest cannot be run here, it drives the
    # whole gate. The exit code is read once by whoever ran the gate; the manifest is read by
    # everything afterwards, so it is the copy that must not lie -- and it was being serialized
    # `status: GREEN, overallPassed: true` for a run the same script was about to call RED.
    # Spelled without the operator: this cell is about the CORRECTION happening, not about
    # which comparer spells it, and quoting the comparer made an ordinal fix look like a
    # regression.
    # The rule moved into `Get-RecordedStatus` so a cell could feed it; this assertion is about
    # the ORDER, which is unchanged: the status is corrected before the record that carries it
    # is built. It asks for the call, not for the comparison, because the comparison now lives
    # in the function two cells above exercise directly.
    $statusCorrected = '\$Status = Get-RecordedStatus -Status \$Status -HeadMoved \$headMoved'
    Assert-True -Condition ($gateText -cmatch $statusCorrected) `
        -Message 'the status is corrected before the record is built, not after it is written'
    Assert-True -Condition ($gateText -cmatch 'overallPassed\s+= \(\$passedEverything -and -not \$headMoved\)') `
        -Message 'and overallPassed carries the same fact, so a consumer counting successes cannot count this one'

    # ---- git decides what a boolean is.
    Write-Host ''
    Write-Host '-- every spelling git calls true is treated as true --' -ForegroundColor Cyan
    # The check listed the spellings I remembered. git accepts more of them, case-insensitively, so a
    # repository configured `commit.gpgSign = On` had its signing policy silently dropped by a gate
    # that believed it was honouring it. The cell drives the case my regex missed, with a signer that
    # cannot work: if the policy is read, the commit fails and nothing is published.
    foreach ($spelling in @('on', 'yes', 'True')) {
        $repo = New-Repo -Name "gpgspelling-$spelling"
        Push-Location $repo
        try {
            & git config commit.gpgsign $spelling 2>&1 | Out-Null
            & git config gpg.program ([System.IO.Path]::Combine($repo, 'no-such-gpg.exe')) 2>&1 | Out-Null
        } finally { Pop-Location }
        $gated = (& git -C $repo rev-parse HEAD).Trim()
        $store = Join-Path $repo '.factory/gate-runs'
        [System.IO.Directory]::CreateDirectory($store) | Out-Null
        $manifest = Join-Path $store 'run.json'
        [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
        $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
        Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
            -Message "commit.gpgSign = '$spelling' is honoured, so a broken signer stops the publication"
    }

    # ---- half a broken instrument is still a broken instrument.
    Write-Host ''
    Write-Host '-- a lookup document that does not parse is a failed query --' -ForegroundColor Cyan
    # Two queries run, and truncated JSON from one was absorbed as long as the other answered: the
    # pool looked complete and a verdict got recorded from half the evidence. The house rule is the
    # same one this file applies everywhere else -- a failed instrument is UNKNOWN, never an answer.
    $repo = New-Repo -Name 'truncatedjson'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $stub = New-GhStub -Name 'truncatedjson' -Json '[]' -SearchJson '[{"number":77,"headRefOid":'
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($null -eq $result.pullRequest -and $result.pullRequestReason -cmatch 'did not parse') `
        -Message 'a truncated answer beside a clean empty one is unknown, not "no open pull request"'

    # ---- Identity is re-verified before each act that depends on it.
    Write-Host ''
    Write-Host '-- the publication needs no read; the index touch needs one --' -ForegroundColor Cyan
    # THIS CELL REPLACES THE ONE I WROTE LAST ROUND, and the replacement is the point. That cell
    # asserted ZERO identity reads in the publisher and it would have passed with the defect below
    # alive -- a green cell certifying a rule that was stated too widely. The rule that survives is
    # narrower: the PUBLICATION needs no read, because `update-ref <new> <expected>` compares and
    # writes atomically; the INDEX TOUCH does, because the index is per WORKTREE and no plumbing
    # makes that write atomic with the swap.
    # AND THE SWEEP IS OVER THE WHOLE GATE, not the publisher alone. Limiting it to the publisher is
    # what let the provenance lookup keep reading `rev-parse --abbrev-ref HEAD`: the movement check
    # compares SHAs, so a checkout onto another branch AT THE SAME COMMIT left every guard quiet
    # while the lookup asked about the branch the operator had just moved to. Identity captured once
    # has to feed every consumer; a cell scoped to one consumer cannot see the others.
    #
    # Four reads are allowed and each is named, because an exception nobody can enumerate is not an
    # exception, it is a hole:
    #   the two CAPTURES, which are the single reading this rule is built on;
    #   the two INDEX GUARDS, which protect a write git gives no atomic form for -- declared with
    #   their residue in ci/gate.ps1 and in this suite's worktree cell.
    # THE FOUR NAMES MOVED IN #762 AND THE PIN MOVED WITH THEM, deliberately and no wider. Each read
    # is now a CAPTURE whose exit code is read before the value is reduced, so the spelling is
    # `$xOutput = @(& git ...)` rather than `$x = (& git ... | Select-Object -First 1)`. The
    # allow-list still names four specific variables: widening it to a pattern would have been the
    # cheap way through this failure and would have retired the guard rather than moved it.
    $allowedIdentityReads = @('$gatedBranchOutput = @(& git symbolic-ref', '$gatedHeadOutput = @(& git rev-parse HEAD',
        '$branchAtIndexOutput = @(& git symbolic-ref', '$branchAfterTouchOutput = @(& git symbolic-ref',
        '$headNow = (git rev-parse HEAD)')
    $strayIdentityReads = @(($gateText -split "`n") | Where-Object {
            $line = $_
            $line -cmatch 'symbolic-ref|rev-parse\s+HEAD|rev-parse --abbrev-ref' -and $line -cnotmatch '^\s*#' -and
                -not @($allowedIdentityReads | Where-Object { $line -cmatch [regex]::Escape($_) })
        })
    Assert-True -Condition ($strayIdentityReads.Count -eq 0) `
        -Message ("no identity read in the gate outside the four named ones" +
            $(if ($strayIdentityReads.Count -gt 0) { ': ' + (($strayIdentityReads | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))

    # Not vacuous: a fresh read of the shape that caused this finding has to be found.
    $strayCanary = '        $branch = (& git rev-parse --abbrev-ref HEAD 2>$null | Select-Object -First 1)'
    $foundStray = @(($strayCanary -split "`n") | Where-Object {
            $line = $_
            $line -cmatch 'symbolic-ref|rev-parse\s+HEAD|rev-parse --abbrev-ref' -and $line -cnotmatch '^\s*#' -and
            -not @($allowedIdentityReads | Where-Object { $line -cmatch [regex]::Escape($_) })
        })
    Assert-True -Condition ($foundStray.Count -eq 1) `
        -Message 'and the sweep finds the provenance read that caused this finding when it is put back'

    # And the value the lookup uses comes from the capture, through a parameter.
    Assert-True -Condition ($gateText -cmatch 'Get-HeadProvenance -HeadSha \$headSha -BranchRef \$script:gatedBranchAtStart') `
        -Message 'the provenance lookup is handed the captured branch, not one it read itself'

    $publisherText = $publisher
    $casLine = ($publisherText -split "`n" | Select-String -Pattern 'git update-ref' | Select-Object -First 1)
    Assert-True -Condition ($null -ne $casLine) -Message 'ARRANGEMENT: the publication is a single update-ref'
    $beforeCas = $publisherText.Substring(0, $publisherText.IndexOf('git update-ref'))
    $readsBeforeCas = @(($beforeCas -split "`n") | Where-Object {
            $_ -cmatch 'symbolic-ref|rev-parse\s+HEAD' -and $_ -cnotmatch '^\s*#'
        })
    Assert-True -Condition ($readsBeforeCas.Count -eq 0) `
        -Message ("nothing between the entry and the swap asks git what HEAD is" +
            $(if ($readsBeforeCas.Count -gt 0) { ': ' + (($readsBeforeCas | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))
    $afterCas = $publisherText.Substring($publisherText.IndexOf('git update-ref'))
    Assert-True -Condition ($afterCas -cmatch '(?s)symbolic-ref.{0,900}?update-index --add --cacheinfo') `
        -Message 'and the index touch is guarded by a read taken immediately before it'

    Write-Host ''
    Write-Host '-- a worktree that moved does not get the manifest staged into it --' -ForegroundColor Cyan
    # The index is per worktree. With the branch switched after the swap, the compare-and-swap is
    # still correct -- the captured ref really does hold the gated head -- and the index touch would
    # write into the NEW branch's tree: the operator finds the gate's file staged in their work and
    # their next unqualified commit carries it under their message. The foreign-change refusal at
    # the top of the publisher defeated through the exit instead of the entrance.
    #
    # No interposition needed: the publisher is handed the CAPTURED branch while the worktree is
    # already on another one, which is exactly the state that race ends in.
    $repo = New-Repo -Name 'worktreemoved'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $captured = (& git -C $repo symbolic-ref --quiet HEAD).Trim()
    Push-Location $repo
    try { & git checkout --quiet -b bar 2>&1 | Out-Null } finally { Pop-Location }
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)

    $runner = Join-Path $fixtureRoot 'run-moved.ps1'
    [System.IO.File]::WriteAllText($runner,
        ($publisher + "`nPublish-RunManifest -ManifestPath '$manifest' -HeadSha '$gated' -PullRequest 7 -BranchRef '$captured'`n"),
        $utf8NoBom)
    Push-Location $repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>&1 | ForEach-Object { [string]$_ }) -join "`n"
    } finally { Pop-Location }

    Assert-True -Condition ((& git -C $repo rev-parse $captured).Trim() -cne $gated) `
        -Message 'the manifest is still published onto the branch the run gated'
    $stagedInBar = @(& git -C $repo diff --cached --name-only)
    Assert-True -Condition ($stagedInBar.Count -eq 0) `
        -Message "nothing is staged in the worktree's current branch ($($stagedInBar -join ', '))"
    Assert-True -Condition ($out -cmatch 'the index was NOT touched') `
        -Message 'and the run says what it skipped and how to recover, rather than doing it silently'

    Write-Host ''
    Write-Host '-- no durable record claims GREEN before the last thing that can turn it RED --' -ForegroundColor Cyan
    # Asserted over the SOURCE and said plainly: Write-RunManifest drives the whole gate. The order
    # is the defect -- the JSON was serialized GREEN, both copies went to disk, RUN-END said GREEN,
    # and only then could the compare-and-swap fail and take the exit code to 1. Whoever counts runs
    # from the store or the log counted a success the gate rejected.
    $publishAt = $gateText.IndexOf('Publish-RunManifest -ManifestPath $path')
    $runEndAt = $gateText.IndexOf("Write-SlotEvent -Event 'RUN-END'")
    Assert-True -Condition ($publishAt -gt 0 -and $runEndAt -gt $publishAt) `
        -Message 'RUN-END is written after the publication, not before it'
    # Single-quoted: a double-quoted regex has its `$` eaten by PowerShell before the engine sees
    # it, and the pattern then fails to PARSE -- a cell that cannot run is not a cell that passes.
    # Second time this trap has cost a cell in this file; the quoting is the fix, saying so is the
    # rest of it.
    $endWired = '(?s)\$endStatus = if \(\$script:manifestPublished\) \{ \$Status \} else \{ ''RED'' \}'
    Assert-True -Condition ($gateText -cmatch $endWired) `
        -Message 'and it carries RED when the publication was refused'
    # The window is generous on purpose: this pattern's job is to prove the correction sits inside
    # the refusal branch, not to police how much prose stands between them. A tight window made the
    # cell fail on a COMMENT being added -- an assertion about the distance between two lines rather
    # than about the program, and I pushed that red before noticing.
    $rewriteWired = '(?s)if \(-not \$script:manifestPublished\) \{[\s\S]{0,2500}?\$manifest\.status = ''RED'''
    Assert-True -Condition ($gateText -cmatch $rewriteWired) `
        -Message 'and the durable copies are rewritten, so no file on disk still says GREEN'

    # EVERY DERIVED FIELD, from the same source. Correcting the ones I remembered left the record
    # saying RED beside `headMovedDuringRun: false` -- the run failed and the record denied the fact
    # that explains it. Same shape as guarding remembered fields in the checker, one file over.
    $movedWired = '\$manifest\.headMovedDuringRun = \[bool\]\$script:headMovedDuringRun'
    Assert-True -Condition ($gateText -cmatch $movedWired) `
        -Message 'the corrected record takes headMovedDuringRun from the flag the late CAS sets, not the early local'

    # A CORRECTION THAT CANNOT REPORT ITS OWN FAILURE IS NOT A CORRECTION. The empty catch left a
    # durable copy saying GREEN while the gate exited RED -- in silence, and reading as coverage
    # because the block is visibly there.
    # Written over the SHAPE, not over the variable names the old code happened to use: a pattern
    # naming `$copies` would have been vacuously true against the previous subject, where the loop
    # was written differently. An empty `catch` anywhere in this file is the defect.
    $emptyCatches = @(($gateText -split "`n") | Where-Object { $_ -cmatch 'catch \{\s*\}' })
    Assert-True -Condition ($emptyCatches.Count -eq 0) `
        -Message ("nothing in the gate swallows an exception without a word" +
            $(if ($emptyCatches.Count -gt 0) { ': ' + (($emptyCatches | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))
    $reportWired = '(?s)\$script:manifestCorrectionFailed.{0,600}?\$failed \+= ''run manifest correction'''
    Assert-True -Condition ($gateText -cmatch $reportWired) `
        -Message 'and a correction that could not be persisted is a gate failure of its own'
    # #742: THE DISCIPLINE IS NO LONGER BORROWED, IT IS CALLED. This used to grep for a hand-rolled
    # `$copy.correcting` staging loop, and the comment beside it explained why one existed: the pair
    # writer reserved a NEW name on every call, and a correction must reuse the name already on disk.
    # Splitting the NAME from the WRITE removed that reason -- `Write-ManifestPairContent` takes the
    # final paths -- so both callers now share one implementation of the half-pair invariant instead
    # of each carrying a copy that drifts. They had already drifted: only one of them could roll back.
    #
    # So the assertion moves from "does this block re-implement the discipline correctly" to "does it
    # implement it at all". Staging-before-commit is now covered where it lives, and BEHAVIOURALLY
    # rather than by pattern: ci/manifest-name.tests.ps1 arms a partial commit and asserts both
    # stores are restored. A pattern here could only have asserted the wiring, which is the failure
    # mode gate.ps1's own comment records about this very call site.
    $sharedWriterWired = '(?s)Write-ManifestPairContent -FinalPaths @\(\$written\)[\s\S]{0,300}?-Commit ''Replace'''
    Assert-True -Condition ($gateText -cmatch $sharedWriterWired) `
        -Message 'the correction writes through the shared pair writer, on the names it already holds'
    # THE NEGATIVE HALF, and it is the one that keeps the invariant single. A second hand-rolled
    # staging loop appearing here later would satisfy the assertion above and re-open #742 in
    # silence -- two writers again, with the call to the shared one still visibly present.
    Assert-True -Condition (-not ($gateText -cmatch '\.correcting')) `
        -Message 'and carries no staging loop of its own, so the half-pair invariant has one implementation'


    # ---- The class is derived, so a corrected status carries a corrected class.
    Write-Host ''
    Write-Host '-- a refused publication does not leave the class saying it passed --' -ForegroundColor Cyan
    # #199 made the class DERIVED precisely so it could not disagree with the status. Writing the
    # status by hand on the refusal path reintroduced the drift one field over: `status: RED` beside
    # a class that still said the run passed, in the two records that outlive the process.
    # SINGLE-QUOTED, and this is the THIRD time the double-quoted form has cost a cell in this file
    # -- the second time with the warning already written three hundred lines up. A comment did not
    # stop me; a habit has to. Every regex here is single-quoted from now on, whether or not it
    # happens to contain a `$`.
    $classWired = '(?s)\$endClass = if \(\$script:manifestPublished\) \{ \$runClass \}'
    Assert-True -Condition ($gateText -cmatch $classWired) `
        -Message 'the ledger event recomputes the class when the publication was refused'
    $durableClassWired = '\$manifest\.runClass = Get-RunClassFrom -Status ''RED'''
    Assert-True -Condition ($gateText -cmatch $durableClassWired) `
        -Message 'and so does the durable copy, from the same function that derived it in the first place'

    # ---- The preflight asks about the host the question is about.
    Write-Host ''
    Write-Host '-- an unrelated gh host does not silence the pull request lookup --' -ForegroundColor Cyan
    # Bare `gh auth status` reports on EVERY host gh knows, so an expired token for some unrelated
    # enterprise host made the preflight fail and the pull request was recorded as "nobody could
    # look" while github.com was perfectly reachable. An instrument that answers about the wrong
    # subject is not conservative, it is wrong.
    $repo = New-Repo -Name 'authhost'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    # The stub exits non-zero for `auth status` UNLESS it is scoped to github.com, which is exactly
    # the shape of the defect: the unscoped call fails, the scoped one succeeds.
    $stub = New-GhStub -Name 'authhost' -Json ('[{"number":88,"headRefOid":"' + $head + '"}]') -FailUnscopedAuth
    $result = Invoke-Provenance -Repo $repo -StubDir $stub
    Assert-True -Condition ($result.pullRequest -eq 88) `
        -Message "the lookup happens because the preflight asked about github.com (got $($result.pullRequest))"

    # ---- Two branches at one commit: the SHA guards see nothing.
    Write-Host ''
    Write-Host '-- the lookup asks about the branch this run gated --' -ForegroundColor Cyan
    # The movement check compares SHAs. A checkout onto a DIFFERENT local branch pointing at the
    # SAME commit leaves every SHA-based guard quiet -- correctly, nothing about the SHA changed --
    # while a fresh read of the branch name sends the pull-request lookup somewhere else. The record
    # then names a pull request belonging to a branch this run never gated.
    #
    # The stub answers only when asked about the captured branch, so the assertion is about WHICH
    # name reached gh, not merely about the number that came back.
    $repo = New-Repo -Name 'twobranches'
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $captured = (& git -C $repo symbolic-ref --quiet HEAD).Trim()
    $capturedShort = $captured -replace '^refs/heads/', ''
    Push-Location $repo
    try { & git checkout --quiet -b sibling 2>&1 | Out-Null } finally { Pop-Location }
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $head) `
        -Message 'ARRANGEMENT: both branches point at the same commit, so every SHA guard stays quiet'

    $stub = New-GhStub -Name 'twobranches' -Json ('[{"number":91,"headRefOid":"' + $head + '"}]') -RequireHead $capturedShort
    $runner = Join-Path $fixtureRoot 'run-twobranches.ps1'
    [System.IO.File]::WriteAllText($runner,
        ($subject + "`n`$r = Get-HeadProvenance -HeadSha '$head' -BranchRef '$captured'`n`$r | ConvertTo-Json -Compress`n"),
        $utf8NoBom)
    $previousPath = $env:PATH
    $env:PATH = "$stub;$env:PATH"
    Push-Location $repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>$null | ForEach-Object { [string]$_ })
    } finally { Pop-Location; $env:PATH = $previousPath }
    $json = ($out | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1)
    $result = if ($json) { $json | ConvertFrom-Json } else { $null }
    Assert-True -Condition ($null -ne $result -and $result.pullRequest -eq 91) `
        -Message "the lookup used the captured branch, not the one the worktree moved to (got $($result.pullRequest))"

    # ---- Absent, false, true, and MALFORMED are four states.
    Write-Host ''
    Write-Host '-- a config git cannot read is not a config saying no --' -ForegroundColor Cyan
    # Measured before writing this, because the exit code IS the discriminator and no message
    # parsing is needed:
    #   absent      exit 1
    #   'yesplease' exit 128   fatal: bad boolean config value
    #   'True'      exit 0     true
    # `2>$null` collapsed 128 into 1, so a malformed config produced an UNSIGNED commit from the
    # gate where a normal `git commit` refuses. Silent policy change, which this file's own comment
    # two lines up forbids.
    $repo = New-Repo -Name 'gpgmalformed'
    Push-Location $repo
    try { & git config commit.gpgsign 'yesplease' 2>&1 | Out-Null } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
        -Message 'a malformed commit.gpgsign stops the publication instead of producing an unsigned commit'
    # THE CELL NOW ASKS FOR GIT'S WORDS, not for my sentence. It used to assert the phrase "cannot
    # read as a boolean", which the run ASSERTED as the cause from an exit code that also covers a
    # config file git could not read at all -- measured: three runs of this suite printed that
    # sentence with no `commit.gpgsign` set at any level. `bad boolean config value` is git's own
    # text, so matching it proves the run reported what happened instead of naming a cause.
    Assert-True -Condition ($out -cmatch 'commit\.gpgsign' -and $out -cmatch 'bad boolean config value') `
        -Message 'and the run repeats what GIT said about the config, rather than naming a cause of its own'

    # The control: ABSENT is a different state and still publishes, unsigned, as it always has.
    $repo = New-Repo -Name 'gpgabsent'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -cne $gated) `
        -Message 'CONTROL: an absent commit.gpgsign is not malformed, and the run publishes'

    # ---- A refused ref update is not proof the branch moved.
    Write-Host ''
    Write-Host '-- a hook refusing the update is not a moved head --' -ForegroundColor Cyan
    # Any non-zero from `update-ref` was recorded as movement, so a `reference-transaction` hook
    # saying no wrote `headMovedDuringRun: true` into the durable record with the branch standing
    # still -- and told the operator to re-run on a stable checkout, which fixes nothing they can
    # see. The tool failing and the fact being observed are different states.
    $repo = New-Repo -Name 'hookrefuses'
    # OUTSIDE the repository. Written inside it, the hook is itself a change outside the manifest
    # store, so the foreign-change refusal fires first and the publication never reaches the ref
    # update -- the cell would have been measuring the wrong refusal, and its own PASS on the two
    # assertions above would have hidden that.
    $hooks = Join-Path $fixtureRoot 'hookrefuses-hooks'
    [System.IO.Directory]::CreateDirectory($hooks) | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $hooks 'reference-transaction'), "#!/bin/sh`nexit 1`n", $utf8NoBom)
    Push-Location $repo
    try { & git config core.hooksPath $hooks 2>&1 | Out-Null } finally { Pop-Location }
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    [System.IO.File]::WriteAllText($manifest, (@{ headSha = $gated } | ConvertTo-Json), $utf8NoBom)
    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -ceq $gated) `
        -Message 'ARRANGEMENT: the hook really did refuse, so the branch never moved'
    Assert-True -Condition ($out -cmatch 'HEADMOVEDFLAG=False') `
        -Message 'the durable record does not claim the head moved when it did not'
    Assert-True -Condition ($out -cmatch 'still points at' -and $out -cmatch 'look at what refused') `
        -Message "and the operator is sent to the thing that refused, not to a stable checkout -- publisher said: $out"

    # ---- The commit carries the bytes the run serialized.
    Write-Host ''
    Write-Host '-- the published blob is what the gate wrote, not what the disk holds --' -ForegroundColor Cyan
    # `hash-object` reads the FILE. Between the pair being written and the publication, the store
    # copy is ordinary disk -- and the foreign-change refusal deliberately allows every path under
    # the store, so nothing upstream objects. The gate would publish somebody else's bytes under its
    # own message, with the durable copy saying something different, and the ref CAS still
    # succeeding.
    #
    # The fixture replaces the store copy AFTER writing it and BEFORE publishing, which is exactly
    # the window: no timing needed, because the substitution simply happens first.
    $repo = New-Repo -Name 'tamperedstore'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $manifest = Join-Path $store 'run.json'
    $honest = (@{ headSha = $gated; status = 'GREEN' } | ConvertTo-Json -Compress)
    [System.IO.File]::WriteAllText($manifest, $honest, $utf8NoBom)
    [System.IO.File]::WriteAllText($manifest, '{"status":"tampered"}', $utf8NoBom)

    $out = Invoke-Publisher -Repo $repo -ManifestPath $manifest -HeadSha $gated -PullRequest 7 -Content $honest
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -cne $gated) `
        -Message 'ARRANGEMENT: the publication happened, so there is a blob to look at'
    $committed = (& git -C $repo show "HEAD:.factory/gate-runs/run.json") -join "`n"
    Assert-True -Condition ($committed -cmatch 'GREEN' -and $committed -cnotmatch 'tampered') `
        -Message "the committed blob is the run's own serialization, not the file on disk ($committed)"
    # AND THE DISK IS BROUGHT BACK INTO AGREEMENT. Securing the object said nothing about the file:
    # left as it was, the gate exits GREEN with the commit holding one record and the worktree
    # holding another, and the index refresh stages a blob that does not match the file -- so the
    # operator's next `git commit -a` writes the stranger's bytes over the published ones.
    $onDisk = [System.IO.File]::ReadAllText($manifest)
    Assert-True -Condition ($onDisk -cnotmatch 'tampered' -and $onDisk -cmatch 'GREEN') `
        -Message "and the copy on disk is reconciled with what was published, not left disagreeing with it -- disk holds: $onDisk; publisher said: $out"
    # AND A RECONCILIATION THAT FAILS IS A GATE FAILURE, not a warning -- the rule the correction
    # path already carries, which I wrote as a warning here before being reminded of it. Asserted
    # over the source: the write it would have to fail is a file write the suite cannot make fail
    # reliably on this platform.
    # Matches the ASSIGNMENT, not the sentence: the message is built by concatenation across two
    # source lines, so a pattern spanning its words was asserting how the string is typeset. The
    # verdict wiring below is what carries the meaning.
    # #938: the pattern pins the ASSIGNMENT, not what follows it. It used to require `= (` because
    # the message was built by concatenation across two lines; the message now comes from
    # `Sync-ManifestCopies`, so requiring the parenthesis was asserting typesetting after all --
    # which is the thing the comment above says it is not doing.
    $reconcileWired = '\$script:manifestReconcileFailed\s*='
    Assert-True -Condition ($gateText -cmatch $reconcileWired) `
        -Message 'a failed reconciliation is recorded in a flag rather than printed and forgotten'
    $reconcileVerdict = '(?s)if \(\$script:manifestReconcileFailed\) \{.{0,400}?\$failed \+= ''run manifest reconciliation'''
    Assert-True -Condition ($gateText -cmatch $reconcileVerdict) `
        -Message 'and the final verdict fails the run for it'

    # ---- An upstream is only evidence of a push if it lives on a remote.
    Write-Host ''
    Write-Host '-- a local upstream is not a server --' -ForegroundColor Cyan
    # `branch.<name>.merge` can point at another LOCAL branch. `pushed: true` then certified a
    # commit that never left the machine -- and the field exists to answer "did the server get
    # this", which a local tracking relationship does not answer at all.
    $repo = New-Repo -Name 'localupstream'
    Push-Location $repo
    try {
        & git branch sibling 2>&1 | Out-Null
        & git branch --set-upstream-to=sibling 2>&1 | Out-Null
    } finally { Pop-Location }
    $result = Invoke-Provenance -Repo $repo
    Assert-True -Condition ($result.pushed -ne $true) `
        -Message "a local upstream does not certify a push ($(Format-ProvenanceForFailure -Result $result))"
    Assert-True -Condition ($result.pushedReason -cmatch 'a LOCAL branch') `
        -Message "and the reason names the local upstream rather than pretending there was none (reason: $($result.pushedReason))"

    # ---- The upstream is the CAPTURED branch's.
    Write-Host ''
    Write-Host '-- the upstream question names its subject --' -ForegroundColor Cyan
    # A bare `@{upstream}` resolves against whatever HEAD names now, so a sibling branch at the same
    # commit -- the case every SHA guard is blind to -- makes the answer about somebody else's
    # tracking configuration. Measured while fixing this, because the spelling is not obvious:
    #   master@{upstream}            -> refs/remotes/origin/master
    #   refs/heads/master@{upstream} -> fatal: no such branch
    $repo = New-Repo -Name 'siblingupstream'
    Push-Location $repo
    try {
        & git remote add origin (Join-Path $fixtureRoot 'origin-bare') 2>&1 | Out-Null
        & git fetch origin --quiet 2>&1 | Out-Null
        & git push -u origin HEAD:refs/heads/sib --quiet 2>&1 | Out-Null
        & git branch --set-upstream-to=origin/sib 2>&1 | Out-Null
        $captured = (& git symbolic-ref --quiet HEAD).Trim()
        # The worktree moves to a sibling at the same commit, with NO upstream of its own.
        & git checkout --quiet -b nowhere 2>&1 | Out-Null
    } finally { Pop-Location }
    $head = (& git -C $repo rev-parse HEAD).Trim()
    $runner = Join-Path $fixtureRoot 'run-siblingupstream.ps1'
    [System.IO.File]::WriteAllText($runner,
        ($subject + "`nGet-HeadProvenance -HeadSha '$head' -BranchRef '$captured' | ConvertTo-Json -Compress`n"),
        $utf8NoBom)
    Push-Location $repo
    try {
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner 2>$null | ForEach-Object { [string]$_ })
    } finally { Pop-Location }
    $json = ($out | Where-Object { $_.TrimStart().StartsWith('{') } | Select-Object -Last 1)
    $result = if ($json) { $json | ConvertFrom-Json } else { $null }
    Assert-True -Condition ($null -ne $result -and $null -ne $result.upstreamSha) `
        -Message 'the upstream is resolved from the branch this run gated, not from the one the worktree moved to'

    # ---- Ordering, and the primitive, for the two writes that outlive the process.
    Write-Host ''
    Write-Host '-- the correction lands before the event that describes it --' -ForegroundColor Cyan
    # RUN-END was appended BEFORE the durable copies were corrected, so a process killed between the
    # two left the ledger saying RED and the manifests saying GREEN. There is no way to make two
    # writes one; what there is, is a choice about which one can be re-derived. A ledger line with no
    # manifest correction is a RUN-START with no RUN-END, which #199 already treats as a dead run.
    # Anchored on the line that exists ONLY in the correction. `if (-not $script:manifestPublished)`
    # appears twice -- the ledger event tests the same flag -- so an index built from it compared the
    # event against ITSELF and passed against a subject where the correction came last.
    $correctionAt = $gateText.IndexOf("`$manifest.status = 'RED'")
    $runEndAt = $gateText.IndexOf("Write-SlotEvent -Event 'RUN-END'")
    Assert-True -Condition ($correctionAt -gt 0 -and $runEndAt -gt $correctionAt) `
        -Message 'the durable copies are corrected before RUN-END is appended, not after'

    Write-Host ''
    Write-Host '-- every manifest write uses the same primitive --' -ForegroundColor Cyan
    # `WriteAllText` writes THROUGH the destination: a kill halfway leaves a truncated file where a
    # valid record was. Both places that rewrite a published manifest now stage a sibling and
    # replace, and the sweep is what keeps the third one from being written the old way.
    $directManifestWrites = @(($gateText -split "`n") | Where-Object {
            $_ -cmatch 'WriteAllText\(' -and $_ -cnotmatch '^\s*#' -and
                ($_ -cmatch '\$ManifestPath\s*,' -or $_ -cmatch '\$copies\[' -or $_ -cmatch '\$copy\s*,')
        })
    Assert-True -Condition ($directManifestWrites.Count -eq 0) `
        -Message ("no manifest is overwritten in place" +
            $(if ($directManifestWrites.Count -gt 0) { ': ' + (($directManifestWrites | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))
    # #938: THE THIRD COPY IS GONE. This used to pin `$reconcileTmp ... File::Replace` -- the
    # reconciliation's own hand-rolled staging, which was the third implementation of a discipline
    # #742 gave a single one. It now goes through `Sync-ManifestCopies`, so the assertion moves from
    # "does this re-implement it correctly" to "does it call the one implementation", and the
    # behaviour is covered where it lives: ci/manifest-name.tests.ps1 arms a partial reconciliation
    # and asserts both copies keep their original bytes.
    $reconcileWired = '(?s)Sync-ManifestCopies -Paths[\s\S]{0,200}?-Content \$Content'
    Assert-True -Condition ($gateText -cmatch $reconcileWired) `
        -Message 'the reconciliation writes through the shared pair writer, on the copies the gate holds'
    # THE NEGATIVE HALF, and it is what keeps the invariant single. A hand-rolled staging sibling
    # reappearing here later would satisfy the assertion above and re-open #938 in silence.
    # BOUNDED TO CODE, NOT PROSE. The comment above this call site NAMES the leaked `.reconciling`
    # sibling in order to explain why it is gone, so a whole-file match would fail on its own
    # vocabulary -- the searcher matching the searcher, which is the trap #150's guard records and
    # which I flagged on #942 an hour before writing it here myself.
    $reconcilingCode = @(($gateText -split "`n") | Where-Object { $_ -cmatch '\.reconciling' -and $_ -cnotmatch '^\s*#' })
    Assert-True -Condition ($reconcilingCode.Count -eq 0) `
        -Message ('and carries no staging loop of its own, so the discipline has one implementation' +
            $(if ($reconcilingCode.Count -gt 0) { ': ' + (($reconcilingCode | ForEach-Object { $_.Trim() }) -join ' | ') } else { '' }))

    # Not vacuous: the sweep has to find a direct write when one is put in front of it.
    $writeCanary = '                    [System.IO.File]::WriteAllText($ManifestPath, $Content, $enc)'
    $canaryFound = @(($writeCanary -split "`n") | Where-Object {
            $_ -cmatch 'WriteAllText\(' -and $_ -cnotmatch '^\s*#' -and
                ($_ -cmatch '\$ManifestPath\s*,' -or $_ -cmatch '\$copies\[' -or $_ -cmatch '\$copy\s*,')
        })
    Assert-True -Condition ($canaryFound.Count -eq 1) `
        -Message 'and the sweep finds an in-place manifest write when one is put in front of it'

    # ---- The startup pair has to describe one state.
    Write-Host ''
    Write-Host '-- the branch and the head are one snapshot, or the run refuses --' -ForegroundColor Cyan
    # Two commands, so a checkout between them yields a branch and a sha that never described the
    # same state -- and every later guard compares against that pair as though they did. There is no
    # primitive that reads both at once, so the pair is read and then CHECKED.
    # The capture is now `@(& git ...)` and the reduction is a separate statement (#762), so the
    # window between the assignment and the comparison grew by one line. The BOUNDS are what make
    # this a wiring assertion rather than two unrelated greps, so they are widened by the size of
    # the inserted statement and not to a number that would match any file.
    $snapshotWired = '(?s)\$branchValueOutput = @\(& git rev-parse[\s\S]{0,400}?\$branchValueAtStart[\s\S]{0,220}?\$gatedHeadAtStart[\s\S]{0,600}?exit 1'
    Assert-True -Condition ($gateText -cmatch $snapshotWired) `
        -Message 'the captured branch is checked against the captured head, and a mismatch refuses the run'

    # ---- The reconciliation failure is recorded before anything is serialized.
    $flagBeforeSerialize = $gateText.IndexOf('$script:manifestReconcileFailed =')
    Assert-True -Condition ($flagBeforeSerialize -gt 0 -and $flagBeforeSerialize -lt $correctionAt) `
        -Message 'and a failed reconciliation is flagged before the record that carries it is built'

    # ---- The pair is the unit, not the file.
    Write-Host ''
    Write-Host '-- every copy of the record is reconciled, not just the one named --' -ForegroundColor Cyan
    # `Write-GateManifestPair` writes the committable copy and the durable twin under one name
    # precisely so they are ONE record -- half a pair must not survive. The reconciliation I added
    # took a single path, so the twin could stay corrupted while the run declared success: the
    # parameter named one file and that made it easy to forget the other.
    $repo = New-Repo -Name 'twincopies'
    $gated = (& git -C $repo rev-parse HEAD).Trim()
    $store = Join-Path $repo '.factory/gate-runs'
    [System.IO.Directory]::CreateDirectory($store) | Out-Null
    $durable = Join-Path $fixtureRoot 'twincopies-durable'
    [System.IO.Directory]::CreateDirectory($durable) | Out-Null
    $honest = (@{ headSha = $gated; status = 'GREEN' } | ConvertTo-Json -Compress)
    $primary = Join-Path $store 'run.json'
    $twin = Join-Path $durable 'run.json'
    [System.IO.File]::WriteAllText($primary, $honest, $utf8NoBom)
    # BOTH copies corrupted, so a fix that reconciled only the first would leave this one behind --
    # which is exactly the state the finding describes.
    [System.IO.File]::WriteAllText($twin, '{"status":"tampered-twin"}', $utf8NoBom)
    [System.IO.File]::WriteAllText($primary, '{"status":"tampered-primary"}', $utf8NoBom)

    $out = Invoke-Publisher -Repo $repo -ManifestPath $primary -HeadSha $gated -PullRequest 7 `
        -Content $honest -Copies @($primary, $twin)
    Assert-True -Condition ((& git -C $repo rev-parse HEAD).Trim() -cne $gated) `
        -Message 'ARRANGEMENT: the publication happened, so there are copies to compare'
    $primaryAfter = [System.IO.File]::ReadAllText($primary)
    $twinAfter = [System.IO.File]::ReadAllText($twin)
    Assert-True -Condition ($primaryAfter -cnotmatch 'tampered') `
        -Message "the committable copy agrees with what was published ($primaryAfter) -- publisher said: $out"
    Assert-True -Condition ($twinAfter -cnotmatch 'tampered') `
        -Message "and so does the DURABLE twin, which the single-path version left corrupted ($twinAfter)"

    Write-Host ''
    Write-Host '-- the record says what the run COVERED, not only that it passed --' -ForegroundColor Cyan
    # #725, case 1. `-SkipPostgres` skips both PostgreSQL passes and the manifest said `GREEN` with
    # nothing recording that the run was partial, so a persistence-affecting change could be
    # certified by a gate the repository itself prints "is not a full gate" about. The producer has
    # to record the fact before any consumer can refuse it, and a verifier (`merge-proof`, retired
    # 2026-09-24) can only check fields the manifest carries.
    $coverageFn = ([System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$null)).Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -ceq 'Get-RunCoverage'
        }, $true)
    Assert-True -Condition ($null -ne $coverageFn) `
        -Message 'ARRANGEMENT: the coverage rule is a function this cell can call'
    if ($null -ne $coverageFn) {
        . ([scriptblock]::Create($coverageFn.Extent.Text))
        $partial = Get-RunCoverage -SkipPostgres $true
        Assert-True -Condition ($partial.postgres -ceq 'skipped') `
            -Message "a -SkipPostgres run records postgres as skipped (got [$($partial.postgres)])"
        Assert-True -Condition ($partial.complete -eq $false) `
            -Message 'and says the run was not complete, so a consumer can refuse it without knowing which flag was set'
        $full = Get-RunCoverage -SkipPostgres $false
        Assert-True -Condition ($full.postgres -ceq 'included' -and $full.complete -eq $true) `
            -Message "CONTROL: a run without the flag records postgres as included and complete (got [$($full.postgres)])"
    }

    Write-Host ''
    Write-Host '-- an untracked file is a tree no commit contains --' -ForegroundColor Cyan
    # #725, case 2. `dirtyDiffHash` came from `git diff HEAD`, which OMITS untracked paths, so a run
    # with an automatically discovered `build.rs` sitting untracked recorded a NULL hash -- the value
    # that reads as "the worktree was exactly the commit" -- while measuring a tree no commit
    # contains. The value was present and honest about the wrong question, which is why neither of
    # merge-proof's refusals (retired 2026-09-24) could catch it.
    $dirtFn = ([System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$null)).Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -ceq 'Get-WorktreeDirt'
        }, $true)
    Assert-True -Condition ($null -ne $dirtFn) `
        -Message 'ARRANGEMENT: the dirt rule is a function this cell can call'
    if ($null -ne $dirtFn) {
        . ([scriptblock]::Create($dirtFn.Extent.Text))
        $untracked = Get-WorktreeDirt -Diff '' -PorcelainLines @('?? build.rs')
        Assert-True -Condition ($null -ne $untracked.hash) `
            -Message 'an untracked build-affecting file makes the run dirty, where a diff-only reading called it clean'
        Assert-True -Condition (@($untracked.untracked) -ccontains 'build.rs') `
            -Message 'and the record NAMES it, so a reader does not have to re-derive which path made the run dirty'

        # THE GATE'S OWN ARTEFACTS ARE NOT THE AUTHOR'S WORK, and this control is what keeps the
        # fix from freezing the board: the manifest store gains a file per run by design and
        # `Write-CanaryNonce` rewrites the nonce before every run, so counting either as dirt would
        # mark EVERY run dirty and refuse every merge. Same two exclusions the publisher already
        # makes, for the same reasons.
        $ownOnly = Get-WorktreeDirt -Diff '' -PorcelainLines @(
            '?? .factory/gate-runs/abc123-20260904T000000.000Z-deadbeef.json',
            ' M tools/ci-canary/src/nonce.rs')
        Assert-True -Condition ($null -eq $ownOnly.hash -and @($ownOnly.untracked).Count -eq 0) `
            -Message "CONTROL: the gate's own store and nonce are not dirt, or every run would refuse every merge"

        # THE CASE EVERY ORDINARY RUN IS IN, and the one that made the merge proof unsatisfiable.
        # `Write-CanaryNonce` rewrites the tracked nonce before the run starts, so `git diff HEAD`
        # is NON-EMPTY on every gate invocation. Deciding dirtiness from the diff recorded a
        # non-null hash every time -- 29 of 29 manifests in this worktree -- and merge-proof (retired
        # 2026-09-24) refused a non-null `dirtyDiffHash`. The porcelain is what decides, so a tree whose only change is
        # the gate's own artefact is CLEAN however much diff text that artefact produces.
        $nonceOnly = Get-WorktreeDirt `
            -Diff "diff --git a/tools/ci-canary/src/nonce.rs b/tools/ci-canary/src/nonce.rs`n-old`n+new" `
            -PorcelainLines @(' M tools/ci-canary/src/nonce.rs')
        Assert-True -Condition ($null -eq $nonceOnly.hash) `
            -Message "a tree whose only change is the gate's own nonce is clean, or no run can ever satisfy the merge proof (got [$($nonceOnly.hash)])"

        # AND A REAL DIFF STILL HASHES, unchanged: this widens what counts as dirty and must not
        # narrow it.
        $tracked = Get-WorktreeDirt -Diff "diff --git a/x b/x`n+one" -PorcelainLines @(' M x')
        Assert-True -Condition ($null -ne $tracked.hash) `
            -Message 'CONTROL: a tracked modification still hashes, so this only widens the reading'
        $sameAgain = Get-WorktreeDirt -Diff "diff --git a/x b/x`n+one" -PorcelainLines @(' M x')
        Assert-True -Condition ($tracked.hash -ceq $sameAgain.hash) `
            -Message 'and the hash is a function of the tree, not of when it was taken'
    }

# Exact gate functions, real local Git and native children; no GitHub call is permitted.
& {
    $tokens=$null; $errors=$null
    $tree=[Management.Automation.Language.Parser]::ParseInput($gateText,[ref]$tokens,[ref]$errors)
    foreach($name in @('Invoke-LandingCommand','Get-LandingSnapshot','Get-RunCoverage')) {
        $node=$tree.Find({param($a) $a -is [Management.Automation.Language.FunctionDefinitionAst] -and $a.Name -eq $name},$false)
        Invoke-Expression $node.Extent.Text
    }
    $native=${function:Invoke-LandingCommand}
    function Invoke-LandingCommand {
        param($Program,$Arguments,$Root,$TimeoutMilliseconds=30000)
        if($Program -eq 'gh'){throw 'Network discovery is forbidden in the offline gate'}
        & $native -Program $Program -Arguments $Arguments -Root $Root -TimeoutMilliseconds $TimeoutMilliseconds
    }
    $repo=Join-Path $fixtureRoot 'offline landing'
    [void][IO.Directory]::CreateDirectory($repo)
    & git -C $repo init --quiet -b main
    & git -C $repo -c user.name=Fixture -c user.email=fixture@example.invalid -c commit.gpgSign=false -c core.hooksPath=NUL commit --quiet --allow-empty -m baseline
    $head=(& git -C $repo rev-parse HEAD).Trim()
    & git -C $repo update-ref refs/remotes/origin/main $head
    $local=Get-LandingSnapshot -Root $repo -Head $head
    Assert-True ($local.sha -ceq $head -and $local.mode -ceq 'non-pr') 'default landing capture is fully local and needs no gh'
    $coverage=Get-RunCoverage -SkipPostgres $false -NonPullRequest $true
    Assert-True $coverage.complete 'offline local evidence preserves full test completeness'
    $path=Join-Path $fixtureRoot 'landing.json'
    $record=@{head=$head;sha=$head;ref='stacked';pullRequest=993}
    [IO.File]::WriteAllText($path,($record|ConvertTo-Json),$utf8NoBom)
    $supplied=Get-LandingSnapshot -Root $repo -Head $head -SnapshotPath $path
    Assert-True ($supplied.mode -ceq 'supplied-pr' -and $supplied.sha -ceq $head -and $supplied.pullRequest -eq 993) 'supplied snapshot validates locally and remains distinct from verified PR proof'
    foreach($case in @('wrong head','bad sha','missing PR','string PR','oversize','bad JSON','base mismatch','local conflict')) {
        $record=@{head=$head;sha=$head;ref='stacked';pullRequest=993}
        $expected=''; $localFlag=$false
        switch($case){
            'wrong head' {$record.head='b'*40}
            'bad sha' {$record.sha='bad'}
            'missing PR' {$record.Remove('pullRequest')}
            'string PR' {$record.pullRequest='993'}
            'base mismatch' {$expected='main'}
            'local conflict' {$localFlag=$true}
        }
        $json=$record|ConvertTo-Json
        if($case -eq 'oversize'){$json=' '*8193}
        if($case -eq 'bad JSON'){$json='invalid'}
        [IO.File]::WriteAllText($path,$json,$utf8NoBom)
        $refused=$false
        try{$null=Get-LandingSnapshot -Root $repo -Head $head -SnapshotPath $path -ExpectedRef $expected -LocalOnly:$localFlag}catch{$refused=$true}
        Assert-True $refused "offline snapshot refuses $case"
    }
    $result=& $native -Program powershell.exe -Root $repo -Arguments @('-NoProfile','-Command','exit 7')
    Assert-True ($result.ExitCode -eq 7) 'native exit status survives completion'
    $refused=$false
    try{$null=& $native -Program powershell.exe -Root $repo -Arguments @('-NoProfile','-Command','Start-Sleep 10') -TimeoutMilliseconds 50}catch{$refused=$true}
    Assert-True $refused 'real sleeping child reaches the polling timeout refusal'
    $refused=$false
    try{$null=& $native -Program powershell.exe -Root $repo -Arguments @('-NoProfile','-Command','exit 0') -TimeoutMilliseconds 0}catch{$refused=$true}
    Assert-True $refused 'zero budget refuses native admission'
}
Write-Host ''
Write-Host '#950: the extracted publisher runs under the environment the gate gives it' -ForegroundColor Cyan
{
    # Not a fresh construction: the bytes the LAST publisher subprocess of this run was handed. A
    # builder asserted on its own is a claim about a function nobody has to call.
    $seen = [string]$script:lastPublisherScript
    if (-not $seen) {
        Write-Host 'HARNESS-BROKE: no publisher runner script was captured; the cells above did not run.' -ForegroundColor Magenta
        exit 2
    }
    foreach ($line in $runnerPreambleLines) {
        Assert-True ($seen.Contains($line)) "the runner carries the gate's own statement: $line"
    }
}.Invoke() | Out-Null

Write-Host ''
Write-Host '#950/#1169: the derived preamble carries EVERY strictness-setting import, not just the first' -ForegroundColor Cyan
{
    # THE HOLE THIS CLOSES, reproduced before it was closed. This preamble used to reproduce one
    # hard-coded import, `ci/manifest-name.ps1`. `ci/gate.ps1` dot-sources a SECOND library that
    # also sets `Set-StrictMode -Version Latest` in the caller's scope, `ci/crate-input-hash.ps1`,
    # and it is dot-sourced LAST, so it is the one that decides what `Publish-RunManifest` runs
    # under. Mutating that file from `Latest` to `2.0` -- a change to production -- left this
    # suite at 207/207 passed, exit 0, nothing red.
    #
    # Asserted by NAME. Re-running the subject's own discovery loop here would produce a cell that
    # cannot disagree with the subject.
    $derived = @(Get-GateRunnerPreamble -GateText $gateText -CiRoot $PSScriptRoot)
    $setsStrictness = @('manifest-name.ps1', 'crate-input-hash.ps1')
    # THE DECOY. `run-class.ps1` is dot-sourced at column zero and sets NO strictness. Without it
    # this cell would pass just as well for a subject that reproduced every dot-source
    # indiscriminately -- a different program, one that drags libraries this preamble does not
    # need into the fixture runner.
    $setsNothing = @('run-class.ps1')

    # The repository IS the fixture here, so assert the fixture before the subject: if one of these
    # files changes which side it is on, this cell must SAY that, not fail as though the subject
    # had regressed.
    $fixtureOk = $true
    foreach ($group in @(@{ names = $setsStrictness; want = $true }, @{ names = $setsNothing; want = $false })) {
        foreach ($name in $group.names) {
            $libPath = Join-Path $PSScriptRoot $name
            if (-not (Test-Path -LiteralPath $libPath)) {
                Write-Host "HARNESS-BROKE: ci/$name, which this cell classifies, is not in ci/" -ForegroundColor Magenta
                $fixtureOk = $false
                continue
            }
            $has = @((([System.IO.File]::ReadAllText($libPath)) -split "`r?`n") |
                Where-Object { $_ -cmatch '^Set-StrictMode -Version \S+$' }).Count -ge 1
            if ($has -ne $group.want) {
                Write-Host "HARNESS-BROKE: ci/$name sets column-zero Set-StrictMode = $has; this cell assumed $($group.want)" -ForegroundColor Magenta
                $fixtureOk = $false
            }
        }
    }
    # And the decoy has to actually BE dot-sourced, or it is not a decoy -- just an unrelated file
    # nobody would have expected in the preamble.
    foreach ($name in ($setsStrictness + $setsNothing)) {
        $dot = ". (Join-Path `$PSScriptRoot '$name')"
        if (-not (@($gateText -split "`r?`n") -ccontains $dot)) {
            Write-Host "HARNESS-BROKE: ci/gate.ps1 no longer dot-sources $name at column zero" -ForegroundColor Magenta
            $fixtureOk = $false
        }
    }
    if (-not $fixtureOk) { exit 2 }

    foreach ($name in $setsStrictness) {
        $expected = ". (Join-Path '$PSScriptRoot' '$name')"
        Assert-True ($derived -ccontains $expected) "the derived preamble reproduces the strictness-setting import ci/$name (production's strictness is decided by the LAST such import, not the first)"
    }
    foreach ($name in $setsNothing) {
        $expected = ". (Join-Path '$PSScriptRoot' '$name')"
        Assert-True (-not ($derived -ccontains $expected)) "the derived preamble does NOT reproduce ci/$name, which sets no strictness"
    }
}.Invoke() | Out-Null

Write-Host ''
Write-Host '#950: the preamble observer reads ci/gate.ps1 in SOURCE ORDER and refuses an ambiguous read' -ForegroundColor Cyan
{
    # A read-only mutation of the source -- moving the only `Set-StrictMode` to immediately before
    # `headMovedDuringRun`, after every import -- changes what production does: the library's
    # `Latest` would now be overridden by the gate's `2.0`. A builder that emits the four statements in an order of its
    # own answers byte-identically to that mutation, so the harness would go on running under a
    # strictness the gate no longer has. These cells measure the ORDER, not just the membership,
    # and they measure the refusal when the read is not unambiguous.
    $gateLines = $gateText -split "`r?`n"
    # Named here, not recomputed by the subject's own discovery loop: a cell that re-derived
    # membership the way the subject derives it would agree with the subject however wrong both
    # were. The names are guarded against the repository by the cell above.
    $importPattern = "^\. \(Join-Path \`$PSScriptRoot '(manifest-name|crate-input-hash)\.ps1'\)$"
    $isPreamble = {
        param([string] $Line)
        ($Line -cmatch '^Set-StrictMode -Version \S+$') -or
        ($Line -cmatch "^\`$ErrorActionPreference = '\w+'$") -or
        ($Line -cmatch $importPattern) -or
        ($Line -cmatch "^\`$script:headMovedDuringRun = \`$(true|false)$")
    }
    # The dot-source is the one statement the builder is allowed to rewrite (the fixture runner's
    # `$PSScriptRoot` is not `ci/`). Expectation applies the same substitution, so the comparison
    # is about ORDER and nothing else.
    $expectFrom = {
        param([string[]] $Lines)
        @($Lines | ForEach-Object {
            if ($_ -cmatch $importPattern) {
                $_.Replace('$PSScriptRoot', "'" + $PSScriptRoot + "'")
            } else { $_ }
        })
    }

    $unchanged = Get-GateRunnerPreamble -GateText $gateText -CiRoot $PSScriptRoot
    $wantUnchanged = & $expectFrom @($gateLines | Where-Object { & $isPreamble $_ })
    Assert-True ($null -ne $unchanged -and ((($unchanged) -join "`n") -ceq (($wantUnchanged) -join "`n"))) 'the unmutated gate is read in the order its own lines appear'

    $strictLine = @($gateLines | Where-Object { $_ -cmatch '^Set-StrictMode -Version \S+$' })[0]
    $dotLine = @($gateLines | Where-Object { $_ -cmatch "^\. \(Join-Path \`$PSScriptRoot 'manifest-name\.ps1'\)$" })[0]
    $eapLine = @($gateLines | Where-Object { $_ -cmatch "^\`$ErrorActionPreference = '\w+'$" })[0]
    $movedLine = @($gateLines | Where-Object { $_ -cmatch "^\`$script:headMovedDuringRun = \`$(true|false)$" })[0]

    # THE REGRESSION. Legal PowerShell, a different program: strictness now lands after every
    # import, so it overrides the library's `Latest` in this scope.
    $reordered = New-Object 'System.Collections.Generic.List[string]'
    foreach ($line in $gateLines) {
        if ($line -ceq $strictLine) { continue }
        $reordered.Add($line)
        if ($line -ceq $movedLine) { $reordered.Add($strictLine) }
    }
    $reorderedLines = @($reordered.ToArray())
    $gotReordered = Get-GateRunnerPreamble -GateText ($reorderedLines -join "`n") -CiRoot $PSScriptRoot
    $wantReordered = & $expectFrom @($reorderedLines | Where-Object { & $isPreamble $_ })
    Assert-True ($null -ne $gotReordered -and ((($gotReordered) -join "`n") -ceq (($wantReordered) -join "`n"))) 'a source whose only Set-StrictMode moves after the dot-source is read in THAT order, not a fixed one'

    # Ambiguity, not tolerance: with two column-zero `Set-StrictMode` lines the gate runs under the
    # LAST one, and a builder that silently takes the first would hand the runner the losing
    # statement. There is no correct pick here, so there is no answer to return.
    foreach ($dup in @(
        @{ name = 'Set-StrictMode'; after = $strictLine; line = 'Set-StrictMode -Version Latest' },
        @{ name = '$ErrorActionPreference'; after = $eapLine; line = "`$ErrorActionPreference = 'Continue'" }
    )) {
        $mutated = New-Object 'System.Collections.Generic.List[string]'
        foreach ($line in $gateLines) {
            $mutated.Add($line)
            if ($line -ceq $dup.after) { $mutated.Add($dup.line) }
        }
        $got = Get-GateRunnerPreamble -GateText (($mutated.ToArray()) -join "`n") -CiRoot $PSScriptRoot
        Assert-True ($null -eq $got) ("a second column-zero " + $dup.name + " makes the read ambiguous and is refused")
    }

    foreach ($missing in @(
        @{ name = 'the manifest-name dot-source'; line = $dotLine },
        @{ name = '$script:headMovedDuringRun'; line = $movedLine }
    )) {
        $mutated = @($gateLines | Where-Object { -not ($_ -ceq $missing.line) })
        $got = Get-GateRunnerPreamble -GateText ($mutated -join "`n") -CiRoot $PSScriptRoot
        Assert-True ($null -eq $got) ("a source with no " + $missing.name + " at column zero is refused")
    }
}.Invoke() | Out-Null

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
