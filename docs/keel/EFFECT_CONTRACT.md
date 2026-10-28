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
effect kind, the closure sources that perform it (`by_kind`), inferred from the same blobs the key
covers. Every `.rs` file in the closure is scanned, test files included; the only code dropped is
the body of an inline module carrying exactly one `#[cfg(test)]` (and no `cfg_attr`). No file is
excluded: deciding which files a build loads (`mod` nesting, `#[path]` on inline modules,
`include!`) is the one step that can err toward hiding production code, and four review rounds
found a new hole in it each time. Each source is lexed first: comments (nested) removed; strings, byte, C
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
crate API not named above is not attributed (an `ffi` site is still marked). Test files (`tests/`,
`*_tests.rs`, files only `#[cfg(test)] mod` loads) are scanned as if production, so their effects
appear in the record and in `+`/`-` sites: an accepted over-report, in the safe direction. The
tests and their sabotage guard the kinds, the lexer and the inline-body rule.

## Cost per PR: pre-registration (#1476 slice d)

This section was pushed before any number below was measured. The issue's kill bar is a cost per PR
of more than 2x; slice d reports what the contract adds, against what a PR already pays.

1. **`check` time.** `python tools/keel-effect/evidence_key.py check --rev R`, a fresh process each
   time. Walk first-parent history back from `origin/main` (at most 400 commits) and take the first 5
   revisions that answer `reuse` and the first 5 that answer `invalidate` against the committed
   record. On each revision the first run is `cold` (a revision this harness has not read yet) and
   the next 5 are `warm`. Report median, min and max per case: cold has 5 samples, warm has 25. The
   operating system's file cache is not dropped, so "cold" means a first read of that revision's
   objects, not an empty machine.
2. **Observer rerun.** `cargo +1.97.1 test --locked -p graphhelm-events --test append_effect_contract`
   in the build slot (own root under `D:/gh`) with a warm target, 3 runs after one build run that is
   not counted. Compared with the reached-plan time of `tools/reached-tests/run_reached.py` for the
   same diff: a one-line change to `core/events/src/local.rs` on a local, unpushed branch from the same
   `origin/main`, measured once (the plan runs many cargo commands; N=1 is stated as such).
3. **Reuse fraction.** For each of the last 50 first-parent commits of `origin/main` (each a
   squash-merged PR), the closure key at the commit against the key at its first parent, computed by
   `evidence_key.state`. Equal is `reuse`, different is `invalidate`; a commit whose manifests cannot
   be read is `error`, never `reuse`. Local git only.

The harness is `tools/keel-effect/measure_cost.py`; it never starts cargo (the slot refuses wrappers),
so the observer and reached-plan seconds are passed in. Raw numbers go to
`docs/keel/effect-evidence/cost-per-pr.json`. Limits fixed in advance: one host, the machine's load
was not controlled, and timings vary run to run; only the reuse/invalidate counts are deterministic.

## Cost per PR: measured (#1476 slice d)

Raw numbers: `effect-evidence/cost-per-pr.json` (harness `tools/keel-effect/measure_cost.py`, head
`e29871aa`, record commit `be8fcba4`). One host, load not controlled (other lanes build on it), N is
small: read the medians as orders of magnitude, not as constants.

| Measurement | Result |
|---|---|
| `check`, answer `reuse`, cold (first read of a revision) | median 3.6 s, range 3.3 to 4.0 s, **n=4** |
| `check`, answer `reuse`, warm (5 repeats on each) | median 3.5 s, range 3.0 to 4.5 s, n=20 |
| `check`, answer `invalidate`, cold | median 9.3 s, range 6.4 to 10.5 s, n=5 |
| `check`, answer `invalidate`, warm | median 7.7 s, range 5.7 to 14.5 s, n=25 |
| Observer rerun, nothing to rebuild (warm target) | median 0.9 s, range 0.9 to 1.0 s, n=3 |
| Observer rerun after a one-line edit in `core/events/src/local.rs` (events crate rebuilt) | median 3.5 s, range 3.4 to 3.5 s, n=3 |
| Reached plan for that same one-line diff (`run_reached.py --stage review`, 268 steps, 15 packages) | 1330 s, n=1 |
| Last 50 merged PRs: closure key unchanged (`reuse`) | 41 of 50 (82%) |
| Last 50 merged PRs: closure key moved (`invalidate`) | 9 of 50 (18%), 0 unreadable |

Reading it:

- **Cost per PR against the 2x bar.** A PR that moves the closure pays `check` (about 8 s) plus an
  observer rerun (about 3.5 s), about 12 s on top of a 1330 s reached plan: under 1%. A PR that does
  not move it pays `check` alone, about 3.5 s. Weighted by the 82/18 split, the contract adds about 5 s
  per PR on average. The kill bar (more than 2x) is not near for a PR that touches the closure.
- **Where the bar could bite.** The reached plan of a docs-only PR was not measured. If it is a few
  seconds, 3.5 s of `check` is not small against it. `check` is a reviewer's command and nothing runs it
  automatically today, so this cost is paid only when someone asks.
- **`check` is the dominant part and it is slow for what it does.** It reads about one tree listing
  and one batched `git cat-file` over the closure, twice on `invalidate` (the revision and the record's
  commit), and lexes every closure `.rs` source in Python for the effect set. That is the likely
  source of the 3.5 s and of the doubling on `invalidate`; it was not profiled.

Limits, in addition to the ones fixed in advance:

- The walk back through the last 400 first-parent commits found only 4 revisions that answer `reuse`
  against the record (5 were pre-registered): the record's commit is old enough that most recent
  revisions moved the closure. Cold `reuse` is therefore n=4. This also says the record goes stale
  fast: 9 of the last 50 PRs would have invalidated it.
- "Cold" is a first read of a revision, not an empty file cache.
- The reached-plan time is one run, with a worktree whose build target was only partly warm; a lane
  with a fully warm target would see less. It includes the rebuild of every dependent crate, which
  the observer rerun does not need.
- The fraction counts PRs by whether the closure key moved (`reuse` = no closure byte changed between
  the merge's parent and the merge); it does not say how many of those PRs a reviewer would have
  asked `check` for.
- Two additions to the pre-registration, made after seeing the first observer runs: the observer was
  also timed with nothing to rebuild (an uncounted build run came first and was not timed), and its
  rebuilt-crate time is the one compared with the reached plan.
