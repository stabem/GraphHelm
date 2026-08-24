# #200: the gate's read of SLOT.lock, redesigned to three states discriminated by a `status` tag
# instead of a boolean `present`. Split out of gate.ps1 into its own dot-sourceable file so these
# functions can be unit-tested in isolation (ci/slot-lock.tests.ps1) without executing the rest of
# the gate - gate.ps1 dot-sources this file rather than defining the functions inline.
#
# THE DEFECT THIS REPLACES (full account: .factory/e-agent-200-design.md): the old
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

    return [ordered]@{
        schemaVersion = 2
        status        = 'present'
        path          = $lockPath
        content       = $content
        observedAtUtc = [DateTime]::UtcNow.ToString('o')
    }
}

# Gate 9 (L, from re-measuring a real committed manifest): slotLockAtStart and slotLockAtEnd are
# both captured specifically so they COULD be compared, and nothing did the comparing. An
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
    $a = [ordered]@{}
    foreach ($key in $Start.Keys) {
        if ($key -ne 'observedAtUtc') { $a[$key] = $Start[$key] }
    }
    $b = [ordered]@{}
    foreach ($key in $End.Keys) {
        if ($key -ne 'observedAtUtc') { $b[$key] = $End[$key] }
    }

    return (($a | ConvertTo-Json -Depth 8 -Compress) -eq ($b | ConvertTo-Json -Depth 8 -Compress))
}
