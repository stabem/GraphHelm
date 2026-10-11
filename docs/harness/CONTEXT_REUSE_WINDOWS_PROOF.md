# Context reuse and native Windows lifecycle evidence

Issue: #786. Observation date: 2026-10-11. Source baseline: `054e3996bcc792e2678d119d74a1d50b12bf1ba2`.

## Promise and boundary

An agent should consume current task evidence before repeating a query. Runtime reuse must not
mix source generations, executions, readers or authorization boundaries, and must not require an
idle provider process. Selected source is still read and each node's capsule compiled with fresh
node identity and budget. Partial results and empty searches do not establish code absence.

The production consumer is `compile_for_node_cancellable` in `core/runtime/src/context.rs`, used
by the drive's context ports. Keel can supply a retained immutable search/reader pair. Live source
ports remain uncached. Reuse is limited to the project pair; execution-tree callbacks keep their existing retrieval. Cache contention bypasses reuse without waiting, so concurrent single-flight deduplication is not promised. The memo holds at most 32 entries and 64 KiB of candidate-path bytes; it stores no source excerpts. The first Runtime memo slice did not activate the optional Codebase Memory adapter: its
concrete consumer is currently the development benchmark/tests, and ADR-034's activation amendment
remained proposed at that head. Runtime query reuse does not establish production CBM structural retrieval.

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
is changed. At the first memo slice, product cancellation/timeout and successful indexed structural search
remained unobserved. The follow-up observations below address those separately. Sustained peak
memory, automatic crash disk cleanup, TCP teardown and model-token savings are not established.

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


## Direct containment follow-up: cancellation and timeout

Source baseline: `0c63bdacf767aefd97a4154eb8219b8bb1958517`. The provider executable remained the
same SHA-256 listed above; no provider upgrade or shared configuration change was made.

The external observer compiled a temporary native Rust integration observer against the actual
`ContainedProviderSession`, `CancelSignal` and `ProcessLimits`. It launched the verified installed
Codebase Memory binary with the fixed three-message MCP request, not a process-tree mock. The
observer raised cancellation only after seeing both owned frontend and daemon alive. Timeout was
the product's own process deadline; an outer safety kill was never counted as a pass. The temporary
provider-dependent test was archived outside the repository and removed before versioned tests.

| Case | Product stimulus | Observer elapsed | Session call elapsed | Result |
| --- | --- | ---: | ---: | --- |
| `cancel-native-4` | Real `CancelSignal.cancel()` after frontend and daemon appeared | 12.547 s | 1.289 s | `cancelled=true`, `timedOut=false`, complete tree termination; zero observed owned survivors; baseline CBM identities preserved. |
| `timeout-native-6` | Two-second `ProcessLimits.timeout`, without raising cancellation | 14.188 s | 2.163 s | `timedOut=true`, `cancelled=false`, complete tree termination; zero observed owned survivors; baseline CBM identities preserved. |
| `cancel-native-3` | Same product cancellation with a verified stdin-holder and real CBM descendants | 6.797 s | 3.217 s | Cancellation and complete closure passed; baseline preserved. |
| `timeout-native-3` | Eight-second product timeout with the same holder and real CBM descendants | 11.594 s | 8.210 s | Timeout and complete closure passed; baseline preserved. |

These direct requests used an absent project to observe lifecycle, not successful indexed search.
The observer's total duration includes private worktree creation and executable hashing; the
process deadline does not bound that preparation. Neither output reader was abandoned or lost.
Raw reports are in `.graphhelm/proof-786/<case>/observation.json` beneath the owner-authorized
private profile root. Scripts, build logs and archived observer source are in the isolated
`codex-memory/issue-786-direct` lane.

Failures remain evidence: `cancel-native-2` failed before provider launch because a Windows Store
Python alias could not be read; the corrected holder used the physical executable. The direct
three-second timeout attempt finished naturally, so it did not observe expiration. The one-second
attempt did expire and close its owned frontend but did not observe both CBM processes alive, so
it was not promoted to daemon-timeout proof. The earlier tester admission request contained a
UTF-8 BOM and failed before launch. No failed setup is counted as a retrieval or closure pass.

A separate installed-broker `index-admission-1` request built a real tiny committed Rust source
with `cli index_repository`: provider exit 0, `status=indexed`, eight nodes and seven edges in
6.719 seconds. All observed frontend, daemon, indexing worker and console processes closed;
baseline CBM identities survived. This establishes the installed builder contract, not yet the new
CLI's end-to-end build/query contract.

## Explicit direct integration

The direct surface is `graphhelm tool code-index build|query`. It is opt-in per invocation, with
explicit project, staging root, bundle, verified executable digest and actor. It does not replace
or restart a shared MCP/Runtime, install a memory server, or silently assemble Runtime context
ports. The bundled Keel skill teaches callers to reuse current evidence and this explicit bundle.
Existing installed skill copies require their ordinary update; this patch does not edit them.

```powershell
$cbm = 'C:/Users/gabri/AppData/Local/Programs/codebase-memory-mcp/codebase-memory-mcp.exe'
$sha = 'b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6'
# Choose private, non-overlapping roots. The bundle must not already exist.
graphhelm --json tool code-index build --project C:/private/source --staging C:/private/staging --bundle C:/private/index --executable $cbm --sha256 $sha --actor my-task
graphhelm --json tool code-index query --project C:/private/source --staging C:/private/staging --bundle C:/private/index --executable $cbm --sha256 $sha --actor my-task --path src/lib.rs
```

The builder indexes a disposable committed-HEAD worktree only after matching its selected bytes
and file population to the actual project's current Git index. Dirty tracked files and staged
additions refuse. A successful builder result is followed by bounded store copying and durable
file writes into a unique temporary sibling; the final bundle appears only after publication.
Failed preparation removes only that invocation's temporary sibling and leaves the destination
retryable. A small per-destination publication lock file remains on disk, without an open handle
between commands. A crash may leave owned scratch; automatic crash disk cleanup is unobserved.

A query verifies the executable, project identity, current tracked population, selected source
bytes and pinned store generation. Successful receipts are capped at 32 evidence entries of
128 KiB each per bundle. Reuse revalidates the exact request and actual broker record against
current source; it reports `reused=true` and `providerInvocations=0`. Different actors, requests,
source/store generations or executable digests cannot borrow that cached receipt. Concurrent
queries against the same bundle refuse contention; separate bundles use independent locks and
workspace IDs. Cancellation/timeout never become success even if a valid reply was buffered.

Chosen project, staging and bundle ancestor chains are owner-trusted and must remain stable.
Descendant source/store reads refuse links, reparse points and special files and enforce file,
entry, path, depth and byte limits while reading. This does not claim adversarial root-chain or
destination-ancestor replacement protection. Sidecar digests detect staleness/corruption, not
forgery by the local bundle owner. Query preparation still copies and hashes bounded data; reuse
eliminates a provider invocation, not all local work. Process timeout starts after preparation.
A preexisting cancel marker refuses before provisioning; a later marker cancels the provider
call, with preparation between these points bounded by its own existing limits rather than this
timeout. No model-token or peak-memory saving is claimed.

### Direct Windows observations

`code-index-final-1/observation.json`, beneath the same authorized private proof root, records:
real index build; a real `src/lib.rs` hit with partial coverage; repeat reuse with zero provider
invocations; staged-addition and dirty-source refusal before CBM launch; preexisting cancellation
without a new bundle or CBM launch; and actual two-second query expiration. The timeout produced
`GHTOOL_CBM_QUERY` with a persisted `timed_out` invocation, and both observed owned CBM processes
closed. An initial post-daemon cancellation marker arrived too late and the query completed
normally; that attempt remains recorded as normal completion, not cancellation proof.

`code-index-live-cancel-1/observation.json` closes that CLI cancellation gap: the marker was raised
after observing the owned frontend; the query returned a persisted `host_error` invocation with
`GHTOOL011_CANCELLED`. No owned process survived and no outer observer kill was used. The earlier
`cancel-native-4` separately observed cancellation after both frontend and daemon were alive.
All three baseline PID/creation identities remained present across these direct CLI cases.

Offline regressions retain the native boundary obligations without requiring an installed CBM:
valid reply followed by process timeout must refuse; snapshot file bounds and Windows child
junction refusal; dirty-source refusal and owned workspace cleanup; failed bundle publication
and retry. The parent provider accepted a buffered valid reply after timeout (RED), and the
parent snapshot copied through a child junction (RED). The corresponding focused checks passed
after correction. ADR-034's unchanged framing ceiling was also exercised: an extra fourth
message failed the exact message-count assertion, and an undeclared client failed population
enumeration; restored guards passed. An earlier line-ending mutation broke the guard harness and
is retained as a setup failure, not the ceiling proof.

`code-index-concurrent-1/observation.json` observes two overlapping direct queries using the same
project and staging root, with separate bundles. One returns `GHTOOL011_CANCELLED`; the other
returns a real `src/lib.rs` hit. All observed owned CBM identities closed, all baseline identities
survived, and the observer killed nothing. The source was then integrated with current main;
reviewed-head checks and a fresh task-binary smoke establish the final source state separately.

The initial reached-test run exceeded its 180-second target (213.953 s, twelve completed checks,
remaining checks pending). It is incomplete, not a pass. An existing reached CLI HTTP target
alone consumed 88 seconds. Completing this selected dependency surface uses an explicitly
recorded 900-second run budget; it does not claim the 180-second target was met. No extra test
scope was added to compensate. A first clippy run also refused an eight-argument composition
function; removing a redundant source identity argument addresses that finding.
