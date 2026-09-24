mod args;
mod commands;
mod error_codes;
mod human;
mod output;
mod palette;

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
            present(
                &outcome.output,
                flag_requested(&arguments, "--json"),
                pretty_requested(&arguments),
            );
            std::process::exit(outcome.exit_code);
        }
        Err(error) => error.exit(),
    };
    let outcome = commands::run(cli.command);
    present(&outcome.output, cli.json, cli.pretty);
    std::process::exit(outcome.exit_code);
}

/// #1172: one face per run, decided by [`output::face`] and printed here.
///
/// STDOUT CARRIES EXACTLY ONE OF THE TWO. Without a terminal that is always the JSON envelope, so
/// every pipe, test harness and MCP caller reads the bytes it read before #1172 — escape bytes
/// included, of which there are none, because [`palette::Palette::decide`] is given the same
/// terminal answer. At a terminal without `--json`/`--pretty`, a command that can render itself
/// prints its rendering instead, and the envelope is not printed at all: printing both is what
/// #1172 was filed about.
fn present(output: &output::CommandOutput, json: bool, pretty: bool) {
    let is_terminal = std::io::stdout().is_terminal();
    let environment = palette::Environment::from_process();
    match output::face(
        is_terminal,
        json,
        pretty,
        human::has_renderer(output.command),
    ) {
        output::Face::Human => {
            let palette = palette::Palette::decide(is_terminal, environment);
            if palette.writes_escapes() {
                palette::prepare_stream();
            }
            // `face` only answers `Human` for a command with a renderer, but a renderer answers
            // `None` for an envelope carrying no `data`. The machine face is what that run gets:
            // the alternative is a command that printed nothing at all.
            match human::render(output, palette) {
                Some(rendered) => {
                    let mut stdout = std::io::stdout().lock();
                    let _ = stdout.write_all(rendered.as_bytes());
                    let _ = stdout.flush();
                }
                None => output::print(output, true),
            }
        }
        output::Face::Machine { pretty } => {
            output::print(output, pretty);
            print_human_summary(output, is_terminal, environment);
        }
    }
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
fn print_human_summary(
    output: &output::CommandOutput,
    stdout_is_terminal: bool,
    environment: palette::Environment<'_>,
) {
    if !stdout_is_terminal {
        return;
    }
    // Colour on STDERR is decided by stderr's own terminal answer: this summary is written there
    // and a redirected stderr must be as clean as a redirected stdout.
    let palette = palette::Palette::decide(std::io::stderr().is_terminal(), environment);
    if palette.writes_escapes() {
        palette::prepare_stream();
    }
    if let Some(summary) = human::render(output, palette) {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(summary.as_bytes());
        let _ = stderr.flush();
    }
}

fn structured_invocation(arguments: &[OsString]) -> Option<&'static str> {
    let command = arguments
        .iter()
        .skip(1)
        // Global presentation flags may appear before the command. Ignore both of them while
        // recovering the command name after clap rejects a malformed invocation, so the error
        // path can still emit the requested JSON envelope.
        .find(|argument| {
            argument.as_os_str() != OsStr::new("--pretty")
                && argument.as_os_str() != OsStr::new("--json")
        })
        .and_then(|argument| top_level_command(argument.to_string_lossy().as_ref()))?;

    // Schema and extension already promised structured parse errors before #1172. Other command
    // families opt into that contract only when the caller explicitly asks for a machine face;
    // otherwise their long-standing clap diagnostics (including usage) stay byte-for-byte intact.
    if matches!(command, "schema" | "extension")
        || flag_requested(arguments, "--json")
        || flag_requested(arguments, "--pretty")
    {
        Some(command)
    } else {
        None
    }
}

/// The root command is the only stable name available when clap rejected the rest of an
/// invocation. Keep this complete with [`args::TopLevel`], so every command family gets the same
/// structured argument-error contract instead of only the families that happened to need it first.
fn top_level_command(argument: &str) -> Option<&'static str> {
    match argument {
        "graph" => Some("graph"),
        "schema" => Some("schema"),
        "extension" => Some("extension"),
        "development" => Some("development"),
        "events" => Some("events"),
        "execution" => Some("execution"),
        "gateway" => Some("gateway"),
        "tool" => Some("tool"),
        "serve" => Some("serve"),
        "mcp" => Some("mcp"),
        "wake-wait" => Some("wake-wait"),
        "quality" => Some("quality"),
        "gate" => Some("gate"),
        "init" => Some("init"),
        "keel" => Some("keel"),
        _ => None,
    }
}

fn pretty_requested(arguments: &[OsString]) -> bool {
    flag_requested(arguments, "--pretty")
}

/// Whether a global flag was typed, read off the raw arguments because this path runs when clap
/// itself refused to parse them.
fn flag_requested(arguments: &[OsString], flag: &str) -> bool {
    arguments.iter().skip(1).any(|argument| argument == flag)
}
