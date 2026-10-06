# `graphhelm journey explore`: agent-built journey flows with deterministic replay

Status: design, owner-approved feature, issue #333. Date 2026-10-06.
Plan for phase 1: [`docs/superpowers/plans/2026-10-06-journey-explore-phase-1.md`](../superpowers/plans/2026-10-06-journey-explore-phase-1.md).

## 1. Promise

An agent drives the running app in a browser, black-box and language-independent, and **builds the
user-journey flow**: screens are nodes, user actions are edges, each screen carries the observable
state the user should see, and every visited screen is captured as a sealed screenshot. The output
is `.graphhelm/journeys/<id>.journey.yaml`. After the first exploration, the recorded actions replay
with **no model call** — a fast, cheap automated test of the journey. When the app drifts, the agent
takes over **only at the broken edge**, and the flow marks what changed. A flow is `draft` until
the owner approves it in the Studio Journey tab (or with `graphhelm journey approve`).

This is not page QA: the agent is not hunting bugs. It maps what a user can do and pins it.

Inspiration: the open-source `e2e` runner from tester.army (`tools/e2e-observer` already wraps it):
agentic `explore` toward a goal; a passed agent action is cached and replayed with no model call,
matching controls by role, name, test id and context with exact names; on drift the agent takes
over from the current screen; credentials are filled by name and redacted from model input. We keep
those four ideas and change the product: the artifact is a GraphHelm journey flow, not a test file.

## 2. Primary design rule: the reader is an AI

Keel optimises read tokens first (`docs/keel/KEEL_SPEC.md` §1). The flow file is read by agents far
more often than by people, so every format choice below is judged by: *can an agent load it cheaply,
trust it without opening anything else, and diff it line by line?*

| Rule | How the format meets it |
|---|---|
| Schema-first | `schemas/journey-flow.schema.json`, JSON Schema 2020-12, `$id` `https://p50.dev/schemas/journey-flow.schema.json`, registered in `schemas/catalog.json` (document version `1.0.0`). Every object `additionalProperties: false`. The file's first key is `schema: graphhelm.journey-flow/1`; a `/2` is a new schema, never an edit. |
| Deterministic ordering, stable ids | Canonical form: keys in schema order, `screens` and `edges` sorted by `id`, `paths` sorted by name with `main` first, flow-style one-line objects for `act`/`expect`. `graphhelm journey validate` refuses a non-canonical file (`flow.not_canonical`) and `journey compile --fmt` rewrites it. Ids follow the existing journey id rule `^[a-z0-9][a-z0-9._:/-]*$`; an edge id is `<from>.<verb>` by convention, never positional. |
| Compact | No prose field that duplicates structure: no `description`, no `title` per edge, no `from`/`to` restated in paths. One optional `title` per flow and per screen (the Studio label). Fingerprints, locator details and timings live in the replay cache, not in the flow. |
| Every reference by id | Edges name screens by id; paths name edges by id; `act.secret` names an entry in `secrets`; `actor` names an entry in `actors`. The validator checks each (`flow.unknown_*`). |
| Explicit absence | `scope: unknown` (string) vs a non-empty list; an empty list is a schema error. `approved: null` vs an approval object. `drift: []` is "replay found none", never omitted. No field is optional *and* meaningful when absent, except `title`. |
| Diffable, line per fact | Each screen, edge and drift entry is a short block whose lines are independent facts; one changed button name changes one line. |
| Cheap to load | Budget: a flow of 12 screens and 16 edges is ≤ 1,500 tokens (≈ 6 KB). Hard cap 32 KiB per file (`flow.too_large`), 64 screens, 128 edges, 16 paths. An agent reading one journey reads one file; the compiled contract JSON (≈ 4× larger) is never the thing an agent should load. |
| Machine-checkable | `graphhelm --json journey validate` returns `{findings, diagnostics:[{code, pointer, message}]}` with stable codes (§7), exit 0 clean, 2 findings, 3 unusable input — the same envelope `journey validate` already prints for contracts. |

### 2.1 How an agent reads it

1. `graphhelm --json journey validate --all` — one call, says which flows and contracts are sound.
2. Read the flow file. Field order is the reading order: `status` and `approved` (may I trust it?),
   `drift` (what is broken now?), `paths.main` (the journey in one line), then only the `edges` and
   `screens` the task touches.
3. `scope` on a screen tells a coding agent which files the screen depends on, so a Keel card can
   name the journey (`journeys: [<contractId>]`) without opening the app.
4. `graphhelm --json journeys --execution <id>` for proof (captures, arrows, freshness) — unchanged.

## 3. The format

```yaml
schema: graphhelm.journey-flow/1
id: checkout
title: Shopper pays for the cart
status: draft                 # draft | approved
approved: null                # or {revision: <40-hex>, digest: sha256:<64-hex>}
base: http://localhost:3000
actors: [shopper]
secrets: [shopper_password]   # names only; values come from GRAPHHELM_SECRET_<NAME>
risks: [money]                # contract riskSignals enum
screens:
  - id: cart
    url: /cart
    state: stable
    expect: [{role: heading, name: Cart}, {role: button, name: Checkout}]
    scope: [app/cart/page.tsx]
  - id: done
    url: /orders/:id
    state: success
    expect: [{role: heading, name: Order placed}]
    scope: unknown
  - id: pay
    url: /checkout
    state: stable
    expect: [{role: textbox, name: Password}]
    scope: [app/checkout/page.tsx, app/api/pay/route.ts]
edges:
  - id: cart.checkout
    from: cart
    to: pay
    acts:
      - {kind: activate, role: button, name: Checkout}
  - id: pay.submit
    from: pay
    to: done
    acts:
      - {kind: enter_text, role: textbox, name: Password, secret: shopper_password}
      - {kind: submit, role: button, name: Pay now}
paths:
  main: [cart.checkout, pay.submit]
drift: []
```

Field notes (normative):

- `url` is a **pattern**: path only, numeric/uuid/hex segments replaced by `:id`, query dropped
  unless the explorer saw it change the screen (then kept as `?tab=:v`). Absolute URLs are refused;
  the host is `base`.
- `acts[*].kind` reuses the contract's `semanticAction.kind` enum. The locator is `role` + exact
  `name` (accessible name); `text` is allowed for `enter_text` only when no `secret` is named, and is
  refused if it equals any secret's runtime value (`flow.secret_literal`, checked at record time).
- `state` uses the contract's expected-state enum; `expect` is 1–8 `{role, name}` pairs, the
  observable assertion of the screen (exact accessible name match).
- `drift` entries: `{edge, act, code, seen, at, healed?}` where `act` is the index, `code` is a drift code (§6),
  `seen` is a short machine string (`button "Pay"`), and `at` is the revision. Drift entries are
  cleared only by `approve`.
- `approved.digest` is sha256 of the canonical file with `status`, `approved` and `drift` removed.
  Editing an approved flow without re-approving makes the validator report `flow.approval_stale`.

## 4. Relation to the frozen journey-contract

`journey-contract.schema.json` is frozen and pinned (package digest), and every existing surface
keys on it: `journey validate`, `journey capture`/`walked`, `journeys` (freshness), the Studio
Journey tab, and the Keel card `journeys` field. The contract is also **linear**: steps in order,
arrows between consecutive steps.

**Decision (proposed D-057): the YAML is a new artifact, `journey-flow`, that compiles
deterministically to contracts. The flow is the source; the contract JSON is a generated
projection.**

- Each path compiles to one contract: `main` → contractId `<id>`; path `p` → `<id>.<p>`. Written to
  `.graphhelm/journeys/<contractId>.json`, so every existing reader works unchanged.
- Step per visited screen along the path (first edge's `from`, then each `to`); `stepId` =
  screen id; the first step's action is `navigate` to `url`, later steps take the entering edge's
  last act (all acts are kept in the replay cache). `expectedStates` = `[state]`; `screen` =
  `{screenId, title, scopePaths}` (omitted when `scope: unknown`, which the contract already allows).
  A path that revisits a screen is refused in v1 (`flow.path_revisits_screen`), because contract
  step ids are unique.
- One promise per step: `promiseId` `<screen>.visible`, statement generated from `expect`
  (`shows heading "Cart", button "Checkout"`), `requiredFact: content_rendered`,
  `requiredEvidenceKinds: [visual_capture]`, `requiredObserverCapability: browser`,
  `statesToObserve: [state]`, `maxEvidenceAgeSeconds: 604800`. `failureContract` from a fixed
  defaults table (timeout 30 s, `visibleError: "screen <id> not reached"`, `safeStop: "stop replay"`,
  `recoveryAction: null`, `prohibitedSideEffects: []`). Defaults are constants in the compiler, so
  identical flows compile to byte-identical JSON.
- Only `approved` flows compile by default; `--include-draft` compiles drafts for preview.
- `journey compile --check` exits 2 with `flow.contract_stale` when a generated contract differs
  from the file on disk (a hand edit or an uncompiled flow); `journey validate --all` runs it too.
- Rejected alternatives: editing the contract schema (frozen; would re-pin the package and break
  receipts); making the YAML a second contract encoding (keeps the linear shape and all the prose
  fields an agent pays for); storing the graph in the contract via `x-` fields (schema is closed).

## 5. Architecture

```
graphhelm journey explore|replay  (Rust CLI: owns model calls, records, files)
        │ JSON lines over stdio (closed protocol, versioned)
        ▼
tools/journey-driver/driver.mjs   (node + Playwright: owns the browser, nothing else)
```

**Driver choice: Playwright through node, as an external observer process**, like
`tools/playwright-observer`. Reasons: (1) Playwright's `locator.ariaSnapshot()` and
`getByRole(role, {name, exact: true})` are exactly the screen fingerprint input and the replay
locator we need; CDP from Rust would re-implement both. (2) Projects that ran `journey-map` already
installed `@playwright/test` and chromium (`setup --install-observer playwright`). (3) No new Rust
dependency. (4) The decision register requires that model credentials not live in the sandbox that
runs untrusted code: the driver process executes app JavaScript and **never** holds a model
credential or a secret it was not told to type; the CLI holds the gateway lease.

Driver protocol (`graphhelm-journey-driver/1`), one JSON object per line. Requests: `open {base,
viewport, allowOrigins}`, `snapshot {}`, `act {kind, role, name, text?, secretEnv?}`, `capture
{path, maskSecrets}`, `close {}`. Responses: `{ok, url, ariaYaml, fingerprint}` or `{ok:false,
code}` with codes `driver.locator_missing`, `driver.locator_ambiguous`, `driver.host_refused`,
`driver.timeout`. The CLI shipped copy is written to `<project>/.graphhelm/observers/` like the
existing observers.

Commands:

| Command | Does | Model |
|---|---|---|
| `journey explore --base <url> --goal <text> --id <id> --route <route> [--max-steps 40]` | Agent loop: snapshot → model proposes one act → driver acts → new screen or same → repeat until goal reached or budget spent. Writes the draft flow, the replay cache, and records captures if `--execution` is given. | yes |
| `journey replay <id> [--execution ...] [--heal --route <route>]` | Replays every path from the cache, asserts `expect`, records `jpd.screen_captured` per screen and `jpd.transition_walked` per edge. On drift: without `--heal` stop the path, write `drift`, exit 1; with `--heal` hand the edge to the agent (§6). | only with `--heal` |
| `journey validate [--all]` | Contracts (unchanged) **and** `*.journey.yaml`. | no |
| `journey compile [--check] [--fmt] [--include-draft]` | Flow → contracts; canonical rewrite. | no |
| `journey approve <id>` | Sets `status: approved`, `approved {revision, digest}`, clears `drift`, compiles. Studio's Approve button calls the same Runtime/CLI path (every Studio action must exist on the public API/CLI). | no |

### 5.1 Screen identity and dedupe

A screen is `(url pattern, fingerprint)`. The fingerprint is sha256 over a normalised accessibility
skeleton from `ariaSnapshot`: landmarks, headings, and interactive controls as `role "name"`, text
content dropped, numbers and dates masked, repeated list items collapsed to `role ×bucket`
(1, 2–5, 6+). Same pattern **and** Jaccard similarity of the control sets ≥ 0.8 → same screen.
Same pattern, lower similarity → a distinct screen `<base-id>.<n>` (e.g. empty vs filled cart is
`cart` and `cart.2`; the model names it on first visit). Different pattern → different screen.
Screen ids are proposed by the model from the page's main heading and slugged; collisions get a
numeric suffix; once written, an id never changes (stable for diffs and captures).

### 5.2 Replay cache

`.graphhelm/journey-cache/<id>.json` (outside `journeys/`, because the contract reader takes every
`*.json` there). Closed schema `schemas/journey-replay-cache.schema.json`; never loaded into a model
context. Per edge and act: `{role, name, exact: true, testId|null, context: <nearest landmark
"role name">|null, nth|null}`; per screen: `{fingerprint, controls: [...]}`; plus `flowDigest`
(the cache is void when the flow's canonical digest changes) and `viewport`. Matching order on
replay: test id, then role + exact name inside `context`, then role + exact name globally; a
locator that resolves to 0 or >1 elements is drift, never a guess ("Save" never matches "Save as
draft").

## 6. Drift and hand-off

Replay checks after every act and after every edge:

| Code | When |
|---|---|
| `drift.locator_missing` | cached locator resolves to 0 elements |
| `drift.locator_ambiguous` | resolves to >1 |
| `drift.wrong_screen` | after the edge, URL pattern ≠ `to.url` |
| `drift.screen_changed` | pattern matches, fingerprint similarity < 0.8 to the cached screen |
| `drift.expect_failed` | an `expect` pair is not present |

Without `--heal`: the path stops, a `drift` entry is written, the edge's later screens are not
captured (`journeys` shows them stale/unknown exactly like a missing Playwright step today), exit 1.

With `--heal`: the agent gets the edge's intent — `from`, `to` (its `url`, `expect`), the old acts,
the drift code — and the **current** redacted snapshot, with a budget of 8 acts. If it reaches a
screen matching `to`, the edge's `acts` are replaced, the cache updated, a `drift` entry is kept
with `healed: true`, and `status` returns to `draft` (an approved flow that healed needs the owner
again). If it cannot, the edge stays broken. The agent never touches other edges; replay resumes
deterministically after the healed edge.

## 7. Diagnostic codes (stable; new codes may be added, none renamed)

CLI error code `GHCLI034_JOURNEY_FLOW_INVALID` (exit 2 envelope). Findings:
`flow.not_yaml`, `flow.too_large`, `flow.schema_invalid`, `flow.id_mismatch` (file name ≠ `id`),
`flow.not_canonical`, `flow.duplicate_id`, `flow.unknown_screen`, `flow.unknown_edge`,
`flow.unknown_secret`, `flow.unknown_actor`, `flow.path_disconnected` (edge `n.to` ≠ edge
`n+1.from`), `flow.path_revisits_screen`, `flow.base_not_local`, `flow.scope_path_missing`,
`flow.scope_path_outside_project`, `flow.approval_stale`, `flow.approved_with_drift`,
`flow.contract_stale`, `flow.unreachable_screen` (a screen in no path; warning severity).
Explore/replay runtime codes: `explore.host_refused`, `explore.budget_spent`,
`explore.goal_unreached`, `explore.model_invalid_reply`, plus the `drift.*` and `driver.*` codes.

## 8. Model route

The model is reached only through the existing gateway: `--route <name>` resolved from the route
manifest and credential broker exactly as `architect` and `gateway probe` do (`ModelRoute`,
`CredentialBroker`, codes `GHCLI009`/`GHCLI010` for route problems). The Studio launches explore on
the same route Jev uses when none is named. Model input per turn: goal, visited screens as
`id url` lines, the current screen's redacted aria snapshot (capped at 6 KB), and the last 3 acts.
Model output: one closed JSON object `{act: {...}} | {newScreen: {id, title}} | {done: true} |
{giveUp: <code>}`, validated before the driver sees it; an invalid reply is
`explore.model_invalid_reply` (one retry, then stop). Screenshots are not sent to the model in v1
(text is cheaper and redactable). Tests use a recorded model, like the architect's
`RecordedDraftModel`, so no test needs a key.

## 9. Records

Reuse, no new kind in v1: `jpd.screen_captured` (via the `journey capture` path, contractId =
compiled contract, stepId = screen id, sealed PNG evidence) and `jpd.transition_walked` (per edge
of a path, consecutive steps of that contract). Drift is a fact about the flow file, kept in git and
in the command's JSON output; a new `jpd.flow_drift` signal is deferred until a reader needs drift
inside a run's event log (phase 4 decides with evidence).

## 10. Studio

Journey tab: list flows with `status`; render the graph (screens as nodes, edges as arrows, not only
the main path) from the flow file through a new read route `GET /v1/journeys/flows` (CLI twin
`graphhelm --json journey flows`); drift edges in the warning colour with the `drift` code; draft
banner with **Approve** (calls `journey approve`) and **Discard draft**. Captures and freshness keep
coming from `journeys` for the compiled contracts, unchanged.

## 11. Security

- **Local hosts only.** `base` and every top-level navigation must be `localhost`, `127.0.0.1`,
  `[::1]`, `*.localhost` or `*.test` (`flow.base_not_local`, `explore.host_refused`). Sub-resource
  requests to other origins are aborted unless named with `--allow-origin` (fonts/CDNs).
- **Secrets by name.** The flow and cache hold names; values come from `GRAPHHELM_SECRET_<NAME>` in
  the driver's environment, typed by the driver. Before any snapshot leaves the driver, every secret
  value is replaced with `«secret:NAME»` and every filled input's value is masked; the CLI re-scans
  for raw values and refuses the turn if one survives. Model replies cannot contain `text` for a
  field that has a secret.
- **Sealed screenshots.** Captures go through the existing sealed-signal path (keyring required);
  secret-filled locators are masked with Playwright's `mask` option.
- **No destructive exploration by default.** Acts whose accessible name matches the deny list
  (`delete|remove|pay|purchase|transfer|send`, case-insensitive) are refused during *explore* unless
  `--allow-act <regex>` names them; the flow above needed `--allow-act "Pay now"`. Replay runs what
  the approved flow says.
- **Bounded**: max steps, max snapshot bytes, per-act timeout, whole-run timeout.

## 12. Phases, each with its proof

| Phase | Delivers | Proof (observer) |
|---|---|---|
| 1 | Flow schema + catalog entry; `journey validate` for flows with all `flow.*` static codes; `journey compile [--check|--fmt|--include-draft]`; `journey approve` (file-only); D-057; docs | CLI tests: golden flow compiles byte-identical; compiled contract passes the unchanged contract validator and `journeys` reads it; one test per finding code; sabotage fixtures |
| 2 | Driver + protocol; replay cache schema; `journey replay` without model; records captures/walked | Fixture static app on `127.0.0.1`; replay twice with a gateway stub that fails on any call; captures recorded; `journeys` shows the arrows fresh |
| 3 | `journey explore` with recorded model; fingerprint + dedupe; redaction; host guard; deny list | Recorded-model explore of the fixture app yields a golden flow byte-identical across two runs; a secret value never appears in recorded model input (grep the transcript) |
| 4 | Drift detection + `--heal` | Fixture app variant renames "Checkout" → "Go to payment": replay exits 1 with `drift.locator_missing` at `cart.checkout/0`; `--heal` with recorded model rewrites only that edge and sets `draft` |
| 5 | Studio Journey tab graph, drift, Approve | vitest for the tab; CLI twin JSON test; Approve flips status through the public path |
| 6 | `scope` mapping: URL pattern → route files using the `journey-map` framework table; else `unknown` | Next.js and Vue fixtures map to the expected files; an unknown framework stays `unknown` |

## 13. Out of scope

Bug hunting / QA findings; non-browser apps (mobile, desktop); production or remote hosts; visual
diffing of screenshots as a drift signal; sending screenshots to the model; parallel multi-actor
journeys; paths that revisit a screen (v2 with step ids `<screen>@<n>`); generating Playwright test
files (the replay cache *is* the test); editing the frozen contract schema.

## 14. Open risks

- Fingerprint threshold 0.8 is a guess; phase 3 must measure it on at least two real apps.
- Accessible names on poorly labelled apps are weak locators; `testId` helps only when present.
- Generated promise statements are mechanical; owners may want to edit them, which the format does
  not allow in v1 (they would be overwritten by compile).
- Heal can mask a real regression by "finding another way"; mitigated by returning to `draft`.
- Node + Playwright is a runtime dependency of the project under test, not of GraphHelm; absent →
  `OBSERVER_MISSING`, never a red journey.
