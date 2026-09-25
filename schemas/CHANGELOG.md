# Schema Changelog

## activation-receipt 1.0.0

Adds an offline ActivationReceipt claim bound to the exact transaction, accepted plan digest,
host/version, configuration/package digests, environment, fresh session, observer identity and
custody ID, correlated read-only GraphHelm MCP `list` observation, methodology evidence and
installation/observation/expiry timestamps. The digest covers the canonical document without its
digest field. Shape validation and a matching digest never authenticate an observer. Production
has no trusted host observer and remains `installed_unverified` with `observer_missing`.
Synthetic successful custody exists only in adapter unit tests, with explicit fixture labeling.
Production journal readers reject persisted verified claims without trusted in-memory custody;
recomputing a journal checksum never supplies that authority.
The original frozen 1.0.0 release is unchanged. Current catalog inventory also includes the
previously added restore-plan contract.

## restore-plan 1.0.0

Adds a digest-only offline restore preview bound to physical project/home/state roots, the verified original or transaction checkpoint, the owning transaction chain, every supported current file digest and access-metadata digest, and owned package activation digests. Exact approval and a fresh equal preview precede any mutation. Unowned later files are explicitly retained; overlapping text or semantic-key changes are conflicts. JSON/TOML restoration merges only changed owned keys; arrays are indivisible without a stable host identity contract. TOML with later comments conservatively conflicts because the current serializer cannot preserve comment placement. Source contents stay in private verified backups and content-addressed payloads.

The unreleased 1.0.0 contract now covers all fourteen supported project and user configuration surfaces, including override instructions, project settings, managed Claude settings, project MCP configuration, and project/user Codex configuration. Discovered skill and plugin manifests are private backup evidence and are never restore destinations.

The unreleased 1.0.0 contract now covers sixteen surfaces. The Claude user instruction surface is `~/.claude/CLAUDE.md`, the location Claude Code documents; the undocumented `~/CLAUDE.md` is no longer a surface. Two project instruction surfaces are added, `.claude/CLAUDE.md` and `CLAUDE.local.md`. The `current` and `effects` arrays of a restore plan carry exactly one entry per surface; their bounds were the exact count (14) and now are `minItems: 1`, `maxItems: 64`, a compatible loosening. Exactness is not lost: `apply_restore` refuses (`plan_stale`) unless the accepted plan equals a fresh preview built from the current surface set, so a plan carrying fourteen entries against sixteen surfaces is refused by the code that owns the count rather than by a schema that pins it. A backup written against the fourteen-surface set no longer verifies; no release carried that set. The frozen 1.0.0 release is unchanged.

The unreleased adoption journal optionally records its predecessor and a restore intent with the selected backup and source transaction IDs. Completed package entries bind the exact activation-pointer bytes. Restore uses the existing retained-handle publication guards and resumes an interrupted accepted restoration. A partial restore records `recovery_required`; `restored` is published only after all owned file and activation changes complete. Shared user surfaces carry private durable owner references; a different active project is refused before writes. This revision does not add arbitrary file creation, runtime-secret deletion, execution-data deletion, or guard sweeping. The frozen 1.0.0 release is unchanged.

## adoption-plan: a plan may install packages without changing a file (unreleased 1.0.0)

`spec.operations` and `spec.decisions` required at least one item, so the least invasive adoption there is — install the two pinned release packages and change no instruction file — could not be written; fixtures paid for it with a no-op settings operation (#1208 F6). `apply` does install `spec.packages` (pinned, digest-checked, journaled and compensated), so both arrays now accept `minItems: 0`, a compatible loosening. The rule that at least one of `operations` or `packages` is non-empty is enforced by `apply`'s preflight (`invalid_configuration`), not by a cross-field schema keyword, the same split as the coverage entry below. The frozen 1.0.0 release is unchanged.

## adoption-plan: coverage is reported, not asserted (unreleased 1.0.0)

`spec.coverage` accepted only `complete`. A real inventory is never complete — the host API and plugin browser are not observable, and the document says so in `coverageDetails` — so an honest applyable plan could not be written and the only plans that applied were hand-written fixtures claiming `complete`. The field stays required and now carries the inventory's own value (`complete` or `incomplete`); the schema no longer constrains it. Constraining it to the two words would read as a validation-set change, which the branch guard requires to be declared by a major bump, while the release guard requires an unreleased schema to stay at 1.0.0 — so the vocabulary is stated here and enforced by the producer, not by the schema. A complete-compatibility CLAIM still needs complete coverage; applying a reviewed replacement does not. The frozen 1.0.0 release is unchanged.

## adoption-plan 1.0.0, adoption-receipt 1.0.0, adoption-journal 1.0.0

Adds the local, versioned adoption mutation contracts. The plan binds exact approval to bounded operations and explicit scopes. The receipt separates installed file state from verified host activation. The private journal records root identities, relative destinations, before/after digests and access metadata; it never contains source file contents. All three schemas are registered offline. The original 1.0.0 release snapshot remains unchanged, and these additions accumulate in the existing 1.1.0 candidate release.

The unreleased journal contract also records retained publication guards, their source/candidate identities and digests, and `restore_intent`, `detached`, and `restore_detached` phases. Apply and compensation preserve the actual displaced file. Linux uses an atomic exchange; Windows uses two anchored, no-replace renames with a durable journal step between them. Windows readers may briefly observe an absent destination while the host is required to be quiescent. Recovery reconciles a detached file before further mutation and refuses an unrelated file created in that interval. The interval has a fixed number of protocol steps, not a guaranteed wall-clock bound. Initial journals are durable and verified before the active pointer is published; an orphan initial journal can be explicitly recovered without guessing missing intent.

The unreleased journal now optionally records two pinned local Extension activation intents. Each entry carries only its Extension ID, digest, phase, and the identity of an exclusively created private root. A null root represents intent persisted before directory creation; recovery never claims an existing directory from that intent. Activation compensation retires the transaction-owned pointer while retaining package and journal evidence. Package bytes, source configuration, and secrets are not embedded in these entries. Existing journals without package entries retain their meaning.

## node 1.1.0 - `agents` carries the other workers on one task

A node is the TASK: `required` is `type`, `name`, `objective`, `optionality`. `agent` names the
worker, singular, so a task worked by more than one agent was not representable -- only unwritten.
`agents` is an OPTIONAL array of reference bindings naming the other agents working beside the
primary one. `minItems: 1`, because an absent list and an empty one must not mean the same thing;
`maxItems: 64`, the same bound this repository already puts on every other authored list a graph
carries.

**`agent` keeps its meaning and its rule.** A `type == "agent"` node still requires `agent`: it is
the PRIMARY worker, and `agents` names the others beside it. An earlier draft made the two
alternatives (`anyOf`) and refused them together (`not`). `core/schema-evolution` classified that
`GHC003_BREAKING_CHANGE` at `/allOf` (Breaking, Major) and was right: any edit to an existing
`allOf` branch leaves the baseline and candidate branch sets incomparable. `allOf` is byte-for-byte
main's here, and composition -- a primary with a crew -- is the better model anyway.

**A crew member is named by `ref`, never defined inline.** `items` is a closed object requiring a
non-empty `ref`, and not the singular binding's whole `oneOf`. An inline `ephemeral` agent is
externalized by the Graph Governor into an `agent_configuration` control plus up to four sibling
controls, and a persisted node carries AT MOST ONE control of each type
(`core/graph/src/persistence.rs`, the `seen.insert(control_type)` guard). A second inline definition
on the same node therefore has nowhere to be persisted, and a shape that validates but cannot be
published is exactly the half-working surface `AGENTS.md` forbids. Naming the crew by reference is
fully publishable today, and the singular form keeps the inline option it has always had.

**Why this is a Minor and not a Major -- the narrowing argument, answered with the boundary rather
than with rhetoric.** `node` sets `additionalProperties: true`, so a 1.0.0 document could carry a
stray `agents` key of any shape and pass `graph validate`; constraining the key now refuses those
bytes, and a reviewer read that as narrowing the 1.0 acceptance set. Two measured facts decide it:

- **No such document could ever be published.** `collect_node_properties` in
  `core/governor/src/externalize.rs` is an ALLOWLIST -- every node property it does not recognise
  returns `InvalidAuthoring` (`GHE009_EXTERNALIZATION_FAILED`). `agents` was not on that list before
  this change, so every node carrying it was already refused at Graph DSL -> GraphVersion. The set
  of documents that could reach a GraphVersion is not narrowed by one document; only the moment of
  refusal moves earlier, from publication to validation, which is where a diagnostic is more useful.
- **The alternative makes `node` unevolvable.** If a key that a permissive document COULD have
  squatted made an addition breaking, then no property could ever be added to this schema at any
  version, and `completion.customs` (node 1.0.0) and `context-provenance.root` would both have had
  to be majors. The comparator states the house rule directly: a new optional property is
  `GHC102_OPTIONAL_PROPERTY_ADDED`, Compatible, Minor.

`graphhelm schema check --baseline schemas/releases/1.0.0/catalog.json --candidate
schemas/catalog.json` reports exactly one change for `node` --
`GHC102_OPTIONAL_PROPERTY_ADDED` at `/properties/agents`, class `compatible`, impact `minor` -- and
the release gate reports `ok`. That is the version this document declares. `node` joins the declared
divergence ledger in `core/schema-evolution/tests/baseline_origin.rs`, and
`conformance/manifest.json` gains `schema.node.valid.agents`.

**Where the refusals are proven, and why not there.** The public fixture manifest is validated
against BOTH the live schemas and the frozen 1.0.0 snapshot, and
`current_and_1_0_0_release_enforce_identical_shared_public_schema_contracts` forbids exactly one
quadrant: the live validator refusing a document the release accepted. Because 1.0.0 accepts any
`agents` key at all, every refusal this change adds sits in that quadrant, and that guard would be
right to fail. So the acceptance case ships in the shared manifest, where both validators agree,
and the refusals -- empty crew, scalar crew, scalar member, a member with no `ref`, an empty
`ref`, a stray key beside `ref`, and a crew of 65 -- are proven against the LIVE validator alone,
in `core/schema/tests/node_agents.rs`. Two of those tests are red against the previous shape of
this branch, where a member's `ref` was unbounded and the list was unbounded.

**Wired, and only as far as #1049 goes.** Graph Governor publication validates the crew and
records it as one persisted control, `node_agents` (`agentRefCount` plus `agentRef.NNN` in the
Agent reference domain), and extension validation checks every member's reference at
`/spec/nodes/<id>/agents/<i>/ref`. Multi-agent DISPATCH is NOT part of this change: the runtime
still dispatches the primary `agent`. The crew is now representable, publishable and replayable;
acting on it is separate work.

`schemas/releases/1.0.0/**` is untouched.

## event-envelope 1.1.0 - `agentPresenceDeclared` gains `session`, and `model` stops being required

Two changes to the same unreleased `$defs/agentPresenceDeclared`, both inside the one legal 1.1.0
Minor step the entry below already claims, and both compatible against the frozen 1.0.0 baseline
(`schema check` reads the whole def as `schema definition added`, because the def does not exist
there at all).

- **`session`** - OPTIONAL, opaque, `minLength: 1`, `maxLength: 128`, `pattern: \S`: the same bound
  the model carries, for the same reason (caller-controlled and persisted). It says WHICH session
  is speaking. `graphhelm mcp` sends its own per-process nonce, the one its idempotency keys and
  wake leases already carry, so a reader can join a declaration to the rest of that session's
  traffic.
- **`model` moved out of `required`.** An actor id is stable across sessions and a model is a
  property of ONE session. While a session that declared nothing wrote no event at all, its silence
  could not supersede the previous session's record, and a new session reusing a stable actor id
  inherited a model that had stopped being true - the board showed a dead session's model as the
  live one's. An event with an ABSENT `model` is how a session says "I declare nothing" out loud.

**Nothing is inferred, and that has not changed.** A model is still never derived from a route, a
default, or a user agent. The only new thing that can be written is an ABSENCE, and only by a
session that named itself: the Runtime writes this shape when `X-GraphHelm-Actor-Session` is present
and `X-GraphHelm-Actor-Model` is not, and writes nothing at all when neither is. A payload carrying
neither `model` nor `session` is legal to the schema and is never produced - expressing "model
absent implies session present" would mean an `anyOf` inside the def, a composition the comparator
reads as a change of its own, for a shape no writer can reach.

The Studio reads it the way it is meant: `newestPresenceByActor` counts a model-less record as the
newest for that actor and then REMOVES the entry, so the board renders no badge rather than a stale
one.

## event-envelope 1.1.0 - `agent_presence_declared` added, wired writable, and `model` bounded

A session that names the model it runs as, and optionally the effort it runs at, records that as an
event of its own. Identity does NOT widen: `$defs/actor` stays closed and `PersistedActor` keeps
`deny_unknown_fields`, because an actor id is stable while the model behind it is a property of one
session. A new event kind is the only shape that can carry a changing fact about a stable actor.

**`agentPresenceDeclared`** carries `actorId`, `actorType`, a `model` string and an optional
`effort`. `model` is an opaque string and never an enum -- the set of models changes faster than
this repository ships. `effort` is the closed vocabulary `low | medium | high`, because a closed
enum is refusable and a free string is not. Absent is absent: a session that declares nothing
produces no event, and no default or `"unknown"` is ever written as a value.

**Additive by construction.** The change is a new branch in `$defs/eventKind`'s `oneOf`, discriminated
by a `const` no sibling branch carries, plus a new payload under `$defs`. Nothing existing was
edited and no `allOf` was touched: `compare_catalogs` compares composition BRANCH SETS, so an edited
`allOf` branch reads as `composition ambiguous` and an added one reads as narrowing -- only a
disjoint `oneOf` addition classifies as `composition widened`. Every document valid under the
previous bytes is still valid.

**`agent_presence_declared` is writable.** The top-level `oneOf` pairs `agent_presence_declared`
with `scopeWithExecution`. Without that pairing an envelope of this kind matched ZERO top-level
branches and the store answered a bare `Invalid` -- no diagnostic, no schema named, no kind named.
That silence has cost this project eleven commits before
(`core/events/tests/dlq_kinds_are_declared.rs`).

**`scopeWithExecution`, not `projectScope`.** A declaration is a fact about a session, but the
question it answers -- who is working THIS execution, and with what -- is asked of an execution.
Recording it on the execution stream is what lets a reader meet the declaration in the same walk
as the work it explains, and what lets "this model did this" be replayed rather than joined
across streams.

**Additive by construction, and MEASURED rather than assumed.** The top-level union is pinned
UNPROVABLE (`union_provability.rs`, class B), so an addition there is not free by inspection: the
prover cannot compare branches whose discriminating constraints live one level up. It CAN still
decide this one, because the added branch carries a `const` on `kind.type` that no sibling
carries. `compare_catalogs` classes it `Compatible`/`Minor`. A deliberately OVERLAPPING row
(a second branch with an existing `const`) was run as the control and reports
`oneOf overlap unprovable` -> `GHC003_BREAKING_CHANGE` at `/oneOf`, so the green above is the
subject's and not the instrument's blindness.

`$defs/actor` is still untouched and no `allOf` was edited or added.

**`model` is bounded.** It was `{"type": "string", "minLength": 1}`: unbounded in length and
satisfied by `" "`. It is a CALLER-CONTROLLED value that this repository PERSISTS and renders, so
it now carries `maxLength: 128` and `pattern` `\S`.

**128 is borrowed, not invented.** It is `OpaqueId`'s own cap, and a model name is the shape
that cap was chosen for: an opaque, caller-chosen, identifier-like string this repository must
not enumerate. The longest vendor name in play today is under 50 bytes.

**`pattern` says the part `minLength` cannot.** `minLength: 1` accepts a single space, which
renders as a blank badge -- present, unreadable, and indistinguishable from a rendering bug.
`\S` is unanchored, so it means "contains at least one non-whitespace character", which is
exactly the property wanted and nothing more.

**Why the schema and not only the HTTP door.** The Runtime refuses an over-long, blank or
non-ASCII `X-GraphHelm-Actor-Model` header before anything touches the store. That protects the
one door this repository ships. The schema is what binds a producer that never passes through
it. The append-time durable-content scan does NOT substitute for either: it caps the whole
envelope and refuses secret-shaped strings, and has no per-field length rule at all -- a
distinction previously misstated in a doc comment and now written down where it can be checked.

**Additive against the frozen 1.0.0 baseline as a whole.** Every piece above -- the new kind, its
wiring, and the bound on `model` -- reads against the 1.0.0 baseline as `schema definition added`
or `composition widened`, never as a narrowing of anything a 1.0.0 document could rely on. This
was previously shipped and reviewed as three separate version bumps (1.1.0, 1.2.0, 1.3.0) across
three commits of one unreleased change; none of those numbers was ever published, so only the
final one -- 1.1.0, the single legal Minor step from the 1.0.0 baseline -- makes a claim. The
three narratives above are unchanged; only the three illegal version numbers were collapsed.

## context-provenance 1.0.0 - optional `root` (#1086)

One OPTIONAL property, `root`: `"project"` or `"execution"`, the tree the node's search and reads
ran over (the project checkout, or the execution's own Tier 1 tree). Every record the runtime seals
from #1086 on carries it; a record sealed before carries none, and read the project. Optional, not
required, for the reason the entry below states for the receipt: this document is not in the frozen
1.0.0 release, so the frozen-baseline check holds it at `1.0.0`, and a new REQUIRED property is a
breaking change against what landed on `main` that could only be declared at `2.0.0`. An added
optional property is comparator-compatible, so the document stays `1.0.0` with a re-derived catalog
digest. A new fixture, `invalid.root`, pins the refusal of any other value at `/root`; the `valid`
fixture carries `"root":"project"`.

## context-provenance 1.0.0 - added whole (#1065)

The content-free record the runtime seals beside a model reply when a context capsule was
compiled for the node: the tokenizer and estimator ids, the query terms, the repository-relative
`sources` shipped, the counts (candidates returned, dropped, unreadable; items dropped by the
budget; excerpted sources), the byte totals, the capsule digest, the fallback if any — and six
`accounting` lines in the execution-accounting-receipt's own `CostField` vocabulary:
`zero_result_queries`, `retrieval_pages`, `retrieval_fallbacks` `measured` by
`context_retrieval`; `compiled_input_tokens`, `eligible_candidate_tokens`, `tokens_saved`
`derived`, no producer, a note opening with the method id `bytes-div-4/v1: ` and the arithmetic.
Never `measured`, because nobody counted tokens — the `invalid.measured-estimate` fixture pins
that refusal at `/accounting/3`. No `oneOf` anywhere: every line has one shape, because the record
exists only when the chain ran. Added whole, the same class of divergence from the frozen 1.0.0
baseline as the receipt itself; the by-name ledger in `core/schema-evolution/tests/baseline_origin.rs`
names it.

**Why the receipt's own lines did not move, stated so nobody repeats the attempt.** The first
shape of #1065 changed `execution-accounting-receipt` in place — `zero_result_queries`,
`retrieval_pages`, `retrieval_fallbacks` to a measured-or-unavailable line, `compiled_input_tokens`
to a derived-or-unavailable line, and `eligible_candidate_tokens` / `tokens_saved` appended with
`minItems: 13` so every existing document stayed valid. By meaning that is additive. The
compatibility comparator does not reason about meaning: it classes a changed positional `$ref`
and a newly constrained `prefixItems` position as `GHC003_BREAKING_CHANGE`, and the house then
holds two rules at once — `no_silent_breaking_change_against_what_landed_on_main` demands the
document move to exactly `major + 1`, while `graphhelm schema check` against the frozen 1.0.0
baseline refuses any unreleased schema not at `1.0.0` (`GHC004_SEMVER_MISMATCH`, "new milestone
02 schemas must start at document version 1.0.0"). An unreleased schema therefore admits only
comparator-compatible changes, and no reshaping of positional lines is one. The receipt stays
byte-for-byte at 1.0.0 with its six context lines `unavailable`; the numbers live here; the
receipt's lines move when the next frozen baseline is cut.
## event-envelope 1.0.0 - `execution_form_declared` gains `name`, `objective` and `executor`

Three OPTIONAL properties on the declared shape (#1063), so a resume briefing derived from the
store alone can say what a run is for and who was going to run it:

- **`name`** - the graph document's `metadata.name`: what an authored graph calls itself, and
  where a synthesized graph puts the goal it was compiled from.
- **`objective`** - the `objective` of the FIRST node in `spec.entrypoints` order: the operator's
  request in their own words, which is where the Studio's draft keeps them while its
  `metadata.name` is a placeholder.
- **`executor`** - `fixture` or `gateway`: what `execution start` was going to drive the nodes
  with, recorded at the door rather than inferred later from the outcomes.

Both texts are bounded to 2000 characters (`maxLength`, counting code points as the Rust side
does) and are TRUNCATED at that bound by the writer, never refused: a briefing with a shortened
name is better than a start refused over a long one. A blank text is omitted, not written empty,
and so is a text the durable-content scan would reject (a `secret://` reference, a token-shaped
run): the start proceeds without it. `executor` is absent for a held start, which drives nothing.

**Old journals replay unchanged.** None of the three is `required` and the Rust side carries
`skip_serializing_if`, so a declaration written before this entry re-serializes to the same bytes
and keeps its hash. Absent means "written before the briefing existed", never "unnamed".

**Old-to-new only.** A journal that RECORDED any of the three is refused on read by a pre-#1063
binary (`additionalProperties: false` here, `deny_unknown_fields` on the Rust payload), as with
every additive event change under D-037's in-place correction of the pre-release baseline.

## event-envelope 1.0.0 - `memory_publication_transitioned` and `memory_record_superseded` added

Two durable events for the two lifecycle axes ADR-032 (decision 3) keeps independent: a memory
record's PUBLICATION state (`unpublished`/`proposed`/`published`/`withdrawn`) and its SEMANTIC
state (`candidate`/`validated`/`contradicted`/`deprecated`/`expired`). Both mirror `#488`'s shipped
shape for `memory_admission_refused`: a closed enum variant, opaque ids, no candidate content, no
provider output, no secret.

**`memoryPublicationTransitioned`** carries `recordId`, the `transition` applied
(`propose`/`publish`/`withdraw`) and the `resultingState`. It never touches the semantic axis --
carrying a field this event cannot legitimately change would be exactly the wrong attribute on the
wrong event.

**`memoryRecordSuperseded`** is a relationship between two records, not a transition on one: it
moves the PREDECESSOR's semantic axis to the reason's target (`contradicted`/`deprecated`) and
records the relationship on the SUCCESSOR. Neither record's publication axis is touched.

**Why this entry exists when `#488`'s did not.** `memory_admission_refused` landed without a
CHANGELOG entry, which is itself worth naming rather than silently repeating: this file's own
history is not as complete as later review assumed it was, and this is the point where that gets
corrected rather than compounded.

## event-envelope 1.0.0 - the four customs events added

`completion_claimed`, `completion_cleared`, `completion_rejected` and `completion_refused`, each
with a payload `$defs` entry, a branch in the `eventKind` union and an execution-scoped row in the
envelope's `oneOf`. Additive: every document valid under the previous bytes is still valid, and no
existing kind changed shape.

Also declared here, ahead of their Rust variants and deliberately:
`clearance_identity_registered` and `clearance_identity_revoked`. See the note on
`clearanceIdentityRegistered` in the schema itself for why, and for when that stops being true.

**`overdue_exception` and `sweep_performed` are NOT here, and this line exists because they once
were.** They were declared in this branch and removed before it landed: both were also declared,
with INCOMPATIBLE payloads, by the sweep lane (#162), and since `validate_envelope` runs on READ a
journal written under one shape would have failed replay under the other, permanently. The sweep
owns the sweep's events; this lane kept only the reckoning (`overdue_at`).

**Why this file is what makes an event real.** `validate_envelope` runs every appended event
through this schema and refuses anything it does not describe, so a variant on the Rust enum with
no entry here cannot be written to a journal at all. The typed enum, the fold and its guards were
all in place while the store rejected all four — the declaration below is the part that made them
appendable.

**`clearanceVerifier` is tagged, and its fields are camelCase for a reason.** The Rust enum
carried `rename_all = "camelCase"`, which renames variants and NOT the fields inside a struct
variant; `manifest_hash` and `key_fingerprint` were therefore heading for the wire in snake_case,
alone among every payload in the family. The type gained `rename_all_fields` rather than this
schema gaining a snake_case spelling: a wire format is only free to correct before anything has
published it.

## node 1.0.0 - `completion.customs` added

What a completion CLAIM must present, and how long each customs stage may park:
`proofKinds` plus `budgets.{waitWithinSeconds, clearanceWithinSeconds, dlqWithinSeconds?}`.

**Why nested inside `completion` rather than a sibling key.** `completion` already existed on
nodes with a different meaning — a completion CONTRACT (`requires`/`forbids`), declared in this
schema, carried by three checked-in example graphs, and consumed by the governor's content
externalizer. Customs is the same question one layer down: `requires` says what makes the node
complete, customs says what a claim of completion must PROVE and how long each stage may wait.
Nesting keeps that parentage, stays additive (nothing existing changes meaning or validity), and
leaves a future unification of `requires` with `proofKinds` a local refactor instead of a
schema migration. Making it a sibling would have created two confusable top-level keys about
completing.

**Why strict inside and permissive outside.** `customs` sets `additionalProperties: false` and
requires both stage budgets, because a misspelled budget name that silently defaults reads
exactly like a stage with infinite patience — which is the parked-forever failure this milestone
exists to end. The surrounding block keeps the permissiveness it has always had: tightening
`requires`/`forbids` would change the validity of graphs that work today, and that is a separate
decision for whoever owns that field.

**Why `dlqWithinSeconds` is optional.** Absent means dead-letter occupancy raises no time
exception. The dead-letter state IS the exception — something already fired to route work there —
so a second timer on it is escalation policy, not a default anyone chose.

## event-envelope 1.0.0 - `wakeLeaseConsumed.capturedArming` added

Which arming a consumption was FOR: the sequence of the `wake_lease` event the sweep read when
it built its capture.

**Why the captured side and not the live one.** The defect this exists for is a mismatch between
what a sweep captured and what was live when it recorded — a sleeper wakes, re-arms on the same
rendezvous, and a delayed sweep burns the lease it just armed. Replay already knows the live
side; it rebuilds it from the arming events. Only the captured side was ever unknown to the log.
Recording the live one would compare a value against itself: always equal, a check that cannot
fail. The two versions are one field of the same name and type, and nothing in a diff
distinguishes them, which is why this note exists.

**Why optional, and omitted rather than null.** Every event is re-hashed on replay. A
`"capturedArming": null` on consumptions written before this field existed would change their
canonical bytes and break the hash chain of every one of them — the same reasoning that governs
`nodeOutcomeRecorded.reason`. It is absent from `required`, so committed events stay valid, and
`skip_serializing_if` keeps absence off the wire.

**What actually enforces that, so whoever loosens it knows what they are unguarding.** The live
guard is THIS SCHEMA'S `"type": "integer"`: a null is not an integer, so validation rejects the
append before anything downstream sees it. Measured rather than reasoned — removing
`skip_serializing_if` fells four tests in `core/events/tests/execution_projection.rs`, every one
of them at its own append. Relaxing that type is therefore not a cosmetic edit: it removes the
only thing standing between an absent value and a broken hash chain across all committed
consumptions. Loosening it is a deliberate act that passes through the digest and catalog ritual,
which is where this sentence is meant to be met.

**Where we stopped measuring, stated rather than implied.** The compound state — schema loosened
AND `skip_serializing_if` removed — is EXPECTED-UNMEASURED. Its direct backstop is a wire-absence
assertion in `a_matching_consumption_and_a_pre_change_one_record_no_mis_burn`, which is dormant
while the schema holds and becomes a single-sabotage blade the moment the type is relaxed; the
test carries its own note saying it is currently redundant and why. Behind that, the fixture
journal's integrity verification should refuse a struct that emits null, since it re-hashes every
committed consumption — but that is a READING, not a measurement, and it is labelled as one.
Reaching the compound state takes two deliberate changes, each individually guarded and each
meeting a written warning, and we drew the line there on purpose. Saying so beats pretending the
line does not exist.

**What absence means, permanently.** No consumption committed before this change carries the
captured side, so no future analysis can decide whether this defect ever fired in the past.
Replay can say which lease was burned; nothing can say which one the sweep meant to burn, and
the discrepancy between them IS the defect. "Precondition present, incident not observed" is the
strongest claim the old data can ever support — not because nobody has looked hard enough, but
because the log recorded one side of a two-sided property.

**What the fold does with a mismatch: records it, refuses nothing.** A consumption naming an
arming other than the one it burns is a wrong action FAITHFULLY RECORDED, which is a different
thing from the log the fold already refuses — a consumption with no live lease at all, which
cannot be interpreted. Refusing here would make history unreadable because it recorded something
bad, on a product whose thesis is that history reproduces, and it would brick the glance at the
moment an operator most needs it. The mismatch lands in the projection's `wakeMisBurns` instead,
where the attention verdict can reach it. Recorded but not yet surfaced: the wiring to attention
is seeded, not shipped here.

## event-envelope 1.0.0 - `wakeLease.maturesInSeconds` added

How long a sleeper's quiet may last, declared at arming and carried on the lease that already
records the sleeper's intent to be woken.

**Why a field on `wake_lease` rather than an expiry event of its own.** Whether the horizon has
passed is DERIVABLE from the horizon and the current instant, and an event whose entire content
is a derivable fact is a cached counter living inside the log — the same defect this milestone
is otherwise removing from outside it. The lease is also already excluded from the doorbell's
content predicate BY CONSTRUCTION, so arming an alarm cannot inflate the `contentHead` an
operator reads as progress. A new event kind would have had to be added to that exclusion by
hand, and a rule that must be remembered for every future kind is a rule one of them will
forget.

**Why a duration on the wire and an instant in the projection.** The first draft carried the
absolute instant, on the reasoning that a duration puts the horizon in the reader's hands. The
guard for it could not be written honestly: the instant would have been computed from a clock
reading microseconds away from the one that stamps the event, so the only assertion available
was "roughly now plus N" — and roughly-now passes for an implementation that measured from the
wrong base, which is the thing in question. The fold derives the horizon as `occurred_at +
maturesInSeconds`, arithmetic over the event's OWN recorded instant. No clock is read, replay
stays byte-identical, the reader is still handed an instant, and there is exactly one clock in
the story instead of two microseconds apart.

**The stored instant is a timestamp, not its wire string.** The canonical rendering emits 0, 3,
6 or 9 fractional digits depending on the value, so `...:01.500Z` sorts BEFORE `...:01Z` — `.`
is 0x2E and `Z` is 0x5A. A projection holding the string would answer ordering questions
backwards for the commonest pair there is, since the horizon inherits the fraction of whatever
instant stamped the arming event.

**It is stored, never evaluated below the surface.** Maturity is asked at the surface with an
injected instant. A projection that cached `matured` would be a function of the wall clock, and
byte-identical replay would fail only on the machines whose clock crossed the horizon
mid-replay — the quietest possible way to lose the property the event store exists for.

**Bounded at BOTH ends, and the loose end was the dangerous one.** A declared bound of zero is
not a bound and is refused. So is one beyond ten years: a trillion seconds produced a horizon in
the year 33715, and a surface that answers with a DATE reads as a promise while meaning never —
absence laundered into calm through arithmetic. Past `i64`, the conversion overflowed and
yielded no horizon at all, so an operator who declared a bound silently received none. The
ceiling is `315576000`, the one `nodeTimeoutSeconds` and `observedSilenceSeconds` already use
for a declared duration, so the answer is the house's existing one rather than a number invented
here.

**Absent means absent.** A lease armed without a bound gets no expiry promise, and none is
invented for it. Existing leases in committed journals replay unchanged.

## event-envelope 1.0.0 - `execution_form_amended` added

A silence bound the operator declares AFTER a run began.

**Why a separate event rather than a new field on `execution_form_declared`.** The
declaration is the shape a run started with; the amendment is a decision made later. Folding
the second into the first would erase the boundary between them, and that boundary is the
whole point: an amendment binds FORWARD, from its own sequence. A replay positioned before it
still answers with what was known then, so the timeline reads "not judged for twelve minutes,
judged from here" rather than "it was fine all along". Nothing an operator declares can make
the history claim it knew something it did not.

For the same reason the projection keeps amendments as a LIST in log order rather than
folding them into one map. A folded map cannot be un-folded to an earlier moment, so the past
would silently inherit a decision taken after it.

**`computedAtSequence`** names the frontier the amendment was computed against, so an
amendment that cannot be placed in the history is refusable. It exists before the submission
path does, precisely so that path can refuse rather than guess.

**`observedSilenceSeconds`** records what the operator was looking at when they decided, so a
later reader sees the decision in its own light rather than in hindsight's.

**Old histories keep reading.** A stream with no amendment folds exactly as before and its
hash chain is untouched: the projection field carries `skip_serializing_if`, so nothing new
is serialized for streams that never amended.

## event-envelope 1.0.0 - `execution_form_declared` added

A new replay-safe event carrying the shape the operator DECLARED for an execution: the node
set, and the deadline each node declared. It is recorded WITHOUT a seal, and that is the whole
point of it being a separate event rather than another `graph_version_published`. A seal exists
to protect the evidence behind content slots; a declared shape carries no evidence to protect.
The two ideas had been fused into one type, which forced every rule that merely needs the shape
to demand a credential it has no use for.

`nodeTimeoutSeconds` holds an entry ONLY for a node that declared a deadline. An absent key
means the operator declared nothing, and must never be read as a budget of zero - zero would
make every undeclared node look permanently overdue.

Histories written before this event existed hold an `execution_started` with no declaration and
keep replaying unchanged; the projection reads their missing declaration as UNDECLARED. That is
an honest unknown, not a clean bill of health.


## [1.0.0] - 2026-08-09

- Prepared the initial pre-release `1.0.0` baseline with 15 authoring and safe persistence contracts; this package has not yet been publicly published.
- Added the bounded `PersistedGraphVersion`, safe event envelope, encrypted Evidence metadata, artifact reference, repository scope, and sensitivity contracts.
- Corrected the bounded policy waiver contract while preserving the graph, agent, node, and edge authoring contracts.
- Corrected the single pre-release `1.0.0` baseline in place under D-037/ADR-023 by registering the typed `context_path`, `permission_path`, and `isolation_path` content positions. This is not a published-version migration and has no legacy alias or intermediate release.
- Canonicalized persistence timestamps to uppercase UTC `Z`, four-digit years, and at most nine fractional digits so schema validation and Rust round-trips remain exact.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the OPTIONAL `reason` property to `nodeOutcomeRecorded` in `event-envelope.schema.json`, carrying a closed `nodeOutcomeReason` vocabulary (the §17 route classes plus the in-process causes: empty reply, malformed judgment, judge and gate refusals, the four tool dispositions, and fixture-scripted outcomes), closing the gap where a failed node recorded no actionable cause. The property is deliberately NOT `required` and is omitted when absent: replay re-serializes each envelope and recomputes its hash against the stored one, so an always-emitted `null` would break the hash chain of every event written before this milestone.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the OPTIONAL `timeoutSeconds` property to the persisted `node` in `persisted-graph-version.schema.json`. The graph already accepted a per-node timeout and the linter already warned when an executable node omitted it (`GHG101_DEFAULT_TIMEOUT`); persistence dropped it, so nothing downstream could derive a silence budget from a bound the user had already declared. The property is deliberately NOT `required` and is omitted when absent: every graph version already published must keep validating and keep its hash, and absence must stay ABSENCE — never zero, never a default — because downstream it has to read as unknown rather than as calm.
- No predecessor release or persistence-format migration exists for this initial baseline.
- Preserved the provisional `p50.dev` schema IDs and wire-format identifiers.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `ghost` node state to `event-envelope.schema.json`'s `nodeState` enum, closing a gap where `graphhelm_protocols::NodeState::Ghost` could not be represented in a `NodeStateChanged` event on the wire.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, and `execution_completed` event kinds to `event-envelope.schema.json`, closing a gap where the durable execution wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `signal_recorded`, `ghost_node_proposed`, and `mutation_accepted` event kinds to `event-envelope.schema.json`, closing a gap where the in-flight governance wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_paused` and `execution_resumed` event kinds and widening the `nodeOutcome` and `simulationStatus` enums with `paused`, `interrupted`, and `cancelled` in `event-envelope.schema.json`, closing a gap where the pause, resume, and cancel lifecycle had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by bounding `completion.customs.budgets.{waitWithinSeconds,clearanceWithinSeconds,dlqWithinSeconds}` in `node.schema.json` to 315,576,000 seconds (ten years), matching the existing house limit used by persisted customs budgets. This makes an oversized authoring value fail at schema validation instead of passing authoring and failing later at persistence; it does not change optionality or add a tri-state runtime outcome.
