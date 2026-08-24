# #200 design — three states for the gate's slot-lock read, not two

Author: E, 2026-08-20. Design-only, zero cargo. Waits on #187 landing before any patch
(both touch `ci/gate.ps1`); this document is the design, not the edit.

Verified against `origin/main` (`d0b3f04`) myself before writing anything —
`git show origin/main:ci/gate.ps1`, not taken on the orchestrator's line citations
alone, though they checked out exactly: `Read-SlotLockSnapshot` at `:191-215`, call
sites at `:341`, `:359-360`, `:466`, manifest assembly at `:326-327`. Confirmed zero
other consumer of `slotLockAtStart`/`slotLockAtEnd` in Rust or Python source
(`git grep` against `origin/main`, positive control run first: the same tool finds 4
real hits for `SlotLockAtStart` inside `gate.ps1` itself, so the zero elsewhere is a
genuine absence, not a broken search) — the manifest field is free to change shape
without a compat shim; nothing downstream parses it yet.

## 1. The actual defect, named precisely

`Read-SlotLockSnapshot` derives the lock's location from `$env:CARGO_TARGET_DIR`
(`:195`, `Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'`). This was always a category
error, not just a stale path: `CARGO_TARGET_DIR` is now explicitly a **PER-LANE**
variable (each lane gets its own `D:/graphhelm-target-<lane>`, per the factory's own
decree), while `SLOT.lock` is a **MACHINE-WIDE** coordination singleton — its entire
purpose is arbitrating cargo access ACROSS lanes on one physical machine. Deriving a
cross-lane fact from a per-lane variable only ever "worked" by coincidence, back when
every lane shared one `CARGO_TARGET_DIR`. Per-lane target dirs broke the coincidence
silently; the canonical lock relocating (`graphhelm-target-m10` →
`D:/graphhelm-slot/SLOT.lock`) is a second, compounding staleness, but not the root
cause — the function would be wrong today even if the lock had never moved, because
it is reading the wrong KIND of variable, not merely an outdated VALUE of the right
one.

**Today's consequence:** every call now returns `present: false` — for two entirely
different underlying reasons a `bool` cannot distinguish: (a) genuinely nobody holds
the slot, or (b) the function looked in a location that has not been the lock's home
for a while. Both currently produce a well-formed, confident-looking JSON object with
a plausible `reason` string. **A populated, wrong field reads as evidence; only an
absent field reads as a question** — and this field is never absent, it is always
populated, always plausible, and sometimes lying.

## 2. Three states, discriminated by tag, not by boolean

```
{ status: "present",       path: <string>, content: <string>, observedAtUtc: <string> }
{ status: "absent",        path: <string>, observedAtUtc: <string> }
{ status: "indeterminate", reason: <string>, observedAtUtc: <string> }
```

`status` is a string tag, not a boolean `present` field. This is the structural part
of the fix, not a naming preference: a boolean has exactly two values, and today's
bug is precisely that a THIRD, semantically distinct fact ("I don't know where to
look") was folded into one of the two available slots. A three-way tag makes that
folding a visible, reviewable act — collapsing `indeterminate` into `absent` later
requires someone to delete a branch, not merely fail to add one.

- **`present`**: the canonical location was resolved, checked, and a lock file
  exists there. Carries its content, same as today's `present: true` branch.
- **`absent`**: the canonical location was resolved AND checked, and genuinely holds
  no lock file. A real, positive fact about the machine's current state — "nobody
  holds the slot," not "I couldn't tell."
- **`indeterminate`**: the canonical location could not be resolved at all, OR it
  was resolved but reading it failed for a reason unrelated to non-existence (a
  permissions error, an I/O error, a manifestly malformed configured value). **Never
  collapses into `absent`.** This is the state today's function cannot express.

## 3. How the gate learns where the lock is, without reproducing today's failure

**Do not keep deriving the lock's location from `CARGO_TARGET_DIR` at all** — that
variable is now provably the wrong kind of thing to ask, independent of its current
value. Give the lock its own dedicated, machine-wide-scoped source:

Proposal (open to a better name from whoever picks this up, same as `deadline`'s
field name was a proposal until it landed): `$env:GRAPHHELM_SLOT_LOCK_PATH`, pointing
**directly at the lock file itself**, not at a directory to `Join-Path` against.
Removing the directory-plus-join step removes a whole class of "right directory,
wrong concatenation" bugs — the exact shape of today's defect — rather than moving
the same shape one variable over.

- **Unset** → `indeterminate`, `reason: "GRAPHHELM_SLOT_LOCK_PATH not set"`. This is
  the case the orchestrator named as the one that must not reproduce today's bug:
  an unset dedicated variable is NOT "no lock," it is "nobody told this run where to
  look," and it must render as a visibly different fact.
- **Set, path does not exist** → `absent`, carrying the resolved `path` so a reader
  can independently confirm the gate looked in the right place.
- **Set, path exists but unreadable** (permission/I/O error) → `indeterminate`, not
  `absent` — existence and readability are different facts; a lock that exists but
  can't be read is not evidence that nobody holds the slot.
- **Set, path exists, readable** → `present`, with content, same as today.

**Why this design resists a future repeat, not just this one:** the property that
matters is that "the dedicated variable is unset" and "the dedicated variable is set
and correctly says nothing's there" are DIFFERENT VARIANTS of a tagged union, not two
paths that compute to the same boolean. `CARGO_TARGET_DIR` never appears in this
function's logic anymore, so a future per-lane change to THAT variable (which has
already happened once) cannot silently affect this function's answer again — the
categories are now actually separated, not just relabeled.

## 4. Sealed trap fixtures — named before the design closes, not after

Two arrangements, each proving a different half of the fix. Neither has been run
(#200 has no code yet, waits on #187) — sealed here so the prediction exists before
the machine does.

### Fixture A — today's confident lie, on real machine state

**Arrangement:** a real `SLOT.lock` file exists at the current canonical location
(`D:/graphhelm-slot/SLOT.lock`), with genuine parseable content (e.g. `"X |
2026-08-20T14:00:00Z | lane #200 | STATUS: RUNNING"`). `$env:CARGO_TARGET_DIR` is set
to a per-lane dir that holds no lock file at all (e.g. `D:/graphhelm-target-e163`).

**Predicted, today's code:** `{present: false, reason: "no SLOT.lock at
CARGO_TARGET_DIR", targetDir: "D:/graphhelm-target-e163"}` — confident, well-formed,
wrong. A lock genuinely exists; this reports that none does.

**Predicted, redesigned code**, with `$env:GRAPHHELM_SLOT_LOCK_PATH` correctly set to
`D:/graphhelm-slot/SLOT.lock`: `{status: "present", path: "D:/graphhelm-slot/SLOT.lock",
content: "X | 2026-08-20T14:00:00Z | lane #200 | STATUS: RUNNING", ...}` — correct,
and correct REGARDLESS of what `CARGO_TARGET_DIR` is set to, which is the proof the
decoupling actually happened rather than being renamed in place.

This is the regression guard: same real-world state, run both versions, old lies,
new doesn't.

### Fixture B — the indeterminate case, proving the redesign didn't just relocate the bug

**Arrangement:** `$env:GRAPHHELM_SLOT_LOCK_PATH` is unset — deliberately reproducing
the shape of the ORIGINAL failure (a variable nobody exported) one layer removed.

**Predicted, a NAIVE reimplementation** that just swaps the variable name into
today's two-branch shape without adding the third state: `{present: false, reason:
"GRAPHHELM_SLOT_LOCK_PATH not set"}` — the exact same class of lie, new variable,
same mistake.

**Predicted, the actual redesign:** `{status: "indeterminate", reason:
"GRAPHHELM_SLOT_LOCK_PATH not set"}`.

**The sabotage this fixture exists to catch:** collapse the `indeterminate` branch
into `status: "absent"` — the "helpful simplification" a future editor reaches for
when a three-state union looks like it could be "cleaned up" back to two. This
fixture must go red the moment that happens; a green result here with the branches
merged would mean the fixture never distinguished the two states it claims to.

## 4b. A cell neither fixture covers — the orchestrator caught it, and it's real, not hypothetical

`absent` was defined above as "the canonical location was resolved AND checked, and
genuinely holds no lock file." **The gate cannot observe "canonical."** It can only
observe "the configured path is set" and "nothing exists there." If
`GRAPHHELM_SLOT_LOCK_PATH` is set — not unset, Fixture B's cell — but set to a path
that **was** canonical and no longer is, my design as first written reports `absent`
with full confidence. That is today's bug with a third state bolted on, not removed:
the false claim just moved from "I looked at the wrong derived place" to "I looked at
the wrong CONFIGURED place," and nothing in the design distinguished the two.

**This is not a hypothetical cell — I went and checked the actual retired location
while writing this, and it is live, right now:**

```
$ cat D:/graphhelm-target-m10/SLOT.lock
HELD by A Agent | lane #160 (issue-160-event-family-readiness) | STATUS: RUNNING ...
NOTE: this file records POSSESSION, not ORDER.
[...A's own note explains the collision that motivated moving SLOT.lock out of
CARGO_TARGET_DIR in the first place...]
```

`D:/graphhelm-target-m10/SLOT.lock` — the retired location — is not empty and not a
tombstone. It holds a REAL, currently-parseable lock record from today. **If
`GRAPHHELM_SLOT_LOCK_PATH` were misconfigured to point here, the gate would not just
report a false `absent` — it would report a confident, detailed, plausible-looking
`present: true`, describing a hold that has nothing to do with the run being
measured.** The failure this cell names is strictly worse than the one Fixture A
guards: a wrong `absent` reads as "the machine is free"; a wrong `present` reads as
"here is who's holding it and since when," and both would be believed.

**Decision, both halves — the orchestrator asked for a choice, not a hedge:**

1. **Reframe what `absent` (and `present`) actually claim, in the schema itself, not
   just the prose.** Neither state is "the canonical location, checked." Both are
   "the CONFIGURED location, checked" — the manifest should say `path` and mean
   exactly that, with no implied claim that the configuring was itself correct. This
   is honest, but honesty alone doesn't stop a human or a downstream tool from
   reading `"status": "present"` and believing it describes the current run — the
   reframe fixes what the field CLAIMS, not what a reader CONCLUDES, so it is
   necessary but not sufficient.
2. **A structural, not literal, tripwire.** Rather than a hand-maintained list of
   known-retired path STRINGS (which is exactly the class of stale literal #190
   spent today naming three separate times), check the STRUCTURE: does the
   configured path resolve to somewhere inside what matches a per-lane target-dir
   shape — literally equal to `$env:CARGO_TARGET_DIR` if that happens to be set, or
   matching the `graphhelm-target-*` naming convention the factory's own decree
   established? Both are true of `D:/graphhelm-target-m10/SLOT.lock` today. A match
   downgrades the read to `indeterminate`, with a reason naming the specific
   structural signal that fired — **not because the path is proven wrong, but
   because it matches the exact shape of the category error this whole design
   exists to close**, and asking a human to confirm is cheaper than reporting a
   confident answer built on the same mistake this document diagnosed in §1. Named
   explicitly as a tripwire, not a proof: a misconfiguration that doesn't match this
   shape (a typo pointing somewhere unrelated, say) still slips through — declared
   gap, not hidden one.

### Fixture C — configured-and-wrong, content seeded from a preserved snapshot, path built to trigger the tripwire

**UPDATED, 2026-08-20, and the update caught a real mistake in my own first pass at
this fixture, not just a path rename.** The orchestrator overwrote
`D:/graphhelm-target-m10/SLOT.lock` with a tombstone at `~18:15Z` (leaving the live
false hold in place was a real danger, not just a test fixture) and preserved the
original content byte-identical at
`D:/graphhelm-slot/retired-target-m10-SLOT.lock.snapshot`
(sha256 `7f9ad1fa0658f191c0e8f1ddfaa9103c87a6eb17c15e530a6e7d7e7f0fa3d2e7`, **verified
myself against the live file, not taken on the claim** — matches, and the snapshot's
first line matches what I originally read before the overwrite).

My first fix for this section pointed the fixture straight at the snapshot's own
path. **That's wrong, caught rereading my own draft**: the snapshot lives at
`D:/graphhelm-slot/...`, which does NOT match the `graphhelm-target-*` structural
pattern the tripwire (§4b decision 2) checks for — pointing the fixture there would
make the tripwire correctly NOT fire, and the fixture would silently stop testing
what it claims to. The snapshot is the right CONTENT source; it is the wrong PATH
for this specific fixture's purpose.

**Corrected arrangement:** a test-local fixture file, seeded with the snapshot's
exact bytes (content copied, not re-typed — the sha stays the provenance chain), but
placed at a path that matches the structural trigger — e.g. the test's own isolated
`$env:CARGO_TARGET_DIR` pointed at a `graphhelm-target-*`-shaped test tempdir, with
the seeded file at `<that tempdir>/SLOT.lock`, and `$env:GRAPHHELM_SLOT_LOCK_PATH`
pointed at the same file. This exercises the structural tripwire AND uses real,
sha-verified stale content, without depending on any live or shared infrastructure
path that could change again out from under the fixture (which is exactly what
happened to the first draft between writing it and rereading it).

**Predicted, the design as first written (§2-3, before this section) — i.e. without
the tripwire:** `{status: "present", path: "<test tempdir>/SLOT.lock", content:
"HELD by A Agent | lane #160 ...", ...}` — confident, detailed, and describing a
hold irrelevant to the run reading it. The exact failure this section exists to name.

**Predicted, with the structural tripwire from decision (2) added:** `{status:
"indeterminate", reason: "configured path matches a per-lane target-dir shape
(graphhelm-target-*), which is known-wrong for a machine-wide lock — see #200"}` —
refuses to answer rather than answering from a location the design itself has already
proven is the wrong KIND of place.

**The sabotage this fixture exists to catch:** remove the structural check (revert to
trusting any configured path that merely exists and parses) — this fixture must be
the one that goes red, since Fixture A and B's arrangements don't exercise a
configured-but-wrong path at all.

**Declared gap this fixture does NOT close, worth stating precisely now that a real
example exists:** the preserved snapshot's OWN actual path
(`D:/graphhelm-slot/retired-target-m10-SLOT.lock.snapshot`) is exactly the kind of
"typo to somewhere structurally innocent-looking" my tripwire already named as an
accepted gap (§4b) — if `GRAPHHELM_SLOT_LOCK_PATH` were ever misconfigured to point
there directly, the tripwire would NOT fire (the path doesn't match
`graphhelm-target-*`), and stale content would be reported as `present` with full
confidence. Not a new gap discovered late; the same one already declared, now with a
concrete instance sitting on disk rather than only a hypothetical.

## 6. L's frozen criteria (`.factory/l-agent-200-review-criteria.md`) — checked, not accepted on relay

### 6a. The two-state counter-example, verified against the real code

L's claim: `#166`'s TARGET-DIR REFUSAL door (`ci/gate.ps1`, ~40 lines from
`Read-SlotLockSnapshot`) correctly has TWO states, not three, and that's not an
oversight — it's a different KIND of question. **Verified myself, not taken on
description**: fetched `origin/issue-166-target-dir-refusal`, diffed against
`origin/main`. The door (`:110-146` in that branch) compares `$env:CARGO_TARGET_DIR`
against a literal expected path after normalization and refuses the whole script
before any stage runs if they don't match exactly. It never touches the filesystem —
there is no arrangement where the answer is unavailable, because the question is
"does this string equal that string," and a string comparison always terminates with
an answer.

**L's rule, generalized and worth keeping past this one lane:** two states suffice
exactly when the question cannot fail to have an answer. `Read-SlotLockSnapshot`
asks "does a file exist" — a question about the WORLD, which can fail to answer
(unreadable path, permission denied, and the case that started all of this: looking
at the wrong place). That's why the third state belongs here and not on the door
next to it. If a future fix could rephrase the slot-lock question so it stopped being
about the world, it wouldn't need the third state either — but the value that
matters (does someone hold the machine right now) is irreducibly a fact about disk,
so it can't be rephrased away.

**The boundary this also draws, which keeps #200 from creeping into #166's territory:**
the door proves OPERATOR INTENT (did you set the variable correctly), not disk state
— a well-spelled path to a location that doesn't exist still passes the door. If
`#200`'s fix started validating existence "to match the door's strictness," that
would be WIDENING THE DOOR, not fixing the snapshot. Keeping them separate is the
right call, not a missed opportunity to unify.

**And #166 does not obsolete #200, despite sharing a variable name nearby in the same
file — checked, not assumed:** #166's door guarantees `CARGO_TARGET_DIR` equals the
one correct shared BUILD-cache dir before any stage runs. But the canonical
`SLOT.lock` has moved OUTSIDE all target dirs entirely (`D:/graphhelm-slot/`, not
inside any `graphhelm-target-*`) — so even with #166 fully enforced,
`Read-SlotLockSnapshot`'s `Join-Path $env:CARGO_TARGET_DIR 'SLOT.lock'` still points
at the wrong kind of place. #166 fixes a real, separate precondition (the gate always
builds against the shared cache); #200 fixes a different defect (the lock's location
derived from a variable that was never the right root for a machine-wide singleton).
Confirmed in F's own branch: L's review already marked the `CARGO_TARGET_DIR`-unset
branch inside `Read-SlotLockSnapshot` as dead code once #166 lands (the door exits
first) — but the WRONG-PLACE branch (`:271` in that branch, `no SLOT.lock at
CARGO_TARGET_DIR`) stays fully reachable and fully wrong.

### 6b. Undefined config: evidence vs. precondition decides which of L's two outs applies

L: an unset dedicated variable must NEVER resolve to "no lock" (that's the defect
reinstalled with a new name), and a silent DEFAULT is unacceptable (a fallback makes
every run assert about a place nobody chose — exactly how this defect was born).
Two acceptable outs: refuse at the door like #166, or record `indeterminate`.

**Decision: record `indeterminate`, not a door-refusal — because this field's own
purpose, stated in its own existing comment (`:187-190` in `origin/main`), is
evidence for the manifest, never a precondition on the run.** `#152`'s original
design is explicit: "SLOT.lock is agent-managed discipline, not something this
script owns the lifecycle of - it only ever READS whatever is there... as evidence."
A door-refusal would upgrade an evidentiary field into a hard gate the rest of the
script never asked for — L's own framework says that's the wrong one of the two outs
for something that isn't a precondition. `indeterminate` recorded plainly, with a
named reason, is the correct choice for THIS field specifically; a different field
whose absence really did block correctness would earn the door instead.

### 6c. Already-written manifests need a version marker — the expensive part nobody looked at

L: every manifest since the relocation recorded `present:false` (or, per my own
correction below, a false `present:true`) — one is already committed. A future
auditor reading "was there a slot at run X?" gets the wrong answer, and re-editing
committed manifests is unacceptable (they were true records of what the script
observed at the time it ran — [[record-true-when-written]]).

**Decision: the redesigned `Read-SlotLockSnapshot` return shape carries an explicit
schema marker** — e.g. `schemaVersion: 2` (or similar) on every returned object —
so a manifest written by the OLD reader (no such field, or absent) is mechanically
distinguishable from one written by the FIXED reader, without touching a single
already-committed file. Old manifests stay exactly as committed, wrong-but-true-to-
their-instrument; new ones are self-describing about which instrument produced them.
This is new to this design — not present in my first draft — and is owed entirely to
L's review, not something I'd have thought to add on my own.

### 6d. One definition of the path — L's measurement, checked as far as I could without re-deriving all 26

L measured: of 26 `Join-Path` calls across 4 scripts (`origin/main`), only ONE derives
from `CARGO_TARGET_DIR` — the defect itself (`:195`/`:269` depending on branch). The
rest derive from `$PSScriptRoot`, `$repositoryRoot`, `GetTempPath()`. **Spot-checked
rather than re-deriving all 26 independently** (re-counting every `Join-Path` in 4
files is expensive for a claim I have no specific reason to doubt and that L
explicitly invited correction on with a date rather than demanding blind trust): confirmed
the ONE `CARGO_TARGET_DIR`-derived call is real and exactly where cited, via my own
`origin/main` read in §1. **Accepting the sharpened framing this measurement earns:**
the risk was never "deriving paths" in general (`$repositoryRoot`/`$PSScriptRoot` are
also derived roots, and nobody's proposing to eliminate derivation) — it's deriving
from a root ANOTHER PARTY can remap out from under the reader. `CARGO_TARGET_DIR` has
been remapped by factory-wide decree twice in one day; `$PSScriptRoot` (the script's
own location) and `$repositoryRoot` (git-derived) are not similarly volatile. The
fix's dedicated variable should be understood as solving THIS property specifically —
owned by the lock's own lifecycle, remapped only when the lock itself moves, not
whenever an unrelated per-lane build convention changes.

### 6e. Who reads this field — verified independently, and it surfaced a stronger example than mine

L measured: nothing in the repository reads `slotLockAtStart`/`slotLockAtEnd` back as
an input to any check. **Verified independently, broader than my own earlier check**
(I'd only grepped `*.rs`/`*.py` for #163's parity work — redone here across ALL file
types, both `origin/main` and `origin/issue-166-target-dir-refusal`, positive control
first: the same search finds 4 real hits inside `gate.ps1` itself before trusting the
zero elsewhere). Confirmed: the only matches outside `gate.ps1` are DATA — committed
`.factory/gate-runs/*.json` manifests and F's own results text — never CODE reading
the field as a check input. The fix still stands (a wrong record is worse than no
record — a future incident investigation deserves the truth, not silence), but per
L's own framing, the PR must say explicitly: **this field is evidence for a human
after an incident, not an input to any current check.** Skipping that line would let
a future reader believe the class of bug is enforced-closed when it is only
observable-open.

**And verifying this turned up a real, already-committed instance of the exact
failure #200 exists to fix — stronger than anything I could construct synthetically**,
plus a correction to a note that was written specifically to prevent this exact
misreading:

`.factory/gate-runs/ea16b2d656c9-20260820T163851Z.json` (committed on
`origin/issue-166-target-dir-refusal`), a REAL gate run from today
(`observedAtUtc: 2026-08-20T16:38:51Z`): `slotLockAtStart`/`slotLockAtEnd` both read
`present: true`, `targetDir: "D:/graphhelm-target-m10"`, `content:` **A's own stale
note from an unrelated earlier hold** ("HELD by A Agent | lane #160 ... PROPOSED
FIX (with the Orchestrator): move SLOT.lock OUT of CARGO_TARGET_DIR..."). This is
Fixture C's exact arrangement, not a synthetic stand-in — the retired location held
real, detailed, plausible content, and the gate reported it as this run's own slot
state. F had added a note beside this same artifact describing it as recording
`present: false` — **checked against the actual committed JSON, that description
doesn't match the file**; the real failure is false-`present`-with-misattributed-
content, which is the worse of the two readings (a false "ran without a slot" is at
least conservative; a false "here's who's holding it" is actively misleading).
Flagged directly to F rather than silently using the corrected reading — a note whose
entire purpose is preventing a misreading is exactly the kind of artifact worth
double-checking before citing.

**Provenance, stated once here so it never needs re-litigating (cost three messages
between L and me to settle — cheaper to write it down than to reconstruct it from
memory later, mine included):** E found the false-`present` instance and F's mismatched
note, and published both (this doc, the orchestrator, and a direct message to F);
E also flagged the discrepancy directly to L in the same message that reported the
frozen-criteria verification. L independently verified the claim against the
committed manifest (lock content, both `observedAtUtc` values, the run's time window)
before using it in the `#187` correction and the criteria amendment — not a relayed
repeat of E's claim, a separate confirmation that also extended it (§6f, next). Finding
is E's; independent verification and the extension to Gate 9 are L's. Neither found
it "alone."

### 6f. Gate 9, from L's own re-measurement — the manifest already carries evidence nobody reads

L measured the committed manifest fully, not just the fields I'd already pulled:
`slotLockAtStart` and `slotLockAtEnd` are not just both `present: true` with A's
content — they are **byte-identical**, across the full 28-minute span between
`runStartUtc` (16:10:05Z) and `runEndUtc` (16:38:51Z). Neither of us had compared the
pair; each of us had only read one side of it.

**Why this is evidence, not just corroboration:** an untouched, stale file produces
identical start/end reads BY CONSTRUCTION — nothing is writing to it. A genuinely
live lock held across the same span OFTEN would not (a real holder re-verifying,
renewing, or simply having a different `observedAtUtc`-adjacent state) — though L is
careful that this is a tendency, not a proof: a real hold that nobody touches for the
whole run would ALSO read identical, so this is a tripwire, same epistemic status as
my own structural check in §4b, not a determination. **The manifest already captures
both reads specifically so they COULD be compared — the design gap is that nothing
does the comparing or surfaces the result.**

**Added to the design:** the redesigned manifest-writing step computes and records
whether `slotLockAtStart` and `slotLockAtEnd` matched (content-equal), alongside the
existing pair — not to assert staleness from a match (that would overclaim exactly
what L flagged as non-provable), but so the signal that already exists in the data
stops requiring a human to notice it needs checking. Framed the same way as the
structural tripwire: a match is worth surfacing, not worth trusting alone.

**L's own correction, worth recording because it's the same discipline this whole
design has leaned on:** L had verified F's caveat commit LANDED, not that what it
SAID was true — the exact distinction this session has spent the day insisting on
for everyone else. Named against themselves, in their own review, the same day they
were the one telling others to check the artifact and not the summary.

The exact env var name (`GRAPHHELM_SLOT_LOCK_PATH` is a proposal, same status
`deadline`'s field name had before A named it for real). Whether `CARGO_TARGET_DIR`
being unset should independently also gate anything else in the gate script — out of
scope, this design only touches slot-lock resolution. The actual PowerShell edit —
waits on #187 per the orchestrator's sequencing (both touch `ci/gate.ps1`).

## 7. Landed (2026-08-24) — PR #228, branch `issue-200-three-state-slot-lock` off `13ab96f`

`GRAPHHELM_SLOT_LOCK_PATH` shipped as proposed. `Read-SlotLockSnapshot`, the structural
tripwire (`Test-SlotLockPathMatchesTargetDirShape`), and Gate 9's comparison
(`Test-SlotLockSnapshotsIdentical`) moved to a new `ci/slot-lock.ps1`, dot-sourced by
`gate.ps1` — no test harness existed anywhere in this repo for `.ps1` logic (zero
`*.Tests.ps1`, no Pester dependency ever added), so this split is what made "isolated
tests per fixture" possible at all rather than requiring the whole gate to run.
`ci/slot-lock.tests.ps1`: homegrown PASS/FAIL harness, 24/24, TDD followed literally
(RED confirmed before `slot-lock.ps1` existed, GREEN after), plus two sabotage-and-revert
runs (collapse indeterminate→absent, bypass the tripwire) each producing exactly the
predicted 2- and 3-assertion failure and nothing else — both guards proven non-vacuous,
not just written.

**One reconciliation the sealed schema (§2) didn't anticipate, found while implementing,
not before:** §2's `indeterminate` shape never had `observedAtUtc` collide with anything,
because the OLD `present:false` branches this design replaces omitted `observedAtUtc`
entirely — which is the ONLY reason the real manifest that motivated Gate 9 (§6f)
happened to compare byte-identical at all. The new design stamps `observedAtUtc` on
EVERY branch (§2's own schema requires it), which means two reads of a genuinely
untouched lock would never again compare byte-identical by accident. `Test-
SlotLockSnapshotsIdentical` explicitly strips `observedAtUtc` before comparing for
exactly this reason — undocumented in §6f itself, added here so the next reader doesn't
rediscover the same near-miss by watching Gate 9 report `false` for two reads that agree
on everything else.

**A PowerShell trap worth its own line for whoever writes the next `.ps1` test in this
repo:** `[ordered]@{}` produces a `System.Collections.Specialized.OrderedDictionary`,
whose `Clone()` is an EXPLICIT `ICloneable` implementation — PowerShell's method adapter
does not surface it as a callable `.Clone()`, and calling it throws `MethodNotFound` at
the call site, not at parse time. Caught live by actually running the test (would not
have shown up in a syntax-only parse check). Fixed by rebuilding a fresh ordered
hashtable key-by-key instead of cloning. Filed to the shared `shell-tooling-traps`
memory rather than left only here.

**Fields kept exactly to §2's frozen shape**, deliberately not widened past what was
reviewed: `indeterminate` carries `schemaVersion`/`status`/`reason`/`observedAtUtc`
only — no `path`, even on the tripwire branch, even though the misconfigured path would
be useful for debugging. That's a real, small loss; adding it was out of scope for a
design L had already reviewed and frozen.

Validation evidence, security review, and rollback plan are in the PR body, not
duplicated here. Not run: the full gate itself — this change needed no cargo, no build,
no slot to write or unit-test (confirmed: the isolated tests touch only a temp
directory). Left for whoever runs the next cold gate to observe the new manifest shape
end-to-end.

## 8. H's review on #228 — four actions, all closed (2026-08-24)

**(1) Harness had no declared expected count, PASS/FAIL only — a silently-shrunk run
(block dropped by a bad merge, commented out) would still print a clean-looking
`N/N passed`.** Added `$ExpectedAssertionCount = 24` and a third outcome distinct from
PASS/FAIL: `HARNESS-BROKE` (exit 2) when the actual count doesn't match, checked before
the pass/fail summary. Reconciled H's cited 27 against my own 24 in a comment at the
declaration site: 27 is what a naive `grep -c` returns over the file (it counts the two
`function` *definition* lines plus `Assert-Equal`'s own internal delegation call to
`Assert-True`, none of which are independent runtime assertions); 24 is the true dynamic
count, and it's what the harness itself now asserts against. Sabotage-and-revert #3
(dropped one assertion, left `$ExpectedAssertionCount` untouched) produced exactly
`HARNESS-BROKE: ran 23 assertions, expected 24` — proven non-vacuous like the other two.

**(2) The design doc itself was never committed anywhere — `ls-tree` found it in neither
`main` nor the branch, positive-control-checked.** A worktree is a separate physical
filesystem from the main checkout even though both are the same git history — I'd
followed the [[worktree-is-a-bench]] convention of writing durable `.factory/` artifacts
to the main checkout, but never copied this one into the branch that actually needed to
carry it for review. Copied into the worktree and committed alongside the fixes.

**(3) Gate 9's comparison is order-sensitive and that wasn't said anywhere.**
`Test-SlotLockSnapshotsIdentical` compares by JSON string, which is only safe because
every `Read-SlotLockSnapshot` branch is a flat `[ordered]@{}` literal with a fixed key
order. Comment added at the arming site (inside `Read-SlotLockSnapshot` itself, not just
at the comparison function) warning that a future conditional-key branch would make Gate
9 report false divergence between identical states.

**(4) The two original sabotage runs were prose claims in the PR body with no committed
evidence.** Re-ran all three sabotages (the two original plus the new HARNESS-BROKE one)
with real output redirected straight to
`.factory/e-agent-200-sabotage-evidence.txt` — not retyped from memory. One transcription
artifact caught and fixed before committing: the PowerShell here-string used to write one
section header hit the *exact same* backtick-escape trap already in this factory's
`shell-tooling-traps` memory (`` `a `` is PowerShell's alert/bell escape, not a literal
"a") — cosmetic, in my own descriptive text only, the redirected program output itself
was never touched by it. Fixed by hand, confirmed with `grep -P '\x07'` afterward.
