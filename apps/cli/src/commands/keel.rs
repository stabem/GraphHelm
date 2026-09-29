use std::path::Path;
use std::process::Command;

use graphhelm_policy::keel::{self as policy_keel, Card, KeelPolicy};
use graphhelm_protocols::Diagnostic;

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel";

/// The shipped Keel rules, embedded so the verdict names the policy version the binary was built
/// with and never depends on where the package is installed. The same bytes are the
/// `policy/keel` contribution `extension.json` binds by digest.
const KEEL_POLICY: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/policies/keel.yaml"
);

/// A card is a few hundred bytes (`keel.yaml` `card.maxCardBytes`); this only stops an unbounded
/// read. A card between the two is read and reported as `keel.card.too_large`.
const MAX_CARD_READ_BYTES: u64 = 1024 * 1024;

const INPUT_INVALID: &str = crate::error_codes::GHCLI030_KEEL_CHECK_INPUT;

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

/// `graphhelm keel check --diff <base>..<head> [--card <card.json>] [--repo <dir>]` (#1330).
///
/// Exit 0 when nothing blocks (signals travel as warnings), 2 when a finding blocks (the report
/// still travels in `data`), 3 when the range, the repository or the card cannot be read.
pub(super) fn check(repo: &Path, range: &str, card_path: Option<&Path>) -> Outcome {
    if range.starts_with('-') || !range.contains("..") {
        return input_error("--diff takes a git range `<base>..<head>`", "/diff");
    }
    let card = match card_path.map(read_card).transpose() {
        Ok(card) => card,
        Err(outcome) => return *outcome,
    };
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
            range,
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
    let diagnostics: Vec<Diagnostic> = report
        .findings
        .iter()
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
    let data = match serde_json::to_value(&report) {
        Ok(data) => data,
        Err(error) => return Outcome::internal(COMMAND, error.to_string()),
    };
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
    let card: Card = serde_json::from_slice(&bytes).map_err(|error| {
        Box::new(input_error(
            format!("card is not a keel card (keel-card.schema.json): {error}"),
            "/card",
        ))
    })?;
    Ok((card, bytes.len() as u64))
}
