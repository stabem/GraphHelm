# `graphhelm keel check`: a diff against its card

`graphhelm keel check` is the command that runs Keel's scope counter on a real change (#1330). The
counter itself is `core/policy/src/keel.rs` (`classify_write`, `check_card`, and `check`, which
joins them with the scope rule). The rules and their version are in
`extensions/builtin/graphhelm-development-contracts/policies/keel.yaml` (version `1.3.0`).

```sh
graphhelm --json keel check --diff origin/main..HEAD --card card.json [--repo <dir>]
```

- `--diff` is a git range. The command runs `git diff` on it in `--repo` (default: the current
  directory), with fixed `a/` and `b/` prefixes and rename detection.
- `--card` is optional. Without it, the scope rule does not run and only the surface counts are
  reported.

## The card

The card is JSON, with the shape `schemas/keel-card.schema.json` in the same package. It is the
card of [`docs/process/DELIVERY.md`](../process/DELIVERY.md) §2 written down:

```json
{
  "promise": "graphhelm keel check refuses a diff that touches a path outside its card",
  "scopePaths": ["core/policy/src/keel.rs", "apps/cli/src/commands/keel.rs", "apps/cli/tests"],
  "proof": "cargo test -p graphhelm-cli --test keel_check",
  "exportedSymbols": ["check", "Card"],
  "allowance": { "newModule": 1, "newType": 2, "newPublicFn": 2, "newDependency": 0, "newTest": 4 }
}
```

`promise`, `scopePaths` and `proof` are required. A scope path names a file or a directory; paths,
not globs. `exportedSymbols` lists the public symbols the change adds. `allowance` is extra surface
declared at planning time: it is added to `keel.yaml` `surface` and capped by `maxAllowance`.

## What it checks

| Rule | Blocks? |
|---|---|
| `keel.scope.path_outside_card`: a changed path (added, edited, deleted or renamed, either side) is not a scope path and not under one | Yes, when a card was given and the path was read plainly. A quoted or escaped path that the checker does not decode is a warning. |
| `keel.card.scope_empty`, `keel.card.scope_not_a_path` | Yes |
| `keel.card.scope_too_wide`, `too_many_symbols`, `too_large` | No (warning) |
| `keel.diff.unparseable` | Yes |
| `keel.surface.<kind>_over_budget`, `keel.body.oversized_change` | No while `surfaceEnforcement: signal` (the shipped value) |

## Output and exit codes

The reply is the standard CLI envelope. `data` holds the whole report: `policyVersion`,
`cardDeclared`, `changedPaths`, `surface`, `classification` (every charge, with path, line and
symbol), `findings` and `refused`. Each finding is also a diagnostic whose `code` is the rule id:
`error` when it blocks, `warning` when it is a signal.

`surface` puts the counts beside the card: `changedFiles`, `newFiles`, `newTestFiles`, `newTests`,
`newModules`, `newPublicSymbols`, `newDependencies`, `cardScopePaths`, `cardExportedSymbols`, and
`undeclaredPublicSymbols` (public symbols the diff adds that the card does not name; reported,
never refused).

| Exit | Meaning |
|---|---|
| 0 | Nothing blocks. Warnings may be present. |
| 2 | A finding blocks. `ok` is false and the report is still in `data`. |
| 3 | Input error `GHCLI031_KEEL_CHECK_INPUT`: the range is not a range, git failed, or the card is not a card. |

## Proving new tests: `--prove-new-tests`

Keel Law 3 says a test is born against a named defect. `--prove-new-tests` checks that for every new
Rust `#[test]` the counter charges (#1333):

```sh
graphhelm --json keel check --diff origin/main..HEAD --card card.json --prove-new-tests   [--prove-target-dir <dir>] [--prove-timeout-secs 900]
```

1. The base and the head are checked out as two detached worktrees under a fresh directory in the
   system temporary directory. The user's checkout is never written. Both worktrees and that
   directory are removed, by exact path, when the command ends.
2. The head's test code is put into the base tree: a changed file under a test path (`tests/`,
   `*_test.rs`, ...) is copied whole; a test inside a production file (an inline `mod tests`) is
   grafted alone, with its module's `use` lines, into a `#[cfg(test)] mod keel_prove_graft` appended
   to the base's copy of that file.
3. Each test runs once per side: `cargo test -p <package> <--lib|--bins|--test <name>> -- <test name>`,
   bounded by `--prove-timeout-secs` (the build included; the process tree is killed on timeout).
   Every run shares one target directory (`--prove-target-dir`, else `CARGO_TARGET_DIR`, else
   `graphhelm-keel-prove-target` in the temporary directory), with a `parent` and a `head`
   subdirectory: each crate builds once per side, and cargo cannot take one checkout's build as
   fresh for the other.

Each test is reported in `data.testProof.proofs` with `name`, `path`, `line`, `parent` and `head`
(`outcome` and the `detail` line that shows it) and `verdict`:

| Parent | Head | Verdict | Signal |
|---|---|---|---|
| failed | passed | `earned` | none |
| did not compile | passed | `unproven` | `keel.test.unproven`; the compiler line is in `parent.detail` |
| passed | passed | `green_on_parent` | `keel.test.green_on_parent` |
| any | failed or did not compile | `red_on_head` | `keel.test.red_on_head` |
| timed out, not found, ignored, not run | | `unproven` | `keel.test.unproven` |

All three signals are warnings; none changes the exit code. A parent compilation failure alone
cannot establish that the production subject is new: an inline graft can omit a sibling test
helper. The reviewer reads `parent.detail` and treats that experiment as unproven. A TypeScript or Python test the diff adds is listed as `unproven`
("no runner for this language yet"); only Rust is run today. A setup failure (a revision that does
not resolve, a worktree that cannot be added) is input error `GHCLI031_KEEL_CHECK_INPUT`, exit 3.

Cleanup is by exact path. The two worktrees live in `keel-prove-<pid>-<nanos>/parent` and `/head`
under the temporary directory; on every exit the prover runs `git worktree remove --force` on each
of those two paths (also when an add failed halfway) and deletes the directory. It never runs
`git worktree prune`. A run killed before it can clean up (Ctrl-C, a killed terminal) leaves its
records; the next run removes only stale `parent` and `head` records under its own scratch root,
again one exact path at a time, and leaves any record whose directory still exists, since another
run may own it. If manual recovery is necessary, use `git worktree list --porcelain`, confirm that
each missing directory belongs to your interrupted `keel-prove-*` run, and remove only that exact
path with `git worktree remove --force <path>`. Never run `git worktree prune`: it removes every
prunable record, including worktrees owned by other sessions.

The pathogen suite `tools/pathogens/src/keel_prove.rs` runs the prover for real on a two-commit
crate: a tautological test and a test that asserts its own mock are refused as
`keel.test.green_on_parent`, and a real regression test beside the same fix passes.

## Limits

The counts come from a line grammar over the diff, not a parser; the limits are the ones
`keel.rs` and `keel.yaml` name. The scope rule reads paths from the diff headers. It does not
decode git's quoted form for unusual file names, so such a path is a warning to check by hand.
The test prover finds a test's name on the first added `fn` line after its `#[test]`, its crate by
the nearest `Cargo.toml` with a `[package]`, and grafts an inline test by counting braces; a test
it cannot place is `unproven` with the reason, never silently dropped.
The pathogen suite `tools/pathogens/src/keel_scope.rs` holds two specimens the check must refuse:
an edit to a file the card does not list, and an extra test file added outside the card.

## The Stop gate

The `graphhelm` plugin runs `plugins/graphhelm/hooks/keel_stop_hook.py` on the host's `Stop` event.
When the branch changes more than five lines outside docs since its merge base with `main`, the
turn may not end until a card exists at `.graphhelm/keel-card.json` (ignored by git); when the
`graphhelm` CLI is on `PATH`, that card must also pass `keel check` on `<merge-base>..HEAD`. The
hook blocks at most once in a row (`stop_hook_active`) and lets the turn end when it cannot run.

## The edit gate

Before the Stop gate, `plugins/graphhelm/hooks/keel_pretool_hook.py` runs on `PreToolUse` for
`Edit`, `Write` and `MultiEdit`. When the edit targets a file outside docs and the branch's code
change plus the lines the edit writes pass five, the edit is denied until
`.graphhelm/keel-card.json` exists. Writing the card is always allowed; a hook failure allows the edit.

Inside a GraphHelm execution (`GRAPHHELM_EXECUTION_ID` bound), each refusal by the Stop or
PreToolUse lock is also recorded as a `keel.blocked` signal on the bound node
(`plugins/graphhelm/hooks/keel_record.py`), so the Studio's Keel box shows that a step was stopped.
Recording is best effort and never changes the lock's decision.
