<#
.SYNOPSIS
    Trap guard for #152, run BEFORE trusting the canary anywhere else: proves the canary's fixture
    is constructable and that a real contamination replay reds ON THE CANARY'S OWN ASSERTION
    (named panic site), not an adjacent error — the vacuous-red rule this repo has been burned by
    before (#96-era: a stale binary reused silently instead of rebuilt).

.DESCRIPTION
    Armed by an explicit switch, never bare invocation — the #149 pattern (SABOTAGE_CONFIRM_ENV),
    translated to this script's own shape: a destructive replay that swaps binaries under
    CARGO_TARGET_DIR must never fire from an accidental `./replay-contamination.ps1`.

    Steps, each observed and printed, not assumed:
      1. Clean build+test of ci-canary alone (sanity: the crate works at all).
      2. Preserve that binary's bytes aside — this is "the stale binary" the replay plants back.
      3. Change src/ (rewrite nonce.rs) WITHOUT rebuilding — the disk now disagrees with the
         preserved binary's baked-in hash, exactly the contamination shape.
      4. Run the PRESERVED (stale) binary directly, bypassing cargo's own rebuild decision, and
         confirm it panics on `the_running_binary_matches_the_src_tree_on_disk` specifically, with
         "CONTAMINATION" in the message — not a different test, not a build error, not a generic
         panic. That specificity is the whole point: proof by construction that detection power
         exists, not just that something red happened.
      5. Restore nonce.rs, clean up the preserved copy.

.PARAMETER Confirm
    Required. Refuses to run any destructive step without it.

.EXAMPLE
    ./tools/ci-canary/replay-contamination.ps1 -Confirm
#>
param(
    [switch] $Confirm
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

if (-not $Confirm) {
    Write-Host '[replay] Refusing: pass -Confirm. This script deliberately plants a stale binary.' -ForegroundColor Yellow
    exit 1
}

$repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$toolchain = '+1.97.1'
$noncePath = Join-Path $PSScriptRoot 'src\nonce.rs'
# Explicit UTF8 (no BOM) throughout, both read and write - caught live, not assumed: Windows
# PowerShell 5.1's `Get-Content -Raw` without -Encoding reads as the system codepage, not UTF-8,
# so the committed file's em-dashes came back mis-decoded; `Set-Content -Encoding utf8` then
# re-encodes that already-corrupted string AND adds a BOM Rust source in this repo never carries.
# [System.IO.File]::ReadAllText/WriteAllText with an explicit no-BOM UTF8Encoding sidesteps both -
# the restore this script does in its own `finally` block must reproduce the original byte-for-
# byte, not a mangled approximation of it.
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$originalNonce = [System.IO.File]::ReadAllText($noncePath, [System.Text.Encoding]::UTF8)
$preservedBinary = $null
$replaySucceeded = $false

function Write-Nonce([string] $Value) {
    $content = "// #152: rewritten by ci/gate.ps1 before every gate run - see the comment this replaced for why.`npub const RUN_NONCE: &str = `"$Value`";`n"
    [System.IO.File]::WriteAllText($noncePath, $content, $utf8NoBom)
}

# Native tools write progress to stderr; under Windows PowerShell 5.1 a redirected native stderr
# line becomes a NativeCommandError that $ErrorActionPreference='Stop' promotes to a terminating
# error before the command's own exit code is ever consulted (gate.ps1's Invoke-Stage exists for
# exactly this - this script needed the same treatment and did not have it on its first run: cargo
# clean's own "Removed N files" summary line, printed to stderr, terminated the script here before
# it reached a single real assertion). Judge every native call by its exit code, never by whether
# PowerShell decided its stderr was an error.
function Invoke-Native {
    param([scriptblock] $Body)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Body
    } finally {
        $ErrorActionPreference = $previous
    }
}

try {
    Push-Location -LiteralPath $repositoryRoot
    try {
        Write-Host '[replay] Step 1: clean baseline build+test of ci-canary' -ForegroundColor Cyan
        Write-Nonce "replay-baseline-$([guid]::NewGuid())"
        Invoke-Native { cargo $toolchain clean -p ci-canary } | ForEach-Object { Write-Host $_ }
        if ($LASTEXITCODE -ne 0) { throw "cargo clean -p ci-canary failed (exit $LASTEXITCODE)" }

        $rawMessages = Invoke-Native { cargo $toolchain test -p ci-canary --locked --no-run --message-format=json }
        $noRunExit = $LASTEXITCODE
        $messages = $rawMessages | ForEach-Object { $_ | ConvertFrom-Json -ErrorAction SilentlyContinue }
        if ($noRunExit -ne 0) { throw "cargo test -p ci-canary --no-run failed (exit $noRunExit)" }
        # Matched on package_id, not target.name: cargo reports the TARGET name with hyphens
        # turned to underscores (ci_canary, the Rust identifier), while package_id always carries
        # the real Cargo.toml package name (ci-canary) verbatim in its path - caught live on the
        # first run of this exact line, which is exactly why this is a comment now, not a repeat.
        $artifact = $messages | Where-Object {
            $_.reason -eq 'compiler-artifact' -and $_.package_id -match 'ci-canary' -and $_.profile.test -eq $true -and $_.executable
        } | Select-Object -Last 1
        if (-not $artifact -or -not $artifact.executable) {
            throw 'could not locate the built ci-canary test executable in --message-format=json output'
        }
        $builtExe = $artifact.executable
        Write-Host "[replay] Built test binary: $builtExe" -ForegroundColor Cyan

        # tests::-qualified: a #[test] fn inside `mod tests { ... }` carries its enclosing
        # module's name in libtest's own naming, not the bare fn name - the bare name is exactly
        # the mistake that produced "0 tests ... 1 filtered out" (vacuously GREEN, nothing
        # actually ran) the first time this line was written, caught only because the LATER stale-
        # binary check also expected a real failure and got a suspicious green instead. Fixed at
        # the source here rather than papered over downstream.
        $testName = 'tests::the_running_binary_matches_the_src_tree_on_disk'
        $sanity = Invoke-Native { & $builtExe --exact $testName 2>&1 }
        $sanityExit = $LASTEXITCODE
        $sanityCombined = $sanity -join "`n"
        if ($sanityExit -ne 0) {
            throw "sanity run of a freshly-built, uncontaminated binary FAILED (exit $sanityExit) - the fixture itself is broken, fix that before anything else:`n$sanity"
        }
        if ($sanityCombined -notmatch '1 passed') {
            throw "sanity run reported exit 0 but did not actually run the test (vacuous pass - check the --exact name matches libtest's own naming):`n$sanityCombined"
        }
        Write-Host '[replay] Sanity PASS: a freshly-built binary reports clean, as expected.' -ForegroundColor Green

        Write-Host '[replay] Step 2: preserving the clean binary as "the stale one"' -ForegroundColor Cyan
        $preservedBinary = Join-Path ([System.IO.Path]::GetTempPath()) "ci-canary-stale-$([guid]::NewGuid()).exe"
        Copy-Item -LiteralPath $builtExe -Destination $preservedBinary

        Write-Host '[replay] Step 3: changing src/ WITHOUT rebuilding (the contamination shape)' -ForegroundColor Cyan
        Write-Nonce "replay-post-change-$([guid]::NewGuid())"

        Write-Host '[replay] Step 4: running the STALE (preserved) binary against the CHANGED tree' -ForegroundColor Cyan
        $output = Invoke-Native { & $preservedBinary --exact $testName 2>&1 }
        $exitCode = $LASTEXITCODE
        $combined = $output -join "`n"

        $caughtOnOwnAssertion = ($combined -match 'the_running_binary_matches_the_src_tree_on_disk') -and
            ($combined -match 'CONTAMINATION')

        if ($exitCode -eq 0) {
            Write-Host '[replay] TRAP GUARD FAILED: the stale binary reported GREEN against a changed tree.' -ForegroundColor Red
            Write-Host $combined
            exit 1
        }
        if (-not $caughtOnOwnAssertion) {
            Write-Host '[replay] TRAP GUARD FAILED (vacuous red): the binary failed, but NOT on the named canary assertion. This proves nothing about detection power.' -ForegroundColor Red
            Write-Host $combined
            exit 1
        }

        Write-Host '[replay] TRAP GUARD CONFIRMED: contamination replay reds on the canary''s own named assertion.' -ForegroundColor Green
        Write-Host $combined
        # Explicit, not inherited: without this the script's own exit code would be whatever the
        # STALE BINARY's failing test run happened to return (101) - correct in spirit (something
        # failed) but wrong in meaning (the REPLAY succeeded; a caller checking this script's exit
        # code needs "did the trap guard confirm detection power", not "did the last native tool
        # exit non-zero").
        $script:replaySucceeded = $true
    } finally {
        Pop-Location
    }
} finally {
    [System.IO.File]::WriteAllText($noncePath, $originalNonce, $utf8NoBom)
    if ($preservedBinary -and (Test-Path -LiteralPath $preservedBinary)) {
        Remove-Item -LiteralPath $preservedBinary -Force -ErrorAction SilentlyContinue
    }
    Write-Host '[replay] nonce.rs restored, preserved binary removed.' -ForegroundColor Cyan
}

if ($replaySucceeded) { exit 0 }
exit 1
