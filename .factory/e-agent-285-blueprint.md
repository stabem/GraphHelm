# Blueprint — #285: six declared, unread `spec.contracts` manifest fields

**Status: BLUEPRINT ONLY. No code, no fixtures, no manifest edits.** Threat assessment first, per
house process; per-field decisions named and reasoned individually, per the orchestrator's explicit
instruction not to collapse five (now six) fields into one verdict.

**Provenance, stated up front so it travels with the artifact, not just this document
(`handoff-package-provenance`):** the measurement this blueprint is built on — `publication`,
`activation`, `composition`, `missingCapabilityResult`, `hostViews` declared in every Extension
manifest's `spec.contracts` and read by nothing, with the three-control methodology and the
concrete exploit ("a package declaring `\"publication\": \"self\"` validates clean today") — is
**B's**, measured 2026-08-24 while implementing #224 (task-008), filed as issue #285. This blueprint
is E's (mine): the per-field enforce/defer/advisory decisions below, one correction to B's own
measurement (§1.1), and the sixth field it surfaces.

**Sources read:** issue `#285`; `core/schema/src/extension.rs`; `schemas/extension.schema.json`;
both shipped manifests (`extensions/builtin/graphhelm-development-contracts/extension.json`,
`extensions/builtin/graphhelm-jpd/extension.json`); `docs/agents/AGENTS_SKILLS_PLUGINS.md`;
`docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md`; `.factory/b-agent-224-pr-body.md`,
`.factory/b-agent-224-blueprint.md`; `.factory/e-agent-213-blueprint.md` (my own prior work, which
already named `missingCapabilityResult` as package-level and out of #213's scope); issue #212
(atomic extension installation/activation, open, unimplemented); issue #223 (public Runtime/CLI/MCP
contract, references host views).

---

## 1. What exists today, precisely

`schemas/extension.schema.json` declares `spec.contracts` as `{"type": "object",
"additionalProperties": true}` — no `required` list, no `properties` for anything inside it. The
schema layer does not know these six field names exist. Every enforcement that DOES happen on
anything inside `contracts` is ad hoc Rust code in `core/schema/src/extension.rs`, field by field:
`contributions[]` (the bulk of the file), `artifactFlowFormat` (line 1774), `entryFamilies` (line
1786). Nothing else in `contracts` is read anywhere in `core/`.

### 1.1 A correction to B's own measurement, found by re-verifying it (not assumed correct)

B's table used `formatVersion` (claimed 22 hits in `core/**/*.rs`) as one of three controls proving
the Rust-side instrument was alive. Re-run with a precise quoted-key search: `artifactFlowFormat`
(1 hit) and `entryFamilies` (1 hit) check out — genuine reads, confirmed at the line numbers above.
**`formatVersion` does not.** Zero hits for that string anywhere in `core/schema/src/extension.rs`,
in any form. The 22 hits are a different, same-named field in an unrelated schema —
`schema-evolution`'s migration-document format and the event-envelope format
(`{"formatVersion":1,"schema":"graph",...}`) — confirmed by reading every hit site. This does not
undermine B's core finding (the five named fields are genuinely unread; I get the same zero) but it
does mean the control itself was cross-domain contamination, and it surfaces a **sixth field with
the identical defect** that #285's own acceptance criteria never named. Folded into this blueprint's
scope rather than filed separately — same file, same mechanism, same class of decision.

### 1.2 The exact current values (read directly, not assumed from the issue's examples)

| field | development-contracts | jpd | agree? |
|---|---|---|---|
| `formatVersion` | `"1.0.0"` | `"1.0.0"` | yes |
| `activation` | `"explicit"` | `"explicit"` | yes |
| `publication` | `"governor-only"` | `"governor-only"` | yes |
| `composition` | `"atomic"` | `"adaptive"` | **no** |
| `missingCapabilityResult` | `"refuse"` | `"unresolved"` | **no** |
| `hostViews` | `[]` (array) | `"derived-and-deletable"` (**string**) | **no — type mismatch** |

The three fields that agree across both shipped packages are the three where I have the most
grounding to enforce a narrow, non-disruptive check. The three that disagree are exactly the three
where I found the weakest (or a genuine type-level) design intent — not a coincidence; see §2.

---

## 2. Per-field decision, reasoned individually (per the orchestrator's instruction: no collapse)

**Calibration applied here, per the orchestrator's reinforcement of B's own scope statement:**
"unread" (measured, true of all six) is not the same claim as "needs enforcement" (analyzed,
proven true of exactly one). Confusing them would launder a measurement into a conclusion it never
earned — the same shape as trusting a paraphrase over the source text, aimed at my own first draft
of this blueprint instead of someone else's. Every field below is marked with which bar it clears:

- **CONSEQUENCE-ANALYZED**: a concrete abuse or a concrete, self-evident structural defect exists
  *in the manifests as they stand today*, independent of what the field is eventually for.
- **MEASURED ONLY**: the field is unread; no consequence has been demonstrated, only imagined by
  analogy. Enforcing here would be inventing significance the measurement never proved — advisory
  or deletion are the honest defaults, not a weaker fallback.

### `publication` — **ENFORCE** (CONSEQUENCE-ANALYZED — B's own analysis)

**Grounding:** `docs/agents/AGENTS_SKILLS_PLUGINS.md:281` and
`docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md:27` state the rule in prose: only the Graph Governor
may publish. B traced the attack, not just the absence: `"publication": "self"` validates clean
today, a package actively misrepresenting what it is under a manifest key that exists specifically
to make that claim. This is the one field where "measured unread" and "needs a fix" are the same
finding, because the abuse was demonstrated, not inferred.

**Decision:** refuse any value other than `"governor-only"` under a new diagnostic code. Narrow and
minimal — it does not build the governor-publication INTEGRATION (nothing today makes the
declaration true or false at runtime; B's own framing: "what a contribution may DO" vs "what the
package SAYS IT IS"). It closes exactly the demonstrated gap: a reader can no longer be shown a
false claim that passed validation. Both shipped manifests already declare `"governor-only"` — zero
manifest edits required.

### `hostViews` — **ENFORCE SHAPE ONLY** (CONSEQUENCE-ANALYZED — a structural defect exists today, not by analogy)

**Grounding:** `docs/superpowers/plans/.../plan.md:401` and the development-contracts `README.md`
describe host views as the derived, deletable per-host identity files (`plugin.json`, `.mcp.json`).
Issue #223 owns what the field eventually MEANS. But independent of that: the two shipped manifests
disagree not just on value but on **type** — `development-contracts` declares `[]` (array), `jpd`
declares a bare string. That is not "unread, so who knows" — it is two live documents actively
contradicting each other about the field's own shape, observable without knowing what either value
is meant to accomplish. An array and a string cannot both be the "correct" representation of the
same conceptual field; this clears the consequence bar on structure alone, the same way B's
`publication` finding clears it on content.

**Decision:** `hostViews` must be a JSON array under a new diagnostic code — the narrowest claim
that resolves the actual contradiction, deliberately not validating array CONTENT (host-view file
semantics stay #223/#224's territory).

**Manifest consequence, confirmed by the orchestrator — COUPLED, not sequenced separately:**
enforcing this makes `graphhelm-jpd/extension.json`'s current `"derived-and-deletable"` string
invalid. The fix must land in the SAME PR as the validator change, or `jpd` fails validation the
moment the check merges (an ED-18-class break: the merge result, not just the branch, must
validate). Confirmed NOT a `graphhelm-development-contracts` manifest-queue collision — `jpd` is a
different, non-contended package; J's membership check does not gate this, `jpd`'s own
`extension validate` result is the check.

**Value preserved, not discarded:** `"derived-and-deletable"` is real content, not an empty
placeholder (re-verified directly from the file before deciding) — per the orchestrator's condition,
the fix wraps it as `["derived-and-deletable"]`, a one-element array carrying the string verbatim,
not `[]`. Package digest recomputed after the edit; `jpd`'s own `extension validate` run green
before this PR is considered done.

### `formatVersion` — **REVISED to ADVISORY** (MEASURED ONLY — no consumer, no demonstrated need)

**Grounding:** none in prose (§1.1). My first draft of this blueprint proposed ENFORCE here, by
analogy to `DEVELOPMENT_API_MAJOR`'s "unknown major fails closed" pattern elsewhere in this
codebase — an argument from *stylistic consistency*, not from a demonstrated need, and exactly the
measured-vs-consequence conflation named above. Nothing reads `formatVersion` today, nothing plans
to (no issue claims it, unlike `activation`/#212 or `hostViews`/#223), and both shipped manifests
already agree on `"1.0.0"` with no observed inconsistency to point at the way `hostViews` has one.

**Decision, corrected from the first draft:** document as advisory in the validator's own comment —
declared, unread, no known consumer; a diagnostic gets allocated the day a real design need for a
contracts-format version shows up, not before. This is the "documented as advisory with the reason"
branch the issue's acceptance criteria allows, applied honestly instead of reaching for enforcement
because the shape of the fix was easy to write. Zero manifest edits.

### `missingCapabilityResult` — **REVISED to ADVISORY** (MEASURED ONLY — the two values I'd have codified were observed, not decided)

**Grounding:** `JOURNEY_PROVEN_DEVELOPMENT.md:269-271` states the gap in prose: "Missing runtime
capabilities are reported, never simulated." `ValidatedContribution.required_capabilities` is
parsed but consumed by nothing — no capability registry exists to check it against. My first draft
proposed enforcing a closed enum of `{"refuse", "unresolved"}` — but that enum is MY OWN inference
from what the two packages happen to currently say, not a decided vocabulary from any design
document. Unlike `hostViews`' array-vs-string, "refuse" and "unresolved" are not in structural
contradiction with each other — they are two plausible values with no ranking between them and no
proof either is wrong.

**Decision, corrected from the first draft:** document as advisory — declared, unread, no
capability registry exists to make the value meaningful yet. Codifying a two-value enum today would
freeze a vocabulary nobody has actually decided, and the real behavioral question ("what happens
when a capability actually is missing") needs the registry named as a follow-up, not a manifest-
level enum standing in for it. Zero manifest edits.

### `activation` — **DEFER, documented as advisory** (MEASURED ONLY — owned by #212, unimplemented)

**Grounding:** `AGENTS_SKILLS_PLUGINS.md:444` — "Discovery does not activate a package. Activation
remains explicit" — real, repeated design intent, but issue #212 ("Add atomic extension
installation, activation, rollback, and CLI discovery") explicitly claims this concept by name and
is open, unimplemented.

**Decision:** do not enforce. #212 has design authority over what "activation" means — hard-coding
an enum here, even a narrow one, risks contradicting a decision #212 hasn't made yet. Document the
field as advisory-only with the owning issue named, so the next reader finds the deferral instead of
re-discovering the gap. Zero manifest edits.

### `composition` — **ADVISORY, documented** (final, after two revisions — the orchestrator pulled
this back from an enforce-shape verdict I'd escalated to and they briefly confirmed, for a
reversibility reason worth keeping visible)

**Grounding:** the single relevant sentence found, `AGENTS_SKILLS_PLUGINS.md` §12.6 "One
composition path" (line 442), is about NOT adding a second plugin format — it says nothing about
what `"atomic"` vs `"adaptive"` mean as values. No code composes or merges extensions anywhere in
`core/`. No issue claims this field by name.

**History, kept rather than silently overwritten:** I escalated this rather than guess (no
consequence, no design document, two shipped values with zero documented distinction). First
resolution: enforce a closed-enum shape check, on the reasoning that shape is decidable without
semantics. Second look caught the flaw in that reasoning applied here — a closed-enum check
freezes a choice nobody has actually made, the exact overreach `missingCapabilityResult`'s own
first draft made and got corrected for. The two verdicts are not equivalent the way they first
looked: `hostViews`' array-vs-string is a TYPE contradiction no design intent could justify either
way; `composition`'s atomic-vs-adaptive is two unranked VALUES with no proof either is wrong,
identical in shape to `missingCapabilityResult`'s refuse-vs-unresolved.

**Decision:** document as advisory — declared, unread, no enforcement — with an explicit wake
condition: enforce once `atomic` vs `adaptive` gets a documented behavioral distinction, not
before. Preserve over delete for the same reason: when neither the semantics nor "this field is
vestigial" can be grounded, advisory-then-maybe-enforce is reversible (cheap to tighten later);
delete-then-maybe-re-add is not (the two packages' current values, weak as the signal is, are
lost). Zero manifest edits — the value most worth naming: this keeps `composition` fully outside
the manifest-queue question entirely, no `development-contracts` edit, no second `jpd` edit.

---

## 3. Threat assessment (only where a concrete abuse or defect was analyzed, not imagined by analogy)

| # | Abuse / defect | Concretely | Closed by |
|---|---|---|---|
| U1 | Self-publication (CONSEQUENCE-ANALYZED, B's finding) | A package declares `"publication": "self"` (or any non-`governor-only` value) and validates clean, misrepresenting how it may be published | `publication` enforcement refuses any value but `"governor-only"` |
| U2 | Host-view type confusion (CONSEQUENCE-ANALYZED — the two live manifests already disagree) | A tool that reads `hostViews` expecting an array (the shape most packages use) crashes or silently no-ops against `jpd`'s bare string today, and a future package could declare a number or object with nothing to stop it | `hostViews` enforcement refuses any non-array shape |
| U3 | Silent scope creep via an unnamed field | A future field is added to `contracts` copy-pasted from a template, exactly like `composition`/`formatVersion` were, and ages into production with the same defect this issue measures | Not closed by this PR — named as a real residual risk. `additionalProperties: true` on `contracts` (§1) means EVERY future field starts unenforced by construction; #285 fixes the fields found today, not the shape of the hole they came from. Worth its own follow-up (schema-level closed enumeration of `contracts` keys), out of scope here. |

`formatVersion`, `missingCapabilityResult`, and `composition` do not appear here: no consequence
was demonstrated for any of them, only imagined by analogy in earlier drafts of this blueprint — an
enforcement decision without a U-row to justify it would be exactly the conflation the
orchestrator's reinforcement named, twice over for `composition` (escalated to enforce-shape, then
pulled back). `activation` does not appear: it is not being enforced, so there is no closing claim
to make a threat row about.

---

## 4. Files in scope (derived; #285 is tech-debt-labeled with no Files-in-scope section)

- `core/schema/src/extension.rs` — two new validator checks (`publication`, `hostViews` shape),
  four new doc comments (`formatVersion`, `missingCapabilityResult`, `composition`, `activation` —
  each declared advisory with its own reason, not one shared comment). New diagnostic codes in the
  `GHEX0NN` namespace for the two enforced fields — next free numbers confirmed against the real
  set before use (`GHEX021` onward as of this writing; re-checked at implementation time in case
  another lane claimed one since).
- `apps/cli/tests/extension_cli.rs` — hostile fixtures for the two ENFORCE decisions
  (self-publication, non-array host views) plus pin tests for the four advisory fields, matching
  this file's own established idiom (`valid_package()`, mutate one field, assert the domain code).
- `extensions/builtin/graphhelm-jpd/extension.json` — **one field** (`hostViews`), confirmed by the
  orchestrator and COUPLED into this same PR (not sequenced separately): enforcing the shape check
  makes `jpd`'s current string invalid, so the fix lands with the check or `jpd` fails validation
  the moment it merges. Value preserved: `"derived-and-deletable"` becomes
  `["derived-and-deletable"]`, not `[]` — confirmed non-empty, real content, wrapped rather than
  discarded. Package digest recomputed; `jpd`'s own `extension validate` run green is part of this
  PR's own evidence, not deferred.
- `.factory/e-agent-285-blueprint.md` — this document.

**Not in scope:** `schemas/extension.schema.json` (the fields stay outside the schema's
`properties`/`required` for `contracts`, consistent with how `contributions`/`artifactFlowFormat`/
`entryFamilies` are already validated — Rust-side, not schema-side). `activation`'s enum (deferred
to #212). `composition`'s enum and the behavioral distinction between its two values (advisory,
wake condition named in §2). The capability-registry infrastructure `missingCapabilityResult`'s
full behavior would eventually need (named follow-up). `graphhelm-development-contracts/
extension.json` — confirmed untouched: every enforced field's closed set already contains what
that manifest currently declares, and this package is the one under this session's active
manifest-queue contention, so staying out of it entirely is the safer outcome, not merely the
convenient one.

## 5. Test plan (hostile fixtures with a named real emitter, per house process)

1. `a_non_governor_only_publication_is_refused` — the exact exploit B named:
   `"publication": "self"` on an otherwise-valid package, asserts the new domain code.
2. `a_non_array_host_views_is_refused` — the real, already-shipped `jpd` shape
   (`"hostViews": "derived-and-deletable"`) reproduced as a fixture, asserts the new code — the
   red-before-green case is real production data, not invented.
3. `an_array_host_views_still_validates` — green-path control for U2 (subset-of-empty-is-false:
   a refusal test alone cannot prove the check isn't refusing every shape).
4. `format_version_is_read_and_ignored_without_a_diagnostic` — pins the advisory decision: any
   string value (including nonsense) currently validates, so a future accidental enforcement
   attempt is visible as a test change, not a silent behavior shift.
5. `missing_capability_result_is_read_and_ignored_without_a_diagnostic` — same shape, any value.
6. `composition_is_read_and_ignored_without_a_diagnostic` — same shape, pins the reversibility
   decision (advisory now, enforce only once atomic-vs-adaptive gets a documented distinction).
7. `activation_is_read_and_ignored_without_a_diagnostic` — same shape, pins the #212 deferral.
8. `jpds_real_package_validates_clean_after_the_host_views_fix` — the coupled manifest edit's own
   evidence: `graphhelm-jpd`'s actual on-disk package, post-fix, validates with zero diagnostics.

Each hostile fixture states, before being written, which line in `extension.rs` is expected to
produce the refusal (legal-vs-produced discipline, matching #213's own test plan).
