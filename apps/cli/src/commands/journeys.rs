//! `graphhelm journeys` (#315, spec §5.4): the proven-journey map of one run. Contracts come
//! from `<project>/.graphhelm/journeys/<contractId>.json`; captures and walked transitions are
//! ordinary signals (`jpd.screen_captured`, `jpd.transition_walked`) whose sealed envelopes are
//! opened with the keyring; `graphhelm_execution::fold_journeys` does the rest.
//!
//! `GET /v1/executions/{id}/journeys` and the MCP `journeys` tool call [`read`], so the three
//! surfaces return the same `data`.
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use graphhelm_events::{EvidenceOpener, EvidenceRead};
use graphhelm_execution::{
    CaptureRecord, ContractInput, GitHistory, ScreenInput, StepInput, TransitionRecord, Viewport,
    fold_journeys, valid_journey_id, valid_revision,
};
use graphhelm_protocols::{EventKind, EvidenceId};
use serde::Deserialize;

use super::event_store;
use super::execution::{self, Failure, signal::SignalKeyring};
use crate::output::Outcome;

pub(crate) const COMMAND: &str = "journeys.read";
pub(crate) const CAPTURE_KIND: &str = "jpd.screen_captured";
pub(crate) const TRANSITION_KIND: &str = "jpd.transition_walked";
pub(crate) const CAPTURE_PROTOCOL: &str = "graphhelm-screen-capture-v1";
pub(crate) const TRANSITION_PROTOCOL: &str = "graphhelm-transition-walked-v1";
const MAX_CONTRACT_BYTES: u64 = 1024 * 1024;
const CONTRACT_SCHEMA_ID: &str =
    "https://p50.dev/extensions/graphhelm-jpd/schemas/journey-contract.schema.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ViewportDocument {
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureDocument {
    protocol: String,
    contract_id: String,
    step_id: String,
    revision: String,
    dirty: bool,
    viewport: ViewportDocument,
    observer: String,
    pr: Option<u64>,
    phase: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransitionDocument {
    protocol: String,
    contract_id: String,
    from_step_id: String,
    to_step_id: String,
    revision: String,
    observer: String,
    from_capture_id: String,
    to_capture_id: String,
}

fn valid_observer(observer: &str) -> bool {
    (1..=128).contains(&observer.chars().count())
}

/// Ruling 2/3: a capture document, or `None` when it must be ignored.
fn capture(
    signal_id: &str,
    sequence: u64,
    image_evidence_id: &str,
    text: &str,
) -> Option<CaptureRecord> {
    let doc: CaptureDocument = serde_json::from_str(text).ok()?;
    let viewport_ok = |v: u32| (1..=16384).contains(&v);
    let ok = doc.protocol == CAPTURE_PROTOCOL
        && valid_journey_id(&doc.contract_id)
        && valid_journey_id(&doc.step_id)
        && valid_revision(&doc.revision)
        && viewport_ok(doc.viewport.width)
        && viewport_ok(doc.viewport.height)
        && valid_observer(&doc.observer)
        && doc.pr.is_none_or(|pr| pr >= 1)
        && doc
            .phase
            .as_deref()
            .is_none_or(|phase| phase == "before" || phase == "after");
    ok.then(|| CaptureRecord {
        signal_id: signal_id.to_owned(),
        sequence,
        image_evidence_id: image_evidence_id.to_owned(),
        contract_id: doc.contract_id,
        step_id: doc.step_id,
        revision: doc.revision,
        dirty: doc.dirty,
        viewport: Viewport {
            width: doc.viewport.width,
            height: doc.viewport.height,
        },
        observer: doc.observer,
        pr: doc.pr,
        phase: doc.phase,
    })
}

fn transition(signal_id: &str, sequence: u64, text: &str) -> Option<TransitionRecord> {
    let doc: TransitionDocument = serde_json::from_str(text).ok()?;
    let ok = doc.protocol == TRANSITION_PROTOCOL
        && valid_journey_id(&doc.contract_id)
        && valid_journey_id(&doc.from_step_id)
        && valid_journey_id(&doc.to_step_id)
        && valid_revision(&doc.revision)
        && valid_observer(&doc.observer)
        && !doc.from_capture_id.is_empty()
        && !doc.to_capture_id.is_empty();
    ok.then(|| TransitionRecord {
        signal_id: signal_id.to_owned(),
        sequence,
        contract_id: doc.contract_id,
        from_step_id: doc.from_step_id,
        to_step_id: doc.to_step_id,
        revision: doc.revision,
        observer: doc.observer,
        from_capture_id: doc.from_capture_id,
        to_capture_id: doc.to_capture_id,
    })
}

pub(crate) struct Records {
    pub(crate) captures: Vec<CaptureRecord>,
    pub(crate) transitions: Vec<TransitionRecord>,
    pub(crate) ignored: u64,
}

/// Replays the run and decodes every capture/transition signal. A record that cannot be opened
/// or decoded is counted, never folded.
pub(crate) fn records(
    events: &Path,
    execution: &str,
    keyring: &SignalKeyring,
) -> Result<Records, Failure> {
    let store = event_store(events).map_err(|e| execution::repository_failure(&e))?;
    let (scope, _, history) = execution::resolve_stream(&store, Some(execution))?;
    let opener = execution::signal::open_sealer(keyring)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| {
            execution::execution_state("the evidence reader could not start", "/keyring")
        })?;
    let mut out = Records {
        captures: Vec::new(),
        transitions: Vec::new(),
        ignored: 0,
    };
    for event in &history {
        let EventKind::SignalRecorded(record) = &event.kind else {
            continue;
        };
        let is_capture = record.kind == CAPTURE_KIND;
        if !is_capture && record.kind != TRANSITION_KIND {
            continue;
        }
        let signal_id = record.signal_id.as_str();
        let expected_refs = if is_capture { 2 } else { 1 };
        let envelope_id = format!("signal-{signal_id}");
        let decoded = (|| {
            if event.evidence_refs.len() != expected_refs
                || event.evidence_refs[0].evidence_id().as_str() != envelope_id
            {
                return None;
            }
            let id = EvidenceId::parse(&envelope_id).ok()?;
            let EvidenceRead::Available(sealed) = store.sealed_evidence(&scope, &id).ok()? else {
                return None;
            };
            let plaintext = runtime.block_on(opener.open(scope.clone(), &sealed)).ok()?;
            let value: serde_json::Value = plaintext
                .expose(|bytes| serde_json::from_slice(bytes))
                .ok()?;
            let text = value.get("description")?.as_str()?.to_owned();
            Some(text)
        })();
        let Some(text) = decoded else {
            out.ignored += 1;
            continue;
        };
        if is_capture {
            let image = event.evidence_refs[1].evidence_id().as_str();
            match capture(signal_id, event.sequence, image, &text) {
                Some(record) => out.captures.push(record),
                None => out.ignored += 1,
            }
        } else {
            match transition(signal_id, event.sequence, &text) {
                Some(record) => out.transitions.push(record),
                None => out.ignored += 1,
            }
        }
    }
    Ok(out)
}

fn contract_schemas() -> Option<&'static graphhelm_schema::OfflineSchemaSet> {
    static SCHEMAS: OnceLock<Option<graphhelm_schema::OfflineSchemaSet>> = OnceLock::new();
    SCHEMAS
        .get_or_init(|| {
            let document = serde_json::from_str(include_str!(
                "../../../../extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json"
            ))
            .ok()?;
            graphhelm_schema::OfflineSchemaSet::compile(BTreeMap::from([(
                CONTRACT_SCHEMA_ID.to_owned(),
                document,
            )]))
            .ok()
        })
        .as_ref()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractFile {
    contract_id: String,
    title: String,
    steps: Vec<StepFile>,
    #[serde(default)]
    promises: Vec<PromiseFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromiseFile {
    step_id: String,
    statement: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepFile {
    step_id: String,
    screen: Option<ScreenFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenFile {
    screen_id: String,
    title: String,
    #[serde(default)]
    scope_paths: Vec<String>,
}

/// One contract file, or the reason it is refused (Ruling 8).
pub(crate) fn contract(path: &Path, stem: &str) -> Result<ContractInput, &'static str> {
    if !valid_journey_id(stem) {
        return Err("invalid_file_name");
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "unreadable")?;
    if metadata.file_type().is_symlink() {
        return Err("symlink");
    }
    if !metadata.is_file() {
        return Err("not_a_file");
    }
    if metadata.len() > MAX_CONTRACT_BYTES {
        return Err("too_large");
    }
    let bytes = std::fs::read(path).map_err(|_| "unreadable")?;
    if bytes.len() as u64 > MAX_CONTRACT_BYTES {
        return Err("too_large");
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "not_json")?;
    let schemas = contract_schemas().ok_or("schema_unavailable")?;
    if !schemas
        .validate(CONTRACT_SCHEMA_ID, &value, "/journeys")
        .is_empty()
    {
        return Err("schema_invalid");
    }
    let file: ContractFile = serde_json::from_value(value).map_err(|_| "schema_invalid")?;
    if file.contract_id != stem {
        return Err("contract_id_mismatch");
    }
    if file.steps.iter().any(|step| {
        !valid_journey_id(&step.step_id)
            || step
                .screen
                .as_ref()
                .is_some_and(|screen| !valid_journey_id(&screen.screen_id))
    }) {
        return Err("invalid_id");
    }
    let promises = file.promises;
    Ok(ContractInput {
        contract_id: file.contract_id,
        title: file.title,
        steps: file
            .steps
            .into_iter()
            .map(|step| StepInput {
                promises: promises
                    .iter()
                    .filter(|promise| promise.step_id == step.step_id)
                    .map(|promise| promise.statement.clone())
                    .collect(),
                step_id: step.step_id,
                screen: step.screen.map(|screen| ScreenInput {
                    screen_id: screen.screen_id,
                    title: screen.title,
                    scope_paths: screen.scope_paths,
                }),
            })
            .collect(),
    })
}

/// Every `*.json` directly under `<project>/.graphhelm/journeys/`, sorted by file name.
fn contracts(project: &Path) -> (Vec<ContractInput>, Vec<serde_json::Value>) {
    let directory = project.join(".graphhelm").join("journeys");
    let mut accepted = Vec::new();
    let mut refused = Vec::new();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return (accepted, refused);
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    for name in names {
        let stem = &name[..name.len() - ".json".len()];
        match contract(&directory.join(&name), stem) {
            Ok(input) => accepted.push(input),
            Err(reason) => refused.push(serde_json::json!({"file": name, "reason": reason})),
        }
    }
    (accepted, refused)
}

/// The shared read behind the CLI, `GET /v1/executions/{id}/journeys` and MCP `journeys`.
pub(crate) fn read(
    events: &Path,
    execution: &str,
    project: &Path,
    keyring: &SignalKeyring,
) -> Result<serde_json::Value, Failure> {
    let records = records(events, execution, keyring)?;
    let (contracts, refused) = contracts(project);
    let view = fold_journeys(
        &contracts,
        &records.captures,
        &records.transitions,
        &GitHistory::new(project),
    );
    let mut value =
        serde_json::to_value(view).expect("a view built from serializable fold types serializes");
    value["refusedContracts"] = serde_json::Value::Array(refused);
    value["ignoredRecords"] = records.ignored.into();
    Ok(value)
}

pub fn run(
    events: &Path,
    execution: &str,
    project: &Path,
    keyring: &Path,
    key_id: &str,
) -> Outcome {
    let keyring = SignalKeyring {
        directory: keyring.to_path_buf(),
        key_id: key_id.to_owned(),
    };
    execution::finish(
        COMMAND,
        read(events, execution, project, &keyring),
        |value| value,
    )
}
