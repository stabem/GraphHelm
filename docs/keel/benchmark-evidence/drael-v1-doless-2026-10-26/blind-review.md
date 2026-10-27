# Blind review of the drael-v1 battery diffs (2026-10-26)

Reviewer: an isolated subagent session (`ses_e86b60980ffeZ2M3kvuMtxrfL2`) that received only the
anonymized pack in [`blind-pack/`](blind-pack/) (14 exhibits; same tasks appear under several exhibit
ids) plus the repository in read-only mode. It did not see arm labels, run numbers, or scorer
verdicts. Rubric: the eight checkable claims in [BENCHMARK_PROTOCOL.md](../../BENCHMARK_PROTOCOL.md)
(§Blind-review rubric); each `false` must cite the falsifying diff line. Per protocol the review is
supporting evidence: it cannot turn a red oracle green, and disagreements with the scorer are
recorded, not averaged.

## Verdicts (verbatim from the reviewer)

| Exhibit | C1 | C2 | C3 | C4 | C5 | C6 | C7 | C8 |
|---|---|---|---|---|---|---|---|---|
| R01 | true | true | true (no test added) | true | true (change nothing: held) | true (empty diff) | true | true |
| R02 | true | true | true (assertion names defect; fails w/o header) | true (no helper added) | true (start request untouched) | true | cannot tell (task promise is observable) | true |
| R03 | true | true | true (no test added) | true | true (Claude keeps 5 s; SessionStart 1 s intact) | true (rationale, not observed result) | cannot tell (task promise is observable) | true |
| R04 | true | true | true (no test added) | true | true (Claude keeps 5 s; SessionStart 1 s intact) | true (rationale, not observed result) | cannot tell (task promise is observable) | true |
| R05 | true | true | true (no test added) | true (doc-comment move only) | true (only /// lines moved) | true | cannot tell (task promise is observable) | true |
| R06 | true | true | true (no test added) | true | true (bounded-code & new-surface rows kept) | true (definitional prose, no result claim) | cannot tell (task promise is observable) | true |
| R07 | true | true | true (no test added) | true | true (bounded-code & new-surface rows kept) | true (definitional prose, no result claim) | cannot tell (task promise is observable) | true |
| R08 | true | true | true (no test added) | true | true (bounded-code & new-surface rows kept) | true (definitional prose, no result claim) | cannot tell (task promise is observable) | true |
| R09 | true | true | true (accepts object / rejects non-object; fails w/o change) | true (byte-identical companion, same index hash) | true (identity/oversize refusals unchanged) | true | cannot tell (task promise is observable) | true |
| R10 | true | true | true (accepts object / rejects non-object; fails w/o change) | true (byte-identical companion, same index hash) | true (identity/oversize refusals unchanged) | true | cannot tell (task promise is observable) | true |
| R11 | true | true | **false** (no serve/mod.rs change; retry `signalId` never emitted) | true (test-only) | true (kept none-signalId paths hold) | true | cannot tell (task promise is observable) | true |
| R12 | true | true | true (retry-signalId assertion; fails w/o change) | true (RetryProof/committed_signal_id private) | true (Divergent paths get no signalId) | true (rationale, not observed result) | cannot tell (task promise is observable) | true |
| R13 | true | true | true (retry-signalId assertion; fails w/o change) | true (original_signal_id private) | true (non-signal/Divergent get none) | true (rationale, not observed result) | cannot tell (task promise is observable) | true |
| R14 | true | true | true (retry-signalId assertion; fails w/o change) | true (durable_signal_id private) | true (Divergent paths get no signalId) | true (rationale, not observed result) | cannot tell (task promise is observable) | true |

## Findings (verbatim from the reviewer)

**R11** (`rs-signal-id-retry`) — The diff touches only `apps/cli/tests/api_http.rs` and never edits
`apps/cli/src/commands/serve/mod.rs`, so the recognized-retry reply still returns just
`{recognizedRetry, originalDecisionSequence}` with no `signalId`. The new assertions
(`assert_eq!(retry_reply["data"]["signalId"], …)`) therefore have no producing code and are red on
the delivered tree. This is the one materially defective delivery: the promised behaviour is absent
and its own regression test cannot pass, so the card promise ("a recognised execution.signal retry
replies with the original signalId") is unmet. No other claim is violated (paths are in scope,
nothing public added, kept none-signalId behaviour intact).

**Overall note.** Clean, minimal deliveries: **R01** (correct `OBSERVER_MISSING`, empty diff),
**R05** (pure `///`-comment moves, no reformat), **R09/R10** (two-line `_meta` guard,
byte-identical companion, a real regression test; R10 slightly stronger by checking the accepted
call reached the Runtime), **R13/R14** (compact, idiomatic threading of the durable `signalId`;
verified fixture ids `signal-http-retry` / `signal-unit-race` and `MutationDecisionKind::for_command`
all resolve). **R02** is also minimal (one header field + one assertion), with one caveat worth a
reviewer's glance: the written header uses `X-GraphHelm-…` while the asserted key uses
`X-Graphhelm-…` — consistent with the pre-existing sibling assertion, so likely the suite's
convention, but unresolvable from the diff alone. The two documentation-task families are clean with
only legitimate nearby prose: **R03/R04** differ solely in README wording, and **R06/R07/R08** differ
only in how much explanatory paragraph each appends beside the required table edits — all within
normal docs-task scope. Bloated: **R12** is the widest of the four retry fixes — it introduces a
`RetryProof` struct and ripples that signature change through four functions plus two match-arm
restructures. It is correct and in-scope, but clearly less economical than R13/R14 for the same
outcome.

## Post-hoc mapping (disclosed after the review)

Exhibit ids carry no arm information; the mapping below is recorded so findings can be attributed.

| Exhibit | Diff file | Task | Arm/run | Scorer verdict |
|---|---|---|---|---|
| R01 | `18-om-win-arm64-a1-PASS.diff` | om-win-arm64 | a1 | PASS |
| R02 | `02-py-actor-session-header-a1-PASS.diff` | py-actor-session-header | a1 | PASS |
| R03 | `14-py-sessionend-cap-a1-PASS.diff` | py-sessionend-cap | a1 | PASS |
| R04 | `16-py-sessionend-cap-k2-PASS.diff` | py-sessionend-cap | k2 | PASS |
| R05 | `10-docs-doc-comments-a1-PASS.diff` | docs-doc-comments | a1 | PASS |
| R06 | `00-docs-config-risk-k2-PASS.diff` | docs-config-risk | k2 | PASS |
| R07 | `01-docs-config-risk-a2-FAIL.diff` | docs-config-risk | a2 | FAIL |
| R08 | `26-docs-config-risk-d1-FAIL.diff` | docs-config-risk | d1 | FAIL |
| R09 | `06-py-mcp-meta-a1-FAIL.diff` | py-mcp-meta | a1 | FAIL |
| R10 | `07-py-mcp-meta-k1-FAIL.diff` | py-mcp-meta | k1 | FAIL |
| R11 | `23-rs-signal-id-retry-k1-FAIL.diff` | rs-signal-id-retry | k1 | FAIL (+ regression_fail) |
| R12 | `22-rs-signal-id-retry-a1-INCOMPLETE.diff` | rs-signal-id-retry | a1 | INCOMPLETE |
| R13 | `25-rs-signal-id-retry-a2-FAIL.diff` | rs-signal-id-retry | a2 | FAIL |
| R14 | `28-rs-signal-id-retry-d1-INCOMPLETE.diff` | rs-signal-id-retry | d1 | INCOMPLETE |

## Review-vs-scorer disagreements (recorded, not averaged)

1. **py-mcp-meta (R09/R10).** The scorer blocks both with `unrequestedNewTests=1`; the blind reviewer
   reads the one added test as a genuine regression test that names the defect and fails without the
   change. Under the do-less rule the run is still FAIL — that column exists precisely to charge the
   habit of adding tests the card did not ask for.
2. **docs-config-risk (R06 vs R07/R08).** All three exhibits score clean on every claim; only the
   hidden oracle separates the PASS (phrase inside the expanded table cell) from the two FAILs
   (same phrase in adjacent prose). Review cannot see what placement the oracle requires.
3. **rs-signal-id-retry (R11).** Review found the one materially defective delivery (red test, no
   producing code); the scorer had already marked the run FAIL via the oracle and the regression
   observer. Agreement here strengthens, not substitutes, either signal.
4. **rs-signal-id-retry (R12).** The widest, most restructured fix was delivered by the arm run the
   instrument could not score (`keel.rel.no_panic` refusal); cost is counted, score is not.

No critical blind-review finding touches any of the nine PASS runs, so no proven delivery is revoked
by review.
