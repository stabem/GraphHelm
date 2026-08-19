# Storm instrument controls — row-level evidence (A, for H's co-sign and D's records)

Raw probe logs alongside this file (copied from session temp before evaporation).
"Pair" = one open-begin/open-end with matching id.

| scenario | log | pairs by caller | ids (caller, phase) | verdict vs predicted |
|---|---|---|---|---|
| C1 happy (1 GET) | c1-happy | 1 request, 0 driver, 0 sweep | id0 (request) | PASS: exactly 1 request pair, zero driver |
| C1 sabotage (Edit 4 removed) | c1-sab-e4 | 1 request, 0 driver | id0 (request) | identical to happy -> sabotage NOT observable (led to D's option-a redesign) |
| C1 resume observation | c1-resume | 7 request, 2 sweep, 0 driver | sweeps id3, id8 | ZERO driver rows -> D's frozen prediction CONFIRMED |
| C2 happy (arm + signal) | c2-happy | 3 sweep | id3 (arm-sweep ph1), id7 (signal-sweep ph1), id8 (signal-sweep ph3) | PASS: exactly 3 |
| C2 sabotage Edit-2-only | c2-sab-e2 | 1 sweep | id8 (ph3) | 1 != 3 -> OBSERVED FAILING |
| C2 sabotage Edit-3-only | c2-sab-e3only | 2 sweep | id3, id7 (both ph1) | 2 != 3 -> OBSERVED FAILING |
| C2 extended (Edits 2+3) | c2-sab-e23 | 0 sweep | — | fails but isolates nothing; DISCARDED per D |

## Resume status question (D's pre-registered coincidence check)
The HTTP status codes were printed but not persisted (script wrote responses to stdout
only — my miss). SETTLED VIA THE THIRD APPARATUS instead: the kept events tree of the
c1-resume run (session temp, journal read before evaporation) shows `execution_paused`
then `execution_resumed` COMMITTED, followed by post-resume `node_outcome_recorded`
events — a refused resume (409/400) appends nothing, so the mutation landed and the
SYNC drive path ran. Committed-store evidence is independent of both the probe and the
lost status code. D's reading stands confirmed as verification, not coincidence.

## Run 2b tail stats (D's request: median answers a different question than the flake)
Per run (server pid, caller=request open elapsed, ms):
run1 n=152 med 29.6 p99 173.4 max 261.1 | run2 n=147 med 31.3 p99 124.6 max 135.2
run3 n=143 med 32.6 p99 159.3 max 169.4 | run4 n=143 med 31.3 p99 135.1 max 174.5
run5 n=156 med 30.1 p99 119.7 max 134.1 | run6 n=146 med 31.1 p99 152.1 max 203.2
run7 n=150 med 30.6 p99 133.4 max 142.8 | run8 n=149 med 29.1 p99 109.7 max 127.5
run9 n=144 med 30.6 p99 153.0 max 156.1 | run10 n=151 med 29.4 p99 140.6 max 196.7
Aggregate: med 29-33, p99 110-173, max 128-261.
Tail arithmetic (raw, not a prediction): median-based 8xO = 0.56-0.64s (~8x headroom);
p99-based 8 x 2.5 x p99 = 2.2-3.5s; degenerate all-at-max = 8 x 2.5 x 0.261 = 5.2s —
brushes the 5s budget. The budget lives in the tail, exactly as D said.
