mod args;
mod commands;
mod output;

use std::ffi::{OsStr, OsString};

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
                    "GHCLI001_ARGUMENT_INVALID",
                    format!("{command} command arguments are invalid"),
                    "/arguments",
                    format!("{command}-cli"),
                )],
            );
            output::print(&outcome.output, pretty_requested(&arguments));
            std::process::exit(outcome.exit_code);
        }
        Err(error) => error.exit(),
    };
    let outcome = commands::run(cli.command);
    output::print(&outcome.output, cli.pretty);
    std::process::exit(outcome.exit_code);
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
