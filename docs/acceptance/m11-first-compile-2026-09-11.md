# M11 — the first compile, run by hand (2026-09-11)

The acceptance of spec §4 (`docs/specs/2026-09-11-graph-architect-design.md`):
a goal → `graph synthesize` with the recorded fixture → the `--out` document → `graph lint`
reports zero `GHG102` → `execution start --file` with node fixtures → status `completed`. The
CLI journey test `a_goal_becomes_a_document_that_starts_and_completes`
(`apps/cli/tests/architect_cli.rs`) holds this run; the transcript below is the same road walked
once by hand, with the real outputs pasted.

Head run against: `e4c97241` on `issue-107-graph-architect`. Binary: `cargo +1.97.1 build -p
graphhelm-cli --locked`, debug profile. `<tmp>` is an empty scratch directory; no path below
depends on it.

## The commands

```sh
graphhelm graph synthesize \
  --goal "$(cat core/architect/fixtures/first-compile/GOAL.txt)" \
  --out <tmp>/first-compile.json \
  --allow-program cargo \
  --fixture core/architect/fixtures/first-compile/replies.json

graphhelm graph lint <tmp>/first-compile.json

printf '%s' '{"nodeOutcomes":{"build_check":"success","summarize":"success"}}' > <tmp>/fixtures.json
graphhelm execution start \
  --file <tmp>/first-compile.json \
  --events <tmp>/events \
  --fixtures <tmp>/fixtures.json \
  --mode supervised \
  --execution exec-first-compile

sha256sum <tmp>/first-compile.json
```

`GOAL.txt` is `check that the repository builds and summarize the result` — one string, read
by the crate test and the CLI test so no second copy exists. `--allow-program cargo` is passed
explicitly: the goal needs `cargo`, and the allowlist has no default (D-052).

## 1. `graph synthesize` — exit 0

The document, as the command printed it (`data.document`; `data.out` omitted, it is the `--out`
path):

```json
{"apiVersion":"p50.dev/graph/v1","kind":"ExecutionGraph","metadata":{"executionId":"exec_27fe0b8b","id":"arch_27fe0b8b_v1","labels":{"origin":"architect","template":"32236956956cd087fb4a7fb5b043a9a6e10716f7601191b08c79ba87fc84f123"},"name":"check that the repository builds and summarize the result","version":1},"spec":{"budgets":{"maxNodes":2},"completion":{"terminalNodes":["summarize"]},"edges":[{"from":"build_check","id":"build_to_summary","to":"summarize","type":"control"}],"entrypoints":["build_check"],"nodes":{"build_check":{"completion":{"customs":{"budgets":{"clearanceWithinSeconds":3600,"waitWithinSeconds":86400},"proofKinds":[]}},"name":"Build check","objective":"Run the repository build and record its exit code.","optionality":"required","tool":{"call":{"arguments":["build","--locked"],"program":"cargo","tool":"shell"}},"type":"tool"},"summarize":{"agent":{"ephemeral":{"capabilities":["summarize.build"],"completionContract":{"requires":["summary"]},"inputSchema":"schema://BuildReport@1","instructions":"State whether the build passed and cite the exit code.","outputSchema":"schema://BuildSummary@1","purpose":"Summarize the build result."}},"completion":{"customs":{"budgets":{"clearanceWithinSeconds":3600,"waitWithinSeconds":86400},"proofKinds":[]}},"name":"Summarize the build","objective":"Summarize the build outcome for the operator.","optionality":"required","type":"agent"}},"policies":[]}}
```

The rest of `data`, verbatim:

| field | value |
|---|---|
| `rounds` | `1` |
| `stampedCustoms` | `["build_check","summarize"]` |
| `templateSha256` | `32236956956cd087fb4a7fb5b043a9a6e10716f7601191b08c79ba87fc84f123` |
| `promptSha256s` | `["cf19c9bc4258dfb70fc2582366289b3852bfef8592bd531d7c89145b5dffe741"]` |
| `rationale[0]` | `{"node":"build_check","reason":"why it exists: Run the repository build and record its exit code.; customs stamped by the compiler (waitWithinSeconds 86400, clearanceWithinSeconds 3600)"}` |
| `rationale[1]` | `{"node":"summarize","reason":"why it exists: Summarize the build outcome for the operator.; customs stamped by the compiler (waitWithinSeconds 86400, clearanceWithinSeconds 3600)"}` |
| `usage` | absent (a recording reports none) |
| `diagnostics` | `[]` |

The shape spec §4 expected: a `tool` node `build_check` (`shell cargo build`) → an `agent` node
`summarize`, both stamped. The `metadata` is the compiler's (`arch_27fe0b8b_v1`, the goal's
sha8), and the `template` label equals `templateSha256`.

## 2. `graph lint` — exit 0, zero `GHG102`

```json
{"ok":true,"command":"graph.lint","data":{"errors":[],"warnings":[{"code":"GHG101_DEFAULT_TIMEOUT","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/build_check/timeoutSeconds","severity":"warning","source":"first-compile.json"},{"code":"GHG101_DEFAULT_TIMEOUT","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/summarize/timeoutSeconds","severity":"warning","source":"first-compile.json"}]},"diagnostics":[]}
```

No error; two `GHG101_DEFAULT_TIMEOUT` warnings (the template does not ask for `timeoutSeconds`,
so the runtime default applies) and **no `GHG102_UNBOUNDED_CUSTOMS`** — the stamp is what the
lint sees.

## 3. `execution start` — exit 0, status `completed`

```json
{"ok":true,"command":"execution.start","data":{"acceptedMutations":0,"attention":"can_sleep","attentionReasons":[],"executionId":"exec-first-compile","mode":"supervised","nodeStateCounts":{"blocked":0,"cancelled":0,"draft":0,"failed":0,"ghost":0,"invalidated":0,"linting":0,"paused":0,"queued":0,"ready":0,"running":0,"skipped":0,"succeeded":2,"waiting_capacity":0,"waiting_input":0,"waived":0},"nodeStates":{"build_check":"succeeded","summarize":"succeeded"},"signalsRecorded":0,"silenceUnevaluated":[],"status":"completed","untriagedInterruptions":[]},"diagnostics":[{"code":"GHG101_DEFAULT_TIMEOUT","severity":"warning","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/build_check/timeoutSeconds","source":"first-compile.json"},{"code":"GHG101_DEFAULT_TIMEOUT","severity":"warning","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/summarize/timeoutSeconds","source":"first-compile.json"}]}
```

The three wall-clock fields the command also prints (`startedAt`, `lastEventAt`,
`nodeLastEventAt`) are elided here: they are the run's time, not its evidence. `status` is
`completed`, both nodes `succeeded`, `attention` `can_sleep`.

## 4. The document's content hash

```
sha256  1e80a040d2c5a14831b914bc9fdfd7232ba275dee66af32b41ec8279beffd9a7  first-compile.json
```

This is the hash of the `--out` file (`to_vec_pretty` plus a trailing newline), which the CLI
test `the_document_is_byte_identical_across_two_runs` proves is the same on every run. The
crate golden `core/architect/fixtures/first-compile/expected.json` holds the same document in
its own serialization.

## 5. Spec §5, cell by cell — the test that holds each one

| cell (spec §5) | test | file |
|---|---|---|
| Golden: the recorded reply compiles to a byte-stable document | `the_recorded_reply_for_the_goal_compiles_to_a_byte_stable_document`; `the_document_carries_the_compilers_metadata_and_the_models_spec` | `core/architect/tests/golden.rs` |
| Sabotage — schema-broken | `a_schema_broken_draft_is_invalid_under_a_ghs_code_at_the_nodes_pointer` | `golden.rs` |
| Sabotage — lint-dirty (an edge to a missing node) | `an_edge_to_a_missing_node_is_invalid_under_ghg003_after_every_repair` | `golden.rs` |
| Sabotage — a `deploy` node named innocently | `a_deploy_node_named_innocently_is_invalid_under_its_own_code_not_laundered` | `golden.rs` |
| Sabotage — more nodes than `max_nodes` | `more_nodes_than_the_profile_allows_is_too_many_nodes_without_a_repair` | `golden.rs` |
| Sabotage — a shell program outside the catalog | `a_shell_program_outside_the_catalog_is_capability_missing_naming_node_and_program` | `golden.rs` |
| Sabotage — added by the review: the three tool-call shapes the broker's parser refuses, a budget above the profile, a non-JSON reply on every round, an invalid profile | `a_repository_call_without_an_action_is_gha003_at_the_call`; `a_tests_call_whose_arguments_are_not_an_array_is_gha003_at_the_call`; `a_repository_write_is_gha003_at_the_call_because_the_template_offers_only_reads`; `a_budget_above_the_profile_is_repaired_under_gha004_and_round_two_wins`; `a_reply_that_is_not_json_on_every_round_is_not_json_at_the_last_round`; `an_invalid_profile_is_refused_before_any_model_is_asked` | `golden.rs` |
| The repair loop repairs (round 2 valid → `rounds == 2`) | `the_repair_loop_repairs_a_round_two_reply_that_is_valid_wins` | `golden.rs` |
| Stamping is load-bearing (no seam; a reply without customs → `stampedCustoms` lists them and lint sees zero `GHG102`) | `stamping_is_load_bearing_every_parkable_node_leaves_with_customs_and_lint_sees_no_ghg102`; `the_profiles_budgets_are_the_ones_stamped_not_the_models`; and the cross-crate witness `ghg102_after_stamping_iff_the_lint_parks_the_type_and_the_compiler_did_not_stamp_it`, `every_executable_type_leaves_with_zero_ghg102_by_name` | `golden.rs`; `core/architect/tests/park_witness.rs` |
| Catalog ⊆ what the runtime executes; every `NodeType` variant reaches a decision | `every_variant_reaches_the_catalog_decision`; `the_catalog_admits_exactly_the_types_the_runtime_executes`; `programs_are_the_operators_allowlist_and_nothing_else` | `core/architect/tests/catalog.rs` |
| Template hash rides the reply and the labels; a foreign key is `FixtureMissing` naming the hash | `the_template_hash_rides_the_reply_and_the_document_and_a_foreign_key_is_fixture_missing`; `a_recording_that_answers_nothing_names_the_first_prompt`; `a_recorded_model_answers_only_the_prompt_it_recorded_and_names_the_missing_hash` | `golden.rs`; `core/architect/tests/template.rs` |
| CLI: `--fixture --out` writes the file, refuses to overwrite, and the file starts and completes (the first compile) | `a_goal_becomes_a_document_that_starts_and_completes`; `an_existing_out_path_is_never_overwritten`; `the_document_is_byte_identical_across_two_runs`; `a_fixture_without_the_prompt_prints_the_hash_to_record` | `apps/cli/tests/architect_cli.rs` |
| CLI refusal exit code and diagnostic shape for `CapabilityMissing` | `a_goal_needing_a_program_outside_the_allowlist_is_refused_and_names_the_program` | `architect_cli.rs` |
| HTTP and MCP return the same JSON the CLI does (byte-identical `data`) | `the_api_and_the_cli_compile_the_same_goal_to_the_same_bytes`; `the_architect_route_refuses_with_the_cli_codes_and_names_both_model_doors`; `the_synthesize_tool_reaches_the_architect_route_and_relays_its_document`; the tool is counted in `tools_list_names_exactly_the_registered_tools_with_closed_schemas` | `apps/cli/tests/api_http.rs`; `apps/cli/tests/mcp_stdio.rs` |

## 6. What this run does not show

- Tier B (semantic quality) — the reply is a recording, so this proves the road, not the
  model's judgement. See `docs/harness/GRAPH_ARCHITECT.md` §6.
- The gateway door — no credential was used; the CLI's `--manifest --route` path is exercised
  only for its refusal shapes in the suite.
- A real `cargo build` — `build_check` succeeded by node fixture, as the spec's acceptance
  states; the executor's real tool path is proven elsewhere (M05).
