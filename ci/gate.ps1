<#
.SYNOPSIS
    Runs the complete GraphHelm verification gate locally.

.DESCRIPTION
    This is the authoritative gate. The project does not run hosted CI, so nothing verifies a change
    unless it is run here. Treat a red gate exactly as you would a red pipeline: do not merge.

    The gate is ordered cheapest-first so an obvious failure is reported early, but a failing stage
    does NOT stop the run: every stage executes regardless, so one invocation reports every failure
    rather than only the first. The script exits non-zero if any stage failed, zero if every stage
    passed - verified directly by running the exit-code logic against a forced failure and a clean
    pass (see the PR that closed https://github.com/stabem/GraphHelm/issues/97).

    ONE EXCEPTION TO "every stage runs regardless" (#152): the contamination canary runs FIRST and
    aborts the whole gate immediately on failure, before any other stage executes. A contaminated
    build environment (a shared CARGO_TARGET_DIR serving a stale binary from another worktree or
    commit instead of rebuilding) makes every other stage's result meaningless - there is nothing
    to gain by running 26 more stages against a build nobody can trust, and doing so would bury the
    one finding that actually matters under noise from its downstream symptoms.

    THE SAFE PATTERN, when you need to check the exit code: redirect the run to a file, check the
    exit code of THAT SAME COMMAND immediately, then read the file separately - never pipe the
    live run into anything if the exit code matters.
        From PowerShell:
            ./ci/gate.ps1 > gate.log 2>&1; echo "exit: $LASTEXITCODE"; Get-Content gate.log -Tail 50
        From bash (invoking the PowerShell host directly, no pipe on the invocation itself):
            powershell.exe -File ci/gate.ps1 > gate.log 2>&1; echo "exit: $?"; tail -50 gate.log
    Both read the exit code from the command that produced it, then inspect the FILE afterward -
    a completely separate step with no bearing on `$?`/`$LASTEXITCODE`, however it gets filtered.

    DO NOT PIPE THIS SCRIPT'S LIVE OUTPUT (e.g. `./ci/gate.ps1 | tail -50`,
    `powershell.exe -File ci/gate.ps1 | tail -50`) if you intend to check its exit code afterward.
    In bash/POSIX shells, `$?` after a pipe reflects the LAST command in the pipe (`tail`, here),
    never this script's - the exit code you read back is the pager's, not the gate's, and a
    genuinely red run reads as success. This is not a bug in this script; it is how pipes work,
    and no script on the producing end of one can fix it from the inside. (Issue #97, found live:
    the exact pipe-through-tail pattern above produced a RED banner with an apparently-successful
    exit status, twice in one day.)

    A branch cut before #100 carries the pre-#100 `Invoke-Stage` (the `& $Body` that swallows a
    native tool's own stdout - issue #97/#98's log-completeness finding) and will keep producing
    gates with no per-test failure text until it rebases onto a #100-or-later `main`, regardless of
    how the gate is invoked. A red gate on such a branch is real; its missing diagnostic text is not
    evidence of a new capture bug - check the branch's base before treating swallowed text as a
    fresh regression.

    RUN-MANIFEST (#152): every invocation writes `.factory/gate-runs/<HEAD12>-<timestamp>.json` -
    HEAD sha, a hash of the uncommitted diff (if any), every stage's PASS/FAIL and wall time, the
    path and content hash of every test binary `cargo`'s own `--message-format=json` reported, a
    freshness flag for any binary whose mtime predates this run's start (it did not actually
    rebuild - reused, not regenerated), and whatever this run observed in a shared CARGO_TARGET_DIR's
    SLOT.lock at start and end. A GREEN report with no well-formed manifest is RED by rule: the
    manifest failing to write is itself gate-failing, not a warning.

.PARAMETER SkipPostgres
    Skips the ignored PostgreSQL matrix. Use only when a change cannot touch persistence, and say so
    when reporting the result - a gate run without it is not a full gate.

.PARAMETER PostgresBin
    Passed through to ci/postgres.ps1 as GRAPHHELM_PG_BIN. Required unless PostgreSQL is discoverable
    on this machine.

.EXAMPLE
    ./ci/gate.ps1
    ./ci/gate.ps1 -PostgresBin 'C:\pgsql\bin'
    ./ci/gate.ps1 -SkipPostgres
#>
param(
    [switch] $SkipPostgres,
    [string] $PostgresBin
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$toolchain = '+1.97.1'
$failed = @()
$stageRecords = New-Object System.Collections.Generic.List[object]
$runStartUtc = [DateTime]::UtcNow

# Runs ci/postgres.ps1 as a fully detached child.
#
# Two hazards make the obvious invocations wrong. Calling it with `&` propagates its `exit` and
# terminates this script. Letting it inherit this script's output handles is worse: the PostgreSQL
# server it spawns inherits them too and holds them open, so if the gate's own output is redirected
# to a file the parent blocks forever on a stream that never closes. Giving the child explicit
# temporary files of its own closes both, and `WaitForExit` waits for that process alone rather than
# for its descendants - the server is stopped by postgres.ps1's own teardown before it returns.
function Invoke-Postgres {
    $hostExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh' } else { 'powershell' }
    $outFile = [System.IO.Path]::GetTempFileName()
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $process = Start-Process -FilePath $hostExe -PassThru -NoNewWindow `
            -ArgumentList @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass',
                '-File', (Join-Path $PSScriptRoot 'postgres.ps1')
            ) `
            -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        # Touching Handle caches it so ExitCode is readable after the wait. Without this the
        # property comes back empty and a passing run is misreported as a failure.
        $null = $process.Handle
        $process.WaitForExit()
        foreach ($file in @($outFile, $errFile)) {
            if (Test-Path -LiteralPath $file) {
                Get-Content -LiteralPath $file -ErrorAction SilentlyContinue |
                    ForEach-Object { Write-Host $_ }
            }
        }
        $global:LASTEXITCODE = $process.ExitCode
    } finally {
        Remove-Item -LiteralPath $outFile, $errFile -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-Stage {
    param([string] $Name, [scriptblock] $Body)

    Write-Host ''
    Write-Host "[gate] $Name" -ForegroundColor Cyan
    # Native tools write progress to stderr. Under Windows PowerShell 5.1 a redirected native stderr
    # line becomes a NativeCommandError that $ErrorActionPreference='Stop' promotes to a terminating
    # error, so a native process is judged by its exit code instead.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        # `& $Body` without capturing its result makes the native command's STDOUT part of THIS
        # FUNCTION'S OWN output stream (ordinary PowerShell function behaviour) - every call site
        # discards it with `| Out-Null`, so the tool's actual output (a test binary's own
        # `test foo ... ok/FAILED` lines, a compiler's stdout diagnostics) never reached the
        # console or a log at all, silently, on every stage. Piping through Write-Host here
        # forces it out immediately as its own side effect, decoupled from this function's return
        # value, so callers remain free to discard the numeric exit code without losing the tool's
        # own evidence of what happened (issue #97/#98's log-completeness finding). Also captured
        # into $capturedLines, live output unaffected - the manifest (#152) embeds the tail of
        # this on failure, so a reader sees the named assertion in the JSON itself, not only in a
        # console scrollback that may already be gone by the time anyone reads the manifest.
        $capturedLines = New-Object System.Collections.Generic.List[string]
        & $Body | ForEach-Object { $capturedLines.Add([string]$_); Write-Host $_ }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
        $stopwatch.Stop()
    }
    if ($code -ne 0) {
        Write-Host "[gate] FAILED: $Name (exit $code)" -ForegroundColor Red
        $script:failed += $Name
    }
    $record = [ordered]@{
        name         = $Name
        passed       = ($code -eq 0)
        exitCode     = $code
        wallTimeSecs = [math]::Round($stopwatch.Elapsed.TotalSeconds, 3)
    }
    if ($code -ne 0) {
        # Last 40 lines: enough to carry a panic site ("thread 'x' panicked at file:line") and its
        # message without embedding an entire compiler-error wall into every red manifest.
        $tailCount = [Math]::Min(40, $capturedLines.Count)
        $record.outputTail = @($capturedLines.GetRange($capturedLines.Count - $tailCount, $tailCount))
    }
    $script:stageRecords.Add($record)
    return $code
}

# #152: rewrites the canary's nonce so its build.rs reruns and re-hashes THIS run's src/ tree,
# even when nothing else under tools/ci-canary/src changed. Without this, a legitimately-unchanged
# canary crate would never rebuild at all under cargo's own caching, and the canary would only ever
# prove something the FIRST time it ran - every subsequent green would be trivially true regardless
# of contamination, which is exactly the vacuous-green shape this crate exists to rule out.
function Write-CanaryNonce {
    $noncePath = Join-Path $repositoryRoot 'tools\ci-canary\src\nonce.rs'
    $nonce = "$([DateTime]::UtcNow.ToString('o'))-$([guid]::NewGuid())"
    $content = "// #152: rewritten by ci/gate.ps1 before every gate run - see build.rs and src/lib.rs`n" +
        "// for what this forces and why. The value itself carries no meaning beyond ``this file`n" +
        "// changed this run``.`n" +
        "pub const RUN_NONCE: &str = `"$nonce`";`n"
    # No BOM: Set-Content -Encoding utf8 always prepends one under Windows PowerShell 5.1, which
    # this repo's .rs files never carry (#152's own trap-guard script hit the same thing on its
    # restore step - fixed there, applied here too rather than left as a second instance).
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($noncePath, $content, $utf8NoBom)
}

# #152: SLOT.lock is agent-managed discipline, not something this script owns the lifecycle of -
# it only ever READS whatever is there, at start and at end, as evidence for the manifest. Absent
# CARGO_TARGET_DIR (no shared target dir in play) there is no lock to read at all, and that absence
# is itself recorded rather than treated as an error.
function Read-SlotLockSnapshot {
    if (-not $env:CARGO_TARGET_DIR) {
        return [ordered]@{ present = $false; reason = 'CARGO_TARGET_DIR not set' }
    }
    $lockPath = Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'
    if (-not (Test-Path -LiteralPath $lockPath)) {
        return [ordered]@{ present = $false; reason = 'no SLOT.lock at CARGO_TARGET_DIR'; targetDir = $env:CARGO_TARGET_DIR }
    }
    # [System.IO.File]::ReadAllText, NOT Get-Content -Raw - caught live, the hard way: Get-Content
    # attaches PowerShell PROVIDER metadata (PSPath/PSParentPath/PSDrive/PSProvider) onto the
    # string it returns, and PSDrive.Provider.ImplementingType chains straight into .NET's own
    # reflection Type graph - which is enormous and heavily self-referential. Nothing about that
    # is visible from a plain Write-Host of the value (it PRINTS like an ordinary string); it only
    # surfaces once something tries to serialize the whole object, which is exactly what
    # ConvertTo-Json -Depth 8 does downstream. Manifested as a multi-minute hang with zero error
    # and zero output, reproduced by isolating each field of the manifest hashtable individually
    # until this one field, alone, was the difference between instant and un-returning. A plain
    # .NET file read carries no provider metadata at all, so there is nothing extra to walk.
    return [ordered]@{
        present   = $true
        targetDir = $env:CARGO_TARGET_DIR
        content   = if (Test-Path -LiteralPath $lockPath) { [System.IO.File]::ReadAllText($lockPath) } else { $null }
        observedAtUtc = [DateTime]::UtcNow.ToString('o')
    }
}

# #152: one `--no-run --message-format=json` pass over the whole workspace enumerates every test
# binary (unit-test binaries per crate, integration-test binaries per crate including each
# apps/cli/tests/*.rs suite) in one shot - hashing and mtime-checking them here, BEFORE the
# human-facing stages below run the same tests for real, means those later stages execute against
# binaries this pass already fingerprinted, not a second, potentially-different build. cargo's own
# build caching makes the later stages' own compilation a no-op reuse of exactly what got hashed
# here, so the fingerprint stays true to what actually ran.
function Get-TestArtifactManifest {
    # Same treatment Invoke-Stage gives every OTHER native call, needed here too: this cargo
    # invocation runs outside Invoke-Stage (its output is JSON to parse, not human text to
    # forward), so without this it inherits $ErrorActionPreference='Stop' directly and a native
    # stderr progress line becomes a terminating NativeCommandError before $LASTEXITCODE is ever
    # read - caught live (#152's own trap-guard script hit the identical bug on `cargo clean`
    # first, which is exactly why this got checked here too instead of assumed fine).
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $lines = cargo $toolchain test --workspace --all-features --locked --no-run --message-format=json 2>&1
        $buildExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    $artifacts = New-Object System.Collections.Generic.List[object]
    foreach ($line in $lines) {
        $parsed = $null
        try { $parsed = $line | ConvertFrom-Json -ErrorAction Stop } catch { continue }
        if (-not $parsed) { continue }
        if ($parsed.reason -ne 'compiler-artifact') { continue }
        if (-not $parsed.profile -or $parsed.profile.test -ne $true) { continue }
        if (-not $parsed.executable) { continue }
        $exePath = $parsed.executable
        $hash = $null
        $mtimeUtc = $null
        $freshBuild = $null
        if (Test-Path -LiteralPath $exePath) {
            $hash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
            $mtimeUtc = (Get-Item -LiteralPath $exePath).LastWriteTimeUtc
            # A binary whose mtime predates this run's start did not get regenerated by the build
            # above - it was REUSED, not rebuilt. The canary proves this for its own crate via
            # content, not timestamps; this is the coarser, timestamp-based cross-check that covers
            # the other packages the canary's own hash cannot see into.
            $freshBuild = ($mtimeUtc -ge $runStartUtc)
        }
        $artifacts.Add([ordered]@{
            package    = $parsed.package_id
            target     = $parsed.target.name
            executable = $exePath
            sha256     = $hash
            mtimeUtc   = if ($mtimeUtc) { $mtimeUtc.ToString('o') } else { $null }
            freshBuild = $freshBuild
        })
    }
    return [ordered]@{
        buildExitCode = $buildExit
        artifacts     = $artifacts
    }
}

function Write-RunManifest {
    # $Status: one of GREEN, RED, ABORTED-BY-CANARY - the house's PASS/FAIL/HARNESS-BROKE
    # discipline, plus the fourth value a contaminated build environment needs (orchestrator
    # ruling on #152): a reader must be able to tell "the gate ran and something failed" apart
    # from "the gate refused to trust its own environment and stopped before measuring anything" -
    # collapsing both into a bare RED would read as the same finding when they are not.
    param(
        [ValidateSet('GREEN', 'RED', 'ABORTED-BY-CANARY')]
        [string] $Status,
        [bool] $CanaryPassed,
        [object] $ArtifactManifest,
        [object] $SlotLockAtStart,
        [object] $SlotLockAtEnd
    )

    $manifestDir = Join-Path $repositoryRoot '.factory\gate-runs'
    New-Item -ItemType Directory -Force -Path $manifestDir | Out-Null

    $headSha = (git rev-parse HEAD).Trim()
    $diff = git diff HEAD
    $dirtyDiffHash = if ($diff) {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes(($diff -join "`n"))
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try { [System.BitConverter]::ToString($sha256.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant() }
        finally { $sha256.Dispose() }
    } else {
        $null
    }

    $staleArtifacts = @($ArtifactManifest.artifacts | Where-Object { $_.freshBuild -eq $false })

    $manifest = [ordered]@{
        status             = $Status
        headSha            = $headSha
        dirtyDiffHash      = $dirtyDiffHash
        runStartUtc        = $runStartUtc.ToString('o')
        runEndUtc          = [DateTime]::UtcNow.ToString('o')
        canaryPassed       = $CanaryPassed
        # [object[]] cast, NOT @() - caught live, reproduced in isolation before guessing: under
        # this machine's Windows PowerShell 5.1 (5.1.26100.9168), `@(<a System.Collections.
        # Generic.List[object] VARIABLE>)` throws "Argument types do not match" unconditionally
        # (StrictMode-independent, reproduced with plain hashtables, ordered hashtables, and
        # PSCustomObject items alike - not about what's IN the list). `@()` around a PIPELINE
        # result stays fine (see $staleArtifacts below, built from `| Where-Object`, untouched);
        # only wrapping the raw List[object] variable itself is the trigger. An explicit
        # [object[]] cast on the same variable works cleanly.
        stages             = [object[]]$stageRecords
        artifactBuildExit  = $ArtifactManifest.buildExitCode
        testArtifacts      = [object[]]$ArtifactManifest.artifacts
        staleArtifactCount = $staleArtifacts.Count
        staleArtifacts     = $staleArtifacts
        slotLockAtStart    = $SlotLockAtStart
        slotLockAtEnd      = $SlotLockAtEnd
        overallPassed      = ($script:failed.Count -eq 0) -and $CanaryPassed -and ($staleArtifacts.Count -eq 0)
    }

    $fileName = "$($headSha.Substring(0, 12))-$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')).json"
    $path = Join-Path $manifestDir $fileName
    # No BOM: caught live, third instance of the same Set-Content -Encoding utf8 hazard in this
    # file - a manifest read back with a strict JSON parser (Python's json.load, no -sig) rejects
    # a leading BOM outright, and this manifest exists specifically to be machine-read later.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($path, ($manifest | ConvertTo-Json -Depth 8), $utf8NoBom)
    return $path
}

$slotLockAtStart = Read-SlotLockSnapshot

Push-Location -LiteralPath $repositoryRoot
try {
    # #152: the canary runs FIRST and is the one stage that aborts the whole gate immediately
    # rather than accumulating alongside the rest - see the top-of-file rationale.
    Write-CanaryNonce
    $canaryCode = Invoke-Stage 'contamination canary (ci-canary)' {
        cargo $toolchain test -p ci-canary --locked
    }
    $canaryPassed = ($canaryCode -eq 0)
    if (-not $canaryPassed) {
        Write-Host ''
        Write-Host '[gate] ABORTING: the contamination canary failed. Every other stage below would' -ForegroundColor Red
        Write-Host '[gate] run against a build environment this run cannot trust - nothing past this' -ForegroundColor Red
        Write-Host '[gate] point is evidence of anything. See #152.' -ForegroundColor Red
        $emptyArtifacts = [ordered]@{ buildExitCode = $null; artifacts = @() }
        $manifestPath = Write-RunManifest -Status 'ABORTED-BY-CANARY' -CanaryPassed $false `
            -ArtifactManifest $emptyArtifacts -SlotLockAtStart $slotLockAtStart `
            -SlotLockAtEnd (Read-SlotLockSnapshot)
        Write-Host "[gate] manifest: $manifestPath" -ForegroundColor Cyan
        exit 1
    }

    # One build pass, enumerated and fingerprinted, ahead of the human-facing stages that reuse it.
    $artifactManifest = Get-TestArtifactManifest

    Invoke-Stage 'rustfmt' { cargo $toolchain fmt --all -- --check } | Out-Null
    Invoke-Stage 'clippy (deny warnings)' {
        cargo $toolchain clippy --workspace --all-targets --all-features --locked -- -D warnings
    } | Out-Null
    Invoke-Stage 'workspace tests' {
        cargo $toolchain test --workspace --all-features --locked
    } | Out-Null

    # DERIVED, not hand-maintained (#98): a hardcoded allowlist under-gates every new suite by
    # DEFAULT and silently - a new tests/*.rs file still runs inside `workspace tests` above, but
    # misses the isolated `--test <suite>` pass this loop exists to give, which is exactly what
    # catches cross-test interference (server-spawning/port-binding/tempdir suites, the common
    # shape here). Enumerating the directory means a new suite is gated the day it is born.
    #
    # Any exclusion must be a NAMED entry here, with a reason, so it is visible in the gate's own
    # output (below) rather than only inferable from a diff against the filesystem - an allowlist
    # that rots silently becomes a denylist nobody chose, which is the exact defect this replaces.
    $excludedSuites = @{
        # (none today - add 'suite_name' = 'reason' here if one is ever needed)
    }
    $suiteFiles = Get-ChildItem -LiteralPath (Join-Path $repositoryRoot 'apps\cli\tests') -Filter '*.rs' |
        Sort-Object -Property Name
    $suites = $suiteFiles | ForEach-Object { $_.BaseName } | Where-Object { -not $excludedSuites.ContainsKey($_) }
    Write-Host ''
    Write-Host "[gate] cli suites: $($suites.Count) discovered in apps/cli/tests/*.rs" -ForegroundColor Cyan
    foreach ($excluded in $excludedSuites.Keys) {
        Write-Host "[gate] cli suite EXCLUDED: $excluded - $($excludedSuites[$excluded])" -ForegroundColor Yellow
    }
    foreach ($suite in $suites) {
        Invoke-Stage "cli: $suite" {
            cargo $toolchain test -p graphhelm-cli --test $suite --locked
        } | Out-Null
    }

    Invoke-Stage 'schema catalog' {
        cargo $toolchain run --locked -q -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
    } | Out-Null
    Invoke-Stage 'schema baseline compatibility' {
        cargo $toolchain run --locked -q -p graphhelm-cli -- schema check `
            --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
    } | Out-Null
    Invoke-Stage 'schema conformance' {
        cargo $toolchain run --locked -q -p graphhelm-cli -- schema conformance `
            --catalog schemas/catalog.json --fixtures conformance/manifest.json
    } | Out-Null
    Invoke-Stage 'locked metadata' {
        cargo $toolchain metadata --locked --no-deps --format-version 1 | Out-Null
    } | Out-Null
    Invoke-Stage 'whitespace' { git diff --check } | Out-Null

    if ($SkipPostgres) {
        Write-Host ''
        Write-Host '[gate] PostgreSQL matrix SKIPPED - this is not a full gate.' -ForegroundColor Yellow
    } else {
        if ($PostgresBin) { $env:GRAPHHELM_PG_BIN = $PostgresBin }
        # postgres.ps1 ends in `exit`, which terminates the *calling* script in PowerShell, so it
        # must run as a child process or the gate dies here and never reports.
        Invoke-Stage 'PostgreSQL ignored matrix' { Invoke-Postgres } | Out-Null
        # The C locale makes text ordering identical to COLLATE "C", which is exactly the condition
        # under which collation-dependent ordering defects stay invisible. This second pass is the
        # regression guard for that class and is not optional.
        $previousLocale = $env:GRAPHHELM_PG_LOCALE
        try {
            # $IsWindows exists only in PowerShell Core, and under Set-StrictMode referencing it
            # on Windows PowerShell 5.1 is a terminating error. $env:OS is set on Windows in both.
            $env:GRAPHHELM_PG_LOCALE = if ($env:OS -eq 'Windows_NT') {
                'English_United States.1252'
            } else {
                'en_US.UTF-8'
            }
            Invoke-Stage 'PostgreSQL matrix under a non-C collation' { Invoke-Postgres } | Out-Null
        } finally {
            $env:GRAPHHELM_PG_LOCALE = $previousLocale
        }
    }
} finally {
    Pop-Location
}

# Freshness cross-check BEFORE the manifest write, deliberately - $Status and $failed must both
# be final by the time Write-RunManifest reads them, or a manifest written first would report
# GREEN for a run the freshness check was about to fail moments later (#152 review: this ordering
# bug was caught before the slot, not after - a manifest race is exactly the kind of thing that
# would have shipped quietly otherwise).
if ($artifactManifest -and $artifactManifest.artifacts) {
    $staleCount = @($artifactManifest.artifacts | Where-Object { $_.freshBuild -eq $false }).Count
    if ($staleCount -gt 0) {
        Write-Host ''
        Write-Host "[gate] FRESHNESS CROSS-CHECK: $staleCount test binary(ies) predate this run's start - see the manifest." -ForegroundColor Red
        $failed += 'binary freshness cross-check'
    }
}

$manifestPath = $null
$manifestFailed = $false
try {
    $status = if ($failed.Count -eq 0) { 'GREEN' } else { 'RED' }
    $manifestPath = Write-RunManifest -Status $status -CanaryPassed $true -ArtifactManifest $artifactManifest `
        -SlotLockAtStart $slotLockAtStart -SlotLockAtEnd (Read-SlotLockSnapshot)
} catch {
    # "Green without a well-formed manifest is red by rule" (#152) - a manifest that fails to
    # write is a gate failure in its own right, never a silent gap a clean stage run papers over.
    $manifestFailed = $true
    Write-Host "[gate] MANIFEST WRITE FAILED: $($_.Exception.Message)" -ForegroundColor Red
}

Write-Host ''
if ($manifestPath) {
    Write-Host "[gate] manifest: $manifestPath" -ForegroundColor Cyan
}
if ($manifestFailed) {
    $failed += 'run-manifest write'
}
if ($failed.Count -gt 0) {
    Write-Host "[gate] RED - failed stages: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host '[gate] GREEN - every stage passed.' -ForegroundColor Green
if ($SkipPostgres) {
    Write-Host '[gate] Reminder: the PostgreSQL matrix was skipped.' -ForegroundColor Yellow
}
exit 0
