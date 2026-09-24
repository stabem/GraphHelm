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

.PARAMETER MergeTargetRef
    Optional local baseline ref, or expected base name when LandingSnapshotPath is supplied.

.PARAMETER LandingSnapshotPath
    Optional JSON captured by an external orchestrator: head, sha, ref, pullRequest. Validated
    locally without credentials. Trusted main's merge proof independently checks PR-base evidence.
    Example: {"head":"<source SHA>","sha":"<base SHA>","ref":"main","pullRequest":993}.
    Fetch the objects externally, then run ./ci/gate.ps1 -LandingSnapshotPath <file>. The unchanged
    ./ci/gate.ps1 command still runs the complete offline test suite using local baseline provenance.
    Rollout: existing receipts keep their historical test-coverage meaning. Once this producer and
    target-aware verifier land on trusted main, merging requires a receipt with supplied target
    evidence; legacy/local-only receipts do not acquire that evidence retroactively. Gate again with
    an externally captured snapshot when target proof is required. This is not an atomic merge guard.

.PARAMETER NonPullRequest
    Explicit local mode (also the default without LandingSnapshotPath). Full test coverage remains
    available offline; local provenance alone is not PR-base proof after trusted-main activation.

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
    [string] $PostgresBin,
    # #903: the path to a selection written by ci/select-scope.ps1. ABSENT MEANS FULL, and so does
    # every selection this gate cannot read: the default is the whole gate until a runner decides
    # otherwise, because a scoped run that narrows on a bad selection runs fewer stages and reports
    # the same green.
    [string] $ScopeSelection,
    # Optional local baseline ref, or expected supplied snapshot base name; defaults to origin/main.
    [string] $MergeTargetRef,
    [string] $LandingSnapshotPath,
    # Explicit local mode; also the default when no LandingSnapshotPath is supplied.
    [switch] $NonPullRequest,
    # #883: permits a detached-HEAD run to proceed past the startup refusal below, to measure
    # without publishing. Off by default -- the refusal exists because a detached run's manifest
    # can never be committed, and that must stay the loud default, not something a flag quietly
    # opts out of by omission.
    [switch] $AllowDetachedHead,
    # #1053 item 7: run EVERY stage even after one fails, which is this gate's behaviour before
    # item 7. Default OFF, so the common case -- 56 % of red runs fail exactly one stage -- stops
    # paying a median 940 s for stages nobody will read. Pass it when you want the whole list of
    # failures in one run instead of one failure per run.
    [switch] $NoFailFast
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$repositoryRoot = Split-Path -Parent $PSScriptRoot

# #166: refuse before any setup, Push-Location, canary, or stage banner. SET-ONLY is intentional:
# callers use isolated fresh targets to prevent cross-lane contamination, and the run manifest
# records the chosen path. This precondition prevents the silent worktree-local DEFAULT caused by
# an unset variable; it does not replace the runner's target allocation policy with a path regex.
# #1053 item 4: INCREMENTAL IS OFF FOR THE GATE, AND ONLY FOR THE GATE.
#
# Incremental compilation buys a second build of a tree that CHANGED. This gate builds a tree that
# will not change again -- one pass, then the run ends -- so every incremental artefact it writes is
# work done for a reuse that never comes, plus a large number of small files on a disk the target
# inventory already measures at 23.1 GB and 68 364 files. Set HERE and not in `.cargo/config.toml`,
# so a developer's interactive edit loop keeps the setting it genuinely benefits from. The linker
# choice goes the other way and lives in `.cargo/config.toml`; that file says why.
#
# BEFORE THE FIRST CARGO CALL, which is the canary far below. A value set after it would leave the
# canary -- the one stage that decides whether this build environment can be trusted at all --
# compiled under a different configuration than everything it vouches for.
$env:CARGO_INCREMENTAL = '0'

# #1053 item 4: THE TEST RUNNER IS PINNED, AND ITS ABSENCE IS A REFUSAL -- NEVER A FALLBACK.
#
# `workspace tests` runs `cargo nextest run`. A gate that quietly ran `cargo test` instead when the
# runner was missing would publish a green from a DIFFERENT INSTRUMENT than the one this repository
# measured, on a machine nobody would think to ask about. That is the failure this refusal exists
# for, and it is why there is no else-branch anywhere below that reaches for `cargo test`.
#
# EXACT MATCH, like the Rust toolchain. Two nextest versions can disagree about defaults that decide
# a verdict -- `--no-tests`, retries, slow-timeout termination -- so "close enough" is a version
# nobody named. Refused BEFORE the canary, because every stage after it is measured by this tool.
$toolVersionsPath = Join-Path $PSScriptRoot 'tool-versions.json'
$pinnedNextest = $null
try {
    $pinnedNextest = [string](([System.IO.File]::ReadAllText($toolVersionsPath) | ConvertFrom-Json).'cargo-nextest')
} catch {
    Write-Host "[gate] REFUSED: ci/tool-versions.json is unreadable ($($_.Exception.Message))." -ForegroundColor Red
    exit 1
}
if ([string]::IsNullOrWhiteSpace($pinnedNextest)) {
    Write-Host '[gate] REFUSED: ci/tool-versions.json names no cargo-nextest version.' -ForegroundColor Red
    exit 1
}
# `2>&1` and the exit code, not the text alone: a missing subcommand prints to stderr and exits
# non-zero, and both readings must reach the refusal rather than one of them being swallowed.
# A 'Continue' WINDOW, like every other native `2>&1` in this file. This one ran at SCRIPT SCOPE
# under the 'Stop' set at :122, and measured on PowerShell 5.1 that is fatal twice over: a process
# that writes ANY stderr line raises a TERMINATING NativeCommandError, and a command that is not on
# PATH raises a terminating CommandNotFoundException. Both killed the gate on THIS line.
#
# WHICH MADE BOTH REFUSALS BELOW UNREACHABLE FOR THE CASES THEY EXIST FOR. `cargo` here is the
# rustup shim (rust-toolchain.toml pins the channel), and rustup writes `info: syncing channel
# updates for '1.97.1-...'` to stderr while the command it proxies prints the right version and
# exits 0 -- so a CORRECTLY installed, correctly pinned runner killed the gate. An ABSENT
# cargo-nextest died on the not-found error instead of printing the install command. The only
# failure that ever reached the refusal was a SILENT non-zero exit, which is the failure that does
# not happen. Reproduced both ways in ci/gate-native-stderr.tests.ps1, section H, which runs this
# very block in a child process against real `.cmd` shims.
function Read-NextestVersionProbe {
    <#
      .SYNOPSIS
        `cargo nextest --version`, as an exit code and its text, without taking the gate down.

      .DESCRIPTION
        A FUNCTION, so that the two `$ErrorActionPreference` assignments it needs are INDENTED.
        ci/gate-manifest-provenance.tests.ps1 derives the preamble it hands Publish-RunManifest by
        reading this file's strictness statements AT COLUMN ZERO, and refuses when one of them is
        "duplicated so the read is ambiguous". A top-level 'Continue' window here made that read
        ambiguous and took that suite down -- the guard was right and the shape was wrong.

        WHY A WINDOW AT ALL. This probe ran at script scope under the 'Stop' set above, and measured
        on PowerShell 5.1 that is fatal twice over: a process that writes ANY stderr line raises a
        TERMINATING NativeCommandError, and a command that is not on PATH raises a terminating
        CommandNotFoundException. Both killed the gate on the invocation below, BEFORE either
        refusal that follows could print, before the CARGO_TARGET_DIR precondition and before the
        canary -- with a raw PowerShell error instead of the install instructions.

        SO BOTH REFUSALS WERE UNREACHABLE FOR THE CASES THEY EXIST FOR. `cargo` here is the rustup
        shim (rust-toolchain.toml pins the channel), and rustup writes `info: syncing channel
        updates for '1.97.1-...'` to stderr while the command it proxies prints the right version
        and exits 0 -- so a CORRECTLY installed, correctly pinned runner killed the gate. An ABSENT
        cargo-nextest died on the not-found error instead of naming the install command. The only
        failure that ever reached the refusal was a SILENT non-zero exit, which is the failure that
        does not happen. ci/gate-native-stderr.tests.ps1 section H runs this very block in a child
        process against real `.cmd` shims, chatty, silent, failing and absent.

        NO `| Out-String`, and deliberately no `$?`. A pipe makes `$?` report the LAST element --
        `Out-String`, which always succeeds -- so `$?` could never speak for cargo here. The caller's
        regex carries that weight instead: a command that never ran leaves `$LASTEXITCODE` holding
        whatever ran before it, and no `cargo-nextest <version>` can be matched out of a capture that
        command never produced.
    #>
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    # ASSIGNED BEFORE THE TRY. `Set-StrictMode -Version 2.0` above throws on a variable nothing has
    # assigned, and a failed assignment leaves one in exactly that state.
    $probe = @()
    $code = $null
    try {
        $probe = & cargo nextest --version 2>&1
        # Read through Get-Variable because StrictMode throws on an automatic variable nothing has
        # assigned yet, which is the state an absent command leaves it in.
        $code = Get-Variable -Name 'LASTEXITCODE' -ValueOnly -ErrorAction SilentlyContinue
    } catch {
        # A CATCH, NOT A PREFERENCE, and the difference is measured: with the preference already at
        # 'Continue' a command that does not exist still stopped the script, because
        # CommandNotFoundException is raised at DISCOVERY and the preference does not downgrade it.
        # The absent runner is the first thing the refusal exists to report, so it has to survive.
        $probe = @([string]$_.Exception.Message)
        $code = $null
    } finally {
        $ErrorActionPreference = $previousPreference
    }
    return [pscustomobject]@{
        ExitCode = $code
        Text     = (@($probe) | ForEach-Object { [string]$_ }) -join "`n"
    }
}
$nextestProbeResult = Read-NextestVersionProbe
$nextestProbeCode = $nextestProbeResult.ExitCode
$nextestVersionOutput = $nextestProbeResult.Text
$nextestFound = ''
$nextestMatch = [regex]::Match([string]$nextestVersionOutput, 'cargo-nextest\s+([0-9]+\.[0-9]+\.[0-9]+)')
if ($nextestMatch.Success) { $nextestFound = $nextestMatch.Groups[1].Value }
if ($nextestProbeCode -ne 0 -or -not $nextestMatch.Success) {
    Write-Host '[gate] REFUSED: cargo-nextest is not installed, and this gate does not fall back to' -ForegroundColor Red
    Write-Host '[gate] `cargo test`: that would publish a green from an instrument nobody named.' -ForegroundColor Red
    Write-Host "[gate]   cargo install cargo-nextest --locked --version $pinnedNextest" -ForegroundColor Red
    exit 1
}
if (-not [string]::Equals($nextestFound, $pinnedNextest, [System.StringComparison]::Ordinal)) {
    Write-Host "[gate] REFUSED: cargo-nextest $nextestFound is installed; ci/tool-versions.json pins $pinnedNextest." -ForegroundColor Red
    Write-Host '[gate] The runner decides which tests ran, so it is pinned exactly, like the toolchain.' -ForegroundColor Red
    Write-Host "[gate]   cargo install cargo-nextest --locked --version $pinnedNextest" -ForegroundColor Red
    exit 1
}
Write-Host "[gate] test runner: cargo-nextest $nextestFound (pinned)" -ForegroundColor DarkGray

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

# #903: turn a scope selection into cargo's package arguments. THE FEATURE SET IS NEVER NARROWED --
# only the package list is. A scoped run that also trimmed features would skip the code a
# feature-gated change actually alters, and report the same green: the failure this whole
# deliverable exists to avoid, arriving through the flag nobody was watching.
#
# A FULL run yields an EMPTY array, and the caller keeps `--workspace`. It does not yield
# `-p <every crate>`: an enumeration of the workspace would silently stop covering a crate added
# after the selection was computed, and `--workspace` cannot.
function Get-ScopePackageArgs {
    param([Parameter(Mandatory)] [object] $Scope)

    if ($Scope.full) { return @() }
    $crates = @($Scope.crates)
    if ($crates.Count -eq 0) { return @() }
    return @($crates | ForEach-Object { '-p'; $_ })
}

# #903: what the MANIFEST records about the scope. Separate from Read-ScopeSelection because the
# decision and the record are different jobs: the decision picks what runs, this says what ran so a
# later reader can tell a GREEN that covered the workspace from a GREEN that covered two crates.
# A scoped run that does not record its scope is unauditable -- every reader of the manifest would
# read "GREEN" and none could tell which one they had.
function Get-ScopeRecord {
    param([Parameter(Mandatory)] [object] $Selection)

    return [ordered]@{
        full         = [bool]$Selection.full
        reason       = [string]$Selection.reason
        crates       = @($Selection.crates)
        matrix       = [bool]$Selection.matrix
        # INDEX, do not dot. `$Selection.missingKey` returns $null with StrictMode off and THROWS
        # under `Set-StrictMode -Version 2.0`, which this file sets at :88 -- so "read the member
        # directly" was a trap, not a remedy, and it killed the manifest write on every run.
        # `$Selection['key']` answers '' for an absent key under both. The constructor above makes
        # the key always present; this makes the reader safe even if a future producer forgets.
        matrixReason = [string]$Selection['matrixReason']
        # #901 slice 2. ABSENT READS AS TRUE -- a producer that forgot the key ran the Rust stages,
        # and a record must never say fewer ran than did. Only a real boolean false says "skipped".
        rust         = -not (($Selection['rust'] -is [bool]) -and (-not $Selection['rust']))
    }
}

# #903: ONE SHAPE, ONE PLACE. Every FULL answer is built here, so a consumer can read any key of a
# selection without asking whether this particular branch happened to set it. The first version
# spelled the FULL result out at each of the seven refusal sites, none of them carried
# `matrixReason`, and `Get-ScopeRecord` read that key with `[string]$Selection.matrixReason` --
# which under `Set-StrictMode -Version 2.0` (:88) THROWS rather than returning $null. Nobody passes
# -ScopeSelection today, so every run took a FULL branch and every gate died at the manifest write
# AFTER running all 56 stages: `MANIFEST WRITE FAILED: The property 'matrixReason' cannot be found`
# (X, measured on this PR's own gate run). A missing key on one branch of seven is a defect a
# constructor cannot have.
function New-FullScope {
    param([Parameter(Mandatory)] [string] $Reason)

    return [ordered]@{
        full         = $true
        reason       = $Reason
        crates       = @()
        matrix       = $true
        matrixReason = 'the PostgreSQL matrix runs: this is a FULL run'
        rust         = $true
    }
}

# #464: independent of the crate-graph scope above -- `apps/studio` is a Node/TypeScript project,
# not a Rust crate, so `ci/select-scope.ps1`'s compiled-dependency graph has nothing to map it
# onto. Read the SAME selection file `select-scope.ps1` already wrote, but for a question it never
# answers: did this change touch `apps/studio`. FAIL WIDE, same discipline as `Read-ScopeSelection`
# above -- absent, unreadable, unparseable, or escalated all mean "run it", because a selector's
# failure mode is a green run that measured less than it claimed, and running the Studio suite an
# unnecessary time costs minutes, not correctness.
function Test-StudioScopeChanged {
    param([string] $Path)

    if ([string]::IsNullOrWhiteSpace($Path)) { return $true }
    if (-not (Test-Path -LiteralPath $Path)) { return $true }
    $selection = $null
    try {
        $selection = [System.IO.File]::ReadAllText($Path) | ConvertFrom-Json
    } catch {
        return $true
    }
    if ($null -eq $selection) { return $true }
    if ($selection.PSObject.Properties.Name -contains 'escalated' -and $selection.escalated) {
        return $true
    }
    if (-not ($selection.PSObject.Properties.Name -contains 'changedFiles')) {
        return $true
    }
    # A SHAPE THIS PREDICATE CANNOT READ MUST WIDEN, THE SAME AS AN ABSENT ONE (Codex P2, review of
    # #1003). `changedFiles: null` used to fall through to `@($null) | ForEach-Object { [string]$_ }`
    # -- one element, coerced to `''` -- which matches no `apps/studio/*` pattern and silently
    # narrowed to false: the Studio stage skipped on a document this function could not actually
    # read, not on one it read and found clean. Any element that is not a string is the same
    # unreadable shape, however it arrived (a number, an object, a nested array).
    if ($null -eq $selection.changedFiles) {
        return $true
    }
    $rawChanged = @($selection.changedFiles)
    if ($rawChanged | Where-Object { $_ -isnot [string] }) {
        return $true
    }
    $changed = $rawChanged | ForEach-Object { [string]$_ }
    return [bool]($changed | Where-Object { $_ -like 'apps/studio/*' })
}

# #901 slice 2: does this change reach the PowerShell suites? `ci/select-scope.ps1` answers in
# `psSuites` and names why in `psSuitesReason`. FAIL WIDE, the Studio predicate's discipline above:
# absent, unreadable, escalated, or anything but a real boolean false runs every suite. The suites
# stage is 13-14 minutes of every gate, and it ran for a diff no suite reads.
#
# A SKIP STILL RUNS ONE SUITE. `ci/event-contract-sweep.tests.ps1` is the only producer of the
# `[event-contract]` marker the manifest's `eventContractSweep` is read from (`merge-proof`, retired
# 2026-09-24, refused a GREEN whose sweep did not measure). Its population is every open branch of the remote,
# not this diff, so no diff can prove it unreached. It runs in the same stage, alone.
function Get-PsSuitesScope {
    param([string] $Path)

    $all = [ordered]@{ included = $true; reason = 'ran: no scope selection was given' }
    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path)) { return $all }
    $selection = $null
    try {
        $selection = [System.IO.File]::ReadAllText($Path) | ConvertFrom-Json
    } catch {
        return [ordered]@{ included = $true; reason = 'ran: the scope selection is unreadable' }
    }
    if ($null -eq $selection) { return [ordered]@{ included = $true; reason = 'ran: the scope selection parsed to nothing' } }
    if ($selection.PSObject.Properties.Name -contains 'escalated' -and $selection.escalated) {
        return [ordered]@{ included = $true; reason = 'ran: this is a FULL run' }
    }
    $field = $selection.PSObject.Properties['psSuites']
    $why = $selection.PSObject.Properties['psSuitesReason']
    $reason = if ($null -ne $why -and $why.Value -is [string]) { [string]$why.Value } else { '' }
    if ($null -ne $field -and $field.Value -is [bool] -and $field.Value -eq $false) {
        return [ordered]@{ included = $false; reason = $reason }
    }
    if ([string]::IsNullOrWhiteSpace($reason)) { $reason = 'ran: the selection does not say the suites are unreached' }
    return [ordered]@{ included = $true; reason = $reason }
}

# #903: read a scope selection and decide whether this run is FULL. EVERY FAILURE PATH IS FULL --
# absent, missing, unparseable, shapeless, or empty -- because the failure mode of a scope selector
# is not a crash, it is a green run that measured less than it claimed. A narrow selection is
# accepted only when it says so explicitly AND names at least one crate; an empty crate list would
# run nothing and pass, which is the widest possible failure wearing the narrowest possible output.
function Read-ScopeSelection {
    param([string] $Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return New-FullScope -Reason 'FULL: no scope selection was given'
    }
    if (-not (Test-Path -LiteralPath $Path)) {
        return New-FullScope -Reason "FULL: the scope selection was not found at $Path"
    }
    $selection = $null
    try {
        $selection = [System.IO.File]::ReadAllText($Path) | ConvertFrom-Json
    } catch {
        return New-FullScope -Reason "FULL: the scope selection is unreadable ($($_.Exception.Message))"
    }
    if ($null -eq $selection) {
        return New-FullScope -Reason 'FULL: the scope selection parsed to nothing'
    }
    if ($selection.PSObject.Properties.Name -contains 'escalated' -and $selection.escalated) {
        $rule = if ($selection.PSObject.Properties.Name -contains 'escalationRule') { [string]$selection.escalationRule } else { 'unnamed rule' }
        return New-FullScope -Reason "FULL: the selector escalated ($rule)"
    }
    if (-not ($selection.PSObject.Properties.Name -contains 'crates')) {
        return New-FullScope -Reason 'FULL: the scope selection carries no crate list'
    }
    $crates = @($selection.crates)
    if ($crates.Count -eq 0) {
        # TWO EMPTINESSES, and until #903's third state they were one. An empty list usually means
        # the derivation could not answer, and that must widen -- selecting nothing would run
        # nothing and pass. But a selection can also be empty because nothing the run compiles
        # CHANGED: a `ci/*.tests.ps1` suite is discovered by `ci/run-ps-suites.ps1` and run in an
        # unconditional stage, so it cannot change what a Rust stage measures. The producer says
        # which of the two this is, in `rustInputsChanged`, and the field is read by INDEX because
        # an absent one must read as "may have changed" rather than throw or default to the narrow
        # answer (`$selection['x']` answers '' for an absent key under StrictMode; `.x` throws).
        #
        # The narrow answer here is still not narrow about Rust: an empty crate list produces no
        # `-p` arguments, so `workspace tests` keeps compiling the whole workspace. What this state
        # buys is the PostgreSQL matrix, measured at 35% of all gate time -- and it buys it only for
        # a diff that provably cannot reach the adapter.
        #
        # A WRONG TYPE MUST WIDEN, and `-eq $false` alone made it narrow. PowerShell coerces the
        # right operand's type onto the left, so the string 'false', the string 'False' and the
        # integer 0 all satisfy `-eq $false` -- measured under StrictMode 2.0, together with '0',
        # which widens, so the coercion is not even uniform. That inverts the whole rule this state
        # was added under: every way of being unsure runs everything. A producer emitting a JSON
        # string after a refactor, or a hand-written selection file, would have quietly run fewer
        # stages and reported the same green. `-is [bool]` first: only a real boolean false narrows,
        # and anything else -- wrong type, absent, null -- falls through to FULL.
        $rustInputs = $selection.PSObject.Properties['rustInputsChanged']
        if ($null -ne $rustInputs -and $rustInputs.Value -is [bool] -and $rustInputs.Value -eq $false) {
            $matrix = $false
            if ($selection.PSObject.Properties['matrix']) { $matrix = [bool]$selection.matrix }
            $reason = [string]($selection.PSObject.Properties['matrixReason']).Value
            return [ordered]@{
                full         = $false
                reason       = 'SCOPED: no Rust build input changed, so no crate is selected and no Rust stage runs'
                crates       = @()
                matrix       = $matrix
                matrixReason = $reason
                # #901 slice 2: THE ONLY PLACE `rust` IS FALSE. The comment above used to end "the
                # workspace still compiles": the selector proved no Rust input changed and the gate
                # then built and tested the whole workspace anyway -- 30 minutes cold, measured on the
                # runner, spent re-proving the tree `main` already proved. A run whose diff reaches no
                # Rust input has nothing for a Rust stage to measure. Every other branch here is `true`.
                rust         = $false
            }
        }
        return New-FullScope -Reason 'FULL: the scope selection names no crates, which would run nothing and pass'
    }
    $matrix = $true
    if ($selection.PSObject.Properties.Name -contains 'matrix') { $matrix = [bool]$selection.matrix }
    $matrixReason = ''
    if ($selection.PSObject.Properties.Name -contains 'matrixReason') { $matrixReason = [string]$selection.matrixReason }
    return [ordered]@{
        full = $false
        reason = "SCOPED: $($crates.Count) crate(s) selected"
        crates = $crates
        matrix = $matrix
        matrixReason = $matrixReason
        rust = $true
    }
}

# #903: resolved ONCE, here, so every later reader sees the same answer -- and so a manifest
# written by an early abort carries the scope too. Absent or unreadable means FULL.
# READ THE PARAMETER DEFENSIVELY, and the reason is structural rather than cautious: several
# ci/*.tests.ps1 cut REGIONS out of this file by anchor and run them as their own script,
# WITHOUT the param() block above. Under `Set-StrictMode -Version 2.0` a bare `$ScopeSelection`
# in such a slice throws "the variable cannot be retrieved because it has not been set" --
# measured: it reddened gate-target-dir and gate-run-abort, two suites that have nothing to do
# with scope. `Test-Path variable:` asks whether it exists instead of assuming it does.
$scopeArgument = if (Test-Path variable:ScopeSelection) { [string]$ScopeSelection } else { '' }
$script:gateScope = Read-ScopeSelection -Path $scopeArgument
# #464: DECIDED ONCE, same instant the crate scope is, from the same file. A Rust-only change
# never pays npm; a Studio-only change (which the crate graph reads as an empty selection with
# nothing to compile) still gets its own suite run.
$script:studioScopeIncluded = Test-StudioScopeChanged -Path $scopeArgument
# #901 slice 2: the two reach decisions, DECIDED ONCE beside the others. `rustStagesIncluded` is
# false only when the selection proved no Rust build input changed (`Read-ScopeSelection`'s one
# `rust = $false`); then every stage that compiles or runs Rust is recorded `notRun` with this reason
# instead of executed -- through `Invoke-Stage`, so the record says so stage by stage.
$script:rustStagesIncluded = -not (($script:gateScope['rust'] -is [bool]) -and (-not $script:gateScope['rust']))
$script:psSuitesScope = Get-PsSuitesScope -Path $scopeArgument
$script:scopeSkippedStages = @{}
if (-not $script:rustStagesIncluded) {
    foreach ($rustStage in @('contamination canary (ci-canary)', 'rustfmt', 'clippy (deny warnings)', 'workspace tests',
            'cli suites', 'schema catalog', 'schema baseline compatibility', 'schema conformance', 'locked metadata',
            'required-features coverage')) {
        $script:scopeSkippedStages[$rustStage] = 'no Rust build input changed (#901)'
    }
    Write-Host '[gate] Rust stages SKIPPED by scope - no Rust build input changed; coverage.rust is skipped and coverage.complete is false.' -ForegroundColor Yellow
}
if (-not $script:psSuitesScope.included) {
    Write-Host "[gate] PowerShell suites NARROWED by scope - only the event-contract sweep runs: $($script:psSuitesScope.reason)" -ForegroundColor Yellow
}
# #903: THE MATRIX DECISION, DECIDED ONCE. Measured over 169 manifests, the two PostgreSQL
# matrices are 10.3 minutes of a 26.2-minute run -- 35% of all gate time on this board -- and
# four of one day's seven pull requests touched neither Rust nor SQL and paid it anyway (X).
# This is the cheap half of the epic and the big one: a path predicate, not a crate graph.
#
# A SKIPPED MATRIX IS NOT A GREEN MATRIX, so this feeds `Get-RunCoverage` exactly as
# `-SkipPostgres` does: `coverage.postgres` reads `skipped` and `coverage.complete` reads
# false. A run that did not exercise persistence must not record that it did -- the same rule
# the checklist already carries for an ABSENT `scope` field.
# `Test-Path variable:` for the same reason the scope argument uses it two lines above: several
# ci/*.tests.ps1 cut REGIONS out of this file and run them WITHOUT the param() block, and under
# `Set-StrictMode -Version 2.0` a bare `$SkipPostgres` there throws. Measured: writing it plainly
# reddened gate-run-abort and gate-target-dir, two suites with nothing to do with PostgreSQL --
# the SECOND time this file has taught me that lesson today.
$skipPostgresFlag = if (Test-Path variable:SkipPostgres) { [bool]$SkipPostgres } else { $false }
$script:matrixSkipped = $skipPostgresFlag -or (-not $script:gateScope.matrix)
Write-Host "[gate] scope: $($script:gateScope.reason)" -ForegroundColor Cyan

$toolchain = '+1.97.1'
# #896: the exit code a stage body reports when it set none. 99 is the sentinel `merge-proof`
# (retired 2026-09-24) also ran behind; here it is what `Invoke-Stage` poisons `$LASTEXITCODE` with.
$MuteStageExitCode = 99

# #1053 item 7: FAIL-FAST IS A DECISION THE RUN MAKES ONCE, HERE -- not a property Invoke-Stage
# acquires on its own. Armed ABOVE the failed-stage list initialiser below, which is the line every
# ci/*.tests.ps1 uses as its slice-start anchor -- so a SLICE never inherits it. Do NOT write that
# anchor's text into a comment above it: Get-GateSlice takes the FIRST match, so a comment quoting
# it cuts the slice from the wrong place and the slice does not even parse. Measured; this comment
# did exactly that on its first draft.
#
# That boundary is load-bearing and it was found the hard way. Keying the skip on the abort flag
# alone made five suites red on their first gate -- gate-background-stage-evidence,
# gate-postgres-evidence, gate-stage-overlap, gate-stage-reddens, gate-stage-stderr-evidence -- all
# of which drive Invoke-Stage several times to observe several INDEPENDENT scenarios. A fixture
# stage failing in scenario one silently skipped scenario two, and their assertions were about
# stages that no longer ran. They were right and the change was wrong: a slice is not a run, and
# fail-fast is a statement about a run.
#
# Read through `Test-Path variable:` in Invoke-Stage for the same reason, so an unarmed slice
# answers "off" instead of throwing under StrictMode.
$script:failFastEnabled = $true
if (Test-Path 'variable:NoFailFast') { $script:failFastEnabled = -not [bool]$NoFailFast }

$failed = @()
# #909: what each PostgreSQL stage actually EXECUTED. Kept apart from $failed for the same reason
# $staleBinaryCount is (#822/#856): "this stage ran nothing" is not a finding about the code, it is a
# statement that the stage did not measure it. Populated from the count file postgres.ps1 writes;
# empty means no PostgreSQL stage ran at all, which is a legal run (-SkipPostgres) and not a zero.
$script:postgresExecution = [ordered]@{}
$script:postgresMatrixUnavailable = $false
# #956 step 2b: the two PostgreSQL matrices, started right after the workspace build and joined at
# their old positions under their old names. $null means "not started early": the join then runs
# the matrix in line, exactly as before. The two flags are a measurement of the optimisation's
# saving, published as a note -- never a verdict (#989).
$script:pgIgnoredEarly = $null
$script:pgCollationEarly = $null
$script:pgMatricesStartedEarly = $false
$script:pgMatricesOverlapped = $false
# #822: how many test binaries predated this run's start. Kept apart from $failed because it is
# not a failure of the code under test: it says whether the other stages measured this tree at all.
$staleBinaryCount = 0

# #822: A STALE TEST BINARY IS NOT A THIRD FAILURE. The freshness cross-check used to join $failed as
# a peer of 'workspace tests', and the summary line printed the two side by side -- so a reader
# went looking for the bug in 'workspace tests' when there had been no measurement of this head at
# all: those binaries were built from the previous head, which is a DIFFERENT PROGRAM, not a flaky
# result. The status stays RED (fail closed, as before); what changes is that the verdict says which
# of the two happened. The manifest already carries the same distinction as `instrumentSuspect`.
# THE ARMING SITE AND THE FILTER MUST SPELL IT THE SAME WAY, and a constant is the only version of
# that sentence a rename cannot break. #822 separated the cross-check from the reds it invalidates by
# EXCLUDING it from the peer list by name -- so the entry appended two thousand lines below and the
# name filtered here were two copies of one string with nothing holding them together.
#
# Measured before this change: renaming only the arming literal (one hyphen, the most ordinary edit
# anyone makes to a message) left ci/gate-verdict.tests.ps1 at 18/18 while the cross-check reappeared
# in the peer list -- #822's defect back, silently, at the next rename. The cells could not see it
# because they hand the decision the literal themselves.
#
# A cell that watches for the drift would have been the other remedy. This one removes the drift.
$FreshnessStageName = 'binary freshness cross-check'
# #956: the overlap alarm's own name, a CONSTANT for the same reason #822's is. It is not a stage:
# nothing ran under it, it has no record and no output tail, so a reader who finds it listed among
# real stages goes looking for a transcript that does not exist. Like the freshness cross-check it is
# the FRAME the run is read in, and `Get-GateVerdictLines` prints it as one.

function Get-GateVerdictLines {
    param(
        [AllowEmptyCollection()] [string[]] $Failed,
        [int] $StaleBinaryCount,
        # THE NAME IS PASSED IN, not repeated here. It is the same string the cross-check appends to
        # `$failed` two thousand lines below, and the two used to be independent literals: renaming
        # only the arming one left ci/gate-verdict.tests.ps1 at 18/18 while the cross-check went back
        # to being printed as a peer of the reds it invalidates -- #822's defect, restored silently
        # at the next rename, because the cells hand this function the literal themselves.
        #
        # A parameter keeps the function extractable (the suite cuts it out by anchor and dot-sources
        # it alone, so a script-scope constant would not travel with it) AND lets the caller be the
        # single place the string is written.
        [Parameter(Mandatory)] [string] $FreshnessStageName
    )
    $lines = New-Object System.Collections.Generic.List[string]
    # The cross-check's own entry is never printed as a peer; it is the frame the others are read in.
    # ORDINAL, not -cne: PowerShell's case-sensitive operators are still culture comparisons (#753).
    # #956: the overlap alarm used to be a second frame here. It is a NOTE now (see the alarm site):
    # a run's status is derived from its stages alone, so there is no overlap name in $Failed to frame.
    $others = @($Failed | Where-Object { -not [string]::Equals($_, $FreshnessStageName, [System.StringComparison]::Ordinal) })
    if ($StaleBinaryCount -gt 0) {
        $lines.Add("[gate] NOT A MEASUREMENT: $StaleBinaryCount test binary(ies) predate this run's start, so every stage that ran a test binary measured a DIFFERENT PROGRAM than this run's tree. This run's stage results are not readable as results for this head.")
        if ($others.Count -gt 0) {
            $lines.Add("[gate] RED - stages that failed while measuring that other program (not findings about this head): $($others -join ', ')")
        } else {
            $lines.Add('[gate] RED - no stage failed, and none of them measured this head.')
        }
        return $lines.ToArray()
    }
    if ($others.Count -gt 0) {
        $lines.Add("[gate] RED - failed stages: $($others -join ', ')")
    }
    return $lines.ToArray()
}
# (end #822)
$stageRecords = New-Object System.Collections.Generic.List[object]
# #956: the run's two instants, both stamped at the claim site (search: 'THE WAIT STARTS HERE').
# `runQueuedUtc` is the instant the gate asks for a slot and `runStartUtc` the instant it holds one;
# the manifest carries both and their difference as `slotWaitSecs`. A single stamp up here once
# served as the run's start, and a manifest read 64.8 min for a 38.4-minute run (#982) because a
# 26.5-minute wait for E: was inside it. Stamping the queued instant up here was not right either:
# across 98 no-wait runs it put a median 0.274 s (max 1.49 s) of dot-sourcing and git probes inside
# the wait, so a free slot never read 0. Nothing reads either name before the claim.
$runQueuedUtc = $null
$runStartUtc = $null
# #700: the slot this run holds, and how far it got. `slotOutcome` is Enter-GateSlot's word;
# `stagesCompleted` is set as the LAST statement inside the stage try; `slotRunEnded` is set where
# RUN-END is written. The finally reads all three so it can tell a run that left early from one
# that finished, and release only a claim that is this process's own.
$script:slotOutcome = $null
$script:stagesCompleted = $false
# #1019 review (B, twice): DEFINED HERE, not only at the required-features stage, so the manifest
# write on the CANARY-ABORT path (which runs before that stage even starts) can read them under
# `Set-StrictMode -Version 2.0` without throwing.
#
# `Unreadable` STARTS TRUE. The first version started it `$false`, which read as "checked, and
# nothing was excluded" -- exactly wrong for a run that aborts before the stage that would prove
# either claim ever starts. UNREPORTED IS THE DEFAULT; only a stage that actually read its own scope
# report earns `$false`, at the one place that sets it (the success branch, where `Excluded` is also
# filled in). Every failure path -- missing report, unparseable, wrong shape -- already sets `$true`
# explicitly and stays that way; this default just means an abort that never reaches any of them
# also lands on `$true` instead of silently inheriting a claim nobody made.
$script:requiredFeaturesExcluded = @()
$script:requiredFeaturesReportUnreadable = $true
# #1003 review (Codex P2): DEFINED HERE, not only at their own start sites, so the outer `finally`
# can reach them even when the run aborts before either background stage is ever started (the
# canary failing is the common case) -- `Set-StrictMode -Version 2.0` throws on a script-scope
# variable that was never assigned, and a `finally` block that throws while reaping a leaked
# process is a worse abort than the one it was cleaning up after.
$script:psSuitesStarted = $null
$script:studioStarted = $null
$script:pgIgnoredEarly = $null
$script:pgCollationEarly = $null
# #1003 review (X): the SAME class, on `Write-RunManifest` rather than the reap `finally` --
# `studioNodePresent`/`studioStartedEarly`/`studioOverlapped` are read into the manifest and are
# not assigned until well after the canary can already have aborted and called that function. `$false`
# is correct for all three on that path: node was not confirmed present, the stage was not started
# early, and it did not overlap anything, all because the run never got that far.
$script:studioNodePresent = $false
$script:studioStartedEarly = $false
$script:studioOverlapped = $false
# #207: every stage's captured output, concatenated, so a check at the end can ask about a
# transcript rather than about whichever stage happened to run last.
$script:allStageLines = @()
$script:slotRunEnded = $false
# #455: DECLARED HERE because `Write-RunManifest` reads both unconditionally, and the early
# target-provenance abort calls it long before the stage block assigns them. Under
# `Set-StrictMode -Version 2.0` an unassigned variable THROWS, so that abort would have died during
# serialization -- writing no manifest and never reaching its own `exit` (Codex P2 on #1009). The
# real values are computed after the stages; these are the honest "not measured yet" defaults for a
# run that ends before that point.
$script:psSuitesStartedEarly = $false
$script:psSuitesOverlapped = $false

# #755: ONE WRITER FOR THE TERMINAL LINE, used by both exits. #905 gave the stage block its own
# inline RUN-ABORT; this keeps that behaviour and adds the two things it left open.
#
# 1. THE REASON CANNOT BREAK THE LINE. A ledger line is `<utc> | gate | <event> | <detail>`, and an
#    exception message carrying a newline or a pipe would split one event into two, or invent a
#    field. The reason is base64 here, the same encoding RUN-START already uses for the target dir,
#    so a message from a `catch` is safe to record verbatim.
# 2. THE MANIFEST WRITE IS THE OTHER WAY A RUN ENDS WITHOUT RUN-END, and it was silent: the catch
#    printed MANIFEST WRITE FAILED and left the ledger with a START and nothing else.
#
# The guard is #905's own `slotRunEnded`, not a second flag: RUN-END sets it, so a run that ended
# properly writes no ABORT, and two callers cannot produce two terminal lines.
# What it still cannot cover, stated: a process killed from outside runs no `finally`, so a
# RUN-START with neither END nor ABORT remains "killed or still running" -- #700's liveness reader
# is the instrument for that, not this line.
function Write-RunAbort {
    param([Parameter(Mandatory)] [string] $Reason)
    if ($script:slotRunEnded) { return }
    $head = if ($gatedHeadAtStart) { $gatedHeadAtStart.Substring(0, [Math]::Min(12, $gatedHeadAtStart.Length)) } else { 'unknown' }
    Write-SlotEvent -Event 'RUN-ABORT' -Detail "head=$head reasonBase64=$(ConvertTo-SlotEventField -Value $Reason)"
    $script:slotRunEnded = $true
}
# (end #755)

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
# #909: one PostgreSQL stage, plus the answer to "did it measure anything".
#
# THE STAGE'S EXIT CODE CANNOT ANSWER THAT. `cargo test` that selects zero tests exits 0, so a stage
# that ran nothing is indistinguishable from one that ran everything and passed -- and the 80-line
# tail Invoke-Postgres publishes destroys the evidence either way. postgres.ps1 now writes a count to
# the file named here; this reads it back and reddens the stage when the run reports groups but
# executed nothing.
#
# 'unknown' does NOT redden. A missing or unreadable count file means the READER failed, not that the
# stage ran nothing, and turning "I could not see" into "it ran nothing" is exactly the inference that
# produced a false report on 2026-09-05. It is surfaced loudly and left to a human.
function Invoke-PostgresStage {
    param(
        [Parameter(Mandatory)] [string] $Name,
        # #956 step 2b: a matrix started early hands in the count file its child already wrote to and
        # a body that JOINS that child instead of running one. The defaults are the in-line path,
        # unchanged, which is also the path ci/gate-postgres-count.tests.ps1 drives.
        [string] $CountFile = '',
        [scriptblock] $Body = $null
    )

    $countFile = if ([string]::IsNullOrWhiteSpace($CountFile)) { [System.IO.Path]::GetTempFileName() } else { $CountFile }
    if ($null -eq $Body) { $Body = { Invoke-Postgres } }
    $previousCountFile = $env:GRAPHHELM_PG_COUNT_FILE
    $env:GRAPHHELM_PG_COUNT_FILE = $countFile
    try {
        Invoke-Stage $Name $Body -AlwaysRun | Out-Null
    } finally {
        $env:GRAPHHELM_PG_COUNT_FILE = $previousCountFile
    }

    $record = [ordered]@{ verdict = 'unknown'; executed = 0; groups = 0; stdoutLines = 0 }
    if (Test-Path -LiteralPath $countFile) {
        foreach ($line in (Get-Content -LiteralPath $countFile -ErrorAction SilentlyContinue)) {
            $pair = [regex]::Match($line, '^([A-Za-z]+)=(.*)$')
            if (-not $pair.Success) { continue }
            $key = $pair.Groups[1].Value
            $value = $pair.Groups[2].Value
            if ($record.Contains($key)) {
                $record[$key] = if ($key -eq 'verdict') {
                    $value
                } else {
                    $parsed = 0
                    if ([int]::TryParse($value, [ref] $parsed)) { $parsed } else { 0 }
                }
            }
        }
        Remove-Item -LiteralPath $countFile -Force -ErrorAction SilentlyContinue
    }
    $script:postgresExecution[$Name] = $record

    if ($record.verdict -eq 'none') {
        Write-Host "[gate] NOT A MEASUREMENT: '$Name' reported $($record.groups) binary summary(ies) and executed 0 tests." -ForegroundColor Magenta
        Write-Host '[gate] The stage exited 0 because a run that selects nothing succeeds. It measured nothing about this head.' -ForegroundColor Magenta
        if ($failed -notcontains $Name) { $script:failed = @($failed) + $Name }
    } elseif ($record.verdict -eq 'unknown') {
        Write-Host "[gate] '$Name' published no executed-test count; this run cannot say whether it measured anything." -ForegroundColor Yellow
    }
}

function Get-NonCLocale {
    # $IsWindows exists only in PowerShell Core, and under Set-StrictMode referencing it on Windows
    # PowerShell 5.1 is a terminating error. $env:OS is set on Windows in both.
    if ($env:OS -eq 'Windows_NT') { return 'English_United States.1252' }
    return 'en_US.UTF-8'
}

# #956 step 2b: START A MATRIX EARLY. postgres.ps1 is already a child process with its own random
# loopback port and private data directory, so two of them beside each other are isolated by
# construction; what they share is the cargo target, whose build lock serialises the compile step
# and nothing after it. A child reads the gate's environment at the instant of the spawn, so the
# count file and the locale are set around Start-BackgroundStage and restored after it; a start
# that fails answers $null and the join runs the matrix in line.
function New-PostgresStageJob {
    param([string] $SupportScriptPath)
    . $SupportScriptPath -InitializeJobSupportOnly
    return [GraphHelmPostgresJob]::new()
}

function Start-PostgresStageEarly {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [string] $Locale = '',
        # Same shape as Invoke-Postgres: the default is the real script, the parameter is what the
        # suite hands in when the function is cut out and driven where $PSScriptRoot is empty.
        [string] $ScriptPath = (Join-Path $PSScriptRoot 'postgres.ps1'),
        [string] $SupportScriptPath = (Join-Path $PSScriptRoot 'postgres.ps1'),
        [bool] $WindowsHost = ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT)
    )
    # Only Windows has this containment mechanism. Other hosts keep the existing inline matrix.
    if (-not $WindowsHost) { return $null }
    $job = $null
    try {
        $job = New-PostgresStageJob -SupportScriptPath $SupportScriptPath
        $countFile = [System.IO.Path]::GetTempFileName()
    } catch {
        if ($null -ne $job) { $job.Dispose() }
        Write-Host "[gate] $Name job setup failed ($($_.Exception.Message)); it will run in line." -ForegroundColor Yellow
        return $null
    }
    $previousCountFile = $env:GRAPHHELM_PG_COUNT_FILE
    $previousLocale = $env:GRAPHHELM_PG_LOCALE
    $env:GRAPHHELM_PG_COUNT_FILE = $countFile
    if (-not [string]::IsNullOrWhiteSpace($Locale)) { $env:GRAPHHELM_PG_LOCALE = $Locale }
    try {
        # -File cannot bind a native argument list to a PowerShell array parameter. Encode the
        # invocation so both locale axes receive the exact PostgreSQL-only Cargo vector.
        $quotedPath = $ScriptPath.Replace("'", "''")
        $supportPath = $SupportScriptPath.Replace("'", "''")
        $command = "`$ErrorActionPreference='Stop'; . '$supportPath' -InitializeJobSupportOnly; [GraphHelmPostgresJob]::Join('$($job.Name)'); & '$quotedPath' -TestArgs @('+1.97.1','test','-p','graphhelm-postgres-event-store','--all-features','--locked','--','--ignored','--test-threads=1'); exit `$LASTEXITCODE"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $started = Start-BackgroundStage -Name $Name -FilePath 'powershell' -WorkingDirectory $repositoryRoot `
            -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded)
    } catch {
        $job.Dispose()
        Remove-Item -LiteralPath $countFile -Force -ErrorAction SilentlyContinue
        throw
    } finally {
        $env:GRAPHHELM_PG_COUNT_FILE = $previousCountFile
        $env:GRAPHHELM_PG_LOCALE = $previousLocale
    }
    if ($null -eq $started) {
        $job.Dispose()
        Remove-Item -LiteralPath $countFile -Force -ErrorAction SilentlyContinue
        return $null
    }
    return [pscustomobject]@{ Name = $Name; Started = $started; CountFile = $countFile; Job = $job }
}

function Stop-PostgresStageEarly {
    param([Parameter(Mandatory)] $Early)
    # The retained Process handle pins the wrapper identity. Kill/wait it BEFORE draining the job:
    # otherwise a child still compiling Add-Type could join after an empty-job observation.
    $process = $Early.Started.Process
    if (-not $process.HasExited) { $process.Kill() }
    if (-not $process.WaitForExit(10000)) { throw 'PostgreSQL wrapper did not exit; retaining slot.' }
    $Early.Job.StopAndDrain(10000)
    $Early.Job.Dispose()
    Remove-Item -LiteralPath $Early.CountFile -Force -ErrorAction SilentlyContinue
}

# THE JOIN, at the stage's old position and under its old name, so the manifest keeps one shape and
# the count file is read by the same code either way. $null runs the matrix in line.
function Complete-PostgresStage {
    param([Parameter(Mandatory)] [string] $Name, [AllowNull()] $Early)
    if ($null -eq $Early) {
        Invoke-PostgresStage -Name $Name
        return
    }
    $script:pgStageToJoin = $Early.Started
    Invoke-PostgresStage -Name $Name -CountFile $Early.CountFile -Body { Complete-BackgroundStage -Started $script:pgStageToJoin }
}

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
        $quotedPath = $ScriptPath.Replace("'", "''")
        $command = "& '$quotedPath' -TestArgs @('+1.97.1','test','-p','graphhelm-postgres-event-store','--all-features','--locked','--','--ignored','--test-threads=1'); exit `$LASTEXITCODE"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $process = Start-Process -FilePath $hostExe -PassThru -NoNewWindow `
            -ArgumentList @(
                '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-EncodedCommand', $encoded
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

# #810: Select-GateEvidenceLines lives in its own file so it can be exercised against
# constructed transcripts (ci/gate-evidence.tests.ps1) without running the gate, the same
# split ci/slot-lock.ps1 uses. Sourced HERE, adjacent to its only caller below, rather than
# with the other dot-sources further down: Invoke-Stage is defined at this point and the
# ordering should not depend on where an unrelated block happens to sit.
. (Join-Path $PSScriptRoot 'gate-evidence.ps1')

# #751: A BUILD LOCK IS NOT A FINDING, AND NO EXIT CODE SEPARATES THE TWO. `cargo` cannot take the
# build directory's lock while another run holds it, and what it does then has two shapes. Measured
# on this machine, holding `<target>/debug/.cargo-build-lock`:
#
#   error: failed to open: <target>/debug/.cargo-build-lock
#   Caused by: The process cannot access the file because it is being used by another process. (os error 32)
#
# with exit 101 -- THE SAME CODE A FAILING TEST RETURNS. So widening the set of "bad" codes cannot
# work: there is no code that means contaminated and not locked. The signal has to be POSITIVE and
# textual. A non-zero exit says something failed, never what.
#
# THE TWO SHAPES ARE NOT THE SAME CLAIM, and conflating them re-creates this issue pointing the
# other way. The HARD shape means cargo never opened the lock, so nothing ran. The COOPERATIVE shape
# -- `Blocking waiting for file lock on build directory` -- means cargo WAITED AND THEN PROCEEDED:
# after it, the canary runs. A canary that waited, got the lock, ran, and found real contamination
# carries BOTH the wait line and its own failure, and reading the wait alone would file a genuine
# finding as "never ran" -- which tells the operator to re-run on a stable host when the honest
# instruction is that this environment is dirty. (Found in review of #833.)
function Test-GateHardCargoLock {
    # cargo could not open a lock at all: it never got to run anything, whatever else is printed.
    #
    # NAMED BY SHAPE, NOT BY FILE. The first version required the literal `.cargo-build-lock`, which
    # is only the target directory's lock -- and the target directory is the one lock an isolated
    # CARGO_TARGET_DIR actually removes. cargo also locks `~/.cargo/.package-cache`,
    # `.package-cache-mutate` and `.global-cache`, which are GLOBAL TO THE MACHINE and are therefore
    # the contended ones when several isolated gates run at once. A matcher listing lock filenames is
    # a deny-list that grows by one per review; cargo's own error shape does not.
    #
    #   error: failed to open: <any lock path>
    #   Caused by:
    #     The process cannot access the file because it is being used by another process. (os error 32)
    #
    # `failed to open` plus `os error 32` is that shape and says nothing about which lock. The pair is
    # required: `failed to open` alone is a plain I/O failure, and `os error 32` alone can appear in a
    # test's own output. (Raised on #833 by the orchestrator after G's gate hung on the package cache
    # with an isolated target -- the case the first matcher read as contamination.)
    param([AllowNull()] [AllowEmptyCollection()] [string[]] $Lines)

    if ($null -eq $Lines) { return $false }
    $sawFailedToOpen = $false
    $sawInUse = $false
    foreach ($line in $Lines) {
        if ($null -eq $line) { continue }
        $text = [string]$line
        # Ordinal: cargo's own words, one case, one producer.
        if ($text.IndexOf('failed to open', [System.StringComparison]::Ordinal) -ge 0) { $sawFailedToOpen = $true }
        if (($text.IndexOf('os error 32', [System.StringComparison]::Ordinal) -ge 0) -or
            ($text.IndexOf('being used by another process', [System.StringComparison]::Ordinal) -ge 0)) { $sawInUse = $true }
    }
    return ($sawFailedToOpen -and $sawInUse)
}

function Test-GateWaitedForCargoLock {
    # cargo queued behind another run. On its own this says NOTHING about whether the canary ran --
    # only that it started late.
    #
    # The prefix is deliberately cut before the lock's name: cargo prints `... on build directory`
    # and `... on package cache`, and this must match both. Matching the longer form would have
    # covered only the lock an isolated target already removes.
    param([AllowNull()] [AllowEmptyCollection()] [string[]] $Lines)

    if ($null -eq $Lines) { return $false }
    foreach ($line in $Lines) {
        if ($null -eq $line) { continue }
        if (([string]$line).IndexOf('Blocking waiting for file lock', [System.StringComparison]::Ordinal) -ge 0) { return $true }
    }
    return $false
}
function Test-GateCanaryProducedResult {
    # Did the test harness reach the point of reporting? `cargo test` prints one `test result:` line
    # per binary, including for binaries with no tests, so its presence is evidence the run got past
    # building and locking. This is what turns a wait into a wait rather than a non-run.
    param([AllowNull()] [AllowEmptyCollection()] [string[]] $Lines)

    if ($null -eq $Lines) { return $false }
    foreach ($line in $Lines) {
        if ($null -eq $line) { continue }
        $text = [string]$line
        if ($text.IndexOf('test result:', [System.StringComparison]::Ordinal) -ge 0) { return $true }
        if (($text.IndexOf('running ', [System.StringComparison]::Ordinal) -ge 0) -and
            ($text.IndexOf(' test', [System.StringComparison]::Ordinal) -ge 0)) { return $true }
    }
    return $false
}
function Invoke-Stage {
    # #1053 item 7: -AlwaysRun exempts a stage from the fail-fast skip below. It is carried by the
    # three JOINS of background children and by nothing else: a child that was already started must
    # be reaped whatever the verdict, or the run leaks a process past its own lifetime -- the #1003
    # defect, re-armed by the thing meant to save time.
    param([string] $Name, [scriptblock] $Body, [switch] $AlwaysRun)

    # #1053 item 7: A STAGE NOBODY WILL READ IS NOT WORTH PAYING FOR, AND ITS ABSENCE IS RECORDED.
    #
    # Measured over the 370 receipts committed to .factory/gate-runs before the store was retired on
    # 2026-09-24: 39 % of runs are RED, a red run medians
    # 1734 s against GREEN's 1664 s, and the FIRST failing stage ends at a median of 795 s -- so a
    # red gate spends a further 940 s after its answer is already known. 56 % of red runs have
    # exactly ONE failing stage, so for most of them this skips nothing a reader would have used;
    # 37 % have more than one, and those pay a second gate, which is far cheaper now that a re-run
    # reuses a vouched target.
    #
    # `passed = $null` IS LOAD-BEARING and is the whole reason this is not a silent early `return`.
    # `$false` would fabricate a failure this run never observed, `$true` would fabricate a green,
    # and OMITTING the entry makes "not run" indistinguishable from "does not exist in this build"
    # -- the exact confusion every other refusal in this file exists to prevent. A reader that
    # treats $null as falsy sees "not passed", which is true; one that treats it as a verdict is
    # wrong, and `notRun` beside it says so in a word.
    # #901 slice 2: A STAGE THE DIFF CANNOT REACH is recorded the same way -- `passed = $null`,
    # `notRun` -- with the scope's reason as the cause, never a fabricated pass. Read through
    # `Test-Path` because the suites that slice this function out run it without the script body.
    if ((Test-Path 'variable:script:scopeSkippedStages') -and $script:scopeSkippedStages -and $script:scopeSkippedStages.ContainsKey($Name)) {
        Write-Host ''
        Write-Host "[gate] SKIPPED by scope: $Name -- $($script:scopeSkippedStages[$Name])" -ForegroundColor Yellow
        # The capture of a stage that did not run is EMPTY, never the previous stage's: a caller that
        # reads it next (the canary's outcome) must not judge another stage's words. Measured: left
        # unset, the canary call below threw under StrictMode and the whole run died (e2e, #901).
        $script:lastStageLines = @()
        $script:stageRecords.Add([ordered]@{
                name        = $Name
                passed      = $null
                notRun      = $true
                notRunCause = "scope: $($script:scopeSkippedStages[$Name])"
                startedUtc  = [DateTime]::UtcNow.ToString('o')
                endedUtc    = [DateTime]::UtcNow.ToString('o')
            })
        return 0
    }
    if ((-not $AlwaysRun) -and (Test-Path 'variable:script:failFastEnabled') -and $script:failFastEnabled -and (Test-Path 'variable:script:abortAfterStage') -and $script:abortAfterStage) {
        Write-Host ''
        Write-Host "[gate] SKIPPED: $Name -- the run already failed at '$($script:abortAfterStage)' (#1053; -NoFailFast runs every stage)" -ForegroundColor Yellow
        $script:stageRecords.Add([ordered]@{
                name        = $Name
                passed      = $null
                notRun      = $true
                notRunCause = [string]$script:abortAfterStage
                startedUtc  = [DateTime]::UtcNow.ToString('o')
                endedUtc    = [DateTime]::UtcNow.ToString('o')
            })
        return 0
    }

    Write-Host ''
    Write-Host "[gate] $Name" -ForegroundColor Cyan
    # Native tools write progress to stderr. Under Windows PowerShell 5.1 a redirected native stderr
    # line becomes a NativeCommandError that $ErrorActionPreference='Stop' promotes to a terminating
    # error, so a native process is judged by its exit code instead.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    # #841: CREATED BEFORE THE TRY, so the `finally` below always has a list to publish. A body that
    # throws before this line existed inside the try would leave `$capturedLines` undefined, and the
    # publication in the finally would then throw over the top of the original error -- replacing the
    # cause with a complaint about the instrument that was trying to record it.
    $capturedLines = New-Object System.Collections.Generic.List[string]
    # #956: a stage whose WORK began earlier -- started in the background and only joined here --
    # says so by setting $script:stageStartedOverrideUtc from inside its own body, because the body
    # is the only code that knows whether the early start actually happened. Read in the finally,
    # for #841's reason, and cleared there, so it can never leak into the next stage's record.
    $stageStartedUtc = [DateTime]::UtcNow.ToString('o')
    $stageEndedUtc = $null
    $script:stageStartedOverrideUtc = $null
    $script:stageEndedOverrideUtc = $null
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
        # Invoke-Postgres publishes its detached child's files with Write-Host so that reading
        # them cannot become this function's numeric return value. Write-Host is information
        # stream 6 in Windows PowerShell 5.1 and PowerShell Core. Merge only that stream into the
        # success stream here, then redact once before both evidence sinks. Without 6>&1 the child
        # is visible at best in the console but absent from outputTail (#484).
        # #896: POISON, DO NOT CLEAR. `$LASTEXITCODE` is written only by a native command, so a body
        # that fails on the PowerShell side -- a lookup over MAX_PATH, a missing root, a cmdlet that
        # throws -- writes nothing and the stage would inherit its NEIGHBOUR's verdict. Clearing to 0
        # would be worse than the inheritance: a mute body would then PASS every time, and nobody
        # investigates good news. The sentinel fails in the safe direction: a body that never spoke
        # exits $MuteStageExitCode and reddens, and the neighbour's value is destroyed before the body
        # runs so inheritance is impossible. `Invoke-Postgres` already sets the code itself (:179);
        # this moves that discipline from memory into the structure.
        $global:LASTEXITCODE = $MuteStageExitCode
        #
        # 2>&1 merges the ERROR stream for the same reason, one stream later (#816). Native tools
        # put their diagnostics on stderr -- rustc's errors, a test harness's panic, cargo's own
        # complaints -- and stderr is in neither the success stream nor stream 6, so the shell
        # printed those lines straight to the console and $capturedLines never saw them. A stage
        # could fail with its cause on screen and record a manifest naming the stage and nothing
        # else. This ADDS a stream rather than trading one away: without 6>&1 the detached child
        # of Invoke-Postgres goes missing again, which is what #484 already paid for.
        #
        # Safe here precisely because of the paragraph above the try: $ErrorActionPreference is
        # 'Continue' for the duration, so the NativeCommandError that 5.1 manufactures for a
        # redirected native stderr line stays non-terminating and arrives as an ordinary record.
        & $Body 2>&1 6>&1 | ForEach-Object {
            $line = Protect-GateEvidenceLine -Line ([string]$_)
            $capturedLines.Add($line)
            Write-Host $line
        }
        $code = $LASTEXITCODE
        # #751: the FULL capture, not the aimed tail. `Select-GateEvidenceLines` anchors its budget
        # on the first line that names a failure, and `Blocking waiting for file lock` is printed
        # BEFORE any of that -- so the one line that identifies a lock is exactly the line the tail
        # is entitled to drop.
    } finally {
        $ErrorActionPreference = $previous
        $stopwatch.Stop()
        # #841: PUBLISHED IN THE FINALLY, so these two always describe the stage that just ran --
        # including one whose body THREW. As the last statements of the `try` they were skipped by
        # any terminating error, and the effect was not that they went empty; it was that
        # `$script:lastStageLines` kept the PREVIOUS STAGE'S capture. A reader would then answer
        # confidently about the wrong subject, which is worse than answering nothing: an empty read
        # reddens, a neighbour's transcript does not.
        #
        # Measured on the real sliced Invoke-Stage before the move: stage A printed STAGE-A-MARKER
        # and passed, stage B printed STAGE-B-MARKER and threw, and afterwards
        # `$script:lastStageLines` held STAGE-A-MARKER while `$script:allStageLines` had never
        # heard of stage B at all -- B's bytes were in `$capturedLines` and were simply not
        # published. `ci/gate-stage-verdict-source.tests.ps1` drives exactly that.
        #
        # Harmless in the gate as it stands, because today a throwing body aborts the run through
        # the outer finally and nothing reads the stale value afterwards. It is written for the
        # SECOND consumer: the day a stage reads these after a neighbour throws, it reads the wrong
        # stage's transcript, and nothing about the value says so.
        $script:lastStageLines = $capturedLines.ToArray()
        # #207: and the running concatenation, which feeds the run transcript the
        # `required-features coverage` stage writes. A target a manifest gates behind a feature
        # prints its binary in whichever stage builds it, so a check reading only
        # `$script:lastStageLines` would ask the wrong transcript and answer "not run" about a
        # target that ran earlier -- and a stage that threw used to contribute nothing here at all,
        # so whatever it had already printed was lost from the record.
        # #956: a stage joined from the background emits its child's output through `Write-Output`
        # inside the body, so those lines are in `$capturedLines` and reach this concatenation like
        # any other stage's -- the required-features check still sees a target the overlapped stage built.
        $script:allStageLines += $capturedLines.ToArray()
        # #956: the overrides are read HERE for #841's reason. Inside the try they were skipped by a
        # throwing body, and a start override left set would then stamp the NEXT stage's record.
        if (-not [string]::IsNullOrWhiteSpace([string]$script:stageStartedOverrideUtc)) {
            $stageStartedUtc = [string]$script:stageStartedOverrideUtc
            $script:stageStartedOverrideUtc = $null
        }
        if (-not [string]::IsNullOrWhiteSpace([string]$script:stageEndedOverrideUtc)) {
            $stageEndedUtc = [string]$script:stageEndedOverrideUtc
            $script:stageEndedOverrideUtc = $null
        }
    }
    if ($code -ne 0) {
        Write-Host "[gate] FAILED: $Name (exit $code)" -ForegroundColor Red
        $script:failed += $Name
        # #1053 item 7: ARM THE SKIP HERE, never abort from inside this function. The manifest write
        # far below is what every other session reads, and it must still run in full -- a fail-fast
        # that skipped the record would trade 940 s of compute for the evidence the run exists to
        # produce. Setting a flag lets the remaining `Invoke-Stage` calls record themselves as
        # `notRun` and lets the tail of the script finish normally.
        # The run decided this once, above the slice boundary; this only records WHERE it tripped.
        # An unarmed context (a slice) never sets the flag, so nothing downstream can skip.
        if ((Test-Path 'variable:script:failFastEnabled') -and $script:failFastEnabled) {
            if (-not (Test-Path 'variable:script:abortAfterStage') -or (-not $script:abortAfterStage)) {
                $script:abortAfterStage = $Name
            }
        }
    }
    $record = [ordered]@{
        name         = $Name
        passed       = ($code -eq 0)
        exitCode     = $code
        # The stopwatch measures THIS call. For a stage joined from the background that is the join,
        # not the work, so the duration comes from the same pair of instants the record publishes --
        # one number, derived from the two fields beside it, unable to disagree with them.
        wallTimeSecs = $(
            if ($null -ne $stageEndedUtc) {
                [math]::Round(([DateTime]::Parse($stageEndedUtc).ToUniversalTime() - [DateTime]::Parse($stageStartedUtc).ToUniversalTime()).TotalSeconds, 3)
            } else {
                [math]::Round($stopwatch.Elapsed.TotalSeconds, 3)
            })
        # #956: WHEN, not only how long. A sum of durations cannot tell a 32-minute serial run from
        # a 27-minute overlapped one -- only the intervals can, and `Test-StageOverlapped` reads
        # exactly these two fields, so the manifest publishes the evidence for its own claim.
        startedUtc   = $stageStartedUtc
        endedUtc     = $(if ($null -ne $stageEndedUtc) { $stageEndedUtc } else { [DateTime]::UtcNow.ToString('o') })
    }
    if ($code -ne 0) {
        # A 40-line budget, but AIMED rather than sliced from the end (#810). Taking the last 40
        # lines carries the panic site only when the failure is the last thing the stage printed.
        # `workspace tests` runs cargo with --no-fail-fast, which is the instruction to keep going
        # after a binary fails, so every remaining binary prints its own trailing summary and the
        # failing one's `failures:` block ends up 40+ lines from the end. Measured over 29
        # manifests: single-binary stages named their failure in 10 of 10 captured tails, the
        # workspace stage in 2 of 12 - the other ten holding a FULL forty lines of other binaries
        # reporting `0 passed; 0 failed`. Select-GateEvidenceLines anchors on the first line that
        # names a failure and still keeps the stage's own ending; with no such line it returns the
        # last 40, which is what this did before and is right for the stages where it worked.
        $tailLines = @(Select-GateEvidenceLines -Lines $capturedLines.ToArray() -Budget 40)
        # AN EMPTY TAIL HAS TO SAY SO. Capturing stderr above removes the commonest way evidence
        # went missing, and `outputTail: []` still cannot distinguish a stage that printed nothing
        # from one whose output was lost some other way -- two states with one representation, the
        # shape #751 and #755 were both about. A reader who sees an empty array cannot tell whether
        # to go looking for the missing lines or to accept that there were none, so the record says
        # which, in words, the way #858 writes `head=unknown` rather than leaving the field blank.
        if ($tailLines.Count -eq 0) {
            $tailLines = @("<absent: exit $code with no output on stdout, stderr or the information stream>")
        }
        $record.outputTail = $tailLines
    }
    $script:stageRecords.Add($record)
    return $code
}

# #751: WHICH OF THE TWO THINGS HAPPENED. "The canary caught contamination" and "the canary never
# ran" are opposite claims about the same run, and the durable manifest is what #674(b) reads to
# decide a merge. Recording a lock as ABORTED-BY-CANARY tells every later reader that this head's
# build environment is untrustworthy, when what actually happened was a queue.
#
# EXTRACTED so the decision can be exercised without a real cargo and a real lock. Extracting a
# predicate MOVES a defect unless the arm is shown to use it, so the suite also asserts that the
# canary arm calls this function rather than repeating the comparison.
function Get-CanaryOutcome {
    param(
        [Parameter(Mandatory)] [int] $ExitCode,
        [AllowNull()] [AllowEmptyCollection()] [string[]] $Lines
    )

    if ($ExitCode -eq 0) {
        return [ordered]@{
            passed            = $true
            status            = 'GREEN'
            cargoLockObserved = $false
            reason            = $null
        }
    }
    # DID IT REPORT? THAT IS THE WHOLE QUESTION, AND IT IS ASKED FIRST. A transcript carrying
    # `test result:` (or the `running N tests` the harness prints before executing) is a canary that
    # RAN -- whatever else its output happens to contain -- and a canary that ran and exited non-zero
    # made a finding.
    #
    # Asked first because the lock signatures are matched over the WHOLE transcript, so a genuine
    # contamination whose output happens to carry `failed to open` on one line and `being used by
    # another process` on another would otherwise be read as a lock. (Found by C on #833.) That is
    # this issue's defect re-entering through the hard shape after `5e1c1d71` removed it from the
    # waiting one: the accumulating matcher cannot tell a lock's own two lines from two unrelated
    # ones, and it does not have to -- the result line already answers the question it is guessing at.
    if (Test-GateCanaryProducedResult -Lines $Lines) {
        return [ordered]@{
            passed            = $false
            status            = 'ABORTED-BY-CANARY'
            cargoLockObserved = $false
            reason            = 'the canary ran and reported a contaminated build environment'
        }
    }
    # From here the canary did NOT report, so nothing below is a finding -- only a description of
    # which way it failed to run. NO LOCK IS NAMED: the matcher does not key on a filename and the
    # reason must not claim one either. Saying "the build directory lock" was wrong in the very
    # commit whose point is that the contended lock is usually the machine-global package cache.
    if (Test-GateHardCargoLock -Lines $Lines) {
        return [ordered]@{
            passed            = $false
            status            = 'HARNESS-BROKE'
            cargoLockObserved = $true
            reason            = 'the canary never ran: cargo could not open a lock it needed'
        }
    }
    if (Test-GateWaitedForCargoLock -Lines $Lines) {
        return [ordered]@{
            passed            = $false
            status            = 'HARNESS-BROKE'
            cargoLockObserved = $true
            reason            = 'the canary never ran: it waited for a cargo lock and never reported a result'
        }
    }
    return [ordered]@{
        passed            = $false
        status            = 'HARNESS-BROKE'
        cargoLockObserved = $false
        reason            = 'the canary never reported a result, so nothing here is a finding about this environment'
    }
}
# #956: did this stage actually run alongside another one?
#
# A CLAIM THE GATE MAKES ABOUT ITSELF HAS TO BE CHECKABLE, or it decays in silence. Overlapping one
# stage is worth ~5 minutes a run and the failure mode is invisible: put the stage back in line and
# every run still passes, every stage is still listed, and the only symptom is minutes nobody
# measures. So the run reads its OWN records at the end and reddens when the overlap it was built
# for did not happen.
#
# The comparison is on the INTERVALS, never on a flag the code sets about itself -- a boolean saying
# "we started it early" stays true after someone moves the start back down. Touching endpoints do
# not count: end == start is the serial case measured to the second. And ABSENT IS NOT OVERLAPPED:
# a record without timestamps (every manifest written before this change) answers false rather than
# throwing or being read as concurrency.
function Test-StageOverlapped {
    param(
        [Parameter(Mandatory)] [AllowEmptyCollection()] $Records,
        [Parameter(Mandatory)] [string] $Name
    )
    $toUtc = {
        param($value)
        if ([string]::IsNullOrWhiteSpace([string]$value)) { return $null }
        $parsed = [DateTime]::MinValue
        if ([DateTime]::TryParse([string]$value, [ref] $parsed)) { return $parsed.ToUniversalTime() }
        return $null
    }
    $subject = $null
    $others = @()
    # [object[]], NOT @(). This file already documents the trap 40 lines below, at the manifest's
    # `stages` field, and I wrote this loop anyway: under this machine's Windows PowerShell 5.1,
    # `@(<a System.Collections.Generic.List[object] VARIABLE>)` throws "Argument types do not match"
    # UNCONDITIONALLY. `$script:stageRecords` is exactly such a list, so on any run that COMPLETES
    # its stages this line throws, between the last stage and the manifest -- a guaranteed silent
    # loss of the whole run's evidence. The cast handles every shape this can receive: a List, a
    # plain array, a lone record, and $null (0 iterations).
    #
    # THIS IS WHAT KILLED THE TWO FULL RUNS AT THIS HEAD. Evidence, and this comment has now said
    # both things, so read the evidence rather than the sentence: manifest
    # `d0b304e9977c-20260907T034614.685Z-85178f23.json` is a complete 57-stage GREEN run WITH the
    # cast; the two runs before it died with the cast absent. A PASSING STAGE PRINTS NO COMPLETION
    # LINE -- which is what misled the first reading of those logs -- so the log going quiet after
    # the last stage's output does not mean that stage was still running. Measured against the
    # successful log: each dead run's final line is line 5689 of 5693, and the next thing printed
    # is the manifest commit. They finished all 57 stages and died in the four-line gap where this
    # predicate is called.
    #
    # ONE THING REMAINS UNEXPLAINED and is left stated rather than tidied: under this file's `Stop`
    # preference the throw should have reached the runner's `*>` redirect, and neither dead log
    # carries its text. The cause is established; that silence is not.
    foreach ($record in ([object[]]$Records)) {
        if ($null -eq $record) { continue }
        $recordName = [string]$record['name']
        $from = & $toUtc $record['startedUtc']
        $to = & $toUtc $record['endedUtc']
        if ($null -eq $from -or $null -eq $to) { continue }
        if ($recordName -eq $Name -and $null -eq $subject) {
            $subject = [pscustomobject]@{ From = $from; To = $to }
        } elseif ($recordName -ne $Name) {
            $others += [pscustomobject]@{ From = $from; To = $to }
        }
    }
    if ($null -eq $subject) { return $false }
    foreach ($other in $others) {
        if ($subject.From -lt $other.To -and $other.From -lt $subject.To) { return $true }
    }
    return $false
}

# #956: start a stage's work now, account for it later at the place it has always been recorded.
#
# The child is a PowerShell process and touches no cargo, so it holds no target-directory lock, no
# port and no temp cluster -- which is why THIS stage is the one that can move while the Rust stages
# cannot. Output goes to FILES rather than a pipe: a pipe makes the parent block on the reader and
# there is no overlap left to have.
function Start-BackgroundStage {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $FilePath,
        [Parameter(Mandatory)] [string[]] $ArgumentList,
        [Parameter(Mandatory)] [string] $WorkingDirectory
    )
    $stem = Join-Path ([System.IO.Path]::GetTempPath()) ("graphhelm-bg-" + [guid]::NewGuid().ToString('N'))
    $out = "$stem.out"
    $err = "$stem.err"
    try {
        $process = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -WorkingDirectory $WorkingDirectory `
            -RedirectStandardOutput $out -RedirectStandardError $err -WindowStyle Hidden -PassThru
    } catch {
        # FAIL INTO THE SLOW PATH, never into a skipped stage: the join runs the same command in line
        # and the run is exactly as long as it used to be, with a line saying why.
        Write-Host "[gate] $Name could not be started early ($($_.Exception.Message)); it will run in line." -ForegroundColor Yellow
        return $null
    }
    # TOUCH THE HANDLE WHILE THE PROCESS IS STILL ALIVE. Caught by the guard cell, not by reading:
    # `Start-Process -PassThru` hands back a Process whose `.ExitCode` is $null after WaitForExit
    # unless .NET has cached the native handle, and reading `.Handle` is what caches it. Without this
    # line the join returned $null, `$LASTEXITCODE` became $null, and a background stage that failed
    # with exit 3 was recorded as a PASS -- the exact swallowed failure this change had to not have.
    $null = $process.Handle
    Write-Host "[gate] $Name started early (pid $($process.Id)); it runs while the Rust stages do." -ForegroundColor DarkGray
    return [pscustomobject]@{
        Name       = $Name
        Process    = $process
        Out        = $out
        Err        = $err
        StartedUtc = [DateTime]::UtcNow.ToString('o')
    }
}

# The join. Emits the child's own output so `Invoke-Stage` captures, redacts and tails it exactly as
# it would for a stage that ran here, and sets the exit code the verdict reads: `$LASTEXITCODE` is
# written by native commands, and a function that forgot to set it would hand the stage the AMBIENT
# code of whatever ran last (#896).
function Complete-BackgroundStage {
    param([Parameter(Mandatory)] [AllowNull()] $Started)
    if ($null -eq $Started) { return $null }
    $Started.Process.WaitForExit()
    $code = $Started.Process.ExitCode
    # #956: THE CHILD'S OWN INTERVAL, from the kernel, not this function's stopwatch. Without this the
    # joined stage records the duration of the JOIN -- about zero seconds -- and `wallTimeSecs` is
    # exactly the field #956 sums to measure its own progress. A change that buys five minutes and
    # makes the meter unreadable has not been measured, it has been believed. `StartTime`/`ExitTime`
    # need the cached handle for the same reason `ExitCode` does; if the OS refuses them anyway, fall
    # back to the launch instant this object already recorded rather than inventing one.
    try {
        $script:stageStartedOverrideUtc = $Started.Process.StartTime.ToUniversalTime().ToString('o')
        $script:stageEndedOverrideUtc = $Started.Process.ExitTime.ToUniversalTime().ToString('o')
    } catch {
        Write-Host "[gate] $($Started.Name): the OS would not give the child's own start and exit times ($($_.Exception.Message)); using the launch instant." -ForegroundColor DarkGray
        $script:stageStartedOverrideUtc = $Started.StartedUtc
        $script:stageEndedOverrideUtc = [DateTime]::UtcNow.ToString('o')
    }
    foreach ($path in @($Started.Out, $Started.Err)) {
        if (Test-Path -LiteralPath $path) {
            # WRITE-HOST, NOT WRITE-OUTPUT, and it is the difference between a stage that speaks and
            # one that is recorded as mute. `Write-Output` puts the child's lines on THIS FUNCTION'S
            # success stream, and the call site assigns that stream -- `$joined = Complete-Background
            # Stage ...` -- so the assignment consumed the lines together with the exit code and
            # `Invoke-Stage` captured nothing. Measured on #1009's own red: the runner printed
            # `HARNESS-BROKE in: <name> (exit 2)` and the manifest recorded `outputTail: ["<absent:
            # exit 2 with no output on stdout, stderr or the information stream>"]`. 1971 seconds,
            # zero bytes, and the absence marker made it read as a child that had nothing to say.
            #
            # Write-Host is information stream 6, which `Invoke-Stage` merges with `6>&1` and
            # redacts, and which an assignment cannot swallow. `Invoke-Postgres` already publishes
            # its own detached child's files this way for exactly this reason, and the comment above
            # `& $Body` in Invoke-Stage states the rule; this stage was the one place that did not
            # follow it.
            foreach ($line in [System.IO.File]::ReadAllLines($path)) { Write-Host $line }
            # Named, never silent: a gate that swallows an exception without a word teaches the next
            # reader that the failure did not happen. Leaving a temp file behind is not fatal, so the
            # stage carries on -- but it says so.
            try {
                Remove-Item -LiteralPath $path -Force
            } catch {
                Write-Host "[gate] could not remove $path after joining $($Started.Name): $($_.Exception.Message)" -ForegroundColor DarkGray
            }
        }
    }
    $global:LASTEXITCODE = $code
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
# #956: how long a run waited for its slot, from two instants the run itself recorded. A start
# before its own queue instant is clock skew; it is published as absent, so skew cannot mint a
# saving and cannot pass for an instant claim either.
function Get-SlotWaitSecs {
    param([Parameter(Mandatory)] [DateTime] $QueuedUtc, [Parameter(Mandatory)] [DateTime] $StartUtc)
    $secs = ($StartUtc.ToUniversalTime() - $QueuedUtc.ToUniversalTime()).TotalSeconds
    # Negative is clock skew: an unknown, and the manifest's rule is that unknown is ABSENT, never 0.
    # A reader summing the field skips $null; it would have counted a 0 as an instant claim.
    if ($secs -lt 0) { return $null }
    return [math]::Round($secs, 3)
}

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
# ci/slot-lock.tests.ps1 and design note #200 for the full account of why a boolean
# `present` field could not tell "genuinely no lock" apart from "looked in the wrong place", and why
# that distinction is now a `status` tag with three states instead of two.
. (Join-Path $PSScriptRoot 'slot-lock.ps1')
. (Join-Path $PSScriptRoot 'manifest-name.ps1')
. (Join-Path $PSScriptRoot 'run-class.ps1')
# #904: the hash primitives a content-addressed reuse proof is built from --
# Get-Sha256Hex, Sort-Ordinal, Join-Elements, New-HashPreimage, Get-CrateInputHash,
# Get-ToolchainId. Dot-sourced rather than copied: the preimage format is the part that must not
# drift, and two copies of a cache-key format is two keys.
. (Join-Path $PSScriptRoot 'crate-input-hash.ps1')

# #152: one `--no-run --message-format=json` pass over the whole workspace enumerates every test
# binary (unit-test binaries per crate, integration-test binaries per crate including each
# apps/cli/tests/*.rs suite) in one shot - hashing and mtime-checking them here, BEFORE the
# human-facing stages below run the same tests for real, means those later stages execute against
# binaries this pass already fingerprinted, not a second, potentially-different build. cargo's own
# build caching makes the later stages' own compilation a no-op reuse of exactly what got hashed
# here, so the fingerprint stays true to what actually ran.

# BEGIN Get-RustfmtPlan
# #895: `cargo fmt --all` puts every workspace target on ONE command line. Windows refuses a
# command line over 32767 characters (`os error 206`) before rustfmt reads a byte, so on a long
# bench path the stage went red with no formatting cause. The plan is decided from LENGTHS: one
# `--all` invocation when it fits, one per package when it does not, chunks of explicit files when
# a single package does not fit either. $Packages: ordered map package-name -> absolute target paths.
# The length model is the joined paths plus 80 characters for the fixed part of the line; a
# pessimistic model errs towards splitting, which costs a few process launches, never a red.
function Get-RustfmtPlan {
    param(
        [Parameter(Mandatory)] [System.Collections.IDictionary] $Packages,
        [int] $Limit = 32767
    )
    $fixed = 80
    $plan = New-Object System.Collections.Generic.List[object]
    $all = @($Packages.Values | ForEach-Object { $_ })
    $allLength = $fixed + (@($all | ForEach-Object { $_.Length + 1 } | Measure-Object -Sum).Sum)
    if ($allLength -le $Limit) {
        $plan.Add([pscustomobject]@{ mode = 'all'; package = $null; files = $all })
        # No leading comma: a one-element array unrolls to a scalar and every caller wraps the
        # call in @(); a leading comma would hand a wrapped caller ONE element holding the array.
        return $plan.ToArray()
    }
    foreach ($name in $Packages.Keys) {
        $files = @($Packages[$name])
        $pkgLength = $fixed + (@($files | ForEach-Object { $_.Length + 1 } | Measure-Object -Sum).Sum)
        if ($pkgLength -le $Limit) {
            $plan.Add([pscustomobject]@{ mode = 'package'; package = $name; files = $files })
            continue
        }
        # One package alone does not fit: run rustfmt on explicit files, greedily under the limit.
        $chunk = New-Object System.Collections.Generic.List[string]
        $chunkLength = $fixed
        foreach ($f in $files) {
            if ($chunk.Count -gt 0 -and ($chunkLength + $f.Length + 1) -gt $Limit) {
                $plan.Add([pscustomobject]@{ mode = 'files'; package = $name; files = $chunk.ToArray() })
                $chunk = New-Object System.Collections.Generic.List[string]
                $chunkLength = $fixed
            }
            $chunk.Add($f)
            $chunkLength += $f.Length + 1
        }
        if ($chunk.Count -gt 0) { $plan.Add([pscustomobject]@{ mode = 'files'; package = $name; files = $chunk.ToArray() }) }
    }
    return $plan.ToArray()
}
# END Get-RustfmtPlan

# BEGIN Get-RustfmtHarnessNote
# A rustfmt red whose tail carries `os error 206` is the harness (the command line), not the code:
# the note names it so the manifest reader does not go looking for a formatting diff that never
# existed. Any other red is a verdict and gets no note.
function Get-RustfmtHarnessNote {
    param([string[]] $Tail, [int] $BenchPathLength)
    foreach ($line in @($Tail)) {
        if ($line -match 'os error 206') {
            return "[gate] rustfmt: command line too long (os error 206) - bench path is $BenchPathLength characters; this red is a harness condition, not a formatting verdict (#895)"
        }
    }
    return $null
}
# END Get-RustfmtHarnessNote

# BEGIN Get-RustfmtStageExit
# The stage's verdict over its invocations: any invocation that did not succeed reddens the stage.
# NOT a maximum -- `-gt` is an ordering, and exit codes are not ordered by gravity: a process that dies
# on Windows with the high bit set reports a NEGATIVE $LASTEXITCODE (0xC0000005 -> -1073741819), which
# is never greater than 0, and a body that never spoke reports $MuteStageExitCode. Both must redden
# (M, #913). The real codes are written as lines into the tail by the stage; the verdict is 0 or 1.
function Get-RustfmtStageExit {
    param([int[]] $Codes)
    foreach ($code in @($Codes)) {
        if ($code -ne 0) { return 1 }
    }
    return 0
}
# END Get-RustfmtStageExit

function Get-WorkspaceFmtTargets {
    # Package name -> absolute src_path of every target, from cargo metadata (manifests only, no
    # compilation). The same input cargo-fmt itself hands to rustfmt.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $json = cargo $toolchain metadata --format-version 1 --no-deps --locked 2>$null | Out-String
        $metaExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    $packages = [ordered]@{}
    if ($metaExit -ne 0 -or -not $json) { return $packages }
    $meta = $json | ConvertFrom-Json
    foreach ($pkg in $meta.packages) {
        $files = @($pkg.targets | ForEach-Object { $_.src_path } | Where-Object { $_ })
        if ($files.Count -gt 0) { $packages[$pkg.name] = $files }
    }
    return $packages
}

# #904: WHY A WARM TARGET IS RED TODAY, AND WHAT REPLACES THE RULE THAT MAKES IT SO.
#
# `$freshBuild = ($mtimeUtc -ge $runStartUtc)` (in Get-TestArtifactManifest, below) is the only
# question the gate asks about a reused binary, and on ANY reused target every binary predates the
# run start. `$staleArtifacts.Count -eq 0` is a term of `$passedEverything`, so a warm target is
# unconditionally RED and every run pays a cold compile -- measured at 323.8 s of a 1042 s gate,
# against 1.0 s warm. That is the largest single item in the gate, and it is paid for nothing.
#
# THE HAZARD THIS MUST NOT REINTRODUCE, and the reason "just trust cargo" is REFUTED rather than
# merely unattractive: cargo's fingerprint is MTIME-based, and a source file with a BACKDATED mtime
# reads as fresh. Reproduced: change a source to V2, backdate its mtime, rebuild -- cargo compiles
# nothing, the V1 binary runs, every ordinary test passes, rc=0, and the target's own marker says
# `complete`. `git checkout`, `mv` and `copy` all produce backdated mtimes, and this repository's
# own sabotage rituals do exactly that. So the reuse verdict may not rest on cargo's fingerprint,
# and it may not rest on mtime either.
#
# IT RESTS ON CONTENT. Per test executable:
#
#     rebuiltThisRun = (mtimeUtc >= runStartUtc)     <- unchanged, and now a RECORD, not a verdict
#     crateInputHash = H(crate directory tree || Cargo.lock || toolchain id || effective features
#                        || every dependency crate's own inputs)
#
#     reuseProof = 'rebuilt'         when rebuiltThisRun
#                  'proven-reuse'    when the ledger's sha256 AND crateInputHash both still match
#                  'unproven-reuse'  when the ledger has no entry for this executable
#                  'contaminated'    otherwise
#
# The ledger is written ONLY by a run in which every artefact was `rebuilt` or `proven-reuse`, and
# its first entry can therefore only come from a cold run -- where the mtime rule above still has
# full authority. The chain of custody is content-addressed end to end.
#
# THIS CAN ONLY TURN REDS GREEN, never greens red, and that is worth stating because the hash is
# deliberately over-inclusive (a whole directory, untracked files included): today EVERY artefact
# that was not rebuilt is RED, so an over-inclusive hash costs at worst the red the run already had.

function ConvertTo-ComparablePath {
    <#
      .SYNOPSIS
        One spelling for a path, so two producers of the same directory agree on the key.

      .DESCRIPTION
        `cargo metadata` reports `manifest_path` with backslashes on Windows and a
        compiler-artifact message reports `executable` the same way, but a caller that joined a
        path itself may not. Case is folded because the platform folds it, and a key that
        distinguished `D:\X` from `D:\x` would miss its own entry on exactly the machines this
        gate runs on.
    #>
    param([Parameter(Mandatory)][AllowEmptyString()][string] $Path)

    return ($Path -replace '\\', '/').TrimEnd([char]'/').ToLowerInvariant()
}

function Get-CrateDirectoryDigest {
    <#
      .SYNOPSIS
        SHA-256 over every byte of a crate directory as it sits in the WORKING TREE.

      .DESCRIPTION
        THE WORKING TREE, not the object database -- and that is the whole difference between this
        and `Get-CrateTreeObject` in ci/crate-input-hash.ps1, which answers the same question at a
        REVISION and is the right instrument for keying a proof shard that has to travel.
        It is the WRONG instrument here: the hazard is a source file edited to V2 in the working
        tree with a backdated mtime, and a revision-keyed hash cannot see an uncommitted byte at
        all. An instrument blind to the defect it is deployed against is worse than none, because
        it reads as coverage.

        THE WHOLE DIRECTORY, not `src/**/*.rs` plus `Cargo.toml`. A file list misses the file
        nobody thought of -- a `build.rs` include, a `.proto`, a fixture read at compile time, a
        `.cargo/config.toml` dropped in beside it. Walking the directory makes "an unhashed input"
        not a category that exists. Two exclusions, both OUTPUT rather than input:
          - a `target` directory at the crate's own top level (cargo's output, and on a developer
            box it can be larger than the repository);
          - any `.git` directory (git's own store).
        Neither is a build input, and neither is somewhere a source edit can hide.

        REPARSE POINTS ARE SKIPPED, not followed: a junction pointing at its own ancestor turns
        this walk into a non-terminating loop, and a hang has no colour.

        NOTHING IS CAUGHT. An unreadable file throws, and the caller records the failure as an
        ABSENT hash, which can never come back as `proven-reuse`. A digest that silently omitted a
        file it could not read would be the unsafe direction: it says "these inputs" about fewer
        inputs than there are.

        THE ENTRY FORM IS `<64 hex><space><relative path>`, hash FIRST. A path could in principle
        contain the separator; a SHA-256 hex digest is exactly 64 characters of [0-9a-f] and
        cannot, so the boundary is fixed-width rather than a character the content is trusted not
        to spell. `Join-Elements` then length-prefixes each entry, which is what makes the SET
        unambiguous -- see its own note for why that is the level where the ambiguity was real.
    #>
    param([Parameter(Mandatory)][string] $Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "cannot digest the crate directory '$Path': there is no such directory"
    }
    $root = (Resolve-Path -LiteralPath $Path).ProviderPath.TrimEnd([char]'\', [char]'/')
    $prefixLength = $root.Length + 1
    $entries = New-Object 'System.Collections.Generic.List[string]'
    $pending = New-Object 'System.Collections.Generic.Stack[string]'
    $pending.Push($root)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        while ($pending.Count -gt 0) {
            $current = $pending.Pop()
            $directory = New-Object System.IO.DirectoryInfo $current
            foreach ($entry in $directory.EnumerateFileSystemInfos()) {
                if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { continue }
                $relative = ($entry.FullName.Substring($prefixLength)) -replace '\\', '/'
                if ($entry.Attributes -band [System.IO.FileAttributes]::Directory) {
                    # ORDINAL, never `-ceq`: that operator is case-strict AND culture-aware, so a
                    # directory named `.git` plus a weightless code point would be skipped as if it
                    # were git's own store. ci/gate-manifest-provenance.tests.ps1 holds this line
                    # for the whole file.
                    if ([string]::Equals($entry.Name, '.git', [System.StringComparison]::Ordinal)) { continue }
                    if ([string]::Equals($relative, 'target', [System.StringComparison]::Ordinal)) { continue }
                    $pending.Push($entry.FullName)
                    continue
                }
                $stream = [System.IO.File]::Open($entry.FullName, [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
                try {
                    $bytes = $sha.ComputeHash($stream)
                } finally {
                    $stream.Dispose()
                }
                $hex = -join ($bytes | ForEach-Object { $_.ToString('x2') })
                $entries.Add("$hex $relative")
            }
        }
    } finally {
        $sha.Dispose()
    }
    $sorted = Sort-Ordinal -Values $entries.ToArray()
    return (Get-Sha256Hex -Text (Join-Elements -Values $sorted))
}

function Get-EmbeddedInputReferences {
    <#
      .SYNOPSIS
        Every compile-time file a Rust source pulls in from OUTSIDE itself, read out of its text.

      .DESCRIPTION
        THE HOLE THIS CLOSES, and it is a hole the directory digest above cannot see by
        construction. `Get-CrateDirectoryDigest` walks ONE crate directory, and this workspace
        embeds files from outside EVERY member directory at COMPILE time -- nine sites found in
        review of #1038 at `bcde3cf3`: `core/schema-evolution/tests/catalog_integrity.rs` and
        `core/protocols/tests/persistence_wire.rs` (`../../../schemas/*.json`),
        `core/protocols/tests/wire_roundtrip.rs` (`examples/graphs/…`),
        `core/schema-evolution/tests/compatibility.rs` and `conformance.rs` (`conformance/**`),
        `apps/cli/tests/attention_tag_domain.rs` (`README.md`, `QUICKSTART.md`),
        `apps/cli/src/commands/development.rs` and `apps/cli/tests/gate_http.rs`
        (`extensions/builtin/**`), `apps/cli/src/commands/events/backup.rs`
        (`docs/acceptance/**`), and every `tests/source_invariants.rs`, which `include!`s
        `tools/source-invariants/detect.rs` -- a directory that is not a workspace member at all.
        Edit any one of those with a BACKDATED mtime and cargo rebuilds nothing, every crate
        directory digest is byte-identical, and the whole workspace would come back
        `proven-reuse`: the exact adversary this mechanism was written against, walking in through
        the one door the digest does not cover.

        A PARSER, NOT A LINE MATCH. The argument is found by balancing parentheses from the
        macro's own `(`, skipping string literals as it goes, so a macro whose path sits on the
        NEXT line -- which is how `rustfmt` writes every long one in this tree, and how six of
        those nine sites are written -- is read the same as an inline one.

        TWO SHAPES ARE RESOLVED, and they are the two this tree spells:
          - a bare string literal, `include_str!("…")` / `include_bytes!("…")` / `include!("…")`,
            resolved relative to the DIRECTORY OF THE FILE THAT SPELLS IT, which is rustc's rule;
          - `concat!(env!("CARGO_MANIFEST_DIR"), "…"[, "…"])`, resolved relative to the CRATE
            directory, which is rustc's rule for that shape and the shape all 18 `include!` sites
            in this tree use.
        `include_dir!` is resolved against the crate directory too, for the day one appears.

        EVERYTHING ELSE IS A `reason`, NEVER A SILENT SKIP. A macro whose argument is a `const`, a
        `format!`, an env var this function does not model, or a literal it cannot unescape comes
        back with the reason spelled out, and the caller turns that into `unproven-reuse` for the
        whole crate. That is the only safe direction: a shape the scanner cannot resolve is a
        compile-time input it cannot hash, and an unhashed input is precisely how a stale binary
        is certified. Failing closed costs a cold compile; failing open costs the gate's meaning.

        PURE: it takes TEXT and returns records. The filesystem is the caller's business, so the
        parser can be driven from a cell with a string and no crate on disk at all.
    #>
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string] $Text,
        [Parameter(Mandatory)][AllowEmptyString()][string] $SourceLabel
    )

    $results = New-Object 'System.Collections.Generic.List[object]'
    if ([string]::IsNullOrEmpty($Text)) { return , $results.ToArray() }
    $pattern = New-Object System.Text.RegularExpressions.Regex '\binclude(?<kind>_str|_bytes|_dir)?!\s*\('
    foreach ($match in $pattern.Matches($Text)) {
        $kind = $match.Groups['kind'].Value
        $open = $match.Index + $match.Length - 1
        $depth = 0
        $end = -1
        $index = $open
        while ($index -lt $Text.Length) {
            $ch = $Text[$index]
            if ($ch -eq '"') {
                $index++
                while ($index -lt $Text.Length) {
                    if ($Text[$index] -eq '\') { $index += 2; continue }
                    if ($Text[$index] -eq '"') { break }
                    $index++
                }
                $index++
                continue
            }
            if ($ch -eq '(') { $depth++; $index++; continue }
            if ($ch -eq ')') {
                $depth--
                if ($depth -le 0) { $end = $index; break }
                $index++
                continue
            }
            $index++
        }
        if ($end -lt 0) {
            $results.Add([ordered]@{
                    source   = $SourceLabel
                    macro    = "include$kind!"
                    baseKind = ''
                    relative = ''
                    reason   = "include$kind! at offset $($match.Index) has no closing parenthesis, so its argument cannot be read"
                })
            continue
        }
        $argument = $Text.Substring($open + 1, $end - $open - 1).Trim()
        $baseKind = if ([string]::Equals($kind, '_dir', [System.StringComparison]::Ordinal)) { 'manifest' } else { 'file' }
        $relative = ''
        $reason = ''
        $literal = [regex]::Match($argument, '^"((?:[^"\\]|\\.)*)"$', [System.Text.RegularExpressions.RegexOptions]::Singleline)
        if ($literal.Success) {
            $relative = $literal.Groups[1].Value
        } else {
            $concat = [regex]::Match($argument,
                '^concat!\s*\(\s*env!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,(?<rest>.*)\)\s*$',
                [System.Text.RegularExpressions.RegexOptions]::Singleline)
            if ($concat.Success) {
                $rest = $concat.Groups['rest'].Value
                # WHAT IS LEFT OVER AFTER THE LITERALS ARE REMOVED decides this, rather than a
                # pattern that tries to spell every legal arrangement of commas: a `concat!` whose
                # tail carries anything but string literals, commas and whitespace is a shape this
                # function is not modelling, and it must say so rather than hash a prefix of it.
                $residue = [regex]::Replace($rest, '"(?:[^"\\]|\\.)*"', '')
                if ($residue -match '[^\s,]') {
                    $reason = "include$kind! uses a concat! this scanner cannot resolve: $($argument -replace '\s+', ' ')"
                } else {
                    $parts = New-Object 'System.Collections.Generic.List[string]'
                    foreach ($piece in [regex]::Matches($rest, '"(?:[^"\\]|\\.)*"')) {
                        $parts.Add($piece.Value.Substring(1, $piece.Value.Length - 2))
                    }
                    # THE BASE IS THE CRATE DIRECTORY, whatever the macro was. `include!` alone
                    # resolves relative to the FILE, but `env!("CARGO_MANIFEST_DIR")` names the
                    # crate root explicitly, and it is the leading term of the concatenation --
                    # so the shape, not the macro name, decides. Getting this wrong resolved all
                    # 18 sites one directory short (`apps/tools/...` for `apps/cli/tests/...`)
                    # and reported every one of them as a missing input.
                    $baseKind = 'manifest'
                    $relative = ($parts.ToArray() -join '')
                }
            } else {
                $reason = "include$kind! takes an argument this scanner cannot resolve to a path: $($argument -replace '\s+', ' ')"
            }
        }
        if ([string]::IsNullOrEmpty($reason)) {
            # `$CARGO_MANIFEST_DIR/…` is `include_dir!`'s own spelling of the crate root, and it is
            # resolved rather than refused so the day that crate arrives it does not redden a gate
            # for a shape that is perfectly determinate.
            if ($relative.StartsWith('$CARGO_MANIFEST_DIR/')) {
                $baseKind = 'manifest'
                $relative = $relative.Substring('$CARGO_MANIFEST_DIR/'.Length)
            }
            if ($relative -match '\\[^\\"]') {
                $reason = "include$kind! carries an escape this scanner does not unescape: $relative"
            } else {
                $relative = ($relative -replace '\\\\', '\') -replace '\\"', '"'
            }
        }
        if ([string]::IsNullOrEmpty($reason) -and [string]::IsNullOrWhiteSpace($relative)) {
            $reason = "include$kind! resolves to an empty path"
        }
        $results.Add([ordered]@{
                source   = $SourceLabel
                macro    = "include$kind!"
                baseKind = $baseKind
                relative = $relative
                reason   = $reason
            })
    }
    return , $results.ToArray()
}

function Get-CrateEmbeddedInputDigests {
    <#
      .SYNOPSIS
        The digest of every compile-time input one crate embeds from OUTSIDE its own directory.

      .DESCRIPTION
        Scans every `*.rs` under the crate directory -- `src/`, `tests/`, `benches/`, `examples/`
        and a `build.rs` alike, because the directory walk is what makes "a source nobody thought
        of" not a category -- and resolves what `Get-EmbeddedInputReferences` found.

        A REFERENCE THAT LANDS BACK INSIDE THE CRATE IS DROPPED, not hashed twice: the directory
        digest already covers every byte of it, and a second entry would only add cost. The
        entries that survive are exactly the inputs the digest is blind to.

        A DIRECTORY IS HASHED AS A TREE. `include_dir!`-shaped references name a directory, and
        the whole subtree is its content; `Get-CrateDirectoryDigest` is reused verbatim so one
        walk answers for both, exclusions included.

        A MISSING PATH IS A PROBLEM, NOT A ZERO. An input that is not there cannot be hashed, and
        recording it as an empty digest would make two different trees agree. It joins `problems`,
        and the caller refuses to prove any artefact of this crate.

        THE ENTRY FORM IS `<64 hex><space><base>:<relative>`, hash first and fixed-width, for the
        same reason `Get-CrateDirectoryDigest` gives for its own entries; `base` is `file` or
        `manifest` so two spellings that resolve differently cannot collide.
    #>
    param([Parameter(Mandatory)][string] $CrateDirectory)

    $entries = New-Object 'System.Collections.Generic.List[string]'
    $problems = New-Object 'System.Collections.Generic.List[string]'
    $seen = @{}
    if (-not (Test-Path -LiteralPath $CrateDirectory -PathType Container)) {
        $problems.Add("the crate directory '$CrateDirectory' does not exist, so its embedded inputs cannot be read")
        return [ordered]@{ entries = [string[]]@(); problems = [string[]]$problems.ToArray() }
    }
    $root = (Resolve-Path -LiteralPath $CrateDirectory).ProviderPath.TrimEnd([char]'\', [char]'/')
    $prefixLength = $root.Length + 1
    $sources = New-Object 'System.Collections.Generic.List[string]'
    $pending = New-Object 'System.Collections.Generic.Stack[string]'
    $pending.Push($root)
    while ($pending.Count -gt 0) {
        $current = $pending.Pop()
        $directory = New-Object System.IO.DirectoryInfo $current
        foreach ($entry in $directory.EnumerateFileSystemInfos()) {
            if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { continue }
            if ($entry.Attributes -band [System.IO.FileAttributes]::Directory) {
                if ([string]::Equals($entry.Name, '.git', [System.StringComparison]::Ordinal)) { continue }
                if ([string]::Equals($entry.Name, 'target', [System.StringComparison]::Ordinal)) { continue }
                $pending.Push($entry.FullName)
                continue
            }
            if ($entry.Name.EndsWith('.rs', [System.StringComparison]::OrdinalIgnoreCase)) {
                $sources.Add($entry.FullName)
            }
        }
    }
    foreach ($source in (Sort-Ordinal -Values $sources.ToArray())) {
        $label = ($source.Substring($prefixLength)) -replace '\\', '/'
        $text = [System.IO.File]::ReadAllText($source)
        foreach ($reference in (Get-EmbeddedInputReferences -Text $text -SourceLabel $label)) {
            if (-not [string]::IsNullOrEmpty([string]$reference['reason'])) {
                $problems.Add("$($label): $([string]$reference['reason'])")
                continue
            }
            $base = if ([string]::Equals([string]$reference['baseKind'], 'manifest', [System.StringComparison]::Ordinal)) { $root } else { Split-Path -Parent $source }
            $relative = [string]$reference['relative']
            $key = "$([string]$reference['baseKind']):$($relative -replace '\\', '/')"
            # ONE ENTRY PER DISTINCT REFERENCE. `core/schema-evolution` embeds
            # `conformance/migrations/success.json` from three match arms and `core/events` embeds
            # one schema five times; a second entry carries no information and costs another read
            # of the file. Measured on this workspace: 51 entries fall to 38, and the seen-set also
            # keeps the SET unambiguous -- the digest is over a set, and a repeated element would
            # make its cardinality part of the key for no reason.
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = $true
            $full = $null
            try {
                # THE LEADING SEPARATOR IS STRIPPED, and this is not cosmetic. rustc resolves
                # `concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/x.rs")` by string concatenation,
                # so the crate directory and the relative part are joined with exactly one
                # separator. `Join-Path 'X\apps\cli' '/../../tools'` leaves the empty segment in
                # place (`X\apps\cli\/../../tools`), and `GetFullPath` then spends one `..` undoing
                # it -- resolving to `X\apps\tools`, which does not exist. Measured: every one of
                # the 18 `include!` sites in this tree came back a false `problem` until this line.
                $full = [System.IO.Path]::GetFullPath((Join-Path $base ($relative -replace '^[\\/]+', '')))
            } catch {
                $problems.Add("$($label): embeds '$relative', which is not a usable path: $($_.Exception.Message)")
                continue
            }
            if ($full.StartsWith($root + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase) -or
                [string]::Equals($full, $root, [System.StringComparison]::OrdinalIgnoreCase)) {
                # Inside the crate: the directory digest already carries every byte of it.
                continue
            }
            if (Test-Path -LiteralPath $full -PathType Leaf) {
                $digest = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant()
                $entries.Add("$digest $key")
                continue
            }
            if (Test-Path -LiteralPath $full -PathType Container) {
                $entries.Add("$(Get-CrateDirectoryDigest -Path $full) $key")
                continue
            }
            $problems.Add("$($label): embeds '$relative', which does not exist at '$full'")
        }
    }
    return [ordered]@{
        entries  = [string[]](Sort-Ordinal -Values $entries.ToArray())
        problems = [string[]]$problems.ToArray()
    }
}

function Get-GateToolchainId {
    <#
      .SYNOPSIS
        The identity of the compiler THIS GATE uses, not the one that answers on PATH.

      .DESCRIPTION
        `Get-ToolchainId` in ci/crate-input-hash.ps1 runs `rustc -Vv`, which answers for the
        DEFAULT toolchain. This gate pins `+1.97.1` on every cargo call, and on a box whose default
        is a nightly the two are different compilers. A key that names the wrong compiler is the
        unsafe direction: it would let artefacts built by one toolchain be proven against a ledger
        written by another. So when the gate pins a toolchain, the id is read THROUGH it.

        No fallback. If `rustup run` cannot answer, this throws and the caller records an absent
        hash, which fails closed. An infallible fallback here would give every toolchain on the
        machine one generation, which is precisely the collision the field exists to prevent.
    #>
    param([AllowEmptyString()][string] $ToolchainArgument = '')

    if ([string]::IsNullOrWhiteSpace($ToolchainArgument)) {
        return (Get-ToolchainId)
    }
    $version = $ToolchainArgument.TrimStart([char]'+')
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $verbose = & rustup run $version rustc -Vv 2>&1
        # `$?` FIRST, and this order is load-bearing: reading `$LASTEXITCODE` is itself a successful
        # statement and resets `$?` to true. `$?` is what distinguishes "rustup ran and failed" from
        # "rustup is not on PATH", where `$LASTEXITCODE` keeps whatever the PREVIOUS command left --
        # a stale zero that would read as success. The variable is never pre-cleared, because
        # ci/gate-stage-verdict-source.tests.ps1 forbids writing 0 to it anywhere in this file:
        # the mute-stage poison lives in the same variable.
        $ran = $?
        $exit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    # NOT `-not $ran`, which is also false for a rustup that merely wrote an `info:` line to stderr
    # and exited 0. See Test-NativeCallFailed for the measurement that separates the two.
    if (Test-NativeCallFailed -Ran $ran -ExitCode $exit -Capture $verbose) {
        throw "cannot read the toolchain id: 'rustup run $version rustc -Vv' did not succeed (ran=$ran, exit=$exit): $(Get-NativeStderrText -Capture $verbose)"
    }
    # THE STDERR IS NOT PART OF THE ID. While a chatty rustup threw, its lines could never reach this
    # join. Now that one exits 0, an unfiltered join would fold `info: downloading component` INTO
    # the id -- and that id keys the artifact ledger, so it would name a compiler generation no
    # other run reproduces, in silence, in the one field built to prevent exactly that collision.
    $identity = ((@($verbose |
                    Where-Object { -not ($_ -is [System.Management.Automation.ErrorRecord]) } |
                    ForEach-Object { [string]$_ }) -join "`n") -replace "`r", '').Trim()
    # THE SHAPE, not a claim about which lines rustup chose to write: `rustc -Vv` opens with
    # `rustc `. Without this the filter above could hand back an EMPTY id and every guard asking
    # "is the stderr absent from the id" would pass on the absence of an id.
    if (-not $identity.StartsWith('rustc ', [System.StringComparison]::Ordinal)) {
        throw "cannot read the toolchain id: 'rustup run $version rustc -Vv' exited 0 without printing a rustc version (got: $identity)"
    }
    return $identity
}

function Test-NativeCallFailed {
    <#
      .SYNOPSIS
        Whether a native call that has just run actually FAILED, as opposed to merely having spoken.

      .DESCRIPTION
        `$?` ANSWERS TWO QUESTIONS AT ONCE and only one of them is about failure. Under `2>&1` in
        PowerShell 5.1 every stderr line a native process writes becomes a NativeCommandError record
        in the capture and clears `$?` -- even when the process exited 0.

        MEASURED, in a real gate on 2026-09-20: `cargo metadata` queued on ~/.cargo/.package-cache
        while this gate's own cargo held it, printed `Blocking waiting for file lock on package
        cache`, and exited 0. The caller published `did not succeed (ran=False, exit=0)` about a
        cargo that had SUCCEEDED, and took ci/gate-artifact-reuse.tests.ps1 red with it. The
        expensive half is silent: ci/gate.ps1 calls the same reader for its own crate input hashes
        inside a catch that degrades to a yellow NOTE, so a cargo that merely waited on a lock costs
        the run every reused artefact -- a 12 s build pass against a 566 s one.

        SO A FALSE `$?` IS EVIDENCE OF FAILURE ONLY WHEN THE CAPTURE DOES NOT EXPLAIN IT. Measured
        on PowerShell 5.1:
          stderr + exit 0     -> $? False, capture HOLDS NativeCommandError records, $LASTEXITCODE 0
          not on PATH         -> $? False, capture holds NO such record (the CommandNotFoundException
                                 goes to the console, not into the `2>&1` capture), and $LASTEXITCODE
                                 keeps whatever the PREVIOUS command left -- a stale zero that reads
                                 as success
        `$?` therefore remains the only thing that catches an absent command, and it is kept for
        exactly that. What changes is that a process which spoke is no longer mistaken for one that
        never ran.

        ONE FUNCTION, TWO CALLERS. Inlining the rule beside each `$?` would make the gate's two
        native readers two oracles that can drift apart, and the direction they drift in is the
        permissive one. ci/gate-native-stderr.tests.ps1 drives both callers through real `.cmd`
        shims -- chatty, silent, failing, and absent -- and pins that each routes its verdict here.
    #>
    param(
        [Parameter(Mandatory)][bool] $Ran,
        [AllowNull()][object] $ExitCode,
        [AllowNull()][object] $Capture
    )
    # A non-zero exit is failure whatever else happened, and covers the absent command whose
    # $LASTEXITCODE was left non-zero by something earlier.
    if ($ExitCode -ne 0) { return $true }
    if ($Ran) { return $false }
    $spoke = @($Capture | Where-Object {
            $_ -is [System.Management.Automation.ErrorRecord] -and
            $_.FullyQualifiedErrorId -like 'NativeCommand*'
        }).Count -gt 0
    return (-not $spoke)
}

function Get-NativeStderrText {
    <#
      .SYNOPSIS
        Only the stderr a native call wrote, joined, with its stdout left behind.

      .DESCRIPTION
        A refusal must name what it saw. The throw that exposed all of this printed `ran=False,
        exit=0` and dropped the capture, so cargo's own one-line explanation reached nobody and the
        next reader was sent to look for a defect in the wrong subject.

        STDERR ALONE, because the stdout here is a `cargo metadata` document: joining the capture
        whole would put megabytes of JSON inside an exception message.
    #>
    param([AllowNull()][object] $Capture)
    return (@($Capture |
                Where-Object { $_ -is [System.Management.Automation.ErrorRecord] } |
                ForEach-Object { ([string]$_).Trim() } |
                Where-Object { $_ }) -join '; ')
}

function Get-CargoDependencyGraph {
    <#
      .SYNOPSIS
        The resolved package graph, read from `cargo metadata`, as plain maps a fold can walk.

      .DESCRIPTION
        `--all-features`, matching the build this gate actually runs (`cargo test --workspace
        --all-features`). Measured on this workspace: without it, `graphhelm-postgres-event-store`
        resolves to NO features while the gate compiles it with `test-support`. A key computed from
        the default resolution would name a build nobody ran.

        `--locked`, so the graph cannot be the product of a resolution this call silently performed.

        RETURNED AS MAPS, not as the parsed document, so the fold below can be driven from a cell
        with a hand-built graph. A fold reachable only through a real `cargo metadata` run can only
        be tested against the workspace it happens to be standing in.
    #>
    param(
        [Parameter(Mandatory)][string] $WorkspaceRoot,
        [AllowEmptyString()][string] $ToolchainArgument = ''
    )

    $manifestPath = Join-Path $WorkspaceRoot 'Cargo.toml'
    $arguments = @('metadata', '--format-version', '1', '--locked', '--all-features',
        '--manifest-path', $manifestPath)
    if (-not [string]::IsNullOrWhiteSpace($ToolchainArgument)) {
        $arguments = @($ToolchainArgument) + $arguments
    }
    # The same treatment Invoke-Stage gives every other native call, and needed for the same
    # reason: under $ErrorActionPreference='Stop' a native stderr line becomes a terminating
    # NativeCommandError before $LASTEXITCODE is ever read.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & cargo @arguments 2>&1
        # `$?` FIRST -- see Get-GateToolchainId for why the order matters and why the variable is
        # never pre-cleared to zero.
        $ran = $?
        $exit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    # NOT `-not $ran`. `cargo metadata` writes to stderr whenever it queues on the package cache,
    # which is ROUTINE while this gate's own cargo holds it -- and that read as a cargo that never
    # launched. See Test-NativeCallFailed.
    if (Test-NativeCallFailed -Ran $ran -ExitCode $exit -Capture $output) {
        throw "cannot read the dependency graph: 'cargo $($arguments -join ' ')' did not succeed (ran=$ran, exit=$exit): $(Get-NativeStderrText -Capture $output)"
    }
    # `cargo metadata` emits its document as ONE line and everything else it says is stderr. The
    # document is picked by SHAPE rather than by position, because a progress line printed first
    # would otherwise be parsed as the document and fail with a message about JSON instead of a
    # message about cargo.
    $document = @($output | ForEach-Object { [string]$_ } |
            Where-Object { $_.StartsWith('{', [System.StringComparison]::Ordinal) }) |
        Select-Object -First 1
    if (-not $document) {
        throw "cannot read the dependency graph: 'cargo $($arguments -join ' ')' printed no JSON document"
    }
    $meta = $document | ConvertFrom-Json
    if ($null -eq $meta.resolve -or $null -eq $meta.resolve.nodes) {
        throw 'cannot read the dependency graph: cargo metadata carried no resolve section'
    }

    $packages = @{}
    foreach ($package in $meta.packages) {
        $packages[[string]$package.id] = [ordered]@{
            name      = [string]$package.name
            directory = (Split-Path -Parent ([string]$package.manifest_path))
        }
    }
    $members = @{}
    foreach ($member in $meta.workspace_members) { $members[[string]$member] = $true }
    $nodes = @{}
    foreach ($node in $meta.resolve.nodes) {
        $nodes[[string]$node.id] = [ordered]@{
            features = @($node.features | ForEach-Object { [string]$_ })
            deps     = @($node.deps | ForEach-Object { [string]$_.pkg })
        }
    }
    return [ordered]@{ packages = $packages; members = $members; nodes = $nodes }
}

function Get-CrateInputHashesFromGraph {
    <#
      .SYNOPSIS
        One content-addressed input hash per workspace member, dependencies folded in transitively.

      .DESCRIPTION
        REACHABILITY, NOT A TOPOLOGICAL FOLD, and this is a deviation from #904's written design
        made because the real graph refutes it: `cargo metadata` on this workspace reports THREE
        back-edges among the workspace members (`core/execution` <-> `core/events`,
        `core/simulation` <-> `core/events`, `core/simulation` <-> `core/execution`), all of them
        dev-dependency cycles, which cargo permits and for which no topological order exists. A
        recursive fold over that graph does not terminate, and a hang has no colour.

        So each member gets a SHALLOW hash first -- its own directory, the lock, the root manifest,
        the toolchain and its effective features, with no dependencies -- and its final hash folds
        in the shallow hash of every workspace member REACHABLE from it, plus the package id of
        every external package reachable from it. Reachability is a SET, so it is defined on a
        cyclic graph and does not depend on traversal order.

        It carries the same information a recursive fold would: a crate's hash moves when any
        ancestor's own inputs move, which is the property #904 asks for. The one thing a set does
        not encode is the SHAPE of the graph -- and an edge exists only because some crate's
        `Cargo.toml` declares it, and that manifest is inside that crate's directory, so a changed
        edge changes a shallow hash anyway.

        EXTERNAL PACKAGES ARE FOLDED AS THEIR PACKAGE ID, not as a directory digest. The id carries
        name, version and source, and `Cargo.lock` -- folded into every hash -- carries the
        checksum. Hashing ~/.cargo/registry on every gate would cost more than the compile this
        saves.

        THE `member:` / `external:` PREFIXES are not decoration: without them a package id and a
        digest are two opaque strings in one set, and a future edit that made an id look like a
        digest would silently merge two populations that must not merge.
    #>
    param(
        [Parameter(Mandatory)][object] $Graph,
        [Parameter(Mandatory)][string] $ToolchainId,
        [Parameter(Mandatory)][AllowEmptyString()][string] $LockfileSha256,
        [Parameter(Mandatory)][AllowEmptyString()][string] $WorkspaceManifestSha256
    )

    $memberIds = Sort-Ordinal -Values (@($Graph.members.Keys | ForEach-Object { [string]$_ }))
    if ($memberIds.Count -eq 0) {
        throw 'cannot compute crate input hashes: the graph names no workspace members'
    }

    $trees = @{}
    $shallow = @{}
    $embeddedInputs = @{}
    # #1038 review (lane S): which members embed a compile-time input this scanner could not
    # resolve. A member in this map gets NO published hash, so every artefact built from it comes
    # back `unproven-reuse` -- see the reading site in Get-TestArtifactManifest.
    $unresolvedByPackageId = @{}
    foreach ($id in $memberIds) {
        if (-not $Graph.packages.ContainsKey($id)) {
            throw "cannot compute crate input hashes: workspace member '$id' has no package record"
        }
        $package = $Graph.packages[$id]
        $tree = Get-CrateDirectoryDigest -Path ([string]$package.directory)
        $trees[$id] = $tree
        $features = @()
        if ($Graph.nodes.ContainsKey($id)) { $features = @($Graph.nodes[$id].features) }
        # #1038 review (lane S): THE INPUTS THAT LIVE OUTSIDE THE DIRECTORY. `BuildScriptInputs` is
        # the preimage field #904 reserved for exactly this population -- inputs a crate consumes
        # that its own tree does not contain -- and folding the embedded-input digests in HERE, at
        # the SHALLOW hash, is what makes them propagate: a dependent's hash folds this one, so a
        # changed `schemas/*.json` moves every crate downstream of the crate that embeds it.
        $embedded = Get-CrateEmbeddedInputDigests -CrateDirectory ([string]$package.directory)
        if ($embedded.problems.Count -gt 0) {
            $unresolvedByPackageId[$id] = ($embedded.problems -join '; ')
        }
        # KEPT, because the PUBLISHED hash is computed in a SECOND pass below and folding the
        # embedded inputs only into the shallow hash would carry them to every crate DOWNSTREAM of
        # this one and not to this one. Measured while writing the cell: with the fold here alone,
        # a single-member graph whose test embeds `../../schemas/x.json` kept the same published
        # hash across a V1 -> V2 edit of that file -- the exact false `proven-reuse` this work
        # exists to close, reintroduced one line away from its fix.
        $embeddedInputs[$id] = [string[]]$embedded.entries
        $shallow[$id] = Get-CrateInputHash -Crate ([string]$package.name) -TreeObject $tree `
            -LockSlice $LockfileSha256 -WorkspaceManifest $WorkspaceManifestSha256 `
            -ToolchainId $ToolchainId -Features $features -DependencyHashes @() `
            -BuildScriptInputs ([string[]]$embedded.entries)
    }

    $byPackageId = @{}
    $byDirectory = @{}
    $unresolvedPublished = @{}
    $unresolvedByDirectory = @{}
    foreach ($id in $memberIds) {
        # Breadth-first reachability with a visited set. The visited set is what makes this
        # terminate on the cyclic graph described above, and it is the whole reason this is not a
        # recursion.
        $visited = @{}
        $frontier = New-Object 'System.Collections.Generic.Queue[string]'
        if ($Graph.nodes.ContainsKey($id)) {
            foreach ($dep in @($Graph.nodes[$id].deps)) { $frontier.Enqueue([string]$dep) }
        }
        while ($frontier.Count -gt 0) {
            $next = $frontier.Dequeue()
            if ($visited.ContainsKey($next)) { continue }
            $visited[$next] = $true
            if ($Graph.nodes.ContainsKey($next)) {
                foreach ($dep in @($Graph.nodes[$next].deps)) { $frontier.Enqueue([string]$dep) }
            }
        }
        $dependencyKeys = New-Object 'System.Collections.Generic.List[string]'
        foreach ($reached in $visited.Keys) {
            $key = [string]$reached
            if ([string]::Equals($key, $id, [System.StringComparison]::Ordinal)) { continue }
            if ($shallow.ContainsKey($key)) {
                $dependencyKeys.Add("member:$($shallow[$key])")
            } else {
                $dependencyKeys.Add("external:$key")
            }
        }
        $package = $Graph.packages[$id]
        $features = @()
        if ($Graph.nodes.ContainsKey($id)) { $features = @($Graph.nodes[$id].features) }
        $hash = Get-CrateInputHash -Crate ([string]$package.name) -TreeObject $trees[$id] `
            -LockSlice $LockfileSha256 -WorkspaceManifest $WorkspaceManifestSha256 `
            -ToolchainId $ToolchainId -Features $features `
            -DependencyHashes $dependencyKeys.ToArray() `
            -BuildScriptInputs ([string[]]$embeddedInputs[$id])
        # #1038 review (lane S): FAIL CLOSED, AND TRANSITIVELY. A member whose own embedded inputs
        # could not be resolved publishes no hash; so does a member that DEPENDS on one, because its
        # hash folds a shallow hash that is itself incomplete, and a hash computed over fewer inputs
        # than there are is the shape that certifies a stale binary. The reason travels with the
        # refusal, so the manifest can name the file and the macro rather than only the refusal.
        $reasons = New-Object 'System.Collections.Generic.List[string]'
        if ($unresolvedByPackageId.ContainsKey($id)) {
            $reasons.Add([string]$unresolvedByPackageId[$id])
        }
        foreach ($reached in (Sort-Ordinal -Values (@($visited.Keys | ForEach-Object { [string]$_ })))) {
            if ([string]::Equals($reached, $id, [System.StringComparison]::Ordinal)) { continue }
            if ($unresolvedByPackageId.ContainsKey($reached)) {
                $reasons.Add("via dependency '$reached': $([string]$unresolvedByPackageId[$reached])")
            }
        }
        $directoryKey = ConvertTo-ComparablePath -Path ([string]$package.directory)
        if ($reasons.Count -gt 0) {
            $joined = ($reasons.ToArray() -join '; ')
            $unresolvedPublished[$id] = $joined
            $unresolvedByDirectory[$directoryKey] = $joined
            continue
        }
        $byPackageId[$id] = $hash
        $byDirectory[$directoryKey] = $hash
    }
    return [ordered]@{
        toolchainId             = $ToolchainId
        lockfileSha256          = $LockfileSha256
        workspaceManifestSha256 = $WorkspaceManifestSha256
        byPackageId             = $byPackageId
        byDirectory             = $byDirectory
        unresolvedByPackageId   = $unresolvedPublished
        unresolvedByDirectory   = $unresolvedByDirectory
    }
}

function Get-CrateInputHashes {
    <#
      .SYNOPSIS
        The input hash of every workspace member of a checkout, from the checkout alone.

      .DESCRIPTION
        A thin composer over three pieces that can each be measured on their own: the graph
        (`cargo metadata`), the two workspace-wide inputs (`Cargo.lock`, and the ROOT `Cargo.toml`
        where the build profiles live and which no crate's directory contains -- see
        `Get-WorkspaceManifestBlob` in ci/crate-input-hash.ps1 for the measurement that established
        that), and the fold.
    #>
    param(
        [Parameter(Mandatory)][string] $WorkspaceRoot,
        [AllowEmptyString()][string] $ToolchainArgument = ''
    )

    $lockPath = Join-Path $WorkspaceRoot 'Cargo.lock'
    $manifestPath = Join-Path $WorkspaceRoot 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $lockPath)) {
        throw "cannot compute crate input hashes: '$lockPath' does not exist, so the resolved versions are unknown"
    }
    $lockSha = (Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $manifestSha = if (Test-Path -LiteralPath $manifestPath) {
        (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    } else {
        ''
    }
    $toolchainId = Get-GateToolchainId -ToolchainArgument $ToolchainArgument
    $graph = Get-CargoDependencyGraph -WorkspaceRoot $WorkspaceRoot -ToolchainArgument $ToolchainArgument
    return Get-CrateInputHashesFromGraph -Graph $graph -ToolchainId $toolchainId `
        -LockfileSha256 $lockSha -WorkspaceManifestSha256 $manifestSha
}

function Get-ArtifactLedgerPath {
    param([Parameter(Mandatory)][string] $TargetDir)
    return (Join-Path $TargetDir '.graphhelm-artifact-ledger.json')
}

function Read-ArtifactLedger {
    <#
      .SYNOPSIS
        What the last fully-proven run recorded about this target's executables.

      .DESCRIPTION
        RETURNS AN EMPTY MAP FOR EVERY FAILURE, and that is safe here in a way it is nowhere else
        in this file: an absent entry produces `unproven-reuse`, which is RED. A ledger that cannot
        be read therefore fails CLOSED -- the run behaves exactly as the gate behaves today, and
        the only cost is the cold compile this mechanism exists to avoid.

        Keyed by the executable path in its comparable spelling, because the producer of the key (a
        compiler-artifact message) and the consumer (this map) are two different cargo surfaces and
        neither promises the other's casing.
    #>
    param([Parameter(Mandatory)][string] $TargetDir)

    $entries = @{}
    $path = Get-ArtifactLedgerPath -TargetDir $TargetDir
    if (-not (Test-Path -LiteralPath $path)) { return $entries }
    try {
        $document = Get-Content -LiteralPath $path -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    } catch {
        return $entries
    }
    if ($null -eq $document -or $null -eq $document.PSObject.Properties['artifacts']) { return $entries }
    foreach ($record in @($document.artifacts)) {
        if ($null -eq $record) { continue }
        $executable = ''
        $sha = ''
        $inputHash = ''
        if ($null -ne $record.PSObject.Properties['executable']) { $executable = [string]$record.PSObject.Properties['executable'].Value }
        if ($null -ne $record.PSObject.Properties['sha256']) { $sha = [string]$record.PSObject.Properties['sha256'].Value }
        if ($null -ne $record.PSObject.Properties['crateInputHash']) { $inputHash = [string]$record.PSObject.Properties['crateInputHash'].Value }
        if ([string]::IsNullOrWhiteSpace($executable)) { continue }
        $entries[(ConvertTo-ComparablePath -Path $executable)] = [ordered]@{
            executable     = $executable
            sha256         = $sha
            crateInputHash = $inputHash
        }
    }
    return $entries
}

function Write-ArtifactLedger {
    <#
      .SYNOPSIS
        Record this run's executables as the custody chain the NEXT run proves reuse against.

      .DESCRIPTION
        The caller writes this only when every artefact of the run was `rebuilt` or `proven-reuse`.
        That condition is what makes the chain sound: the first entry for any executable can only
        come from a run that built it inside the run's own window, where the mtime rule still has
        full authority, and every later entry restates a chain that was checked by CONTENT.

        An artefact missing either half of the pair is DROPPED rather than written with a blank,
        because a blank comes back through `Get-ArtifactReuseProof` as an equality between two
        empty strings -- a false `proven-reuse` manufactured by the writer.
    #>
    param(
        [Parameter(Mandatory)][string] $TargetDir,
        [Parameter(Mandatory)][AllowEmptyCollection()][AllowNull()][object] $Artifacts
    )

    # [object[]], NOT @() -- the cast this file already prescribes twice, at `stages =
    # [object[]]$stageRecords` and inside `Test-StageOverlapped`, whose note says in so many words
    # that `@(<a System.Collections.Generic.List[object] VARIABLE>)` throws "Argument types do not
    # match" UNCONDITIONALLY on this machine's Windows PowerShell 5.1 (5.1.26100.9168) and that it
    # therefore breaks every run that COMPLETES. `Get-TestArtifactManifest` builds `artifacts` as
    # exactly such a list, and #904 opened both of these functions with `@($Artifacts)` anyway.
    # Every cell handed them a literal `@(...)` ARRAY, so the trap lived only on the gate's own call
    # path; it cost PR #1038's 36-minute run, which completed all 60 stages and then died with no
    # manifest and no rc.
    #
    # THE CAST IS CORRECT ON EVERY SHAPE THIS SEAM TAKES, measured rather than reasoned about: a
    # List[object] (234 records -> 234), a plain array, an empty array, an empty list, `$null` (zero
    # iterations, which is what `[AllowNull()]` is for), and a LONE ordered dictionary -- which comes
    # back as ONE element that is the record itself, not as its key/value pairs. My first draft hand-
    # rolled a three-line normalisation to avoid a DictionaryEntry unrolling that does not happen,
    # and said so in a comment; the sabotage that should have reddened for it stayed green, which is
    # how the claim got checked.
    $records = New-Object 'System.Collections.Generic.List[object]'
    foreach ($artifact in ([object[]]$Artifacts)) {
        if ($null -eq $artifact) { continue }
        $executable = [string]$artifact['executable']
        $sha = [string]$artifact['sha256']
        $inputHash = [string]$artifact['crateInputHash']
        if ([string]::IsNullOrWhiteSpace($executable)) { continue }
        if ([string]::IsNullOrWhiteSpace($sha)) { continue }
        if ([string]::IsNullOrWhiteSpace($inputHash)) { continue }
        $records.Add([ordered]@{
                executable     = $executable
                sha256         = $sha
                crateInputHash = $inputHash
            })
    }
    $document = [ordered]@{
        writtenUtc = [DateTime]::UtcNow.ToString('o')
        artifacts  = [object[]]$records
    }
    $path = Get-ArtifactLedgerPath -TargetDir $TargetDir
    # No BOM, for the same reason the run manifest carries none: this file exists to be machine-read
    # later, and a strict JSON parser rejects a leading BOM outright.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($path, ($document | ConvertTo-Json -Depth 5), $utf8NoBom)
    return $path
}

function Get-ArtifactReuseProof {
    <#
      .SYNOPSIS
        What this run can actually VOUCH FOR about one test executable.

      .DESCRIPTION
        Four states, and the two that are neither `rebuilt` nor `proven-reuse` are both RED -- which
        is what makes this change unable to turn a green red: today EVERY artefact that was not
        rebuilt is RED, and this can only move some of them to `proven-reuse`.

        `unproven-reuse` and `contaminated` are kept APART even though they share a verdict,
        because they want different remedies: the first is a target this machine holds no custody
        record for (run once cold, or clean it), the second is a target whose INPUTS moved under a
        binary that did not -- something edited a source and the binary did not follow, which is
        the backdated-mtime shape this whole mechanism exists for.

        BOTH HALVES MUST BE PRESENT, NOT MERELY EQUAL. A null exe hash and a null recorded hash are
        equal, and an equality between two absences is exactly how a measurement that FAILED comes
        back wearing the colour of one that passed.

        The exe hash is compared case-INSENSITIVELY (`Get-FileHash` returns upper case, JSON
        round-trips whatever was written) and the input hash case-SENSITIVELY (it is produced in
        lower case by one function and nothing else may spell it).
    #>
    param(
        [Parameter(Mandatory)][bool] $RebuiltThisRun,
        [AllowNull()][AllowEmptyString()][string] $ExeSha256,
        [AllowNull()][AllowEmptyString()][string] $CrateInputHash,
        [AllowNull()][object] $LedgerEntry,
        # NOT Mandatory, for the reason `Get-CrateInputHash`'s own note gives at length: a missing
        # Mandatory parameter PROMPTS under the gate's own invocation, and a hang has no colour.
        # The default is the safe one only because the caller that knows sets it; a caller that
        # does not know is a caller with no scanner, which is the pre-#1038 world.
        [bool] $UnresolvedInputs = $false
    )

    if ($RebuiltThisRun) { return 'rebuilt' }
    # #1038 review (lane S): AN INPUT SET THIS RUN COULD NOT READ WHOLE proves nothing, and it is
    # answered here rather than at the call site so there is ONE place a reuse can be refused. It
    # ranks above the ledger comparison on purpose: the recorded hash and the computed hash could
    # be EQUAL and still both be computed over an incomplete input set, which is an agreement
    # between two measurements that were each missing the same file.
    if ($UnresolvedInputs) { return 'unproven-reuse' }
    if ($null -eq $LedgerEntry) { return 'unproven-reuse' }
    $recordedSha = [string]$LedgerEntry['sha256']
    $recordedInput = [string]$LedgerEntry['crateInputHash']
    if ([string]::IsNullOrWhiteSpace($ExeSha256) -or [string]::IsNullOrWhiteSpace($CrateInputHash) -or
        [string]::IsNullOrWhiteSpace($recordedSha) -or [string]::IsNullOrWhiteSpace($recordedInput)) {
        return 'contaminated'
    }
    if ([string]::Equals($recordedSha, $ExeSha256, [System.StringComparison]::OrdinalIgnoreCase) -and
        [string]::Equals($recordedInput, $CrateInputHash, [System.StringComparison]::Ordinal)) {
        return 'proven-reuse'
    }
    return 'contaminated'
}

function Read-ArtifactManifestField {
    <#
      .SYNOPSIS
        One field of the artefact manifest, for a writer that may not have been given one.

      .DESCRIPTION
        `Write-RunManifest` is called from three sites and only one of them passes an artefact
        manifest: the HARNESS-BROKE path and the canary-abort path both run before any artefact is
        enumerated. Reading `$ArtifactManifest.field` there would throw under `Set-StrictMode`,
        inside the writer, after every stage has already run -- the shape that lost two whole runs'
        evidence between the last stage and the record (see the `matrixReason` note near the top of
        this file, and the `stages` field's [object[]] cast).

        $null FOR ABSENT, which is the value the manifest already uses for NOT MEASURED everywhere
        else. A zero would be a claim about a population nobody enumerated.
    #>
    param(
        [AllowNull()][object] $Manifest,
        [Parameter(Mandatory)][string] $Name
    )

    if ($Manifest -isnot [System.Collections.IDictionary]) { return $null }
    if (-not $Manifest.Contains($Name)) { return $null }
    return $Manifest[$Name]
}

function Select-UnprovenReuse {
    <#
      .SYNOPSIS
        The artefacts this run cannot vouch for -- the population that reddens the gate.

      .DESCRIPTION
        ONE PLACE, for the reason `$FreshnessStageName` is a constant in this file: the verdict, the
        instrument flag, the `complete` stamp and the freshness cross-check all have to price the
        SAME population, and four hand-written copies of one filter is four chances for three of
        them to keep the old one. Editing the verdict alone would be the worst of the four
        outcomes -- a legitimate warm run would fail to stamp `complete`, the NEXT run would read
        `interrupted`, and the gate would abort before its first compile.

        INDEXED, not dotted. Under `Set-StrictMode` a missing key on a dictionary THROWS when read
        with `.`, and this is the seam a cell hands hand-built records to.

        `return , $array`, AND THE COMMA IS LOAD-BEARING -- caught by this function's own cells
        before it shipped. `return @(...)` hands the array to the pipeline, which UNROLLS it: with
        one match the caller receives the record itself, and `.Count` on an ordered dictionary is
        its number of KEYS. Measured: a population of exactly one unproven artefact answered 2.
        With no matches the caller receives `$null`, and `.Count` throws. The same shape
        `Sort-Ordinal` uses one file over, for the same reason.
    #>
    param([Parameter(Mandatory)][AllowEmptyCollection()][AllowNull()][object] $Artifacts)

    # ORDINAL MEMBERSHIP, spelled out. `-in` is the same culture comparer over a list -- the
    # repository's own sweep lists it as `Iin` -- and this predicate decides the gate's verdict.
    # Inlined rather than factored into a helper because this function is dot-sourced OUT of
    # gate.ps1 on its own by a cell, and a call to a sibling it did not bring with it would throw.
    $unproven = @('unproven-reuse', 'contaminated')
    # [object[]], NOT @() -- see the note in `Write-ArtifactLedger` for the measurement, and
    # `Test-StageOverlapped` for where this file already prescribes the same cast for the same reason.
    # THIS is the site that actually fired: `$unprovenAtEnd = (Select-UnprovenReuse -Artifacts
    # $artifactManifest.artifacts).Count` is the first statement of the tail after the last stage, so
    # `@($Artifacts)` here is what lost #1038 its whole manifest after 60 green stages. Spelled out at
    # both seams rather than shared, for this function's own stated reason: a cell dot-sources it out
    # of gate.ps1 ALONE, and a call to a helper it did not bring would throw.
    $matched = New-Object 'System.Collections.Generic.List[object]'
    foreach ($artifact in ([object[]]$Artifacts)) {
        if ($null -eq $artifact) { continue }
        $proof = [string]$artifact['reuseProof']
        foreach ($candidate in $unproven) {
            if ([string]::Equals($proof, $candidate, [System.StringComparison]::Ordinal)) {
                $matched.Add($artifact)
                break
            }
        }
    }
    return , $matched.ToArray()
}

function Get-TestArtifactManifest {
    # #243: SCOPE IS A PARAMETER, NOT A CONSTANT. This used to hardcode `--workspace
    # --all-features`, which is the ONLY invocation this function could ever fingerprint -- so the
    # binaries the `cli: <suite>` stages below actually execute (`-p graphhelm-cli --test <suite>`,
    # a DIFFERENT compiled unit under cargo's feature-unification rules, measured on #243) were
    # never in the population `staleArtifacts`/`instrumentSuspect` judge at all. Not "unlinked to a
    # stage" -- absent. Default preserves the original workspace-wide call untouched.
    # #1007: `-Enrolling` marks the ONE retry this function may make of itself (see the enrolment
    # block before the return); it is never passed by an outside caller.
    param([string[]] $CargoArgs = @('--workspace', '--all-features'), [switch] $Enrolling)

    # #901 slice 2: NO RUST STAGE RUNS, SO NOTHING IS BUILT OR ENUMERATED. The shape is the full one,
    # so every reader downstream reads a key rather than throwing; `buildExitCode` is `$null`, which
    # every reader already treats as "not established" -- never 0, which would claim a build.
    # An EMPTY generic List is a shape no real build pass produced before, and Windows PowerShell 5.1
    # throws "Argument types do not match" on `@($emptyList)`: measured on the first no-Rust gate, at
    # the end-of-run note, which now reads `.Count` behind a truth test instead.
    if ((Test-Path 'variable:script:rustStagesIncluded') -and -not $script:rustStagesIncluded) {
        return [ordered]@{
            buildExitCode = $null; buildPassNonJsonTail = @(); artifactsEnrolled = 0
            artifacts = (New-Object System.Collections.Generic.List[object]); outputTail = @()
            artifactsRebuilt = 0; artifactsProvenReuse = 0; artifactsUnprovenReuse = 0; artifactsContaminated = 0
            buildMode = 'unknown'; buildPassSecs = 0; crateInputHashSecs = 0; toolchainId = $null
            lockfileSha256 = $null; crateInputHashError = $null; embeddedInputProblems = $null
        }
    }

    # #956: THE CLAIM INSTANT MUST EXIST BEFORE ANY ARTEFACT IS JUDGED. Every freshness verdict
    # below is `$mtimeUtc -ge $runStartUtc`, and `-ge $null` is True for every file: called before
    # the slot claim, this would report a clean rebuild the run did not do, staleArtifactCount 0,
    # instrumentSuspect false -- total, and in the flattering direction (ISSUES 1 on #987). The
    # manifest writer fails loudly on the same null; so does this, before cargo is even asked.
    if ($null -eq $runStartUtc) {
        throw '#956: Get-TestArtifactManifest ran before the slot claim; $runStartUtc is null and every artefact would read fresh'
    }
    # #904: THE INPUTS ARE READ BEFORE THE BUILD, not after it. The hash has to describe the tree
    # that was HANDED to the compile below; computed afterwards it would describe whatever the tree
    # had become, and the window between the two is precisely where a concurrent edit lives.
    #
    # CAUGHT, AND THE FAILURE IS RECORDED RATHER THAN FATAL. A throw here would abort a gate for a
    # missing `rustup` or an unreadable file, and the fallback direction is safe in a way it almost
    # never is: with no hashes, every artefact that was not rebuilt reads `contaminated`, which is
    # RED -- exactly the verdict this gate gives today. Failing closed costs a cold compile; a throw
    # costs the whole run.
    #
    # TIMED, for the same reason the build pass below is: this mechanism buys minutes and it also
    # COSTS something -- one `cargo metadata` and a content hash of every workspace member -- and an
    # uncounted cost decays in silence exactly the way an uncounted saving does. Measured on this
    # box: 9.7 s over 25 members and 567 files, against the 322.8 s it is spent to avoid.
    $crateInputHashWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $crateInputHashes = $null
    $crateInputHashError = $null
    # MEMOISED ACROSS THE RUN (#1053). Get-TestArtifactManifest is called ONCE PER SUITE -- 54 times
    # on this workspace -- and this helper was recomputed on every one of them. Measured at 2.84 s
    # average over three runs, that is +153.2 s per gate from a single helper, 51% of #1053's entire
    # 300 s target, and 306.7 s at twice the suite count: more than the whole budget.
    #
    # THE INVARIANT THAT MAKES THE CACHE SOUND: the crate input hashes are a pure function of the
    # workspace source tree and the toolchain id. A gate run measures ONE head and does not rewrite
    # its own sources, so both are fixed for the life of the run. The key carries both anyway rather
    # than assuming it, so a caller that ever varies either gets a fresh computation instead of a
    # silently wrong one.
    #
    # THE LEDGER READ BELOW IS DELIBERATELY NOT CACHED. It is a file that stages write during the
    # run, so a cached copy could answer with a state that has since moved -- and it is a file read,
    # not a `cargo metadata` plus a content hash of every workspace member. The expensive half is
    # the half that is safe to reuse.
    # `Test-Path variable:` rather than a $null comparison: gate.ps1 runs under Set-StrictMode, where
    # READING an unset $script: variable throws. A $null test would have crashed the gate on the very
    # first suite. Caught by a probe that counted the calls, not by the suite, which stayed 12/12.
    if (-not (Test-Path 'variable:script:crateInputHashCache')) { $script:crateInputHashCache = @{} }
    # THE ONE FILE THE GATE ITSELF REWRITES INSIDE THE HASHED SET, and therefore part of the key.
    #
    # `tools/ci-canary` IS a workspace member (Cargo.toml:29) and `Write-CanaryNonce` rewrites the
    # TRACKED tools/ci-canary/src/nonce.rs every run by design (#152). So "the workspace source is
    # fixed for the life of the run" -- the invariant this cache rests on -- is NOT intrinsically
    # true on this repo. It is true only because the rotation at :4693 happens before both call
    # sites, at :4774 and :4900. Measured, not assumed.
    #
    # That is an ORDERING assumption, and an ordering assumption is exactly the kind that a later
    # edit breaks silently: move the rotation below :4774 and every suite would be judged against a
    # pre-rotation hash, with nothing red to say so. Keying on the nonce's own bytes makes the
    # rotation INVALIDATE the cache instead, so the ordering stops being load-bearing. One small
    # tracked file read per call, against a `cargo metadata` plus a hash of every member avoided.
    #
    # Found by a reviewer's brief asking whether anything the gate writes lands inside the hashed
    # set, not by me: I had written the invariant down and not tested it against this repo.
    $noncePathForKey = Join-Path $repositoryRoot 'tools\ci-canary\src\nonce.rs'
    $nonceStamp = if (Test-Path -LiteralPath $noncePathForKey) {
        (Get-FileHash -LiteralPath $noncePathForKey -Algorithm SHA256).Hash
    } else {
        'absent'
    }
    $crateInputHashKey = "$repositoryRoot|$toolchain|$nonceStamp"
    try {
        if ($script:crateInputHashCache.ContainsKey($crateInputHashKey)) {
            $cached = $script:crateInputHashCache[$crateInputHashKey]
            $crateInputHashes = $cached.Hashes
            $crateInputHashError = $cached.ErrorMessage
            # The NOTE is not reprinted 53 more times, but the FAILURE is still carried: a cached
            # error must refuse exactly as the first one did, or the cache would launder it.
        } else {
            $crateInputHashes = Get-CrateInputHashes -WorkspaceRoot $repositoryRoot -ToolchainArgument $toolchain
            $script:crateInputHashCache[$crateInputHashKey] = [pscustomobject]@{
                Hashes       = $crateInputHashes
                ErrorMessage = $null
            }
        }
    } catch {
        $crateInputHashError = [string]$_.Exception.Message
        $script:crateInputHashCache[$crateInputHashKey] = [pscustomobject]@{
            Hashes       = $null
            ErrorMessage = $crateInputHashError
        }
        Write-Host "[gate] NOTE: the crate input hashes could not be computed, so no artefact can be PROVEN reused this run: $crateInputHashError" -ForegroundColor Yellow
    }
    $ledger = Read-ArtifactLedger -TargetDir $actualTargetDir
    $crateInputHashWatch.Stop()

    # Same treatment Invoke-Stage gives every OTHER native call, needed here too: this cargo
    # invocation runs outside Invoke-Stage (its output is JSON to parse, not human text to
    # forward), so without this it inherits $ErrorActionPreference='Stop' directly and a native
    # stderr progress line becomes a terminating NativeCommandError before $LASTEXITCODE is ever
    # read - caught live (#152's own trap-guard script hit the identical bug on `cargo clean`
    # first, which is exactly why this got checked here too instead of assumed fine).
    #
    # #904: TIMED, because the whole point of proving a reuse is the time it buys, and a saving
    # nobody counts is a saving that decays in silence. Measured on this box before the change:
    # 323.8 s cold against 1.0 s warm, of a 1042 s gate.
    $buildPassWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # `--all-features` is load-bearing and unguarded for the WORKSPACE scope - see the note on
        # the 'workspace tests' stage below. The per-suite scope below deliberately carries none:
        # it must fingerprint the SAME invocation the `cli: <suite>` stage actually runs, not a
        # wider one -- that scope-changes-the-binary property is #243's entire subject.
        $lines = cargo $toolchain test @CargoArgs --locked --no-run --message-format=json 2>&1
        $buildExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
        $buildPassWatch.Stop()
    }
    # A NON-JSON LINE IS DISCARDED, AND ON A FAILED BUILD THAT IS THE ONLY COPY OF THE CAUSE.
    # `$lines` carries cargo's stderr merged in, so a compile or link error arrives here as plain
    # text, fails `ConvertFrom-Json`, and used to vanish at the `continue` below -- leaving a run
    # whose build pass exited non-zero with no diagnosis anywhere on disk. Measured on this branch:
    # a cold run reported `artifactBuildExit: 101` after enumerating 22 of 234 artefacts, published
    # GREEN, and wrote a ledger covering the 22. Nothing said why. Kept only when the build FAILED,
    # because on a green build these lines are hundreds of `Compiling ...` progress rows and
    # printing them would bury the log.
    $nonJson = New-Object System.Collections.Generic.List[string]
    $artifacts = New-Object System.Collections.Generic.List[object]
    foreach ($line in $lines) {
        $parsed = $null
        try { $parsed = $line | ConvertFrom-Json -ErrorAction Stop } catch { [void]$nonJson.Add([string]$line); continue }
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
        # #904: WHICH CRATE'S INPUTS THIS BINARY WAS BUILT FROM. Looked up by package id first --
        # measured on cargo 1.97.1, a compiler-artifact message's `package_id` is the same
        # PackageIDSpec string `cargo metadata` reports as `id`, so the two surfaces agree -- and by
        # the manifest's DIRECTORY second, which is the same fact spelled differently and survives a
        # future cargo changing the id format. Which key answered is RECORDED rather than inferred:
        # a lookup that fell through to neither leaves the hash absent, and an absent hash can never
        # come back as a proven reuse.
        $crateInputHash = $null
        $crateInputHashSource = 'none'
        # #1038 review (lane S): WHY there is no hash, when there is no hash. An absent hash was
        # already RED, but it was red without a sentence, and the remedy for "this crate embeds a
        # macro shape the scanner cannot resolve" is not the remedy for "cargo metadata failed".
        $crateInputHashReason = $null
        if ($null -ne $crateInputHashes) {
            $packageId = [string]$parsed.package_id
            $crateDirectory = ''
            if ($parsed.PSObject.Properties['manifest_path']) {
                $crateDirectory = ConvertTo-ComparablePath -Path (Split-Path -Parent ([string]$parsed.manifest_path))
            }
            if ($crateInputHashes.byPackageId.ContainsKey($packageId)) {
                $crateInputHash = [string]$crateInputHashes.byPackageId[$packageId]
                $crateInputHashSource = 'package-id'
            } elseif (-not [string]::IsNullOrEmpty($crateDirectory) -and $crateInputHashes.byDirectory.ContainsKey($crateDirectory)) {
                $crateInputHash = [string]$crateInputHashes.byDirectory[$crateDirectory]
                $crateInputHashSource = 'manifest-path'
            } elseif ($crateInputHashes.unresolvedByPackageId.ContainsKey($packageId)) {
                $crateInputHashSource = 'unresolved-embedded-input'
                $crateInputHashReason = [string]$crateInputHashes.unresolvedByPackageId[$packageId]
            } elseif (-not [string]::IsNullOrEmpty($crateDirectory) -and $crateInputHashes.unresolvedByDirectory.ContainsKey($crateDirectory)) {
                $crateInputHashSource = 'unresolved-embedded-input'
                $crateInputHashReason = [string]$crateInputHashes.unresolvedByDirectory[$crateDirectory]
            }
        }
        $ledgerKey = ConvertTo-ComparablePath -Path $exePath
        $ledgerEntry = $null
        if ($ledger.ContainsKey($ledgerKey)) { $ledgerEntry = $ledger[$ledgerKey] }
        $reuseProof = Get-ArtifactReuseProof -RebuiltThisRun ([bool]$freshBuild) -ExeSha256 ([string]$hash) `
            -CrateInputHash ([string]$crateInputHash) -LedgerEntry $ledgerEntry `
            -UnresolvedInputs ([string]::Equals($crateInputHashSource, 'unresolved-embedded-input', [System.StringComparison]::Ordinal))
        $artifacts.Add([ordered]@{
            package    = $parsed.package_id
            target     = $parsed.target.name
            executable = $exePath
            # #1007 (thread 5): the INVOCATION that produced this row. A per-suite row and the
            # workspace `--all-features` row for the same target differed only by the hash-suffixed
            # path; no consumer decides on this field today, but a reader of `staleArtifacts` could
            # not tell the two apart without it.
            scope      = ($CargoArgs -join ' ')
            sha256     = $hash
            mtimeUtc   = if ($mtimeUtc) { $mtimeUtc.ToString('o') } else { $null }
            freshBuild = $freshBuild
            # #904: WHAT THIS RUN CAN VOUCH FOR. `freshBuild` above is unchanged and is now a
            # RECORD -- what a cold-only gate would have said -- while `reuseProof` is the verdict.
            crateInputHash       = $crateInputHash
            crateInputHashSource = $crateInputHashSource
            crateInputHashReason = $crateInputHashReason
            ledgerInputHash      = if ($null -ne $ledgerEntry) { [string]$ledgerEntry['crateInputHash'] } else { $null }
            ledgerSha256         = if ($null -ne $ledgerEntry) { [string]$ledgerEntry['sha256'] } else { $null }
            reuseProof           = $reuseProof
        })
    }
    # #904: COUNTED HERE, where the population is whole. A consumer that recomputed these from
    # `artifacts` would be a second copy of the classification, and a second copy is a second
    # chance to disagree.
    $rebuilt = @($artifacts | Where-Object { [string]::Equals([string]$_['reuseProof'], 'rebuilt', [System.StringComparison]::Ordinal) }).Count
    $proven = @($artifacts | Where-Object { [string]::Equals([string]$_['reuseProof'], 'proven-reuse', [System.StringComparison]::Ordinal) }).Count
    $unproven = @($artifacts | Where-Object { [string]::Equals([string]$_['reuseProof'], 'unproven-reuse', [System.StringComparison]::Ordinal) }).Count
    $contaminated = @($artifacts | Where-Object { [string]::Equals([string]$_['reuseProof'], 'contaminated', [System.StringComparison]::Ordinal) }).Count
    # `cold` means NOTHING was reused; anything else is `warm`. A run with no artefacts at all is
    # neither -- it is `unknown`, because zero reused out of zero is not a cold build, it is an
    # enumeration that found nothing, and those two must not share a word.
    $buildMode = if ($artifacts.Count -eq 0) { 'unknown' } elseif ($rebuilt -eq $artifacts.Count) { 'cold' } else { 'warm' }
    # THE BUILD PASS'S OWN FAILURE IS SAID OUT LOUD, where a reader of the log meets it in order.
    # The exit code alone reached the manifest and no further: `artifactBuildExit` has five sites in
    # this file and not one of them is a verdict term, so a build pass that died mid-enumeration
    # published GREEN over a partial artefact list. The caller decides the verdict (see the
    # freshness cross-check); this function's job is to make the failure legible instead of leaving
    # the count of enumerated artefacts as the only hint that something went wrong.
    if ($buildExit -ne 0) {
        Write-Host "[gate] THE ARTEFACT BUILD PASS FAILED: cargo exited $buildExit after enumerating $($artifacts.Count) test binary(ies)." -ForegroundColor Red
        Write-Host '[gate] Every stage below still compiles what it needs, so stages can pass while this list is INCOMPLETE -- and an incomplete list is an incomplete artefact ledger.' -ForegroundColor Red
        $tail = @($nonJson | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Last 30)
        if ($tail.Count -gt 0) {
            Write-Host "[gate] cargo's own last $($tail.Count) non-JSON line(s):" -ForegroundColor Red
            foreach ($t in $tail) { Write-Host "    $t" }
        } else {
            Write-Host '[gate] cargo produced no non-JSON output, so it exited non-zero without saying why.' -ForegroundColor Red
        }
    }
    # #1007 TAKEOVER MERGE: main's #904 classification and this branch's redacted diagnostic
    # tail are COMPLEMENTARY, not competing. Main counts the reuse population and returns it;
    # this branch renders cargo's JSON compiler-messages into readable, redacted lines so a
    # fingerprint failure says WHY. Both are kept: the classification is untouched, and the
    # selected tail is added BESIDE main's raw tail rather than replacing it -- the raw one is
    # what cargo said, the selected one is what a reader needs, and #484 is the record of
    # losing the first by keeping only the second.
    $evidenceTail = @()
    if ($buildExit -ne 0) {
        # Cargo's compiler messages are JSON, so the evidence selector cannot recognise their
        # error markers until rendered text is decoded into lines. Redact AFTER decoding: secrets
        # can be escaped inside JSON. Other output, including ordinary stderr, stays in the stream.
        $diagnosticLines = New-Object System.Collections.Generic.List[string]
        foreach ($rawLine in $lines) {
            $text = [string]$rawLine
            $messageRecord = $null
            # A line that is not JSON is cargo's plain stderr (progress, warnings, the human error
            # summary). It is NOT swallowed: the first pass over $lines above already appended every
            # such line to $nonJson, which is printed and published as buildPassNonJsonTail. This
            # second pass only wants the compiler-message records, so the non-JSON case is a null
            # here BY DESIGN and its word was said once, by the first pass.
            try { $messageRecord = $text | ConvertFrom-Json -ErrorAction Stop } catch { $messageRecord = $null }
            if ($null -ne $messageRecord -and $messageRecord.PSObject.Properties['reason'] -and $messageRecord.reason -eq 'compiler-message' -and $messageRecord.PSObject.Properties['message']) {
                $message = $messageRecord.message
                if ($null -ne $message -and $message.PSObject.Properties['rendered'] -and -not [string]::IsNullOrWhiteSpace([string]$message.rendered)) {
                    $text = [string]$message.rendered
                } elseif ($null -ne $message -and $message.PSObject.Properties['message']) {
                    $level = 'diagnostic'
                    if ($message.PSObject.Properties['level']) { $level = [string]$message.level }
                    $text = "${level}: $($message.message)"
                }
            }
            foreach ($diagnosticLine in ($text -split '\r?\n')) { $diagnosticLines.Add($diagnosticLine) }
        }
        $protectedLines = @($diagnosticLines | ForEach-Object { Protect-GateEvidenceLine -Line $_ })
        $evidenceTail = @(Select-GateEvidenceLines -Lines $protectedLines -Budget 40)
        if ($evidenceTail.Count -eq 0) {
            $evidenceTail = @("<absent: fingerprint exit $buildExit with no output on stdout or stderr>")
        }
    }
    # #1007 ENROLMENT. A binary with NO ledger row that cargo chose not to rebuild is `unproven-reuse`
    # and RED -- correctly, the run cannot vouch for it. But it is not contamination: the ledger
    # simply never covered it. That happens to every binary the first time the ledger learns of it
    # (the first run after this mechanism landed found 212 of them; #1007 adds ~54 per-suite
    # binaries in one go), and cargo, by mtime, sees nothing to rebuild -- so the RED repeats on
    # every run until a human deletes the target. The remedy the doc promised ("run once cold") is
    # done HERE, for exactly those binaries and no others: delete the executable so cargo MUST
    # rebuild it, run the same enumeration once more, and let the mtime rule -- which has full
    # authority inside the run's own window -- prove it. `contaminated` (a row that DISAGREES) is
    # deliberately not enrolled: that is the backdated-mtime shape this whole mechanism exists to
    # catch, and rebuilding it here would erase the evidence. One retry, flagged, never nested.
    if (-not $Enrolling -and $buildExit -eq 0) {
        $enrol = @($artifacts | Where-Object {
            [string]::Equals([string]$_['reuseProof'], 'unproven-reuse', [System.StringComparison]::Ordinal) -and
            $null -eq $_['ledgerInputHash'] -and $null -eq $_['ledgerSha256'] -and
            -not [string]::Equals([string]$_['crateInputHashSource'], 'unresolved-embedded-input', [System.StringComparison]::Ordinal) -and
            -not [string]::IsNullOrEmpty([string]$_['executable']) -and (Test-Path -LiteralPath ([string]$_['executable']))
        })
        if ($enrol.Count -gt 0) {
            Write-Host "[gate] ENROLLING $($enrol.Count) test binary(ies) the ledger has no row for: deleting them so cargo rebuilds them inside this run's window, then enumerating once more." -ForegroundColor Yellow
            foreach ($candidate in $enrol) {
                Remove-Item -LiteralPath ([string]$candidate['executable']) -Force -ErrorAction Stop
            }
            $again = Get-TestArtifactManifest -CargoArgs $CargoArgs -Enrolling
            $again['artifactsEnrolled'] = $enrol.Count
            return $again
        }
    }
    return [ordered]@{
        buildExitCode          = $buildExit
        buildPassNonJsonTail   = @($nonJson | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Last 30)
        # #1007: how many binaries THIS enumeration enrolled (see above). 0 on the retry itself and on
        # every run whose ledger already covers everything; the outer call overwrites it with the count.
        artifactsEnrolled      = 0
        artifacts              = $artifacts
        outputTail             = $evidenceTail
        # #904: the record the audit of this change reads. `staleArtifactCount` in the run manifest
        # keeps saying what a cold-only gate would have refused; these say what was proven instead.
        artifactsRebuilt       = $rebuilt
        artifactsProvenReuse   = $proven
        artifactsUnprovenReuse = $unproven
        artifactsContaminated  = $contaminated
        buildMode              = $buildMode
        buildPassSecs          = [math]::Round($buildPassWatch.Elapsed.TotalSeconds, 3)
        crateInputHashSecs     = [math]::Round($crateInputHashWatch.Elapsed.TotalSeconds, 3)
        toolchainId            = if ($null -ne $crateInputHashes) { [string]$crateInputHashes.toolchainId } else { $null }
        lockfileSha256         = if ($null -ne $crateInputHashes) { [string]$crateInputHashes.lockfileSha256 } else { $null }
        crateInputHashError    = $crateInputHashError
        # #1038 review (lane S): the crates whose compile-time inputs this run could not read whole,
        # named -- `<package id>: <file>: <macro and argument>`. Published even when it is empty,
        # because an empty list is a measurement that found nothing and an ABSENT field is a run
        # that did not look, and a reader of two manifests must be able to tell those apart.
        embeddedInputProblems  = if ($null -ne $crateInputHashes) {
            [string[]]@(Sort-Ordinal -Values (@($crateInputHashes.unresolvedByPackageId.Keys |
                            ForEach-Object { "$([string]$_): $([string]$crateInputHashes.unresolvedByPackageId[$_])" })))
        } else {
            $null
        }
    }
}

function Get-TargetBuildState {
    <#
        #455: whether a target directory can be REUSED, answered before the first compile.

        The late artefact-freshness flag (`$freshBuild = ($mtimeUtc -ge $runStartUtc)` below) is
        correct and useless this early: at the instant before a build nothing has been rebuilt, so
        every artefact of every reused target predates the run's start. Measured: 77 of 77 test
        executables in a real target fail that predicate at run start, while across the 128
        manifests then committed in `.factory/gate-runs/` only 9 of 27 observed reuses were actually
        contaminated.
        The canary nonce is no better a key -- `Write-CanaryNonce` rotates it unconditionally every
        run (#152), so a nonce-keyed check also flags all 27. An early guard wrong two times in
        three is one the fleet learns to skip, and it then occupies the place where a real one
        would go.

        The distinction that DOES separate them is in #455's own evidence: the contaminated target
        was reused "after an earlier gate ATTEMPT". A target left mid-build carries cargo
        fingerprints claiming freshness for binaries whose source has since moved; a target left by
        a run that finished does not. So the question this asks is not what the target was built
        FOR, but whether the last build ENDED.

        Fails closed, with one deliberate exception: an absent marker condemns a directory that
        already holds build output, and does NOT condemn an empty or missing one. Without that
        exception "absent means contaminated" would flag every first run on a new target, and the
        guard would collapse back into the naive check it exists to replace.

        KNOWN LIMIT, in the safe direction: the owner check asks the operating system whether the
        recorded process id is still running, and a recycled pid therefore reads as `concurrent`
        when the truth is `interrupted`. Both are contaminated, so the VERDICT is unaffected and
        only the named cause can be wrong. Narrowing it further would add a path no cell covers.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $TargetDir)

    # The name is repeated in Write-TargetBuildState rather than shared, because each function is
    # lifted out of this file on its own by ci/gate-target-build-state.tests.ps1 and a script-scope
    # constant would not travel with it. The round-trip cell in that suite is what holds the two
    # copies together: a writer that emitted a different name would read back as 'unknown'.
    $markerName = '.graphhelm-build-state.json'

    if ([string]::IsNullOrWhiteSpace($TargetDir) -or -not (Test-Path -LiteralPath $TargetDir)) {
        return [ordered]@{
            contaminated = $false
            suspect      = $false
            state        = 'fresh'
            reason       = 'no target directory yet, so nothing is being reused'
        }
    }

    # BOUNDED IN DEPTH, not merely short-circuited (Codex on #1009). The first version recursed with
    # `Select-Object -First 1`, which stops only once a FILE has been emitted -- so a target whose
    # tree begins with many directories is walked in full before the pipeline can stop, and this runs
    # before the first compile in the only authoritative gate. Depth is now fixed: the target's own
    # top level, then `debug` and `release`, never below them. Cargo puts its output in those two, so
    # a directory holding build output is found by looking exactly where build output goes.
    $hasOutput = $false
    $scanExhausted = $false
    $probeFailed = $false
    # FILES, never directories -- measured, and the cell caught it: counting entries at the top level
    # made an EMPTY target "have output", because the empty `debug` directory is itself an entry.
    # `debug`/`release` existing says nothing; something INSIDE them does.
    # `deps` is named because a target used only for `cargo test` can have NOTHING at the top of
    # `debug` and all of its binaries one level down -- a probe that stopped at `debug` would read
    # such a target as empty and skip the very check this function exists for. Depth is still fixed.
    $probes = @(
        $TargetDir,
        (Join-Path $TargetDir 'debug'), (Join-Path $TargetDir 'debug\deps'),
        (Join-Path $TargetDir 'release'), (Join-Path $TargetDir 'release\deps')
    )
    foreach ($probe in $probes) {
        if ($hasOutput) { break }
        if (-not (Test-Path -LiteralPath $probe)) { continue }
        # BREADTH AS WELL AS DEPTH (Codex on #1009). `-File` filters AFTER enumeration, so a probed
        # directory holding only subdirectories is walked in full before `Select-Object` can see a
        # file and stop. The ceiling is on ENTRIES INSPECTED, not on files found: 512 is far past any
        # real `debug` or `deps`, and a directory wider than that has told us enough -- we are asking
        # "is there any build output here", and a target that wide is not one this gate should be
        # walking before its first compile.
        # THE ENUMERATION ITSELF IS BOUNDED, not only the loop body (Codex on #1009). PowerShell's
        # `foreach` statement materialises its whole collection BEFORE the body runs once, so a
        # counter inside the body bounds nothing: measured on a 5000-entry directory,
        # `Get-ChildItem` + break at 512 took 76 ms while the lazy enumerator took 9 ms and inspected
        # the same 513. `EnumerateFileSystemInfos` streams, and its `Attributes` come from the find
        # data already read -- so telling a file from a directory costs no extra call.
        $inspected = 0
        try {
            $probeInfo = New-Object System.IO.DirectoryInfo $probe
            foreach ($entry in $probeInfo.EnumerateFileSystemInfos()) {
                $inspected++
                # EXHAUSTING THE CAP IS NOT "NOTHING HERE" (Codex on #1009). Breaking out left
                # `$hasOutput` false, which nothing downstream could tell apart from having looked at
                # everything and found none -- so a target whose first 512 entries are directories
                # read as `fresh`, and the check this function exists for was skipped on the one
                # target too wide to inspect. Two states in one representation, inside my own bound.
                if ($inspected -gt 512) { $scanExhausted = $true; break }
                if ($entry.Attributes -band [System.IO.FileAttributes]::Directory) { continue }
                if ($entry.Name -eq $markerName) { continue }
                $hasOutput = $true
                break
            }
        } catch {
            # A PROBE WE COULD NOT READ IS NOT A PROBE THAT FOUND NOTHING (Codex/J on #1009). This
            # catch was silent, so a restrictive ACL or a transient I/O error left BOTH flags false
            # and an unmarked target then returned `fresh` -- "could not inspect" arriving downstream
            # as "contains nothing, safe to reuse". A FALSE GREEN inside the mechanism this PR exists
            # to build, and the same shape as the exhaustion case one commit earlier: exhaustion got
            # a state and failure did not.
            $probeFailed = $true
        }
    }

    $markerPath = Join-Path $TargetDir $markerName
    if (-not (Test-Path -LiteralPath $markerPath)) {
        if (-not $hasOutput -and ($scanExhausted -or $probeFailed)) {
            $whyUnknown = if ($scanExhausted) {
                'a probed directory exceeded the 512-entry scan bound before any build output was seen'
            } else {
                'a probed directory could not be read'
            }
            return [ordered]@{
                contaminated = $true
                suspect      = $false
                state        = 'unknown'
                reason       = "$whyUnknown, so whether this target holds output is unknown"
            }
        }
        if (-not $hasOutput) {
            return [ordered]@{
                contaminated = $false
                suspect      = $false
                state        = 'fresh'
                reason       = 'the target directory holds no build output, so nothing is being reused'
            }
        }
        return [ordered]@{
            contaminated = $true
            suspect      = $false
            state        = 'unknown'
            reason       = "the target holds build output but no $markerName, so what produced it cannot be established"
        }
    }

    # BOUNDED BEFORE IT IS READ (Codex on #1009). This file lives in a target directory another
    # process wrote, and this runs before the first compile in the only authoritative gate -- so an
    # oversized or deeply nested marker must become a bounded UNKNOWN, never a parse that exhausts
    # the gate. A real marker is four short fields; 64 KiB is four hundred times that.
    # ONE HANDLE FOR THE CHECK AND THE READ (Codex on #1009). Asking `FileInfo.Length` and then
    # calling `Get-Content -Raw` are two operations on a path another process owns: it can replace or
    # grow the file in between, and the read would then consume whatever is there despite the guard.
    # The stream below is opened once and reads at most 64 KiB + 1 byte; if that last byte exists the
    # file is over the bound, and the bound is enforced by the SAME handle that produced the bytes.
    $markerText = $null
    $markerTooBig = $false
    try {
        $stream = [System.IO.File]::Open($markerPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        try {
            $buffer = New-Object byte[] 65537
            $read = 0
            while ($read -lt 65537) {
                $n = $stream.Read($buffer, $read, 65537 - $read)
                if ($n -le 0) { break }
                $read += $n
            }
            if ($read -gt 65536) { $markerTooBig = $true }
            else { $markerText = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $read) }
        } finally { $stream.Dispose() }
    } catch {
        $markerText = $null
    }
    if ($markerTooBig) {
        return [ordered]@{
            contaminated = $true
            suspect      = $false
            state        = 'unknown'
            reason       = "$markerName is larger than the 65536 bytes this gate will parse"
        }
    }
    $marker = $null
    if ($null -ne $markerText) {
        try { $marker = $markerText | ConvertFrom-Json -ErrorAction Stop } catch { $marker = $null }
    }
    if ($null -eq $marker) {
        return [ordered]@{
            contaminated = $true
            suspect      = $false
            state        = 'unknown'
            reason       = "$markerName is present but could not be read as JSON, so provenance is unknown"
        }
    }

    # `$marker.state` would THROW under Set-StrictMode 2.0 when the field is absent, which is one of
    # the states this function exists to report. Read through PSObject.Properties instead.
    $recordedState = ''
    if ($null -ne $marker.PSObject.Properties['state']) {
        $recordedState = [string] $marker.PSObject.Properties['state'].Value
    }
    if ([string]::IsNullOrWhiteSpace($recordedState)) {
        return [ordered]@{
            contaminated = $true
            suspect      = $false
            state        = 'unknown'
            reason       = "$markerName parses but names no build state -- valid JSON is not a valid marker"
        }
    }

    if ([string]::Equals($recordedState, 'complete', [System.StringComparison]::Ordinal)) {
        return [ordered]@{
            contaminated = $false
            suspect      = $false
            state        = 'complete'
            reason       = 'the last build in this target finished, so the reuse is a legitimate one'
        }
    }

    # #1007: A RUN THAT FINISHED AND COULD NOT VOUCH IS NOT A RUN THAT DIED. Before this state existed
    # such a run left its `building` mark in place, the next run found the owner gone and read
    # `interrupted` -- suspect -- and aborted at the #455 guard before its first compile, sticky
    # until a human deleted the target. The comment beside the stamp said the next run "rebuilds
    # cold"; it did not, it refused. `unproven` is the honest word: the pass completed, the ledger
    # was not written because one or more binaries could not be proven, and the next run must run
    # its own freshness check like any other -- contaminated (no clean-reuse claim), NOT suspect
    # (nothing is half-written; cargo's fingerprints are whole).
    if ([string]::Equals($recordedState, 'unproven', [System.StringComparison]::Ordinal)) {
        return [ordered]@{
            contaminated = $true
            suspect      = $false
            state        = 'unproven'
            reason       = 'the last run in this target finished but could not vouch for every binary, so nothing is reused on trust; this run proves or rebuilds each one itself'
        }
    }

    if ([string]::Equals($recordedState, 'building', [System.StringComparison]::Ordinal)) {
        $ownerPid = 0
        if ($null -ne $marker.PSObject.Properties['processId']) {
            [void][int]::TryParse([string]$marker.PSObject.Properties['processId'].Value, [ref] $ownerPid)
        }
        $owner = $null
        if ($ownerPid -gt 0) {
            $owner = Get-Process -Id $ownerPid -ErrorAction SilentlyContinue
        }
        if ($null -ne $owner) {
            return [ordered]@{
                contaminated = $true
                suspect      = $true
                state        = 'concurrent'
                reason       = "process $ownerPid is still building in this target directory -- two gates in one target corrupt each other's fingerprints"
            }
        }
        return [ordered]@{
            contaminated = $true
            suspect      = $true
            state        = 'interrupted'
            reason       = "the build owned by process $ownerPid never finished and that process is gone, so cargo's fingerprints may claim freshness for binaries whose source has moved"
        }
    }

    # NEVER ECHOED VERBATIM (Codex on #1009). `$recordedState` is a string another process put in a
    # file, and this `reason` is printed to the gate console AND persisted into `targetBuildState` in
    # a manifest that is COMMITTED. A hostile or accidental value could carry newlines or terminal
    # control sequences into the log, or copy content into the repository. So the value is quoted
    # back only when it is a plain short identifier -- which keeps the diagnostic useful for the
    # ordinary case, a typo or a state from a newer gate -- and is otherwise described by SHAPE.
    $safeState = ''
    if ($recordedState -cmatch '^[A-Za-z0-9_-]{1,32}$') { $safeState = "'$recordedState'" }
    else { $safeState = "a value that is not a plain identifier ($($recordedState.Length) chars)" }
    return [ordered]@{
        contaminated = $true
        suspect      = $false
        state        = 'unknown'
        reason       = "$markerName names an unrecognised build state $safeState"
    }
}

function Write-TargetBuildState {
    <#
        #455: the other half of Get-TargetBuildState. Called with 'building' before the first
        compile and 'complete' after the build stage returns, so that a run which dies in between
        leaves a marker saying exactly that.

        Written at the target ROOT, never under debug/ or release/, so no cargo fingerprint hashes
        it: a marker that became build input would change every crate's fingerprint and destroy the
        caching this guard exists to preserve.
    #>
    param(
        [Parameter(Mandatory)] [string] $TargetDir,
        # NOT a [ValidateSet]. ci/classify-run.tests.ps1 DERIVES the gate's closed status
        # vocabulary by finding a ValidateSet in this file rather than copying it, so a second one
        # here silently redefines what a gate status is: adding this function with the attribute
        # made that suite report the gate's statuses as `building, complete`. The constraint is kept
        # -- it just cannot be spelled in the shape another guard reads as its subject.
        [Parameter(Mandatory)] [string] $State,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $Head
    )
    # #1007 adds `unproven`: the pass FINISHED but could not vouch for every binary. It is a terminal
    # state like `complete`, written where `complete` would have been, so that a finished-unproven
    # run is never mistaken for one that died mid-compile (see the `unproven` branch of the reader).
    if (-not ([string]::Equals($State, 'building', [System.StringComparison]::Ordinal) -or
            [string]::Equals($State, 'complete', [System.StringComparison]::Ordinal) -or
            [string]::Equals($State, 'unproven', [System.StringComparison]::Ordinal))) {
        throw "Write-TargetBuildState: '$State' is not a build state; expected 'building', 'complete' or 'unproven'."
    }
    if (-not (Test-Path -LiteralPath $TargetDir)) {
        [void][System.IO.Directory]::CreateDirectory($TargetDir)
    }
    $document = [ordered]@{
        state      = $State
        processId  = $PID
        head       = $Head
        startedUtc = [DateTime]::UtcNow.ToString('o')
    }
    # No BOM, for the reason Write-CanaryNonce gives: 5.1's -Encoding utf8 always prepends one.
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText(
        (Join-Path $TargetDir '.graphhelm-build-state.json'),
        ($document | ConvertTo-Json -Compress),
        $utf8NoBom)
}

function Get-GateStatus {
    <#
        #455: the run's status, as a rule a cell can call.

        Extracted from an inline `if` at the caller because the claim it carries could not otherwise
        be tested: `merge-proof.ps1` (retired 2026-09-24) required `status == GREEN` and read neither
        `overallPassed` nor `instrumentSuspect` -- measured, the latter appeared there zero times --
        so the STATUS was the only field that gated anything, and a rule that decides it belongs
        where a cell can reach it.

        HARNESS-BROKE and not RED for a suspect target. RED is a claim about the tree, and the tree
        did not fail: the run could not vouch for what it measured, which is the third state this
        vocabulary already carries. A red raised for the instrument sends a lane hunting a defect
        that is not there, and is the kind pressers learn to wave through.

        Failure outranks suspicion: a run with a failed stage is RED whatever the target looked like,
        because the failing stage is the more specific fact.
    #>
    param(
        [Parameter(Mandatory)] [int] $FailedStageCount,
        [Parameter(Mandatory)] [bool] $TargetSuspect
    )
    if ($FailedStageCount -ne 0) { return 'RED' }
    if ($TargetSuspect) { return 'HARNESS-BROKE' }
    return 'GREEN'
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

function Invoke-LandingCommand {
    # The deadline bounds admission and polling, not OS Start/Kill/Dispose calls. A timed-out
    # command refuses the gate; it is not a promise that all native descendants have exited.
    param([string]$Program, [string[]]$Arguments, [string]$Root, [int]$TimeoutMilliseconds = 30000)
    if ($TimeoutMilliseconds -le 0) { throw 'Landing metadata command admission expired.' }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Program
    $info.WorkingDirectory = $Root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = New-Object System.Text.UTF8Encoding($false)
    # Windows CRT quoting also works for ProcessStartInfo on supported Unix .NET hosts.
    $info.Arguments = (@($Arguments | ForEach-Object {
        '"' + (($_ -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"'
    }) -join ' ')
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $started = $false
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Landing metadata command did not start.' }
        $outBuffer = New-Object char[] 4096
        $errBuffer = New-Object char[] 4096
        $outRead = $process.StandardOutput.ReadAsync($outBuffer, 0, $outBuffer.Length)
        $errRead = $process.StandardError.ReadAsync($errBuffer, 0, $errBuffer.Length)
        $output = New-Object Text.StringBuilder
        $total = 0L
        while ($null -ne $outRead -or $null -ne $errRead -or -not $process.HasExited) {
            if ($clock.ElapsedMilliseconds -ge $TimeoutMilliseconds) { throw 'Landing metadata command timed out.' }
            if ($null -ne $outRead -and $outRead.IsCompleted) {
                $count = $outRead.GetAwaiter().GetResult()
                $total += $count
                if ($total -gt 4194304) { throw 'Landing metadata output exceeded 4 MiB characters.' }
                if ($count -eq 0) { $outRead = $null } else {
                    [void]$output.Append($outBuffer, 0, $count)
                    $outRead = $process.StandardOutput.ReadAsync($outBuffer, 0, $outBuffer.Length)
                }
            }
            if ($null -ne $errRead -and $errRead.IsCompleted) {
                $count = $errRead.GetAwaiter().GetResult()
                $total += $count
                if ($total -gt 4194304) { throw 'Landing metadata output exceeded 4 MiB characters.' }
                if ($count -eq 0) { $errRead = $null } else {
                    $errRead = $process.StandardError.ReadAsync($errBuffer, 0, $errBuffer.Length)
                }
            }
            Start-Sleep -Milliseconds 10
        }
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Output = $output.ToString() }
    } finally {
        try { if ($started -and -not $process.HasExited) { $process.Kill() } }
        finally { $process.Dispose() }
    }
}

function Get-LandingSnapshot {
    param([string]$Root, [string]$Head, [string]$ExpectedRef, [switch]$LocalOnly, [string]$SnapshotPath)
    if ($Head -cnotmatch '^[0-9a-f]{40}$') { throw 'Cannot capture landing snapshot without candidate SHA.' }
    $mode = 'non-pr'
    $name = if ($ExpectedRef) { $ExpectedRef } else { 'origin/main' }
    $landing = $null
    $number = $null
    if ($SnapshotPath) {
        if ($LocalOnly) { throw 'NonPullRequest cannot be combined with LandingSnapshotPath.' }
        $file = Get-Item -LiteralPath $SnapshotPath -ErrorAction Stop
        if ($file.Length -gt 8192) { throw 'Landing snapshot exceeds 8192 bytes.' }
        $data = New-Object byte[] 8193
        $stream = [IO.File]::OpenRead($file.FullName)
        try {
            $used = 0
            while ($used -lt $data.Length) {
                $read = $stream.Read($data, $used, $data.Length - $used)
                if ($read -eq 0) { break }
                $used += $read
            }
        } finally { $stream.Dispose() }
        if ($used -gt 8192) { throw 'Landing snapshot exceeds 8192 bytes.' }
        $record = ConvertFrom-Json -InputObject ([Text.Encoding]::UTF8.GetString($data, 0, $used)) -ErrorAction Stop
        if ($record -isnot [pscustomobject] -or $record.head -isnot [string] -or
            -not [string]::Equals($record.head, $Head, [StringComparison]::Ordinal) -or
            $record.sha -isnot [string] -or $record.ref -isnot [string] -or
            ($record.pullRequest -isnot [int] -and $record.pullRequest -isnot [long]) -or $record.pullRequest -le 0) {
            throw 'Landing snapshot must identify the captured source head, base and PR.'
        }
        $name = $record.ref
        $landing = $record.sha
        $number = $record.pullRequest
        if ($landing -cnotmatch '^[0-9a-f]{40}$' -or [string]::IsNullOrWhiteSpace($name)) { throw 'PR base metadata is incomplete.' }
        if ($ExpectedRef -and -not [string]::Equals($ExpectedRef, $name, [StringComparison]::Ordinal)) { throw 'MergeTargetRef does not match the actual PR base.' }
        $mode = 'supplied-pr'
    }
    $reference = if ($landing) { $landing } else { $name }
    $verified = Invoke-LandingCommand -Program git -Root $Root -Arguments @('rev-parse', '--verify', '--end-of-options', "$reference^{commit}")
    $sha = $verified.Output.Trim()
    if ($verified.ExitCode -ne 0 -or $sha -cnotmatch '^[0-9a-f]{40}$' -or ($landing -and -not [string]::Equals($sha, $landing, [StringComparison]::Ordinal))) { throw 'Cannot verify captured landing commit.' }
    $base = Invoke-LandingCommand -Program git -Root $Root -Arguments @('merge-base', $Head, $sha)
    $baseSha = $base.Output.Trim()
    if ($base.ExitCode -ne 0 -or $baseSha -cnotmatch '^[0-9a-f]{40}$') { throw 'Cannot compute captured merge base.' }
    # This describes one gate-time snapshot. Neither this record nor a later read binds a merge.
    return [pscustomobject]@{ mode = $mode; ref = $name; head = $Head; sha = $sha; mergeBase = $baseSha; pullRequest = $number }
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
                            # that treats different strings as the same string. Found in ci/merge-proof.ps1 (since retired) first, where
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
            # CAPTURED, THEN THE EXIT CODE, THEN REDUCED (#762). `Select-Object -First` stops the
            # pipeline as soon as it has its item, and stopping a pipeline TERMINATES the native
            # command feeding it: on PowerShell 7 that leaves `$LASTEXITCODE` at -1 with the VALUE
            # correct. Reading the code after the stop reports a failure that did not happen, and
            # here that failure means "the branch moved", which turns a healthy run RED.
            $refNowOutput = @(& git rev-parse --verify --quiet "$branchRef" 2>$null)
            $refNowExit = $LASTEXITCODE
            $refNow = $refNowOutput | Select-Object -First 1
            # The compare-and-swap's own comparison: the one place where an approximate comparer
            # would let the record be written against a ref that had moved.
            $refMoved = ($refNowExit -ne 0 -or
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
            # THROUGH THE ONE WRITER (#938). This used to be a per-path loop that rewrote each copy
            # as it walked, with `WriteAllText` staging and a null-backup `File.Replace` -- the third
            # hand-copy of the discipline #742 gave a single implementation, and the only one that
            # also LEAKED its staging file: its `catch` set a message and nothing deleted the
            # `.reconciling` sibling. Measured on this machine under five concurrent gates, that
            # failure is reachable: `File.Replace` -> "Unable to remove the file to be replaced".
            #
            # A loop that writes as it walks cannot roll back what it already committed, so the
            # subset is decided first and written as ONE unit. Missing copies are partitioned out:
            # `File.Replace` cannot create a destination, and a vanished twin used to report
            # `could not be rewritten: FileNotFound`, a message about the wrong thing.
            $reconcile = Sync-ManifestCopies -Paths (@($ManifestPath) + @($Copies)) -Content $Content
            if ($reconcile.Reconciled.Count -gt 0) {
                Write-Host ("[gate] $($reconcile.Reconciled.Count) manifest copy/copies did not match what was " +
                    'published, and were rewritten from the published bytes.') -ForegroundColor Yellow
            }
            if ($reconcile.Failure) {
                $script:manifestReconcileFailed = $reconcile.Failure
                Write-Host ("[gate] $script:manifestReconcileFailed. The COMMITTED record is the authoritative " +
                    'one, and this run is RED.') -ForegroundColor Red
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
        # Captured before reduced, see #762 at the compare-and-swap above.
        $branchAtIndexOutput = @(& git symbolic-ref --quiet HEAD 2>$null)
        $branchAtIndexExit = $LASTEXITCODE
        $branchAtIndexTime = $branchAtIndexOutput | Select-Object -First 1
        if ($branchAtIndexExit -ne 0 -or
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
            # Captured before reduced, see #762 at the compare-and-swap above.
            $branchAfterTouchOutput = @(& git symbolic-ref --quiet HEAD 2>$null)
            $branchAfterTouchExit = $LASTEXITCODE
            $branchAfterTouch = $branchAfterTouchOutput | Select-Object -First 1
            if ($branchAfterTouchExit -eq 0 -and
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

function Get-EventContractSweepRecord {
    <#
        #205: what the event-contract sweep could say about this run, as a fact the record carries.

        The sweep runs inside `ci powershell suites` and its population is the REMOTE: a night the
        network does not answer is a fact about the instrument, not about the tree, so it must not
        become `status: RED` (a red that fires on an unstable network is the class pressers learn to
        wave through, and then the real collision hides behind it). The suite prints one marker line
        into the stage capture -- `[event-contract] MEASURED -- ...` or
        `[event-contract] NOT MEASURED -- <reason>` -- and this rule turns it into the manifest's
        `eventContractSweep` so a presser sees it without reading the transcript.

        `absent` is its own state: a run whose capture holds neither marker (an older suite, a
        stage that never ran) is not a run that measured nothing.
        Exactly one recognized marker is required. Duplicate markers, even identical ones,
        make the global capture ambiguous and cannot establish measurement.
    #>
    param([AllowNull()] [AllowEmptyCollection()] [string[]] $Lines)
    $state = 'absent'
    $reason = ''
    $markers = 0
    foreach ($line in @($Lines)) {
        $text = [string] $line
        if ($text -cmatch '^\[event-contract\] NOT MEASURED -- (.*)$') {
            $markers++
            $state = 'notMeasured'
            $reason = $Matches[1].Trim()
        } elseif ($text -cmatch '^\[event-contract\] MEASURED -- ') {
            $markers++
            $state = 'measured'
            $reason = ''
        }
    }
    if ($markers -gt 1) {
        $state = 'notMeasured'
        $reason = 'ambiguous sweep capture: multiple recognized markers'
    }
    return [ordered]@{
        state  = $state
        reason = $reason
    }
}

function Get-RunCoverage {
    <#
        What this run actually COVERED, as a fact the record carries rather than an inference a
        reader makes from a console line (#725, case 1).

        `-SkipPostgres` skips both PostgreSQL passes and the gate prints "this is not a full gate"
        -- to the console, which is read once by whoever started it. The manifest is read by
        everything afterwards, and it said `GREEN` with nothing distinguishing a partial run from a
        complete one. So a persistence-affecting change could be certified by a run the repository
        itself says did not cover persistence, and `ci/merge-proof.ps1` (retired 2026-09-24) could not
        refuse it, because a verifier can only check fields the producer wrote.

        TWO FIELDS, and the second is not redundant. `postgres` names WHICH coverage was skipped, so
        a future consumer can decide per subject; `complete` answers the question every consumer has
        without teaching it the flag list -- and it is the field that keeps meaning when a second
        skip switch is added, which a consumer matching on `postgres` alone would silently miss.

        #904 ADDS A THIRD KEY, `build`, AND DELIBERATELY DOES NOT TOUCH `complete`. A warm run ran
        every stage: it IS complete, and flipping that would have made `ci/merge-proof.ps1` refuse
        every warm run outright -- the whole saving, refused by the verifier. What a reader wants to know
        is a different question ("did this run compile, or reuse?"), so it gets a different field.

        `-BuildMode` IS NOT MANDATORY, and that is measured rather than stylistic. Under the gate's
        own invocation (`powershell -NoProfile -ExecutionPolicy Bypass -File`, no `-NonInteractive`)
        a missing Mandatory parameter PROMPTS instead of failing, so a caller that forgets it would
        HANG the gate; and `ci/gate-manifest-provenance.tests.ps1` calls this function with
        `-SkipPostgres` alone. Its default is `unknown`, never `warm`: ABSENT IS NOT A MEASUREMENT,
        and the one value that must not be guessable is the flattering one.

        A SEAM THAT TAKES A BOOL AND RETURNS A RECORD, for the reason the other rules in this file
        are: `Write-RunManifest` drives the whole gate and cannot be run from a cell, so the rule
        lives where a cell can dot-source it out of the subject by AST.
    #>
    param(
        [Parameter(Mandatory)] [bool] $SkipPostgres,
        [string] $BuildMode = 'unknown',
            [bool] $NonPullRequest = $false,
        # #901 slice 2: two more skips, and `complete` takes both. Defaults are the ran-everything
        # answer so the callers that pass `-SkipPostgres` alone keep their record unchanged.
        [bool] $SkipRust = $false,
        [bool] $SkipPsSuites = $false
    )

    # IN THE BODY, NOT AS A [ValidateSet] ATTRIBUTE. `ci/classify-run.tests.ps1` derives the run
    # STATUS vocabulary from the single ValidateSet this file carries, and refuses to read a second
    # one -- with two, the one it reads is a sample rather than the vocabulary. A declarative guard
    # here would have silently turned that census into a coin toss. The body check costs four lines,
    # names the value it refused, and is ordinal for the reason every other comparison here is.
    $recognised = $false
    foreach ($candidate in @('cold', 'warm', 'unknown')) {
        if ([string]::Equals($BuildMode, $candidate, [System.StringComparison]::Ordinal)) { $recognised = $true }
    }
    if (-not $recognised) {
        throw "Get-RunCoverage was given BuildMode '$BuildMode', which is not one of cold, warm, unknown. A coverage record may not carry a word no consumer can read, and guessing which of the three was meant would put the flattering one in a durable record."
    }

    return [ordered]@{
        postgres = if ($SkipPostgres) { 'skipped' } else { 'included' }
        landing  = if ($NonPullRequest) { 'non-pr' } else { 'pr-base' }
        complete = (-not $SkipPostgres) -and (-not $SkipRust) -and (-not $SkipPsSuites)
        build    = $BuildMode
        rust     = if ($SkipRust) { 'skipped' } else { 'included' }
        psSuites = if ($SkipPsSuites) { 'narrowed' } else { 'included' }
    }
}

function Get-WorktreeDirt {
    <#
        Whether the tree this run measured is the commit it names, over EVERYTHING the build can
        see -- not only over tracked modifications (#725, case 2).

        `dirtyDiffHash` was computed from `git diff HEAD`, which omits untracked paths. A run with
        an automatically discovered `build.rs` sitting untracked therefore recorded a NULL hash --
        the value that reads as "the worktree was exactly the commit" -- while cargo compiled a tree
        no commit contains. `ci/merge-proof.ps1` (retired 2026-09-24) refused a non-null hash and
        refused the field being absent, and neither refusal helped: the value was present and honest
        about the wrong question.

        THE TWO EXCLUSIONS ARE WHAT KEEP THIS FROM FREEZING THE BOARD, and they are the publisher's
        own, deliberately: the manifest store gains a file per run BY DESIGN (#674(a)) and
        `Write-CanaryNonce` rewrites the nonce before every run BY DESIGN (#152), so counting either
        as dirt would mark every ordinary run dirty and refuse every merge. A guard that fires on
        its own artefacts protects nothing; it just never lets the thing it guards happen.

        THE PATHS TRAVEL WITH THE HASH. A hash says "this tree is not that commit" and a reader
        then has to go find out what made it so -- at the moment they are reading a refusal, on a
        machine that has moved on. Naming them costs one field.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $Diff,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $PorcelainLines
    )

    $storePrefix = '.factory/gate-runs/'
    $gateOwnPath = 'tools/ci-canary/src/nonce.rs'
    $foreign = New-Object System.Collections.Generic.List[string]
    foreach ($line in @($PorcelainLines)) {
        if ([string]::IsNullOrWhiteSpace($line) -or $line.Length -lt 4) { continue }
        # A porcelain line is `XY <path>`; a rename is `XY <old> -> <new>`. Both ends count, because
        # a rename INTO the store from outside is still a path the commit does not carry.
        $paths = ($line.Substring(3) -split ' -> ') | ForEach-Object { $_.Trim('"') }
        foreach ($candidate in $paths) {
            $normalised = $candidate -replace '\\', '/'
            if ([string]::Equals($normalised, $gateOwnPath, [System.StringComparison]::Ordinal)) { continue }
            if ($normalised.StartsWith($storePrefix, [System.StringComparison]::Ordinal)) { continue }
            if (-not $foreign.Contains($normalised)) { $foreign.Add($normalised) }
        }
    }
    $untracked = @($foreign | Sort-Object)

    # THE PORCELAIN DECIDES, AND THE DIFF IS ONLY THE MATERIAL. `git status --porcelain` lists
    # tracked modifications (` M path`) as well as untracked ones (`?? path`), so the foreign set
    # above already answers "is this tree the commit it names" for BOTH halves. The diff text is
    # what the hash is computed over; it is not what decides.
    #
    # WHY THAT ORDER MATTERS, MEASURED: `git diff HEAD` is non-empty on EVERY ordinary gate run,
    # because `Write-CanaryNonce` rewrites the tracked `tools/ci-canary/src/nonce.rs` before the run
    # starts (#152, by design). Deciding from the diff therefore recorded a non-null hash every
    # time -- 29 of 29 manifests in this worktree -- and `ci/merge-proof.ps1` (retired 2026-09-24)
    # refused a non-null `dirtyDiffHash` with "the gate ran with uncommitted edits". So the merge proof could not be
    # satisfied by ANY run the gate has ever produced: the producer's own artefact permanently
    # failed the consumer's check.
    #
    # That is the same defect the publisher's purity check already names one function up -- "a guard
    # that refuses on its own artefact does not protect anything; it just never lets the thing it
    # guards happen" -- and it was living one field over, in the value rather than in the refusal.
    if ($untracked.Count -eq 0) {
        return [ordered]@{ hash = $null; untracked = @() }
    }
    $material = ($Diff + "`n" + ($untracked -join "`n"))
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($material)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = [System.BitConverter]::ToString($sha256.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant()
    } finally { $sha256.Dispose() }
    return [ordered]@{ hash = $hash; untracked = $untracked }
}

function Write-RunManifest {
    # $Status: one of GREEN, RED, ABORTED-BY-CANARY - the house's PASS/FAIL/HARNESS-BROKE
    # discipline, plus the fourth value a contaminated build environment needs (orchestrator
    # ruling on #152): a reader must be able to tell "the gate ran and something failed" apart
    # from "the gate refused to trust its own environment and stopped before measuring anything" -
    # collapsing both into a bare RED would read as the same finding when they are not.
    param(
        [ValidateSet('GREEN', 'RED', 'ABORTED-BY-CANARY', 'HARNESS-BROKE')]
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
    # #993 (Codex, "preserve supplied PR provenance in offline manifests"): when the gate host has
    # no `gh` and no server, Get-HeadProvenance answers null for both load-bearing top-level fields,
    # and `merge-proof`'s `Test-ManifestVouches` refused such a manifest before the supplied landing
    # snapshot -- which DOES identify the PR and this head, captured online -- could say anything.
    # So a supplied-pr snapshot fills exactly those two fields, only when the lookup found NOTHING (a
    # measured value, including a measured `pushed: false`, is never overwritten), with reasons
    # naming the source. `merge-proof` (retired 2026-09-24) re-verified the snapshot against the
    # LIVE PR at press time (Test-ManifestLandingSnapshot: number, base ref, base sha, merge-base);
    # nothing does now, so the two fields are the snapshot's CLAIM and the reason strings say so.
    if ($null -ne $mergeTargetSnapshot -and [string]::Equals([string]$mergeTargetSnapshot.mode, 'supplied-pr', [System.StringComparison]::Ordinal) -and
        [string]::Equals([string]$mergeTargetSnapshot.head, [string]$headSha, [System.StringComparison]::Ordinal)) {
        if ($null -eq $provenance.pullRequest -and $mergeTargetSnapshot.pullRequest -is [int]) {
            $provenance.pullRequest = [int]$mergeTargetSnapshot.pullRequest
            $provenance.pullRequestReason = "gh could not look ($($provenance.pullRequestReason)); taken from the supplied landing snapshot -- a CLAIM of the snapshot, not re-verified against the live PR"
        }
        if ($null -eq $provenance.pushed) {
            $provenance.pushed = $true
            $provenance.pushedReason = "no server to ask ($($provenance.pushedReason)); the supplied landing snapshot named this commit as the PR's head when captured online -- a CLAIM of the snapshot, not re-verified against the live PR"
        }
    }
    # BOTH READS, because a tree is the commit it names only when nothing is modified AND nothing
    # untracked is sitting in it (#725). `git diff HEAD` answers the first; only `git status
    # --porcelain --untracked-files=all` answers the second, and the publisher below has been using
    # exactly that read for its own purity check all along -- the manifest was the half that could
    # not see it.
    $diff = @(& git diff HEAD 2>$null | ForEach-Object { [string]$_ }) -join "`n"
    $porcelain = @(& git status --porcelain --untracked-files=all 2>$null | ForEach-Object { [string]$_ })
    $dirt = Get-WorktreeDirt -Diff $diff -PorcelainLines $porcelain
    $dirtyDiffHash = $dirt.hash
    # #904: READ OFF THE ARTEFACT MANIFEST, never recomputed here -- the record and the verdict
    # then have no way to disagree. INDEXED rather than dotted, and that is measured: two of this
    # function's three call sites hand it `$emptyArtifacts`, which carries `buildExitCode` and
    # `artifacts` and none of the #904 fields, and under `Set-StrictMode` a missing key read with
    # `.` THROWS -- inside the writer, after every stage has run, which is how a whole run's
    # evidence gets lost between the last stage and the record (see the `matrixReason` note near
    # the top of this file for the time that actually happened).
    $buildMode = 'unknown'
    if ($ArtifactManifest -is [System.Collections.IDictionary]) {
        $recordedMode = [string]$ArtifactManifest['buildMode']
        foreach ($candidate in @('cold', 'warm', 'unknown')) {
            if ([string]::Equals($recordedMode, $candidate, [System.StringComparison]::Ordinal)) { $buildMode = $candidate }
        }
    }
    # #901 slice 2: the two newer skips are computed ABOVE the call, which stays on ONE line because
    # ci/gate-postgres-early.tests.ps1 lifts that line by regex and executes it. Measured: a
    # backtick-continued call broke that suite into HARNESS-BROKE ("Incomplete string token").
    $coverageSkipsRust = [bool]((Test-Path 'variable:script:rustStagesIncluded') -and -not $script:rustStagesIncluded)
    $coverageSkipsPsSuites = [bool]((Test-Path 'variable:script:psSuitesScope') -and $script:psSuitesScope -and -not $script:psSuitesScope.included)
    $coverage = Get-RunCoverage -SkipPostgres ([bool]($script:matrixSkipped -or $script:postgresMatrixUnavailable)) -BuildMode $buildMode -NonPullRequest ([bool]($NonPullRequest -or -not $LandingSnapshotPath)) -SkipRust ([bool]$coverageSkipsRust) -SkipPsSuites ([bool]$coverageSkipsPsSuites)
    # #205: the sweep's own reading, out of the full stage capture (#751) so the background-joined
    # suites stage is read like any other.
    $eventContractSweep = Get-EventContractSweepRecord -Lines $script:allStageLines

    $staleArtifacts = @($ArtifactManifest.artifacts | Where-Object { $_.freshBuild -eq $false })
    # #904: THE POPULATION THAT DECIDES, beside the one that used to. `$staleArtifacts` is every
    # artefact a cold-only gate would have refused -- kept, unchanged, and still published, because
    # it is the record that lets this change be audited: a reader can see how many reuses were
    # PROVEN rather than having to trust that the number went down for a good reason.
    # `$unprovenReuse` is what actually reddens the gate. See Select-UnprovenReuse for why the
    # filter lives in one place and not in the four sites that need it.
    $unprovenReuse = Select-UnprovenReuse -Artifacts $ArtifactManifest.artifacts

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
# #455: computed HERE and not beside `instrumentSuspect` below, because `$passedEverything`
# uses it on the very next line -- the first draft assigned it 27 lines later, which under
# Set-StrictMode is a throw and without it a silent $null that reads as 'not suspect'.
$targetSuspect = $false
if ($null -ne $script:targetBuildState) { $targetSuspect = [bool] $script:targetBuildState.suspect }
# THE ARTEFACT BUILD PASS IS A TERM, because everything downstream is computed over the list it
# produced. `artifactBuildExit` had five sites in this file and not one decided anything, so a pass
# that died mid-enumeration published GREEN over a PARTIAL population: measured on this branch, a
# cold run exited 101 after 22 of 234 artefacts, went green, and wrote an artefact ledger covering
# the 22 -- which then made the next run refuse 212 it had no custody record for. The stages passed
# because each compiles what it needs; the INSTRUMENT that counts them did not.
#
# A WRONG TYPE MUST WIDEN, the same rule the scope selector states: `$null` is what this reads when
# the manifest could not be built at all (`$emptyArtifacts` carries `buildExitCode = $null`), and a
# run that cannot say whether its build pass succeeded has not established that it did.
$buildPassExit = $ArtifactManifest.buildExitCode
$buildPassSucceeded = ($buildPassExit -is [int]) -and ($buildPassExit -eq 0)
# #901 slice 2: A RUN THAT RAN NO RUST STAGE HAS NO BUILD PASS, NO CANARY AND NO TARGET TO JUDGE.
# The three terms are released here, for the verdict only; the RECORD is not faked -- `buildExitCode`
# stays `$null`, `canaryPassed` is written `$null` below, `targetBuildState` is published as read,
# and `coverage.rust` says `skipped`. They are the instruments of the Rust stages, and none ran. Only `Read-ScopeSelection`'s proven "no Rust build input
# changed" reaches this; every other state leaves both terms exactly as they were.
$rustScopeSkipped = (Test-Path 'variable:script:rustStagesIncluded') -and -not $script:rustStagesIncluded
# ONE LINE, NO COLUMN-0 BRACE: several suites cut this function out at its first "`n}", and a
# block closing at column 0 here ended their slice mid-function (measured: gate-postgres-early).
if ($rustScopeSkipped) { $buildPassSucceeded = $true; $CanaryPassed = $true; $targetSuspect = $false }
$passedEverything = ($script:failed.Count -eq 0) -and $CanaryPassed -and ($unprovenReuse.Count -eq 0) -and (-not $targetSuspect) -and $buildPassSucceeded
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
# #455: a target whose previous build DEMONSTRABLY broke -- interrupted, or being written by another
# live gate -- is an instrument this run cannot vouch for, read BEFORE the first compile rather than
# inferred from artefact mtimes at the end. It joins this disjunction instead of getting a refusal of
# its own, so it forbids a GREEN without inventing a second way for a run to die.
#
# `unknown` is deliberately NOT here; see the reading site for the measurement. It is recorded in the
# manifest as a fact about the instrument, and the unproven-reuse term above still catches an
# unknown target whose binaries this run cannot vouch for.
#
# #904: the term is `$unprovenReuse`, not `$staleArtifacts`. A binary that predates this run's start
# and whose crate inputs are PROVEN unchanged against the ledger is not a broken instrument -- it is
# the same program, established by content rather than by a timestamp.
$instrumentSuspect = ($unprovenReuse.Count -gt 0) -or (-not $CanaryPassed) -or $targetSuspect -or (-not $buildPassSucceeded)

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
        mergeTarget        = $mergeTargetSnapshot
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
        # #725: WHAT MADE IT DIRTY, beside the hash that says it is. A hash sends a reader looking;
        # the paths tell them, at the moment they are reading a refusal on a machine that has moved
        # on. Empty on a clean tree, and never contains the gate's own store or nonce.
        untrackedPaths     = @($dirt.untracked)
        # #725: WHAT THIS RUN COVERED. `status` says the stages passed; this says which of them ran
        # at all. Without it a `-SkipPostgres` run -- every run on a machine without PostgreSQL --
        # is indistinguishable from a full gate to any reader of the manifest, which can only check
        # fields the producer wrote.
        coverage           = $coverage
        # #207 follow-up: which gated targets this run's SCOPE excluded from the required-features
        # check, so a reader can tell "SCOPED, and nothing gated was excluded" from "SCOPED, and the
        # postgres targets were" without re-deriving the population itself. Empty on a FULL run,
        # always -- every gated target is in scope by definition -- and empty on a scoped run that
        # happens to select the gating crate too.
        #
        # NULL, NOT `@()`, WHEN THE STAGE'S OWN SCOPE REPORT COULD NOT BE READ (#1019 review, B): an
        # empty array here is the CLAIM "this run excluded nothing", and a report that could not be
        # parsed has not earned that claim -- it could just as well have excluded everything. The
        # first version wrote `@()` in both cases, which is the exact "unreadable is not empty"
        # failure this whole field exists to name in #207's own subject, arriving through the record
        # meant to prevent it. `status`/`coverage` remain the fields that decide PASS/FAIL either
        # way: this one is a record, not a verdict (instrument-fact rule, #989/#1009).
        #
        # THE LEADING COMMA IS LOAD-BEARING, found writing this cell's own test. An `if`/`else`
        # EXPRESSION assigned to a hashtable value collapses a branch whose last statement outputs
        # ZERO items to `$null` -- so `else { @($script:requiredFeaturesExcluded) }` alone would
        # write `$null` for "read, and genuinely nothing excluded" too, indistinguishable from the
        # unreadable case this field exists to tell apart from it. `, @(...)` is the standard
        # PowerShell idiom that stops the collapse without nesting a non-empty array inside another.
        requiredFeaturesExcluded = if ($script:requiredFeaturesReportUnreadable) { $null } else { , @($script:requiredFeaturesExcluded) }
        # #205: `measured` | `notMeasured` (+ reason) | `absent`. A mute remote is written HERE, as a
        # fact about the instrument, and never into `status`.
        eventContractSweep = $eventContractSweep
        # A FACT, not an inference: the head moved between the start of the gate and this write.
        # The record names the head the STAGES ran against, and this says the working tree is no
        # longer on it -- which is why the publication below refuses to commit.
        headMovedDuringRun = $headMoved
        cargoTargetDir     = $actualTargetDir
        # #956: the CLAIM instant. `queuedUtc` is when the gate began waiting; `slotWaitSecs` is the
        # difference, derived once by Get-SlotWaitSecs so no reader has to reconstruct it from the
        # ledger. A manifest written before this change has neither and its runStartUtc may include
        # a wait -- absent means not measured, never "waited 0". Negative skew is published as absent.
        queuedUtc          = $runQueuedUtc.ToString('o')
        runStartUtc        = $runStartUtc.ToString('o')
        slotWaitSecs       = Get-SlotWaitSecs -QueuedUtc $runQueuedUtc -StartUtc $runStartUtc
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
        canaryPassed       = if ($rustScopeSkipped) { $null } else { $CanaryPassed }
        # #901 slice 2: which reach decisions this run took, in words. `coverage` carries the
        # machine-readable half; these carry the reasons a presser reads.
        rustStages         = [ordered]@{
            included = -not $rustScopeSkipped
            reason   = if ($rustScopeSkipped) { 'skipped: no Rust build input changed' } else { 'ran' }
        }
        psSuitesScope      = if ((Test-Path 'variable:script:psSuitesScope') -and $script:psSuitesScope) { $script:psSuitesScope } else { $null }
        # #956: the two states above, published. ABSENT IS NOT FALSE: every manifest written before
        # this change has neither field, which means NOT MEASURED and never "it ran serial".
        psSuitesStartedEarly = $script:psSuitesStartedEarly
        psSuitesOverlapped   = $script:psSuitesOverlapped
        # #464: whether this run's scope touched `apps/studio` at all, whether Node/npm were found
        # (only meaningful when the first is true -- a Rust-only run never checked), and the same
        # started/overlapped pair the PowerShell suites carry. NEVER a warning read as green: a
        # scope-included, Node-absent run has no `apps/studio (npm)` entry in `stages[]` at all --
        # these four fields are the only record of what happened.
        studioScopeIncluded = [bool]$script:studioScopeIncluded
        studioNodePresent   = [bool]$script:studioNodePresent
        studioStartedEarly  = [bool]$script:studioStartedEarly
        studioOverlapped    = [bool]$script:studioOverlapped
        pgMatricesStartedEarly = $script:pgMatricesStartedEarly
        pgMatricesOverlapped   = $script:pgMatricesOverlapped
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
        # #904: UNCHANGED, deliberately. This is now the record of what a cold-only gate would have
        # refused, which is how this change gets audited -- a reader comparing it against
        # `artifactsProvenReuse` below can see exactly how many reuses the content check rescued.
        staleArtifactCount = $staleArtifacts.Count
        # #904: the four states, the build's own mode, and the two workspace-wide inputs the hash
        # is namespaced by. Read off the artefact manifest rather than recomputed, so the record and
        # the verdict cannot disagree.
        artifactsRebuilt       = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'artifactsRebuilt'
        artifactsProvenReuse   = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'artifactsProvenReuse'
        artifactsUnprovenReuse = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'artifactsUnprovenReuse'
        artifactsContaminated  = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'artifactsContaminated'
        # #1007: how many binaries this run enrolled (deleted and rebuilt inside its window because
        # the ledger had no row for them). Summed across the workspace pass and every per-suite
        # pass; it was summed and never published until lane B read the manifest for it.
        artifactsEnrolled      = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'artifactsEnrolled'
        buildMode              = $buildMode
        buildPassSecs          = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'buildPassSecs'
        # What the proof COST, beside what it bought. A mechanism whose overhead nobody records is
        # one nobody can decide to retire.
        crateInputHashSecs     = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'crateInputHashSecs'
        # #1038 review (lane S): the crates whose compile-time inputs could not be read whole, named
        # in the record rather than only refused in the run. Every artefact of such a crate is
        # `unproven-reuse`, and without this field the only evidence would be a count.
        embeddedInputProblems  = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'embeddedInputProblems'
        toolchainId            = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'toolchainId'
        lockfileSha256         = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'lockfileSha256'
        # ABSENT IS NOT FALSE, and it is not 'no error' either: a run whose hashes could not be
        # computed says so here, so a reader is never left inferring a cold compile's cause.
        crateInputHashError    = Read-ArtifactManifestField -Manifest $ArtifactManifest -Name 'crateInputHashError'
        # #909: per-stage executed-test counts. A count survives the 80-line tail that the stage's
        # transcript does not, so this is the only place a later reader can learn whether the
        # PostgreSQL axis was measured at all. Empty when no PostgreSQL stage ran.
        postgresExecution  = $script:postgresExecution
        staleArtifacts     = $staleArtifacts
        slotLockAtStart    = $SlotLockAtStart
        slotLockAtEnd      = $SlotLockAtEnd
        # Gate 9 (#200, L): the pair above was already captured specifically so it COULD be
        # compared, and nothing did the comparing - a reader had to notice on their own that two
        # fields existed to diff. A match is a tripwire worth surfacing, not a determination: an
        # untouched, stale lock reads identical by construction, but so would a real hold nobody
        # touched for the whole run. See Test-SlotLockSnapshotsIdentical in ci/slot-lock.ps1.
        slotLockStartEndIdentical = Test-SlotLockSnapshotsIdentical -Start $SlotLockAtStart -End $SlotLockAtEnd
        # #903: WHAT THIS RUN COVERED. A GREEN over two crates and a GREEN over the workspace are
        # the same word; this is the field that tells them apart.
        scope              = Get-ScopeRecord -Selection $script:gateScope
        instrumentSuspect  = $instrumentSuspect
        # #455: what the target looked like BEFORE this run compiled anything. Recorded whole --
        # state and reason, not a boolean -- because "interrupted" and "concurrent" want different
        # remedies and a presser reading the manifest should not have to guess which happened.
        targetBuildState   = $script:targetBuildState
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
        # ONE WRITER FOR THE PAIR (#742). This block used to be a hand-copied version of
        # Write-GateManifestPair's discipline. The copy existed for a real reason -- that helper
        # reserves a NEW name on every call, so routing a correction through it would write a SECOND
        # pair and leave the stale GREEN one exactly where readers look -- but two implementations of
        # one invariant is the setup for them to drift, and they had.
        #
        # The split that removes the copy is between the NAME and the WRITE:
        # `Write-ManifestPairContent` takes the final PATHS, which this path already holds in
        # `$written`, so reserving a name stays the creation path's concern and the half-pair
        # invariant lives in exactly one function.
        #
        # WHAT THAT BUYS HERE IS A ROLLBACK THIS PATH NEVER HAD. The hand-copied version passed a
        # real null for `File.Replace`'s backup argument, so the original bytes were gone the instant
        # the first replace landed: a failure on the second copy could only be REPORTED -- "an
        # earlier copy WAS corrected, so the two stores now disagree" -- and the pair was left
        # disagreeing. The shared writer keeps a backup and restores from it, so a correction is now
        # both stores or neither, the same as a creation has been since #674.
        $correctionFailure = $null
        try {
            Write-ManifestPairContent -FinalPaths @($written) -Json $correctedJson -Commit 'Replace' | Out-Null
        } catch {
            $correctionFailure = "the corrected record could not be written: $($_.Exception.Message)"
        }
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
    $script:slotRunEnded = $true

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
# THE STARTUP SNAPSHOT IS WHERE THIS COSTS THE MOST (#762): all three reads below run on every
# single invocation, so under PowerShell 7 the gate would refuse to start on a healthy checkout.
# Capture, read the exit code, and only then reduce -- nothing that can STOP a pipeline may sit
# between a native command and the read of its exit code.
$gatedBranchOutput = @(& git symbolic-ref --quiet HEAD 2>$null)
$gatedBranchExit = $LASTEXITCODE
$gatedBranchAtStart = $gatedBranchOutput | Select-Object -First 1
$gatedBranchAtStart = if ($gatedBranchExit -eq 0 -and $gatedBranchAtStart) { ([string]$gatedBranchAtStart).Trim() } else { $null }
$gatedHeadOutput = @(& git rev-parse HEAD 2>$null)
$gatedHeadExit = $LASTEXITCODE
$gatedHeadAtStart = $gatedHeadOutput | Select-Object -First 1
$gatedHeadAtStart = if ($gatedHeadExit -eq 0 -and $gatedHeadAtStart) { ([string]$gatedHeadAtStart).Trim() } else { $null }
# #883: A DETACHED HEAD IS REFUSED HERE, not discovered 40 minutes later at publish. The
# capture above already answers the question -- $gatedBranchAtStart is $null exactly when
# there is no branch -- so this reads nothing new; it only acts on what was just read. Before
# this, a detached run passed every one of the 56 stages, reached Write-RunManifest, and only
# THEN printed a WARNING that the manifest could not be committed (H, measured: run 1
# `4107363f`, 40 minutes from launch to that discovery). The fix moves the same fact to the
# only place it can still save the 40 minutes: before the first stage runs.
# -AllowDetachedHead is the escape hatch the acceptance criterion asks for -- a detached
# checkout is still useful to MEASURE without publishing -- and it must be explicit and
# overridable, not silent: passing it prints the same fact as a warning and continues: nothing
# below this block changes, and Write-RunManifest's own detached-HEAD warning (unchanged)
# still fires at publish time either way.
if (-not $gatedBranchAtStart) {
    if (-not $AllowDetachedHead) {
        Write-Host '[gate] REFUSED: HEAD is detached.' -ForegroundColor Red
        Write-Host ('[gate] There is no branch to publish the run manifest onto, so every stage would run and ' +
            'the manifest would still not be committed at the end (#883).') -ForegroundColor Red
        Write-Host '[gate] Check out a branch and re-run, or pass -AllowDetachedHead to measure without publishing.' -ForegroundColor Red
        exit 1
    }
    Write-Host ('[gate] HEAD is detached and -AllowDetachedHead was passed: this run will measure but the run ' +
        'manifest will not be committed (#883).') -ForegroundColor Yellow
}

# #993: THE LANDING SNAPSHOT IS TAKEN AFTER THE DETACHED-HEAD PRECONDITION, not before it. The
# first placement sat between the head capture and that refusal, so the gate's precondition slice
# (ci/gate-detached-head.tests.ps1) reached `$MergeTargetRef` with no parameters bound and refused
# every checkout, and a real run asked the server before it had checked it had a branch to publish
# onto. The snapshot needs `$gatedHeadAtStart`, which the capture above already produced.
try {
    $mergeTargetSnapshot = Get-LandingSnapshot -Root $repositoryRoot -Head $gatedHeadAtStart -ExpectedRef $MergeTargetRef -LocalOnly:$NonPullRequest -SnapshotPath $LandingSnapshotPath
} catch {
    Write-Host ('[gate] REFUSED: landing snapshot unavailable: ' + $_.Exception.Message) -ForegroundColor Red
    exit 1
}
$mergeTargetRefResolved = $mergeTargetSnapshot.sha
$env:GRAPHHELM_MERGE_TARGET_REF = $mergeTargetSnapshot.sha

if ($gatedBranchAtStart -and $gatedHeadAtStart) {
    $branchValueOutput = @(& git rev-parse --verify --quiet $gatedBranchAtStart 2>$null)
    $branchValueExit = $LASTEXITCODE
    $branchValueAtStart = $branchValueOutput | Select-Object -First 1
    if ($branchValueExit -ne 0 -or
        -not [string]::Equals(([string]$branchValueAtStart).Trim(), $gatedHeadAtStart, [System.StringComparison]::Ordinal)) {
        Write-Host ("[gate] the branch and the head read at startup do not describe one state: $gatedBranchAtStart " +
            "holds $(([string]$branchValueAtStart).Trim()) while HEAD read $gatedHeadAtStart. Something moved between " +
            'the two reads, so this run has no coherent subject to gate. Re-run on a stable checkout.') -ForegroundColor Red
        exit 1
    }
}

# #700: CLAIM THE MACHINE-WIDE SLOT before the first ledger line. Seven gates ran at once on this
# machine on 2026-09-05 because the slot's reader had no consumer and its producer had no caller
# (ci/slot-lock.ps1, #700). This is both halves connected: the gate is the durable process the pair
# names -- its own pid and start time, at the precision Test-SlotHolderLiveness parses -- and it
# waits on a live or unreadable holder, inherits a wrapper's claim when the exported pair matches
# (the merge checklist, retired 2026-09-24, had the wrapper claim, then launch), and reclaims only a
# pair the OS says is dead.
# The lock path is Get-SlotLockPath over Get-SlotDir, and GRAPHHELM_SLOT_LOCK_PATH is defaulted to it
# so Read-SlotLockSnapshot below reads the same file instead of answering 'not set' every run.
$slotLockPath = Get-SlotLockPath -SlotDir (Get-SlotDir)
if (-not $env:GRAPHHELM_SLOT_LOCK_PATH) { $env:GRAPHHELM_SLOT_LOCK_PATH = $slotLockPath }
$gateStartUtc = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
$slotWaitMinutes = 180
if ($env:GRAPHHELM_SLOT_WAIT_MINUTES -and [int]::TryParse($env:GRAPHHELM_SLOT_WAIT_MINUTES, [ref] $slotWaitMinutes)) { } else { $slotWaitMinutes = 180 }
# #956: THE WAIT STARTS HERE, as the last statement before the slot is asked for, so a free slot
# reads 0 and a held one reads only the wait. The placement cell asserts adjacency, not precedence.
$runQueuedUtc = [DateTime]::UtcNow
$script:slotOutcome = Enter-GateSlot -Path $slotLockPath -HolderPid $PID -HolderStartUtc $gateStartUtc `
    -Detail "gate cwd=$repositoryRoot head=$gatedHeadAtStart" -BudgetSeconds ($slotWaitMinutes * 60) -PollSeconds 30 `
    -WriteEvent { param($Event, $Detail) Write-SlotEvent -Event $Event -Detail $Detail }
if ($script:slotOutcome -notin @('claimed', 'inherited')) {
    Write-Host "[gate] NO SLOT ($($script:slotOutcome)): $slotLockPath" -ForegroundColor Magenta
    if ($script:slotOutcome -eq 'expired') {
        Write-Host "[gate] another gate held the machine-wide slot for the whole $slotWaitMinutes-minute budget (GRAPHHELM_SLOT_WAIT_MINUTES)." -ForegroundColor Magenta
        Write-Host '[gate] Nothing here says anything about this head. Re-run when the ledger shows its RUN-END, or raise the budget.' -ForegroundColor Magenta
    } else {
        Write-Host '[gate] the lock could not be created and no lock file exists: a path or permission fault, not contention. Nobody holds the slot.' -ForegroundColor Magenta
    }
    exit 1
}
# #956: THE RUN STARTS HERE, with the slot held. Everything above this line is waiting; everything
# below is the machine working on this head. Stamped after Enter-GateSlot answered claimed/inherited
# so `runEndUtc - runStartUtc` is a machine number, and the ledger's RUN-START agrees to the second.
$runStartUtc = [DateTime]::UtcNow

# #455: THE EARLY READ, before the first compile and therefore before the 40 minutes a contaminated
# run would otherwise spend proving itself suspect at the end. It asks whether the previous build in
# this target FINISHED -- see Get-TargetBuildState for why age and the canary nonce both condemn
# every reuse instead, and for the measurement.
#
# IT DOES NOT ABORT, and `unknown` DOES NOT REDDEN. Both are deliberate, and the second was a
# correction: the first draft fed every non-clean state into `instrumentSuspect`, and `unknown` is
# the state every target in the fleet is in until a run writes its first marker. Measured on this
# machine, 178 of 178 cargo-shaped target directories on D: and E: hold build output and no marker,
# so that draft cost one non-GREEN hand-launched run per directory on first reuse -- for a guard
# whose entire argument is that a check firing on clean runs is one the fleet learns to skip.
#
# It also bought no detection: an `unknown` target that really is stale still fails the late
# `staleArtifactCount` check, which is how #455's own contamination was caught in the first place.
# So `unknown` is a fact about the INSTRUMENT and goes in the manifest, while the verdict stays with
# the tree -- the same division #989 and #1005 settled on. Only a build that demonstrably broke --
# `building` with a dead owner, or `building` with a live one -- makes the run's instrument suspect,
# and both states can only be observed for builds started AFTER this change. The guard therefore
# tightens by itself as markers appear, with no migration and no flag day.
$script:targetBuildState = Get-TargetBuildState -TargetDir $actualTargetDir
if ($script:targetBuildState.suspect) {
    Write-Host "[gate] TARGET PROVENANCE: $($script:targetBuildState.state) -- $($script:targetBuildState.reason)" -ForegroundColor Red
    if ($script:rustStagesIncluded) {
        Write-Host '[gate] Read before the first compile. The run continues and cannot report GREEN.' -ForegroundColor Red
    } else {
        Write-Host '[gate] Recorded only: no Rust stage runs in this scope, so this run never opens the target.' -ForegroundColor Yellow
    }
} elseif ($script:targetBuildState.contaminated) {
    Write-Host "[gate] NOTE: target provenance is $($script:targetBuildState.state) -- $($script:targetBuildState.reason)" -ForegroundColor Yellow
    Write-Host '[gate] This is recorded in the manifest and does NOT redden the run; artefact freshness is still checked at the end.' -ForegroundColor Yellow
} else {
    Write-Host "[gate] target provenance: $($script:targetBuildState.state) -- $($script:targetBuildState.reason)"
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
    # #455: A SUSPECT TARGET ENDS THE RUN HERE, before the first compile -- which is the word the
    # issue asked for and the first draft did not deliver. It continued into every cargo stage and
    # recorded the verdict at the end, which is the expensive half of exactly the defect #455 is
    # about (Codex P2 on #1009).
    #
    # The rollout argument that keeps `unknown` from aborting does NOT reach here: `unknown` is the
    # state every target is in until a run writes its first marker, but `interrupted` and
    # `concurrent` require a marker a POST-CHANGE run wrote, so no legacy target can produce them
    # and there is no flag day to fear.
    #
    # It aborts the way the canary does -- manifest, then exit 1 -- rather than through a new path:
    # both are "this run cannot vouch for what it would measure", and the status tells them apart.
    # #901 slice 2: a run with no Rust stage never opens the target, so a suspect one is recorded
    # (the manifest carries `targetBuildState`) and neither aborts this run nor is touched by it.
    if ($script:targetBuildState.suspect -and $script:rustStagesIncluded) {
        Write-Host ''
        Write-Host "[gate] ABORTING: $($script:targetBuildState.reason)" -ForegroundColor Magenta
        Write-Host '[gate] Every stage below would run against a target this run cannot trust, and' -ForegroundColor Magenta
        Write-Host '[gate] nothing past this point would be evidence of anything. See #455.' -ForegroundColor Magenta
        Write-Host "[gate] To proceed: use a FRESH target directory (the runner allocates one per run), or" -ForegroundColor Magenta
        Write-Host "[gate] clean this one and delete its marker: $actualTargetDir\.graphhelm-build-state.json" -ForegroundColor Magenta
        Write-Host '[gate] This mark is deliberately sticky -- the target stays refused until someone acts.' -ForegroundColor Magenta
        $emptyArtifacts = [ordered]@{ buildExitCode = $null; artifacts = @() }
        $manifestPath = Write-RunManifest -Status 'HARNESS-BROKE' -CanaryPassed $false `
            -ArtifactManifest $emptyArtifacts -SlotLockAtStart $slotLockAtStart `
            -SlotLockAtEnd (Read-SlotLockSnapshot)
        Write-Host "[gate] manifest: $manifestPath" -ForegroundColor Cyan
        exit 1
    }
    # Claim the directory for this run -- AFTER the abort, never before it. Writing this first would
    # overwrite the very marker just read, destroying the evidence of the previous run's death.
    # A run killed between here and the completion marker leaves `building` with this process id,
    # which is exactly the `interrupted` state the next run reads.
    # #901 slice 2: no Rust stage, no claim -- the sticky marks are evidence and this run built nothing.
    if ($script:rustStagesIncluded) { Write-TargetBuildState -TargetDir $actualTargetDir -State 'building' -Head ([string]$gatedHeadAtStart) }

    # #152: the canary runs FIRST and is the one stage that aborts the whole gate immediately
    # rather than accumulating alongside the rest - see the top-of-file rationale.
    Write-CanaryNonce
    $canaryCode = Invoke-Stage 'contamination canary (ci-canary)' {
        cargo $toolchain test -p ci-canary --locked
    }
    # #751: the exit code alone cannot say WHICH of two opposite things happened, so the decision
    # reads cargo's own words as well. `$script:lastStageLines` is the stage's FULL capture.
    $canaryOutcome = Get-CanaryOutcome -ExitCode $canaryCode -Lines $script:lastStageLines
    $canaryPassed = $canaryOutcome.passed
    if (-not $canaryPassed) {
        Write-Host ''
        if ($canaryOutcome.cargoLockObserved) {
            Write-Host '[gate] HARNESS-BROKE: the contamination canary never ran. Another run holds the build' -ForegroundColor Magenta
            Write-Host '[gate] directory lock and cargo said so in its own words above. That is a QUEUE, not a' -ForegroundColor Magenta
            Write-Host '[gate] finding about this head: nothing here says this build environment is contaminated.' -ForegroundColor Magenta
            Write-Host '[gate] Re-run when the other run releases the lock. See #751.' -ForegroundColor Magenta
            Write-Host '[gate] SKIPPED: the contamination canary did not run' -ForegroundColor Magenta
        } else {
        Write-Host '[gate] ABORTING: the contamination canary failed. Every other stage below would' -ForegroundColor Red
        Write-Host '[gate] run against a build environment this run cannot trust - nothing past this' -ForegroundColor Red
        Write-Host '[gate] point is evidence of anything. See #152.' -ForegroundColor Red
        }
        $emptyArtifacts = [ordered]@{ buildExitCode = $null; artifacts = @() }
        $manifestPath = Write-RunManifest -Status $canaryOutcome.status -CanaryPassed $false `
            -ArtifactManifest $emptyArtifacts -SlotLockAtStart $slotLockAtStart `
            -SlotLockAtEnd (Read-SlotLockSnapshot)
        Write-Host "[gate] manifest: $manifestPath" -ForegroundColor Cyan
        # #455: RELEASE THE TARGET before leaving. This exit sits between the `building` stamp and
        # the `complete` one, so without this line the marker stays `building` with a pid about to
        # die -- and the NEXT run reads `interrupted` and aborts before the canary. The
        # cargoLockObserved branch above tells the operator to re-run when the other run releases the
        # lock, and that re-run could never succeed: the instruction and the guard would contradict
        # each other, with no way out but deleting the marker by hand (Codex P1 on #1009).
        #
        # `complete` is the honest word. Neither branch leaves a half-built target: the lock branch
        # never compiled at all, and a canary that ran and failed is a TEST result, not an
        # interrupted build.
        # ONLY THE LOCK BRANCH RELEASES THE TARGET (Codex on #1009, and it is my own defect from the
        # commit that added this line). The release was unconditional, and `ABORTED-BY-CANARY` means
        # the canary DETECTED that cargo served contaminated output -- so stamping `complete` there
        # told the next run the target was a legitimate reuse, and it would skip the very early abort
        # this PR adds and recompile against the same uncorrected target. A mechanism that erases the
        # proof of exactly what it detects.
        #
        # The lock branch is different and is why the release exists at all: that run never compiled,
        # its own instruction is "re-run when the other run releases the lock", and leaving `building`
        # behind would make that instruction impossible to follow.
        if ($canaryOutcome.cargoLockObserved) {
            Write-TargetBuildState -TargetDir $actualTargetDir -State 'complete' -Head ([string]$gatedHeadAtStart)
        }
        exit 1
    }

    # #956: the PowerShell suites start HERE and are joined at their own stage, far below. They are
    # the only stage that can move: no cargo, so no target-directory lock, no port, no cluster. The
    # canary has already passed at this point, which matters -- a run whose build environment cannot
    # be trusted aborts above, and starting a child before that would leak a process into a run that
    # never happens.
    # #901 slice 2: a narrowed scope runs the one suite no diff can prove unreached (see
    # `Get-PsSuitesScope`); the stage keeps its name, its join and its accounting.
    $psSuitesFile = if ($script:psSuitesScope.included) { 'ci/run-ps-suites.ps1' } else { 'ci/event-contract-sweep.tests.ps1' }
    $script:psSuitesStarted = Start-BackgroundStage -Name 'ci powershell suites' -FilePath 'powershell' `
        -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $repositoryRoot $psSuitesFile)) `
        -WorkingDirectory $repositoryRoot

    # #464: Studio still runs only when its scope changed. Node's presence is checked once here so
    # a machine without Node records that fact without spawning a child. #1102 deliberately does
    # NOT start the stage here: even one reused Vitest thread missed its fixed 60-second startup
    # wait while this part of the gate was compiling Rust and running both PostgreSQL matrices.
    # The same fail-closed Studio command runs in line after those competing stages have joined.
    $script:studioNodePresent = $false
    $script:studioStarted = $null
    if ($script:studioScopeIncluded) {
        $studioNode = Get-Command -Name 'node' -ErrorAction SilentlyContinue
        $studioNpm = Get-Command -Name 'npm' -ErrorAction SilentlyContinue
        $script:studioNodePresent = ($null -ne $studioNode -and $null -ne $studioNpm)
        if (-not $script:studioNodePresent) {
            # NEVER a warning that could read as green: no stage entry exists for this run, so
            # nothing in `stages[]` could be mistaken for the Studio suite having passed. The
            # manifest's `studioNodePresent: false` is the only record, and it is unambiguous.
            Write-Host '[gate] NOTE: apps/studio changed but node/npm were not found on PATH; the Studio suite did not run this gate.' -ForegroundColor Yellow
        }
    }

    # One build pass, enumerated and fingerprinted, ahead of the human-facing stages that reuse it.
    $artifactManifest = Get-TestArtifactManifest
    # The manifest records native `$LASTEXITCODE` as Int32. Only that exact successful shape is
    # usable; zero-like strings, booleans, floating point values, and wider integers are unknown.
    $script:postgresMatrixUnavailable = -not (($artifactManifest.buildExitCode -is [int]) -and ($artifactManifest.buildExitCode -eq 0))

    Invoke-Stage 'rustfmt' {
        $fmtTargets = Get-WorkspaceFmtTargets
        $fmtPlan = @(Get-RustfmtPlan -Packages $fmtTargets -Limit 32767)
        $fmtCodes = New-Object System.Collections.Generic.List[int]
        $fmtLines = New-Object System.Collections.Generic.List[string]
        foreach ($inv in $fmtPlan) {
            $out = switch ($inv.mode) {
                'all'     { cargo $toolchain fmt --all -- --check 2>&1 }
                'package' { cargo $toolchain fmt -p $inv.package -- --check 2>&1 }
                'files'   { rustfmt $toolchain --check --edition 2024 @($inv.files) 2>&1 }
            }
            $rc = $LASTEXITCODE
            foreach ($l in @($out)) { $fmtLines.Add([string]$l); Write-Output ([string]$l) }
            $fmtCodes.Add([int]$rc)
            if ($rc -ne 0) {
                $what = if ($inv.mode -eq 'all') { '--all' } elseif ($inv.mode -eq 'package') { "-p $($inv.package)" } else { "$(@($inv.files).Count) files of $($inv.package)" }
                $why = "[gate] rustfmt: invocation ($what) exited $rc"
                $fmtLines.Add($why); Write-Output $why
            }
        }
        $fmtNote = Get-RustfmtHarnessNote -Tail $fmtLines.ToArray() -BenchPathLength $repositoryRoot.Length
        if ($fmtNote) { Write-Output $fmtNote }
        # The body's verdict: 1 if any invocation did not succeed (a crash's negative code and the mute
        # sentinel included), set through a native exit so Invoke-Stage reads it like every other stage.
        & cmd /c "exit $(Get-RustfmtStageExit -Codes $fmtCodes.ToArray())"
    } | Out-Null
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
    # #956 step 2c: THE MATRICES START HERE, one stage earlier than they used to.
    #
    # THE BUILD THAT MADE THEIR BINARIES IS `Get-TestArtifactManifest`, NOT `workspace tests`. Step 2b
    # placed this block after the `workspace tests` stage and said that stage had compiled every test
    # target. It had, but it was not the first to: `Get-TestArtifactManifest` runs
    # `cargo test --workspace --all-features --locked --no-run` far above, and its own comment calls
    # itself "one build pass ... ahead of the human-facing stages that reuse it". So the matrices were
    # waiting on a stage that only RE-proved what was already built, and the wait cost the whole of
    # `workspace tests` -- 409.9 s of it on the run that first measured step 2b (#1007, c0190949).
    #
    # BEFORE `workspace tests`, NOT BEFORE `clippy`, and the distinction is the reason this is safe.
    # `cargo clippy` COMPILES: it drives a compiler over the workspace into the same target directory
    # and holds the build lock while it does. `cargo test` on an already-built tree does not -- two
    # concurrent invocations were measured finishing in the time of one (8.7 s against an 8.5 s solo
    # baseline, three warm rounds each), because the lock is taken for a freshness check and released
    # before the binaries run. Starting the matrices above clippy would put two compilers on one lock
    # for the sake of clippy's 27.5 s; starting them here contends with nothing that compiles.
    #
    # WHAT IT ACTUALLY BUYS IS 77.3 s, NOT THE MATRICES' 254.9 s, AND THE GAP IS THE POINT. An earlier
    # draft of this comment claimed the full 254.9 s. The run that proved the placement refuted it in
    # the same breath and I had already read the number:
    #
    #     before  c0190949 (#1007)   1125.9 s     matrices after `workspace tests`
    #     after   8fa1a3d2 (#1052)   1048.6 s     matrices inside it
    #     workspace tests            409.9 s -> 450.6 s   (+40.7 s, +10 %)
    #
    # REMOVING A STAGE FROM THE CRITICAL PATH DOES NOT REMOVE ITS WORK FROM THE MACHINE. `workspace
    # tests` absorbed part of the matrices' cost by running slower beside them, and the rest went to
    # smaller growth across the `cli:` stages. On 32 cores the hidden work still cost a tenth of the
    # stage hiding it. Overlap is cheaper than serial; it is not free, and a comment that says "buys
    # the full N" is a claim this file's own manifests can falsify.
    #
    # THE COVER IS MEASURED, NOT ASSUMED: `workspace tests` ran 409.9 s and the two matrices together
    # ran 254.9 s on that same run, so the matrices finish inside it with room -- and on the run after
    # the move they did, t+291.5 -> t+505.2 against the stage's t+291.5 -> t+742.1. If that ever
    # inverts, the run is correct and merely serial again -- `Test-StageOverlapped` reports it as a
    # NOTE, and the manifest keeps both intervals so the decay is visible rather than silent.
    #
    # The join is unmoved, under the old names, after the PowerShell suites' join. Skipped matrices
    # are not started.
    if (-not $script:matrixSkipped -and -not $script:postgresMatrixUnavailable) {
        if ($PostgresBin) { $env:GRAPHHELM_PG_BIN = $PostgresBin }
        $script:pgIgnoredEarly = Start-PostgresStageEarly -Name 'PostgreSQL ignored matrix'
        $script:pgCollationEarly = Start-PostgresStageEarly -Name 'PostgreSQL matrix under a non-C collation' -Locale (Get-NonCLocale)
    }

    Invoke-Stage 'workspace tests' {
        # #903: `--workspace` when the run is FULL, `-p <crate>` per selected crate when it is not.
        # `--all-features` is UNCHANGED in both: the selection narrows WHAT is compiled, never which
        # cfg the code is compiled under.
        #
        # #1053 item 4: `nextest run` instead of `cargo test`. `cargo test` executes one test binary
        # at a time, each internally threaded, so a workspace of many small crates never fills the
        # box; nextest schedules every test from one pool, each in its own process.
        #
        # MEASURED before it was written, alternating on ONE warm target, idle box:
        #   cargo test      364 s, 346 s
        #   nextest run      90 s,  99 s      then five consecutive burn-in runs: 91/92/85/88/93 s
        # Seven runs, 3049 tests passed every time, zero flakes -- so no `.config/nextest.toml`
        # test-group exists here. A group nothing has been seen to need is a guard that only fires
        # because of a defect nobody measured.
        #
        # THE POPULATION IS IDENTICAL, which is the claim that makes the time claim mean anything.
        # `cargo nextest list --message-format json` and `cargo test -- --list` both enumerate
        # 3098 tests, and a name-by-name multiset diff in BOTH directions is empty. 3049 run plus
        # 49 ignored on each side. nextest loses nothing.
        #
        # WHAT IT DOES NOT RUN: doctests. This workspace has none -- every doc-comment fence opens
        # ```text -- and `ci/gate-nextest.tests.ps1` fails if a Rust one is ever added, because that
        # is the one way this change could silently stop running something.
        #
        # THE FLOOR IS NOW A SLEEP. nextest names it: six `wake_http` tests each sit >60 s on
        # `RACING_WAKE_LEASE_SECONDS`, so ~85 s of a ~90 s stage is one constant. Plan item L10
        # refused lowering it when it was worth ~20 s of a 1644 s gate; against a 90 s stage that
        # arithmetic is different and the refusal should be re-read, not inherited.
        $scopeArgs = @(Get-ScopePackageArgs -Scope $script:gateScope)
        if ($scopeArgs.Count -eq 0) {
            cargo $toolchain nextest run --workspace --all-features --locked --no-fail-fast
        } else {
            cargo $toolchain nextest run @scopeArgs --all-features --locked --no-fail-fast
        }
    } | Out-Null

    # #1053: ONE `nextest run` OVER THE PACKAGE, replacing 58 serial `cargo test --test <suite>`
    # invocations. This was 284 s of a 681 s gate -- 42 %, the single largest item on the critical
    # path -- and it was running the SAME tests a second time: `workspace tests` above already runs
    # them, under `--all-features`, since #1176 put nextest there.
    #
    # WHY REPLACED AND NOT DELETED, which is the whole design. Deleting would have dropped the
    # DEFAULT-FEATURE execution of these tests: the loop ran `-p graphhelm-cli` (no
    # `--all-features`), and cargo's feature unification makes that a DIFFERENT compiled unit from
    # the workspace pass. That difference is not incidental -- it is literally #243. Running
    # `nextest run -p graphhelm-cli` keeps that unit and that execution.
    #
    # MEASURED, alternating on one warm target under the same load:
    #   58 serial `cargo test --test <suite>`   175 s, 169 s
    #   one `nextest run -p graphhelm-cli`       62 s,  63 s     845 tests passed, both times
    #
    # AND IT IS A STRICT SUPERSET, which is what makes the time number mean anything. The loop
    # enumerates 691 integration tests; nextest enumerates the SAME 691 -- a name-by-name multiset
    # diff in BOTH directions is empty -- plus 156 lib and bin tests of this package that the loop
    # never ran at all, because `--test <suite>` reaches only the integration targets.
    #
    # ISOLATION IS STRONGER, NOT WEAKER. #98 gave each suite its own process to catch cross-test
    # interference -- servers, ports, temp dirs. nextest gives each TEST its own process, so the
    # interference the loop could only catch BETWEEN binaries is now caught between tests as well.
    #
    # WHAT IS LOST, stated plainly: 58 stage names become one. No automated consumer read them --
    # `ci/merge-proof.ps1` read no stage name at all and the merge checklist read only the set of
    # FAILED stage names, which survives (both retired 2026-09-24). On a red, nextest names the failing TEST, which
    # is a finer address than the binary the loop would have named.
    #
    # #243's PROPERTY SURVIVES, expressed per package instead of per suite: fingerprint the unit
    # this stage is about to run, BEFORE it runs, into the SAME artefact list the freshness check
    # (`staleArtifacts`, `instrumentSuspect`) reads. One call now covers all 58 binaries because one
    # stage now runs all 58.
    $cliFingerprintStartedUtc = [DateTime]::UtcNow
    $cliManifest = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli')
    $cliFingerprintEndedUtc = [DateTime]::UtcNow
    # A fingerprint pass that could not build is a gate failure in its own right, not a value to
    # drop and move past: the stage below would run its own tests regardless, and if THAT succeeded
    # the gate would finish GREEN while the freshness population was incomplete -- the exact blind
    # spot #243 exists to close, silently restored.
    # #901 slice 2: read through `Test-Path` -- ci/gate-suite-artifact.tests.ps1 runs this block
    # without the script body, where an unset flag read as $null silently dropped the failure record.
    $cliRustSkipped = (Test-Path 'variable:script:rustStagesIncluded') -and -not $script:rustStagesIncluded
    if ((-not $cliRustSkipped) -and $cliManifest.buildExitCode -ne 0) {
        Write-Host "[gate] cli suites - fingerprint build FAILED (exit $($cliManifest.buildExitCode)); the freshness population for this package is incomplete" -ForegroundColor Red
        foreach ($evidenceLine in $cliManifest.outputTail) { Write-Host $evidenceLine }
        $script:failed = @($script:failed) + 'cli suites (fingerprint)'
        $script:stageRecords.Add([ordered]@{
                name         = 'cli suites (fingerprint)'
                passed       = $false
                exitCode     = [int]$cliManifest.buildExitCode
                wallTimeSecs = [math]::Round(($cliFingerprintEndedUtc - $cliFingerprintStartedUtc).TotalSeconds, 3)
                startedUtc   = $cliFingerprintStartedUtc.ToString('o')
                endedUtc     = $cliFingerprintEndedUtc.ToString('o')
                outputTail   = @($cliManifest.outputTail)
            })
    }
    foreach ($cliArtifact in $cliManifest.artifacts) {
        $artifactManifest.artifacts.Add($cliArtifact)
    }
    # #1007: the counts follow the list. Growing the LIST without growing the COUNTS published a
    # receipt reading `artifactsUnprovenReuse: 0` over an unproven population, which the merge
    # checklist's receipt-signature paragraph reads as a PROVEN reuse.
    foreach ($countName in @('artifactsRebuilt', 'artifactsProvenReuse', 'artifactsUnprovenReuse', 'artifactsContaminated', 'artifactsEnrolled')) {
        $artifactManifest[$countName] = [int]$artifactManifest[$countName] + [int]$cliManifest[$countName]
    }
    if ($cliManifest.artifacts.Count -gt 0 -and [string]$artifactManifest['buildMode'] -ne 'unknown' -and [string]$cliManifest['buildMode'] -eq 'warm') {
        $artifactManifest['buildMode'] = 'warm'
    }
    Invoke-Stage 'cli suites' {
        cargo $toolchain nextest run -p graphhelm-cli --locked --no-fail-fast
    } | Out-Null

    # `--profile test` ON ALL THREE, AND IT IS A BUILD FLAG, NOT A TEST ONE (#1053).
    #
    # `cargo run` builds under the DEV profile. `Cargo.toml` declares `[profile.test] debug = 1`
    # against dev's default `debug = 2`, and a differing `-C debuginfo` is a different compiled unit
    # to cargo -- so these three stages, running immediately after a `cli:` loop that just built this
    # crate's entire dependency chain under the TEST profile, recompiled all of it to change one
    # debuginfo level.
    #
    # THE MEASUREMENT THAT NAMES IT A BUILD, from the 370 receipts committed to .factory/gate-runs
    # before the store was retired on 2026-09-24: `schema
    # catalog` n=358, p10 51.7 s, median 73.2 s, p90 153.3 s -- while the two stages below it, which
    # reuse the binary it just built, cost 0.7 s and 0.5 s. One program answering three questions
    # cannot be a hundred times slower on the first for any reason but compiling.
    #
    # WHAT IT DOES NOT CHANGE: feature resolution, opt-level, debug-assertions, overflow-checks and
    # panic are identical between the two profiles here; the flag moves exactly `-C debuginfo`.
    # That equality is the whole licence for this flag, and it is not left to this comment --
    # `ci/schema-stage-profile.tests.ps1` fails if `[profile.test]` ever carries a key other than
    # `debug`, naming it, because on that day these three stages would be running a DIFFERENT
    # program than the one the gate tested and the flag must come back out.
    #
    # NOT FIXED BY `[profile.dev] debug = 1` INSTEAD. That would also collapse the two units, and it
    # would silently degrade the debuginfo every developer gets from a plain `cargo build` in order
    # to solve a gate-only problem. The flag puts the coupling where a reader of the stage sees it.
    Invoke-Stage 'schema catalog' {
        cargo $toolchain run --profile test --locked -q -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
    } | Out-Null
    Invoke-Stage 'schema baseline compatibility' {
        cargo $toolchain run --profile test --locked -q -p graphhelm-cli -- schema check `
            --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
    } | Out-Null
    Invoke-Stage 'schema conformance' {
        cargo $toolchain run --profile test --locked -q -p graphhelm-cli -- schema conformance `
            --catalog schemas/catalog.json --fixtures conformance/manifest.json
    } | Out-Null
    Invoke-Stage 'locked metadata' {
        cargo $toolchain metadata --locked --no-deps --format-version 1 | Out-Null
    } | Out-Null
    Invoke-Stage 'whitespace' { git diff --check } | Out-Null
    # #643: every versioned ci/*.tests.ps1, discovered from the tree. See the script for why the
    # inventory is a pinned SET of names and not a count, and why exit 2 is not folded into 1.
    # #194: A PINNED RELEASE IS HISTORY, AND REWRITING IT HAS TO SAY SO.
    #
    # `schemas/releases/*` is a frozen copy of a schema that four readers pin -- `schema migrate`
    # builds `schemas/releases/{from_version}/catalog.json` to migrate FROM, and
    # `postgres-event-store/src/backup.rs` pins the catalog twice. A vocabulary change must touch
    # the LIVE schema and never the frozen one, but the natural gesture for "the schema changed" is
    # to regenerate the schema set, and a bulk regeneration rewrites the frozen release silently.
    # The destructive gesture is the maintenance gesture, and until this stage nothing anywhere made
    # a diff under that directory red.
    #
    # It refuses the SILENCE, not the rewrite: a release genuinely being reissued is somebody's
    # decision to make, and a guard that could not be satisfied would be routed around within a
    # week. `Rewrites-Release: D-0nn -- …` on the commit that does it is the whole licence.
    #
    # The merge base is resolved here rather than passed in. Measured on the runner's own benches:
    # `git merge-base HEAD origin/main` resolves in a worktree, because a worktree shares the
    # repository's refs. On `main` itself the range is empty and the stage is trivially clean.
    Invoke-Stage 'frozen release guard' {
        & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass `
            -File (Join-Path $repositoryRoot 'ci/frozen-release-guard.ps1') `
            -RepoRoot $repositoryRoot `
            -MergeBase $mergeTargetSnapshot.mergeBase `
            -Head 'HEAD'
    } | Out-Null

    # #956: the same stage, the same name, the same accounting -- only the WORK started earlier. The
    # call keeps the shape it always had because ci/gate-stage-reddens.tests.ps1 slices this block
    # out of this file by anchor: a changed call shape breaks that suite's FIXTURE rather than its
    # subject, and a harness breaking while wearing an ordinary red is the thing it exists to catch.
    Invoke-Stage 'ci powershell suites' -AlwaysRun {
        # The join emits the child's lines on the success stream and its exit code LAST. The lines
        # must go on to Invoke-Stage's capture (this stage's outputTail, `$script:allStageLines`,
        # and so the manifest's `eventContractSweep`); the code is kept. `$joined = ...` swallowed
        # both into one variable: ten landed manifests carried 0 lines for this stage (#205).
        $joined = $null
        Complete-BackgroundStage -Started $script:psSuitesStarted | ForEach-Object {
            if ($_ -is [int]) { $joined = $_ } else { $_ }
        }
        if ($null -eq $joined) {
            # The early start failed. Same command, same place, exactly as long as before #956.
            # #901 slice 2: the same choice as the early start. Read through `Test-Path` because
            # ci/gate-stage-reddens.tests.ps1 runs this block without the script body.
            $psSuitesNarrowed = (Test-Path 'variable:script:psSuitesScope') -and $script:psSuitesScope -and (-not $script:psSuitesScope.included)
            if ($psSuitesNarrowed) {
                & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $repositoryRoot 'ci/event-contract-sweep.tests.ps1')
            } else {
                & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $repositoryRoot 'ci/run-ps-suites.ps1')
            }
        }
        # No else: `Complete-BackgroundStage` sets both instants itself, from the child's own times.
    } | Out-Null

    if ($script:matrixSkipped) {
        Write-Host ''
        if ($SkipPostgres) {
            Write-Host '[gate] PostgreSQL matrix SKIPPED (-SkipPostgres) - this is not a full gate.' -ForegroundColor Yellow
        } else {
            # NAMED, never a bare "skipped": a reader who cannot tell WHY a matrix did not run cannot
            # tell a scoped run from a broken one, and both leave the same hole in the record.
            Write-Host "[gate] PostgreSQL matrix SKIPPED by scope - $([string]$script:gateScope.matrixReason)" -ForegroundColor Yellow
            Write-Host '[gate] this run does NOT cover persistence: coverage.complete is false.' -ForegroundColor Yellow
        }
    } elseif ($script:postgresMatrixUnavailable) {
        Write-Host '[gate] PostgreSQL matrix NOT RUN: the artifact build did not succeed; persistence coverage is incomplete.' -ForegroundColor Red
    } else {
        if ($PostgresBin) { $env:GRAPHHELM_PG_BIN = $PostgresBin }
        # postgres.ps1 ends in `exit`, which terminates the *calling* script in PowerShell, so it
        # must run as a child process or the gate dies here and never reports.
        Complete-PostgresStage -Name 'PostgreSQL ignored matrix' -Early $script:pgIgnoredEarly
        # The C locale makes text ordering identical to COLLATE "C", which is exactly the condition
        # under which collation-dependent ordering defects stay invisible. This second pass is the
        # regression guard for that class and is not optional.
        $previousLocale = $env:GRAPHHELM_PG_LOCALE
        try {
            # The locale matters here only for the in-line fallback; the early child read it at its spawn.
            $env:GRAPHHELM_PG_LOCALE = Get-NonCLocale
            Complete-PostgresStage -Name 'PostgreSQL matrix under a non-C collation' -Early $script:pgCollationEarly
        } finally {
            $env:GRAPHHELM_PG_LOCALE = $previousLocale
        }
    }

    # #1102: start Node only after the Rust work and both PostgreSQL children are finished. The
    # previous overlap made Vitest's startup timeout a test of host contention rather than of this
    # tree. This remains an ordinary fail-closed stage: no retry, no special exit-code handling.
    if ($script:studioScopeIncluded -and $script:studioNodePresent) {
        Invoke-Stage 'apps/studio (npm)' {
            & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $repositoryRoot 'ci/studio-stage.ps1')
        } | Out-Null
    }

    # #207: THE TARGETS A MANIFEST HIDES BEHIND A FEATURE EITHER BUILT, OR THIS RUN SAYS SO.
    #
    # `adapters/postgres-event-store/Cargo.toml` gates `concurrency` and `repository_conformance`
    # behind `test-support`, which is not in `default`. They run today only because this file passes
    # `--all-features` -- a flag that is here to COMPILE everything, not to cover
    # `required-features`. Narrow those flags for speed and the targets are not skipped with a
    # message: they are never built, print no per-test lines, and the run stays green. "0 tests from
    # a target that never built" is byte-identical to "a target with nothing to run", and only an
    # absence distinguishes them.
    #
    # The population is DERIVED from `cargo metadata`, never written down: a target added tomorrow
    # is checked tomorrow, and a hand-maintained list fails in the opposite direction by staying
    # green about targets nobody added it to. That derivation is #207's condition for building this
    # at all.
    #
    # The transcript goes to a temp file OUTSIDE the repository. Inside it, `dirtyDiffHash` goes
    # non-null and this gate correctly refuses to commit a manifest into a tree holding changes it
    # did not make -- a run with every stage green ending RED for "run manifest not published",
    # measured twice on 2026-09-05 by two lanes.
    # #207 follow-up: a SCOPED run never builds a crate outside its selection, and checking every
    # workspace-gated target regardless made every scoped run that excludes the gating crate
    # (`adapters/postgres-event-store`) red for a defect that is not there -- measured on #990's
    # first scoped run, the stage's own manifest, `graphhelm-postgres-event-store/concurrency` and
    # `/repository_conformance` reported missing on a diff that never selected that crate. The
    # POPULATION `required-features.ps1` derives is unaffected (`cargo metadata`, unscoped, as
    # #207 requires); only which of it this run is judged AGAINST narrows, to `$script:gateScope`.
    $requiredFeaturesScopeReportPath = Join-Path ([System.IO.Path]::GetTempPath()) `
        ("gate-required-features-scope-" + [guid]::NewGuid().ToString('N').Substring(0, 12) + ".json")
    $script:requiredFeaturesExcluded = @()
    $script:requiredFeaturesReportUnreadable = $true
    Invoke-Stage 'required-features coverage' {
        $transcriptPath = Join-Path ([System.IO.Path]::GetTempPath()) `
            ("gate-transcript-" + [guid]::NewGuid().ToString('N').Substring(0, 12) + ".txt")
        try {
            [System.IO.File]::WriteAllText($transcriptPath, ($script:allStageLines -join "`n"))
            # THE ARGUMENT LIST IS BUILT, NOT WRITTEN INLINE (measured, #1019 follow-up). Two separate
            # defects lived in the inline form this replaces, both across the SAME `-File` process
            # boundary:
            #   1. `-Full:([bool]$script:gateScope.full)` -- an EXPLICIT VALUE on a `[switch]`
            #      parameter. Under `-File` on Windows PowerShell 5.1 the value arrives as a bare
            #      STRING ("True"/"False"), and `[switch]` refuses to convert one:
            #      "Cannot convert value \"System.String\" to type \"System.Management.Automation.
            #      SwitchParameter\"" -- exit 1, every run, `-Full:$true` and `-Full:$false` alike.
            #   2. `-InScopeCrates ''` -- an EMPTY STRING passed as a `-File` argument is dropped
            #      entirely at the process boundary, not received as an empty string. The next
            #      token (`-Full:...`) is then read as ITS value, and PowerShell reports
            #      "Missing an argument for parameter 'InScopeCrates'" -- so a FULL run (where this
            #      argument is always empty) failed before the switch bug was even reached.
            # Both are avoided by never emitting either token on the command line unless it carries a
            # real value: `-Full` (bare, `IsPresent`-style) only when true, `-InScopeCrates` (still one
            # comma-joined STRING, not `[string[]]` -- see `required-features.ps1`'s own param doc for
            # why) only when there is a non-empty selection to pass. Verified directly against the
            # real script, both branches, before wiring it here: FULL -> reportWritten (empty
            # `excluded`); SCOPED, selection excluding the gating crate -> reportWritten (both gated
            # targets named `excluded`). Neither exited 1 with the report missing, which is what every
            # prior call shape did.
            $requiredFeaturesArgs = @('-TranscriptPath', $transcriptPath)
            if ($script:gateScope.full) {
                $requiredFeaturesArgs += '-Full'
            } else {
                $inScopeCratesArg = (@($script:gateScope.crates) -join ',')
                if ($inScopeCratesArg) {
                    $requiredFeaturesArgs += '-InScopeCrates'
                    $requiredFeaturesArgs += $inScopeCratesArg
                }
            }
            $requiredFeaturesArgs += '-ScopeReportPath'
            $requiredFeaturesArgs += $requiredFeaturesScopeReportPath
            & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass `
                -File (Join-Path $repositoryRoot 'ci/required-features.ps1') `
                @requiredFeaturesArgs
        } finally {
            Remove-Item -LiteralPath $transcriptPath -Force -ErrorAction SilentlyContinue
        }
    } | Out-Null
    if (Test-Path -LiteralPath $requiredFeaturesScopeReportPath) {
        try {
            $requiredFeaturesReport = [System.IO.File]::ReadAllText($requiredFeaturesScopeReportPath) | ConvertFrom-Json
            if ($requiredFeaturesReport -and ($requiredFeaturesReport.PSObject.Properties.Name -contains 'excluded')) {
                $script:requiredFeaturesExcluded = @($requiredFeaturesReport.excluded)
                # THE ONE PLACE THIS GOES FALSE: a report was actually read and had the shape this
                # reads, whether or not it named anything as excluded. Every other path here leaves
                # the `$true` default (or sets it explicitly) standing.
                $script:requiredFeaturesReportUnreadable = $false
            } else {
                # PARSED, BUT NOT THE SHAPE THIS READS: same as a parse failure below -- a document
                # that is not an object, or is one with no `excluded` key, has not said "nothing was
                # excluded" any more than an unparseable one has.
                $script:requiredFeaturesReportUnreadable = $true
            }
        } catch {
            # UNREADABLE IS NOT EMPTY (#1019 review, B): the manifest write reads this flag and
            # writes `null` rather than `@()`, so a reader can tell "the stage's report could not be
            # read" apart from "the stage reported nothing excluded" -- an empty array is a claim
            # this run has not earned when its own instrument never answered.
            $script:requiredFeaturesReportUnreadable = $true
        } finally {
            Remove-Item -LiteralPath $requiredFeaturesScopeReportPath -Force -ErrorAction SilentlyContinue
        }
    } else {
        # THE STAGE'S SUBPROCESS NEVER WROTE THE REPORT AT ALL (crashed, or `-ScopeReportPath` was
        # never reached) -- the same absence as a report that could not be parsed, not a report that
        # said "nothing excluded".
        $script:requiredFeaturesReportUnreadable = $true
    }

    # #455: the build in this target finished. Written HERE and not in the finally below, because
    # the whole value of the marker is that it distinguishes a run that completed from one that did
    # not -- a completion stamp written on every exit path would say "complete" for the interrupted
    # run too, which is the one case the next gate needs to recognise.
    #
    # AND BEFORE THE FLAG, not after it (Codex P1 on #1009), for two reasons that point the same way.
    # `ci/gate-run-abort.tests.ps1` pins the literal text `$script:stagesCompleted = $true` followed
    # by `} finally {` -- the flag must be the LAST statement of the try so nothing after it can be
    # skipped -- and my first draft put this call between them: measured, that suite went to exit 1
    # on the one assertion, which would have reddened `ci powershell suites` on every gate in the
    # fleet. The pin was right and the draft was wrong. Second, if this write throws, the flag is
    # still false when `finally` runs, so `Write-RunAbort` fires and the run leaves a terminal
    # ledger record; with the old order a throwing marker write left the run with none.
    # ONLY OVER A RUN WHOSE ARTEFACTS ARE FRESH (Codex on #1009). The stamp was unconditional, so a
    # run that finished with stale binaries -- the exact contamination the freshness cross-check is
    # about to redden the gate for -- still recorded `complete`, and the NEXT run trusted that target
    # as a legitimate reuse and spent another full gate rediscovering the same stale output. Third
    # instance tonight of one shape: releasing the mark before the answer is known. The canary path
    # had it, and I fixed that one and left this.
    #
    # A stale run deliberately leaves the mark UNSTAMPED, so the next run reads `interrupted` and
    # refuses early. The word is imprecise -- nothing was interrupted, the run completed badly -- and
    # the VERDICT is right, which is the half that matters; the reason string names the marker and
    # the abort names the remedy.
    #
    # #904: THE NEW COUNT, and this site is the one that must not be forgotten. Leave it reading
    # `freshBuild -eq $false` and every legitimate warm run fails to stamp `complete`; the NEXT run
    # then reads `interrupted` and aborts before its first compile -- a silent trap that turns the
    # saving into a permanent refusal. That is why the filter is a function and not four copies.
    $unprovenAtEnd = (Select-UnprovenReuse -Artifacts $artifactManifest.artifacts).Count
    # #1038 review (lane S): AND THE BUILD PASS ITSELF MUST HAVE SUCCEEDED. The count above is taken
    # over whatever the build pass ENUMERATED, so it is silent about a pass that stopped early: a
    # cold run on this branch exited 101 after 22 of 234 test binaries, all 22 rebuilt, count 0 --
    # and the two lines below then wrote a ledger covering 22 and stamped the target `complete`.
    # The run itself was correctly RED (`$buildPassSucceeded` is a term of `$passedEverything`), but
    # the TARGET kept a truncated custody record under a stamp saying the opposite, and every later
    # run found 212 executables with no entry, called them `unproven-reuse`, and went RED -- while
    # cargo refused to rebuild them, because by mtime they were fresh. Stuck RED until a human
    # deleted the target.
    #
    # THE REMEDY USED TO BE "LEAVE THE MARK UNSTAMPED", on the belief that a `building` mark whose
    # process is gone reads back as `interrupted` and is "rebuilt cold by the next run". The first
    # half was true; the second was FALSE (#1007, verified link by link): `interrupted` is SUSPECT,
    # and a suspect target ABORTS at the #455 guard before its first compile, HARNESS-BROKE, and the
    # mark is deliberately sticky. A finished run that could not vouch was therefore bricking its
    # target, and #1007 -- which enrols ~54 per-suite binaries that have no ledger row -- made every
    # target hit it. The pass was not interrupted; it finished. So it is stamped `unproven`: a
    # terminal state the reader treats as contaminated (no clean-reuse claim) and NOT suspect (no
    # abort), and the next run proves or rebuilds each binary itself.
    #
    # THE SAME DERIVATION AS THE VERDICT'S, spelled the same way on purpose (`-is [int]` first, so
    # the `$null` of a manifest that was never built widens to "not established").
    $buildExitAtEnd = $artifactManifest.buildExitCode
    $buildPassSucceededAtEnd = ($buildExitAtEnd -is [int]) -and ($buildExitAtEnd -eq 0)
    # THE REFUSAL'S SENTENCE IS BUILT HERE, ABOVE THE GUARD, and that is a constraint of this file
    # rather than a preference: ci/gate-target-build-state.tests.ps1 requires the `complete` stamp to
    # sit within 400 characters of `$script:stagesCompleted`, so that a stamp can never drift onto a
    # path that did not finish. Two branches' worth of prose after the stamp broke that distance;
    # one short `else` keeps the guard readable and the stamp adjacent to the flag.
    $unstampedNote = if (-not $buildPassSucceededAtEnd) {
        "[gate] NOTE: the artefact build pass did not succeed (exit $buildExitAtEnd), so its list of $(if ($artifactManifest.artifacts) { $artifactManifest.artifacts.Count } else { 0 }) binary(ies) may be PARTIAL. No ledger is written and this target is stamped unproven, so the next run reuses nothing on trust and proves or rebuilds each binary itself."
    } else {
        "[gate] NOTE: $unprovenAtEnd artefact(s) this run cannot vouch for, so this target is NOT stamped as a clean reuse."
    }
    # #1007: the `else` below stamps `unproven` -- FINISHED, NOT DIED -- on both of its cases. A build
    # pass that failed returns from cargo with whole fingerprints (cargo writes a fingerprint only on
    # success), so it is as far from `interrupted` as a pass that could not vouch; leaving `building`
    # there is what made the next run abort before its first compile. The prose sits HERE, above the
    # guard, for the same 400-character reason as the note above: a comment inside the `else` pushed
    # the completion flag out of the stamp's reach and ci/gate-target-build-state.tests.ps1 said so.
    # #901: no build, no ledger, no stamp -- the `elseif` below carries the flag.
    if (-not $script:rustStagesIncluded) { Write-Host '[gate] NOTE: no Rust stage ran, so the target and its build-state marker are left exactly as they were.' -ForegroundColor DarkGray }
    if ($unprovenAtEnd -eq 0 -and $buildPassSucceededAtEnd) {
        # #904: ONLY HERE IS THE LEDGER WRITTEN, and BEFORE the stamp. Every artefact of this run
        # was either rebuilt inside the run's own window -- where the mtime rule still has full
        # authority -- or proven against the previous ledger by content; that is what makes the
        # chain of custody sound, and a run that could not vouch for one of its binaries does not
        # get to vouch for any of them. It goes first so the claim "this target is a clean reuse"
        # is never recorded without the custody record that justifies it already on disk, and so
        # the stamp stays adjacent to the completion flag that ci/gate-target-build-state.tests.ps1
        # requires it to sit beside.
        try {
            $ledgerPath = Write-ArtifactLedger -TargetDir $actualTargetDir -Artifacts $artifactManifest.artifacts
            Write-Host "[gate] artefact ledger written: $ledgerPath" -ForegroundColor DarkGray
        } catch {
            # A ledger this run could not write costs the NEXT run a cold compile and nothing else:
            # an absent entry reads as `unproven-reuse`, which is RED, which is today's behaviour.
            # Not worth failing a run that passed everything, and not worth hiding either.
            Write-Host "[gate] NOTE: the artefact ledger could not be written, so the next run cannot prove a reuse: $($_.Exception.Message)" -ForegroundColor Yellow
        }
        Write-TargetBuildState -TargetDir $actualTargetDir -State 'complete' -Head ([string]$gatedHeadAtStart)
    } elseif ($script:rustStagesIncluded) {
        Write-Host $unstampedNote -ForegroundColor Yellow
        Write-TargetBuildState -TargetDir $actualTargetDir -State 'unproven' -Head ([string]$gatedHeadAtStart)
    }
    $script:stagesCompleted = $true
} finally {
    Pop-Location
    # #1003 review (Codex P2): a background stage that was STARTED but never reached its own
    # `Complete-BackgroundStage` join -- because something between the two threw -- used to leak its
    # child process past this run's lifetime. A retry in the same checkout could then start a SECOND
    # `npm ci`/`run-ps-suites.ps1` against files the first is still using. Reaped here because this
    # is the one path every abort (an exception, an `exit` inside a stage, Ctrl-C's own finally)
    # still runs; a normal run reaches this with both processes already joined and exited, so
    # `HasExited` is true and nothing below does anything.
    foreach ($started in @($script:studioStarted, $script:psSuitesStarted)) {
        if ($null -eq $started) { continue }
        try {
            if (-not $started.Process.HasExited) {
                Write-Host "[gate] $($started.Name) never reached its join; stopping its background process tree (pid $($started.Process.Id))." -ForegroundColor Yellow
                # `/T`, NOT `.Kill()` (X, review of #1003). `.Process` is the `powershell.exe`
                # WRAPPER `Start-BackgroundStage` launched to run `ci/studio-stage.ps1`; on
                # PowerShell 5.1, `Process.Kill()` ends only that one process, not the `npm`/`node`
                # children it spawned to run the stage. A killed wrapper leaves `npm ci` still
                # writing into `apps/studio/node_modules` -- exactly the interference this reap
                # exists to prevent, just one process down. `taskkill /T` walks the tree by parent
                # pid, the same shape B's tree-kill for #1005 uses.
                & taskkill /T /F /PID $started.Process.Id 2>&1 | Out-Null
                $started.Process.WaitForExit(5000) | Out-Null
            }
        } catch {
            # Best effort: a process that exited between the check and the kill, or a handle this
            # session can no longer reach, is not a reason to fail the abort path itself.
        }
    }
    $postgresCleanupErrors = @()
    foreach ($early in @($script:pgIgnoredEarly, $script:pgCollationEarly)) {
        if ($null -ne $early) {
            try { Stop-PostgresStageEarly -Early $early }
            catch { $postgresCleanupErrors += $_ }
        }
    }
    if ($postgresCleanupErrors.Count -gt 0) {
        # RECORDED, NOT THROWN (X's adversarial review of this PR). The first draft threw here, and
        # a throw at this point in a `finally` costs more than it buys:
        #
        #   1. There is no `catch` between `} finally {` above and its close below, and
        #      `Write-RunManifest` for the normal path is OUTSIDE this block. So a 10-second drain
        #      timeout during any abort discarded the whole run's manifest -- every stage record,
        #      every timing -- and replaced a real, nameable stage failure with a sentence about
        #      PostgreSQL cleanup. That is the trap `Invoke-Stage`'s own #841 comment names:
        #      replacing the cause with a complaint about the instrument that was recording it.
        #   2. It also jumped over `Remove-SlotClaim` below, so the claim it said it was RETAINING
        #      was left holding a PID that was about to die -- and `ci/slot-lock.ps1`'s reclaimer
        #      keys on pure PID liveness with no clause for "a RUN-ABORT asked for this to be held".
        #      The next gate takes the slot regardless. The sentence described a guarantee the
        #      system does not provide.
        #
        # What actually contains the children is `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: the gate
        # process exiting closes the job handle and the clusters die with it. That is the mechanism
        # the throw made redundant while costing the receipt. So: say it on the ledger, let the
        # `finally` finish, and let the manifest carry the real cause.
        Write-RunAbort -Reason "PostgreSQL cleanup was not verified: $($postgresCleanupErrors -join '; ')"
    }
    # #700: a run that leaves this block without completing its stages and without having written
    # RUN-END -- an exception, an `exit` inside a stage, Ctrl-C -- says so on the ledger. The canary
    # abort writes RUN-END before it exits, so it is not double-counted. A hard kill skips every
    # finally and leaves the lock: that is the case the pair-liveness reader recovers on the NEXT
    # gate, by design, not a gap.
    if (-not $script:stagesCompleted) {
        # #755: through the one writer, so this exit and the manifest catch cannot drift apart and
        # the guard lives in one place. `slotRunEnded` is still what stops a double line; it is
        # checked inside Write-RunAbort rather than repeated here.
        Write-RunAbort -Reason 'the stage block was left before it finished'
    }
    # Released HERE, before the manifest write: the slot serialises cargo, not git, and the next
    # gate can start compiling while this one records itself. Only a claim this process made is
    # released; an inherited claim belongs to the wrapper that made it.
    if ($script:slotOutcome -eq 'claimed') { Remove-SlotClaim -Path $slotLockPath -HolderPid $PID | Out-Null }
}

# Freshness cross-check BEFORE the manifest write, deliberately - $Status and $failed must both
# be final by the time Write-RunManifest reads them, or a manifest written first would report
# GREEN for a run the freshness check was about to fail moments later (#152 review: this ordering
# bug was caught before the slot, not after - a manifest race is exactly the kind of thing that
# would have shipped quietly otherwise).
if ($artifactManifest -and $artifactManifest.artifacts) {
    # #904: the fourth and last consumer of the count. The QUESTION is unchanged -- did the stages
    # run this tree's program? -- and only the way of answering it moved, from a timestamp to the
    # content chain. A binary that predates the run's start but whose crate inputs are proven
    # identical IS this tree's program, so it is no longer counted here.
    $staleCount = (Select-UnprovenReuse -Artifacts $artifactManifest.artifacts).Count
    if ($staleCount -gt 0) {
        Write-Host ''
        # #822: the message says what it does to its siblings. A stale binary is a different program,
        # so the stages that ran it did not measure this tree; their reds are not findings about it.
        Write-Host "[gate] FRESHNESS CROSS-CHECK: $staleCount test binary(ies) cannot be vouched for -- not rebuilt this run, and not proven unchanged against this target's ledger -- so every stage that ran a test binary may have measured a DIFFERENT PROGRAM than this run's tree - see the manifest. The other stage results are not readable as results for this head." -ForegroundColor Red
        # The same constant the verdict filters by: one name, one place. (#822 follow-up.)
        $failed += $FreshnessStageName
        $staleBinaryCount = $staleCount
    }
}

# #956: the run checks its OWN overlap, from its own recorded intervals.
#
# The saving here is structural, not a constant: `ci powershell suites` is the only stage that runs
# no cargo, so it is the only one that holds no target-directory lock, and its ~5 minutes are spent
# beside the Rust stages instead of after them. That arrangement is exactly the kind that decays in
# silence -- put the start back where it was and every stage still runs, still passes, still appears
# in the manifest, and the ONLY symptom is minutes nobody counts. So the claim is MEASURED and the
# measurement is published, in the manifest and on the console.
#
# TWO STATES, NOT ONE. `psSuitesStartedEarly` records whether the child was launched at all;
# `psSuitesOverlapped` whether it actually ran beside something. A run whose early start FAILED (no
# Start-Process, a busted PowerShell) is correct and merely slow, and already said so on the console.
#
# A NOTE, NEVER A VERDICT. The first version appended a frame to $failed when the suites started
# early and overlapped nothing, and on 2026-09-07 it turned a 59-of-59 run RED (#979, manifest
# 66259741ff2a-20260907T212946.729Z-45d781f4.json: failed stages [], both matrices 49 of 256, canary
# true, stale 0). On the HDD the first build pass takes longer than the suites do -- there the
# suites finished at 19:55 and cargo reached its first stage at 20:35 -- so "serial" is the
# common case, not a regression, and it says nothing about the tree. A run's status is derived
# from its STAGES alone; what the overlap measures is the optimisation's saving, and a saving that
# did not happen is reported as a saving that did not happen. The fields stay in the manifest so
# the arrangement's decay is still visible to anyone who reads them.
$script:psSuitesStartedEarly = ($null -ne $script:psSuitesStarted)
$script:psSuitesOverlapped = Test-StageOverlapped -Records $stageRecords -Name 'ci powershell suites'
# #956 step 2b: the same measurement for the matrices, under the same rule -- a note, never a
# verdict. The pair overlapping each other counts: that is the saving this step buys.
$script:pgMatricesStartedEarly = ($null -ne $script:pgIgnoredEarly) -or ($null -ne $script:pgCollationEarly)
$script:pgMatricesOverlapped = (Test-StageOverlapped -Records $stageRecords -Name 'PostgreSQL ignored matrix') -or
    (Test-StageOverlapped -Records $stageRecords -Name 'PostgreSQL matrix under a non-C collation')
if ($script:pgMatricesStartedEarly -and -not $script:pgMatricesOverlapped) {
    Write-Host "[gate] NOTE: the PostgreSQL matrices started early but overlapped no other stage; the run went serial there and the status does not depend on it." -ForegroundColor Yellow
}
if ($script:psSuitesStartedEarly -and -not $script:psSuitesOverlapped) {
    Write-Host "[gate] NOTE: ci powershell suites started early but overlapped no other stage; the run went serial and the status does not depend on it." -ForegroundColor Yellow
}

# #464: same measured-not-assumed discipline as the PowerShell suites above.
$script:studioStartedEarly = ($null -ne $script:studioStarted)
$script:studioOverlapped = Test-StageOverlapped -Records $stageRecords -Name 'apps/studio (npm)'
if ($script:studioStartedEarly -and -not $script:studioOverlapped) {
    Write-Host "[gate] NOTE: apps/studio (npm) started early but overlapped no other stage; the run went serial and the status does not depend on it." -ForegroundColor Yellow
}

$manifestPath = $null
$manifestFailed = $false
try {
    # #455: a suspect target reaches the STATUS, because that is the only field anything downstream
    # reads (Codex P1 on #1009, and it was right about a claim I had published twice).
    # `$passedEverything` and `instrumentSuspect` are computed inside Write-RunManifest and are
    # recorded, not enforced: `merge-proof.ps1` (retired 2026-09-24) required `status == GREEN` and
    # never read either of them -- measured, `instrumentSuspect` appeared in that file zero times. So
    # the first draft's console line "the run continues and cannot report GREEN" was FALSE: every
    # stage passing on a target left broken by a previous run produced GREEN, exit 0, and a manifest
    # merge-proof accepted.
    #
    # HARNESS-BROKE, not RED. RED is a claim about the tree, and the tree did not fail -- the run
    # could not vouch for what it measured, which is exactly the third state this vocabulary already
    # carries. Reporting it as RED would send a lane hunting a defect that is not there, and a red
    # that fires for the instrument is the kind pressers learn to wave through.
    $status = Get-GateStatus -FailedStageCount $failed.Count -TargetSuspect ([bool]($null -ne $script:targetBuildState -and $script:targetBuildState.suspect))
    $manifestPath = Write-RunManifest -Status $status -CanaryPassed $true -ArtifactManifest $artifactManifest `
        -SlotLockAtStart $slotLockAtStart -SlotLockAtEnd (Read-SlotLockSnapshot)
} catch {
    # "Green without a well-formed manifest is red by rule" (#152) - a manifest that fails to
    # write is a gate failure in its own right, never a silent gap a clean stage run papers over.
    $manifestFailed = $true
    Write-Host "[gate] MANIFEST WRITE FAILED: $($_.Exception.Message)" -ForegroundColor Red
    # #755: the OTHER way a run ends without RUN-END. The stage block completed, so its finally
    # wrote nothing; without this the ledger holds a START and no terminal line at all, which is
    # indistinguishable from a run still going.
    Write-RunAbort -Reason "the run manifest could not be written: $($_.Exception.Message)"
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
    # #822: the cross-check is printed as the FRAME the other reds are read in, never as their peer.
    foreach ($line in (Get-GateVerdictLines -Failed $failed -StaleBinaryCount $staleBinaryCount `
                -FreshnessStageName $FreshnessStageName)) {
        Write-Host $line -ForegroundColor Red
    }
    exit 1
}
# #455: the process verdict, not only the manifest. The abort above makes this unreachable for a
# suspect target today, and it is here anyway: "unreachable" is a claim, the terminal block decided
# on `$failed.Count` alone for the whole life of this script, and a later edit that moves the abort
# would restore GREEN-and-exit-0 over an untrusted target with nothing to catch it (Codex P1, #1009).
if ($null -ne $script:targetBuildState -and $script:targetBuildState.suspect) {
    Write-Host "[gate] HARNESS-BROKE: every stage passed, but $($script:targetBuildState.reason)" -ForegroundColor Magenta
    Write-Host '[gate] The stages are not evidence about this head. See #455.' -ForegroundColor Magenta
    exit 1
}
Write-Host '[gate] GREEN - every stage passed.' -ForegroundColor Green
if ($SkipPostgres) {
    Write-Host '[gate] Reminder: the PostgreSQL matrix was skipped.' -ForegroundColor Yellow
}
exit 0
