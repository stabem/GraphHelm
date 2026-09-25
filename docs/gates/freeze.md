# The gate freeze

**The rule (M06 binding decision 5):** a change touching gate machinery together with anything
outside it is a hard violation — the judge and the judged never move in one pull request. It ships
as a pure check, `freeze_violation`, in `core/quality/src/lib.rs`.

`Cargo.lock` has one narrow neutral exception. A gate-only dependency change may update the
lockfile when the exact base and current lockfiles are both parseable, the package set and every
non-gate package record remain structurally equivalent, and each changed dependency edge in an
existing gate package is explained by a same-diff gate `Cargo.toml`. A changed version, checksum,
source, top-level lock metadata, package set, malformed input, unrelated dependency edge, or
concurrent non-gate path remains a freeze violation. Callers without both lockfile snapshots keep
the original path-only refusal.

## Where the frozen set lives — and why it is not restated here

The authoritative list is the `GATE_MACHINERY` constant beside `freeze_violation`. This charter
deliberately does not copy it. The one document that restated the list —
`docs/milestones/quality.md` — drifted within weeks: it still described three prefixes while the
constant held four (`tools/source-invariants/` joined in #361 and no prose noticed). A
hand-restated list is a second producer of one set, and the copy is the side nothing checks.

The reason each prefix is frozen lives beside its own named assertion in
`core/quality/tests/freeze.rs` (#402), where removing an entry means deleting an assertion with
its reason attached — not a comma in a list.

## Why this file exists, and why it lives here

Before this file, `docs/gates/` was an empty prefix (#282): one quarter of the frozen surface
pointed at a directory that did not exist, the guard exercising the freeze could not tell that
entry from a typo, and the assertion protecting it justified it with contents — "the gate stamps
a stream is certified by" — that never existed anywhere. Certification stamps are `GateCertified`
events in the event stream; nothing ever wrote a stamp file under `docs/gates/`.

Placing the charter here fixes both halves at once:

- **the prefix stops being empty**, so the freeze's `docs/` entry finally guards something; and
- **the rule's own text comes under the rule.** Until now the freeze charter sat in
  `docs/milestones/quality.md`, outside the freeze it defines — a pull request could rewrite what
  the freeze means in the same breath as the code the freeze judges. This file cannot be edited
  that way: it is gate machinery, and the freeze it documents refuses exactly that diff.

## What belongs under `docs/gates/`

Normative gate documentation: this charter, and any future document that defines what a gate
refuses. Anything here moves only in gate-only pull requests. Descriptive prose about milestones,
history, or usage belongs outside, where it can evolve alongside the code it describes.
