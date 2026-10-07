//! `graphhelm journey capture` / `graphhelm journey walked` (#315, spec §6.3): the producers of
//! the two journey records. Both are ordinary sealed signals recorded through the shared signal
//! core (`execution::signal::execute`) as the owner; this module only builds valid documents
//! (Ruling 3) and refuses everything else before the store is touched.
//!
//! Every id from the command line is checked against Ruling 1 before any filesystem, git or
//! store access, so a path-like id never reaches a path.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use graphhelm_execution::{ContractInput, valid_journey_id, valid_revision};

use super::execution::{
    self, Failure, argument, attachments, idempotency_key, owner_actor, signal::SignalKeyring,
};
use super::journey_validate;
use super::journeys::{
    CAPTURE_KIND, CAPTURE_PROTOCOL, TRANSITION_KIND, TRANSITION_PROTOCOL, contract, records,
};
use crate::args::JourneyWalkedArgs;
use crate::args::{JourneyArgs, JourneyCaptureArgs, JourneyCommand, JourneyRecordArgs};
use crate::output::Outcome;

const CAPTURE_COMMAND: &str = "journey.capture";
const WALKED_COMMAND: &str = "journey.walked";
const MAX_VIEWPORT: u32 = 16384;

pub fn run(args: &JourneyArgs) -> Outcome {
    match &args.command {
        JourneyCommand::Capture(capture) => {
            execution::finish(CAPTURE_COMMAND, run_capture(capture), recorded)
        }
        JourneyCommand::Walked(walked) => {
            execution::finish(WALKED_COMMAND, run_walked(walked), recorded)
        }
        JourneyCommand::Validate(validate) => journey_validate::run(validate),
        JourneyCommand::Compile(compile) => super::journey_flow::run_compile(compile),
        JourneyCommand::Approve(approve) => super::journey_flow::run_approve(approve),
        JourneyCommand::Replay(replay) => super::journey_replay::run(replay),
    }
}

/// #319: a journey record is evidence and never proposes a graph change, so the shared signal
/// reply always carries the Governor's `decision: "rejected"` with `signal_not_actionable`
/// (`core/governor/src/inflight.rs` `decide_mutation`). That is the normal verdict for every
/// evidence-only signal, not a refusal: the record is appended and folds. `outcome` says so in
/// plain words; the Governor's fields stay as they are.
fn recorded(mut value: serde_json::Value) -> serde_json::Value {
    value["outcome"] = "recorded".into();
    value
}

fn id(value: &str, pointer: &str) -> Result<(), Failure> {
    if valid_journey_id(value) {
        Ok(())
    } else {
        Err(argument(
            "a journey id must match ^[a-z0-9][a-z0-9._-]{0,127}$ and must not contain \"..\"",
            pointer,
        ))
    }
}

fn project(record: &JourneyRecordArgs) -> PathBuf {
    record.project.clone().unwrap_or_else(|| PathBuf::from("."))
}

fn keyring(record: &JourneyRecordArgs) -> SignalKeyring {
    SignalKeyring {
        directory: record.keyring.clone(),
        key_id: record.key_id.clone(),
    }
}

/// The contract `<project>/.graphhelm/journeys/<id>.json`, read with the same rules as
/// `graphhelm journeys` (Ruling 8). `id` has already passed Ruling 1.
fn load_contract(project: &Path, id: &str) -> Result<ContractInput, Failure> {
    let path = project
        .join(".graphhelm")
        .join("journeys")
        .join(format!("{id}.json"));
    contract(&path, id).map_err(|reason| {
        argument(
            &format!("the journey contract could not be used ({reason})"),
            "/contract",
        )
    })
}

fn step_index(contract: &ContractInput, step: &str, pointer: &str) -> Result<usize, Failure> {
    contract
        .steps
        .iter()
        .position(|candidate| candidate.step_id == step)
        .ok_or_else(|| argument("the contract has no step with this id", pointer))
}

/// One git read with a fixed argument list (Ruling 5's invocation rules).
fn git(project: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

pub(crate) fn head(project: &Path) -> Result<String, Failure> {
    git(project, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map(|text| text.trim().to_owned())
        .filter(|revision| valid_revision(revision))
        .ok_or_else(|| {
            argument(
                "the project's git revision could not be read; git must be installed and the \
                 project must be a repository with a commit",
                "/project",
            )
        })
}

fn dirty(project: &Path) -> Result<bool, Failure> {
    git(project, &["status", "--porcelain"])
        .map(|text| !text.trim().is_empty())
        .ok_or_else(|| argument("the project's git status could not be read", "/project"))
}

fn parse_viewport(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    let parse = |part: &str| {
        part.parse::<u32>()
            .ok()
            .filter(|value| (1..=MAX_VIEWPORT).contains(value))
    };
    Some((parse(width)?, parse(height)?))
}

/// Width and height from a PNG's IHDR chunk, which the format requires to come first.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let read = |range: std::ops::Range<usize>| {
        u32::from_be_bytes(bytes[range].try_into().expect("four bytes"))
    };
    let (width, height) = (read(16..20), read(20..24));
    let ok = |value: u32| (1..=MAX_VIEWPORT).contains(&value);
    (ok(width) && ok(height)).then_some((width, height))
}

fn envelope(kind: &str, description: &serde_json::Value, evidence: String) -> Vec<u8> {
    let value = serde_json::json!({
        "id": idempotency_key("journey").as_str(),
        "type": kind,
        "source": {"type": "test", "id": "journey-observer"},
        "severity": "low",
        "description": description.to_string(),
        "evidence": [evidence],
        "emittedAt": chrono::Utc::now().to_rfc3339(),
    });
    serde_json::to_vec(&value).expect("a JSON value serializes")
}

fn run_capture(args: &JourneyCaptureArgs) -> Result<serde_json::Value, Failure> {
    let record = &args.record;
    id(&record.contract, "/contract")?;
    id(&args.step, "/step")?;
    if args.pr == Some(0) {
        return Err(argument("--pr must be at least 1", "/pr"));
    }
    if let Some(phase) = args.phase.as_deref()
        && phase != "before"
        && phase != "after"
    {
        return Err(argument("--phase must be before or after", "/phase"));
    }
    let given_viewport = match args.viewport.as_deref() {
        Some(text) => Some(parse_viewport(text).ok_or_else(|| {
            argument(
                "--viewport must be WIDTHxHEIGHT, each 1 to 16384",
                "/viewport",
            )
        })?),
        None => None,
    };
    let project = project(record);
    let contract = load_contract(&project, &record.contract)?;
    step_index(&contract, &args.step, "/step")?;
    let images = attachments::from_files(std::slice::from_ref(&args.image))?;
    let image = &images[0];
    let (width, height) = given_viewport
        .or_else(|| (image.media_type == "image/png").then(|| png_dimensions(&image.bytes))?)
        .ok_or_else(|| {
            argument(
                "--viewport is required unless the image is a PNG with a readable header",
                "/viewport",
            )
        })?;
    let revision = head(&project)?;
    let dirty = dirty(&project)?;
    let actor = owner_actor();
    let mut description = serde_json::json!({
        "protocol": CAPTURE_PROTOCOL,
        "contractId": record.contract,
        "stepId": args.step,
        "revision": revision,
        "dirty": dirty,
        "viewport": {"width": width, "height": height},
        "observer": actor.id().as_str(),
    });
    if let Some(pr) = args.pr {
        description["pr"] = pr.into();
    }
    if let Some(phase) = &args.phase {
        description["phase"] = phase.as_str().into();
    }
    let signal = envelope(
        CAPTURE_KIND,
        &description,
        format!(
            "journey capture {}/{} at {revision}",
            record.contract, args.step
        ),
    );
    execution::signal::execute(
        &record.events,
        Some(&record.execution),
        &signal,
        None,
        actor,
        idempotency_key("journey-capture-recorded"),
        Some(&keyring(record)),
        &images,
    )
}

fn run_walked(args: &JourneyWalkedArgs) -> Result<serde_json::Value, Failure> {
    let record = &args.record;
    id(&record.contract, "/contract")?;
    id(&args.from, "/from")?;
    id(&args.to, "/to")?;
    if args.from_capture.is_some() != args.to_capture.is_some() {
        return Err(argument(
            "--from-capture and --to-capture must be supplied together",
            "/fromCaptureId",
        ));
    }
    let project = project(record);
    let contract = load_contract(&project, &record.contract)?;
    let from = step_index(&contract, &args.from, "/from")?;
    let to = step_index(&contract, &args.to, "/to")?;
    if to != from + 1 {
        return Err(argument(
            "a walked transition joins two consecutive steps of the contract, --from first",
            "/to",
        ));
    }
    let keyring = keyring(record);
    let found = records(&record.events, &record.execution, &keyring)?;
    let newest = |step: &str, pointer: &str| {
        found
            .captures
            .iter()
            .filter(|capture| capture.contract_id == record.contract && capture.step_id == step)
            .max_by_key(|capture| capture.sequence)
            .map(|capture| capture.signal_id.clone())
            .ok_or_else(|| argument("this step has no capture in this run", pointer))
    };
    let selected = |given: &Option<String>, step: &str, pointer: &str| match given {
        None => newest(step, pointer),
        Some(signal_id) => found
            .captures
            .iter()
            .find(|capture| {
                capture.signal_id == *signal_id
                    && capture.execution_id == record.execution
                    && capture.contract_id == record.contract
                    && capture.step_id == step
            })
            .map(|capture| capture.signal_id.clone())
            .ok_or_else(|| {
                argument(
                    "the supplied capture does not identify this contract step in this run",
                    pointer,
                )
            }),
    };
    let from_capture = selected(&args.from_capture, &args.from, "/fromCaptureId")?;
    let to_capture = selected(&args.to_capture, &args.to, "/toCaptureId")?;
    let revision = head(&project)?;
    let actor = owner_actor();
    let description = serde_json::json!({
        "protocol": TRANSITION_PROTOCOL,
        "contractId": record.contract,
        "fromStepId": args.from,
        "toStepId": args.to,
        "revision": revision,
        "observer": actor.id().as_str(),
        "fromCaptureId": from_capture,
        "toCaptureId": to_capture,
    });
    let signal = envelope(
        TRANSITION_KIND,
        &description,
        format!(
            "journey transition {}/{} -> {} at {revision}",
            record.contract, args.from, args.to
        ),
    );
    execution::signal::execute(
        &record.events,
        Some(&record.execution),
        &signal,
        None,
        actor,
        idempotency_key("journey-walked-recorded"),
        Some(&keyring),
        &[],
    )
}
