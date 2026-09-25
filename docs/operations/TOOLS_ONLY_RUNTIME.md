# Tools-only Runtime: a useful change with no model credential

Issue #1066. A Runtime can run real tool nodes — `apply_patch`, `tests`, `commit`, `shell`,
`diff` — with no model route configured at all. The change an execution makes survives the
execution, and it lands where the operator can see it and nowhere the operator did not choose.

## What `serve` accepts

The real-executor wiring is two independent halves. Each is all-or-none on its own; either,
both, or neither may be given. Anything real requires the keyring pair.

| half | flags | what it wires |
|---|---|---|
| model | `--manifest --broker --route` | cognitive nodes (agent, planner, classifier, evaluator) run on the named route |
| tool | `--staging --allow-program <program>…` | tool nodes run on the real tool host |
| sealing | `--keyring --key-id` | evidence sealing; required with either half, usable alone for `signal` |

- **Neither half**: the fixture-only server. Every node is answered by node fixtures.
- **Tool half only**: tool nodes are real; cognitive nodes are answered by node fixtures exactly as
  in fixture-only mode (a cognitive node with no fixture parks as `needs_input`, and the
  fixture-only diagnostic still explains it). No `GRAPHHELM_GATEWAY_KEY` is needed; only
  `GRAPHHELM_EVENTS_KEY`, for sealing.
- **Model half only**: cognitive nodes are real; tool nodes are answered by node fixtures.
- **Both**: the all-real composition, unchanged.

A half-given group refuses at startup with `GHCLI006_SERVE_INVALID`:

```text
--manifest, --broker and --route must be given together or not at all
--staging and --allow-program must be given together or not at all: the set of programs an execution may spawn is declared per run, never defaulted
the real-executor flags require --keyring and --key-id as well
```

The refusal names flags, never programs: the allowlist is declared per run, not suggested.

A tools-only server:

```sh
# GRAPHHELM_EVENTS_KEY (64 hex) is required by `keyring init` and by `serve`: it is the
# passphrase the sealing key is wrapped under. The keyring DIRECTORY must exist before
# `keyring init`: the command refuses with `GHCLI010_GATEWAY_CREDENTIAL: the keyring
# directory does not exist` rather than create one (integrated verification, F4).
mkdir -p <keyring>
GRAPHHELM_EVENTS_KEY=<64 hex> graphhelm gateway keyring init --keyring <keyring> --key-id runtime-key   # once
GRAPHHELM_EVENTS_KEY=<64 hex> graphhelm serve \
  --events <events> --bind 127.0.0.1:0 \
  --staging <staging> --allow-program git --tests-runner cargo \
  --keyring <keyring> --key-id runtime-key
```

`--tests-runner` is host configuration (the program a `tests` node runs; default `cargo`) and is
never in the allowlist — a node cannot rename its way around the lease.

## One Tier 1 workspace per execution

The tool host keys one detached git worktree on the execution id. The first Tier 1 call of an
execution provisions it under `--staging`; every later call of that execution runs in the same
tree, so the patch node A applied is still there when node B tests it and node C commits it.
Two calls of one execution serialize on that tree; two executions run side by side in two
trees. Deployment assumption: one Runtime per staging directory — two Runtimes sharing a
`--staging` and driving the same execution id would each take the other's tree for a stale one
and reclaim it (declared gap, 2026-09-14, no follow-up filed).

The tree lives exactly as long as the drive: when the execution reaches a terminal state, pauses,
or is cancelled, the workspace is removed. Nothing uncommitted survives that removal — the tree
is scratch. The ref is the result (next section), and a later drive of the same execution
provisions its next tree **from that ref**, not from the operator's `HEAD`, so committed work
continues where it left off.

`graphhelm tool invoke` (and `--keep-workspace`) keep the per-call contract they always had: a
fresh tree per call, torn down after it. A commit made that way names its object id in the
record but lands no ref — there is no execution to land under.

Containment is unchanged: `WorkspaceConfig::validated` still refuses a staging area inside the
project or overlapping the keyring/broker, and the lease still decides what a call may do. The
execution id is a workspace key, not a capability.

**Hooks are disabled on every git spawn, not only at provisioning.** Provisioning runs
`git worktree add` with `core.hooksPath` pointed at an empty directory; every other spawn through
the tool host — `git apply`, `git add`/`git commit`, `git update-ref`, the tests runner, and any
allowlisted program that shells out to git — carries `GIT_CONFIG_COUNT=1` /
`GIT_CONFIG_KEY_0=core.hooksPath` / `GIT_CONFIG_VALUE_0=<staging>/ghtool-nohooks-<pid>-<token>-<n>`,
an empty directory created for that one spawn as a sibling of the workspace — outside the tree
the execution's children write to, so a hook a shell or tests call plants under its `.home` is
never where git looks — refused if the name already exists, and removed when the call returns.
So the project's `pre-commit`, `post-commit`, `reference-transaction` and every other hook never
run on an execution's behalf, and neither does one an earlier call of the execution planted. The
three names are reserved from a caller's extra environment. Cell:
`adapters/tool-host/tests/execution_workspace.rs::no_repository_hook_runs_on_commit_or_landing`.

**The landing never dereferences.** `git update-ref --no-deref` moves
`refs/graphhelm/executions/<id>` itself; a repository that carries that name as a symbolic ref
aimed at a branch gets the symref replaced by a direct ref at the landed commit, and the branch
does not move. Cell: `…::a_symbolic_execution_ref_is_replaced_and_the_branch_never_moves`.

**A server that dies mid-drive leaves a tree behind; the next drive reclaims it.** The
workspace is released when the drive ends; a server killed before that leaves
`<staging>/ghtool-exec-<digest>` and its registration in the project's `.git/worktrees/`. The
next Tier 1 call of that execution finds the stale tree at its own root (under the staging
directory, so it is ours), removes it (`git worktree remove --force`, then `remove_dir_all`,
then `git worktree prune`), and provisions afresh from the execution's ref — exactly as if
nothing had been left. The call's record carries `recoveredWorkspace: true` so the audit trail
says it happened. Cell: `…::a_stale_tree_from_a_dead_server_is_reclaimed_on_the_next_drive`.

**The tree is held for a call, not between calls.** The cancellation span that makes an
immediate pause wait for a teardown in flight is taken for the duration of each Tier 1 call and
released when it returns, so a pause between two calls of an execution returns at once. A
commit whose object id cannot be read back is a landing failure (host-error disposition), never
a "completed" commit node that published nothing; the ref probe that decides where the next
tree starts runs through the same funnel as every spawn (deadline, output caps, cancel signal).
A registration git kept for a directory that is gone (a provision cancelled or killed midway)
is reclaimed the same way a stale tree is.

**Landing is a compare-and-swap.** `update-ref` is given the commit the ref held when the
execution's tree was provisioned (empty when the ref did not exist), so a ref moved by anyone
else meanwhile refuses rather than being overwritten; the refused call records the commit it
made (`commit`), no `landedRef`, and a host-error disposition.

## The commit lands as a ref

A `commit` tool node that completes runs, in the **project**:

```text
git update-ref refs/graphhelm/executions/<execution id> <commit>
```

A plain ref, never a branch under `refs/heads`, never the operator's checkout: `HEAD` does not
move, the working tree is untouched, no branch appears. The operator merges when they choose:

```sh
git -C <project> log refs/graphhelm/executions/<execution id>
git -C <project> merge refs/graphhelm/executions/<execution id>
```

The `ToolCallRecord` of the commit call carries two content-free fields, sealed as evidence
beside the node outcome and readable through `GET /v1/executions/{id}/evidence/{evidenceId}`:

| field | value |
|---|---|
| `commit` | the full object id the call produced |
| `landedRef` | the ref it moved (`Some` only inside an execution) |
| `recoveredWorkspace` | `true` when a stale tree was reclaimed before the call ran (absent otherwise) |

An execution id that is not a lowercase `[a-z0-9._-]` segment git accepts (`..`, a leading
`.`, `~`, `:`, `?`, `*`, `[`, or ANY uppercase byte — loose refs are files, and `Build-1` would
alias `build-1` on a case-insensitive filesystem) lands under
`refs/graphhelm/executions/sha256-<first 32 hex of the id's sha256>`; so does any id that itself
begins with `sha256-`, so a literal id spelled like a digest ref can never share a ref with the
id it is the digest of. The record names the ref that was actually written either way. The
derivation is `graphhelm_tool_broker::record::execution_ref`.

**Scratch never lands.** Every spawn through the tool host gets its `HOME`/`USERPROFILE`,
`TEMP`/`TMP` and `CBM_CACHE_DIR` redirected to `.home`, `.tmp` and `.cbm-cache` INSIDE the
workspace root — inside an execution, beside the tracked tree. `Commit` stages with
`git add -A -- . ':(exclude,top).home' ':(exclude,top).tmp' ':(exclude,top).cbm-cache'`, so a
shell or tests call's droppings (a `.cargo/credentials` under HOME, temp files, the index cache)
never enter the execution's ref. Cell: `adapters/tool-host/tests/execution_workspace.rs`
`scratch_written_by_a_shell_call_never_lands_in_the_ref`.

## Proof

- `apps/cli/tests/runtime_http.rs` `a_useful_change_lands_with_tools_and_no_model_credential`:
  a temp project whose "test" (`git grep FIXED`) fails, three tool nodes, `serve` with the tool
  half only, `start` over HTTP → `completed`; the ref resolves to a commit whose tree carries the
  fix and whose parent is where the project started; the test passes in a worktree of that ref;
  `HEAD` unchanged; no branch; staging empty; the sealed tests stdout is the runner's real
  output; the sealed commit record names the ref; the stream replays byte-identically twice.
- `apps/cli/tests/runtime_http.rs` `a_half_given_tool_half_is_refused_at_startup`.
- `adapters/tool-host/tests/execution_workspace.rs`: the same contract at the host layer, plus
  resumption from the ref, two executions in two trees, the per-call door unchanged,
  `keep_workspace`, a failed commit landing nothing, and the digest ref for a hostile id.
- The hand-run record: `docs/acceptance/useful-change-2026-09-13.md`.

## Out of scope

Merging the ref into the operator's branch (the operator does it); deploy (#114); Tier 2/3
isolation; the briefing's `workDone` naming the ref (stacked on #1063, declared in the PR).
