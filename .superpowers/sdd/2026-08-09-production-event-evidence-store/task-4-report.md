# Task 4 report — encrypted Evidence, artifacts, and sealed keys

Date: 2026-08-09
Base: `34e538b5948981d0d057469b83bbcf19f6734c7b`
Branch/worktree: `issue-5-production-event-evidence-store` / `F:\github\GraphHelm\.worktrees\issue-5-production-event-evidence-store`

## Status

Task 4 is implemented within its approved file scope. It adds an adapter-neutral encrypted Evidence boundary, immutable artifact registration validation, the object-safe `KeyProvider` contract, and a durable local `SealedKeyProvider`. It does not replace event envelopes or JSONL, externalize Governor content, implement retention/PostgreSQL, or alter schemas. Fix round 5 clarifies only the already accepted provider rollback limitation in the focused threat model.

## RED → GREEN chronology

1. The complete core integration test was written before production modules. After refreshing the lockfile for the test-only `zeroize` dependency, `cargo +1.97.1 test -p graphhelm-events --test evidence_crypto --offline` failed with E0432 because every Evidence/key/artifact import was absent. This was the intended interface RED.
2. Minimal core implementation made `cargo +1.97.1 test -p graphhelm-events --test evidence_crypto --locked` GREEN: 7 passed, 0 failed.
3. The sealed-provider integration test and crate harness were then added before adapter behavior. `cargo +1.97.1 test -p graphhelm-sealed-key-provider --test sealed_provider --offline` failed with E0432 because `SealedKeyProvider` was absent. This was the adapter RED.
4. The first adapter GREEN attempt compiled but all creation cases returned the stable `Storage` error on Windows. A temporary stage-isolation unit probe proved directory `sync_all` was the failing operation. Opening the Windows directory handle with read+write access plus `FILE_FLAG_BACKUP_SEMANTICS` made the probe pass; the probe was removed.
5. Dependency audit found that authentication still used an AEAD-empty tag instead of the planned HMAC-SHA-256. A focused assertion failed with actual algorithm `xchacha20poly1305` versus required `hmac-sha256`. After exact dependency correction, the focused test passed. The full reopen suite then found one residual 16-byte keyring-tag check; a temporary split probe localized it to keyring verification, it was corrected to 32 bytes, and the probe was removed.
6. Final focused GREEN: Evidence 7/7, sealed provider 6/6, focused Clippy warning-free.

## Implemented contracts

- `SecretBytes` owns `Zeroizing<Vec<u8>>`, implements explicit `Zeroize`, and exposes bytes only through borrowed callbacks. It has no `Clone`, `Debug`, `Display`, or `Serialize`; consuming exposure drops and zeroizes afterward.
- `EvidenceInput` validates nominal Evidence ID, media type, closed retention class, and the 16 MiB plaintext limit before crypto. `seal_batch` validates the checked 64 MiB aggregate before the first provider call.
- `EvidenceProtector<K>` generates a fresh 32-byte DEK and 24-byte XChaCha nonce through `getrandom::fill` for every item, encrypts with XChaCha20-Poly1305, hashes plaintext/ciphertext with SHA-256, wraps the DEK through `KeyProvider`, and zeroizes plaintext/DEK buffers on every success and failure path.
- Evidence AAD is an unambiguous length-prefixed binary sequence binding domain/version, exact workspace/project/optional-execution scope, Evidence ID, `1.0.0` schema version, media type, sensitivity, retention class, and content SHA-256. Opening recomputes scope, ciphertext digest, AAD digest, AEAD tag, plaintext length, and content digest before returning `SecretBytes`.
- `WrappedKey` carries the Evidence-AAD digest because the fixed object-safe `unwrap(wrapped)` signature does not accept AAD. The provider additionally binds the handle plus that digest in its wrapping AAD; altering handle, nonce, ciphertext, metadata, or the stored AAD digest fails closed.
- `ArtifactRegistration` enforces the local `artifact://sha256/<digest>` / `contentSha256` sibling equality and validates the producer idempotency key without dereferencing the locator.
- All public failures use stable generic codes/messages and retain no plaintext, key, path, crypto-library error, or backtrace.

## Sealed provider format and durability

- Key material is supplied explicitly as a 32-byte `SecretBytes`; it is never read from environment/arguments and is never persisted. HMAC-SHA-256 domain-separated subkeys derive wrapping/authentication material from the supplied key.
- DEKs are wrapped with XChaCha20-Poly1305 using a fresh 24-byte nonce. Provider authentication, keyring tags, journal-header/record-chain tags, and receipt tags use full 32-byte HMAC-SHA-256.
- `keyring.v1.json` stores only format version, opaque key ID, and HMAC. Publication writes a restricted-permission same-directory pending file, flushes and syncs it, publishes by atomic hard-link no-replace, syncs the directory, removes the pending file, and syncs again. Existing destinations and symlinks are never overwritten.
- `revocations.v1.jsonl` begins with an authenticated header. Each bounded record binds monotonic epoch, handle, idempotency key, previous record tag, and receipt tag. Appends run under one cross-platform exclusive lock, reload and verify the complete bounded journal, append one newline-terminated record, flush, file-sync, and directory-sync.
- Exact idempotency retry reconstructs the original field/tag-equivalent receipt; divergent reuse fails with `KeyError::Conflict`. `unwrap` reloads revocations under the same lock and rejects a revoked handle even when given stale wrapped-key bytes after reopen.
- Truncated, reordered, malformed, tag-invalid, oversized, symlinked, missing, or wrong-key state fails closed. Reads/appends use `O_NOFOLLOW` on Unix and `FILE_FLAG_OPEN_REPARSE_POINT` on Windows; `libc = 0.2.189` is exact-pinned solely for the Unix `O_NOFOLLOW` constant.
- A crash during keyring staging leaves only the fixed owned pending file, which the next locked create removes without following it. A partial journal append is not guessed or truncated; reopen fails integrity so an operator can restore authoritative provider state.

## Dependency audit

Direct versions/features match the plan:

- `chacha20poly1305 = 0.11.0`, default features disabled, `alloc` + `zeroize`;
- `getrandom = 0.4.3` (the lock still contains 0.3.4 only as an unrelated transitive version);
- `hmac = 0.13.0`;
- `zeroize = 1.9.0` with `derive`;
- `static_assertions = 1.1.0` test-only;
- `libc = 0.2.189` adapter-only for Linux no-follow opens.

`base64` and `tokio` were not added because this task does not use them. Core has no filesystem/provider dependency and the adapter depends inward on `graphhelm-events`/`graphhelm-protocols` only.

## Verification

Fresh required gates after the final change:

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-events --test evidence_crypto --locked
  7 passed; 0 failed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  6 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  262 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

The actual symlink branch is exercised on platforms/hosts that allow unprivileged symlink creation. This Windows host returned OS error 1314, and the test follows the repository's established exact-error skip rule; existing-destination/no-overwrite and no-follow/reparse code paths remain exercised without elevation.

## Adversarial self-review

- Nonce/key reuse: every Evidence DEK/nonce and every wrapped-DEK nonce comes from OS randomness; no production deterministic injection exists.
- AAD ambiguity: every variable field is length-prefixed and optional execution identity has an explicit presence byte; metadata and content digest are recomputed on open.
- Oracle/error leakage: crypto/storage causes collapse into stable redacted enums; no error contains user bytes, keys, paths, temporary names, or library prose.
- Resource exhaustion: item, aggregate, AAD, authenticated bytes, keyring, journal, record-count, and record-size limits execute before expensive work or allocation where applicable.
- Filesystem: fixed filenames only, validated non-symlink root, create-new staging, atomic no-replace publication, no-follow/reparse opens, post-open regular-file checks, cross-platform lock, file sync and directory sync.
- Idempotency/order: journal state is fully authenticated before lookup; exact retry precedes epoch allocation; record chain and exact `epoch = previous + 1` reject reorder/duplicates.
- Stale resurrection: every unwrap reloads the durable revocation journal under lock before decrypting; old wrapped bytes cannot bypass it.
- Secret traits/lifetimes: compile-time negative trait assertions and explicit zeroization test pass; decrypted bytes enter `SecretBytes` before digest validation so failures also zeroize.
- Scope/dependencies: only Task 4 files changed; no schema, event-envelope, JSONL, Governor, retention, PostgreSQL, CLI, or documentation file changed.

No unresolved Task 4 implementation concern remains. The Windows no-privilege symlink execution limitation is reported above and remains covered by Linux plus the exact Windows skip convention.

## Review fix round 1 — secret and filesystem boundaries

The four Important findings from the independent Task 4 review were reproduced before production changes and are closed by this round.

### RED evidence

1. `partial_random_fill_failure_clears_destination` initially failed to compile because no injectable private fill seam existed. The first GREEN attempt also proved the exact `Zeroize<Vec<u8>>` behavior: contents are overwritten and the vector length is cleared, so the assertion was corrected from 32 visible zero bytes to an empty destination.
2. The batch-count test initially failed to compile because `MAX_EVIDENCE_ITEMS_PER_BATCH` did not exist. Inspection confirmed that 10,001 empty items passed the aggregate byte guard and could reach output allocation, randomness and provider work.
3. Compile-time `Sha256: ZeroizeOnDrop` proof initially failed, while `cargo tree -e features` showed no `sha2/zeroize`, `hmac/zeroize` or `digest/zeroize` activation.
4. Focused provider tests showed that a relative root failed after a CWD change, root replacement created a lock in the replacement directory, and lock replacement was accepted. A parent-component symlink test uses the exact Windows error 1314 skip because this host cannot create symlinks without the privilege; Linux executes the real branch.

### Corrections

- The DEK destination is now a `Zeroizing<Vec<u8>>` before the OS CSPRNG call. A private generic fill seam clears the destination on a partial-error return; successful ownership moves the same zeroizing allocation directly into `SecretBytes` without a plain intermediate DEK buffer.
- The public evidence batch ceiling is 10,000 items, matching the repository append ceiling. Count rejection executes before aggregate traversal, output preallocation, RNG, AEAD or provider calls. Existing 16 MiB/item and checked 64 MiB aggregate limits remain unchanged.
- Exact-pinned `sha2 0.11.0` and `hmac 0.13.0` enable their `zeroize` features, and the resolved graph activates `digest/zeroize`. `Sha256: ZeroizeOnDrop` is compile-time asserted. `Hmac<Sha256>` 0.13 does not itself expose that marker, so the adapter does not overclaim it: its standard HMAC-SHA-256 construction explicitly owns inner/outer pads, derived subkeys and intermediate digest bytes in `Zeroizing`, while the underlying SHA-256 states use their verified drop contract. A fixed interoperability vector proves the emitted tag is unchanged. Final authentication tags are intentionally returned/persisted. This is a type/drop guarantee, not proof that allocator pages or compiler-created copies are scrubbed.
- The provider canonicalizes the accepted root once to an absolute existing directory after walking and rejecting every supplied symlink/reparse component. It retains the directory handle and stable identity (`dev`/`ino` on Linux; volume serial/file index on Windows), syncs through that handle, and verifies named-root identity before and after operations.
- One retained lock handle is serialized through a mutex and exclusively locked. After acquisition and after the operation, the handle identity must still equal the final-component no-follow lock path identity. Root or lock replacement therefore fails closed without opening or creating state under a replacement root. On Windows, the retained directory handle may prevent root rename at the OS boundary; that secure branch is asserted explicitly.
- Root and provider files require owner-only permissions on Unix. Existing no-overwrite hard-link publication, bounded authenticated journal loading, ordered/idempotent revocation, file flush/sync and no-follow/reparse behavior remain intact.
- `windows-sys 0.61.2` was already exact-pinned in the workspace and lockfile. The adapter adds only the target-specific `Win32_Storage_FileSystem` feature needed for handle identity; no crate or version was introduced.
- The earlier symlink skip was narrowed to exactly Windows `ERROR_PRIVILEGE_NOT_HELD` 1314. The nonce-randomness Minor remains unchanged because this round was scoped to the four Important findings plus the directly adjacent exact-skip correction.

### Final verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-events --locked
  16 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  10 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  267 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

cargo +1.97.1 tree -p graphhelm-sealed-key-provider -e features --locked
  confirmed sha2/zeroize, hmac/zeroize, digest/zeroize and only Win32_Storage_FileSystem directly requested for windows-sys

git diff --check
  passed
```

### Adversarial self-review after fixes

- Partial RNG failure cannot leave a live plain DEK destination; the test injects a prefix write followed by failure and observes the zeroizing vector cleared.
- Item-count rejection happens before the iterator fold and before any provider call; the counting provider remains at zero for 10,001 empty inputs.
- The local HMAC implementation matches an independently computed fixed standard vector and preserves all existing keyring, journal, receipt and checkpoint verification tests.
- Relative paths are resolved once, parent traversal is rejected, every supplied component is checked, and later CWD changes cannot alter the stored canonical path.
- The retained directory and lock identities are checked through live handles. A replacement directory is not touched; a replaced lock is not accepted even after locking its stale retained handle.
- All filesystem failures remain stable `KeyError` variants and expose no path, OS prose, key material or authenticated content through public errors.

## Review fix round 2a — key contracts and secret-output cleanup

This sub-round addresses only the HMAC-output, public-code, adapter-neutral metadata, dependency-authorization, and deterministic Unix-test findings from the first Task 4 re-review. The root/path/ACL and live-state anchoring findings are intentionally reserved for fix round 2b and were not changed here.

### RED evidence

1. The new core test failed to compile with E0432/E0407 because `KeyProviderMetadata` and `KeyProvider::metadata` did not exist. This established the missing adapter-neutral rollback boundary before implementation.
2. The first digest-output test failed because the private cleanup seam did not exist. It also proved that `digest::Output<Sha256>` itself does not implement `Zeroize`; the corrected contract therefore avoids producing an owned output at all and finalizes directly into an already-zeroizing fixed buffer.
3. After the metadata compile RED was isolated, the exact catalog regression test was run against the restored old mappings and failed with actual `GHEV002_EVIDENCE_TOO_LARGE` versus required `GHE006_LIMIT_EXCEEDED`. Reapplying the normative mappings made that exact test pass.
4. A WSL probe under umask `0022` observed a newly created provider directory at mode `0755`, then `0700` after the same explicit permission operation used by the tests. This demonstrates why the previous Unix setup could fail at the permission precondition instead of the symlink/replacement assertion.

### Corrections

- The manual HMAC implementation now routes both inner and outer SHA-256 finalization through one private helper. The helper allocates `Zeroizing<[u8; 32]>` first and calls `Digest::finalize_into` through a borrowed `Output<Sha256>` view of that storage. No owned `Output<Sha256>` temporary is created before copy, use, or drop. The full 32-byte RFC-compatible HMAC vector is unchanged; compiler optimization and physical allocator/stack erasure are not overclaimed.
- `EvidenceError::Unavailable` remains `GHEV001_EVIDENCE_UNAVAILABLE`; metadata/AAD/crypto/sealing failures use `GHEV004_EVIDENCE_INVALID`; both item and batch bounds use `GHE006_LIMIT_EXCEEDED`. The five internal `KeyError` subvariants remain useful for fail-closed control flow, while every public `code()` is exactly `GHK001_KEY_UNAVAILABLE`.
- `KeyProviderMetadata` is a private-field, bounded, adapter-neutral value exposing only nominal non-secret key ID, bounded algorithm/version tokens, and current monotonic revocation epoch. Its exact camelCase serialization has four fields, its `Debug` contains only those safe fields, and it has no `Display` or custody-state bytes.
- `KeyProvider::metadata` is object-safe and asynchronous. `SealedKeyProvider` loads the authenticated journal under its existing exclusive lock and returns key ID, `xchacha20poly1305+hmac-sha256`, provider version `1.0.0`, and the current epoch. The existing concrete `epoch()` delegates to the same source.
- The unused direct `hmac` crate was removed; the independently tested manual construction remains. `sha2 = 0.11.0` keeps `zeroize`, so the hash state and the explicit final buffers are covered by their respective drop contracts.
- The workspace `libc = 0.2.189` entry is documented as authorized only for this adapter, and the adapter dependency is now under `cfg(unix)` solely for `O_NOFOLLOW`. `windows-sys = 0.61.2` remains under `cfg(windows)` with only `Win32_Storage_FileSystem` directly requested. The CLI retains its pre-existing separate target-specific libc pin outside this Task 4 change.
- Both parent-symlink and root-replacement tests set and immediately assert Unix mode `0700` before invoking the provider. The replacement directory is also set to `0700`, so a replacement assertion cannot pass merely because the secure-permission precondition rejected it. Windows behavior and the exact raw-error 1314 skip are unchanged.

### Verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-events --locked
  18 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  12 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  271 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

The Windows host executed the full suite. Linux behavior was compile/lint-checked through the installed target; the WSL environment had no Cargo toolchain, so the `0700` setup was verified there with the exact filesystem mode probe rather than executing the Rust tests.

### Adversarial self-review after round 2a

- A source scan finds exactly one SHA-256 finalization site in the sealed adapter, the `finalize_into` helper backed by pre-zeroizing storage. HMAC output remains full length and constant-length verification behavior is unchanged.
- Exhaustive enum tests cover every public Evidence and key error variant against the fixed catalog, preventing the retention-reserved `GHEV002/GHEV003` values or invented key codes from returning.
- Metadata construction rejects empty, non-ASCII/unsupported-character, and over-64-byte algorithm/version tokens; the nominal key ID retains the repository's existing bound. Exact serialization and negative canary checks cover KEK bytes, provider path, keyring/journal names, and journal field names.
- Metadata loads the current authenticated epoch on every call and is exercised through `&dyn KeyProvider` before and after revocation. It exposes no key bytes, ciphertext, nonce, tag, path, journal content, or OS/library error.
- Dependency metadata confirms libc is `cfg(unix)` and windows-sys is `cfg(windows)` for this adapter. No `hmac` package or source reference remains.
- No root/path/ACL/anchoring production code was changed in this sub-round. The related re-review findings and the accepted whole-provider rollback limitation remain for round 2b adjudication.

## Review fix round 2b — handle-anchored storage and protected Windows ACLs

This sub-round closes the pathname check-to-open and permissive Windows DACL findings. It changes only the sealed-key adapter, its tests, target-specific feature selection, and this report. No core, schema, Governor, JSONL, retention, PostgreSQL, CLI, crate-version, or dependency-version change was made.

### RED evidence

1. `transient_journal_rollback_cannot_hide_a_revocation` installed a deterministic test-only barrier around the trusted operation, exchanged the authenticated current journal for its valid pre-revocation snapshot, restored the current name before the old post-check, and failed because the old pathname load reported the handle as not revoked. The exact focused run was 1 failed, 1 passed: the journal rollback test failed while the Windows root-swap branch was already denied by this host's live root handle.
2. `created_provider_state_has_a_protected_dacl` failed because the existing adapter accepted the inherited/default temporary-directory ACL and left root, lock, journal, and keyring DACLs unprotected.
3. `permissive_existing_state_is_rejected` failed because a provider reopened after its keyring received an Everyone/full-control DACL. The final test uses a protected DACL with that unexpected allow ACE, proving rejection is based on the principal/permission allowlist rather than only the inheritance flag.
4. The existing permanent lock-replacement test exposed the intended Windows GREEN boundary after implementation: rename now returns sharing violation 32 while the retained lock is live. On Unix, the new deterministic split-writer test exchanges the named lock while a critical section is active and requires the competing root handle's nonblocking advisory lock to fail. The Linux test binary could not execute on this Windows host because WSL has no Cargo toolchain; its complete Rust path compiled and passed target Clippy before link.

### Handle-anchored implementation

- Unix opens the supplied root by walking every component with retained directory descriptors and `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`. Every lock, keyring, journal, temporary, link, unlink, read, and append operation is then relative to that retained root through `openat`, `linkat`, `unlinkat`, or `fstatat`; no `/proc/self/fd` path or reconstructed trusted pathname is used.
- The Unix critical section locks both the retained root descriptor and the one retained lock-file descriptor. The root lock prevents a transient replacement lock from creating a cooperative split writer; lock and journal directory entries are identity-checked through `fstatat` while all journal reads and appends continue on the retained journal handle.
- Windows opens the root and retained lock without delete sharing and with final-component reparse protection. The journal and keyring handles are also retained without delete sharing. Journal authentication, state load, length check, append, flush, and sync use one journal handle under the retained lock; no check/open/check sequence selects a second file.
- Keyring publication still uses same-directory create-new staging, file flush/sync, no-replace hard-link publication, root-handle sync, fixed-name cleanup, and destination verification. Only the unpublished temporary handle allows delete sharing because Windows `CreateHardLink` requires it; after linking, identity is checked, the temp name is removed, and the published destination is reopened, authenticated, and retained without delete sharing before success.
- Operator-visible root pathname checks remain only as replacement detection. They never choose the root or child object used for sensitive Unix I/O, and Windows denies root replacement while the live no-delete-share handle exists.

### Windows ACL boundary

- `SealedKeyProvider::create` applies a protected DACL to the supplied root before creating provider bytes and applies the same DACL to every newly created provider file before writing authenticated state. `open` validates rather than repairs existing state.
- The exact allowlist is Owner Rights, Local SYSTEM, and built-in Administrators, each with full control and no inheritance flags. Full control is justified for owner custody and administrative recovery. The DACL is protected and contains exactly these three allow ACEs; null, absent, inherited, unreadable, deny, duplicate, unexpected-principal, unexpected-mask, or unexpected-flag entries fail closed.
- DACL creation uses a fixed SDDL descriptor through `ConvertStringSecurityDescriptorToSecurityDescriptorW` and handle-based `SetSecurityInfo`. Validation uses handle-based `GetSecurityInfo`, descriptor control, bounded ACL metadata, exact ACE count/type/size/flags/mask, and aligned well-known SID buffers. Every Windows-owned descriptor has an RAII `LocalFree` guard, every OS/file handle is owned by `File`, and all public failures remain the single redacted `GHK001_KEY_UNAVAILABLE` family.
- `windows-sys 0.61.2` remains exact-pinned and target-specific. The only directly requested features are `Win32_Storage_FileSystem`, `Win32_Security`, and `Win32_Security_Authorization`. `libc 0.2.189` remains exact-pinned and adapter-only, now explicitly authorized for the required Unix `open/openat/fstatat/linkat/unlinkat` no-follow boundary.

### Verification

Fresh final gates after formatting and the final handle/DACL change:

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-events --locked
  18 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  16 passed; 0 failed; all doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test -p graphhelm-sealed-key-provider --target x86_64-unknown-linux-gnu --no-run --locked
  all Rust/Unix sources compiled; link unavailable because this Windows host selected MinGW ld, which rejected Linux option --eh-frame-hdr

wsl.exe bash -lc 'cargo --version && rustc --version'
  executable Linux fallback unavailable: cargo: command not found

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  275 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

The Windows symlink test retains only the established exact error-1314 skip. DACL tests have no privilege skip and executed as an ordinary owner. Linux runtime execution remains a CI/host gate because this host has neither a Linux linker nor a WSL Cargo toolchain; Linux target compilation and warning-free Clippy are current.

### Adversarial self-review after round 2b

- Handle lifetime: root, lock, keyring, and journal handles are RAII-owned; the journal mutex is acquired only inside the complete cross-process critical section; failed secondary lock acquisition releases the Unix root lock before returning.
- TOCTOU: Unix trusted I/O contains no child pathname selection after root acceptance. Windows root/lock/journal/keyring delete sharing is denied. The only linkable temp exception writes and syncs before publication, verifies the published inode, removes the temp name, then reopens and authenticates the destination without delete sharing.
- Split writers: Unix root-FD locking remains stable even if the named lock is swapped; a competing provider that captured a replacement lock cannot enter the root critical section and later fails lock binding. Windows prevents the replacement at the sharing boundary.
- Rollback: a single journal replacement cannot hide revocation because operations read the retained current handle and verify its named identity. The accepted rollback boundary remains only restoration of the whole provider snapshot plus external path state, handled later by backup/restore epoch comparison.
- ACL semantics: validation is a positive exact allowlist. Deny ACEs are rejected rather than interpreted as permission, so no deny ordering or inherited-ACE ambiguity can widen effective access. Everyone, Authenticated Users, Users, anonymous/network, or any other principal fails even under a protected DACL.
- FFI and bounds: every unsafe block documents live handles, allocation ownership, output initialization, or descriptor transfer; ACE headers are size-checked before casting, the fixed SID header is proven in-bounds before validation, and the validated SID length must fit both the ACE and the platform maximum before comparison. SID buffers are 32-bit aligned; counts and structure sizes use checked conversions; all Windows allocations and Rust files close on every return path.
- Durability/authentication: authenticated keyring and chained journal formats, exact/divergent idempotency, monotonic epochs, no-overwrite publication, flush/file-sync/root-sync order, stable error codes, HMAC cleanup, and all round-2a contracts remain unchanged.

## Review fix round 3 — safe epochs and effective owner identity

This round addresses only the four Important findings from the second Task 4 re-review. It does not change the keyring or journal wire formats, dependency versions, cryptographic construction, retained-handle/DACL allowlist, or any module outside the authorized Task 4 scope.

### RED evidence

1. `key_provider_epochs_stay_within_the_wire_safe_integer_boundary` failed because `KeyProviderMetadata::new` accepted `9_007_199_254_740_992`. The same regression covers `RevocationReceipt`, its existing nonzero requirement, and the exact maximum-safe boundary.
2. The Windows unit-test build failed with E0425 because the owner/token SID predicate did not exist. This established the missing byte-equivalent owner identity check before the security-descriptor implementation.
3. Linux target Clippy failed with E0425 because the effective-UID predicate did not exist. The predicate tests correct directory/file modes with matching and mismatching owner UIDs without privileged `chown`.
4. A WSL logic probe observed `open(O_NOFOLLOW)` on a symlink returning `errno=40/ELOOP`. A second probe exchanged the named journal while retaining its descriptor, observed divergent inodes before retained-handle load, read the current revoked state from the retained descriptor, restored the names, and verified the attacker's stale bytes were unchanged.

### Corrections

- `KeyProviderMetadata::new` now accepts epochs only through `9_007_199_254_740_991`; zero remains the valid initial metadata epoch. `RevocationReceipt::new` retains its positive-epoch rule and enforces the same upper bound. The sealed journal already rejects more than 10,000 records and uses `checked_add` before serialization or mutation, so its reachable epoch is strictly within the wire-safe range.
- Unix directory and regular-file validation now requires exact `0700`/`0600`, no unexpected special bits, and `metadata.uid() == geteuid()`. The real metadata path and the pure wrong-owner regression share the same predicate.
- Windows handle validation requests both owner and DACL from `GetSecurityInfo`, bounds the returned security descriptor, validates and copies the owner SID, loads the current process token user through a query-only owned token handle, bounds and validates that SID, and requires byte equivalence before checking the existing protected-DACL allowlist. Null, malformed, out-of-region, oversized, unreadable, or mismatched owner data fails closed.
- The concurrent journal rollback regression now requires `KeyError::Integrity` on Unix when the pre-load name/handle identity gate sees divergence, while preserving the Windows sharing-denial branch. It also proves the stale attacker journal receives no mutation.
- The keyring symlink gate now expects the actual stable public mapping, `KeyError::Integrity`; the Windows skip remains limited to symlink-creation error 1314.

### Verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-events --locked
  19 passed; 0 failed; doc tests passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  17 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

wsl.exe python3 - <focused ELOOP and retained-journal probe>
  symlink_errno=40 name=ELOOP
  journal_identity_diverged=True
  retained_state=current-revoked attacker_state=unchanged

wsl.exe bash -lc 'cargo --version && rustc --version'
  Linux runtime unavailable: cargo: command not found

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  277 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

### Adversarial FFI and boundary self-review

- Every Windows-owned allocation or handle has one RAII owner: the security descriptor uses `LocalFree`; the process token uses `OwnedHandle`; provider files remain `File`-owned. The current-process pseudo-handle is never closed.
- Both variable-size Windows regions have hard maximums and checked conversions. SID pointers must contain the fixed header inside their reported live region, pass `IsValidSid`, remain below `SECURITY_MAX_SID_SIZE`, and fit entirely inside that region before copying or comparison.
- Owner and DACL come from the same live descriptor. The owner comparison occurs before accepting ACE count, flags, masks, or principals, so Owner Rights cannot substitute for actual token ownership.
- Unix checks use the effective rather than real UID and reject mismatched owners even for a privileged process. Exact mode/type/no-follow checks remain on every trusted root and regular-file path.
- No path, SID, OS message, key material, authentication tag, journal content, or attacker bytes enter a public error. All failures retain the stable `KeyError` variants and the single public `GHK001_KEY_UNAVAILABLE` code.

## Review fix round 4 — live journal monotonicity

This round closes only the open-instance authenticated-journal rollback finding. It changes the sealed provider's in-memory synchronization state, journal head comparison, focused tests, and this report. It does not change wire or keyring formats, schemas, dependencies, core crates, cryptographic constructions, OS handle/ACL behavior, or Task 5 surfaces.

### RED and mutation evidence

1. `authenticated_journal_regression_is_rejected_for_the_lifetime_of_an_open_provider` retained the same owned root and journal file identity, restored the fully authenticated epoch-zero bytes after a live revocation, and failed because `epoch()` returned `Ok(0)` instead of `KeyError::Integrity`. The test also preserves a pre-revocation wrapped key and exercises every state-dependent public operation after rollback.
2. The equal-epoch divergent-head regression was mutation-checked by temporarily removing the authenticated-head comparison. `authenticated_journal_divergence_at_the_live_epoch_is_rejected` then failed with `Ok(1)` instead of `KeyError::Integrity`; restoring the comparison made the focused test pass. No mutation remains in the source.
3. `an_open_provider_accepts_an_authenticated_extension_from_another_instance` keeps both providers open, lets each durably append once, then proves the first provider accepts epoch two and still rejects the second revoked handle.

### Corrections

- `LockedJournal` owns the retained journal file and a private `JournalFloor` under one `Mutex`. The floor records the greatest fully authenticated epoch and its exact 32-byte authenticated chain head.
- `create` and `open` initialize the floor only by fully loading and authenticating the retained journal while the repository lock is held.
- Every `with_locked_state` call authenticates the journal, rejects a lower epoch or any head that does not include the current floor as its exact prefix, and records a legitimate already-durable extension before invoking the operation. This prevents a failed logical read such as revoked-key unavailability from forgetting a newer observed floor.
- Successful operations re-verify the retained file identity, reload and authenticate the resulting journal, require it to extend the state used by the operation, and only then publish the new floor. `append_revocation` already completes write, flush, file sync, and directory sync before returning, so a newly appended epoch cannot become the in-memory floor before durability and reauthentication.
- `wrap`, `unwrap`, `metadata`/`epoch`, `revoke`, `authenticate`, and `verify` now all cross the same monotonicity gate. A failed append, sync, authentication, identity check, or post-append reload cannot advance the floor to its candidate state or return success.
- Multi-instance append and exact idempotent revoke behavior remain unchanged. A fully internally consistent provider snapshot can still be restored after every provider instance is closed; no external anti-rollback guarantee across process restarts is claimed.

### Verification

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  20 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  280 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

The Windows host executed the complete tests. Linux target compilation and Clippy executed with the installed Rust target; as recorded in the prior rounds, this host has no Linux linker or WSL Rust toolchain for native Linux test execution.

### Adversarial self-review after round 4

- Same-object rollback: the regression truncates and rewrites through the retained journal object, asserts both handle and named identities remain equal, and never relies on pathname replacement or an OS-specific rename outcome.
- Prefix semantics: equal epochs require the exact accepted head; later epochs must contain that head at the accepted epoch. A legitimate authenticated extension advances, while older and equal-epoch divergent snapshots fail before any requested operation executes.
- Failure ordering: the floor may advance to an authenticated extension already durable before the current call even when the requested logical operation fails. It advances to bytes appended by the current operation only after durable sync, identity verification, full reload, chain authentication, and prefix verification.
- Synchronization: the file and floor share one process mutex inside the existing cross-process directory/lock critical section. No separate lock order, public state, serialized field, key material copy, or new path access was introduced.
- Residual claim: live providers remember the greatest authenticated head they observed. Closing all instances discards that in-memory anchor, so complete authenticated snapshot restore after shutdown remains explicitly outside this guarantee.

## Review fix round 5 — shared live provider floor

This round closes the remaining same-process multi-instance rollback gap. It changes only the sealed provider's private in-memory floor registry and tests, the focused threat-model limitation, and this report. It does not change any persisted keyring/journal format, schema, public wire/API, dependency, core crate, Governor behavior, or Task 5 surface.

### RED and mutation evidence

1. The first A/B test attempt passed for the wrong reason because A read the journal after B's revocation and learned epoch one before rollback. The invalid observation was removed before production edits. The corrected deterministic test opens A and B at authenticated epoch zero, lets B durably revoke, restores the same retained journal object in place to the valid epoch-zero bytes before A observes B, and failed with `left: Ok(0), right: Err(Integrity)`.
2. The final A/B regression exercises `metadata`, `epoch`, `wrap`, `unwrap`, `authenticate`, `verify`, and `revoke`; every state-dependent operation fails with `KeyError::Integrity` after rollback, and the previously wrapped revoked key never becomes available.
3. Mutation-checking removed only the shared-floor extension comparison from the final implementation. The A/B regression again failed with `Ok(0)` instead of `Err(Integrity)`. Restoring the comparison returned the focused test to GREEN.
4. A lifecycle regression proves two providers share one live floor entry, dropping one preserves the entry for the other, and dropping the final provider removes the registry entry rather than retaining a process-lifetime strong anchor.

### Shared-floor implementation and lock ordering

- A process-global `OnceLock<Mutex<HashMap<...>>>` maps the stable retained root-object identity plus the authenticated nominal key ID to a `Weak` reference. It is not keyed by a caller pathname or by replaceable child names/inodes.
- `create` and `open` fully authenticate their own retained journal and keyring under the existing repository lock before registry join. The global registry mutex is then used only for bounded weak-reference lookup/insert; no filesystem I/O, journal authentication, crypto, repository locking, or provider callback executes while it is held.
- Each live storage/key identity owns a separate `Arc<Mutex<JournalFloor>>`. Under the existing cooperative repository lock, journal bytes are loaded and authenticated first; the per-identity mutex is held only long enough to compare/replace epoch plus 32-byte authenticated head, then released before the operation callback or crypto. Successful mutation advances the shared floor only after append flush, file sync, directory sync, identity verification, full reload, authentication, and prefix verification.
- Lower or equal-epoch-divergent state fails before an operation callback. A legitimate authenticated extension from any sibling provider advances the same floor. Registry or floor mutex poisoning maps to `KeyError::Integrity`; no path, tag, journal bytes, or OS detail is exposed.
- `Drop` removes the matching weak entry only when the final live strong reference is being released. Pointer identity prevents an old provider from deleting a replacement entry. The global registry mutex and per-identity floor mutex are never nested, so registry join/drop cannot create a floor/repository/callback lock cycle.
- Existing retained root/lock/journal/keyring handles and cross-process locks remain unchanged. Concurrent honest writers still serialize and publish unique epochs.

### Residual trust boundary

All live instances in one process now share the greatest observed authenticated floor. Independent processes do not share this memory. Filesystem locks serialize cooperative writers but are not trusted anti-rollback custody against another process with provider-root write access or a compromised host. A stronger guarantee requires future external monotonic custody/HSM, and restore still must reject a backup provider epoch below the current provider epoch before releasing plaintext.

### Verification

Fresh gates after the final source and threat-model changes:

```text
cargo +1.97.1 fmt --all -- --check
  passed

cargo +1.97.1 test -p graphhelm-sealed-key-provider authenticated_journal_regression_is_shared_across_open_providers --locked -- --nocapture
  1 passed; 0 failed

cargo +1.97.1 test -p graphhelm-sealed-key-provider concurrent_writers_publish_unique_monotonic_epochs --locked -- --nocapture
  1 passed; 0 failed

cargo +1.97.1 test -p graphhelm-sealed-key-provider shared_floor_registry_releases_the_last_provider_entry --locked -- --nocapture
  1 passed; 0 failed

cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
  22 passed; 0 failed; doc tests passed

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
  passed on Windows with zero warnings

cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --target x86_64-unknown-linux-gnu --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
  passed with zero warnings

cargo +1.97.1 test --workspace --all-features --locked
  282 passed; 0 failed; all doc tests passed

cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
  8 passed; 0 failed

cargo +1.97.1 metadata --locked --no-deps --format-version 1
  passed; 10 packages; 10 workspace members

git diff --check
  passed
```

Linux target compilation and Clippy are current. As in prior rounds, this Windows host has no Linux linker or WSL Rust toolchain for native Linux test execution.

### Adversarial self-review after round 5

- Registry identity uses stable root handle identity and a key ID accepted only after authenticated keyring verification; pathname aliases cannot split the in-process floor, and replaceable child inode identity cannot bypass it.
- The A/B test restores the same file object, so it does not rely on rename, sharing, symlink, or platform-specific pathname behavior. Its revoked wrapped key is never returned after rollback.
- The shared floor cannot regress: join, initial observation, and post-operation confirmation each require an authenticated extension before replacing the floor. The mutation-check proves the comparison is behaviorally necessary.
- Registry bookkeeping never owns secret material, paths, keyring/journal bytes, authentication tags, or provider handles. Weak references prevent lifecycle retention after the last provider.
- Existing whole-snapshot-after-shutdown and independent-process limitations remain explicit; the in-memory registry is not described as external anti-rollback storage and does not weaken restore-time epoch validation.
