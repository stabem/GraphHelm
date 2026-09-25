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
//! - `spawn-grandchild <addr>`: spawn `sleep` as a CHILD OF THIS CHILD, then connect to the
//!   loopback address `<addr>` and write `direct_pid,grandchild_pid` before sleeping. The fixture for
//!   #618: every other mode is a single process, so none of them can observe whether a kill
//!   reached a tree or only its root. The grandchild reports its own id because the host never had
//!   a handle to it.
//! - `cli-surrogate <addr>`: run `spawn-grandchild <addr>` through the real ToolHost process
//!   funnel, then block until the outer observer kills this CLI surrogate.
//!
//!   It reports over a SOCKET rather than into a file because a file has no arrival: a reader can
//!   only ask again later, so the waiting side of the protocol was 400 sleeps of 50 ms and the
//!   normal path's timing was decided by the poll interval rather than by the fixture (#727). A
//!   connection is an event the test blocks on, so the wait ends the instant the id exists and the
//!   bound is only ever reached when nothing was reported at all.
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
//! - `leader-exits-holding-stdout <addr>`: spawn `sleep` as a grandchild that INHERITS this
//!   process's stdout, report `direct_pid,grandchild_pid` over `<addr>`, write one marker line and
//!   EXIT. The fixture for #714: every other grandchild mode keeps the leader alive until the host
//!   kills it, so none of them can reach the ordinary leader-exit path, where the poll loop breaks
//!   on `Ok(Some(status))` and nothing is ever terminated. Here the leader is gone and a silent
//!   orphan holds the write end of stdout, which is the only arrangement in which the drain's
//!   release decides whether the capture arrives or is abandoned.
//! - `exit-code <n>`: exit with `<n>` parsed as `i32`

use std::collections::BTreeMap;
use std::io::Write as _;

use graphhelm_tool_host::process::{ProcessLimits, run_in_workspace};

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
        "cli-surrogate" => {
            let report_to = arguments
                .next()
                .expect("cli-surrogate needs a loopback address to report to");
            let workspace = std::env::current_dir().expect("cli-surrogate current dir");
            let program = std::env::current_exe().expect("cli-surrogate own path");
            let program = program.to_str().expect("cli-surrogate path is UTF-8");
            let result = run_in_workspace(
                &workspace,
                program,
                &["spawn-grandchild".to_owned(), report_to],
                &BTreeMap::new(),
                &[],
                None,
                &ProcessLimits {
                    // The outer observer kills this process; the timeout must not end the fixture.
                    timeout: std::time::Duration::from_secs(120),
                    max_output_bytes: 1024,
                },
                None,
            );
            panic!("cli-surrogate ToolHost returned before outer kill: {result:?}");
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
        "spawn-grandchild" => {
            // The fixture for #618: this process spawns ANOTHER one and reports its id, so a test
            // can ask about a process the host never had a handle to. `append-forever` and `sleep`
            // are both single processes by construction and cannot observe a tree kill at all --
            // which is why the descendant gap survived every cell until this one.
            let report_to = arguments
                .next()
                .expect("spawn-grandchild needs a loopback address to report to");
            // Never waited on, DELIBERATELY: the whole point is a process that outlives its parent
            // and must be reached by something other than this handle. Waiting here would reap the
            // grandchild before the host's kill could fail to, which is the observation the fixture
            // exists to make possible.
            #[allow(clippy::zombie_processes)]
            let grandchild = std::process::Command::new(std::env::current_exe().expect("own path"))
                .arg("sleep")
                .stdin(std::process::Stdio::null())
                // NOT enough to stop inheritance on Windows, and this comment used to claim it was.
                // MEASURED: with the tree kill sabotaged, the cell did not fail -- it HUNG, with a
                // live grandchild, because `Command` spawns with `bInheritHandles = TRUE` and the
                // parent's pipe handles are inheritable at that moment. Setting the grandchild's own
                // stdio to null decides what its std handles POINT AT; it does not decide which
                // handles it receives.
                //
                // So this fixture carries BOTH halves of #618 whether it wants to or not: the
                // descendant survives, and it holds the pipes open so the reader joins never
                // return. That is the failure mode the issue names, and it is why the cell's red is
                // a hang rather than an assertion.
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("the grandchild spawns");
            // AFTER the spawn, deliberately. A socket opened before it would be a handle in this
            // process at the moment `Command` spawns with `bInheritHandles = TRUE`, and this
            // fixture exists precisely because that inheritance is not obvious -- the comment above
            // is about the pipes it already leaks by accident. Connecting afterwards means the
            // grandchild cannot hold the reporting channel open, so the test's read reaches EOF
            // when this process closes it rather than when the grandchild finally dies.
            {
                let mut report = std::net::TcpStream::connect(report_to.as_str())
                    .expect("the readiness channel accepts a connection");
                report
                    .write_all(format!("{},{}", std::process::id(), grandchild.id()).as_bytes())
                    .expect("the direct child and grandchild ids are reported");
                report.flush().expect("the grandchild id is flushed");
            }
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
        // #748: the same fixture, with the grandchild LEAVING the process group before it does
        // anything else.  in  runs after fork and before exec, so the escape is
        // complete before the grandchild has written a byte -- which is the shape the issue asks
        // for, and the shape an escaping tool would really have.
        //
        // Unix only, and deliberately not mirrored on Windows: a job object holds every process
        // its members create, and leaving one needs CREATE_BREAKAWAY_FROM_JOB plus a job that
        // permits it. There is nothing to escape with.
        #[cfg(unix)]
        "spawn-escaping-grandchild" => {
            use std::os::unix::process::CommandExt as _;

            let report_to = arguments
                .next()
                .expect("spawn-escaping-grandchild needs a loopback address to report to");
            #[allow(clippy::zombie_processes)]
            let grandchild = {
                let mut command =
                    std::process::Command::new(std::env::current_exe().expect("own path"));
                command
                    .arg("sleep")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                unsafe {
                    command.pre_exec(|| {
                        if libc::setsid() == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                command.spawn().expect("the escaping grandchild spawns")
            };
            {
                let mut report = std::net::TcpStream::connect(report_to.as_str())
                    .expect("the readiness channel accepts a connection");
                report
                    .write_all(format!("{},{}", std::process::id(), grandchild.id()).as_bytes())
                    .expect("the grandchild id is reported");
                report.flush().expect("the grandchild id is flushed");
            }
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
        // #714, and Unix-only for the same reason the escaping fixture is: what this measures is
        // whether `close` releases anything on a platform where the group is a process group and
        // not a job object. On Windows the job already kills the holder, so the arrangement is
        // not the open question there.
        #[cfg(unix)]
        "leader-exits-holding-stdout" => {
            let report_to = arguments
                .next()
                .expect("leader-exits-holding-stdout needs a loopback address to report to");
            // INHERITED stdout, deliberately, and it is the whole fixture. `spawn-grandchild`
            // sets the grandchild's stdio to null and still leaks the pipes on Windows by
            // handle inheritance; on Unix null stdio really does mean the grandchild holds no
            // write end, so nothing would block. Asking for the parent's stdout is how a Unix
            // orphan comes to hold the pipe -- and it is what a tests runner's worker does.
            #[allow(clippy::zombie_processes)]
            let grandchild = std::process::Command::new(std::env::current_exe().expect("own path"))
                .arg("sleep")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("the grandchild spawns");
            {
                let mut report = std::net::TcpStream::connect(report_to.as_str())
                    .expect("the readiness channel accepts a connection");
                report
                    .write_all(format!("{},{}", std::process::id(), grandchild.id()).as_bytes())
                    .expect("the direct child and grandchild ids are reported");
                report.flush().expect("the grandchild id is flushed");
            }
            // The marker goes out BEFORE the exit, so the capture this cell asks about is bytes
            // that certainly reached the pipe. A cell that asserted on an empty expectation could
            // not tell "released and got the bytes" from "released and there were none".
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            writeln!(out, "leader-line").expect("stdout");
            out.flush().expect("the marker is flushed");
            // And then this process RETURNS. No sleep: the poll loop must break on
            // `Ok(Some(status))`, which is the arm #714 is about.
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
