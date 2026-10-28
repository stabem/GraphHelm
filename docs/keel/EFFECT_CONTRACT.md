# Append effect contract: planted-defect observation (#1476)

`python tools/keel-effect/plant_defects.py --root <slot root> --lane <lane> --jobs 2 --output <report>.json`
plants each of five defects into `core/events/src/local.rs`, runs the contract tests
(`append_effect_contract`, `local_atomicity`, one slot run per file because `workspace slot test`
takes one cargo target) and restores the file byte-identically. Verdicts: `caught` (a test went
red), `missed` (all green), `UNOBSERVED` (anchor did not match once, did not compile, or baseline
not green). The issue's bar: pass at 5/5, kill below 4/5.

Measured on `main` at `e6ce369c` plus the tool (lane gh-claude-1476, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | **missed** | 0 |

Result: 4/5, status `partial` (above the kill bar, below the pass bar). Baseline green.
Total added wall time for the observation: 92.4 s with a warm target (6 slot runs).

## Finding: kill-mid-write was not pinned (fixed by #1582)

With the unterminated-tail check in replay turned into a pass-through, every append contract test
and `local_atomicity` stayed green. The torn-write test in `append_effect_contract.rs` tears the
write mid-record, so the line parser refuses it before that check is reached. #1582 adds
`a_complete_record_missing_only_its_newline_is_refused_as_an_unterminated_tail`: one valid event is
appended, only the journal's final newline is removed, and `open` must refuse with exactly
`IntegrityAt("load_state:journal-tail-unterminated")`. The record parses and its hash chain holds,
so that check is the only refusal possible.

Measured again on `main` at `dd34ae68` plus that test (lane gh-claude-17, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | caught | 101 |

Result: 5/5, status `pass`. Baseline green; `core/events/src/local.rs` restored byte-identically.
Total wall time for the observation: 630.2 s (a cold target for this worktree).

## After ADR-046: a torn tail is recovered, not refused (#41)

ADR-046 changes what kill-mid-write means. On open, the bytes after the journal's last newline
are now trimmed, under the exclusive lock and only after every earlier line verifies. They are
quarantined first in the sibling `<events>.quarantine/`, with a manifest. #1582's newline-only cell
now asserts that recovery too (renamed
`a_complete_record_missing_only_its_newline_is_trimmed_as_an_unacknowledged_tail`). Other cells
assert the rest: the interrupted operation's retry appends exactly one event; a torn first append
trims to an empty journal; a corrupt terminated line, and a tail longer than one batch, still
refuse. The planted kill-mid-write defect turns the unterminated-tail check into a pass-through.
The torn bytes then reach the line parser, open refuses instead of recovering, and the recovery
cells go red.

Measured on `main` at `21348962` plus #41's change (lane gh-claude-17, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | caught | 101 |

Result: 5/5, status `pass`, 318.5 s. `core/events/src/local.rs` restored byte-identically.

## Evidence keyed to the append path's closure (#1476 slice b)

A green run of the lost-ack observer (`append_effect_contract`) vouches for one set of bytes: the
files that can change what the append path does. `tools/keel-effect/evidence_key.py` computes that
set at a git revision from the repository's own manifests: `graphhelm-events`, every workspace
member it reaches through `path` dependencies (normal, build and `[target.*]` forms), its own
dev-dependencies with their normal closure, and `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
`.cargo/config.toml`, plus every file those crates' Rust sources embed with a literal `include!`,
`include_str!` or `include_bytes!` (resolved from the source file, followed through embedded `.rs`,
bounded at 5000 files and 4 MiB each), such as `schemas/*.schema.json` behind `core/schema`'s
registry and `conformance/schemas/valid/*.json` in the event tests. An include whose argument is not
one plain string literal, or names a file absent at the revision, makes `check` answer
`invalidate` with an `unknown` list and makes `record` refuse. The key is SHA-256 over the sorted `path NUL blob-id` lines of those files.

The record lives beside this file in `effect-evidence/append-lost-ack.json`: guarantee, observer
command, observed result, the commit it ran on, the closure and the key. A reviewer asks:

    python tools/keel-effect/evidence_key.py check --rev <head>

Exit 0 `reuse`: no closure byte moved since the recorded run, so the run still stands (a Studio,
docs or CLI change, or a crate that depends on `graphhelm-events` without being linked by it).
Exit 1 `invalidate`: it names the closure paths that differ and the observer to rerun; after a green
rerun, `record --rev <sha> --observed "<result line>"` writes the new key.

The key says *which bytes* a run covered; it is not proof by itself. A `reuse` is only as strong as
the recorded run, and the closure is a crate-level over-approximation: any edit inside a linked
crate invalidates, even one the append path never calls.

## Effect-set change beside the evidence (#1476 slice c)

"Files changed" says the run must be repeated; it does not say whether the append path can now do
something the guarantee never considered. The record therefore also holds `effects`: for each
effect kind, the closure's production sources that perform it (`by_kind`), inferred from the same
blobs the key covers. Production excludes `tests/`, `benches/`, `examples/`, inline
`#[cfg(test)] mod x { ... }` bodies, and a file only when every closure declaration that loads it
(`mod x;` resolving to `x.rs` / `x/mod.rs`, or `#[path = "..."]`) carries exactly one
`#[cfg(test)]`. One other declaration (plain, `not(test)`, `any(..)`, a feature gate, a second
cfg) keeps it production; a `tests.rs` nobody declares still counts. A `#[path]` the scan cannot
pair with its declaration, or a `cfg_attr` path, puts the source in `unlexed`. Each source is lexed first: comments (nested) removed; strings, byte, C
and raw strings (`r#"..."#`) and char literals (`'"'`, `'\''`, `b'x'`) emptied; lifetimes kept.
A source the lexer cannot place (an unterminated literal or comment, a quote that is neither a char
literal nor a lifetime) is listed in `unlexed`: `check` then answers `effects unknown` and `record`
refuses.

| Kind | Matched by |
|---|---|
| `file_write` | `File::create`, `OpenOptions`, `fs::write/rename/remove_*/create_dir*/copy/hard_link`, `.set_len(` |
| `fsync` | `.sync_all(`, `.sync_data(`, `FlushFileBuffers` |
| `file_lock` | `lock_exclusive`, `lock_shared` (and `try_`), `LockFileEx`, `UnlockFileEx`, `flock` |
| `network` | `std::net`, `TcpStream`, `TcpListener`, `UdpSocket`, `ToSocketAddrs`, `reqwest`, `hyper::`, `tokio::net`, `ureq` |
| `process_spawn` | `Command::new`, `process::Command`, `graphhelm_process::command`, `CreateProcess` |
| `ffi` | `unsafe`, `extern "…"`, `asm!`: code the scan cannot see inside, flagged as its own kind |

`check` adds `effects_changed`, compared with the record per effect site `kind@source`:
`effects changed: +process_spawn@core/events/src/local.rs -file_lock@...`, or
`effects unchanged`. A new spawn in the append file shows even when another closure file already
spawns. A pure refactor inside a file (rename, reflow, a new pure function) reads `invalidate`
(files changed) with `effects unchanged`; moving an effect to another file shows as `-` on the old
source and `+` on the new one. A record written before this field reports `effects unknown`,
never `unchanged`.

Limits: sites are per source file, not per call path; an effect reached only through an external
crate API not named above is not attributed (an `ffi` site is still marked). The tests and their
sabotage guard the kinds and the lexer: a detector that ignores `network` fails the per-kind test
and P2, and a lexer that stops recognising char literals fails P2 and the literal test.
