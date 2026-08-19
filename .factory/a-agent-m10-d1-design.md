# M10 D1 detailed design: verified-prefix incremental load — Agent A

Status: DESIGN, zero code, zero machine. Input: my M10 proposal
(.factory/a-agent-m10-proposal.md), M's CONV-1 finding (mandatory), M09 lessons.
Read against 576e553-era source (local.rs line refs from 53d212d; unchanged by later
commits in the events crate to my knowledge — verify at implementation).

## 0. CONV-1 honored: what the current coherence mechanism IS, and what replaces it

M's finding: opening the repository per request IS today's coherence mechanism — every
open rebuilds state from the journal under a lock, so no process can ever act on another
process's unseen append. D1 SUBSTITUTES that mechanism, never deletes it: coherence
moves from "rebuild everything at open" to "re-check the journal's identity and length
under the SAME per-operation lock every operation already takes, and verify exactly the
unseen suffix". The guarantee is unchanged — no operation acts on state older than the
journal as of its own lock acquisition. What changes is the COST: from O(total history)
per operation to O(new bytes since this handle last looked).

## 1. The coherence mechanism, exactly

### 1.1 Cache shape (per LocalEventRepository instance, in-memory only)

```
struct VerifiedPrefix {
    journal_identity: FileIdentity,   // same identity validate_anchors already compares
    verified_offset: u64,             // bytes of journal verified so far (ends on '\n')
    state: LoadedState,               // the fold of [0..verified_offset)
    budget: LoadBudget,               // accounting as of verified_offset (see 4.3)
}
```
Guarded by the existing per-handle serialization (journal Mutex + operation_gate). No
new on-disk state — crash consistency and doctor untouched by construction.

### 1.2 What is checked, and why these fields

Under `with_lock` (shared for reads, exclusive for appends), after the existing
`validate_anchors` (which already re-verifies the journal FILE IDENTITY against the
handle — local.rs:589-624):
1. `len = journal.metadata().len()` from the SAME open handle the lock protects.
2. Compare against `verified_offset`. Three cases:
   - `len == verified_offset` -> reuse `state` as-is.
   - `len >  verified_offset` -> read ONLY `[verified_offset..len)`, verify every line
     exactly as load_state does today (per-line: inclusive size limit, parse, checksum,
     canonical-bytes equality, format version, sequence continuity from the CACHED
     next_sequence map, event hash chain from the CACHED per-stream last_hash, budget
     accounting continued) — then extend state, set `verified_offset = len`.
   - `len <  verified_offset` -> DROP the cache, full reload from zero (policy, 1.4).
3. The trailing-newline torn-tail rule applies to the SUFFIX read: a suffix not ending
   in '\n' is Integrity, same as today's whole-file check (local.rs:1082).

NOT checked, deliberately: mtime (unreliable granularity, meaningless on the append-only
design) and head sequence (knowing it requires reading — length is the cheap monotone
because the journal is append-only: local.rs:750-761 seeks End and writes, never
truncates, never rewrites).

### 1.3 The residual window between check and use: CLOSED BY THE LOCK, not by timing

The check and the use happen under ONE lock acquisition, and every writer needs the
EXCLUSIVE named lock (append_locked runs under with_exclusive_lock). A concurrent
process CANNOT append between my check and my use: its exclusive acquisition cannot
coexist with my shared one, and my check happens after my acquisition. So the answer to
"what if another process appends between check and use" is: it cannot, by the same lock
discipline that makes today's per-open rebuild sound. The append path checks under its
own exclusive lock, appends, then advances its own cache by the bytes it just wrote
(content known, no re-read needed); if the process dies between append and cache
update, the next operation's length check sees len > offset and verifies the suffix —
self-healing, no separate recovery path.

What remains OUTSIDE the lock's protection (named residuals, both exist today too):
- R1: a writer that bypasses the store API entirely (raw file write). Today's full
  reload would catch a malformed one at the NEXT OPERATION; D1 catches appends (suffix
  verify) and truncations (len < offset) at the next operation, but a REWRITE of the
  verified prefix that preserves length and identity is caught only at the next fresh
  open or doctor run. This is D1's one honest widening: detection latency for in-place
  prefix tampering moves from next-op to next-open. Bounded, named, and priced: the
  threat model already declares non-API writers outside the model; the doctor (M09) is
  the designated re-verifier. NOT closable without re-reading the prefix, which is the
  cost D1 exists to remove.
- R2: identity swap of the whole directory — caught by validate_anchors, unchanged.

### 1.4 The len < offset policy: full reload, matching today byte-for-byte

Today, an externally truncated journal at an exact line boundary reloads as a VALID
shorter history (load_state cannot know); truncated mid-line it fails Integrity. D1
must not silently strengthen: on len < offset it DROPS the cache and full-reloads,
reproducing today's outcomes exactly (boundary truncation -> shorter state; mid-line ->
Integrity). The tempting alternative — treat len < offset as tamper evidence and refuse
— is a REAL strengthening but a BEHAVIOR CHANGE; it is flagged as an owner decision,
default OFF in D1.

### 1.5 serve's long-lived handle

ServeState holds one LocalEventRepository for the process lifetime; handlers use it
directly (the per-request open/use/drop cycle and its comment at serve/mod.rs:106-118
are replaced — CONV-1's mechanism substituted by 1.2/1.3). Step-zero includes the
empirical retest of the M05a claim ("open holds an OS-level exclusive lock for the
handle's lifetime"): by source, open_inner releases both locks before returning
(local.rs:390-397); the retest is one concurrent-CLI-during-serve probe, N=5. CLI
one-shot commands keep paying one full load at open — out of D1's scope, priced for D2.

## 2. Step-zero measurement plan (provenance: CITE-or-MARK from birth)

The motivating numbers — 48% of request time in history re-read, load_state ~3x per
request, 15ms -> 36ms during a run — are MARKED: UNSOURCED-IN-REPO (verified 2026-08-19:
zero hits in docs/, commit messages, issues #63, PRs #70/#66/#62; provenance = owner's
out-of-tree measurement relayed via orchestrator). Step zero REPRODUCES them before any
fix lands, or replaces them with sourced ones:

- Instrument: promote the existing `load_count` (local.rs:1075, cfg(test) today) to a
  metrics feature (`#[cfg(any(test, feature = "load-metrics"))]`), plus a per-call
  duration; emit per OPERATION KIND (open/read_replay/next_sequence/append), never
  flattened into one counter (M09 lesson: flattening in instruments — one bucket made
  F2-H2 unfalsifiable; the metric must name which operation paid).
- Metric A: load_state calls per logical operation (record outcome; status read;
  wake arm). Expected from source: 3 for record (driver.rs:66 chain), 1+N for serve
  requests (open + ops). CITE: each expectation carries its call-site line.
- Metric B: load_state wall time vs total request time at journal sizes ~100 / ~1k /
  ~5k events, grown by existing harnesses.
- N >= 10 per point; scopes measured SEPARATELY: serve request path AND CLI one-shot
  (D1 helps only the former — the split is the point); machine named in the artifact
  (this box: owner's Windows 11, same as flake-#2 evidence).
- AFTER numbers: same harness, same N, same sizes, same machine. Success = Metric B
  flat in journal size for warm-handle serve ops; Metric A per-op unchanged in MEANING
  (we amortized verification, never skipped it — each byte still verified exactly once
  per handle, chained from genesis).

## 3. Cache sabotage list (each run individually; expected-greens REPORTED BY NAME)

- SC1 corrupt one byte in the verified prefix, cache WARM -> operation SUCCEEDS
  (EXPECTED GREEN — this is R1's honest widening, reported by name, never hidden);
  then fresh open of the same directory -> CorruptBatch/Integrity EXACTLY as today.
  Both halves asserted; the pair documents the detection-latency trade instead of
  letting it pass silently.
- SC2 truncate the tail MID-LINE, cache warm -> next op: len < offset -> full reload ->
  Integrity, same error as today.
- SC3 truncate at an exact LINE BOUNDARY, cache warm -> next op: full reload -> shorter
  state accepted, same as today (EXPECTED GREEN by policy 1.4, reported by name).
- SC4 external WELL-FORMED append (raw bytes, valid batch) while cache warm -> next op
  verifies the suffix and extends — result identical to today's full reload seeing it.
- SC5 append attempt from a second process WHILE a reader holds the shared lock -> must
  block until release (the 1.3 window-closure claim, tested as an interleave, not
  argued): failpoint holds the shared lock open; the rival append must not land before
  release.
- SC6 chain-break in the suffix (valid checksum, wrong prev-hash) -> Integrity from the
  CACHED last_hash — proves suffix verification chains from the cache, not from a
  re-read (sabotage the cache's last_hash in a test build: verification must then FAIL
  on a legitimate suffix — the guard measures the chaining, not a coincidence).
- SC7 budget equivalence (see 4.3) sabotaged: make incremental accounting skip one
  batch -> the property test MUST fall.

## 4. Boundaries from M09's lessons

- 4.1 Flattening: the load metrics and every new error path name their operation and
  kind; no collapsing distinct io/verification failures into one bucket (the ring()
  lesson: one bucket = unfalsifiable hypotheses downstream).
- 4.2 Final-state oracle: D1's guards assert on what a FRESH open reads back (the
  store's own state), never on the mutating call's reply — the write's own reply is one
  level above where the truth lives (assert-at-the-finest-grain; the M08 lesson paid
  eleven times).
- 4.3 Earn-vs-spend: D1 SPENDS retained memory (LoadedState held per handle, bounded by
  MAX_JOURNAL_BYTES — same peak as today's per-call load, now resident) and a
  path-dependence RISK in LoadBudget. The budget must be provably path-independent:
  property test — for random journals, incremental accounting == from-zero accounting,
  exact equality on every limit counter (SC7 sabotages it). If any limit cannot be made
  path-independent, that limit forces a full recount at its boundary — named, not
  fudged.
- 4.4 Measurer-vs-author: step-zero base numbers and the AFTER numbers should be run by
  someone other than the implementer where the queue allows (flake-#2 board note made
  this class explicit).

## 5. What D1 does not do (unchanged from the proposal, restated as scope fence)

No on-disk snapshot (that is D2, separate milestone decision after D1's numbers). No
change to CLI one-shot cost. No change to any error semantics except the two named
EXPECTED GREENs (SC1 first half, SC3 — both matching or matching-today behavior). No
change to the fold, the wire format, the locks' exclusivity rules, or the doctor.
