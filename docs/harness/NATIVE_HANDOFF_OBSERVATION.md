# Native handoff observation protocol

This protocol implements the current [issue #62 native acceptance observation card](https://github.com/stabem/GraphHelm/issues/62#issuecomment-6151341241). It prepares one bounded Claude-to-Codex work transfer and defines what a future pilot must observe; this document does not run or pass that pilot. A handoff receipt means the adapter retrieved an offer and briefing. It does not mean that a model consumed the task, did the work, or accepted it.

## Current evidence gap (RED)

The records below are the initial obligation-by-evidence table. `Missing` means the cited record does not establish the obligation. Separate records cannot be combined into a single successful chain unless their task, revision, workspace, identities, and evidence links match.

| Obligation | Existing evidence | Status |
| --- | --- | --- |
| One exact task, source revision, workspace, objective, prior evidence, and unresolved duty is frozen before the offer | Issue #62 defines the broader goal and slices. The merged handoff adapter records a bounded offer and briefing digest, but the reviewed issue/PR records do not show a complete pilot card bound to resulting work. | Missing for one pilot |
| Native sender and receiver are identified by their host sessions | The adapter accepts declared host/session labels and validates them against sealed Runtime records. The public contract says owner credentials authorize those labels; they do not cryptographically authenticate a host or model. Historical native launch observations exist, but are not tied to the full chain below. | Partial |
| The intended receiver natively consumes the offer without being given the expected answer | Earlier native receiving/context observations are recorded separately. They do not show the same task proceeding through work and independent acceptance. | Missing for one complete chain |
| Work is produced against the frozen task and revision | Handoff receipts and adapter/Runtime continuity evidence exist. A receipt is retrieval evidence, not resulting work. | Missing |
| An independent reviewer accepts that work against the same task and revision | No cited same-task reviewer verdict is linked through offer, receipt, and resulting work. | Missing |
| Studio shows workspace, responsibility, and remaining proof for that same task | Historical Studio observations cover other issue slices and views. No cited observation links this handoff, its resulting work, reviewer verdict, and remaining duty in one view. | Missing |

This is an evidence gap, not a reproduced production defect or a failing automated test. Therefore RED is the current observation result, not a test failure.

## Frozen pilot card

Before any native attempt, the coordinator records one immutable pilot card containing:

- a unique pilot ID and the exact repository, source commit, and workspace identity;
- the sender's and receiver's actual native host session IDs, kept separate from adapter actor labels;
- one small, independently answerable source-inspection task, its objective, expected answer held outside the receiver prompt, and a completion rule;
- the exact offer, bounded task card, prior evidence references, and exactly one unresolved duty;
- the reviewer identity, acceptance checklist, and Studio fields/screens that must show responsibility and remaining proof;
- a finite limit of **one offer attempt and one receive attempt**. A retry is not allowed; an ambiguous write must be reconciled first, and a missing capability ends the pilot;
- maximum waits: 30 seconds for each native tool/process operation, 15 seconds for each Runtime reconciliation read, and 60 seconds for Studio rendering and capture. These are caller-enforced whole-operation deadlines, not socket inactivity limits. A timeout is `unobserved` until reconciled;
- the evidence directory, owner of every process started, cleanup command/identity, and cleanup observer. No process is stopped unless it was started by this pilot and its exact identity is still verified.

The expected answer is sealed in the card or held by the independent reviewer. It must not appear in the offer, receiver prompt, or briefing supplied to the receiver. The receiver gets enough context to inspect the named source revision and report findings, not the expected conclusion.

## Preconditions and operator sequence

This sequence is for a separately authorized, isolated pilot. It must not use the production Runtime by default. The pilot coordinator supplies an isolated Runtime binding and confirms that the native Claude and Codex sessions are available. If either session, required tool, source revision, reviewer, isolated binding, or Studio observer is unavailable, stop before the offer and record the missing capability.

1. **Freeze and verify the card.** Save the card, expected answer, commit, workspace identity, deadlines, evidence directory, and one-attempt budget. Verify the worktree is at the recorded commit and the task file hashes match the card. Do not edit the task after freezing it.
2. **Observe the native sender.** Record the Claude host's actual session identity from its native session context. Separately record the adapter's declared `--host` and `--session-id` values. These labels are not proof of native identity. Capture the native tool invocation and its bounded result.
3. **Offer once.** Use the installed adapter command documented in the [explicit task handoff contract](../../plugins/graphhelm/README.md#explicit-task-handoff):

   ```text
   python <installed-plugin>/hooks/task_handoff.py offer --host claude --session-id <sender-session> --recipient-host codex --recipient-session-id <receiver-session> --handoff-id <pilot-id>
   ```

   Set the isolated `GRAPHHELM_EXECUTION_ID`, `GRAPHHELM_RUNTIME_URL`, and `GRAPHHELM_TOKEN_FILE` through the authorized host configuration. Never put credentials in the card or prompt. Keep the adapter's actor labels distinct from the native session evidence.
4. **Reconcile before any retry.** If offer output times out or disconnects, do not POST again. Run the documented read-only `status` command for the exact offer, within the 15-second reconciliation deadline. Preserve the original failed attempt and its unobserved write acknowledgement even if sealed evidence later proves the record exists. If no exact valid record can be read, stop.
5. **Observe native receipt and consumption.** In the actual Codex session named by the frozen card, invoke the existing `receive` tool with its actual thread/session metadata. The companion MCP tool list also exposes `offer` and `status`; use `status` only for read-only reconciliation. Save the native call evidence and session identity. Then ask it to perform the bounded source inspection without the expected answer. Capture its response and resulting work with commit or artifact hashes. A Runtime `receive` receipt alone proves adapter retrieval, not model consumption. These tool contracts and the optional `team_report` tool are documented in the [handoff section](../../plugins/graphhelm/README.md#explicit-task-handoff) and [run-team section](../../plugins/graphhelm/README.md#explicit-run-team-and-shared-mailbox); a host-native call must also be evidenced at the host boundary.
6. **Reconcile receipt ambiguity.** On a lost response, do not receive again. Read exact-offer status once within the deadline and preserve the failed native attempt. A matching sealed receipt can establish adapter retrieval, but missing native consumption or work evidence stays missing.
7. **Review independently.** Give the reviewer the frozen task, source commit, expected answer, produced work, and evidence links. Do not let the receiver choose or approve its own result. The reviewer records a verdict against the same task ID, commit, and output hash, plus the reason and any remaining duty. No verdict or a verdict against another revision is not acceptance.
8. **Observe Studio.** Open the existing Studio view for the exact execution/workspace. The [Recorded handoffs view](../../apps/studio/src/components/delegation-tree.tsx) renders reported assignments and reviewer lanes and explicitly treats missing records as unknown. Capture those values as displayed, then record whether produced work, an independent verdict, and remaining proof are actually represented in the view. Do not infer these fields from the tree when they are absent. Record URL/route, viewport, time, screenshot, and relevant console/network errors. An API response, adapter status, or screenshot of another run is not a Studio observation.
9. **Close only the observation.** Preserve all attempt logs, failed outputs, reconciliation reads, hashes, review verdict, and Studio capture. Stop only exact pilot-owned processes after verifying identity and record that they exited. Report each obligation as `observed`, `missing`, or `contradicted`; do not turn partial evidence into a pass.

The existing adapter commands are bounded by five seconds of socket inactivity per Runtime request, not by total process time. The outer deadlines above cover process startup, all requests, and reconciliation reads. The operator must terminate only its own still-running invocation at the whole-operation deadline, record it as timed out/unobserved, and verify its exact child process has exited before cleanup. Never terminate a host session or unrelated Runtime process.

## Evidence chain and interpretation

Every row must link to retained evidence and to the same pilot ID, task, commit, workspace, and output hash. Hashes identify artifacts; they do not prove their meaning.

| Claim | Required evidence | Does not establish it |
| --- | --- | --- |
| Offer was durably recorded | Offer ID, Runtime journal event and opened sealed evidence matching task/execution, addressing, digest, and pilot ID; report write acknowledgement separately | CLI success text alone; a later record with mismatched identity |
| Intended native receiver got the task | Actual native host/session identity plus captured native tool invocation/result bound to the offer ID | Adapter `--host codex` label, Runtime actor label, or receipt by itself |
| Receiver consumed the task and did work | Native receiver response that cites the frozen source revision, plus resulting patch/artifact and hash; expected answer was not in its prompt | Receipt, current briefing digest, tool discovery, or a claim without the output |
| Work was independently accepted | Reviewer identity distinct from receiver, verdict, checklist, task/commit/output hash, and remaining duty | Receiver self-review, unrelated PR approval, or a verdict for a different revision |
| Studio represents the same outcome | Fresh view capture for the matching execution/task and evidence that responsibility, work/verdict, and remaining proof are shown | HTTP acceptance, stale screenshot, or a view from another execution |

**Worked positive interpretation (hypothetical, not an execution receipt):** if all five rows link through pilot `P`, task `T`, commit `C`, and output hash `H`, with native Codex identity matching the frozen receiver and an independent verdict on `H`, then the protocol obligations are observed for that pilot. This does not establish broad reliability, cost savings, or activation in other sessions.

**Negative interpretations:**

- A valid offer and receipt with no native response or output is `adapter retrieval observed; native consumption and work missing`.
- Work against another task ID, source commit, workspace, or output hash is `chain mismatch`; acceptance remains missing.
- A verdict written by the receiver, absent reviewer evidence, or absent Studio capture leaves independent acceptance or Studio representation missing.
- A historical receipt-only result remains partial even if its adapter tests passed.

## Abort and ambiguous effects

Abort immediately on a missing native capability, identity mismatch, unexpected task/revision, deadline expiry, or invalid/missing sealed evidence. Preserve the failed attempt and do not widen the task, change its prompt, or silently retry. For a write with a lost response, perform only the one exact read-only reconciliation described above; do not issue a second offer or receive. Mark the effect `unobserved` if reconciliation cannot establish the exact record. Keep the original failure in the report even when a later read confirms durable state.

If a pilot-owned child does not exit by its deadline, retain its process identity and evidence; do not kill a process whose ownership cannot be proven. The coordinator must arrange safe cleanup before another attempt. Never stop or write to a live Runtime outside the isolated binding authorized for that pilot.

Record actual wall time per step and total time. Record native usage only when the host exposes it with evidence; otherwise write `unknown`. One pilot cannot prove comparative cost. Any later cost study needs a separately frozen baseline, matched tasks, acceptance rule, and authorization.

## Observer and remaining state

The observer for this document is one independent review that checks every command against the linked source contracts and applies the interpretation rules to existing issue/PR records. The native pilot remains pending until a separate coordinator-routed task supplies an isolated binding and executes the protocol.

`OBSERVER_MISSING: same-task native consumption, accepted work and Studio evidence`.
