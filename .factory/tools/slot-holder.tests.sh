#!/usr/bin/env bash
# Cells for the slot holder resolver. BASH, on purpose.
#
# These lived in ci/slot-lock.tests.ps1 and passed there -- for me. From PowerShell, which is how
# ci/gate.ps1 runs, `bash` resolves to C:\WINDOWS\system32\bash.exe (WSL): a different filesystem
# mapping and not necessarily installed. The cells were green for an environment-specific reason
# and would have failed the authoritative gate, so the gate stays PowerShell-only.
#
# NOT GATED: nothing runs this automatically. #700 carries that beside the reader's missing
# consumer. Run by hand:  bash .factory/tools/slot-holder.tests.sh
set -u
cd "$(dirname "$0")"
pass=0; fail=0
check() {
  if [ "$2" = "$3" ]; then pass=$((pass+1)); echo "  PASS: $1"
  else fail=$((fail+1)); echo "  FAIL: $1 (expected [$2] got [$3])"; fi
}
# UNSET FIRST: an operator who followed slot-claim.sh's refusal message has these exported, and an
# inherited pair would make the absent-pair cell see a valid one. The error message instructs the
# action that breaks the test. (Codex on #686.)
probe() {
  env -u GRAPHHELM_HOLDER_PID -u GRAPHHELM_HOLDER_START "$@" \
    bash -c '. ./slot-holder.sh; resolve_slot_holder; echo "$HOLDER_SOURCE|$HOLDER_VALID|$HOLDER_PID"'
}

echo "=== slot-holder resolver ==="
check "nothing supplied is INVALID, never derived" "supplied|0|" "$(probe)"
check "an unparseable pair is INVALID, and no default replaces it" "supplied|0|abc" \
  "$(probe GRAPHHELM_HOLDER_PID=abc GRAPHHELM_HOLDER_START=nope)"

# A process that is STILL ALIVE while the resolver checks it. An earlier fixture captured $PID from
# a PowerShell that exited immediately, so the pair named a dead process -- and once the resolver
# started verifying against the process table, that fixture failed. The check caught my own stale
# pair, which is exactly what it is for.
SLEEPER="$(powershell -NoProfile -Command '
  $p = Start-Process -FilePath powershell -ArgumentList "-NoProfile","-Command","Start-Sleep 30" -PassThru -WindowStyle Hidden
  Start-Sleep -Milliseconds 400
  $proc = Get-Process -Id $p.Id
  Write-Output ("{0} {1}" -f $proc.Id, $proc.StartTime.ToUniversalTime().ToString("o"))' 2>/dev/null | tr -d '\r' | head -1)"
SPID="${SLEEPER%% *}"; SSTART="${SLEEPER#* }"

check "a LIVE process supplied by its owner validates" "supplied|1|$SPID" \
  "$(probe GRAPHHELM_HOLDER_PID="$SPID" GRAPHHELM_HOLDER_START="$SSTART")"

# THE CASE THE PARSER-ONLY CHECK LET THROUGH: a live pid with a start time one tick off. It parses
# perfectly and names nobody, and the reader would answer 'dead' while the claimant works -- a lock
# that frees itself under recovery. Verification against the process table is what refuses it.
SKEW="$(powershell -NoProfile -Command "
  \$t = [datetime]::Parse('$SSTART', [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind)
  Write-Output \$t.AddTicks(1).ToUniversalTime().ToString('o')" 2>/dev/null | tr -d '\r' | head -1)"
check "a live pid with a start time ONE TICK off is INVALID" "supplied|0|$SPID" \
  "$(probe GRAPHHELM_HOLDER_PID="$SPID" GRAPHHELM_HOLDER_START="$SKEW")"

# A zone-less start time is a timestamp plus an assumption: the same text means different
# instants before and after a DST change, so the reader would answer 'dead' for a live holder.
# The writer refuses it rather than persisting something whose meaning moves. (Codex on #686.)
ZONELESS="${SSTART%Z}"
check "a start time with no UTC offset is INVALID" "supplied|0|$SPID"   "$(probe GRAPHHELM_HOLDER_PID="$SPID" GRAPHHELM_HOLDER_START="$ZONELESS")"

powershell -NoProfile -Command "Stop-Process -Id $SPID -Force -ErrorAction SilentlyContinue" 2>/dev/null
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
