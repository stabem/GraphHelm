//! #1308 (plan D6): the `--help` text of every command, pinned byte for byte.
//!
//! The CLI is a public surface. A refactor of its argument structs (shared groups flattened in,
//! fields moved between structs) must not move a flag, a value name, a doc line or an order. The
//! fixture holds `graphhelm <path> --help` for the whole command tree, rendered by clap at a fixed
//! width so the terminal does not change it. `GRAPHHELM_WRITE_FIXTURES=1` rewrites it; a rewrite is
//! a surface change and belongs in the PR's diff, never in a refactor.

use clap::CommandFactory;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/help/graphhelm-help.txt"
);

fn walk(command: &mut clap::Command, path: &str, out: &mut String) {
    let help = command.render_long_help().to_string();
    out.push_str(&format!("===== graphhelm{path} --help\n{help}\n"));
    let mut names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    names.sort();
    for name in names {
        let sub = command
            .find_subcommand_mut(&name)
            .expect("a listed subcommand exists");
        walk(sub, &format!("{path} {name}"), out);
    }
}

fn rendered() -> String {
    let mut root = crate::args::Cli::command()
        .term_width(100)
        .max_term_width(100);
    root.build();
    let mut out = String::new();
    walk(&mut root, "", &mut out);
    out.replace("\r\n", "\n")
}

#[test]
fn every_command_help_is_byte_identical_to_the_golden() {
    let actual = rendered();
    if std::env::var_os("GRAPHHELM_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(std::path::Path::new(FIXTURE).parent().unwrap()).unwrap();
        std::fs::write(FIXTURE, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(FIXTURE)
        .expect("tests/fixtures/help/graphhelm-help.txt exists")
        .replace("\r\n", "\n");
    assert!(
        actual.len() > 10_000,
        "HARNESS-BROKE: the walk rendered almost nothing"
    );
    if actual != expected {
        let first = actual
            .lines()
            .zip(expected.lines())
            .position(|(a, e)| a != e)
            .unwrap_or_else(|| actual.lines().count().min(expected.lines().count()));
        let section = actual
            .lines()
            .take(first + 1)
            .filter(|line| line.starts_with("===== "))
            .last()
            .unwrap_or("?");
        panic!(
            "--help changed in `{section}` at line {}:\n  got:      {:?}\n  expected: {:?}",
            first + 1,
            actual.lines().nth(first),
            expected.lines().nth(first)
        );
    }
}
