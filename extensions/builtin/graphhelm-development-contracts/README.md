# GraphHelm development contracts

An Extension package of **data only**. It ships schemas, policies, fixtures, and entry skills.
It ships no binary, no provider, and no code that runs.

## Entry skills

The inventory is declared in [extension.json](extension.json). Entry skills have `SKILL.md` paths;
[Keel's reference](skills/keel/REFERENCE.md) is supporting material, not another entry skill.

| skill | what it produces | what it may never do |
|---|---|---|
| [`task-plan`](skills/task-plan/SKILL.md) | the task's `keel.plan`: paths, promise, proof kind, review count, skills | write code |
| [`implement`](skills/implement/SKILL.md) | the change inside the card, with cited context and the reached tests run | add surface the promise does not need |
| [`blind-review`](skills/blind-review/SKILL.md) | one blind verdict on a pinned head, with commands and results | review a change it helped write |
| [`merge`](skills/merge/SKILL.md) | a pinned squash merge, read back, and a released workspace | merge a head the approval did not name |
| [`memory-curator`](skills/memory-curator/SKILL.md) | advisory candidates for durable lessons | record anything directly |
| [`keel`](skills/keel/SKILL.md) | a scoped contract card, measured write surface, and named proof | enforce its own rules or activate the penalty ladder |
| [`test-audit`](skills/test-audit/SKILL.md) | an authoring gate, suite audit, or deletion record naming the covering observer | enforce rules or remove a test without a covering-observer record |

For the left-to-right overview of both built-in skill packages, see the
[skills README](../../../docs/skills/README.md).

## The authority this package holds, and why it is small

The grant in `extension.json` is the whole story:

```
filesystem  package: read          workspaceArtifacts: proposal-write
network     external: false        loopbackRuntimeApi: true
runtime     read: true             mutations: []
secrets     artifactValues: false  tokenFile: false
```

Read that as: these skills can look at the Runtime through public read surfaces, propose an artifact
for someone else to accept, and do nothing else. No mutation, no direct write, no network beyond the
local Runtime API, no access to secret values.

**A skill that could enforce its own contract, publish its own memory, or mutate the Runtime would
be a second authority whose rules live in prose.** Prose cannot be validated, versioned, or refused,
which is why the boundary is in the manifest rather than in a paragraph asking nicely. A
contribution that requests more than the grant is refused at package validation, before any host
sees it.

## Host views

`.claude-plugin/plugin.json`, `.codex-plugin/plugin.json` and `.mcp.json` are **hand-written and
deletable**. They exist so a host can discover the package; they carry no authority of their own,
and deleting them removes discovery and changes nothing else.

**Hand-written, not derived, and the distinction is not cosmetic.** Raised on review: this section
originally said "derived", copied from the sibling package's language, and nothing in this
repository generates these files. Claiming derivation would have described a process that does not
exist, and the reader who believed it would expect regeneration to fix any drift.

What follows from them being written by hand is the part worth stating. Each one is **another
producer of this package's identity** — the name and version appear in three files that no
mechanism keeps in step. The only thing holding them together is the validator's cross-match, which
is why that cross-match is tested one cell per compared fact rather than once: five identity
comparisons across the two plugin manifests, plus the closed shape of the MCP registration. A suite
that changed one field and called the property covered would stay green through a regression that
stopped comparing any of the other four.

The MCP registration points at the local Runtime through the public CLI, with the token supplied by
file path rather than value — the package never sees the secret.

## Guarantees you can check rather than trust

Run the package validator:

```
cargo run -p graphhelm-cli -- extension validate extensions/builtin/graphhelm-development-contracts
```

And the hostile-package guards, which build broken copies of this package in a temporary directory
and assert that each is refused **under its own diagnostic code** — refused for a neighbouring
reason would not be coverage:

```
cargo test -p graphhelm-cli --test development_plugin
```

## Known gap, stated rather than left to be discovered

`/spec/contracts/publication` declares `governor-only`, and **nothing reads that field** — measured,
and filed as issue #285. The adjacent authority *is* enforced: a contribution that writes artifacts
instead of proposing them exceeds the grant and is refused. Those are different claims, and the
second does not cover the first: one is about what a contribution may do, the other about what the
package says it is.

## Activation

Activation is `explicit` and depends on #212. Nothing here activates the bundle, and the test that
would prove activation behaviour is present as a **declared-pending case** naming what it will
assert — not as a skipped test, because a skipped test and a passing one are the same green row in a
summary.
