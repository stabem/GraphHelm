# #200: isolated tests for ci/slot-lock.ps1 - the redesigned three-state read of SLOT.lock.
#
# Dot-sources ONLY slot-lock.ps1, never gate.ps1. These functions have no cargo dependency, no
# slot dependency, and no repository dependency - they read one env var and (at most) one file
# under a throwaway temp directory this script creates and removes itself. Writing and running
# this file needs no machine coordination; see design note #200 for the sealed
# predictions these cases were written FROM, before any of ci/slot-lock.ps1 existed.
#
# Homegrown PASS/FAIL harness, not Pester: this repository has never carried a Pester dependency
# (checked: zero *.Tests.ps1 files anywhere in history before this one), and introducing a new
# test-framework dependency for one issue's worth of pure functions is scope the fix does not need.

# H's review (#228): PASS/FAIL alone cannot tell a complete run from a partially-vanished one - a
# block silently dropped by a bad merge, commented out, or lost in a refactor still leaves every
# REMAINING assertion passing, and "18/18 passed" reads exactly as healthy as "24/24 passed" to
# anyone who doesn't independently know 24 is the real count. A declared expected total plus a
# THIRD, distinct outcome for a mismatch is the same PASS/FAIL/HARNESS-BROKE discipline gate.ps1
# itself already uses for GREEN/RED/ABORTED-BY-CANARY - the harness proving it ran everything it
# was supposed to, not just that everything it ran happened to be green.
#
# 24, not the 27 a naive `grep -c "Assert-True|Assert-Equal"` over this file returns: that count
# includes the two FUNCTION DEFINITION lines (Assert-True, Assert-Equal - not calls) and Assert-
# Equal's own internal delegation to Assert-True (below, in the function body) - source text the
# same pattern matches once, but which fires once per Assert-Equal CALL at runtime, not as an
# independent assertion beyond the call that triggers it. 13 direct Assert-True calls + 11
# Assert-Equal calls = 24 actual runtime assertions; that's the number this harness itself counts.
$ExpectedAssertionCount = 58

$ErrorActionPreference = 'Stop'
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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected [$Expected], got [$Actual])"
}

function New-TempTestDir {
    # $Name becomes the START of the directory's own basename, not a suffix fused onto a random
    # prefix - Test-SlotLockPathMatchesTargetDirShape matches a path SEGMENT that starts with
    # `graphhelm-target-`, the same shape a real per-lane dir has (`D:/graphhelm-target-e163`),
    # so a fixture meant to trigger it has to be named that way for real, not just contain the
    # substring somewhere inside a longer unrelated segment.
    param([Parameter(Mandatory)] [string] $Name)
    $dir = Join-Path ([System.IO.Path]::GetTempPath()) "$Name-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    [System.IO.Directory]::CreateDirectory($dir) | Out-Null
    return $dir
}

. (Join-Path $PSScriptRoot 'slot-lock.ps1')

$prevCargoTargetDir = $env:CARGO_TARGET_DIR
$prevLockPath = $env:GRAPHHELM_SLOT_LOCK_PATH
$cleanupDirs = @()

try {

    # --- Fixture A: correctly-configured present, independent of CARGO_TARGET_DIR ---------------
    # Design doc Fixture A: a real lock at the canonical location, CARGO_TARGET_DIR pointed at an
    # unrelated per-lane dir holding no lock at all. Old code answered `present: false` here -
    # confident and wrong. The fix must answer `present`, and must do so REGARDLESS of what
    # CARGO_TARGET_DIR happens to be, which is the proof the two are actually decoupled.
    Write-Host "`n=== Fixture A: correctly-configured present, independent of CARGO_TARGET_DIR ==="
    $canonicalDir = New-TempTestDir -Name 'canonical-slot'
    $cleanupDirs += $canonicalDir
    $lockFile = Join-Path $canonicalDir 'SLOT.lock'
    $lockContent = 'X | 2026-08-20T14:00:00Z | lane #200 | STATUS: RUNNING'
    [System.IO.File]::WriteAllText($lockFile, $lockContent, (New-Object System.Text.UTF8Encoding($false)))

    $perLaneDir = New-TempTestDir -Name 'graphhelm-target-e163'
    $cleanupDirs += $perLaneDir
    $env:CARGO_TARGET_DIR = $perLaneDir
    $env:GRAPHHELM_SLOT_LOCK_PATH = $lockFile

    $resultA = Read-SlotLockSnapshot

    Assert-Equal -Expected 'present' -Actual $resultA.status -Message 'Fixture A: status is present'
    Assert-Equal -Expected $lockFile -Actual $resultA.path -Message 'Fixture A: path echoes the configured file'
    Assert-Equal -Expected $lockContent -Actual $resultA.content -Message 'Fixture A: content matches the real lock file'
    Assert-Equal -Expected 2 -Actual $resultA.schemaVersion -Message 'Fixture A: schemaVersion is 2'
    Assert-True -Condition ($null -ne $resultA.observedAtUtc) -Message 'Fixture A: observedAtUtc is stamped'

    # --- #700: the snapshot answers WHOSE lock it is, not just that there is one ----------------
    #
    # Read-SlotLockSnapshot returned the lock's raw `content` and nothing extracted the holder pair
    # from it, so Test-SlotHolderLiveness had zero non-test callers: the reader had no caller because
    # nothing produced its two arguments in-process. These cells pin the parse and the verdict.
    Write-Host "`n=== #700: holderVerdict on a present snapshot ==="
    $me = Get-Process -Id $PID
    $meStart = $me.StartTime.ToUniversalTime().ToString('o')

    # (1) LIVE: a lock whose holder line names a process that is running, with the start time that
    # identifies it. A held slot whose owner is alive must never read as recoverable.
    Set-Content -LiteralPath $lockFile -Encoding utf8 -Value @(
        'HELD by tester | stamp | lane | STATUS: working',
        'cargo/rustc alive at claim: 0',
        "holder: pid=$PID start=$meStart")
    $live = Read-SlotLockSnapshot
    Assert-Equal -Expected 'present' -Actual $live.status -Message '#700 live: status is still present'
    Assert-Equal -Expected 'live' -Actual $live.holderVerdict -Message '#700: a running holder reads live'

    # (2) DEAD by recycled pid: the same pid, a different start. The pair is the identity; the pid
    # alone would hold the slot hostage after the owner exited and its number was reused.
    $shiftedStart = $me.StartTime.ToUniversalTime().AddHours(-1).ToString('o')
    Set-Content -LiteralPath $lockFile -Encoding utf8 -Value @(
        'HELD by tester | stamp | lane | STATUS: working',
        "holder: pid=$PID start=$shiftedStart")
    Assert-Equal -Expected 'dead' -Actual (Read-SlotLockSnapshot).holderVerdict -Message '#700: a recycled pid reads dead'

    # (3) INDETERMINATE by omission: a lock with no holder line at all. The retired slot-claim.sh's
    # own comment promised this degrades to indeterminate rather than to dead -- an unattributable lock must
    # never be declared recoverable, which is the fail-CLOSED direction.
    Set-Content -LiteralPath $lockFile -Encoding utf8 -Value @(
        'HELD by tester | stamp | lane | STATUS: working',
        'cargo/rustc alive at claim: 0')
    Assert-Equal -Expected 'indeterminate' -Actual (Read-SlotLockSnapshot).holderVerdict -Message '#700: no holder line reads indeterminate, never dead'

    # (4) The verdict is BESIDE the identity comparison, not inside it (#700, M's review). A holder
    # that dies mid-run must not make the snapshots differ: that would read as "somebody touched the
    # lock", the benign answer, for the least benign event.
    $sA = [ordered]@{ schemaVersion = 2; status = 'present'; path = 'x'; content = 'same text'; holderVerdict = 'live'; observedAtUtc = '2026-01-01T00:00:00.0000000Z' }
    $sB = [ordered]@{ schemaVersion = 2; status = 'present'; path = 'x'; content = 'same text'; holderVerdict = 'dead'; observedAtUtc = '2026-01-01T00:05:00.0000000Z' }
    Assert-True -Condition (Test-SlotLockSnapshotsIdentical -Start $sA -End $sB) -Message '#700: a holder dying mid-run leaves the lock text identical'

    # --- Fixture B: unset GRAPHHELM_SLOT_LOCK_PATH is indeterminate, never absent ---------------
    # This is the case the orchestrator named as the one that must not reproduce today's bug: an
    # unset dedicated variable is "nobody told this run where to look", not "no lock".
    Write-Host "`n=== Fixture B: unset GRAPHHELM_SLOT_LOCK_PATH is indeterminate, never absent ==="
    Remove-Item Env:\GRAPHHELM_SLOT_LOCK_PATH -ErrorAction SilentlyContinue

    $resultB = Read-SlotLockSnapshot

    Assert-Equal -Expected 'indeterminate' -Actual $resultB.status -Message 'Fixture B: status is indeterminate, not absent'
    Assert-Equal -Expected 'GRAPHHELM_SLOT_LOCK_PATH not set' -Actual $resultB.reason -Message 'Fixture B: reason names the unset variable'
    Assert-Equal -Expected 2 -Actual $resultB.schemaVersion -Message 'Fixture B: schemaVersion is 2'
    # The sabotage this fixture exists to catch (design doc #4): collapsing `indeterminate` back
    # into `status: 'absent'` as a "simplification". This assertion is the trap - it goes red the
    # moment that collapse happens, because 'absent' will never equal 'indeterminate'.
    Assert-True -Condition ($resultB.status -ne 'absent') -Message 'Fixture B sabotage guard: never silently reads as absent'

    # --- Fixture C: configured-and-wrong (structural tripwire) ----------------------------------
    # Design doc Fixture C / #4b: GRAPHHELM_SLOT_LOCK_PATH set, file exists, file is READABLE and
    # parseable - and STILL must not answer `present`, because the configured path matches the
    # exact shape (`graphhelm-target-*`) already proven wrong for a machine-wide lock. Content is
    # representative of the real stale record the orchestrator preserved (see
    # design note #200, Fixture C, for the sha256-verified snapshot this is modeled
    # on); this test seeds its own bytes rather than reading that snapshot file directly, so the
    # test stays hermetic on a machine that doesn't have it.
    Write-Host "`n=== Fixture C: configured path matches target-dir shape -> indeterminate, never present ==="
    $testTargetDir = New-TempTestDir -Name 'graphhelm-target-e200'
    $cleanupDirs += $testTargetDir
    $wrongLockFile = Join-Path $testTargetDir 'SLOT.lock'
    $staleContent = "HELD by A Agent | lane #160 (issue-160-event-family-readiness) | STATUS: RUNNING`nNOTE: this file records POSSESSION, not ORDER.`n"
    [System.IO.File]::WriteAllText($wrongLockFile, $staleContent, (New-Object System.Text.UTF8Encoding($false)))
    $env:GRAPHHELM_SLOT_LOCK_PATH = $wrongLockFile

    $resultC = Read-SlotLockSnapshot

    Assert-Equal -Expected 'indeterminate' -Actual $resultC.status -Message 'Fixture C: status is indeterminate, never present, despite a real readable file'
    Assert-True -Condition ($resultC.reason -like '*graphhelm-target-*') -Message 'Fixture C: reason names the structural signal that fired'
    # The sabotage this fixture exists to catch: removing the structural check so any configured
    # path that merely exists and parses is trusted. Fixture A and B's arrangements never exercise
    # a configured-but-wrong path, so only this one goes red when that check disappears.
    Assert-True -Condition ($resultC.status -ne 'present') -Message 'Fixture C sabotage guard: a real readable file at a known-wrong path is never trusted'

    # --- Baseline: genuinely absent (no fixture in the design doc covers this cell directly) ----
    Write-Host "`n=== Baseline: configured, non-target-dir-shaped path, nothing there -> absent ==="
    $cleanDir = New-TempTestDir -Name 'clean-slot'
    $cleanupDirs += $cleanDir
    $neverCreated = Join-Path $cleanDir 'SLOT.lock'
    $env:GRAPHHELM_SLOT_LOCK_PATH = $neverCreated

    $resultAbsent = Read-SlotLockSnapshot

    Assert-Equal -Expected 'absent' -Actual $resultAbsent.status -Message 'Baseline: genuinely nothing there reads as absent'
    Assert-Equal -Expected $neverCreated -Actual $resultAbsent.path -Message 'Baseline: absent still echoes the path that was checked'

    # --- Baseline: configured path exists but cannot be read as a file -> indeterminate ---------
    # A directory at the configured path throws on File.ReadAllText - existence and readability
    # are different facts, and a lock that exists but can't be read is not evidence nobody holds
    # the slot (design doc #2).
    Write-Host "`n=== Baseline: unreadable path -> indeterminate, never a false absent ==="
    $dirAsLock = New-TempTestDir -Name 'not-a-file'
    $cleanupDirs += $dirAsLock
    $env:GRAPHHELM_SLOT_LOCK_PATH = $dirAsLock

    $resultUnreadable = Read-SlotLockSnapshot

    Assert-Equal -Expected 'indeterminate' -Actual $resultUnreadable.status -Message 'Baseline: unreadable path is indeterminate, never a false absent'
    Assert-True -Condition ($resultUnreadable.reason -like '*could not be read*') -Message 'Baseline: reason explains the read failure'

    # --- Test-SlotLockPathMatchesTargetDirShape, directly -----------------------------------------
    Write-Host "`n=== Test-SlotLockPathMatchesTargetDirShape ==="
    Assert-True -Condition (Test-SlotLockPathMatchesTargetDirShape -LockPath 'D:/graphhelm-target-m10/SLOT.lock') -Message 'matches the literal retired path'
    Assert-True -Condition (Test-SlotLockPathMatchesTargetDirShape -LockPath 'D:/graphhelm-target-e163/SLOT.lock') -Message 'matches any per-lane target dir, not just the one that bit us'
    Assert-True -Condition (-not (Test-SlotLockPathMatchesTargetDirShape -LockPath 'D:/graphhelm-slot/SLOT.lock')) -Message 'the canonical location itself does not false-positive'
    Assert-True -Condition (-not (Test-SlotLockPathMatchesTargetDirShape -LockPath 'D:/graphhelm-slot/retired-target-m10-SLOT.lock.snapshot')) -Message 'declared gap (design 4b): the snapshot own path does not match - known, accepted limitation, not a new failure'

    # --- Gate 9: Test-SlotLockSnapshotsIdentical ---------------------------------------------------
    # L's measurement: the manifest already carries both reads specifically so they COULD be
    # compared. observedAtUtc is stamped fresh on every call and MUST be excluded, or two reads of
    # an untouched lock would never register as identical purely because of the clock - which
    # would make this field permanently useless the moment every branch gained a timestamp.
    Write-Host "`n=== Gate 9: Test-SlotLockSnapshotsIdentical ignores observedAtUtc, respects everything else ==="
    $snapA = [ordered]@{ schemaVersion = 2; status = 'present'; path = 'X'; content = 'same'; observedAtUtc = '2026-08-20T16:10:05Z' }
    $snapB = [ordered]@{ schemaVersion = 2; status = 'present'; path = 'X'; content = 'same'; observedAtUtc = '2026-08-20T16:38:51Z' }
    Assert-True -Condition (Test-SlotLockSnapshotsIdentical -Start $snapA -End $snapB) -Message 'identical content, different timestamp, still identical'

    $snapDiffContent = [ordered]@{ schemaVersion = 2; status = 'present'; path = 'X'; content = 'DIFFERENT'; observedAtUtc = '2026-08-20T16:38:51Z' }
    Assert-True -Condition (-not (Test-SlotLockSnapshotsIdentical -Start $snapA -End $snapDiffContent)) -Message 'different content is correctly NOT identical'

    $snapDiffStatus = [ordered]@{ schemaVersion = 2; status = 'absent'; path = 'X'; observedAtUtc = '2026-08-20T16:38:51Z' }
    Assert-True -Condition (-not (Test-SlotLockSnapshotsIdentical -Start $snapA -End $snapDiffStatus)) -Message 'different status (present vs absent) is correctly NOT identical'

    Assert-True -Condition ($null -eq (Test-SlotLockSnapshotsIdentical -Start $null -End $snapB)) -Message 'a missing snapshot on either side reports unknown, not a guess'

} finally {
    foreach ($dir in $cleanupDirs) {
        Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
    }
    $env:CARGO_TARGET_DIR = $prevCargoTargetDir
    if ($null -eq $prevLockPath) {
        Remove-Item Env:\GRAPHHELM_SLOT_LOCK_PATH -ErrorAction SilentlyContinue
    } else {
        $env:GRAPHHELM_SLOT_LOCK_PATH = $prevLockPath
    }
}

    # THE WRITER'S CELLS LIVE IN BASH, NOT HERE, and that is measured rather than preferred.
    # They shelled out to bash from this suite and passed for me -- because I ran them from an MSYS
    # shell, where `bash` is Git's. From PowerShell, which is how the authoritative gate runs, the
    # name resolves to C:\WINDOWS\system32ash.exe: WSL's bash, a different filesystem mapping
    # (/mnt/c, not /c) and not necessarily installed. The cells were green for an environment-
    # specific reason and would have failed in the gate. (Codex on #686 raised presence; the
    # measurement found identity, which is worse.)
    #
    # So the gate stays PowerShell-only and the writer was covered by
    # slot-holder.tests.sh, run by hand, until it was retired on 2026-09-24. That coverage was NOT
    # GATED, and #700 carried it alongside the reader's missing consumer.

    # --- #624: the query-failure classification, extracted so it can be reached at all ----------
    Write-Host ""
    Write-Host "=== #624: Test-SlotHolderQueryIdMeansAbsent ==="
    # Only this id is a STATE. A sabotage making every failure read 'dead' passed every liveness
    # cell before this pair existed - the branch was uncovered while appearing covered.
    Assert-True -Condition (Test-SlotHolderQueryIdMeansAbsent -ErrorId 'NoProcessFoundForGivenId,Microsoft.PowerShell.Commands.GetProcessCommand') -Message '#624 NoProcessFoundForGivenId states the process is absent'
    Assert-True -Condition (-not (Test-SlotHolderQueryIdMeansAbsent -ErrorId 'PermissionDenied,Microsoft.PowerShell.Commands.GetProcessCommand')) -Message '#624 a permission failure is the QUERY failing, never a dead holder'

    # --- #624: holder liveness is a PAIR, and the third state is not "dead" -----------------------
    Write-Host ""
    Write-Host "=== #624: Test-SlotHolderLiveness ==="
    $self = Get-Process -Id $PID
    $selfStart = $self.StartTime.ToUniversalTime().ToString('o')

    # (1) LIVE: the same pair. Taking a slot whose holder is alive must be forbidden, so this
    # must read 'live' - not merely "did not crash".
    Assert-Equal -Expected 'live' -Actual (Test-SlotHolderLiveness -HolderPid $PID -HolderStartUtc $selfStart) -Message '#624 same pid AND same start time reads live'

    # (2) DEAD by recycled pid: same pid, DIFFERENT start time. This is the cell that proves the
    # pair identifies and the pid alone does not. Without it a recycled pid would hold the slot
    # hostage forever - the deadlock side of the question.
    $shifted = $self.StartTime.ToUniversalTime().AddHours(-1).ToString('o')
    Assert-Equal -Expected 'dead' -Actual (Test-SlotHolderLiveness -HolderPid $PID -HolderStartUtc $shifted) -Message '#624 same pid but a different start time reads dead (pid recycled)'

    # (3) DEAD by absence: no such process. Get-Process THROWS here rather than returning empty,
    # and the error id NoProcessFoundForGivenId is what makes this a STATE and not a query failure.
    # A pid that genuinely existed and is gone, obtained by RETRY rather than by hoping.
    #
    # Three attempts at this cell, and each failed differently -- worth recording because the shape
    # repeats: 999999 can be live on a busy host (green via the mismatched-start-time branch, never
    # touching absence); a spawned-then-reaped pid can be REASSIGNED during the wait (same wrong
    # branch, narrower window); and -1, which I reached for next, returns 'indeterminate' because
    # this reader's own guard rejects non-positive pids before Get-Process is called. My own
    # validation blocked the shortcut. (Codex on #686, twice on this cell.)
    #
    # So: reap, then VERIFY absence, and retry if the pid came back. Bounded, and it declares
    # HARNESS-BROKE rather than measuring the wrong branch if it never gets a free pid.
    # Representativeness lives in Test-SlotHolderQueryIdMeansAbsent, which asserts on the real
    # error id; determinism lives here. One cell carrying both is what made this flaky.
    $absentPid = 0
    foreach ($attempt in 1..8) {
        $spawn = Start-Process -FilePath cmd -ArgumentList '/c', 'exit' -PassThru -WindowStyle Hidden
        $candidate = $spawn.Id
        $spawn.WaitForExit()
        Start-Sleep -Milliseconds 120
        if (-not (Get-Process -Id $candidate -ErrorAction SilentlyContinue)) { $absentPid = $candidate; break }
    }
    Assert-True -Condition ($absentPid -gt 0) `
        -Message '#624 ARRANGEMENT: a reaped pid stayed free long enough to measure, or this cell measures nothing'
    Assert-Equal -Expected 'dead' -Actual (Test-SlotHolderLiveness -HolderPid "$absentPid" -HolderStartUtc $selfStart) -Message '#624 a pid whose process has exited reads dead'

    # (4c) A pid that is not a pid must still answer one of the three states, never throw. It was
    # typed [int], so a malformed record failed at PARAMETER BINDING - a fourth outcome from a
    # three-state contract (Codex on #686).
    Assert-Equal -Expected 'indeterminate' -Actual (Test-SlotHolderLiveness -HolderPid 'abc' -HolderStartUtc $selfStart) -Message '#624 a non-numeric recorded pid is indeterminate, never a thrown binding error'

    # (4d) A zone-less record is a timestamp plus an ASSUMPTION: RoundtripKind leaves
    # Kind=Unspecified and ToUniversalTime() applies whatever offset the machine has today, so the
    # same stored text means different instants across a DST change and a live holder would read
    # 'dead' after the clock shifted. Uninterpretable, never dead. (Codex on #686.)
    $zoneless = (Get-Process -Id $PID).StartTime.ToString('yyyy-MM-ddTHH:mm:ss.fffffff')
    Assert-Equal -Expected 'indeterminate' -Actual (Test-SlotHolderLiveness -HolderPid "$PID" -HolderStartUtc $zoneless) -Message '#686 a start time with no UTC offset is indeterminate, never dead'

    # (5) INDETERMINATE on a malformed record: an unparseable stored start time is a query the
    # instrument cannot answer, not evidence the holder died.
    Assert-Equal -Expected 'indeterminate' -Actual (Test-SlotHolderLiveness -HolderPid $PID -HolderStartUtc 'not-a-timestamp') -Message '#624 an unparseable recorded start time is indeterminate, NEVER dead'

# ============================================================================================
# #700: THE CLAIM AND THE RELEASE, as pure functions the gate can call and this file can measure.
# Each cell below is guarded so an ABSENT function reads as a failed assertion, not a crash:
# red at the assertion, or it is not red.
# ============================================================================================
function Test-FnPresent { param([string] $Name) return [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

Write-Host "`n=== #700 (b) Get-SlotLockPath: one spelling of where the lock lives ==="
$prev700 = $env:GRAPHHELM_SLOT_LOCK_PATH
try {
    if (Test-FnPresent 'Get-SlotLockPath') {
        Remove-Item Env:\GRAPHHELM_SLOT_LOCK_PATH -ErrorAction SilentlyContinue
        # Path.Combine, not Join-Path: Join-Path validates the DRIVE and throws DriveNotFound for X: here.
        Assert-Equal -Expected ([System.IO.Path]::Combine('X:\some-slot', 'SLOT.lock')) -Actual (Get-SlotLockPath -SlotDir 'X:\some-slot') -Message '#700 (b1) unset env: <slotdir>\SLOT.lock, the file the retired slot-claim.sh used'
        $env:GRAPHHELM_SLOT_LOCK_PATH = 'Y:\override\the.lock'
        Assert-Equal -Expected 'Y:\override\the.lock' -Actual (Get-SlotLockPath -SlotDir 'X:\some-slot') -Message '#700 (b2) GRAPHHELM_SLOT_LOCK_PATH wins when set, so tests and the reader agree on the path'
    } else { 1..2 | ForEach-Object { Assert-True -Condition $false -Message "#700 (b$_) Get-SlotLockPath is absent" } }
} finally { if ($null -ne $prev700) { $env:GRAPHHELM_SLOT_LOCK_PATH = $prev700 } else { Remove-Item Env:\GRAPHHELM_SLOT_LOCK_PATH -ErrorAction SilentlyContinue } }

Write-Host "`n=== #700 (c) New-SlotClaim: create-or-fail, the pair embedded, bytes bash can read ==="
$claimDir = New-TempTestDir -Name 'slot-claim-700'
$claimPath = Join-Path $claimDir 'SLOT.lock'
if (Test-FnPresent 'New-SlotClaim') {
    $first = New-SlotClaim -Path $claimPath -HolderPid 4242 -HolderStartUtc '2026-09-05T02:25:29.5209025Z' -Detail 'gate ISSUES-4'
    $readBack = if (Test-FnPresent 'Get-SlotHolderIdentity') { Get-SlotHolderIdentity -Content ([System.IO.File]::ReadAllText($claimPath)) } else { $null }
    Assert-True -Condition ($first -and $null -ne $readBack -and [string]$readBack.pid -eq '4242') -Message '#700 (c1) a claim on a free path succeeds and its own pair reads back through the parser'
    $bytesBefore = [System.IO.File]::ReadAllBytes($claimPath)
    $second = New-SlotClaim -Path $claimPath -HolderPid 9999 -HolderStartUtc '2026-09-05T03:00:00.0000000Z' -Detail 'a second claimant'
    Assert-True -Condition (-not $second) -Message '#700 (c2) a claim on a held path returns false: create-or-fail, the kernel refused'
    $bytesAfter = [System.IO.File]::ReadAllBytes($claimPath)
    Assert-True -Condition ([System.Linq.Enumerable]::SequenceEqual([byte[]]$bytesBefore, [byte[]]$bytesAfter)) -Message '#700 (c3) the refused claim wrote NOTHING: the holder file is byte-identical'
    Assert-True -Condition (($bytesBefore.Length -ge 3) -and -not ($bytesBefore[0] -eq 0xEF -and $bytesBefore[1] -eq 0xBB -and $bytesBefore[2] -eq 0xBF) -and -not ([System.Text.Encoding]::UTF8.GetString($bytesBefore).Contains("`r"))) -Message '#700 (c4) no BOM and LF only, so head -1 (as the retired slot-claim.sh used it) reads the same bytes'
} else { 1..4 | ForEach-Object { Assert-True -Condition $false -Message "#700 (c$_) New-SlotClaim is absent" } }

Write-Host "`n=== #700 (d) Remove-SlotClaim: release only what is yours ==="
if (Test-FnPresent 'Remove-SlotClaim') {
    $foreign = Remove-SlotClaim -Path $claimPath -HolderPid 9999
    Assert-True -Condition ((-not $foreign) -and (Test-Path -LiteralPath $claimPath)) -Message '#700 (d1) a pid that is not the holder cannot release: returns false, file intact (never delete another lane''s lock)'
    $own = Remove-SlotClaim -Path $claimPath -HolderPid 4242
    Assert-True -Condition ($own -and -not (Test-Path -LiteralPath $claimPath)) -Message '#700 (d2) the holder releases: file gone, true returned'
    $gone = Remove-SlotClaim -Path $claimPath -HolderPid 4242
    Assert-True -Condition (-not $gone) -Message '#700 (d3) releasing an absent lock is false, never a throw'
} else { 1..3 | ForEach-Object { Assert-True -Condition $false -Message "#700 (d$_) Remove-SlotClaim is absent" } }
Remove-Item -LiteralPath $claimDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "`n=== #700 (e) Enter-GateSlot: wait on a live or unreadable holder, reclaim a dead one, never guess ==="
$enterDir = New-TempTestDir -Name 'slot-enter-700'
$enterPath = Join-Path $enterDir 'SLOT.lock'
$events = New-Object System.Collections.Generic.List[string]
$record = { param($Event, $Detail) $events.Add("$Event|$Detail") }
$selfPid = $PID
$selfStart = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
$holder = $null
try {
    if (Test-FnPresent 'Enter-GateSlot') {
        # (e1) free path -> claimed, and the lock carries OUR pair
        $r1 = Enter-GateSlot -Path $enterPath -HolderPid $selfPid -HolderStartUtc $selfStart -Detail 'e1' -BudgetSeconds 3 -PollSeconds 1 -WriteEvent $record
        $p1 = Get-SlotHolderIdentity -Content ([System.IO.File]::ReadAllText($enterPath))
        Assert-True -Condition ($r1 -eq 'claimed' -and $null -ne $p1 -and $p1.pid -eq $selfPid) -Message '#700 (e1) a free slot is claimed and the lock names this process'
        Remove-SlotClaim -Path $enterPath -HolderPid $selfPid | Out-Null

        # (e2) a LIVE holder: a real process, its real pair -> expired, lock untouched, WAIT logged once
        $holder = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120' -PassThru -WindowStyle Hidden
        Start-Sleep -Milliseconds 300
        $hStart = (Get-Process -Id $holder.Id).StartTime.ToUniversalTime().ToString('o')
        New-SlotClaim -Path $enterPath -HolderPid $holder.Id -HolderStartUtc $hStart -Detail 'live holder' | Out-Null
        $bytesLive = [System.IO.File]::ReadAllBytes($enterPath)
        $events.Clear()
        $r2 = Enter-GateSlot -Path $enterPath -HolderPid $selfPid -HolderStartUtc $selfStart -Detail 'e2' -BudgetSeconds 3 -PollSeconds 1 -WriteEvent $record
        Assert-True -Condition ($r2 -eq 'expired') -Message '#700 (e2a) a live holder is waited on, and an exhausted budget answers expired - never claimed'
        Assert-True -Condition ([System.Linq.Enumerable]::SequenceEqual([byte[]]$bytesLive, [byte[]][System.IO.File]::ReadAllBytes($enterPath))) -Message '#700 (e2b) the live holder lock is byte-identical: waiting touched nothing'
        $waits = @($events | Where-Object { $_ -like 'SLOT-WAIT|*' }).Count
        $expired = @($events | Where-Object { $_ -like 'SLOT-WAIT-EXPIRED|*' }).Count
        Assert-True -Condition ($waits -eq 1 -and $expired -eq 1) -Message "#700 (e2c) the ledger gets ONE SLOT-WAIT (not one per poll) and ONE SLOT-WAIT-EXPIRED (got $waits/$expired)"
        Stop-Process -Id $holder.Id -Force -ErrorAction SilentlyContinue; $holder.WaitForExit(5000) | Out-Null; $holder = $null
        [System.IO.File]::Delete($enterPath)

        # (e3) a DEAD pair: a pid nothing owns, a start it never had -> reclaimed, RECLAIM logged with the dead pair
        $absent = 0; foreach ($c in 70000..70400) { if (-not (Get-Process -Id $c -ErrorAction SilentlyContinue)) { $absent = $c; break } }
        New-SlotClaim -Path $enterPath -HolderPid $absent -HolderStartUtc '2026-09-05T00:00:00.0000000Z' -Detail 'dead holder' | Out-Null
        $events.Clear()
        $r3 = Enter-GateSlot -Path $enterPath -HolderPid $selfPid -HolderStartUtc $selfStart -Detail 'e3' -BudgetSeconds 3 -PollSeconds 1 -WriteEvent $record
        $p3 = Get-SlotHolderIdentity -Content ([System.IO.File]::ReadAllText($enterPath))
        Assert-True -Condition ($r3 -eq 'claimed' -and $null -ne $p3 -and $p3.pid -eq $selfPid) -Message '#700 (e3a) a pair the OS says is gone is reclaimed: the lock now names this process'
        Assert-True -Condition (@($events | Where-Object { $_ -like "SLOT-RECLAIM|*pid=$absent*" }).Count -eq 1) -Message '#700 (e3b) the reclaim is on the ledger, naming the dead pair it replaced'
        Remove-SlotClaim -Path $enterPath -HolderPid $selfPid | Out-Null

        # (e4) INDETERMINATE: our own live pid with an unparseable start -> never dead, never freed
        [System.IO.File]::WriteAllText($enterPath, "HELD by someone`nholder: pid=$selfPid start=not-a-timestamp`n", (New-Object System.Text.UTF8Encoding($false)))
        $bytesInd = [System.IO.File]::ReadAllBytes($enterPath)
        $r4 = Enter-GateSlot -Path $enterPath -HolderPid $selfPid -HolderStartUtc $selfStart -Detail 'e4' -BudgetSeconds 2 -PollSeconds 1 -WriteEvent $record
        Assert-True -Condition ($r4 -eq 'expired' -and [System.Linq.Enumerable]::SequenceEqual([byte[]]$bytesInd, [byte[]][System.IO.File]::ReadAllBytes($enterPath))) -Message '#700 (e4) an unreadable holder is waited on and left intact: indeterminate never reads as free'
        [System.IO.File]::Delete($enterPath)

        # (e5) a path fault is not contention: no directory -> path-fault at once, nothing to wait for
        $r5 = Enter-GateSlot -Path (Join-Path $enterDir 'no-such-dir\SLOT.lock') -HolderPid $selfPid -HolderStartUtc $selfStart -Detail 'e5' -BudgetSeconds 3 -PollSeconds 1 -WriteEvent $record
        Assert-True -Condition ($r5 -eq 'path-fault') -Message '#700 (e5) a create that fails with NO file present is a path fault, answered immediately, not a 3-second wait'
    } else { 1..8 | ForEach-Object { Assert-True -Condition $false -Message "#700 (e$_) Enter-GateSlot is absent" } }
} finally {
    if ($holder) { Stop-Process -Id $holder.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $enterDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "`n=== #700 (e6/e7) the wrapper's claim is INHERITED by the gate it launched, never waited on ==="
# The merge checklist (retired 2026-09-24): the WRAPPER claimed the slot with slot-claim.sh before launching the
# gate, and slot-claim.sh told the shell to export GRAPHHELM_HOLDER_PID / GRAPHHELM_HOLDER_START. A gate started
# inside that claim must recognise it as ITS OWN LANE'S -- by the exported pair equalling the lock's pair --
# or it waits on itself for the whole budget. Parentage is not the signal: a detached gate's parent dies.
$inhDir = New-TempTestDir -Name 'slot-inherit-700'
$inhPath = Join-Path $inhDir 'SLOT.lock'
$inhEvents = New-Object System.Collections.Generic.List[string]
$inhRecord = { param($Event, $Detail) $inhEvents.Add("$Event|$Detail") }
$prevHP = $env:GRAPHHELM_HOLDER_PID; $prevHS = $env:GRAPHHELM_HOLDER_START
try {
    if (Test-FnPresent 'Enter-GateSlot') {
        $mePid = $PID; $meStart = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('o')
        # (e6) the lock names a LIVE pair, and the environment carries the SAME pair -> inherited, untouched, silent
        New-SlotClaim -Path $inhPath -HolderPid $mePid -HolderStartUtc $meStart -Detail 'wrapper claim' | Out-Null
        $bytesInh = [System.IO.File]::ReadAllBytes($inhPath)
        $env:GRAPHHELM_HOLDER_PID = "$mePid"; $env:GRAPHHELM_HOLDER_START = $meStart
        $r6 = Enter-GateSlot -Path $inhPath -HolderPid 424242 -HolderStartUtc '2026-09-05T09:00:00.0000000Z' -Detail 'e6' -BudgetSeconds 3 -PollSeconds 1 -WriteEvent $inhRecord
        Assert-True -Condition ($r6 -eq 'inherited') -Message '#700 (e6a) a live lock whose pair equals the exported GRAPHHELM_HOLDER pair is INHERITED, not waited on'
        Assert-True -Condition ([System.Linq.Enumerable]::SequenceEqual([byte[]]$bytesInh, [byte[]][System.IO.File]::ReadAllBytes($inhPath)) -and $inhEvents.Count -eq 0) -Message '#700 (e6b) inheriting writes nothing: lock byte-identical, no ledger line'
        # (e7) the environment carries a pair, but the lock is somebody ELSE's live pair -> not inherited, waited on
        $other = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120' -PassThru -WindowStyle Hidden
        Start-Sleep -Milliseconds 300
        $otherStart = (Get-Process -Id $other.Id).StartTime.ToUniversalTime().ToString('o')
        [System.IO.File]::Delete($inhPath)
        New-SlotClaim -Path $inhPath -HolderPid $other.Id -HolderStartUtc $otherStart -Detail 'another lane' | Out-Null
        $r7 = Enter-GateSlot -Path $inhPath -HolderPid 424242 -HolderStartUtc '2026-09-05T09:00:00.0000000Z' -Detail 'e7' -BudgetSeconds 2 -PollSeconds 1 -WriteEvent $inhRecord
        Assert-True -Condition ($r7 -eq 'expired') -Message '#700 (e7) an exported pair that does not match the lock buys nothing: another lane''s live claim is waited on'
        Stop-Process -Id $other.Id -Force -ErrorAction SilentlyContinue
    } else { 1..3 | ForEach-Object { Assert-True -Condition $false -Message "#700 (e6/7-$_) Enter-GateSlot is absent" } }
} finally {
    if ($null -ne $prevHP) { $env:GRAPHHELM_HOLDER_PID = $prevHP } else { Remove-Item Env:\GRAPHHELM_HOLDER_PID -ErrorAction SilentlyContinue }
    if ($null -ne $prevHS) { $env:GRAPHHELM_HOLDER_START = $prevHS } else { Remove-Item Env:\GRAPHHELM_HOLDER_START -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $inhDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''

# THIRD outcome, distinct from PASS/FAIL: the run itself didn't cover what it was supposed to.
# Checked BEFORE the pass/fail summary, and exits on a different code, so "green" can never mean
# "green over fewer checks than last time" without saying so out loud.
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    Write-Host "Coverage changed silently - a block was skipped, commented out, or lost in a merge." -ForegroundColor Magenta
    Write-Host "If this is a deliberate new test, update `$ExpectedAssertionCount at the top of this file." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
