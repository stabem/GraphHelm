# Task 6 report — atomic local repository and workspace writer migration

Date: 2026-08-11
Task base: `d75640c023317d69a4082bed79ea281efd1d95e4`
Fix-round-6 base: `01d5f36f7598d895d614d1702beb101e9a8ea1ab`
Delivery shape: multi-commit Task 6 implementation plus focused review corrections

## Status

Implemented the single repository-v1 local Event/Evidence store and migrated every current workspace producer/consumer across the Task 6 implementation and its focused review commits. The journal accepts only the strict 16-variant safe envelope, validates the checked-in schema before commit and on replay, chains event hashes, performs exact request-digest idempotency before sequence checks, publishes sealed Evidence with no-replace same-volume staging, and publishes a derived, disposable, rebuildable active marker last.

The local adapter retains both the root lock and named repository lock, plus anchored directory/file handles. Windows uses the already pinned `windows-sys 0.61.2` by-handle volume/file identity and parent-relative `NtCreateFile`; Unix uses device/inode and handle-relative operations. Root, ancestor, lock, journal, blob, temp and active components reject links/reparse points or replacement. Physical batches, events, journal, pages, cursors, sequences, event/evidence/artifact counts and reads are bounded before expensive work. Truncated/partial physical journal writes fail closed as `GHE005_INTEGRITY_FAILURE`; the repository never guesses or exposes a partial committed prefix.

Governor `apply_draft` now consumes a real `ProjectionPreparation` and commits its safe graph event plus exact `SealedEvidence` in one `PreparedAppend`. Foundation authoring diagnostics, graph records and policy evidence labels cannot enter the writer boundary. Successful tests prove the persisted GraphVersion references real blobs and that the active marker points at the published version. Rejections persist only safe codes/paths; repository files and public Debug/Error surfaces contain no plaintext canaries.

Simulation writes only simulation events. It deliberately does not fabricate a GraphVersion publication. Replay consumes the bounded verified repository-v1 view. Earlier JSONL files and unknown formats return `GHE007_UNSUPPORTED_FORMAT`; there is no compatibility reader, import command or migration branch.

## CLI custody adjudication and Task 11 carry-forward

ADR-022 requires externally mediated key custody, but Task 6 has no approved public operator KeyProvider configuration contract. Therefore `graph draft apply` validates authoring input and recognizes an existing repository-v1 marker read-only, then returns stable redacted `GHK001_KEY_UNAVAILABLE` before creating or mutating repository state. It neither invents an environment variable/flag nor installs an automatic/fake KEK. The former successful CLI waiver test was explicitly replaced by fail-closed and legacy-format preservation tests.

Task 11 must define and implement the operator-facing KeyProvider selection/configuration contract consistent with ADR-022, then wire CLI draft publication to the already-working Governor API. Until then, core Governor publication is complete and tested, while operator CLI publication remains intentionally unavailable.

## RED → GREEN evidence

- Strict event wire RED: the old event envelope failed the first safe fixture with `missing field source`; all 16 variants now strict round-trip through Serde and the checked-in schema.
- Local repository RED: old consumers/API did not compile; the first Windows open then failed because a directory handle had no read access. The retained directory handle was corrected and `local_atomicity` became GREEN.
- Simulation/Governor consumer REDs exposed `:` idempotency values outside `OpaqueId` grammar and Foundation policy labels being misused as sealed Evidence IDs. Producers now use safe keys and persist only real sealed IDs.
- Explicit overflow regression: after the safe gate existed, a mutation replaced it with `base.number() + 1`; `u64::MAX` failed with `attempt to add with overflow`. Restoring `checked_add` returned structured `GHP001_STRUCTURAL_IMPOSSIBILITY` before repository, IDs, candidate work or sealing.
- Windows lock-replacement RED showed creation-time/size identity was insufficient. It was replaced by by-handle volume serial/file index identity; the adversarial test is GREEN.
- Failpoint matrix covers validation, Evidence staging, blob sync/publish, physical batch append, journal sync and active marker. Only an injected half-line makes reopen reject the repository as corrupt; no path returns a dangling committed reference.

## Security and acceptance coverage

- exact retry, divergent idempotency reuse, stale sequence and physical-byte append-only behavior;
- concurrent writers and exclusive serialization;
- checksum/hash-chain corruption, non-contiguous replay and partial tail rejection;
- count/page/cursor/safe-integer bounds before persistence;
- root/ancestor/component symlink or Windows reparse rejection, root and lock replacement detection;
- same-volume temp + sync + hard-link no-replace blob/marker publication;
- real Governor Evidence publication, active marker last, and durable plaintext scans;
- unsupported old JSONL preserved byte-for-byte and no compatibility/import surface;
- strict schema validation for all 16 events and compile-time rejection of raw authoring types.

## Dependency note

No new crate or version was introduced. `graphhelm-schema` is an existing internal workspace dependency used for the required checked-in event validation. The already exact-pinned workspace `windows-sys = 0.61.2` is a target-specific direct dependency of `graphhelm-events`; its approved feature set covers the Foundation, Storage FileSystem, Security, WDK Foundation and System IO declarations required for stable by-handle identity and parent-relative `NtCreateFile` traversal unavailable in Rust 1.97.1 std.

## Verification

Focused GREEN evidence:

```text
cargo +1.97.1 test -p graphhelm-events --tests --locked
  53 passed; 0 failed
cargo +1.97.1 test -p graphhelm-governor --all-features --locked
  115 passed; 0 failed
cargo +1.97.1 test -p graphhelm-simulation --test deterministic_simulation --locked
  6 passed; 0 failed
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  9 passed; 0 failed
cargo +1.97.1 test --workspace --all-features --locked
  476 passed; 0 failed, including doc tests
```

Final gates also passed: workspace formatting and warning-denying Clippy, full workspace tests, CLI smoke, locked metadata (10 workspace members), Linux-target warning-denying Clippy for every changed core crate, and `git diff --check`. The canonical CLI smoke validated/linted/hashed `software-feature.yaml` (`sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6`), then simulated and replayed repository v1. The first event used the normative genesis hash `sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3`; replay reconstructed terminal node state with `simulationStatus: completed` and `currentGraph: null` by design.

## Fix round 1 — recoverable authority and anchored reconciliation

The durable journal is now the authority for active Graph Versions. Open validates the complete journal once, derives active versions and expected markers in one event pass, scans every known repository component without mutation, and only then removes proven orphans and republishes missing committed markers. A crash after journal sync therefore reopens the committed version and a sibling successor from the old base is rejected. Marker lookup is `O(events + markers)`, not a marker-by-event rescan.

`GraphVersionPublished` is validated with the shared closed persisted-projection validator at all three boundaries: append preflight (`GHE004`), durable journal load (`GHE005`), and replay (`ReplayError::Corrupt`). The Governor now binds concurrency to both version number and the safe semantic predecessor hash before externalization or append. Idempotency is scoped to `(scope, stream, key)`, and physical batches pass bounded raw JSON, canonical-form and a closed Draft 2020-12 schema before typed deserialization.

All repository components are anchored to retained handles. Unix operations use component-walk `openat`, no-follow identity, retained-inode `linkat`, identity-checked `unlinkat`, and handle-relative enumeration. Linux first uses `AT_EMPTY_PATH` and has an unprivileged `/proc/self/fd/<fd>` + `AT_SYMLINK_FOLLOW` fallback that still names the retained descriptor rather than the mutable temp entry. Windows retains reparse-point handles without delete sharing, validates regular/directory type and reparse attributes on every opened handle, verifies stable volume/file identity immediately before and after `CreateHardLink`, and deletes planned files through the retained `DELETE` handle. The Windows name API is safe here because the retained temp handle omits `FILE_SHARE_DELETE`, so rename/removal/replacement is kernel-denied until publication completes; no unsupported NT API or feature was introduced. Swap tests accept only exact sharing denials 5/32, while symlink tests skip only privilege error 1314.

Initialization first classifies without mutation, acquires/creates the retained lock only for a recognized partial layout, reclassifies under that lock, and writes `format.json` last. A complete v1 marker with any missing component fails closed and recreates nothing. Directory enumeration charges the shared entry/name budget before cloning, visiting, opening, or descending. Durable load likewise charges aggregate batch/event/Evidence/artifact/publication/work budgets before blob I/O and proportional inserts. Reconciliation retains every candidate handle/identity through the mutation phase, so a replaced name is never deleted; a late unknown entry still causes zero prior deletion.

Stored Evidence reopening now verifies every reference and sealed metadata field, ciphertext and AAD digests, algorithms, nonce/ciphertext shapes, wrapped-key identity, scope, media/sensitivity/retention, and the reconstructed original request digest. Digest reconstruction sorts Evidence by `evidenceId`, matching the durable sorted ID set. `evidence_exists` requires committed reachability, so a physically published pre-journal orphan is never reported as committed.

Successive Graph publications are now lineage-checked, not merely valid in isolation: append preflight, durable load, and replay require the next number plus the exact active predecessor semantic hash. The first persisted publication may still reference an external predecessor, preserving the repository bootstrap contract. Append and durable-load Evidence verification build bounded indexes once, verify each committed `(scope, evidenceId)` blob at most once per operation, and reject divergent repeated references without quadratic declaration scans. Durable load retains only the compact verified request-digest material, with its aggregate serialized bytes charged before insertion, rather than retaining every blob ciphertext in memory.

Reconciliation authorizes deletion only for canonical Stored Evidence bytes whose identity matches the filename. Temporary ownership is exact: only `blob-<64 lowercase hex>.tmp` and `active-<64 lowercase hex>.tmp` are recognized. Noncanonical or prefix-lookalike files are unknown input, cause `GHE005_INTEGRITY_FAILURE`, and leave the complete deletion plan unapplied.

### Round-1 RED and mutation evidence

- Active-marker failpoint originally reopened without an active version; removing journal-derived active insertion reproduces the failure and permits the stale lineage path.
- Invalid rehashed topology was accepted independently at append, durable load, and replay; removing each restored gate makes its dedicated test fail.
- Removing the Governor safe-hash comparison reaches the forbidden append path; restoring it returns `GHD002_STALE_HASH` with zero externalizer/repository effects.
- Removing the scope/stream idempotency filter reintroduces cross-stream conflict; removing committed reachability makes the BlobPublish orphan appear committed.
- Removing physical-batch schema validation accepts the typed-Serde digest sentinel; removing reconstructed request-digest verification accepts a nonce mutation.
- The stored-Evidence matrix mutates format, scope, reference IDs/digests, media, sensitivity, retention, evidence algorithm/nonce/ciphertext, and wrapped key ID/handle/algorithm/nonce/ciphertext/AAD independently; every reopen fails closed.
- Explicit inclusive matrices cover `limit-1`, `limit`, and `limit+1` for the 1 MiB event, 16 MiB batch, and 64 MiB journal bounds.
- Publishing/deleting by mutable path failed the retained-identity race tests. Removing the Windows identity gate publishes a swapped name; replacing handle deletion with pathname deletion removes the replacement. Both mutations failed and were restored.
- Moving directory accounting after the visitor observes the forbidden second entry; removing the global Evidence-reference counter accepts `limit+1`. Treating complete-v1 missing content as a recoverable partial layout recreates state. Each mutation failed and was restored.
- Bootstrap-lock race RED initially rejected the initializer that lost `create_new`; the repaired path opens the winner's retained lock, reclassifies under it, and completes or recognizes the same exact layout.
- Removing the append or replay successor check accepts an individually valid v3 whose predecessor hash does not identify committed v2; both dedicated tests fail under those mutations and pass after restoration.
- Removing canonical-byte comparison causes a noncanonical orphan to be deleted; broadening the temp predicate claims a prefix-lookalike user file. Both zero-mutation regressions fail under mutation and were restored.
- Removing the repeated-reference cache makes the focused append verifier call its committed-blob reader twice for one Evidence identity; the mutation failed and the single-read gate was restored.

### Round-1 verification

Fresh on Windows with Rust 1.97.1: workspace fmt and warning-denying Clippy passed; the full all-features workspace test suite and doc tests passed with zero failures; CLI smoke passed 9/9; locked metadata passed. Linux-target warning-denying Clippy passed for `graphhelm-events`, `graphhelm-governor`, and `graphhelm-schema`, including all targets/features. Schema catalog reported 15/15, compatibility was unchanged, and conformance passed 50/50. Official validate/lint/hash/simulate/replay passed with the unchanged software-feature hash `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6`; `git diff --check` passed.

No new crate or version was introduced. The user authorized only the minimal `core/schema/src/{lib.rs,registry.rs}` expansion to reuse the already pinned offline Draft 2020-12 validator for the internal physical batch. Existing exact-pinned `libc 0.2.189` was added to `graphhelm-events` for the required Unix handle-relative primitives, and the workspace authorization comment was broadened accordingly.

## Fix round 2 — root serialization, immutable artifacts and verified replay

Unix now serializes repository open/recovery and every locked operation in one fixed order: per-instance gate, retained root-directory FD, retained named lock, then journal/component handles. The stable root inode stays locked if `repository.lock` is renamed and recreated, preventing a second instance from entering through a replacement lock inode. Windows retains its non-delete-share handle protections. A Unix-only deterministic seam replaces the named lock after validation, proves that replacement inode is independently lockable, and proves a second FD for the same root inode cannot acquire the root lock.

Reconciliation no longer performs the prior Unix `fstatat` followed by `unlinkat`. POSIX has no portable compare-and-unlink-by-retained-FD primitive, so reconciliation validates the planned identity and conservatively preserves the orphan. A replaced name fails integrity validation and neither inode is removed. Windows continues deleting the retained inode by handle. This supersedes the round-1 claim that identity checking made Unix name-based orphan deletion safe.

Artifact registrations are indexed by the exact `(producerIdempotencyKey, artifactId)` relation extracted uniformly from each envelope's `artifactRefs`. Every registration must match the reference carried by that producer in the same scoped stream batch; unreferenced and cross-event registrations fail. The durable catalog stores the complete registration. Exact retries remain idempotent, while any locator, digest, media metadata, byte length, sensitivity, metadata version, or producer-relation divergence fails before append. Durable load independently rejects a correctly hashed and checksummed historical batch containing a divergent catalog entry.

Event and physical-batch sizing now uses the bounded counting writer over the real serialized structures. The synthetic 4 KiB charge per event was removed. Per-event counting precedes JSON `Value` and canonical buffers, and the complete physical batch is counted before its durable line is allocated. A real append with 4,096 small events remains under the actual 16 MiB and 10,000-event limits. The shared counter has inclusive `L-1/L/L+1` coverage.

Public `replay` is now a full integrity boundary. Before projection it bounds every envelope, converts it to JSON, validates the complete checked-in Draft 2020-12 schema, requires one exact `RepositoryScope` and stream, verifies sequence and predecessor chain, recomputes `eventHash`, and then applies semantic GraphVersion validation. Correctly rehashed schema and scope mutations fail independently of the hash-chain gate.

The strict event/batch schema set is compiled once per process with `OnceLock`. Repository open resolves the set before root or named locks and stores a static reference, so append/load loops can only receive an already compiled validator. A 2,048-iteration event+batch matrix records at most one compilation. No schema JSON, catalog digest, dependency, crate, release, compatibility reader, or legacy path changed; the user-authorized expansion is limited to `core/schema/src/lib.rs` and `core/schema/src/registry.rs`.

### Round-2 RED and mutation evidence

- Producer mismatch and unreferenced registrations were accepted before the exact relation map. Removing the restored predicate accepts the adversarial batch and fails its regression.
- A second append could replace an ArtifactId's metadata. Removing the immutable comparison accepts the divergent append; durable-load coverage separately rejects divergent historical bytes.
- Restoring synthetic `+4096` charging rejects the valid 4,096-event request with `GHE006_LIMIT_EXCEEDED`; exact counting commits all events.
- Removing replay hash recomputation accepts a tampered terminal hash; removing schema validation accepts a correctly rehashed `graphVersion: 0`; removing scope equality accepts a correctly rehashed cross-scope event.
- Before caching, eight event/batch pairs compiled the registry 16 times. The restored 2,048-pair matrix completes with one process compilation.
- Unix-only Rust tests compile under warning-denying Linux Clippy but are not executable on the Windows Rust host. Fresh WSL runtime probes proved that a second FD cannot lock the retained root inode after named-lock replacement and that name-based unlink targets a replacement inode after the check/use swap. The target-gated Rust seams encode those two assertions; mutation execution remains a Linux-host CI responsibility.

### Round-2 focused verification

Fresh focused Windows evidence after restoring mutations: `graphhelm-events` passed 59/59 tests, `graphhelm-schema` passed 23/23 tests, and warning-denying Clippy passed for both crates. Linux-target warning-denying Clippy passed for both crates, including Unix-only production and test code. Full workspace and official gates are refreshed after this section.

### Round-2 final verification

Fresh Rust 1.97.1 gates passed: workspace format, warning-denying Clippy for all targets/features, full all-features workspace tests and doc tests, CLI smoke 9/9, locked metadata, and `git diff --check`. Linux-target warning-denying Clippy passed for `graphhelm-events` and `graphhelm-schema`, including target-gated tests. Schema catalog remained 15/15 at release `1.0.0`, compatibility remained unchanged, and conformance passed 50/50. Official validate/lint/hash/simulate/replay passed; the canonical software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6`.

## Fix round 3 — scoped replay and complete crash recovery

Repository replay is now selected by exact `RepositoryScope + streamId`. The public replay boundary rejects foreign scope/stream records, binds execution-scoped `GraphVersionPublished` events to the persisted topology execution, requires the exact ordered content-slot/Evidence-reference bijection, and rejects more than 100,000 events before per-event work while retaining aggregate byte/reference budgets. The CLI accepts bounded workspace/project/execution/stream selectors; selector-free replay succeeds only for one validated stream and returns `GHE010_STREAM_SELECTION_REQUIRED` for an ambiguous repository.

Artifact identity now persists the producing stream and rejects cross-stream aliasing on append/load/reopen. Evidence retry reads the complete encoded `StoredEvidence` bound. Governor application constructs the authoring Graph Version once, externalizes that exact version and returns the same version committed in `GraphVersionPublished`; an advancing-clock regression proves timestamp/hash/version identity. `next_sequence` uses one bounded state load.

Temporary blob/active-marker names combine the production UUID seam with a monotonic repository counter and bounded `create_new` retries. Unix performs no name-based unlink after identity validation and preserves normal/reconciliation temps; Windows retains delete-by-handle. Missing roots are created component-by-component from a retained existing ancestor: Unix `mkdirat/openat(O_NOFOLLOW)`, Windows create/open/reparse validation with retained handles. No `create_dir_all` missing-chain traversal remains. Embedded repository schemas use an explicit rejecting retriever at registry and validator stages.

### Round-3 RED and mutation evidence

- Removing the 100,000-event preflight returned corruption after per-event work; removing execution identity or ordered Evidence bijection accepted a mismatched publication.
- Removing unique-stream cardinality made two-stream CLI replay silently succeed; removing producer-stream identity let the Artifact relation commit under a second stream.
- Restoring the 16 MiB read bound made the maximum valid Evidence retry fail with `GHE006` before the injected `GHE008` failure.
- Removing the explicit retriever left the external file URI unobserved; adding a second state load made the `next_sequence` counter report two.
- Removing component-create observation bypassed the root attack seam. Fixed-ID collision and preserved-crash-temp mutations proved bounded unique allocation.
- The advancing clock originally produced different authoring/prepared versions; the single-publication path now returns the exact committed preparation.
- Unix cleanup mutation execution remains a Linux CI responsibility because this Windows host's WSL has no Rust toolchain; Linux-target Clippy compiled the target-gated preserving path and tests.

### Round-3 final verification

Fresh Rust 1.97.1 gates passed: workspace format, warning-denying workspace Clippy, full all-features workspace tests/doc tests, CLI smoke 10/10, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Schema, Events, Governor and Simulation. Catalog remained 15/15 at `1.0.0`, compatibility unchanged, conformance 50/50. Official validate/lint/hash/simulate/replay passed with canonical hash `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay `simulationStatus: completed`. No schema JSON, catalog digest, dependency, crate, release, migration, compatibility reader or legacy path changed.

## Fix round 4 — exact retries, publication identity and derived marker recovery

The repository now validates intrinsic request structure and complete prepared `SealedEvidence` metadata, computes the canonical digest, and resolves exact `(scope, stream, idempotencyKey)` retries before successor and stale-sequence checks. A journal-synced `GraphVersionPublished` whose marker write failed returns its original envelopes on retry; divergent reuse still fails before mutation.

Evidence IDs now use one graph-layer domain-separated derivation over repository scope, graph number, safe semantic hash and exact typed slot position. Governor preparation, append, durable load and public replay share it. Arbitrary internally consistent IDs are rejected while Evidence IDs remain outside topology and semantic hash materials. Prepared Evidence uses the complete durable metadata/AAD validator before staging. The Governor independently reconstructs the exact expected safe projection and requires full equality with the public externalizer result, covering lineage, timestamp, actor, topology, hashes and slots.

Active markers are disposable projections of the authoritative journal. Missing, truncated, malformed, stale or divergent markers no longer invalidate a sound journal; a correct repair marker is published under the repository locks without rewriting journal bytes. Unix retains its conservative no-name-unlink rule and may preserve corrupt derived files. Simulation obtains the bounded next sequence with exactly one direct repository load.

Windows root traversal starts at the volume root and opens or creates every component atomically relative to the retained parent through `NtCreateFile`, `OBJECT_ATTRIBUTES.RootDirectory`, `FILE_OPEN_IF`, directory-only/reparse-point flags and no delete sharing. Returned handles are validated as non-reparse directories. The exact `windows-sys 0.61.2` pin is unchanged; only the user-authorized minimal WDK/Security/System-IO features were enabled.

Seven isolated mutations were observed and restored: successor-before-retry returned `Invalid`; removing prepared metadata validation committed invalid AAD; number-only Governor comparison committed a divergent actor; removing replay derivation accepted arbitrary IDs; strict marker parsing blocked recovery; paginated simulation hit the forbidden read; and delete-sharing allowed the Windows component swap.

### Round-4 final verification

Fresh Rust 1.97.1 gates passed: workspace format, warning-denying workspace Clippy for all targets/features, full all-features workspace tests and doc tests, CLI smoke 10/10, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Graph, Events, Governor and Simulation. Catalog remained 15/15 at `1.0.0`, compatibility remained unchanged, and conformance passed 50/50. Official validate/lint/hash/simulate/replay passed from a fresh event repository; the canonical software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay reported `simulationStatus: completed`.

No crate or version, schema/protocol/catalog, compatibility reader, legacy path, fake-success path or Task-7 behavior was introduced. The only dependency-surface change is the explicitly authorized minimal feature expansion of the already pinned `windows-sys 0.61.2` for WDK/Foundation, Storage FileSystem, Security and System IO declarations required by `NtCreateFile` and parent-relative `OBJECT_ATTRIBUTES.RootDirectory` traversal.

## Fix round 5 — total derived recovery, publication preflight and bounded simulation keys

The committed journal remains authoritative when an active marker is missing or contains derived-state garbage. Canonical marker collisions no longer propagate marker read/open, size, parse, canonicalization or integrity failures. Recovery writes the journal-derived marker through the retained temporary handle and tries up to 16 collision-resistant, domain-separated no-replace repair names. A corrupt pre-created legacy deterministic repair filename can no longer brick reopen; journal bytes remain unchanged, and Unix orphan cleanup remains conservative.

`apply_draft` now invokes the exact borrowed base/draft/actor preflight shared with `prepare_draft_publication` after the existing stale checks and before candidate clone/application. This closes hard operation count, graph `maxMutations`, audit ID/actor/manual-override, JSON depth/value/byte and shared aggregate accounting at the real Task 6 publication path. Preflight failure returns the stable bounded error without appending a rejection, advancing IDs, sealing Evidence or activating a version.

Simulation event idempotency keys are fixed-length SHA-256 derivations over length-framed, domain-separated phase, simulation ID, optional node ID and ordinal material. Started, transition and completed keys no longer concatenate wire identifiers. Maximum valid 128-byte simulation/node IDs publish successfully; repeated input is stable, and changing the simulation, node or ordinal changes the key without copying authoring identifiers into the journal key.

### Round-5 RED and mutation evidence

- A canonical marker of `MAX_EVENT_BYTES + 1` originally propagated `GHE006_LIMIT_EXCEEDED`; restoring a fallible marker read mutation makes the focused recovery test fail again.
- Corrupt canonical plus pre-created corrupt `repair-<expected digest>.json` markers originally propagated integrity/idempotency failure; restoring the deterministic repair name makes the focused test fail. The bounded unique no-replace repair loop restores recovery.
- Seven valid patches against an example graph with `maxMutations: 6` originally published eight events. Removing the shared `apply_draft` preflight reproduces that publication and fails the zero-effect regression.
- Maximum valid 128-byte simulation and node IDs originally failed raw concatenated transition-key construction. Restoring the concatenation independently fails the real simulation regression. Separate unit coverage proves stability and simulation/node/ordinal separation.

### Round-5 verification

Focused post-mutation GREEN evidence: Events unit tests passed 31/31, Governor draft-application integration passed 6/6, and Simulation unit/integration passed 9/9. Warning-denying focused Clippy passed for Events, Governor and Simulation.

Fresh Rust 1.97.1 gates passed on Windows: workspace format, warning-denying workspace Clippy for all targets/features, the complete all-features workspace suite and doc tests with zero failures, CLI smoke 10/10, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Events, Governor and Simulation. Schema tests, catalog integrity, unchanged compatibility and conformance passed. Official validate/lint/hash/simulate/replay passed from a fresh repository; the software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6`, replay reported `simulationStatus: completed`, and journal idempotency keys contained only fixed `ev-<sha256>` derivations. Post-commit clean-state verification is recorded after the local commit.

No crate/version/feature, schema/protocol/catalog, fixture/golden, compatibility/legacy, fake-success, or Task 7 change was introduced.

## Fix round 6 — committed Governor retry and closed publication lineage

The Governor now derives one bounded request identity from length-framed, domain-separated scope, stream, base, authoritative actor and complete preflighted draft input. Every proposed, obligation, waiver, publication, applied and rejection key is a fixed-length `gov-<sha256>` derivation. A read-only repository lookup by the derived proposed key runs before the normal active-version stale gate. If a prior append committed the authoritative physical batch but active-marker publication reported failure, an exact retry reconstructs the candidate and policy deterministically, reuses the committed timestamp, actor, waivers and envelopes, validates the committed safe projection, and returns the original result without resealing or mutation. A different sibling derives a different identity and remains stale before mutation.

Graph publication lineage is closed at every persistence boundary. An empty stream accepts only v1 with no predecessor; v2+ requires the exact active committed predecessor. Governor application against an unseeded repository fails with zero journal/marker/Evidence effects, and integration setup explicitly seeds the safe v1 projection before applying v2. Append preflight, durable load and public replay also require the publication envelope actor to equal `PersistedGraphVersion.createdBy`.

Publication preflight uses one `PersistencePreflight` aggregate across the borrowed base semantic value, draft operations, authoritative actor and manual override. The resulting candidate is then preflighted immediately after isolated application and metadata update, before Serde, schema validation, lint, policy or sealing. A disjoint near-limit base plus patch now returns `GHE006_LIMIT_EXCEEDED` before candidate clone, and a structurally oversized post-apply candidate returns the same code before serialization or externalizer invocation.

Active-marker repair names are stable functions of expected marker content and a bounded fallback ordinal, never of a fresh temp identifier. A corrupt canonical marker and corrupt pre-created primary repair candidate therefore select one stable valid fallback; repeated reopen of an unchanged authoritative journal reuses it without growing the active directory. Unix remains conservative and performs no unsafe name-based unlink.

### Round-6 RED and mutation evidence

- Suppressing the committed-key lookup restored the observed `StaleVersion` exact-retry failure after journal sync; restoring it returned the original committed batch and version. The sibling request remained stale.
- Removing the empty-stream genesis rule committed v2 into an empty repository. Removing actor binding independently made append preflight, a rehashed/redigested durable batch, and public replay accept mismatched provenance.
- Restoring raw `<draft>-proposed` concatenation made a maximum valid 128-byte draft ID fail after sealing with `InvalidOperation`; fixed-length derivation restored success.
- Removing base accounting let the disjoint base+patch cross `CandidateClone`. Removing the post-apply candidate gate reached schema serialization and changed the stable limit result to structural failure. Both gates were restored.
- Reintroducing the temp UUID into the repair-name hash increased derived marker files from two to three on the second unchanged reopen. Stable expected-content naming restored constant file count, including a corrupt pre-created primary candidate.

### Round-6 verification

Fresh Windows focused evidence after restoring every mutation: Events passed 78/78 tests, Governor passed 122/122 tests and Simulation passed 9/9 tests/doc tests. Warning-denying Clippy passed for Events, Governor and Simulation.

Fresh Rust 1.97.1 pre-commit gates passed: workspace format, warning-denying workspace Clippy for all targets/features, the complete all-features workspace suite and doc tests with zero failures, CLI smoke 10/10, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Events, Governor and Simulation, including target-gated production/tests. Catalog remained 15/15 at `1.0.0`, compatibility remained unchanged, and conformance passed 50/50. Official validate/lint/hash/simulate/replay passed from a fresh repository; the software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay reported `simulationStatus: completed`. Post-commit clean-state verification is recorded in the task ledger.

No crate/version/feature, schema/protocol/catalog, fixture/golden, compatibility/legacy, fake-success, or Task 7 behavior changed. The EventRepository surface gained only the bounded, read-only exact-key recovery lookup required to distinguish a committed authoritative batch from a stale sibling before Governor concurrency rejection.

## Fix round 7 — plaintext gate and durable retry outcomes

Direct local append now applies the shared deterministic durable-content scanner before request digesting, canonical allocation or durable effects. Streaming serialized-size checks bound `NewEvent` and generated envelopes before `serde_json::Value` allocation. Scope/stream, actor, idempotency, payload strings, Evidence references and non-ciphertext metadata, wrapped-key identities, Artifact references and producer identities are covered. Generated envelope identity is scanned before event hashing. Secret-shaped plaintext returns the redacted `GHE009_EXTERNALIZATION_FAILED` with unchanged journal/blobs/markers; ciphertext and nonce bytes are deliberately not reinterpreted as plaintext and the canonical simulation remains accepted.

Committed Governor recovery now separates the exact validated `DraftRejected` image from the successful `GraphVersionPublished + DraftApplied` image. A rejection retry maps the closed reason and matching safe diagnostic back to the original stable apply error, appends no second rejection and leaves journal bytes unchanged. Mixed, unknown or divergent committed images remain integrity failures.

Derived-marker recovery first indexes existing marker bytes once per stream, preserving `O(events + markers)` recovery and reusing an exact valid marker without temp growth. When none exists, up to 16 no-replace attempts derive names from the production UUID ID seam under the `graphhelm-active-repair-v2` domain. A canonical collision, all 16 historical deterministic v1 names and four injected v2 collisions no longer exhaust recovery; Unix still performs no unsafe name unlink.

Every complete canonical journal observed after a failed `JournalSync`, reopen or exact committed-batch lookup is explicitly `sync_data`-ed through the retained journal handle under the repository locks before marker publication or retry acknowledgement. The injected sync-failure seam returns `GHE008_STORAGE_FAILURE` with no marker; a normal reopen records the successful sync before rebuilding the marker. Torn/non-newline batches retain their existing fail-closed integrity behavior.

### Round-7 RED and mutation evidence

- Before the gate, a scope canary and generated event ID `sk-abcdefghijklmnopqrst` were durably accepted. Removing the restored append scanner independently lets unsafe Evidence key metadata commit; removing the envelope scanner accepts the generated ID. Ciphertext carrying the same bytes remains valid.
- The second exact stale-hash application originally returned `GHE005_INTEGRITY_FAILURE`; removing the rejection classifier reproduces it. The restored path returns `GHD002_STALE_HASH`, one durable rejection and identical journal bytes.
- Precreating canonical plus all 16 old deterministic repair names originally returned idempotency conflict. Reverting the v2 entropy path reproduces the failure; the deterministic ID seam now observes four new collisions and eventual no-replace success.
- After an injected complete `JournalSync` failure, the second append originally returned success and promoted the marker. Neutralizing the restored resync reproduces it; the sync observer/failure seam proves sync-before-promotion on retry and reopen.

### Round-7 verification

Fresh focused evidence after restoring all five mutations: Events passed its complete 82-test/unit-integration-doc surface, Governor draft application passed 10/10, and warning-denying focused Clippy passed. Fresh Rust 1.97.1 Windows gates passed: format, warning-denying workspace Clippy for all targets/features, the complete all-features workspace suite and doc tests, CLI smoke 10/10, schema CLI 27/27, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Events and Governor. Catalog remained 15/15 at `1.0.0`, compatibility remained unchanged and conformance passed 50/50. Official validate/lint/hash/simulate/replay passed from the fresh `target/graphhelm-round7-smoke-events` repository; the software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay reported `simulationStatus: completed`.

No crate/version/feature, schema/protocol/catalog, fixture/golden, compatibility/legacy, fake-success or Task 7 behavior changed.

## Fix round 8 — closed committed Governor outcome grammar

Governor retry recovery now passes both committed terminal images through one closed parser before interpreting an outcome. The parser requires the exact request-derived `DraftProposed` envelope first, exact scope/stream/actor/sensitivity, a unique canonical obligation prefix, a unique waiver prefix with request-derived keys, and exactly one terminal image: `DraftRejected`, or `GraphVersionPublished` immediately followed by its matching `DraftApplied`. Success obligations are compared byte-for-typed-field with the freshly evaluated deterministic policy report; rejection obligations are reconstructed from the exact rejection stage. Waivers are bound to the authoritative override, requirement order, actor, execution and next Graph Version. Unknown kinds, extra events, duplicates, mixed terminals, foreign identities, reordered prefixes and schema-valid direct append inventions all return the redacted `GHE005_INTEGRITY_FAILURE` family through `InvalidProjection`.

### Round-8 RED and mutation evidence

- Before the common parser, inserting a valid `GraphImported` into a committed success returned `ApplyResult`; inserting it into a committed rejection returned the original stale-hash error. Both focused tests failed for the expected missing-gate reason before production changes.
- The restored matrix rejects all ten non-Governor event variants in both terminal images, duplicated Proposed/obligation/terminal events, success/rejection mixtures, foreign actors and draft IDs, reordered obligations, and foreign or misplaced waivers. Exact success and rejection batches remain recoverable.
- A success-only mutation filtered the unrelated event before the common gate and made the success matrix fail by returning `ApplyResult`. After restoration, a rejection-only mutation did the same and made the rejection matrix fail by returning `GHD002_STALE_HASH`. Both mutations were removed and the focused matrix returned green.

### Round-8 verification

Fresh Rust 1.97.1 Windows verification passed: focused committed-outcome tests, complete Governor tests, format, warning-denying Governor and workspace Clippy for all targets/features, complete locked all-features workspace tests and doc tests, CLI smoke 10/10, locked metadata and `git diff --check`. Linux-target warning-denying Clippy passed for Events, Governor and Simulation. Catalog remained 15/15 at `1.0.0`, compatibility remained unchanged and conformance passed 50/50. Fresh official validate/lint/hash/simulate/replay commands passed from `target/graphhelm-round8-smoke-events`; the software-feature hash remained `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay reported `simulationStatus: completed`.

No crate/version/feature, schema/protocol/catalog, fixture/golden, compatibility/legacy, fake-success, Event writer behavior or Task 7 behavior changed.
