# B — #224 blueprint (task-008: three data-only entry skills and native bundle wiring)

Branch `issue-224-data-only-entry-skills`, off `0754cda`. Short by intent; the reasoning that
needs to survive is the part that constrains the code.

## What was measured before deciding anything

**The six hostile shapes the issue names all have real emitters.** They were not assumed to exist:
`core/schema/src/extension.rs` declares twenty diagnostic codes, and each hostile fixture maps to
one that already fires.

| hostile fixture | the code that must fire |
|---|---|
| authority smuggling | `GHEX018_AUTHORITY_ESCALATION` — contribution effect or permission exceeding the package grant |
| undeclared tool | `GHEX018_AUTHORITY_ESCALATION` — an effect whose required permission is not declared |
| host mismatch | `GHEX013_HOST` |
| bad digest | `GHEX005_DIGEST` |
| self-publication | `GHEX018_AUTHORITY_ESCALATION` — corrected, see below |
| private import | `GHEX020_EXTENSION_REF` |

**Correction to this table, found by applying its own rule to itself.** The first version mapped
self-publication to `GHEX019_ARTIFACT_FLOW`. That was wrong, and checking it is the only reason it
is not now a fixture that fails for a neighbouring reason.

`GHEX019` governs the artifact-flow *format* and its entry families. Publication authority is
declared elsewhere: `"publication": "governor-only"` in `/spec/contracts`. Measured across `core/`
with the JSON keys the validator actually indexes by, and with two keys it demonstrably does read
as positive controls in the same run:

```
artifactFlowFormat        1     <- control: read by the validator
entryFamilies             1     <- control
formatVersion            22     <- control
publication               0
activation                0
composition               0
missingCapabilityResult   0
hostViews                 0
```

**Five declared contract fields have no reader anywhere in `core/`, `publication` among them.** A
hostile package declaring `"publication": "self"` validates clean today, so that fixture has no
emitter and cannot go red for the right reason.

What *is* enforced is the same intent one layer down, at the effects vocabulary: `artifact.local.write`
requires the `workspace.artifact.write` permission, and this package's grant carries
`workspaceArtifacts: "proposal-write"` and nothing more. So a skill that publishes instead of
proposing is refused under `GHEX018_AUTHORITY_ESCALATION` — a real emitter, semantically the same
threat. The fixture is built there.

The unenforced `publication` field is recorded as a **declared gap**, not quietly satisfied by the
effect-level fixture. They are different claims: one is about what a contribution may do, the other
about what the package says it is, and only the first is checked.

This matters because a fixture whose refusal has no emitter cannot go red for the right reason. It
would still fail — on some neighbouring rule — and the test would read as coverage of a threat
nobody actually guards.

**Skills are validated as documents, not as prose.** `validate_skill` requires UTF-8, an opening
`---` line, and a closing delimiter, then parses the frontmatter under bounded YAML limits. So
frontmatter is a machine contract and the body is not: anything the body claims about authority is
unchecked by construction, which is exactly why the threat assessment points the review at prose.

**The host trio already has a reference implementation.** `extensions/builtin/graphhelm-jpd`
carries `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json` and `.mcp.json` in the shape the
validator accepts, with the MCP server addressed through `${GRAPHHELM_CLI}` and a token file rather
than embedded credentials. The conventions are copied from that artifact, not invented.

## The one design decision worth arguing

**The hostile fixtures live in a `TempDir` inside `development_plugin.rs`, not in the package.**

The strict file list has nine entries and none of them is a fixtures directory, so a hostile
package on disk would need a scope amendment. It would also be worse: a deliberately broken
package sitting inside the real one is a package the inventory guard must then be taught to
ignore, and a guard with an exception list is a guard with a hole shaped like its exception.
`apps/cli/tests/extension_cli.rs` already builds packages in a `TempDir`; this follows it.

Each hostile case is built by mutating **one** field of a known-good copy, so its refusal is
attributable to that field. A fixture that breaks two things is satisfied by a validator that
catches either.

## Order of work, red first

1. `development_plugin.rs` with the six hostile cases and a positive control — the known-good
   bundle must validate, or every negative is satisfied by a validator that refuses everything.
2. Observe each red landing on its own code, not on a neighbour's.
3. The smallest data-only bundle that turns them green: three `SKILL.md`, the host trio, `README.md`.
4. Manifest entries and the inventory in the **same commit** — `development_package_inventory`
   goes red the moment files are added and not declared, which is the guard working.
5. Deduplicate prose last, without touching a machine contract.

## What this task will NOT establish, declared now

**Activation depends on #212, which is open.** Nothing here activates the bundle. The test that
would need activation is written as a **named, declared-pending case** that states what it will
assert and which issue unblocks it — never a silent `#[ignore]`, because a skipped test and a
passing one are the same green row in a summary.

## Shared-file rule, declared at birth rather than at collision

`extension.json` is shared with #225 (D, evaluator token-efficiency plus its own schema). The rule
agreed before either of us touches it: **union by publication order** — whoever lands first keeps
the prefix, the second appends — `--check` from `.factory/bin/union-shared-manifest.py` run on both
sides, and J's membership check before any button that moves the manifest. The invariant is the
empty difference, never a count: two lanes can land between one merge and the next, and chasing an
expected number gets the right total by the wrong composition.
