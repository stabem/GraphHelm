# GraphHelm Safe Persistence Projection Design

**Status:** Accepted architecture baseline under D-036/D-037 and ADR-022/ADR-023
**Date:** 2026-08-09  
**Issue:** #5 — Production Event/Evidence Store  
**Supersedes:** the raw `GraphVersionRecord` persistence and legacy-format assumptions in the earlier Milestone 03 design and plan

## 1. Decision summary

GraphHelm will not persist Foundation domain records verbatim. The Foundation authoring model contains free-form execution content, including agent instructions, objectives, purposes, textual completion contracts, dynamic diagnostic details, and source file paths. Persisting those fields in an immutable append-only journal conflicts with ADR-021, authorized erasure, and the repository rule that prompts, outputs, logs, credentials, environment values, paths, and raw tool content never enter durable event payloads.

The production and local persistence boundary therefore uses an explicit safe projection:

1. users continue authoring normal Graph DSL with inline content;
2. the Graph Governor deterministically externalizes content-bearing fields to encrypted Evidence before publication;
3. the Event Journal stores bounded topology, hashes, state transitions, and typed references only;
4. replay reconstructs canonical state and content availability without decrypting Evidence;
5. an erased or unavailable content slot prevents re-execution but does not falsify history;
6. JSONL and PostgreSQL use the same new wire contract;
7. pre-release legacy envelopes, importers, fixtures, commands, and compatibility tasks are removed rather than supported.

This is a pre-release contract correction. The checked-in schema baseline is rebuilt as the first public `1.0.0`; the intermediate local `1.1.0` package is removed. Git history and the accepted ADR record why the earlier internal contract was replaced.

## 2. Constitutional resolution

The governing invariants are:

- only the Graph Governor publishes operational graph mutations;
- Graph Versions remain immutable, monotonically versioned, canonicalizable, and hash-linked;
- Event Journal records are append-only and replay-safe;
- sensitive or free-form content is encrypted, separately retained, and erasable;
- projections rebuild from journal records even when referenced content is unavailable;
- secrets, prompts, outputs, logs, raw tool results, credentials, environment values, filesystem paths, and crash details never enter event payloads;
- deterministic code, not an LLM, identifies externalizable fields and validates the projection.

Raw Foundation parity is intentionally not a persistence invariant. The invariant is deterministic translation from a schema-valid authoring graph to a schema-valid safe projection. Foundation input schemas remain authoring contracts; persistence schemas define a narrower durable representation.

## 3. Approaches considered

### 3.1 Selected: Governor externalization plus inline safe topology

The Governor accepts inline authoring content, seals it as Evidence, builds a safe topology projection, and atomically publishes references and events. This preserves authoring usability, deterministic replay, erasure, and inspection of non-sensitive topology.

### 3.2 Rejected: reference-only Graph DSL

Requiring users or clients to create Evidence references before authoring would simplify persistence but expose storage concerns at the product boundary and make the DSL unnecessarily difficult to use.

### 3.3 Rejected: encrypted full snapshot with minimal event

Storing the complete graph only as encrypted Evidence would make topology inspection, replay, projection rebuild, and integrity verification depend too heavily on content availability. Erasure would obscure more state than necessary.

## 4. Domain boundaries

The implementation separates four representations:

1. **AuthoringGraph** — schema-valid user input that may contain inline content.
2. **PreparedEvidence** — bounded plaintext held only during preparation, then sealed by `KeyProvider`-mediated encryption.
3. **PersistedGraphVersion** — safe, immutable topology plus typed content slots and semantic digests.
4. **ExecutableGraphMaterialization** — an ephemeral reconstruction created only when all required Evidence is available and authorized.

`PersistedGraphVersion` is not a Serde alias for the existing Foundation `GraphVersionRecord`. Conversions are explicit and fallible. No public constructor permits callers to bypass externalization by directly embedding an authoring graph in a persistence event.

## 5. Persisted graph version

A persisted graph version contains:

- graph/version identity;
- predecessor version and semantic hash;
- creator actor and creation timestamp;
- `topologyHash`;
- `semanticHash`;
- bounded inline topology;
- an ordered list of typed `contentSlots`.

It never contains authoring plaintext.

### 5.1 Inline topology

The inline topology contains only data required to replay deterministic state:

- API and graph kind identifiers;
- graph ID, execution ID, version, predecessor identity, and safe typed labels;
- entrypoint IDs;
- node IDs, node types, optionality, structural capabilities, tool IDs, schema digests, and other registered deterministic controls;
- edge IDs, endpoints, edge types, priority, bindings, and registered structural conditions;
- budgets and bounded policy/control identifiers;
- structural completion rules that contain no human-authored prose;
- content-slot bindings.

Structural JSON Schema fragments are canonicalized with non-semantic annotations such as descriptions, titles, examples, and free-form comments removed. Unknown free-form objects are not copied into the journal. A future extension must register a bounded schema and pass the same forbidden-content checks before it becomes persistence-safe.

### 5.2 Content-bearing fields

The following are externalized by deterministic typed paths, including equivalent future registered fields:

- graph display names and descriptive metadata;
- node objectives;
- agent purpose and instructions;
- textual completion contracts;
- free-form policy explanations or rule text;
- dynamic diagnostic details;
- any unregistered human-authored execution string;
- any value matched by the deterministic secret/content scanner.
- path strings in registered context source-scope entries (`ContextPath`);
- path strings in registered permission scopes (`PermissionPath`);
- path strings in registered isolation filesystem writable paths (`IsolationPath`).

No LLM participates in this classification.

ADR-023 supersedes the former closed eight-kind list. These three added positions make the closed enum eleven kinds; they are not aliases for another content kind. Each path is Evidence-backed, retains its typed authoring order, has `restricted` sensitivity, is required for execution, and participates in topology/semantic slot hashing. Its plaintext is never copied into topology, events, logs, diagnostics, or errors.

### 5.3 Content slots

Each content slot contains:

- `slotId`, deterministically derived from the typed owner, owner ID, field kind, and ordinal;
- closed `ownerKind` and `fieldKind` enums;
- bounded owner ID and ordinal;
- `evidenceId`;
- raw lowercase `contentSha256`;
- closed `Sensitivity`;
- required/optional execution semantics.

Slots are sorted by `(ownerKind, ownerId, fieldKind, ordinal)`. The envelope's `evidenceRefs` must be a one-to-one ordered match with slot evidence IDs. Duplicate, dangling, missing, differently scoped, or differently digested references fail before append.

The Evidence identifier and encryption metadata do not participate in semantic identity. They may change when the same content is re-encrypted.

## 6. Hashing

`topologyHash` is SHA-256 over canonical safe topology with content slots represented only by their typed positions, not their Evidence identifiers or content digests.

`semanticHash` is SHA-256 over:

1. the canonical safe topology; and
2. the ordered tuple `(ownerKind, ownerId, fieldKind, ordinal, contentSha256)` for every slot.

Consequences:

- the same authored graph and content produce the same semantic hash after re-encryption;
- nonce, ciphertext, wrapped key, key handle, and Evidence ID never affect graph identity;
- changing instructions or another content field changes `semanticHash`;
- layout/UI remains excluded;
- an unavailable Evidence item does not change historical hashes.

This pre-release correction intentionally replaces the earlier Foundation semantic-hash projection. Existing golden hashes and developer data are regenerated from the safe projection; no runtime translation or compatibility claim is retained for the superseded hashes.

## 7. Governor publication flow

Publication is one deterministic workflow:

1. load and schema-validate the authoring Graph DSL;
2. apply semantic lint and policy evaluation;
3. identify content-bearing fields through registered typed paths;
4. canonicalize each content value and calculate its plaintext digest;
5. seal each value as Evidence;
6. build safe topology and ordered content slots;
7. validate graph invariants, schema contracts, scope, reference bijection, limits, and hashes;
8. persist Evidence, references, artifacts, and the event batch;
9. update the active graph version only after durable append succeeds.

Any failure leaves the prior active version unchanged. Plaintext is never included in an error, log, diagnostic event, temporary filename, or crash message.

## 8. Transaction and crash consistency

### 8.1 PostgreSQL

Encrypted Evidence rows, content references, graph projection records, and journal events commit in one PostgreSQL transaction. External key wrapping occurs through an idempotent `KeyProvider` preparation step. Failed database transactions never make a graph version active. Provider-side preparation that outlives a failed transaction is reconciled by bounded idempotent cleanup.

### 8.2 Local JSONL

The local implementation uses a single locked repository directory and the same wire format as PostgreSQL:

1. write encrypted Evidence to same-volume temporary files;
2. flush and durable-sync each file;
3. publish blobs with atomic no-replace operations;
4. append and durable-sync the journal batch last;
5. publish the active-version marker last.

A crash may leave an unreachable encrypted Evidence blob, but it must never leave a committed event that references missing Evidence. A bounded reconciler removes unreachable blobs after verifying the journal and active transaction markers.

## 9. Replay, execution, and erasure

Replay consumes only the safe journal projection. It reconstructs:

- graph/version identities and predecessor links;
- topology and deterministic controls;
- semantic and topology hashes;
- typed content slots and their availability states;
- policy, waiver, retention, and erasure receipts.

Replay never requires Evidence plaintext. If Evidence is erased, expired, missing, or fails integrity verification, the projection remains valid and exposes the typed unavailable reason.

Executable materialization requires every slot marked as execution-required to be available and authorized. If any required slot is unavailable, execution fails closed with a stable diagnostic and no model/tool call. Historical state remains queryable.

## 10. Safe diagnostics

Persistence events do not reuse Foundation `Diagnostic` directly. A `PersistedDiagnostic` contains only:

- stable diagnostic code;
- severity;
- bounded `DiagnosticDomainPath`, represented as a JSON Pointer whose complete token sequence matches a registered GraphHelm contract shape;
- closed component identifier;
- optional source-content digest;
- optional Evidence reference for authorized detail.

It contains no free-form dynamic message, source filename, filesystem path, URI, temporary path, or arbitrary caller-supplied source string. User-facing text is resolved from stable codes or retrieved through authorized Evidence when detailed context is necessary.

The empty JSON Pointer is allowed when no safe structural location exists. Non-empty pointers are validated across their complete decoded token sequence against a closed union of registered shapes from the authoring graph (`apiVersion`, `kind`, `metadata`, `spec`), safe graph projection (`number`, `predecessor`, `topology`, `topologyHash`, `semanticHash`, `contentSlots`, `createdBy`, `createdAt`), production event envelope, Evidence record, artifact reference, repository scope, and policy-waiver contracts. Each structural field is closed at its position, scalar fields reject descendants, arrays accept only canonical bounded decimal indices, and map positions use the exact owning grammar such as `OpaqueId` or `SafeKey`. RFC 6901 `~0`/`~1` decoding happens before nominal validation. Safety follows from the positive contract grammar, not a filename/extension denylist; structurally impossible source, filesystem, URI, empty-token, and traversal shapes therefore fail closed. This typed boundary is intentionally narrower than the schema's structural JSON Pointer pattern, and a future field requires an explicit grammar update.

Foundation conversion is explicit and fallible. A producer maps only a known contract location into `DiagnosticDomainPath`; it never copies `Diagnostic.path` or `Diagnostic.source`. If no safe mapping exists it uses the empty root, externalizes authorized detail as Evidence, or fails closed. There is no automatic `From<Diagnostic>` conversion.

## 11. Policy waiver baseline

Because the product and schema package have not been publicly released, the baseline `policy-waiver` schema is corrected before the first public `1.0.0` rather than preserved as an artificial legacy contract.

The bounded contract requires:

- non-empty typed IDs using the repository opaque-ID grammar;
- graph version within the cross-platform safe integer range;
- optional `reason` omitted when absent, never `null`, and bounded when present;
- one to 64 acknowledged risks, each non-empty and bounded;
- closed waiver scope;
- canonical RFC 3339 UTC timestamps with uppercase `T`, terminal uppercase `Z`, a four-digit year `0000..9999`, valid end-of-day leap seconds, and at most nine fractional digits;
- no unknown fields.

Rust serialization uses `skip_serializing_if = Option::is_none` for the optional reason.

All persistence schemas use this same UTC-owned timestamp profile. Numeric offsets and lowercase `t`/`z` are rejected at the durable boundary: offset normalization at the four-digit year limits can produce `-0001` or `+10000`, and fractional precision beyond nine digits cannot round-trip through Chrono without loss. Authoring contracts are unaffected.

## 12. No legacy compatibility layer

GraphHelm is still pre-release. The internal formats discovered to be unsafe are removed rather than supported.

The implementation therefore removes:

- `LegacyEventEnvelope`, `LegacyStoredBatch`, and `LegacyEventImporter`;
- legacy import context, commands, events, fixtures, and conformance cases;
- Task 9's legacy migration work;
- dual readers, dual writers, automatic migration, and fallback branches;
- the intermediate schema release `1.1.0`.

JSONL and PostgreSQL read and write only the safe baseline. Existing developer test data must be deleted and recreated. Unknown repository formats fail with a generic unsupported-format diagnostic; they are never interpreted heuristically.

Documentation records that this replacement occurred before the first public release. Git history remains the audit trail; runtime compatibility code does not.

## 13. Schema release reset

The repository publishes one reviewed baseline:

- aggregate release `1.0.0`;
- corrected existing schemas, including bounded `policy-waiver`;
- the persistence schemas required by this milestone;
- exact root/snapshot byte parity;
- strict catalog and conformance inventories;
- no migration package because there is no supported predecessor release.

All provisional `p50.dev` identifiers remain unchanged unless the corrected representation requires a new document name. Document versions and catalog hashes are regenerated from canonical schema bytes.

## 14. Error handling

Stable failures distinguish:

- invalid authoring input;
- content externalization failure;
- Evidence sealing or key-provider failure;
- projection/schema mismatch;
- reference bijection failure;
- scope or concurrency conflict;
- durable append failure;
- required content unavailable;
- unsupported repository format;
- integrity failure.

Errors expose codes, registered domain JSON Pointers, and redacted component identifiers. They never expose plaintext, credentials, source/filesystem paths, ciphertext, wrapped keys, or unrelated system details.

## 15. Verification strategy

Acceptance requires deterministic RED-to-GREEN tests for:

1. all official example graphs publishing through the real Governor/externalizer;
2. zero known plaintext from authoring content in serialized journal bytes;
3. exact Evidence decrypt round-trip before erasure;
4. topology and semantic hash determinism across re-encryption;
5. semantic-hash change when content changes;
6. slot/reference ordering and one-to-one correspondence;
7. rejection of missing, duplicate, cross-scope, or mismatched references;
8. replay with Evidence available, erased, expired, missing, and corrupt;
9. execution blocked when a required slot is unavailable;
10. local crash points before blob publication, before journal append, after append, and before active marker publication;
11. PostgreSQL transaction rollback and key-provider reconciliation;
12. explicit/fallible diagnostic conversion from relative and absolute source paths into a registered domain JSON Pointer or the empty root, never by copying the source path;
13. bounded waiver fields and omitted optional reason;
14. adversarial prompts, paths, secrets, logs, output, credentials, environment values, and raw tool payloads at every nesting level;
15. absence of legacy symbols, commands, fixtures, conformance cases, schemas, and runtime branches from source/runtime surfaces; decision documentation may name the removed internal types to explain the pre-release correction;
16. a single strict `1.0.0` schema inventory with root/snapshot parity;
17. identical JSON wire behavior across local JSONL and PostgreSQL adapters;
18. full workspace formatting, lint, tests, CLI smoke, metadata, and diff checks on Windows and Linux.

## 16. Plan impact

The Milestone 03 implementation plan must be revised before code resumes:

- add an accepted ADR/decision-register entry for the safe persistence projection and pre-release baseline reset;
- replace raw `GraphVersionRecord`, Foundation `Diagnostic`, and embedded waiver persistence assumptions;
- update schema publication around one `1.0.0` baseline;
- make Governor externalization precede all event publication;
- evolve local JSONL immediately instead of keeping a legacy reader;
- remove the legacy-import task and redistribute its verification budget to projection/externalization tests;
- keep PostgreSQL, retention, projections, backup/restore, CLI, and CI tasks, but bind them to the safe wire contract.

This written specification is accepted by D-036/D-037 and ADR-022/ADR-023. Implementation may proceed only through the revised plan, preserving its task gates and the safe persistence contract above.
