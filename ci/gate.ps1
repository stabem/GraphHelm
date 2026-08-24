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

    ONE EXCEPTION TO "every stage runs regardless" (#152): the contamination canary is the first
    STAGE and aborts the whole gate immediately on failure, before any other stage executes. A
    contaminated build environment (a shared CARGO_TARGET_DIR serving a stale binary from another
    worktree or commit instead of rebuilding) makes every other stage's result meaningless - there
    is nothing to gain by running 26 more stages against a build nobody can trust, and doing so
    would bury the one finding that actually matters under noise from its downstream symptoms.
    (The TARGET-DIR REFUSAL below runs even earlier than the canary - it is a precondition on the
    whole script, not a stage, and is not counted among the 26.)

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

    TARGET-DIR REFUSAL (#166): the gate REFUSES to run at all unless `$env:CARGO_TARGET_DIR` is
    set to a per-lane target dir on D: matching `D:/graphhelm-target-<lane>` (ED-10) - a named,
    legible refusal before any stage runs, not a warning to remember. The disk-fullness decree
    this mechanizes was previously discipline only, and depending on every agent remembering,
    every session, on every command, is exactly the shape that already failed once (F: hit 100%
    from worktree-local builds). The door checks a PATTERN, not one shared literal - the single
    shared dir this originally required was retired for gate runs (ED-10, cross-lane
    contamination) before this ever merged, and the invariant the door protects (never
    worktree-local, never on F:) generalises to per-lane dirs cleanly. Isolated target dirs remain
    fully legitimate for individual cargo commands run directly, outside this script, during
    out-of-slot verification.

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

# Computed early, before the door below, so the door can check "not inside THIS worktree"
# structurally rather than guessing from a drive letter alone - a drive letter is itself an
# instance, not a type, and the orchestrator's #166 adjudication (ED-16) named that explicitly:
# the door's job is to reject target dirs by TYPE (undefined, inside a worktree, on F:), never to
# fix one instance, because the instance is exactly the part this factory keeps changing.
$repositoryRoot = Split-Path -Parent $PSScriptRoot

# #166: the target-dir decree turns from discipline into mechanism here - the thing that must
# not happen (a full gate run building worktree-local, or against some other isolated dir) stops
# being POSSIBLE, rather than depending on every agent remembering, every session, on every
# command. Half the factory once built worktree-local on F: while half built shared on D: with
# nobody having enumerated per session; the failure mode is SILENT until the disk fills, and then
# LNK1180 link failures read as five FAILED stages - HARNESS-BROKE masquerading as test-red.
#
# PATTERN, not one literal - amended before this ever merged, caught by H reading this branch
# against ED-10 (D:/graphhelm-slot/ENV-DECISIONS.md): the ORIGINAL version of this check demanded
# the exact single shared dir `D:/graphhelm-target-m10`. That dir is RETIRED for gate runs (ED-10:
# a shared dir across lanes produced a `cargo test` GREEN and then `cargo build` failing on a type
# it had just compiled against - cross-lane contamination the canary alone did not fully close).
# The convention moved to ONE TARGET DIR PER LANE, `D:/graphhelm-target-<lane>`, before this PR's
# merge could land - a hardcoded single literal would have refused every lane that followed the
# newer, safer convention. The INVARIANT this door protects was never "the one shared dir
# specifically" - it was always "never worktree-local, never on F:" (the actual disk-fullness
# cause). That invariant generalises cleanly to per-lane dirs; the literal did not, and got
# updated instead of the invariant.
#
# gate.ps1 is the SLOT-HOLDER's tool specifically. Running the FULL gate against a worktree-local
# or F:-resident dir leaves build waste on the drive that has none to spare (the exact
# disk-fullness failure mode above). Isolated dirs for INDIVIDUAL cargo commands run OUTSIDE
# gate.ps1, for out-of-slot verification, stay entirely legitimate - exactly the pattern #152's
# own slot-free work used throughout, correctly. This check is stricter than "any cargo command
# anywhere" on purpose: it is the full gate's own gate, not a blanket rule for every invocation of
# cargo on this machine.
#
# Refuses BEFORE any other setup, before Push-Location, before the canary - the trap-guard
# requirement (#166's own issue text): the refusal must be legible in the log as a NAMED reason,
# not just a nonzero exit code, because an exit code alone cannot separate "refused at the door"
# from "ran and failed" (the same distinction #97 existed to draw for the gate's own exit code).
$targetDirPattern = '^D:/graphhelm-target-[^/]+$'
$actualTargetDir = $env:CARGO_TARGET_DIR
if (-not $actualTargetDir) {
    Write-Host '[gate] REFUSED: CARGO_TARGET_DIR is not set.' -ForegroundColor Red
    Write-Host '[gate] The full gate must run against a per-lane target dir on D:, never the' -ForegroundColor Red
    Write-Host '[gate] worktree-local default - the mechanized form of the target-dir decree' -ForegroundColor Red
    Write-Host '[gate] (#166, per-lane convention ED-10). Set it explicitly and rerun, e.g.:' -ForegroundColor Red
    Write-Host "[gate]     `$env:CARGO_TARGET_DIR = 'D:/graphhelm-target-<lane>'" -ForegroundColor Red
    exit 1
}
# -match is case-INSENSITIVE by default in PowerShell (`-cmatch` is the case-sensitive form) -
# left as-is deliberately, not by accident of which operator got typed: this path lives on an NTFS
# volume, where 'D:/graphhelm-target-f166' and 'D:/GraphHelm-Target-F166' are the SAME directory
# on disk, so rejecting a differently-cased spelling of an identical path would be a false
# refusal, not a stricter one.
#
# WHY case is tolerated and nothing wider is: not generosity in the comparator - the disk does not
# distinguish it, so a caller who spells the path with different casing is not pointing at a
# LOOKALIKE dir, they are pointing at the SAME directory, bytes and all. Anyone verifying this
# door by running the actual full gate (rather than testing the check in isolation) is not
# exercising an edge case in isolation - they are running a real gate against a real target dir,
# possibly contending with whoever else is building there. (Caught live during #166's own review,
# back when this compared against the one shared dir: a differently-cased alias let the door pass
# correctly, and the script went on to build for real while another agent's gate was mid-run
# there. No target-dir tolerance is a safe thing to poke at by running this script for real - test
# the door in isolation, e.g. by extracting just this check into its own process.)
$normalizedActual = $actualTargetDir.TrimEnd('/', '\').Replace('\', '/')
if ($normalizedActual -notmatch $targetDirPattern) {
    Write-Host "[gate] REFUSED: CARGO_TARGET_DIR is '$actualTargetDir', not a per-lane target dir" -ForegroundColor Red
    Write-Host "[gate] on D: (expected shape: D:/graphhelm-target-<lane>, ED-10). The full gate is" -ForegroundColor Red
    Write-Host '[gate] the slot-holder''s tool - running it worktree-local or on F: leaves build' -ForegroundColor Red
    Write-Host '[gate] waste on a drive that has none to spare (#166). Isolated dirs for individual' -ForegroundColor Red
    Write-Host '[gate] cargo commands stay legitimate run OUTSIDE gate.ps1, out-of-slot (#152''s own' -ForegroundColor Red
    Write-Host '[gate] pattern) - not for the full gate itself. Set it explicitly and rerun, e.g.:' -ForegroundColor Red
    Write-Host "[gate]     `$env:CARGO_TARGET_DIR = 'D:/graphhelm-target-<lane>'" -ForegroundColor Red
    exit 1
}
# NAMED EXCLUSION, not a side effect of the regex: `^D:/graphhelm-target-[^/]+$` would otherwise
# happily accept `D:/graphhelm-target-m10` itself - the exact dir ED-10 retires for gate runs
# (cross-lane contamination). The pattern's job is to enforce SHAPE (residual gap named by H,
# confirmed by the orchestrator: a regex can fail by accepting what it should refuse, not only by
# rejecting what it should accept - the failure mode differs from the exact-match check this
# replaced). Closing it by name here rather than leaving "the pattern happens to still allow the
# retired value" as an unstated side effect: the day two agents both reach for the old, familiar
# name out of habit, they share a dir again, which is the exact thing ED-10 exists to prevent.
#
# THIS IS A HAND-MAINTAINED LIST, one entry today, and the orchestrator named the tension
# honestly rather than let it surface unexplained in a month: it is the same instance-vs-type
# problem ED-16 raised, now on the negative side. The NEXT retired dir has to be added here by
# hand, or this check goes quietly stale - incomplete, not visibly wrong, exactly the failure
# shape ED-16 warned about.
#
# CONSIDERED AND DECLINED: a derived check reading a tombstone marker (a file whose first line is
# "RETIRED PATH" or similar) inside the candidate dir, which would cover future retirements
# without an edit here. Declined for now, not because it is a bad idea: it moves this door from a
# pure SPELLING question (does the string match a shape? - fully deterministic, testable in
# isolation as every check above was) to a WORLD question (does a file exist right now, what does
# it contain right now? - can fail from a missing file, a race, a permission error, none of which
# a string comparison can). That tombstone convention also is not yet an established, documented
# contract of its own (ED-16's own file happens to be shaped that way for this one retirement,
# not a general "how a target dir gets retired" protocol) - coupling the gate's own door to an
# incidental naming choice from an unrelated saga (the SLOT.lock relocation) would be borrowing
# stability from something that has not been asked to provide it yet. If retirements become
# frequent enough that a hand-maintained list is the actual pain point, revisit this decision with
# a stabilized tombstone convention behind it - not before.
if ($normalizedActual -eq 'D:/graphhelm-target-m10') {
    Write-Host "[gate] REFUSED: CARGO_TARGET_DIR is 'D:/graphhelm-target-m10' - retired for gate" -ForegroundColor Red
    Write-Host '[gate] runs (ED-10: shared dir across lanes produced a `cargo test` GREEN and then a' -ForegroundColor Red
    Write-Host '[gate] `cargo build` failing on a type it had just compiled against). Pick a per-lane' -ForegroundColor Red
    Write-Host '[gate] name instead, e.g.:' -ForegroundColor Red
    Write-Host "[gate]     `$env:CARGO_TARGET_DIR = 'D:/graphhelm-target-<lane>'" -ForegroundColor Red
    exit 1
}
# Structural, not coincidental: the drive-letter check above already excludes THIS machine's
# current worktree layout (every worktree lives under F:), but a drive letter is an instance fact
# about today's disk layout, not a guarantee. Checking directly against $repositoryRoot - the
# worktree this very invocation of gate.ps1 is running from - is the type-level version of "not
# worktree-local" the door is actually supposed to enforce, independent of which drive happens to
# hold worktrees today. Scope: this worktree specifically, not every worktree on the machine -
# pointing at ANOTHER agent's worktree is a different, much stranger misconfiguration than the
# one this door exists to catch (an agent building inside its own worktree).
$normalizedRepoRoot = $repositoryRoot.TrimEnd('/', '\').Replace('\', '/')
if ($normalizedActual.StartsWith("$normalizedRepoRoot/", [System.StringComparison]::OrdinalIgnoreCase) -or
    $normalizedActual.Equals($normalizedRepoRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    Write-Host "[gate] REFUSED: CARGO_TARGET_DIR '$actualTargetDir' is inside this worktree" -ForegroundColor Red
    Write-Host "[gate] ($repositoryRoot). A per-lane dir must sit OUTSIDE any worktree (ED-10) -" -ForegroundColor Red
    Write-Host '[gate] worktree-local is the exact disk-fullness failure mode this door exists to' -ForegroundColor Red
    Write-Host '[gate] prevent (#166). Set it explicitly and rerun, e.g.:' -ForegroundColor Red
    Write-Host "[gate]     `$env:CARGO_TARGET_DIR = 'D:/graphhelm-target-<lane>'" -ForegroundColor Red
    exit 1
}

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

# #199: the slot directory, which is deliberately OUTSIDE any cargo target dir.
#
# Measured on 2026-08-20, not designed around: SLOT.lock used to live inside CARGO_TARGET_DIR, and
# `cargo clean` wipes that directory - so the one operation that most needs exclusion was exactly
# the operation that destroyed the proof you held it. Two agents collided at ~15:41Z with neither
# breaking a rule: one ran the clean the slot law requires and deleted their own lock, the other
# verified a genuine absence and claimed the slot. Anything that must OUTLIVE a clean therefore
# lives here and never under a target dir.
#
# Overridable so a machine with a different layout can point it elsewhere; the default is the path
# the factory already uses for SLOT.lock and check-activity.log.
function Get-SlotDir {
    if ($env:GRAPHHELM_SLOT_DIR) { return $env:GRAPHHELM_SLOT_DIR }
    return 'D:/graphhelm-slot'
}

# #199: one line per gate event, appended, never rewritten.
#
# The lock answers "who holds it now" and CANNOT answer "how many times did it change hands
# today" - every acquisition overwrites the last, so the history is destroyed by the act of using
# the instrument. That number is exactly what a throughput question needs, and on 2026-08-20 it was
# unanswerable in the one instance where it mattered. This is the append-only half the lock cannot
# provide, in the same shape as `check-activity.log`.
#
# Best-effort by construction: a gate run must not fail because a log line could not be written.
# The failure is REPORTED rather than swallowed, because a silent logging failure would leave the
# same hole this exists to close.
function Write-SlotEvent {
    param([Parameter(Mandatory)] [string] $Event, [string] $Detail = '')

    try {
        # .NET APIs, NOT New-Item/Join-Path. Found by this function's own negative control, run live
        # against an unreachable drive before any of this was trusted -- and the finding is narrower
        # than it first looked, so it is stated narrowly: under this file's own
        # `$ErrorActionPreference = 'Stop'` the cmdlets throw and this catch reports the real cause,
        # so THERE IS NO LIVE BREAK on the shipped path. Under 'Continue' -- which `Invoke-Stage`
        # sets for the duration of every native call -- they fail NON-TERMINATINGLY instead: two raw
        # DriveNotFoundExceptions print, `Join-Path` returns $null, and the catch finally fires on
        # "Value cannot be null", a true message about the wrong cause. So the cmdlet version is
        # correct only while nobody calls this from inside a stage. `Directory.CreateDirectory` and
        # `Path.Combine` throw regardless of the ambient preference, which makes the behaviour a
        # property of this function rather than of its caller.
        $slotDir = Get-SlotDir
        [System.IO.Directory]::CreateDirectory($slotDir) | Out-Null
        $line = '{0} | gate | {1} | {2}' -f [DateTime]::UtcNow.ToString('o'), $Event, $Detail
        # AppendAllText, not Add-Content: the same no-BOM discipline the rest of this file uses,
        # and an append that cannot silently re-encode a file other processes also append to.
        [System.IO.File]::AppendAllText([System.IO.Path]::Combine($slotDir, 'SLOT.log'), $line + "`n", (New-Object System.Text.UTF8Encoding($false)))
    } catch {
        Write-Host "[gate] WARNING: could not append to SLOT.log: $($_.Exception.Message)" -ForegroundColor Yellow
    }
}

# #200: Read-SlotLockSnapshot moved to ci/slot-lock.ps1 (with Test-SlotLockPathMatchesTargetDirShape
# and Test-SlotLockSnapshotsIdentical) so these functions can be unit-tested in isolation - see
# ci/slot-lock.tests.ps1 and .factory/e-agent-200-design.md for the full account of why a boolean
# `present` field could not tell "genuinely no lock" apart from "looked in the wrong place", and why
# that distinction is now a `status` tag with three states instead of two.
. (Join-Path $PSScriptRoot 'slot-lock.ps1')

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
        # `--all-features` is load-bearing and unguarded here too - see the note on the
        # 'workspace tests' stage below. Pointer, not a copy: it carries nothing that can drift.
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

    # #199: what the status does NOT say, said out loud.
    #
    # `status: RED` records that a run failed and nothing about whether the failure means anything.
    # Three classes look identical in an exit code and only the first is evidence about code:
    #   real-red       - the code failed something
    #   instrument-red - stale artifacts, a contaminated target dir, a broken manifest write
    #   dead           - killed by a lock collision, a clean underneath it, an aborted slot
    # Measured 2026-08-20: one lane produced three runs - one of each - and ZERO verdicts between
    # them, while a census counting "two complete runs, both RED" could not tell them apart.
    #
    # GREEN classifies itself: there is no ambiguity in a run that passed. RED cannot, because the
    # distinction is a judgement only the holder can make once they have read the failure - so it
    # ships as UNCLASSIFIED, which is a legal and VISIBLE value rather than a silent absence, and
    # `ci/classify-run.ps1` is how the holder settles it.
#
# DERIVED FROM THE STRONGER VERDICT, NOT FROM `$Status` -- and the first version of this line got it
# backwards, which made `instrument-red` UNREACHABLE for the one instrument failure the gate detects
# by itself. `$Status` counts failed STAGES only. The manifest already carries a stricter verdict,
# `overallPassed`, which also weighs the canary and stale artifacts. So a run whose ONLY problem was
# stale artifacts came out `GREEN` -> `green`, and `classify-run.ps1` REFUSES anything not
# `UNCLASSIFIED` -- while the same manifest said `overallPassed: false` with a non-zero
# `staleArtifactCount`. Unclassifiable, and counted as a pass by any census reading `runClass`.
#
# Stale artifacts are the FIRST EXAMPLE in this file's own definition of `instrument-red`. Deriving
# the new field from the WEAKER of two verdicts the manifest already holds imports exactly the
# flattening the field was added to remove. (Found in review by L Agent, against `2b5396e`.)
$passedEverything = ($script:failed.Count -eq 0) -and $CanaryPassed -and ($staleArtifacts.Count -eq 0)
$runClass = if ($Status -eq 'GREEN' -and $passedEverything) { 'green' } else { 'UNCLASSIFIED' }

# #199: "was the INSTRUMENT broken?" is DERIVED, never chosen -- and it is born HERE, beside the
# numbers it is computed from, so it cannot disagree with them. Not "they agree today": they have no
# way to disagree.
#
# It stopped being a CLASS because real data would not fit. A Agent's `gate.log` failed `rustfmt`
# and `clippy` -- CODE -- and carried FOUR stale binaries -- INSTRUMENT -- in the same run. Both true
# at once, and an exclusive class forces one label, which erases half of what happened; the erased
# half is precisely the half that decides whether the red is citable. So `instrument-red` is gone
# from the class set, and `-Class` is left for what needs a human: `real-red` / `dead`.
#
# It is NOT computed in `classify-run.ps1`, deliberately: a value calculated where a run is
# CLASSIFIED would attach itself to old manifests nobody measured at the time -- the same defect as
# stamping a class onto a pre-taxonomy manifest, one layer up.
#
# ABSENT IS NOT FALSE. A manifest written before this change has no `instrumentSuspect` at all, and
# that means NOT MEASURED -- never "the instrument was healthy".
$instrumentSuspect = ($staleArtifacts.Count -gt 0) -or (-not $CanaryPassed)

    # #199: "is it real?" and "is it MINE?" are orthogonal, so they are two fields and not four
    # classes. Found by the first real case the taxonomy met (#166's gate, 2026-08-20): it went RED
    # on a flake in `runtime_http.rs`, a file that lane never touched. By the letter that is
    # `real-red` -- code failed something -- and it says NOTHING about the diff under judgement.
    # A fourth class would have forced that run to be either mis-labelled or mis-attributed.
    #
    # $null, not $true: the gate cannot know. Attribution is a judgement about a DIFF, and the only
    # thing here that knows the diff is the person reading the failure. `ci/classify-run.ps1` sets
    # it, and refuses to set it FALSE without the three conditions that make non-attribution
    # checkable rather than convenient.
    $relatedToDiff = $null

    $manifest = [ordered]@{
        status             = $Status
        runClass           = $runClass
        relatedToDiff      = $relatedToDiff
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
        # Gate 9 (#200, L): the pair above was already captured specifically so it COULD be
        # compared, and nothing did the comparing - a reader had to notice on their own that two
        # fields existed to diff. A match is a tripwire worth surfacing, not a determination: an
        # untouched, stale lock reads identical by construction, but so would a real hold nobody
        # touched for the whole run. See Test-SlotLockSnapshotsIdentical in ci/slot-lock.ps1.
        slotLockStartEndIdentical = Test-SlotLockSnapshotsIdentical -Start $SlotLockAtStart -End $SlotLockAtEnd
        instrumentSuspect  = $instrumentSuspect
        overallPassed      = $passedEverything
    }

    $fileName = "$($headSha.Substring(0, 12))-$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')).json"
    $path = Join-Path $manifestDir $fileName
    # No BOM: caught live, third instance of the same Set-Content -Encoding utf8 hazard in this
    # file - a manifest read back with a strict JSON parser (Python's json.load, no -sig) rejects
    # a leading BOM outright, and this manifest exists specifically to be machine-read later.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $json = $manifest | ConvertTo-Json -Depth 8
    [System.IO.File]::WriteAllText($path, $json, $utf8NoBom)

    # #199: the SAME bytes, outside the repository.
    #
    # The in-repo copy is the committable evidence and stays. It is also only visible to anyone if
    # a human chooses to commit it, and it dies with its branch - so "how many runs happened today"
    # was answerable today only as a LOWER BOUND. This copy is written unconditionally, outside any
    # worktree and outside any target dir, so a run leaves a trace whether or not anybody decides
    # it is worth keeping.
    #
    # Best-effort, and REPORTED on failure: a durable-copy problem must not fail a gate run, and
    # must not be silent either.
    try {
        # .NET APIs for the same reason as Write-SlotEvent: a cmdlet that fails non-terminatingly
        # prints its own error and hands the catch a misleading one.
        $durableDir = [System.IO.Path]::Combine((Get-SlotDir), 'gate-runs')
        [System.IO.Directory]::CreateDirectory($durableDir) | Out-Null
        [System.IO.File]::WriteAllText([System.IO.Path]::Combine($durableDir, $fileName), $json, $utf8NoBom)
    } catch {
        Write-Host "[gate] WARNING: could not write the durable manifest copy: $($_.Exception.Message)" -ForegroundColor Yellow
    }

    Write-SlotEvent -Event 'RUN-END' -Detail "status=$Status class=$runClass head=$($headSha.Substring(0, 12)) manifest=$fileName"
    return $path
}

$slotLockAtStart = Read-SlotLockSnapshot

# #199: the START half of the append-only pair. Written BEFORE any stage runs, so a run that dies
# without ever reaching `Write-RunManifest` - killed by a lock collision, a clean underneath it, an
# aborted slot - still leaves a line. A START with no matching RUN-END IS the record of a dead run,
# and that class was previously invisible: it produces no manifest at all, so a count of manifests
# counts only the runs that survived to write one.
Write-SlotEvent -Event 'RUN-START' -Detail "targetDir=$($env:CARGO_TARGET_DIR) cwd=$repositoryRoot"

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
        # `--all-features` is load-bearing and unguarded here too - see the note on the
        # 'workspace tests' stage below. Pointer, not a copy.
        cargo $toolchain clippy --workspace --all-targets --all-features --locked -- -D warnings
    } | Out-Null
    # `--all-features` is LOAD-BEARING HERE, and UNGUARDED. This comment buys PLACEMENT, not
    # enforcement: it fires only if whoever narrows these flags happens to read it.
    #
    # WHAT DEPENDS ON IT: every `[[test]]` target in the workspace declared with
    # `required-features`, in ANY crate's Cargo.toml. Do not trust a list here - `git grep -n
    # "required-features" -- "*/Cargo.toml"` is the population, and it is the only form of this
    # sentence that cannot rot. Today that grep returns two, both in
    # adapters/postgres-event-store: `concurrency` and `repository_conformance`, behind
    # `test-support`, which is off by default (no `default` feature). They run ONLY because this
    # line asks for every feature. Narrow the flags for speed and they stop running: no error, no
    # skip line, and "0 tests" from a target that never built reads exactly like a target with
    # nothing to run.
    #
    # The protection is INCIDENTAL: the flag is here to compile everything, not to cover
    # `required-features`. The mechanical version - assert the per-test lines of every
    # `required-features` target appear in the run, with the target list DERIVED from the manifests
    # and never hand-maintained - is filed separately; a hand-maintained list would shrink in
    # silence exactly as #98's allowlist did, which is also why the population above is a grep and
    # not two names.
    #
    # STATED ONCE, and the boundary is reasoned rather than forgotten. The other `--all-features`
    # sites carry a one-line pointer instead of a copy: a pointer holds no content that can drift.
    # `ci/postgres.ps1` is deliberately NOT among them - its default run is `-- --ignored`, and
    # measured on origin/main those two targets carry 19 tests and ZERO `#[ignore]`, so narrowing
    # the flags there loses compilation and not coverage. A grep for `--all-features` finds five
    # sites; this sentence is how you tell a considered boundary from a missed one.
    #
    # NOT MEASURED: nobody has observed those targets being skipped. This is a named fragility with
    # a named trigger, not an observed defect.
    # --no-fail-fast, because without it a single failing binary aborts the run and the verdict
    # goes RED without recording how much of the suite never executed. Measured on two consecutive
    # cold gates: one truncated after 9 binaries, the next after 23, and in BOTH a lane's nineteen
    # named guards never ran at all — verified name by name, not inferred. A RED that stopped
    # looking is not the same object as a RED that looked at everything, and before this flag the
    # two printed the same word (#238).
    #
    # THEY STILL CAN, and this comment would mislead without the next sentence: the flag stops
    # CARGO aborting on a failing binary. It does not stop the stage ending early from a harness
    # abort, a timeout, a killed process or a crash — and in those cases the manifest is shaped
    # exactly like a complete run. Coverage becomes visible in the LOG here; it becomes visible in
    # the RECORD only with #238's executed-vs-discovered field. (Caught reviewing #239: a PR body
    # is read once at merge, this line is read by whoever touches it next.)
    Invoke-Stage 'workspace tests' {
        cargo $toolchain test --workspace --all-features --locked --no-fail-fast
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
