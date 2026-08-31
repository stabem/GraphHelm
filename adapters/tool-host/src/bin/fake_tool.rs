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
//! - `big-output`: write 8 MiB of `x` to stdout, exit 0
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
            std::process::exit(code);
        }
        other => {
            eprintln!("fake_tool: unknown mode {other:?}");
            std::process::exit(64);
        }
    }
}
