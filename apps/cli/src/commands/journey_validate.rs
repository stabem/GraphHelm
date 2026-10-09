//! `graphhelm journey validate` (#328): check journey contracts before anything records against
//! them. A contract that `graphhelm journeys` would refuse, or that names a screen file that does
//! not exist, is caught here with the reason, instead of silently missing from the Studio map.
//!
//! Checks, per file: it is JSON within the size limit; it matches the journey-contract schema;
//! the file is named `<contractId>.json`; the contract, step and screen ids follow the journey id
//! rule (`valid_journey_id`, stricter than the schema's id pattern); step ids are unique; every
//! step's `actorId` names an actor; every promise names a step; a screen id used by two steps
//! carries the same title and `scopePaths`; every `scopePaths` entry exists under the project.
//!
//! Exit 0 when every file is clean, 2 when any file has a finding, 3 when the input itself is
//! unusable (no files, a file that cannot be read, `--all` with no journeys directory).
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use graphhelm_execution::valid_journey_id;
use graphhelm_protocols::Diagnostic;
use serde_json::{Value, json};

use super::journeys::{CONTRACT_SCHEMA_ID, MAX_CONTRACT_BYTES, contract_schemas};
use crate::args::JourneyValidateArgs;
use crate::error_codes::{GHCLI001_ARGUMENT_INVALID, GHCLI033_JOURNEY_CONTRACT_INVALID};
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "journey.validate";

pub(crate) struct Finding {
    pub(crate) code: &'static str,
    pub(crate) pointer: String,
    pub(crate) message: String,
}

impl Finding {
    pub(crate) fn is_warning(&self) -> bool {
        // A stale safe mark (#518) is void, not wrong: the guard simply applies again.
        // #534: no owner store to check (a fresh clone, CI) is said, not counted as a forgery;
        // a store without the owner's record (`flow.approval_unsigned`) is an error.
        matches!(
            self.code,
            "flow.unreachable_screen"
                | "flow.safe_stale"
                | "flow.approval_unverifiable"
                // #534 slice 2: an unsigned or unverifiable safe mark is void, not wrong.
                | "flow.safe_unsigned"
                | "flow.safe_unverifiable"
        )
    }
    pub(crate) fn new(
        code: &'static str,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            pointer: pointer.into(),
            message: message.into(),
        }
    }

    fn json(&self) -> Value {
        json!({"code": self.code, "pointer": self.pointer, "message": self.message})
    }
}

fn input_error(message: impl Into<String>, pointer: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(GHCLI001_ARGUMENT_INVALID, message, pointer, "graphhelm"),
    )
}

pub fn run(args: &JourneyValidateArgs) -> Outcome {
    let project = args.project.clone().unwrap_or_else(|| PathBuf::from("."));
    if !project.is_dir() {
        return input_error("--project is not a directory", "/project");
    }
    let mut files = args.files.clone();
    if args.all {
        let directory = project.join(".graphhelm").join("journeys");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return input_error(
                "--all found no <project>/.graphhelm/journeys directory",
                "/all",
            );
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "json")
                    || path.to_string_lossy().ends_with(".journey.yaml")
            })
            .collect();
        found.sort();
        files.extend(found);
    }
    if files.is_empty() {
        return input_error("name at least one contract file, or pass --all", "/files");
    }
    let mut reports = Vec::new();
    let mut diagnostics = Vec::new();
    for file in &files {
        let findings = if file.to_string_lossy().ends_with(".journey.yaml") {
            super::journey_flow::check(file, &project)
        } else {
            match read(file) {
                Ok(value) => check(file, &value, &project),
                Err(Some(message)) => vec![Finding::new("not_json", "", message)],
                Err(None) => {
                    return input_error(
                        format!("{} could not be read as a file", file.display()),
                        "/files",
                    );
                }
            }
        };
        let source = file.display().to_string();
        for finding in &findings {
            let diagnostic = if finding.is_warning() {
                Diagnostic::warning
            } else {
                Diagnostic::error
            };
            diagnostics.push(diagnostic(
                if finding.code.starts_with("flow.") {
                    crate::error_codes::GHCLI034_JOURNEY_FLOW_INVALID
                } else {
                    GHCLI033_JOURNEY_CONTRACT_INVALID
                },
                format!("{}: {}", finding.code, finding.message),
                finding.pointer.clone(),
                source.clone(),
            ));
        }
        reports.push(json!({
            "file": source,
            "ok": findings.iter().all(Finding::is_warning),
            "findings": findings.iter().map(Finding::json).collect::<Vec<_>>(),
        }));
    }
    let clean = diagnostics
        .iter()
        .all(|d| d.severity == graphhelm_protocols::Severity::Warning);
    Outcome {
        output: CommandOutput {
            ok: clean,
            command: COMMAND,
            data: Some(json!({
                "project": project.display().to_string(),
                "checked": reports.len(),
                "findings": diagnostics.len(),
                "files": reports,
            })),
            diagnostics,
        },
        exit_code: if clean { 0 } else { 2 },
    }
}

/// `Err(None)`: not a readable regular file (input error). `Err(Some)`: read, but not usable JSON.
fn read(file: &Path) -> Result<Value, Option<String>> {
    let metadata = std::fs::metadata(file).map_err(|_| None)?;
    if !metadata.is_file() {
        return Err(None);
    }
    if metadata.len() > MAX_CONTRACT_BYTES {
        return Err(Some(format!(
            "the file is larger than {MAX_CONTRACT_BYTES} bytes"
        )));
    }
    let bytes = std::fs::read(file).map_err(|_| None)?;
    serde_json::from_slice(&bytes).map_err(|error| Some(format!("not JSON: {error}")))
}

fn check(file: &Path, value: &Value, project: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(schemas) = contract_schemas() else {
        findings.push(Finding::new(
            "schema_unavailable",
            "",
            "the bundled journey-contract schema did not compile",
        ));
        return findings;
    };
    for diagnostic in schemas.validate(CONTRACT_SCHEMA_ID, value, "journey-contract") {
        findings.push(Finding::new(
            "schema_invalid",
            diagnostic.path,
            diagnostic.message,
        ));
    }
    let text = |pointer: &str| value.pointer(pointer).and_then(Value::as_str);
    let contract_id = text("/contractId").unwrap_or_default();
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if !contract_id.is_empty() && stem != contract_id {
        findings.push(Finding::new(
            "contract_id_mismatch",
            "/contractId",
            format!("the file must be named {contract_id}.json; journeys reads it by that name"),
        ));
    }
    let mut id_rule = |id: &str, pointer: String| {
        if !valid_journey_id(id) {
            findings.push(Finding::new(
                "invalid_id",
                pointer,
                format!(
                    "{id:?} breaks the journey id rule ^[a-z0-9][a-z0-9._-]{{0,127}}$ with no \"..\""
                ),
            ));
        }
    };
    if !contract_id.is_empty() {
        id_rule(contract_id, "/contractId".to_owned());
    }
    let empty = Vec::new();
    let steps = value
        .get("steps")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for (index, step) in steps.iter().enumerate() {
        if let Some(id) = step.get("stepId").and_then(Value::as_str) {
            id_rule(id, format!("/steps/{index}/stepId"));
        }
        if let Some(id) = step.pointer("/screen/screenId").and_then(Value::as_str) {
            id_rule(id, format!("/steps/{index}/screen/screenId"));
        }
    }
    let actors: BTreeSet<&str> = value
        .get("actors")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .filter_map(|actor| actor.get("actorId").and_then(Value::as_str))
        .collect();
    let mut step_ids = BTreeSet::new();
    let mut screens: BTreeMap<&str, (usize, &Value)> = BTreeMap::new();
    for (index, step) in steps.iter().enumerate() {
        if let Some(id) = step.get("stepId").and_then(Value::as_str)
            && !step_ids.insert(id)
        {
            findings.push(Finding::new(
                "duplicate_step",
                format!("/steps/{index}/stepId"),
                format!("step {id:?} appears more than once; steps are walked in order"),
            ));
        }
        if let Some(actor) = step.get("actorId").and_then(Value::as_str)
            && !actors.contains(actor)
        {
            findings.push(Finding::new(
                "unknown_actor",
                format!("/steps/{index}/actorId"),
                format!("actor {actor:?} is not in actors"),
            ));
        }
        let Some(screen) = step.get("screen") else {
            continue;
        };
        if let Some(screen_id) = screen.get("screenId").and_then(Value::as_str) {
            match screens.get(screen_id) {
                Some((first, seen))
                    if seen.get("title") != screen.get("title")
                        || seen.get("scopePaths") != screen.get("scopePaths") =>
                {
                    findings.push(Finding::new(
                        "screen_inconsistent",
                        format!("/steps/{index}/screen"),
                        format!(
                            "screen {screen_id:?} differs from its first use at /steps/{first}/screen"
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    screens.insert(screen_id, (index, screen));
                }
            }
        }
        for (path_index, entry) in screen
            .get("scopePaths")
            .and_then(Value::as_array)
            .unwrap_or(&empty)
            .iter()
            .enumerate()
        {
            let Some(path) = entry.as_str() else { continue };
            let pointer = format!("/steps/{index}/screen/scopePaths/{path_index}");
            // Probe only a path that stays inside the project: every component a plain name or
            // `.`. `Path::join` discards the project for an absolute or drive path (`C:/x`), so
            // anything else is a finding and is never probed (#329 review).
            if path.contains('\\')
                || !Path::new(path)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
            {
                findings.push(Finding::new(
                    "scope_path_outside_project",
                    pointer,
                    format!(
                        "{path} is not a repository-relative path (no root, drive, `..` or backslash)"
                    ),
                ));
                continue;
            }
            if !project.join(path).exists() {
                findings.push(Finding::new(
                    "scope_path_missing",
                    pointer,
                    format!("{path} does not exist under the project"),
                ));
            }
        }
    }
    for (index, promise) in value
        .get("promises")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        if let Some(step) = promise.get("stepId").and_then(Value::as_str)
            && !step_ids.contains(step)
        {
            findings.push(Finding::new(
                "unknown_step",
                format!("/promises/{index}/stepId"),
                format!("promise names step {step:?}, which the contract does not have"),
            ));
        }
    }
    findings
}
