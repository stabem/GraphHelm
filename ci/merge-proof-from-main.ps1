<#
.SYNOPSIS
    Run the merge proof from `origin/main`'s copy, against a candidate checkout (#733).

.DESCRIPTION
    `ci/merge-proof.ps1` decides whether a recorded gate run vouches for the head a pull request
    would merge. Run from the candidate's own checkout, THE PULL REQUEST SUPPLIES THE PREDICATE
    THAT JUDGES IT: replacing that file with `exit 0` certifies anything, and the self-check it
    performs against `origin/main` is removed by the same edit that removes everything else.

    Self-attestation has no fixed point. The control belongs to the PROCEDURE -- the file being
    executed must not be one the candidate can edit -- and this script is that procedure made
    executable, so the human instruction is not the only thing standing between a candidate and
    its own predicate.

    THIS FILE CANNOT BOOTSTRAP ITSELF, AND SAYING SO IS THE POINT. It lives in the repository, so
    a candidate can edit IT too. Running this from the candidate checkout has exactly the weakness
    it exists to remove. The supported invocation obtains BOTH files from `origin/main`:

        git fetch --quiet origin main
        git worktree add --detach --quiet $env:TEMP\mp-main origin/main
        powershell -NoProfile -ExecutionPolicy Bypass `
            -File $env:TEMP\mp-main\ci\merge-proof-from-main.ps1 `
            -PullRequest <N> -RepositoryRoot <the candidate checkout>
        git worktree remove --force $env:TEMP\mp-main

    NOT `git show ... > file`. In Windows PowerShell 5.1 `>` IS `Out-File`, which RE-ENCODES:
    measured here, that spelling turns main's 63908-byte LF file into 64901 bytes with a BOM and
    a different blob id -- and the result PARSES WITH ZERO ERRORS. It is this file's own
    `WriteAllLines`/CRLF bug one layer up, in the layer no blob check covers, and an earlier
    version of this header told the operator to use exactly that spelling (found by a peer
    reviewing #811). `git worktree` has git write the bytes, so no encoding decision exists to
    get wrong.

    What this buys over doing it by hand is not safety it cannot provide. It is that the operator
    types one command instead of four, and that the bytes actually executed are VERIFIED to be
    main's rather than assumed to be -- see the blob check below, which is the half a hand-typed
    procedure silently skips.

.PARAMETER PullRequest
    The pull request whose head is being judged. Passed through unchanged.

.PARAMETER RepositoryRoot
    The checkout UNDER JUDGEMENT. Required in spirit and defaulted for convenience to the current
    directory, because the verifier defaults to the repository IT sits in -- which, once it has
    been extracted to a temporary directory, is not a repository at all. Getting this wrong is the
    likeliest way to run the right file against the wrong tree.

.PARAMETER Head
    Passed through unchanged.

.PARAMETER Json
    Passed through unchanged.

.PARAMETER LedgerDirectory
    Passed through unchanged.

.OUTPUTS
    THE VERIFIER'S EXIT VOCABULARY, UNCHANGED, because the last thing this script does is pass
    `ci/merge-proof.ps1`'s code through. A caller must be able to read one number without knowing
    which of the two scripts produced it.

        0  SATISFIED
        1  HARNESS-BROKE -- including every refusal below: a missing -RepositoryRoot, a failed
           fetch, no verifier on main, an unreadable one, or extracted bytes that are not main's
        2  NOT
        3  ABSENT

    THE FIRST VERSION OF THIS SCRIPT EXITED 2 ON ALL FIVE OF ITS OWN REFUSALS, which is the code
    for NOT -- a judgement about the CANDIDATE. It printed HARNESS-BROKE and returned "your proof
    is not satisfied", so the text and the number disagreed at the only interface a caller has,
    and `ci/merge-proof.tests.ps1` already pins 2 to the NOT verdict. It failed CLOSED, so the
    button blocked rather than certified -- but "the proof says NOT" and "the proof did not run"
    want different actions, and one of them sends someone to fix a branch that is fine. (Found by
    a peer reviewing #811.)

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File ci/merge-proof-from-main.ps1 `
        -PullRequest 696 -RepositoryRoot F:\github\GraphHelm
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [int] $PullRequest,
    [string] $RepositoryRoot = (Get-Location).Path,
    [string] $Head,
    [switch] $Json,
    [string] $LedgerDirectory
)

$ErrorActionPreference = 'Stop'

function Invoke-Git {
    <#
        Same trap `ci/normalize-script-eol.ps1` and `ci/closing-keywords.ps1` document: under
        Windows PowerShell 5.1 a redirected native stderr line becomes a NativeCommandError and
        `$ErrorActionPreference = 'Stop'` promotes it to a terminating error -- so a fetch failure
        would kill this function before its own exit-code check could report it.
    #>
    param([Parameter(Mandatory)] [string[]] $GitArgs)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & git @GitArgs 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [ordered]@{ exitCode = $code; lines = @($output | ForEach-Object { [string]$_ }) }
}

$root = $RepositoryRoot
if (-not (Test-Path -LiteralPath $root)) {
    Write-Host "[from-main] HARNESS-BROKE: -RepositoryRoot '$root' does not exist." -ForegroundColor Magenta
    exit 1
}
$root = (Resolve-Path -LiteralPath $root).Path

Push-Location -LiteralPath $root
try {
    # FETCH FIRST. A stale `origin/main` is the one case the verifier's own self-check is for, and
    # this script would otherwise run yesterday's copy while reporting that it ran main's.
    $fetch = Invoke-Git -GitArgs @('fetch', '--quiet', 'origin', 'main')
    if ($fetch.exitCode -ne 0) {
        Write-Host ("[from-main] HARNESS-BROKE: could not fetch origin/main, so the copy this " +
            "would run is whatever the checkout already had. Refusing rather than running a " +
            "stale predicate.") -ForegroundColor Magenta
        exit 1
    }

    # THIS FILE AGAINST MAIN'S COPY -- FIDELITY, NOT TRUST, and the distinction is the whole of it.
    # It CANNOT stop a hostile edit: a candidate who rewrote this file would rewrite this check
    # with it. What it catches is the accidental corruption, which is the case that actually
    # happens -- an operator who bootstrapped with a redirect and is now running a re-encoded copy
    # of the runner. Nobody should read this as self-attestation having found a fixed point.
    #
    # IT DEPENDS ON `.ps1` ROUND-TRIPPING BYTE-IDENTICALLY, which today it does: `.gitattributes`
    # carries `text eol=lf` rules for `*.rs`, `*.toml`, `*.yml` and `*.yaml` and none for `*.ps1`,
    # so the checked-out bytes equal the stored blob. Add `*.ps1 text eol=crlf` and this NOTE fires
    # on EVERY run -- a warning that always fires is a warning nobody reads, and the failure looks
    # cosmetic. (Hypothesis raised and refuted by a peer reviewing #811, who measured all three
    # hashes agreeing; recorded because nothing here pins the .gitattributes fact it rests on.)
    #
    # A DIFFERENCE WARNS AND DOES NOT REFUSE, deliberately. This file is legitimately different
    # from main's copy while it is being developed, and a refusal would make the branch that
    # changes it unrunnable. Absent on main is the bootstrap state, not a fault.
    $ownPath = $PSCommandPath
    $ownOnMain = Invoke-Git -GitArgs @('rev-parse', 'origin/main:ci/merge-proof-from-main.ps1')
    if ($ownOnMain.exitCode -eq 0 -and @($ownOnMain.lines).Count -gt 0 -and $ownPath) {
        $ownExpected = ([string]@($ownOnMain.lines)[0]).Trim()
        $ownActual = Invoke-Git -GitArgs @('hash-object', '--no-filters', '--', $ownPath)
        $ownActualBlob = ''
        if ($ownActual.exitCode -eq 0 -and @($ownActual.lines).Count -gt 0) {
            $ownActualBlob = ([string]@($ownActual.lines)[0]).Trim()
        }
        if (-not [string]::Equals($ownActualBlob, $ownExpected, [System.StringComparison]::Ordinal)) {
            Write-Host ("[from-main] NOTE: this runner is not main's copy (main $ownExpected, " +
                "this $ownActualBlob). That is expected on a branch that changes it, and it is " +
                "what a redirect-bootstrapped copy looks like. FIDELITY only -- a hostile edit " +
                "would have rewritten this check too. A checkout that PREDATES the `*.ps1 text " +
                "eol=lf` rule also differs here, every single run, because its working-tree bytes " +
                "are CRLF and the stored blob is LF -- use the documented `git worktree add` " +
                "invocation above, which creates the tree fresh and applies the rule.") `
                -ForegroundColor Yellow
        }
    }

    # THE EXPECTED BLOB, read before the extraction rather than after, so the comparison below has
    # an independent side. `rev-parse <rev>:<path>` is the object id git holds for that content.
    $expected = Invoke-Git -GitArgs @('rev-parse', 'origin/main:ci/merge-proof.ps1')
    if ($expected.exitCode -ne 0 -or @($expected.lines).Count -eq 0) {
        Write-Host ("[from-main] HARNESS-BROKE: origin/main has no ci/merge-proof.ps1. That is " +
            "the bootstrap state, and there is nothing trustworthy to run.") -ForegroundColor Magenta
        exit 1
    }
    $expectedBlob = ([string]@($expected.lines)[0]).Trim()

    $show = Invoke-Git -GitArgs @('show', 'origin/main:ci/merge-proof.ps1')
    if ($show.exitCode -ne 0) {
        Write-Host "[from-main] HARNESS-BROKE: could not read origin/main:ci/merge-proof.ps1." -ForegroundColor Magenta
        exit 1
    }

    $staging = Join-Path ([System.IO.Path]::GetTempPath()) ("merge-proof-from-main-" + [guid]::NewGuid().ToString('N'))
    [System.IO.Directory]::CreateDirectory($staging) | Out-Null
    $verifier = Join-Path $staging 'merge-proof.ps1'
    try {
        # LF, EXPLICITLY, and never `WriteAllLines`: that helper joins with `Environment.NewLine`,
        # which is CRLF here, so it reconstructs a file whose bytes differ from the blob on every
        # line. The blob check below caught exactly that during development -- which is the whole
        # argument for having it, since the CRLF copy still PARSES and still RUNS.
        #
        # BOM-less UTF8 for the other half of the same care: `Set-Content` defaults to the system
        # ANSI codepage on 5.1 and `Out-File` writes a BOM, and a BOM in front of `<#` is a parse
        # error in a file that was fine in the object store.
        $text = (@($show.lines) -join "`n")
        if ($text.Length -gt 0) { $text += "`n" }
        [System.IO.File]::WriteAllText($verifier, $text, (New-Object System.Text.UTF8Encoding($false)))

        # THE EXTRACTION IS VERIFIED, NOT ASSUMED, and this is the half a hand-typed procedure
        # skips. Reading a blob through PowerShell's pipeline decodes bytes as text, and a decode
        # that mangles a byte produces a file that still PARSES and still RUNS -- a predicate that
        # is almost main's. `--no-filters` because the working-tree filters are exactly what must
        # not be applied when reconstructing an object.
        $actual = Invoke-Git -GitArgs @('hash-object', '--no-filters', '--', $verifier)
        $actualBlob = ''
        if ($actual.exitCode -eq 0 -and @($actual.lines).Count -gt 0) {
            $actualBlob = ([string]@($actual.lines)[0]).Trim()
        }
        if (-not [string]::Equals($actualBlob, $expectedBlob, [System.StringComparison]::Ordinal)) {
            Write-Host ("[from-main] HARNESS-BROKE: the extracted verifier is not main's bytes " +
                "(expected blob $expectedBlob, got '$actualBlob'). Refusing rather than running a " +
                "predicate that is ALMOST the one on main.") -ForegroundColor Magenta
            exit 1
        }

        Write-Host ("[from-main] running origin/main:ci/merge-proof.ps1 (blob $expectedBlob) " +
            "against $root") -ForegroundColor Cyan

        $arguments = @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $verifier,
            '-PullRequest', "$PullRequest",
            '-RepositoryRoot', $root
        )
        if ($Head) { $arguments += @('-Head', $Head) }
        if ($LedgerDirectory) { $arguments += @('-LedgerDirectory', $LedgerDirectory) }
        if ($Json) { $arguments += '-Json' }

        # SPLATTED, never a single interpolated string. PowerShell drops an empty literal argument
        # while building a native command line, which is how `-Closes ''` died on one launcher out
        # of four in `ci/closing-keywords.ps1` (#757).
        & powershell.exe @arguments
        exit $LASTEXITCODE
    } finally {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    }
} finally {
    Pop-Location
}
