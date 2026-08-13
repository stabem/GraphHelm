# Task 5 report — Governor graph-content externalization

Date: 2026-08-10
Base: `07ee31d27708a55c4cf938de0f86562ef7669fdc`

## Status

Task 5 is implemented within its approved scope. The Governor now prepares a safe, deterministic `PersistedGraphVersion`, seals every registered authoring-content slot as Evidence, validates the exact slot/reference bijection, and returns `ProjectionPreparation` without writing events or activating state. Task 6 remains responsible for atomically switching the writers.

The only Task 4 prerequisite is the authorized public `ArtifactRegistrationError` contract and stable `GHEV004_EVIDENCE_INVALID` mapping. D-037 and ADR-023 intentionally changed the pre-release `ContentFieldKind` wire enum and rebuilt the persisted-schema `1.0.0` baseline in round 13a. Later Task 5 behavior rounds introduced no additional event/store format or compatibility layer; event writers, repository adapters, CLI, retention, migration, and external integrations remain unchanged.

## Fix round 24 - sealed offline retrieval and bounded schema traversal

Round 24 makes the offline boundary independent of dependency feature unification. Both in-memory registry preparation and validator options receive an explicit rejecting `jsonschema::Retrieve` implementation. It returns a constant private error without inspecting, opening or echoing the requested URI. HTTP(S), file URIs, relative external resources and arbitrary schemes therefore reach only the rejecting seam; local fragments and resources embedded in the same schema document remain available.

The inline preflight now uses a streaming borrowed cursor. One iterator frame is retained per active container, so pending work is O(depth), not O(width). Each value and object key is charged before descent, and the existing depth, value, aggregate key-byte and serialized-byte ceilings remain enforced before registry preparation or validator construction. The deliberately supported legacy `definitions` keyword is treated as a schema map by the positional identity canonicalizer: member names remain semantic, annotations within member schemas do not, and structural member changes alter the safe identity.

The former internal `catch_unwind` was removed. The bounded boundary and pinned compiler are invoked through fallible APIs, and dependency panic-freedom is an explicit library invariant. GraphHelm does not mutate the process-global panic hook and does not claim to suppress third-party panic output; callers may still isolate an unexpected dependency panic at their own process boundary.

### Round 24 RED -> GREEN and mutation evidence

- The `definitions` annotation probe first produced different persisted identities. Positional traversal made annotation-only changes invariant while `type: string` versus `type: integer` remains distinct. Omitting `definitions` from the schema-map arm reproduced the failure and was restored.
- The streaming peak test did not compile before instrumentation existed. GREEN records peak pending frames of one for 16,384 siblings and at most one per nesting level. Restoring width-proportional pending work made the peak 16,384; omitting the pre-descent value charge let the over-limit value array pass. Both mutations failed their focused regressions and were restored.
- The retriever canary did not compile before the two explicit seams existed. GREEN observes exactly one rejecting-retriever call at each registry and validator stage for SSH, HTTPS, file and relative references, while public results remain the redacted `Invalid`. Removing the registry retriever and removing the validator-options retriever independently produced zero canary calls; both mutations failed and were restored.
- No Cargo/lock/dependency, protocol, wire, schema JSON/catalog, fixture, example, golden, writer/store or Task 6 file changed.

### Round 24 verification

```text
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy -p graphhelm-schema -p graphhelm-governor -p graphhelm-graph --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-schema --all-features --locked
  21 passed; 0 failed; all doc tests passed
cargo +1.97.1 test -p graphhelm-governor --all-features --locked
  119 passed; 0 failed; all doc tests passed
cargo +1.97.1 test -p graphhelm-graph --all-features --locked
  59 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; workspace tests had zero failures; CLI smoke 8/8; metadata 10 members

cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked -- --nocapture
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked -- --nocapture
  passed; 31/31 catalog and 30/30 conformance tests

root and immutable release catalog, compatibility check and public conformance CLI
  passed; both catalogs contain the same 15 schemas at 1.0.0; unchanged compatibility; 50/50 cases

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-schema -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

all three official graph validate/lint/hash commands plus simulate/replay smoke
  passed; hashes remained unchanged and replay reconstructed completed state

git diff --check
  passed
```

## Fix round 23 — normative offline inline-schema compilation

Round 23 superseded the hand-written JSON Schema grammar introduced in round 22. The Governor delegates validity to the existing exact-pinned `jsonschema 0.49.2` Draft 2020-12 compiler owned by `graphhelm-schema`. A small public boundary accepts only a borrowed JSON value, performs deterministic pre-compilation bounds, builds an isolated in-memory registry, and returns only `Invalid` or `LimitExceeded`. Round 24 subsequently made both retrieval seams explicit. Library diagnostics, schema text, URIs and paths never cross that boundary. Local fragments and in-document `$defs` resolve; absent HTTP and relative resources fail closed during registry preparation/build.

The pre-compilation walk limits depth to 64, structural values to 32,768, aggregate key bytes to 512 KiB and serialized bytes to 1 MiB. Round 24 replaced its eager sibling stack with an O(depth) cursor and removed the internal unwind conversion. The bounded writer still runs before the registry/compiler. Boolean schemas and Draft 2020-12 edge cases accepted by the pinned implementation are preserved, including `enum: []`, `required: []`, `required: [""]`, empty dependency-property arrays, `format: ""`, `$ref: ""`, `$vocabulary: {}`, local `$defs` references, supported ECMA-262 lookbehind and integer-valued `1.0` constraints.

Governor compilation occurs before annotation removal, cloning for canonical identity, digest construction or sealing. The former manual type, cardinality, number, regex, media-type, URI, vocabulary and anchor gates were deleted. The remaining fallible canonicalizer does exactly one job: remove the eight registered annotations only at recognized Draft 2020-12 schema positions. It preserves schema-map names and treats `const`, `enum`, unknown/custom keywords and obsolete/unknown keywords as literal data. The round-22 statements describing a deliberate legacy grammar, tuple `items`, manual URI/regex parsing and nonempty arrays are therefore historical and superseded by this section.

### Round 23 RED → GREEN and mutation evidence

- The direct schema RED did not compile because the bounded public API and redacted result did not exist. GREEN accepts the normative positive matrix and rejects malformed annotation/type shapes, non-schema recognized members, empty combinators rejected by the pinned metaschema, invalid patterns and unavailable resources.
- The previous Governor grammar falsely rejected the accepted empty arrays/strings and integer-valued float matrix. GREEN compiles each through the normative boundary and retains custom/literal identity injectively; all malformed probes fail before the first sealer call.
- Removing the Governor compiler call let malformed `type: 7` reach the failure sealer (`calls = 1`). Permitting an absent HTTPS resource made the offline direct test return success. Reintroducing an empty-reference gate rejected valid `$ref: ""`.
- Descending into custom/literal objects collapsed distinct `definitions`/literal annotation lookalikes. Clearing schema maps collapsed distinct property names. Removing the precompile call let the 1 MiB oversized probe reach and pass the compiler. Each isolated mutation failed its focused regression and was restored.
- No Cargo/dependency/lock, protocol, wire, checked-in schema/catalog, fixture, example, golden, writer/store or Task 6 file changed. Official safe identities remain unchanged.

### Round 23 focused verification

```text
cargo +1.97.1 clippy -p graphhelm-schema -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-schema --all-features --locked
  19 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --all-features --locked
  118 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-graph --all-features --locked
  59 passed; 0 failed; all doc tests passed
```

### Round 23 full repository verification

```text
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  441 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
  27 passed; 0 failed

root/release schema catalog, compatibility check and conformance
  passed; 15 schemas, unchanged compatible baseline, 50/50 conformance

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-schema -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

all three official graph validate/lint/hash commands plus simulate/replay smoke
  passed; all authoring and safe golden hashes unchanged; replay reconstructed the completed terminal state

git diff --check
  passed
```

## Fix round 1 â€” durable content gate and path-sensitive annotations

The first independent Task 5 review found one Critical persistence bypass: `SafeValue` enforced only wire grammar, so secret-shaped labels and edge bindings could survive in clear durable topology. It also found that the global annotation-key list silently discarded `title`, `description`, `examples`, `source`, and `$comment` outside registered JSON Schema contexts, allowing authored changes to escape semantic identity.

Focused RED tests reproduced both defects before implementation. Secret-family tests reached the sealer instead of failing at the boundary, and `agent.ephemeral.description` produced a successful preparation with the same persisted identity. The correction adds one bounded, deterministic, allocation-conscious ASCII scanner over the complete authoring graph plus actor and repository scope before collector allocation, topology construction, or sealing. It recognizes GitHub/OpenAI/AWS/GitLab/Slack token families, JWT-shaped bearer material, Basic/Bearer authorization forms, PEM private-key markers, secret/environment credential references, and case/separator variants without regex, entropy scoring, network access, or input echo. Scanner work is bounded to 64 levels, 131,072 values, and 64 MiB of inspected string/key bytes.

Annotation handling is now path-specific. Only JSON Schema annotations inside the registered node `input` and `output` schema containers are stripped before their structural digest. Graph metadata `source`, `sourcePath`, and `ui`, plus node `ui`, remain explicit non-semantic contexts. Agent definitions, isolation objects, and other unknown extension locations reject annotations instead of silently dropping them. Completion is registered externalized content, so schema-like keys inside it change the semantic digest without appearing inline. `edge.payloadSchema` and agent input/output schema contracts are typed string references and remain structural digests rather than annotation containers.

The official manual-override graph exposed one scanner false positive during GREEN: `environment://staging` is a legitimate structural target reference. The gate now permits nominal environment locators and rejects `environment://` only when the suffix identifies credential material; `secret://`, `env://`, `vault://`, and `credential://` remain rejected. All official examples retain their reviewed Task 5 topology/semantic hashes.

Mutation checks removed the durable scanner call and separately reintroduced schema-annotation stripping under `agent.ephemeral`; the focused secret and context tests failed respectively, proving both gates are behaviorally protected. Separate review findings concerning publication-scoped Evidence IDs, complete sealer-output validation, predecessor overflow, normative-control reconstruction, and artifact/reference bijection remain intentionally deferred to the authorized later fix rounds.

## Fix round 2 â€” publication identity, sealed-output validation, and overflow

The second correction binds every Evidence ID and therefore every requested key handle to the immutable publication rather than only to a reusable typed slot. Externalization now completes in two deterministic phases: it collects, orders and hashes the safe topology/content positions first, then derives each Evidence ID from a length-prefixed, domain-separated tuple containing the exact repository scope, graph version number, safe semantic hash and typed slot identity. Nonce, ciphertext, wrapped-key metadata, clock values and map insertion order are excluded. Exact retry and re-encryption retain Evidence and slot IDs; a content, version or scope change cannot reuse the prior publication's Evidence identity.

The Governor now treats `EvidenceSealer` output as untrusted. Before a preparation can escape, it compares the returned Evidence ID, exact scope, media type, sensitivity, retention class, plaintext byte length and content digest with the request; recomputes the ciphertext digest over the exact returned bytes; verifies the key handle and algorithms; and reconstructs the Task 4 canonical Evidence AAD to verify the wrapped-key AAD digest. Any mismatch returns the existing redacted projection-integrity code with no preparation or active-state mutation.

Focused REDs reproduced the prior Evidence-ID collision for content, version and scope changes, accepted malformed sealer output, and both owning-graph and public-externalizer panics for a `u64::MAX` predecessor. GREEN uses checked successor arithmetic and returns the existing typed invalid-predecessor/externalization failure. The fault matrix covers ID, scope, media type, sensitivity, retention class, length, content digest, ciphertext digest, AAD digest and key handle. Mutation checks independently removed the scope comparison and ciphertext-digest comparison; each allowed its dedicated fault to produce a successful preparation and made the matrix fail, then the checks were restored and the matrix returned GREEN.

## Fix round 3 — reconstructible registered controls

The third correction replaces opaque structural digests with an explicit typed-path registry that can be reconstructed from public `PersistedControl` getters. The synthetic RED was schema-valid and exercised agent ref and ephemeral forms, indexed capabilities/tools/actions, schema and instruction references, agent model/context/evidence/memory controls, node model/input/output/context/permissions/isolation/retry/resources/memory controls, target/timeout/flags, edge bindings, payload schema, and structural condition metadata. The former implementation rejected nested controls and preserved agent/schema/target meaning only as one-way digests.

GREEN stores exact registered identifier, integer and flag meaning in deterministic maps. Arrays use bounded zero-padded indices and preserve order; schema-unique arrays remain schema-enforced while other arrays preserve duplicates. References outside `SafeValue` use the reversible domain-separated `refv1:` plus unpadded base64url encoding. Its allowlist is closed, the raw reference is capped at 91 ASCII bytes so the encoded value stays within the 128-byte wire bound, and the grammar rejects unknown schemes, filesystem/source shapes, traversal, whitespace/prose, credential schemes and secret-bearing environment references. A full `artifact://sha256/<64 lowercase hex>` locator round-trips without its raw URI appearing in serialized topology.

Unknown children and wrong scalar/container types fail closed for every registered object before sealing. Additional negative coverage rejects floats in integer-only controls, nulls, more than 64 items in one indexed array, more than 128 combined identifier entries, integers above the JSON safe range, overlong encodings, writable/source paths, secret-shaped values, unsafe URLs and nested human prose. Structural mutations to a formerly digest-only capability, retry integer or target reference change both topology and semantic hashes. Reordering authoring maps and re-encryption preserve both identities.

The dead Governor import/conversion for `ArtifactRegistrationError` was removed. Task 5 returns sealed Evidence and exact Evidence references only; it cannot register an artifact because it owns no artifact source bytes or media metadata. The public Task 4 error/code remains unchanged in `core/events` for Task 6's real append boundary.

Mutation checks removed capability persistence and then ignored an unknown retry child. The reconstruction test failed on the missing indexed capability, and the negative matrix reached the sealer instead of rejecting. Both mutations were restored and the focused suite returned GREEN.

## Fix round 4 — linear scanning and replay-safe references

The fourth correction removes the separator-driven quadratic path from the compact secret matcher. A deterministic operation-count reproduction measured 33,536 matcher operations for only 128 separator bytes in the old implementation. The replacement maintains one bounded state per fixed PEM marker and consumes each input byte once per marker; the regression covers 128 through 65,536 bytes and enforces a constant-times-input bound without wall-clock timing. Existing token, authorization, JWT, environment-secret and PEM detections, traversal budgets and pre-sealing failure behavior remain intact.

Reference handling is now owned by `core/graph::persistence`, not a Governor test helper. The production `refv1` codec accepts only a closed family grammar, emits unpadded base64url, and rejects bad prefixes, alphabet, padding, trailing bits, UTF-8, re-encoding mismatches, overlong values and noncanonical versions. Artifact locators are exactly `artifact://sha256/<64 lowercase hex>`; schema, context-policy, contract, rules, environment, project/builtin, context/document and the two supported JSON Schema identity URLs each have a bounded positive grammar. The normative nominal instruction selector `inline-or-artifact` remains an ordinary bounded `SafeValue` instead of being disguised as a reference.

The public safe-projection validator traverses all registered control and edge reference positions, requires canonical encodings at reference positions, permits the explicitly registered nominal selector, and rejects `refv1` at ordinary identifier positions. Governor preparation invokes it before any sealing result can be returned, so Task 6 replay can apply the same fail-closed validation to a directly deserialized `PersistedGraphVersion`.

The public externalizer bypass matrix now rejects bare filenames, relative/absolute/drive paths, traversal and dot forms, nested/empty/mixed-case schemes, empty identifiers, query/fragment/userinfo, percent-encoded traversal, secret-bearing environment suffixes, noncanonical artifacts and overlong values before the first sealer call without echoing input. This includes the reviewed `Cargo.lock`, `artifact:///...`, `schema://file://...`, and `environment://.../apiKey` classes.

Within registered input/output schema fragments, only `title`, `description`, `examples`, and `$comment` remain removable annotations. `$id` and `$schema` are preserved as canonical `schemaId` and `schemaDialect` references in the owning persisted contract, so either changes both topology and semantic identity; unsupported identities fail closed. Graph-level `metadata.annotations` remains explicitly excluded and does not change the prepared version.

Mutation checks broke separator-aware PEM detection, inverted the canonical decoder comparison, and removed `$id` persistence. The focused secret, codec round-trip, and schema-identity tests failed respectively; all mutations were restored before verification.

## RED → GREEN chronology

1. A focused events integration test first failed to compile because `ArtifactRegistrationError` was not publicly exported. The public export, stable code, and redacted `Display`/`Debug` behavior made the prerequisite GREEN without changing artifact digest/locator validation.
2. The graph projection tests first failed on absent canonical-content, dual-hash, and bijection APIs. The graph crate then gained canonical content bytes, raw SHA-256, topology/semantic persistence hashes, and exact ordered slot/reference validation. Focused graph tests became GREEN.
3. Official-example Governor tests first failed on absent externalization interfaces. Typed extraction, safe topology construction, sealing, stable bounds, and redacted errors made all three official examples GREEN with reviewed deterministic identities.
4. The Governor preparation test first failed because no isolated preparation phase existed. Schema, lint, policy, version/predecessor, and externalization checks were added before returning a preparation. Failure injection proved the base graph stays byte-identical.
5. Adversarial API review found the plan-fixed `ProjectionPreparation` fields were private. A focused integration test produced E0616 for all three fields; exposing the exact planned public fields made the contract GREEN.
6. The re-encryption independence test was refactored to use deterministic test-only rewrapping metadata. It proves changing sealing output does not affect persisted identity without relying on random inequality.

## Implemented contracts

- `GraphExternalizer` accepts an exact repository scope and immutable `GraphVersionRecord`; `SealingGraphExternalizer` depends only on the adapter-neutral `EvidenceSealer` interface.
- A closed registry extracts graph display/description/completion, node display/objective/description/completion, ephemeral-agent purpose/instructions/completion contract, edge condition/fallback policy, and graph policies. Registered structural agent/node/edge controls are reconstructible identifier/integer/flag maps; documented UI/source/schema annotations are excluded; unknown or arbitrary free-form fields fail closed.
- Content is canonical compact JSON. Slot IDs use the typed position encoding. Evidence IDs use a separate publication-scoped, domain-separated encoding over scope, immutable version/semantic identity and exact slot identity. Slots remain ordered by owner kind, owner ID, field kind, and ordinal.
- The topology hash covers canonical safe topology and slot positions but not content digests or encryption metadata. The semantic hash adds ordered content digests. Existing Foundation canonical hashes are unchanged.
- Bijection validation rejects missing, extra, reordered, duplicate, ID-mismatched, or digest-mismatched Evidence references.
- Limits execute before sealing: 8,192 content items, 16 MiB per canonical item, and 64 MiB aggregate. Item size is counted before allocating canonical bytes; aggregate overflow is checked before allocating the overflowing item.
- Public errors are stable and redacted: externalization/sealing uses `GHE009_EXTERNALIZATION_FAILED`, integrity uses `GHE005_INTEGRITY_FAILURE`, and bounds use `GHE006_LIMIT_EXCEEDED`.
- `prepare_draft_publication` constructs an isolated candidate, applies schema/lint/policy/override checks, links the successor to the predecessor's safe semantic hash, and returns only a preparation. It has no event-store dependency and cannot write or activate state.

## Contract note

The official `research-to-publish.yaml` example now contains the reviewed deploy target and is schema-valid with zero lint errors. The earlier `GHG009_DEPLOY_TARGET_MISSING` observation was superseded when the example gained `targetRef: environment://staging`; the historical correction and reviewed hash changes remain recorded below. Direct `GraphExternalizer::prepare` validates record integrity, schema, and scope, while the publication preparation phase still applies lint and policy before sealing. No lint rule was weakened.

## Verification

Fresh focused verification after the final implementation change:

```text
cargo +1.97.1 test -p graphhelm-events --locked
  20 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-graph --locked
  20 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  20 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-graph -p graphhelm-governor --all-targets --locked -- -D warnings
  passed with zero warnings
```

Fresh required workspace gates:

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  298 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

## Fix round 22 — closed inline JSON Schema keyword grammar

Inline schema identity now passes through one closed, auditable keyword grammar before annotation
removal or hashing. The supported core/reference vocabulary is `$id`, `$schema`, `$ref`,
`$dynamicRef`, `$anchor`, `$dynamicAnchor`, `$vocabulary`, and `$defs`; assertions are `type`,
`enum`, and `const`; numeric validation is `multipleOf`, `minimum`, `maximum`,
`exclusiveMinimum`, and `exclusiveMaximum`; string/content validation is `minLength`, `maxLength`,
`pattern`, `format`, `contentEncoding`, `contentMediaType`, and `contentSchema`; array validation is
`minItems`, `maxItems`, `uniqueItems`, `minContains`, `maxContains`, `prefixItems`, `items`,
`contains`, `additionalItems`, and `unevaluatedItems`; object validation is `minProperties`,
`maxProperties`, `required`, `properties`, `patternProperties`, `additionalProperties`,
`propertyNames`, `dependentRequired`, `dependentSchemas`, and `unevaluatedProperties`; applicators
are `allOf`, `anyOf`, `oneOf`, `not`, `if`, `then`, and `else`. Deliberate legacy compatibility is
limited to `definitions`, `dependencies`, tuple-form `items`, and `additionalItems`. The eight
annotation keywords remain `title`, `description`, `examples`, `$comment`, `default`, `deprecated`,
`readOnly`, and `writeOnly`.

Every recognized keyword is type-checked before recursion or stripping. Type arrays, enums,
`required`, `dependentRequired`, and legacy dependency lists enforce their deliberate nonempty and
uniqueness rules. Size constraints are non-negative JSON-safe integers; `multipleOf` is a finite
positive number; the other numeric limits are finite numbers. The grammar intentionally validates
keyword shapes, not schema satisfiability: contradictory bounds such as `minimum > maximum` or
`minLength > maxLength` remain valid schemas. Schema arrays/maps recursively contain only schemas,
and schema-array applicators that require a member reject empty arrays. Patterns and
`patternProperties` keys pass a bounded syntax parser for balanced classes/groups, the supported
non-capturing/lookaround forms, quantifiers, escapes, and ordered quantifier ranges. URI/reference
keywords accept bounded ASCII URI references with complete percent escapes; anchors use the closed
anchor token grammar; `$vocabulary` maps valid URI references to booleans. No reference is resolved
or fetched.

Unknown/custom keywords are explicitly literal: their complete values are preserved byte-
semantically, never traversed as schemas, and never stripped. Two custom values that differ only
inside an annotation-looking literal produce different structural digests and semantic hashes. This
is a deliberately supported grammar for safe projection identity, not a claim of complete JSON
Schema metaschema or ECMA-262 implementation.

### Round 22 RED → GREEN and mutation evidence

- The first RED reached the sealer for `type: 7`; the completed malformed matrix independently
  covers invalid/duplicate type and enum forms, all eight annotation type families, empty schema
  applicators, empty/blank/duplicate property lists, malformed schema maps/members, invalid regexes,
  negative/fractional sizes, invalid numeric shapes, and invalid URI/anchor/vocabulary forms. Every
  rejection is redacted `GHE009_EXTERNALIZATION_FAILED` with zero sealer calls.
- Boolean schemas and a single all-keywords positive schema cover the complete recognized set. A
  separate unsatisfiable-schema positive proves the boundary does not invent semantic consistency
  rules.
- Relaxing annotation types, the type gate, schema-array cardinality, string-list uniqueness, regex
  validation, non-negative integer validation, and URI grammar separately killed the exact matrix
  case. Replacing unknown/custom values with a common literal separately collapsed the custom-key
  digest pair. Every mutation was restored before fresh verification.

### Round 22 focused and repository verification

```text
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 test -p graphhelm-graph --locked
  59 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  118 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  438 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

schema catalog/check/conformance
  passed; 15 schemas, unchanged compatible baseline, 50/50 conformance

all three official graph validate/lint/hash commands plus simulate/replay smoke
  passed; authoring hashes unchanged, simulation completed and replay reconstructed the terminal state

git diff --check 07ee31d27708a55c4cf938de0f86562ef7669fdc
  passed; full Task 5 diff inspected
```

## Fix round 21 — strict schema shapes and complete record preflight

Inline JSON Schema identity now validates every registered schema position before stripping
annotations or hashing. A schema is exactly an object or boolean. Schema-map keywords require an
object whose members are schemas; single-schema keywords require one schema; and schema-array
keywords require an array whose members are schemas. `items` deliberately retains the already
supported legacy tuple form, but every tuple member is validated as a schema. Draft-dependent
`dependencies` requires an object and accepts only schema members or property-name arrays containing
strings. Literal/custom keyword values remain literal and are not reinterpreted. Boolean root and
nested schemas remain valid, while malformed fragments fail before the first sealer call and cannot
collide through annotation stripping.

The direct `GraphVersionRecord` boundary now uses one shared `PersistencePreflight` for the complete
typed record. `record.graph` is accounted by the same `account_execution_graph_usage` routine used
by base/candidate preflight; the canonical semantic projection is a distinct JSON `Value` rather
than a serializable `ExecutionGraph`, and is traversed completely through the same aggregate counter.
The record object, all six fixed keys, predecessor object or null, hashes, actor object/type/id and
canonical UTC timestamp are counted before `record.clone()`. The graph inventory retains explicit
typed containers for flattened metadata/node property maps in addition to their serialized members.
An independent Serde inventory proves fixed keys, strings, scalars, present/absent options and nested
values match exactly, with only those two documented typed-container additions. The exact 131,072
value boundary passes and one additional semantic value fails.

### Round 21 RED → GREEN and mutation evidence

- `not: []`, `properties: {x: []}`, an invalid `allOf` member and invalid `dependencies`
  shapes reached sealing before the correction. They now return redacted
  `GHE009_EXTERNALIZATION_FAILED` with zero sealer calls. Boolean schemas, object/boolean schema-map
  members, valid schema arrays, tuple `items`, and both dependency forms remain accepted.
- A direct record with 500 nodes and 128 null properties per node plus a deliberately stale semantic
  half returned the later generic authoring failure, proving clone/canonical reconstruction was
  reached. It now returns `GHE006_LIMIT_EXCEEDED`; an allocation-free post-preflight observer remains
  untouched and the sealer call count stays zero.
- Relaxing single-schema shape, schema-map member shape, and schema-array member shape independently
  made the focused schema matrix fail. Removing the typed record container, removing a fixed key and
  predecessor scalar, omitting the semantic half, and invoking the direct observer before preflight
  each failed its exact inventory or ordering regression. Every mutation was restored before fresh
  verification.

### Round 21 focused verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  59 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  116 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings
```

### Round 21 full repository verification

```text
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  436 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

schema root/release catalog, compatibility check and conformance
  passed; 15 schemas in both catalogs; unchanged compatible; 50/50 conformance

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

all three official graph validate/lint/hash commands plus simulate/replay smoke
  passed; no lint errors, reviewed authoring hashes unchanged, simulation completed and replay
  reconstructed the same terminal state

git diff --check 07ee31d27708a55c4cf938de0f86562ef7669fdc
  passed; full Task 5 diff inspected
```

## Fix round 20 — position-aware schema identity and exact key accounting

JSON Schema canonicalization now distinguishes schema positions from literal data. The one closed
eight-key annotation registry is consulted only at actual schema nodes. `properties`,
`patternProperties`, `$defs`, `definitions`, `dependentSchemas`, and draft-dependent
`dependencies` preserve every member name and descend only into the member schema. Registered
single-subschema keywords (`additionalProperties`, `additionalItems`, `unevaluatedProperties`,
`unevaluatedItems`, `propertyNames`, `contains`, `contentSchema`, `not`, `if`, `then`, `else`) and
schema collections (`allOf`, `anyOf`, `oneOf`, `prefixItems`, `items`) receive the same position-aware
walk. Values of `const`, `enum`, removed `examples`/`default`, and unknown/custom keywords remain
literal data and are never treated as implicit schemas. This preserves forward-compatible unknown
keyword identity instead of silently interpreting it.

The shared iterative `serde_json::Value` preflight routes scalar strings and object keys through the
same accounting primitive. Every key therefore contributes one aggregate value, receives the exact
16 MiB individual string bound, and contributes its bytes exactly once. A 65,536-member `key:null`
object now counts 131,073 values (container + 65,536 keys + 65,536 nulls) and fails the 131,072 limit;
a single 16 MiB + 1 key fails independently. Governor stage observation proves both failures occur
before `CandidateClone`, Serde, lint, policy, externalization, or sealing. The typed execution-graph
inventory now always counts the serialized `basedOn` key and exactly one value: the predecessor
string when present or the serialized `null` scalar when absent.

Replay no-panic coverage now spans foreign `from` and `to`, foreign self-loop and ordinary edge
shapes, no budget, and each of the seven supported persisted budget fields. All 32 combinations
return the same structured `InvalidProjection` under `catch_unwind`; endpoint-derived component
lookups remain fallible behind that front-door ordering. No protocol, schema JSON, catalog, fixture,
example, golden, writer, store, dependency, or Task 6 file changed. The schema changelog now calls
`1.0.0` the initial pre-release baseline rather than an already published package.

### Round 20 RED → GREEN and mutation evidence

- Before production changes, a property named `default` produced the same structural digest as an
  absent property. Equivalent collisions were reproduced for annotation-looking members of `$defs`,
  `definitions`, `patternProperties`, `dependentSchemas`, and `dependencies`, plus objects inside
  `const` and `enum`. All eight structural/literal pairs now have distinct contract digests and safe
  semantic hashes; the existing 8 × 2 annotation-keyword matrix remains identity-invariant at root
  and nested schema-node positions.
- The old walker returned `Ok(())` for both the 65,536-key object and the 16 MiB + 1 key. The same
  inputs reached `CandidateClone`, candidate serialization, lint, and policy in the Governor. GREEN
  returns `LimitExceeded` at the borrowed base preflight with no observed stage or sealer call.
- The `basedOn: null` boundary initially accepted a graph one value above the aggregate ceiling.
  GREEN accounts absent and present `basedOn` as the same key-plus-value width and rejects the exact
  boundary.
- Treating schema maps as ordinary schema nodes recreated the property-name collision. Descending
  through unknown/literal values recreated the `const.default` collision. Both focused tests failed
  and the position-aware branches were restored.
- Skipping accumulated key value-counting let the 65,536-key object pass. Keeping aggregate key
  bytes/count but skipping the per-key bound let the 16 MiB + 1 key pass. Both mutations were
  restored to the shared string-accounting path.
- Omitting the null scalar made absent `basedOn` one value narrower than present `basedOn` and failed
  the exact-width assertion. Replacing either fallible component lookup with direct map indexing
  panicked in the internal catch-unwind regression. Both mutations were restored.
- Exact key accounting also makes the historical eager whole-record probe exceed the aggregate
  value ceiling, as does the lazy path; its independent purpose remains intact because lazy root
  buffering peaks at one while the eager mutation still peaks above 100,000.

### Round 20 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  59 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  114 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 434 tests, 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

schema root/release catalog, compatibility check and conformance
  passed; 15 schemas in both catalogs; unchanged compatible; 50/50 conformance

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

git diff --check
  passed
```

## Fix round 19 — replay ordering and exact structural accounting

Replay now rejects unique-ID or endpoint corruption before building traversal maps, SCCs, reachability sets, or budget condensation. Every endpoint-derived component lookup is fallible and returns the same redacted `InvalidProjection`; bounded foreign-source, foreign-target, self-loop, reachability and `maxDepth` matrices cannot unwind.

Publication now preflights the borrowed base graph immediately after the stale-version/hash check and before the draft walk or `CandidateClone`, while retaining the post-apply candidate gate. Draft and graph usage accounting counts every typed object, map/list container, key and scalar exactly once; nested `serde_json::Value` remains owned by its existing single iterative walker. Collection cardinality limits remain independent of the shared 131,072-value, 16 MiB string and 64 MiB aggregate limits.

The structural JSON Schema annotation registry is closed to `title`, `description`, `examples`, `$comment`, `default`, `deprecated`, `readOnly`, and `writeOnly`. Those eight keys are removed only when they occur as annotation keywords of an actual schema node. Property names and schema-map member names remain structural even when they spell an annotation, while values of literal-bearing keywords such as `const` and `enum` are never traversed as schemas. A root schema fragment containing only annotations still fails before sealing; a nested annotation-only subschema may reduce to `{}` under its preserved `properties`/schema-map member and does not by itself make the containing structural fragment empty.

The Foundation authoring hash algorithm itself stayed stable throughout Task 5. The earlier `research-to-publish` authoring golden changed because the separately authorized `targetRef: environment://staging` addition changed the example's authoring semantics; round 19 changes no example or golden literal.

### Round 19 RED → GREEN and mutation evidence

- The replay RED caught the existing `component_by_node[...]` panic for foreign endpoints under a positive `maxDepth`. Moving the endpoint gate after traversal now fails to compile because the traversal accepts only the private `ValidatedTopology` token; independently restoring direct map indexing makes the internal catch-unwind budget regression panic. Endpoint-first membership and fallible component lookup therefore remain separately protected.
- An oversized 1,024-node base reached `CandidateClone`; 4,096-operation node/edge/patch structural matrices likewise reached clone/application under the prior undercount. Removing base preflight, post-apply candidate preflight, operation object, node object, property-map container, or the fixed optionality scalar one at a time fails its dedicated stage/near-limit regression. All restored gates return `GHE006_LIMIT_EXCEEDED` with zero inappropriate downstream stages, zero sealer calls, and byte-identical base state.
- The 16-position annotation matrix first diverged at root `default`; removing each of the eight registry entries independently fails that matrix. The restored shared registry preserves identical persisted topology/semantic identity for annotations at root and nested schema-node positions. Round 20 separately closes schema-map-name and literal-position collisions.

### Round 19 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  57 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  112 passed; 0 failed; all doc tests passed

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 430 tests, 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
  passed

schema root/release catalog, compatibility check and conformance
  passed; 15 schemas in both catalogs; unchanged compatible; 50/50 conformance

three official examples validate/lint/hash plus simulate/replay smoke
  passed; no example or golden literal changed
```

## Fix round 18 — complete draft audit preflight

The single borrowed publication preflight now covers the complete operation, actor, and audit boundary before `CandidateClone`. Draft IDs use the exact durable `OpaqueId` grammar; authoritative and override actors use `ActorId`; and a manual override is accepted only when both actor values are the same valid owner identity. Limit failures retain redacted `GHE006_LIMIT_EXCEEDED`, while within-bound nominal, shape, or authorization failures retain `GHE009_EXTERNALIZATION_FAILED`.

Manual override audit data is closed to a nonblank 1..2,048-character reason, 1..64 acknowledged risks of 1..512 characters each, and 1..64 waived requirements whose members use the exact durable `OpaqueId` grammar. The draft containers, authoritative actor, override actor, reason, both collections, every collection member, and all operation data share one `PersistencePreflight`; no audit-side reset can bypass the 131,072-value or 64 MiB aggregate ceiling. The preflight borrows the input, preserves it byte-for-byte, and cannot prepare or expose Evidence on rejection.

### Round 18 RED → GREEN and mutation evidence

- Four preparation matrices initially reached policy/externalization/sealing or returned a later non-limit failure for invalid IDs, actor mismatches, malformed audit shapes, and an operation-plus-audit aggregate overflow. GREEN rejects all such inputs before the first observed publication stage and before the first sealer call.
- A missing `reason` is rejected at typed deserialization; empty values are rejected at the borrowed preflight. Oversized fields and collections return the limit code, while within-bound invalid nominal values and non-owner/mismatched actors return the existing externalization/authorization code.
- A valid authoritative owner override removes the required `tests` node, waives that exact obligation, preserves the audit input unchanged, prepares Evidence, and retains the established `CandidateClone → CandidateSerialization → CandidateLint → CandidatePolicy → Externalization` order.
- Removing the draft ID gate, authoritative actor gate, owner/equality gate, reason gate, risk gate, waived-requirement gate, and shared aggregate counter one at a time failed the focused regressions. Every mutation was restored before final verification.

### Round 18 fresh verification

```text
cargo +1.97.1 test -p graphhelm-governor --locked
  109 passed; 0 failed; all doc tests passed

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 425 tests, 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

schema catalog/check/conformance
  root/release catalogs matched at 15 schemas; unchanged compatible; 50/50 conformance

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the terminal state

git diff --check
  passed
```

## Fix round 17 — borrowed draft and candidate preflight

Governor publication now applies a shared borrowed usage budget before cloning the base graph. The gate enforces a hard 4,096-operation ceiling together with the predecessor graph's effective `maxMutations`, and inventories the draft ID/hash plus every operation variant's identifiers, node/edge fields, bindings, map cardinalities, and nested JSON values through the same 64-level, 131,072-value, 16 MiB per-string, and 64 MiB aggregate limits used at the persistence boundary. One counter spans those operation payloads; rejected input creates no temporary graph, serialized document, or input-sized validation collection. Round 18 completes this boundary with authoritative actor and manual-override audit fields.

After the preflighted operations are applied to the isolated clone, the resulting `ExecutionGraph` receives the same borrowed aggregate preflight before Serde, schema validation, lint, policy, externalization, or sealing. This second gate covers aggregate expansion between the base and mutations. All limit failures retain the redacted `GHE006_LIMIT_EXCEEDED` code, leave the immutable base bytes unchanged, and return no partial Evidence or projection. Stage-observer coverage proves draft failures occur before cloning and all downstream stages, candidate failures occur after the isolated clone but before serialization, and accepted within-limit input retains the established schema, lint, policy, and externalization order.

Focused REDs first returned `GHE009_EXTERNALIZATION_FAILED` or reached the late externalizer for excess operation counts, deep/wide patches, aggregate operation strings, and candidate aggregate expansion. Oversized node/edge fields reached the existing late bound rather than failing before cloning. GREEN rejects the complete matrix at the new borrowed boundaries with zero sealer calls. Mutation checks independently removed the operation-count gate, aggregate draft accounting, deep-value traversal, and post-apply candidate gate; each dedicated regression failed and every mutation was restored before verification.

### Round 17 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  55 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  103 passed; 0 failed; all doc tests passed

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 419 tests, 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

schema root/release catalog, compatibility and 50-case conformance gates
  passed; root/release 1.0.0 remain compatible and byte-aligned

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

git diff --check
  passed
```

## Fix round 16 - normative safe identity and complete manual waiver

Safe graph identity now follows the accepted formula exactly. `topologyHash` covers canonical safe
topology plus the ordered slot positions `(ownerKind, ownerId, fieldKind, ordinal)`. `semanticHash`
covers that topology material plus the ordered tuples
`(ownerKind, ownerId, fieldKind, ordinal, contentSha256)`. The mandatory, derived and replay-checked
`sensitivity` and `requiredForExecution` profiles remain in every serialized slot, but neither is an
identity input. Evidence ID, ciphertext, nonce, wrapped key, key handle and other encryption metadata
remain excluded. Re-encryption and slot-profile validation regressions remain GREEN.

The manual-override producer and replay grammar now require the exact audit-complete image: at least
one bypassed requirement, at least one acknowledged risk and one valid non-empty `resultLabel`.
Missing fields, empty arrays, zero counts, partial subsets and foreign inline-deny fields fail before
sealing or after adversarial re-hash. No label is fabricated and the complete positive image remains
accepted.

Whole-record preflight now consumes roots through a real work-buffer abstraction. The production
implementation holds at most one borrowed root in an `Option` and records the actual pending length;
it does not allocate a measurement collection. The test-only eager implementation uses the same root
inventory and traversal interface. On the widest bounded inventory, the lazy path peaks at one while
the eager mutation peaks at 110,786, so recollection can no longer hide behind a tautological metric.

### Authorized hash migration

The Rust API computed every value before any literal changed. The controller then obtained explicit
authorization to update exactly the two valid-fixture hashes and six official Task 5 golden hashes.
No schema, catalog digest, protocol, example authoring file, dependency, writer or Task 6 code changed.
The Foundation authoring hash algorithm did not change in this correction. The historical
`research-to-publish` authoring golden had already changed when the authorized deploy `targetRef`
changed that example's authoring semantics; this table records only safe-projection formula updates.

| Projection | Identity | Old | New |
|---|---|---|---|
| valid persisted fixture | topology | `sha256:e60e4df8195f0fd6506cb61efe801ca354af85e8e458581a214943eb41431efa` | `sha256:1b22aa53dd13d245d973d7ed1b74ddea5f5b349790ba96cf95c2b7f919a5b671` |
| valid persisted fixture | semantic | `sha256:2d27a27877da6e67c30061e93a9d02525ae21fc2fdab5289f63af6e75652e4aa` | `sha256:fcc7a24e95bb4dd13467eb7629fa1bf1e7a95a9b2d8f7c27bf520f7a4d1cd2ca` |
| software-feature | topology | `sha256:da66b7fbe11a2669ecbf8593a561dbb4128243a5b7c9736649729317ad0d374c` | `sha256:258ab606014c716ea9a7735107c36f95fcf984b12e84560efb4b06f876c24647` |
| software-feature | semantic | `sha256:ddc0cbb8dfec14bc85c11d20ae12d30b99c97b513719ee39a437ef9c50fd5327` | `sha256:f000e619e23ad61e1ed0b25717f6c80c46723f19c2f6f3dedaa610d5fedbce51` |
| research-to-publish | topology | `sha256:d82d2541777778ba8ffe8f54871f6b08b19caee94eeb634f13e4db0bb7ce040f` | `sha256:5a71610a5a01f0f39f855276fbb3ff6a818cb83f5950ec626e0b686711ed50a1` |
| research-to-publish | semantic | `sha256:85d00feb367ae569691b2f0996b1962d7016d50c6d5aacc04e821bae38d7e954` | `sha256:2564d1a96bcddfe8b02c1dabc48598f41f01b505299a8a48407dfaab9db6d772` |
| manual-override-deploy | topology | `sha256:0f5d1d9a52020b549ee9d8fc4ae22e5b3a0117c51b697d6c47f33cd777e877b3` | `sha256:2c83cf1296d8142e6c8861d34bd4c0d5735de04642bf7e4b713587be729c0559` |
| manual-override-deploy | semantic | `sha256:c01e4e17919bc3b72499e1a8434ac8de9c59de2503a0f9c92cd20915d59045a6` | `sha256:54f7d21c9d1d2a326ebfa02215aed380f0a18c8a0006333d92e133d69d8d9a16` |

### Round 16 RED -> GREEN and mutation evidence

- The independent material test first reported fixture topology
  `sha256:e60e4df8...` instead of normative `sha256:1b22aa53...`; removing both profile fields
  made the independent topology and semantic calculations exact.
- Producer RED reached the sealer for a partial override; replay RED accepted a re-hashed override
  without `resultLabel`. Both boundaries now reject partial, empty and zero-count forms and accept
  the complete form.
- The root metric RED initially had no eager work-buffer API. GREEN measures a real lazy peak of one
  and an eager peak of 110,786 through the shared root inventory and work interface.
- Reintroducing `sensitivity` into the semantic tuple failed the independent semantic assertion.
  Reintroducing `requiredForExecution` into the topology position failed the topology assertion.
  Allowing zero waiver counts failed at `zero_bypassed`; removing the `resultLabel` gate failed at
  `missing_label`; switching production to the eager root implementation failed with peak 110,786
  versus one. Every mutation was restored before verification.

### Round 16 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  55 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  96 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 412 tests, 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

schema catalog root/release, compatibility check and conformance
  passed; release 1.0.0, 15 schemas, unchanged catalog digest, 50/50 conformance

all three official examples validate and lint without errors
canonical graph hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
  passed
```

## Fix round 15 - lazy whole-record roots and zero-count presence

The whole-record preflight now feeds `record.semantic`, metadata properties, policies,
completion, node properties and both edge-condition positions through one borrowed chain of
iterators. It does not collect or clone an intermediate root list. The shared aggregate scalar,
value, key, string and byte counters are preserved across every root, while the explicit JSON
work stack remains bounded by maximum nesting depth. Deterministic instrumentation over the
widest inventory that reaches the root walker records one buffered root; the former eager caller
recorded 110,786 borrowed roots for the same input.

Nested presence is now derived from registered field existence, not from a positive count.
Governor-valid context freshness and isolation network objects containing explicit empty arrays
therefore retain `revalidateCount: 0`/`networkAllowCount: 0` together with their matching true
presence flags. Removing either flag from a re-hashed projection fails replay. The same
field-existence rule covers `writeScopeCount`; its authoring array remains intentionally nonempty
under the existing typed-path contract. All six presence-family matrices and optional/exact
`includeCount` behavior remain unchanged otherwise.

D-036 and ADR-022 now state unambiguously in Portuguese that no legacy compatibility layer
exists, while retaining the exact English gate phrase required by the documentation check. The
earlier report statement that `research-to-publish.yaml` still had `GHG009` is superseded: the
reviewed deploy target is present and lint reports zero errors.

No protocol, schema, catalog, fixture, example, golden, writer, dependency or Task 6 code changed.

### Round 15 RED -> GREEN and mutation evidence

- The width RED initially could not compile because whole-record root-buffer instrumentation did
  not exist. With instrumentation and the lazy borrowed chain, the bounded wide record reaches
  the aggregate limit with a peak root buffer of one.
- Restoring the prior eager `Vec<&Value>` caller made the dedicated width test fail with
  `left: 110786, right: 1`; the lazy traversal was restored before verification.
- Explicit empty freshness/network arrays initially made the Governor's own safe projection fail
  validation. Presence now observes the zero-valued marker itself. Removing either matching true
  flag from the accepted projection makes the replay mutation fail.
- The complete graph and Governor regressions, including the six-family presence matrices and
  `includeCount` cardinality gates, returned GREEN after the changes.

### Round 15 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  54 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  94 passed; 0 failed; all doc tests passed

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 409 tests, 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

research-to-publish validate/lint
  passed; zero lint errors (five existing default-timeout warnings)

git diff --check
  passed
```

## Fix round 14 - bounded preflight and exact nested discriminants

The round-14 JSON-container walker stopped pushing every child of a wide JSON array or object.
One explicit iterator frame is retained per open container and each child is visited only when
selected by its parent cursor. The maximum work stack is therefore bounded by the existing depth
constant (64), while value, key, string and aggregate-byte accounting retain the shared limits
and redacted `LimitExceeded` result. Flat arrays and objects with 131,073 cheap children, nested
wide/deep input and a normal accepted object are covered by deterministic instrumentation; the
rejected wide cases retain at most 64 frames rather than width-proportional pending work. At this
historical round the whole-record caller still collected its roots; round 15 supersedes that
remaining eager layer with borrowed streaming.

The shared context-include discriminator now permits a reference only for the exact `document`
image and only in the canonical Document domain. Artifact, Context, Schema and Environment
references are rejected both during Governor preparation and after deserialization with hashes
recomputed. The producer uses the same predicate before Evidence sealing, so wrong-domain values
leave the sealer call count at zero. `project_kernel`, `dependency_output` and `source_scope`
retain their mutually exclusive no-ref/node/path images.

Context `freshnessPresent`, `budgetPresent` and `expansionPresent`, plus isolation
`filesystemPresent`, `networkPresent` and `brokerPresent`, are now semantic discriminants. A flag,
when present, must be true and own at least one registered child; a child cannot exist without its
matching flag, and a foreign-family child cannot be smuggled into another image. Each exact
non-empty Governor image remains accepted. A context without authoring includes may omit
`includeCount`; when that count exists it must be nonzero and its indexed family must be exact.
No protocol, schema, catalog, fixture, example, golden, writer, dependency or Task 6 code changed.

### Round 14 RED -> GREEN and mutation evidence

- The initial traversal RED failed to compile because bounded work-stack instrumentation did not
  exist. After the incremental cursor traversal was added, wide array/object inputs return the
  same limit error with stack depth bounded by 64. A width-scaled metric mutation failed with
  `wide input retained 131073 pending frames`, then was restored.
- A `document` include carrying `artifact://architecture-auth` reached the failure sealer before
  the fix. Relaxing the final shared predicate back to the broad ContextInclude domain reproduced
  the same sealer call, then the exact Document predicate was restored.
- The nested-context RED also exposed a producer/replay mismatch: Governor can emit a context
  containing only freshness/budget/expansion, while replay previously required `includeCount`.
  Omission is now accepted only when no indexed include field exists; a present family stays
  nonempty and exact.
- Removing freshness, budget and expansion equivalence gates made their matrix fail respectively
  at `freshnessPresent:false`, `freshnessPresent:crossed` and `expansionPresent:false`.
  Removing filesystem, network and broker gates failed at `filesystemPresent:false`,
  `filesystemPresent:crossed` and `networkPresent:crossed`. Every gate was restored before final
  verification.

### Round 14 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  53 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  93 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 407 tests, 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

git diff --check
  passed
```

## Fix round 12 - semantic preflight and persistence ownership domains

The twelfth correction moves the complete `GraphVersionRecord` free-form-value inventory into the shared graph persistence boundary and runs its iterative depth, value-count and key/string-byte preflight before the Governor clones the record or invokes `GraphVersion::from_record`. The inventory now explicitly includes the independently supplied `record.semantic` image together with metadata properties, policies, completion, node extension properties and edge condition/on-false values. Semantic-only values above each bound therefore return the stable redacted limit error before canonical comparison and before the first sealer call.

Replay now permits `ContentOwnerKind::Agent` only when the owner exists and has `nodeType=agent`. Every Node-class slot remains valid on agent nodes for their common display/objective image, while Agent-class slots on all fifteen non-agent node kinds fail even when the attacker derives a valid slot ID/profile, links it into `contentSlotIds` and recomputes both hashes. The pre-existing exact `contentSlotIds` comparison already rejected an unlinked Agent slot; that part of the review finding was stale and required no new production gate, so it was retained only as an explicit regression.

The public binding parser is now the single Governor/replay domain gate for every input, output and edge binding. Raw dot bindings remain limited to typed node outputs. Canonical `refv1` bindings decode only to logical or digest artifacts, bounded `context://` items (including claims, user decisions and project settings) or non-secret `environment://` references. Schema, policy, tool, deploy adapter, evaluator, agent/model, rules, graph-template, contract, document and all other globally registered reference families fail in binding positions while remaining valid in their own registered controls.

One shared isolation predicate accepts exactly `tier_0`, `tier_1`, `tier_2` and `tier_3`. Governor construction applies it to both node `isolation.minimum` and ephemeral-agent `isolationMinimum` before sealing; replay applies the same predicate to `node_isolation.minimum` and `agent_configuration.isolation` after deserialization. Case variants, aliases and unknown tiers fail closed.

### Round 12 RED -> GREEN and mutation evidence

- Semantic-only depth first returned `GHE009_EXTERNALIZATION_FAILED` through the later record reconstruction path; semantic-only value-count and byte cases exercised the same omitted inventory. All three now return `GHE006_LIMIT_EXCEEDED` with zero sealer calls.
- A correctly derived and linked Agent-purpose slot on a `tool` node was accepted. The RED matrix covers every non-agent node kind with a separately replay-valid baseline; all now fail the owner-class gate. An unlinked Agent slot was already rejected by the exact image comparison and is documented as stale review evidence.
- Canonically encoded `schema://Input@1` was accepted in a binding. The adversarial matrix now rejects every foreign registered family and separately proves input, output and edge replay positions; positive artifact, context/claim/decision/project-setting and environment families remain valid.
- Replay accepted `node_isolation.minimum=tier_4` and the corresponding agent position accepted a case alias. Positives cover all four normative tiers in both positions; invalid authoring fails before sealing and independently re-hashed replay mutations fail.
- Mutation checks removed and restored the semantic inventory entry, Agent owner-class test, binding-domain predicate, node isolation enum arm and agent isolation enum arm one at a time. Their focused tests failed at the intended boundary, and all restored gates returned GREEN.

### Round 12 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --all-features --locked
  51 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --all-features --locked
  80 passed; 0 failed; all doc tests passed

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  389 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed all six succeeded nodes

git diff --check
  passed
```

## Fix round 11 - complete safe publication boundary

The eleventh correction closes the remaining authoring/replay parity gaps without changing protocol types, schemas, wire shape, writers, dependencies, or the Task 6 repository path. The shared durable-content scanner now recognizes compact JWS values from the original case-sensitive Base64URL bytes. Each maximal segment is visited once, a candidate header is bounded to 16 KiB, decoded once, parsed as one bounded JSON object, and recognized only when it contains a non-empty string `alg`. Leading/trailing JSON whitespace, key reordering, extra fields, and encodings that do not begin with `eyJ` are covered. The complete scanner retains the documented `512 * input_bytes` operation bound across separator-heavy and historical false-positive corpora; inputs are never echoed.

Governor preparation now applies the shared iterative depth/value/string-key/aggregate-byte preflight to all untrusted authoring `Value` roots before `GraphVersion::from_record`, cloning, sorting, or serialization. The existing `canonical_content_bytes` preflight remains, so both the authoring and direct canonical-content boundaries fail with stable redacted limit errors.

Replay now mirrors the representable Foundation structural rules. A deploy with `reversible` or `compensationRequired` requires an existing rollback node as `compensationNode`; self, missing, and non-rollback targets fail. Persisted `maxNodes`, condensed-SCC `maxDepth`, and `maxRetriesPerNode + 1` attempt limits use bounded iterative traversal. Completion without explicit terminals is projected by the Governor as sorted unique out-degree-zero nodes, matching Foundation reachability; replay requires non-empty, existing, unique terminals. Equivalent explicit and implicit completion produce the same safe identities.

Every durable reference position now uses one public closed graph-owned domain registry shared by Governor construction and replay. A globally valid `refv1` fails when its decoded family is wrong: environment, deploy adapter, schema/schema identity/schema dialect, policy/context-policy, agent, tool, rules, evaluator, graph-template, contract, artifact, document, context include, directive, and completion-requirement positions enforce their registered family. Logical artifact locators, nominal directives, dot bindings, compact-secret gates, and canonical `refv1` behavior remain intact.

Inline JSON Schema values in input/output contracts are bounded structural projections, not Evidence or raw authoring text. Only `title`, `description`, `examples`, and `$comment` are recursively removed; the remaining fragment is content-scanned, canonically serialized, and represented by `digests.schema`. Replay permits that exact digest only on input/output controls and requires a one-of between a string schema reference and the inline schema digest. Annotation-only and unsafe fragments fail before sealing; reordered or annotation-only changes preserve the digest, while a structural keyword change alters it.

The existing replay correlated-group gate already required exactly one `capability.NNN` per permission index, so the review claim that replay accepted duration/scope-only permissions was stale at this HEAD. Round 11 adds explicit authoring and replay regressions plus mutation evidence for that existing gate instead of duplicating production logic. Qualifiers remain optional and aligned to their capability index.

One historical focused test became invalid under the newly enforced Foundation retry budget: it changed `maxAttempts` from three to four while `maxRetriesPerNode` was two. The test only proves that a valid registered structural mutation changes both hashes, so it now changes `maxBackoffSeconds` from 120 to 121. No fixture, official example, schema, wire contract, or reviewed golden hash changed.

### Round 11 RED -> GREEN and mutation evidence

- Structural JWS REDs showed whitespace/non-`eyJ` headers reaching the sealer; near-match JSON without a string `alg` remained accepted. The structural detector made the matrix GREEN within the shared linear bound.
- The programmatic nested-`Value` RED returned generic authoring failure from `GraphVersion::from_record`; aggregate iterative preflight now returns `GHE006_LIMIT_EXCEEDED` before reconstruction and before the first sealer call.
- Re-hashed compensation, node-count, condensed-depth, retry-budget, duplicate-terminal, wrong-family-reference, inline-schema one-of, and capability matrices were accepted at their missing gates; each now fails through `validate_persisted_projection`.
- Direct Governor REDs proved missing compensation, exceeded budgets, implicit terminals, wrong-family references, inline schemas, and structural JWS values crossed the boundary or reached sealing. All now fail or project deterministically before the first sealing call.
- Removing, one at a time, the structural JWS detector, compensation gate, each of the three budget gates, pre-reconstruction preflight, positional reference-domain check, inline-schema one-of, terminal uniqueness, implicit-terminal derivation, and permission-capability correlation made its dedicated test fail. Every mutation was restored before fresh verification.
- Historical Task 5 suites covering scanner families, closed grammar, SCC/deploy/reachability, loops, exact images/profiles/slots/bindings, edge ordinals, paired groups, agent/policy modes, dot owners, Evidence/sealer/bijection, all node kinds, hashes/lineage/no-echo, and official examples remained GREEN.

### Round 11 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  49 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  76 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  383 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

graph validate/lint/hash plus simulate/replay smoke
  passed; validation/hash/simulation/replay succeeded and lint returned only the three reviewed default-timeout warnings

git diff --check
  passed
```

## Fix round 9 — exact projection image and slot profiles

Three focused REDs reproduced the independent review findings with hashes recomputed through the public persistence APIs. Replay accepted a proper subset with every slot removed, accepted independent changes to `sensitivity` and `requiredForExecution`, and the Governor accepted an inline policy combining `deny` with `manualOverride`. The RED matrix also removed Graph/Node mandatory display/objective slots, the required agent configuration, required gate completion, and the unconditional Graph completion image.

The graph safe-projection layer now owns a public pure `derive_content_slot_profile` function beside slot-ID derivation. It maps the exact typed owner/field/ordinal position to the only permitted sensitivity and execution requirement; unsupported positions fail closed. The Governor no longer supplies independent profile literals, and replay recomputes every profile without trusting serialized metadata. Official examples plus a synthetic graph cover Graph and Node descriptions, Node instructions/completion, ephemeral-agent purpose/instructions/completion, edge condition/fallback, and both policy text positions. The existing deterministic re-encryption test remains GREEN, proving these profiles are structural rather than cryptographic metadata.

Replay now enforces the minimum/exact image derivable without authoring plaintext: one Graph display-name slot; one display-name and objective slot for every Node; exact ref/ephemeral Agent content; required `agent_configuration`; required gate `node_completion`; and the Governor-produced `graph_completion` control. Optional authored fields remain optional, while registered Edge, Policy and bound Node content retains the existing exact binding validation. A proper subset cannot become valid merely by recomputing topology and semantic hashes.

Policy modes are mutually exclusive in both authoring and replay. `ref` contains only its reference identity, `inline` contains only deny/reason/rule/explanation material, and `manual_override` contains only bypassed requirements, acknowledged risks and result label. Foreign counters, slots, flags or bindings fail closed. A combined authoring object is rejected before the sealer is called.

The user explicitly authorized the necessary structural correction to the valid conformance fixture after the exact-image gate exposed its stale pre-emitter shape. The fixture gained the derived Graph display slot `slot-69099886…de85`, Node display slot `slot-fa2802cb…a3cf4`, corrected Objective sensitivity from `internal` to `restricted`, an exact ref-mode `agent_configuration`, and the emitted `graph_completion` form. Test-only hash normalization was removed. Fixture hashes changed from placeholder zeroes to:

- topology: `sha256:e60e4df8195f0fd6506cb61efe801ca354af85e8e458581a214943eb41431efa`
- semantic: `sha256:2d27a27877da6e67c30061e93a9d02525ae21fc2fdab5289f63af6e75652e4aa`

This is a fixture-instance correction only: no persistence schema, wire type, protocol production code, writer, adapter or dependency changed. The three official Governor golden pairs remain unchanged.

Mutation checks removed the minimum-image call, slot-profile comparison and exact-policy-mode validation one at a time. Their dedicated tests failed by accepting the reviewed corruption, then returned GREEN after each gate was restored. Focused verification after restoration: protocols persistence wire 24/24, graph 34/34, Governor 62/62, and focused Clippy with zero warnings.

### Round 9 full repository verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  354 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed

git diff --check
  passed
```

Fix round 1 fresh verification:

```text
cargo +1.97.1 test -p graphhelm-governor --test safe_publication --locked
  15 passed; 0 failed

cargo +1.97.1 clippy -p graphhelm-governor --all-targets --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  304 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

Fix round 2 fresh verification:

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  21 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  29 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  308 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

Fix round 3 fresh verification after the final golden update:

```text
cargo +1.97.1 test -p graphhelm-governor --locked
  38 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-governor --all-targets --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  317 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

Fix round 4 fresh verification after restoring every mutation:

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  23 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  43 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  324 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

Round-3 Task 5-only safe identities intentionally changed because runtime-enforceable controls replaced one-way digests. Reviewed golden pairs are now:

```text
software-feature.yaml      cdaf474b... / 1737f3f6...
research-to-publish.yaml   aa3a5480... / fcdf0a22...
manual-override-deploy.yaml bf738e3b... / 37fdf58b...
```

Foundation canonical hashes, schemas, protocols, wire types and dependencies were not changed.

## Adversarial self-review

- Plaintext surfaces: serialized safe versions, preparation debug output, public errors, diagnostics, and sealed bytes were scanned with distinct registered canaries; no authored bytes appeared.
- Heuristic bypass: extraction decisions are exact typed-path matches. Arbitrary nested values and unregistered Foundation extension fields fail closed instead of being classified by suggestive key names.
- Resource exhaustion: count and byte ceilings precede sealing; deterministic counting tests prove zero sealer calls for oversize input and no allocation of the aggregate-overflowing item.
- Hash confusion: topology identity excludes both content digests and sealing metadata; semantic identity includes ordered content digests; insertion order, annotation changes, re-encryption, and content changes have separate regression coverage.
- Reference confusion: exact order, unique positions, unique slot/evidence IDs, reference uniqueness, and digest equality are validated before a preparation can escape.
- Publication identity confusion: Evidence IDs are scoped to repository, immutable graph number/semantic identity and exact slot position while retry/re-encryption remains stable.
- Untrusted sealer output: all request-bound metadata, exact ciphertext digest, AAD digest and key handle are revalidated before `ProjectionPreparation` is constructed.
- Atomicity: preparation owns only an isolated candidate and sealed values; failures cannot mutate the active graph or the existing journal because no writer is reachable from the API.
- Dependency direction: Governor depends inward on events, graph, policy, protocols, and schema interfaces; graph depends only on protocols. No core-to-adapter or circular dependency was introduced.
- Scope: no schema/wire/event-writer/CLI/adapter change, no future scaffold, no network/model/tool/filesystem traversal, and no production deterministic crypto seam.

The original implementation concerns were closed at its commit boundary. Independent-review findings not assigned to fix round 1 remain tracked for the authorized later Task 5 fix rounds.

## Fix round 5 — complete node registry and structural cardinality

The fifth correction closes the remaining semantic-loss gap in the authoring-to-persistence registry. A schema-valid focused graph exercises all 16 v1 node discriminators. The common `tags`, `onCancel`, and `onFailure` controls and every child shown in `GRAPH_DSL_SPEC.md` sections 5–25 are now classified by an explicit node-kind owner. Tool, classifier, gate, fork, join, human-decision, subgraph, materializer, deploy, and rollback controls retain their registered IDs, references, actions, enums, booleans, integers, list order, presence, and counts. Planner, evaluator, timer, trigger, and artifact-transform have no type-specific child in that normative range, so they accept their typed discriminator and common controls only; a child owned by another kind fails before sealing.

The production `refv1` codec remains the only persisted reference representation. The reference validator now covers tool, rules, evaluator, join-result-schema, graph-template, subgraph parameter, document, deploy, policy, precondition, completion-contract, and graph-completion positions. The only grammar addition is the normative `policy://scope/name@version` family. Bare paths, writable/source locations, arbitrary URLs/schemes, malformed artifacts, secret-bearing environment references, raw references at encoded positions, encoded references at nominal positions, and noncanonical encodings remain fail-closed.

Node and graph completion are no longer persisted as one opaque Evidence value. Node completion retains requirement/forbid counts, safe requirement IDs, artifact IDs, `outputSchemaValid`, evidence type/minimum and contract references. Only bounded expression text becomes an owner-exact `CompletionContract` slot. Human prompt text becomes an `Instructions` slot. Graph completion retains terminal node IDs, document/artifact requirements, waiver flag and result statuses. Changing a structural completion field changes both hashes; changing only an expression preserves the topology hash and changes the semantic hash.

Policies now retain their enforcement identity. Canonical policy references, restrictive `deny` capabilities, reason codes, and the existing bounded manual-override risk/requirement/result IDs remain structural; registered rule/explanation text becomes `PolicyText` Evidence. A prose-only policy, unknown child, or permission-expanding inline `allow` shape is rejected. No policy needed for replay is replaced by an untyped content digest.

Every registered array records a deterministic count before items. Optional nested objects either have an explicit presence flag or contain a registered count/value that distinguishes them from absence. Schema-permitted empty registered lists/maps such as tags, bindings, parameters, and completion requirements round-trip as present-empty; empty object/items with no registered semantic child, duplicates in unique lists, values above 64 items, unsafe integers, wrong types, and invalid closed enums fail before the first sealer call. Arrays with authoring order preserve it through zero-padded indices.

The Task 5 safe-projection goldens intentionally changed because formerly externalized completion/policy bodies and implicit list/object presence are now replay-enforceable controls:

```text
software-feature.yaml       8c9b9ff9... / d328e477...
research-to-publish.yaml    8bc631be... / 933abf51...
manual-override-deploy.yaml b2b10a15... / 4b5ac363...
```

Foundation authoring hashes, JSON Schemas, protocol/wire enums, topology/predecessor logic, writers, adapters, dependencies, and public reference encoding were not changed.

### Round 5 RED → GREEN and mutation evidence

- The complete node-kind graph first failed with `GHE009_EXTERNALIZATION_FAILED` because the old registry rejected normative type-specific children and collapsed completion/policy structure.
- Presence/cardinality tests distinguish absent, present-empty, nonempty, duplicate, empty-item and 65-item input. Invalid input returns the redacted stable error before any sealer call.
- Directly deserialized safe versions reject malformed or raw values at the new registered reference positions. Governor invokes the same production validator before returning.
- Removing tool-reference persistence made the node-kind mutation identity test fail with equal topology hashes.
- Removing `outputSchemaValid` persistence made the completion mutation identity test fail with equal topology hashes.
- Removing array counts made present-empty tags collapse to absence and the cardinality test fail. All three mutations were restored before verification.

### Round 5 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph persistence --locked
  2 focused unit tests passed; all filtered integration targets remained clean

cargo +1.97.1 test -p graphhelm-governor --test safe_publication --locked
  37 passed; 0 failed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  330 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed

git diff --check
  passed
```

## Fix round 6 — relational topology and explicit safe predecessor

The sixth correction adds the public, pure `validate_persisted_projection` graph-layer boundary. It validates a deserialized safe version entirely in memory: genesis/successor lineage, stored hashes, registered persisted references, entrypoint membership, unique edge IDs, edge endpoints, registered terminal/failure-route/compensation node membership, exact slot owner/field compatibility, graph/policy owner identity, node/agent and edge existence, and each node's exact ordered Node+Agent `contentSlotIds` list. Diagnostic-owned content is not valid inside a graph version. The validator returns only the existing redacted typed `GraphError::InvalidProjection` and performs no authoring reconstruction, Evidence access, filesystem, network, tool, or model operation.

The Governor invokes that validator before the first sealer call. Direct malformed authoring records now fail before sealing for duplicate edge IDs, missing entrypoints, and missing edge sources or targets. Separately, deserialized safe versions with missing predecessor lineage, dangling or wrongly typed owners, duplicate edges, missing endpoints, or mismatched node slot lists fail through the same reusable validator that Task 6 replay can invoke.

`GraphExternalizer::prepare` is now genesis-only. A successor must use `prepare_with_predecessor` with an explicit validated `PersistedGraphVersionRef`; missing, extra, mismatched, overflowing, or non-monotonic lineage fails closed. The externalizer compares version numbers but never parses or copies the Foundation predecessor `content_hash`. The publication wrapper computes the base safe semantic identity through `prepare_projection_material`, constructs the safe predecessor, and passes it explicitly without mutating either the base or candidate Foundation record. Safe hashing remains predecessor-independent and does not recursively load earlier content or Evidence.

All three reviewed Task 5 topology/semantic golden pairs remain unchanged. Foundation authoring hashes, protocol/schema/wire types, event writers, repositories, adapters, dependencies, and public reference encoding were not changed.

### Round 6 RED -> GREEN and mutation evidence

- The graph RED first failed to compile because `validate_persisted_projection` did not exist. After the initial relational implementation, a deserialized version `2` with no predecessor returned `Ok`; the added missing-lineage RED failed and then passed after the pure validator enforced genesis/successor presence. A second relational RED showed missing graph terminals, gate failure routes, and deploy compensation nodes were still accepted, then returned GREEN after those registered node references joined the same pure validator.
- The Governor API RED first failed because the externalizer had no explicit safe-predecessor operation. GREEN makes `prepare` reject non-genesis and `prepare_with_predecessor` reject genesis, missing/mismatched Foundation lineage, or a safe number other than exactly `current - 1`.
- Direct malformed-topology tests assert `GHE005_INTEGRITY_FAILURE`, zero sealer calls, and no canary echo for missing endpoints/entrypoints and duplicate edges.
- Replacing the explicit safe predecessor with the Foundation predecessor hash made `direct_externalizer_serializes_only_the_explicit_safe_predecessor_hash` fail with the Foundation `ffff...` identity instead of the caller-supplied safe `aaaa...` identity. Restoring the explicit input returned GREEN.
- Removing endpoint membership validation made `direct_externalizer_rejects_relational_topology_before_sealing` reach the failure sealer and return `GHE009_EXTERNALIZATION_FAILED` instead of the expected pre-sealing integrity failure. Restoring the endpoint gate returned GREEN.

### Round 6 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  25 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  52 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  335 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

## Fix round 7 — globally linear scanner and closed replay grammar

The seventh correction moves the durable content gate into `core/graph`, so Governor authoring preparation and direct replay validation use the same bounded implementation. The Governor duplicate was removed. The shared scanner limits depth, values and aggregate bytes before normalization; performs one bounded ASCII-lowercase pass; uses fixed-prefix checks, compact PEM state machines, a segment-based JWT pass and bounded authorization/reference checks. Every detector participates in one deterministic operation counter. No regex, wall-clock assertion, network lookup or input echo was added.

The confirmed repeated-`eyJ` mutation restored the previous suffix-rescan algorithm and measured 404,660,065 operations for 49,152 bytes. The production segment pass stays within the documented global `512 * input_bytes` bound across repeated JWT prefixes and adversarial prefix, authorization, environment-reference, PEM and secret-URI families. Existing GitHub/OpenAI/AWS/GitLab/Slack/JWT/authorization/PEM/secret-reference detections remain covered by the shared gate.

Compact secret-name denial now applies to every registered reference family and to decoded replay references. Case/separator forms of `apiKey`, `privateKey`, `accessKey`, `databaseUrl`, `connectionString`, and the previously recognized secret/password/credential/token families fail closed. `environment://` retains exactly one bounded non-secret nominal environment token. Canonical `refv1` values that decode to a denied reference are rejected even when the encoded wire itself satisfies `SafeValue`.

`validate_persisted_projection` is now the single replay gate for lineage, the complete registered control grammar, canonical references, shared durable-content scanning, stored hashes and relational invariants. The graph-owned grammar enumerates every control type emitted by Task 5, its exact identifier/integer/flag/digest maps, fixed and indexed keys, counts, required discriminators, enums, reference positions and node-kind ownership. Unknown types, foreign keys, wrong maps, missing counts/items, invalid `present` flags, duplicate control types and type-specific controls on foreign node kinds fail closed. The checked-in baseline `terminal_nodes` fixture remains accepted only through its exact historical schema shape; Governor publication emits `graph_completion`.

Replay scanning covers graph/execution/node/edge/content-slot/Evidence/actor identifiers, labels, entrypoints, endpoints, control types and keys, nominal values, bindings, and canonical decoded references under the same aggregate bounds. Digests, ciphertext and Evidence plaintext are neither reconstructed nor inspected. Errors remain the existing redacted `GraphError::InvalidProjection`; Governor maps authoring unsafe/limit failures to its existing stable codes.

### Round 7 RED → GREEN and mutation evidence

- The first graph RED failed to compile because the shared scanner API did not exist. After the scanner moved, re-hashed direct projections still showed the separate grammar defect: unknown control types returned `Ok(())`.
- Re-hashed deserialized projections cover unknown control type, foreign key, key in the wrong map, tool control on an agent node, secret-shaped labels/graph/actor identifiers, nominal control values and a canonical encoded secret-bearing environment binding. All now fail through `validate_persisted_projection` without needing a hash mismatch.
- Restoring the old repeated-JWT suffix rescans made the global linearity test fail at 404,660,065 operations for 49,152 bytes. Restoring the segment pass returned GREEN.
- Removing the closed-grammar call made the unknown/foreign/wrong-map matrix accept its first malformed projection and fail. Removing the shared replay-content call made the secret-SafeValue matrix accept its first secret projection and fail. Both gates were restored before verification.
- Existing all-16-node Governor reconstruction tests pass through the new graph grammar, preserving the reviewed safe hashes and every prior reference, topology, predecessor and Evidence invariant. Round 8 slot/binding/kind/policy/schema-container semantics remain intentionally untouched.

### Round 7 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  29 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  51 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  338 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

## Fix round 8 - exact slots, bindings and control ownership

The eighth correction moves content-slot identity into the public graph safe-projection layer. `derive_content_slot_id` is the single bounded, domain-separated, length-prefixed function of owner kind, owner ID, field kind and ordinal; the Governor and replay validator now use the same implementation. Replay recomputes every slot ID after deserialization, and exact binding validation rejects a slot attached to the wrong typed field even after all stored hashes are recomputed. Node completion expressions, human directives, edge condition/on-false text and policy rule/explanation text each have an exact owner, field, ordinal and binding key. Edge, policy and bound Node text slots cannot remain dangling.

Input/output and edge bindings now share one closed grammar. Registered URI locators remain canonical `refv1`; normative bounded `outputs.<node>.<field...>` and `nodes.<node>.output[.<field...>]` expressions remain raw `SafeValue`. Empty/traversal/path-shaped, excessive-depth/length, foreign-root and compact secret-bearing forms fail before sealing and through direct replay validation. The raw binding grammar is necessarily the subset representable by the fixed `SafeValue` wire type; no protocol or schema shape changed.

`targetRef` is owned only by deploy nodes in both Governor extraction and replay grammar. Policy projection is injective: a policy is exactly a reference or one inline constraint, never both; rule text and explanation use distinct keys and ordinals; ref/inline/manual-override modes cannot retain incompatible enforcement controls. Each permitted text change preserves topology identity while changing only its own content digest and the semantic identity.

Schema containers that become empty after removing `title`, `description`, `examples` and `$comment` now fail before sealing. Those annotations remain hash-insensitive when a registered semantic member is present. Foundation `metadata.mutationId` is excluded alongside the existing non-semantic metadata and does not alter topology, semantic identity, slot count or sealing count. The round-7 compact secret reference families (`apiKey`, `privateKey`, `accessKey`, `databaseUrl`, `connectionString`, including separator/case variants) were revalidated through both Governor and direct replay.

No protocol production code, persistence schema, wire shape, writer/apply path, adapter, dependency or Task 6 implementation changed. All official example golden hashes remain unchanged. After explicit scope authorization, the schema-valid conformance fixture's placeholder `slot-objective` was mechanically replaced in `slotId` and `contentSlotIds` with the exact derived Node/objective/ordinal-0 ID. No other fixture field or wire shape changed, and the semantic validator now consumes the checked-in fixture without test-only normalization.

The fixture update exposed one stale test-only duplicate mutation: the protocol test still appended the removed `slot-objective` placeholder, so it no longer created a duplicate and the full workspace gate correctly failed that assertion. After a second explicit scope authorization, only that test was changed to reuse the fixture's current first `slotId`; no protocol production code or wire contract changed. An earlier focused rerun also ended with a shell-timeout `BrokenPipe` while listing `canonical_hash`; rerunning the identical command with an adequate bounded timeout passed, confirming it was infrastructure noise rather than a behavioral RED.

### Round 8 RED -> GREEN and mutation evidence

- The public slot-derivation RED failed to compile with E0432. After the shared pure function was added, re-hashed projections with renamed Graph, Policy and Edge slots still returned `Ok(())`; exact replay recomputation made them fail closed.
- Normative dot bindings first returned `InvalidAuthoring`; the shared binding grammar made authoring and replay GREEN while the adversarial matrix remained pre-sealing and replay fail-closed.
- A schema-valid `targetRef` on an agent node published successfully; Governor ownership and the independent replay node-kind gate now reject every non-deploy node and retain the positive deploy case.
- `ref + inlineConstraint` published successfully and simultaneous `ruleText`/`explanation` collapsed into one `textSlot`. Closed policy shapes and two deterministic policy ordinals/keys made both behaviors GREEN.
- Annotation-only input reached the failure sealer, proving the empty structural control was constructed. The semantic-member gate now returns the externalization failure with zero sealer calls.
- `metadata.mutationId` returned `InvalidAuthoring`; deterministic exclusion now produces the same persisted version and Evidence count for absent or changed values.
- A node completion binding swapped to a valid objective slot was accepted after hashes were recomputed. Exact control-to-slot validation now rejects it.
- Mutation checks removed, one at a time, slot-ID recomputation, exact-binding validation, deploy-only replay ownership and the ref/inline collision clause. Their focused tests failed respectively, then all four gates were restored and returned GREEN.

### Round 8 focused verification

```text
cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire --locked
  24 passed; 0 failed

cargo +1.97.1 test -p graphhelm-graph --locked
  33 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  59 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-protocols -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings
```

### Round 8 full repository verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  350 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

## Fix round 10 - structural projection invariants

The tenth correction makes the safe projection retain every Foundation structural fact needed to reject impossible replay state without reconstructing authoring plaintext. Governor externalization registers positive `loop.maxIterations` as the closed `node_loop` control. Replay uses bounded iterative traversals and Kosaraju SCC discovery: every self-loop or multi-node SCC needs at least one positive persisted loop limit; every deploy node needs its exact `node_configuration.targetRef`; completion has at least one existing terminal; and every reachable entrypoint node has a path to a declared terminal. These failures are unconditional `InvalidProjection` results and cannot be waived.

The public binding parser now returns the referenced topology node for the closed `outputs.<node>...` and `nodes.<node>.output...` forms. Input/output contracts and edge bindings reject a syntactically valid ghost owner after stored hashes are recomputed. URI bindings still return no topology owner and remain canonical `refv1`. Edge `condition` and `onFalse` retain fixed ordinals zero and one independently of field presence; an on-false-only edge therefore binds `slot1`, never a compacted `slot0`.

Control reconstruction is closed further. Schema contracts require a real registered semantic member rather than `present=true` or `bindingCount=0`. Correlated indexed families require complete per-prefix index sets; subgraph parameter keys and values, contract binding keys and values, permission capabilities and context include types cannot be half-present. Evidence and completion alternatives require exactly one semantic representation per indexed position, and optional minima/references require their typed owner. `agent_configuration` is now an exact ref or ephemeral image: ref mode contains only `mode` and `agentRef`; ephemeral mode requires its input/result schemas and positive capability count and cannot retain an agent ref.

Logical DSL artifact locators such as `artifact://implementation.diff` are accepted as one bounded normalized non-path token and encoded only through canonical `refv1`. Content-addressed `artifact://sha256/<digest>` remains supported. Slashes, traversal, drive/nested schemes, overlength, unsafe punctuation and secret-family names remain rejected. This does not create or imply an `ArtifactRegistration`; Task 5 remains Evidence-only.

`canonical_content_bytes` now executes the shared iterative depth/value/string-and-key-byte preflight before recursive clone/sort. Rejected values therefore fail with the existing structured projection error before recursive work or allocation proportional to rejected input; accepted canonical bytes are unchanged.

The structural deploy gate exposed a real normative example defect: `research-to-publish.yaml` declared deploy node `publish` without `targetRef`, and Foundation lint returned `GHG009_DEPLOY_TARGET_MISSING`. After explicit authorization, the example gained only `targetRef: environment://staging`. Its authoring semantic golden changed from `989a231a...` to `9f8fff5d...`; safe topology from `8bc631be...` to `d82d2541...`; and safe semantic from `933abf51...` to `85d00feb...`. The example validates, lints with zero errors, and externalizes without plaintext. Synthetic all-node helpers gained only an agent-to-deploy edge so their declared entrypoint has a Foundation-valid path to the terminal. No schema, protocol/wire type, fixture, writer, dependency or Task 6 path changed.

### Round 10 RED -> GREEN and mutation evidence

- The public binding-owner RED first failed to compile. After the API existed, seven independently re-hashed graph REDs demonstrated acceptance of an uncontrolled cycle, deploy without target/empty completion, ghost binding, semantic-empty/half-paired contract, hybrid agent mode, rejected normative logical artifact and unbounded canonical content.
- Governor REDs reproduced a controlled authoring self-cycle that could not be projected and an on-false-only edge whose ordinal was compressed to zero. `node_loop` and fixed edge ordinals made both reconstructible and replay-valid.
- Direct replay covers uncontrolled and controlled self-cycles, multi-node SCCs, a reachable dead-end, empty completion, missing deploy target, ghost contract binding, half binding/parameter groups, orphan evidence minimum, missing ephemeral schema, ref/ephemeral hybrid and compacted/swapped edge slots. Direct Governor coverage rejects invalid loop and agent shapes before the first sealer call.
- Removing the persisted topology gate made the uncontrolled-cycle test accept the mutation. Removing topology-owner validation made the ghost contract binding pass. Removing correlated-group validation made the half binding pass. Removing canonical preflight made over-depth content pass. Compressing `onFalse` to ordinal zero made its focused Governor test fail. Every gate was restored before final verification.

### Round 10 fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  42 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  67 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  367 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

graph validate/lint/hash plus simulate/replay smoke
  passed; research example lint has zero errors; software simulation replay completed

git diff --check
  passed
```

## Fix round 13a — registered path content positions

D-037 and accepted ADR-023 resolve the contradiction between Foundation authoring and safe persistence: context source-scope paths, permission-scope paths and isolation filesystem writable paths are valid authoring inputs, but none may enter the append-only boundary as plaintext. The former eight-kind list is superseded by the closed eleven-kind `ContentFieldKind`; the added exact wire values are `context_path`, `permission_path` and `isolation_path`. Each represents one ordered typed authoring position, is Evidence-backed, `restricted`, required for execution and included in topology/semantic slot hashing. No fallback alias, translation or catch-all exists.

The accepted design, threat model and implementation-plan interface now state the same ownership and confidentiality contract. The single pre-release `1.0.0` root/snapshot schema copies remain byte-identical and add only those three enum values. The repository canonical Rust digest changed from `sha256:1e86e5934d0d44f854cdccff2f131be21d23290fad9b635051222e2202c9a113` to `sha256:3b49e3d800018c2fd5ed4f6b78c383f56b7da148cdc8e2bd734b250ce5a81906`; only the matching root/release catalog entries changed. Release number, catalog size and conformance fixture inventory are unchanged.

This round does not externalize the paths. Governor extraction, exact ownership/ordinals, profiles, controls and materialization remain round 13b. The only non-protocol production additions outside that boundary are exact enum-to-byte arms required by exhaustive compilation; they do not accept, emit or validate path slots.

### Round 13a RED → GREEN evidence

- Protocol RED failed to compile because `ContentFieldKind::{ContextPath,PermissionPath,IsolationPath}` did not exist.
- After the enum was registered, the schema RED rejected `context_path` as outside the old eight-value enum.
- After root/release schemas changed identically, the catalog integrity RED rejected the stale digest. The canonical Rust API reported `sha256:3b49e3d800018c2fd5ed4f6b78c383f56b7da148cdc8e2bd734b250ce5a81906`.
- GREEN round-trips all three exact Serde values, rejects `filesystem_path`, accepts all three in both root and release validators, rejects the foreign kind in both, proves raw schema parity and validates both catalogs.

### Round 13a focused verification

```text
cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire --locked
  26 passed; 0 failed

cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked
  31 passed; 0 failed

cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/catalog.json
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog --catalog schemas/releases/1.0.0/catalog.json
  both passed; release 1.0.0, 15 schemas, identical persisted projection digest

cargo +1.97.1 run --locked -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
  passed; unchanged, compatible, no release diagnostics

cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance --catalog schemas/catalog.json --fixtures conformance/manifest.json
  50 passed; 0 failed
```

### Round 13a full repository verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  391 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

## Fix round 13b — typed path scopes and closed replay invariants

Governor preparation now inventories the complete `GraphVersionRecord` before its first clone or semantic reconstruction, including every non-`Value` scalar, map key, entrypoint, node/edge endpoint, binding, actor and predecessor string plus bounded collection counts. The shared limit result remains redacted. Context `source_scope.paths`, object permission `scope.paths`, and isolation `filesystem.writablePaths` are extracted before topology construction. Every array element becomes one Node-owned `ContextPath`, `PermissionPath`, or `IsolationPath` Evidence slot with exact authoring-position ordinal, `restricted` sensitivity, and `requiredForExecution=true`.

The three controls retain only closed count and slot bindings. Context includes distinguish `project_kernel`, `document`, `dependency_output`, and `source_scope`; replay requires exact discriminator-specific fields, existing non-self dependency nodes, canonical document references, paired count/index sets, and exact typed slot ownership. Permission and isolation controls enforce the same exact slot/count image. Empty and oversized path arrays fail before sealing, sealer failures expose no path bytes, and serialized safe projections contain none of the path canaries. A same-position content change preserves topology and changes semantic identity; moving the typed authoring position changes both hashes.

Model independence references and context dependency-output references now share replay validation and reject missing or self targets. Node completion `artifactExists` values normalize bounded bare logical names to `artifact://<name>` and encode them only through canonical `refv1`; replay registers `requiresArtifact.*` and `forbidsArtifact.*` exclusively in the Artifact domain. No ArtifactRegistration is fabricated. One shared node-control order registry covers common, kind-specific, and optional controls; Governor sorts by it and replay requires a strictly increasing order, so a reordered/re-hashed projection has no alternate identity.

The Artifact normalization changes only the reviewed Task 5 safe golden literals for official authoring examples that contain bare `artifactExists` values. With explicit controller authorization, `software-feature.yaml` changed topology `sha256:8c9b9ff97232689fac076187c3d0c213659d4ad62d341e737b09bb922756e9de` to `sha256:da66b7fbe11a2669ecbf8593a561dbb4128243a5b7c9736649729317ad0d374c` and semantic `sha256:d328e477fe9855dbd1cb544761c44e08dbf2a805073f184662159abc265dab87` to `sha256:ddc0cbb8dfec14bc85c11d20ae12d30b99c97b513719ee39a437ef9c50fd5327`. `manual-override-deploy.yaml` changed topology `sha256:b2b10a156b538c4beac0b0aadbd9f572627a0db8f6347294121845590c4da6fd` to `sha256:0f5d1d9a52020b549ee9d8fc4ae22e5b3a0117c51b697d6c47f33cd777e877b3` and semantic `sha256:4b5ac3637229a149a79aa64b2a262b67ec9ea4609bf1daaa01a482d21092782f` to `sha256:c01e4e17919bc3b72499e1a8434ac8de9c59de2503a0f9c92cd20915d59045a6`. `research-to-publish.yaml` is unchanged. No fixture, schema, protocol, example authoring file, writer, dependency, or Task 6 code changed.

### Round 13b RED → GREEN and mutation evidence

- Official-shaped path input first failed as `InvalidAuthoring`. GREEN produces six typed slots for the two entries on each of the three surfaces and validates the exact closed replay image without plaintext.
- Direct whole-record preflight first returned `Ok(())` for a 16 MiB + 1 metadata name. GREEN returns `LimitExceeded` before clone or sealer invocation.
- Bare completion artifact names were raw tokens and failed canonical decode. GREEN normalizes and validates only the Artifact reference family.
- A re-hashed projection with reversed node controls was accepted. GREEN rejects it through the shared canonical order registry.
- Missing/self model and context node relations reached projection construction. GREEN rejects them before the first sealer call through the shared replay relation gate.
- Removing the ContextPath, PermissionPath, and IsolationPath extractors one at a time failed their dedicated focused tests. Relaxing the whole-record string inventory, removing the relational validator, removing the completion Artifact domain, and removing the strict control-order comparison each failed its dedicated regression. Every mutation was restored before verification.

### Round 13b fresh verification

```text
cargo +1.97.1 test -p graphhelm-graph --locked
  51 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-governor --locked
  90 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
  passed; 402 tests, 0 failed; all doc tests passed

cargo +1.97.1 clippy --target x86_64-unknown-linux-gnu -p graphhelm-graph -p graphhelm-governor --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

canonical graph validate/lint/hash plus simulate/replay smoke
  passed; simulation completed and replay reconstructed the same terminal state

git diff --check
  passed
```
