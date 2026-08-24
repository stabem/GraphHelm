# Blueprint — #219 / task-003: snapshot-bound retrieval plans and verified source fallback

> **SUPERSEDED IN PART — implementation opened 2026-08-24 and this document is now a record of what
> was planned, not of what exists.** Read the code and its guards for current truth:
> `core/runtime/src/retrieval.rs`, `core/runtime/src/ports.rs`, `core/runtime/tests/retrieval.rs`.
>
> Kept unedited below, because a plan rewritten to match its outcome stops being evidence of what
> was known beforehand — which is the only thing a blueprint is for. What actually changed:
>
> | planned here | what happened |
> |---|---|
> | §3 manifest scope (BLOCKING) | resolved, option B, verified applied to all seven issues |
> | §4 the four asks to #217 | all four landed at `9e93c6d` and were re-measured before use |
> | §5 `repo_snapshot` content-derived | the arming note did NOT land upstream; issue #246 opened, and G2b/S9 carry the property alone |
> | §8 item 4 `SourceReader` reads | DECIDED workspace-scoped: two permitted exits, not three. Recorded on the port itself |
> | G1/G2/G2b/G4/G5/G6 | implemented, each red observed at its own assertion, eight mutations each reddening one arm |
> | G3, page/byte/token bounds | bounds landed; G3 written and awaiting its first red |
> | the policy YAML | blocked on a schema outside scope, then unblocked by a second #216 amendment; both files now written |
>
> One thing here was WRONG rather than superseded: §3 recommends merging `origin/main` by rebase in
> spirit. Rebasing this branch would have made the frozen-criteria commit cited publicly on #220
> point at a dangling object. The lane merges rather than rebases, and that is now a factory rule.

**Status at authoring: BLUEPRINT ONLY. No code written. Implementation was not open.**
Author: H, under wave-3 assignment. ED-18 applies to any later code PR; this file is
`.factory`-only and so is itself exempt from the merge-result check.

## 1. Bases, and how every claim below was measured

| Base | Ref | Verified how |
|---|---|---|
| Repository state | `origin/main` @ `ef9afcb` | `git merge-base --is-ancestor ef9afcb origin/main` → true |
| Plan / design | `origin/issue-216-token-efficient-development-contracts` @ `f0640cd` | `git rev-parse FETCH_HEAD` → `f0640cd9b1b...`; the orchestrator-named sha is the branch head |
| Issue text | `gh issue view 219` | whole body read |

Every file claim below was read with `git show <ref>:<path>`, so the command contains the ref.
Line numbers are lines **in that blob**, not in a working tree.

## 2. What exists today, measured — not assumed

From `git ls-tree ef9afcb -- core/runtime/src/` and per-file `git cat-file -e`:

| Path in task-003 strict scope | At `ef9afcb` |
|---|---|
| `core/runtime/src/retrieval.rs` | **ABSENT** — task-003 creates it |
| `core/runtime/tests/retrieval.rs` | **ABSENT** — task-003 creates it |
| `core/runtime/src/ports.rs` | PRESENT, 84 lines — task-003 **extends** |
| `core/runtime/src/lib.rs` | PRESENT, 19 lines — module list only |
| `core/runtime/Cargo.toml` | PRESENT, 26 lines |
| `core/protocols/src/development.rs` | **ABSENT** — #217's file, and my dependency |

`extensions/builtin/graphhelm-development-contracts/` does not exist at `ef9afcb` (0 entries). The
only builtin extension is `graphhelm-jpd`, which is the format precedent for everything below.

**The dependency is real and unbuilt.** #219 cannot begin against `main` as it stands; it begins
against #217's merge result. Section 4 is the explicit ask to whoever holds #217.

## 3. ~~BLOCKING~~ RESOLVED — the package validator rejects undeclared files, and the manifest was not in task-003's scope

> **STATUS: RESOLVED. Do not read the rest of this section as an open question.** Decided on #216
> (comment `5392847875`) and **applied to the issues** — verified by me, not taken on report:
> `gh issue view` shows the amendment present on **all seven** package-writing issues (#218, #219,
> #220, #221, #222, #225, #226).
>
> The amendment now in #219's body: `extension.json` **is added to this task's file scope,
> append-only** — this task appends **only its own** `contributions[]` entries, in the same PR as the
> files they declare, with their `sha256`; **editing another task's entry is out of scope and stops
> the PR**; and a content change to a declared file **updates its digest in the same PR**.
>
> The measurement below is kept because it is the *reason* for the rule, and the implementation still
> needs it — but the decision it asked for has been made. **The three options at the end of this
> section are settled history: option B was chosen.**

This was the one item that had to be decided **before** implementation opened, and it was not specific
to my lane.

### The mechanism, verified in `core/schema/src/extension.rs` @ `ef9afcb`

- `CONTRIBUTION_DIRECTORIES` (line 60–71) is an allowlist of top-level package directories. It
  **includes `policies` and `fixtures`**.
- The inventory walk descends every allowlisted directory (line 833–843 → `scan_contribution_directory`)
  to `MAX_INVENTORY_DEPTH = 16` (line 26). `fixtures/retrieval/**` is well inside that.
- Inside the walk, at line 820 and line 923: `if !declared_paths.contains(&relative)` emits
  `GHEX012_INVENTORY` — *"discoverable extension file is not declared by the manifest"*.
- Each declared contribution also carries a `sha256`, verified against the actual bytes at line
  691–696, `GHEX005_DIGEST` — *"contribution digest does not match the declared sha256"*.
- The **only** exempt entries are `extension.json` itself (`MANIFEST_NAME`) and `README.md`
  (line ~794–815).

**Empirical confirmation on the one real package:** `graphhelm-jpd` declares 53 `"path"` entries; its
tree holds 55 files; `README.md` appears zero times as a declared path. 53 + manifest + README = 55.

### Correction to my own first reading, recorded because the shape matters

My first grep of `extension.rs` found no `read_dir` and I was one step from publishing *"undeclared
files land inert and silent"*. That was **wrong** — `GHEX012_INVENTORY`, whose very name would refute
me, was sitting in a list I had already printed. I checked it before publishing and it refuted the
claim. The consequence for #219 survives and is in fact **harder** than what I first believed: not
silent inertness, but a **hard validation failure**.

### Why this blocked task-003 as originally scoped *(settled — kept as the reason for the rule)*

Task-003's strict scope contains:

- `extensions/builtin/graphhelm-development-contracts/policies/retrieval-admission.yaml`
- `extensions/builtin/graphhelm-development-contracts/fixtures/retrieval/**/*.json`

and does **not** contain `extension.json`. Those files cannot exist in the package without a
declaration plus digest in `extension.json`, or `extension validate` fails. `extension.json` is in
**task-001's** strict scope (and task-008's).

The plan is explicit about the consequence, in the parallel-waves section: *"A necessary out-of-scope
edit stops that issue until its scope is explicitly amended."*

### Blast radius — systemic, not mine

Tasks that write files **into** the extension package, against tasks that own its manifest:

| Task | Package files | Owns `extension.json`? |
|---|---|---|
| 001 | `schemas/*`, `fixtures/contracts/**` | **YES** |
| 002 | `policies/code-rule-resolution.yaml`, `fixtures/code-contract/**` | no |
| **003 (mine)** | `policies/retrieval-admission.yaml`, `fixtures/retrieval/**` | no |
| 004 | two policies, `fixtures/memory/**` | no |
| 005 | `policies/owner-output-policy.yaml`, `fixtures/owner-output/**` | no |
| 006 | `policies/context-utilization.yaml`, `fixtures/context/**` | no |
| 008 | skills, README, host adapters | **YES** |
| 009 | `evaluators/token-efficiency.yaml`, `fixtures/benchmark/**` | no |
| 010 | two graphs, `fixtures/journeys/**`, `fixtures/sabotage/**` | no |

**Seven tasks write into a package whose manifest they may not touch.** Task-002, in wave 2, hits
this before I do.

There is a second, permanent coupling beyond the one-time declaration: the digest is over **file
content**. Any later byte change to a task's own fixture requires a matching `sha256` update in
another task's file, indefinitely.

### The three options as they stood *(settled: B was chosen — not an open decision)*

- **A — amend each task's scope to include `extension.json`.** Simple, but N tasks then co-own one
  JSON array. Wave ordering makes the conflicts textual (rebase) rather than semantic, and the plan
  already calls the manifest overlap "sequential".
- **B — treat `contributions[]` as an append-only shared registry with a named owner per entry**, and
  let each task append only its own entries, declared in its scope amendment. Same edits as A, but the
  ownership rule is written down instead of implied, so a reviewer can tell a legitimate append from a
  task editing someone else's entry.
- **C — have #217 pre-declare every downstream task's paths.** Does **not** work: the digest is over
  content that does not exist yet, so #217 would land entries that fail `GHEX005_DIGEST`.

~~This does not block finishing the blueprint. It blocks #219 writing any file under `extensions/`.~~
**Superseded: option B was chosen and the scope amendment is applied. #219 may write under
`extensions/` under the append-only rule quoted at the top of this section.**

## 4. Contracts consumed from #217 — the explicit ask

#217 owns `core/protocols/src/development.rs`. Task-003 does not define wire types; it consumes them.
What task-003 needs to exist there, and in what shape:

1. **`ArtifactBinding`** — already named in #217's description. Task-003 binds every retrieval result
   to one. It must carry scope, schema id and version, producer, digest, and **required snapshots**
   (§5 — the plural is load-bearing).
2. **Two snapshot identifiers with distinct ROLES** — and the sharper formulation is J's, not mine.
   I framed this as "two fields instead of one"; #217 in fact held `required_snapshots: Vec<OpaqueId>`,
   which is **worse than a single field**: two entries in a list **without roles** cannot say which is
   which, so the container destroys the very distinction the verdict rests on. A `Vec` of two passes
   any count check and fails the only question that matters. **The defect is absence of roles; a single
   field is merely one instance of it.** Landed as `SnapshotBinding { repoSnapshot, indexGeneration }`,
   both required, with `is_fresh()` derived rather than stored — a stored `is_fresh` would be a third
   fact able to disagree with the two it summarises.
   **Plus the doc-site ask (see §5):** `repoSnapshot` must be documented, at the site that assigns it,
   as a **content/tree digest and never a ref** — and documented as **not** substituting for the guard.
   This is deliberately not a validator ask: a commit SHA and a tree digest are indistinguishable by
   shape, so no validator could enforce it.
3. **A closed coverage enum** (§6). An enum in the contract — not a bool, not an `Option<...>` — and
   every state must be *nameable by the provider*.
4. **Stable refusal codes.** `NEGATIVE_CLAIM_UNVERIFIED` and `INDEX_STALE` are named in #219's
   acceptance criteria and belong in #217's "stable refusal codes" set, allocated there rather than
   invented locally by #219.
5. **Bounded-size declarations** for result, page, byte and token limits — #219's acceptance criterion
   says these are explicit.

**Two integration notes for whoever holds #217:**

- `core/protocols/src/lib.rs` @ `ef9afcb` re-exports every module with a **glob** (`pub use graph::*;`
  and siblings, lines 13–21). Adding `pub use development::*;` puts every new type in the crate root.
  Any name colliding with an existing root export is a compile error at the crate boundary — seen by
  whoever merges, not by whoever wrote it. Worth a deliberate naming prefix.
- The serde attributes chosen for these types are a **schema** decision, not a style one: an
  `Option<T>` **with** `skip_serializing_if` is a non-required field; **without** it, it is required
  and nullable. #219's fixtures will be written against the schema and will pin whichever was chosen.

## 5. The shape of snapshot-binding

The acceptance criterion is *"stale coordinates never slice live bytes"*. That is only enforceable if
the design separates two ids that are conventionally conflated:

- **`repo_snapshot`** — the identity of the bytes; what the `SourceReader` reads from.
- **`index_generation`** — the identity of what the index was **built from**.

**`repo_snapshot` must be CONTENT-DERIVED — a tree/content digest, never a ref.** *(Found by D in
cross-review of this blueprint; the hole was mine.)* Calling it "the identity of the bytes"
**describes** the field without **requiring** anything of it, and the obvious implementation — a commit
SHA — is the identity of a *commit*, not of a working tree. An uncommitted or unstaged edit then
changes the bytes without changing the identity: the comparison reports fresh, a stored coordinate
slices live bytes, and the acceptance criterion is violated **silently**.

A commit SHA and a tree digest are both forty hex characters, so this is **not structurally
validatable** — #217 cannot be asked for a validator that cannot exist. It closes as a pair instead:
the requirement is documented **at the site that assigns the field**, and the guard that proves it was
actually done lives here (**G2b**, sabotaged by **S9**). The comment arms; only the guard fires.

They are independent values, and their relationship is the freshness verdict:

- `index_generation == repo_snapshot` → coordinates are live-safe.
- `index_generation != repo_snapshot` → coordinates are **stale**, with three permitted exits and no
  fourth: slice **snapshot-owned bytes** (read at `index_generation`, not at HEAD), **reindex**, or
  refuse with **`INDEX_STALE`**.

**Exit one is conditional on a port capability this blueprint must declare, not assume.** *(Found by D;
a gap, not an error.)* "Read at `index_generation`, not at HEAD" requires `SourceReader` to be able to
read **at a historical generation at all**. That is not a #217 ask — `SourceReader` is a port task-003
defines itself — so it belongs here as an explicit requirement on the port:

> **`SourceReader` requirement:** the port either exposes generation-scoped reads, or it does not. If
> it reads only the current workspace, **exit one is unimplementable and the real set is two**
> (reindex, or refuse `INDEX_STALE`).

Two exits is a perfectly acceptable outcome. What is not acceptable is discovering it during
implementation, because a blueprint offering three exits reads as three being available. **The
decision is recorded in §8 item 5 and belongs to the implementation's first hour, not to its middle.**

**Stated by mechanism, so it survives a different provider:** a byte range produced under generation
*G* may only be resolved against snapshot *G*. Any path resolving a *G*-coordinate against a different
snapshot is the defect, whichever provider produced *G*.

`ArtifactBinding` must therefore carry **both**. A plan carrying only one cannot express the
invariant — which is the concrete reason §4 item 2 is an ask and not a preference.

## 6. Coverage: the negative-proof contract

A zero from an index is not absence. It is *"the instrument returned nothing"*, which has several
causes that a single boolean fuses into one legal value — and the fusing **is** the defect, because
each cause has a different correct exit.

The closed coverage lattice task-003 compiles against:

| Coverage state | Meaning | Permitted exit for a zero |
|---|---|---|
| `Complete` | scope fully indexed at a generation matching the snapshot | verified absence may be claimed |
| `Partial` | only part of scope indexed | bounded source fallback, else `NEGATIVE_CLAIM_UNVERIFIED` |
| `Excluded` | scope deliberately not indexed (ignore rules) | same |
| `Skipped` | provider declined this region | same |
| `ExtractionGap` | file reached, parser produced nothing (binary, unsupported grammar) | same |
| `Stale` | `index_generation != repo_snapshot` | `INDEX_STALE`, reindex, or snapshot-owned bytes |
| `Unknown` | provider did not say | as `Partial`; **never** treated as `Complete` |
| `Unresolved` | scope resolved to nothing | as `Partial` |

Three design rules that make the failure hard to reintroduce:

1. **Coverage is a return value, not a query option.** The index port returns hits **and** the coverage
   verdict for the scope asked about, in one value. Results cannot be obtained without coverage, so
   "forgot to check coverage" becomes unrepresentable rather than merely discouraged.
2. **No error channel that erases the reason.** `core/runtime/src/ports.rs` @ `ef9afcb` already sets
   this idiom for the tool seam: *"Errors do not exist on this seam ... every path ends in a record."*
   The retrieval ports follow it. A `Result<Hits, ProviderError>` collapsing "provider down", "scope
   excluded" and "extraction failed" into one error value destroys exactly the distinction the
   fallback decision is made on.
3. **Pagination incompleteness is a coverage state, not a footnote.** A truncated page set is
   `Partial`. A zero on page 1 of an unfinished traversal is not a zero.

**Cost asymmetry, stated deliberately:** under this contract a zero costs *more* than a hit — it
requires coverage, extraction-gap evidence, complete pagination, and possibly a source read. That is
intended, and should not be "optimised" away later. The unverified zero is the cheap, fast,
confident-looking answer, which is exactly why it is the one that gets published.

## 7. Red-first guards the implementation will seal

Each guard is specified as arrangement, the **single** assertion, and the mutation that must redden
only it. A guard whose RED cannot be attributed to its own assertion is not evidence.

### G1 — the negative-claim guard, with its positive control INSIDE

The trap: a test asserting *"partial coverage ⇒ refusal"* passes vacuously if the query would have
returned zero for a boring reason — wrong scope, misspelled symbol, provider never consulted. The
refusal is then correct **by accident**, and the guard is green whether or not the mechanism works.

A zero needs **two** controls: the instrument can see, and the subject exists. So the control lives
inside the guard, sharing its arrangement:

| Arm | Subject in scope | Coverage | Required outcome |
|---|---|---|---|
| **A — positive control** | **present** | `Complete` | ≥1 hit; positive claim compiled |
| **B — verified absence** | absent | `Complete` | zero; verified absence permitted |
| **C — subject arm** | absent | `Partial` | `NEGATIVE_CLAIM_UNVERIFIED` or bounded source fallback |

Arms A, B and C must share the **same** index configuration, scope expression and query. Arm A is what
licenses reading B's and C's zeros as facts about the *subject* rather than facts about the
*instrument*. If A does not produce a hit, the fixture is **HARNESS-BROKE** — the guard reports that
third state rather than folding it into pass or fail.

One arm per coverage state in §6 that maps to refusal (`Partial`, `Excluded`, `Skipped`,
`ExtractionGap`, `Unknown`, `Unresolved`), each with its own assertion. A single arm parameterised over
all six shares one assertion and would stay green while five of the six regressed.

### G2 — stale coordinates never slice live bytes

Arrangement: `index_generation = G1`, `repo_snapshot = G2`, `G1 != G2`, and the file's bytes **differ**
between them at the coordinate. Assertion: the compiled plan either refuses `INDEX_STALE`, reindexes,
or reads bytes **at G1** — never returning G2 bytes under a G1 coordinate. The byte difference is what
makes the arm falsifiable; identical bytes would pass under the defect.

### G2b — identities agree, bytes differ *(the case G2 cannot reach)*

**G2 constructs `G1 != G2`, so it only ever exercises DETECTED staleness** — the path where the
mechanism is handed a visible difference. The violation that matters lives in the other case, and no
refusal-shaped assertion can produce it.

Arrangement: `index_generation` and `repo_snapshot` **equal by construction**, then the working-tree
bytes are edited **before** the stored coordinate is resolved. Assertion: the plan does **not** serve
live bytes under that coordinate. This is the arm that proves `repo_snapshot` was actually derived from
content rather than from a ref — **without it, "must be content-derived" is a sentence in a document;
with it, it is a guard.**

### G3 — determinism

Same inputs and capability receipts ⇒ byte-identical canonical plan, or the identical typed refusal.
Exercise with permuted input key order and with both Windows and POSIX path separators — task-001
already carries a canonical-serialisation criterion across both, and task-003 inherits the exposure.

### G4 — path escape

Provider returns `../`, an absolute path, a drive-relative Windows path, and a symlink leaving the
workspace. Each is a separate arm with its own assertion, and each must be rejected before any read.

### G5 — flood and over-budget

Provider returns more results, pages, bytes and tokens than the declared bounds — four arms, four
assertions. Bound enforcement happens on the **runtime** side; a bound the provider is trusted to
respect is not a bound.

### G6 — authority smuggling

A provider summary containing text shaped like an instruction or a policy waiver must remain candidate
evidence and must not alter the compiled plan. The assertion is on the plan bytes, not on a log line.

### Sabotage matrix — the receipt this blueprint commits the implementation to

Before the implementation is believed, each mutation must redden **exactly** its own assertion, with
baseline and revert both fully green. Anything reddening two rows means the assertions are too coarse
and must be split.

| # | Mutation | Must redden, and only it |
|---|---|---|
| S1 | Treat `Unknown` coverage as `Complete` | G1 `Unknown` arm |
| S2 | Drop the pagination-incompleteness → `Partial` mapping | G1 pagination arm |
| S3 | Resolve a `G1` coordinate against `G2` bytes | G2 |
| S4 | Sort results non-deterministically | G3 |
| S5 | Normalise `..` instead of rejecting it | G4 escape arm |
| S6 | Enforce bounds only when the provider self-reports | G5 |
| S7 | Let a summary field feed plan compilation | G6 |
| S8 | **Break arm A's query so it finds nothing** | G1 must report **HARNESS-BROKE**, not pass |
| S9 | **Derive `repo_snapshot` from HEAD instead of from content** | **G2b only** — G2 stays green, which is the whole point |

S8 tests the guard rather than the code, and it is the reason arm A exists.

### Death conditions — when a guard should be DELETED rather than kept green

*(The practice is D's, from #218; I had no equivalent and the omission is the kind that compounds.)*
**A guard with no stated death condition becomes furniture:** it outlives the reason it was written,
nobody dares remove a green test, and the suite grows a layer nobody can justify or safely touch. Each
guard here therefore carries the condition under which deleting it is the *correct* action.

| Guard | Delete it when | Why not just leave it |
|---|---|---|
| **G2b / S9** | `repo_snapshot`'s type can no longer hold a ref — e.g. it becomes a newtype constructible only from a content digest | The pair exists because the property is unenforceable in the type. Once the type enforces it, G2b tests something that cannot occur, and its green stops meaning anything |
| **G1 arm A / S8** | never — arm A is load-bearing for as long as any zero is read as absence | Stated explicitly so nobody "simplifies" the pair by dropping the control half |
| **G2** | never while coordinates and bytes carry separate identities | — |
| **exit-one arms in G2** | `SourceReader` is decided to be workspace-only (§8 item 4), which removes exit one | A retained arm for an unreachable exit is an assertion about a path that no longer exists |
| **G5 bound arms** | a bound moves from runtime enforcement into a type that cannot represent an over-budget value | Same reasoning as G2b: the guard is a stand-in for a constraint the type could not express |

**The rule this generalises to, and it is the half I was missing:** a guard is justified by a
*reachable* failure. When a change makes the failure unreachable, the honest move is deletion with the
reason recorded — not keeping a green test as a monument. **Deleting the guard that a type now enforces
is not lowering coverage; keeping it is pretending the type did nothing.**

**One qualification, learned the same day this table was written, from a live case in a neighbouring
lane.** "The type now guarantees it" splits in two, and treating them alike is how a house gets
disarmed:

- **Irrevocable** — the guarantee cannot lapse without editing code some guard already watches (a
  newtype with a private constructor, an exhaustive match in the same crate). **Deleting is correct.**
  Every row above is this kind, which is why every row above says delete.
- **Revocable at a distance** — the guarantee comes from a feature flag, a dependency version, or a
  build config. **Deleting here disarms the property from a file nobody associates with the test.**
  The move is not deletion and not blind retention: **change the guard's SUBJECT** to the thing that
  can actually change, and give the surviving guard a message that names *what it wakes* — otherwise
  the eventual failure reads as unrelated noise and gets "fixed" by updating the expectation.

**Test to tell them apart, and it is short enough to actually run:** *can this guard be quietened by
editing a file that is not the subject?* If yes, the guarantee is revocable and the guard is a proxy.
Before deleting any row above, re-run that question against the change that prompted the deletion.

## 8. Open items, carried deliberately

1. ~~**§3 scope decision**~~ — **CLOSED.** Option B decided on #216 and the amendment verified present
   on all seven package-writing issues. #219's remaining obligation is mechanical: declare its own
   `policies/retrieval-admission.yaml` and `fixtures/retrieval/**` entries with `sha256` in the same PR
   as the files.
   **Pre-flight measured at `073d8fa`, so the first hour does not spend it:** a `policy` contribution
   *is* one of the kinds that feeds artifact-flow reference resolution (`schema | evaluator | observer
   | policy`, `extension.rs:1695-1704`), with the ref built as `policy:<id minus the "policy/"
   prefix>`. **But the direction enforced is flow → contribution**, not the reverse
   (`validate_artifact_flow_refs`, `:1882-1884`, `GHEX019_ARTIFACT_FLOW`): each declared flow ref must
   *resolve* to a contribution. **So adding this policy does NOT oblige adding an `artifactFlow`, and
   #219 adds no `entryFamily`.** The obligation runs the other way and belongs upstream: **no flow may
   reference `policy:retrieval-admission` before that policy lands** — a pre-declared reference fails
   exactly as §3's rejected option C did, for the same reason.
   Also fixed vocabulary, from J: `artifactFlowFormat` is `p50.dev/jpd/artifact-flow/v1`, and every
   `entryFamily` requires exactly one `artifactFlow` — relevant to whoever adds families, not to #219.
   **Fixture validation — use the house validator, and take it as a DEV-dependency.**
   `graphhelm_schema::validate_inline_value` is public (`core/schema/src/registry.rs:133`) and is what
   the CLI already uses. `core/runtime/Cargo.toml` (in scope) names **no** `graphhelm-schema` today, in
   neither section — so #219 adds it under **`[dev-dependencies]`**, not `[dependencies]`: fixtures are
   validated in tests, and a schema validator has no business in the runtime's production dependency
   graph. Pulling in a *second* validator (e.g. `jsonschema` directly) would be worse than not
   validating: **it measures a validator the system does not use**, so it can pass while the real
   admission path refuses, and fail while the real path accepts. *(Decision and reasoning from J on
   #217; I verified the symbol and this crate's manifest.)*

   **Trap for the negative fixtures, and it is the mirror of a cell J built deliberately.** J's
   `unknown-major` fixture **passes** schema validation on purpose — the schema admits the *form* of a
   future major, and it is the version check, a *separate* mechanism, that refuses it. That cell exists
   to pin that there are two mechanisms rather than one.
   The mirror applies here: **every negative retrieval fixture must be schema-VALID and refused by the
   retrieval compiler** — never schema-invalid. A schema-invalid fixture goes red at the *validation*
   layer, upstream of the guard, and the guard then passes its own assertion for a reason that has
   nothing to do with coverage, staleness, or bounds. That is a red landing upstream of the assertion,
   which proves the fixture broke and says nothing about whether the guard sees. **Each negative arm
   should assert the refusal at its own pointer — "refused *here*, with *this* code" — not "refused
   somewhere".**
2. **#217's type shapes — the snapshot half is RESOLVED, the doc-site half is not.** `SnapshotBinding`
   landed with both ids (item 5). What remains open is the §5 doc-site requirement that `repoSnapshot`
   be content-derived and that the comment not be read as substituting for the guard. **If that
   sentence does not land at the assigning site, #219 still carries G2b/S9 and the property is still
   guarded here** — the doc makes the next person's mistake less likely, it does not make it impossible.
3. **MCP-backed provider authority is gated on #213.** Nothing here assumes a live provider: every
   guard runs against fixtures. Task-003 should ship provider-independent and stay so until #213 lands.
4. **`SourceReader` generation-scoped reads — decide in the first hour, not the middle.** Per §5: if the
   port cannot read at a historical generation, exit one drops and the permitted set is two. Record the
   answer in this file when it is known; do not let the three-exit wording stand if only two are real.
5. **#217 status as of this revision (reported by J, not measured by me).** `SnapshotBinding` with both
   ids landed on `issue-217-development-contracts` @ `565aa79`, with the closed eight-state coverage
   enum, the two refusal codes allocated as `negative_claim_unverified` and `index_stale` (snake_case —
   **#219 uses these and does not coin its own**), and `DeclaredLimits { maxResults, maxPages, maxBytes,
   maxTokens }` all required. Serde, which my fixtures will pin: `subprojectId` and `executionId` are
   `Option` **with** `skip_serializing_if` (non-required); everything else in the envelope and binding
   is required. **One open contradiction I raised back to J and which is not yet resolved:** a blanket
   `additionalProperties: false` conflicts with #217's own acceptance criterion that *"compatible
   minor-version unknown fields are preserved but never interpreted as authority"* — the policy has to
   be version-dependent (closed across a major bump, preserving-without-authority within a compatible
   minor), or the criterion has to be changed deliberately rather than by schema choice.
6. **Not measured here:** I have not read #217's or #218's in-flight branches. At the time of writing,
   `core/protocols/src/development.rs` does not exist on `main`. Any statement about what #217 *will*
   provide is derived from the plan text, not measured from code.
