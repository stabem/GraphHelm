> **PROVENANCE: this draft became #96 — *GHCLI016 answers two opposite hold-states with one value*.**
> Established by title match against `gh issue list --state all`, not from memory. Written here
> because the derived issue carries the content and never the source's name: provenance is the
> strong relation and the one grep cannot see, so only the author can record it.

GHCLI016 answers two opposite hold-states with one value, so "the call failed" still does not say whether your hold survived

Split out of #83, which fixed the atomicity half. Filed as its own issue on the same reasoning that
put #81 in its own issue rather than folding it into the flake fix: a defect belonging to a *shape*
deserves its own record, or the shape stays invisible.

## What #83 fixed, and what it left

#83's ordering fix means a refused setup no longer commits the resume decision. But the RESPONSE
still answers `GHCLI016_DRIVER_FAILURE` for two cases that are now opposites:

| class | what happened | the operator's hold | their next move |
|---|---|---|---|
| **(a)** setup failure | nothing committed | **intact** | fix the environment, retry — you are where you were |
| **(b)** mid-drive failure | `ExecutionResumed` committed, work attempted then failed | **consumed** | the execution is running/attended; do NOT re-pause blindly |

One value, two causes, opposite responses. The operator cannot tell from the answer which one they
got.

## Note the fix CREATED this divergence — that is the honest framing

Before #83, both `GHCLI016` classes had the resume committed, so "did my hold survive" answered NO
either way. The code was ambiguous about cause but not about consequence. Afterwards the same code
covers opposite hold-states, so the ambiguity is new and #83 manufactured it.

## Why it could wait — and why it is NOT the incident #83 reported

**Post-fix the divergence is loud-resolvable, not silent.** #83's original split was silent because
the STORE lied alongside the response: no second read could rescue you. Now the store tells the
truth, and one status read on the surface built for exactly this — the M07-M09 glance line —
resolves it: `paused` means (a), `running`/`needs_you` means (b).

So the remaining defect is **response ergonomics**: the operator should not NEED the second read.
That is real and issue-worthy, but it must not wear the original incident's clothes in a severity
record. This is deliberately stated because the tempting move is to inherit #83's severity by
association, and severity fields are not re-read once written.

## Flattening: the class, with its sightings

This is the third instance of one shape found in a single day, which is when a shape stops being an
incident and becomes something to hunt on purpose. **A boundary maps distinct causes onto one legal
value, and the consumer needs exactly the distinction that was destroyed.**

1. **#81 — `InvalidRestore`.** An elapsed timeout and a genuinely corrupt archive arrive as the same
   redacted error. Opposite responses: retry on a quieter machine vs never trust this backup.
2. **This issue — `GHCLI016`.** Setup failure and mid-drive failure, opposite hold-states.
3. **The reader-level case (#83's fixture selection).** `manual-override-deploy.yaml` is unusable
   because a node *is* `NodeType::Deploy`; the M09 release graph has a node *named* `deploy` whose
   *type* is `tool`, and it is the correct fixture. The word "deploy" fused two node types in the
   READER rather than in a value — and a careless `grep type:` on that YAML mixes edge types in with
   node types, so the obvious check returns the wrong answer too.

The third is worth keeping in the list precisely because it is not in the code: the same failure
mode operates on whoever is reading, which is why "audit the values" does not catch all of it.

**Suggested for the M10 close doc:** flattening as a named defect class with these three pins.

## Suggested fix

Give class (a) its own distinguishable code (or a distinguishing field on the existing one), so the
answer carries which class it is without a second read. Deliberately not folded into #83: that would
bundle an operator-visible contract change with an internal ordering change, making the ordering fix
unrevertable on its own, and would mint an error code with no observed red behind it.

## Seeds

- **S5-measured.** #83's sabotage ledger ran six of seven rows; the seventh (a refused resume with a
  `recovery_plan` node still appends `Interrupted`) was dropped because it needs a node still
  `Running` at stop, i.e. the hanging-executor runtime arrangement. #83's S5 protection currently
  rests on a structural argument (triage lives inside `execute_prepared`; the hoist moves code above
  that call), labelled argument-not-measurement. **If a future runtime-test slice builds that
  arrangement, S5-measured rides it** rather than justifying a slice of its own.
