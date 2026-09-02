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

LOCK="${SLOT_LOCK:-D:/graphhelm-slot/SLOT.lock}"  # overridable so the REFUSAL can be tested off the real file
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
