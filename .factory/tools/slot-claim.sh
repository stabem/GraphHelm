#!/usr/bin/env bash
# Claim the GraphHelm slot as a CONDITIONAL write (ED-22 conditional-write amendment).
#
# The `if` is the mechanism; the read is only its input. This exists because the author read the
# lock, saw "HELD by A Agent" printed on his own screen, and overwrote it anyway -- the read and
# the write were in one command with nothing between them.
#
# usage: claim.sh <agent> <lane> <status-line...>
#
# WHAT IS DEMONSTRATED, and what is not:
#   ARM 1   lock HELD by another   -> REFUSES, file byte-identical after, no log line.   exit 1
#   ARM 2   lock FREE              -> CLAIMS, prev-seen carries what was READ.           exit 0
#   ARM 2b  lock FREE with a BOM   -> CLAIMS. This arm is why the strip below exists.
#           (2 and 2b are positive controls: a gate that always refuses is not a gate)
#   ARM 3   write does not land    -> UNEXERCISED. Not passing -- untested.
#
# THE BOM ARM IS NOT HYPOTHETICAL AND IT NEARLY SHIPPED BROKEN. The live SLOT.lock begins with a
# UTF-8 BOM, because PowerShell writes one by default -- and every agent has just been told to
# prefer the PowerShell channel, so a BOM'd lock is the NORMAL case now. Without the strip,
# `FREE*` can never match and this script refuses a free lock FOREVER.
#
# It was caught by luck. The first production use refused correctly -- A did hold the slot -- and
# the holder line printed with a visible BOM. The refusal was right and the gate could not have
# said anything else. A guard that always refuses looks identical, from the outside, to a guard
# that is working, on every single invocation. That is why arm 2 is a control and not a formality.
#
# Arm 3 resisted two attempts. chmod 444 does not stop it, because `mv -f` replaces the DIRECTORY
# ENTRY and never opens the old file. Pointing LOCK at a directory does not reach it either: the
# read fails first and arm 1 refuses, which is correct behaviour and the wrong arm. So the
# read-back branch below is a HYPOTHESIS, written because that failure was measured for real -- a
# lock correction that died behind a background job and never landed -- not because this script has
# been seen to catch it.
#
# Measuring those exit codes needed a second try of its own: the first read $? after a pipe and got
# TAIL's status, printing 0 for a refusal that had correctly returned 1.
set -u

LOCK="${SLOT_LOCK:-D:/graphhelm-slot/SLOT.lock}"  # overridable so the REFUSAL can be tested off the real file
LOG="${SLOT_LOG:-D:/graphhelm-slot/check-activity.log}"
AGENT="${1:?agent}"
LANE="${2:?lane}"
shift 2
STATUS="$*"

BOM=$'\xef\xbb\xbf'
CR=$'\r'

CURRENT="$(head -1 "$LOCK" 2>/dev/null || echo 'MISSING')"
CURRENT="${CURRENT#$BOM}"
CURRENT="${CURRENT%$CR}"
[ -n "$CURRENT" ] || CURRENT="MISSING"

# THE GATE. Everything below only runs if this branch is taken.
case "$CURRENT" in
  FREE*) ;;
  *)
    echo "REFUSING TO CLAIM -- the lock is not FREE."
    echo "  holder line: $CURRENT"
    echo "Nothing was written. This is the branch that did not exist when K overwrote A's claim."
    exit 1
    ;;
esac

# ADVISORY, NEVER A BLOCK: who else has an open wait. The list only works if the CLAIMER reads it,
# and nothing made them -- five claims in a row on 2026-08-25 went to whoever polled at the right
# second, each over an older declared wait, with no rule broken. Printing it here puts the
# information in front of the one person whose next action depends on it.
#
# It does not gate, and that is deliberate rather than lazy: a wait that could refuse a claim would
# let one dead agent wedge the machine for everybody. Compare with the FREE check above, which DOES
# gate -- these two lines look alike and only one of them is allowed to say no.
if [ -x "$(dirname "$0")/slot-waits.sh" ] || [ -f "$(dirname "$0")/slot-waits.sh" ]; then
  echo "open waits (advisory, oldest first):"
  SLOT_LOG="$LOG" bash "$(dirname "$0")/slot-waits.sh" | sed 's/^/  /'
fi

STAMP="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
PROCS="$(tasklist 2>/dev/null | grep -ci 'cargo.exe\|rustc.exe\|link.exe')"
TMP="$LOCK.claim.$$"

{
  echo "HELD by $AGENT | $STAMP | $LANE | STATUS: $STATUS"
  echo "prev-seen: $CURRENT @ read $STAMP"
  echo "Claimed through the conditional-write path: the read above GATED this write, and the write"
  echo "is read back below rather than assumed."
  echo "cargo/rustc alive at claim: $PROCS"
} > "$TMP"
mv -f "$TMP" "$LOCK"

# READ BACK. A write that never landed is invisible to whoever believes it did.
BACK="$(head -1 "$LOCK")"
BACK="${BACK#$BOM}"
BACK="${BACK%$CR}"
case "$BACK" in
  "HELD by $AGENT | $STAMP"*)
    printf '%s | %s | START | %s | claimed via conditional write; prev-seen: %s\n' \
      "$STAMP" "$AGENT" "$STATUS" "$CURRENT" >> "$LOG"
    echo "CLAIMED and verified: $BACK"
    ;;
  *)
    echo "WRITE DID NOT LAND. The lock now reads: $BACK"
    echo "Do NOT proceed -- another agent may hold it, or the write failed silently."
    exit 2
    ;;
esac
