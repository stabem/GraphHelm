#!/usr/bin/env bash
# Claim the GraphHelm slot with a CREATE-OR-FAIL write (ED-24, commit a2619715, PR #620).
#
# usage: slot-claim.sh <agent> <lane> <status-line...>
#
# WHAT CHANGED, AND WHY THE OLD SHAPE WAS BOTH STUCK AND UNSAFE.
#
# ED-24 makes ABSENCE the only spelling of free. Against that rule the previous version of this
# script had two defects pulling in opposite directions, and only one of them announces itself:
#
#   DEADLOCK, loud.  An absent lock read as `MISSING`, and the gate accepted only `FREE*`, so the
#                    helper refused the one state that now means free. Anyone running it after a
#                    correct release would be told the slot is taken, forever.
#
#   BYPASS, silent.  The claim was `{...} > tmp; mv -f tmp "$LOCK"` -- a read, then a write, with a
#                    window between them. Two lanes that both read a claimable lock both passed the
#                    gate and both wrote; the last one won and BOTH believed they held it. That is
#                    literally the lane ED-24 says the primitive cannot refuse, written by the
#                    helper the lanes are told to run.
#
# The read-back below does NOT cover that race and never did. It fires when a write fails to land,
# which is a different failure: each racer reads back its own line successfully, and the loser only
# discovers the truth later, by acting on a lock that now names someone else. The atomic claim sees
# overwrites; the read-back sees lost writes. Neither substitutes for the other, so both are here.
#
# THE PRIMITIVE, MEASURED RATHER THAN ASSUMED. `( set -o noclobber; printf ... > "$LOCK" )` is
# create-or-fail on this machine's bash: 40 concurrent racers on ONE path yield exactly 1 winner, over 3 rounds.
# The positive control is the half that makes that number mean something -- the same 40 racers on
# DISTINCT paths yield 40 winners, so "1" is exclusion and not a broken harness. Measured twice,
# independently, by M and by K.
#
# WHAT IS DEMONSTRATED HERE:
#   ARM 1  lock PRESENT (any content)  -> REFUSES, file untouched, no log line.        exit 1
#   ARM 2  lock ABSENT                 -> CLAIMS.                                       exit 0
#   ARM 3  write does not land         -> read-back refuses.  UNEXERCISED -- see above.
#
# `FREE` is no longer accepted, and that is not an oversight: under ED-24 a file that exists is a
# held slot whatever it says inside. A leftover `FREE` from the old convention blocks, which is the
# safe direction of the transition, and it is cleared by DELETING the file, never by writing to it.
set -u

# EVERY line this script prints names the convention it enforces, so a refusal is DATABLE. A lane
# running a stale claimer fails in a way that reads as PRUDENCE -- "refused because the lock does
# not exist" is a true sentence about what it saw, and cannot tell a reader that the checker is
# the stale party. Nobody investigates a process that appears to be protecting them (A, on #635).
# When ED-25 arrives this script becomes the stale one, and the string below is what lets the next
# reader notice in seconds instead of hours.
PROTOCOL="ED-24 (a2619715): absence is free, claim is create-or-fail, release is delete"

# THE LOCK IS DERIVED FROM THE TARGET, because a wrapper that has to name it will eventually
# name the wrong one -- and did (#906). The checklist has said since #892 that there is ONE LOCK
# PER DISK, HDD at `D:/graphhelm-slot/SLOT.lock` and SSD at `E:/graphhelm-slot/SLOT.lock`, but
# this script had one default and no idea which spindle the caller was about to build on. So the
# safe configuration required every wrapper to set SLOT_LOCK correctly by hand, and the failure
# of any single one was invisible to the others.
#
# MEASURED INSTANCE (#906, 2026-09-05 14:11:43Z): a lane launched with
# CARGO_TARGET_DIR=E:/issues3-targets and no SLOT_LOCK, took the default, and wrote
# `HELD by issues3 | ... | STATUS: gate #826 E:` into the file that governs D: -- while another
# lane already held E:. The lock's own STATUS line said E: and the file it lived in governed D:.
# Two builds on one SSD, both lanes believing they held a slot, arriving through the DEFAULT
# rather than through a race.
#
# DERIVATION, NOT REFUSAL, when SLOT_LOCK is unset. A refusal there would hard-stop every lane
# whose target is on E: and whose wrapper has not been updated -- a worse failure than the one
# this closes. With the target known, the right lock is computable, so it is computed.
#
# REFUSAL when BOTH are given and their drives disagree, because that is not a lane that has not
# caught up: it is a wrapper stating two different slots in one breath, and one of them is wrong.
# Silence there would reproduce the measured instance with an explicit setting instead of a
# default.
#
# The target comes from SLOT_TARGET, or from CARGO_TARGET_DIR which every gate wrapper already
# sets -- so this claim chooses the matching default. This subprocess cannot export the path
# into its parent: the wrapper must pass the same path as GRAPHHELM_SLOT_LOCK_PATH to gate.ps1,
# as required by the merge checklist. Claim derivation alone does not configure the later gate.
slot_drive_of() {
  # THREE SPELLINGS OF THE SAME DISK, because this script is called from Git bash where an MSYS
  # path is ordinary. `E:/x` and `E:\x` are the Windows forms; `/e/x` is what an MSYS shell
  # produces for the same directory, and a wrapper that exports CARGO_TARGET_DIR from inside
  # bash will hand over that third one (Codex P1 on #1030). Missing it made the drive
  # undetectable, and an undetectable drive fell back to the D: default -- re-creating, for the
  # path form nobody tested, exactly the defect this change exists to remove.
  case "$1" in
    [A-Za-z]:/*|[A-Za-z]:\\*) printf %s "$(printf %s "${1%%:*}" | tr "[:lower:]" "[:upper:]")" ;;
    /[A-Za-z]/*) printf %s "$(printf %s "$1" | cut -c2 | tr "[:lower:]" "[:upper:]")" ;;
    *) printf %s "" ;;
  esac
}

SLOT_TARGET_PATH="${SLOT_TARGET:-${CARGO_TARGET_DIR:-}}"
TARGET_DRIVE="$(slot_drive_of "$SLOT_TARGET_PATH")"

# PRECEDENCE IS NOT AGREEMENT. SLOT_TARGET wins over CARGO_TARGET_DIR above, and that preference is
# only safe while the two name the same disk. A wrapper that INHERITS SLOT_TARGET=E:/... from an
# earlier lane and sets CARGO_TARGET_DIR=D:/... builds on D: while this script derives -- or
# validates -- the E: lock: the D: ceiling goes unanswered and a second D: gate launches beside it.
# That is the measured #906 shape reached through precedence rather than through a default (Codex
# P1 on #1030), so a disagreement refuses instead of one variable masking the other.
#
# The comparison is between DISKS, not strings: `E:/t`, `E:\t` and `/e/t` are one disk and claim
# normally. An unreadable drive beside a readable one is also a disagreement, because the readable
# one would then place a lock for a build that may land anywhere.
if [ -n "${SLOT_TARGET:-}" ] && [ -n "${CARGO_TARGET_DIR:-}" ]; then
  SLOT_TARGET_DRIVE="$(slot_drive_of "$SLOT_TARGET")"
  CARGO_TARGET_DRIVE="$(slot_drive_of "$CARGO_TARGET_DIR")"
  if [ "$SLOT_TARGET_DRIVE" != "$CARGO_TARGET_DRIVE" ]; then
    echo "slot-claim: REFUSED - the two target variables do not name one disk." >&2
    echo "  SLOT_TARGET: $SLOT_TARGET (disk: ${SLOT_TARGET_DRIVE:-<unreadable>})" >&2
    echo "  CARGO_TARGET_DIR: $CARGO_TARGET_DIR (disk: ${CARGO_TARGET_DRIVE:-<unreadable>})" >&2
    echo "  ONE LOCK PER DISK: SLOT_TARGET wins here, so the lock would answer for a disk cargo is" >&2
    echo "  not building on, leaving that disk's ceiling free for a second gate." >&2
    echo "  Unset one of them, or spell the same disk in both." >&2
    echo "  $PROTOCOL" >&2
    exit 6
  fi
fi

# A supplied target must identify a disk even when the lock is explicit. Otherwise the explicit
# branch bypasses the refusal and can claim one drive for a relative target on another (#1030).
# No target remains a supported legacy call; only a nonempty, unplaceable target is rejected.
if [ -n "$SLOT_TARGET_PATH" ] && [ -z "$TARGET_DRIVE" ]; then
  echo "slot-claim: REFUSED - the target names no disk this script can identify." >&2
  echo "  target: $SLOT_TARGET_PATH" >&2
  echo "  understood forms: X:/path, X:\\path, /x/path" >&2
  echo "  ONE LOCK PER DISK: an explicit lock cannot place an unknown target disk." >&2
  echo "  Give a target whose drive can be read." >&2
  echo "  $PROTOCOL" >&2
  exit 6
fi

if [ -n "${SLOT_LOCK:-}" ]; then
  LOCK="$SLOT_LOCK"
  LOCK_DRIVE="$(slot_drive_of "$LOCK")"
  if [ -n "$TARGET_DRIVE" ] && [ -n "$LOCK_DRIVE" ] && [ "$TARGET_DRIVE" != "$LOCK_DRIVE" ]; then
    echo "slot-claim: REFUSED - the lock and the target are on different disks." >&2
    echo "  SLOT_LOCK names $LOCK_DRIVE: ($LOCK)" >&2
    echo "  the target is on $TARGET_DRIVE: ($SLOT_TARGET_PATH)" >&2
    echo "  ONE LOCK PER DISK: a claim on $LOCK_DRIVE: leaves the $TARGET_DRIVE: ceiling unanswered" >&2
    echo "  and the $LOCK_DRIVE: slot held by a build that is not on it. Unset SLOT_LOCK to derive it." >&2
    echo "  $PROTOCOL" >&2
    exit 6
  fi
  if [ -n "$TARGET_DRIVE" ] && [ -z "$LOCK_DRIVE" ]; then
    echo "slot-claim: REFUSED - the explicit lock names no disk this script can identify." >&2
    echo "  SLOT_LOCK: $LOCK" >&2
    echo "  the target is on $TARGET_DRIVE: ($SLOT_TARGET_PATH)" >&2
    echo "  ONE LOCK PER DISK: an unplaced lock cannot satisfy the target disk ceiling." >&2
    echo "  Set SLOT_LOCK to a path whose drive can be read, or unset it to derive the lock." >&2
    echo "  $PROTOCOL" >&2
    exit 6
  fi
elif [ -n "$TARGET_DRIVE" ]; then
  LOCK="$TARGET_DRIVE:/graphhelm-slot/SLOT.lock"
else
  # SLOT_LOCK_DEFAULT exists for the same reason SLOT_LOCK does -- so a suite can exercise this
  # path without touching the machine's real slot. It is not a second way to name a slot: it is
  # only reached when the caller supplies NEITHER a lock nor a target, which in production means
  # a wrapper that sets no CARGO_TARGET_DIR. A test of the derivation must be able to make the
  # FALLBACK harmless too, because a broken derivation lands here -- and a suite whose failure
  # mode is "claimed the real HDD slot and blocked the fleet" is not a suite anyone should run.
  LOCK="${SLOT_LOCK_DEFAULT:-D:/graphhelm-slot/SLOT.lock}"
fi
# AND THE VARIABLE THE GATE ACTUALLY READS IS CHECKED TOO, because this script's careful choice
# governs nothing by itself. `ci/gate.ps1:4509` builds its lock path with `Get-SlotLockPath`, and
# `ci/slot-lock.ps1:345` returns `GRAPHHELM_SLOT_LOCK_PATH` verbatim when it is set -- so THAT is
# the file the gate will claim. The checklist therefore makes forwarding it mandatory, and until
# now this script refused a `SLOT_LOCK` on the wrong disk while ignoring the one that decides.
#
# MEASURED (lane S, review of #1030): target on W: with GRAPHHELM_SLOT_LOCK_PATH=D:/... exited 0
# and claimed W: here, while the gate that followed would sit on D: -- a claim and a gate on two
# disks, silently, which is #906 split across two processes instead of one.
#
# Compared against the lock THIS RUN chose, not against the target, so the explicit and derived
# branches are both covered by one comparison. A chosen lock whose own disk cannot be read is
# already refused above wherever a target exists; where no target exists there is nothing to
# disagree with, and the comparison is skipped rather than guessed.
if [ -n "${GRAPHHELM_SLOT_LOCK_PATH:-}" ]; then
  GATE_LOCK_DRIVE="$(slot_drive_of "$GRAPHHELM_SLOT_LOCK_PATH")"
  CHOSEN_LOCK_DRIVE="$(slot_drive_of "$LOCK")"
  if [ -n "$CHOSEN_LOCK_DRIVE" ] && [ "$GATE_LOCK_DRIVE" != "$CHOSEN_LOCK_DRIVE" ]; then
    echo "slot-claim: REFUSED - the gate's lock and this claim are on different disks." >&2
    echo "  GRAPHHELM_SLOT_LOCK_PATH: $GRAPHHELM_SLOT_LOCK_PATH (disk: ${GATE_LOCK_DRIVE:-<unreadable>})" >&2
    echo "  this claim would take: $LOCK (disk: $CHOSEN_LOCK_DRIVE)" >&2
    echo "  ONE LOCK PER DISK: the gate reads GRAPHHELM_SLOT_LOCK_PATH, so it would claim or wait" >&2
    echo "  on a different disk than the one this claim holds, leaving one ceiling unanswered." >&2
    echo "  Forward the SAME path the claim takes, as the merge checklist requires." >&2
    echo "  $PROTOCOL" >&2
    exit 6
  fi
fi
LOG="${SLOT_LOG:-D:/graphhelm-slot/check-activity.log}"
AGENT="${1:?agent}"
LANE="${2:?lane}"
shift 2
STATUS="$*"

BOM=$'\xef\xbb\xbf'
CR=$'\r'

# EVERYTHING THE CLAIM SAYS IS BUILT BEFORE THE CLAIM IS MADE. Creating an empty file and filling
# it afterwards leaves a window in which the lock exists and names NOBODY: a claimant killed in
# between -- while `tasklist` runs, say -- leaves a zero-byte file that blocks every later claim and
# carries neither owner nor timestamp to identify it as stale. That is the worst possible leftover,
# because stale-lock recovery is undefined by design (#619) and an unattributable lock gives whoever
# finds it nothing to act on (Codex on #635).
STAMP="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
# #624: the LIVENESS SIGNAL, taken from the environment rather than discovered here.
#
# WHY NOT DISCOVERED HERE, and this is measured rather than preferred. The first version walked the
# process ancestry looking for the session that spawned this script. It works when run directly and
# BREAKS when run the way a claim actually runs -- the tool spawns transient shells that exit, so
# the chain is orphaned before it reaches the session:
#
#   hop0 powershell.exe pid=14076 parent=65812
#   hop1 bash.exe       pid=65812 parent=64800
#   hop2 bash.exe       pid=64800 parent=12868
#   hop3 NO SUCH PROCESS 12868 - chain broken
#
# A walk that depends on every intermediate still being alive is not an instrument, and it fails
# SILENTLY: an empty result is indistinguishable from "no session found". So the identity is
# captured where it is reliable -- by the caller, which can see its own process -- and passed in.
#
# THE PAIR, not the pid: pids recycle, and the start time is what makes this an identity.
#
# DEGRADES SAFELY: unset -> written empty -> Test-SlotHolderLiveness reads 'indeterminate', never
# 'dead'. An unidentifiable holder must never be declared dead, so a caller that forgets this loses
# stale-lock recovery for that claim and takes nothing else with it.
# EXIT CODES, and they are distinct because a caller cannot branch on a code that means two things.
# `exit 2` previously covered BOTH a missing holder pair and "the write did not land" -- and the
# second is the dangerous one: the slot file exists and does not carry your claim, so a caller
# retrying on a shared code would retry into a lock it does not own.
#
#   1  contention: the slot is held by someone else
#   2  the write did not land: the lock exists and is NOT yours   <- never retry blindly
#   3  path or permission fault: nobody holds the slot
#   4  the holder cannot be identified at all
#   5  a holder pair was supplied, and it is unusable
#   6  the disks disagree: nothing was claimed, and no wait is implied  <- #1030, four sites
#
# CODE 6 IS NOT CONTENTION AND NOT A FAULT. It fires BEFORE any claim, at four places above, and
# every one of them is a caller stating two disks in one breath: an explicit SLOT_LOCK on a disk
# other than the target's; a target, or an explicit lock, whose disk cannot be read at all;
# SLOT_TARGET and CARGO_TARGET_DIR naming two disks; and GRAPHHELM_SLOT_LOCK_PATH -- the variable
# the GATE reads -- naming a disk other than the one this claim takes. A caller that retries on 6
# retries the same configuration forever: the remedy is to fix the variables, never to wait.
#
# THE PAIR IS ALWAYS SUPPLIED; there is no default and nothing here derives one. An earlier
# revision defaulted to the parent process and it was retracted under measurement -- slot-holder.sh
# carries the readings that forbid bringing it back. Fail-closed is right when the value is
# unknowable HERE, and a pid this script could derive is not the pid that owns the claim.
. "$(dirname "$0")/slot-holder.sh"
resolve_slot_holder

if [ "$HOLDER_VALID" -ne 1 ]; then
  if [ -n "$HOLDER_PID$HOLDER_START" ]; then
    # INVALID is not MISSING. Telling someone to supply what they already supplied sends them back
    # to the step that is not the problem.
    echo "slot-claim: REFUSED - the holder pair is present but unusable." >&2
    echo "  source: $HOLDER_SOURCE   pid: [$HOLDER_PID]   start: [$HOLDER_START]" >&2
    echo "The pid must parse as a positive Int32 and the start time must round-trip as a UTC" >&2
    echo "timestamp. These are the READER's own parses, so a value failing here would record an" >&2
    echo "identity Test-SlotHolderLiveness could only answer 'indeterminate' about." >&2
    exit 5
  fi
  echo "slot-claim: REFUSED - no holder pair was supplied." >&2
  echo "" >&2
  echo "The pair is never derived here (see slot-holder.sh); it is supplied by the process that" >&2
  echo "owns the claim. ANSWER THIS FIRST, because it decides whether you should supply one at all:" >&2
  echo "" >&2
  echo "  Does the shell you would type it into outlive this claim?" >&2
  echo "" >&2
  echo "A persistent terminal does. AN AGENT'S TOOL-CALL SHELL DOES NOT -- each call is a fresh" >&2
  echo "PowerShell that exits with the call (measured: pid 69756 in one call, NO SUCH PROCESS in" >&2
  echo "the next). Recording a pid that dies seconds later is WORSE than this refusal: liveness" >&2
  echo "then answers 'dead' for a working owner and the slot is reaped out from under them --" >&2
  echo "fail-open and silent, where a refusal fails closed and loud." >&2
  echo "" >&2
  echo "NO  -- then the claim is not yours to make. It belongs to a long-lived producer" >&2
  echo "       (ci/gate.ps1), and this refusal is the correct answer, not an obstacle. See #700." >&2
  echo "" >&2
  echo "YES -- then \$PID is your session, and these are the two values to export:" >&2
  echo "" >&2
  echo '  $env:GRAPHHELM_HOLDER_PID   = $PID' >&2
  echo '  $env:GRAPHHELM_HOLDER_START = (Get-Process -Id $PID).StartTime.ToUniversalTime().ToString("o")' >&2
  exit 4
fi

PROCS="$(tasklist 2>/dev/null | grep -ci 'cargo.exe\|rustc.exe\|link.exe')"
CLAIM="HELD by $AGENT | $STAMP | $LANE | STATUS: $STATUS
Claimed through create-or-fail: the kernel refused every other claimant, and this write is
read back below rather than assumed.
cargo/rustc alive at claim: $PROCS
holder: pid=$HOLDER_PID start=$HOLDER_START"

# THE GATE IS THE CLAIM. There is no read before it, because a read before a write is the window
# this script exists to close. The kernel decides, once, and tells us which way it went -- and the
# file it creates already identifies its owner.
if ! ( set -o noclobber; printf '%s
' "$CLAIM" > "$LOCK" ) 2>/dev/null; then
  # A FAILED CREATE IS NOT NECESSARILY AN OCCUPIED SLOT. `noclobber` refuses when the file exists,
  # and the same redirection also fails when the parent directory is missing or unwritable -- a
  # configuration fault with a completely different remedy. Reporting both as contention sends an
  # operator to stale-lock handling for a broken path, and stale-lock handling is undefined by
  # design (#619), so that is the worst possible place to send them (Codex on #635).
  if [ ! -e "$LOCK" ]; then
    echo "REFUSING TO CLAIM -- the create failed and NO lock file exists."
    echo "  enforcing: $PROTOCOL"
    echo "  lock path: $LOCK"
    echo "  parent dir exists: $( [ -d "$(dirname "$LOCK")" ] && echo yes || echo NO )"
    echo "  parent dir writable: $( [ -w "$(dirname "$LOCK")" ] && echo yes || echo NO )"
    echo "This is a PATH or PERMISSION fault, not contention. Nobody holds the slot. Fix the path"
    echo "or the permissions -- do not go looking for a stale lock, there is nothing to recover."
    exit 3
  fi
  HOLDER="$(head -1 "$LOCK" 2>/dev/null || echo '<unreadable>')"
  HOLDER="${HOLDER#$BOM}"
  HOLDER="${HOLDER%$CR}"
  echo "REFUSING TO CLAIM -- the slot file exists, so the slot is held."
  echo "  enforcing: $PROTOCOL"
  echo "  holder line: $HOLDER"
  echo "Nothing was written."
  echo "If that line says FREE it is a leftover of the old convention: it must be DELETED by whoever"
  echo "released, not overwritten. Deleting someone else's lock is stale-lock recovery (#619),"
  echo "which is undefined by design -- do not do it here."
  exit 1
fi

# From here the slot is OURS: the file exists and we are the process that created it.

# ADVISORY, NEVER A BLOCK: who else has an open wait. It does not gate, and that is deliberate
# rather than lazy -- a wait that could refuse a claim would let one dead agent wedge the machine
# for everybody. Compare with the create-or-fail above, which DOES gate: these two look alike and
# only one of them is allowed to say no.
if [ -f "$(dirname "$0")/slot-waits.sh" ] && [ -f "$LOG" ]; then
  echo "open waits (advisory, oldest first):"
  SLOT_LOG="$LOG" bash "$(dirname "$0")/slot-waits.sh" | sed 's/^/  /'
fi

# READ BACK. A write that never landed is invisible to whoever believes it did -- and possession is
# a property of the last READ, not of the last write.
BACK="$(head -1 "$LOCK" 2>/dev/null || echo '<unreadable>')"
BACK="${BACK#$BOM}"
BACK="${BACK%$CR}"
case "$BACK" in
  "HELD by $AGENT | $STAMP"*)
    printf '%s | %s | START | %s | claimed via create-or-fail under %s\n' \
      "$STAMP" "$AGENT" "$STATUS" "$PROTOCOL" >> "$LOG"
    echo "CLAIMED and verified: $BACK"
    echo "  enforcing: $PROTOCOL"
    ;;
  *)
    echo "WRITE DID NOT LAND. The lock now reads: $BACK"
    echo "Do NOT proceed -- the slot file exists but does not carry your claim."
    echo "  enforcing: $PROTOCOL"
    exit 2
    ;;
esac
