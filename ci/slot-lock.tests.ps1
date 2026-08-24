# #200: isolated tests for ci/slot-lock.ps1 - the redesigned three-state read of SLOT.lock.
#
# Dot-sources ONLY slot-lock.ps1, never gate.ps1. These functions have no cargo dependency, no
# slot dependency, and no repository dependency - they read one env var and (at most) one file
# under a throwaway temp directory this script creates and removes itself. Writing and running
# this file needs no machine coordination; see .factory/e-agent-200-design.md for the sealed
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
$ExpectedAssertionCount = 24

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
    # .factory/e-agent-200-design.md Fixture C for the sha256-verified snapshot this is modeled
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
