# Studio redesign phase 5: Journey tab and Before / after — implementation plan

Issue #317. Spec: `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §4.5
(one line), §4.6 (Journeys, Before / after), §4.7 (Journey tab). Branch `issue-317-journey-tab`.

**Goal:** the Studio draws the folded journey map from `GET /v1/executions/{id}/journeys`
(#316) with images read through the image evidence route (#314), lists journeys and before/after
pairs in the right panel, and counts an `after` capture as shipped in the handover.

**Architecture:** one small Runtime addition (the route must carry what the Studio cannot read on
its own), one client method pair, pure derivations in `apps/studio/src/runtime/journeys.ts`,
two components, App wiring. No new polling: the journeys read is re-issued when the event tail
grows by a `jpd.*` signal.

## Rulings

1. **Promise text and capture age come from the route.** The Studio never reads project files and
   `CaptureView` carries no time. `StepView` gains `promises: string[]` (the contract's
   `promises[].statement` for that `stepId`, contract order) and `CaptureView` gains `sequence`
   (the capture signal's event sequence). The Studio maps `sequence` to `occurredAt` from its event
   tail; no event in the tail means "age unknown", never a guessed time.
2. **Before / after pairs are derived in the Studio** from `signal_recorded` events of kind
   `jpd.screen_captured` and their opened envelopes (the envelope text is the
   `graphhelm-screen-capture-v1` document). A pair is the newest `before` and newest `after` with
   the same `(contractId, stepId, pr)`, `pr` required; image id is `evidenceRefs[1]`. Documents
   that fail to parse or carry another protocol are ignored.
3. **Images:** `RuntimeClient.readImage(executionId, evidenceId)` GETs
   `/v1/executions/{id}/evidence/{evidenceId}` with the bearer header and returns a `Blob` only for
   `image/png|jpeg|webp`. A `useImageUrl` hook makes a `blob:` URL, renders it only in `<img>`, and
   revokes it on unmount and on id change.
4. **States:** card `fresh` normal; `stale` greyed + crack + "Code changed after this shot" +
   first changed file (and "+N more"); `unknown` drawn with a "Freshness unknown" badge and its
   cause, never the fresh styling; no capture → "Not captured yet". Arrows: `walked` solid,
   `never_walked` dashed grey labelled "never walked", `stale` dashed red labelled "stale".
   "Proven" in the right panel = step whose capture is `fresh` and not dirty.
5. **Segment colours:** green proven, red stale, grey everything else (never captured or unknown).

## Tasks

### T1 Runtime: promises and capture sequence on the journeys view (Rust)
Paths: `core/execution/src/journeys.rs`, `apps/cli/src/commands/journeys.rs`, their tests
(`core/execution/tests/journeys_freshness.rs`, `apps/cli/tests/journeys_surfaces.rs`).
Promise: the route's JSON carries `steps[].promises` and `steps[].capture.sequence`.
Proof: a test that folds a contract with two promises on one step and asserts both statements and
the capture sequence in the serialized view; `cargo +1.97.1 test -p graphhelm-execution` and the
CLI journeys tests; fmt + clippy on the two crates.

### T2 Studio client
Paths: `apps/studio/src/runtime/client.ts`, `apps/studio/src/runtime/types.ts`, new
`apps/studio/src/runtime/client.journeys.test.ts`.
Promise: `journeys(executionId)` returns the typed view; `readImage` sends `Authorization: Bearer`,
returns a Blob for the three image types and refuses anything else.

### T3 Derivations
Paths: new `apps/studio/src/runtime/journeys.ts` + test; `apps/studio/src/runtime/handover.ts` +
test. Promise: `journeySummary`, `beforeAfterPairs`, `captureAge`; handover shipped gains
"<bot> captured <screen> after (PR #N)" for `phase: "after"` captures.

### T4 Components
Paths: new `apps/studio/src/components/journey-canvas.tsx` + test, `use-image-url.ts` + test,
`right-panel.tsx` + test, `styles.css`.
Promise: rendering tests over a folded fixture for walked, never walked, stale (file named),
unknown, no capture; detail view with full image, before/after, records, promise; blob URLs
revoked on unmount.

### T5 App wiring
Paths: `apps/studio/src/App.tsx`, `App.test.tsx`. Journeys fetched on run select and when a new
`jpd.*` signal arrives; Journey tab renders `JourneyCanvas`; right panel rows open the tab.

### T6 Observer (S6–S7), by the orchestrator
Throwaway project in a temp dir, CLI from this branch, three Studio screenshots captured and
walked, one scope file changed and committed, Journey tab screenshotted.

Proof for every Studio task: `npx tsc -b` and `npx vitest run` in `apps/studio`.
