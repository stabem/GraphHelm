# Manifest ownership: how a reviewer tells an append from an edit

Seven of the ten tasks in this milestone write files into
`extensions/builtin/graphhelm-development-contracts/`, and only two own the manifest. Every file
must be declared or `extension validate` refuses the package, so five lanes must add entries to a
file that is not theirs.

**The bar:** a reviewer distinguishes a legitimate append from an edit to someone else's entry **by
reading the diff, without context.**

## The obvious mechanism is impossible, and the impossibility chose a better one

An `owner` field per contribution was the first idea. Measured before proposing it:

```
core/schema/src/extension.rs:184  #[derive(Debug, Deserialize)]
core/schema/src/extension.rs:185  #[serde(deny_unknown_fields)]
core/schema/src/extension.rs:186  struct Contribution { id, kind, path, sha256, ... }
```

**`deny_unknown_fields`.** An `owner` field makes `extension validate` reject the manifest, and that
file is outside this task's scope. The obvious mechanism required widening exactly the format this
task promised not to widen — which is the mechanical test for "existing format" versus "the format
moved to fit".

## The rule: `contributions` is append-only

**The previous array must be a PREFIX of the new one.**

A legitimate append is `+` lines only, contiguous, at the end. Any `-` line, or a `+` interleaved
between existing entries, is an edit to another lane's entry. **The reviewer needs no knowledge of
who owned what — the position says it.** No new field, no format change, `extension validate` still
runs against an unchanged `schemas/extension.schema.json`.

Parallel landings do not break it: A branches from `[e1..e5]` and lands `[e1..e5,a6]`; B rebases and
produces `[e1..e5,a6,b6]`. The state B rebased onto is a prefix of B's result. **The rule holds
against the state you rebase onto, not the state you started from.**

## Removal is a separate, reviewed operation — and saying so is what keeps the rule usable

**Prefix-append forbids every removal, including the legitimate removal of your own entry.** A lane
retiring an obsolete fixture must drop its entry, which breaks the prefix; and there is no way
around it, because deleting the file while keeping the entry fails validation on the missing path.

Left unstated, the rule makes legitimate self-removal indistinguishable from vandalism, and the
pressure it creates is to **leave dead entries rather than clean them** — accumulating furniture in
the manifest to avoid violating a rule.

**So: prefix-append is the rule for the normal path. Removal breaks it deliberately and therefore
requires justification in the PR.** The bar still holds — a `-` inside `contributions` is never
routine, so a reviewer seeing one knows immediately that it needs reading, still without context.

## Three guards, three different questions, and none of them covers another

| guard | question it answers |
|---|---|
| prefix-append (review, by diff shape) | did you edit someone else's entry? |
| `GHEX012_INVENTORY` | did you lose your own entry? |
| `GHEX003_CONTRIBUTION` | did you add one that collides? |

**The second is the one this rule cannot see, and it matters.** Two lanes appending at the end of the
same array touch the same textual region, so git conflicts. A hasty `--ours`/`--theirs` **drops one
entry silently — and the result still satisfies prefix-append.** A lost entry produces a perfectly
valid prefix relation. What catches it is a different guard entirely: the dropped lane's files are
still in the tree, undeclared, and the inventory sweep refuses them.

**The third was my open question and is now measured** (credit to the consuming lane's author, who
measured it rather than leaving it):

```
core/schema/src/extension.rs:585  !ids.insert(...)                        -> GHEX003_CONTRIBUTION
core/schema/src/extension.rs:643  !paths.insert(...)
                              ||  !portable_paths.insert(lowercased)      -> GHEX003_CONTRIBUTION
```

Duplicate ids and duplicate paths are both refused — **and the lowercase comparison means two paths
differing only in case are refused too**, which is what protects a case-insensitive filesystem where
`Foo.json` and `foo.json` are one file.

**Write the three together.** Each answers a question the others do not, and separated they look
redundant — which is how one of them gets removed by someone who believes another already covers it.

## Verification

```
extension validate extensions/builtin/graphhelm-development-contracts
  ok: true, contributionCount: 1, diagnostics: []
```
