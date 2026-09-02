//! Test-fixture binary for the tool-host integration tests (reached via
//! `CARGO_BIN_EXE_fake_tool`, the 05b `fake_runtime` precedent). It ships with the crate
//! because Cargo only builds `[[bin]]` targets for integration tests when they are real
//! targets; it does nothing an ordinary child process could not, and nothing harmful.
//!
//! Behavior is selected by the FIRST argument:
//!
//! - `env-dump`: print every environment variable as `NAME=VALUE` lines, exit 0
//! - `echo <words...>`: print the remaining argv joined by spaces, exit 0
//! - `write-file <path>`: write the bytes `written` to `<path>` resolved against the current
//!   directory, exit 0
//! - `cwd`: print the current directory, exit 0
//! - `sleep`: sleep 3600 s (the host must kill it)
//! - `append-forever <path>`: append a line to <path> every 5 ms, forever. The mirror of
//!   `sleep` for CANCELLATION (#180): `sleep` proves a child was killed by observing that the
//!   CALL returned, which a caller can fake by abandoning the handle. This one leaves a trace
//!   OUTSIDE the process, so "the child is gone" is measured by the file no longer growing
//!   rather than by the parent claiming it stopped waiting.
//! - `big-output`: write 8 MiB of `x` to stdout, exit 0
//! - `marked-output`: write a HEAD sentinel, 8 MiB of filler, then a TAIL sentinel, exit 0.
//!   `big-output` cannot test which END of an overflowing stream survives -- it writes 8 MiB of
//!   identical `x`, so keeping the head and keeping the tail produce the same bytes. This one
//!   makes the two answers different.
//! - `big-stderr`: write 8 MiB of `x` to STDERR and one short line to stdout, exit 0.
//!   The mirror of `big-output`: it exists so a cut on one stream can be told apart
//!   from a cut on the other, which a single fused flag cannot express (#177).
//! - `exit-code <n>`: exit with `<n>` parsed as `i32`

use std::io::Write as _;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next().unwrap_or_default();
    match mode.as_str() {
        "env-dump" => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            for (name, value) in std::env::vars() {
                writeln!(out, "{name}={value}").expect("stdout");
            }
        }
        "echo" => {
            println!("{}", arguments.collect::<Vec<_>>().join(" "));
        }
        "write-file" => {
            let path = arguments.next().expect("write-file needs a path");
            std::fs::write(path, b"written").expect("write");
        }
        "cwd" => {
            println!(
                "{}",
                std::env::current_dir().expect("current dir").display()
            );
        }
        "append-forever" => {
            let path = arguments.next().expect("append-forever needs a path");
            let mut tick: u64 = 0;
            loop {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .expect("append-forever opens its file");
                writeln!(file, "tick {tick}").expect("append");
                tick += 1;
                // FIVE milliseconds, an order under the host's 50 ms poll, and the interval is
                // the whole instrument. At 50 ms the child wrote at the same rate the loop
                // polls, so "still alive for one more poll" produced either one extra line or
                // none -- a coin flip, and the cancel-returns-after-the-reap cell came out GREEN
                // against a `cancel` that only signalled. A fixture whose grain matches the
                // defect's grain cannot see the defect.
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        "sleep" => {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
        "big-output" => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..128 {
                out.write_all(&chunk).expect("stdout");
            }
        }
        "marked-output" => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            out.write_all(
                b"HEAD-SENTINEL
",
            )
            .expect("stdout");
            let chunk = vec![b'.'; 64 * 1024];
            for _ in 0..128 {
                out.write_all(&chunk).expect("stdout");
            }
            out.write_all(
                b"
TAIL-SENTINEL
",
            )
            .expect("stdout");
        }
        "big-stderr" => {
            // stdout stays SHORT on purpose: the pair (big-output, big-stderr) differs in which
            // stream overflows, and nothing else.
            println!("short");
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..128 {
                err.write_all(&chunk).expect("stderr");
            }
        }
        "exit-code" => {
            let code: i32 = arguments
                .next()
                .expect("exit-code needs a value")
                .parse()
                .expect("an i32");
            // 259 is STATUS_PENDING, which is what Windows reports as the exit code of a process
            // that has NOT exited. A liveness check reads it as "still running", so a fixture that
            // could exit with it would be indistinguishable from a fixture still alive.
            //
            // Refused HERE rather than promised in a comment (L, on #680). The seal used to read
            // "every caller spawns programs that do not exit with 259", which was true of today's
            // suite and said nothing about the binary — a seal against a corpus rots the moment
            // somebody adds a caller. This one is a property of `fake_tool` itself.
            assert_ne!(
                code, 259,
                "fake_tool refuses to exit with STATUS_PENDING: a process reporting 259 cannot be \
                 told apart from one that is still running, and the liveness observer would read \
                 this fixture as alive forever"
            );
            std::process::exit(code);
        }
        other => {
            eprintln!("fake_tool: unknown mode {other:?}");
            std::process::exit(64);
        }
    }
}
