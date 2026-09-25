# Milestone 05 acceptance map

Generated from `m05-clauses.toml` by `tools/acceptance-map` — do not edit by hand; regenerate with `cargo run -p acceptance-map`. The `acceptance_map_is_grounded` test (gate: workspace tests) verifies every binding below against the real tree, so this document cannot rust silently.

## The §8 clauses and their provers

### real-work-to-completion

> A published graph with agent and tool nodes runs to completion doing real work — a model call and a repository/shell/test cycle — with every transition durable and the full history replaying byte-identically.

The 05d/05e acceptance sentence as one named test: a replying model fake, a real git tool in a Tier 1 worktree, sealed evidence, and a double replay compared byte for byte.

- `an_agent_and_a_tool_node_run_to_completion_with_sealed_evidence` (suite: runtime_http) — fingerprint: `replay must be byte-identical`
- **committed run evidence** `docs/acceptance/m05-run-2026-08-16` — one real story on the owner-subscription route (claude CLI), completed and replayed byte-identically (every file checksummed in its `SHA256SUMS`; the grounding test re-hashes it AND opens the archived store, replaying it against the current build. The store is archived as bytes: git cannot carry its empty `.tmp/` and `active/` directories, so the shape is restored before opening — see the directory's `README.md`)
- **recorded demonstration** `docs/acceptance/demos/m06-fixture-journey` — a fixture journey (manual-override-deploy, autopilot) recorded whole: committed store, frozen seed, seed-derived node-check traversal, and the projection digest the current build must reproduce (frozen seed, seed-derived traversal, and a projection digest the current build must reproduce on replay; every file tracked and checksummed)

### one-state-from-every-surface

> The same execution can be started, observed, signalled, approved, paused, resumed and cancelled from the CLI, the HTTP API, and a Claude Code or Codex chat via the MCP server — with identical observable state at every point.

Two parity stories: CLI vs API (05a's tripwire, surviving the 05d async swap) and MCP vs API (05e, with the exception list empty by design).

- `the_cli_and_the_api_report_identical_status_for_the_same_story` (suite: api_http) — fingerprint: `identical status data for the identical story`
- `the_mcp_and_the_api_report_identical_status_for_the_same_story` (suite: mcp_stdio) — fingerprint: `empty by design`

### monitor-without-a-single-mutation

> The monitor shows a running execution's states, triage list and events without offering a single mutation.

Rendering proven over the same projection status folds, and read-only proven over store bytes: hammer every route with every verb and the store is bit-identical.

- `the_monitor_renders_states_triage_and_tail_from_the_same_projection_fixture` (suite: workspace tests) — fingerprint: `renders its state exactly once`
- `hammering_the_monitor_never_changes_a_byte_of_the_store` (suite: monitor_http) — fingerprint: `the store must be bit-identical after the hammer`

### credentials-absent-from-tier-1

> Credentials are demonstrably absent from every Tier 1 workspace.

The 05c hard constraint: a sentinel-laden environment runs a Tier 1 tool and the sentinel is provably unreachable from inside the workspace.

- `credentials_are_demonstrably_absent_from_the_tier_1_workspace` (suite: workspace tests) — fingerprint: `inner sentinel run failed`

### exhausted-route-parks

> An exhausted subscription route parks the node in NeedsCapacity — wait, not aggressive retry — per the gateway spec.

The §12 wait rule as a total mapping: quota-class errors land in NeedsCapacity and only them.

- `capacity_class_errors_park_the_node_and_only_them` (suite: workspace tests) — fingerprint: `NeedsCapacity`

### full-gate-green

> The full local gate is green, both PostgreSQL locale passes included.

Proven by the gate run itself, not by one test: the grounding check pins that the gate script still ends in the GREEN verdict line and still carries both PostgreSQL passes.

**Proven by the gate run itself**: `./ci/gate.ps1` must end in its GREEN verdict with both PostgreSQL locale passes on the surface.

### surfaces-cannot-disagree-about-attention

> No surface recalculates the attention verdict. The CLI, the HTTP API, the MCP server and the monitor all derive it from the same predicate, so they cannot disagree about whether the operator needs to wake up.

Three angles, none of which a private copy of the predicate can survive: the API's own answer must be a FILTER over its reasons rather than a second computation; the monitor must say exactly what the API says about silence, on a store seeded with a node in flight so the question actually exists; and the two parity traces (CLI vs API, MCP vs API) carry exception lists that are empty by design, so a surface that answered differently would have to be listed.

- `the_api_answers_the_sleep_question_and_zero_fills_every_bucket` (suite: api_http) — fingerprint: `not a second computation`
- `the_page_and_the_api_never_disagree_about_silence` (suite: monitor_http) — fingerprint: `the page must say 'not evaluated' exactly when the API does`
- `the_cli_and_the_api_report_identical_status_for_the_same_story` (suite: api_http) — fingerprint: `identical status data for the identical story`
- `the_mcp_and_the_api_report_identical_status_for_the_same_story` (suite: mcp_stdio) — fingerprint: `empty by design`

## Refused scope (D-040)

Every affordance below is banned from the monitor; the citation is the decision register's own sentence, and the grounding test verifies it still appears there.

- **an approve button** — "It contains no mutating action: a UI that can mutate is Studio"
- **a pause button** — "It contains no mutating action: a UI that can mutate is Studio"
- **a retry button** — "It contains no mutating action: a UI that can mutate is Studio"
- **a cancel button** — "It contains no mutating action: a UI that can mutate is Studio"
