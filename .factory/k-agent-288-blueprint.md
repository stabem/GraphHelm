# #288 — `NodeType::dead_letter` and the sweep verb's three surfaces: blueprint

K Agent, 2026-08-25. Branch `issue-288-dead-letter-node-type`, cut from `27727a5`.
**No production code written yet.** This file is the enumeration that #162 taught this lane to do
*before* declaring anything.

---

## 1. Why this document starts with an enumeration and not a design

#162 declared five event kinds and needed **seven** carriers to do it. The count went

```
six    claimed from memory
four   measured (a different schema than the one remembered)
five   a digest tripwire, found by running the suite
six    a conformance table, found by running the neighbouring crate
seven  a wire round-trip table, found by running the whole workspace
```

Every one of those was found by *widening the instrument*, never by thinking harder. A new
`NodeType` variant is the same shape of change, so the enumeration comes first and it is **measured
rather than recalled**.

## 2. The carrier enumeration, and the three instruments it took

### 2a. Probe by one variant string — WRONG, too narrow

```
grep -rl artifact_transform  ->  10 files
grep -rl ArtifactTransform   ->   5 files
overlap: core/protocols/src/graph.rs ONLY
```

**Two spellings of the same variant find almost disjoint sets.** The wire spelling finds the
schemas and the governor; the Rust spelling finds `core/runtime/src/classify.rs` — the structural
set this issue names — and the lint. Either probe alone misses half the work.

### 2b. Probe by several variants — WRONG, too broad

Adding `classifier`, `rollback`, `subgraph`, `materializer` grew the union to 47 files, and the
extra ones are false:

```
adapters/postgres-event-store/src/backup.rs   matches "rollback"
                                              mentions NodeType: 0 times
```

Those are ordinary domain words that happen to also be variants. **The first instrument was blind;
the second hallucinated.**

### 2c. Probe by the TYPE, plus the schemas that list the enum — the one that holds

```
grep -rl NodeType --include=*.rs        23 files
schemas listing the variant strings      4 files
```

**And the third defect is in this one too:** `grep -rl '"nodeType"' schemas/` finds only **two** of
the four, because the two schemas name the same concept with different keys:

```
schemas/node.schema.json                     key: "type"        16 variants
schemas/persisted-graph-version.schema.json  key: "nodeType"    16 variants
```

**One grep cannot find both.** Authoring and persisted vocabularies diverge in the KEY, not just the
value — the same trap #162 hit when `customs` was nested under `completion` in one schema and flat
in the other.

### 2d. The carrier set this lane will work from

| role | carrier |
| --- | --- |
| the enum | `core/protocols/src/graph.rs` |
| structural set | `core/runtime/src/classify.rs` — `work_kind` |
| lint | `core/graph/src/lint/mod.rs`, `lint/deployment.rs` |
| persistence | `core/graph/src/persistence.rs` |
| authoring schema | `schemas/node.schema.json` (key `type`) + `releases/1.0.0/` mirror |
| persisted schema | `schemas/persisted-graph-version.schema.json` (key `nodeType`) + mirror |
| catalog pins | `schemas/catalog.json` + `releases/1.0.0/catalog.json` |
| pin defence | `core/schema-evolution/tests/catalog_integrity.rs` (if the digest moves) |
| round-trip / conformance | `core/protocols/tests/persistence_wire.rs`, `core/schema-evolution/tests/conformance.rs` — **check whether either enumerates NodeType by name the way both enumerate EventKind** |
| policy / runtime | `core/policy/src/evaluator.rs`, `core/runtime/src/driver.rs`, `prompt.rs` |

**Not claimed:** that this list is complete. It is what three instruments agree on, and #162's
history says the next carrier is found by running something wider, not by re-reading this table. The
first `cargo test --workspace` on a declared variant is the real enumeration.

## 3. The decision the issue asks for, and where it goes

`NodeType::dead_letter` joins the structural (refused-to-execute) set **declared-only in v1**, with
the legal-vs-produced note **at the variant**.

A type table says what is **legal**; it never says what is **produced**. A variant nothing emits is
a different fact from a variant something emits, and the difference belongs written where the
variant is declared — not inferred later by a reader who greps for emitters, finds none, and cannot
tell "not yet" from "never".

**Measured caution for whoever writes it:** the seven `dead_letter` hits in #162's diff were the
CUSTOMS STAGE `dead_lettered` — a different enum, a different vocabulary, a different lane. Counting
those as evidence of this work would be a false green.

## 4. The surfaces, and a gap worth naming before writing any of them

An existing mutation verb reaches operators through four files:

```
apps/cli/src/args.rs                       the CLI argument
apps/cli/src/commands/<area>/<verb>.rs     the implementation
apps/cli/src/commands/serve/routes.rs      HTTP
apps/cli/src/commands/mcp/tools.rs         the MCP tool table (14 entries today)
```

**The surfaces have AGREEMENT guards and no COMPLETENESS guard.** `api_http.rs` and `mcp_stdio.rs`
both carry parity guards, and they all have the same shape: *drive the same story on two surfaces
and compare the answers*. That proves the surfaces do not disagree **about the verbs they both
have**. Nothing asserts that a verb reached all three.

So a verb landing on CLI and HTTP and silently missing MCP is **green on every existing guard** —
which is exactly the shape of the seventh carrier in #162, one layer up. This is the same defect
class as `EventKind::EVERY_WIRE_NAME`, which exists precisely so a new variant cannot be forgotten
by a table; the surfaces have no equivalent.

**Proposal, to be decided before implementation:** add the completeness guard as part of this issue
rather than after it, or file it and say so. Not silently rely on remembering.

## 5. Sealed before any code

Sealed at `27727a5`, read at 2026-08-25.

1. **Declaring the variant will break at least one carrier this table does not list.** Death
   condition: a full `cargo test --workspace` after the declaration comes back green on the first
   attempt. #162 went 6→7; predicting "the table is complete" would be the same claim that was
   wrong four times.
2. **`cargo test --workspace` will report `targets == results`.** If they diverge, the run is
   truncated or something is double-reported and no count from it may be quoted.
3. **The MCP surface will need reading again before it is touched** — `url.rs` (#301) and capability
   tokens (#307) landed the night before this file was written, and this blueprint has NOT read
   them. That is stated as unread, not as unchanged.

## 6. Order of work

1. Declare the variant and its note. Run `--workspace`. **Let the carriers announce themselves.**
2. Fix each carrier the run names, one commit per role.
3. Decide the completeness guard question (§4) — with the Orchestrator, since it widens scope.
4. Then the surfaces, MCP last and only after re-reading it.

**Nothing in §6 starts until §5's first prediction has been given its chance to fail.**

---

## 7. What the first run taught (appended 2026-08-25; §1–§6 left standing on purpose)

The predictions in §5 stay as written. This section records what happened to them, because a
blueprint that is edited to match its outcome stops being a record of what was believed beforehand.

### 7a. The carriers that announced, and the sub-prediction that was wrong

```
cargo check --workspace --all-targets
  1) core/protocols/src/graph.rs   as_str, non-exhaustive        <- in the §2d table
  2) core/runtime/src/classify.rs  work_kind, non-exhaustive     <- in the §2d table
  then green
```

**§5 assumed `check` would enumerate several carriers at once. It does not.** Crates compile in
dependency order and the first failing crate blocks its dependents, so carriers arrive **one
dependency layer at a time**. `protocols` had to be fixed before `runtime` could even be compiled to
fail. "Check is green" therefore means *the compile-time layer is exhausted*, not *the carriers are
exhausted*.

### 7b. The first seal died UNDECIDED, and the defect was in the seal

`cargo test --workspace` was not green — one test failed — but the failure was **not a carrier**: a
staleness instrument compared `<root>/target/debug/graphhelm.exe` against a source this lane had
just edited, while the run had built into an isolated target dir the instrument cannot see (#349).

So nothing outside the table announced, **and** the workspace was not green. The seal had two cells
for a three-valued world and had to report the wrong one of the two. Re-sealed with three, the third
being: *the run fails only on something not attributable to the variant — and that cell decides
nothing and may not be argued into either of the others.*

### 7c. THE OPEN QUESTION IN §2d IS ANSWERED, AND THE ANSWER IS THE FINDING

§2d asked whether the by-name tables enumerate `NodeType` the way both enumerate `EventKind`.
Measured:

```
NodeType equivalent of EventKind::EVERY_WIRE_NAME     NONE
conformance.rs mentions of NodeType                   0
any test comparing the Rust enum to the schema enum   0
dead_letter in node.schema.json                       0
dead_letter in persisted-graph-version.schema.json    0
dead_letter in both 1.0.0 mirrors                     0
```

**The mechanism that caught #162's sixth and seventh carriers does not exist for `NodeType`.**
`EVERY_WIRE_NAME` is why a forgotten event kind goes red by name; nothing plays that role here.

**The consequence is the whole reason this section exists:** for this variant, a **green** workspace
would not show that the schema carriers are satisfied — it would show that **nothing asks**. That is
the #162 `customs` defect in a new place: a value the Rust type accepts and the schema refuses, with
no guard to make it loud. There it cost a day and was found only because a fixture went through the
public door.

The refuting cell was replaced accordingly, **before the next run and for a reason independent of
any result**: green refutes only if the schemas are *also* shown to carry the variant by a check
that names it; otherwise green is the uninformative cell.

### 7d. Scope addition, decided with the orchestrator

**The missing guard is part of this deliverable, not a follow-up.** Declaring the variant means the
schemas carry it AND something asks by name — otherwise the next variant is forgotten exactly the
way this one would have been, and the failure has no symptom.

§6's order gains one step before the surfaces:

```
3b. do the four schemas list the variant?  (measured: no)
3c. add it, and add the by-name guard that would have caught its absence
```
