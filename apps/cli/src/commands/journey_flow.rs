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

pub(super) fn local_base(base: &str) -> bool {
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
        if !edge["safe"].is_null() {
            if value["status"] != "draft" {
                findings.push(Finding::new(
                    "flow.safe_not_draft",
                    format!("/edges/{i}/safe"),
                    "only a draft carries a safe mark; an approved flow is not guarded",
                ));
            } else if !edge_marked_safe(value, edge) {
                findings.push(Finding::new(
                    "flow.safe_stale",
                    format!("/edges/{i}/safe"),
                    "the edge changed since the owner marked it safe; the mark is void",
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

/// What a safe mark binds (#518): WHERE the acts are played and WHAT they are. The flow's `base`
/// (the app a watch opens), the screen the edge leaves and that screen's URL, the screen it
/// reaches, and the acts exactly as the canonical flow writes them. Editing an act (kind, role,
/// name, text, secret, order, count), moving the edge to another screen, repointing its screen at
/// another URL, or pointing the flow at another app voids the mark: "Send" marked safe on one
/// page of one app is not "Send" on another. `base_not_local` keeps a flow on this machine, not
/// on the app the owner was shown, so `base` is bound here.
fn mark_digest(flow: &Value, edge: &Value) -> String {
    let url = flow["screens"]
        .as_array()
        .and_then(|screens| screens.iter().find(|screen| screen["id"] == edge["from"]))
        .map(|screen| scalar(&screen["url"]))
        .unwrap_or_default();
    let mut lines = vec![
        format!("base: {}", scalar(&flow["base"])),
        format!("from: {}", scalar(&edge["from"])),
        format!("url: {url}"),
        format!("to: {}", scalar(&edge["to"])),
    ];
    if let Some(acts) = edge["acts"].as_array() {
        lines.extend(acts.iter().map(|act| inline(act, ACT_FIELDS)));
    }
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
    )
}

/// The owner marked this edge safe to watch on a draft, and it is still that edge.
///
/// WHAT IT PROVES, AND WHAT IT DOES NOT. The digest proves the edge is unchanged since the mark
/// was written. It does not prove WHO wrote it: the mark lives in the flow file, like
/// `approved: {revision, digest}`, and whoever can write that file can compute a digest. What
/// makes it the owner's are the doors: the Runtime route takes the owner credential (the agent
/// session token is refused), `graphhelm journey mark-safe` takes none and is the owner's own
/// machine, like `journey approve`, and the agent's own writer (`draft_bytes`) refuses a flow
/// that carries a mark.
pub(crate) fn edge_marked_safe(flow: &Value, edge: &Value) -> bool {
    edge["safe"]["digest"].as_str() == Some(mark_digest(flow, edge).as_str())
}

/// #534: a YAML approval counts only with the owner's record for its exact digest in the
/// project's owner store. Without the record the flow is unsigned (an agent could have written
/// the YAML); without a readable store the approval cannot be verified at all.
fn owner_signature(value: &Value, project: &Path) -> Vec<Finding> {
    if value["status"] != "approved" {
        return vec![];
    }
    let (Some(id), Some(digest)) = (value["id"].as_str(), value["approved"]["digest"].as_str())
    else {
        return vec![];
    };
    match super::journey_owner::approved(project, id, digest) {
        Ok(true) => vec![],
        Ok(false) => vec![Finding::new(
            "flow.approval_unsigned",
            "/approved",
            "no owner record approves this flow at this digest; only the owner's approve records one",
        )],
        Err(message) => vec![Finding::new(
            "flow.approval_unverifiable",
            "/approved",
            message,
        )],
    }
}

pub(crate) fn approval_digest(flow: &Value) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical(flow, true).as_bytes()))
    )
}

/// The exploration/healing writer uses the original schema, semantic checker,
/// compiler and canonical renderer before publishing an unapproved memory snapshot.
pub(super) fn draft_bytes(flow: &Value, project: &Path) -> Result<String, Vec<Finding>> {
    let Some(schemas) = flow_schemas() else {
        return Err(vec![Finding::new(
            "flow.schema_invalid",
            "",
            "flow schema unavailable",
        )]);
    };
    let mut findings: Vec<_> = schemas
        .validate(FLOW_SCHEMA_ID, flow, "journey-flow")
        .into_iter()
        .map(|d| Finding::new("flow.schema_invalid", d.path, d.message))
        .collect();
    if !findings.is_empty() {
        return Err(findings);
    }
    if flow["status"] != "draft" || !flow["approved"].is_null() {
        return Err(vec![Finding::new(
            "flow.approval_stale",
            "/approved",
            "only unapproved drafts may be published by the agent",
        )]);
    }
    // #518: a safe mark is the owner's. Every flow that reaches this writer is the agent's own
    // draft or an approved flow demoted by drift (approve drops marks), so none is legitimate.
    if let Some(i) = flow["edges"]
        .as_array()
        .and_then(|edges| edges.iter().position(|edge| !edge["safe"].is_null()))
    {
        return Err(vec![Finding::new(
            "flow.safe_owner_only",
            format!("/edges/{i}/safe"),
            "only the owner marks an act safe (graphhelm journey mark-safe)",
        )]);
    }
    let file = project
        .join(".graphhelm/journeys")
        .join(format!("{}.journey.yaml", flow["id"].as_str().unwrap()));
    findings.extend(semantic(&file, flow, project));
    if let Err(finding) = compile(flow) {
        findings.push(finding);
    }
    if findings.iter().any(|f| !f.is_warning()) {
        return Err(findings);
    }
    let text = canonical(flow, false);
    if text.len() as u64 > MAX_FLOW_BYTES {
        return Err(vec![Finding::new(
            "flow.too_large",
            "",
            "flow exceeds 32 KiB",
        )]);
    }
    Ok(text)
}

pub(crate) fn check(file: &Path, project: &Path) -> Vec<Finding> {
    match read(file) {
        Ok((text, value)) => check_snapshot(file, &text, &value, project),
        Err(findings) => findings,
    }
}

/// `journey watch` plays a flow before its owner approves it: the same validated snapshot as
/// replay, without the approval requirement. Watching proves nothing and writes nothing.
pub(crate) fn read_for_watch(file: &Path, project: &Path) -> Result<Value, Vec<Finding>> {
    let (text, value) = read(file)?;
    let mut findings = check_snapshot(file, &text, &value, project);
    // #490: watch plays the flow's own acts and never reads its generated contract, so a stale
    // or absent `<id>.json` (git-ignored output, often left over from an earlier compile) is no
    // reason to refuse it. `validate` still reports it.
    findings.retain(|f| f.code != "flow.contract_stale");
    // #534: watch plays drafts too, so an unsigned approval only means "a draft" here.
    findings
        .retain(|f| f.code != "flow.approval_unsigned" && f.code != "flow.approval_unverifiable");
    if findings.iter().any(|f| !f.is_warning()) {
        Err(findings)
    } else {
        Ok(value)
    }
}

/// Replay consumes exactly the validated source snapshot, never a second unchecked read.
pub(crate) fn read_for_replay(file: &Path, project: &Path) -> Result<Value, Vec<Finding>> {
    let (text, value) = read(file)?;
    let mut findings = check_snapshot(file, &text, &value, project);
    if value["status"] != "approved" {
        findings.push(Finding::new(
            "flow.not_approved",
            "/status",
            "replay requires an approved flow",
        ));
    }
    if findings.iter().any(|f| !f.is_warning()) {
        Err(findings)
    } else {
        Ok(value)
    }
}

fn check_snapshot(file: &Path, text: &str, value: &Value, project: &Path) -> Vec<Finding> {
    let mut findings = semantic(file, value, project);
    findings.extend(owner_signature(value, project));
    if text != canonical(value, false) {
        findings.push(Finding::new(
            "flow.not_canonical",
            "",
            "flow bytes differ from canonical YAML",
        ));
    }
    if findings.iter().all(Finding::is_warning) {
        let contracts = match compile(value) {
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
                && output_bytes(&path).ok().as_deref() != Some(contract_bytes(&contract).as_slice())
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

/// The committed flows as `keel plan` and `keel check` read them (#426). The generated
/// `<id>.json` contracts are git-ignored, so a clone holds only the `*.journey.yaml` sources: each
/// is projected here in memory exactly as `journey compile` would write it.
#[derive(Default)]
pub(crate) struct ProjectedFlows {
    /// Every contract of every approved flow that validates and projects: (flow id, contract).
    pub(crate) approved: Vec<(String, Value)>,
    /// Every draft flow: its id and the scope paths of its screens.
    pub(crate) drafts: Vec<(String, Vec<String>)>,
    /// Every flow file that does not validate or project: its file name and the first refusal.
    pub(crate) invalid: Vec<(String, String)>,
}

pub(crate) fn projected_flows(project: &Path) -> ProjectedFlows {
    let mut projected = ProjectedFlows::default();
    let Some(files) = files(project, &[]) else {
        return projected;
    };
    for file in files {
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flow = match read(&file) {
            Ok((_, flow)) => flow,
            Err(findings) => {
                let code = findings.first().map_or("flow.not_yaml", |f| f.code);
                projected.invalid.push((name, code.to_owned()));
                continue;
            }
        };
        if let Some(refusal) = semantic(&file, &flow, project)
            .into_iter()
            .find(|finding| !finding.is_warning())
        {
            projected.invalid.push((name, refusal.code.to_owned()));
            continue;
        }
        let id = flow["id"].as_str().unwrap_or_default().to_owned();
        // #534: an approval with no owner record for its digest is a draft to keel plan too.
        if flow["status"] != "approved"
            || owner_signature(&flow, project)
                .iter()
                .any(|f| !f.is_warning())
        {
            let scopes = flow["screens"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|screen| screen["scope"].as_array())
                .flatten()
                .filter_map(|path| path.as_str().map(str::to_owned))
                .collect();
            projected.drafts.push((id, scopes));
            continue;
        }
        match compile(&flow) {
            Ok(contracts) => projected.approved.extend(
                contracts
                    .into_iter()
                    .map(|(_, contract)| (id.clone(), contract)),
            ),
            Err(refusal) => projected.invalid.push((name, refusal.code.to_owned())),
        }
    }
    projected
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

/// Replay found drift on an approved flow: persist it as a draft fact, and with `repair` (heal)
/// replace only that edge's acts. The writer lock and a re-read of the replayed source refuse a
/// concurrent edit instead of merging into it; the result passes the agent-draft validator.
pub(crate) fn record_drift(
    project: &Path,
    file: &Path,
    replayed: &Value,
    entry: Value,
    repair: Option<&[Value]>,
) -> Result<Value, &'static str> {
    let _lock = write_lock(project).map_err(|_| "replay.drift_unpersisted")?;
    let (_, mut flow) = read(file).map_err(|_| "replay.source_changed")?;
    if flow != *replayed {
        return Err("replay.source_changed");
    }
    flow["status"] = json!("draft");
    flow["approved"] = Value::Null;
    if let Some(acts) = repair {
        let edge = flow["edges"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|edge| edge["id"] == entry["edge"])
            .ok_or("heal.repair_invalid")?;
        edge["acts"] = acts.to_vec().into();
    }
    flow["drift"].as_array_mut().unwrap().push(entry);
    let text = draft_bytes(&flow, project).map_err(|_| {
        if repair.is_some() {
            "heal.repair_invalid"
        } else {
            "replay.drift_unpersisted"
        }
    })?;
    atomic_write(file, text.as_bytes()).map_err(|_| "replay.drift_unpersisted")?;
    Ok(flow)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
    let written = if !args.check && findings.iter().all(Finding::is_warning) {
        writes.len()
    } else {
        0
    };
    report(
        COMMAND,
        reports,
        findings,
        json!({"skipped":skipped,"written":written}),
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

/// The findings `run_approve` refuses on: everything `semantic` reports except what approval
/// itself clears (a stale binding, drift), plus non-canonical bytes.
fn approval_findings(file: &Path, text: &str, flow: &Value, project: &Path) -> Vec<Finding> {
    let mut findings = semantic(file, flow, project);
    findings.retain(|f| !matches!(f.code, "flow.approval_stale" | "flow.approved_with_drift"));
    if text != canonical(flow, false) {
        findings.push(Finding::new(
            "flow.not_canonical",
            "",
            "approve requires a canonical flow",
        ));
    }
    findings
}

/// `graphhelm journey flows` (#353): every flow source with what the owner needs to review it -
/// its status (`approval_stale` when an approved flow no longer matches its binding), drift, the
/// `validate` findings, whether an Approve would be accepted and change something, and its
/// screens, edges and paths.
pub(crate) fn run_flows(args: &crate::args::JourneyFlowsArgs) -> Outcome {
    const COMMAND: &str = "journey.flows";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    if !project.join(".graphhelm/journeys").exists() {
        return Outcome::success(COMMAND, json!({"flows": []}));
    }
    let Some(files) = files(&project, &[]) else {
        return input_error(COMMAND, "unsafe or unreadable journeys directory");
    };
    let flows: Vec<Value> = files
        .iter()
        .map(|file| {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            let id = name.trim_end_matches(".journey.yaml");
            let findings = check(file, &project);
            let listed: Vec<Value> = findings
                .iter()
                .map(|f| {
                    json!({"code": f.code, "pointer": f.pointer, "message": f.message,
                        "severity": if f.is_warning() { "warning" } else { "error" }})
                })
                .collect();
            let Ok((text, flow)) = read(file) else {
                return json!({"id": id, "title": null, "status": "unreadable",
                    "approved": null, "drift": [], "findings": listed, "approvable": false,
                    "screens": [], "edges": [], "paths": {}});
            };
            let accepted = approval_findings(file, &text, &flow, &project)
                .iter()
                .all(Finding::is_warning);
            let stale = findings.iter().any(|f| f.code == "flow.approval_stale");
            let drifted = flow["drift"].as_array().is_some_and(|d| !d.is_empty());
            let settled = flow["status"] == "approved" && !stale && !drifted;
            let status = if stale {
                json!("approval_stale")
            } else {
                flow["status"].clone()
            };
            json!({"id": id, "title": flow.get("title").cloned().unwrap_or(Value::Null),
                "status": status, "approved": flow["approved"], "drift": flow["drift"],
                "findings": listed, "approvable": accepted && !settled,
                "screens": flow["screens"], "edges": flow["edges"], "paths": flow["paths"]})
        })
        .collect();
    Outcome::success(COMMAND, json!({"flows": flows}))
}

/// `graphhelm journey approve <id>` from a terminal (#534): the owner's door, so it asks for the
/// owner's token (`--token-file`, the project's `.graphhelm/events.token`) and refuses without it.
/// The Runtime route and the MCP tool authenticate the owner themselves and call `approve_owned`.
pub(crate) fn run_approve(args: &crate::args::JourneyApproveArgs) -> Outcome {
    const COMMAND: &str = "journey.approve";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    let owner = super::secret_file::token_path(&super::journey_owner::store(&project));
    let given = args
        .token_file
        .as_deref()
        .and_then(|path| super::secret_file::read_existing(path, "bearer token").ok());
    let expected = super::secret_file::read_existing(&owner, "bearer token").ok();
    if given.is_none() || given != expected {
        return input_error(
            COMMAND,
            "approving is the owner's: give --token-file with the project's owner token (.graphhelm/events.token)",
        );
    }
    approve_owned(&args.id, project)
}

/// The approval itself, for a caller that has already authenticated the owner (#534).
pub(crate) fn approve_owned(id: &str, project: std::path::PathBuf) -> Outcome {
    const COMMAND: &str = "journey.approve";
    let args = crate::args::JourneyApproveArgs {
        id: id.to_owned(),
        project: Some(project.clone()),
        token_file: None,
    };
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
    let findings = approval_findings(file, &text, &flow, &project);
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
    // #518: an approved flow is not guarded in watch, so its marks have nothing left to say.
    for edge in flow["edges"].as_array_mut().unwrap() {
        if let Some(edge) = edge.as_object_mut() {
            edge.remove("safe");
        }
    }
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
    // #534: the owner's record first, then the YAML, so an approval never exists unsigned.
    let digest = flow["approved"]["digest"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if let Err(message) = super::journey_owner::record(&project, &args.id, &digest, &revision) {
        return report(
            COMMAND,
            vec![],
            vec![Finding::new(
                "flow.owner_record_failed",
                "/approved",
                message,
            )],
            json!({}),
        );
    }
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

/// `graphhelm journey mark-safe <flow> <edge>` (#518): the owner says "this draft's edge may be
/// played in a watch although an act on it looks destructive". Writes `safe: {digest: <digest>}`
/// on that edge of a canonical DRAFT and nothing else; the reply lists every act the mark covers,
/// so one mark never blesses an act the owner was not shown. Owner door: the CLI on the owner's
/// machine, or the Runtime route with the owner credential.
pub(crate) fn run_mark_safe(args: &crate::args::JourneyMarkSafeArgs) -> Outcome {
    const COMMAND: &str = "journey.mark_safe";
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
    let mut findings = approval_findings(file, &text, &flow, &project);
    // A stale mark is what a new mark replaces.
    findings.retain(|f| f.code != "flow.safe_stale");
    if flow["status"] != "draft" {
        findings.push(Finding::new(
            "flow.safe_not_draft",
            "/status",
            "only a draft's act is marked safe; an approved flow is not guarded",
        ));
    }
    let index = flow["edges"].as_array().and_then(|edges| {
        edges
            .iter()
            .position(|edge| edge["id"] == args.edge.as_str())
    });
    if index.is_none() {
        findings.push(Finding::new(
            "flow.edge_unknown",
            "/edges",
            "the flow has no edge with that id",
        ));
    }
    let (Some(index), true) = (index, findings.iter().all(Finding::is_warning)) else {
        return report(
            COMMAND,
            vec![
                json!({"findings":findings.iter().map(|f|json!({"code":f.code,"pointer":f.pointer,"message":f.message})).collect::<Vec<_>>()}),
            ],
            findings,
            json!({}),
        );
    };
    let digest = mark_digest(&flow, &flow["edges"][index]);
    flow["edges"][index]["safe"] = json!({"digest": digest});
    if atomic_write(file, canonical(&flow, false).as_bytes()).is_err() {
        return input_error(COMMAND, "the flow could not be written; nothing changed");
    }
    Outcome::success(
        COMMAND,
        json!({"id": args.id, "edge": args.edge, "safe": flow["edges"][index]["safe"],
            "acts": flow["edges"][index]["acts"], "written": 1}),
    )
}

const ACT_FIELDS: &[&str] = &["kind", "role", "name", "text", "secret"];

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
        "viewport", "storage",
    ] {
        if approval_projection && matches!(field, "status" | "approved") {
            continue;
        }
        // #585: how a run opens its browser is part of what the owner approves.
        let order: &[&str] = match field {
            "viewport" => &["width", "height"],
            "storage" => &["key", "value"],
            _ => &["revision", "digest"],
        };
        if let Some(v) = value.get(field) {
            out.push_str(&format!("{field}: {}\n", inline(v, order)));
        }
    }
    for (group, fields) in [
        (
            "screens",
            &["id", "title", "url", "state", "expect", "scope"][..],
        ),
        ("edges", &["id", "from", "to", "acts", "safe"][..]),
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
                        out.push_str(&format!("{:6}- {}\n", "", inline(act, ACT_FIELDS)));
                    }
                } else {
                    out.push_str(&format!(
                        "{prefix}{field}: {}\n",
                        inline(v, &["role", "name", "digest"])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #518 (`keel.invariant.permissions`): the agent's own flow writer (explore, heal, drift)
    /// must never publish a safe mark, even one whose digest is right - an agent can compute it.
    /// Defect named: `draft_bytes` accepting a flow that carries `safe`, which would let the hand
    /// that wrote a destructive act also bless it. No CLI door reaches this writer without a
    /// model or a browser, so the cell sits beside it. Cost: one tempdir, no I/O beyond it.
    #[test]
    fn the_agent_writer_refuses_a_flow_that_carries_a_safe_mark() {
        let project = tempfile::tempdir().unwrap();
        for file in [
            "app/cart/page.tsx",
            "app/checkout/page.tsx",
            "app/api/pay/route.ts",
        ] {
            let path = project.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "export {}").unwrap();
        }
        let mut flow: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        assert!(draft_bytes(&flow, project.path()).is_ok());

        assert!(!edge_marked_safe(&flow, &flow["edges"][1]));
        let digest = mark_digest(&flow, &flow["edges"][1]);
        flow["edges"][1]["safe"] = json!({"digest": digest});
        assert!(edge_marked_safe(&flow, &flow["edges"][1]));
        let refused = draft_bytes(&flow, project.path()).unwrap_err();
        assert_eq!(refused[0].code, "flow.safe_owner_only");
        assert_eq!(refused[0].pointer, "/edges/1/safe");
    }

    /// #585: a flow may declare the page size its runs open at and the localStorage they start
    /// with. Both are part of what the owner approves (a phone-sized run, or one that "remembers"
    /// a visit, is a different journey), both survive the canonical writer, and both are bounded.
    /// Defects named: either field dropped by `canonical` (a `--fmt` or an agent write would erase
    /// it), left out of the approval digest (changing it would keep an approval), or unbounded.
    /// Cost: one tempdir, no I/O beyond it.
    #[test]
    fn a_declared_viewport_and_storage_are_written_approved_and_bounded() {
        let project = tempfile::tempdir().unwrap();
        for file in [
            "app/cart/page.tsx",
            "app/checkout/page.tsx",
            "app/api/pay/route.ts",
        ] {
            let path = project.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "export {}").unwrap();
        }
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let mut flow = plain.clone();
        flow["viewport"] = json!({"width": 390, "height": 844});
        flow["storage"] = json!([{"key": "graphhelm.handover.last-seen:demo:demo", "value": "1"}]);

        let written = draft_bytes(&flow, project.path()).unwrap_or_else(|findings| {
            panic!(
                "a bounded declaration is valid: {:?}",
                findings
                    .iter()
                    .map(|f| (&f.code, &f.pointer))
                    .collect::<Vec<_>>()
            )
        });
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(reread["viewport"], flow["viewport"], "{written}");
        assert_eq!(reread["storage"], flow["storage"], "{written}");

        let digest = approval_digest(&flow);
        assert_ne!(digest, approval_digest(&plain));
        let mut wider = flow.clone();
        wider["viewport"]["width"] = 391.into();
        assert_ne!(digest, approval_digest(&wider));
        let mut other = flow.clone();
        other["storage"][0]["value"] = "2".into();
        assert_ne!(digest, approval_digest(&other));

        for (field, value) in [
            ("viewport", json!({"width": 100, "height": 844})),
            ("viewport", json!({"width": 390})),
            ("storage", json!([{"key": "", "value": "1"}])),
            (
                "storage",
                Value::from(vec![json!({"key": "k", "value": ""}); 17]),
            ),
        ] {
            let mut bad = plain.clone();
            bad[field] = value.clone();
            let refused = draft_bytes(&bad, project.path()).unwrap_err();
            assert!(
                refused.iter().any(|f| f.code == "flow.schema_invalid"
                    && f.pointer.starts_with(&format!("/{field}"))),
                "{field}={value}: {:?}",
                refused
                    .iter()
                    .map(|f| (&f.code, &f.pointer))
                    .collect::<Vec<_>>()
            );
        }
    }
}
