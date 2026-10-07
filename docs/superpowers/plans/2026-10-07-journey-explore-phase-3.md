# Journey Explore Phase 3 (agentic exploration and bounded healing) Implementation Plan

> **Execution:** follow `docs/process/DELIVERY.md`: issue, proportional Keel card, one independent
> review, merge by the approving reviewer. Tasks below are a work breakdown, not a second delivery
> protocol. This documentation change implements nothing and certifies no browser journey.

**Goal:** An operator names a local app, goal and gateway route. The agent observes the real browser,
proposes bounded semantic actions, and writes a canonical draft journey flow plus a digest-bound
cache. Deterministic code owns identity, permissions, validation and publication. On replay drift,
an explicitly requested heal may repair only the failed edge and requires renewed owner approval.

**Architecture:** Rust retains model calls, credentials, artifact custody, supervision and recording.
The installed phase-2 Node/Playwright driver retains browser execution through its closed protocol.
Reuse the existing gateway-backed `DraftModel` door and recorded-model fixture format; do not create
a provider adapter, generic browser tool executor, domain pack or duplicate flow compiler.

**Tech Stack:** existing CLI Rust dependencies and gateway/process-tree adapters, Node and the
project's installed Playwright/Chromium. No new Rust dependency is assumed. Any dependency actually
needed must be justified, named and reviewed. Browser setup remains explicit and separate from
offline tests. Record exact Node, Playwright and Chromium versions in browser evidence.

**Spec:** [approved design](../../specs/2026-10-06-journey-explore-design.md), particularly §§5.1,
6, 8 and 11–14. **Previous format:** [phase-2 plan](2026-10-07-journey-explore-phase-2.md).
**Tracking:** issue #351, Refs #333, #347 and #349; phase-2 prerequisite
[PR #348](https://github.com/stabem/GraphHelm/pull/348).

## Baseline and prerequisite

Inspected public `main` at `d5bbe1702dbebfcc1d697d51bcc3d0cc5bc451c8`. Phase 1 landed through
PR #342; PR #350 corrected its refused-compilation write counter. Phase 2 is not merged at this
baseline. Its interfaces below were inspected in committed PR #348 head
`c15161adb520d6d1076f0d7739c30370e3ed084b`, which has an open security BLOCK. Before implementing,
fetch successfully, read the repaired reviewed/merged head, and update these seam assumptions.

The graph index has missing/changed metadata for these paths. Discovery used the graph for the
existing gateway model door, followed by direct committed-source reads. No negative or exhaustive
architecture claim rests on that index.

| Source inspected | Actual interface or constraint |
|---|---|
| Main, `apps/cli/src/commands/architect.rs` | `pub(crate) build_model(&ModelSource)` returns `Box<dyn DraftModel>`; `ModelSource::Fixture` reads a recording, `Gateway` resolves manifest/route and optional broker/keyring/key-id. Direct API uses a broker lease and `ByokAdapter`; native runtime uses `RuntimeAdapter` without a BYOK lease. Errors retain `GHCLI009`/`GHCLI010`. |
| Main, `core/architect/src/model.rs` | `DraftModel::draft(&str)` returns `DraftReply {text, usage}`. `RecordedDraftModel` reads `{"replies":{"<full-prompt-sha256>":"<text>"}}`; it has no prompt transcript. Add transcript observation at the model I/O boundary in tests, not a claim that this fixture already records inputs. |
| Main, `journey_flow.rs` | Flows are validated `serde_json::Value`s. `check`, `run_compile` and `run_approve` are crate-visible; `canonical` and `approval_digest` are private on this baseline. The shared renderer/digest must remain the owner of canonical bytes and approval projection. |
| PR #348, same module | Adds crate-visible `read_for_replay`, `approval_digest` and `atomic_write`; `canonical` remains private. Replay loading requires approved artifacts. Explore/heal need a deliberately draft-capable validator/writer, not weaker checks in `read_for_replay`. |
| PR #348, `journey_replay.rs` | Only `run(&JourneyReplayArgs)` is crate-visible. `Driver`, cache validation/loading, observation, safe environment and supervisor bodies are private. Do not pretend exploration can already call them. Extend visibility only for real explore/heal callers, or extract the smallest shared body while preserving replay behavior. |
| PR #348, installed `journey_driver.mjs` | Versioned, request-ID-bound `open`, `snapshot`, `act`, `capture`, `close`. Empty `snapshot.expect` is allowed. Snapshot returns redacted URL/ARIA, fingerprint and normalized controls; act returns the actually resolved exact locator. It does not classify screens, execute model replies or approve flows. |
| PR #348, `args.rs` | `JourneyReplayArgs` has id, project, optional complete recording bundle, allow-origin and hidden replay-worker; it has no route, heal or goal. Phase 3 must add real parser/dispatch callers, not merely flags. |
| Main and PR #348 | Flow v1 and replay-cache v1 are closed schemas. Paths cannot revisit a screen; edges hold 1–8 acts; a screen has 1–8 exact visible expectations and `scope: unknown` or existing concrete paths. Frozen journey-contract and existing capture/walked event contracts stay unchanged. |

**Blocking prerequisite:** PR #348's capture can report `masked: true` while a known entered secret
is readable in a same-origin iframe. The repairing PR must demonstrate frame-wide masking or a
stable fail-closed refusal with independently decoded image pixels. Phase-3 text redaction must
also cover frame observations before model dispatch; a screenshot fix is not text proof. No explore
or heal implementation may reuse the blocked capture as a trusted privacy boundary. This plan may
merge independently because it adds no executable behavior.

Design §12 originally separates exploration (phase 3) from drift/heal (phase 4). The coordinator's
order for this plan explicitly includes heal. Tasks 1–5 retain the exploration slice; Task 6 adds
the design's edge-local recovery slice with its own proof. This does not silently mark historical
phase 4 delivered. Studio graph/approval UI and automatic framework scope mapping remain later work.

## Global Constraints

- Preserve D-057: compact YAML is source; frozen contracts are generated projections. No edits to
  the frozen contract schema, release snapshots, digest-bound JPD package or capture/walked kinds.
- Model output is a proposal. Deterministic code validates closed JSON, exact action semantics,
  permissions, identifiers, graph/path invariants and artifact custody before any driver act.
- Only the explicitly selected enabled route may dispatch. Preserve the broker/native transport
  choice and quota refusal; subscription exhaustion stops/pauses, with no automatic paid fallback.
- Driver environment starts from the phase-2 allowlist. No model credential, broker passphrase,
  event key, `NODE_OPTIONS` or broad inherited environment reaches the browser process.
- Local base/navigation and exact subresource-origin policy apply to explore and heal as to replay.
  An `--allow-act` match never grants an origin, upload, shell, secret, provider or approval right.
- No screenshots go to the model. Redact and bound all text, including goal, history, screen
  metadata, URL, current snapshot, model replies and diagnostics; refuse surviving known secrets.
- Retain 32 KiB flow, 2 MiB cache, 64-screen/128-edge/16-path/8-act/8-expect schema limits,
  64 KiB driver frames and 6 KiB snapshots. Do not truncate an obligation or record guessed locators.
- Browser-enabled validation is opt-in. Offline tests require no network, browser, account or key.
  Missing browser capabilities remain `OBSERVER_MISSING`; an ignored test is not a passed journey.
- A valid partial draft is not success. Preserve failed attempts, incomplete work and uncertainty
  after browser acts or event appends; never retry a possibly committed mutation blindly.
- Deadline checks must bound the longest blocking call, not merely when it starts. Gateway calls,
  lease/open, file/git operations, process startup, capture/sealing and cleanup must be within the
  observed supervisor boundary. Do not borrow replay's timeout constants as proof of model timing.
- Do not weaken accepted promises to fit a fixture. No generic JPD certification claim follows
  from a model `done`, a draft flow, HTTP acceptance, an image or a test-only transcript.

## Operator Journey and Proof Obligations

Actor: a developer/operator with an explicitly observer-enabled local app and selected route.
Entry: `graphhelm --json journey explore ...`; healing enters through `journey replay ... --heal`.
Preconditions: repaired/merged phase 2, installed runnable driver, local concrete entry URL, valid
flow id, finite goal, named secret inputs and complete recording bundle when recording is requested.
No owner approval is manufactured by exploration. Screens below are CLI-visible states, not a new
Studio screen contract; artifact scopes refer to the files that own those states.

| Obligation ID / step | Visible and durable fact | Failure, safe stop and recovery |
|---|---|---|
| `explore.preflight` / prepare | Invalid route/host/id/bundle/secret/regex refuses before browser or artifact effects. | Stable diagnostic at the relevant argument; correct input and invoke anew. No automatic install or route substitution. |
| `explore.observed` / observe and act | Each accepted act addresses a unique visible exact control, then a bounded snapshot establishes the resulting screen. | Missing/ambiguous control, disabled/loading timeout, unsafe navigation or unsupported act stops. Previously observed actions remain facts; later screens are unobserved. |
| `explore.private` / propose | A captured model-I/O transcript contains only redacted text; no credential reaches the browser; sealed images contain no readable known secret. | Surviving value or uncertain masking refuses before dispatch/capture. Repair the privacy boundary; never relax it to complete the goal. |
| `explore.draft` / finish | Canonical YAML is `draft`, `approved: null`, `drift: []`; cache binds its shared approval digest; identifiers and expectations came from actual observations. | Budget/goal/give-up failure is nonzero, with only a valid explicitly partial prefix eligible for publication. Invalid or unrepresentable prefix leaves existing artifacts intact. |
| `explore.replayable` / owner review | Owner explicitly approves/compiles a complete draft; two later deterministic replays render the recorded path with zero model dispatch. | Approval refused or replay failed remains separate from exploration. Inspect/edit and re-approve; do not silently approve on `done`. |
| `heal.edge_local` / recover | Only the failing edge's acts change after its original destination URL/expectations are observed; drift history remains and flow becomes draft. | At most eight recovery acts; failure retains broken edge and stops that path. New owner approval is required before a later ordinary replay. |

Per-action settle limit starts from phase 2's 30 s. Proposed exploration run limit is 180 s;
`--max-steps` defaults to 40, accepts 1–128 and counts every model turn (including naming, retry,
done/give-up) as well as limiting executed acts. Heal consumes at most eight acts within the same
supervised invocation budget. These are proposed runtime limits needing fault evidence, not measured
results of this plan. Loading/disabled/empty/error/retrying/partial-success/recovered/success states
are retained when actually observed; no model can turn a missing state into an observed success.

## Interfaces to Implement

All additions below are proposals until their implementation merges. Final PRs must name new flags,
crate-visible seams and files, and settle exact diagnostics before exposing producer/consumer pairs.

### CLI and model door

```text
graphhelm --json journey explore --base <local-url> --goal <text> --id <id>
    [--project <root>] --manifest <manifest> --route <name>
    [--broker <dir> --gateway-keyring <dir> --gateway-key-id <id>]
    [--secret <name>]... [--allow-act <regex>]... [--allow-origin <exact-origin>]...
    [--max-steps 40] [--events <dir> --execution <id> --keyring <dir> --key-id <id>]

graphhelm --json journey replay <id> --heal --manifest <manifest> --route <name>
    [--broker <dir> --gateway-keyring <dir> --gateway-key-id <id>]
    [--allow-act <regex>]... [existing replay/recording arguments]
```

Proposed separate gateway-keyring/key-id names avoid colliding with the existing evidence-recording
keyring. Internally map them to `architect::ModelSource::Gateway` fields. `--secret` declares names,
never values; values are read from `GRAPHHELM_SECRET_<name>`, with the existing valid-id/env-mapping
constraints checked before use. Do not offer `--password`, raw-value arguments or arbitrary env
forwarding. The fixture-model option follows existing architect fixture conventions, conflicts
with the live route door, and is recorded-model I/O only, never a fake-browser success mode.

Route flags without explore or `--heal` are invalid. Ordinary replay's unchanged path resolves no
manifest, leases no credential and makes no model call. For heal, validate argument shape before
replay, but do not open/lease/call the model door until a drift occurs. Missing route metadata is
`GHCLI009`; credential/broker refusal retains `GHCLI010`. Explicit standalone CLI route selection
does not implement the deferred Studio default-route behavior.

Command output `journey.explore` names id/digest, outcome, turns/acts/model calls/usage actually
observed, screens/edges, published artifacts, recording status, unresolved obligations and partial
effects. No prompt/reply/raw secret is emitted. Exit 0 means the observed valid draft was completed,
not approved/certified; 1 is observed goal/action/budget failure; 2 invalid flow/cache; 3 unusable
arguments/capability. Reuse stable `explore.*`, `driver.*`, `flow.*` and `drift.*` codes; add named
permission/redaction/publication diagnostics with pointers, not free-form provider errors.

### Closed model proposal and prompt

Reuse design §8's exclusive variants: `{act: {...}}`, `{newScreen: {id, title}}`, `{done: true}`,
`{giveUp: <code>}`. Reject extra fields, multiple variants, wrong types, unsupported acts, oversize
JSON and invalid literals before side effects. Give-up codes come from a small deterministic list;
arbitrary model strings must not become output/logs. One invalid-reply retry costs one turn and
must not repeat a browser action. Incomplete gateway output is a refusal, not parsed success.

Model input: bounded redacted goal, visited `id url` lines, current 6 KiB ARIA snapshot and last
three acts. Use fixed prompt ordering and data delimiters; page text is untrusted data, not an
instruction source. No screenshot, replay cache, environment dump, broker data or file contents.
The orchestrator rechecks all assembled text immediately before `DraftModel::draft`. On response,
recheck secret literals and validate the flow act schema plus driver capability/permission policy.
Snapshot or model strings cannot authorize a delete, different host or new secret name.

`newScreen` names an already observed identity; it cannot change fingerprint, URL, expected facts
or earlier IDs. `done` is not evidence: accept it only after the current bounded path is valid and
its visible expectations have been observed. Describe completion as the agent stopping at that
observed state, not deterministic proof of an arbitrary natural-language goal. A goal requiring an
unavailable external fact remains unresolved. Persist `state: stable` when no stronger state is
grounded; never infer a generic success state solely from `done`.

### Identity, deduplication and graph construction

- Reuse the phase-2 normalized controls/fingerprint producer, not a second normalization algorithm.
  Keep exact original role/name for actions and expectations; masked numeric tokens cannot click.
- Deterministically normalize observed local URLs: path-only, numeric/UUID/hex segments to `:id`,
  fragments dropped. Retain a query key only when an actually observed query change distinguishes
  a screen, with `:v` as specified; never put a secret or sensitive dynamic value in the pattern.
  The concrete first entry must remain replayable; refuse an unresolved dynamic initial URL rather
  than inventing an ID. Do not silently change already written patterns on a later observation.
- Same URL pattern and Jaccard control-set similarity at least 0.8 reuse the identity. Compare
  integer counts (`5 * intersection >= 4 * union`) rather than platform-dependent float rounding.
  For an empty union, reuse only when fingerprints match; otherwise distinguish. Different URL
  patterns or similarity below threshold create a screen. Tie-break candidates by stable id.
- Slug the proposed id deterministically, validate every base/suffix/composed id against the
  consumer rule and length bound, then append the smallest available numeric suffix. Earlier ids
  never change. No traversal, truncation collision or filename derived from unvalidated model text.
- Derive 1–8 expectations from unique visible exact role/name pairs, deterministically prioritizing
  the observed main heading and meaningful controls. Redacted placeholders are not exact visible
  labels; reject an expectation requiring a runtime secret. `scope: unknown` remains honest until
  phase 6 maps source files. A normalized control count alone is not visibility/uniqueness proof.
- Coalesce acts while the observed screen identity stays the same; close an edge only after a
  distinct destination is observed. Enforce eight acts before another act can exceed representation.
  Main is the actually traversed acyclic path, not model-invented alternate branches. Revisit or a
  zero-edge already-satisfied goal cannot be fabricated into a valid v1 path; report unrepresentable
  work. Do not create a self-edge, duplicate screen or no-op click to satisfy the schema.
- Cache entries contain observed locators and redacted controls, bind the shared approval digest
  and reuse phase-2 viewport/custody rules. No model transcript or duplicated act text enters cache.

The 0.8 threshold is accepted provisional policy, not measured accuracy. The final validation task
requires two explicitly enabled real local apps with independently labelled same/distinct screens,
including empty/filled/loading and dynamic URL/list states. Record false merges/splits and evidence;
do not silently tune the threshold. Missing apps remain `OBSERVER_MISSING` for calibration.

### Permissions, secrets and process boundary

Enforce `delete|remove|pay|purchase|transfer|send` case-insensitively on the resolved accessible name
before action dispatch in explore and heal. `--allow-act` is operator-supplied, repeatable, bounded
and compiled before effects; a matching expression allows only that named action. Model/page text
cannot add or broaden it. Apply the same rule to cached-locator fallbacks and recovery proposals.
This deny list is a named-control policy, not proof that every dangerous app effect is discoverable
from a label; document that limitation and use disposable local apps. Unsupported phase-2 acts
remain unsupported even with an allow match. Approved ordinary replay keeps its existing semantics.

The driver receives only declared secret names/values needed for exploration; gateway and recording
credentials stay in Rust or their explicitly supervised recording children. Redact known values
and filled-input values before driver replies, then parent-rescan the decoded assembled prompt and
validated replies (including escaped representations). Fail closed on unknown secret-bearing data.
Observe main-frame, nested same/cross-origin frame echoes, accessible names, URL and error channels.
Do not equate text privacy with image masking or grepping compressed PNG bytes with visual privacy.

Extend the proven phase-2 contained-worker design for explore/heal rather than placing synchronous
`build_model`/`draft`/broker calls in a deadline loop outside it. The model-capable Rust worker may
receive the minimum gateway material; its Node child must get a separate allowlisted environment.
Supervise actual model/lease I/O and native-runtime descendants, not a detached thread that merely
returns timeout while calls continue. Retain partial effect/call uncertainty, explicit group cleanup
and platform containment limits. Reuse the ordinary worker handshake; name a new worker mode only
if a real production caller needs it.

### Publication and capture semantics

Create `.graphhelm/journeys/<id>.journey.yaml` only after schema/semantic/canonical validation of
the same snapshot to be published. New exploration never overwrites an existing flow/cache; refuse
the id conflict before actions. Do not introduce a force flag or silently replace approved work.
Use the canonical owner in `journey_flow`, validated directory/target custody, per-flow writer lock
and checked atomic replacement. The phase-2 `atomic_write` helper alone is not a multi-file
transaction or sufficient parent-directory custody proof.

Stage flow and cache with one digest. Publish individually with explicit partial outcome; readers
must refuse a mismatched digest. Reconcile committed artifacts after failure, without rollback of
someone else's edits or deletion of event history. A failed run may publish only a valid observed
acyclic prefix explicitly marked partial in output, still `draft`; otherwise preserve old bytes.
Never report the full goal reached for that prefix. Keep unobserved intentions out of the flow.

Without recording bundle, no persistent screenshots. With a complete bundle, stage a masked PNG
at each actually visited screen under owned temporary custody. After the observed draft path is
validated and its contract preview compiled with explicit `--include-draft`, seal those images
through the existing capture path and record pinned walked IDs. Do not rerun actions to recreate
past captures, and do not approve to satisfy replay's
approved-only loader. Retain temporary masked inputs only within owned custody, seal before exposing
evidence, and record arrows only for passed consecutive destination observations. Draft/uncommitted
artifacts or `scope: unknown` can yield unknown freshness; say so. Do not claim fresh proof merely
because the event append succeeded. No new signal kind or generic acceptance evaluator is added.

### Drift and bounded healing

First add deterministic mapping of actual phase-2 refusals to design §6's drift codes, with exact
edge/act pointer and redacted `seen`. URL mismatch, low control similarity and expectation failure
are separate facts; use the recorded baseline controls, not today's fingerprint compared to itself.
Without heal, stop that path and never call a model. Persisting drift requires setting `draft` and
`approved: null`, because existing validation refuses approved flows carrying drift. Record the
previous approval as prior Git/command provenance, not a new field in the closed flow schema.

With explicit heal, retain the current isolated browser at the failed edge; do not restart the
path or repeat previously successful acts. Send only that edge's intent/old acts, drift code and
redacted current snapshot. At most eight acts and the same permission/secret/origin policies apply.
Success requires the original destination URL and every original visible expectation; preserve
original screen ids, expectation facts, other edges and paths. A model cannot rename the destination
or weaken it to make recovery pass. Replace only the failing edge's acts, update bound cache, retain
`drift` with `healed: true`, set draft/null approval and regenerate the draft projection explicitly.

If failure happened midway through a multi-act edge, retain its completed prefix and append only
the observed replacement suffix; store a full replayable edge from its original source within the
eight-act bound. Never serialize a suffix-only recipe that cannot run from the source. Failure to
represent the full edge stops with old acts intact. Resume only the remaining deterministic path
in this invocation under its immutable repair snapshot; ordinary future replay still refuses draft.
Clear neither old drift entries nor prior failures; only explicit `journey approve` clears drift.
Failed healing retains the broken edge and reports later work unobserved. Recheck the original
source/digest under writer lock before publication; concurrent edits are a conflict, never merged
or implicitly approved. Flow/cache/contracts/event appends remain distinct partial effects.

## File Structure

Implementation candidates below are exact paths, not permission to change all of them. Each code
issue/card narrows its actual surface. **This docs PR changes only this plan.**

- Create `apps/cli/src/commands/journey_explore.rs` for the real exploration caller, closed proposal
  validation, prompt policy and loop. Modify `apps/cli/src/args.rs`,
  `apps/cli/src/commands/journey.rs` for the subcommand caller, and
  `apps/cli/src/commands/mod.rs` for module registration.
- Modify `apps/cli/src/commands/journey_flow.rs` for the minimal shared canonical/draft publication
  seam, and `journey_replay.rs` for actual shared driver/supervisor and edge-local heal callers.
  Preserve ordinary replay; do not extract an empty generic framework. Reuse `architect.rs` model
  building as-is where possible; widen/change it only with an identified caller and neighbor proof.
- Extend `tools/journey-driver/driver.mjs` only for missing observation/redaction capabilities
  required by real explore/heal requests; the existing wire operation set should suffice. Any
  incompatible protocol change needs explicit version/producer/consumer review.
- Create `apps/cli/tests/journey_explore_cli.rs` (offline model/process/filesystem I/O) and
  `apps/cli/tests/journey_explore_browser.rs` (explicit opt-in real browser). Reuse phase-2
  `tools/journey-driver/fixture-server.mjs`, `driver.browser.test.mjs`, `journey_replay_cli.rs`,
  `journey_replay_browser.rs` and existing flow/producer fixtures before adding duplicate observers.
- Add only independent canonical fixtures actually needed under
  `apps/cli/tests/fixtures/journey_explore/checkout.journey.yaml` and
  `apps/cli/tests/fixtures/journey_explore/checkout-model.json`; recorded replies must match manually
  reviewed full prompts, not be regenerated from arbitrary production output to make tests pass.
- Update `docs/specs/2026-10-06-journey-explore-design.md`,
  `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md` and `tools/journey-driver/README.md` with settled flags,
  limitations and proof status. No Studio feature, schema pack or automatic scope mapper in this slice.

## Task 1: Parser, gateway door and closed proposal boundary

**Files:** explore command, args/dispatch and offline explore target. Reuse architect model builder.
**Interfaces:** actual `JourneyCommand::Explore` caller, `ModelSource`, closed proposal parser.

- [ ] **Step 1 — RED.** Extend real CLI coverage for route/fixture conflicts, disabled/missing
  route, invalid id/local base, missing declared secret, incomplete record bundle and bad regex.
  Model/process tripwires establish no call/spawn/write on preflight refusal. Missing-command RED
  proves only absent subject; once wired, negative cases must reach the named boundary.
- [ ] **Step 2 — RED model boundary.** Recorded model I/O proposes unknown fields, extra variant,
  invalid act/secret, oversize/incomplete reply and malicious page instruction. Driver-I/O counter
  remains zero for refusals. A live positive control proves that counter before reset. Two invalid
  replies stop after one retry; count every turn and preserve the first failure.
- [ ] **Step 3 — Implement.** Reuse `build_model`/`DraftModel`, map existing gateway errors, and
  accept only deterministically validated proposals. Preserve direct/native credential choice and
  quota failure; no provider fallback. Add no fixture-only production success path.
- [ ] **Step 4 — GREEN/commit.** Offline CLI tests and existing architect route-door tests. Use
  local recorded/transport I/O; no provider request or key is needed to prove command wiring.

## Task 2: Real-browser privacy and action permissions

**Files:** explore loop, existing driver/browser fixtures and opt-in explore target.
**Depends on:** repaired, approved and merged PR #348 privacy boundary.

- [ ] **Step 1 — RED secret transcript.** Actual browser fill/echo of a synthetic canary produces
  a recorded model-I/O transcript. Assert the intended field really received the canary, then
  independently scan decoded prompts/replies, output, YAML/cache and env probe for raw/escaped
  values. Include frame echoes and secret-bearing accessible names/URL; no copied fixture token
  can stand in for the real browser observation. Stop before dispatch if redaction is uncertain.
- [ ] **Step 2 — RED deny-list effects.** A local app increments a durable server counter for
  `Pay now`/`Delete`/`Send`, including alternate exact-locator tiers. Without allow-act there is
  no request/effect. An operator regex allows exactly the intended action, while a different name,
  page instruction and broad model proposal remain refused. Invalid regex stops before model I/O.
- [ ] **Step 3 — Implement.** Deterministic permission check before driver calls, bounded declared
  secrets, frame-aware text redaction and parent prompt/reply rescans. Reuse repaired masking;
  independently decoded frame PNG evidence is required for recording, not compressed-byte grep.
- [ ] **Step 4 — GREEN/commit.** Existing driver privacy/origin tests remain green plus the smallest
  actual browser/transcript/action-canary cases closing explore-specific gaps. Record known gaps
  in semantic danger detection; no claim that a deny-list finds every destructive behavior.

## Task 3: Observed screen identity and stable graph construction

**Files:** explore command, draft seam in flow owner, independent flow fixture and browser target.
**Interfaces:** deterministic URL/id/set operations; existing driver snapshots and exact locator facts.

- [ ] **Step 1 — RED independent identity cases.** Fixed labelled control sets pin the exact 0.8
  boundary, empty set, id suffix/composed-length collisions and URL normalization. Existing driver
  tests normalize/fingerprint but do not choose persistent identity. Browser variants verify same
  URL empty/filled screens, numeric/list changes and distinct controls with independent expectations.
- [ ] **Step 2 — Implement.** Reuse fingerprints/controls, deterministic set comparison and
  tie-breaks; use model naming only for newly observed identities. Keep original exact accessible
  names for visible expectations and action locators. No secret placeholder becomes an expectation.
- [ ] **Step 3 — RED representation failures.** Eight same-screen acts, ninth refusal, revisit,
  unreachable screen, zero-edge goal and missing unique visible expectation cannot silently produce
  fake valid paths. Prior valid bytes survive. Keep source-scope unknown; no inferred filename.
- [ ] **Step 4 — GREEN/commit.** Compare canonical draft against an independently reviewed oracle;
  two runs with identical concrete fixture input produce byte-identical YAML/cache, excluding record
  IDs/timestamps/images. Schema and existing compiler validate every representable flow.

## Task 4: Supervised model loop, budgets and cleanup

**Files:** explore/replay private transport/supervisor seams and offline process-I/O tests.
**Interfaces:** production-contained worker with a separately allowlisted browser child.

- [ ] **Step 1 — RED actual blocking boundaries.** Reuse phase-2 stuck-pipe tests; add only new
  model call/lease/native-descendant blocking cases and malicious repeated naming/invalid-reply
  loops. Real I/O helper processes are faults at the actual boundary, not mock timeout arithmetic.
- [ ] **Step 2 — Implement.** Supervise startup, broker/route/model operations, browser requests,
  artifact/record writes and group close with one declared run budget. Every proposal consumes a
  turn. Count observed usage and failures without logging sensitive prompts; cap reply/input bytes.
- [ ] **Step 3 — Prove cleanup.** Finite elapsed return with generous platform scheduling tolerance,
  owned descendant termination and successful later invocation. Kill only owned contained processes.
  Name any residual escaped-descendant/storage/external-provider uncertainty. Returning from a
  timed receive while an uncontained model thread lives is not a hard timeout proof.
- [ ] **Step 4 — GREEN/commit.** Focused offline faults plus installed-driver startup/close. If any
  longest call lacks a proven bound, label the budget advisory and retain the unresolved obligation.

## Task 5: Draft artifacts, cache and optional sealed observations

**Files:** flow/explore publication, existing recording seams, explore CLI/browser targets.
**Interfaces:** shared canonical/digest owner, draft validator, phase-2 cache/capture/walked custody.

- [ ] **Step 1 — RED custody.** Existing id refuses before effects; unsafe directory/link target,
  writer conflict, source edit, staged write failure and digest mismatch preserve earlier valid
  artifacts. Independent readback cannot accept a half-published flow/cache as complete work.
- [ ] **Step 2 — Implement.** Publish only observed valid draft/prefix with explicit outcome;
  never approve, clear drift or overwrite existing flows. Validate canonical bytes and generated
  draft projection through the original compiler; no second renderer/compiler in explore.
- [ ] **Step 3 — RED actual records.** With the full bundle, fresh reader processes decrypt masked
  sealed PNGs and read pinned walked pairs for passed steps. Refused capture gets no arrow; failed
  record keeps earlier events and nonzero outcome. Without bundle, no persistent PNG or fake record.
- [ ] **Step 4 — GREEN/commit.** Flow/compiler/producer tests plus opt-in draft-to-reader proof.
  Report unknown freshness from unknown scope/dirty draft honestly. Then explicit owner approval
  and two ordinary replays establish reuse, separately from draft-generation success.

## Task 6: Deterministic drift and edge-local heal

**Files:** replay/explore caller seam, replay args, flow publication and existing replay observers.
**Depends on:** Tasks 1–5, a valid approved flow and repaired phase-2 driver.

- [ ] **Step 1 — RED drift.** Real fixture renames `Checkout` to `Go to payment`: ordinary replay
  stops at `cart.checkout/0`, persists `drift.locator_missing` with draft/null approval and zero
  model calls. Wrong URL, changed controls and failed exact expectation get their own codes. Existing
  replay failures do not yet prove persisted drift; extend that observer rather than duplicating it.
- [ ] **Step 2 — RED recovery.** Recorded model receives only failed-edge intent/current redacted
  snapshot. It reaches the unchanged destination with a replacement act; other edges/paths/screens/
  expectations stay identical. A request to weaken destination, act on another edge or exceed eight
  acts fails. Already completed actions are not repeated; a side-effect counter observes this.
- [ ] **Step 3 — Implement.** Keep the browser at the broken edge. Validate/permit proposals with
  the same explore policy, retain completed prefix plus observed suffix as the full replacement
  edge, and prove original URL/expectations before accepting repair. Rebind cache, retain healed
  drift and draft state, then resume remaining deterministic work without widening the model scope.
- [ ] **Step 4 — RED failure/publication.** Budget, model refusal, ambiguous destination, missing
  fact, mid-edge overlong recipe and concurrent edit retain old acts/broken drift. Record new head
  provenance and partial effects accurately; no later captures for unobserved destinations. Crash
  between individual writes is reconciled as partial, not described as atomic delivery.
- [ ] **Step 5 — GREEN/commit.** Real renamed-button and multi-act partial-prefix cases, unchanged
  ordinary-replay no-model tripwire, and explicit approve/replay afterward. `approve` alone clears
  drift; healed draft cannot acquire trusted status by replaying itself.

## Task 7: Two-app calibration, docs and delivery

**Files:** opt-in explore/replay observers and English design/JPD/driver documentation.

- [ ] **Step 1 — Final recorded-model browser journey.** Disposable committed checkout app,
  installed driver, fixed synthetic secrets, exact prompts/replies and independently reviewed flow
  oracle. Explore twice, inspect real visible states, artifacts/model transcript and sealed records;
  approve explicitly, then replay twice with live model tripwire at zero calls.
- [ ] **Step 2 — Calibrate.** Two explicitly enabled real local apps with labelled identity cases,
  not two routes of the same static fixture. Report threshold false merges/splits and limits. No
  production account, external purchase/send/delete or autonomous tool installation. Missing apps
  are retained as an unobserved calibration obligation, not replaced with invented labels.
- [ ] **Step 3 — Document.** Actual command syntax, route/credential isolation, allow-act scope,
  secret naming, draft/approval distinctions, partial publication, drift/heal limits, whole-run
  bound and missing observers. Record compatible observer versions. No screenshot/model result
  is upgraded to a generic JPD certification or arbitrary business-goal proof.
- [ ] **Step 4 — Final proof.** Commit before checks, list commands/results and RED/retry lineage.
  One independent reviewer runs reached checks, resolves any BLOCK, checks Keel/closing intent and
  merges only the reviewed head. Refs #333 stays open until its whole intended scope is delivered.

## Test-audit Ledger for Planned Additions

Every implementation PR answers contract/regression/gap/seams and actual cost before adding or
changing tests. Reuse existing observers; no quotas or test deletion are planned. RED for missing
subject is separate from RED demonstrating a concrete unsafe behavior after wiring.

| Contract | Credible defect and nearest coverage gap | Smallest adequate observer / cost / seams |
|---|---|---|
| Closed model/action boundary | Reply with extra fields or page instruction reaches browser; architect fixtures do not enforce explore proposals. | Offline real CLI + recorded model/process-I/O tripwire, seconds after build; use real entry point, no public test-only executor. |
| Model input privacy | Canary is masked only after dispatch, frame echo survives or secret is encoded in history; phase-2 privacy lacks model prompts. | Actual browser fill plus model-I/O transcript/env observation, independently decoded image when recording; seconds/minutes, explicit Chromium. |
| Action permission | Cached/fallback/heal path bypasses denied name or model supplies allow policy; approved replay has no exploration permission check. | Real browser and durable local effect counter with positive control; seconds, no external side effect. |
| Persistent identity | Threshold boundary, unordered candidates or slug collision changes ids/merges states; driver normalization does not choose screen identities. | Independent fixed control-set/id cases plus real browser states; milliseconds/offline and seconds/browser, no new production-only-for-tests seam. |
| Honest draft | Model `done` approves, invents expectation or writes invalid partial flow; existing compiler assumes authored sources. | Recorded-model/browser-to-canonical-flow oracle, actual schema/compiler readback, conflict fixtures; seconds. |
| True bounded calls | Synchronous model/lease outlives a clock or native child leaks; phase-2 faults contain no model door. | Owned real blocking process/transport I/O and subsequent invocation; seconds, offline. |
| Isolated healing | Earlier act repeats, neighboring edge changes, suffix-only recipe or destination weakens; phase-2 replay has no recovery. | Actual renamed-control/multi-act browser and side-effect counter, semantic diff + approve/replay; seconds/minutes. |
| Zero model ordinary replay | Shared explore code dispatches eagerly; old no-model proof must still hold after integration. | Reuse existing successful credential-free replay and live tripwire with positive control, no new unused route flag. |
| Threshold calibration | Static fixture hides false merges/splits; current threshold has no two-app evidence. | Labelled real local app states, explicit observer receipts; time depends on enabled apps, missing remains unobserved. |

## Proving Commands and Environment

These are **future implementation commands**, not evidence from this docs PR. Narrow them to the
final changed paths and actual test names. No browser/network/credential requirement enters the
default offline suite. Use owned worktree/target/temp directories; never another lane's checkout or
target. Follow coordinator Cargo lock protocol (atomic lock-file creation, remove only your lock
after the command) and jobs limit; no GitHub Actions.

```powershell
$env:TEMP = 'D:/codex/scratch/<lane>-phase3-temp'
$env:TMP = $env:TEMP
$env:CARGO_TARGET_DIR = 'F:/<lane>-phase3-target'
$env:CARGO_BUILD_JOBS = '2'
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_explore_cli
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_flow_cli --test journey_validate_cli --test journey_replay_cli --test journey_producers_cli --test journeys_surfaces
# Also run this existing model-door target when its boundary is changed:
cargo +1.97.1 test --locked -p graphhelm-cli --test architect_cli
cargo +1.97.1 test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --locked -p graphhelm-cli --all-targets --all-features -- -D warnings
git diff --check
```

Create the owned directories before use; acquire the shared Cargo lock before each Cargo command.
No target name here is a delivered interface. Retain existing phase-2 private transport/worker
tests when those bodies change, and existing architect route/credential fixtures when reused seams
change. Run touched non-CLI crate tests/lints only if their actual sources change.

```powershell
# Explicit observer-enabled validation only; setup/install is a separate authorized operation.
$env:GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT = '<owned-validation-project>'
node --test tools/journey-driver/driver.test.mjs tools/journey-driver/driver.browser.test.mjs
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_explore_browser -- --ignored --nocapture
cargo +1.97.1 test --locked -p graphhelm-cli --test journey_replay_browser -- --ignored --nocapture
```

Ignored browser groups must fail explicitly on missing prerequisites when selected, never early
return `ok`. Record passes, failures, ignored/skipped and unobserved separately, including host
platform and two-app calibration. Future RED-first tests name credible regressions and fail on the
parent for that reason; absence/compilation failure alone is not security proof. Keep all attempts.
Reviewer runs `graphhelm --json keel check --diff <merge-base>..<head> --card <card.json>` with exact
paths and named real exports. No optional proof polish after the requested obligations are proven.

## Review Focus and Completion

1. Repaired merged phase-2 seam assumptions, frame privacy and ordinary replay regressions; no
   treating a private PR function or missing protocol capability as an already public interface.
2. Existing gateway lease/route choice, isolation of browser credentials, quota refusal and no paid
   fallback; strict proposal validation independent of untrusted model/page instructions.
3. Real observed identity/expectations, stable IDs, bounded graph representation, canonical draft
   and independent cache digest; model `done` never certifies arbitrary goals or owner approval.
4. Actual transcript and image/frame privacy, observed destructive-action refusal and allow scope;
   no screenshot transport to the model and no generic semantic-safety claim from a name regex.
5. Supervision includes the longest model/lease/storage call and owned descendants; partial effects,
   publication conflicts, missing observers and timing limitations are explicit.
6. Heal retains original destination obligations, only changes the failed edge, preserves completed
   prefix and old drift, returns to draft and requires explicit approval for later ordinary replay.
7. Real installed-driver two-run exploration/approve/replay and two-app calibration receipts are
   distinguished from offline fixture evidence, platform gaps and generic JPD certification.

This plan is delivered when its documentation PR is independently reviewed and merged. Phase-3
implementation is delivered only when the later code PR(s) merge and the named user-visible
obligations are actually observed. Phase 2 remains a dependency until PR #348's BLOCK is repaired
and approved. Missing browser, privacy, timing or calibration observers remain unresolved.
