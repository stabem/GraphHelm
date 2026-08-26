#!/usr/bin/env bash
# Who has been waiting for the slot, oldest first.
#
# ADVISORY BY DESIGN, AND THAT IS NOT AN OVERSIGHT. This never blocks a claim. A wait that could
# wedge the machine would be worse than the contention it describes: an agent that declared a wait
# and then died would hold the slot shut for everybody, and nobody would know why.
#
# It exists because the waiter list only works if the person CLAIMING reads it, and nothing made
# them. Measured on 2026-08-25: five claims in a row went to whoever polled at the right second,
# each time over an older declared wait, and no rule was broken -- the information was in the file
# and the claimer had no reason to look.
#
# A wait is OPEN when the agent's last wait-line is a DECLARED. SATISFIED and SUPERSEDED close it.
# Reading only the last line per agent is what makes a stale declaration harmless: one closing line
# retires it, and an agent that re-declares simply moves its own timestamp.
set -u

LOG="${SLOT_LOG:-D:/graphhelm-slot/check-activity.log}"

awk -F'|' '
  {
    for (i = 1; i <= NF; i++) { gsub(/^[ \t]+|[ \t]+$/, "", $i) }
  }
  $3 == "WAIT-DECLARED"   { state[$2] = "OPEN";   stamp[$2] = $1; what[$2] = substr($4, 1, 58) }
  $3 == "WAIT-SATISFIED"  { state[$2] = "CLOSED" }
  $3 == "WAIT-SUPERSEDED" { state[$2] = "CLOSED" }
  END {
    n = 0
    for (agent in state) {
      if (state[agent] == "OPEN") { n++; printf "%s  %-3s  %s\n", stamp[agent], agent, what[agent] }
    }
    if (n == 0) { print "no open waits" }
  }
' "$LOG" | sort
