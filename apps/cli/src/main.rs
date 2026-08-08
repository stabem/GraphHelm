mod args;
mod commands;
mod output;

use clap::Parser;

fn main() {
    let cli = args::Cli::parse();
    let outcome = commands::run(cli.command);
    output::print(&outcome.output, cli.pretty);
    std::process::exit(outcome.exit_code);
}
