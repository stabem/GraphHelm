# Unreleased

## Removed

- `graphhelm gate classify-red` (command id `gate.classify-red`, #1138 shadow mode) and the `gate`
  command group, with `graphhelm_architect::judgment::red` (`KnownFlake`, `RED_CLASSES`,
  `RedClassification`, `RedExcerpt`) that only it used. Its subject, the `ci/gate.ps1` runner's RED
  banner, was retired with the gate on 2026-09-24 (AGENTS.md: there is no gate). #1203.
- The acceptance map's `full-gate-green` clause and its live read of `ci/gate.ps1`; the map now
  holds six §8 clauses, each with a test prover. #1203.
- The gate runner and everything only it ran: `ci/gate.ps1`, the 28 `ci/gate-*.tests.ps1` suites,
  `gate-evidence`, `slot-lock`, `classify-run`, `run-class`, `select-scope`, `run-ps-suites`,
  `studio-stage`, `schema-stage-profile.tests` and `ci/fixtures/`; the token-bench task `1145`
  whose oracle cut functions out of `ci/gate.ps1`. `ci/` keeps the scripts a live caller names.
  #1204.
- The `ci/` scripts only the gate runner called: `event-contract-sweep`, `frozen-release-guard`,
  `find-culture-comparisons`, `crate-input-hash`, `manifest-name`, `required-features` (each with
  its suite) and `tool-versions.json`. `ci/` keeps `closing-keywords`, `normalize-script-eol`,
  `postgres` and `test-count`. #1231.
