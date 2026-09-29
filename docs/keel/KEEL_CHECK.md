# `graphhelm keel check`: a diff against its card

`graphhelm keel check` is the command that runs Keel's scope counter on a real change (#1330). The
counter itself is `core/policy/src/keel.rs` (`classify_write`, `check_card`, and `check`, which
joins them with the scope rule). The rules and their version are in
`extensions/builtin/graphhelm-development-contracts/policies/keel.yaml` (version `1.2.0`).

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
| 3 | Input error `GHCLI030_KEEL_CHECK_INPUT`: the range is not a range, git failed, or the card is not a card. |

## Limits

The counts come from a line grammar over the diff, not a parser; the limits are the ones
`keel.rs` and `keel.yaml` name. The scope rule reads paths from the diff headers. It does not
decode git's quoted form for unusual file names, so such a path is a warning to check by hand.
The pathogen suite `tools/pathogens/src/keel_scope.rs` holds two specimens the check must refuse:
an edit to a file the card does not list, and an extra test file added outside the card.
