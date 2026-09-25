# #200: the gate's read of SLOT.lock, redesigned to three states discriminated by a `status` tag
# instead of a boolean `present`. Split out of gate.ps1 into its own dot-sourceable file so these
# functions can be unit-tested in isolation (ci/slot-lock.tests.ps1) without executing the rest of
# the gate - gate.ps1 dot-sources this file rather than defining the functions inline.
#
# THE DEFECT THIS REPLACES (full account: design note #200): the old
# Read-SlotLockSnapshot derived the lock's location from $env:CARGO_TARGET_DIR, which is a
# PER-LANE variable (each lane gets its own D:/graphhelm-target-<lane>), while SLOT.lock is a
# MACHINE-WIDE coordination singleton. That was always a category error, not just a stale path -
# and a boolean `present` field could not distinguish "genuinely nobody holds the slot" from "this
# function looked in the wrong place". Both produced a well-formed, confident-looking `false`; a
# populated field reads as evidence even when it is wrong.
#
# THE FIX: $env:GRAPHHELM_SLOT_LOCK_PATH points DIRECTLY at the lock file itself (no directory to
# Join-Path against, which removes a whole class of "right directory, wrong concatenation" bugs -
# the exact shape of the defect this replaces). Three states:
#   present       - the configured path was resolved, checked, and a lock file exists there.
#   absent        - the configured path was resolved AND checked, and genuinely holds no file.
#   indeterminate - the configured path could not be resolved at all (unset), or a structural
#                   tripwire flags it as a known-wrong KIND of place, or it exists but could not
#                   be read. Never collapses into absent - that is the bug this document exists to
#                   close, reinstalled with a new variable name.

# #200 / design doc #4b: the gate cannot observe "canonical", only "the configured path is set".
# A misconfigured GRAPHHELM_SLOT_LOCK_PATH pointing at a retired per-lane target dir would - absent
# this check - report a confident, detailed, PLAUSIBLE `present: true` describing a hold that has
# nothing to do with the run reading it. That is strictly worse than a wrong `absent`: a wrong
# absent reads as "the machine is free"; a wrong present reads as "here is who holds it, and
# since when", and both get believed.
#
# Structural, not a hand-maintained list of known-retired path STRINGS (the class of stale literal
# #190 spent a day naming three separate times): checks whether the configured path resolves to
# somewhere inside a per-lane target-dir SHAPE - literally equal to $env:CARGO_TARGET_DIR if that
# happens to be set, or matching the `graphhelm-target-*` naming convention the factory's own
# decree established. Declared gap, not a hidden one: a misconfiguration that does not match this
# shape (e.g. a typo pointing somewhere structurally unrelated) still slips through.
function Test-SlotLockPathMatchesTargetDirShape {
    param([Parameter(Mandatory)] [string] $LockPath)

    if ($env:CARGO_TARGET_DIR) {
        $lockDir = Split-Path -Path $LockPath -Parent
        if ($lockDir -and ($lockDir.TrimEnd('\', '/') -eq $env:CARGO_TARGET_DIR.TrimEnd('\', '/'))) {
            return $true
        }
    }
    return [bool]($LockPath -match '(?i)[\\/]graphhelm-target-')
}

# #152: SLOT.lock is agent-managed discipline, not something this script owns the lifecycle of -
# it only ever READS whatever is there, as evidence for the manifest.
#
# ORDER-SENSITIVITY THIS FUNCTION MUST KEEP (H's review, #228): every branch below returns a
# straight [ordered]@{ ... } LITERAL - the keys are always assigned in the same fixed order for a
# given status. Test-SlotLockSnapshotsIdentical (below) compares two snapshots by converting each
# to a JSON string and comparing the strings, which is only a safe stand-in for "same content" as
# long as key order can't vary independently of content. A future branch that builds its object
# conditionally - adding or omitting a key based on some check, rather than always assigning every
# key in the same order - would make Gate 9 report two semantically identical snapshots as
# DIFFERENT purely because of key ordering. Keep every branch a flat literal; if a branch ever
# needs conditional keys, Test-SlotLockSnapshotsIdentical needs to compare a sorted/normalized
# form instead, not the raw JSON string.
# #700: the holder pair, read out of the lock's own text.
#
# `slot-claim.sh` (retired 2026-09-24) wrote `holder: pid=<pid> start=<iso8601>` as the last line
# of the claim, the same line New-SlotClaim below writes. The
# snapshot carried that text as `content` and nothing extracted the pair, which is why
# `Test-SlotHolderLiveness` had ZERO non-test callers: the reader had no caller because nothing
# produced its two arguments in-process. The gap was a parse, not a policy.
#
# ABSENT IS NOT MALFORMED, and neither is death. A lock with no holder line returns empty strings,
# which `Test-SlotHolderLiveness` answers 'indeterminate' for -- the promise `slot-claim.sh:226`
# made in writing ("DEGRADES SAFELY: unset -> written empty -> ... 'indeterminate'").
# An unattributable lock must never be declared recoverable.
function Get-SlotHolderPairFromContent {
    param([Parameter(Mandatory)] [AllowEmptyString()] [AllowNull()] [string] $Content)

    $pair = [ordered]@{ pid = ''; startUtc = '' }
    if ([string]::IsNullOrEmpty($Content)) { return $pair }
    # LAST match, not first: the word "holder" appears in the claim's prose above the line that
    # carries the values, and a first-match read would return the prose's non-match or an older
    # line if a lock were ever appended to.
    $matches = [regex]::Matches($Content, 'holder:\s*pid=(\S+)\s+start=(\S+)')
    if ($matches.Count -eq 0) { return $pair }
    $last = $matches[$matches.Count - 1]
    $pair.pid = $last.Groups[1].Value
    $pair.startUtc = $last.Groups[2].Value
    return $pair
}

function Read-SlotLockSnapshot {
    if (-not $env:GRAPHHELM_SLOT_LOCK_PATH) {
        # Unset must NEVER resolve to "no lock" - that folds a third, semantically distinct fact
        # ("nobody told this run where to look") into the same slot as a real observation about
        # the machine. This is the case the orchestrator named explicitly as the one that must not
        # reproduce today's bug.
        return [ordered]@{
            schemaVersion = 2
            status        = 'indeterminate'
            reason        = 'GRAPHHELM_SLOT_LOCK_PATH not set'
            observedAtUtc = [DateTime]::UtcNow.ToString('o')
        }
    }

    $lockPath = $env:GRAPHHELM_SLOT_LOCK_PATH

    if (Test-SlotLockPathMatchesTargetDirShape -LockPath $lockPath) {
        return [ordered]@{
            schemaVersion = 2
            status        = 'indeterminate'
            reason        = 'configured path matches a per-lane target-dir shape (graphhelm-target-*), which is known-wrong for a machine-wide lock - see #200'
            observedAtUtc = [DateTime]::UtcNow.ToString('o')
        }
    }

    if (-not (Test-Path -LiteralPath $lockPath)) {
        return [ordered]@{
            schemaVersion = 2
            status        = 'absent'
            path          = $lockPath
            observedAtUtc = [DateTime]::UtcNow.ToString('o')
        }
    }

    try {
        # [System.IO.File]::ReadAllText, NOT Get-Content -Raw - carried over from the function this
        # replaces: Get-Content attaches PowerShell PROVIDER metadata onto the returned string,
        # which chains into .NET's reflection Type graph and made ConvertTo-Json -Depth 8 hang for
        # multiple minutes with zero error and zero output downstream. A plain .NET file read
        # carries no provider metadata, so there is nothing extra to walk.
        $content = [System.IO.File]::ReadAllText($lockPath)
    } catch {
        # Existence and readability are different facts. A lock that exists but can't be read
        # (permissions, I/O error, or - as exercised in ci/slot-lock.tests.ps1 - a directory sitting
        # at the configured path) is not evidence that nobody holds the slot.
        return [ordered]@{
            schemaVersion = 2
            status        = 'indeterminate'
            reason        = "GRAPHHELM_SLOT_LOCK_PATH exists but could not be read: $($_.Exception.Message)"
            observedAtUtc = [DateTime]::UtcNow.ToString('o')
        }
    }

    # #700: the verdict travels WITH the observation. A reader of this manifest could see a lock
    # and could not tell a live hold from a corpse; now the same record answers both.
    $holder = Get-SlotHolderPairFromContent -Content $content
    return [ordered]@{
        schemaVersion  = 2
        status         = 'present'
        path           = $lockPath
        content        = $content
        holderVerdict  = (Test-SlotHolderLiveness -HolderPid $holder.pid -HolderStartUtc $holder.startUtc)
        observedAtUtc  = [DateTime]::UtcNow.ToString('o')
    }
}

# Gate 9 (L, from re-measuring a real committed manifest): slotLockAtStart and slotLockAtEnd are
# both captured specifically so they COULD be compared. `Test-SlotLockSnapshotsIdentical`
# now does the comparing (gate.ps1), and since #700 each snapshot also carries a
# `holderVerdict`, which the comparison deliberately IGNORES. Identical still means
# the lock's text did not change; the two verdicts sit beside it, so a holder that was already
# dead at both reads and a holder that died between them are distinguishable -- the first is a
# stale lock, the second is a hold that ended mid-run. An
# untouched, stale lock produces identical start/end reads BY CONSTRUCTION; a genuinely live lock
# held across the same span often would not, though a real hold nobody touches for the whole run
# would ALSO read identical - so a match is a tripwire worth surfacing, not a determination.
#
# observedAtUtc is stamped fresh on every Read-SlotLockSnapshot call and is deliberately excluded
# here: every branch of this redesign carries a timestamp (the function this replaces omitted it
# on the `present: false` branches, which is the only reason the real example manifest that
# motivated this check happened to compare byte-identical at all), so without excluding it, two
# reads of a genuinely untouched lock would never register as identical and the field would be
# permanently useless.
#
# ASSUMES FIXED KEY ORDER: compares by JSON string, which is only correct because
# Read-SlotLockSnapshot's branches are flat literals with a fixed key order per status - see the
# comment on that function for what breaks this assumption and how to fix it if it ever needs to.
function Test-SlotLockSnapshotsIdentical {
    param([object] $Start, [object] $End)

    if ($null -eq $Start -or $null -eq $End) {
        return $null
    }

    # Rebuild rather than .Clone(): OrderedDictionary implements Clone() as an EXPLICIT ICloneable
    # member, which PowerShell's method adapter does not surface as a callable instance method -
    # caught live, the direct way, rather than assumed to work.
    # #700 (M, reviewing this change): `holderVerdict` is excluded for the same reason as
    # `observedAtUtc`, and NOT for the same reason as the rest. This comparison answers ONE question --
    # did the lock's own text change between the two reads -- and a verdict is not part of the text.
    # Left in, a holder that DIED mid-run made the pair differ, which reads as "somebody touched the
    # lock": the benign answer, for the least benign event. The verdict travels BESIDE the comparison,
    # where live -> dead is visible as two verdicts rather than hidden as an identity change.
    $ignored = @('observedAtUtc', 'holderVerdict')
    $a = [ordered]@{}
    foreach ($key in $Start.Keys) {
        if ($ignored -notcontains $key) { $a[$key] = $Start[$key] }
    }
    $b = [ordered]@{}
    foreach ($key in $End.Keys) {
        if ($ignored -notcontains $key) { $b[$key] = $End[$key] }
    }

    return (($a | ConvertTo-Json -Depth 8 -Compress) -eq ($b | ConvertTo-Json -Depth 8 -Compress))
}

# #624: the verdict, given a recorded start and whatever the OS reported.
#
# EXTRACTED so the "process exists but its start time is unreadable" case can be tested WITHOUT
# depending on host privilege. The first version of this cell used pid 4 (System), whose StartTime
# is empty for an unprivileged caller -- but readable for a privileged one, so the suite's result
# depended on the account the gate happened to run under. That is a fixture measuring the host, not
# the code. Passing $null directly asks the question the code actually answers. (Codex on #686.)
#
# THE CLOCK HAZARD IS CLOSED BY THE SAME EXACTNESS, and it is worth naming because it is the one
# member of this family that fails toward DEADLOCK rather than toward a wrong FREE. A backward
# clock step could in principle give a NEW process the start time of an old one, so a recycled pid
# would read 'live' and hold the slot forever. At tick precision that requires the clock to land
# within 100 nanoseconds of the recorded instant -- measured, StartTime carries seven decimal
# places. A one-second window would have made this reachable; exact ticks make it not worth a
# guard. Recorded rather than left unexamined, because 'unlikely' and 'considered' are different
# claims and only one of them survives review.
#
# EXACT TICKS, no tolerance window. The recorded value is persisted with 'o' and read back with
# RoundtripKind, which is exact to the tick -- measured: ticks equal after round-trip. A one-second
# window would MERGE two distinct processes whose starts fall inside it, so a recycled pid could
# read 'live' and wedge the stale lock forever: the deadlock direction this issue exists to keep
# open. Tolerance bought nothing and cost the property. (Codex on #686.)
function Get-SlotHolderVerdict {
    param(
        [Parameter(Mandatory)] [datetime] $RecordedUtc,
        [AllowNull()] [object] $ObservedStart
    )
    if ($null -eq $ObservedStart) { return 'indeterminate' }
    $observed = ([datetime] $ObservedStart)
    if ($observed -eq [datetime]::MinValue) { return 'indeterminate' }
    if ($observed.ToUniversalTime().Ticks -eq $RecordedUtc.ToUniversalTime().Ticks) { return 'live' }
    return 'dead'
}

# NOT WIRED YET, and dated so the gap has an age (2026-09-02, K's review of #686). Measured on
# that branch: this function had ZERO non-test callers, and slot-claim.sh (retired 2026-09-24) had
# zero callers of its own. Stale-lock recovery is DEFINED, not connected -- the reader can answer, and
# nothing asks it, because nothing writes a pair either. A reader with no writer and a writer with
# no caller are the same gap from opposite ends, and #700 carries both. The recovery rule itself is
# #619's and deliberately not here.
#
# #624: is the recorded slot holder still alive?
#
# THREE states, matching this file's existing vocabulary rather than inventing a second one:
# 'live', 'dead', 'indeterminate'. The third is load-bearing - a query this instrument cannot
# answer must never read as death, because a wrong 'dead' frees a slot someone is holding, which
# is the exact substitution ED-1 exists to prevent.
#
# THE PAIR IDENTIFIES, THE PID ALONE DOES NOT. Pids recycle; a recycled pid carries a different
# process start time, so the pair is what makes this an identity instead of a guess.
#
# WHY NOT A HEARTBEAT (the design this replaces, #624's own proposal): gate runs here are 22-39
# minutes and blocking, and no daemon is permitted, so nothing would write the beat during
# legitimate long work - the cadence would lapse exactly when the holder is busiest and recovery
# would declare a live holder dead. This asks the OS instead, so there is no cadence to defend and
# no absence to interpret.
#
# WHAT IT DOES NOT MEASURE: progress. A holder that is alive but wedged reads 'live' forever. That
# direction is deliberate - this instrument can never falsely declare death, and a wedge is cleared
# by killing the process, after which the pair reads dead with no rule change.
#
# SINGLE MACHINE ONLY. Get-Process cannot see a holder on another host, where 'not found' would
# read as dead - the dangerous direction. Stated as a precondition, not discovered later.
# #624: does a Get-Process failure STATE that the process is absent, or did the query merely fail?
#
# Extracted so it can be exercised at all. Inline, this branch was unreachable by any cell: the
# tests cannot conjure an access-denied or provider failure on demand, so a sabotage that made
# EVERY query failure read as 'dead' changed no result and the discrimination was uncovered while
# looking covered. A pure function over the error id is the seam that makes it a decision anyone
# can test.
#
# Exactly one id is a statement about the world. Everything else is the instrument failing, and an
# instrument that cannot answer must not answer 'dead'.
function Test-SlotHolderQueryIdMeansAbsent {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $ErrorId)
    return $ErrorId -like 'NoProcessFoundForGivenId*'
}

function Test-SlotHolderLiveness {
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $HolderPid,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $HolderStartUtc
    )

    # A record we cannot parse is a question we cannot ask - never evidence of death.
    # A pid that is not a pid is a question this cannot ask. It was typed [int], so a malformed
    # record failed at PARAMETER BINDING - a fourth outcome, thrown, from a function whose whole
    # contract is three states. The promise has to hold for every input, including the ones a stale
    # or hand-edited lock can carry. (Codex on #686.)
    $pidValue = 0
    if (-not [int]::TryParse($HolderPid, [ref] $pidValue)) { return 'indeterminate' }
    if ($pidValue -le 0) { return 'indeterminate' }

    $recorded = [datetime]::MinValue
    $parsed = [datetime]::TryParse(
        $HolderStartUtc,
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind,
        [ref] $recorded)
    if (-not $parsed) { return 'indeterminate' }
    # A zone-less record is a timestamp plus an assumption: RoundtripKind leaves Kind=Unspecified
    # and ToUniversalTime() would apply whatever offset the machine has TODAY, so the same stored
    # text means different instants across a DST change. The writer refuses these, and the reader
    # refuses them too, because a lock outlives the script that wrote it and may have been written
    # by an older one. Uninterpretable, never 'dead'. (Codex on #686.)
    if ($recorded.Kind -eq [System.DateTimeKind]::Unspecified) { return 'indeterminate' }

    try {
        $process = Get-Process -Id $pidValue -ErrorAction Stop
    }
    catch {
        # MEASURED, and the distinction is the whole point: Get-Process THROWS on an absent pid
        # rather than returning empty. Exactly ONE error id is a statement about the world -
        # NoProcessFoundForGivenId means there is no such process. Every other failure (access
        # denied, provider error) is the QUERY failing, not the holder dying. Catching them all
        # and calling it 'dead' is how an instrument fails into the wrong colour.
        if (Test-SlotHolderQueryIdMeansAbsent -ErrorId $_.FullyQualifiedErrorId) { return 'dead' }
        return 'indeterminate'
    }

    # MEASURED: a process can EXIST while its StartTime is unreadable - pid 4 (System) returns an
    # empty StartTime WITHOUT throwing. Both shapes have to be caught, and neither is death.
    $started = $null
    try { $started = $process.StartTime } catch { $started = $null }

    return Get-SlotHolderVerdict -RecordedUtc $recorded -ObservedStart $started
}

# ============================================================================================
# #700: THE CLAIM AND THE RELEASE. The reader above could always answer live/dead/indeterminate
# about a holder pair; nothing wrote a pair and nothing asked. These four connect both ends,
# as pure functions so ci/slot-lock.tests.ps1 measures them without a gate, a slot, or a repo.
# ============================================================================================

# ONE spelling of where the lock lives. `GRAPHHELM_SLOT_LOCK_PATH` wins when set -- it is what
# Read-SlotLockSnapshot already reads -- otherwise `<slotdir>/SLOT.lock`, the file the retired
# slot-claim.sh claimed. The gate passes its own Get-SlotDir in, so the
# 'D:/graphhelm-slot' default is not spelled a second time here.
function Get-SlotLockPath {
    param([Parameter(Mandatory)] [string] $SlotDir)
    if ($env:GRAPHHELM_SLOT_LOCK_PATH) { return $env:GRAPHHELM_SLOT_LOCK_PATH }
    # Path.Combine, not Join-Path: Join-Path validates the DRIVE and throws DriveNotFound on an
    # unmounted one (gate.ps1 documents the same trap). A path is a string here, not a mount check.
    return [System.IO.Path]::Combine($SlotDir, 'SLOT.lock')
}

# #881 owns the parse (`Get-SlotHolderPairFromContent`, above): last-match, because the word
# "holder" appears in the claim's prose above the line that carries the values. This is the one
# adaptation its callers here need. It returns STRINGS and an empty pair on a miss; a claimant
# identity has to be a number, and a pid that will not parse is one `Test-SlotHolderLiveness`
# could only answer 'indeterminate' about. So: $null unless the pid is a positive integer, and
# every caller below reads one shape instead of three.
function Get-SlotHolderIdentity {
    param([Parameter(Mandatory)] [AllowEmptyString()] [AllowNull()] [string] $Content)
    $pair = Get-SlotHolderPairFromContent -Content $Content
    $value = 0
    if (-not [int]::TryParse([string]$pair.pid, [ref] $value)) { return $null }
    if ($value -le 0) { return $null }
    if ([string]::IsNullOrWhiteSpace([string]$pair.startUtc)) { return $null }
    return [ordered]@{ pid = $value; startUtc = [string]$pair.startUtc }
}

# Create-or-fail, the same primitive the retired slot-claim.sh got from `noclobber`: FileMode.CreateNew is
# refused by the kernel when the file exists, so two claimants can never both believe they won.
# Returns $true on a claim, $false when the create was refused -- and writes NOTHING in the
# second case. UTF-8 without BOM and LF only, so `head -1` in the bash tool reads the same bytes.
function New-SlotClaim {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [int] $HolderPid,
        [Parameter(Mandatory)] [string] $HolderStartUtc,
        [string] $Detail = ''
    )
    $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    $content = "HELD by gate | $stamp | $Detail | STATUS: gate run`n" +
        "Claimed through create-or-fail: the kernel refused every other claimant.`n" +
        "holder: pid=$HolderPid start=$HolderStartUtc`n"
    $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($content)
    $stream = $null
    try {
        $stream = New-Object System.IO.FileStream($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        $stream.Write($bytes, 0, $bytes.Length)
        return $true
    } catch [System.IO.IOException] {
        return $false
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
}

# Release only what is yours. The file's own holder line is read back and its pid compared to the
# caller's; a mismatch releases nothing and returns $false. Deleting another lane's lock is
# stale-lock recovery (#619) and is decided by the liveness reader, never by whoever happens to
# be exiting. Absent file: $false, no throw.
function Remove-SlotClaim {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [int] $HolderPid
    )
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    try {
        $pair = Get-SlotHolderIdentity -Content ([System.IO.File]::ReadAllText($Path))
        if ($null -eq $pair -or $pair.pid -ne $HolderPid) { return $false }
        [System.IO.File]::Delete($Path)
        return $true
    } catch {
        return $false
    }
}

# The gate's entry to the slot: claim it, inherit the wrapper's claim, or wait on a live holder --
# and NEVER free a lock this instrument cannot prove dead.
#
# Returns one of four words, and the gate branches on the word rather than on a boolean, because
# three of them mean "do not run" for three different reasons:
#   claimed     this process wrote the lock; it releases it in its finally
#   inherited   the lock is held by the pair this process was handed in GRAPHHELM_HOLDER_PID/START
#               -- a wrapper's claim (the retired merge checklist: the wrapper claims, then launches). Not
#               ours to release. Parentage is NOT the signal: a detached gate's parent dies.
#   expired     a live or unreadable holder outlasted the budget; nothing was touched
#   path-fault  the create failed and no file exists: a path/permission fault, not contention
#               (the same distinction the retired slot-claim.sh drew before it said "held")
#
# 'dead' -- the pair's process is gone, or a recycled pid carries a different start time -- is the
# one case this function recovers, by deleting and re-claiming, and it says so on the ledger with
# the pair it replaced (#624's reader, #619's rule: only the pair may pronounce death).
# 'indeterminate' -- pid or start unparseable, or the OS would not say -- is waited on exactly as a
# live holder is. A wrong 'free' is the substitution every issue in this file exists to prevent.
#
# SLOT-WAIT is written ONCE per wait, not once per poll: the ledger records that a wait happened
# and how long it was allowed, not a heartbeat.
function Enter-GateSlot {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [Parameter(Mandatory)] [int] $HolderPid,
        [Parameter(Mandatory)] [string] $HolderStartUtc,
        [string] $Detail = '',
        [int] $BudgetSeconds = 10800,
        [int] $PollSeconds = 30,
        [scriptblock] $WriteEvent = { param($Event, $Detail) }
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($BudgetSeconds)
    $waited = $false
    while ($true) {
        if (New-SlotClaim -Path $Path -HolderPid $HolderPid -HolderStartUtc $HolderStartUtc -Detail $Detail) { return 'claimed' }
        if (-not (Test-Path -LiteralPath $Path)) { return 'path-fault' }

        $content = ''
        try { $content = [System.IO.File]::ReadAllText($Path) } catch { $content = '' }
        $pair = Get-SlotHolderIdentity -Content $content

        if ($null -ne $pair) {
            $envPid = 0
            $handed = [int]::TryParse([string]$env:GRAPHHELM_HOLDER_PID, [ref] $envPid) -and
                $envPid -eq $pair.pid -and
                [string]::Equals([string]$env:GRAPHHELM_HOLDER_START, $pair.startUtc, [System.StringComparison]::Ordinal)
            $verdict = Test-SlotHolderLiveness -HolderPid "$($pair.pid)" -HolderStartUtc $pair.startUtc
            if ($handed -and $verdict -eq 'live') { return 'inherited' }
            if ($verdict -eq 'dead') {
                & $WriteEvent 'SLOT-RECLAIM' "dead-holder pid=$($pair.pid) start=$($pair.startUtc) path=$Path"
                try { [System.IO.File]::Delete($Path) } catch { }
                continue
            }
        } else {
            $verdict = 'indeterminate'
        }

        if (-not $waited) {
            $who = if ($null -ne $pair) { "pid=$($pair.pid) start=$($pair.startUtc)" } else { 'no readable holder pair' }
            & $WriteEvent 'SLOT-WAIT' "holder $who verdict=$verdict budgetSeconds=$BudgetSeconds path=$Path"
            $waited = $true
        }
        $remaining = ($deadline - [DateTime]::UtcNow).TotalSeconds
        if ($remaining -le 0) {
            & $WriteEvent 'SLOT-WAIT-EXPIRED' "verdict=$verdict budgetSeconds=$BudgetSeconds path=$Path"
            return 'expired'
        }
        Start-Sleep -Seconds ([Math]::Max(1, [Math]::Min($PollSeconds, [Math]::Ceiling($remaining))))
    }
}
