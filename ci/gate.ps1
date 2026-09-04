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

    TARGET-DIR REFUSAL (#166): the gate REFUSES to run at all unless `$env:CARGO_TARGET_DIR` has
    an explicit nonblank value - a named, legible refusal before any stage runs, not a warning to
    remember. Validation is deliberately SET-ONLY, not an exact path or naming rule: isolated fresh
    targets are part of the authoritative gate workflow, and the run manifest records the exact
    path used for audit. This door mechanizes the missing-variable failure without rejecting valid
    isolation strategies owned by the gate runner.

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

# #166: refuse before any setup, Push-Location, canary, or stage banner. SET-ONLY is intentional:
# callers use isolated fresh targets to prevent cross-lane contamination, and the run manifest
# records the chosen path. This precondition prevents the silent worktree-local DEFAULT caused by
# an unset variable; it does not replace the runner's target allocation policy with a path regex.
$actualTargetDir = $env:CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($actualTargetDir)) {
    Write-Host '[gate] REFUSED: CARGO_TARGET_DIR is unset or blank.' -ForegroundColor Red
    Write-Host '[gate] Set an explicit target directory before running the authoritative gate.' -ForegroundColor Red
    Write-Host '[gate] Isolated fresh targets are allowed; the run manifest records the exact path (#166).' -ForegroundColor Red
    exit 1
}

function ConvertTo-SlotEventField {
    param([AllowEmptyString()] [string] $Value)

    if ($null -eq $Value) { return '' }
    return [System.Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($Value))
}

$toolchain = '+1.97.1'
$failed = @()
$stageRecords = New-Object System.Collections.Generic.List[object]
$runStartUtc = [DateTime]::UtcNow

# Evidence can contain connection strings printed by a failing test. Redact before the line reaches
# either Write-Host (the human gate log) or $capturedLines (the machine-readable manifest). Keep
# this pure so PowerShell 5.1 and PowerShell Core apply exactly the same substitutions.
function Protect-GateEvidenceLine {
    param([AllowEmptyString()] [string] $Line)

    $protected = [regex]::Replace(
        $Line,
        '(?i)\b(postgres(?:ql)?://[^:\s/@]+:)([^@\s]+)(@)',
        '$1***$3'
    )
    $protected = [regex]::Replace(
        $protected,
        '(?i)\b(PGPASSWORD|password|passwd|pwd)(\s*[:=]\s*)([^\s;]+)',
        '$1$2***'
    )
    $protected = [regex]::Replace(
        $protected,
        '(?i)(--pwfile(?:=|\s+))(?:"[^"]+"|''[^'']+''|\S+)',
        '$1[REDACTED_SECRET_FILE]'
    )
    $protected = [regex]::Replace(
        $protected,
        '(?i)(?:"[^"\r\n]*initdb\.pwfile"|''[^''\r\n]*initdb\.pwfile''|(?:[A-Za-z]:[\\/]|/)[^\s;]*initdb\.pwfile)',
        '[REDACTED_SECRET_FILE]'
    )
    return $protected
}

# Runs ci/postgres.ps1 as a fully detached child.
#
# Two hazards make the obvious invocations wrong. Calling it with `&` propagates its `exit` and
# terminates this script. Letting it inherit this script's output handles is worse: the PostgreSQL
# server it spawns inherits them too and holds them open, so if the gate's own output is redirected
# to a file the parent blocks forever on a stream that never closes. Giving the child explicit
# temporary files of its own closes both, and `WaitForExit` waits for that process alone rather than
# for its descendants - the server is stopped by postgres.ps1's own teardown before it returns.
function Invoke-Postgres {
    param(
        # Focused harness tests inject a deterministic child without starting PostgreSQL or the
        # cargo matrix. Production callers omit this and retain the checked-in postgres.ps1.
        [string] $ScriptPath = (Join-Path $PSScriptRoot 'postgres.ps1')
    )

    $hostExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh' } else { 'powershell' }
    $outFile = [System.IO.Path]::GetTempFileName()
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $process = Start-Process -FilePath $hostExe -PassThru -NoNewWindow `
            -ArgumentList @(
                '-NoProfile', '-ExecutionPolicy', 'Bypass',
                '-File', $ScriptPath
            ) `
            -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        # Touching Handle caches it so ExitCode is readable after the wait. Without this the
        # property comes back empty and a passing run is misreported as a failure.
        $null = $process.Handle
        $process.WaitForExit()
        foreach ($file in @($outFile, $errFile)) {
            if (Test-Path -LiteralPath $file) {
                # Bound each child stream before publishing it. The manifest applies its tighter
                # combined 40-line tail below; the human log gets at most 80 stdout plus 80 stderr
                # lines from this detached stage instead of an unbounded cargo/test transcript.
                Get-Content -LiteralPath $file -Tail 80 -ErrorAction SilentlyContinue |
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
        # Invoke-Postgres publishes its detached child's files with Write-Host so that reading
        # them cannot become this function's numeric return value. Write-Host is information
        # stream 6 in Windows PowerShell 5.1 and PowerShell Core. Merge only that stream into the
        # success stream here, then redact once before both evidence sinks. Without 6>&1 the child
        # is visible at best in the console but absent from outputTail (#484).
        & $Body 6>&1 | ForEach-Object {
            $line = Protect-GateEvidenceLine -Line ([string]$_)
            $capturedLines.Add($line)
            Write-Host $line
        }
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
. (Join-Path $PSScriptRoot 'manifest-name.ps1')
. (Join-Path $PSScriptRoot 'run-class.ps1')

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

function Get-RecordedStatus {
    <#
        The status the RECORD carries, given what the run computed and whether the head moved.

        Extracted so it can be fed. It was two operands of an `if` inside `Write-RunManifest`, which
        drives the whole gate and cannot be run from a cell -- so the comparison that decides what
        the durable record says about a moved head had no input that could tell an ordinal comparer
        from a culture-aware one. A reviewer measured that: swapping all eleven comparisons in this
        file to InvariantCulture reddens exactly ONE cell. That does not mean ten are untested; it
        means no cell hands them an input that DISTINGUISHES the two comparers, which is a gap in
        inputs rather than in lines.

        AND THE EXACTNESS IS THE POINT, in the direction that needs saying. A status of 'GREEN' plus
        an ignorable code point is not downgraded here, and that is deliberate: it is not a status
        this gate can produce, so the honest record keeps the odd value where somebody can see it
        rather than laundering it into a tidy 'RED'. The RUN is red either way -- movement adds its
        own entry to the failure list and sets the script-scope flag, neither of which passes
        through this function -- so nothing about the verdict rests on the downgrade. What rests on
        it is whether a reader of the store can tell "this gate called it red" from "something wrote
        a status this gate cannot emit".
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [AllowNull()] [string] $Status,
        [Parameter(Mandatory)] [bool] $HeadMoved
    )

    if ($HeadMoved -and [string]::Equals($Status, 'GREEN', [System.StringComparison]::Ordinal)) { return 'RED' }
    return $Status
}

function Get-HeadProvenance {
    <#
        Where this run's head can be found later, if anywhere.

        A gate run records `headSha`, and a squash merge THROWS THAT COMMIT AWAY -- so a manifest
        that names only the head cannot certify anything that reaches main. Measured on this
        repository: of the heads certified on 2026-09-01, six of seven do not exist on the server
        at all, because the gate ran on a commit that was then amended or rebased before any push.

        Two fields fix that, and they answer different halves:

          pullRequest   the number survives the squash, in the merge title's `(#N)`, so it is the
                        only key that connects a commit on main back to the head that was gated
          pushed        TRUE when the SERVER was asked and answered that this commit is the tip
                        of the branch this run tracked, and $null in every other case. Never read
                        out of `refs/remotes/*`: that is what the last fetch left behind. A
                        certified sha that dies in the next amend certifies NOTHING, and a reader
                        treating a cached ref as proof is worse off than with no proof, because it
                        looks like compliance

        Both are recorded as facts, never inferred: when a value cannot be determined it is `$null`
        with `reason` saying which lookup failed. An absent field and a field that says "nobody
        could tell" are different states, and only the second is honest here.
    #>
    param(
        [Parameter(Mandatory)] [string] $HeadSha,
        # The branch this run gated, captured before the stages. See the note at the lookup below:
        # a SHA-based movement check cannot see a checkout onto a different branch at the same
        # commit, so a fresh read here would look up somebody else's pull request while every guard
        # stayed quiet.
        [string] $BranchRef
    )

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # THE CAPTURED BRANCH'S UPSTREAM, not "the current branch's". A bare `@{upstream}` resolves
        # against whatever HEAD names now, so a sibling branch at the same commit -- the case the
        # SHA guards are blind to by construction -- makes this answer about somebody else's
        # tracking configuration. Another consumer of identity that the table missed, and it was
        # missed the same way as the pull-request lookup: the read is spelled without a subject, so
        # it reads as if it had one.
        # SHORT NAME, measured rather than assumed: `@{upstream}` resolves against a branch NAME, and
        # the fully-qualified form is not one.
        #   master@{upstream}            -> refs/remotes/origin/master
        #   refs/heads/master@{upstream} -> fatal: no such branch
        # Declared BEFORE the read, not after it: the first version put this beside the pull-request
        # locals further down, which run LATER in the function and quietly reset it to null -- the
        # assignment happened and then was undone. A cell caught it; reading the code did not.
        $upstreamLocalRef = $null
        $upstreamRev = if ($BranchRef) { ($BranchRef -replace '^refs/heads/', '') + '@{upstream}' } else { '@{upstream}' }
        # AND THESE TWO READS KEEP GIT'S WORDS TOO. A reviewer on another machine hit a case where
        # NOTHING here resolves -- every cell that needs an upstream failed, every cell that does not
        # passed -- and the record said only `pushed: null, "this branch tracks no remote branch"`.
        # That sentence is what this function CONCLUDED, not what it OBSERVED: "no upstream is
        # configured", "the tracking ref is missing" and "git could not read the config at all"
        # arrive identically once stderr is dropped, and they are three different repairs.
        $upstreamOutput = @(& git rev-parse $upstreamRev 2>&1 | ForEach-Object { [string]$_ })
        $upstreamExit = $LASTEXITCODE
        $upstream = @($upstreamOutput | Where-Object { $_ -cmatch '^[0-9a-f]{40}$' } | Select-Object -First 1)
        $upstreamSha = if ($upstreamExit -eq 0 -and $upstream) { ([string]$upstream).Trim() } else { $null }
        # Kept for the reason string below: on the failing path this is the only evidence of WHY.
        $upstreamWhy = if ($upstreamExit -ne 0) {
            "git rev-parse $upstreamRev exited $upstreamExit and said: " + (($upstreamOutput | Select-Object -First 2) -join ' | ')
        } else { $null }

        # AND AN UPSTREAM IS ONLY EVIDENCE OF A PUSH IF IT LIVES ON A REMOTE. `branch.<name>.merge`
        # can point at another LOCAL branch, and `pushed: true` then certified a commit that never
        # left the machine -- the field exists to answer "did the server get this", and a local
        # tracking relationship answers a different question entirely.
        $upstreamRefOutput = @(& git rev-parse --symbolic-full-name $upstreamRev 2>&1 | ForEach-Object { [string]$_ })
        $upstreamRefExit = $LASTEXITCODE
        $upstreamRef = @($upstreamRefOutput | Where-Object { $_ -cmatch '^refs/' } | Select-Object -First 1)
        $upstreamIsRemote = ($upstreamRefExit -eq 0 -and ([string]$upstreamRef).Trim() -cmatch '^refs/remotes/')
        if ($upstreamRefExit -ne 0 -and -not $upstreamWhy) {
            $upstreamWhy = ("git rev-parse --symbolic-full-name $upstreamRev exited $upstreamRefExit and said: " +
                (($upstreamRefOutput | Select-Object -First 2) -join ' | '))
        }
        # The ref itself is kept, because the remote NAME and the branch name on the server are read
        # out of it below: `pushed` is answered by asking that server, not by reading its cache here.
        $upstreamRemoteRef = if ($upstreamIsRemote) { ([string]$upstreamRef).Trim() } else { $null }
        if ($null -ne $upstreamSha -and -not $upstreamIsRemote) {
            # Not silently dropped: the equality fast path below must not fire, and the reason has to
            # say why a configured upstream did not count.
            $upstreamLocalRef = ([string]$upstreamRef).Trim()
            $upstreamSha = $null
        }

        # `gh` may be absent, unauthenticated or offline, and none of those is a gate failure --
        # this is provenance, not a verdict. A number that cannot be looked up is recorded as
        # unknown rather than guessed from the branch name, which would be a second spelling of
        # something the server already knows.
        $prNumber = $null
        $prReason = $null
        # THE CAPTURED BRANCH, not a fresh read -- and this is the consumer my table missed. The
        # movement check compares SHAs, so another shell checking out a different local branch that
        # points at the SAME commit leaves it false while this lookup asks about the branch the
        # operator is standing on now. The record then names a pull request belonging to a branch
        # this run never gated, and every SHA-based guard says nothing is wrong, because nothing
        # about the SHA is.
        #
        # Identity captured once has to feed every consumer that uses it. Feeding only the publisher
        # left the provenance reading a different clock.
        $branch = if ($BranchRef) { $BranchRef -replace '^refs/heads/', '' } else { $null }
        if (-not $branch) {
            $prReason = 'detached HEAD when this run started: no branch to look a pull request up by'
        } else {
            # THE TOOL BEING UNUSABLE AND THE ANSWER BEING "NONE" ARE DIFFERENT STATES, and a
            # single non-zero exit conflates them: `gh pr view` fails the same way when there is no
            # pull request and when it is missing, unauthenticated or offline. A consumer reading
            # one reason for both cannot tell "this branch has no PR" from "nobody could look".
            # So the tool is asked whether it can answer at all, first.
            # RESOLVE THE COMMAND BEFORE RUNNING IT. With `gh` absent, `&` does not launch a
            # process and does not set an exit code: it throws CommandNotFoundException, which
            # `2>$null` does not silence (the command never ran, so that text is not its stderr) and
            # which no `catch` here stops -- the try below has only a `finally`. Measured: the line
            # reading `$LASTEXITCODE` is never reached, the error propagates out of this function,
            # and the manifest write goes with it -- `Write-RunManifest` never returns, the caller's
            # catch sets `$manifestFailed`, and the gate exits RED for "run-manifest write". That
            # contradicts this function's own docstring, which says an absent gh is not a gate
            # failure.
            #
            # HISTORY, not present tense: with the `Get-Command` check below, none of that happens
            # any more. The paragraph is kept because it says why the check exists, not what the
            # code does today -- a comment describing behaviour that has since been fixed is a
            # defect this repository keeps finding, so it is labelled rather than left to rot.
            #
            # And the escape depends on the CALLER, which is worth recording because two
            # measurements disagreed about it: called from inside a try -- the production path,
            # through `Write-RunManifest` -- the error propagates. Called at top level with no
            # enclosing try, as an extracted-function harness does, the statement is abandoned and
            # the function still RETURNS, with the reason unset. Same function, different structure,
            # and only the first is what the gate does.
            if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
                $prReason = 'gh is not installed here: nobody could look'
            } else {
            # SCOPED TO THE HOST THIS QUESTION IS ABOUT. Bare `gh auth status` reports on EVERY host
            # gh knows, so an expired token for some unrelated enterprise host made the preflight fail
            # and the pull request was recorded as "nobody could look" while github.com was perfectly
            # reachable. An instrument that answers about the wrong subject is not a conservative
            # instrument; it is a wrong one, and this one fails toward silence.
            # AND THE ACTIVE ACCOUNT, not any account on the host. `gh auth status --hostname` exits
            # non-zero when ANY account configured for that host has an expired token, including one
            # that will not be used -- so a stale second login suppressed the lookup while the
            # account that would actually answer was fine. Sibling of the host scoping, one level in:
            # the preflight has to ask about the identity that will do the work.
            #
            # `--active` where the installed gh has it, and the "Active account: true" block where it
            # does not -- the flag is recent enough that assuming it would make the preflight fail on
            # older installs, which is the same failure this is fixing.
            $authProbe = @(& gh auth status --hostname github.com --active 2>&1 | ForEach-Object { [string]$_ })
            $authOk = ($LASTEXITCODE -eq 0)
            if (-not $authOk -and (($authProbe -join "`n") -cmatch 'unknown flag|unrecognized|--active')) {
                $authProbe = @(& gh auth status --hostname github.com 2>&1 | ForEach-Object { [string]$_ })
                # Without the flag, read the block that names the active account: a non-zero exit
                # here can be entirely about an account nobody is going to use.
                $authOk = (($authProbe -join "`n") -cmatch 'Active account: true')
            }
            if (-not $authOk) {
                $prReason = 'gh is unavailable or not authenticated for the active github.com account: nobody could look'
            } else {
                # `pr list`, not `pr view`. A LIST answers with an empty array and exit 0 when
                # there is nothing, and fails only when the lookup itself failed -- so the two
                # states arrive on different signals instead of sharing one non-zero exit. With
                # `pr view` an offline blip and a branch with no pull request are the same event,
                # and the `gh auth status` preflight only narrows that, it does not separate it.
                # `--head` FILTERS BY BRANCH NAME ONLY -- gh's own help says the `<owner>:<branch>`
                # form is unsupported -- so two forks using `fix-thing`, or a branch renamed since
                # the pull request was opened, both answer with somebody else's pull request. `.[0]`
                # then discarded the ambiguity in silence, and the manifest recorded a number that
                # names a different head: the provenance chain this field exists to carry, pointing
                # somewhere else.
                #
                # So the head decides, not the branch name. `headRefOid` is requested and the
                # candidates are filtered by it, and anything other than exactly one match is a
                # REFUSAL with its reason rather than a choice among strangers.
                # AND THE CANDIDATE SET IS NOT PINNED TO THE BRANCH NAME EITHER. Filtering the
                # candidates by `headRefOid` decided correctly AMONG them, but `--head <name>`
                # chooses who is in the room: a branch renamed after its pull request was opened
                # returns an EMPTY list, and empty read as "this branch has no open pull request".
                # Same fail-open as before, one layer further out.
                #
                # So the head is searched for as well, and the two answers are pooled before the
                # `headRefOid` filter runs. A search that fails is not fatal -- it narrows the set
                # back to what the branch name found, which is what this did before.
                $lookup = & gh pr list --head ([string]$branch).Trim() --state open --json number,headRefOid 2>$null
                $byBranchFailed = ($LASTEXITCODE -ne 0)
                $searchLookup = & gh pr list --search $HeadSha --state open --json number,headRefOid 2>$null
                $bySearchFailed = ($LASTEXITCODE -ne 0)
                # A FAILED QUERY IS NOT AN ANSWER -- but it is also not a reason to throw away the
                # other one. The search is the only query that can find a pull request whose branch
                # was renamed, so a successful-but-empty `--head` beside a FAILED search does not
                # establish that no pull request exists. If what survived still names this head,
                # that is a real answer; if it does not, the honest word is "nobody could look",
                # never "this branch has no open pull request".
                $anyQueryFailed = ($byBranchFailed -or $bySearchFailed)
                $whichFailed = if ($byBranchFailed -and $bySearchFailed) { 'both queries' }
                    elseif ($byBranchFailed) { 'the branch-name query' }
                    elseif ($bySearchFailed) { 'the head search' } else { $null }
                if ($byBranchFailed -and $bySearchFailed) {
                    $prReason = 'gh could not complete the lookup: nobody could look'
                } else {
                    if ($byBranchFailed) { $lookup = $searchLookup }
                    elseif (-not $bySearchFailed) { $lookup = @($lookup) + @($searchLookup) }
                    # `@(ConvertFrom-Json)` DOES NOT ENUMERATE. Under Windows PowerShell 5.1 the
                    # cmdlet emits a JSON array as ONE object, so `@(...)` wrapped the whole array
                    # in a single-element array: the count was 1 whatever the answer held, and no
                    # candidate ever matched. Measured, not reasoned -- a two-element response came
                    # back with `count=1` and `matching=0`. The pipeline enumerates it properly.
                    # Two answers, so two documents: each is parsed on its own and the results are
                    # pooled by pull request number. Joining the texts would produce `[...][...]`,
                    # which is not JSON at all.
                    $candidates = @()
                    $parseFailed = $false
                    foreach ($doc in @($lookup | Where-Object { ([string]$_).Trim() -ne '' })) {
                        try {
                            $parsedJson = ([string]$doc) | ConvertFrom-Json
                            # `@(ConvertFrom-Json)` DOES NOT ENUMERATE under Windows PowerShell 5.1:
                            # the cmdlet emits a JSON array as ONE object, so the wrapper produced a
                            # single-element array holding the whole array and no candidate ever
                            # matched. Measured, not reasoned. The pipeline enumerates it properly.
                            $candidates += @($parsedJson | ForEach-Object { $_ })
                        } catch { $parseFailed = $true }
                    }
                    # A DOCUMENT THAT DOES NOT PARSE IS A FAILED QUERY. Two queries run here, and
                    # truncated JSON from one of them was being absorbed as long as the other
                    # answered: the pool looked complete, and "no open pull request" or a match got
                    # recorded from half the evidence. This is my own rule about instruments --
                    # a failed instrument is UNKNOWN, never an answer -- applied to the case where
                    # only part of the instrument failed.
                    if ($parseFailed) {
                        $anyQueryFailed = $true
                        $whichFailed = if ($whichFailed) { "$whichFailed and a lookup that did not parse" }
                            else { 'a lookup that did not parse' }
                    }
                    if ($parseFailed -and $candidates.Count -eq 0) {
                        $candidates = $null
                        $prReason = "gh answered a document that did not parse as JSON, so nobody could look: $lookup"
                    } else {
                        $candidates = @($candidates | Group-Object -Property number | ForEach-Object { $_.Group[0] })
                    }
                    if ($null -ne $candidates) {
                        if ($candidates.Count -eq 0) {
                            $prReason = if ($anyQueryFailed) { "gh could not complete $whichFailed, so nobody could look" }
                                else { 'gh answered: this branch has no open pull request' }
                        } else {
                            # ORDINAL, BECAUSE -ceq IS NOT. PowerShell's case-sensitive operators are still CULTURE
                            # aware, and a culture comparison gives some code points no weight at all: measured on this
                            # machine, 'GREEN' plus U+FFFD, plus U+FE00, or plus U+00AD each comes back -ceq 'GREEN'.
                            # THE CLASS IS "IGNORABLE TO THE COMPARER", NOT "INVISIBLE". Measured, because
                            # the difference decides what a reader does next: U+00AD, U+200D, U+2060,
                            # U+FE00, U+FEFF and U+FFFD all fold; U+200B, which is the first character
                            # anyone thinks of, does NOT (-ceq False). Control: 'GREENX' is false under
                            # both comparers. Calling the class "invisible" invites a deny-list of
                            # invisible characters -- which would miss U+FFFD, include U+200B for
                            # nothing, and be the population defect this file already carries a fix for.
                            # Every one of these comparisons decides something -- which pull request this head belongs
                            # to, whether the ref still holds the commit this run is about to certify, whether the
                            # manifest on disk is the one that was published -- and each was deciding it with a comparer
                            # that treats different strings as the same string. Found in ci/merge-proof.ps1 first, where
                            # a status of GREEN plus a weightless code point read as GREEN; this file has the same
                            # operator in ten places, and one of them is a compare-and-swap.
                            $matching = @($candidates | Where-Object {
                                    [string]::Equals([string]$_.headRefOid, $HeadSha, [System.StringComparison]::Ordinal) })
                            if ($matching.Count -eq 1) {
                                $parsed = 0
                                if ([int]::TryParse(([string]$matching[0].number).Trim(), [ref] $parsed)) { $prNumber = $parsed }
                                else { $prReason = "gh answered something that is not a number: $($matching[0].number)" }
                            } elseif ($matching.Count -eq 0) {
                                $prReason = if ($anyQueryFailed) {
                                    ("$($candidates.Count) open pull request(s) were found and none names $HeadSha, but " +
                                        "$whichFailed failed -- so nobody could look where the answer may be")
                                } else {
                                    ("$($candidates.Count) open pull request(s) use this branch name and none of them " +
                                        "names $HeadSha, so none of them is the pull request this run gated")
                                }
                            } else {
                                $prReason = ("$($matching.Count) open pull requests name $HeadSha on this branch name, so which " +
                                    'one this run belongs to cannot be decided here')
                            }
                        }
                    }
                }
            }
            }
        }
    } finally {
        $ErrorActionPreference = $previous
    }

    # NO UPSTREAM IS NOT "NOT PUSHED". `git push origin HEAD:feature` without `-u` puts the commit
    # on the server and leaves no tracking branch, so an equality test against a missing upstream
    # reported a sha that IS on the remote as unpushed -- the same "unknown read as an answer" this
    # function exists to avoid, in its own load-bearing field.
    #
    # So when there is no upstream the question is asked directly: does any REMOTE ref contain this
    # commit? Containment is the right test here and equality is not, because the question has
    # changed -- not "is this the tip the remote tracks" but "does the server have this commit at
    # all". Where an upstream DOES exist, equality still decides: an ancestor test there would say
    # yes for every unpushed commit on a tracked branch.
    # ONE QUESTION, ONE SEARCH. `pushed` asks whether the SERVER has this commit -- nothing more --
    # and there are two ways to know: the upstream is exactly it, or some remote ref contains it.
    # The first two versions of this had a containment search in each branch, which made the
    # branches interchangeable: removing one fell through to the other and the sabotage proved
    # nothing. Written as one condition and one search, each part can be removed and seen.
    #
    # EQUALITY IS THE FAST PATH AND NOT THE TEST. It answers yes exactly when the upstream IS this
    # commit; it must never answer NO on its own, because a tracked branch is not the only route
    # (`git push origin HEAD:review` leaves the upstream behind) and a missing upstream is not an
    # absent commit (`git push origin HEAD:feature` without -u). An ancestor test is wrong for both:
    # it says yes for every unpushed commit on a tracked branch.
    # AND A LOCAL REF IS A MEMORY OF THE SERVER, NOT A READING OF IT. `refs/remotes/*` is what the
    # last successful fetch left behind, so both paths this replaces certified `pushed: true` from a
    # cache: delete or force-push the upstream afterwards and the local ref still equals $HeadSha
    # while the server no longer has that commit anywhere. Because #674(b) refuses a manifest whose
    # `pushed` is not TRUE, a stale true is not a stale note -- it is PERMISSION TO MERGE, granted on
    # evidence this machine cannot see.
    #
    # It is the same shape as the empty-search note that used to close this block: an empty local
    # search is not absence at the server, AND a present local ref is not presence at the server.
    # Same instrument, the other half; the note covered only the half I was looking at.
    #
    # So the fast path becomes the whole test, and it asks the server. `ls-remote` returns TIPS, and
    # "the upstream's tip IS this commit" is exactly a tip question. Containment -- "some ref out
    # there contains this commit" -- is NOT answerable by tips; answering it needs a fetch or a
    # server API, and neither belongs in the gate's path for a provenance field. That half descends
    # to the third state rather than being answered from the cache.
    #
    # `pushed` therefore has two values and one meaning: TRUE means the server was asked and said
    # this commit is the tip of the branch this run tracked. Everything else is $null -- nobody could
    # tell -- which #674(b) already treats as a question rather than as a certificate.
    # The bound on the one network call this function makes, read INSIDE it: the cells extract this
    # function by anchor text and run it alone, so a script-scope variable declared above the anchor
    # would exist in the gate and be $null in every cell -- and Wait-Job with a null timeout does not
    # fail loudly, it waits.
    #
    # AND THE ENVIRONMENT CANNOT REINSTATE THAT NULL. `[int]'abc'` under `Continue` writes an error
    # and leaves the assignment UNDONE, so a malformed override produced exactly the $null the
    # paragraph above defends against, by a different route: the variable was moved inside the
    # function so the cells could not leave it null, and the operator could still hand it one.
    # `0` IS LEGAL AND IS A SEAM, not an oversight: `Wait-Job -Timeout 0` returns immediately, which
    # is how `gate-manifest-provenance.tests.ps1` forces the expiry branch deterministically instead
    # of sleeping through a real bound. The first version of this refusal rejected it and reddened
    # that cell, which is the cell teaching the fix what the parameter is for. A NEGATIVE value is
    # refused, because it is not a bound at all.
    # (Found by the GraphHelm ISSUES 4 lane reviewing this pull request; narrowed by its own suite.)
    $lsRemoteTimeoutSeconds = 30
    if ($env:GATE_LS_REMOTE_TIMEOUT_SECONDS) {
        $override = 0
        if (-not [int]::TryParse($env:GATE_LS_REMOTE_TIMEOUT_SECONDS, [ref] $override) -or $override -lt 0) {
            throw ("GATE_LS_REMOTE_TIMEOUT_SECONDS is [$($env:GATE_LS_REMOTE_TIMEOUT_SECONDS)], which is not a " +
                'whole number of seconds. Unset it to use the default of 30, or give it one (0 expires ' +
                'immediately and is what the suite uses to reach the timeout branch).')
        }
        $lsRemoteTimeoutSeconds = $override
    }
    $pushedReason = $null
    $pushed = $null
    $localMemory = @(& git for-each-ref --contains $HeadSha --format '%(refname)' refs/remotes 2>$null |
            Where-Object { $_ })
    $localNote = if ($localMemory.Count -gt 0) {
        " (a local ref, $($localMemory[0]), names it, but that is memory from the last fetch)"
    } else { '' }

    if ($null -eq $upstreamRemoteRef) {
        $where = if ($upstreamLocalRef) { "the upstream is $upstreamLocalRef, a LOCAL branch, which says nothing about the server" }
            elseif ($upstreamWhy) { "the upstream of $BranchRef could not be resolved ($upstreamWhy)" }
            else { 'this branch tracks no remote branch' }
        $pushedReason = "$where, so there was no server to ask$localNote"
    } elseif ($upstreamRemoteRef -cnotmatch '^refs/remotes/([^/]+)/(.+)$') {
        $pushedReason = ("the upstream ref $upstreamRemoteRef does not name a remote and a branch, so there was no " +
            "server to ask$localNote")
    } else {
        $remoteName = $Matches[1]
        $remoteBranch = $Matches[2]
        # BOUNDED, AND UNABLE TO ASK FOR CREDENTIALS. An unbounded network call in the path that
        # publishes the record is a hang where a refusal belongs, and a credential prompt on a
        # headless runner is the same hang wearing a question mark. A timeout is not an answer: it is
        # the third state, like every other lookup in this function.
        $tip = $null
        $tipError = $null
        $job = Start-Job -ScriptBlock {
            param($root, $remote, $branch)
            $env:GIT_TERMINAL_PROMPT = '0'
            $env:GCM_INTERACTIVE = 'never'
            $out = & git -C $root ls-remote $remote ('refs/heads/' + $branch) 2>&1
            [ordered]@{ code = $LASTEXITCODE; lines = @($out | ForEach-Object { [string]$_ }) }
        } -ArgumentList @((Get-Location).Path, $remoteName, $remoteBranch)
        $finished = Wait-Job -Job $job -Timeout $lsRemoteTimeoutSeconds
        if ($null -eq $finished) {
            # `Stop-Job` ends the job's own PowerShell process; `git ls-remote` is a NATIVE child of
            # that process, so a git blocked on a network read can outlive it. This repository knows
            # that shape by name -- #714, #715, #717, #748 are all a descendant surviving a stop --
            # and the record says so rather than implying the process is gone, because one orphaned
            # `git ls-remote` per timed-out run accumulates on a machine that gates all day.
            Stop-Job -Job $job -ErrorAction SilentlyContinue
            $tipError = ("the server did not answer within $lsRemoteTimeoutSeconds seconds (the " +
                'lookup was abandoned; a git process may still be running)')
        } else {
            $result = Receive-Job -Job $job
            if ($null -eq $result -or $result.code -ne 0) {
                $firstLine = if ($null -ne $result -and @($result.lines).Count -gt 0) { @($result.lines)[0] } else { 'no output' }
                $tipError = "asking the server failed: $firstLine"
            } else {
                $answer = @(@($result.lines) | Where-Object { $_ -cmatch '^[0-9a-f]{40}\s' })
                if ($answer.Count -gt 0) { $tip = ($answer[0] -split '\s+')[0] }
            }
        }
        Remove-Job -Job $job -Force -ErrorAction SilentlyContinue

        if ($tipError) {
            $pushedReason = "$tipError, so nobody here knows whether $remoteName has this commit$localNote"
        } elseif ($null -eq $tip) {
            # The branch is gone from the server, or was never there under this name. The COMMIT can
            # still be on it under another ref, which is the containment question tips cannot answer,
            # so this is not `false` either.
            $pushedReason = ("$remoteName has no branch $remoteBranch, so this commit is not the tip of the branch " +
                "this run tracked -- which is not the same as the server not having it$localNote")
        } elseif ([string]::Equals($tip, $HeadSha, [System.StringComparison]::Ordinal)) {
            $pushed = $true
            $pushedReason = "the tip of $remoteBranch on $remoteName IS this commit, read from the server"
        } else {
            $pushedReason = ("the tip of $remoteBranch on $remoteName is $tip, not this commit, and a commit that is " +
                "not a tip is not thereby absent from the server$localNote")
        }
    }

    return [ordered]@{
        pullRequest       = $prNumber
        pullRequestReason = $prReason
        upstreamSha       = $upstreamSha
        pushedReason      = $pushedReason
        pushed            = $pushed
    }
}

function Publish-RunManifest {
    <#
        Commits the manifest, alone, or refuses.

        THE COMMIT IS PURE BY CONSTRUCTION, and that is not a nicety: this runs inside a gate that
        an author started, on a branch holding their work. A commit that swept anything else in
        would put the author's uncommitted changes into history under a message they did not write,
        at the one moment they are least likely to be watching -- which is a far worse defect than
        the missing record this exists to fix. So it refuses when ANYTHING outside the manifest
        store differs, staged or unstaged, and it stages exactly one path.

        THE GATED HEAD IS THE PARENT OF THIS COMMIT. `headSha` was captured before the manifest was
        written, so it names the commit that was actually gated; this commit is its child, and the
        button's rule (#674(b)) is `headSha == parent(head)` when the tip touches only the store.
        The two facts a reader needs -- which commit was judged, and that the tip added nothing but
        the record -- are then both checkable with git alone.
    #>
    param(
        [Parameter(Mandatory)] [string] $ManifestPath,
        [Parameter(Mandatory)] [string] $HeadSha,
        $PullRequest,
        # THE BRANCH THIS RUN GATED, captured before the stages and passed in. Read here it would
        # answer "where is the operator standing?" when the question is "which branch did this run
        # gate?", and every read between the capture and the write was a window somebody could move
        # through. A parameter cannot drift; a fresh read can.
        [string] $BranchRef,
        # The exact bytes this run serialized. See the note at the hash below: hashing the PATH
        # commits whatever is on disk at that instant, and the store is a directory the
        # foreign-change refusal deliberately allows anyone to write in.
        [string] $Content,
        # EVERY copy of this record. `Write-GateManifestPair` writes two, and reconciling only the
        # one whose path the publisher happened to receive left the durable twin corrupted while
        # the run declared success -- the pair is the unit, and a parameter that names one file
        # made it easy to forget that.
        [string[]] $Copies = @()
    )

    $relative = 'ci/../.factory/gate-runs'
    $storePrefix = '.factory/gate-runs/'
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $status = @(& git status --porcelain --untracked-files=all 2>$null | ForEach-Object { [string]$_ })
        if ($LASTEXITCODE -ne 0) {
            Write-Host '[gate] WARNING: the manifest was not committed: git status did not answer.' -ForegroundColor Yellow
            return
        }
        # A porcelain line is `XY <path>`; a rename is `XY <old> -> <new>`. Both ends of a rename
        # are checked, because a rename INTO the store from outside is still the gate moving a file
        # the author is holding.
        # THE GATE'S OWN NONCE IS NOT THE AUTHOR'S WORK. `Write-CanaryNonce` rewrites this TRACKED
        # file before every run by design (#152), so `git status` reports it on EVERY normal
        # invocation -- and this refusal then fired every time, which means #674(a) committed a
        # manifest on no ordinary run at all. A guard that refuses on its own artefact does not
        # protect anything; it just never lets the thing it guards happen.
        #
        # Named, single, and narrow: this one path, written by this script, in this run. Everything
        # else outside the store still refuses, which is the case the guard exists for.
        $gateOwnPath = 'tools/ci-canary/src/nonce.rs'
        $foreign = @()
        foreach ($line in $status) {
            if ($line.Length -lt 4) { continue }
            $paths = ($line.Substring(3) -split ' -> ') | ForEach-Object { $_.Trim('"') }
            foreach ($candidate in $paths) {
                $normalised = $candidate -replace '\\', '/'
                if ([string]::Equals($normalised, $gateOwnPath, [System.StringComparison]::Ordinal)) { continue }
                if (-not $candidate.StartsWith($storePrefix)) { $foreign += $candidate }
            }
        }
        if ($foreign.Count -gt 0) {
            Write-Host ("[gate] the run manifest was NOT committed: this tree holds changes outside " +
                "$storePrefix, and a gate that commits them would put your work into history under " +
                "a message you did not write. Commit or stash them and re-run to record the run:`n  " +
                (($foreign | Sort-Object -Unique) -join "`n  ")) -ForegroundColor Yellow
            return
        }

        $fileName = [System.IO.Path]::GetFileName($ManifestPath)

        # A FIXED FORM, so a reader and a checker parse the same thing. The pull request number is
        # omitted rather than guessed when it is unknown -- the manifest already records why.
        $message = if ($null -ne $PullRequest) {
            "gate: run manifest for $HeadSha (#$PullRequest)"
        } else {
            "gate: run manifest for $HeadSha"
        }

        # THE PARENT IS PART OF THE PUBLICATION, NOT A CHECK IN FRONT OF IT. Re-reading HEAD and
        # then running `git commit` left a read-then-act window: another shell advancing the branch
        # in between meant `git commit` read the NEWER head, committed on top of it, and reported
        # success while `manifest.headSha == HEAD~1` -- the predicate the checker relies on -- was
        # false. A window narrow enough to be hard to hit is still a window, and this one produces a
        # green record of the wrong thing.
        #
        # So the commit is BUILT on $HeadSha and the branch is moved by compare-and-swap:
        #   hash-object   the manifest blob
        #   read-tree     $HeadSha into a TEMPORARY index -- never the author's
        #   update-index  put the blob at its path
        #   commit-tree   with $HeadSha as the parent, explicitly
        #   update-ref    <new> <old>, which git refuses if the branch moved
        # git decides the race, and it decides it by refusing.
        #
        # The author's index is never touched now, which also retires the `git reset` recovery: the
        # earlier form staged into the real index and had to undo that on every failure path.
        # THE BRANCH IS THE ONE CAPTURED BEFORE THE STAGES, never one read now. Reading it here
        # asked "where is the operator standing?" when the question is "which branch did this run
        # gate?" -- and every answer between the capture and the write was a window.
        $branchRef = $BranchRef
        if (-not $branchRef) {
            Write-Host ('[gate] WARNING: HEAD was detached when this run started, so there is no branch to publish ' +
                'the run manifest onto; it is written but not committed.') -ForegroundColor Yellow
            return
        }

        # THE BYTES THIS RUN SERIALIZED, not whatever is at that path now. `hash-object` reads the
        # FILE, and between Write-GateManifestPair returning and this line the store copy is
        # ordinary disk: anything can replace it, and the foreign-change refusal explicitly permits
        # every path under the store, so the swap still succeeds and the gate publishes somebody
        # else's bytes under its own message while the durable copy says something different.
        #
        # The content is handed in and staged where only this run knows the path, so there is no
        # window to race: the commit carries what the gate generated, whatever happens to the copy
        # in the store.
        $hashSource = $ManifestPath
        $contentStaging = $null
        if ($Content) {
            $contentStaging = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(),
                "graphhelm-manifest-$([guid]::NewGuid().ToString('N')).json")
            [System.IO.File]::WriteAllText($contentStaging, $Content, (New-Object System.Text.UTF8Encoding($false)))
            $hashSource = $contentStaging
        }
        # GIT'S OWN WORDS ARE KEPT FOR EVERY STEP OF THIS PUBLICATION, not only for the one that
        # flaked. The `commit-tree` call below already did this, and its comment says why: "the
        # warning said only that it failed, so three sightings produced no diagnosis at all". THAT IS
        # A RULE ABOUT FIVE CALLS AND I APPLIED IT TO ONE -- the seventh time in this pull request
        # that a rule of mine was applied to the subject in front of me instead of to its subjects.
        #
        # It was paid for immediately: a reviewer saw this exact warning 37 times on his machine and
        # could not tell me WHY, because the only text the run produced was that this step failed.
        # Five measurements were spent guessing at a sentence git had already written and this code
        # threw away. `2>&1` costs nothing on the happy path.
        #
        # The sha is picked out BY SHAPE rather than by position, so a warning line on stderr cannot
        # be mistaken for the answer.
        $blobOutput = @(& git hash-object -w -- $hashSource 2>&1 | ForEach-Object { [string]$_ })
        $blobExit = $LASTEXITCODE
        if ($contentStaging) { Remove-Item -LiteralPath $contentStaging -Force -ErrorAction SilentlyContinue }
        $blob = @($blobOutput | Where-Object { $_ -cmatch '^[0-9a-f]{40}$' } | Select-Object -First 1)
        if ($blobExit -ne 0 -or -not $blob) {
            Write-Host ('[gate] WARNING: the run manifest could not be written to the object store; it is not ' +
                "committed. git exited $blobExit and said: " + (($blobOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Yellow
            return
        }
        $blob = ([string]$blob).Trim()

        $tempIndex = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(),
            "graphhelm-gate-index-$([guid]::NewGuid().ToString('N'))")
        $previousIndex = $env:GIT_INDEX_FILE
        try {
            $env:GIT_INDEX_FILE = $tempIndex
            $readTreeOutput = @(& git read-tree $HeadSha 2>&1 | ForEach-Object { [string]$_ })
            if ($LASTEXITCODE -ne 0) {
                Write-Host ("[gate] WARNING: could not read the tree of $HeadSha; the run manifest is not " +
                    'committed. git said: ' + (($readTreeOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Yellow
                return
            }
            $updateIndexOutput = @(& git update-index --add --cacheinfo "100644,$blob,$storePrefix$fileName" 2>&1 |
                    ForEach-Object { [string]$_ })
            if ($LASTEXITCODE -ne 0) {
                Write-Host ('[gate] WARNING: the run manifest could not be placed in the tree; it is not ' +
                    'committed. git said: ' + (($updateIndexOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Yellow
                return
            }
            $treeOutput = @(& git write-tree 2>&1 | ForEach-Object { [string]$_ })
            $treeExit = $LASTEXITCODE
            $tree = @($treeOutput | Where-Object { $_ -cmatch '^[0-9a-f]{40}$' } | Select-Object -First 1)
            if ($treeExit -ne 0 -or -not $tree) {
                Write-Host ('[gate] WARNING: the run manifest tree could not be written; it is not committed. ' +
                    "git exited $treeExit and said: " + (($treeOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Yellow
                return
            }
        } finally {
            if ($null -eq $previousIndex) { Remove-Item Env:\GIT_INDEX_FILE -ErrorAction SilentlyContinue }
            else { $env:GIT_INDEX_FILE = $previousIndex }
            Remove-Item -LiteralPath $tempIndex -Force -ErrorAction SilentlyContinue
        }
        $tree = ([string]$tree).Trim()

        # SIGNING IS HONOURED, because `commit-tree` does not read `commit.gpgsign` the way `commit`
        # does: switching to the plumbing would otherwise have made the gate the one writer in a
        # signing repository that quietly produces unsigned commits. `--no-verify` opting out of
        # HOOKS was a deliberate decision recorded above; opting out of signing was not, and a
        # silent policy change is worse than a refusal.
        # `--type=bool` LETS GIT DECIDE WHAT A BOOLEAN IS. My regex listed the spellings I happened
        # to remember; git accepts more of them, and case-insensitively, so a repository configured
        # with `commit.gpgSign = On` or `Yes` would have had its policy silently dropped by a gate
        # that believed it was honouring it. Reimplementing a parser the tool already exposes is how
        # a check ends up agreeing with itself instead of with the thing it checks.
        # FOUR STATES, NOT THREE -- and this is the fourth instance of the pattern named a hundred
        # lines up, arriving before the comment had settled. Enumerating the subjects of
        # "commit.gpgsign" gives absent, false, true, and MALFORMED, and `2>$null` collapsed the
        # fourth into the first. Measured:
        #
        #   absent      exit 1     (nothing)
        #   'yesplease' exit 128   fatal: bad boolean config value 'yesplease' for 'commit.gpgsign'
        #   'True'      exit 0     true
        #
        # So a repository whose config is malformed got an UNSIGNED commit from the gate, where a
        # normal `git commit` refuses outright. That is the silent policy change this block's own
        # comment forbids, and the exit code alone distinguishes it -- no message parsing needed.
        # AND THE EXIT CODE IS NOT THE DIAGNOSIS. This block used to ASSERT the cause -- "gpgsign is
        # set to something git cannot read as a boolean" -- from an exit code that covers more than
        # that. Observed here, three runs of the suite, no `commit.gpgsign` set at any level
        # (`git config --show-origin --get-all commit.gpgsign` exits 1, nothing anywhere): one run
        # printed that sentence anyway. `git config` returns 128 for a malformed value AND for a
        # config file it could not read -- and the fleet rewrites the shared `.git/config` and the
        # global one constantly, so a reader can land mid-rewrite.
        #
        # The refusal stays: an unreadable signing policy is not a licence to write an unsigned
        # commit, and this fails closed on purpose. What changes is that the run reports what GIT
        # said instead of naming a cause it did not measure. A message that asserts one cause for a
        # class of exits sends the next reader to fix a config that is not broken.
        $signOutput = @(& git config --type=bool --get commit.gpgsign 2>&1 | ForEach-Object { [string]$_ })
        $signExit = $LASTEXITCODE
        $signRequested = @($signOutput | Where-Object { $_ -cmatch '^(true|false)$' } | Select-Object -First 1)
        if ($signExit -ne 0 -and $signExit -ne 1) {
            Write-Host ("[gate] git could not read commit.gpgsign (git exited $signExit and said: " +
                (($signOutput | Select-Object -First 3) -join ' | ') + "), so this run will NOT commit the " +
                'manifest: signing here would be a policy decision the repository did not make, and skipping it ' +
                'silently would be the same decision by omission. If the value is malformed, fix it; if git could ' +
                'not read the file at all, this run raced something that was rewriting it.') -ForegroundColor Red
            return
        }
        $signArgs = if ($signExit -eq 0 -and
            [string]::Equals(([string]$signRequested).Trim(), 'true', [System.StringComparison]::Ordinal)) { @('-S') } else { @() }
        # GIT'S OWN WORDS ARE KEPT FOR THIS STEP. It is the one that has failed intermittently in the
        # suite (#741) and the warning said only that it failed, so three sightings produced no
        # diagnosis at all. `2>&1` here costs nothing on the happy path and is the difference between
        # a fourth sighting and an answer.
        $commitOutput = @(& git commit-tree @signArgs $tree -p $HeadSha -m $message 2>&1 | ForEach-Object { [string]$_ })
        $commitExit = $LASTEXITCODE
        $commit = @($commitOutput | Where-Object { $_ -cmatch '^[0-9a-f]{40}$' } | Select-Object -First 1)
        if ($commitExit -ne 0 -or -not $commit) {
            Write-Host ("[gate] WARNING: the run manifest commit could not be created; it is written but not " +
                "committed. git exited $commitExit and said: " + (($commitOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Yellow
            return
        }
        $commit = [string]$commit
        $commit = ([string]$commit).Trim()

        # THE COMPARE-AND-SWAP. `<new> <old>` makes git verify the branch still points at the head
        # this run judged, and refuse otherwise -- there is no instant between the check and the
        # move for anything to happen in.
        # NO RE-READ, BY CONSTRUCTION. The previous revision read `symbolic-ref` again and compared
        # -- which narrowed the window and left one, because a check followed by an act is still two
        # things. This asks git nothing: the ref NAME came from the capture, and `update-ref` with an
        # expected value compares and writes in ONE operation. A shell that switched branches does
        # not touch `$branchRef`, so it cannot affect this; a shell that MOVED `$branchRef` makes the
        # command fail. There is no instant between looking and acting because there is no looking.
        $updateRefOutput = @(& git update-ref -m $message $branchRef $commit $HeadSha 2>&1 |
                ForEach-Object { [string]$_ })
        if ($LASTEXITCODE -ne 0) {
            # WHICH OF THE TWO FAILED: the swap refusing because the branch moved, or the swap being
            # refused for some other reason entirely -- a `reference-transaction` hook saying no,
            # permissions, a broken ref store. Both arrive as one non-zero exit, and treating that as
            # movement wrote `headMovedDuringRun: true` into the durable record with the branch
            # standing still, then told the operator to re-run on a stable checkout, which fixes
            # nothing they can see.
            #
            # This is the first finding of this pull request in the newest place: THE TOOL FAILING
            # AND THE FACT BEING OBSERVED ARE DIFFERENT STATES, and one non-zero exit cannot carry
            # both. The re-read is diagnostic and happens AFTER the act, so it decides no
            # publication -- it only decides which true sentence to record.
            $refNow = (& git rev-parse --verify --quiet "$branchRef" 2>$null | Select-Object -First 1)
            # The compare-and-swap's own comparison: the one place where an approximate comparer
            # would let the record be written against a ref that had moved.
            $refMoved = ($LASTEXITCODE -ne 0 -or
                -not [string]::Equals(([string]$refNow).Trim(), $HeadSha, [System.StringComparison]::Ordinal))
            # THE REFUSAL IS A VERDICT, NOT A LOG LINE. The compare-and-swap failing means the
            # branch moved during the run, which is the same fact the start-versus-end comparison
            # reports -- and it was reaching nobody: `Publish-RunManifest` returned, the caller still
            # got a path, the flag stayed false, and the gate printed GREEN and exited 0 for a head
            # the stages never tested. Detecting a race and then not acting on it is worse than not
            # detecting it, because the detection reads as coverage.
            if ($refMoved) {
                $script:headMovedDuringRun = $true
                Write-Host ("[gate] $branchRef no longer points at $HeadSha, so the run manifest was NOT committed. " +
                    'Something moved the branch while this run was finishing, and a commit here would name a head this ' +
                    'run never judged. This run is RED.') -ForegroundColor Red
            } else {
                # And git's own words, which are the whole diagnosis in this branch: "a hook,
                # permissions, or the ref store itself" is a list of guesses, and the command that
                # refused usually says which. Captured above rather than discarded.
                Write-Host ("[gate] $branchRef still points at $HeadSha, and the update was refused anyway -- a hook, " +
                    'permissions, or the ref store itself. The run manifest was NOT committed. This run is RED, and ' +
                    're-running on a stable checkout will not help: look at what refused the ref update. git said: ' +
                    (($updateRefOutput | Select-Object -First 3) -join ' | ')) -ForegroundColor Red
            }
            return
        }
        # AND THE REAL INDEX IS BROUGHT LEVEL WITH THE COMMIT, for exactly one path. Building the
        # commit in a temporary index left the author's index BEHIND the branch: `git status` then
        # showed the manifest as staged for DELETION, and their next unqualified commit would have
        # removed it. The old `git commit --only` updated the index for that path as a side effect,
        # and dropping it dropped that too. Caught by the cell that asserts what is staged
        # afterwards, not by reading the code.
        # AND THE WORKTREE COPY IS RECONCILED WITH WHAT WAS PUBLISHED. Hashing the captured bytes
        # secured the OBJECT; it said nothing about the file at $ManifestPath, which anything may
        # have replaced or deleted in the meantime. Left alone, the gate exits GREEN with the commit
        # holding one record and the disk holding another -- and the index refresh below would then
        # stage a blob that does not match the file, so the operator's next `git commit -a` writes
        # the stranger's bytes over the published ones.
        #
        # There is no read-then-act gap to worry about here: the bytes are already published and
        # immutable in the object store, so this only makes the mutable copy agree with them.
        if ($Content) {
            # EVERY COPY, not the one this function was handed. Write-GateManifestPair writes the
            # committable copy and the durable twin under one name precisely so they are one record;
            # reconciling a single path meant the twin could stay corrupted while the run declared
            # success, and the whole reason that helper exists is that half a pair must not survive.
            $toReconcile = @($ManifestPath) + @($Copies | Where-Object { $_ })
            $toReconcile = @($toReconcile | ForEach-Object { [System.IO.Path]::GetFullPath($_) } | Select-Object -Unique)
            foreach ($copyPath in $toReconcile) {
                $onDisk = $null
                try { $onDisk = [System.IO.File]::ReadAllText($copyPath) } catch { $onDisk = $null }
                if (-not [string]::Equals([string]$onDisk, [string]$Content, [System.StringComparison]::Ordinal)) {
                    try {
                        $reconcileTmp = "$copyPath.reconciling"
                        [System.IO.File]::WriteAllText($reconcileTmp, $Content, (New-Object System.Text.UTF8Encoding($false)))
                        [System.IO.File]::Replace($reconcileTmp, $copyPath, [NullString]::Value)
                        Write-Host ("[gate] a manifest copy did not match what was published, and was rewritten from " +
                            'the published bytes.') -ForegroundColor Yellow
                    } catch {
                        $script:manifestReconcileFailed = ("a manifest copy does not match what was published and could " +
                            "not be rewritten: $($_.Exception.Message)")
                        Write-Host ("[gate] $script:manifestReconcileFailed. The COMMITTED record is the authoritative " +
                            'one, and this run is RED.') -ForegroundColor Red
                    }
                }
            }
        }

        # AND THE INDEX IS PER WORKTREE, NOT PER BRANCH -- which is why this one read has to exist,
        # and why my previous "the gate never asks git what HEAD is again" was the wrong rule stated
        # too widely. The compare-and-swap guarantees the VALUE of the captured ref; it cannot
        # express which ref HEAD names. A shell that runs `git checkout bar` after the swap leaves
        # the swap correct and this line writing into BAR's index: the operator, standing on bar,
        # finds the gate's manifest staged in their tree, and their next unqualified commit carries
        # it under their message. That is the foreign-change refusal at the top of this function
        # defeated through the exit instead of the entrance.
        #
        # The rule that survives is narrower and truer: IDENTITY IS RE-VERIFIED IMMEDIATELY BEFORE
        # EACH ACT THAT DEPENDS ON IT. The publication does not need a read because `update-ref
        # <new> <expected>` compares and writes atomically. This does, because it writes into
        # whatever worktree is current, and no plumbing makes that atomic with the swap.
        #
        # Skipping is safe where publishing was not: the worst case becomes one path staged in the
        # gate's own store, with the remedy printed. The publication has already happened and the
        # commit is correct; only the courtesy touch is withheld.
        $branchAtIndexTime = (& git symbolic-ref --quiet HEAD 2>$null | Select-Object -First 1)
        if ($LASTEXITCODE -ne 0 -or
            -not [string]::Equals(([string]$branchAtIndexTime).Trim(), $branchRef, [System.StringComparison]::Ordinal)) {
            Write-Host ("[gate] the run manifest is committed on $branchRef, and this worktree has since moved to " +
                "$(([string]$branchAtIndexTime).Trim()) -- the index was NOT touched, because staging the gate's file " +
                "into another branch's tree would hand you a commit you did not write. Nothing to do; if " +
                "``git status`` looks odd on $branchRef, run ``git reset -- $storePrefix$fileName`` there.") -ForegroundColor Yellow
        } else {
            & git update-index --add --cacheinfo "100644,$blob,$storePrefix$fileName" 2>$null | Out-Null
            $touchFailed = ($LASTEXITCODE -ne 0)
            # AND CHECKED AGAIN AFTER THE WRITE. There is no atomic index update -- git offers a CAS
            # for refs and nothing for the index -- so a checkout between the guard and this line
            # still lands the file in another tree. Checking afterwards does not close that; it makes
            # the damage self-undoing, because the window now has to be entered TWICE for anything to
            # survive: once before the write and once before this repair.
            #
            # THE DIRECTION, since it cannot be removed: worst case the gate's manifest is left staged
            # in another branch's tree, where the operator's next unqualified commit would carry it
            # under their message. It is one path, inside the gate's own store, and the run prints the
            # `git reset` that clears it.
            #
            # NOT dropping the touch, and the reason is the opposite hazard: without it the operator's
            # index sits BEHIND the branch, `git status` shows the manifest staged for DELETION, and
            # their next unqualified commit REMOVES the record this whole ticket exists to keep. That
            # failure needs no race at all -- just an ordinary commit -- so trading a racy nuisance
            # for a routine erasure would be the worse bargain.
            $branchAfterTouch = (& git symbolic-ref --quiet HEAD 2>$null | Select-Object -First 1)
            if ($LASTEXITCODE -eq 0 -and
                -not [string]::Equals(([string]$branchAfterTouch).Trim(), $branchRef, [System.StringComparison]::Ordinal)) {
                & git reset --quiet HEAD -- "$storePrefix$fileName" 2>$null | Out-Null
                Write-Host ("[gate] this worktree moved to $(([string]$branchAfterTouch).Trim()) while the index was " +
                    'being refreshed; the staging was undone. Nothing of the gate is in your tree.') -ForegroundColor Yellow
            } elseif ($touchFailed) {
                Write-Host ('[gate] WARNING: the run manifest is committed, but this index still shows it as changed; ' +
                    "run ``git reset -- $storePrefix$fileName`` if git status looks odd.") -ForegroundColor Yellow
            }
        }
        $script:manifestPublished = $true
        Write-Host "[gate] run manifest committed: $message" -ForegroundColor Cyan
    } finally {
        $ErrorActionPreference = $previous
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

    # The head the STAGES ran against, not the head the publication happens to find. They differ
    # exactly when something moved underneath a long gate, which is the case this records.
    $headNow = (git rev-parse HEAD).Trim()
    $headSha = if ($script:gatedHeadAtStart) { $script:gatedHeadAtStart } else { $headNow }
    $headMoved = (-not [string]::Equals([string]$headSha, [string]$headNow, [System.StringComparison]::Ordinal))
    # AND THE RECORD SAYS SO BEFORE IT IS WRITTEN. The flag reached the process exit code but not
    # the manifest: the durable record was serialized `status: GREEN`, `overallPassed: true`, and
    # every consumer counting successful verifications counted this one. The exit code is read once
    # by whoever ran the gate; the manifest is read by everything afterwards, so it is the copy that
    # must not lie.
    $Status = Get-RecordedStatus -Status $Status -HeadMoved $headMoved
    # AND IT LEAVES THIS FUNCTION. Recorded only in the manifest, movement was a note nobody acted
    # on: stages could have inspected a mixture of revisions, the publisher refused to commit, and
    # the gate still finished GREEN and exited 0. A detection that does not reach the verdict is a
    # detection the caller never made.
    if ($headMoved) { $script:headMovedDuringRun = $true }
    # WHAT THIS COMPARISON CANNOT SEE, written down rather than left for the next reader to
    # discover: a head that moved to B and back to A between two stages ends where it started, so
    # comparing the start against the end says "did not move". The tree the gate measured at the
    # end is then the right tree, and the risk is narrower but real -- a STAGE that ran while the
    # checkout was on B measured something else, and its green belongs to B.
    #
    # Catching that needs a watch or the reflog, neither of which this comparison is. The slot lock
    # keeps other SESSIONS out; another shell in the same session is what remains. Declared as a
    # limit rather than papered over: this detects movement that LASTS, not movement that returns.
    $provenance = Get-HeadProvenance -HeadSha $headSha -BranchRef $script:gatedBranchAtStart
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
$runClass = Get-RunClassFrom -Status $Status -PassedEverything $passedEverything

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
        # #639: WHO assigned the class. `automatic` means no stage failed, not that anyone judged
        # the run -- which is what lets `classify-run.ps1` refine a green without overwriting a
        # human judgement.
        runClassOrigin     = 'automatic'
        relatedToDiff      = $relatedToDiff
        headSha            = $headSha
        # #674(a): the three fields that let a MERGED head be traced back to the run that gated it.
        # `headSha` alone cannot do it -- a squash merge discards the commit this run measured, so
        # the number is the only key that survives into main's history, and `pushed` says whether
        # this sha ever left the machine at all.
        pullRequest        = $provenance.pullRequest
        # The REASONS travel too. Without them a null `pullRequest` is indistinguishable from a
        # field nobody filled in, which is the exact distinction this design is built on: a value
        # that could not be determined and a value nobody looked for are different states, and only
        # the first is honest. The function computed both reasons and the record dropped them.
        pullRequestReason  = $provenance.pullRequestReason
        pushed             = $provenance.pushed
        pushedReason       = $provenance.pushedReason
        upstreamSha        = $provenance.upstreamSha
        dirtyDiffHash      = $dirtyDiffHash
        # A FACT, not an inference: the head moved between the start of the gate and this write.
        # The record names the head the STAGES ran against, and this says the working tree is no
        # longer on it -- which is why the publication below refuses to commit.
        headMovedDuringRun = $headMoved
        cargoTargetDir     = $actualTargetDir
        runStartUtc        = $runStartUtc.ToString('o')
        # THE INSTANT THIS RECORD WAS SERIALIZED, WHICH IS NOT THE INSTANT THE RUN ENDED. Everything
        # that can still turn this run RED happens after this line: the pair is written, the
        # compare-and-swap can refuse, the correction can fail. A reader holding only the manifest
        # cannot tell a finished run from one that died between here and its last write.
        #
        # The witness of completion is the ledger, not this field: RUN-END carries
        # `manifest=<basename>`, so a run whose manifest has no RUN-END naming it is a RUN-START
        # with no end, which #199 already treats as a dead run. That link is one-directional -- the
        # name is chosen after this object is built, so the record cannot name itself -- and a cell
        # pins it, because a consumer that reads `runEndUtc` as proof of completion under #674(b)
        # would be trusting a timestamp written before the run could still fail.
        #
        # For the overlap tool (#638) the field is conservative in the safe direction: every stage
        # has finished by the time this is stamped, so the window it closes covers all the work that
        # could touch a shared target dir. Publication touches git, not the target dir.
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
        # A run whose head moved did not pass everything, whatever the stages said: they did not all
        # inspect one revision, so there is no revision this record can vouch for.
        overallPassed      = ($passedEverything -and -not $headMoved)
    }

    # #667: sub-second stamp plus a random suffix, and a CREATE-ONLY write. The old name was
    # head12 + whole second with WriteAllText, so two runs sharing both silently overwrote each
    # other -- and the pairs most likely to collide are the most concurrent ones in the store, so
    # the loss erased exactly the overlap the manifests exist to prove (#638).

    # No BOM: caught live, third instance of the same Set-Content -Encoding utf8 hazard in this
    # file - a manifest read back with a strict JSON parser (Python's json.load, no -sig) rejects
    # a leading BOM outright, and this manifest exists specifically to be machine-read later.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $json = $manifest | ConvertTo-Json -Depth 8
    # #667: one name, free in BOTH stores before either is finalised, content staged in a .tmp and
    # moved into place only when complete. Writing the stores independently let the durable copy
    # rename itself (orphaning the twin) or collide and warn (leaving the PREVIOUS run's manifest
    # exactly where classify-run.ps1:267-273 looks for this run's, so the later classification
    # overwrote the earlier record).
    $durableDir = $null
    try {
        $durableDir = [System.IO.Path]::Combine((Get-SlotDir), 'gate-runs')
        [System.IO.Directory]::CreateDirectory($durableDir) | Out-Null
    } catch {
        Write-Host "[gate] WARNING: durable manifest directory unavailable: $($_.Exception.Message)" -ForegroundColor Yellow
        $durableDir = $null
    }

    # The invariant is "never HALF a pair", not "both or the gate fails". Those read alike and are
    # not: the durable store can exist and still refuse a file (ACLs, quota, an IO fault), and
    # making that fail this stage would turn an environment problem into a blocked gate for every
    # otherwise-green change -- on the repository's authoritative gate, with no hosted CI behind it.
    # #199 wrote that copy best-effort AND REPORTED for exactly this reason; strengthening the
    # pairing quietly took the fallback away with it.
    #
    # So: attempt the pair, and on ANY durable-side failure warn loudly and write the committable
    # copy alone. Write-GateManifestPair already guarantees the half-pair cannot survive -- it rolls
    # a finalised primary back out before throwing -- so the retry starts from a clean directory.
    $written = $null
    if ($durableDir) {
        try {
            $written = Write-GateManifestPair -PrimaryDirectory $manifestDir -SecondaryDirectory $durableDir -Json $json -HeadSha $headSha
        } catch {
            # NOT every durable failure is safe to retry. The helper reports one case specially:
            # the secondary move failed AND the already-finalised primary could not be withdrawn.
            # Falling back there would write a SECOND complete-looking manifest while RUN-END names
            # only one -- two records of one run, corrupting the counts and the overlap evidence
            # this store exists to provide. A blocked gate is recoverable; a corrupted record is
            # believed. The marker comes from manifest-name.ps1 so there is one producer of it.
            if (Test-ManifestRollbackLeftFinalised -Exception $_.Exception) { throw }
            Write-Host "[gate] WARNING: durable manifest copy failed, keeping the committable one only: $($_.Exception.Message)" -ForegroundColor Yellow
            $written = $null
        }
    }
    if (-not $written) {
        $written = Write-GateManifestPair -PrimaryDirectory $manifestDir -Json $json -HeadSha $headSha
    }
    # @() around the result. PowerShell UNROLLS a single-element array, so when the durable store
    # is unavailable and only the primary copy is written, `$written[0]` indexed into the STRING and
    # returned its first character -- the manifest written correctly, and $path, the return value
    # and the basename in RUN-END all one letter long, so nothing downstream could link the run to
    # its file. The tests already wrap; the production caller did not.
    $path = @($written)[0]
    # THE NAME IS AN INDEX; THE IDENTITY IS `headSha` INSIDE THE FILE. The artefact is called
    # `<first 12 of the head>-<timestamp>.json`, so a glob on twelve characters is the obvious way to
    # find a run and it is NOT a way to identify one: it matches a NAME, and a reader that stops
    # there has married a filename to a question about a commit. Measured, not imagined -- on #639 a
    # reader matched a manifest by a sha that appeared in a file name and reported the wrong run.
    #
    # AND THE TWO FAILURES OF THAT GLOB ARE DIFFERENT FACTS, which is the half that saves whoever
    # writes the next reader:
    #   nothing matched          -> no run recorded for that head (or the store is not where you looked)
    #   matched, headSha differs -> a twelve-character prefix collision, or the wrong artefact
    # The first is absence; the second is a mismatch that a bare `Test-Path` reports as presence.
    # Anything deciding on a manifest must open it and compare `headSha` in full.
    $fileName = [System.IO.Path]::GetFileName($path)

    # #199's durable copy is now written by Write-GateManifestPair above, in the same reservation
    # as the committable one, so the two cannot end up under different names or with one store
    # holding a previous run's file where this run's twin belongs.

    # RUN-END IS WRITTEN AFTER THE PUBLICATION, not before it. The ledger event and the durable JSON
    # are the two records that survive this process, and both were being written while the operation
    # that can still turn the run RED had not run yet: `$json` said GREEN at serialization, the pair
    # went to disk, RUN-END said GREEN, and only then could the compare-and-swap fail and take the
    # exit code to 1. Anyone counting runs from the store or the log counted a success the gate
    # rejected.
    #
    # The committed copy is the one record that CANNOT carry this -- it is the input to the commit
    # whose outcome it would have to describe. That is a real limit of the shape, and it is why the
    # durable copy and this event are the two that must tell the truth.

    # #674(a): the manifest is COMMITTED, or nothing about it travels with the merge. A file in a
    # working tree is evidence for whoever is standing at that machine; the whole point of this
    # ticket is a record a merged commit can be traced back to.
    # THE VERDICT IS PERSISTED ONLY AFTER THE PUBLICATION SUCCEEDS. The record was serialized to
    # BOTH stores before this call, so a refused publication left a durable `status: GREEN` for a run
    # that never published -- the copy every later reader trusts, saying the opposite of what
    # happened. The flag is set inside the publisher and the record is corrected here.
    $script:manifestPublished = $false
    Publish-RunManifest -ManifestPath $path -HeadSha $headSha -PullRequest $provenance.pullRequest `
        -BranchRef $script:gatedBranchAtStart -Content $json -Copies @($written)
    # THE CLASS IS DERIVED FROM THE STATUS, so correcting one without the other leaves the record
    # disagreeing with itself: `status: RED` beside a class that still says the run passed. #199
    # made the class derived precisely so the two could not drift, and writing the status by hand on
    # the refusal path reintroduced the drift one field over.
# THE CORRECTION RUNS FIRST, AND THE EVENT DESCRIBES WHAT IT FOUND. RUN-END was appended before the
# durable copies were corrected, so a process killed between the two left the ledger saying RED and
# the manifests saying GREEN -- the append-only record and the store disagreeing about the same run,
# with the store being the one most readers open.
#
# Ordering is the whole remedy here: there is no way to make two writes one, so the one that can be
# re-derived goes last. A ledger line missing its manifest correction is a run whose RUN-START has
# no RUN-END, which #199 already treats as a dead run; a corrected manifest with no ledger line is a
# record nobody can place in the sequence.
    if (-not $script:manifestPublished) {
        # THE PATTERN, NAMED, because this is its third appearance in one pull request and naming it
        # is cheaper than a fourth fix: a principle applied one level short of where it reaches.
        # The host scoping stopped at the host and missed the second ACCOUNT on it. The captured
        # identity fed the publisher and missed the provenance CONSUMER. The derived-field
        # correction fixed the class and missed the next FIELD. Each time the sentence was already
        # written in this file, and each time it was applied to the instance in front of me.
        #
        # The rule that generalises all three: WHEN A RULE IS STATED, ENUMERATE ITS SUBJECTS.
        # For a hand-corrected record that means every field the failure touches, or derive them
        # all from one source -- never the fields I happened to remember.
        #
        # EVERY DERIVED FIELD, FROM THE SAME SOURCE. Correcting status, overallPassed, runClass and
        # publication while leaving `headMovedDuringRun` as the local `$headMoved` -- captured before
        # the compare-and-swap could set the script-scope flag -- produced a record saying RED beside
        # `headMovedDuringRun: false`: the run failed and the record denies the fact that explains
        # it. That is the drift I named two commits ago, one field further along, and the lesson is
        # that "correct the fields I remember" loses to the field list the same way guarding
        # remembered fields lost to the field list in the checker.
        $manifest.status = 'RED'
        $manifest.overallPassed = $false
        $manifest.runClass = Get-RunClassFrom -Status 'RED' -PassedEverything $false
        $manifest.headMovedDuringRun = [bool]$script:headMovedDuringRun
        $manifest['publication'] = ('refused: the manifest was written but not committed, so nothing about this run ' +
            'travels with the merge')
        $correctedJson = $manifest | ConvertTo-Json -Depth 8

        # AND A CORRECTION THAT CANNOT REPORT ITS OWN FAILURE IS NOT A CORRECTION. The empty `catch`
        # here left a durable copy saying GREEN while the gate exited RED -- the false historical
        # verdict this whole block exists to prevent, arriving in silence and reading as coverage
        # because the block is visibly present.
        #
        # It cannot go through Write-GateManifestPair: that reserves a NEW name on every call, so it
        # would write a second pair and leave the stale GREEN one in place. What it can borrow is the
        # DISCIPLINE -- write both to `.tmp` siblings first, and only replace once both are written.
        # Two file replacements still cannot be made atomic here; what changes is that a failure at
        # either end is REPORTED and becomes a gate failure instead of a quiet lie.
        $copies = @($written)
        $staged = @()
        $correctionFailure = $null
        try {
            foreach ($copy in $copies) {
                $tmp = "$copy.correcting"
                [System.IO.File]::WriteAllText($tmp, $correctedJson, (New-Object System.Text.UTF8Encoding($false)))
                $staged += $tmp
            }
        } catch {
            $correctionFailure = "the corrected record could not be prepared: $($_.Exception.Message)"
        }
        if (-not $correctionFailure) {
            for ($i = 0; $i -lt $copies.Count; $i++) {
                try {
                    # REPLACE, NOT COPY. `File.Copy(src, dst, overwrite)` writes THROUGH the
                    # destination: a kill halfway leaves a truncated file where a VALID record was,
                    # which is the one direction worse than not correcting at all. `File.Replace` is
                    # a rename over an existing file on the same volume -- both conditions hold here
                    # -- so each file is replaced atomically.
                    #
                    # Measured on this runtime, because the obvious alternative does not exist here:
                    #   File.Replace(3-arg)  : True
                    #   File.Move(overwrite) : False   (.NET Core 3.0+ only)
                    #
                    # THE PAIR IS STILL NOT ATOMIC, and that declaration stands. What was wrong was
                    # carrying "the pair cannot be atomic" into "so neither file can be" -- only the
                    # first half is true.
                    # Full paths here too, for the reason measured one function above.
                    # Same two traps as the reconciliation above: full paths, and a REAL null for
                    # the backup. This site has been throwing since the day it was written -- the
                    # catch below turned it into a reported failure, so nothing was silently wrong,
                    # but the correction it exists to perform never happened. Its cell asserted the
                    # WIRING and not the outcome, which is exactly how it stayed green.
                    [System.IO.File]::Replace([System.IO.Path]::GetFullPath($staged[$i]),
                        [System.IO.Path]::GetFullPath($copies[$i]), [NullString]::Value)
                } catch {
                    $correctionFailure = ("the corrected record could not replace $($copies[$i]): $($_.Exception.Message)" +
                        $(if ($i -gt 0) { ' -- an earlier copy WAS corrected, so the two stores now disagree' } else { '' }))
                    break
                }
            }
        }
        foreach ($tmp in $staged) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
        if ($correctionFailure) {
            Write-Host "[gate] MANIFEST CORRECTION FAILED: $correctionFailure" -ForegroundColor Red
            $script:manifestCorrectionFailed = $correctionFailure
        }
        $script:manifestNotPublished = $true
    }

    $endStatus = if ($script:manifestPublished) { $Status } else { 'RED' }
    $endClass = if ($script:manifestPublished) { $runClass }
        else { Get-RunClassFrom -Status 'RED' -PassedEverything $false }
    $endDetail = "status=$endStatus class=$endClass head=$($headSha.Substring(0, 12)) manifest=$fileName" +
        $(if (-not $script:manifestPublished) { ' publication=refused' } else { '' })
    Write-SlotEvent -Event 'RUN-END' -Detail $endDetail

    return $path
}

# THE GATED HEAD IS CAPTURED BEFORE ANY STAGE RUNS. Read at publication time instead, it named
# whatever HEAD had become after a long gate: another shell committing or checking out underneath
# put a head into the manifest that no stage ever verified, and that record still satisfies
# #674(b)'s `headSha == parent(head)` predicate. Green record, wrong subject.
#
# The window is the WHOLE GATE, not the moment of the commit, so this is the start of the pair --
# the check immediately before the commit is the other end, and neither replaces the other.
$script:headMovedDuringRun = $false
$script:manifestNotPublished = $false
$script:manifestCorrectionFailed = $null
$script:manifestReconcileFailed = $null
# THE BRANCH IS CAPTURED HERE TOO, and this is the LAST time the gate asks git what HEAD is.
# Every later question about identity was a fresh read, and each fresh read opened a window
# somebody could move through -- eight rounds of review found eight of them, one per site. Reading
# once and comparing against the captured value turns "which window is left?" into a question with
# no instances: `update-ref refs/heads/<captured> <new> <expected>` compares and writes as ONE
# operation, and a shell that switches branches does not touch the captured ref at all.
# ONE COHERENT SNAPSHOT, VERIFIED. These are two commands, so a checkout onto a sibling branch
# between them yields a branch and a sha that never described the same state -- and every later
# guard compares against that pair as though it did. There is no atomic primitive for "read HEAD's
# ref and value together", so the pair is READ and then CHECKED: the captured branch must still
# point at the captured sha. If it does not, this run has no coherent subject and refuses to start.
#
# The check does not close the window -- something can still move between the check and the first
# stage -- but the compare-and-swap at publication is what makes that residue harmless: it refuses
# unless the branch still holds the captured sha at the moment of writing.
$gatedBranchAtStart = (& git symbolic-ref --quiet HEAD 2>$null | Select-Object -First 1)
$gatedBranchAtStart = if ($LASTEXITCODE -eq 0 -and $gatedBranchAtStart) { ([string]$gatedBranchAtStart).Trim() } else { $null }
$gatedHeadAtStart = (& git rev-parse HEAD 2>$null | Select-Object -First 1)
$gatedHeadAtStart = if ($LASTEXITCODE -eq 0 -and $gatedHeadAtStart) { ([string]$gatedHeadAtStart).Trim() } else { $null }

if ($gatedBranchAtStart -and $gatedHeadAtStart) {
    $branchValueAtStart = (& git rev-parse --verify --quiet $gatedBranchAtStart 2>$null | Select-Object -First 1)
    if ($LASTEXITCODE -ne 0 -or
        -not [string]::Equals(([string]$branchValueAtStart).Trim(), $gatedHeadAtStart, [System.StringComparison]::Ordinal)) {
        Write-Host ("[gate] the branch and the head read at startup do not describe one state: $gatedBranchAtStart " +
            "holds $(([string]$branchValueAtStart).Trim()) while HEAD read $gatedHeadAtStart. Something moved between " +
            'the two reads, so this run has no coherent subject to gate. Re-run on a stable checkout.') -ForegroundColor Red
        exit 1
    }
}

$slotLockAtStart = Read-SlotLockSnapshot

# #199: the START half of the append-only pair. Written BEFORE any stage runs, so a run that dies
# without ever reaching `Write-RunManifest` - killed by a lock collision, a clean underneath it, an
# aborted slot - still leaves a line. A START with no matching RUN-END IS the record of a dead run,
# and that class was previously invisible: it produces no manifest at all, so a count of manifests
# counts only the runs that survived to write one.
Write-SlotEvent -Event 'RUN-START' -Detail "targetDirBase64=$(ConvertTo-SlotEventField -Value $actualTargetDir) cwd=$repositoryRoot"

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
    # named guards never ran at all -- verified name by name, not inferred. A RED that stopped
    # looking is not the same object as a RED that looked at everything, and before this flag the
    # two printed the same word (#238).
    #
    # THEY STILL CAN, and this comment would mislead without the next sentence: the flag stops
    # CARGO aborting on a failing binary. It does not stop the stage ending early from a harness
    # abort, a timeout, a killed process or a crash -- and in those cases the manifest is shaped
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
    # #643: every versioned ci/*.tests.ps1, discovered from the tree. See the script for why the
    # inventory is a pinned SET of names and not a count, and why exit 2 is not folded into 1.
    Invoke-Stage 'ci powershell suites' {
        & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repositoryRoot 'ci/run-ps-suites.ps1')
    } | Out-Null

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
# MOVEMENT IS A GATE FAILURE, not an annotation. If HEAD changed between the capture before the
# first stage and the manifest write, the stages did not all look at one revision -- so there is no
# revision this run can vouch for, whatever the individual stages said. Green here would be a green
# about nothing in particular.
# A REFUSED PUBLICATION IS A GATE FAILURE, not a warning: #674(a) exists so the record travels with
# the merge, and a record that stayed on this disk does not.
# A correction that could not be persisted leaves a record claiming the opposite of the verdict, so
# it is a manifest failure in its own right -- the same rule as "green without a well-formed manifest
# is red by rule", applied to the copy that says green after the run went red.
# Same rule as the correction below it: the commit is authoritative, the file on disk is what most
# readers open, and a run that leaves them disagreeing has not recorded what it claims to have.
if ($script:manifestReconcileFailed) {
    Write-Host ("[gate] the manifest on disk disagrees with the published record: $script:manifestReconcileFailed") -ForegroundColor Red
    $failed += 'run manifest reconciliation'
}
if ($script:manifestCorrectionFailed) {
    Write-Host ("[gate] a durable record may still say GREEN for this run: $script:manifestCorrectionFailed") -ForegroundColor Red
    $failed += 'run manifest correction'
}
if ($script:manifestNotPublished) {
    Write-Host ('[gate] the run manifest was written but NOT committed, so nothing about this run travels ' +
        'with the merge. The durable copies were corrected to say so.') -ForegroundColor Red
    $failed += 'run manifest not published'
}
if ($script:headMovedDuringRun) {
    Write-Host ("[gate] HEAD moved during this run: the stages did not all inspect $gatedHeadAtStart, so no " +
        'revision was gated. Re-run on a stable checkout.') -ForegroundColor Red
    $failed += 'HEAD moved during the run'
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
