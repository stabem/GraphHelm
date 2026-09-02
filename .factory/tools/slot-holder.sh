#!/usr/bin/env bash
# #624 / K's review of #686: resolve the slot holder pair, and say where it came from.
#
# EXTRACTED so the WRITER can be tested at all. K's review named the asymmetry: the reader has a
# suite and the writer -- default, override, validation, degradation -- is exercised by nothing.
# Inline in slot-claim.sh the only way to reach this logic was to make a real claim, which writes a
# real lock. A sourceable resolver is the seam that makes the decision testable without touching
# the slot.
#
# Sets HOLDER_PID, HOLDER_START, HOLDER_SOURCE (parent|override), HOLDER_VALID (0|1).
# Decides nothing: the caller owns the exit codes, because those belong to the claim, not to a
# lookup.
#
# MEASURED LIMIT of the default, and it is why the consumer is not wired yet. In the shell path the
# parent is this script's own bash, which exits when the script does:
#
#   derived pid 34656 is ALREADY GONE
#
# So the default can record a process that is already gone, and liveness would answer 'dead' for a
# live holder -- the ED-1 substitution inverted. It is inert ONLY because nothing consumes
# Test-SlotHolderLiveness yet. Whoever wires the consumer fixes this first.
resolve_slot_holder() {
  # NO DERIVATION. An earlier revision defaulted to the parent process; it is retracted, and both
  # measurements that killed it are worth keeping because they are different:
  #
  #   mine  the parent of the PowerShell we spawn IS a real Windows pid (bash.exe), but it is the
  #         script's own shell and exits with the script -- "derived pid 34656 is ALREADY GONE"
  #   K's   deriving from bash's own $PPID is worse still: that is an MSYS-namespace pid that
  #         Win32_Process cannot resolve, so it names either nothing or an unrelated process
  #
  # Both produce a lock that says its owner is dead while the owner works -- the confident-wrong
  # FREE #624 exists to prevent, arriving through the convenience door. That is worse than the
  # refusal it replaced, because it fails OPEN and in silence.
  #
  # The pair is therefore always SUPPLIED. Its producer is ci/gate.ps1, a long-lived PowerShell
  # whose $PID is a real Windows process that outlives the claim; a human claiming by hand supplies
  # it the same way. No pair, no claim: the floor stays.
  HOLDER_PID="${GRAPHHELM_HOLDER_PID:-}"
  HOLDER_START="${GRAPHHELM_HOLDER_START:-}"
  HOLDER_SOURCE="supplied"

  # VALIDATED AGAINST THE PROCESS TABLE, not merely parsed. A parser-only check accepts a
  # syntactically perfect pair that names nobody -- a live pid with a start time one tick off, or a
  # pair copied from an older session -- and writes it permanently. The reader then answers 'dead'
  # while the claimant is working, and recovery could free a held slot: the confident-wrong FREE
  # this whole issue exists to prevent, arriving through a pair that merely LOOKS right.
  # (Codex on #686.)
  #
  # So the claim asks the same question the reader will ask: does a process with this pid exist, and
  # does its start time match to the tick? One oracle for writer and reader, so a pair that passes
  # here cannot read 'dead' there. Values reach PowerShell through the environment rather than
  # interpolation, so a hostile value is data and never code.
  HOLDER_VALID=0
  if [ -n "$HOLDER_PID" ] && [ -n "$HOLDER_START" ]; then
    if GRAPHHELM_CHECK_PID="$HOLDER_PID" GRAPHHELM_CHECK_START="$HOLDER_START" powershell -NoProfile -Command '
        $p = 0
        if (-not [int]::TryParse($env:GRAPHHELM_CHECK_PID, [ref] $p)) { exit 1 }
        if ($p -le 0) { exit 1 }
        $d = [datetime]::MinValue
        if (-not [datetime]::TryParse($env:GRAPHHELM_CHECK_START,
              [System.Globalization.CultureInfo]::InvariantCulture,
              [System.Globalization.DateTimeStyles]::RoundtripKind, [ref] $d)) { exit 1 }
        # AN EXPLICIT OFFSET IS REQUIRED. RoundtripKind gives Kind=Unspecified for a zone-less
        # string, and ToUniversalTime() then applies the machine CURRENT offset -- so the same text
        # means different instants before and after a DST change, and the reader would answer
        # "dead" for a live holder after the clock shifted. A timestamp without a zone is not a
        # timestamp, it is a timestamp plus an assumption. (Codex on #686.)
        if ($d.Kind -eq [System.DateTimeKind]::Unspecified) { exit 1 }
        $proc = Get-Process -Id $p -ErrorAction SilentlyContinue
        if (-not $proc) { exit 1 }
        $started = $null
        try { $started = $proc.StartTime } catch { exit 1 }
        if ($null -eq $started) { exit 1 }
        if ($started.ToUniversalTime().Ticks -ne $d.ToUniversalTime().Ticks) { exit 1 }
        exit 0' >/dev/null 2>&1; then
      HOLDER_VALID=1
    fi
  fi
  return 0
}
