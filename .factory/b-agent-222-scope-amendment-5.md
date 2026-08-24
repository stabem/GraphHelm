# Scope amendment 5 for #222 — written by B, published in the issue body

**State: PUBLISHED.** Written here first because `gh issue edit` was refused by the authoring
session's permission policy after amendment 4 had already gone through, and the refusal was not
worked around. The orchestrator published it to #222's body from this file's blob; verified at the
destination afterwards rather than taken on report: the body holds all five amendments and names
both files.

The earlier version of this file said UNPUBLISHED, in its name and in this header. That was true
when written and stopped being true the moment the amendment landed — a name that asserts a state
is a claim that goes stale silently, because nobody re-reads a filename.

---

---

## Scope amendment 5 (B, 2026-08-24) — the citation cases and what walks them

Two files enter this task's scope. Both are **NEW files** — nothing existing is edited, so there is zero conflict with the other six lanes writing into this package:

- `extensions/builtin/graphhelm-development-contracts/schemas/context-citation-case.schema.json`
- `apps/cli/tests/context_citation_fixtures.rs`

`fixtures/context/**/*.json` was already in the strict file list; what was missing is the shape that closes those files and the code that runs them.

**Why they are required rather than convenient.** Gate 3 of this task's review criteria requires the citation-spoofing threat — named in this issue's own threat assessment — to exist as a **fixture in the shared package**, not only as a Rust test, and it names two shapes: an ID that is not in the capsule, and an ID belonging to a **different** capsule. The second is the scope-bleed version, and is the one a naive "does this ID parse" check passes. A fixture directory with nothing reading it is inert data; the walker is the half that makes the case a guard.

**Why the schema is not optional.** The cases declare capsule **items**, never item IDs, because identity is derived from content: a fixture carrying a hard-coded `item-…` string would assert against a number nothing computed and would keep passing after the derivation changed, certifying its own stale copy of the answer. The walker therefore derives every ID through the same `item_id` production callers use — which means a malformed case would reach the verifier as garbage rather than as a refusal. The schema closes the shape (`additionalProperties: false` throughout, sections constrained to the capsule schema's six, expected codes constrained to the two allocated in amendment 4) so a broken case is refused as broken instead of running.

**Measured, not asserted.** The walker was born green, so it was sabotaged in four arms — the cases are runtime data, so no rebuild sits between them:

| arm | perturbation | result |
|---|---|---|
| 0 | untouched — the control the others must differ from | `2 passed` |
| 1 | the scope-bleed case declares one of its two codes | red, naming both sides |
| 2 | the cited item is moved **into** the capsule under test | red — the verifier produced `[]` |
| 3 | the case directory is absent | red, both tests, on the sweep |

Arm 2 is the one that matters. The two capsules hold an item with the **same section and the same text**, so moving the citation across makes the verifier accept — which is the proof that the case's red comes from cross-capsule identity and not from the text differing. Arm 3 exists because an empty sweep looks exactly like a sweep whose every case succeeded.

A second guard refuses any case declaring an empty expectation: three cases asserting nothing would give three green rows standing for no coverage, and a count of cases cannot see that.

**Also recorded, outside the code scope.** `.factory/b-agent-222-section-authority.md` carries this lane's answer to J's `DECLARED_SECTION_ORDER` finding. It is a record rather than a change to the product, committed with the change it describes because the direct channel was down when it was written and a record that travels with the artifact outlives the channel that would have carried it.
