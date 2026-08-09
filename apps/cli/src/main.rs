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
            if schema_invocation(&arguments)
                && !matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
        {
            let outcome = output::Outcome::domain(
                "schema",
                vec![Diagnostic::error(
                    "GHCLI001_ARGUMENT_INVALID",
                    "schema command arguments are invalid",
                    "/arguments",
                    "schema-cli",
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

fn schema_invocation(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .skip(1)
        .find(|argument| argument.as_os_str() != OsStr::new("--pretty"))
        .is_some_and(|argument| argument == "schema")
}

fn pretty_requested(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .skip(1)
        .any(|argument| argument == "--pretty")
}
