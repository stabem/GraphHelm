# D Agent — mechanism study: the api_http storm flake

Study of `the_storm_holds_under_eight_concurrent_agents` (`apps/cli/tests/api_http.rs:1433`).
Branch: `issue-m09-arming-the-alarm` @ `53d212d`. READ-ONLY study — nothing here was measured
by me; every claim below is either "read directly from the code" (cited) or labeled
HYPOTHESIS (NOT MEASURED). Written 2026-08-19 by D Agent.

## (l) RE-DERIVED AGAINST d10916b — anchors re-grepped, not offset

Position 5 opened on `origin/issue-m09-arming-the-alarm` @ **d10916b** ("Merge PR 2 (#74): a
consumption names the arming it burns"). Note it is the tip of the **milestone branch**, not of
`main` — `origin/main` is at `df5e431`. Both diffs re-derived against d10916b; **every anchor
grepped fresh in the new tree**, none offset, because #74's insertions are non-uniform.

**What #74 touched, among my anchor files: `wake.rs` only** (+561 net). Everything else I anchor
on is byte-identical — and that was verified by grepping each site, not inferred from the diff
stat:

| Anchor | At 53d212d | At d10916b |
|---|---|---|
| `event_store` (commands/mod.rs) | 281 | **281** unchanged |
| sweep spawn (serve/mod.rs) | 806 | **806** unchanged |
| `store_open` closure (routes.rs) | 819 | **819** unchanged |
| `raw_request` / connect / write / read | 161 / 163 / 172 / 175 | **unchanged** |
| `post_request` / connect / write / read | 436 / 443 / 460-461 / 464 | **unchanged** |
| storm test / `run_storm` / `scope.spawn` | 1433 / 1460 / 1470 | **unchanged** |
| `status::execute` open (status.rs) | 24 | **24** unchanged |
| **sweep phase 1 (wake.rs)** | 84 | **90 — MOVED** |
| **sweep phase 3 (wake.rs)** | 140 | **147 — MOVED** |

**The cheap-path claim RE-VERIFIED, not inherited** (the orchestrator was right to demand this):
phase 1 still performs exactly **one** `event_store` open (wake.rs:91) before its reads, and the
early return `if due.is_empty() { return; }` is **still present**, now at wake.rs:131. So the
storm — which arms no lease — still takes the one-open cheap path, phases 2 and 3 unreached. C's
phase-3 changes therefore cannot affect the storm's sweep cost. `DueLease` gained an
`armed_at_sequence` field, read from the projection already loaded; **no additional open**.

**Step 0's decision table survives the rebase intact** — `:443` / `:460-461` / `:464` / `:221`
all still name the same phases. One addition found while re-checking: **`:227` is a second
laundering site** (`get_json`'s identical panic), used by `head_sequence`, `last_event_of_kind`
and `all_events`. H's inventory never showed it, but a future run panicking there belongs to the
same phase-unknown class as `:221`.

**⚠ H's baseline is NOT a valid comparison point for post-#74 numbers.** #74 changed
`core/events/src/projection.rs` (+97) and `core/events/src/integrity.rs` (+10) — both in the
replay/open path my instrument measures. H measured at 53d212d. Comparing rung A's numbers
against H's would compare across a store change and attribute the difference to the wrong cause.
This is why the machine order puts a **re-baseline at d10916b before** the instrumented runs, and
it must not be treated as a formality: the re-baseline *is* the comparison point, and H's numbers
are now history rather than control.

---

## Status of claims — read this before anything else

This document grew by accretion through an adversarial review, so corrections sit *next to* the
claims they kill rather than replacing them. A cold reader must not mistake a corpse for a live
claim. **Rewritten in full 2026-08-19 after this header was itself found stale** — the orienting
summary at the top of a document about stale summaries, which is the failure mode the file spends
several sections on. Do not trust an older copy of this block.

- **I have run nothing** — zero cargo commands, by standing order. **But the file now contains
  real measurements: H Agent's**, from a clean run of unmodified `53d212d`
  (`.factory/h-agent-base-measurements.md`). Distinguish carefully: H's numbers are measured;
  every claim of mine is code reading or is labelled HYPOTHESIS (NOT MEASURED).
- **Current standing.** **H5 dead** (it predicts error *codes*, and codes survive the laundered
  site). **H4 dead on the mutation path and explains ZERO RUNS** even in its best case; its
  status-path corner rests on a premise the run never exercised. **H1, H2, H3 entirely open** —
  Step 0 settled the failing *phase* (read starvation), not the mechanism, and all three predict
  a read-phase death equally. **H6** dismissed on first-party evidence plus an unexercised
  argument about dependencies; the instrumentation constraint it produced stands regardless.
- **My retracted claims are numerous and they are NOT listed here** — an enumeration in a header
  is precisely what goes stale first, as this block did. To find them, **grep the file for
  RETRACTED, WITHDRAWN, SUPERSEDED, and DO NOT APPLY.** The largest: the "8 × 600ms" arithmetic;
  "the sweep contends for the runtime thread"; "blocking-pool rows = the sweep"; the per-panic
  20% bound (really zero runs); "disjoint failure modes"; "Patch 2 is redundant"; and a first
  draft of the rung A diff that is fenced off but still present.
- **Nothing is applied.** Every patch here exists only as a diff. One of them —
  the first rung A draft in section (f) — carries a **DO NOT APPLY** banner; apply the version
  under "Rung A, written out" instead.
- **Plan as of now:** Step 0 is **DONE** (H's run). Position 5 = **rung A and Patch 1 together**,
  on the post-flake-3 tree, with one wall-clock requirement across both and three-apparatus
  verification. Rung B deferred (C's file until flake-3 lands). Fix candidates in section (g)
  remain a menu, explicitly not a decision.

Measured symptom (H, N=13): **4/10 isolated, 2/3 in-suite**, always `os error 10060`
(WSAETIMEDOUT), every failing run panicking at the read phase.
Binding context kept intact: commit `53d212d` (readers take a shared lock) states explicitly
it does NOT close this flake — measured 24.0s / 25.5s / 22.5s against ~25s before, unchanged.
Its own named next suspect: the per-request repository open (1.5ms vs 0.37ms op on an empty
store, "opens barely parallelising"). This study agrees and says why.

---

## (a) Mechanism map: one storm request, end to end

### Client side (`api_http.rs`)

- 8 OS threads × 6 rounds; op = `(round + thread_id) % 4` → pause / resume / signal / status,
  12 of each; mutations retry once on 409 with a fresh key. ~48 logical requests + retries.
- Every request is a NEW TCP connection, `Connection: close`, read to EOF.
- `TcpStream::connect` has **no connect timeout** (OS default governs, ~21s of SYN retries on
  Windows). `set_read_timeout(5s)` / `set_write_timeout(5s)` are set AFTER connect
  (`raw_request` :161, `post_request` :436).
- **Windows fact that frames everything:** WSAETIMEDOUT (10060) is returned BOTH for a connect
  that times out AND for an `SO_RCVTIMEO` (read timeout) expiry. So "TCP 10060" in the failure
  does not by itself say which phase died. The 5s read timeout is the tight one; the connect
  path has ~21s of slack.

### Server side — the runtime

- `serve` runs on `tokio::runtime::Builder::new_current_thread()` — the `runtime()` fn is
  `apps/cli/src/commands/events/mod.rs:156`, the builder call itself is **:157**; used by
  `serve/mod.rs:164`. **The entire HTTP server — accept loop, every handler — is ONE thread.**
- The hot request path does synchronous file IO **inline in async handlers** — no
  `spawn_blocking` anywhere on it. `spawn_blocking` exists only for: wake sweep (`wake.rs:84,140`),
  gateway routes/probe (`routes.rs:967,1021`), monitor pages (`monitor.rs:539,585`), and the
  executor ports (`ports.rs`). `status`, `events`, and the whole
  `run_idempotent_mutation` pre-flight + command body run on the runtime thread.
- Consequence: while one handler is inside a blocking file-lock wait or an fsync, NO other
  request progresses — not even `/health` — and accepts are not processed. Concurrency across
  requests is zero on this path; the 8 clients are served strictly one at a time.

### Server side — one mutation request's lock/IO path (pause as the example)

Every step that says "open" means a full `LocalEventRepository::open` (`core/events/src/local.rs:318`),
because `ServeState` deliberately holds no repository handle (`serve/mod.rs:108-118`) — each
open does, in order:

1. `lock.lock_exclusive()` on `repository.lock` — **EXCLUSIVE, blocking, no timeout**
   (`initialize_root_locked`, local.rs:2146; fs2 → LockFileEx without FAIL_IMMEDIATELY).
   Note: on Windows `lock_root_exclusive` is a **no-op** (local.rs:2184) — the named lock file
   is the only cross-process lock.
2. Under that exclusive lock: `validate_anchors` (≥10 handle opens + identity checks),
   `load_state()` — **reads the FULL journal + every batch blob, O(head)** —
   `sync_loaded_journal` — **an fsync (`sync_data`) of the journal on every open of a
   non-empty store, reads included** (local.rs:627-642, called from open at :379) —
   `reconcile_orphans` (directory scans), `publish_active_marker`.
3. Unlock, return handle.

Then, per mutation request (`run_idempotent_mutation`, serve/mod.rs:738):

- **Open #1** — `classify_existing_keys` (:901): `event_store` + `resolve_stream` reads the
  FULL history (shared lock per 53d212d) for the idempotency pre-flight.
- (**Open #1b** only with If-Match: `current_head` (:1073) opens again.)
- **Open #2** — the command body (`execution::pause::execute` etc.) opens the store itself,
  re-reads, appends its decision event (+ per-node fan-out events) under the exclusive lock;
  each append writes a blob + journal entry with `sync_all` calls (local.rs:887, 985, 3169).
- **Open #3** — on success, `current_head` (:797) opens AGAIN just to attach `headSequence`
  (the code's own comment at :787 says why: `execute()` returns no head).
- **Open #4** — `tokio::spawn(wake::sweep(...))` (serve/mod.rs:806-809, comment at 803-805).
  The spawn is `tokio::spawn`, NOT `spawn_blocking`; `spawn_blocking` appears only *inside* the
  sweep, around its two blocking phases (wake.rs:84, :140). Those blocking phases each open the
  store and do a **FULL replay** to look for due leases (wake.rs:84-118); the storm never arms a
  lease, so all 36 of these are pure overhead, and they contend from the blocking pool for the
  same open-time exclusive lock the request path needs. **This lock contention is the sweep's
  whole cost** — see the correction in section (e): the sweep does NOT block the runtime thread.
- A 409 path (frequent by design — pause/resume flip-flop) still costs open #1 + the command's
  own open + a `current_head` open for the conflict body, plus the retry repeats everything.

So: **at least 4 full opens per mutation — a FLOOR, not an estimate.** A successful `resume`
also drives, and the async driver opens the store once per reread and once per append
(driver.rs:220, :274, :308), so resume rounds cost more than four. Each open is a blocking
exclusive lock + full O(head) journal load + one journal fsync + directory scans, with the
request-path ones serialized on the single runtime thread. `status` costs 1 open. History GROWS
as the storm appends (**≥ 36** mutations × 1-2 events, more once 409 retries are counted), so
every open gets more expensive round by round.

### The queueing bound (RETRACTED ARITHMETIC — see below)

**Original claim, WITHDRAWN after B's review:** "~48 requests / ~25s → ~500ms each; 8 × ~600ms
≈ 4.8s, sitting exactly at the 5s timeout." Three things were wrong with it, all verified:

- **There are no synchronized rounds.** `run_storm` (api_http.rs:1468-1474) spawns free-running
  scoped threads with no barrier of any kind. The threads drift apart immediately; "8 × slowest
  in round" describes a structure that does not exist.
- **600ms appears in no measurement.** I chose it because it lands on 4.8s. That is fitting a
  number to a conclusion.
- **~500ms/request attributes the whole wall clock to requests.** The ~25s also contains
  `cli_start` (a full CLI process driving the graph), the server spawn plus health poll, and
  `verify_storm_left_a_coherent_stream`, which pages the entire tail and then runs **two full
  `graph replay` CLI processes** (api_http.rs:1646-1652). Retries are uncounted too — the
  pause/resume flip-flop makes 409s routine, so the real request count is above 48.

**What survives:** a bound, not a number. Because the request path is fully serialized (single
runtime thread, blocking store IO inline), a request's wait is the sum of the work admitted
ahead of it, so the tail grows with concurrency and with history length. That is enough to
motivate measuring; it is not enough to claim the ceiling is "exactly" 5s. **Do not anchor the
fix on the retracted numbers.**

---

## (b) Ranked failure hypotheses

**H1 — the 5s READ timeout expires on a request stuck behind the single-thread convoy.**
HYPOTHESIS (NOT MEASURED). 10060 = SO_RCVTIMEO expiry; the killed request was accepted but its
response was >5s away because the one runtime thread was grinding through other requests'
opens/fsyncs/appends (amplified by 36 sweep opens contending for the open lock from the
blocking pool). Decides it: wrap `raw_request`/`post_request` to record WHICH phase errored
(connect vs write vs read) plus elapsed-per-phase, run the storm N times, and look at the
failing sample. Cheap corroboration: raise only the client read timeout to 30s — if the test
then always passes (with a longer tail, total ~unchanged), the server never hangs, it is just
slower than 5s at the tail. Complementary server-side measurement: per-request service time +
count of store opens per request (the read-audit records reads; opens per request can be
counted with a probe build or ETW/procmon on `repository.lock`).

**H2 — sweep amplification: the fire-and-forget `wake::sweep` per mutation doubles open-lock
pressure.** HYPOTHESIS (NOT MEASURED). Not a separate death mode — a load multiplier inside
H1: 36 extra full opens + full replays racing the request path for the same exclusive
open-time lock, from the blocking pool. Decides it: measure storm total + tail latency with
the sweep spawn commented out (a measurement build, not a fix): if tail collapses, the
multiplier is real.

**H3 — fsync-per-open is the dominant cost term and environmental fsync variance is the
flake's randomness.** HYPOTHESIS (NOT MEASURED). Every open of a non-empty store fsyncs the
journal (`sync_loaded_journal` from `open_inner`), reads included: ~4 fsyncs per mutation
before the append's own `sync_all`s. FlushFileBuffers on Windows is milliseconds-to-tens-of-ms
and varies with disk load and Defender activity on the temp directory — variance that would
explain 3-in-6 on the same machine. Decides it: time `open()` on a store seeded with the
storm's own final history (~100+ events) on this machine, distribution not average; compare
with a Defender exclusion on the temp dir.

**H4 — connect-side 10060: accept starvation long enough to exhaust SYN retries (~21s).**
HYPOTHESIS (NOT MEASURED). **Final standing after Step 0: DISFAVOURED, still live** — see
section (j). Ranked low originally, downgraded after B's review (tokio's 1024-entry backlog
absorbs 8 pending connects, and on Windows loopback an over-full backlog answers 10061 refused
rather than 10060), and *not* killed by H's run: the 10061 argument was never exercised because
no connect failure was ever captured, and the laundered `get_status` rows could hide one. Retired
only by Patch 1's `connect_us`/`read_us` split.

**H5 — client-side resource exhaustion (ephemeral ports / TIME_WAIT).** HYPOTHESIS
(NOT MEASURED). ~150 short-lived loopback connections is far below Windows' default ephemeral
range; TIME_WAIT would surface as 10048/10055, not 10060. Kept only for completeness; the
phase instrumentation kills or revives it too.

**A stale-comment finding worth re-verifying (feeds the fix, so it matters):**
`ServeState.events`' doc comment (serve/mod.rs:108-118) justifies per-request open with
"Task 1 confirmed empirically that `open` holds an OS-level exclusive lock for the handle's
entire lifetime, so a cached handle would lock out every concurrent CLI process." The CURRENT
code does not do that: `open_inner` **unlocks** the named lock before returning
(local.rs:390-397), and `with_lock` takes/releases it per operation (:540-587). If that
empirical claim is now stale, the architectural reason for per-request open is gone, and a
cached handle (locks held only during operations) becomes the obvious fix direction.
HYPOTHESIS (NOT MEASURED) — decided by a two-process probe: hold an open server handle idle,
run `graphhelm execution status` against the same directory from another process; if it
answers, the claim is stale.

---

## (c) Draft sabotage list for the eventual fix

Named breakages the eventual guard(s) must be observed to catch, whatever shape the fix takes:

1. **Re-serialize the runtime**: if the fix moves store work off the runtime thread
   (multi-thread runtime or `spawn_blocking` on the hot path), sabotage = put one route's
   store work back inline on a current-thread runtime → the guard (tail-latency bound, or
   "/health answers < Xms while a slow request is in flight") must fail.
2. **Re-open per step**: if the fix caches/shares a repository handle or coalesces the ~4
   opens per mutation into 1, sabotage = restore one extra `event_store()` call in
   `run_idempotent_mutation`'s success path → an opens-per-request guard must fail (the guard
   must COUNT, not eyeball — assert at the grain of "opens per request", not "test got faster").
3. **Re-fsync on read**: if the fix drops `sync_loaded_journal` from the read/open path,
   sabotage = put the fsync back on open → a syncs-per-read-op guard must fail
   (`journal_sync_count` already exists under `#[cfg(test)]`, local.rs:640 — the finest grain
   is already instrumented).
4. **Lock-out regression**: any handle-caching fix must keep the CLI able to operate on the
   same events directory while the server holds its handle idle — sabotage = hold the named
   lock for the handle's lifetime again (the behavior the stale comment describes) → a
   two-process coexistence guard must fail.
5. **Sweep still swept**: if the fix batches/debounces the per-mutation wake sweep, sabotage =
   drop the sweep entirely → the wake-path guards (C Agent's territory — coordinate before
   touching) must fail, proving the sweep still runs; and sabotage = restore per-mutation
   sweep → the opens-per-mutation guard from (2) must fail.
6. **Timeout papering**: raising the client's 5s read timeout may be honest as a measurement
   but must NOT be the fix alone — sabotage for any latency guard = re-tighten to 5s on the
   unfixed code and observe the original failure reproduce, proving the guard measures the
   server, not the client's patience.

**This list is extended by three more items in section (i)** — guard realism, attribution, and
sabotaging the instrument itself. The third is the one this list was missing entirely.

Boundary note: this study read `core/events/src/local.rs` and `serve/wake.rs` because the
request path crosses them; the wake_http side (wake.rs behavior, its tests) is C Agent's.
Fix-phase file ownership to be assigned by the orchestrator.

---

## Fix scope, as set by the orchestrator (2026-08-19)

A independently found the same per-request repository open (`serve/mod.rs:106`) justified by an
obsolete M05a claim — the same finding recorded above as "a stale-comment finding worth
re-verifying", reached from a different direction. One mechanism, two symptoms: the storm
convoy (this study) and the O(history) per-request cost (A's). **The storm fix stays minimal
and aimed at the flake; the root fix is M10's.** D holds the storm-fix pen, activating after
W1's base numbers land. The phase-logged client measurement below is pre-approved as the
deciding step — PREPARED HERE, APPLIED NOWHERE.

---

## (d) The deciding measurement, prepared: phase-attributed client instrumentation

Purpose: decide H1 (read-timeout expiry behind the serialized request path) against H4
(connect-side death), and put a latency distribution where the retracted "~500ms serial, 8 deep,
5s ceiling" arithmetic used to sit. **That arithmetic is dead, not merely unmeasured** — this
patch does not vindicate it, it replaces it. This is a MEASUREMENT BUILD, not a fix, and
not a commit: apply, run, record numbers, revert.

**What it deliberately does NOT change.** `TcpStream::connect` stays exactly as it is — no
`connect_timeout`. Imposing our own connect deadline would change *when* a connect failure
fires, i.e. change the thing being measured. Phase attribution comes from WHICH call returned
the error, never from the error's kind — and that distinction is the whole point on Windows,
where `error.kind()` is `TimedOut` for both a connect timeout and an `SO_RCVTIMEO` expiry
(the 10060 dual meaning). The 5s read/write timeouts stay 5s.

**⚠ The embedded original error text is EVIDENCE-BEARING — do not reformat it** (L's closing
catch, verified). `probe_failure` rewraps as `io::Error::new(kind, "… ({error}) …")`, and that
`({error})` is what preserves the original `os error 10060` Display. H5 died *by the OS code
surviving into the panic text at the laundered site* — so dropping or reformatting that
interpolation later would retroactively destroy the only evidence that killed a hypothesis. A
second, currently inert consequence to state so nobody trips on it: the rewrapped error has **no
`raw_os_error()`** (an `io::Error::new` Custom returns `None`). Nothing consumes it
programmatically today, but a future check written against `raw_os_error` would silently get
`None` on exactly the path that matters. An instrument that destroys the evidence which killed a
hypothesis is the flattening problem again, self-inflicted.

**Expected noise:** `wait_for_health` polls through `raw_request` during server startup, so a
short burst of `outcome=err phase=connect kind=ConnectionRefused` lines is normal and is
filtered out by that kind when reading results.

### Patch 1 — phase timing in both request helpers (`apps/cli/tests/api_http.rs`)

```diff
@@ apps/cli/tests/api_http.rs: imports
 use std::io::{BufRead, BufReader, Read, Write};
 use std::net::TcpStream;
 use std::path::{Path, PathBuf};
 use std::process::{Child, Command, Stdio};
+use std::sync::OnceLock;
 use std::time::{Duration, Instant};

@@ apps/cli/tests/api_http.rs: after `struct RawResponse`, before `raw_request`
+// -----------------------------------------------------------------------------------------
+// MEASUREMENT ONLY — revert before committing anything. Records, per request, how long each
+// phase took and which phase an error came out of. Exists because Windows reports WSAETIMEDOUT
+// (10060) for BOTH a connect timeout and a read-timeout expiry, so the storm's failure text
+// alone cannot say which one killed the request.
+// -----------------------------------------------------------------------------------------
+
+#[derive(Default)]
+struct PhaseTimes {
+    connect_us: u128,
+    write_us: u128,
+    read_us: u128,
+}
+
+/// EPOCH microseconds, matching rung A's server-side stamps exactly. This is what makes the
+/// two instruments comparable: the client clock is the INDEPENDENT apparatus that validates
+/// rung A's storm-phase denominator (first client request must precede the first server open
+/// row; last client response must follow the last). A per-process `Instant` here would make
+/// that cross-check impossible, which is the whole reason Patch 1 shares the slot.
+fn probe_epoch_us() -> u128 {
+    std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map_or(0, |d| d.as_micros())
+}
+
+/// A FILE sink, not stderr — matching rung A. libtest's capture is thread-local and is NOT
+/// inherited by scoped storm threads, so stderr would capture the health-poll noise while the
+/// storm's own samples bypassed it: two sinks for one dataset. `create_new` refuses an existing
+/// file so run N cannot be appended onto run N-1, and a path that cannot be created PANICS
+/// rather than degrading to silence.
+fn probe_sink() -> Option<&'static Mutex<std::fs::File>> {
+    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
+    SINK.get_or_init(|| {
+        let path = std::env::var("GRAPHHELM_CLIENT_PROBE").ok()?;
+        let file = std::fs::OpenOptions::new()
+            .create_new(true)
+            .write(true)
+            .open(&path)
+            .unwrap_or_else(|error| panic!("GRAPHHELM_CLIENT_PROBE unusable at {path}: {error}"));
+        Some(Mutex::new(file))
+    })
+    .as_ref()
+}
+
+/// One line per request, built as a single formatted string so eight threads interleave by
+/// whole line rather than mid-line.
+fn probe_record(outcome: &str, phase: &str, kind: &str, url: &str, times: &PhaseTimes) {
+    let Some(sink) = probe_sink() else { return };
+    let line = format!(
+        "CLIENT t={} thread={} outcome={outcome} phase={phase} kind={kind} \
+         connect_us={} write_us={} read_us={} url={url}\n",
+        probe_epoch_us(),
+        std::thread::current().name().unwrap_or("main"),
+        times.connect_us,
+        times.write_us,
+        times.read_us
+    );
+    if let Ok(mut file) = sink.lock() {
+        let _ = file.write_all(line.as_bytes());
+    }
+}
+
+/// Records the failure and rewraps it with the phase and every phase's elapsed time, KEEPING
+/// the original `ErrorKind` so callers behave exactly as before.
+fn probe_failure(
+    phase: &str,
+    url: &str,
+    times: &PhaseTimes,
+    error: std::io::Error,
+) -> std::io::Error {
+    probe_record("err", phase, &format!("{:?}", error.kind()), url, times);
+    std::io::Error::new(
+        error.kind(),
+        format!(
+            "{phase} failed after connect={}us write={}us read={}us ({error}) [{url}]",
+            times.connect_us, times.write_us, times.read_us
+        ),
+    )
+}
+
@@ apps/cli/tests/api_http.rs: fn raw_request
 fn raw_request(url: &str, token: Option<&str>) -> std::io::Result<RawResponse> {
     let (host, port, path) = split_url(url);
-    let mut stream = TcpStream::connect((host.as_str(), port))?;
+    let mut times = PhaseTimes::default();
+
+    let started = Instant::now();
+    let connected = TcpStream::connect((host.as_str(), port));
+    times.connect_us = started.elapsed().as_micros();
+    let mut stream = match connected {
+        Ok(stream) => stream,
+        Err(error) => return Err(probe_failure("connect", url, &times, error)),
+    };
     stream.set_read_timeout(Some(Duration::from_secs(5)))?;
     stream.set_write_timeout(Some(Duration::from_secs(5)))?;

     let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
     if let Some(token) = token {
         request.push_str(&format!("Authorization: Bearer {token}\r\n"));
     }
     request.push_str("\r\n");
-    stream.write_all(request.as_bytes())?;
+    let started = Instant::now();
+    let written = stream.write_all(request.as_bytes());
+    times.write_us = started.elapsed().as_micros();
+    if let Err(error) = written {
+        return Err(probe_failure("write", url, &times, error));
+    }

     let mut raw = Vec::new();
-    stream.read_to_end(&mut raw)?;
+    let started = Instant::now();
+    let read = stream.read_to_end(&mut raw);
+    times.read_us = started.elapsed().as_micros();
+    if let Err(error) = read {
+        return Err(probe_failure("read", url, &times, error));
+    }
+    probe_record("ok", "-", "-", url, &times);
     parse_response(&String::from_utf8_lossy(&raw))
 }

@@ apps/cli/tests/api_http.rs: fn post_request
 ) -> RawResponse {
     let (host, port, path) = split_url(url);
-    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap();
+    let mut times = PhaseTimes::default();
+
+    let started = Instant::now();
+    let connected = TcpStream::connect((host.as_str(), port));
+    times.connect_us = started.elapsed().as_micros();
+    let mut stream = match connected {
+        Ok(stream) => stream,
+        Err(error) => panic!("{}", probe_failure("connect", url, &times, error)),
+    };
     stream
         .set_read_timeout(Some(Duration::from_secs(5)))
         .unwrap();
     stream
         .set_write_timeout(Some(Duration::from_secs(5)))
         .unwrap();
@@ (same function, the write and read phases)
     request.push_str("\r\n");
-    stream.write_all(request.as_bytes()).unwrap();
-    stream.write_all(&payload).unwrap();
+    // TWO STATEMENTS, deliberately, not `.and_then(|()| stream.write_all(&payload))`: that
+    // closure borrows `stream` mutably inside a call whose receiver is `stream`. It is probably
+    // fine under NLL — and nobody can run cargo to find out, so it is not worth the gamble in a
+    // measurement patch. `write_us` covers header AND payload as one phase; stated, not implied.
+    let started = Instant::now();
+    let mut written = stream.write_all(request.as_bytes());
+    if written.is_ok() {
+        written = stream.write_all(&payload);
+    }
+    times.write_us = started.elapsed().as_micros();
+    if let Err(error) = written {
+        panic!("{}", probe_failure("write", url, &times, error));
+    }

     let mut raw = Vec::new();
-    stream.read_to_end(&mut raw).unwrap();
+    let started = Instant::now();
+    let read = stream.read_to_end(&mut raw);
+    times.read_us = started.elapsed().as_micros();
+    if let Err(error) = read {
+        panic!("{}", probe_failure("read", url, &times, error));
+    }
+    probe_record("ok", "-", "-", url, &times);
     parse_response(&String::from_utf8_lossy(&raw)).unwrap()
 }

@@ apps/cli/tests/api_http.rs: fn run_storm — name the threads so `thread=` identifies them
     std::thread::scope(|scope| {
         for thread_id in 0..threads {
-            scope.spawn(move || {
-                storm_thread(base, token, execution, evidence_dir, thread_id, rounds);
-            });
+            std::thread::Builder::new()
+                .name(format!("storm-{thread_id}"))
+                .spawn_scoped(scope, move || {
+                    storm_thread(base, token, execution, evidence_dir, thread_id, rounds);
+                })
+                .expect("spawning a named storm thread");
         }
     });

@@ apps/cli/tests/api_http.rs: the storm test — preserve the events directory so the panic
@@ stops destroying its own evidence (D-P10-ALT). `into_path()` after `run_storm` is
@@ UNREACHABLE: the scope re-panics at the join, so no later line executes and `TempDir::drop`
@@ deletes the tree while unwinding. `catch_unwind` + `resume_unwind` keeps the ORIGINAL panic
@@ and its file:line intact, which Step 0's whole method depends on.
     let directory = tempfile::tempdir().unwrap();
     …
-    run_storm(&base, &token, execution, &evidence_dir, STORM_THREADS, STORM_ROUNDS);
+    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
+        run_storm(&base, &token, execution, &evidence_dir, STORM_THREADS, STORM_ROUNDS);
+    }));
+    // Preserved on PASSING runs too — without a passing baseline there is nothing to compare
+    // the failing committed-event count against. Printed, or the path is unfindable.
+    if std::env::var("GRAPHHELM_CLIENT_PROBE").is_ok() {
+        let kept = std::mem::replace(&mut directory, tempfile::tempdir().unwrap()).keep();
+        println!("PRESERVED events tree at {}", kept.display());
+    }
+    if let Err(payload) = outcome {
+        std::panic::resume_unwind(payload);
+    }
```

**Note on the preservation hunk:** `directory` must become `let mut directory` for the
`mem::replace`, and `TempDir::keep` is the current name for what older releases called
`into_path`. The replacement tempdir is dropped normally and costs nothing. This hunk is the
only part of Patch 1 that is not a pure observer, so it is gated behind the same env var: with
the probe off, the test behaves exactly as it does today.

### How to run it, and what each outcome means

```
cargo test -p graphhelm-cli --test api_http the_storm_holds_under_eight_concurrent_agents -- --nocapture
```

Repeat until at least two failing runs are captured (the flake is ~3 in 6), keeping every run's
stderr. Then read the PROBE lines:

| What the failing sample shows | Verdict |
|---|---|
| `phase=read kind=TimedOut`, `read≈5000ms`, `connect` small | **H1 confirmed**: accepted, then starved of a response. The convoy is the mechanism. |
| `phase=connect kind=TimedOut`, `connect≈21000ms` | **H4 revives, H1 refuted**: accept starvation, not response starvation. |
| Successes show `read_us` climbing toward 5 000 000 across rounds | Corroborates the threshold-on-tail reading, and says the ceiling is latency, not a hang. |
| Successes stay flat and one request dies alone | Refutes the convoy arithmetic; look for a genuine single-request stall instead. |

Also worth reading off the same output for free: total wall time, and whether `read_us` grows
monotonically with round (the O(history) cost A found would show up exactly there).

**Honest limit of this patch.** It says WHICH PHASE and HOW LONG. It does not say WHY the server
was slow — it cannot distinguish "the single runtime thread was busy" (H1) from "the sweep's
extra opens made it busy" (H2) from "fsync stalled" (H3). Those need the server-side
measurements below, which are separate builds and separate steps.

### Patch 2 — H2 discriminator (server-side, SEPARATE step, not to be run together)

Only after Patch 1 has confirmed the phase. One line, in `run_idempotent_mutation`'s success
arm (`apps/cli/src/commands/serve/mod.rs:806`):

```diff
-            tokio::spawn(wake::sweep(
-                std::sync::Arc::from(events),
-                execution.to_owned(),
-            ));
+            // MEASUREMENT ONLY (H2): does the per-mutation sweep's extra open + full replay
+            // drive the storm's tail? Revert immediately — this breaks the doorbell.
+            let _ = (&events, &execution);
```

Read the storm's tail latency from Patch 1's PROBE lines with and without this. A collapsed
tail means the sweep is a real multiplier and the minimal fix has a target. **This deliberately
breaks the wake path — wake_http failures under it are expected and are themselves the proof
the sweep runs.** The wake path is C Agent's territory: coordinate before running this, and it
never leaves the working tree.

### Patch 3 — H3, deferred

`journal_sync_count` already exists under `#[cfg(test)]` (`core/events/src/local.rs:640`), which
is the finest grain for "how many fsyncs did one request cost". Reaching it from an
out-of-process integration test needs a counter surface that does not exist today, so H3 is
better measured directly — time `LocalEventRepository::open` against a store seeded with the
storm's own final history, as a distribution, in a small standalone test — rather than by
instrumenting the storm. Not drafted here; it is not on the critical path for the flake.

---

## (e) Revision after B's adversarial review (2026-08-19)

Each claim re-checked against the code before answering. Verdicts, then the consequences.

| B's point | Verdict |
|---|---|
| T1 arithmetic is constructed | **ACCEPT in full.** Retracted above. |
| T2 phase attribution is already free | **ACCEPT for mutations, REFUTE for status** — see below. |
| T2 loopback backlog gives 10061, not 10060 | **ACCEPT.** H4 downgraded above. |
| T3 stale doc confirmed, cached handle couples to sabotage #1 | **ACCEPT, and it is sharper than stated.** |
| T4 phase-log cannot split H1/H2/H3 | **Already my own stated limit**, not new — but B's replacement experiment is better than my three ablations. **ACCEPT the histogram.** |
| Nits (events/mod.rs:157; `tokio::spawn` vs `spawn_blocking`) | **ACCEPT.** Both corrected above. |

**T2, the correction that matters.** Verified: `post_request` unwraps at distinct lines —
connect :443, write :460 and :461, read :464 — so a panic's own `file:line` names the phase for
free, exactly as B says. But this holds only for the three MUTATION operations. The status
operation goes through `raw_request`, whose `?` propagates the error into `get_status`
(:219-223), which panics with the uniform text `request to {url} failed: {error}`. On that path
the phase is **laundered** — one panic site for all three phases, and on Windows the error kind
is `TimedOut` for both connect and read. So: if W1's failure text points into `post_request`,
the phase is already known and Patch 1 buys only elapsed-per-phase; if it points at
`get_status`, the phase is still open and Patch 1 earns its place.

**T3, sharper.** `with_lock` takes `self.operation_gate.lock()` FIRST (local.rs:554) and holds
that plain mutex across the whole operation — the file-lock wait, the full journal load, every
fsync. One cached, shared handle would therefore serialize **every in-process operation, reads
included**, on that mutex — deleting exactly the win 53d212d bought. And 53d212d's own recorded
measurement ("making the gate an `RwLock` moved nothing") does **not** transfer: it moved
nothing *because* the gate was per-instance and serve opened one instance per request. With a
shared instance the gate becomes load-bearing for the first time, and that measurement no
longer applies. Consequence for the fix: caching the handle is not a one-line change; it drags
in the gate and the journal mutex with it. Sabotage items 1 and 4 are coupled and must be
designed together.

**H6, raised and dropped in the same breath.** I checked whether the harness could manufacture
a hang: `serve_with` pipes the server's stderr and **never drains it** (it is read only on the
no-stdout error path), so anything writing more than the ~64KB pipe buffer to stderr would
block the server forever — a hang indistinguishable from this flake. Grepped: **zero**
`eprintln!`/`eprint!`/`io::stderr` in `apps/cli/src` and zero in `core/`. Only the panic hook
writes there, orders of magnitude below the buffer. So H6 is not a live hypothesis — but it IS
a hard constraint on any measurement build: **never instrument the server to stderr under this
harness.**

> **Rule 9 applied backwards to this paragraph** (M's meta-rule: run a new rule over already-sealed
> claims *as part of adopting it*). That "zero" names a population — **first-party source only**.
> The dependency tree was never searched, and the hypothesis could live there: any crate in the
> serve path that writes to stderr under some condition would satisfy H6 while leaving my grep at
> zero. So the honest form is: zero first-party writers, plus an *argument* that this stack
> (axum, tokio, no tracing subscriber installed) does not write to stderr in normal operation.
> H6 stays dismissed — but on a first-party measurement plus an unexercised argument about
> dependencies, which is exactly the structure rule 9 exists to make visible. The constraint it
> produced is unaffected either way.

### Revised measurement order (supersedes section (d)'s ordering)

**Step 0 — free, do this first.** Read W1's verbatim failure text. If it names
`post_request` :443/:460/:461/:464, the phase is known with zero new runs. (Written before the
run. What actually happened: H5 died there, H4 only shrank — see section (j).)
Only if it names `get_status` does Patch 1 become the phase-deciding step.

**Step 1 — the decider: a per-open timing histogram, split by caller.** B's design, adopted
over my three separate ablations, because one build yields the fsync share, the sweep share and
the serial share simultaneously rather than three runs each answering one question. Two design
constraints B did not state, both load-bearing:

- **`serve` is a separate process.** In-process statics in the test binary cannot see the
  server's opens. The histogram must be written by the server to a **file**, gated behind an
  env var, and read after the run.
- **Not stderr**, per H6 above.

And one simplification: the caller split needs no tag threaded through `event_store`. Request-path
opens run on the current-thread runtime's own thread (`rt.block_on`, serve/mod.rs:166); the
sweep's opens run in `spawn_blocking`, i.e. on the blocking pool. `std::thread::current().id()`
separates them for free. Sample line per open: thread id, elapsed for the whole open, elapsed
for the lock acquisition alone, elapsed for `sync_loaded_journal`, and the event count loaded.
That last field is what makes A's O(history) cost visible in the same data.

Reading it: lock-wait share large → the convoy is contention (H1); `sync_loaded_journal` share
large → fsync dominates (H3); blocking-pool rows a large fraction of all opens → the sweep is a
real multiplier (H2). These are not exclusive, and the histogram gives all three proportions at
once instead of forcing a choice.

**Step 2 — Patch 1.** *(Written when Patch 1 was demoted. Both halves of this paragraph were
later overturned — kept with the corrections attached rather than rewritten, since the reversals
are part of the record.)* Step 0 **did** land on `get_status` (4 hits across 3 runs), so Patch 1
was **re-promoted**; and it was promoted again when it turned out to be the only independent
clock validating rung A's storm-phase denominator. It runs **with** rung A in position 5.
~~Patch 2 (the sweep ablation) is now redundant with Step 1's blocking-pool rows~~ — **wrong, and
a descendant of the retracted contention claim**: the thread-id share was never a valid sweep
measure (the driver shares the pool), while the ablation is valid under either threading model.
Patch 2 is **NOT** redundant; it stands as the fallback until `caller=` exists.

### Correction: my own runtime-thread claim was overstated (and B's extension of it is refuted)

I told B, and wrote above, that the sweep's async half "competes directly with request handling"
on the single runtime thread. B extended it: the phase-2 ring is sync IO awaited on that thread,
so a slow named-pipe client would stall every request. I read the Windows ring before accepting
that, and **both statements are wrong on this platform**:

- `ClientOptions::open` (wake.rs:55) is synchronous, but it is a single `CreateFile` against
  `\\.\pipe\...`. It does not wait for a free pipe instance (no `WaitNamedPipe` here — a busy
  pipe returns immediately, a missing one returns not-found immediately). Microseconds, not a
  stall vector.
- `client.write_all(&[1_u8]).await` (wake.rs:56) is genuinely **async**. A non-reading or slow
  client parks the task; it cannot pin the runtime thread. B's stall scenario is **REFUTED**.
- Every other step of the sweep is `spawn_blocking(...).await` (wake.rs:84, :140), which yields.

So the sweep's async half **yields at every step**. What is left is scheduler-level polling cost,
which is negligible — not the "direct contention" I claimed. **The sweep's real and only cost is
what the original mechanism map already said:** extra store opens from the blocking pool,
contending on the store's file lock, each doing a full replay for nothing.

The inverse remains true and is the actual asymmetry: while the runtime thread is blocked inside
a request handler's inline sync store IO, **nothing** else on that thread is polled — sweep
tasks included. The request path blocks the sweep, not the other way round.

Consequence for Step 1: a phase-2 ring-elapsed field is nearly free, so include it, but expect it
near zero. It is a confirmation field, **not a discriminator** — no hypothesis now rests on it.

---

## (f) Step 1 prepared: the per-open histogram, as a two-rung ladder

Drafted, applied nowhere. Rung A is entirely inside my own file boundary and answers the first
question; rung B crosses into `core/events/src/local.rs`, which the orchestrator assigned to C
as a study file, and is only worth the coordination if rung A says opens dominate.

**Why a ladder rather than one build:** rung A tells us how much of a request is spent opening
the store, and how many of those opens come from the sweep. If that share is small, the internal
split rung B measures answers a question that no longer matters, and we look at the append path
instead. If it is large, rung B says *why* — lock wait (H1) or fsync (H3).

### Rung A — total open cost per caller, at the one choke point (mine)

Every serve open funnels through `event_store` (`apps/cli/src/commands/mod.rs:281-285`), so one
function carries the whole measurement.

> ### ⛔ THE DIFF BELOW IS SUPERSEDED — DO NOT APPLY IT
>
> **Apply the version in "Rung A, written out" instead.** This first draft is kept only so the
> history stays visible, and it is fenced off because it is *executable text*: applying it would
> silently reinstate five things the review killed — a two-way caller split that mis-attributes
> driver opens as `request`, a sink that fails silently when the path is unusable, `append` mode
> that concatenates run N onto run N-1, `ok: bool` instead of the error kind, no begin row (so an
> open parked on the blocking lock writes nothing at all), and a per-process `Instant` that cannot
> bound the storm phase across pids.
>
> Finding this was the lane's own lesson landing on me a fourth time: I corrected the *prose*
> around this block repeatedly while leaving the **artifact** — the part someone would actually
> run — carrying the superseded design. Grepping for a field name (`_ms`) is what surfaced it,
> exactly as the retraction rule's operational form says.

```diff
@@ apps/cli/src/commands/mod.rs  [SUPERSEDED DRAFT — see "Rung A, written out"]
 pub(super) fn event_store(
     path: &std::path::Path,
 ) -> Result<LocalEventRepository, EventRepositoryError> {
-    LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds))
+    let started = std::time::Instant::now();
+    let opened = LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds));
+    open_probe::record(started.elapsed(), opened.is_ok());
+    opened
 }
+
+/// MEASUREMENT ONLY — revert before committing anything. Writes one line per store open to the
+/// file named by `GRAPHHELM_OPEN_PROBE`, or does nothing at all when that variable is unset.
+/// Deliberately NOT `#[cfg(test)]`: the storm drives a separately spawned `graphhelm` binary,
+/// built without the test cfg, so a test-gated probe would record nothing. Deliberately NOT
+/// stderr: this harness pipes the server's stderr and never drains it (see H6), so writing
+/// there would manufacture the very hang under study.
+mod open_probe {
+    use std::fs::{File, OpenOptions};
+    use std::io::Write;
+    use std::sync::{Mutex, OnceLock};
+    use std::time::{Duration, Instant};
+
+    static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
+    static START: OnceLock<Instant> = OnceLock::new();
+
+    fn sink() -> &'static Option<Mutex<File>> {
+        SINK.get_or_init(|| {
+            let path = std::env::var("GRAPHHELM_OPEN_PROBE").ok()?;
+            let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
+            Some(Mutex::new(file))
+        })
+    }
+
+    pub(super) fn record(elapsed: Duration, ok: bool) {
+        let Some(sink) = sink() else { return };
+        let since = START.get_or_init(Instant::now).elapsed().as_micros();
+        let line = format!(
+            "open pid={} thread={:?} t={since}us elapsed={}us ok={ok}\n",
+            std::process::id(),
+            std::thread::current().id(),
+            elapsed.as_micros()
+        );
+        if let Ok(mut file) = sink.lock() {
+            let _ = file.write_all(line.as_bytes());
+        }
+    }
+}
```

**Two things that must be understood before reading the output:**

- **`pid` is not decoration.** `std::process::Command` inherits the parent environment, so
  setting `GRAPHHELM_OPEN_PROBE` for the `cargo test` run propagates it to the spawned server
  *and* to every short-lived `cli()` invocation (`cli_start`, and the two `graph replay`
  processes in the verify step). All of them append to the same file. The server is the pid with
  many rows spanning the whole run; the CLI pids are short bursts. Without `pid` the rows are
  uninterpretable.
- **Cross-process interleaving.** The `Mutex` only orders writers within one process. Across
  processes this relies on append-mode writes of a sub-4KB line being atomic, which holds on
  Windows for `FILE_APPEND_DATA`. Lines are ~90 bytes.

**The caller split is NOT free — this was wrong, see section (i).** I claimed the blocking pool
belonged to the sweep, so a thread id would separate sweep opens from request opens. It does not:
the async driver opens the store only inside `spawn_blocking` (driver.rs:220, :274, :308, each
calling the `store_open` closure built at routes.rs:819), and a successful `resume` reaches it
whenever `drive_is_viable_for` passes (routes.rs:765-771 — true in fixture mode too, since it
falls through to the node-classification test when `runtime` is `None`). The storm's rotation
issues up to 12 resumes, so pool rows are **driver + sweep mixed**, and the sweep share would
read high by exactly the driver's fan-out. Thread ids are also reused by tokio across callers.

**Repair, inside my own boundary:** a `thread_local` caller label, defaulting to `"request"` and
set to `"sweep"` at wake.rs:84 and :140, printed as `caller=`. It survives pool-thread reuse,
which a thread id does not.

Read it as: opens per request (row count ÷ requests), total time in opens vs **the storm phase**
(the serial share), and the `caller=sweep` share (H2's effect size). **Patch 2 is therefore NOT
redundant** — the ablation is valid under either threading model, while the thread-id share never
was. Keep Patch 2 as the fallback until `caller=` exists.

**The denominator is the storm phase, NEVER the run's wall clock.** A fourth surviving descendant
of the retracted T1 arithmetic, caught by L's corpse audit: the ~25s wall clock was disqualified
as a per-request divisor precisely because it contains `cli_start`, the spawn and health poll,
and the verify step — yet rung A's reading instruction and C1's selector both still divided by
it. Using it would dilute the serial share and could **wrongly reject C1**. The phase is bounded
from the probe rows themselves: first-to-last `caller=request` row of the **server** pid, with
the CLI pid bursts marking `cli_start` and the verify replays. **G2 (epoch microseconds) is
therefore REQUIRED, not cosmetic** — per-process `Instant` origins cannot bound a phase across
pids, and cross-pid ordering is exactly what identifies the bursts that bracket it.

**Residual contamination in the new denominator, stated rather than inherited.** The server pid's
rows do not stop at the storm: `verify_storm_left_a_coherent_stream` calls `all_events` over
HTTP *before* it runs the two CLI replays, and those reads are single `caller=request` opens
indistinguishable by tag from the storm's own status reads. With the storm's history under the
1000-event page cap, `all_events` costs one full page plus one empty page — **2 extra rows**.
So the phase's true end is the last row before that trailing pair, identifiable as the ≤2
single-open reads falling after the final `caller=sweep` row and before the CLI replay burst.
The residual bias is bounded by those two rows' span and must be reported with the number rather
than silently absorbed — replacing a contaminated denominator with a quietly-contaminated one is
the same defect one refactor later.

**The phase-end rule needs TWO ARMS, not one** (L, round 4 — accepted, and I had missed it):
`verify_storm_left_a_coherent_stream` runs **only on passing runs**, because the panic at the
scope join skips it entirely. So the trailing pair exists on passing runs *only*. On a **failing**
run the phase simply ends at the last `caller=request` row, with nothing to strip — applying the
passing-run rule there would truncate real storm rows and shorten the denominator, biasing the
serial share upward. One rule, two arms, chosen by run outcome.

**Probe gaps that must be closed before rung A is worth running** (L's list, all accepted except
where noted):

- **G1, the worst:** `record()` fires only *after* `open` returns, so an open parked on the
  blocking, timeout-free `lock.lock_exclusive()` (local.rs:2146) writes **no row at all** —
  blind at exactly the point H1 lives. Emit a begin row and an end row; a begin with no end *is*
  the evidence.
- **G2:** the per-process `Instant` makes `t=` a private timeline, so server and CLI rows cannot
  be interleaved. Stamp epoch microseconds instead.
- **G3:** a mutex plus a `write_all` syscall per open, inline on the runtime thread, into a
  virus-scanned temp tree, is observer effect at the contended point. Bound it with a probe-off
  control run.
- **G4:** `sink()` returning `None` for both "env unset" and "file could not be opened" makes a
  wasted run look like a deliberate probe-off run. Fail loudly — this is measurement code.
- **G5:** append plus a fixed path concatenates run N onto run N-1; the protocol wants ≥10 runs.
  Fresh path per run, and refuse an existing file.
- **G6:** env inheritance reaches every other test's spawned server too. Run with an exact filter
  and `--test-threads=1`; rows separate by pid, not by test.
- **G7 — PARTIALLY REFUTED.** The direct `LocalEventRepository::open` L cites (api_http.rs:2536)
  is `strand_running`, a helper the storm test never calls. The storm's verify step uses `cli()`
  subprocesses, which *do* go through `commands::event_store` and so *are* recorded, under their
  own pids. The general warning stands — test-process direct opens are invisible to this probe —
  but it does not hole the storm's own accounting.
- **G8:** record the error kind, not `ok: bool`.

### Rung A, written out — the gates as an actual diff, not prose

I argued rung A should take the contested slot because it separates H1/H2/H3 while Patch 1 only
settles a corner (I said "20%" then; the run-unit correction later made it **zero runs** — see
section (j)). That argument was incomplete in my own favour: **rung A was not ready.** Its four
gates existed as prose while Patch 1 existed as a diff, so "rung A should win the slot" was advice
I had not made actionable. Written out below so value and readiness agree. *(The slot question
was itself later dissolved: Patch 1 turned out to be the only independent clock validating rung
A's denominator, so the two run together rather than competing.)*

**A third caller tag, which the prose version missed.** The review killed "pool = sweep" because
the async driver opens from the same pool. A two-way `request`/`sweep` tag would therefore *still*
mis-attribute — driver opens would silently count as `request` and inflate the serial share, the
same defect pointed the other way. Three tags: `request` (default), `sweep`, `driver`.

```diff
@@ apps/cli/src/commands/mod.rs
 pub(super) fn event_store(
     path: &std::path::Path,
 ) -> Result<LocalEventRepository, EventRepositoryError> {
-    LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds))
+    let span = open_probe::begin();
+    let opened = LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds));
+    open_probe::end(span, opened.as_ref().err());
+    opened
 }
+
+/// MEASUREMENT ONLY — revert before committing. Writes one begin row and one end row per store
+/// open to the file named by `GRAPHHELM_OPEN_PROBE`; silent no-op when that variable is unset.
+/// NOT `#[cfg(test)]` (the storm drives a separately spawned binary) and NOT stderr (this
+/// harness never drains the server's stderr pipe — see H6).
+pub(crate) mod open_probe {
+    use std::cell::Cell;
+    use std::fs::{File, OpenOptions};
+    use std::io::Write;
+    use std::sync::atomic::{AtomicU64, Ordering};
+    use std::sync::{Mutex, OnceLock};
+    use std::time::{Instant, SystemTime, UNIX_EPOCH};
+
+    thread_local! {
+        /// Which caller this thread's opens belong to. A thread_local, NOT a thread id: tokio
+        /// reuses blocking-pool threads across callers, and the async driver opens the store
+        /// from the same pool the sweep uses — which is exactly why the thread-id split was
+        /// killed in review.
+        static CALLER: Cell<&'static str> = const { Cell::new("request") };
+    }
+
+    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
+
+    pub(crate) struct Span { id: u64, started: Instant }
+
+    /// Runs `body` with this thread's caller label set, restoring the previous value after —
+    /// so a nested open cannot leave the label stuck on a pooled thread.
+    pub(crate) fn with_caller<T>(label: &'static str, body: impl FnOnce() -> T) -> T {
+        let previous = CALLER.with(|c| c.replace(label));
+        let result = body();
+        CALLER.with(|c| c.set(previous));
+        result
+    }
+
+    /// `None` = variable unset (probe off, zero cost). A path that cannot be created PANICS
+    /// rather than degrading to silence: a wasted run must not look like a deliberate
+    /// probe-off run (G4). `create_new` also refuses an existing file, so run N can never be
+    /// appended onto run N-1 (G5).
+    fn sink() -> Option<&'static Mutex<File>> {
+        static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
+        SINK.get_or_init(|| {
+            let path = std::env::var("GRAPHHELM_OPEN_PROBE").ok()?;
+            let file = OpenOptions::new()
+                .create_new(true)
+                .write(true)
+                .open(&path)
+                .unwrap_or_else(|error| {
+                    panic!("GRAPHHELM_OPEN_PROBE could not be created at {path}: {error} \
+                            (an existing file is refused on purpose — give a fresh path per run)")
+                });
+            Some(Mutex::new(file))
+        })
+        .as_ref()
+    }
+
+    /// Epoch microseconds, NOT a per-process `Instant`: the storm phase is bounded by
+    /// interleaving server-pid and CLI-pid rows, which private clock origins cannot do (G2).
+    fn epoch_micros() -> u128 {
+        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros())
+    }
+
+    fn row(text: &str) {
+        if let Some(sink) = sink()
+            && let Ok(mut file) = sink.lock()
+        {
+            let _ = file.write_all(text.as_bytes());
+        }
+    }
+
+    /// A begin row is emitted BEFORE the open. An open parked forever on the store's blocking,
+    /// timeout-free `lock_exclusive` would otherwise write nothing at all — blind at exactly
+    /// the point H1 lives. A begin with no matching end IS the evidence (G1).
+    pub(crate) fn begin() -> Option<Span> {
+        sink()?;
+        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
+        row(&format!(
+            "open-begin id={id} t={} pid={} thread={:?} caller={}\n",
+            epoch_micros(),
+            std::process::id(),
+            std::thread::current().id(),
+            CALLER.with(Cell::get)
+        ));
+        Some(Span { id, started: Instant::now() })
+    }
+
+    /// Records the error KIND, never a bool (G8).
+    pub(crate) fn end(span: Option<Span>, error: Option<&graphhelm_events::EventRepositoryError>) {
+        let Some(span) = span else { return };
+        row(&format!(
+            "open-end id={} t={} elapsed_us={} caller={} result={}\n",
+            span.id,
+            epoch_micros(),
+            span.started.elapsed().as_micros(),
+            CALLER.with(Cell::get),
+            error.map_or_else(|| "ok".to_owned(), |error| format!("{error:?}"))
+        ));
+    }
+}

@@ apps/cli/src/commands/serve/wake.rs  (both blocking phases)
@@ ANCHORS RE-GREPPED AT d10916b: phase 1 is now wake.rs:90 (was :84), phase 3 is wake.rs:147
@@ (was :140). #74 added +561 lines to this file — do NOT apply by the old line numbers.
     let due = tokio::task::spawn_blocking(move || -> Option<Vec<DueLease>> {
+        crate::commands::open_probe::with_caller("sweep", || {
         let store = crate::commands::event_store(&events_read).ok()?;
         …
+        })
     })
@@ (and the phase-3 recorder at wake.rs:140)
     let _ = tokio::task::spawn_blocking(move || {
-        record_consumptions(&events, &execution, &consumptions);
+        crate::commands::open_probe::with_caller("sweep", || {
+            record_consumptions(&events, &execution, &consumptions);
+        });
     })

@@ apps/cli/src/commands/serve/routes.rs:819  (the driver's own opens — the tag the kill demands)
-    let store_open: StoreOpen = Arc::new(move || event_store(&events));
+    let store_open: StoreOpen =
+        Arc::new(move || crate::commands::open_probe::with_caller("driver", || event_store(&events)));
```

**Every feature above is a repair derived from this lane's own scoring rules** (M's observation,
verified by me feature by feature — it is a better argument for rung A than the slot argument
was):

| Feature | The rule it repairs |
|---|---|
| Three-way tag **including `driver`** | flattening — widen the value; a two-way tag merges driver into `request`, the same defect that killed P7 |
| `begin`/`end` rows | ambiguous absence (G1) — a missing row could mean "no open" or "open never returned"; two rows make absence self-evident |
| Epoch microseconds | the denominator repair — private clock origins cannot bound a phase across pids |
| Error **kind**, not `ok: bool` | flattening again, one field over |
| Fail-loud sink refusing an existing file | a wasted run must not be indistinguishable from a deliberate probe-off run, nor run N from run N-1 |
| Known-count controls | the instrument was the one unguarded thing carrying the lane's conclusions |

**The caveat that keeps this from being a comfort claim:** an instrument assembled entirely from
repairs to errors we have *already made* is protected against exactly the failure modes we have
already seen. It is no evidence at all about the ones we have not. "Built from our own rules"
argues that rung A will not repeat this lane's history; it argues nothing about whether rung A is
right. The known-count controls are the only part that tests the instrument against reality
rather than against our past.

**The known-count control (sabotage item 9) — the instrument's own guard.** Rung A carries this
lane's conclusions and was, until now, the one unguarded thing in it. Two controls, both of which
must be observed failing when the tag or the counting is broken:

1. **Exactly-one:** a single `GET /v1/executions/{id}` against a served store must produce
   **exactly one** `open-begin`/`open-end` pair with `caller=request`, and **zero** rows with any
   other tag. Sabotage: drop the `with_caller` wrapper at routes.rs:819 and the control must fail
   by seeing a driver open tagged `request`.
2. **Forced-sweep:** one armed lease plus one successful mutation must produce at least one
   `caller=sweep` row. Sabotage: remove the `sweep` wrapper in wake.rs and the control must fail.

Without both observed failing, the tag is an assertion about itself.

**Every control must name where its expected value comes from** (L's closing test, applied here
to my own): a control whose expected value is derived from the same reasoning that built the
probe cannot test the probe — it can only agree with it. This is P4's question pointed at the
controls rather than at a row: *could this design have produced the other answer?*

| Control | Expected value comes from | Independent of the probe? |
|---|---|---|
| Exactly-one | reading `status.rs:24` — `status::execute` opens once | **Yes** — source code, not probe rows |
| Forced-sweep | reading wake.rs — sweep spawns on the `Ok` arm only | **Yes** — source code |
| `caller=sweep` rows = N(200 mutations) | **client-side HTTP status codes** | **Yes, and strongest** — a different measuring apparatus entirely |
| Storm-phase denominator | first/last `caller=request` row of the server pid | **NO — probe-derived, self-asserted** |

**The last row is the problem, and it has a fix I had not seen.** The storm-phase denominator is
computed from the probe's own rows, so if the probe systematically missed opens the phase would
be wrong and nothing in the probe's data could reveal it — every share computed against it would
be confidently mis-scaled. **Patch 1 supplies the independent clock.** Its client-side timestamps
record when requests were actually issued and answered, from a different apparatus than the
server-side probe: the first client request must precede the first server open row, and the last
client response must follow the last one. A disagreement means the probe is dropping rows.

**This partially reverses my own argument against Patch 1.** I had reasoned it down to "settle D2
alone" after the run-unit bound emptied H4's corner. That undercounted it: Patch 1 is also the
only independent source that makes the storm-phase denominator *checkable* rather than
self-asserted — and that denominator is what P6 and C1's selector are both scored against. So the
two instruments should run **together**, not merely if machine time allows.

**Wall-clock stamps are ONE requirement across BOTH patches, not two notes.** Both originally used
a per-process `Instant` offset, which makes the two files incomparable and the whole cross-check
unevaluable — L raised it as G2's mirror image at the same moment I found it in my own diff. Both
now stamp `SystemTime` epoch microseconds. Carried as a single requirement deliberately: written
as two separate notes, one patch gets applied without the other and the check silently degrades
to unevaluable rather than failing loudly.

**And the cross-check needs TWO tests, because edges and drops fail differently** (L's Gap 2,
accepted — I had only drafted the first):

- **Edges bound the SPAN.** First client request must precede the first server open row; last
  client response must follow the last. This catches a truncated or shifted phase.
- **Counts detect DROPS.** A systematic mid-run drop leaves both edges perfectly intact and
  passes the span test while understating every share computed against it. Only counts from the
  independent apparatus catch it: client request counts by outcome against server row counts by
  outcome — the P5 accounting identity doing a second job.

Neither substitutes for the other. **Caveat on the count test:** the identity holds only on
passing runs (a timed-out request has no client status code while the server may have done the
work), so drop detection is clean on passing runs and weakens to a bound on failing ones.

**And that caveat is worse than "passing-run-only" — it is REGIME-LIMITED** (L, and it is the
dimension-earned-vs-spent test aimed at my own honesty note). Completeness certified on a passing
run certifies the probe under *lighter load than the run we care about*. Every plausible drop
mechanism is load-dependent — the sink `Mutex`, the append syscall under contention, a process
killed mid-write — so the passing run cannot certify the failing one **even in principle**,
because the conditions that would produce drops are exactly the conditions that were absent.
State it as "completeness established under a load regime that excludes the suspected drop
mechanisms", or a later reader spends it as general completeness.

**A THIRD apparatus recovers part of it, and it costs nothing extra.** The preserved events
directory survives the failing run by construction — that is what the `catch_unwind` preservation
is for. Committed events are durable, independent of client status codes *and* independent of
probe rows. Counting storm-attributed decision events (`agent-storm-*` actors) gives:

- `caller=request` rows **≥ 3 ×** committed decision events (a fresh success costs three), so a
  shortfall on a *failing* run is direct drop evidence — the regime where the client-code test is
  blind;
- `caller=sweep` rows **≤** committed decision events, with equality expected on passing runs.

**That second line makes leak 3 measurable, which neither of us had — with one confound, since
resolved.** My first form said `committed − sweep rows` **is** the count of unscheduled sweeps.
L caught that it is a **ceiling**, not an identity: `resume` commits its decision at
`execute_prepared` (routes.rs:677-684) and only *then* awaits `drive`, so a drive that returns
`Err` leaves a durable decision event while the mutation takes the `Err` arm and **never reaches
the sweep spawn** (serve/mod.rs:806 is `Ok`-arm only). So

    committed − sweep rows = (unscheduled sweeps) + (committed-then-failed mutations)

**The second term is already measured at zero, by an assertion that was there all along.** A
failed drive maps to `GHCLI016_DRIVER_FAILURE`, which `respond_failure` sends as **500** — and
`assert_storm_status` (api_http.rs:1517-1523) panics at **:1518** on any status outside
{200, 400, 409}. H's panic-line inventory across all 13 runs is `{:464, :221, :1468}` **only**;
`:1518` never fired. So no storm mutation has ever committed and then failed its drive in this
test's measured history, the confound term is zero, and the ceiling collapses to the identity —
on evidence collected before the question was asked.

L proposed measuring this rather than assuming it, which was right; it turns out the storm's own
assertion 1 has been measuring it continuously. Carried forward as a standing condition, not a
closed question: **if `:1518` ever appears in a future run, the leak-3 identity reverts to a
ceiling** and the two terms must be separated before the number is used.

**Scope of that resolution — narrower than I stated, and it is the regime point a third time.**
The guard's own doc comment (api_http.rs:1514-1516) says a hung request "would already have
panicked inside the request helper via its 5s timeout, **before this function is even reached**".
So a timed-out request never reaches the status assertion, and the zero-500 evidence covers
**only requests that returned**. On a failing run the killed requests are exactly the ones whose
server-side outcome is unknown: a committed-then-failed resume whose 500 was lost to the timeout
leaves no `:1518` and no trace in the inventory. So the identity is clean on passing runs and
reverts to a ceiling on failing ones — **the same runs where leak 3 was worth measuring.** My
resolution restored the identity precisely in the regime that did not need it.

**The standing check this is the third instance of** (M's framing, and three occurrences under
three authors makes it a rule rather than three catches): **every "zero X" claim must name the
population in which X was observable.** If the hypothesis at stake lives outside that population,
the zero is silence, not evidence. The three: *zero `:443` connect panics* — true of the
attributed 16, while H4 lived in the 4 laundered; *zero `:1518`* — true of requests whose status
returned, while the committed-then-failed case hides among the 20 that timed out; *completeness
verified* — true under a load regime that excludes the drop mechanisms it was meant to rule out.

**One precision on "two independent derivations reached the same instrument"** (M, accepted, and
it guards a real misreading of something I wrote): that is the closest thing to corroboration
available before a run — but it corroborates an **instrument choice**, never a **result**. Nobody
may later read "L and D derived it independently" as evidence about what the instrument will
show.

**And here my own claim-utility rule says stop.** What does leak 3's magnitude hold up? Only the
*sweep-based* form of D-P10. **D-P10-ALT — commits-past-timeout = `committed − N(200 observed)` —
involves no sweep rows at all**, so it is untouched by leak 3 in any regime. Leak 3's magnitude is
therefore load-bearing **only on the fallback path** where directory preservation fails to land;
on the primary path it is decoration. Further rounds tightening it would be exactly the failure
the claim-utility rule names: correct work on a claim with no consumer. Recorded, bounded, and
deliberately left here. And it makes D-P10 cleaner still:
commits-past-timeout can be computed as `committed − N(200 observed)` **entirely from the store
and client codes, with the sweep out of the calculation altogether** — which is what D-P10-ALT
already specified, now arriving from a second direction.

Three apparatus, and only the third works where the flake lives.

**The shared-upstream problem, and where each probe failure mode is actually caught.** M found
the same extension in their own file: P8, the denominator span and the G2 stamps are not three
independent conditions — all three are fed by the probe, and L's G1 (an open parked forever on
the timeout-free lock writes no row) would degrade all three at once and silently. That is right,
and it is worth mapping which mitigation catches which probe failure, because "shared upstream"
alone understates what the design already does:

| Probe failure mode | Detected by | Where |
|---|---|---|
| An open **parks** and never returns (G1) | the dangling `open-begin` with no `open-end` | inside the probe — this is why begin/end pairing is load-bearing beyond H1 |
| Rows **dropped** mid-run | cross-apparatus counts — client codes (passing runs), committed store (failing runs) | outside the probe entirely |
| Sink **unusable** | fail-loud panic on `create_new` | at startup, before any data exists |

So the three conditions do share one upstream, and **each of that upstream's failure modes has a
detector that does not depend on the conditions it would corrupt.** That is the honest
architectural claim — stronger than "independent" (false) and more useful than "shared upstream"
(true but defeatist).

**Rule 10 applied backwards here too: they have DIFFERENT failure modes, not DISJOINT ones.** I
had been calling them disjoint. They are not: all three are produced downstream of the same
harness and the same server process, so a failure early enough — the server never binding, the
storm never starting — blinds all three at once. What is genuinely different is *how each fails
once the run is under way*: the probe fails by dropping rows, the client codes fail by requests
timing out before a status exists, the store fails only if the preservation does not land.
"Different failure modes, one shared upstream" is the accurate claim, and it is still enough for
the cross-checks to work — but "disjoint" claimed an independence the arrangement does not have.

### Rung B — the internal split, ONLY if rung A says opens dominate

Crosses into C's study file; needs the orchestrator's assignment and C's coordination first, so
it is specified rather than diffed:

- **Lock wait** — time `lock.lock_exclusive()` alone (`local.rs:2146`), not the whole of
  `initialize_root_locked`, which also does `classify_layout` and would blend the two.
- **Journal fsync** — time `sync_loaded_journal` at its call site in `open_inner` (`local.rs:379`).
- **Work size** — event count from `load_state()`'s `LoadedState` (`local.rs:378`), summed over
  `batches`. This is the field that makes A's O(history) growth visible in the same data, and it
  is why rung B is worth doing even if the flake turns out to be pure lock contention.
- Same sink, same `pid`/`thread` convention as rung A, so both rungs' rows read together.

A large lock-wait share confirms H1; a large fsync share confirms H3; a large blocking-pool
share confirms H2. They are proportions of one total, not competing verdicts — which is the
whole reason this replaced three ablations.

### When rung A runs, and why the order matters

Order, confirmed directly by the orchestrator after B relayed it: **flake-3 → re-measure storm
baseline → storm fix** — and explicitly **cheap insurance, not a hard dependency**. If the
minimal storm fix caches no handles, the coupling never materializes; the re-measure is what
proves attribution either way.

Runner note: W1 was stopped by the owner. The base measurement is now H Agent's, with the
verbatim-failure-text requirement (panic `file:line`, and the `get_status` phase-laundering
caveat) baked into it. **The wait target for Step 0 is H's report, not W1's.**

That has a consequence for rung A that is worth stating before a run is spent on it. Flake-3
lives in the wake path, and rung A's most valuable single number — the sweep's share of store
opens — is a measurement *of that path*. If C's fix changes when or how often the sweep runs,
a sweep share measured beforehand expires the moment flake-3 lands. The request-path rows would
survive; the sweep rows would not.

So rung A should **not** be spent as a separate run beforehand. Its natural slot is exactly the
"re-measure storm baseline" step already in the binding order: with the probe applied, that one
run produces the baseline numbers *and* the histogram together, against the post-flake-3 code
the storm fix will actually be built on. One run, two answers, no expired data.

Step 0 (reading H's verbatim failure text) is unaffected — it costs nothing and can happen the
moment the text exists.

Approved by the orchestrator as proposed: Step 0 → rung A → decide rung B. Rung B's pen is
**deferred, not assigned**: `local.rs` stays C's until flake-3 lands, and comes to me with C's
coordination only if rung A shows open cost is large.

---

## (g) Fix candidates, written BEFORE the numbers — and tied to what would select each

Drafted while blocked on Step 0, deliberately as a menu rather than a decision. Nothing here is
chosen; each entry names the measurement that would justify it and the sabotage that would guard
it. Recording them now means the fix phase starts from candidates that were reasoned about
before anyone was invested in a result — but **no candidate may be adopted on the strength of
this section alone.** Constraint from the orchestrator throughout: minimal for the flake, the
root fix (eliminating the per-request open) is M10's, and this must not preempt it.

**C1 — move the store work off the runtime thread (`spawn_blocking` in the handlers).**
The handlers do synchronous file IO inline on a current-thread runtime, so one slow request
blocks the accept loop and every other request, `/health` included. Wrapping the command-layer
call in `spawn_blocking` frees the runtime thread without touching the store, the lock model, or
handle lifetime — and specifically **without caching a handle**, so the `operation_gate`
coupling from section (e) never arises. Files are mine (`serve/routes.rs`, `serve/mod.rs`).
*Selected by (REWRITTEN — the original selector was a descendant of the retracted contention
model and named a state that can never be observed, since the pool is busy with the driver
throughout a storm):* rung A showing `caller=request` opens strictly non-overlapping in time,
with their summed elapsed a large share of **the storm phase** — not of the run's wall clock,
which is disqualified as a denominator and would dilute the share enough to reject C1 wrongly.
That is the serialization claim itself, measured, with no reference to what the pool is doing.
*Guarded by:* sabotage 1 — put one route's store work back inline and watch the guard fail.
**But NOT guarded by `/health` alone.** L's hazard, accepted and load-bearing: C1 does not
remove the convoy, it **relocates** it. Several pool threads then contend on the same blocking,
timeout-free exclusive open lock (local.rs:2146). `/health` would go free — that is the accept
loop — while **mutation tail latency, which is what actually produces the 10060, might not move
at all.** A guard satisfied by "/health answers while a slow request is in flight" would go
green having fixed the wrong number. C1's guard must be mutation tail latency.

**C2 — multi-thread the runtime.** Smaller diff than C1 (one builder call, events/mod.rs:157),
but strictly worse as a fix: it raises the number of threads that can block rather than stopping
handlers from blocking, and leaves blocking-in-async in place for M10 to trip over. Worth naming
only so the cheap-looking option is on record as considered and rejected on merit, not missed.
*Would be selected only by:* evidence that accept-loop starvation, not response latency, is the
mechanism — i.e. Step 0 landing on `:443`, which section (e) argues is unlikely.

**C3 — stop paying for opens the request does not need.** Open #3 (`current_head` purely to
attach `headSequence`) and open #1b exist because the command layer returns no head. Removing
them is a real reduction but reaches across `execution/{signal,approve,pause,resume,cancel}.rs`
to widen return shapes — outside the serve layer, and squarely the shape of M10's work.
*Selected by:* rung A showing opens-per-request ≥ 3 AND open cost dominating. *If so:* propose
to M10 rather than absorb here, and say so explicitly rather than quietly growing the fix.

**C4 — do not sweep when nothing is armed.** 36 full replays for leases that never existed is
the clearest waste in the trace. But the cheap version keeps armed-lease state in `ServeState`,
which is wake-path design and therefore C's call, and the sweep is what the doorbell is for —
a fix here risks the milestone's own feature to speed a test. *Selected by:* rung A showing the
blocking-pool share large. *Owner:* C, proposed by me, never taken unilaterally.

**C5 — raise the client's 5s read timeout.** Not a fix, and named here only to be refused:
sabotage 6 exists precisely to stop a latency guard from being satisfied by the client's
patience. Legitimate as a *measurement* (does the run pass with 30s?), never as the change that
closes the flake.

The honest position until Step 0 and rung A report: **C1 is the only candidate that is both
minimal and independent of what the numbers say about lock-vs-fsync**, because it fixes the
serialization rather than the cost of each operation. That is an argument for its robustness,
not evidence that it is right — and if rung A shows a single open costing more than the whole
convoy, C1 would make the storm pass while leaving the real cost untouched, which is exactly the
failure mode of fixing what you measured last instead of what is slow.

---

## (h) Frozen predictions — written BEFORE any number exists

Sent to M Agent's prediction ledger and recorded here at the same time, 2026-08-19, while H's
run is still in flight and nothing has been measured by me. A selection criterion ("adopt C1 if
rung A shows X") is a decision rule, not a prediction — it cannot be wrong. These can. Numbers
are committed to deliberately so that being wrong is visible rather than reinterpretable.

**Step 0 — what H's failure text will say:**

- **P1.** The panic location is `api_http.rs:464` (read phase). Stated with its own caveat: the
  storm's op mix is 3 mutations to 1 status, so if failures fall proportionally there is roughly
  a 1-in-4 chance of landing at `:220-222` instead, where the phase is laundered. P1 is a
  prediction about the modal outcome, not a claim that `:220-222` would surprise me.
- **P2.** The OS error is 10060, not 10061. High confidence — 10061 would mean refused rather
  than timed out and would redirect the whole study.
- **P3.** The failure comes from a storm thread, not from the post-storm verify step.
- **P4.** The flake rate is near 50% (~3 in 6), matching the prior report rather than improving.

**Rung A — what the histogram will show.** These are the ones that put my mechanism map at risk;
if the counts are wrong, the map is wrong, and no amount of prose rescues it:

- **P5 — RE-AMENDED TWICE, 2026-08-19, both times before any number existed.** The original
  ("≈ 4 opens per mutation") was a floor, not an estimate. The first amendment ("exactly 3 per
  mutation") was still unclassifiable, because open count is **outcome-dependent, not
  mutation-dependent** — L's round-2 finding, verified. Restated per outcome, counting
  `caller=request` opens (serve/mod.rs:744-812):

  | Outcome | `caller=request` opens | `caller=sweep` |
  |---|---|---|
  | fresh 200 mutation | 3 (classify :746 + command + `current_head` :797) | exactly 1 (:806, Ok arm only) |
  | 409 precondition, `GHCLI005` — the storm's routine flip-flop | **2** (classify + command; `respond_failure` opens nothing) | 0 |
  | 409 sequence conflict, `GHE001` — rare race | 3 (classify + command + `conflict_with_current_head` :1025) | 0 |
  | 200 status read | 1 (`status::execute`, status.rs:24) | 0 |
  | recognized retry, `KeyState::Complete` | 2 | 0 |

  **Two corrections to L's table, both verified.** (1) The storm's routine 409s are `GHCLI005`
  precondition refusals from the pause/resume flip-flop, which fall to `respond_failure` and cost
  **2**, not the 3 an `GHE001` conflict costs. (2) **The recognized-retry row never fires in this
  storm at all:** `retry_once_on_409` retries under a *fresh* key (`{key}-retry`,
  api_http.rs:1532) and every base key is unique per thread and round, so no request ever repeats
  a key with the same body. Predicting **zero** `KeyState::Complete` occurrences is itself
  checkable.

  **Scored as an accounting identity, not a per-request count** (two outcomes cost 3 and 3, and
  nothing in probe output separates them per-request): total `caller=request` opens =
  3·N(200 mutations) + 2·N(409 precondition) + 3·N(409 conflict) + 1·N(status), and
  `caller=sweep` rows = N(200 mutations).

  **BOTH identities hold only on PASSING runs** (L, round 3 — accepted). A request killed by the
  5s timeout has no client status code while the server may have completed the mutation anyway,
  so client-side counts undercount server work by exactly the killed requests; and a panicking
  storm thread runs no further rounds, shrinking the population unevenly. On a failing run these
  are inequalities, not identities.
  **Scorable only if `caller=` exists AND per-outcome client counts are recorded.** Absent
  either, P5 is unscoreable and must be recorded as such, never as "did not fire" — the same
  posture that withdrew P7. A prediction that cannot be classified is not a weaker prediction; it
  is P7's failure mode wearing a number.
- **P6 — AMENDED 2026-08-19, pre-measurement, denominator corrected.** Summed `caller=request`
  open elapsed is **≥ 50% of the STORM PHASE**, where the phase is bounded from the probe rows
  (first-to-last server-pid `caller=request` row, less the ≤2 trailing verify reads). The
  original said "of the storm's wall duration" — a surviving descendant of the retracted T1
  arithmetic, since that wall clock contains `cli_start`, spawn, health poll and verify. Scoring
  against wall clock would dilute the share and could fail P6 spuriously. **Scoreable only
  against the storm-phase denominator, and only with G2 epoch stamps** (per-process `Instant`
  origins cannot bound a phase across pids). Still dependent on P8 per M's seal: if opens
  overlap, summing them against any denominator double-counts and P6 scores UNINFORMATIVE.
- **P7 — WITHDRAWN 2026-08-19, before any number existed. Not amended: withdrawn.** It predicted
  "blocking-pool rows are 20-30% of server-pid rows" as the sweep share. L killed the premise:
  the pool carries the async driver's opens too, so the quantity I named does not measure the
  thing I named it for, and any value it takes would be unfalsifiable rather than right or wrong.
  A replacement (`caller=sweep` rows are 20-30% of server-pid rows) is only legitimate once the
  `caller=` tag exists, and I am **not** freezing it now — the tag is unwritten, so a prediction
  about its output would be guessing at my own future code. The original conditional
  (flake-3 changing sweep frequency) still applies to any replacement.
- **P8.** Request-path open intervals do not overlap each other. A single overlap falsifies the
  single-runtime-thread serialization claim that the whole convoy argument rests on.
- **P9.** Open elapsed grows across the run as history grows: last-decile median ≥ 2× first-decile
  median. This is A's O(history) cost, predicted in my data before it is theirs to claim.

**Scoring bindings sealed by M Agent's ledger** (accepted by me, recorded here so this file and
the ledger cannot drift): P6 is *dependent* on P8 — if opens turn out to overlap, P6 scores
UNINFORMATIVE rather than confirmed, because summing overlapping intervals against wall clock
double-counts, and one histogram must never bank "P8 falsified" and "P6 confirmed" from the same
data. P1's modality cuts both ways: a `:220-222` landing neither kills nor confirms P1, and it
simultaneously **kills** my claim that Step 0 settles the phase for free. C5's refusal is a
pre-commitment, so a raised client timeout landing as the fix scores as a **breach** even though
rules are otherwise unscored. And the falsifier below gets an explicit UNINFORMATIVE cell: it is
arithmetic over two measured quantities, so if either is missing it *could not be evaluated*, and
that must never be recorded as "it did not fire".

**The falsifier I commit to in advance:** if median open cost × opens-per-request already exceeds
the per-request budget the 5s timeout allows under 8-way queueing, then serialization is not the
binding constraint and **C1 must not be adopted** even though the storm would likely pass with
it. That is the outcome that would send this lane to C3/M10 territory instead, and I would rather
name it now than discover the temptation after a green test.

---

## (j) STEP 0 RESULT — H Agent's clean baseline, and my rows scored

H ran unmodified `53d212d`, no probes, no env var. **This is the first measurement in this
study, and it is not mine.** Report: `.factory/h-agent-base-measurements.md` (main checkout).

**The phase is READ starvation.** Every failing run panicked at `api_http.rs:464:34` (1-5 hits
per run). Zero `:443` connect panics, zero `:460`/`:461` write panics, across every storm and
suite log. OS error 10060 verbatim in all failures; **10061 never appeared anywhere**. All
panics came from unnamed scoped storm threads; the verify step never panicked; the main thread
dies only at the join.

**H5 is DEAD. H4 is DISFAVOURED, NOT DEAD — corrected three times, most recently by M against
their own verdict.** I first wrote "H4 and H5 dead for zero instrumentation". L narrowed it once
(I could not use the laundered `:221` rows as queueing evidence *and* as H4-killing evidence).
M then struck the H4 half of their own scoring, and they are right:

- **H5 (port exhaustion): DEAD, by DATA, on both paths.** It predicts *codes* — 10048/10055 —
  and **the code is legible even in the laundered rows**, where every observed failure reads
  10060. `get_status` destroys the phase, not the code.
- **H4 (connect starvation): DISFAVOURED.** Its three supports are all weak and none is a
  measurement: (a) zero `:443` panics among the **attributed 16 only** — a connect failure could
  sit invisibly in the 4 laundered rows; (b) B's argument that a loopback connect failure would
  read 10061 refused, which M scored **vacuous** because no connect failure was ever captured to
  test it against; (c) no elapsed data at all, so the ~21s SYN-retry signature was never checked
  either way. Patch 1's `connect_us`/`read_us` split on the status path is what retires it.

**Reconciled form (M's, after our messages crossed and left two verdicts on the board):** H4 is
**dead on the MUTATION path** — a connect failure there panics at `:443`, and across every
captured run there are zero — **not dead on the status path**, where its remaining support is a
premise this run never tested.

**A bound that follows — and I first computed it in the wrong unit.** I wrote: 4 of 20 captured
panics are laundered, so H4 explains at most 20%. L corrected it, and the correction runs in my
own favour: **the flake's unit is the RUN, not the panic.** A run fails if any thread panics, and
a verdict is about which runs failed and why. Checked against H's table: the four `:221` hits sit
inside three runs (iso 3, suite 1, suite 3) that each *also* carried at least one `:464`, and the
other three failing runs carry none at all. So **even if Patch 1 shows every laundered hit was a
connect death, the number of runs explained by H4 is ZERO** — it could at best be a co-occurring
extra death inside runs already killed on the read path.

**Therefore Patch 1's purpose is to settle D2, and it should be described that way** rather than
carried as the phase-decider it was originally sold as. The headline was already decided by the
run-unit bound before the patch exists. But D2 is untouched by that argument — my inference that
a one-open request exceeding 5s proves queueing is about a *request*, not a run, so it evaporates
entirely if those four were connect failures. Net effect on ordering: **rung A gains on Patch 1
by more than I claimed.**

**⚠ And the read-phase majority is a statement about PHASE, not about MECHANISM.** Read-phase
death is equally consistent with H1, H2 and H3 — every one of them predicts it. So "the
read-phase finding stands regardless" must never be read as support for H1 over H2/H3. **H1 vs
H2 vs H3 remains entirely open.** These two sentences stay adjacent on purpose.

**M's sharpening, and it generalises beyond this lane:** the flattening at `get_status` destroyed
the evidence for exactly the hypothesis that asked a *phase* question, while leaving untouched
the one that asked a *code* question. So when you discover a flattened boundary, **re-audit the
verdicts that leaned on the flattened dimension — not all of them.** That is what tells you
which conclusions to reopen without reopening everything.

**Consequence: Patch 1 is RE-PROMOTED, by my own Step 0 rule.** I wrote that Patch 1 becomes the
phase-deciding step if the failure text names `get_status`. It did — **4 occurrences across 3
runs** (L said 3; H's table is iso 3 ×1, suite 1 ×1, suite 3 ×2). So `connect_us` vs `read_us` on
that path is what settles the status-path phase and closes H4 by measurement instead of by
argument. Patch 1 is no longer the demoted step, and D2 below is conditional on its result.

**My rows, scored:**

| Row | Result |
|---|---|
| P1 — panic at `:464` is the modal outcome | **CONFIRMED**, and stronger than predicted: `:464` in *every* failing run |
| P2 — 10060 not 10061 | **CONFIRMED** verbatim; no 10061 anywhere |
| P3 — storm thread, not verify step | **CONFIRMED** |
| P4 — flake rate near 50% | **CONFIRMED, loosely**: 4/10 isolated (40%), 2/3 in-suite (67%), 6/13 combined (46%) |
| "Step 0 settles the phase for free" | **SURVIVES** — see the mixed-capture note below |

**M's rulings, including two that are worse for me than my own framing** (M read H's report
directly rather than take my summary — the correct posture, and it is how they caught these):

- **`DT-1` scores NOT-YET, not confirmed-by-proxy.** My decision-table row required three
  conjuncts: phase read, kind TimedOut, **and `read≈5000ms` with `connect` small**. The base run
  supplies the first two from the panic site and **no elapsed times at all**, because Patch 1
  never ran. So the "starved of a response for ~5s" *magnitude* remains unmeasured, and H1's
  selection over H4 rests on the panic sites and the absent 10061 — not on DT-1. Net standing:
  **H5 dead, H4 disfavoured but live**, H1 preferred to H4, and **H1 vs H2 vs H3 entirely open**.
  That is one dead hypothesis and one demoted one, not a mechanism.
- **`T2` scores UNINFORMATIVE-VACUOUS.** Its confirming cell was "any captured connect-side
  failure carries 10061" — and there were **zero** connect-phase failures, so the claim was never
  exercised. "10061 appears nowhere" confirms P2 (the error *is* 10060), a different row. This has
  a consequence for the paragraph above: **H4's disfavouring** on the status path rests on a
  premise the run never tested, so it is accepted-by-argument and stays that way until Patch 1
  measures it. This is why H4 is demoted rather than dead.
- **`P4` is looser than "confirmed-loose".** At N=10 the 95% interval around 4/10 spans roughly
  12-74%, so a true rate materially away from 50% would probably not have been distinguishable —
  **this row could not have been killed by this N.** A confirmation from an instrument that could
  not disconfirm is worth very little. The general lesson, which I should have applied when
  freezing it: **a rate prediction must name the N that would falsify it**, or it is nearly free.
- **`D-TRAP-1` stands on its mechanism, not on a numeric gap** — same correction I reached
  independently from H's table. Cite the truncation mechanism; the durations overlap, so the
  first counterexample run would otherwise appear to refute a sound rule.

**The mixed capture, and why I do not get to score it as a clean win.** The laundered site
`:221:33` *did* fire (once in isolated run 3, once in suite run 1, twice in suite run 3) — the
outcome M's seal said would kill the free-read row. It survives only because `:221` **never
appeared alone**: every run carrying it also carried at least one clean `:464`, so no run's
verdict ever depended on a laundered hit. That is a weaker result than "the caveat never fired",
and it should be recorded as such. M rules on the scoring, not me.

**New finding out of the `:221` hits, and it is not in my favour.** `:221` is `get_status`'s
panic site, so **status reads timed out too** — and a status read costs exactly ONE store open
against a mutation's three or four. A one-open request exceeding 5s cannot be explained by its
own cost; it waited behind other work. That is direct support for queueing over per-operation
expense, i.e. for H1 over H3 — but it is *support*, not proof, because `:221`'s phase is
formally unknown (it could be connect, though zero `:443` hits and zero 10061 make that
unlikely). Rung A still decides.

**⚠ A trap in H's numbers that threatens sabotage item 8 (attribution) — with my own overclaim
corrected.** I first wrote "failing runs are FASTER than passing ones (19-26s vs 23-31s)", mixing
isolated and suite runs. Reading H's per-run table directly: isolated failures are 22/21/24/26s
(mean 23.3) against passes 23/31/29/24/23/24s (mean 25.7). **The ranges overlap heavily** — a 26s
failure sits above three 23-24s passes. So the correct statement is stronger for the trap and
weaker for me: **run duration cannot distinguish a passing run from a failing one in either
direction**, and it is therefore unusable for attribution, not merely biased. The reason is
structural —
a panic unwinds, the scope joins early, and the test ends before the remaining rounds run. So
**wall-clock duration is not a load proxy and must never be used to compare fixed against
unfixed**: a successful fix will *increase* total wall time by letting every round complete.
Any attribution built on "the run got faster" would score a real fix as a regression, and could
score a *regression* as a fix. Attribution must use the storm-phase open accounting and mutation
tail latency, never run duration. This also retroactively vindicates disqualifying the wall
clock as rung A's denominator — for a second, independent reason nobody had named.

### The commit-past-timeout instrument, and why it must be one-directional

L's round-3 proposal: on a failing run, `(caller=sweep rows − N(200 observed))` counts mutations
that **committed server-side while the client saw a timeout** — direct evidence for H1 (accepted,
committed, response late) and against a hang. Accepted in substance; the sweep spawns from
exactly one site in the `Ok` arm (serve/mod.rs:806) and, with no lease armed, opens once and
returns at wake.rs:126.

**But the test cannot be symmetric, because L's own leak 3 confounds it in the opposite
direction.** `tokio::spawn` queues each sweep behind the runtime thread, which during the storm
is busy with blocking store IO. On a *failing* run the scope re-panics at join, `verify` never
runs, and `ServerGuard::drop` kills the child — so queued sweeps that never got scheduled leave
**no row at all**. Leak 1 inflates the discrepancy; leak 3 deflates it. On the only runs that
matter, both are live at once.

So the honest form is **one-directional**: a discrepancy **> 0 confirms** commits past timeout
(leak 3 can only subtract, so a positive residue cannot be manufactured by it), while a
discrepancy of **0 is UNINFORMATIVE** — it does not support a hang hypothesis and does not
refute H1, because leak 3 may simply have eaten the evidence.

**A leak-3-immune version exists and is better: ask the store.** Committed events are durable and
care about neither client status codes nor unscheduled sweeps. After a failing run, replay the
stream and compare committed decision events against client-observed 200s.

**My proposed mechanism for it was unimplementable — L, round 4, accepted.** "Preserve the events
directory via `into_path()` behind a flag" cannot work: the panic propagates out of `run_storm`
at the scope join, so no line after it ever executes, and `TempDir::drop` deletes the tree while
unwinding. There is no call site left to reach `into_path` from. Two ways out:

- **(a) preferred:** wrap `run_storm` in `std::panic::catch_unwind` (with `AssertUnwindSafe`),
  preserve on `Err`, then `resume_unwind` — this keeps the original panic and its `file:line`
  intact, which Step 0's whole method depends on;
- **(b)** a Drop-order guard checking `std::thread::panicking()` and leaking the path instead of
  deleting. Smaller conceptually but fights `TempDir` rather than working with it.

Either way the preserved path must be **printed**, and preserved on **passing runs too** —
without a passing-run baseline there is nothing to compare the failing count against.

---

## (k) H1 vs H2 vs H3 — the discrimination plan, keyed to H's actual numbers

Step 0 settled the *phase* (read starvation), killed H5 and demoted H4 to disfavoured-but-live
(section (j) — it is retired only by Patch 1's status-path split). It says nothing about *why* the
server is slow. All three survivors predict a read-phase timeout, so they are separated by
proportions and timing structure, never by failure text. Everything below runs in the lane's
position-4 slot (post-flake-3 re-baseline), Patch 1 and rung A together.

**D1 — the decisive inequality: max single-open elapsed against the 5s budget.** H3 says the cost
is *inside* an operation; H1 says it is *waiting for other operations*. Rung A measures both
sides. If the largest observed open is far below 5s while requests still exceed 5s, the wait
cannot live in any single operation and queueing is proven, with depth ≈ (request wait ÷ median
open). If a single open approaches seconds, H3 is binding and **C1 must not be adopted** — this
is where the pre-committed falsifier fires. This is the one measurement that can retire a
hypothesis outright.

**D2 — free corroboration already in H's data: `:221` fired 4 times.** `:221` is `get_status`, a
**one-open** request. Under H3 alone it can only time out if that single open takes 5s, which D1
confirms or refutes directly. Under H1 it is unremarkable. H's numbers already tilt toward H1
before rung A runs; D1 turns the tilt into a verdict.

**D3 — H2's share, bounded rather than argued.** `caller=sweep` rows ÷ all rows, and sweep
elapsed ÷ total elapsed. A ~25% share bounds the sweep's contribution at ~25%: worth removing,
and *not sufficient alone* to explain a 5s overrun. H2 is an amplifier hypothesis and gets scored
as one.

**D4 — growth, from P9.** Last-decile against first-decile open elapsed. ≥2× makes the O(history)
cost real; combined with D1 it says whether that growth crosses the 5s line or merely rides
along.

**D5 — timing structure of multi-hit runs, from Patch 1 alone.** H's per-run failure counts vary
1 to 5 (iso 6 had five; iso 7 and 10 had one). Under H1 the failures in a five-hit run should
**cluster in time** — one stall kills everything queued behind it — while independent per-request
costs would scatter them. Patch 1's timestamps test this without rung A.

**D6 — DROPPED. The lead does not survive reading the test** (checked at wake_http.rs:947-1007,
before position 5 as promised). `concurrent_sweeps_never_double_consume_a_lease` runs longer than
the storm (25-37s vs 21-31s) and passes 10/10, but its length is **not** concurrent load: it is
15 sequential rounds, each with a `Barrier::new(2)` — **two** racing clients, not eight — plus a
600ms sleep to let the fire-and-forget sweeps drain and a fresh `graphhelm` subprocess for the
per-round status check. Longer wall clock, far lighter contention, with drain windows the storm
never gets. The contrast says nothing about H2 at 8-way concurrency and is dropped rather than
kept as atmosphere.

**One genuinely useful byproduct, in the opposite direction from what I was fishing for.** That
test **arms leases** (`arm_lease` per round), so its sweeps run the *expensive* path — ring, then
`record_consumptions` appending under the exclusive lock (wake.rs phases 2 and 3). The storm arms
nothing, so its sweeps take the **cheap** path: one open, `due` empty, early return at
wake.rs:126, phases 2 and 3 never reached. That confirms the storm's sweep cost is bounded to
**one extra open per successful mutation**, which is exactly what the P5 accounting assumes — so
the assumption is now read off the code rather than inherited from my own model. It also caps
H2's maximum effect: whatever the sweep costs in the storm, it is one open, not a ring and an
append.

---

## (i) L Agent's cold review: verdicts, and what it cost me

L read the report cold at `53d212d` and landed one kill I did not see coming. Everything below
was re-verified against code by me before being accepted — the same standard I held B to.

| L's finding | Verdict |
|---|---|
| Blocking-pool rows ≠ sweep; the driver opens from the same pool | **ACCEPTED — kill confirmed.** |
| H2's *replacement metric* inherited the retracted premise | **ACCEPTED.** The corpse was clean; its heir was not. |
| "Patch 2 is redundant" is a descendant and is wrong | **ACCEPTED.** Patch 2 restored as fallback. |
| C1's argument is clean, but C1's *selector* is a descendant | **ACCEPTED.** Selector rewritten. |
| C1 relocates the convoy; `/health` guard would go green wrongly | **ACCEPTED — hazard nobody had written down.** |
| Probe gaps G1-G6, G8 | **ACCEPTED**, all folded into rung A. |
| G7 (verify-step cost invisible) | **PARTIALLY REFUTED** — see below. |
| Patch 1 borrow risk, output routing, line-number shift | **ACCEPTED.** |

**The kill, verified myself.** `drive_is_viable_for` (routes.rs:765-771) is `runtime.is_some() ||
all nodes classify` — so fixture mode, which the storm runs, still reaches the async drive via
the `None` branch at routes.rs:858-861. `drive` builds `store_open` at routes.rs:819 and hands it
to `drive_to_quiescence_async` at :871. Inside, **every** store open sits in `spawn_blocking`
(driver.rs:220, :274, :308, calling `store_open()` at :221, :275, :309), and the type's own doc
at driver.rs:191-194 says so outright. The storm's rotation issues up to 12 resumes, each
entitled to a drive. So blocking-pool rows are driver + sweep mixed, and my sweep share would
have read high by exactly the driver's fan-out. **P7 is withdrawn, P5 amended** — both before any
number existed.

**Why this one stings, recorded deliberately.** My retraction of "the sweep contends for the
runtime thread" was complete *as prose*. But killing it forced the sweep's cost onto the pool
side, and I re-aimed the decider to match — inheriting `pool == sweep` in the replacement without
ever testing it. The corpse was buried and its heir carried the defect. **A retraction is not
finished when the sentence is struck; it is finished when everything the retraction produced has
been re-derived.** That is the general lesson here and it is worth more than the finding.

**G7, partially refuted.** The direct `LocalEventRepository::open` L cites is `strand_running`
(api_http.rs:2536), a helper the storm test never calls. The storm's verify step runs `cli()`
subprocesses, which go through `commands::event_store` and *are* recorded under their own pids.
The general caution — test-process direct opens are invisible to a probe living in the CLI crate
— is correct and worth keeping; it just does not hole this test's accounting.

**Three sabotage items added, extending section (c):**

7. **Guard realism.** Point any latency guard at an EMPTY store. If it still passes, it is
   passing on triviality — the same defect 53d212d's own first guard had, where a workload
   dominated by something the change could not affect made the ratio immovable.
8. **Attribution.** The fix must move a *measured* number: with/base, N runs, stated scope, same
   probe build, against the post-flake-3 baseline. "The test is green now" is not attribution.
9. **Sabotage the instrument.** My list guarded the product and left rung A — which carries this
   lane's conclusions — completely unguarded, while it was in fact mis-attributing. A known-count
   control (one status request must produce exactly one `caller=request` row) and a forced-sweep
   control must fail when the tag or the counting is broken. **L had to point this out, which is
   itself the evidence that an unguarded instrument is easy to miss from the inside.**
