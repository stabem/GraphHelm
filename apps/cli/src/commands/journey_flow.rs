//! Bounded flow source files; frozen JSON contracts remain the reader boundary.
use super::journey_validate::Finding;
use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;
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
    let (host, port) = if let Some(rest) = authority.strip_prefix("[::1]") {
        ("[::1]", rest.strip_prefix(':').unwrap_or(rest))
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
    findings
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
            findings
        }
        Err(findings) => findings,
    }
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
                let prefix = if field == "id" { "  - " } else { "    " };
                if field == "acts" {
                    out.push_str("    acts:\n");
                    for act in v.as_array().unwrap() {
                        out.push_str(&format!(
                            "      - {}\n",
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
                            if field == "edge" { "  - " } else { "    " },
                            scalar(v)
                        ));
                    }
                }
            }
        }
    }
    out
}
