# Rung A — server-side open probe (D Agent)

**NAMED BASE: `ef51193`** (tip of `origin/issue-m09-arming-the-alarm`, *not* of `main`).
Anchors re-grepped in that tree on 2026-08-19. **MEASUREMENT ONLY — revert before committing.**

> **Re-derived from `d10916b` to `ef51193` (#72).** #72 was described as test-only, but it
> **does** touch production `apps/cli/src/commands/serve/wake.rs` (+17): a
> `test_only_phase3_delay()` call inserted **inside Edit 3's anchor block**, plus the function
> itself. Verified by `git diff d10916b ef51193 -- apps/cli/src/commands/serve/wake.rs`.
> **Edit 3 below is updated accordingly** — the old anchor text no longer matches and would have
> failed to apply. Edit 2 (phase 1) is textually unchanged, still at wake.rs:90-91. The
> early-return `if due.is_empty()` is still present at wake.rs:131, so the storm's one-open
> cheap path is re-confirmed at this tip.

**Why this is an edit recipe and not `git apply` input:** hunk headers need exact line counts,
and I cannot run tooling to verify them. Every edit below is anchored on **exact existing text**,
so it survives line drift and fails loudly (no match) rather than silently applying in the wrong
place. Match the text, not the line number; line numbers are given only to help you find it.

---

## Edit 1 — `apps/cli/src/commands/mod.rs` (~line 281)

**Find:**

```rust
pub(super) fn event_store(
    path: &std::path::Path,
) -> Result<LocalEventRepository, EventRepositoryError> {
    LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds))
}
```

**Replace with:**

```rust
pub(super) fn event_store(
    path: &std::path::Path,
) -> Result<LocalEventRepository, EventRepositoryError> {
    let span = open_probe::begin();
    let opened = LocalEventRepository::open(path, Arc::new(SystemClock), Arc::new(UuidIds));
    open_probe::end(span, opened.as_ref().err().map(|error| format!("{error:?}")));
    opened
}

/// MEASUREMENT ONLY — revert before committing. One begin row and one end row per store open,
/// to the file named by `GRAPHHELM_OPEN_PROBE`; silent no-op when that variable is unset.
/// Deliberately NOT `#[cfg(test)]` (the storm drives a separately spawned binary, built without
/// the test cfg) and deliberately NOT stderr (this harness pipes the server's stderr and never
/// drains it, so writing there could block the server and manufacture the hang under study).
pub(crate) mod open_probe {
    use std::cell::Cell;
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    thread_local! {
        /// Which caller this thread's opens belong to. A thread_local, NOT a thread id: tokio
        /// reuses blocking-pool threads across callers, and the async driver opens the store
        /// from the same pool the sweep uses — which is why a thread-id split mis-attributes.
        static CALLER: Cell<&'static str> = const { Cell::new("request") };
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct Span {
        id: u64,
        started: Instant,
    }

    /// Runs `body` with this thread's caller label set, restoring the previous value after, so a
    /// nested open cannot leave the label stuck on a pooled thread.
    pub(crate) fn with_caller<T>(label: &'static str, body: impl FnOnce() -> T) -> T {
        let previous = CALLER.with(|c| c.replace(label));
        let result = body();
        CALLER.with(|c| c.set(previous));
        result
    }

    /// `None` = variable unset (probe off, zero cost). A path that cannot be created PANICS
    /// rather than degrading to silence: a wasted run must not look like a deliberate probe-off
    /// run. `create_new` also refuses an existing file, so run N cannot append onto run N-1.
    ///
    /// **ONE FILE PER PID — the env var names a BASE path and each process appends its own pid.**
    /// FIXED AFTER IT KILLED A RUN: several processes inherit this variable under one run (the
    /// server, `cli_start`, the replay subprocesses), so a single shared path made `create_new`
    /// refuse every process after the first — `cli_start` created the file, the server panicked
    /// on it, and all ten storm runs died in under a second with "malformed HTTP response" on
    /// every thread. Per-pid files keep BOTH properties (fresh-file enforcement AND multi-process
    /// capture) and make "rows separate by pid" structural instead of a parsing step. Join for
    /// analysis with a glob over `{base}.*`.
    fn sink() -> Option<&'static Mutex<File>> {
        static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
        SINK.get_or_init(|| {
            let base = std::env::var("GRAPHHELM_OPEN_PROBE").ok()?;
            let path = format!("{base}.{}", std::process::id());
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .unwrap_or_else(|error| {
                    panic!(
                        "GRAPHHELM_OPEN_PROBE could not be created at {path}: {error} \
                         (an existing file is refused on purpose — give a fresh BASE path per run)"
                    )
                });
            Some(Mutex::new(file))
        })
        .as_ref()
    }

    /// Epoch microseconds, NOT a per-process `Instant`. The storm phase is bounded by
    /// interleaving server-pid and CLI-pid rows, and validated against the client-side clock in
    /// Patch 1 — private clock origins make both impossible.
    fn epoch_micros() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_micros())
    }

    fn row(text: &str) {
        if let Some(sink) = sink()
            && let Ok(mut file) = sink.lock()
        {
            let _ = file.write_all(text.as_bytes());
        }
    }

    /// The begin row is emitted BEFORE the open. An open parked forever on the store's blocking,
    /// timeout-free `lock_exclusive` would otherwise write nothing at all — blind at exactly the
    /// point the leading hypothesis lives. A begin with no matching end IS the evidence.
    pub(crate) fn begin() -> Option<Span> {
        sink()?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        row(&format!(
            "open-begin id={id} t={} pid={} thread={:?} caller={}\n",
            epoch_micros(),
            std::process::id(),
            std::thread::current().id(),
            CALLER.with(Cell::get)
        ));
        Some(Span {
            id,
            started: Instant::now(),
        })
    }

    /// Records the error KIND text, never a bool.
    pub(crate) fn end(span: Option<Span>, error: Option<String>) {
        let Some(span) = span else { return };
        row(&format!(
            "open-end id={} t={} elapsed_us={} caller={} result={}\n",
            span.id,
            epoch_micros(),
            span.started.elapsed().as_micros(),
            CALLER.with(Cell::get),
            error.as_deref().unwrap_or("ok")
        ));
    }
}
```

> If the `let ... && let ...` chain in `row` is rejected by the toolchain, split it into a nested
> `if let`. Nothing else depends on that form.

---

## Edit 2 — `apps/cli/src/commands/serve/wake.rs`, sweep phase 1 (**now ~line 90**, was 84)

**Find** (the opening of the phase-1 closure):

```rust
    let due = tokio::task::spawn_blocking(move || -> Option<Vec<DueLease>> {
        let store = crate::commands::event_store(&events_read).ok()?;
```

**Replace with:**

```rust
    let due = tokio::task::spawn_blocking(move || -> Option<Vec<DueLease>> {
        crate::commands::open_probe::with_caller("sweep", || {
        let store = crate::commands::event_store(&events_read).ok()?;
```

…and close the added closure at the **end of that same `spawn_blocking` body**, immediately
before its `})`:

**Find:**

```rust
        Some(due)
    })
    .await
```

**Replace with:**

```rust
        Some(due)
        })
    })
    .await
```

## Edit 3 — same file, sweep phase 3 (**now ~line 147**; `#72` inserted a line inside it)

**Find** (note the `test_only_phase3_delay();` line — added by `#72`; if your tree lacks it you
are **not** on `ef51193`, stop and re-check the base):

```rust
    let _ = tokio::task::spawn_blocking(move || {
        test_only_phase3_delay();
        record_consumptions(&events, &execution, &consumptions);
    })
```

**Replace with:**

```rust
    let _ = tokio::task::spawn_blocking(move || {
        crate::commands::open_probe::with_caller("sweep", || {
            test_only_phase3_delay();
            record_consumptions(&events, &execution, &consumptions);
        });
    })
```

> The delay seam is wrapped **inside** the label rather than outside it. It opens no store, so
> either placement records the same rows; keeping the block intact is simply less to get wrong.
> `#72`'s seam is inert here regardless — it reads one env var we do not set, and phase 3 never
> executes during the storm.

> Phase 3 does **not** execute during the storm (no lease is armed, so phase 1 returns early at
> `wake.rs:131`). It is tagged for correctness, not because the storm exercises it.

---

## Edit 4 — `apps/cli/src/commands/serve/routes.rs` (~line 819) — **the tag the review demanded**

Without this, the async driver's opens are counted as `request` work and inflate the serial
share. A two-way request/sweep split mis-attributes exactly as badly as the thread-id split it
replaced.

**Find:**

```rust
    let store_open: StoreOpen = Arc::new(move || event_store(&events));
```

**Replace with:**

```rust
    let store_open: StoreOpen = Arc::new(move || {
        crate::commands::open_probe::with_caller("driver", || event_store(&events))
    });
```

---

## The instrument's own controls — both MUST be observed failing

Rung A carries this lane's conclusions and is otherwise unguarded. Neither control's expected
value comes from the probe's own output.

1. **Exactly-one** (expectation from reading `status.rs:24`): a single
   `GET /v1/executions/{id}` against a served store produces **exactly one**
   `open-begin`/`open-end` pair with `caller=request`, and **zero** rows with any other tag.
   *Sabotage:* remove Edit 4 and this control must fail, by seeing a driver open tagged
   `request`.
2. **Forced-sweep** (expectation from reading `wake.rs`): one armed lease plus one successful
   mutation produces at least one `caller=sweep` row. *Sabotage:* remove Edit 2 and this control
   must fail.

Without both observed failing, the tag is an assertion about itself.
