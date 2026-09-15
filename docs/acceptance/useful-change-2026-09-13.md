# A useful change lands, run by hand (2026-09-13)

Issue #1066. A Runtime with **no model credential** — no manifest, no broker, no route, only
`--staging`, `--allow-program`, `--tests-runner` and the sealing keyring — takes a graph of tool
nodes over HTTP, applies a patch to a git project, runs the project's test against the patched
tree, commits, and lands the commit as `refs/graphhelm/executions/<execution id>` in the project.
The operator's `HEAD` never moves. A second execution, compiled by `graph synthesize` from a
recorded fixture, proves the landed change on the same server.

The CLI journey test `a_useful_change_lands_with_tools_and_no_model_credential`
(`apps/cli/tests/runtime_http.rs`) holds this run; the transcript below is the same road walked
once by hand, with the real outputs pasted. The operator doc is
`docs/operations/TOOLS_ONLY_RUNTIME.md`.

Head run against: `07eb961b` on `issue-1066-useful-change-lands`. Binary: `cargo +1.97.1 build
-p graphhelm-cli --locked`, debug profile. `<tmp>` is an empty scratch directory; no path below
depends on it. `GRAPHHELM_EVENTS_KEY` is 64 hex characters set in the shell; no other secret
exists anywhere in this run, and the bearer token is read from `<tmp>/events.token` and never
printed.

**Declared gap (2026-09-13):** the architect's template offers the repository's reads, shell and
tests, and refuses `apply_patch`/`commit` (`core/architect/src/synthesize.rs`, `tool_call_refusal`,
`GHA003_TOOL_CALL_MISSING`), so the graph that makes the change (§3) is authored by hand and the
synthesized graph (§2, §8) is the verify half. Teaching the architect to offer repository writes
is a product decision outside #1066.

## 0. The project

One file, one commit, and a "test" — `git grep -n FIXED -- src/lib.rs` — that fails before the
change (exit 1) and passes after it. `--tests-runner git` makes `git` the runner; the node
supplies only the arguments.

```text
$ git init project; commit src/lib.rs; git grep -n FIXED -- src/lib.rs (expected: exit 1)
HEAD before: 7ad75dc4e7a4ddd7aafe02ad75d7834dddfe6762
exit: 1
```

## 1. The key — `gateway keyring init`, exit 0

The keyring directory must exist first: `keyring init` refuses with `GHCLI010_GATEWAY_CREDENTIAL:
the keyring directory does not exist` rather than create it (finding F4 of
`mvp-integrated-2026-09-14.md`; in this run `<tmp>/keyring` had been created by the setup step).

```text
$ mkdir <tmp>/keyring
$ graphhelm gateway keyring init --keyring <tmp>/keyring --key-id useful-change
{"ok":true,"command":"gateway.keyring.init","data":{"createdUnder":"GRAPHHELM_EVENTS_KEY","keyId":"useful-change"},"diagnostics":[]}
```

## 2. `graph synthesize` from the recorded fixture — exit 0

`GOAL.txt` is `prove that src/lib.rs defines the FIXED constant by running the repository tests`,
read by the crate test (`core/architect/tests/golden.rs`,
`the_useful_change_goal_compiles_to_one_tests_node_and_nothing_cognitive`) and by this run so one
string exists. The reply was recorded the way `core/architect/fixtures/README.md` describes:
`rounds` authored by hand, `ARCHITECT_RECORD=1` filing it under the prompt sha the compiler asked
for, a second run without the variable proving the committed file answers. `--allow-program git`
is the catalog; the draft names no shell program at all.

```text
$ graphhelm graph synthesize --goal "$(cat core/architect/fixtures/useful-change/GOAL.txt)" --out <tmp>/prove-fixed.json --allow-program git --fixture core/architect/fixtures/useful-change/replies.json
{"ok":true,"command":"graph.synthesize","data":{"document":{"apiVersion":"p50.dev/graph/v1","kind":"ExecutionGraph","metadata":{"executionId":"exec_1c792165","id":"arch_1c792165_v1","labels":{"origin":"architect","template":"32236956956cd087fb4a7fb5b043a9a6e10716f7601191b08c79ba87fc84f123"},"name":"prove that src/lib.rs defines the FIXED constant by running the repository tests","version":1},"spec":{"budgets":{"maxNodes":1},"completion":{"terminalNodes":["run_tests"]},"edges":[],"entrypoints":["run_tests"],"nodes":{"run_tests":{"completion":{"customs":{"budgets":{"clearanceWithinSeconds":3600,"waitWithinSeconds":86400},"proofKinds":[]}},"name":"Run the tests","objective":"Run the repository tests and record whether src/lib.rs defines the FIXED constant.","optionality":"required","tool":{"call":{"arguments":["grep","-n","FIXED","--","src/lib.rs"],"tool":"tests"}},"type":"tool"}},"policies":[]}},"out":"<tmp>/prove-fixed.json","promptSha256s":["e7530161fa43d4b72c1b9b16f8143710ddd1ac86acdab58528fe0fc3209f35ec"],"rationale":[{"node":"run_tests","reason":"why it exists: Run the repository tests and record whether src/lib.rs defines the FIXED constant.; customs stamped by the compiler (waitWithinSeconds 86400, clearanceWithinSeconds 3600)"}],"rounds":1,"stampedCustoms":["run_tests"],"templateSha256":"32236956956cd087fb4a7fb5b043a9a6e10716f7601191b08c79ba87fc84f123"},"diagnostics":[]}

$ graphhelm graph lint <tmp>/prove-fixed.json
{"ok":true,"command":"graph.lint","data":{"errors":[],"warnings":[{"code":"GHG101_DEFAULT_TIMEOUT","message":"executable node relies on the runtime default timeout","path":"/spec/nodes/run_tests/timeoutSeconds","severity":"warning","source":"prove-fixed.json"}],"diagnostics":[]}
```

`promptSha256s` is the one key `replies.json` holds; zero errors, and the one warning is the
runtime default timeout every synthesized node carries.

## 3. The change graph

Three tool nodes, control edges in order, `land` terminal — the same document the CLI journey
writes (`useful_change_graph` in `apps/cli/tests/runtime_http.rs`):

```yaml
apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_useful_change_v1
  name: A useful change lands
  executionId: exec-useful-change
  version: 1
spec:
  entrypoints: [apply_fix]
  nodes:
    apply_fix:
      type: tool
      name: Apply the fix
      objective: Apply the unified diff that adds the FIXED constant.
      optionality: required
      tool:
        call:
          tool: repository
          action: apply_patch
          patch: |
            --- a/src/lib.rs
            +++ b/src/lib.rs
            @@ -1 +1,2 @@
             // scratch
            +pub const FIXED: bool = true;
    run_tests:
      type: tool
      name: Run the tests
      objective: Prove the fix with the configured tests runner.
      optionality: required
      tool:
        call:
          tool: tests
          arguments: [grep, -n, FIXED, --, src/lib.rs]
    land:
      type: tool
      name: Commit the fix
      objective: Record the tested tree as a commit the operator can merge.
      optionality: required
      tool:
        call:
          tool: repository
          action: commit
          message: "fix: add the FIXED constant (landed by the execution)"
  edges:
    - { id: apply_to_tests, from: apply_fix, to: run_tests, type: control }
    - { id: tests_to_land, from: run_tests, to: land, type: control }
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes: [land]
```

## 4. `serve`, tools only

No `--manifest`, no `--broker`, no `--route`, no `GRAPHHELM_GATEWAY_KEY`.

```text
$ graphhelm serve --events <tmp>/events --bind 127.0.0.1:0 --staging <tmp>/staging --allow-program git --tests-runner git --keyring <tmp>/keyring --key-id useful-change
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:60000"},"diagnostics":[]}
```

## 5. `execution start` over HTTP — 200, `completed`

```text
$ curl -X POST http://127.0.0.1:60000/v1/executions/exec-useful-change/start -H 'Authorization: Bearer <token>' -H 'Idempotency-Key: useful-change-start' -H 'X-GraphHelm-Actor: owner-local' -H 'X-GraphHelm-Actor-Type: owner' -d '{"file":"<tmp>/useful-change.yaml","mode":"autopilot","project":"<tmp>/project"}'
{"ok":true,"command":"execution.start","data":{"acceptedMutations":0,"attention":"can_sleep","attentionReasons":[],"customs":{"clearances":{},"nodes":{},"quarantinedNodes":[]},"executionId":"exec-useful-change","headSequence":15,"lastEventAt":null,"mode":"autopilot","nodeLastEventAt":{},"nodeStateCounts":{"blocked":0,"cancelled":0,"draft":0,"failed":0,"ghost":0,"invalidated":0,"linting":0,"paused":0,"queued":0,"ready":0,"running":0,"skipped":0,"succeeded":3,"waiting_capacity":0,"waiting_input":0,"waived":0},"nodeStates":{"apply_fix":"succeeded","land":"succeeded","run_tests":"succeeded"},"signalsRecorded":0,"silenceUnevaluated":[],"startedAt":null,"status":"completed","untriagedInterruptions":[]},"diagnostics":[]}
```

## 6. The ref, in the project

```text
$ git -C <tmp>/project rev-parse refs/graphhelm/executions/exec-useful-change
e83e553422f7c7d375e6aa44236b4ca05a8f7606

$ git -C <tmp>/project log --format='%H %s' refs/graphhelm/executions/exec-useful-change
e83e553422f7c7d375e6aa44236b4ca05a8f7606 fix: add the FIXED constant (landed by the execution)
7ad75dc4e7a4ddd7aafe02ad75d7834dddfe6762 scratch

$ git -C <tmp>/project show refs/graphhelm/executions/exec-useful-change:src/lib.rs
// scratch
pub const FIXED: bool = true;

$ git -C <tmp>/project rev-parse HEAD   (unchanged: 7ad75dc4e7a4ddd7aafe02ad75d7834dddfe6762)
7ad75dc4e7a4ddd7aafe02ad75d7834dddfe6762

$ git -C <tmp>/project branch --list; git -C <tmp>/project status --short; ls <tmp>/staging
* master
```

One commit on top of where the project started; `HEAD` unchanged; the working tree clean; the
only branch is the operator's `master`; the staging directory is empty (the execution's workspace
was released when the drive ended — the ref is what stays).

## 7. The sealed evidence, through the API

Every tool outcome sealed four items (record, stdout, stderr, accounting receipt):

```text
$ GET http://127.0.0.1:60000/v1/executions/exec-useful-change/events?limit=1000
apply_fix succeeded ['exec-exec-useful-change-apply_fix-a1-record', 'exec-exec-useful-change-apply_fix-a1-stdout', 'exec-exec-useful-change-apply_fix-a1-stderr', 'exec-exec-useful-change-apply_fix-a1-accounting-receipt']
run_tests succeeded ['exec-exec-useful-change-run_tests-a1-record', 'exec-exec-useful-change-run_tests-a1-stdout', 'exec-exec-useful-change-run_tests-a1-stderr', 'exec-exec-useful-change-run_tests-a1-accounting-receipt']
land succeeded ['exec-exec-useful-change-land-a1-record', 'exec-exec-useful-change-land-a1-stdout', 'exec-exec-useful-change-land-a1-stderr', 'exec-exec-useful-change-land-a1-accounting-receipt']
```

The commit node's record names the commit and the ref (`data.content`, decrypted by the server
under the keyring):

```text
$ GET http://127.0.0.1:60000/v1/executions/exec-useful-change/evidence/exec-exec-useful-change-land-a1-record
{"tool":"repository","action":"commit","actor":"runtime","programAllowlist":["git"],"tier":"tier_1","disposition":{"kind":"completed","exit_code":0},"stdoutSha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","stdoutBytes":0,"stderrSha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","stderrBytes":0,"truncated":false,"reused":false,"commit":"e83e553422f7c7d375e6aa44236b4ca05a8f7606","landedRef":"refs/graphhelm/executions/exec-useful-change"}
```

The tests node's stdout is the runner's real output — the grep hit on the patched line:

```text
$ GET http://127.0.0.1:60000/v1/executions/exec-useful-change/evidence/exec-exec-useful-change-run_tests-a1-stdout
src/lib.rs:2:pub const FIXED: bool = true;
```

## 8. The synthesized graph proves the landed change, on the same server

The operator checks the ref out (`git worktree add`, their own act) and starts the synthesized
document against that tree; then the same document against the unchanged project, which must
fail — the discriminator that shows the runner is judging the tree it was pointed at.

```text
$ git -C <tmp>/project worktree add --detach <tmp>/landed refs/graphhelm/executions/exec-useful-change
HEAD is now at e83e553 fix: add the FIXED constant (landed by the execution)

$ curl -X POST http://127.0.0.1:60000/v1/executions/exec_1c792165/start ... -d '{"file":"<tmp>/prove-fixed.json","mode":"autopilot","project":"<tmp>/landed"}'
{"ok":true,"command":"execution.start","data":{"acceptedMutations":0,"attention":"can_sleep","attentionReasons":[],"customs":{"clearances":{},"nodes":{},"quarantinedNodes":[]},"executionId":"exec_1c792165","headSequence":7,"lastEventAt":null,"mode":"autopilot","nodeLastEventAt":{},"nodeStateCounts":{"blocked":0,"cancelled":0,"draft":0,"failed":0,"ghost":0,"invalidated":0,"linting":0,"paused":0,"queued":0,"ready":0,"running":0,"skipped":0,"succeeded":1,"waiting_capacity":0,"waiting_input":0,"waived":0},"nodeStates":{"run_tests":"succeeded"},"signalsRecorded":0,"silenceUnevaluated":[],"startedAt":null,"status":"completed","untriagedInterruptions":[]},"diagnostics":[]}

$ the same synthesized graph against the UNCHANGED project (the test must fail there)
{"ok": true, "status": "failed", "nodeStateCounts": {"blocked": 0, "cancelled": 0, "draft": 0, "failed": 1, "ghost": 0, "invalidated": 0, "linting": 0, "paused": 0, "queued": 0, "ready": 0, "running": 0, "skipped": 0, "succeeded": 0, "waiting_capacity": 0, "waiting_input": 0, "waived": 0}, "diagnostics": []}
```

The last line is **summarized by the run script** (it printed `ok`, `status`, `nodeStateCounts`
and `diagnostics` out of the full `execution.start` envelope; the other fields are the same
shape as the envelopes above). Every other block in this record is the command's output verbatim.
Note: this run predates the hooks/recovery/compare-and-swap fixes made on review of #1073; none
of them changes a line of it (no hooks, no stale tree, and a ref that did not exist before the
landing).

## 9. `events verify` and a byte-identical double replay

```text
$ graphhelm events verify --repository <tmp>/events
{"ok":true,"command":"events.verify","data":{"formatSupported":true,"verified":false},"diagnostics":[]}

$ graphhelm graph replay --events <tmp>/events | sha256sum   (twice)
d8eb4d23c5bca463159a82d7f23f0373d18d7167d5c6317a8f4f585488478233 *-
d8eb4d23c5bca463159a82d7f23f0373d18d7167d5c6317a8f4f585488478233 *-

$ stderr of serve (expected empty):
```

`verified:false` is the documented answer for a local repository: `--repository` recognizes the
stored format and refuses nothing, while chain verification (`verified:true`, `verifiedEvents`) is
an `AsyncEventRepository` capability the PostgreSQL adapter provides under `--config`
(`apps/cli/src/commands/events/verify.rs`, `verify_local`). The replay is the same bytes twice.

## What this run establishes

| promise | observer | result |
|---|---|---|
| real tools with no model credential | `serve` accepted `--staging`/`--allow-program`/keyring alone (§4); no gateway key set | started; refused before #1066 with `--manifest, --broker, --route and --staging must be given together or not at all` (the RED run of `d7b24377`) |
| one workspace per execution | `apply_fix`'s patch was what `run_tests` tested and `land` committed (§5–§7) | three nodes succeeded on one tree |
| the commit lands as a ref | `git rev-parse refs/graphhelm/executions/exec-useful-change` (§6) | `e83e5534…`, parent `7ad75dc4…` |
| the operator's checkout is untouched | `HEAD`, `status --short`, `branch --list` (§6) | unchanged, clean, `master` only |
| the workspace is released, the ref stays | `ls <tmp>/staging` (§6), the ref afterwards (§8) | empty; ref checked out by the operator |
| the sealed tests output is the runner's | §7 stdout evidence | `src/lib.rs:2:pub const FIXED: bool = true;` |
| the record names the ref and the commit | §7 record evidence | `commit`, `landedRef` present |
| a synthesized graph runs tools-only | §2, §8 | completed on the landed tree, failed on the unchanged one |
| the stream replays byte-identically | §9 | same sha256 twice |

Out of scope, as the issue names it: merging the ref into the operator's branch (`git merge
refs/graphhelm/executions/exec-useful-change` is the operator's act), deploy (#114), Tier 2/3.
