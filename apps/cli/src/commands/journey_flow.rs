//! Bounded flow source files; frozen JSON contracts remain the reader boundary.
use super::atomic::atomic_write;
use super::journey_validate::Finding;
use crate::output::{CommandOutput, Outcome};
use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;
use serde_json::json;
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

/// #1784: the starting states a flow's screens must be seen in, in declared order. Empty: the flow
/// declares none and runs once, in whatever state the launcher seeds.
pub(crate) fn declared_states(flow: &Value) -> Vec<String> {
    flow["states"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// #1782: an instant the driver can install as the page's now.
fn clock_is_valid(clock: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(clock).is_ok_and(|instant| {
        instant.offset().local_minus_utc().abs() <= 14 * 3600
            && clock.get(17..19).is_some_and(|second| second < "60")
    })
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
    // #1777: a fixed `viewport` is one size, run alone; naming `viewports` beside it would leave
    // the Run unable to say which size it proved.
    if value.get("viewport").is_some() && value.get("viewports").is_some() {
        findings.push(Finding::new(
            "flow.viewport_and_viewports",
            "/viewports",
            "a flow declares either a fixed viewport or the viewports it proves, not both",
        ));
    }
    // #1782: the schema checks the clock's shape; here the instant must exist, and a browser
    // must be able to hold it (no leap second, offset within +-14:00).
    if let Some(clock) = value.get("clock").and_then(Value::as_str)
        && !clock_is_valid(clock)
    {
        findings.push(Finding::new(
            "flow.clock_invalid",
            "/clock",
            "clock must be an existing ISO-8601 instant with an offset",
        ));
    }
    // #1781: "says X" and "is gone" contradict each other. Refused here, not in the schema, so
    // journey-flow 1.9.0 stays a purely additive change for the schema-evolution baseline.
    for (s, screen) in value["screens"].as_array().unwrap().iter().enumerate() {
        for (e, expect) in screen["expect"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if expect["absent"] == true && expect.get("text").is_some() {
                findings.push(Finding::new(
                    "flow.text_with_absent",
                    format!("/screens/{s}/expect/{e}/text"),
                    "an absent expectation cannot also require text",
                ));
            }
        }
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
            } else {
                // #534 slice 2: the mark binds the edge, and only the owner's record makes it the
                // owner's. Without it (written by hand, copied) the mark is void and the guard
                // applies; without a readable store it cannot be checked and is void too.
                findings.extend(mark_signature(value, edge, i, project));
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
    findings.extend(weak_journey(value, false));
    findings.extend(outcome_elsewhere(value, false));
    findings.extend(unexplained(value, false));
    findings.extend(observer_missing(value, false));
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
    for (code, pointer, message) in super::journey_backend::findings(value) {
        findings.push(Finding::new(code, pointer, message));
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
/// #1633 follow-up: a flow that admits part of its purpose is not observed (the token
/// `OBSERVER_MISSING` in its title or purpose, or in a screen's or edge's title or notes) is not
/// proof of that purpose. `strict` (approval, and so `approvable`) refuses it as
/// `flow.observer_missing`; everywhere else it is a `flow.weak_journey` warning naming it, so an
/// existing or approved flow is flagged, never broken (#1287).
fn observer_missing(value: &Value, strict: bool) -> Vec<Finding> {
    const MARK: &str = "OBSERVER_MISSING";
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let marked = |text: &str| {
        text.match_indices(MARK).any(|(at, _)| {
            !word(text[..at].chars().next_back()) && !word(text[at + MARK.len()..].chars().next())
        })
    };
    let message = "this flow says part of its purpose is not observed (OBSERVER_MISSING); observe it before approving";
    let mut found = Vec::new();
    let mut check = |text: &Value, pointer: String| {
        if text.as_str().is_some_and(marked) {
            found.push(if strict {
                Finding::new("flow.observer_missing", pointer, message)
            } else {
                Finding::new(
                    "flow.weak_journey",
                    pointer,
                    format!("flow.observer_missing: {message}"),
                )
            });
        }
    };
    for field in ["title", "purpose"] {
        check(&value[field], format!("/{field}"));
    }
    for kind in ["screens", "edges"] {
        for (i, step) in value[kind].as_array().into_iter().flatten().enumerate() {
            for field in ["title", "notes"] {
                check(&step[field], format!("/{kind}/{i}/{field}"));
            }
        }
    }
    found
}

/// #1287: a journey must say what it proves. `strict` (the agent's draft writer and approval) refuses
/// each gap; everywhere else (validate, flows, watch, replay, keel) a flow that already exists keeps
/// working and carries each gap as a `flow.weak_journey` warning, so it is flagged, never broken.
fn weak_journey(value: &Value, strict: bool) -> Vec<Finding> {
    let mut gaps = Vec::new();
    if value["purpose"]
        .as_str()
        .is_none_or(|p| p.trim().is_empty())
    {
        gaps.push(Finding::new(
            "flow.purpose_missing",
            "/purpose",
            "say what this journey proves for the user in `purpose`",
        ));
    }
    let screens: BTreeMap<_, _> = value["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s))
        .collect();
    for (i, screen) in value["screens"].as_array().unwrap().iter().enumerate() {
        if screen["title"].as_str().is_none_or(|t| t.trim().is_empty()) {
            gaps.push(Finding::new(
                "flow.screen_title_missing",
                format!("/screens/{i}/title"),
                "say what this screen is for in its `title`",
            ));
        }
    }
    let edges: BTreeMap<_, _> = value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, e)| (e["id"].as_str().unwrap(), (i, e)))
        .collect();
    for (name, path) in value["paths"].as_object().unwrap() {
        let path: Vec<_> = path
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|id| edges.get(id.as_str().unwrap()))
            .collect();
        let mut has_input = false;
        for &&(i, edge) in &path {
            for (a, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
                match act["kind"].as_str().unwrap_or_default() {
                    "enter_text" | "select" | "upload" => has_input = true,
                    "submit" if !has_input && edge["intent"].is_null() => gaps.push(Finding::new(
                        "flow.submit_without_input",
                        format!("/edges/{i}/acts/{a}"),
                        format!(
                            "path {name} submits before any input; add the enter_text/select acts, \
                             or say in the edge `intent` what is submitted as-is"
                        ),
                    )),
                    _ => {}
                }
            }
        }
        let Some(&&(i, last)) = path.last() else {
            continue;
        };
        let expects = |id: &Value| -> BTreeSet<(String, String, bool, String)> {
            screens
                .get(id.as_str().unwrap_or_default())
                .and_then(|s| s["expect"].as_array())
                .into_iter()
                .flatten()
                // #1041: "X is gone" and "X is shown" are different outcomes.
                .map(|e| {
                    (
                        e["role"].to_string(),
                        e["name"].to_string(),
                        e["absent"] == true,
                        // #1781: "says X" and "says Y" are different outcomes too.
                        e["text"].to_string(),
                    )
                })
                .collect()
        };
        let before = expects(&last["from"]);
        let after = expects(&last["to"]);
        if !after.is_empty() && after.is_subset(&before) {
            gaps.push(Finding::new(
                "flow.outcome_unchecked",
                format!("/edges/{i}/to"),
                format!(
                    "path {name} ends on a screen that expects nothing its previous screen did not \
                     already show; expect the outcome (a saved value, a confirmation, the new row)"
                ),
            ));
        }
    }
    if !strict {
        return gaps
            .into_iter()
            .map(|g| {
                Finding::new(
                    "flow.weak_journey",
                    g.pointer,
                    format!("{}: {}", g.code, g.message),
                )
            })
            .collect();
    }
    gaps
}

/// #1785: a submit's effect is checked where it shows, not only on the page that pressed it.
///
/// "Same subject" is read from the flow's own navigation, nothing else: a screen `S` *shows the
/// subject* of the page at URL `u` when `u` is one item's page (a `:param` segment), `S` is at
/// another URL, and an edge with no submit leaves `S` for a screen at `u` by `activate`-ing an
/// element `S` itself expects (same role and name) whose role names a listed thing (a link, list
/// item, row, cell, option, tree item or article; a button is a command, not a subject). The
/// flow opened the item's page by clicking the item on `S`, so `S` shows that item. A submit on an edge leaving a screen at `u` then needs some path that plays that edge
/// and, at it or later, reaches a screen at `S`'s URL that expects something; otherwise the act
/// is named with `S`. `strict` (the agent's draft writer and approval) refuses it as
/// `flow.outcome_elsewhere_unchecked`; everywhere else it is a `flow.weak_journey` warning, so an
/// existing or approved flow is flagged, never broken. Replay, keel and the settled status do
/// not read it.
fn outcome_elsewhere(value: &Value, strict: bool) -> Vec<Finding> {
    // A button is a command; these roles name a thing the screen lists.
    const ITEM_ROLES: &[&str] = &[
        "link", "listitem", "row", "cell", "gridcell", "option", "treeitem", "article",
    ];
    let screens: BTreeMap<&str, &Value> = value["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["id"].as_str()?, s)))
        .collect();
    let url = |id: &Value| -> Option<&str> { screens.get(id.as_str()?)?["url"].as_str() };
    let expects = |id: &Value| -> Vec<(&Value, &Value)> {
        id.as_str()
            .and_then(|id| screens.get(id))
            .and_then(|s| s["expect"].as_array())
            .into_iter()
            .flatten()
            .filter(|e| e["absent"] != true)
            .map(|e| (&e["role"], &e["name"]))
            .collect()
    };
    let edges: Vec<&Value> = value["edges"].as_array().into_iter().flatten().collect();
    let acts =
        |edge: &Value| -> Vec<Value> { edge["acts"].as_array().cloned().unwrap_or_default() };
    // The screens that show the subject of the page at `u`, each with the act that opened it.
    let subjects = |u: &str| -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        for edge in &edges {
            let (Some(from), Some(to)) = (url(&edge["from"]), url(&edge["to"])) else {
                continue;
            };
            let acts = acts(edge);
            // One item's page (a `:param` in its URL), opened by an element that names an item.
            if to != u
                || from == u
                || !u.split('/').any(|segment| segment.starts_with(':'))
                || acts.iter().any(|a| a["kind"] == "submit")
            {
                continue;
            }
            let shown = expects(&edge["from"]);
            if let Some(act) = acts.iter().find(|a| {
                a["kind"] == "activate"
                    && ITEM_ROLES.contains(&a["role"].as_str().unwrap_or_default())
                    && shown.contains(&(&a["role"], &a["name"]))
            }) && let Some(id) = edge["from"].as_str()
            {
                found.entry(id.to_owned()).or_insert_with(|| {
                    format!(
                        "{} {}",
                        act["role"].as_str().unwrap_or_default(),
                        act["name"].as_str().unwrap_or_default()
                    )
                });
            }
        }
        found
    };
    let paths: Vec<Vec<&Value>> = value["paths"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, path)| {
            path.as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| edges.iter().copied().find(|e| e["id"] == *id))
                .collect()
        })
        .collect();
    // Some path plays `edge` and, at it or later, reaches a screen at `target` that expects.
    let checked_after = |edge: &Value, target: &str| {
        paths.iter().any(|path| {
            path.iter()
                .position(|e| e["id"] == edge["id"])
                .is_some_and(|at| {
                    path[at..]
                        .iter()
                        .any(|e| url(&e["to"]) == Some(target) && !expects(&e["to"]).is_empty())
                })
        })
    };
    let mut found = Vec::new();
    for (i, edge) in edges.iter().enumerate() {
        if !paths.iter().flatten().any(|e| e["id"] == edge["id"]) {
            continue;
        }
        let Some(u) = url(&edge["from"]) else {
            continue;
        };
        let Some((a, act)) = acts(edge)
            .into_iter()
            .enumerate()
            .find(|(_, a)| a["kind"] == "submit")
        else {
            continue;
        };
        for (screen, opened_by) in subjects(u) {
            let Some(target) = screens.get(screen.as_str()).and_then(|s| s["url"].as_str()) else {
                continue;
            };
            if checked_after(edge, target) {
                continue;
            }
            let message = format!(
                "`{}` changes what screen {screen} shows (the flow opens {u} from it by \
                 `{opened_by}`), but no path visits {screen} after this act; add a path that does \
                 and expects the change there",
                act["name"].as_str().unwrap_or_default()
            );
            let pointer = format!("/edges/{i}/acts/{a}");
            found.push(if strict {
                Finding::new("flow.outcome_elsewhere_unchecked", pointer, message)
            } else {
                Finding::new(
                    "flow.weak_journey",
                    pointer,
                    format!("flow.outcome_elsewhere_unchecked: {message}"),
                )
            });
        }
    }
    found
}

/// The roles a command or HTTP screen expects (`journey_backend`): an exit code, printed text or a
/// JSON field is opaque to a person who did not write the flow, so each needs its `because`.
const BACKEND_ROLES: &[&str] = &["exit", "stdout", "json"];

/// #1955: a journey explains itself to whoever approves it. Every edge says in `why` what the step
/// proves for the user, and every non-obvious expectation says in `because` why its value is the
/// right one: any expectation on a screen an `http` or `cli` act leads to, any with a backend role
/// (`exit`, `stdout`, `json`), and any that checks `text` or `absent`. `strict` (the agent's draft
/// writer, approval, and `journey validate` on a draft) refuses each gap with its own code;
/// everywhere else an existing flow keeps working and carries each gap as a `flow.unexplained`
/// warning, so a flow approved before the rule is flagged, never broken.
fn unexplained(value: &Value, strict: bool) -> Vec<Finding> {
    let mut gaps = Vec::new();
    let said = |v: &Value| v.as_str().is_some_and(|t| !t.trim().is_empty());
    let mut backend = BTreeSet::new();
    for (i, edge) in value["edges"].as_array().into_iter().flatten().enumerate() {
        if !said(&edge["why"]) {
            gaps.push(Finding::new(
                "flow.step_why_missing",
                format!("/edges/{i}/why"),
                "say in the edge `why` what this step proves for the user",
            ));
        }
        if edge["acts"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|act| matches!(act["kind"].as_str(), Some("http" | "cli")))
            && let Some(to) = edge["to"].as_str()
        {
            backend.insert(to);
        }
    }
    for (i, screen) in value["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let on_backend = screen["id"].as_str().is_some_and(|id| backend.contains(id));
        for (j, expect) in screen["expect"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let opaque = on_backend
                || expect["role"]
                    .as_str()
                    .is_some_and(|role| BACKEND_ROLES.contains(&role))
                || expect.get("text").is_some()
                || expect["absent"] == true;
            if opaque && !said(&expect["because"]) {
                gaps.push(Finding::new(
                    "flow.expect_because_missing",
                    format!("/screens/{i}/expect/{j}/because"),
                    "say in `because` why this expected value is the right one",
                ));
            }
        }
    }
    if strict {
        return gaps;
    }
    gaps.into_iter()
        .map(|g| {
            Finding::new(
                "flow.unexplained",
                g.pointer,
                format!("{}: {}", g.code, g.message),
            )
        })
        .collect()
}

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
    graphhelm_protocols::hash::wire_sha256(lines.join("\n").as_bytes())
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

/// #534 slice 2: the mark binds this edge AND the owner recorded it. What a watch plays.
pub(crate) fn edge_marked_safe_by_owner(flow: &Value, edge: &Value, project: &Path) -> bool {
    edge_marked_safe(flow, edge) && mark_signature(flow, edge, 0, project).is_empty()
}

fn mark_signature(flow: &Value, edge: &Value, index: usize, project: &Path) -> Vec<Finding> {
    let (Some(id), Some(edge_id), Some(digest)) = (
        flow["id"].as_str(),
        edge["id"].as_str(),
        edge["safe"]["digest"].as_str(),
    ) else {
        return vec![];
    };
    match super::journey_owner::marked(project, id, edge_id, digest) {
        Ok(true) => vec![],
        Ok(false) => vec![Finding::new(
            "flow.safe_unsigned",
            format!("/edges/{index}/safe"),
            "no owner record marks this edge safe; the mark is void",
        )],
        Err(message) => vec![Finding::new(
            "flow.safe_unverifiable",
            format!("/edges/{index}/safe"),
            format!("{message}; the mark is void"),
        )],
    }
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
            "no owner or reviewer record approves this flow at this digest; only `graphhelm journey approve` records one",
        )],
        Err(message) => vec![Finding::new(
            "flow.approval_unverifiable",
            "/approved",
            message,
        )],
    }
}

pub(crate) fn approval_digest(flow: &Value) -> String {
    graphhelm_protocols::hash::wire_sha256(canonical(flow, true).as_bytes())
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
    findings.extend(weak_journey(flow, true));
    findings.extend(outcome_elsewhere(flow, true));
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
        Ok((text, value)) => {
            let mut findings = check_snapshot(file, &text, &value, project);
            // #1955: a draft must explain itself before it is approved; an approved flow written
            // before the rule keeps its `flow.unexplained` warning.
            if value["status"] != "approved" {
                findings.retain(|f| f.code != "flow.unexplained");
                findings.extend(unexplained(&value, true));
            }
            findings
        }
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

/// #1684: what a preview plays. As `read_for_watch`, and an approved flow edited since its approval
/// (`flow.approval_stale`) is previewed as the draft it now is: re-approving it needs a recorded
/// preview, and the approval digest leaves out `status` and `approved`, so the key is unchanged.
/// It also returns the exact text that was parsed (#1911: the reviewed-on-main check compares
/// these bytes, never a second read of the file).
pub(crate) fn read_for_preview_source(
    file: &Path,
    project: &Path,
) -> Result<(Value, String), Vec<Finding>> {
    let (text, mut value) = read(file)?;
    let mut findings = check_snapshot(file, &text, &value, project);
    findings.retain(|f| {
        !matches!(
            f.code,
            "flow.contract_stale" | "flow.approval_unsigned" | "flow.approval_unverifiable"
        )
    });
    if findings.iter().any(|f| f.code == "flow.approval_stale") {
        findings.retain(|f| f.code != "flow.approval_stale");
        value["status"] = json!("draft");
        value["approved"] = Value::Null;
    }
    if findings.iter().any(|f| !f.is_warning()) {
        Err(findings)
    } else {
        Ok((value, text))
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
    // #1287 follow-up: replay is proof, and a flow that does not say what it proves proves
    // nothing, even one approved before the rule. It is refused with the gaps it has to close.
    findings.retain(|f| f.code != "flow.weak_journey");
    findings.extend(weak_journey(&value, true));
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
    let backend = super::journey_backend::is_backend(flow);
    let http = super::journey_backend::needs_app(flow);
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
                match act["kind"].as_str() {
                    // #927: the frozen contract has no backend kinds; a request is submitted and
                    // a command activated, each named by its own label.
                    Some(kind @ ("http" | "cli")) => {
                        json!({"kind":if kind == "http" {"submit"} else {"activate"},"target":{"strategy":"label","value":act["name"],"role":act["role"],"geometryClaim":false}})
                    }
                    _ => {
                        json!({"kind":act["kind"],"target":{"strategy":"accessible_name","value":act["name"],"role":act["role"],"geometryClaim":false}})
                    }
                }
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
                        "{}{} {:?}{}",
                        if e["absent"] == true { "no " } else { "" },
                        e["role"].as_str().unwrap(),
                        e["name"].as_str().unwrap(),
                        e["text"]
                            .as_str()
                            .map_or(String::new(), |text| format!(" saying {text:?}"))
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
            let (fact, evidence, observer) = if !backend {
                ("content_rendered", "visual_capture", "browser")
            } else if http {
                ("request_accepted", "http_acceptance", "http")
            } else {
                ("command_completed", "process_exit", "process")
            };
            promises.push(json!({"promiseId":format!("{sid}.visible"),"stepId":sid,"statement":statement,"requiredFact":fact,"requiredEvidenceKinds":[evidence],"requiredObserverCapability":observer,"statesToObserve":[screen["state"]],"maxEvidenceAgeSeconds":604800}));
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
        // #1287 follow-up: a weak approved flow is not proof to keel either; it plans as a draft.
        if flow["status"] != "approved"
            || owner_signature(&flow, project)
                .iter()
                .any(|f| !f.is_warning())
            || !weak_journey(&flow, true).is_empty()
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

/// #1309 card 1c, #1684: the preview gate, one answer for approve and for `journey flows`'
/// `approvable`: a recorded passing preview of `flow` at its digest and scope commit, or the
/// finding approve refuses on.
fn preview_gate(project: &Path, id: &str, flow: &Value) -> Option<Finding> {
    let scope_commit = super::journey_preview::scope_commit_of(project, id, flow);
    let passed =
        super::journey_owner::previewed(project, id, &approval_digest(flow), &scope_commit)
            .unwrap_or(false);
    (!passed).then(|| {
        Finding::new(
            "flow.preview_not_passed",
            "",
            "approve requires a recorded passing preview of this flow at its scope commit",
        )
    })
}

/// The findings `run_approve` refuses on: everything `semantic` reports except what approval
/// itself clears (a stale binding, drift), plus non-canonical bytes.
fn approval_findings(file: &Path, text: &str, flow: &Value, project: &Path) -> Vec<Finding> {
    let mut findings = semantic(file, flow, project);
    findings.retain(|f| !matches!(f.code, "flow.approval_stale" | "flow.approved_with_drift"));
    // #1287: nothing is approved that does not say what it proves.
    findings.retain(|f| f.code != "flow.weak_journey");
    findings.extend(weak_journey(flow, true));
    // #1785: nor a submit whose effect is never checked where it shows.
    findings.extend(outcome_elsewhere(flow, true));
    // #1955: nothing is approved that does not say why each step and check is there.
    findings.retain(|f| f.code != "flow.unexplained");
    findings.extend(unexplained(flow, true));
    findings.extend(observer_missing(flow, true));
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
    // #1972: the entries are built on a few threads, order kept, so one flow's `git log`
    // (`preview_gate`) does not wait for every flow before it.
    let width = files.len().div_ceil(FLOWS_THREADS).max(1);
    let flows: Vec<Value> = std::thread::scope(|threads| {
        let jobs: Vec<_> = files
            .chunks(width)
            .map(|chunk| {
                let project = &project;
                threads.spawn(move || {
                    chunk
                        .iter()
                        .map(|file| flow_entry(file, project))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter()
            .flat_map(|job| job.join().unwrap_or_default())
            .collect()
    });
    Outcome::success(COMMAND, json!({"flows": flows}))
}

/// #1972: how many threads `journey flows` builds its entries on.
const FLOWS_THREADS: usize = 8;

/// One `journey flows` entry for one flow source.
fn flow_entry(file: &Path, project: &Path) -> Value {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let id = name.trim_end_matches(".journey.yaml");
    let findings = check(file, project);
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
                    "screens": [], "edges": [], "paths": {},
                    "lastRun": super::journey_preview::last_run(project, id, None),
                    "running": super::journey_preview::running(project, id)});
    };
    let stale = findings.iter().any(|f| f.code == "flow.approval_stale");
    let drifted = flow["drift"].as_array().is_some_and(|d| !d.is_empty());
    let settled = flow["status"] == "approved" && !stale && !drifted;
    // #1287 follow-up: an approved flow that does not say what it proves is not settled:
    // `weak` with no approved digest, so no replay cached under that digest reads as proof.
    let weak = flow["status"] == "approved" && !weak_journey(&flow, true).is_empty();
    let settled = settled && !weak;
    // #1972: `approvable` is `accepted && !settled`, so a settled flow skips the checks
    // (and `preview_gate`'s `git log`) whose answer it would discard.
    let accepted = !settled
        && approval_findings(file, &text, &flow, project)
            .iter()
            .all(Finding::is_warning)
        && preview_gate(project, id, &flow).is_none();
    let status = if stale {
        json!("approval_stale")
    } else if weak {
        json!("weak")
    } else {
        flow["status"].clone()
    };
    let approved = if weak {
        Value::Null
    } else {
        flow["approved"].clone()
    };
    json!({"id": id, "title": flow.get("title").cloned().unwrap_or(Value::Null),
                "purpose": flow.get("purpose").cloned().unwrap_or(Value::Null),
                "name": flow.get("name").cloned().unwrap_or(Value::Null),
                "feature": flow.get("feature").cloned().unwrap_or(Value::Null),
                "status": status, "approved": approved,
                // #1588: the owner's latest rejection of this exact content, with its reason.
                "rejected": super::journey_owner::rejection(project, id, &approval_digest_of(&flow))
                    .unwrap_or(Value::Null),
                "drift": flow["drift"],
                "findings": listed, "approvable": accepted && !settled,
                "screens": flow["screens"], "edges": flow["edges"], "paths": flow["paths"],
                // #1835: the last finished run as its runner recorded it; `not_run` when none.
                "lastRun": super::journey_preview::last_run(project, id, Some(&flow)),
                // #1953: the run in flight, `null` when none, so a reloaded Studio shows it.
                "running": super::journey_preview::running(project, id)})
}

/// `graphhelm journey approve <id>` from a terminal (#534): the owner's door, so it asks for the
/// owner's token (`--token-file`, the project's `.graphhelm/events.token`) and refuses without it.
/// The Runtime route and the MCP tool authenticate the owner themselves and call `approve_owned`.
pub(crate) fn run_approve(args: &crate::args::JourneyApproveArgs) -> Outcome {
    const COMMAND: &str = "journey.approve";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    if !owner_token_matches(&project, args.token_file.as_deref()) {
        return input_error(
            COMMAND,
            "approving is the owner's here: give --token-file with the project's owner token (.graphhelm/events.token); a reviewer agent approves through the Runtime (POST /v1/journey-flows/{id}/review)",
        );
    }
    approve_owned(&args.id, project)
}

/// `graphhelm journey reject <id> --note <text>` (#1588): the owner's door, like `approve`.
pub(crate) fn run_reject(args: &crate::args::JourneyRejectArgs) -> Outcome {
    const COMMAND: &str = "journey.reject";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    if !owner_token_matches(&project, args.token_file.as_deref()) {
        return input_error(
            COMMAND,
            "rejecting is the owner's: give --token-file with the project's owner token (.graphhelm/events.token)",
        );
    }
    reject_owned(&args.id, &args.note, project)
}

/// The rejection itself, for a caller that has already authenticated the owner (#1588). It binds
/// the digest of the flow's current content, so an edit after it clears it.
pub(crate) fn reject_owned(id: &str, note: &str, project: std::path::PathBuf) -> Outcome {
    const COMMAND: &str = "journey.reject";
    let Some(files) = files(&project, &[id.to_owned()]) else {
        return input_error(COMMAND, "invalid flow id or unsafe journeys directory");
    };
    let flow = match read(&files[0]) {
        Ok((_, flow)) => flow,
        Err(findings) => return report(COMMAND, vec![], findings, json!({})),
    };
    let digest = approval_digest_of(&flow);
    if let Err(message) = super::journey_owner::record_rejection(&project, id, &digest, note) {
        return input_error(COMMAND, &message);
    }
    Outcome::success(
        COMMAND,
        json!({"id": id, "rejected": super::journey_owner::rejection(&project, id, &digest)}),
    )
}

/// Whether `token_file` holds the project's owner token (#534): the owner's doors ask for it.
pub(crate) fn owner_token_matches(project: &Path, token_file: Option<&Path>) -> bool {
    let owner = super::secret_file::token_path(&super::journey_owner::store(project));
    let given =
        token_file.and_then(|path| super::secret_file::read_existing(path, "bearer token").ok());
    given.is_some() && given == super::secret_file::read_existing(&owner, "bearer token").ok()
}

/// `graphhelm journey sign-legacy` (#534 slice 3): approvals made before the owner's record
/// existed read as `flow.approval_unsigned` on the owner's machine. Any lane can also write an
/// `approved` block into a YAML (the digest is a plain sha256), and this command cannot tell the
/// two apart by itself (#597 review). So it never signs in bulk: it lists each unsigned approval
/// with the commit, author and date that introduced it (git's pickaxe on the digest), and signs
/// one flow per `--id` the owner names after reading that. An approval that no commit introduced
/// (only in the working tree), or a flow whose approval no longer matches its content, is listed
/// as not signable and is never signed. The YAML is not changed. Owner door: owner token.
pub(crate) fn run_sign_legacy(args: &crate::args::JourneySignLegacyArgs) -> Outcome {
    const COMMAND: &str = "journey.sign_legacy";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    if !owner_token_matches(&project, args.token_file.as_deref()) {
        return input_error(
            COMMAND,
            "signing approvals is the owner's: give --token-file with the project's owner token (.graphhelm/events.token)",
        );
    }
    let Some(files) = files(&project, &[]) else {
        return input_error(COMMAND, "unsafe journeys directory");
    };
    let (mut unsigned, mut not_signable) = (Vec::new(), Vec::new());
    for file in files {
        let Ok((text, flow)) = read(&file) else {
            continue;
        };
        if flow["status"] != "approved" {
            continue;
        }
        let id = flow["id"].as_str().unwrap_or_default().to_owned();
        let findings = check_snapshot(&file, &text, &flow, &project);
        if !findings.iter().any(|f| f.code == "flow.approval_unsigned") {
            continue;
        }
        let mut blocking: Vec<&str> = findings
            .iter()
            .filter(|f| !f.is_warning() && f.code != "flow.approval_unsigned")
            .map(|f| f.code)
            .collect();
        let digest = flow["approved"]["digest"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let revision = flow["approved"]["revision"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let introduced = introduced_by(&project, &file, &digest);
        if introduced.is_none() {
            blocking.push("flow.approval_uncommitted");
        }
        if blocking.is_empty() {
            unsigned.push((id, digest, revision, introduced.unwrap_or(Value::Null)));
        } else {
            not_signable.push(json!({"id": id, "findings": blocking}));
        }
    }
    let unknown: Vec<_> = args
        .ids
        .iter()
        .filter(|id| !unsigned.iter().any(|(candidate, ..)| candidate == *id))
        .collect();
    let mut signed = Vec::new();
    for (id, digest, revision, _) in unsigned.iter().filter(|(id, ..)| args.ids.contains(id)) {
        if let Err(message) = super::journey_owner::record(&project, id, digest, revision) {
            return input_error(COMMAND, &message);
        }
        signed.push(id.clone());
    }
    Outcome::success(
        COMMAND,
        json!({
            "unsigned": unsigned.iter().map(|(id, digest, revision, introduced)| json!({
                "id": id, "digest": digest, "revision": revision, "introducedBy": introduced,
            })).collect::<Vec<_>>(),
            "signed": signed,
            "notSignable": not_signable,
            "notUnsigned": unknown,
        }),
    )
}

/// The commit that introduced `digest` into the flow file (git's pickaxe), as
/// `{commit, author, date, subject}`; `None` when no commit did (the approval only exists in the
/// working tree) or git cannot answer.
fn introduced_by(project: &Path, file: &Path, digest: &str) -> Option<Value> {
    let relative = file
        .strip_prefix(project)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    let hex = digest.strip_prefix("sha256:")?;
    let line = super::journey::git(
        project,
        &[
            "log",
            "-1",
            "--format=%H%x09%an%x09%aI%x09%s",
            "-S",
            hex,
            "--",
            &relative,
        ],
    )?;
    let mut fields = line.trim().splitn(4, '\t');
    let commit = fields.next().filter(|commit| !commit.is_empty())?;
    Some(
        json!({"commit": commit, "author": fields.next(), "date": fields.next(), "subject": fields.next()}),
    )
}

/// The approval itself, for a caller that has already authenticated the owner (#534).
pub(crate) fn approve_owned(id: &str, project: std::path::PathBuf) -> Outcome {
    approve_as(id, project)
}

/// The digest `flow` will carry once approved: what an approval and a `drafted` record bind.
fn approval_digest_of(flow: &Value) -> String {
    let mut flow = flow.clone();
    flow["status"] = json!("approved");
    flow["drift"] = json!([]);
    if let Some(edges) = flow["edges"].as_array_mut() {
        for edge in edges {
            if let Some(edge) = edge.as_object_mut() {
                edge.remove("safe");
            }
        }
    }
    flow["approved"] = Value::Null;
    approval_digest(&flow)
}

/// The current draft of `id` and the digest an approval would bind, or the refusal.
fn draft_digest(command: &'static str, id: &str, project: &Path) -> Result<String, Outcome> {
    let Some(files) = files(project, &[id.to_owned()]) else {
        return Err(input_error(
            command,
            "invalid flow id or unsafe journeys directory",
        ));
    };
    let (_, flow) =
        read(&files[0]).map_err(|findings| report(command, vec![], findings, json!({})))?;
    if flow["status"] == "approved" {
        return Err(input_error(command, "the flow is already approved"));
    }
    Ok(approval_digest_of(&flow))
}

/// `POST /v1/journey-flows/{id}/draft` (#1130 follow-up): `writer` (an authenticated scoped agent)
/// writes the draft's YAML through the Runtime, and the store records that lane as a writer of the
/// resulting digest. A draft is reviewable only at a digest some lane wrote this way, so a file
/// edited outside the route has no author and cannot be approved. The new bytes must change the
/// file: re-posting the current content cannot claim someone else's work.
pub(crate) fn write_draft(
    id: &str,
    project: std::path::PathBuf,
    writer: &str,
    yaml: &str,
) -> Outcome {
    const COMMAND: &str = "journey.draft";
    if yaml.len() as u64 > MAX_FLOW_BYTES {
        return input_error(COMMAND, "flow exceeds 32 KiB");
    }
    let Some(files) = files(&project, &[id.to_owned()]) else {
        return input_error(COMMAND, "invalid flow id or unsafe journeys directory");
    };
    let Ok(_lock) = write_lock(&project) else {
        return input_error(COMMAND, "journey-flow write lock unavailable or unsafe");
    };
    let file = &files[0];
    let previous = std::fs::read(file).ok();
    if previous.as_deref() == Some(yaml.as_bytes()) {
        return input_error(
            COMMAND,
            "the draft is unchanged: only a lane that changes a flow is recorded as its writer",
        );
    }
    let restore = |previous: &Option<Vec<u8>>| match previous {
        Some(bytes) => std::fs::write(file, bytes).is_ok(),
        None => std::fs::remove_file(file).is_ok(),
    };
    if std::fs::write(file, yaml).is_err() {
        restore(&previous);
        return input_error(COMMAND, "the draft could not be written");
    }
    let flow = match read(file) {
        Ok((_, flow))
            if flow["id"] == id && flow["status"] == "draft" && flow["approved"].is_null() =>
        {
            flow
        }
        Ok(_) => {
            restore(&previous);
            return input_error(
                COMMAND,
                "a draft names this id, says `status: draft` and has `approved: null`",
            );
        }
        Err(findings) => {
            restore(&previous);
            return report(COMMAND, vec![], findings, json!({}));
        }
    };
    // #1178: review approves only canonical bytes, and the digest ignores the difference, so a
    // non-canonical draft would be accepted here and refused only at the reviewer. Refuse it now.
    if yaml != canonical(&flow, false) {
        restore(&previous);
        return report(
            COMMAND,
            vec![],
            vec![Finding::new(
                "flow.not_canonical",
                "",
                "the draft is not canonical YAML: review approves only canonical bytes (`graphhelm journey compile` formats them)",
            )],
            json!({}),
        );
    }
    // #1955: an agent's draft says why each step and each non-obvious check is there.
    let gaps = unexplained(&flow, true);
    if !gaps.is_empty() {
        restore(&previous);
        return report(COMMAND, vec![], gaps, json!({}));
    }
    let digest = approval_digest_of(&flow);
    if let Err(message) = super::journey_owner::record_drafted(&project, id, &digest, writer) {
        restore(&previous);
        return input_error(COMMAND, &message);
    }
    Outcome::success(
        COMMAND,
        json!({"id": id, "digest": digest, "writer": writer}),
    )
}

/// `POST /v1/journey-flows/{id}/review` (#1130): `reviewer` (an authenticated scoped agent)
/// records its blind review of the draft. #1309 ruling A: the review never approves; the flow stays
/// a draft until the owner approves it. Fails closed: no lane wrote this exact digest through the
/// draft route, or the reviewer ever wrote any version of the flow, refuses and writes nothing.
pub(crate) fn review_approve(
    id: &str,
    project: std::path::PathBuf,
    reviewer: &str,
    checked: &str,
) -> Outcome {
    const COMMAND: &str = "journey.review";
    let digest = match draft_digest(COMMAND, id, &project) {
        Ok(digest) => digest,
        Err(outcome) => return outcome,
    };
    let author = match super::journey_owner::drafted_by(&project, id, &digest) {
        Ok(Some(author)) => author,
        Ok(None) => {
            return input_error(
                COMMAND,
                "no lane wrote this draft through the Runtime: its author writes it with POST /v1/journey-flows/{id}/draft",
            );
        }
        Err(message) => return input_error(COMMAND, &message),
    };
    match super::journey_owner::ever_wrote(&project, id, reviewer) {
        Ok(false) => {}
        Ok(true) => {
            return input_error(
                COMMAND,
                "the reviewer wrote a version of this flow: a journey is approved by a lane that never wrote it",
            );
        }
        Err(message) => return input_error(COMMAND, &message),
    }
    let approver = match super::journey_owner::Approver::agent(reviewer, &author, checked) {
        Ok(approver) => approver,
        Err(message) => return input_error(COMMAND, &message),
    };
    let revision = match super::journey::head(&project) {
        Ok(rev) if rev.len() == 40 => rev,
        _ => {
            return input_error(
                COMMAND,
                "project git revision is missing or unsupported; a review requires a committed SHA-1 repository",
            );
        }
    };
    if let Err(message) =
        super::journey_owner::record_as(&project, id, &digest, &revision, &approver)
    {
        return input_error(COMMAND, &message);
    }
    Outcome::success(
        COMMAND,
        json!({"id": id, "digest": digest, "reviewer": reviewer, "author": author,
            "reviewed": true, "status": "draft"}),
    )
}

/// The owner's approval (#1309 ruling A: final approval is the owner's alone; a reviewer agent
/// records a review through `review_approve` and never reaches here).
pub(crate) fn approve_as(id: &str, project: std::path::PathBuf) -> Outcome {
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
    let mut findings = approval_findings(file, &text, &flow, &project);
    // #1309 ruling A, card 1c: the owner approves what the preview runner recorded passing on
    // this flow's digest and scope commit. A kept `state.json` is a cache and proves nothing.
    findings.extend(preview_gate(&project, &args.id, &flow));
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
/// `graphhelm journey label <id> --name .. --feature ..` (#1296): writes the display labels and
/// re-renders the canonical file. Refuses if the approved digest would move (it never should).
pub(crate) fn run_label(args: &crate::args::JourneyLabelArgs) -> Outcome {
    const COMMAND: &str = "journey.label";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    let clean = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    let (name, feature) = (clean(&args.name), clean(&args.feature));
    if name.is_none() && feature.is_none() {
        return input_error(COMMAND, "give --name and/or --feature");
    }
    if [&name, &feature]
        .iter()
        .any(|v| v.as_ref().is_some_and(|v| v.chars().count() > 60))
    {
        return input_error(COMMAND, "a label is at most 60 characters");
    }
    let Some(files) = files(&project, std::slice::from_ref(&args.id)) else {
        return input_error(COMMAND, "invalid flow id or unsafe journeys directory");
    };
    let Ok(_lock) = write_lock(&project) else {
        return input_error(COMMAND, "journey-flow write lock unavailable or unsafe");
    };
    let file = &files[0];
    let (_, mut flow) = match read(file) {
        Ok(value) => value,
        Err(findings) => return report(COMMAND, vec![], findings, json!({})),
    };
    let before = approval_digest(&flow);
    if let Some(name) = &name {
        flow["name"] = json!(name);
    }
    if let Some(feature) = &feature {
        flow["feature"] = json!(feature);
    }
    if approval_digest(&flow) != before {
        return input_error(
            COMMAND,
            "labelling would change the approved digest; nothing written",
        );
    }
    if atomic_write(file, canonical(&flow, false).as_bytes()).is_err() {
        return input_error(COMMAND, "could not write the flow");
    }
    Outcome::success(
        COMMAND,
        json!({"id": args.id, "name": flow["name"], "feature": flow["feature"], "digest": before}),
    )
}

pub(crate) fn run_mark_safe(args: &crate::args::JourneyMarkSafeArgs) -> Outcome {
    const COMMAND: &str = "journey.mark_safe";
    let project = args.project.clone().unwrap_or_else(|| ".".into());
    if !owner_token_matches(&project, args.token_file.as_deref()) {
        return input_error(
            COMMAND,
            "marking an act safe is the owner's: give --token-file with the project's owner token (.graphhelm/events.token)",
        );
    }
    mark_safe_owned(&args.id, &args.edge, project)
}

/// The mark itself, for a caller that has already authenticated the owner (#534 slice 2).
pub(crate) fn mark_safe_owned(id: &str, edge: &str, project: std::path::PathBuf) -> Outcome {
    const COMMAND: &str = "journey.mark_safe";
    let args = crate::args::JourneyMarkSafeArgs {
        id: id.to_owned(),
        edge: edge.to_owned(),
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
    // #534 slice 2: the owner's record first, then the YAML, so a mark never exists unsigned.
    if let Err(message) = super::journey_owner::record_mark(&project, &args.id, &args.edge, &digest)
    {
        return report(
            COMMAND,
            vec![],
            vec![Finding::new("flow.owner_record_failed", "/edges", message)],
            json!({}),
        );
    }
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
        "schema",
        "id",
        "title",
        "name",
        "feature",
        "purpose",
        "status",
        "approved",
        "base",
        "actors",
        "secrets",
        "risks",
        "viewport",
        "viewports",
        "storage",
        "clock",
        "states",
    ] {
        // #1296: `name` and `feature` are labels: kept in the file's canonical bytes, never in
        // what the owner approves.
        if approval_projection && matches!(field, "status" | "approved" | "name" | "feature") {
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
            &[
                "id",
                "title",
                "intent",
                "url",
                "ignoreQuery",
                "state",
                "expect",
                "scope",
            ][..],
        ),
        (
            "edges",
            &["id", "from", "to", "intent", "why", "acts", "safe"][..],
        ),
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
                        inline(v, &["role", "name", "absent", "text", "because", "digest"])
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
                for field in ["edge", "act", "code", "seen", "at", "healed", "viewport"] {
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
    /// #1633 follow-up (gh-claude-18): the mark is the whole token. Lookalikes do not fire; the
    /// token beside punctuation or a space does. Cost: microseconds.
    #[test]
    fn only_the_whole_observer_missing_token_marks_a_flow() {
        let flow = |title: &str| json!({"title": title, "screens": [], "edges": []});
        for lookalike in [
            "NOT_OBSERVER_MISSING",
            "OBSERVER_MISSING_FIXED",
            "XOBSERVER_MISSING2",
        ] {
            assert!(
                super::observer_missing(&flow(lookalike), true).is_empty(),
                "{lookalike}"
            );
        }
        for marked in [
            "(OBSERVER_MISSING: #1042)",
            "card: OBSERVER_MISSING #1042",
            "OBSERVER_MISSING",
        ] {
            assert_eq!(
                super::observer_missing(&flow(marked), true).len(),
                1,
                "{marked}"
            );
        }
    }

    use super::*;

    /// #1287 follow-up: replay is proof. Defect named: a flow approved before the rule, still
    /// without a purpose, replays and counts as proven. Cost: one tempdir.
    #[test]
    fn a_weak_flow_is_never_replayed_as_proof() {
        let project = tempfile::tempdir().unwrap();
        let mut flow: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        flow.as_object_mut().unwrap().remove("purpose");
        let dir = project.path().join(".graphhelm/journeys");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("checkout.journey.yaml");
        std::fs::write(&file, canonical(&flow, false)).unwrap();
        // The Studio reads `journey flows`: a weak approved flow is `weak` with no approved
        // digest, so a replay cached under its old digest reads as stale, never as passed.
        let mut approved = flow.clone();
        approved["status"] = json!("approved");
        let digest = approval_digest(&approved);
        approved["approved"] = json!({"revision": "a".repeat(40), "digest": digest});
        std::fs::write(&file, canonical(&approved, false)).unwrap();
        let listed = run_flows(&crate::args::JourneyFlowsArgs {
            project: Some(project.path().to_path_buf()),
        });
        let entry = &listed.output.data.unwrap()["flows"][0];
        assert_eq!(entry["status"], "weak", "{entry}");
        assert!(entry["approved"].is_null(), "{entry}");
        std::fs::write(&file, canonical(&flow, false)).unwrap();
        let refused = read_for_replay(&file, project.path()).unwrap_err();
        assert!(
            refused
                .iter()
                .any(|f| f.code == "flow.purpose_missing" && !f.is_warning()),
            "{:?}",
            refused.iter().map(|f| f.code).collect::<Vec<_>>()
        );
    }

    /// #1286: a screen can say what it is for. Defect named: the canonical writer drops a screen
    /// `intent` (so the owner's words vanish on format), or the approval ignores it (so changing
    /// what a screen proves leaves an approval valid). An intent-free flow keeps its bytes.
    /// Cost: in memory only.
    #[test]
    fn a_screen_intent_is_written_validated_and_approved() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let mut with = plain.clone();
        with["screens"][0]["intent"] = json!("Confirma que a obra foi criada");
        let schemas = flow_schemas().unwrap();
        let base = schemas.validate(FLOW_SCHEMA_ID, &plain, "journey-flow");
        assert!(base.is_empty(), "fixture invalid: {base:?}");
        let accepted = schemas.validate(FLOW_SCHEMA_ID, &with, "journey-flow");
        assert!(accepted.is_empty(), "{accepted:?}");
        let mut empty = plain.clone();
        empty["screens"][0]["intent"] = json!("");
        assert!(
            !schemas
                .validate(FLOW_SCHEMA_ID, &empty, "journey-flow")
                .is_empty()
        );
        let written = canonical(&with, false);
        assert!(
            written.contains("intent: Confirma que a obra foi criada"),
            "{written}"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(canonical(&reread, false), written);
        assert_ne!(approval_digest(&with), approval_digest(&plain));
        let mut edited = with.clone();
        edited["screens"][0]["intent"] = json!("Outra coisa");
        assert_ne!(approval_digest(&edited), approval_digest(&with));
        // Only the one line is new: a flow without a screen intent keeps its exact bytes.
        assert_eq!(
            written.replacen(
                &format!("{}intent: Confirma que a obra foi criada\n", " ".repeat(4)),
                "",
                1
            ),
            canonical(&plain, false)
        );
    }

    /// #1890: a screen may name query keys its URL match ignores. Defects named: the schema
    /// refuses the field (or takes an empty or duplicated list), the canonical writer drops it,
    /// or the approval ignores it (so widening what a screen accepts leaves an approval valid).
    /// A flow without the field keeps its exact bytes. Cost: in memory only.
    #[test]
    fn a_screen_ignore_query_is_written_validated_and_approved() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let mut with = plain.clone();
        with["screens"][0]["ignoreQuery"] = json!(["session", "project"]);
        let schemas = flow_schemas().unwrap();
        let accepted = schemas.validate(FLOW_SCHEMA_ID, &with, "journey-flow");
        assert!(accepted.is_empty(), "{accepted:?}");
        for bad in [json!([]), json!(["session", "session"]), json!(["a b"])] {
            let mut refused = plain.clone();
            refused["screens"][0]["ignoreQuery"] = bad.clone();
            assert!(
                !schemas
                    .validate(FLOW_SCHEMA_ID, &refused, "journey-flow")
                    .is_empty(),
                "{bad}"
            );
        }
        let written = canonical(&with, false);
        let line = format!("{}ignoreQuery: [session, project]\n", " ".repeat(4));
        assert!(written.contains(&line), "{written}");
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(canonical(&reread, false), written);
        assert_ne!(approval_digest(&with), approval_digest(&plain));
        assert_eq!(written.replacen(&line, "", 1), canonical(&plain, false));
    }

    /// #1955: a journey explains itself to whoever approves it. Defects named: the canonical writer
    /// drops `why` or `because` (so formatting erases the explanation and the approval ignores it),
    /// the schema takes an empty one, a draft with an unexplained step or command check is accepted
    /// by the strict doors, an obvious browser check is wrongly demanded a reason, or an approved
    /// flow without them is broken instead of warned. Cost: in memory only.
    #[test]
    fn a_step_why_and_a_check_because_are_written_required_and_approved() {
        let mut plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        plain["edges"][0]["why"] = json!("The shopper reaches the payment step");
        plain["edges"][1]["why"] = json!("Paying places the order");
        let schemas = flow_schemas().unwrap();
        let mut bare = plain.clone();
        for edge in bare["edges"].as_array_mut().unwrap() {
            edge.as_object_mut().unwrap().remove("why");
        }
        let codes = |found: Vec<Finding>| -> Vec<(&'static str, String)> {
            found.into_iter().map(|f| (f.code, f.pointer)).collect()
        };
        assert_eq!(
            codes(unexplained(&bare, true)),
            vec![
                ("flow.step_why_missing", "/edges/0/why".to_owned()),
                ("flow.step_why_missing", "/edges/1/why".to_owned()),
            ],
            "a browser check of a heading needs no because; every step needs a why"
        );
        // Not strict (an approved flow, preview, watch): the same gaps, as warnings.
        let warned = unexplained(&bare, false);
        assert!(
            warned
                .iter()
                .all(|f| f.code == "flow.unexplained" && f.is_warning())
        );
        assert_eq!(warned.len(), 2);
        assert!(
            unexplained(&plain, true).is_empty(),
            "the fixture explains itself"
        );

        // A command step: every expectation on the screen it leads to needs its because.
        let mut command = plain.clone();
        command["edges"][1]["acts"] = json!([{"kind": "cli", "role": "exec", "name": "sh pay.sh"}]);
        command["screens"][1]["expect"] = json!([
            {"role": "exit", "name": "0", "because": "the payment script succeeds"},
            {"role": "heading", "name": "Order placed"}
        ]);
        assert_eq!(
            codes(unexplained(&command, true)),
            vec![(
                "flow.expect_because_missing",
                "/screens/1/expect/1/because".to_owned()
            )]
        );
        assert!(
            schemas
                .validate(FLOW_SCHEMA_ID, &command, "journey-flow")
                .is_empty()
        );
        for (field, at) in [
            ("why", "/edges/0/why"),
            ("because", "/screens/1/expect/0/because"),
        ] {
            let mut empty = command.clone();
            let target = if field == "why" {
                &mut empty["edges"][0]
            } else {
                &mut empty["screens"][1]["expect"][0]
            };
            target[field] = json!("");
            assert!(
                schemas
                    .validate(FLOW_SCHEMA_ID, &empty, "journey-flow")
                    .iter()
                    .any(|d| d.path.starts_with(at)),
                "an empty {field} is refused"
            );
        }

        // Written, read back the same, and part of what the owner approves.
        let written = canonical(&command, false);
        assert!(
            written.contains(&format!("{}why: ", " ".repeat(4))),
            "{written}"
        );
        assert!(
            written.contains("{role: exit, name: \"0\", because: the payment script succeeds}"),
            "{written}"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(canonical(&reread, false), written);
        let mut reworded = command.clone();
        reworded["screens"][1]["expect"][0]["because"] = json!("another reason");
        assert_ne!(approval_digest(&reworded), approval_digest(&command));
        let mut rewhy = command.clone();
        rewhy["edges"][0]["why"] = json!("another reason");
        assert_ne!(approval_digest(&rewhy), approval_digest(&command));
    }

    /// #1041: "this thing is gone" is an expectation. Defects named: the canonical writer drops
    /// `absent` (so `journey compile --fmt` silently turns "gone" into "shown" and the approval
    /// ignores it), the schema takes `absent: false` (two spellings of one meaning), or a screen
    /// whose every expectation is an absence passes on a page that never rendered.
    /// Cost: in memory only.
    #[test]
    fn an_absent_expectation_is_written_validated_and_approved() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let schemas = flow_schemas().unwrap();
        let gone = json!({"role":"region","name":"Banner","absent":true});
        let mut with = plain.clone();
        with["screens"][0]["expect"]
            .as_array_mut()
            .unwrap()
            .push(gone.clone());
        let accepted = schemas.validate(FLOW_SCHEMA_ID, &with, "journey-flow");
        assert!(accepted.is_empty(), "{accepted:?}");
        let written = canonical(&with, false);
        assert!(
            written.contains("{role: region, name: Banner, absent: true}"),
            "{written}"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(
            reread["screens"], with["screens"],
            "absent survives the round trip"
        );
        assert_ne!(approval_digest(&with), approval_digest(&plain));
        // The same pair shown instead of gone is a different approval.
        let mut shown = with.clone();
        let last = shown["screens"][0]["expect"].as_array().unwrap().len() - 1;
        shown["screens"][0]["expect"][last]
            .as_object_mut()
            .unwrap()
            .remove("absent");
        assert_ne!(approval_digest(&shown), approval_digest(&with));
        let mut spelled_false = with.clone();
        spelled_false["screens"][0]["expect"][last]["absent"] = json!(false);
        let mut only_absent = plain.clone();
        only_absent["screens"][0]["expect"] = json!([gone]);
        for refused in [spelled_false, only_absent] {
            assert!(
                !schemas
                    .validate(FLOW_SCHEMA_ID, &refused, "journey-flow")
                    .is_empty(),
                "{refused}"
            );
        }
        // A flow without `absent` keeps its exact bytes.
        assert_eq!(
            canonical(&plain, false),
            canonical(
                &serde_yaml_ng::from_str(&canonical(&plain, false)).unwrap(),
                false
            )
        );
    }

    /// #1777: a flow names the viewports its Run proves. Defects named: the writer drops
    /// `viewports` or the approval ignores it, a fixed `viewport` beside it is accepted (the Run
    /// could not say which size it proved), or adding the field moves the bytes or digest of flows
    /// that never use it. Cost: in memory only.
    #[test]
    fn a_flow_viewports_list_is_written_approved_and_exclusive_with_a_fixed_viewport() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let schemas = flow_schemas().unwrap();
        let file = Path::new("checkout.journey.yaml");
        assert_eq!(
            approval_digest(&plain),
            "sha256:aace853dc94a68d337b4485ce74cc878b13f5d291d2dff3e40d2ec5004fc7072",
            "a flow without viewports keeps the digest origin/main computes"
        );
        let mut web = plain.clone();
        web["viewports"] = json!(["web"]);
        assert!(
            schemas
                .validate(FLOW_SCHEMA_ID, &web, "journey-flow")
                .is_empty()
        );
        assert!(
            !semantic(file, &web, Path::new("."))
                .iter()
                .any(|f| f.code == "flow.viewport_and_viewports"),
            "web alone is a valid opt-out"
        );
        let written = canonical(&web, false);
        assert!(
            written.contains(
                "
viewports: [web]
"
            ),
            "{written}"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(reread["viewports"], web["viewports"]);
        assert_ne!(approval_digest(&web), approval_digest(&plain));
        for shape in [
            json!([]),
            json!(["tablet"]),
            json!(["web", "web"]),
            json!("web"),
        ] {
            let mut bad = plain.clone();
            bad["viewports"] = shape.clone();
            assert!(
                !schemas
                    .validate(FLOW_SCHEMA_ID, &bad, "journey-flow")
                    .is_empty(),
                "{shape}"
            );
        }
        let mut both = web.clone();
        both["viewport"] = json!({"width": 390, "height": 844});
        assert!(
            schemas
                .validate(FLOW_SCHEMA_ID, &both, "journey-flow")
                .is_empty(),
            "the schema stays additive"
        );
        assert!(
            semantic(file, &both, Path::new("."))
                .iter()
                .any(|f| f.code == "flow.viewport_and_viewports" && f.pointer == "/viewports")
        );
    }

    /// #1784: a flow names the starting states its screens must be seen in. Defects named: the
    /// writer drops `states` (so `--fmt` silently returns the flow to one state) or the approval
    /// ignores it, a malformed list is accepted, or adding the field moves the bytes or digest of
    /// flows that never use it. Cost: in memory only.
    #[test]
    fn a_flow_states_list_is_written_approved_and_leaves_stateless_flows_alone() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let schemas = flow_schemas().unwrap();
        let plain_bytes = canonical(&plain, false);
        assert!(!plain_bytes.contains("states"), "{plain_bytes}");
        assert_eq!(
            approval_digest(&plain),
            "sha256:aace853dc94a68d337b4485ce74cc878b13f5d291d2dff3e40d2ec5004fc7072",
            "a flow without states keeps the digest origin/main computes"
        );
        assert!(declared_states(&plain).is_empty());
        let mut with = plain.clone();
        with["states"] = json!(["on_the_way", "delivered"]);
        assert!(
            schemas
                .validate(FLOW_SCHEMA_ID, &with, "journey-flow")
                .is_empty()
        );
        let written = canonical(&with, false);
        assert_eq!(
            written,
            plain_bytes.replacen(
                "\nscreens:",
                "\nstates: [on_the_way, delivered]\nscreens:",
                1
            ),
            "only the states line is added"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(declared_states(&reread), ["on_the_way", "delivered"]);
        assert_ne!(approval_digest(&with), approval_digest(&plain));
        let mut reordered = with.clone();
        reordered["states"] = json!(["delivered", "on_the_way"]);
        assert_ne!(approval_digest(&reordered), approval_digest(&with));
        for shape in [
            json!([]),
            json!(["Delivered"]),
            json!(["a", "a"]),
            json!("delivered"),
            json!(["a..b"]),
        ] {
            let mut bad = plain.clone();
            bad["states"] = shape.clone();
            assert!(
                !schemas
                    .validate(FLOW_SCHEMA_ID, &bad, "journey-flow")
                    .is_empty(),
                "{shape}"
            );
        }
    }

    /// #1782: a flow's `clock` is the instant its browser runs see as now. Defects named: the
    /// canonical writer drops `clock` (so `--fmt` silently unfixes the date) or the approval
    /// ignores it, an instant that does not exist passes validation and only fails in the
    /// browser, or adding the field moves the bytes or digest of flows that never use it.
    /// Cost: in memory only.
    #[test]
    fn a_flow_clock_is_written_validated_and_approved() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let schemas = flow_schemas().unwrap();
        let file = Path::new("checkout.journey.yaml");
        // A flow without `clock` keeps its canonical bytes and approval digest; the literal is
        // what origin/main (ce0918b22) computes for this fixture.
        let plain_bytes = canonical(&plain, false);
        assert!(!plain_bytes.contains("clock"), "{plain_bytes}");
        assert_eq!(
            approval_digest(&plain),
            "sha256:aace853dc94a68d337b4485ce74cc878b13f5d291d2dff3e40d2ec5004fc7072",
            "the digest origin/main computes for this fixture"
        );
        let mut with = plain.clone();
        with["clock"] = json!("2026-10-31T12:00:00-03:00");
        assert!(
            schemas
                .validate(FLOW_SCHEMA_ID, &with, "journey-flow")
                .is_empty()
        );
        assert!(
            !semantic(file, &with, Path::new("."))
                .iter()
                .any(|f| f.code == "flow.clock_invalid")
        );
        let written = canonical(&with, false);
        assert_eq!(
            written,
            plain_bytes.replacen(
                "\nscreens:",
                "\nclock: 2026-10-31T12:00:00-03:00\nscreens:",
                1
            ),
            "only the clock line is added"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(reread["clock"], with["clock"]);
        assert_ne!(approval_digest(&with), approval_digest(&plain));
        let mut later = with.clone();
        later["clock"] = json!("2026-11-01T12:00:00-03:00");
        assert_ne!(approval_digest(&later), approval_digest(&with));
        // Shape is the schema's; existence is the validator's, with a stable code and pointer.
        for shape in ["2026-10-31T12:00:00", "2026-10-31 12:00:00Z", "tomorrow"] {
            let mut bad = plain.clone();
            bad["clock"] = json!(shape);
            assert!(
                !schemas
                    .validate(FLOW_SCHEMA_ID, &bad, "journey-flow")
                    .is_empty(),
                "{shape}"
            );
        }
        for impossible in [
            "2026-13-01T12:00:00Z",
            "2026-02-30T12:00:00Z",
            "2026-10-31T24:00:00Z",
            "2026-10-31T12:00:60Z",
            "2026-10-31T12:00:00+15:00",
        ] {
            let mut bad = plain.clone();
            bad["clock"] = json!(impossible);
            assert!(
                schemas
                    .validate(FLOW_SCHEMA_ID, &bad, "journey-flow")
                    .is_empty(),
                "{impossible} has the right shape"
            );
            assert!(
                semantic(file, &bad, Path::new("."))
                    .iter()
                    .any(|f| f.code == "flow.clock_invalid" && f.pointer == "/clock"),
                "{impossible}"
            );
        }
    }

    /// #1781: "the screen says X" is an expectation. Defects named: the canonical writer drops
    /// `text` (so `--fmt` silently turns "says R$ 246,90" into "has a status"), the approval
    /// ignores it, the schema takes `text` beside `absent` (a contradiction), or adding the field
    /// moves the digest of flows that never use it. Cost: in memory only.
    #[test]
    fn a_text_expectation_is_written_validated_and_approved() {
        let plain: Value = serde_yaml_ng::from_str(include_str!(
            "../../tests/fixtures/journey_flow/checkout.journey.yaml"
        ))
        .unwrap();
        let schemas = flow_schemas().unwrap();
        let plain_digest = approval_digest(&plain);
        let mut with = plain.clone();
        with["screens"][0]["expect"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"status","name":"Total","text":"R$ 246,90"}));
        let accepted = schemas.validate(FLOW_SCHEMA_ID, &with, "journey-flow");
        assert!(accepted.is_empty(), "{accepted:?}");
        let written = canonical(&with, false);
        assert!(
            written.contains("{role: status, name: Total, text: \"R$ 246,90\"}"),
            "{written}"
        );
        let reread: Value = serde_yaml_ng::from_str(&written).unwrap();
        assert_eq!(
            reread["screens"], with["screens"],
            "text survives the round trip"
        );
        assert_ne!(approval_digest(&with), plain_digest);
        let last = with["screens"][0]["expect"].as_array().unwrap().len() - 1;
        let mut other = with.clone();
        other["screens"][0]["expect"][last]["text"] = json!("R$ 24,69");
        assert_ne!(approval_digest(&other), approval_digest(&with));
        let mut both = with.clone();
        both["screens"][0]["expect"][last]["absent"] = json!(true);
        // Text beside absent passes the additive schema but is refused with a stable code.
        let file = Path::new("checkout.journey.yaml");
        let refused = semantic(file, &both, Path::new("."));
        assert!(
            refused.iter().any(|f| f.code == "flow.text_with_absent"
                && f.pointer == format!("/screens/0/expect/{last}/text")),
            "{:?}",
            refused
                .iter()
                .map(|f| (f.code, &f.pointer))
                .collect::<Vec<_>>()
        );
        assert!(
            !semantic(file, &with, Path::new("."))
                .iter()
                .any(|f| f.code == "flow.text_with_absent")
        );
        let mut empty = with.clone();
        empty["screens"][0]["expect"][last]["text"] = json!("");
        assert!(
            !schemas
                .validate(FLOW_SCHEMA_ID, &empty, "journey-flow")
                .is_empty(),
            "{empty}"
        );
        // A flow without `text` keeps its digest.
        assert_eq!(approval_digest(&plain), plain_digest);
    }

    /// #1296: `name` and `feature` only label a journey for the Studio rail. Defect named: adding
    /// or renaming them changes the approved digest, so labelling an approved journey makes it
    /// stale. Cost: in memory only.
    #[test]
    fn a_journey_name_and_feature_never_change_the_approved_digest() {
        let mut flow: Value = serde_yaml_ng::from_str(
            "schema: graphhelm.journey-flow/1\nid: itens\ntitle: Usuario da obra cria um item novo\nstatus: draft\napproved: null\nbase: http://127.0.0.1:3000\nactors: [owner]\nsecrets: []\nrisks: []\nscreens:\n  - {id: a, url: /a, state: stable, expect: [], scope: unknown}\nedges: []\npaths: {main: []}\ndrift: []\n",
        )
        .unwrap();
        let before = approval_digest(&flow);
        flow["name"] = json!("Cadastrar novo item");
        flow["feature"] = json!("Itens");
        assert_eq!(approval_digest(&flow), before);
        // The labels survive the canonical writer, so a labelled file is still canonical.
        let text = canonical(&flow, false);
        assert!(
            text.contains("name: Cadastrar novo item\nfeature: Itens\n"),
            "{text}"
        );
    }

    /// #1785: Zuvemo's `entrega` shape. The item is opened from the agenda, "Chegou" is submitted
    /// on the item page, and the flow ends on that page's `status Entregue`; the delivered item
    /// stayed on the agenda and nothing looked. Defects named: that flow passes approval or its
    /// warning does not name the act and the agenda; a path that goes back to the agenda after the
    /// act and expects there is still flagged; a flow with no other screen on the item is flagged;
    /// or validation (non-strict) refuses instead of warning. Cost: in memory only.
    #[test]
    fn outcome_elsewhere_names_the_screen_the_act_changes() {
        let entrega: Value = serde_yaml_ng::from_str(
            "schema: graphhelm.journey-flow/1\nid: entrega\nstatus: draft\napproved: null\nbase: http://127.0.0.1:3000\nactors: [owner]\nsecrets: []\nrisks: []\nscreens:\n  - {id: agenda, url: /agenda, state: stable, expect: [{role: heading, name: Agenda}, {role: link, name: Sofa}], scope: unknown}\n  - {id: a-caminho, url: /itens/:id, state: stable, expect: [{role: status, name: A caminho}], scope: unknown}\n  - {id: entregue, url: /itens/:id, state: success, expect: [{role: status, name: Entregue}], scope: unknown}\n  - {id: agenda-depois, url: /agenda, state: success, expect: [{role: heading, name: Agenda}, {role: link, name: Sofa, absent: true}], scope: unknown}\nedges:\n  - {id: agenda.abrir, from: agenda, to: a-caminho, acts: [{kind: activate, role: link, name: Sofa}]}\n  - {id: a-caminho.chegou, from: a-caminho, to: entregue, acts: [{kind: submit, role: button, name: Chegou}]}\n  - {id: entregue.agenda, from: entregue, to: agenda-depois, acts: [{kind: activate, role: link, name: Agenda}]}\npaths:\n  main: [agenda.abrir, a-caminho.chegou]\ndrift: []\n",
        )
        .unwrap();
        let strict = outcome_elsewhere(&entrega, true);
        assert_eq!(strict.len(), 1);
        assert_eq!(strict[0].code, "flow.outcome_elsewhere_unchecked");
        assert_eq!(strict[0].pointer, "/edges/1/acts/0");
        assert!(strict[0].message.contains("`Chegou`") && strict[0].message.contains("agenda"));
        assert!(!strict[0].is_warning());
        let warned = outcome_elsewhere(&entrega, false);
        assert_eq!(warned.len(), 1);
        assert_eq!(warned[0].code, "flow.weak_journey");
        assert!(warned[0].is_warning());
        assert!(
            warned[0]
                .message
                .starts_with("flow.outcome_elsewhere_unchecked: ")
        );

        let mut back = entrega.clone();
        back["paths"]["volta"] = json!(["a-caminho.chegou", "entregue.agenda"]);
        assert!(outcome_elsewhere(&back, true).is_empty());

        // A button is a command, not the item: checkout's "Checkout" or sign-in's "Entrar".
        let mut command = entrega.clone();
        command["screens"][0]["expect"][1]["role"] = json!("button");
        command["edges"][0]["acts"][0]["role"] = json!("button");
        assert!(outcome_elsewhere(&command, true).is_empty());

        let mut alone = entrega;
        alone["paths"] = json!({"main": ["a-caminho.chegou"]});
        alone["edges"] = json!([alone["edges"][1].clone()]);
        assert!(outcome_elsewhere(&alone, true).is_empty());
    }

    /// #1287: a journey that clicks, submits and checks only "success" proves nothing. Defects
    /// named: a draft without a purpose, a screen without a title, a submit with no input and no
    /// intent, or an end screen that expects nothing new is accepted; or an approved flow with
    /// the same gaps is broken instead of flagged. Cost: in memory only.
    #[test]
    fn a_journey_that_does_not_say_what_it_proves_is_refused() {
        let weak: Value = serde_yaml_ng::from_str(
            "schema: graphhelm.journey-flow/1\nid: lojas\nstatus: draft\napproved: null\nbase: http://127.0.0.1:3000\nactors: [owner]\nsecrets: []\nrisks: []\nscreens:\n  - {id: admin, url: /admin, state: stable, expect: [{role: link, name: Lojas}], scope: unknown}\n  - {id: lojas, title: Store tags, url: /admin/lojas, state: stable, expect: [{role: button, name: Salvar}], scope: unknown}\n  - {id: salvo, url: /admin/lojas, state: success, expect: [{role: button, name: Salvar}], scope: unknown}\nedges:\n  - {id: admin.lojas, from: admin, to: lojas, acts: [{kind: activate, role: link, name: Lojas}]}\n  - {id: lojas.salvar, from: lojas, to: salvo, acts: [{kind: submit, role: button, name: Salvar}]}\npaths:\n  main: [admin.lojas, lojas.salvar]\ndrift: []\n",
        )
        .unwrap();
        let codes = |flow: &Value| {
            weak_journey(flow, true)
                .into_iter()
                .map(|f| f.code)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            codes(&weak),
            [
                "flow.purpose_missing",
                "flow.screen_title_missing",
                "flow.screen_title_missing",
                "flow.submit_without_input",
                "flow.outcome_unchecked"
            ]
        );
        let flagged = weak_journey(&weak, false);
        assert!(
            flagged
                .iter()
                .all(|f| f.code == "flow.weak_journey" && f.is_warning())
        );
        assert_eq!(flagged.len(), 5);

        let mut strong = weak;
        strong["purpose"] =
            json!("The owner tags a store and the tag is still there after a reload.");
        strong["screens"][0]["title"] = json!("Admin home");
        strong["screens"][2]["title"] = json!("Saved tags, after a reload");
        strong["screens"][2]["expect"] = json!([{"role": "cell", "name": "parceiro"}]);
        strong["edges"][1]["acts"] = json!([
            {"kind": "enter_text", "role": "textbox", "name": "Tag Amazon", "text": "parceiro"},
            {"kind": "submit", "role": "button", "name": "Salvar"}
        ]);
        assert!(codes(&strong).is_empty(), "{:?}", codes(&strong));
        strong["edges"][1]["acts"] =
            json!([{"kind": "submit", "role": "button", "name": "Salvar"}]);
        strong["edges"][1]["intent"] = json!("Save the default tags unchanged");
        assert!(codes(&strong).is_empty());
    }

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
