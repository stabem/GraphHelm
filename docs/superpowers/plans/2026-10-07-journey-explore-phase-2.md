# Journey Explore Phase 2 (driver and deterministic replay) Implementation Plan

> **Execution:** follow `docs/process/DELIVERY.md`: issue, proportional Keel card, one independent
> review, merge by the approving reviewer. These tasks are the work breakdown, not a replacement
> workflow. This document is a plan; it does not certify an implemented browser journey.

**Goal:** Replay every path of an approved journey flow in a real browser without a model call,
produce a bounded replay cache, and record sealed screen captures and walked transitions that the
existing `journeys` reader can display.

**Architecture:** Rust owns flow validation, cache custody, process supervision and recording.
`tools/journey-driver/driver.mjs` owns an isolated Playwright browser and implements the closed
`graphhelm-journey-driver/1` JSON-lines protocol. It runs from the project's installed copy at
`.graphhelm/observers/journey_driver.mjs`. The compact YAML remains the source; compiled contracts
remain the unchanged reader projection. No gateway lease, model route or agent loop is involved.

**Tech Stack:** existing Rust CLI dependencies (`serde_json`, `serde_yaml_ng`, `sha2`, `hex`, `fs2`,
`graphhelm-process-tree`), Node.js and the project's `@playwright/test` plus Chromium. No new Rust
dependency. Node/Playwright/browser installation is explicit observer setup, never an offline test
side effect. Record the exact installed Node, Playwright and Chromium versions in browser evidence.

**Spec:** [approved design](../../specs/2026-10-06-journey-explore-design.md), especially §§4–5, 9,
11–12. **Previous format:** [phase-1 plan](2026-10-06-journey-explore-phase-1.md).
**Tracking:** issue #345, Refs #333 and #339; prerequisite [PR #342](https://github.com/stabem/GraphHelm/pull/342).

## Baseline and prerequisite

Inspected `main` at `0b90b6791094e212e57a4a2d241cef7a33266716` and phase 1 at PR #342 head
`d22f315cd374a7d76f705ac13c3fef2b77bdbd46`. Phase 1 is open and blocked at that inspected head;
re-read its final reviewed/merged commit before implementation. The graph index has stale metadata
and no phase-1 module; the interfaces below were checked against committed source, not inferred
from graph coverage. There is no driver or replay command on this baseline.

| Existing source | Actual interface or behavior to reuse |
|---|---|
| PR #342, `apps/cli/src/commands/journey_flow.rs` | Schema-first `serde_json::Value`, not the phase-1 plan's proposed `Flow` DTO. `check(file, project)` is `pub(crate)`; `read`, `semantic`, `compile`, `canonical(value, approval_projection)` and `approval_digest(&Value)` are private. `run_compile` and `run_approve` already exist. |
| Same module | Approval projection omits `status`, `approved` and `drift`; cache must call that digest implementation. File approval supports committed SHA-1 repositories; it refuses SHA-256 Git repositories before writes. Approval revision is provenance, not a requirement that every future HEAD equal it. |
| `apps/cli/src/commands/journey.rs` | `run_capture` and `run_walked` are private behind the public CLI. They call `execution::signal::execute`; capture stamps current HEAD/dirty state and seals an image, walked cites capture IDs of consecutive contract steps. Current walked selects the newest captures in the named run. |
| `apps/cli/src/args.rs` | Existing `JourneyRecordArgs` has required events/execution/keyring/key-id/project/contract fields. Replay needs an optional complete recording bundle, not a blindly flattened required bundle. |
| `apps/cli/src/commands/observers.rs` | `Spec`, `write_script`, `check`, `readiness` and `install` ship embedded scripts and install only on `setup --install-observer`. Current Playwright readiness checks do not establish driver protocol or browser launch availability. |
| `tools/playwright-observer/playwright_observe.py` | `record_journey` invokes public capture/walked commands, checks exit/envelope and retains missing captures. Reuse the recording semantics; do not reinterpret screenshots as promise matches. |
| `core/execution/src/journeys.rs` | `valid_journey_id`; captures expose `freshness`; arrows expose `state: walked|never_walked|stale`. Dirty projects or missing scope paths produce unknown freshness. The reader folds project captures across runs. |
| `adapters/process-tree/src/lib.rs` | `configure` before spawn, `create` after spawn, explicit `close` on every exit. Windows children start suspended until contained; dropping the raw group is not cleanup. Unix containment covers descendants still in the process group, not arbitrary escaped processes. |

The phase-1 BLOCK identifies unsafe output acceptance in `compile --check` and silently truncated
expectation statements. **Implementation depends on their repair and approval.** Do not use the
old plan's truncation recipe, duplicate a compiler in replay, or paper over the defects in this
phase. The plan can merge independently because it adds no runnable interface.

## Global Constraints

- Do not edit the frozen journey-contract schema, digest-bound JPD package or existing event kinds.
  Reuse `jpd.screen_captured` and `jpd.transition_walked`; no new flow-drift kind.
- No new Rust crate, provider dependency, model flag or gateway fallback. Phase 3 owns explore,
  model-based dedupe and redacted model input; phase 4 owns persisted drift/healing; phase 5 owns
  Studio graph/approval routes; phase 6 owns source-scope discovery.
- Draft, stale approval, invalid/noncanonical flow, stale/unreadable compiled contract or unsafe
  cache is refused before browser actions. Runtime refuses unsupported acts instead of guessing.
- Preserve flow/contract bytes during replay. Cache writes never approve a flow or clear drift.
  A missing cache can be derived deterministically from approved acts; a malformed existing cache
  is an input error, not permission to discard evidence silently.
- Bound input before parsing: flow 32 KiB (phase 1), cache 2 MiB, driver frame 64 KiB, redacted ARIA
  snapshot 6 KiB. Retain the flow's 64-screen/128-edge/16-path/8-act bounds. Oversized observations
  fail explicitly; truncation cannot erase an expectation.
- Browser-enabled validation is explicit. The normal Rust suite needs no Node, browser session,
  internet, provider account or credentials. Missing browser capability is `OBSERVER_MISSING`, not
  a passed test and not evidence of an app failure.
- Keep files, snapshots, diagnostics, arguments, logs and cache free of secret values. Images must
  be masked before capture and then sealed. Never send gateway/event keys to the browser driver.
- Consult `AGENTS.md`'s deadline rule: name the longest blocking operation between clock checks.
  A clock around unbounded pipe/storage/git calls is advisory. A claimed run bound must have a
  separately observed supervisor that can stop those calls; do not ship an untested deadline claim.

## Interfaces to implement

All entries in this section are **proposed phase-2 interfaces**, not existing callable symbols.
Document the wire details in design §5 in the implementation PR; the driver is not shipped yet,
so settle this closed version before a producer or consumer lands.

### CLI

```text
graphhelm --json journey replay <id> [--project <root>]
    [--events <dir> --execution <id> --keyring <dir> --key-id <id>]
    [--allow-origin <exact-origin>]...
```

`JourneyCommand::Replay(JourneyReplayArgs)` calls `journey_replay::run`. No `--heal`, `--route`,
`--goal`, `--include-draft`, automatic execution creation or approval. With no recording bundle,
browser assertions run and output says `recording: not_requested`; with all four fields, sealed
recording is required. Any partial bundle is exit 3 before browser/file/store changes.
Without that bundle, do not persist screenshots: observe/assert screens and publish only the cache.
With it, screenshot files are temporary inputs to sealing and never become an unsealed output.

Output command `journey.replay`: flow ID/digest, viewport, `modelCalls: 0`, path outcomes in `main`
then name order, observed screen IDs, captured signal IDs, walked pairs and unresolved steps.
`modelCalls: 0` describes this implementation's absence of model dispatch; it is not sufficient
proof by itself. Exit 0 only when all selected work succeeds; exit 1 for an observed action/assertion
failure; exit 2 for invalid artifacts; exit 3 for unusable input or missing observer. Diagnostics
carry stable code and path, such as `/paths/main/edges/cart.checkout/acts/0`. Recording refusal is
nonzero with partial evidence retained. A deadline termination reports uncertain partial effects.

For concurrent recording safety, extend public `journey walked` with an optional pair:
`--from-capture <signalId> --to-capture <signalId>`. Both or neither; each supplied ID must identify
the correct step of that contract **in that execution**. Existing callers keep newest-capture
behavior. Replay supplies the IDs returned by its successful capture calls, so another writer
cannot silently substitute a different observation. The existing transition document already has
`fromCaptureId`/`toCaptureId`; no wire schema or event-kind change is needed.

### Flow consumption

Expose only the reused `approval_digest(&Value)` and a validated replay loader from `journey_flow`.
Proposed loader: `pub(crate) fn read_for_replay(file: &Path, project: &Path)
-> Result<Value, Vec<journey_validate::Finding>>`. Extract validation of one loaded Value from the
existing check body; validate and return the **same snapshot**, rather than checking once and
re-reading unchecked bytes. Keep canonical output and compilation in their existing owner.
Read approved flow and generated contracts with the phase-1 bounds/refusals, then bind all replay
work to that snapshot/digest. Recheck source/digest before publishing a completed cache; an edit
during replay is a failure with recorded observations retained, not a new implicit approval.

### Driver protocol

One outstanding request at a time. Every line has `protocol: graphhelm-journey-driver/1` and a
monotonic integer `requestId`; both peers reject unknown fields, wrong version/IDs, partial frames,
extra replies and oversized output. Stdout contains only replies; bounded, redacted stderr is
diagnostic input, never another JSON result. End-of-input closes the browser.

| Operation | Closed request payload | Successful result |
|---|---|---|
| `open` | `base`, `viewport`, `allowOrigins` | Opened local page/context; redacted URL. New context per path. |
| `snapshot` | `expect` (bounded role/name pairs; empty allowed for future explore) | Redacted `url`, `ariaYaml`, fingerprint, normalized controls; every requested exact-name expectation is unique and visible. |
| `act` | `kind`, `role`, `name`, optional `text` OR `secretEnv`, optional cached locator | Actual result plus the uniquely resolved locator for cache construction. |
| `capture` | Relative `path` under the supervisor-owned output directory; `maskSecrets: true` | PNG metadata/path only; the image is written after secret masking. |
| `close` | No extra fields | Context/browser released. |

Failed replies contain `ok: false`, code and stable action path, without raw input or exception
stacks. Retain `driver.locator_missing`, `driver.locator_ambiguous`, `driver.host_refused`,
`driver.timeout`; add documented codes for invalid protocol, unsupported act, failed expectation,
wrong screen, oversize snapshot and redaction failure. Phase 4 may translate those facts into
persisted `drift.*` entries; phase 2 stops and reports them without editing the flow.

Driver arguments `--project <root> --output-dir <owned-directory>` are supplied by Rust, without a
shell. Resolve `@playwright/test` from that project's `package.json` using Node's module loader.
Do not rely on the driver's source checkout having `node_modules`. The response additions above
are needed by the real replay/cache consumer; do not add a generic evaluate-JavaScript operation.

Act matrix for v1: `activate` and `submit` click the uniquely matched control; `enter_text` fills
its explicit literal or named secret; `navigate` as an edge act activates its named link (the
path's initial navigation comes from its first screen URL); `wait_for` waits for that exact control
to be visible; `inspect` observes it without a click. `select`, `upload`, `download`, `recover` and
`approve` lack a complete resource/value/recovery policy in this format: preflight refuses them
with `driver.unsupported_act`. Do not synthesize an upload path, pick a default option or silently
click an approval. Add support only when an actual producer can encode and prove its semantics.

### Cache

File `.graphhelm/journey-cache/<id>.json`, separate from `journeys/`. Closed schema
`https://p50.dev/schemas/journey-replay-cache.schema.json`, document version `1.0.0`:

```json
{
  "schema": "graphhelm.journey-replay-cache/1",
  "id": "checkout",
  "flowDigest": "sha256:<64 lowercase hex>",
  "viewport": {"width": 1280, "height": 720},
  "screens": {
    "cart": {"fingerprint": "sha256:<64 lowercase hex>", "controls": [{"role":"heading","name":"Cart"}]}
  },
  "edges": {
    "cart.checkout": [{"role":"button","name":"Checkout","exact":true,"testId":null,"context":null,"nth":null}]
  }
}
```

Each edge's array follows its acts exactly. Each locator has all nullable fields; context is a
normalized nearest-landmark `role "name"` string, never arbitrary CSS. IDs obey `valid_journey_id`.
Fingerprint/control entries are redacted, bounded and deterministically ordered. No screenshot
bytes, literal act text, secret values, credentials, timestamps, absolute capture paths or model
transcript. The flow already contains approved literal text; duplicating it adds exposure/cost.

Fingerprint preimage is the normalized ARIA skeleton: retain landmarks, headings and interactive
`role "name"` pairs, discard free text, mask numeric/date fragments, and collapse repeated list
items into the design's `1`, `2–5`, `6+` buckets. Sort the control set before SHA-256. Keep the exact
unmasked accessible name separately for locator resolution; a masked fingerprint token must never
be used to click a control. Pin normalization with independent fixed skeleton/hash fixtures.
This supplies deterministic cache data; model naming and screen dedupe remain phase 3.

Matching order: cached test ID, then role/exact name within unique context, then global role/exact
name. Only a missing tier can fall back. Ambiguity at any tier is refusal; never use `.first()` or
`nth` to turn >1 matches into success. Record `nth: null` in v1; a non-null unsupported ordinal is
refused, not silently interpreted. A test-ID hit must still satisfy the approved role/name.

Use cache only when schema, IDs, act counts, flow digest and viewport match. A stale digest voids
locator reuse; derive a replacement from approved semantic acts and write it only after successful
replay. First replay without a cache uses role/exact name and observes locators. Failed replay
leaves the prior cache intact. Publish deterministic JSON by checked same-directory temp/write/
sync/atomic replacement under a per-flow writer lock; refuse symlinks/reparse points and paths
outside the cache directory. Cache publication and event appends are **not** one transaction.

Initial viewport is 1280×720; existing valid cache supplies its viewport on later runs. Reset
browser/session state for each path. A first-screen URL with an unresolved `:id`/`:v` cannot be
invented: return an input diagnostic naming the missing concrete entry. Later screens compare
observed paths against the flow's URL pattern, with only declared dynamic segments/query values.

## File Structure

These are implementation scope candidates; each task's issue/card narrows them. This docs PR
changes only this plan.

- Create `schemas/journey-replay-cache.schema.json`; modify `schemas/catalog.json` and
  `schemas/CHANGELOG.md`. Use `graphhelm_schema_evolution::schema_digest`/the existing digest CLI,
  which hash canonical JSON, not raw file bytes.
- Modify `apps/cli/tests/schema_cli.rs`, `core/schema-evolution/tests/catalog_integrity.rs` and
  `core/schema-evolution/tests/baseline_origin.rs` for the exact additive inventory/divergence.
  Main has 22 schemas; phase 1 has 23; this adds one (24 absent other accepted additions). Never
  weaken the inventory assertion or edit `schemas/releases/1.0.0/`.
- Create `tools/journey-driver/driver.mjs`, `tools/journey-driver/driver.test.mjs`,
  `tools/journey-driver/driver.browser.test.mjs`, `tools/journey-driver/fixture-server.mjs` and
  `tools/journey-driver/README.md`. The fixture server uses Node built-ins and serves an owned
  temporary project's static app on `127.0.0.1`; it is an observer fixture, not a product backend.
- Create `apps/cli/src/commands/journey_replay.rs`; modify `apps/cli/src/args.rs`,
  `apps/cli/src/commands/mod.rs`, `apps/cli/src/commands/journey.rs` and phase-1 `journey_flow.rs`.
- Modify `apps/cli/src/commands/observers.rs` to embed/install/check the companion driver without
  changing e2e behavior. Extend its existing tests for installation behavior.
- Create `apps/cli/tests/journey_replay_cli.rs` (offline),
  `apps/cli/tests/journey_replay_browser.rs` (explicitly ignored browser group) and
  `apps/cli/tests/fixtures/journey_replay/checkout-cache.json` (independently reviewed canonical
  cache oracle). Reuse phase-1 checkout flow/contract fixtures and existing producer helpers where
  they cover the obligation; extend `apps/cli/tests/journey_producers_cli.rs` for capture-ID pinning.
- Modify `docs/specs/2026-10-06-journey-explore-design.md` for settled protocol/refusal details and
  `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md` for replay/setup/evidence boundaries. All English.

## Task 1: Cache schema and the phase-1 replay seam

**Files:** cache schema/catalog/changelog; schema inventory tests; `journey_flow.rs`;
`journey_replay.rs`; `journey_replay_cli.rs` and independent cache fixture.

**Interfaces:** `read_for_replay`, the shared approval digest, private bounded cache loading and
semantic validation. Keep fields as Values or small wire types with real producer/consumer calls;
do not recreate a redundant flow DTO.

- [ ] **Step 1 — Write RED coverage.** The approved phase-1 checkout projection loads; drafts,
  stale digest, noncanonical YAML and a generated-contract directory are refused without spawning
  a driver. A valid independent cache passes; unknown nested fields, wrong flow ID, extra edge,
  mismatched act count, `exact: false`, secret/text fields, oversize file and unsafe path fail.
  Extend existing inventory observers instead of inventing a second catalog oracle.
- [ ] **Step 2 — Establish the defect.** Run the focused target on the parent; lack of the new
  replay interface proves only that missing subject, not the later negative behavior. Once wiring
  exists, each refusal test must reach the claimed validator. Use a driver-I/O tripwire so an
  invalid-input test cannot pass after browser startup or unrelated setup failure.
- [ ] **Step 3 — Implement schema/seam.** Register canonical schema digest; name the additive
  divergence and exact live inventory. Reuse the phase-1 digest; validate the same loaded snapshot
  and every generated contract. No truncation or unsafe-output workaround.
- [ ] **Step 4 — GREEN.** `cargo +1.97.1 test --locked -p graphhelm-cli --test journey_replay_cli`
  and `cargo +1.97.1 test --locked -p graphhelm-schema-evolution --test catalog_integrity --test baseline_origin`.
  Re-run `journey_flow_cli`, `journey_validate_cli` and `schema_cli` after inventory/seam changes.
- [ ] **Step 5 — Commit** a buildable schema/seam slice, with identity line and `Refs #<task-issue>`.

## Task 2: Closed driver, exact actions and browser security

**Files:** driver, driver tests, static fixture server, README. Depends on Task 1's wire decisions.

**Interfaces:** the five protocol operations above; exact locator result; bounded, redacted
snapshot/fingerprint and masked capture. Driver never owns GraphHelm event keys or a model route.

- [ ] **Step 1 — RED tests.** Pure protocol tests refuse unknown method/fields, wrong request ID,
  text+secretEnv, nonsecret act payloads with secrets, missing close and oversized frames. Real
  browser tests distinguish `Save` from `Save as draft`, refuse duplicate `Save`, verify context
  and test-ID fallback, assert invisible/missing expectations fail, and check the supported act
  matrix against an actual static page. No fake-browser verdict is visual proof.
- [ ] **Step 2 — Implement routing before navigation.** Use a nonpersistent browser context with
  service workers blocked. Apply context-wide routing before creating pages, including popups,
  redirects, frames and subresources; top-level destinations remain local even when an external
  subresource origin is explicitly allowed. Restrict browser WebSockets before page creation.
  Refuse remote/lookalike/userinfo/encoded-host inputs. Pin approved `.test`/`.localhost` aliases
  to loopback resolution for the validation context; hostname syntax alone is not DNS isolation.
  Refuse a capability/environment where that local-host guarantee cannot be observed.
- [ ] **Step 3 — Browser refusal proof.** A second local canary server observes redirected/popup/
  iframe/fetch/WebSocket requests. Disallowed-origin attempts produce no request at the canary;
  a permitted exact subresource origin succeeds, without permitting top-level navigation there.
  Test a service-worker registration attempt as well. This is request interception, not a claim
  that the Node process is an OS sandbox.
- [ ] **Step 4 — Secret handling.** Pass only secret names actually referenced by selected acts;
  values come from their `GRAPHHELM_SECRET_<NAME>` environment entries. Named missing/empty secrets
  are refused before actions. Reject act literals equal to a supplied secret. Redact known values
  and filled input values before every reply; parent rescans textual payloads. Mask secret-filled
  controls and visible known-secret echoes before screenshots; inability to mask is a refusal.
  Driver environment starts from an allowlist; exclude gateway/event/provider keys, `NODE_OPTIONS`
  and injection-bearing runtime variables. Keep event keys only in Rust/recording children.
- [ ] **Step 5 — Secret observer.** Known canary bytes are absent from stdout/stderr/cache/flow;
  browser assertions confirm the value was actually filled. Independently inspect masked image
  pixels at the fixture's secret field/echo region. Grepping compressed PNG bytes is not proof of
  visual redaction. No new PNG decoder dependency is needed: the browser can decode the captured
  image into a test canvas. Test hidden values and a displayed secret echo separately.
- [ ] **Step 6 — GREEN.** `node --test tools/journey-driver/driver.test.mjs`; in the explicitly
  observer-enabled project, `node --test tools/journey-driver/driver.browser.test.mjs`. Record
  missing Node/package/browser as unobserved, not a green early return. Commit only working acts.

Playwright availability is version/capability-bound: `locator.ariaSnapshot()` was introduced in
1.49 ([official API](https://playwright.dev/docs/api/class-locator#locator-aria-snapshot)); do not
use newer snapshot options without probing them. Context routing misses service-worker-controlled
requests, and WebSocket routing must be set up before the socket is created
([official context API](https://playwright.dev/docs/api/class-browsercontext#browser-context-route)).
Use a compatible installed release and record it; a package folder's presence is not a runnable
capability receipt.

## Task 3: Rust transport, deadlines and owned-process cleanup

**Files:** `journey_replay.rs`, CLI dispatch/args; offline `journey_replay_cli.rs`.
Depends on the settled protocol. Reuse the existing process-tree adapter; do not alter it casually.

**Interfaces:** private driver session/transport with real replay caller; bounded JSON-line I/O,
explicit shutdown and outcome classification. Proposed per-operation limit 30 s and replay work
budget 180 s, monotonic clock. Budget values are runtime-affecting configuration and need behavior
proof. Use small fixed limits in fault tests through ordinary internal parameterization used by
the real runner, not a new public test-only flag.

- [ ] **Step 1 — RED I/O observers.** A portable helper subprocess sends malformed/partial/
  oversized replies, never reads stdin, never sends a reply, exits mid-action or leaves a
  descendant holding stdout. These helpers mock process I/O only. Private transport tests live
  beside `journey_replay.rs` and pass a real `Command` to the same private spawn body the Node
  caller uses; do not add a public fake-driver flag or a test-only export. Verify stable failures,
  finite elapsed time with generous platform tolerance, released descendants/handles and an
  unaffected subsequent replay invocation. Reuse existing containment helper patterns.
- [ ] **Step 2 — Implement supervised waits.** Configure containment before spawn and acquire
  the group before interacting with the child. Bound both write and read halves, including
  startup, snapshot, capture and close. Reader/writer threads communicate over bounded queues;
  the supervisor never joins an unbounded reader while its pipe-owning group is still alive.
  On timeout/EOF/protocol corruption, close the owned group and preserve a failed/uncertain result.
  On setup failure, terminate/reap the suspended Windows child explicitly.
- [ ] **Step 3 — Cover blocking record/file stages.** Public capture/walked child invocations must
  share the deadline and containment. A synchronous cache write, git read or record call outside
  this supervision makes the total budget advisory. Implement a supervised replay worker using
  the same executable to contain those blocking stages: proposed hidden `JourneyReplayArgs`
  flag `--replay-worker`, forwarded only by the ordinary replay supervisor. Artifact loading,
  git reads, cache publication and recording occur inside that worker, not before supervision.
  Its mode has one production caller, no model dispatch and no independent success shortcut.
  Name that new CLI/internal surface in its Keel card. A watchdog kills only its own worker group,
  never a GraphHelm execution or another session. Partial committed effects remain.
- [ ] **Step 4 — Prove the claimed boundary.** Run one test per distinct blocking boundary not
  already covered: stuck write, stuck read and blocked record worker. Name the longest possible
  call between deadline checks in the implementation/PR. Include process startup, shutdown and
  inherited-pipe handling. If any remaining stage is unbounded, label the deadline advisory and
  keep the hard whole-run obligation unresolved; do not claim phase-2 acceptance on that result.
- [ ] **Step 5 — GREEN/commit.** Focused `journey_replay_cli` target, fmt, touched-crate Clippy and
  workspace source guard. Report Linux/Windows containment separately; another OS is a proxy.

## Task 4: Deterministic path replay and cache custody

**Files:** `journey_replay.rs`, phase-1 seam, cache fixture, `journey_replay_cli.rs`;
browser integration target for actual replay. Depends on Tasks 1–3.

**Interfaces:** flow snapshot → ordered paths → initial screen → all edge acts → destination
assertions → staged cache. Rust maps screen/path/edge IDs; Node resolves actual browser controls.

- [ ] **Step 1 — RED planning/cache observers.** Extend the offline target for `main`-first path
  ordering, every act in a multi-act edge, every expectation, unsupported acts, draft with a
  matching cache, stale flow digest, missing dynamic entry and invalid/incomplete recording
  arguments. Cache output is checked against the independent fixture, not copied from a run.
  Driver stubs prove sequencing/refusal only, never that screens rendered.
- [ ] **Step 2 — Implement replay.** Preflight all paths and capabilities before side effects.
  Open a fresh context for each path, navigate its concrete entry, assert initial expectations,
  then execute every act in each edge. After each act inspect the resulting state; after the
  edge assert its destination URL and all exact-name visible expectations before capture. Stop
  at the first failure and list later steps as unobserved. Do not guess a dynamic URL or action.
- [ ] **Step 3 — Seed/use cache.** Missing cache derives locators from unique approved role/name
  matches. Use cached test ID/context/global matching only while its digest is valid. Normalize
  accessibility controls for deterministic fingerprints/cache data; model-based naming, Jaccard
  screen dedupe and the measured 0.8 drift policy remain later work. Do not infer a screen-change
  verdict from an unimplemented threshold or silently retarget an ambiguous control.
- [ ] **Step 4 — Publication/fault proof.** Keep previous bytes on invalid input, action failure,
  write refusal and source edit during replay. Publish the replacement cache only after all
  paths pass. Real filesystem sabotage tests cover a directory/symlink cache target, oversized
  cache and conflicting writers. Readback must remain a complete valid cache, not a partially
  rewritten JSON file. Report cache/event atomicity honestly; do not erase earlier records.
- [ ] **Step 5 — GREEN.** Focused offline target plus real-browser happy path. Two runs have
  identical semantic actions, observed path sequence and canonical cache bytes. Do not assert
  cross-platform screenshot byte equality, random capture IDs or wall-clock record timestamps.
- [ ] **Step 6 — Commit** replay/cache behavior with actual caller paths and proving commands.

## Task 5: Sealed captures, explicit walked links and accurate freshness

**Files:** `journey_replay.rs`, `journey.rs`, `args.rs`, existing `journey_producers_cli.rs` and
new browser integration target. Depends on successful replay and the phase-1 compiled contracts.

**Interfaces:** existing public `journey capture` and `journey walked` arguments, optional pinned
capture pair above, existing image attachment/sealed-signal core and existing `journeys` output.

- [ ] **Step 1 — RED producer regression.** Extend producer coverage: supplying a mismatched/
  foreign-run/foreign-step capture ID or only one ID fails before append; a valid explicit pair
  is retained even if a newer unrelated capture appeared. Existing newest-pair callers stay green.
  The current interface lacks pinning, so name that coverage gap and inspect persisted references.
- [ ] **Step 2 — Record after observation.** For every passed screen on each path, invoke the
  public capture command with its compiled contract ID, screen step ID, masked PNG, viewport and
  complete recording bundle. Check exit, envelope and `data.outcome: recorded`; Governor
  `decision: rejected`/`signal_not_actionable` is normal for an appended evidence-only signal.
  Preserve the returned capture ID. Then invoke walked for that path's consecutive pair with
  those IDs. Never record an arrow for a failed/unobserved destination or use captures from a
  different path contract. Named path `guest` records under `<flowId>.guest`, not `<flowId>`.
- [ ] **Step 3 — Persistence observer.** A fresh CLI `journeys` process reads the event store
  after replay; decrypt/read image evidence through existing public evidence support, verify PNG
  dimensions and masked content, and inspect capture/transition references. Process exit or
  HTTP acceptance alone is not sufficient. No new record schema or fake certification event.
- [ ] **Step 4 — Freshness fixture.** Build a committed temporary app with real nonempty scope
  paths. Commit approved flow and compiled contracts before replay; the approval revision remains
  provenance. Ignore only test-owned cache/observer/node_modules/output directories before that
  commit; keep events/keyring/scratch outside the app. Require a clean tree at capture time.
  Assert captures `freshness: fresh` and arrows `state: walked` at unchanged HEAD. A dirty-source
  variant remains `unknown`; a committed in-scope edit makes captures/links stale. Reuse existing
  freshness tests for the fold itself; add only replay-to-reader integration gaps.
- [ ] **Step 5 — Partial/uncertain recording.** Refused capture records no arrow; refused walked
  retains prior captures. Timeout after append may leave durable records: report uncertainty and
  do not blindly replay that mutation. Reconcile later with the same store/public surface. Earlier
  captures remain append-only; a later failed run does not erase them or force historical fresh
  captures to become missing. Output distinguishes this run's unobserved steps from reader history.
- [ ] **Step 6 — GREEN/commit.** Producer target and existing `journeys_surfaces`; opt-in real
  browser integration verifies captures across two isolated replay runs and the existing reader.

## Task 6: Ship the driver through explicit observer setup

**Files:** `observers.rs`, embedded driver, existing observer unit tests and driver README.
Depends on the functioning driver; no installation-only success handler.

**Interfaces:** `setup --install-observer playwright` installs the existing browser dependency
and writes both the Python observer and `journey_driver.mjs`. Readiness/preview lists the driver
version/script mismatch and the existing explicit install commands. Replay checks runnable
capability at invocation; preview file/PATH checks alone are not browser observation.

- [ ] **Step 1 — RED behavior.** Existing setup fixtures show the new driver's absence/version
  mismatch as missing. Installation writes exactly the embedded bytes to the requested project,
  retains the old Python observer and leaves optional e2e behavior intact. Offline tests intercept
  install process I/O; they do not npm-install or launch a browser. Do not add source-spelling tests.
- [ ] **Step 2 — Implement** the companion artifact in the existing installer body rather than
  a separate domain package. Read-only preview never installs or silently replaces files. Replay
  refuses missing Node/package/browser/driver with actionable `OBSERVER_MISSING`; it never npm
  installs itself. No extra model key or project-root GraphHelm source checkout is required.
- [ ] **Step 3 — Installation observer.** In the explicitly authorized validation project, run
  setup and then real driver startup/snapshot/capture/close. Check the installed executable copy,
  not just `tools/journey-driver/driver.mjs`. Missing or incompatible capabilities remain unresolved.
- [ ] **Step 4 — GREEN/commit.** Existing observer unit tests, offline replay target and opt-in
  installed-driver browser proof; record the exact dependency/browser versions.

## Task 7: Model-free browser acceptance, docs and delivery

**Files:** opt-in `journey_replay_browser.rs`, driver browser tests/fixture, README, design and JPD
docs. Depends on all preceding tasks. Run the smallest adequate set; no blanket test quota.

- [ ] **Step 1 — RED complete journey.** Fixture app `/cart` → `/checkout` → `/orders/42`, exact
  headings/buttons, a two-act fill+submit edge and a named second path. Use a temporary committed
  SHA-1 project and approved phase-1 flow; no production account. An initial command-not-found
  failure documents missing subject only; after wiring, assert real rendered outcomes and records.
- [ ] **Step 2 — No-model tripwire.** Run twice with no usable model credential/lease. Keep a
  loopback gateway/provider stub which fails and counts every request, using the existing route
  fixture/manifest conventions if a Runtime is attached. Prove the counter is live with an
  independent positive-control request, reset it, then require zero during both replays. Inspect
  replay architecture: no route resolution, broker lease or SDK/model executor call. Do not add
  an unused replay route flag merely to feed a stub. Stub silence alone is not an exhaustive
  proof of all network activity; combine it with successful credential-free replay and boundary
  inspection. App network traffic is governed separately by the driver origin policy.
- [ ] **Step 3 — Final observer.** First replay derives the canonical cache; second reuses it.
  Both render all expectations, record masked sealed captures and explicit walked links. Fresh
  reader processes show both path contracts, fresh captures and walked arrows. A missing control,
  ambiguous control, unsafe navigation, missing secret, failed record and missing browser each
  retain their own failure/missing classification. No heal, persisted flow drift or model recovery.
- [ ] **Step 4 — Document** install prerequisites, public replay syntax, optional recording,
  cache invalidation, supported/refused acts, partial effects, time-budget boundary, observer
  versions and `OBSERVER_MISSING`. Captures/arrows are rendering/traversal observations, not an
  authoritative generic JPD acceptance receipt. The missing generic deterministic matcher remains
  `EVIDENCE_MATCH_EVALUATOR_MISSING` advisory; do not invent `quality certify` support for replay.
- [ ] **Step 5 — Final proof** on the committed head, list commands/results and preserve all RED,
  retry and missing-observer evidence. One independent reviewer runs reached checks, answers any
  BLOCK, checks Keel/closing intent and merges only the reviewed head. Parent #333 stays open.

## Test-audit ledger for planned additions

Each implementation PR answers contract/regression/gap/seams and actual cost for its new/changed
tests. The table selects observers; it does not pre-approve a number of tests. Reuse existing
observers and combine cases at the same boundary. No test deletion is planned.

| Obligation | Credible defect and coverage gap | Smallest adequate observer / cost / seams |
|---|---|---|
| Cache/approval binding | Wrong digest or malformed cache drives stale locators; phase-1 tests observe no replay cache. | Offline schema + real CLI refusals and independent canonical cache; milliseconds/seconds after build. No public test-only API. |
| Exact semantic actions | Fuzzy/first-match selection clicks the wrong control; current capture producers never locate controls. | Real static browser Save/Save-as/duplicate/context cases; seconds, Node/package/Chromium required. |
| Host isolation | Page-only routing misses a popup/worker/WebSocket or redirects to a disallowed host. | Real browser plus local request canary; seconds, no external host needed. Pure allowlist parsing is insufficient. |
| Secrets | A filled/echoed value survives text serialization, env inheritance or PNG masking. | Fixed canary, actual fill, output/cache inspection and independently decoded mask pixels; browser required. No provider key. |
| Deadline/cleanup | Blocking stdin/stdout/recording outlives a clock or leaked pipe descendant. | Portable subprocess I/O faults, elapsed bound and subsequent run; seconds, no Node/browser in offline target. |
| Durable observations | A failed destination gets an arrow or concurrent capture replaces the actual pair. | Existing producer tests extended for pinning, then browser-to-fresh-reader sealed evidence; seconds/minutes for browser group. |
| Accurate freshness | Generated/cache files dirty the proof fixture or unknown scopes get called fresh. | Existing fold tests reused; committed-app replay reader checks known scopes/clean HEAD and one dirty variant. |
| Zero model use | Replay accidentally takes a gateway lease/call or needs a provider key. | Two successful real replays without credentials plus live tripwire counter and code boundary inspection; no simulated browser verdict. |
| Installation | Setup writes only the old observer, reports files as ready without a runnable driver. | Existing setup I/O fixtures plus installed-copy browser startup; offline and observer-enabled results separate. |

## Proving Commands and Environment

These commands are **future implementation validation**, not results of this documentation PR.
Use an owned worktree and task-specific short Cargo target on Windows. Do not share another lane's
target or invoke GitHub Actions. Browser artifacts, keys and event stores stay outside tracked
source unless they are approved canonical fixtures. Install browsers only in the named validation
environment; normal offline Rust tests never install tools or require a live browser.

```powershell
$env:CARGO_TARGET_DIR = 'F:/gh-<lane>-phase2-target'
$env:CARGO_BUILD_JOBS = '1'
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_replay_cli
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_flow_cli --test journey_validate_cli --test journey_producers_cli --test journeys_surfaces --test schema_cli
cargo +1.97.1 test --locked -p graphhelm-schema-evolution --test catalog_integrity --test baseline_origin
cargo +1.97.1 test --locked --all-features -p graphhelm-cli -p graphhelm-schema-evolution
cargo +1.97.1 test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --locked -p graphhelm-cli -p graphhelm-schema-evolution --all-targets --all-features -- -D warnings
git diff --check
```

The browser target uses `#[ignore = "requires an explicitly observer-enabled validation project"]`;
offline runs report it ignored. Explicit selection must fail with a clear missing-capability reason
if prerequisites are absent; no conditional early return marked `ok`. The harness owns fresh temp
projects, reuses the declared validation toolchain without modifying another project, and cleans
only its own process groups/files. Start with a compatible installed Node/Playwright/Chromium in
`<validation-project>` and record their versions and a real launch capability receipt.

```powershell
# Explicit observer-enabled environment only; setup may use npm/network.
graphhelm --json setup --project <validation-project> --home <validation-home> --install-observer playwright
$env:GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT = '<validation-project>'
node --test tools/journey-driver/driver.test.mjs
node --test tools/journey-driver/driver.browser.test.mjs
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_replay_browser -- --ignored
```

Do not call a skipped browser target a replay pass. Record `passed`, `failed`, `ignored/skipped`
and `unobserved` separately, including platform coverage. A later GREEN keeps the earlier RED and
retry reason. `graphhelm keel check --diff <base>..<head> --card <card.json>` reports declared
surface; the independent reviewer runs it. New schema/CLI flags/worker mode are named explicitly
in the relevant card and PR, with exact paths rather than globs.

## Review Focus and Phase-2 Completion

1. Actual phase-1 Value APIs and shared digest, repaired projection checks; no old truncation/DTO
   assumption and no change to frozen contract schemas or release snapshots.
2. All acts and expectations execute/observe; exact-name locator ambiguity fails; unsupported
   semantics never become a guessed action. Cache is derived without model assistance.
3. Routing is established before requests; secret privacy is proved on actual text/image/env
   boundaries. Node dependency installation and browser execution stay opt-in.
4. A replay deadline covers the longest blocking call through a proven supervisor; uncertainty
   and remaining containment limits are stated. No raw handle leak or blind retry after append.
5. Sealed captures and pinned transitions are read back by an independent process. Freshness has
   honest scope/Git preconditions; historical records remain append-only after failures.
6. Installed driver works without the GraphHelm source checkout, and two credential-free real
   replays satisfy both path contracts with zero requests at the live model tripwire.

Phase 2 is implemented only after its code is merged and the browser obligations above are actually
observed. An open PR, schema pass, driver stub or screenshot alone is not that completion. If the
observer or hard-timeout boundary is missing, retain the unresolved obligation and report it.
Formal generic JPD certification remains separate from ordinary browser/test evidence.
