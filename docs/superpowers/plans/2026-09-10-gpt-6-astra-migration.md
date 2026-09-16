# GPT-6 Astra Migration Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` to implement this plan task by task. This document authorizes no deployment or paid experiment by itself. Steps use checkboxes for tracking.

**Goal:** Make the project's selected OpenAI route run GPT-6 Astra with verified output, preserved billing boundaries, and a tested rollback.

**Architecture:** Keep GraphHelm's provider-neutral gateway and deterministic kernel. Configure Astra at the existing route boundary; repair the Codex event decoder if the selected route uses Codex. Keep a Responses API/tool-loop expansion separate from the initial migration.

**Tech stack:** Rust 1.97.1, existing `serde`/`serde_json`, `HttpTransport`/`ureq`, native Codex CLI, PowerShell local verification.

**Spec:** `docs/DECISION_REGISTER.md` D-014/D-015/D-016/D-039/D-041; `docs/models/UNIVERSAL_MODEL_GATEWAY.md` sections 4–16; ADR-025 in `docs/reference/REFERENCE_STACK_AND_ADRS.md`.

## Status and recommendation

Implementation authorized by the owner on 2026-09-10, tracked by issue #1034. The owner explicitly requires additive support that preserves the project's foundations and existing providers. The implementation record is `docs/acceptance/gpt-6-astra-migration.md`.

**Implementation ruling:** the inspected local manifest contains only `claude_subscription`. There is no existing OpenAI route to migrate. Add a separate, opt-in `codex_astra_subscription` route and preserve Claude. Task 3 and the optional API expansion do not apply. Use absolute correctness assertions for the new route; no comparison with a previous OpenAI model, cost improvement, latency improvement, or production migration may be claimed. A running production Runtime and its selected route were not established.

**Current-main route refresh:** `resolve_requested_route` re-reads the manifest before each drive, including the default path. Only the default route ID is retained from server startup. A request may select an enabled route with `"route"`; an existing active drive keeps its prepared model port. This supersedes the old checkout's configuration-snapshot observation below. The acceptance record documents enablement and rollback at execution boundaries.

Initial planning source baseline: `603e972132004d3330140acc4c11f395aa929a58`. That checkout is a divergent operator branch. Implementation integration starts from fetched `origin/main` at `d08bead81d589c7a3c3f8644d02f525c7ef570ba`; the three decoder implementation/test files are identical between these baselines before this change. Only the scoped migration commits are transported. Existing `.gitignore` edits and untracked operator files remain the user's work. The paragraphs below describing unknown access and CLI 0.152.1 are the original planning observations; the acceptance record contains subsequent measurements.

**Recommendation:** migrate one existing OpenAI route first, keeping its current billing method. If it is a Codex subscription route, complete Task 2 before activation. If it is a direct API route, use Task 3. Do not turn every work profile or provider into Astra.

The active operator manifest, production host, current selected model, effective reasoning setting, and account access were not established. They are explicit inputs to Task 1, not assumptions about production. The locally installed CLI reports `codex-cli 0.152.1`; `codex exec --help` confirms `--model`, `--json`, stdin input, and sandbox selection. This does not prove Astra access or successful execution.

## Current implementation and migration impact

| Observed boundary | Evidence | Consequence |
|---|---|---|
| Direct API model comes from the route | `core/gateway/src/manifest.rs`, `ModelRoute.model`, `validate_direct_api` | Change the selected manifest entry; no global model constant needs replacing in this path. |
| OpenAI sends only `model` and `messages` to `/v1/chat/completions` | `adapters/model-gateway/src/byok.rs`, `ByokAdapter::call_openai` | A text-only model change can remain on this endpoint. The adapter currently sends no sampling, reasoning, or cache controls. |
| OpenAI ignores `ModelCall.max_tokens` | Same method; `core/gateway/src/call.rs` | A model-name edit does not establish a per-call output/spend cap. Record this limitation; use the optional API follow-up if a hard cap is required. |
| Native runtime forwards `command.program` and `command.args`; prompt travels over stdin | `adapters/model-gateway/src/runtime.rs`, `RuntimeAdapter::invoke` | Select Astra with the Codex command arguments. A native route's top-level `model` value alone does not select the subprocess model. |
| Codex decoder expects nested `msg.type` / `msg.message` | `runtime.rs`, `parse_codex_jsonl`, `parsed_codex_error_text`; `src/bin/fake_runtime.rs` | It does not consume the currently documented `item.completed` / `item.text` stream. This is a source-level compatibility finding, not a reproduced live failure. |
| Runtime clones route wiring and dispatches either adapter | `apps/cli/src/commands/serve/ports.rs`, `ServeModelPort` | Verify how the running server reloads its manifest. Do not assume editing a file changes an already loaded route. |
| Gateway calls contain prompt/max tokens and return text/usage | `core/gateway/src/call.rs` | There is no tool-call conversation in this value contract. Responses tool support would require a separate design, not merely a URL swap. |

Evidence used the graph project `F-github-GraphHelm` at generation `2026-09-09T01:13:58Z`, followed by direct source checks. Coverage reported changed filesystem metadata for the evidence paths and a parse gap at `ci/gate.ps1:1–807`. Graph call tracing for `call_openai` returned only a file-level relationship; adapter dispatch was checked in source instead. This is a bounded migration assessment, not an exhaustive audit. Refresh discovery and coverage at implementation time, and read the gate directly before running it.

## Verified OpenAI requirements

- Target identifier: `gpt-6-astra`. Supported API reasoning levels: `low`, `medium`, `high`, `xhigh`, `max`. [Model reference](https://developers.openai.com/api/docs/models/gpt-6-astra)
- Preserve the previous effective effort; map `none`/`minimal` to `low`. Tool calls require Responses. Remove unsupported sampling/logprob fields if adding request options. Existing text-only Chat Completions use is supported. [Migration guidance](https://developers.openai.com/api/docs/guides/latest-model)
- API cache migration, if applicable: use `prompt_cache_options.ttl: "30m"` in place of old retention settings. EU residency requires Standard processing. Neither setting is currently emitted by GraphHelm's OpenAI request builder. [Migration guidance](https://developers.openai.com/api/docs/guides/latest-model)
- Current Codex JSONL exposes `item.completed`, `turn.completed`, `turn.failed`, and `error`; final message text belongs to an agent-message item. [Codex non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
- Listed API prices per million tokens are $10 input, $1 cached input, $12.50 cache writes, and $50 output. Requests above 272K input tokens have higher rates. These are API prices, not subscription charges; account eligibility and actual task cost need measurement. [Model pricing](https://developers.openai.com/api/docs/models/gpt-6-astra)

## Global constraints and threat assessment

- Keep the Policy Engine independent of models, SDKs, network, and browsers. Only the Governor publishes operational graph edits.
- Preserve immutable graph history, append-only evidence, `p50.dev` wire identifiers, route independence, and cheaper workload roles.
- Subscription exhaustion pauses execution. Never add automatic paid BYOK/OpenRouter fallback.
- Credentials stay in the existing broker or official runtime credential namespace. Preserve stdin prompts, environment filtering, response-size limits, timeouts, TLS, and disabled redirects.
- Treat prompts, repository instructions, tool output, and provider replies as untrusted. Text claiming success is not proof of an action. Malformed output or a failed turn cannot become a successful node.
- Before implementing, create or identify the migration issue, freeze `files_in_scope`, and use its issue branch in an isolated checkout. The Foundation #1 branch rule is milestone-specific; do not close #1 for this migration. Re-read current instructions and record any actual authority conflict in an RFC/ADR before implementation.
- All new documentation is English. CI is local only. Use Rust 1.97.1. No hosted Actions or model credentials in offline tests.
- No new SDK or dependency is needed for the initial route/decoder migration. `ureq` remains confined to `transport.rs` under ADR-025.
- Principal failure cases: unintended paid billing, false success from partial Codex output, lost quota classification, sensitive data in logs, and changed model quality/latency. Tasks below provide observers for each.

## Task 1: Freeze the route and baseline

**Files:** create `docs/acceptance/gpt-6-astra-migration.md` as an evidence summary; inspect the operator's active manifest without copying secrets. The exact operator-owned path must be recorded in the issue before it enters `files_in_scope`.

**Interface:** consume the existing `RouteManifest`; produce a recorded route ID, transport, billing mode, previous configuration, effective model/effort, approved data boundary, and rollback copy.

- [ ] Identify the manifest actually supplied to GraphHelm and the route selected by the running Runtime. Record the configuration reload/restart mechanism. If this task was intended only to change the coding host's model, stop at host configuration and do not change gateway code.
- [ ] Record source SHA, CLI version for native routes, current auth mode, enabled work profiles, and previous effective model/effort without exposing auth data. Verify Astra availability on that same account during the later opt-in live check.
- [ ] Keep the old manifest and executable version for rollback. Preserve route IDs and credential leases where possible.
- [ ] Freeze ten representative, non-sensitive tasks: four normal tasks, two structured-answer tasks, two ambiguous requests, and two adversarial instruction cases. Record exact inputs, expected assertions, output requirements, and allowed actions before running Astra.
- [ ] Set acceptance thresholds before comparison: all schema/policy/billing assertions pass; at least the baseline's task-success count; median latency and observed API cost per successful task each at most 1.25 times baseline. These are proposed project rollout thresholds, not model guarantees. For subscriptions, unknown monetary cost remains unknown; compare observed quota where available.
- [ ] Capture baseline results under the same prompt, effort, and data limits. Preserve failed attempts and retries separately. No live run belongs in the offline gate.

**Deliverable:** a concrete migration issue and repeatable comparison corpus. If the active route cannot be identified, continue offline decoder work only; do not claim a production migration.

## Task 2: Make the Codex subscription route compatible

**Run this task for a selected Codex native route.**

**Files:** modify `adapters/model-gateway/src/runtime.rs`, `adapters/model-gateway/src/bin/fake_runtime.rs`, `adapters/model-gateway/tests/runtime_adapters.rs`; extend `apps/cli/tests/gateway_cli.rs` only for observable route-command behavior. Document the supported CLI stream in `docs/acceptance/gpt-6-astra-migration.md`.

**Interfaces:** preserve `RuntimeAdapter::call(&ModelCall) -> Result<ModelReply, GatewayError>`. Decoder changes consume documented JSONL and return final text, reported usage, or the existing typed failure. Do not expose raw provider errors.

- [ ] Add a focused decoder regression beside `runtime.rs` that fails on the present parser:

```rust
#[test]
fn codex_current_jsonl_returns_completed_text_and_usage() {
    let stream = br#"{"type":"thread.started","thread_id":"test"}
{"type":"item.completed","item":{"id":"a","type":"agent_message","text":"ok"}}
{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":3}}
"#;
    let reply = parse_codex_jsonl(stream).expect("completed reply");
    assert_eq!(reply.text, "ok");
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(3));
}
```

- [ ] Add negative cases with an agent message followed by `turn.failed`, an error without a completed turn, reasoning-only output, malformed JSON, and a truncated stream. None may return a successful reply. A current-format success requires terminal completion as well as final text.
- [ ] Cover provisional top-level reconnect errors followed by terminal success, terminal failure, malformed input, or end of stream. The terminal outcome decides the result; a completed stream still requires process exit zero, and post-completion events cannot rescue a failure.
- [ ] Implement a typed current-event decoder, including `turn.completed.usage`. Preserve missing usage as `None`. Parse current error events before existing taxonomy classification. Retain the legacy decoder as a clearly separated format only if the pinned installed fleet still needs it; reject ambiguous mixed formats. Do not let a failed current stream fall through to legacy success.
- [ ] Extend `fake_runtime` with a current-Codex mode while retaining historical fixtures. Exercise successful output, quota/auth failure, unknown failure, timeout, size caps, argv forwarding, and credential exclusion through the actual subprocess adapter. Run the focused suite:

```powershell
cargo +1.97.1 test -p graphhelm-model-gateway --locked
```

- [ ] In the selected native route's existing argument list, replace or add the model pair `"--model", "gpt-6-astra"`. Retain `"exec"`, `"--json"`, stdin handling, required permission flags, and existing workspace isolation. Preserve effective effort; explicitly set `model_reasoning_effort` through the installed CLI's supported config override if needed.
- [ ] Resolve `command.program` to an executable the Rust process can spawn. The local shell resolves `codex` to a PowerShell shim; this is not proof that `Command::new` can execute that shim. Use the installed native binary or the existing verified launcher. Keep secrets and prompt text out of command arguments.
- [ ] Test this exact launcher and manifest on Windows and the actual Runtime OS. Check account/subscription auth; model selection must not silently change billing mode.

**Deliverable:** the gateway reads current Codex output correctly and requests Astra through the existing subscription route. Commit after the offline tests pass.

## Task 3: Migrate an existing direct API text route

**Run this task for a selected OpenAI direct API route.**

**Files:** modify only the identified operator manifest initially; extend `adapters/model-gateway/tests/byok_adapters.rs` for the Astra request contract. `adapters/model-gateway/src/byok.rs` and `core/gateway/src/manifest.rs` enter implementation scope only if the effort/output-limit requirement below is selected explicitly in the issue.

**Interfaces:** preserve `ByokAdapter::call`, `ModelCall`, and `ModelReply`. Continue to lease the credential through the existing broker.

- [ ] Change the selected route's `model` to `gpt-6-astra`. Preserve `transport: direct_api`, `authentication: api_key`, `billingMode: per_token`, route ID, `credentialRef`, base URL, profiles, and timeout.
- [ ] Use the existing fake HTTP server to capture the serialized Astra request. Assert the existing `/v1/chat/completions` endpoint, exact model ID and prompt, bearer-header placement, and absence of sampling/logprob fields. Keep tests for other models/providers intact.
- [ ] Re-run 401, 403, ordinary 429, quota 429, 5xx, malformed success, and missing-usage cases. Confirm quota failures lead to capacity waiting and make no second paid request.
- [ ] Record the current limitation: the request builder provides no explicit reasoning setting or output cap. Before live use, determine whether the existing effective effort can be preserved with this request. If explicit effort or a hard per-call cap is required, expand the issue with typed, validated route options and serialization tests before activation; do not invent unrecognized JSON fields in the manifest, which denies unknown fields.
- [ ] Run focused checks:

```powershell
cargo +1.97.1 test -p graphhelm-model-gateway --test byok_adapters --locked
cargo +1.97.1 test -p graphhelm-gateway --test manifest_contract --test capacity_mapping --locked
```

**Deliverable:** verified text-request compatibility and a recorded route rollback. Account access and quality remain pending Task 4.

## Task 4: Validate and roll out one route

**Files:** update `docs/acceptance/gpt-6-astra-migration.md`; run existing adapter, gateway, CLI, and full workspace verification. No gate-script changes are planned.

- [ ] Inspect the current local gate and environment instructions. Use a dedicated target directory so another task's build cannot contaminate the result:

```powershell
$env:CARGO_TARGET_DIR = 'D:/graphhelm-target-astra-migration'
cargo +1.97.1 test -p graphhelm-cli --test gateway_cli --locked
cargo +1.97.1 build --workspace --locked
./ci/gate.ps1 > astra-gate.log 2>&1
$astraGateExit = $LASTEXITCODE
Write-Output "Gate exit: $astraGateExit"
```

Read `astra-gate.log` separately. Require zero exit and a valid gate run manifest. A `-SkipPostgres` run is partial and cannot be reported as a full gate. Reuse the existing full gate rather than duplicating its checks unnecessarily.

- [ ] Obtain independent code/security review of the exact diff and test evidence. Review quota pauses, credential isolation, prompt injection, error redaction, and false-success prevention. No schema, persistence, Governor, or Policy Engine migration should be required by the initial change.
- [ ] In an observer-enabled environment, run the frozen corpus through the actual GraphHelm route. Record requested/effective model when observable, CLI version or API response metadata, final text, expected structured values, terminal Runtime state, latency, reported usage, and task success. Run provider calls only with authorized data and the approved billing path.
- [ ] Do not treat `gateway probe`, executable presence, or HTTP 200 as completion proof. Verify a completed model reply reaches the Runtime result. Where an action is promised, observe that action at its real boundary. Report `OBSERVER_MISSING` when the observer is unavailable.
- [ ] Compare against Task 1 thresholds. Keep prompts unchanged for the first comparison; adjust task-specific instructions only for a measured regression, preserving required output schemas, authorization rules, and GraphHelm's evidence gates. Retry evidence must remain linked to its first attempt.
- [ ] Activate only the selected route, using the verified reload/restart mechanism. Let existing attempts finish or pause them through public Runtime controls; do not rewrite in-flight history.
- [ ] Observe the first ten completed tasks. Revert on schema/policy/billing failure, parser errors, repeated capacity misclassification, or a breached comparison threshold.
- [ ] Rollback by restoring the saved route configuration and prior executable when decoder code changed, then reload/restart and run the same small smoke journey. Preserve Astra events and artifacts as historical evidence. Never achieve rollback by silently switching subscription users to BYOK.

**Done:** exact source/config versions recorded; build and required local gate pass; review passes; live journey and quota behavior are observed; comparison meets thresholds; rollback is demonstrated. Any use of the repository's documented PostgreSQL-skip exception must be reported as a partial gate, never a full gate. If only offline checks ran, report compatibility work complete and live migration unverified.

## Separate follow-up: Responses API and native API tools

This is not required for the current text-only migration. If Astra tools or strict per-call API controls are required, open a separately bounded issue and contract design first.

Candidate implementation boundaries are `core/gateway/src/manifest.rs` (explicit endpoint/capability options), `adapters/model-gateway/src/byok.rs` (dispatch), a focused new Responses codec in that adapter, and associated contract tests. A tool loop also needs a provider-neutral conversation/tool-call contract in `core/gateway/src/call.rs` and integration with the existing Tool Broker and execution controller; those changes cannot be scoped accurately as a model-name edit.

The design must specify request/output bounds, supported effort, retention behavior, `max_output_tokens`, ordered response-item parsing, refusals/incomplete responses, usage accounting, tool-call IDs, approval enforcement, cancellation, retry/idempotency, and terminal success. Preserve old routes' endpoint behavior. Do not enable asynchronous tools, mid-turn steering, hosted agents, larger context budgets, or a new SDK incidentally.

## Planning limitations

This plan establishes source-level compatibility work and execution gates. It does not establish production configuration, Astra account access, live parser success, model quality, spend, or latency. No migration issue was published and no service settings were changed. Task 1 makes the remaining operational inputs concrete before implementation.
