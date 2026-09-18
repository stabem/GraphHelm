mod args;
mod commands;
mod error_codes;
mod human;
mod output;

use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Write};

use clap::{Parser, error::ErrorKind};
use graphhelm_protocols::Diagnostic;

fn main() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let cli = match args::Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error)
            if structured_invocation(&arguments).is_some()
                && !matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
        {
            let command = structured_invocation(&arguments).expect("matched above");
            let outcome = output::Outcome::domain(
                command,
                vec![Diagnostic::error(
                    crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                    format!("{command} command arguments are invalid"),
                    "/arguments",
                    format!("{command}-cli"),
                )],
            );
            output::print(&outcome.output, pretty_requested(&arguments));
            print_human_summary(&outcome.output);
            std::process::exit(outcome.exit_code);
        }
        Err(error) => error.exit(),
    };
    let outcome = commands::run(cli.command);
    output::print(&outcome.output, cli.pretty);
    print_human_summary(&outcome.output);
    std::process::exit(outcome.exit_code);
}

/// #1150: the human summary, on STDERR, and ONLY when stdout is a terminal.
///
/// **Stdout is never touched here.** It carried the JSON contract a line ago and this function
/// writes to a different stream, so a pipe, a test harness, the MCP tool and every other reader
/// see byte-identical output to before this existed — `piped_stdout_is_exactly_the_json_contract`
/// holds that.
///
/// **The terminal check is the whole gate, and there is deliberately no flag.** A flag nobody
/// knows about is not an answer to "a person gets a blob": the person who needs this is the one
/// who has not read the documentation yet. `IsTerminal` is std, so this costs no dependency.
///
/// Failure to write is ignored on purpose: a closed or full stderr must not change the exit code
/// the command earned, and the answer that matters already went to stdout.
fn print_human_summary(output: &output::CommandOutput) {
    if !std::io::stdout().is_terminal() {
        return;
    }
    if let Some(summary) = human::render(output) {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(summary.as_bytes());
        let _ = stderr.flush();
    }
}

fn structured_invocation(arguments: &[OsString]) -> Option<&'static str> {
    arguments
        .iter()
        .skip(1)
        .find(|argument| argument.as_os_str() != OsStr::new("--pretty"))
        .and_then(|argument| match argument.to_string_lossy().as_ref() {
            "schema" => Some("schema"),
            "extension" => Some("extension"),
            _ => None,
        })
}

fn pretty_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .skip(1)
        .any(|argument| argument == "--pretty")
}
