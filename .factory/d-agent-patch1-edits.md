# Patch 1 — client-side phase timing (D Agent)

**NAMED BASE: `ef51193`** (tip of `origin/issue-m09-arming-the-alarm`, *not* of `main`).
All anchors below re-grepped and found **byte-identical to `53d212d`** — neither `#74` nor `#72`
touched `apps/cli/tests/api_http.rs`. (`#72` touched `wake_http.rs` and production `wake.rs`;
this file anchors in neither.) **MEASUREMENT ONLY — revert
before committing.** Anchored on exact text, not line numbers.

**Two jobs, both load-bearing:**
1. Settle the phase on the `get_status` path, where all three phases collapse into one panic.
2. **Supply the independent clock** that validates rung A's storm-phase denominator. Rung A's
   phase bound is computed from its own rows and is otherwise self-asserted.

**What it deliberately does NOT change:** `TcpStream::connect` stays as it is — no
`connect_timeout`. Imposing our own connect deadline would change *when* a connect failure
fires, i.e. change the thing being measured. Phase attribution comes from **which call returned
the error**, never from the error's kind — on Windows `ErrorKind` is `TimedOut` for both a
connect timeout and a read-timeout expiry. The 5s read/write timeouts stay 5s.

**⚠ EVIDENCE-BEARING FORMAT — DO NOT REFORMAT.** `probe_failure` embeds the original error's
`Display` via `({error})`. One hypothesis (client port exhaustion) died *because the OS code
survived into the panic text* at the laundered site. Dropping or reformatting that interpolation
retroactively destroys that evidence. Also note the rewrapped error has **no `raw_os_error()`**
(an `io::Error::new` Custom returns `None`) — nothing reads it programmatically today, but keep
the code in the TEXT.

---

## Edit 1 — imports

**Find:** `use std::process::{Child, Command, Stdio};`
**Add after it:** `use std::sync::{Mutex, OnceLock};`

## Edit 2 — probe helpers, inserted after `struct RawResponse { ... }` and before `fn raw_request`

```rust
#[derive(Default)]
struct PhaseTimes {
    connect_us: u128,
    write_us: u128,
    read_us: u128,
}

/// EPOCH microseconds, matching rung A's server-side stamps exactly. This is what makes the two
/// instruments comparable: the client clock is the INDEPENDENT apparatus validating rung A's
/// storm-phase denominator. A per-process `Instant` here makes that cross-check impossible.
fn probe_epoch_us() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_micros())
}

/// A FILE sink, not stderr — matching rung A. libtest's capture is thread-local and is NOT
/// inherited by the scoped storm threads, so stderr would capture the health-poll noise while
/// the storm's own samples bypassed it: two sinks for one dataset. `create_new` refuses an
/// existing file so run N cannot append onto run N-1, and an unusable path PANICS rather than
/// degrading to silence.
fn probe_sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var("GRAPHHELM_CLIENT_PROBE").ok()?;
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("GRAPHHELM_CLIENT_PROBE unusable at {path}: {error}"));
        Some(Mutex::new(file))
    })
    .as_ref()
}

/// One line per request, built as a single formatted string so eight threads interleave by whole
/// line rather than mid-line.
fn probe_record(outcome: &str, phase: &str, kind: &str, url: &str, times: &PhaseTimes) {
    let Some(sink) = probe_sink() else { return };
    let line = format!(
        "CLIENT t={} thread={} outcome={outcome} phase={phase} kind={kind} \
         connect_us={} write_us={} read_us={} url={url}\n",
        probe_epoch_us(),
        std::thread::current().name().unwrap_or("main"),
        times.connect_us,
        times.write_us,
        times.read_us
    );
    if let Ok(mut file) = sink.lock() {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Records the failure and rewraps it with the phase and every phase's elapsed time, KEEPING the
/// original `ErrorKind` so callers behave exactly as before. The `({error})` interpolation is
/// evidence-bearing — see the header note.
fn probe_failure(
    phase: &str,
    url: &str,
    times: &PhaseTimes,
    error: std::io::Error,
) -> std::io::Error {
    probe_record("err", phase, &format!("{:?}", error.kind()), url, times);
    std::io::Error::new(
        error.kind(),
        format!(
            "{phase} failed after connect={}us write={}us read={}us ({error}) [{url}]",
            times.connect_us, times.write_us, times.read_us
        ),
    )
}
```

## Edit 3 — `fn raw_request` (~line 161)

Replace the three I/O statements with timed equivalents. **Find/replace, in order:**

| Find | Replace with |
|---|---|
| `let mut stream = TcpStream::connect((host.as_str(), port))?;` | (block A below) |
| `stream.write_all(request.as_bytes())?;` | (block B below) |
| `stream.read_to_end(&mut raw)?;` | (block C below) |

Block A:
```rust
    let mut times = PhaseTimes::default();
    let started = Instant::now();
    let connected = TcpStream::connect((host.as_str(), port));
    times.connect_us = started.elapsed().as_micros();
    let mut stream = match connected {
        Ok(stream) => stream,
        Err(error) => return Err(probe_failure("connect", url, &times, error)),
    };
```
Block B:
```rust
    let started = Instant::now();
    let written = stream.write_all(request.as_bytes());
    times.write_us = started.elapsed().as_micros();
    if let Err(error) = written {
        return Err(probe_failure("write", url, &times, error));
    }
```
Block C:
```rust
    let started = Instant::now();
    let read = stream.read_to_end(&mut raw);
    times.read_us = started.elapsed().as_micros();
    if let Err(error) = read {
        return Err(probe_failure("read", url, &times, error));
    }
    probe_record("ok", "-", "-", url, &times);
```

## Edit 4 — `fn post_request` (~line 436)

Same three sites, but this function panics rather than returning `Result`.

| Find | Replace with |
|---|---|
| `let mut stream = TcpStream::connect((host.as_str(), port)).unwrap();` | block D |
| `stream.write_all(request.as_bytes()).unwrap();`<br>`stream.write_all(&payload).unwrap();` (**both lines**) | block E |
| `stream.read_to_end(&mut raw).unwrap();` | block F |

Block D:
```rust
    let mut times = PhaseTimes::default();
    let started = Instant::now();
    let connected = TcpStream::connect((host.as_str(), port));
    times.connect_us = started.elapsed().as_micros();
    let mut stream = match connected {
        Ok(stream) => stream,
        Err(error) => panic!("{}", probe_failure("connect", url, &times, error)),
    };
```
Block E — **two statements deliberately**, not `.and_then(|()| stream.write_all(&payload))`:
that closure borrows `stream` mutably inside a call whose receiver is `stream`. Probably fine
under NLL, but not worth gambling in a patch nobody can compile-check first. `write_us` covers
header AND payload as one phase — stated, not implied.
```rust
    let started = Instant::now();
    let mut written = stream.write_all(request.as_bytes());
    if written.is_ok() {
        written = stream.write_all(&payload);
    }
    times.write_us = started.elapsed().as_micros();
    if let Err(error) = written {
        panic!("{}", probe_failure("write", url, &times, error));
    }
```
Block F:
```rust
    let started = Instant::now();
    let read = stream.read_to_end(&mut raw);
    times.read_us = started.elapsed().as_micros();
    if let Err(error) = read {
        panic!("{}", probe_failure("read", url, &times, error));
    }
    probe_record("ok", "-", "-", url, &times);
```

## Edit 4b — `fn serve_with` (~line 63): print the server's pid

Needed so the post-run positive control can be **mechanical**. The check "at least one probe row
from the SERVER pid" is what distinguishes *the server never inherited the variable* from *the
server was genuinely quiet* — but without this line, identifying which pid is the server is an
inference (largest file, longest-lived), and an inference is exactly what fails silently here.

**Find** (in `serve_with`, immediately after the `.spawn().unwrap();` that creates `child`):

```rust
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
```

**Insert immediately BEFORE it:**

```rust
    // MEASUREMENT ONLY. Gated so the line appears only when probing; the post-run control needs
    // the server's pid to assert the probe was actually ON for the server process.
    if std::env::var("GRAPHHELM_CLIENT_PROBE").is_ok() {
        println!("SERVER pid={}", child.id());
    }
```

## Edit 5 — `fn run_storm` (~line 1460): name the threads so `thread=` identifies them

**Find:**
```rust
            scope.spawn(move || {
                storm_thread(base, token, execution, evidence_dir, thread_id, rounds);
            });
```
**Replace with:**
```rust
            std::thread::Builder::new()
                .name(format!("storm-{thread_id}"))
                .spawn_scoped(scope, move || {
                    storm_thread(base, token, execution, evidence_dir, thread_id, rounds);
                })
                .expect("spawning a named storm thread");
```

## Edit 6 — the storm test (~line 1433): preserve the events tree

The panic currently destroys its own evidence. `into_path()` after `run_storm` is **unreachable**
— the scope re-panics at the join, so no later line executes and `TempDir::drop` deletes the tree
while unwinding. `catch_unwind` + `resume_unwind` keeps the ORIGINAL panic and its `file:line`
intact, which the whole phase-attribution method depends on.

**Find:** `let directory = tempfile::tempdir().unwrap();` (inside
`the_storm_holds_under_eight_concurrent_agents`)
**Replace with:** `let mut directory = tempfile::tempdir().unwrap();`

**Find:**
```rust
    run_storm(
        &base,
        &token,
        execution,
        &evidence_dir,
        STORM_THREADS,
        STORM_ROUNDS,
    );
```
**Replace with:**
```rust
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_storm(
            &base,
            &token,
            execution,
            &evidence_dir,
            STORM_THREADS,
            STORM_ROUNDS,
        );
    }));
    // Preserved on PASSING runs too — without a passing baseline there is nothing to compare the
    // failing committed-event count against. Printed, or the path is unfindable.
    if std::env::var("GRAPHHELM_CLIENT_PROBE").is_ok() {
        let kept = std::mem::replace(&mut directory, tempfile::tempdir().unwrap()).keep();
        println!("PRESERVED events tree at {}", kept.display());
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
```

> `TempDir::keep()` is correct for **tempfile 3.27.0** (confirmed in `Cargo.lock`); `into_path()`
> is the deprecated older name. The replacement tempdir is dropped normally and costs nothing.
> This is the only part of Patch 1 that is not a pure observer, so it is gated behind the env
> var: with the probe off, the test behaves exactly as it does today.
