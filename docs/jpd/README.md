# Journey-Proven Development: a portable guide

Journey-Proven Development (JPD) starts every change from a promise the user can observe ("the
CLI greets me", "`GET /items.json` lists apple") and writes that promise down as a **journey**: a
small graph of screens (observable states) and edges (the acts that move between them). The
journey is the proof. A change is done when an observer has watched the journey hold at the
committed head, and a deliberately broken product makes the same journey fail. This guide is the
part any project can adopt with the `graphhelm` CLI; the full design is
[JOURNEY_PROVEN_DEVELOPMENT.md](../harness/JOURNEY_PROVEN_DEVELOPMENT.md). A runnable example is
[`examples/jpd-quickstart/`](../../examples/jpd-quickstart/README.md).

## Rules

1. **A promise is done only when an observer proves it.** Agent agreement, a passing build or a
   reviewer's reading is advice, not proof.
2. **No observer, no proof.** When nothing installed can observe the promise at the strength it
   needs, the answer is `OBSERVER_MISSING`, never a weaker check silently counted as proof.
3. **A journey must catch a break.** Green on good code is half the proof; a sabotage patch must
   come back `caught`.
4. **Only the owner approves.** A draft journey is never proof and never runs a command or a
   mutating request.
5. **Failures are kept.** A later green run does not erase an earlier failure.

## The journey file

One file per journey: `.graphhelm/journeys/<id>.journey.yaml`, schema `graphhelm.journey-flow/1`
([`schemas/journey-flow.schema.json`](../../schemas/journey-flow.schema.json), version 1.4.0).

```yaml
schema: graphhelm.journey-flow/1
id: quickstart                 # lowercase, matches the file name
title: Client greets and lists items
status: draft                  # draft | approved (approval is written by `journey approve`)
approved: null
base: http://127.0.0.1:8765    # where http acts and browser screens are resolved
actors: [client]
secrets: []                    # ids of secrets an act may use (never the values)
risks: []                      # e.g. irreversible_effect, money, personal_data, security
screens:
  - id: start
    url: /health.json          # path on base; the start screen of an http flow is GET here
    state: stable              # loading|disabled|empty|error|retrying|success|partial_success|recovered|stable
    expect: [{role: status, name: "200"}, {role: json, name: status=ok}]
    scope: unknown             # or a list of project paths this screen depends on
  - {id: greeted, url: /, state: success, scope: unknown,
     expect: [{role: exit, name: "0"}, {role: stdout, name: hello}]}
edges:
  - id: greet
    from: start
    to: greeted
    acts: [{kind: cli, role: exec, name: sh hello.sh}]
paths:
  main: [greet]                # every path is a sequence of edges; `main` is required
drift: []
```

**Acts** (1 to 8 per edge, each `{kind, role, name}` plus optional `text` and `secret`):

- **Browser acts** (`navigate`, `activate`, `enter_text`, `select`, `upload`, `download`,
  `wait_for`, `inspect`, `submit`, `recover`, `approve`) target an element by accessible `role`
  and `name`; screen `expect` entries are `{role, name}` elements that must be visible. They need
  the browser observer (`graphhelm setup --project . --install-observer playwright`).
- **`http`**: `{kind: http, role: GET, name: /items.json}` sends the method in `role` to the path
  on `base`, with optional JSON body `text` and optional `headers`, a map of fixed, non-secret
  request headers (`headers: {x-tenant-id: t1}`; at most 16; `Host`, `Content-Length`,
  `Connection` and `Transfer-Encoding` are refused as `flow.backend_header_reserved`). Do not put
  a token in `headers`: the flow file is committed.
- **`cli`**: `{kind: cli, role: exec, name: sh hello.sh}` runs the argv in `name`, split on
  whitespace with no shell, in the project directory.

A flow is either a browser flow or a backend (`http`/`cli`) flow, never both. Backend `expect`
roles are `status` (HTTP status, exact), `json` (`a.b.0=value` compares a field, `a.b` asserts it
exists), `text` (body contains), `exit` (exit code, exact) and `stdout` (stdout contains). Each act
is bounded at 30 seconds. An act that cannot run is `fail`; an unmet expectation is `drift`.

**Launcher.** A flow that sends requests needs the app running on an isolated port. Declare a
launcher in `.graphhelm/journey-fixture.json`:
`{"schema":"graphhelm-journey-fixture/1","script":"launch.sh","isolated":true}`. The script is
called as `up <dir> <runtime port> <app port>` and `down <dir>`. A `cli`-only flow needs none.
The app port is taken as ready as soon as it accepts a connection, so bind it only once the app
answers. When `up` fails, the tail of its stdout and stderr is kept in the validation report
(`failures[].launchLog`: `{path, tail}`); `<dir>` itself is removed.

**Generated contracts.** `journey compile` writes `<id>.json` (and `<id>.<path>.json`) next to the
YAML. Any other `*.json` in `.graphhelm/journeys/` (a hand-written or legacy contract) is never
executed: validation lists it under `standaloneContracts` as `unsupported` with reason
`standalone_contract_not_executed`, and under `unknown`. It does not block a sweep whose every
declared path completed, and it never counts as evidence. Move such files out of the directory to
keep the report clean.

## Commands

All take `--json` for the machine-readable envelope.

| Command | What it does |
|---|---|
| `graphhelm init --harness claude-code --project .` | Creates `.graphhelm/` and the owner token `.graphhelm/events.token`, and appends `.graphhelm/*`, `!.graphhelm/journeys/` and `!.graphhelm/journey-fixture.json` to `.gitignore`. |
| `graphhelm journey compile <id> --fmt --include-draft --project .` | Checks and formats the flow, writes its generated contract. |
| `graphhelm journey flows --project .` | Lists every flow with status, findings and whether it can be approved. |
| `graphhelm journey preview <id> --project .` | Plays a flow in the background (never proof); read it with `--read`, release a held destructive act with `--confirm`. |
| `graphhelm journey approve <id> --token-file .graphhelm/events.token --project .` | The owner approves the flow at HEAD; commit the result. |
| `graphhelm journey replay <id> --project .` | Replays an approved browser flow with the installed observer. |
| `graphhelm keel test validation --repo .` | Runs every journey path at committed HEAD in isolation. Exit 0 when every path of an approved flow is `completed`; 2 otherwise, with diagnostic `GHCLI039_KEEL_VALIDATION_BLOCKED` and `data.blockingReasons` naming why (for example `flow <id>: execution_failed, <detail>, flow_not_approved` for a failed draft: the concrete cause first, the draft beside it). |
| `graphhelm keel test sabotage --repo . --patch <break.diff>` | Applies a product-breaking patch in a temporary worktree and reruns validation. |

Sabotage verdicts (only `caught` exits 0; every other verdict exits 2):

- `caught`: at least one path ran and failed (`failed` or `drift`) after the patch.
- `missed`: every path stayed green; the journeys do not observe the patched code.
- `no_journey_ran`: nothing observed the break (no report, or paths only `not_run` or `held`).
- `journey_set_changed`: the patch changed which journey paths exist.
- `patch_touches_observer`: the patch edits `.graphhelm/`; sabotage must break the product, not the test.
- `baseline_not_green`: validation was not green before the patch.
- `patch_not_applicable`: `git apply` refused the patch at HEAD.

Validation and sabotage test only committed HEAD; uncommitted edits are invisible to them.

**Dependencies in the sabotage worktree.** Sabotage runs in a fresh worktree, which has no
git-ignored files. It links every `node_modules` it finds in your checkout (the root and each
workspace package's own, up to four levels deep, as pnpm lays them out) into the same place in the
worktree, and lists them in `linkedDependencies`. Other dependencies (a virtualenv, build output)
your launcher must install or link itself. When the baseline is red and the launcher output names
a missing module, the report says `baselineCause: dependencies_missing`; `baselineFailures` and
`baselineBlocking` carry the baseline's own reasons.

## Safety: drafts, approval and destructive acts

- A **draft** never runs a `cli` act (`cli_needs_approval`) and never sends a mutating request
  (`guard_refused`). Approval records the flow's digest at a commit, so the owner
  approves the exact argv and requests the flow holds.
- Any `http` method other than `GET`, `HEAD` or `OPTIONS` is a mutation, whatever its path says.
  In an approved flow it is held at `confirm_needed` until the owner runs
  `journey preview <id> --confirm`. A `cli` argv containing `rm`, `push`, `drop` or a deny-list
  word is held the same way.
- **Validation and sabotage never confirm**, so they never perform a held destructive act.
- Approval needs the owner token. Keep `.graphhelm/*` ignored except `.graphhelm/journeys/` and
  `.graphhelm/journey-fixture.json`.

## Adopt it in your project in 5 steps

1. **Install and init.** Install `graphhelm`, then run `graphhelm init --harness claude-code
   --project .` in your git repository. Track the journeys, not the token: `init` appends
   `.graphhelm/*`, `!.graphhelm/journeys/` and `!.graphhelm/journey-fixture.json` to
   `.gitignore`. Do not add a bare `.graphhelm/` line: git cannot re-include a file whose parent
   directory is ignored.
2. **Write one promise as a journey.** Pick the one behavior a user would notice first, write
   `.graphhelm/journeys/<id>.journey.yaml`, add a launcher if it sends requests, and run
   `journey compile <id> --fmt --include-draft`. Commit.
3. **Approve it as the owner.** Read the argv and requests, run `journey approve`, commit.
4. **Prove it holds.** `keel test validation --repo .` must exit 0.
5. **Prove it catches a break.** Write a small patch that breaks the promise in product code and
   run `keel test sabotage --patch`; it must say `caught`. A `missed` means the journey does not
   observe what you think it does: fix the journey, not the patch.
