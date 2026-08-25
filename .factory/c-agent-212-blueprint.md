# #212 — Atomic extension installation, activation, rollback, and CLI discovery

> **Blueprint only. No code, no fixtures, no package files.** Written against `origin/main`
> `0754cda`. Every claim about the repository below was measured with `git grep` / `git show`
> against that ref and carries its coordinate; the graph index does not see worktrees, so the
> source was the instrument throughout.
>
> Provenance marks: `[ISSUE]` where the issue text decides, `[MEASURED]` for a fact read out of the
> tree, `[DERIVED]` for a design choice of mine, `[GAP]` for something deliberately not settled.

---

## 0. What this task consumes, and what it must not reinvent

**`[MEASURED]` The validator already exists and already produces the binding this task needs.**
`core/schema/src/extension.rs:471`:

```rust
pub fn validate_extension_package(package: &Path)
    -> Result<ValidatedExtensionPackage, Vec<Diagnostic>>
```

returning `{ id, version, contribution_count, package_digest }` (`:463`). **The digest this task
verifies before activation is that one.** This design consumes it; it does not compute a second
package digest, because a second digest is a second ORACLE and two oracles diverge in silence.

**`[MEASURED at `0754cda`, SUPERSEDED — the base moved, the measurement did not become wrong]` The CLI
surface was one subcommand.** #213 has since added a token-minting verb to the same enum, so the
sentence below is true of the ref this document names and false of `main` today. It is left standing
with this note rather than edited, because the number of subcommands is not the point — the point is
that `ExtensionCommand` is the only place a new verb can be declared, and that is still true. A
measurement whose base has moved is superseded, not mistaken; deleting it would hide that the base
moved at all.

`apps/cli/src/args.rs:49`:

```rust
pub enum ExtensionCommand {
    Validate { package: PathBuf },
}
```

and `apps/cli/src/commands/extension.rs:5` did nothing but call the validator. **This task adds
install, activate, roll back, discover and uninstall to that surface**, which is why the scope proposal in §5 names `args.rs` explicitly: the
subcommand enum is the only place the new verbs can be declared.

**`[MEASURED, CORRECTED]` No executable DISCOVERY exists, but the first version of this paragraph
reached that conclusion the wrong way and has been re-measured.**

The original claim said `current_exe` returns "zero hits outside `.factory`". It was written from a
`git grep` output truncated by `head -8`, and **a truncation read as a population is not a
measurement.** Re-measured by scanning every `.rs` blob under `origin/main` directly: `current_exe`
appears in **two** files under `src/`, and `PATH` in **fourteen**. The conclusion survives — both
`current_exe` uses sit inside `#[cfg(test)]` modules, so there is still no production discovery — but
it now rests on a scan with a positive control (`pub fn`, found in 103 files) rather than on a
truncated list. **The method was wrong even though the answer was right, and the answer was only
right by luck.**

**`[MEASURED]` And the re-measurement found prior art that changes G6.** Link and junction refusal is
NOT construction: `O_NOFOLLOW` and `FILE_FLAG_OPEN_REPARSE_POINT` appear in four files — including
**`core/schema/src/extension.rs`, the validator this task consumes** (`open_package_root` at `:1127`
opens the package root with `FILE_FLAG_OPEN_REPARSE_POINT` and anchors every descent from that
handle). `symlink_metadata` appears in nine files and `is_symlink` in seven.

**So the package READ path already refuses links.** This task's job is to make the install COPY and
the uninstall DELETE use that same discipline rather than to write a fifth implementation of it — a
fifth would be a second oracle for "is this path contained", and two oracles diverge in silence.

**`[MEASURED]` The house already has file-locking and atomic-append machinery**, in
`core/events/src/local.rs` (`fs2::FileExt` at `:10`, `append_atomic` at `:506`, shared/exclusive
lock handling at `:662`). This task consumes the PATTERN — lock, write beside, rename, unlock — and
does not invent a second locking discipline. It does not consume the event-store types themselves,
which are about journals rather than packages.

---

## 1. Threat assessment

The issue's four invariants are each the negation of a threat. Naming the attack makes the guard
testable, so each row below is the attack first.

| # | The attack | Why it works if nothing stops it | The invariant it violates |
|---|---|---|---|
| **T1** | A package ships `graphhelm.exe` beside its manifest, and activation runs "the executable next to the package". | Package-relative search is the convenient implementation, and the package is attacker-supplied by definition — an installer that looks inside it is executing untrusted content. | *Activation never trusts package-relative executable search.* |
| **T2** | A package passes `validate` and the caller treats the exit code as permission to act. | `validate` answers "is this well-formed?", and a caller who needs "may this run?" will reach for the answer that exists. | *Validation alone grants no runtime authority.* |
| **T3** | Activation fails halfway; the previous version is already unlinked. | The natural order is remove-then-place, because it avoids thinking about two versions coexisting. The crash window leaves NO active version. | *Failed activation leaves the previous version active.* |
| **T4** | A host view is edited by hand and becomes the source of truth. | Derived files that survive deletion look authoritative; the next reader cannot tell a derived file from an authored one. | *Host views remain derived and deletable.* |
| **T5** | Uninstall removes the package directory and takes user-authored artifacts inside it. | `remove_dir_all` on the install root is one line and looks correct. | *Safe uninstall never removes user-authored artifacts.* |
| **T6** | A package contains a junction or symlink pointing outside its root; install copies through it, or uninstall deletes through it. | Windows junctions are followed by most path APIs and are invisible in a directory listing. **A recursive delete through a junction deletes the TARGET.** | Named in the deliverables ("junction/symlink refusal"). |
| **T7** | The digest is verified, then the bytes are replaced before activation reads them. | Verify-then-use over a mutable path is a TOCTOU by construction. | *Verify package digest and manifest before activation* — verified is worthless if the verified bytes are not the used bytes. |
| **T8** | Two installs race the activation switch and interleave. | Without an exclusive claim, both see "previous is X" and both write "active is mine". | *Explicit activation state and lock metadata.* |
| **T9** | A checkout under `core.autocrlf` rewrites package bytes; the digest no longer matches. | The digest is a pin over CONTENT, and content is defined AFTER normalisation. Hashing the freshly-written working-tree bytes and hashing the blob give different answers on Windows. | *Verify package digest before activation* — a false refusal that looks like tampering. |

**T9 is not hypothetical: it was measured in this repository today**, on another lane, as the cause
of two manifest digests that did not match their own files. **The remedy travels with the design:
digests are computed over the bytes as they will be READ, and the conformance suite carries a
CRLF-checkout case rather than a comment.**

---

## 2. The design

### 2.1 `[DERIVED]` Where the machine lives — a new crate, and the repository's own precedent decides it

**Not `core/gateway`.** `[MEASURED]` Its own header (`core/gateway/src/lib.rs:1`) says: *"Pure
route-manifest and policy types for the Universal Model Gateway. This crate is total and side-effect
free: no clock, no randomness, no filesystem…"* — and it is the MODEL gateway, unrelated to
extensions. Wrong subject and wrong purity.

**Not `core/schema` either, though it is the near miss.** It owns `validate_extension_package` and
already touches the filesystem (`[MEASURED]` 5 `std::fs` uses in `extension.rs`). But its declared
identity is *"Offline schema validation"* (`lib.rs:1`), and installation is not validation: it is
stateful, destructive, lock-holding and platform-specific. **Putting an installer there widens that
crate's charter from "answers a question" to "changes the machine".**

**`core/extension-host`, a new workspace member, depending on `core/schema`.** The argument is the
repository's own established shape rather than my taste: **`core/gateway` is pure and
`adapters/model-gateway` holds the impure half.** The house already separates pure-from-impure by
crate, and this is the same seam. Two further consequences make it the cheaper choice: `validate`
must remain callable WITHOUT the installer (the CLI subcommand does exactly that today), and a
future host adapter needs the discovery API as a library, not as a CLI.

**This is the decision the scope amendment must name**, so a reviewer reads the chosen house rather
than guessing which of the two was intended.

### 2.2 `[ISSUE]` Verify, then activate — and the verified bytes are the used bytes

The order is **stage → verify → claim → switch**, and each arrow is where a threat lives.

1. **Stage.** The package is copied into a staging directory INSIDE the install root, never
   activated in place. Copying is where **T6** is refused: the walk refuses a junction or symlink
   rather than following it, and the refusal names the path.
2. **Verify.** `validate_extension_package` runs against the STAGED copy, not the source. This is
   the answer to **T7**: the bytes that were verified are the bytes that will be activated, because
   nothing outside the install root can reach the staged copy afterwards.
3. **Claim.** An exclusive claim over the activation state is taken before anything switches
   (**T8**). `[MEASURED]` The house atomic-claim rule (ED-22) is `CreateNew`, never `New-Item` —
   the claim must fail when the file already exists, and a create-if-absent that succeeds twice is
   not a claim.
4. **Switch.** The previous version is left in place until the new one is recorded active
   (**T3**). The order is chosen so that **every intermediate a crash can leave behind reads as the
   truth**: an unreferenced staged directory is garbage to collect, whereas an activation record
   pointing at a directory that does not exist is a system asserting something it cannot support.

### 2.3 `[DERIVED]` Validation is not authority — the types say so, not a comment

**T2** is defeated by making the two answers different TYPES rather than by documenting the
difference. `ValidatedExtensionPackage` answers *well-formed*. Activation consumes it and produces a
separate `ActivationRecord`; **nothing accepts a `ValidatedExtensionPackage` where an
`ActivationRecord` is required.** A comment saying "validation is not authority" is a claim about
code; a type that cannot be substituted is the property itself.

### 2.4 `[ISSUE]` Discovery records the executable, never searches for it

**T1** is refused structurally: the canonical executable path is **recorded in the activation state
at install time, from the host's own resolution**, and discovery afterwards is a READ of that
record. There is no search at activation time — not PATH, and above all not package-relative.

The distinction that must survive into the code: **"we could not find it" and "we found several"
are different refusals.** Collapsing them into one loses the case where ambiguity is the defect, and
ambiguity is precisely what the deliverable names ("without PATH ambiguity").

### 2.5 `[ISSUE]` Uninstall removes what it placed, and nothing else

**T5** is refused by construction: uninstall walks the **manifest's declared contributions** and the
files the installer itself recorded placing, and removes those. It never recursive-deletes the
install root. Anything present and undeclared is left, and named in the outcome — an unexpected file
is a report, never a deletion.

This is also the second half of **T6**: a recursive delete that follows a junction deletes the
target. **The uninstall path must refuse a link the same way the install path does**, and the two
refusals are the same check, called from both sides, because written twice they drift.

### 2.6 `[ISSUE]` Host views stay derived

**T4**: a host view is regenerated from the activation record and may be deleted at any time without
loss. The guard is a pair: delete the view, regenerate, and the result must be byte-identical —
which is also what makes "derived" a measured property rather than an adjective.

---

## 3. The guards, red-first

Each guard is observed RED **at its own assertion** before being made green, and the red is reported
by panic site rather than exit code. Guards marked **SABOTAGE** are additionally re-broken after
going green, because a sealed cell is a hypothesis until someone writes the fixture.

| ID | The property | The production change it catches |
|---|---|---|
| **G1** | Activation refuses a package-relative executable and names the refusal. **SABOTAGE:** plant an executable in the package; activation must still refuse. | Falling back to "the exe next to the package" when the record is absent. |
| **G2** | A `ValidatedExtensionPackage` cannot be used where an `ActivationRecord` is required. | Compile-time; the guard is that the wrong program does not build. |
| **G3** | A failed activation leaves the PREVIOUS version active. Every crash prefix is checked, not one hand-picked point. | Remove-then-place: the window where no version is active. |
| **G4** | A staged package whose bytes change after verification is refused at activation. | Verifying the source rather than the staged copy. |
| **G5** | A second concurrent activation is refused, not queued and not interleaved. **SABOTAGE:** the claim must fail on an existing file — a create-if-absent that succeeds twice is not a claim. | `New-Item`-shaped creation that silently succeeds twice. |
| **G6** | A package containing a junction or symlink is refused on INSTALL, by reading the entry type — **never by invoking a delete to see what happens.** The refusal CONSUMES the anchored-handle discipline the validator already uses (`open_package_root`), rather than adding a fifth containment implementation. | Following the link during the copy walk; or writing a new containment check beside the four that exist. |
| **G7** | Uninstall leaves an undeclared file in place and NAMES it in the outcome. | `remove_dir_all` on the install root. |
| **G8** | Uninstall refuses a link exactly as install does, via the same check. **SABOTAGE:** break the shared check once; both call sites must go red. | The check copied into one path and not the other. |
| **G9** | A deleted host view regenerates byte-identically. | A view that accumulates state and stops being derived. |
| **G10** | Discovery distinguishes "not found" from "ambiguous" with two different refusal codes. | One code for both, losing the case the deliverable is about. |
| **G11** | A package checked out under CRLF normalisation verifies. The digest is computed over the bytes as READ. | Hashing freshly-written working-tree bytes; a false tamper refusal on Windows. |

**`[DERIVED]` G6 and G8 carry an explicit safety rule that overrides convenience:** *the refusal is
verified by READING the entry type before any destructive call.* If the guard were tested by
invoking the delete and observing the damage, **the test IS the destruction** — and on a junction it
destroys the target, not the fixture.

---

## 4. What this blueprint does NOT settle, declared rather than implied

- **`[GAP]` Linux conformance is written here and executed elsewhere.** The factory runs on Windows.
  The PR ships **Windows EXECUTED + Linux WRITTEN and declared unrun**, because *conformance* is not
  a word to put on a platform nobody executed. The issue does not close until the Linux suite has
  run once; the agreed target is a Rust container on `dale-main` over SSH, and the VERIFY-AT
  condition is recorded in §6. **If the container turns out not to serve, that decision returns to
  the coordinator declared, never assumed.**
- **`[GAP]` Remote registries, hot reload, and signature verification are out.** #210 declared them
  out and nothing here pulls them back in. Digest verification is not signature verification: this
  task proves the bytes are the ones declared, never who declared them.
- **`[GAP]` Multi-version coexistence.** Rollback restores the previous known-good version; running
  two versions at once is not designed here.
- **`[GAP]` The Project Agent Registry (#110) is NOT a dependency.** Three independent reasons: its
  own body states *"Marker issue; not a design"* — there is no design to consume; its subject is a
  catalogue of AGENTS while this task installs EXTENSIONS, and the only near-neighbour phrase
  ("canonical GraphHelm executable discovery") is about locating a binary; and it carries
  `product-vision` where this issue carries `current-wave`. **`Related`, not `Blocks`** — recorded
  here so the question does not have to be re-answered.

---

## 5. `[DERIVED]` The file scope this task needs — proposed for the ISSUE BODY, not for the PR

The issue uses the old template and declares **no scope at all** (`[MEASURED]` 994-byte body:
Goal / Context / Deliverables / Invariants / Related). This is therefore not "the derived scope
needs one more file" — there is nothing to compare against, and the amendment is required before
code regardless of what I derive.

**Proposed `Files in scope (strict)`:**

```
core/extension-host/Cargo.toml                     (new crate -- the decision in §2.1)
core/extension-host/src/lib.rs
core/extension-host/src/install.rs                 stage, verify, refuse links
core/extension-host/src/activation.rs              claim, switch, rollback, activation record
core/extension-host/src/discovery.rs               canonical executable, recorded not searched
core/extension-host/src/uninstall.rs               remove what was placed, name what was not
core/extension-host/tests/activation.rs            G1-G5, G9, G11
core/extension-host/tests/links.rs                 G6, G8 -- read-before-invoke
core/extension-host/tests/discovery.rs             G10
Cargo.toml                                          workspace members += core/extension-host
apps/cli/src/args.rs                                the new subcommands
apps/cli/src/commands/extension.rs                  their handlers
apps/cli/tests/extension_lifecycle.rs               CLI-level conformance
```

**Not in scope, stated so the boundary is decidable:** `core/schema` is CONSUMED and not modified —
if this task turns out to need a change there, that is a scope amendment, not a quiet edit.
`extensions/builtin/**` is untouched, so this task does **not** enter the manifest queue; if that
changes, the union-by-publication-order rule applies and the coordinator is told before the PR.

**Also proposed for the body, because the old template has no place for them:**

- **Validation:** `cargo +1.97.1 test -p graphhelm-extension-host --locked`,
  `cargo +1.97.1 test -p graphhelm-cli --test extension_lifecycle --locked`,
  `cargo +1.97.1 clippy -p graphhelm-extension-host --all-targets --all-features --locked -- -D warnings`.
- **Rollback:** additive — a new crate, new CLI subcommands, no change to existing behaviour. The
  existing `extension validate` subcommand keeps its exact surface, which is itself a guard: if
  reverting this PR changes what `validate` does, the task took something it should not have.

---

## 6. `[ISSUE]` VERIFY-AT — the condition that must run before #212 closes

Recorded here and repeated in the PR body, because a deferred step with no trigger is a step nobody
takes.

> **Condition:** the Linux conformance suite has been EXECUTED at least once, in a Rust container on
> `dale-main` over SSH, isolated and resource-limited, in the manner the house already uses for the
> Dale repositories.
> **Trigger:** before `Closes #212`.
> **If the container does not serve:** the decision returns to the coordinator, declared and
> measured — never assumed, and never quietly downgraded to "written is enough".

---

## 7. Wave rules this task inherits

- Every guard red at its own assertion before green; reds reported by panic site.
- `CARGO_TARGET_DIR` named explicitly on every cargo invocation; `cargo clean -p` for each touched
  crate recorded beside the result.
- `cargo fmt -p <crate>`, never `--all`; `git add -- <explicit paths>`, never `-A`.
- Public comments signed `## <Role> (C)`.
- Reviewer notice goes in the PR comment; a message is courtesy, never the vehicle.

## Implementer (C)
