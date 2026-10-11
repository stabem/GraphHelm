# Context reuse and native Windows lifecycle evidence

Issue: #786. Observation date: 2026-10-11. Source baseline: `054e3996bcc792e2678d119d74a1d50b12bf1ba2`.

## Promise and boundary

An agent should consume current task evidence before repeating a query. Runtime reuse must not
mix source generations, executions, readers or authorization boundaries, and must not require an
idle provider process. Selected source is still read and each node's capsule compiled with fresh
node identity and budget. Partial results and empty searches do not establish code absence.

The production consumer is `compile_for_node_cancellable` in `core/runtime/src/context.rs`, used
by the drive's context ports. Keel can supply a retained immutable search/reader pair. Live source
ports remain uncached. Reuse is limited to the project pair; execution-tree callbacks keep their existing retrieval. Cache contention bypasses reuse without waiting, so concurrent single-flight deduplication is not promised. The memo holds at most 32 entries and 64 KiB of candidate-path bytes; it stores no source excerpts. This change does not activate the optional Codebase Memory adapter: its
concrete consumer is currently the development benchmark/tests, and ADR-034's activation amendment
remains proposed. Runtime query reuse does not establish production CBM structural retrieval.

## Native Windows observation before implementation

The installed Codebase Memory executable identified itself as 0.10.8. The executable hashes were:

- Provider: `b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6`.
- Installed GraphHelm broker: `f83516fa00efc21cdb53db996268eb0a2110a002c7a2b7a30667aad386963ae6`.

The owner authorized a task-exclusive private root inside the Windows profile. Each case used a
new tiny committed repository, private staging, request JSON and capture directory. Invocation was
`graphhelm --json tool invoke --project <scratch> --staging <private-staging> --request <request>
--actor codex-memory --capability shell.execute --allow-program <program> --capture-out <capture>`.
The broker redirected the provider's home, cache and temporary paths into its owned workspace.
No global MCP configuration, shared ACL, watcher or unrelated daemon was changed.

| Case | Tool request / stimulus | Elapsed | Observed result |
| --- | --- | ---: | --- |
| Normal | `shell`: `codebase-memory-mcp cli list_projects` | 4.984 s | Host 0, provider 0; valid empty isolated store response. Frontend 66552 and daemon 34628 closed. |
| Provider error | `shell`: `codebase-memory-mcp cli search_graph` with an absent project | 4.109 s | Host 0, provider 1, expected project-not-found. Frontend 11440 and daemon 56812 closed. |
| Controller crash | `shell`: Python wrapper launches the real MCP frontend and holds stdin open; stop only the owned broker after its daemon appears | 5.719 s | Forced host exit. Wrapper 58888, frontend 47512 and daemon 24148 closed. |

The observer used native Windows `psutil` 6.1 process inventories, parent chains, PID plus creation
time, and 15 ms polling while the owned broker ran. A bounded final inventory found zero observed
owned survivors in every case. All three pre-existing CBM identities (18544, 72952, 75904) survived
unchanged. No process-name kill or shared daemon stop was used. Short-lived children may escape
polling; the final CBM inventory contained only baseline identities. These durations are measured
case durations, not worst-case product latency guarantees.

Raw reports/captures remain at the owner-authorized profile directory
`.graphhelm/proof-786/{normal,failure,interruption}/`; observer scripts and the cumulative report
remain in lane `codex-memory/issue-786`. Reports contain `ownedSurvivors: []` and
`existingCbmPreserved: true`. Interruption's `outerTimeout: true` denotes the deliberate crash,
not an observed product timeout or cancellation token. Normal/error runs removed execution scratch;
crash left owned scratch directories with no live processes. Automatic crash disk cleanup and
TCP teardown were not independently observed.

Earlier attempts under the shared lane volume failed the provider's ancestor-DACL admission, then
closed their owned children. An outside-broker shared-daemon CLI read closed only its own frontend;
it was not isolated proof. The successful private-root cases supersede that lifecycle blocker for
the listed scenarios and preserve the earlier failed attempts in the cumulative report.

## Observer obligations and recovery

| Obligation | Smallest observer | Failure / recovery |
| --- | --- | --- |
| Normal/error/crash do not leave the tested provider tree alive or stop another session | Native process identities above | Stop only owned broker; preserve baseline. Refuse activation if closure is unobserved. |
| Identical immutable-source queries avoid repeated backend search | Runtime context-chain regression through real compile consumer, with an I/O-counting search port | Bypass bounded reuse when identity is unavailable; compile through existing source fallback. |
| Changed source, execution or reader never reuses another scope | Context-chain regression with changed identities | Recompute from that scope. Never substitute a stale cached capsule. |
| Live sources and search failures remain retryable | Existing/live context-chain observers | Do not cache unavailable/error results. |
| Different node ids and budgets still compile their own capsules | Context-chain compile assertions | Retain fresh source reads and existing budget/refit logic. |

These are ordinary native/automated observations, not a typed JPD certification. No browser screen
is changed. Product cancellation/timeout of the actual provider, successful indexed structural
search, sustained peak memory, cross-drive persistence and token savings remain unobserved.

## Upstream assessment

[ai-memory](https://github.com/akitaonrails/ai-memory) offers useful task handoffs, compact retrieval,
provenance and rebuildable search data. Its [architecture](https://github.com/akitaonrails/ai-memory/blob/main/docs/ARCHITECTURE.md)
also uses a persistent server and background drain work; its [Windows support](https://github.com/akitaonrails/ai-memory/blob/main/docs/windows.md)
is experimental. It is not installed by this change. Reuse those evidence-management ideas within
GraphHelm's existing governance instead of adding broad chat capture or another standing server.
Upstream main documentation was read on the observation date, without a revision pin; these are
upstream claims, not local compatibility tests.

[Codebase Memory's CLI](https://github.com/DeusData/codebase-memory-mcp#cli-mode) can still start or
join a daemon. The native observation above, rather than the label "CLI", establishes the listed
closure results. Different agents still consume model tokens when reading results; neither a
zero-LLM retrieval service nor fewer searches proves lower total model cost.
