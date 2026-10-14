# Unreleased

## Removed

- `graphhelm gate classify-red` (command id `gate.classify-red`, #1138 shadow mode) and the `gate`
  command group, with `graphhelm_architect::judgment::red` (`KnownFlake`, `RED_CLASSES`,
  `RedClassification`, `RedExcerpt`) that only it used. Its subject, the `ci/gate.ps1` runner's RED
  banner, was retired with the gate on 2026-09-24 (AGENTS.md: there is no gate). #1203.
- The acceptance map's `full-gate-green` clause and its live read of `ci/gate.ps1`; the map now
  holds six §8 clauses, each with a test prover. #1203.
