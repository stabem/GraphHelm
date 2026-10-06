use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_policy::keel::{self as policy_keel, Card, KeelPolicy};
use graphhelm_policy::keel_prove::{self, ProveOptions};
use graphhelm_protocols::Diagnostic;

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel";

/// The shipped Keel rules, embedded so the verdict names the policy version the binary was built
/// with and never depends on where the package is installed. The same bytes are the
/// `policy/keel` contribution `extension.json` binds by digest.
const KEEL_POLICY: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/policies/keel.yaml"
);
const KEEL_CARD_SCHEMA: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/schemas/keel-card.schema.json"
);

/// A card is a few hundred bytes (`keel.yaml` `card.maxCardBytes`); this only stops an unbounded
/// read. A card between the two is read and reported as `keel.card.too_large`.
const MAX_CARD_READ_BYTES: u64 = 1024 * 1024;

const INPUT_INVALID: &str = crate::error_codes::GHCLI031_KEEL_CHECK_INPUT;

pub(super) fn run(operation: keel_contract_index::Operation) -> Outcome {
    match keel_contract_index::execute_public(operation) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(error) => Outcome::domain(
            COMMAND,
            vec![Diagnostic::error(
                error.code,
                error.message,
                "/keel",
                "keel",
            )],
        ),
    }
}

fn input_error(message: impl Into<String>, path: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(INPUT_INVALID, message, path, "keel"),
    )
}

/// `--prove-new-tests` and its bounds (#1333).
pub(super) struct ProveArgs {
    pub(super) target_dir: Option<PathBuf>,
    pub(super) timeout_secs: u64,
    pub(super) command: Option<String>,
}

/// `graphhelm keel check --diff <base>..<head> [--card <card.json>] [--repo <dir>]
/// [--prove-new-tests]` (#1330, #1333).
///
/// Exit 0 when nothing blocks (signals travel as warnings), 2 when a finding blocks (the report
/// still travels in `data`), 3 when the range, the repository or the card cannot be read, or the
/// proving worktrees cannot be set up.
pub(super) fn check(
    repo: &Path,
    range: &str,
    card_path: Option<&Path>,
    prove: Option<ProveArgs>,
) -> Outcome {
    if range.starts_with('-') || !range.contains("..") {
        return input_error("--diff takes a git range `<base>..<head>`", "/diff");
    }
    if let Some(command) = prove.as_ref().and_then(|args| args.command.as_deref())
        && !command.contains("{report}")
    {
        return input_error(
            "--prove-command must write a JUnit report to `{report}`; only that report decides",
            "/proveCommand",
        );
    }
    let card = match card_path.map(read_card).transpose() {
        Ok(card) => card,
        Err(outcome) => return *outcome,
    };
    // Both spellings mean "what the head branch changed": the diff starts at the merge-base, so
    // commits that reached the base after the branch was cut are never charged to it.
    let (base, head) = range
        .split_once("...")
        .or_else(|| range.split_once(".."))
        .unwrap_or((range, "HEAD"));
    let base = if base.is_empty() { "HEAD" } else { base };
    let head = if head.is_empty() { "HEAD" } else { head };
    let merge_base = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", base, head])
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        Ok(output) => {
            return input_error(
                format!(
                    "git merge-base {base} {head} failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                "/diff",
            );
        }
        Err(error) => return input_error(format!("git did not start: {error}"), "/repo"),
    };
    let diff_range = format!("{merge_base}..{head}");
    let output = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &diff_range,
            "--",
        ])
        .output()
    {
        Ok(output) => output,
        Err(error) => return input_error(format!("git did not start: {error}"), "/repo"),
    };
    if !output.status.success() {
        return input_error(
            format!(
                "git diff {range} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            "/diff",
        );
    }
    let diff = String::from_utf8_lossy(&output.stdout);
    let policy: KeelPolicy = match serde_yaml_ng::from_str(KEEL_POLICY) {
        Ok(policy) => policy,
        Err(error) => {
            return Outcome::internal(COMMAND, format!("shipped keel.yaml unreadable: {error}"));
        }
    };
    let report = policy_keel::check(
        &diff,
        card.as_ref().map(|(card, bytes)| (card, *bytes)),
        &policy,
    );
    let proof = match prove {
        None => None,
        Some(args) => {
            let scratch_root = std::env::temp_dir();
            let target_dir = args
                .target_dir
                .or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from))
                .unwrap_or_else(|| scratch_root.join("graphhelm-keel-prove-target"));
            let options = ProveOptions {
                repo: repo.to_path_buf(),
                base: merge_base.clone(),
                head: head.to_owned(),
                target_dir,
                scratch_root,
                timeout: Duration::from_secs(args.timeout_secs),
                command: args.command,
            };
            match keel_prove::prove_new_tests(&diff, &options, graphhelm_process_tree::run_bounded)
            {
                Ok(proof) => Some(proof),
                Err(error) => {
                    return input_error(format!("--prove-new-tests: {error}"), "/diff");
                }
            }
        }
    };
    let diagnostics: Vec<Diagnostic> = report
        .findings
        .iter()
        .chain(proof.iter().flat_map(|proof| proof.findings.iter()))
        .map(|finding| {
            let path = finding.path.as_deref().unwrap_or("/");
            let message = format!("{}: {}", finding.rule, finding.detail);
            if finding.blocking {
                Diagnostic::error(finding.rule.clone(), message, path, "keel")
            } else {
                Diagnostic::warning(finding.rule.clone(), message, path, "keel")
            }
        })
        .collect();
    let refused = report.refused;
    let mut data = match serde_json::to_value(&report) {
        Ok(data) => data,
        Err(error) => return Outcome::internal(COMMAND, error.to_string()),
    };
    if let (Some(proof), Some(object)) = (proof, data.as_object_mut()) {
        match serde_json::to_value(proof) {
            Ok(value) => {
                object.insert("testProof".into(), value);
            }
            Err(error) => return Outcome::internal(COMMAND, error.to_string()),
        }
    }
    Outcome {
        output: CommandOutput {
            ok: !refused,
            command: COMMAND,
            data: Some(data),
            diagnostics,
        },
        exit_code: if refused { 2 } else { 0 },
    }
}

fn read_card(path: &Path) -> Result<(Card, u64), Box<Outcome>> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| Box::new(input_error(format!("card unreadable: {error}"), "/card")))?;
    if metadata.len() > MAX_CARD_READ_BYTES {
        return Err(Box::new(input_error(
            format!("card is {} bytes; not read", metadata.len()),
            "/card",
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| Box::new(input_error(format!("card unreadable: {error}"), "/card")))?;
    let is_markdown = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    let (value, bytes) = if is_markdown {
        let value = card_from_markdown(&String::from_utf8_lossy(&bytes));
        let missing: Vec<&str> = ["promise", "scopePaths", "proof"]
            .into_iter()
            .filter(|key| value.get(key).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(Box::new(input_error(
                format!(
                    "markdown card lacks {}; write `Promise:`, `Scope:` and `Proof:` lines",
                    missing.join(", ")
                ),
                "/card",
            )));
        }
        let json = serde_json::to_vec(&value).unwrap_or_default();
        (value, json)
    } else {
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            Box::new(input_error(
                format!("card is not valid JSON: {error}"),
                "/card",
            ))
        })?;
        (value, bytes)
    };
    let schema: serde_json::Value = serde_json::from_str(KEEL_CARD_SCHEMA).map_err(|error| {
        Box::new(Outcome::internal(
            COMMAND,
            format!("shipped keel-card.schema.json unreadable: {error}"),
        ))
    })?;
    let diagnostics = graphhelm_schema::validate_inline_value(&schema, &value, "keel-card")
        .map_err(|error| {
            Box::new(input_error(
                format!("card schema validation failed: {error}"),
                "/card",
            ))
        })?;
    if let Some(diagnostic) = diagnostics.into_iter().next() {
        return Err(Box::new(Outcome::application(COMMAND, diagnostic)));
    }
    let card: Card = serde_json::from_value(value).map_err(|error| {
        Box::new(input_error(
            format!("card is not a keel card (keel-card.schema.json): {error}"),
            "/card",
        ))
    })?;
    Ok((card, bytes.len() as u64))
}

/// Reads a card written as prose in a PR body (`.md`): the lines `Promise:`, `Scope:`, `Proof:`
/// and optionally `Exported:`, each possibly a list item or bold. List fields take the backticked
/// items on the line, else its comma-separated words; `Proof` drops one pair of backticks. A field
/// that is missing stays missing, so the schema names it.
fn card_from_markdown(text: &str) -> serde_json::Value {
    let mut card = serde_json::Map::new();
    for line in text.lines() {
        let line = line
            .trim()
            .trim_start_matches(['-', '*', '+', ' '])
            .replace("**", "");
        let Some((label, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        let list = || {
            let ticked: Vec<&str> = rest.split('`').skip(1).step_by(2).collect();
            let items: Vec<String> = if ticked.is_empty() {
                rest.split(',').map(str::trim).map(str::to_owned).collect()
            } else {
                ticked.into_iter().map(str::to_owned).collect()
            };
            serde_json::Value::from(
                items
                    .into_iter()
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>(),
            )
        };
        let (key, value) = match label.trim().to_ascii_lowercase().as_str() {
            "promise" => ("promise", serde_json::Value::from(rest)),
            "scope" | "scope paths" | "paths" => ("scopePaths", list()),
            "proof" => (
                "proof",
                serde_json::Value::from(
                    rest.strip_prefix('`')
                        .and_then(|r| r.strip_suffix('`'))
                        .unwrap_or(rest),
                ),
            ),
            "exported" | "exported symbols" => ("exportedSymbols", list()),
            "journeys" => ("journeys", list()),
            _ => continue,
        };
        card.entry(key).or_insert(value);
    }
    serde_json::Value::Object(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn bounded_proof_kills_a_descendant_that_holds_an_output_pipe() {
        let scratch = tempfile::tempdir().unwrap();
        let pid_path = scratch.path().join("descendant.pid");
        let mut command = if cfg!(windows) {
            let mut command = Command::new("powershell");
            command.args([
                "-NoProfile",
                "-Command",
                "$i=New-Object Diagnostics.ProcessStartInfo; $i.FileName='ping'; $i.Arguments='-n 30 127.0.0.1'; $i.UseShellExecute=$false; $p=[Diagnostics.Process]::Start($i); Set-Content -LiteralPath $env:GRAPHHELM_TEST_PID_FILE -Value $p.Id",
            ]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args([
                "-c",
                "sleep 30 & echo $! > \"$GRAPHHELM_TEST_PID_FILE\"; exit 0",
            ]);
            command
        };
        command
            .env("GRAPHHELM_TEST_PID_FILE", &pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let worker = thread::spawn(move || {
            graphhelm_process_tree::run_bounded(command, Duration::from_secs(2))
        });
        let observation_deadline = Instant::now() + Duration::from_secs(5);
        let mut identity = None;
        while identity.is_none() && !worker.is_finished() && Instant::now() < observation_deadline {
            identity = std::fs::read_to_string(&pid_path)
                .ok()
                .and_then(|contents| contents.trim().parse().ok())
                .and_then(|pid| graphhelm_process_tree::ProcessIdentity::capture(pid).ok());
            if identity.is_none() {
                thread::sleep(Duration::from_millis(10));
            }
        }
        let result = worker.join().unwrap().unwrap();

        assert!(result.is_none(), "inherited pipe bypassed the deadline");
        let identity =
            identity.expect("descendant identity was not observed before bounded cleanup");
        assert!(
            identity.wait_until_gone(Duration::from_secs(1)).unwrap(),
            "timed out process tree still has a live descendant"
        );
    }
}
