# Quickstart — a real run, start to answer

> For the Studio and a chat harness — `graphhelm init`, `serve`, the Studio, `.mcp.json` — see
> [docs/install/GETTING_STARTED.md](docs/install/GETTING_STARTED.md). This page stays in-process and
> needs no daemon.

Every command and every output below was executed against this repository. Nothing here is
illustrative.

The run below needs no provider at all; the full promise — run, monitor, Studio, export, with no
credentials — is declared in [docs/product/PROVIDER_LESS_MODE.md](docs/product/PROVIDER_LESS_MODE.md).

This exists because a newcomer with no repo access was handed the README and the tool's own
`--help`, given one goal — *start a run and find out whether anything needs you* — and stopped
dead at `execution start`. Not on a flag name: **nothing told them what a node does when it
runs, or what it needs before it will run at all.** So that is what this page answers first.

## What you need, and what you do NOT

| | |
|---|---|
| Rust `1.97.1` | pinned; other versions are untested |
| A clone of this repository | `git clone https://github.com/stabem/GraphHelm.git` |
| `jq` | only for the scripting section at the end |
| **No model provider** | the run below uses fixtures — no API key, no account, no network |
| **No credentials, no daemon, no database** | the store is a directory this command creates |
| **No `serve`** | `execution start` drives in-process; `serve` is a separate surface |

The store is **whatever path you pass to `--events`**. There is no global state, no init step,
and nothing outside that directory. Delete the directory and the run is gone.

## The run

```bash
mkdir -p /tmp/qs
echo '{"nodeOutcomes":{"implementation":"failure"}}' > /tmp/qs/fixtures.json

cargo run --locked -p graphhelm-cli -- execution start \
  --file examples/graphs/manual-override-deploy.yaml \
  --events /tmp/qs/events \
  --fixtures /tmp/qs/fixtures.json \
  --mode supervised \
  --execution demo --pretty
```

`--fixtures` is what makes this offline: it supplies each node's outcome instead of calling a
model. `supervised` means the run parks and waits for a human instead of driving itself.

Real output, trimmed to the part that matters:

```json
{
  "ok": true,
  "command": "execution.start",
  "data": {
    "attention": "needs_you",
    "attentionReasons": [
      { "kind": "blocked_node", "node": "implementation" }
    ],
    "executionId": "demo",
    "startedAt": "2026-08-19T02:00:01.489+00:00",
    "lastEventAt": "2026-08-19T02:00:01.921+00:00",
    "nodeStateCounts": { "blocked": 1, "ready": 1, "...": 0 }
  }
}
```

## The answer to the question

**`attention` is the field.** It has exactly four values:

| value | meaning |
|---|---|
| `needs_you` | something named is waiting on a human — `attentionReasons` says what and which node |
| `can_sleep` | nothing is waiting, and everything in flight was actually CHECKED |
| `unknown` | work is in flight whose silence could not be judged — not an alarm, and not an all-clear |
| `calmed_by_amendment` | calm, and the calm is one YOU bought: a budget you declared is what makes the quiet legitimate |

`unknown` is a real answer, not a failure. It appears when a node is running and nobody
declared how long it may stay quiet, and it carries the remedy: which node, which declaration
is missing, and the operation that supplies it.

`calmed_by_amendment` is what that remedy produces. Apply the declaration `unknown` asks for, and
the next answer is calm — but calm resting on your number rather than on one the graph declared, so
it says which nodes and stays distinguishable from a plain `can_sleep`. **Anything scripting this
field must handle it**: it is the value on the far side of the documented fix.

Ask again at any time, against the same directory:

```bash
cargo run --locked -p graphhelm-cli -- execution status --events /tmp/qs/events --execution demo
```

## Scripting it: there is no exit-code convention

**`execution status` exits `0` whatever the answer.** Measured: it returns `0` while reporting
`attention: "needs_you"`. So in a cron job, a shell prompt, or a `&&` chain, read the field:

```bash
cargo run --locked -q -p graphhelm-cli --   execution status --events /tmp/qs/events --execution demo | jq -r .data.attention
```

**Give that value a default branch.** The four above are what this version emits; the set is
allowed to grow, and a `case` with no fallback treats a new answer as whichever branch it falls
through to. Treat anything you do not recognise as **not** an all-clear — that is the safe
direction, and it is the whole reason this field is not a boolean.

Nothing above puts a `graphhelm` binary on your PATH, and a cron job should not run `cargo`.
After a build, the binary is at `target/debug/graphhelm` (`.exe` on Windows) — use that path, or
install it:

```bash
cargo install --locked --path apps/cli
```

Stated precisely, because the first draft of this page overclaimed and a reviewer caught it:
the answer IS machine-readable — `attention` is a stable enum and it is the same answer a
human reads. What does not exist is a CONVENTION mapping it onto exit codes, and both designs
are defensible: `0` meaning "the command worked, the state is in the payload" is what most
query CLIs do, while a non-zero "someone is needed" is what a watchdog wants without parsing.

The gap is a missing convention, not a missing answer.

## What this does not show

A run that calls a real model, or a tool node touching a real repository. Both exist and both
need a gateway manifest and credentials. Neither is covered here, because a quickstart that
requires an account is not a quickstart — and because everything above is verifiable by anyone
with the clone and no accounts at all.
