# User journeys: getting started

For an AI agent first, a human second. Every command below was run against a throwaway git
project; the output shown is real, trimmed with `…`. Paths are relative to the project root.

**Binary.** `journey compile`, `journey approve` and `journey replay` exist on `main` from
`ffc5a288` (#342) and `05e2ace3` (#348). The CLI installed by `graphhelm update` before those
commits has only `capture`, `walked` and `validate`. Check yours:

```sh
graphhelm journey --help    # must list: capture walked validate compile approve replay
```

If `compile` is missing, run `graphhelm update` (it reinstalls from `origin/main`). The output
in this guide comes from a debug build of the #348 branch, which matches `main` at `05e2ace3` for
these commands.

## 0. The model in one table

| Thing | File or record | Written by |
|---|---|---|
| Flow (the source you edit) | `.graphhelm/journeys/<id>.journey.yaml`, schema `schemas/journey-flow.schema.json` | you |
| Contract (generated projection) | `.graphhelm/journeys/<id>.json`, schema `journey-contract.schema.json` (JPD package) | `journey compile` / `journey approve` |
| Screen capture | signal `jpd.screen_captured` in a run | `journey capture` or `journey replay` |
| Walked transition | signal `jpd.transition_walked` in a run | `journey walked` or `journey replay` |
| Journey map (read model) | `graphhelm journeys`, `GET /v1/journeys`, Studio Journey tab | read only |

A capture is proof that a screen was **seen** at a git revision. It is not proof that a promise
holds. With no observer, the honest result is `OBSERVER_MISSING`.

## 1. Set up a project

```sh
graphhelm init --project .
```

Creates `.graphhelm/{events,keyring,serve.key,events.token}`, a `.gitignore` entry and `.mcp.json`
(it never writes to your home directory). The recording commands below read the sealing key from
the environment, never from a flag:

```sh
export GRAPHHELM_EVENTS_KEY="$(cat .graphhelm/serve.key)"      # bash
$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw .graphhelm/serve.key).Trim()   # PowerShell
```

Without it, `keel check` with records refuses with
`GHCLI001_ARGUMENT_INVALID: GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters`.

## 2. Author a flow

`.graphhelm/journeys/checkout.journey.yaml` (the file name must equal `id`):

```yaml
schema: graphhelm.journey-flow/1
id: checkout
title: Shopper pays for the cart
status: draft
approved: null
base: http://localhost:3000
actors: [shopper]
secrets: [shopper_password]
risks: [money]
screens:
  - id: cart
    url: /cart
    state: stable
    expect: [{role: heading, name: Cart}, {role: button, name: Checkout}]
    scope: [app/cart/page.tsx]
  - id: pay
    url: /checkout
    state: stable
    expect: [{role: textbox, name: Password}]
    scope: [app/checkout/page.tsx]
edges:
  - id: cart.checkout
    from: cart
    to: pay
    acts:
      - {kind: activate, role: button, name: Checkout}
paths:
  main: [cart.checkout]
drift: []
```

The file must be canonical: `validate` checks the raw bytes, so **YAML comments are not
allowed** (`flow.not_canonical`); `journey compile --fmt` rewrites a flow canonically.

| Field | Meaning |
|---|---|
| `status`, `approved` | `draft` and `null` while you author; only `journey approve` sets `approved` and fills `{digest, revision}` |
| `secrets` | names only; replay reads each value from `GRAPHHELM_SECRET_<name>` |
| `scope` | repo-relative files or directories that render the screen, or `unknown` |

Rules: ids match `^[a-z0-9][a-z0-9._-]{0,127}$`; acts and expectations name an ARIA role and
accessible name, never a CSS selector; secrets are names, never values. Bounds: 32 KiB file,
64 screens, 128 edges, 16 paths, 8 acts per edge. Full example with a secret and a submit act:
`apps/cli/tests/fixtures/journey_flow/checkout.journey.yaml`.

## 3. Validate, compile, approve

```sh
graphhelm journey validate --all --json
```
```json
{"ok":true,"command":"journey.validate","data":{"checked":1,"files":[{"file":".\\.graphhelm\\journeys\\checkout.journey.yaml","findings":[],"ok":true}],"findings":0,"project":"."},"diagnostics":[]}
```

`validate` takes `<id>.journey.yaml` and `<contractId>.json` files, or `--all`. Exit `0` clean,
`2` findings, `3` input error. A broken flow (wrong file name, missing scope path, edited after
approval):

```json
{"ok":false,"command":"journey.validate","data":{"checked":1,"files":[{"file":"bad.journey.yaml","findings":[
 {"code":"flow.id_mismatch","message":"flow filename must match its id","pointer":"/id"},
 {"code":"flow.scope_path_missing","message":"scope does not exist","pointer":"/screens/1/scope/0"},
 {"code":"flow.approval_stale","message":"approval does not bind this flow projection","pointer":"/approved/digest"}],"ok":false}],"findings":3,…},
 "diagnostics":[{"code":"GHCLI034_JOURNEY_FLOW_INVALID",…}]}
```
exit `2`. Contract files report `schema_invalid`, `invalid_id`, `contract_id_mismatch`,
`duplicate_step`, `unknown_actor`, `unknown_step`, `screen_inconsistent`, `scope_path_missing`.

Compile turns flows into contracts. Drafts are skipped unless asked:

```sh
graphhelm journey compile --check --json        # verify generated contracts match; writes nothing
```
```json
{"ok":true,"command":"journey.compile","data":{"files":[…],"skipped":[{"id":"checkout","reason":"draft"}],"written":0},"diagnostics":[]}
```
```sh
graphhelm journey compile --include-draft --json
```
```json
{"ok":true,"command":"journey.compile","data":{"files":[…],"skipped":[],"written":1},"diagnostics":[]}
```

Other flags: `--fmt` rewrites the YAML canonically; `--force` replaces a differing handwritten
contract; positional ids limit the selection.

Approve binds the flow to the project's committed `HEAD` (commit first; SHA-256 git repositories
are refused):

```sh
git add -A && git commit -m "journey: checkout flow"
graphhelm journey approve checkout --json
```
```json
{"ok":true,"command":"journey.approve","data":{"approved":{"digest":"sha256:c1be00bf…","revision":"26ac5f4c…"},"id":"checkout","status":"approved","written":2},"diagnostics":[]}
```

It writes `status: approved` plus `approved: {digest, revision}` into the YAML and regenerates
`checkout.json`. Commit both files.

**Approval belongs to the owner** (a person, in the Studio or with `journey approve`), never to an
agent approving its own flow. To change an approved flow: set `status: draft` and `approved: null`
(keep `drift`), edit, `journey validate`, `journey compile --include-draft`, then ask the owner to
re-approve. While `status: approved`, an edited flow reports `flow.approval_stale`, and
`compile --include-draft` does not regenerate it.

## 4. Replay (deterministic, no model)

```sh
graphhelm setup --project . --home <home> --install-observer playwright   # once: @playwright/test, Chromium, .graphhelm/observers/journey_driver.mjs
graphhelm journey replay checkout --json \
  [--events .graphhelm/events --execution <run> --keyring .graphhelm/keyring --key-id studio] \
  [--allow-origin https://cdn.example.com]
```

Replay walks every path of an **approved** flow in a real browser, without a model call, checks
each screen's `expect`, and with the four recording flags (all or none, else
`replay.recording_incomplete`) records the same captures and walked transitions as section 5.
Secrets come from `GRAPHHELM_SECRET_<name>` environment variables. Navigation away from `base`
is refused; `--allow-origin` only permits extra subresource origins.

Without the observer installed (real output):

```json
{"ok":false,"command":"journey.replay","data":{"cachePublished":false,"flowId":"checkout","modelCalls":0,"partialEffects":"retained","paths":[],"recording":"not_requested"},
 "diagnostics":[{"code":"replay.observer_missing","severity":"error","message":"OBSERVER_MISSING: run setup --install-observer playwright in this project, then retry with a runnable Node/Playwright/Chromium observer","path":"/observer","source":"graphhelm"}]}
```
exit `3`. Other stable codes: `replay.timeout`, `replay.cleanup_uncertain`,
`replay.worker_invalid`, `replay.cache_write_refused`, `driver.host_refused`,
`driver.capture_refused`, `driver.secret_missing`. A draft or stale-approved flow is refused
before any browser action. This guide's run had no Playwright install, so a successful replay is
**not** shown here: `OBSERVER_MISSING`.

## 5. Record proof by hand: capture and walked

A run must exist first (any execution id works; this one is offline with a fixture):

```sh
echo '{"nodeOutcomes":{"implementation":"failure"}}' > .graphhelm/fixtures.json
graphhelm execution start --file <graphhelm-clone>/examples/graphs/manual-override-deploy.yaml \
  --events .graphhelm/events --fixtures .graphhelm/fixtures.json --mode supervised --execution demo
```

Then, with `R="--events .graphhelm/events --execution demo --keyring .graphhelm/keyring --key-id studio --contract checkout"`:

```sh
graphhelm journey capture $R --step cart --image cart.png --json
```
```json
{"ok":true,"command":"journey.capture","data":{"attachments":[{"bytes":75,"evidenceId":"signal-journey-211d935c-…-image-1","mediaType":"image/png"}],"decision":"rejected","executionId":"demo","kind":"jpd.screen_captured","mayProposeMutation":false,"outcome":"recorded","rejectionReason":"signal_not_actionable","signalId":"journey-211d935c-…"},"diagnostics":[]}
```

`"outcome":"recorded"` is what matters. `decision: rejected / signal_not_actionable` only means
the signal proposes no graph mutation; it is stored. The image is PNG, JPEG or WebP; a non-PNG
needs `--viewport WIDTHxHEIGHT`. The capture is stamped with `HEAD` and whether the tree is dirty.

```sh
graphhelm journey capture $R --step pay --image pay.png --pr 7 --phase after --json
graphhelm journey walked  $R --from cart --to pay --json
```
```json
{"ok":true,"command":"journey.walked","data":{…,"kind":"jpd.transition_walked","outcome":"recorded","signalId":"journey-af72a3bb-…"},"diagnostics":[]}
```

`walked` cites the newest capture of each of two **consecutive** steps; pin specific ones with
`--from-capture <id> --to-capture <id>` (both or neither).

## 6. Read the map: freshness

```sh
graphhelm journeys --events .graphhelm/events --keyring .graphhelm/keyring --key-id studio --json
```
```json
{"ok":true,"command":"journeys.read","data":{"head":"26ac5f4c…","ignoredRecords":0,"journeys":[{
 "arrows":[{"fromStepId":"cart","state":"walked","toStepId":"pay",…}],
 "contractId":"checkout","steps":[
  {"capture":{"changedFiles":[],"dirty":false,"executionId":"demo","freshness":"fresh",…},"stepId":"cart",…},
  {"capture":{"changedFiles":[],"freshness":"fresh","phase":"after","pr":7,…},"stepId":"pay",…}],…}],
 "refusedContracts":[],"scope":"project"},"diagnostics":[]}
```

The map folds captures from **every run** of the project (`--execution` only asserts a run
exists). Now commit a change to `app/checkout/page.tsx` and read again:

```json
{… "head":"e48aae97…", "arrows":[{"fromStepId":"cart","state":"stale","toStepId":"pay",…}],
 "steps":[{"capture":{"freshness":"fresh",…},"stepId":"cart"},
          {"capture":{"changedFiles":["app/checkout/page.tsx"],"freshness":"stale",…},"stepId":"pay"}]}
```

| `freshness` | Meaning |
|---|---|
| `fresh` | no file in the screen's `scopePaths` changed between the capture's revision and `HEAD` |
| `stale` | some did; `changedFiles` names them |
| `unknown` | the capture was taken on a dirty tree, a scope path is missing, or history cannot answer |

Arrow `state`: `walked`, `never_walked`, or `stale` (an endpoint capture went stale).

## 7. Studio Journey tab

`graphhelm studio start` (from the project directory) opens the Studio for that project. The
**Journey** tab on the canvas draws the same map as section 6: one card per step with its newest
capture and freshness, arrows between steps. PR before/after pairs (section 9) open from it. Not
exercised in this guide's run (it needs Node and the Studio app).

## 8. Keel cards that name journeys (advisory)

A Keel card can name journeys (`"journeys": [...]` in JSON, a `Journeys:` line in a markdown
card). `keel check` then warns when the diff touches a screen whose newest capture is not fresh:

```sh
cat card.json
# {"promise":"the pay screen keeps its password field","scopePaths":["app/checkout/page.tsx"],
#  "proof":"graphhelm journey replay checkout","journeys":["checkout"]}
graphhelm keel check --diff HEAD~1..HEAD --card card.json \
  --events .graphhelm/events --execution demo --keyring .graphhelm/keyring --key-id studio --json
```
```json
"diagnostics":[{"code":"keel.journey.no_fresh_capture","severity":"warning",
 "message":"keel.journey.no_fresh_capture: checkout/pay: code changed after the capture at 26ac5f4c: app/checkout/page.tsx",
 "path":"app/checkout/page.tsx","source":"keel"}]
```
exit `0`: journey findings are warnings, never blocks. Without the four record flags every
touched screen is reported as having no capture read. A named journey with no readable contract is
`keel.journey.contract_unreadable`. Rules: [`docs/keel/KEEL_CHECK.md`](../keel/KEEL_CHECK.md).

## 9. PR before/after

Capture the same step with `--pr <N> --phase before` on the base and `--phase after` on the
branch. The Studio pairs, per `(contractId, stepId, pr)`, the newest `before` with the newest
`after` and shows them side by side; a step with only one phase shows no pair.

## 10. Coming next (not on `main`; do not rely on it)

- **Phase 3, agentic explore:** a model drafts and extends flows by exploring the running app,
  with redacted model input and deduplication. Plan:
  [`docs/superpowers/plans/2026-10-07-journey-explore-phase-3.md`](../superpowers/plans/2026-10-07-journey-explore-phase-3.md).
- Later phases: persisted drift and edge-local healing, Studio graph and approval routes,
  source-scope discovery. Design: [`docs/specs/2026-10-06-journey-explore-design.md`](../specs/2026-10-06-journey-explore-design.md).

## See also

- Mapping an existing app from zero: skill `journey-map`
  (`extensions/builtin/graphhelm-jpd/skills/journey-map/SKILL.md`).
- One new behavior: skill `journey-contract`.
- Signal records: [`docs/keel/RECORDS.md`](../keel/RECORDS.md).
- Why observers: [`docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`](../harness/JOURNEY_PROVEN_DEVELOPMENT.md).
