use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_execution::{
    ChangedSince, Freshness, GitHistory, ScopeHistory, UnknownCause, fold_journeys,
    valid_journey_id,
};
use graphhelm_policy::keel::{self as policy_keel, Card, Finding, JourneyScreen, KeelPolicy};
use graphhelm_policy::keel_plan;
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

/// `--events`, `--execution`, `--keyring`, `--key-id`: the run whose screen captures a card's
/// `journeys` are checked against (#321).
pub(super) struct JourneyRecords {
    pub(super) events: PathBuf,
    pub(super) execution: String,
    pub(super) keyring: PathBuf,
    pub(super) key_id: String,
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
    records: Option<JourneyRecords>,
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
    // A revision that starts with `-` would be read by git as an option, not a revision.
    if base.starts_with('-') || head.starts_with('-') {
        return input_error(
            "--diff takes a git range `<base>..<head>`; a revision may not start with '-'",
            "/diff",
        );
    }
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
    let mut report = report;
    let changed: Vec<String> = report
        .changed_paths
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    if policy.journey_first {
        report.findings.extend(missing_journeys(
            repo,
            card.as_ref().map(|(card, _)| card),
            &changed,
        ));
    }
    if let Some((card, _)) = card.as_ref().filter(|(card, _)| !card.journeys.is_empty()) {
        match journey_findings(repo, head, card, records.as_ref(), &changed) {
            Ok(findings) => report.findings.extend(findings),
            Err(outcome) => return *outcome,
        }
    }
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

/// A history pinned to the range head: the working tree may be checked out elsewhere.
struct AtHead {
    git: GitHistory,
    head: String,
}

impl ScopeHistory for AtHead {
    fn head(&self) -> Option<String> {
        Some(self.head.clone())
    }

    fn changed_since(&self, revision: &str, head: &str) -> ChangedSince {
        self.git.changed_since(revision, head)
    }
}

/// Spec #382 §3, advisory: every compiled journey whose screen a changed path touches must be
/// named in the card's `journeys`; one `keel.journey.card_missing_journey` per journey that is not
/// (or per touched journey when there is no card), naming the touching path and step.
fn missing_journeys(repo: &Path, card: Option<&Card>, changed: &[String]) -> Vec<Finding> {
    let (contracts, _) = super::journeys::contracts(repo);
    contracts
        .iter()
        .filter(|contract| card.is_none_or(|card| !card.journeys.contains(&contract.contract_id)))
        .filter_map(|contract| {
            contract.steps.iter().find_map(|step| {
                let screen = step.screen.as_ref()?;
                let path = changed.iter().find(|path| {
                    screen
                        .scope_paths
                        .iter()
                        .any(|scope| policy_keel::paths_touch(path, scope))
                })?;
                Some(Finding {
                    rule: "keel.journey.card_missing_journey".to_owned(),
                    path: Some(path.clone()),
                    detail: format!(
                        "{}/{}: this change touches the screen; name `{}` in the card's journeys \
                         and replay it at the head",
                        contract.contract_id, step.step_id, contract.contract_id
                    ),
                    blocking: false,
                })
            })
        })
        .collect()
}

/// The steps of flow `<id>.journey.yaml` that a recorded drift touches: the `from` and `to`
/// screens of every edge a `drift` entry names. Empty when there is no flow or it does not parse.
fn drifted_steps(repo: &Path, id: &str) -> Vec<(String, String)> {
    let file = repo
        .join(".graphhelm")
        .join("journeys")
        .join(format!("{id}.journey.yaml"));
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(flow) = serde_yaml_ng::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let empty = Vec::new();
    let edges = flow["edges"].as_array().unwrap_or(&empty);
    flow["drift"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|drift| {
            let edge_id = drift["edge"].as_str()?;
            let edge = edges.iter().find(|edge| edge["id"] == edge_id)?;
            let code = drift["code"].as_str().unwrap_or("drift");
            Some(
                [edge["from"].as_str(), edge["to"].as_str()]
                    .into_iter()
                    .flatten()
                    .map(move |step| (step.to_owned(), format!("{code} on edge {edge_id}"))),
            )
        })
        .flatten()
        .collect()
}

/// Spec §6.3, advisory: the card's journeys folded at the range head; one warning per touched
/// screen whose newest capture is not fresh there, and one per contract that cannot be read.
/// Since 1.4.0 (#382) also one `keel.journey.replay_not_green` per step a changed path touches
/// that has no capture taken at the head, or that a recorded drift of its flow touches.
fn journey_findings(
    repo: &Path,
    head: &str,
    card: &Card,
    records: Option<&JourneyRecords>,
    changed: &[String],
) -> Result<Vec<Finding>, Box<Outcome>> {
    let object = format!("{head}^{{commit}}");
    let head_sha = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet", &object])
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        Ok(_) => {
            return Err(Box::new(input_error(
                format!("git rev-parse {head} did not name a commit"),
                "/diff",
            )));
        }
        Err(error) => {
            return Err(Box::new(input_error(
                format!("git did not start: {error}"),
                "/repo",
            )));
        }
    };
    let mut findings = Vec::new();
    let mut contracts = Vec::new();
    let directory = repo.join(".graphhelm").join("journeys");
    for id in &card.journeys {
        let read = if valid_journey_id(id) {
            super::journeys::contract(&directory.join(format!("{id}.json")), id)
        } else {
            Err("invalid_journey_id")
        };
        match read {
            Ok(contract) => contracts.push(contract),
            Err(reason) => findings.push(Finding {
                rule: "keel.journey.contract_unreadable".to_owned(),
                path: None,
                detail: format!("{id}: {reason}"),
                blocking: false,
            }),
        }
    }
    let (captures, transitions) = match records {
        Some(records) => {
            let keyring = super::execution::signal::SignalKeyring {
                directory: records.keyring.clone(),
                key_id: records.key_id.clone(),
            };
            match super::journeys::records(&records.events, &records.execution, &keyring) {
                Ok(read) => (read.captures, read.transitions),
                Err(failure) => return Err(Box::new(failure.into_outcome(COMMAND))),
            }
        }
        None => (Vec::new(), Vec::new()),
    };
    let history = AtHead {
        git: GitHistory::new(repo),
        head: head_sha.clone(),
    };
    let view = fold_journeys(&contracts, &captures, &transitions, &history);
    let mut screens = Vec::new();
    for journey in view.journeys {
        let drifted = drifted_steps(repo, &journey.contract_id);
        for step in journey.steps {
            let Some(screen) = step.screen else {
                continue;
            };
            let touching = changed.iter().find(|path| {
                screen
                    .scope_paths
                    .iter()
                    .any(|scope| policy_keel::paths_touch(path, scope))
            });
            if let Some(path) = touching {
                let at_head = step
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.revision == head_sha && !capture.dirty);
                let mut reasons: Vec<String> = drifted
                    .iter()
                    .filter(|(drifted_step, _)| *drifted_step == step.step_id)
                    .map(|(_, reason)| reason.clone())
                    .collect();
                if !at_head {
                    reasons.insert(0, "no clean capture taken at the head".to_owned());
                }
                if !reasons.is_empty() {
                    findings.push(Finding {
                        rule: "keel.journey.replay_not_green".to_owned(),
                        path: Some(path.clone()),
                        detail: format!(
                            "{}/{}: {}; run `graphhelm journey replay` at the head",
                            journey.contract_id,
                            step.step_id,
                            reasons.join("; ")
                        ),
                        blocking: false,
                    });
                }
            }
            let not_fresh = match step.capture {
                None => Some(if records.is_some() {
                    "no capture of this step was read".to_owned()
                } else {
                    "no capture of this step was read (pass --events, --execution, --keyring \
                     and --key-id to read the run's captures)"
                        .to_owned()
                }),
                Some(capture) => match capture.freshness {
                    Freshness::Fresh => None,
                    Freshness::Stale => Some(format!(
                        "code changed after the capture at {}: {}",
                        &capture.revision[..8.min(capture.revision.len())],
                        capture.changed_files.join(", ")
                    )),
                    Freshness::Unknown => Some(format!(
                        "freshness unknown ({})",
                        match capture.unknown_cause {
                            Some(UnknownCause::Dirty) => "taken from uncommitted code",
                            Some(UnknownCause::NoScopePaths) => "screen has no scope paths",
                            Some(UnknownCause::RevisionMissing) => {
                                "revision not in the repository"
                            }
                            Some(UnknownCause::NoGit) | None => "no git history",
                        }
                    )),
                },
            };
            screens.push(JourneyScreen {
                contract_id: journey.contract_id.clone(),
                step_id: step.step_id,
                scope_paths: screen.scope_paths,
                not_fresh,
            });
        }
    }
    findings.extend(policy_keel::check_journeys(&card.scope_paths, &screens));
    Ok(findings)
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

/// The `keel.plan` signal kind and its description protocol (#382 phase B).
pub(crate) const PLAN_KIND: &str = "keel.plan";
const PLAN_COMMAND: &str = "keel.plan";

/// The repository's HEAD commit, or an empty revision outside git.
fn head_of(repo: &Path) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The plan a task's paths get under the shipped policy, as JSON (`graphhelm-task-plan-v1`).
pub(crate) fn plan_value(
    repo: &Path,
    task: &str,
    paths: &[String],
    promise: &str,
) -> Result<serde_json::Value, Outcome> {
    if !valid_journey_id(task) {
        return Err(input_error(
            "--task must match ^[a-z0-9][a-z0-9._-]{0,127}$ without `..`",
            "/task",
        ));
    }
    if let Some(bad) = paths.iter().position(|path| {
        path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path.split('/').any(|part| part == "..")
    }) {
        return Err(input_error(
            "paths are repository-relative, forward slashes, without `..`",
            &format!("/paths/{bad}"),
        ));
    }
    let policy: KeelPolicy = serde_yaml_ng::from_str(KEEL_POLICY).map_err(|error| {
        Outcome::internal(
            PLAN_COMMAND,
            format!("shipped keel.yaml unreadable: {error}"),
        )
    })?;
    let Some(rules) = policy.plan.clone() else {
        return Err(Outcome::internal(
            PLAN_COMMAND,
            "shipped keel.yaml has no plan section",
        ));
    };
    let (contracts, _) = super::journeys::contracts(repo);
    let journeys = contracts
        .into_iter()
        .map(|contract| {
            let scopes = contract
                .steps
                .into_iter()
                .filter_map(|step| step.screen)
                .flat_map(|screen| screen.scope_paths)
                .collect();
            (contract.contract_id, scopes)
        })
        .collect();
    let input = keel_plan::PlanInput {
        task_id: task.to_owned(),
        revision: head_of(repo),
        paths: paths.to_vec(),
        promise: promise.to_owned(),
        journeys,
    };
    serde_json::to_value(keel_plan::plan(&input, &policy, &rules))
        .map_err(|error| Outcome::internal(PLAN_COMMAND, error.to_string()))
}

/// `graphhelm keel plan`: print the plan; with records, also record it as one `keel.plan` signal.
pub fn plan(
    repo: &Path,
    task: &str,
    paths: &[String],
    promise: &str,
    records: Option<JourneyRecords>,
) -> Outcome {
    let value = match plan_value(repo, task, paths, promise) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let Some(records) = records else {
        return Outcome::success(PLAN_COMMAND, serde_json::json!({"plan": value}));
    };
    let signal = serde_json::json!({
        "id": super::execution::idempotency_key("keel-plan").as_str(),
        "type": PLAN_KIND,
        "source": {"type": "test", "id": "keel-planner"},
        "severity": "low",
        "description": value.to_string(),
        "evidence": [format!("graphhelm keel plan --task {task}")],
        "emittedAt": chrono::Utc::now().to_rfc3339(),
    });
    let keyring = super::execution::signal::SignalKeyring {
        directory: records.keyring.clone(),
        key_id: records.key_id.clone(),
    };
    match super::execution::signal::execute(
        &records.events,
        Some(&records.execution),
        &serde_json::to_vec(&signal).expect("a JSON value serializes"),
        None,
        super::execution::owner_actor(),
        super::execution::idempotency_key("keel-plan-recorded"),
        Some(&keyring),
        &[],
    ) {
        Ok(recorded) => Outcome::success(
            PLAN_COMMAND,
            serde_json::json!({"plan": value, "recorded": recorded}),
        ),
        Err(failure) => failure.into_outcome(PLAN_COMMAND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::thread;

    #[test]
    fn bounded_proof_kills_a_descendant_that_holds_an_output_pipe() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};

        const TEST: &str =
            "commands::keel::tests::bounded_proof_kills_a_descendant_that_holds_an_output_pipe";
        if let Ok(address) = std::env::var("GRAPHHELM_TEST_HOLDER_ADDRESS") {
            if std::env::var_os("GRAPHHELM_TEST_PIPE_HOLDER").is_some() {
                let mut connection = TcpStream::connect(address).unwrap();
                connection.write_all(&[1]).unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .unwrap();
                // Keep this descendant and its inherited stdout alive until tree cleanup.
                let _ = connection.read(&mut [0]);
            } else {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--exact", TEST, "--nocapture"])
                    .env("GRAPHHELM_TEST_PIPE_HOLDER", "1");
                // The wrapper must exit first; the outer observer owns descendant cleanup.
                #[allow(clippy::zombie_processes)]
                let _descendant = command.spawn().unwrap();
            }
            return;
        }

        // Native wrappers avoid cold PowerShell startup inside the execution bound. The socket
        // retains readiness followed by EOF even when this observer is scheduled after cleanup;
        // sampling a PID file before the kill lost that evidence under competing suite load.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (observed, observation) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ready = [0];
            let result = connection
                .read_exact(&mut ready)
                .and_then(|()| {
                    connection.read(&mut [0]).or_else(|error| {
                        // TerminateProcess closes a Windows socket with a reset rather than EOF.
                        if error.kind() == std::io::ErrorKind::ConnectionReset {
                            Ok(0)
                        } else {
                            Err(error)
                        }
                    })
                })
                .map(|bytes| (ready, bytes));
            let _ = observed.send(result);
        });
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture"])
            .env("GRAPHHELM_TEST_HOLDER_ADDRESS", address.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let result = graphhelm_process_tree::run_bounded(command, Duration::from_secs(2)).unwrap();

        assert!(result.is_none(), "inherited pipe bypassed the deadline");
        assert_eq!(
            observation
                .recv_timeout(Duration::from_secs(5))
                .expect("the descendant must report readiness and disconnect after cleanup")
                .expect("the descendant connection must close after process-tree cleanup"),
            ([1], 0),
            "the real pipe holder must start and then exit during bounded cleanup"
        );
    }
}
