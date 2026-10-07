//! Bounded flow source files; frozen JSON contracts remain the reader boundary.
use super::journey_validate::Finding;
use crate::output::{CommandOutput, Outcome};
use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path};
use std::sync::OnceLock;

const FLOW_SCHEMA_ID: &str = "https://p50.dev/schemas/journey-flow.schema.json";
const MAX_FLOW_BYTES: u64 = 32 * 1024;

fn flow_schemas() -> Option<&'static OfflineSchemaSet> {
    static SCHEMAS: OnceLock<Option<OfflineSchemaSet>> = OnceLock::new();
    SCHEMAS
        .get_or_init(|| {
            let document =
                serde_json::from_str(include_str!("../../../../schemas/journey-flow.schema.json"))
                    .ok()?;
            OfflineSchemaSet::compile(BTreeMap::from([(FLOW_SCHEMA_ID.to_owned(), document)])).ok()
        })
        .as_ref()
}

fn read(file: &Path) -> Result<(String, Value), Vec<Finding>> {
    let metadata = std::fs::metadata(file)
        .map_err(|_| vec![Finding::new("flow.not_yaml", "", "flow could not be read")])?;
    if metadata.len() > MAX_FLOW_BYTES {
        return Err(vec![Finding::new(
            "flow.too_large",
            "",
            "flow exceeds 32 KiB",
        )]);
    }
    let mut text = String::new();
    if !metadata.is_file()
        || std::fs::File::open(file)
            .and_then(|f| f.take(MAX_FLOW_BYTES + 1).read_to_string(&mut text))
            .is_err()
    {
        return Err(vec![Finding::new(
            "flow.not_yaml",
            "",
            "flow is not readable UTF-8",
        )]);
    }
    if text.len() as u64 > MAX_FLOW_BYTES {
        return Err(vec![Finding::new(
            "flow.too_large",
            "",
            "flow exceeds 32 KiB",
        )]);
    }
    // Public parser events are private. Refuse reference tokens outside quoted scalars/comments
    // before parsing; the parser otherwise expands aliases before deserialization.
    if yaml_reference(&text) {
        return Err(vec![Finding::new(
            "flow.not_yaml",
            "",
            "anchors and aliases are not permitted",
        )]);
    }
    let yaml = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text).map_err(|_| {
        vec![Finding::new(
            "flow.not_yaml",
            "",
            "invalid YAML or duplicate mapping key",
        )]
    })?;
    let value = serde_json::to_value(yaml).map_err(|_| {
        vec![Finding::new(
            "flow.not_yaml",
            "",
            "flow requires string mapping keys",
        )]
    })?;
    let schemas = flow_schemas().ok_or_else(|| {
        vec![Finding::new(
            "flow.schema_invalid",
            "",
            "flow schema unavailable",
        )]
    })?;
    let findings: Vec<_> = schemas
        .validate(FLOW_SCHEMA_ID, &value, "journey-flow")
        .into_iter()
        .map(|d| Finding::new("flow.schema_invalid", d.path, d.message))
        .collect();
    if findings.is_empty() {
        Ok((text, value))
    } else {
        Err(findings)
    }
}

fn yaml_reference(text: &str) -> bool {
    let bytes = text.as_bytes();
    let (mut quote, mut comment, mut escaped) = (0, false, false);
    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' {
            comment = false;
        }
        if comment {
            continue;
        }
        if quote != 0 {
            if escaped {
                escaped = false;
                continue;
            }
            if quote == b'"' && byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == quote {
                quote = 0;
            }
            continue;
        }
        let boundary =
            i == 0 || bytes[i - 1].is_ascii_whitespace() || b"[{,:".contains(&bytes[i - 1]);
        if boundary && byte == b'#' {
            comment = true;
        }
        if boundary && matches!(byte, b'&' | b'*') {
            return true;
        }
        if boundary && matches!(byte, b'\'' | b'"') {
            quote = byte;
        }
    }
    false
}

fn local_base(base: &str) -> bool {
    let Some(rest) = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.ends_with(':') {
        return false;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix("[::1]") {
        let Some(port) = rest
            .strip_prefix(':')
            .or_else(|| rest.is_empty().then_some(""))
        else {
            return false;
        };
        ("[::1]", port)
    } else {
        authority.split_once(':').unwrap_or((authority, ""))
    };
    (port.is_empty() || port.parse::<u16>().is_ok_and(|p| p != 0))
        && !authority.contains(['@', '%', '\\'])
        && (matches!(host, "localhost" | "127.0.0.1" | "[::1]")
            || (!host.starts_with('.')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                && (host.ends_with(".localhost") || host.ends_with(".test"))))
}

fn semantic(file: &Path, value: &Value, project: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let id = value["id"].as_str().unwrap();
    if file.file_name().and_then(|n| n.to_str()) != Some(&format!("{id}.journey.yaml")) {
        findings.push(Finding::new(
            "flow.id_mismatch",
            "/id",
            "flow filename must match its id",
        ));
    }
    if !local_base(value["base"].as_str().unwrap()) {
        findings.push(Finding::new(
            "flow.base_not_local",
            "/base",
            "base must use a local HTTP host",
        ));
    }
    for group in ["screens", "edges"] {
        let mut seen = BTreeSet::new();
        for (i, entry) in value[group].as_array().unwrap().iter().enumerate() {
            if !seen.insert(entry["id"].as_str().unwrap()) {
                findings.push(Finding::new(
                    "flow.duplicate_id",
                    format!("/{group}/{i}/id"),
                    "id is repeated",
                ));
            }
        }
    }
    let screens: BTreeSet<_> = value["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    let edges: BTreeMap<_, _> = value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["id"].as_str().unwrap(), e))
        .collect();
    let secrets: BTreeSet<_> = value["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    for (i, edge) in value["edges"].as_array().unwrap().iter().enumerate() {
        for field in ["from", "to"] {
            if !screens.contains(edge[field].as_str().unwrap()) {
                findings.push(Finding::new(
                    "flow.unknown_screen",
                    format!("/edges/{i}/{field}"),
                    "edge names an unknown screen",
                ));
            }
        }
        for (a, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
            if let Some(secret) = act["secret"].as_str()
                && !secrets.contains(secret)
            {
                findings.push(Finding::new(
                    "flow.unknown_secret",
                    format!("/edges/{i}/acts/{a}/secret"),
                    "act names an unknown secret",
                ));
            }
        }
    }
    let mut reached = BTreeSet::new();
    for (name, path) in value["paths"].as_object().unwrap() {
        let contract_id = if name == "main" {
            id.to_owned()
        } else {
            format!("{id}.{name}")
        };
        if !graphhelm_execution::valid_journey_id(&contract_id) {
            findings.push(Finding::new(
                "flow.composed_id_invalid",
                format!("/paths/{name}"),
                "composed contract id is invalid",
            ));
        }
        let mut last = None;
        let mut visited = BTreeSet::new();
        for (i, edge_id) in path.as_array().unwrap().iter().enumerate() {
            let pointer = format!("/paths/{name}/{i}");
            let Some(edge) = edges.get(edge_id.as_str().unwrap()) else {
                findings.push(Finding::new(
                    "flow.unknown_edge",
                    pointer,
                    "path names an unknown edge",
                ));
                continue;
            };
            let from = edge["from"].as_str().unwrap();
            let to = edge["to"].as_str().unwrap();
            reached.insert(from);
            reached.insert(to);
            if let Some(previous) = last
                && previous != from
            {
                findings.push(Finding::new(
                    "flow.path_disconnected",
                    &pointer,
                    "consecutive edges do not connect",
                ));
            }
            if i == 0 {
                visited.insert(from);
            }
            if !visited.insert(to) {
                findings.push(Finding::new(
                    "flow.path_revisits_screen",
                    pointer,
                    "path revisits a screen",
                ));
            }
            last = Some(to);
        }
    }
    for (i, screen) in value["screens"].as_array().unwrap().iter().enumerate() {
        if !reached.contains(screen["id"].as_str().unwrap()) {
            findings.push(Finding::new(
                "flow.unreachable_screen",
                format!("/screens/{i}/id"),
                "screen belongs to no path",
            ));
        }
        if !graphhelm_execution::valid_journey_id(&format!(
            "{}.visible",
            screen["id"].as_str().unwrap()
        )) {
            findings.push(Finding::new(
                "flow.composed_id_invalid",
                format!("/screens/{i}/id"),
                "composed promise id is invalid",
            ));
        }
        let Some(paths) = screen["scope"].as_array() else {
            continue;
        };
        for (j, entry) in paths.iter().enumerate() {
            let path = entry.as_str().unwrap();
            let pointer = format!("/screens/{i}/scope/{j}");
            if path.contains(['\\', ':'])
                || !Path::new(path)
                    .components()
                    .all(|p| matches!(p, Component::Normal(_) | Component::CurDir))
            {
                findings.push(Finding::new(
                    "flow.scope_path_outside_project",
                    pointer,
                    "scope must stay inside the project",
                ));
            } else {
                match (project.canonicalize(), project.join(path).canonicalize()) {
                    (Ok(root), Ok(target)) if !target.starts_with(&root) => {
                        findings.push(Finding::new(
                            "flow.scope_path_outside_project",
                            pointer,
                            "scope resolves outside the project",
                        ))
                    }
                    (_, Err(_)) => findings.push(Finding::new(
                        "flow.scope_path_missing",
                        pointer,
                        "scope does not exist",
                    )),
                    _ => {}
                }
            }
        }
    }
    if value["status"] == "approved" && !value["drift"].as_array().unwrap().is_empty() {
        findings.push(Finding::new(
            "flow.approved_with_drift",
            "/drift/0",
            "approved flow carries drift",
        ));
    }
    if value["status"] == "approved"
        && value["approved"]["digest"].as_str() != Some(approval_digest(value).as_str())
    {
        findings.push(Finding::new(
            "flow.approval_stale",
            "/approved/digest",
            "approval does not bind this flow projection",
        ));
    }
    findings
}

fn approval_digest(flow: &Value) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical(flow, true).as_bytes()))
    )
}

pub(crate) fn check(file: &Path, project: &Path) -> Vec<Finding> {
    match read(file) {
        Ok((text, value)) => {
            let mut findings = semantic(file, &value, project);
            if text != canonical(&value, false) {
                findings.push(Finding::new(
                    "flow.not_canonical",
                    "",
                    "flow bytes differ from canonical YAML",
                ));
            }
            if findings.iter().all(Finding::is_warning) {
                let contracts = match compile(&value) {
                    Ok(contracts) => contracts,
                    Err(finding) => {
                        findings.push(finding);
                        return findings;
                    }
                };
                for (id, contract) in contracts {
                    let path = project
                        .join(".graphhelm/journeys")
                        .join(format!("{id}.json"));
                    if (value["status"] == "approved" || path.exists())
                        && output_bytes(&path).ok().as_deref()
                            != Some(contract_bytes(&contract).as_slice())
                    {
                        findings.push(Finding::new(
                            "flow.contract_stale",
                            format!("/journeys/{id}.json"),
                            "generated contract differs or is missing",
                        ));
                    }
                }
            }
            findings
        }
        Err(findings) => findings,
    }
}

fn compile(flow: &Value) -> Result<Vec<(String, Value)>, Finding> {
    let id = flow["id"].as_str().unwrap();
    let title = flow["title"].as_str().unwrap_or(id);
    let actors = flow["actors"].as_array().unwrap();
    let screens: BTreeMap<_, _> = flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s))
        .collect();
    let edges: BTreeMap<_, _> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["id"].as_str().unwrap(), e))
        .collect();
    let mut contracts = Vec::new();
    for (name, path) in flow["paths"].as_object().unwrap() {
        let edges: Vec<_> = path
            .as_array()
            .unwrap()
            .iter()
            .map(|e| edges[e.as_str().unwrap()])
            .collect();
        let visited = std::iter::once(edges[0]["from"].as_str().unwrap())
            .chain(edges.iter().map(|e| e["to"].as_str().unwrap()));
        let (mut steps, mut promises) = (Vec::new(), Vec::new());
        for (index, sid) in visited.enumerate() {
            let screen = screens[sid];
            let action = if index == 0 {
                json!({"kind":"navigate","target":{"strategy":"label","value":screen["url"],"geometryClaim":false}})
            } else {
                let act = edges[index - 1]["acts"].as_array().unwrap().last().unwrap();
                json!({"kind":act["kind"],"target":{"strategy":"accessible_name","value":act["name"],"role":act["role"],"geometryClaim":false}})
            };
            let mut step = json!({"stepId":sid,"actorId":actors[0],"semanticAction":action,"expectedStates":[screen["state"]],"failureContract":{"timeoutSeconds":30,"visibleError":format!("screen {sid} not reached"),"safeStop":"stop replay","recoveryAction":null,"prohibitedSideEffects":[]}});
            if screen["scope"].is_array() {
                step["screen"] = json!({"screenId":sid,"title":screen["title"].as_str().unwrap_or(sid),"scopePaths":screen["scope"]});
            }
            let statement = format!(
                "shows {}",
                screen["expect"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| format!(
                        "{} {:?}",
                        e["role"].as_str().unwrap(),
                        e["name"].as_str().unwrap()
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            // The frozen schema bounds characters. Refuse rather than drop any expectation.
            if statement.chars().count() > 1024 {
                let screen_index = flow["screens"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|screen| screen["id"] == sid)
                    .unwrap();
                return Err(Finding::new(
                    "flow.promise_too_long",
                    format!("/screens/{screen_index}/expect"),
                    "all expectations must fit the contract's 1024-character promise limit",
                ));
            }
            promises.push(json!({"promiseId":format!("{sid}.visible"),"stepId":sid,"statement":statement,"requiredFact":"content_rendered","requiredEvidenceKinds":["visual_capture"],"requiredObserverCapability":"browser","statesToObserve":[screen["state"]],"maxEvidenceAgeSeconds":604800}));
            steps.push(step);
        }
        let contract_id = if name == "main" {
            id.to_owned()
        } else {
            format!("{id}.{name}")
        };
        contracts.push((contract_id.clone(),json!({"contractId":contract_id,"version":1,"title":title,"taskScope":format!("journey-flow {id} path {name}"),"actors":actors.iter().map(|a|json!({"actorId":a,"name":a.as_str().unwrap().chars().take(120).collect::<String>(),"goal":title})).collect::<Vec<_>>(),"preconditions":[],"steps":steps,"promises":promises,"riskSignals":flow["risks"],"outOfScope":[]})));
    }
    Ok(contracts)
}

fn contract_bytes(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("contract serialization");
    bytes.push(b'\n');
    bytes
}

fn output_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    let limit = super::journeys::MAX_CONTRACT_BYTES;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(std::io::Error::other("unsafe or oversized output"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("output grew past size limit"));
    }
    Ok(bytes)
}

fn input_error(command: &'static str, message: &str) -> Outcome {
    Outcome::application(
        command,
        Diagnostic::error(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            message,
            "/project",
            "graphhelm",
        ),
    )
}

fn report(
    command: &'static str,
    files: Vec<Value>,
    findings: Vec<Finding>,
    data: Value,
) -> Outcome {
    let clean = findings.iter().all(Finding::is_warning);
    let diagnostics = findings
        .iter()
        .map(|f| {
            let diagnostic = if f.is_warning() {
                Diagnostic::warning
            } else {
                Diagnostic::error
            };
            diagnostic(
                crate::error_codes::GHCLI034_JOURNEY_FLOW_INVALID,
                &f.message,
                &f.pointer,
                "journey-flow",
            )
        })
        .collect();
    let mut data = data;
    data["files"] = json!(files);
    Outcome {
        output: CommandOutput {
            ok: clean,
            command,
            data: Some(data),
            diagnostics,
        },
        exit_code: if clean {
            0
        } else if findings.iter().any(|f| f.code == "flow.compile_invalid") {
            3
        } else {
            2
        },
    }
}

fn files(project: &Path, ids: &[String]) -> Option<Vec<std::path::PathBuf>> {
    let root = project.canonicalize().ok()?;
    let directory = project.join(".graphhelm/journeys");
    if !directory.canonicalize().ok()?.starts_with(&root) {
        return None;
    }
    if !ids.is_empty() {
        if !ids
            .iter()
            .all(|id| graphhelm_execution::valid_journey_id(id))
        {
            return None;
        }
        return Some(
            ids.iter()
                .map(|id| directory.join(format!("{id}.journey.yaml")))
                .collect(),
        );
    }
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".journey.yaml"))
        .collect();
    files.sort();
    Some(files)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::other("output symlink refused"));
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut owned = false;
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        owned = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    // Remove only this invocation's temporary file, never a pre-existing one.
    if owned && result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub(crate) fn run_compile(args: &crate::args::JourneyCompileArgs) -> Outcome {
    const COMMAND: &str = "journey.compile";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    let Ok(_lock) = write_lock(&project) else {
        return input_error(COMMAND, "journey-flow write lock unavailable or unsafe");
    };
    let Some(files) = files(&project, &args.ids) else {
        return input_error(COMMAND, "no safe journeys directory or invalid flow id");
    };
    if files.is_empty() {
        return input_error(COMMAND, "no flow files found");
    }
    let (mut reports, mut findings, mut writes, mut skipped) =
        (Vec::new(), Vec::new(), BTreeMap::new(), Vec::new());
    for file in files {
        let mut errors;
        match read(&file) {
            Err(e) => errors = e,
            Ok((text, flow)) => {
                errors = semantic(&file, &flow, &project);
                if !args.fmt && text != canonical(&flow, false) {
                    errors.push(Finding::new(
                        "flow.not_canonical",
                        "",
                        "flow must be canonical; use --fmt",
                    ));
                }
                if errors.iter().all(Finding::is_warning) {
                    if args.fmt {
                        writes.insert(file.clone(), canonical(&flow, false).into_bytes());
                    }
                    if flow["status"] == "draft" && !args.include_draft {
                        skipped.push(json!({"id":flow["id"],"reason":"draft"}));
                    } else {
                        let contracts = match compile(&flow) {
                            Ok(contracts) => contracts,
                            Err(finding) => {
                                errors.push(finding);
                                Vec::new()
                            }
                        };
                        for (id, contract) in contracts {
                            if !super::journeys::contract_schemas().is_some_and(|s| {
                                s.validate(
                                    super::journeys::CONTRACT_SCHEMA_ID,
                                    &contract,
                                    "journey-flow",
                                )
                                .is_empty()
                            }) {
                                errors.push(Finding::new(
                                    "flow.compile_invalid",
                                    "",
                                    "compiled contract violates frozen schema",
                                ));
                                continue;
                            }
                            let target = file.parent().unwrap().join(format!("{id}.json"));
                            let expected = contract_bytes(&contract);
                            let existing = output_bytes(&target);
                            if args.check && existing.is_err() {
                                errors.push(Finding::new(
                                    "flow.contract_stale",
                                    format!("/journeys/{id}.json"),
                                    "generated contract is missing, unreadable or unsafe",
                                ));
                                continue;
                            }
                            if let Ok(existing) = existing
                                && existing != expected
                            {
                                let generated = serde_json::from_slice::<Value>(&existing)
                                    .is_ok_and(|v| v["taskScope"] == contract["taskScope"]);
                                if args.check || (!generated && !args.force) {
                                    errors.push(Finding::new(
                                        "flow.contract_stale",
                                        format!("/journeys/{id}.json"),
                                        "contract differs; handwritten files require --force",
                                    ));
                                    continue;
                                }
                            }
                            if writes.insert(target, expected).is_some() {
                                errors.push(Finding::new(
                                    "flow.contract_stale",
                                    format!("/journeys/{id}.json"),
                                    "multiple flows claim the same output",
                                ));
                            }
                        }
                    }
                }
            }
        }
        reports.push(json!({"file":file.display().to_string(),"findings":errors.iter().map(|f|json!({"code":f.code,"pointer":f.pointer,"message":f.message})).collect::<Vec<_>>()}));
        findings.extend(errors);
    }
    if findings.iter().all(Finding::is_warning) && !args.check && write_batch(&writes).is_err() {
        return input_error(
            COMMAND,
            "flow output could not be written; batch rollback attempted",
        );
    }
    report(
        COMMAND,
        reports,
        findings,
        json!({"skipped":skipped,"written":if args.check {0}else{writes.len()}}),
    )
}

fn write_lock(project: &Path) -> std::io::Result<std::fs::File> {
    use fs2::FileExt;
    let root = project.canonicalize()?;
    let directory = project.join(".graphhelm").canonicalize()?;
    if !directory.starts_with(&root) {
        return Err(std::io::Error::other("project directory escapes root"));
    }
    let path = directory.join("journey-flow.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(std::io::Error::other("lock symlink refused"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock_exclusive()?;
    Ok(file)
}

// Validate every destination and retain its prior bytes before the first mutation. Serialization
// and conflict failures leave all outputs untouched; an I/O failure restores earlier writes.
fn write_batch(writes: &BTreeMap<std::path::PathBuf, Vec<u8>>) -> std::io::Result<()> {
    let mut backups = BTreeMap::new();
    for path in writes.keys() {
        let previous = match std::fs::symlink_metadata(path) {
            Ok(m)
                if m.file_type().is_symlink()
                    || !m.is_file()
                    || m.len() > super::journeys::MAX_CONTRACT_BYTES =>
            {
                return Err(std::io::Error::other("unsafe output destination"));
            }
            Ok(_) => Some(output_bytes(path)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        backups.insert(path.clone(), previous);
    }
    let mut completed: Vec<&Path> = Vec::new();
    for (path, bytes) in writes {
        if let Err(error) = atomic_write(path, bytes) {
            let mut rollback_failed = false;
            for previous in completed.into_iter().rev() {
                let result = match &backups[previous] {
                    Some(bytes) => atomic_write(previous, bytes),
                    None => std::fs::remove_file(previous),
                };
                rollback_failed |= result.is_err();
            }
            return Err(if rollback_failed {
                std::io::Error::other("output write and rollback both failed")
            } else {
                error
            });
        }
        completed.push(path.as_path());
    }
    Ok(())
}

pub(crate) fn run_approve(args: &crate::args::JourneyApproveArgs) -> Outcome {
    const COMMAND: &str = "journey.approve";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    let Some(files) = files(&project, std::slice::from_ref(&args.id)) else {
        return input_error(COMMAND, "invalid flow id or unsafe journeys directory");
    };
    let Ok(_lock) = write_lock(&project) else {
        return input_error(COMMAND, "journey-flow write lock unavailable or unsafe");
    };
    let file = &files[0];
    let (text, mut flow) = match read(file) {
        Ok(value) => value,
        Err(findings) => return report(COMMAND, vec![], findings, json!({})),
    };
    let mut findings = semantic(file, &flow, &project);
    findings.retain(|f| !matches!(f.code, "flow.approval_stale" | "flow.approved_with_drift"));
    if text != canonical(&flow, false) {
        findings.push(Finding::new(
            "flow.not_canonical",
            "",
            "approve requires a canonical flow",
        ));
    }
    if !findings.iter().all(Finding::is_warning) {
        return report(
            COMMAND,
            vec![
                json!({"findings":findings.iter().map(|f|json!({"code":f.code,"pointer":f.pointer,"message":f.message})).collect::<Vec<_>>()}),
            ],
            findings,
            json!({}),
        );
    }
    let revision = match super::journey::head(&project) {
        Ok(rev) if rev.len() == 40 => rev,
        _ => {
            return input_error(
                COMMAND,
                "project git revision is missing or unsupported; approval requires a committed SHA-1 repository",
            );
        }
    };
    flow["status"] = json!("approved");
    flow["drift"] = json!([]);
    flow["approved"] = json!({"revision":revision,"digest":approval_digest(&flow)});
    let mut writes = BTreeMap::new();
    let contracts = match compile(&flow) {
        Ok(contracts) => contracts,
        Err(finding) => {
            return report(COMMAND, vec![], vec![finding], json!({}));
        }
    };
    for (id, contract) in contracts {
        if !super::journeys::contract_schemas().is_some_and(|s| {
            s.validate(
                super::journeys::CONTRACT_SCHEMA_ID,
                &contract,
                "journey-flow",
            )
            .is_empty()
        }) {
            return report(
                COMMAND,
                vec![],
                vec![Finding::new(
                    "flow.compile_invalid",
                    "",
                    "compiled contract violates frozen schema",
                )],
                json!({}),
            );
        }
        let path = file.parent().unwrap().join(format!("{id}.json"));
        if let Ok(existing) = output_bytes(&path)
            && existing != contract_bytes(&contract)
            && !serde_json::from_slice::<Value>(&existing)
                .is_ok_and(|v| v["taskScope"] == contract["taskScope"])
        {
            return report(
                COMMAND,
                vec![],
                vec![Finding::new(
                    "flow.contract_stale",
                    format!("/journeys/{id}.json"),
                    "approval cannot overwrite a handwritten contract",
                )],
                json!({}),
            );
        }
        writes.insert(path, contract_bytes(&contract));
    }
    writes.insert(file.clone(), canonical(&flow, false).into_bytes());
    if let Err(error) = write_batch(&writes) {
        return input_error(
            COMMAND,
            if error.to_string() == "output write and rollback both failed" {
                "output write and rollback failed; inspect project files"
            } else {
                "approval output could not be written; earlier writes restored"
            },
        );
    }
    Outcome::success(
        COMMAND,
        json!({"id":args.id,"status":"approved","approved":flow["approved"],"written":writes.len()}),
    )
}

// Emit the schema's reading order, not map insertion order or a serializer's incidental style.
fn scalar(value: &Value) -> String {
    let Some(text) = value.as_str() else {
        return value.to_string();
    };
    let plain = !text.is_empty()
        && text.trim() == text
        && !text.chars().any(char::is_control)
        && !text.contains(": ")
        && !text.contains(['#', ',', '[', ']', '{', '}'])
        && !text.starts_with([
            '-', '?', ':', '!', '&', '*', '|', '>', '\'', '"', '%', '@', '`',
        ])
        && serde_yaml_ng::from_str::<Value>(text).is_ok_and(|parsed| parsed == *value);
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).expect("string serialization")
    }
}

fn inline(value: &Value, fields: &[&str]) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|v| inline(v, fields))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(object) => format!(
            "{{{}}}",
            fields
                .iter()
                .filter_map(|key| object.get(*key).map(|v| format!("{key}: {}", scalar(v))))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => scalar(value),
    }
}

fn canonical(value: &Value, approval_projection: bool) -> String {
    let mut out = String::new();
    for field in [
        "schema", "id", "title", "status", "approved", "base", "actors", "secrets", "risks",
    ] {
        if approval_projection && matches!(field, "status" | "approved") {
            continue;
        }
        if let Some(v) = value.get(field) {
            out.push_str(&format!(
                "{field}: {}\n",
                inline(v, &["revision", "digest"])
            ));
        }
    }
    for (group, fields) in [
        (
            "screens",
            &["id", "title", "url", "state", "expect", "scope"][..],
        ),
        ("edges", &["id", "from", "to", "acts"][..]),
    ] {
        out.push_str(&format!("{group}:\n"));
        let mut entries = value[group].as_array().unwrap().iter().collect::<Vec<_>>();
        entries.sort_by_key(|v| v["id"].as_str().unwrap());
        for entry in entries {
            for &field in fields {
                let Some(v) = entry.get(field) else {
                    continue;
                };
                let prefix = if field == "id" {
                    "  - ".to_owned()
                } else {
                    " ".repeat(4)
                };
                if field == "acts" {
                    out.push_str(&prefix);
                    out.push_str("acts:\n");
                    for act in v.as_array().unwrap() {
                        out.push_str(&format!(
                            "{:6}- {}\n",
                            "",
                            inline(act, &["kind", "role", "name", "text", "secret"])
                        ));
                    }
                } else {
                    out.push_str(&format!(
                        "{prefix}{field}: {}\n",
                        inline(v, &["role", "name"])
                    ));
                }
            }
        }
    }
    out.push_str("paths:\n");
    let paths = value["paths"].as_object().unwrap();
    for (name, path) in std::iter::once(("main", &paths["main"])).chain(
        paths
            .iter()
            .filter(|(n, _)| n.as_str() != "main")
            .map(|(n, p)| (n.as_str(), p)),
    ) {
        out.push_str(&format!("  {name}: {}\n", inline(path, &[])));
    }
    if !approval_projection {
        let drift = value["drift"].as_array().unwrap();
        if drift.is_empty() {
            out.push_str("drift: []\n");
        } else {
            out.push_str("drift:\n");
            for entry in drift {
                for field in ["edge", "act", "code", "seen", "at", "healed"] {
                    if let Some(v) = entry.get(field) {
                        out.push_str(&format!(
                            "{}{field}: {}\n",
                            if field == "edge" {
                                "  - ".to_owned()
                            } else {
                                " ".repeat(4)
                            },
                            scalar(v)
                        ));
                    }
                }
            }
        }
    }
    out
}
