//! #431: a journey screen's `scope` is what `keel check` and `keel plan` compare a diff against, so
//! a scope that misses the file rendering one of the screen's own controls is the dangerous
//! direction: a change to that file maps no journey and is never replayed. `studio-refuse-question`
//! expected the `Team` region (rendered by `team-canvas.tsx`) but scoped only `beacon.tsx` and
//! `needs-you.ts`.
//!
//! The rule, for every `studio2-*` flow in this repository: each `expect` name that occurs literally
//! in some non-test Studio source file must occur in at least one file of that screen's scope.
//! Names that occur nowhere (fixture data such as a bot's name, or text built from a template) are
//! not judged; this guard can only see literal names. Cost: reads ~30 small files, no build output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn studio_sources(dir: &Path, out: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            studio_sources(&path, out);
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if (name.ends_with(".ts") || name.ends_with(".tsx")) && !name.contains(".test.") {
            let relative = path
                .strip_prefix(repo())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(relative, std::fs::read_to_string(&path).unwrap());
        }
    }
}

/// #2782: a screen reached by an `http` act expects a response (status, json, text) that no Studio
/// source renders; a literal like "403" in a source comparison is not that screen's control. The
/// observer matches the status exactly (journey_backend.rs:765), so the name is never renamed to
/// dodge the guard. Only those screens are skipped, never a whole flow.
fn http_screens(flow: &Value) -> Vec<&str> {
    let mut ids = Vec::new();
    for edge in flow["edges"].as_array().into_iter().flatten() {
        let http = edge["acts"]
            .as_array()
            .is_some_and(|acts| acts.iter().any(|act| act["kind"] == "http"));
        if http {
            ids.extend(["from", "to"].iter().filter_map(|end| edge[end].as_str()));
        }
    }
    ids
}

fn too_narrow(flow: &Value, sources: &BTreeMap<String, String>) -> Vec<String> {
    let mut misses = Vec::new();
    let http = http_screens(flow);
    for screen in flow["screens"].as_array().unwrap() {
        if http.contains(&screen["id"].as_str().unwrap()) {
            continue;
        }
        let Some(scope) = screen["scope"].as_array() else {
            continue; // `scope: unknown` claims nothing
        };
        let scope: Vec<&str> = scope.iter().filter_map(Value::as_str).collect();
        for expected in screen["expect"].as_array().unwrap() {
            let control = expected["name"].as_str().unwrap();
            let owners: Vec<&String> = sources
                .iter()
                .filter(|(_, text)| text.contains(control))
                .map(|(file, _)| file)
                .collect();
            if !owners.is_empty() && !owners.iter().any(|file| scope.contains(&file.as_str())) {
                misses.push(format!(
                    "{}/{}: {} {control:?} is rendered in {owners:?}, none of which is in its scope",
                    flow["id"].as_str().unwrap(),
                    screen["id"].as_str().unwrap(),
                    expected["role"].as_str().unwrap(),
                ));
            }
        }
    }
    misses
}

fn counter_example(http: bool) -> Value {
    let act = if http {
        serde_json::json!({"kind": "http", "role": "GET", "name": "/health"})
    } else {
        serde_json::json!({"kind": "activate", "role": "tab", "name": "Graph"})
    };
    serde_json::json!({
        "id": "counter",
        "screens": [
            {"id": "a", "scope": ["apps/studio/dev-session.ts"],
             "expect": [{"role": "status", "name": "403"}]},
            {"id": "b", "scope": ["apps/studio/dev-session.ts"],
             "expect": [{"role": "status", "name": "403"}]}
        ],
        "edges": [{"id": "a.b", "from": "a", "to": "b", "acts": [act]}]
    })
}

#[test]
fn the_guard_skips_only_screens_reached_by_http_acts() {
    let sources = BTreeMap::from([(
        "apps/studio/src/runtime/client.ts".to_string(),
        "status === 403".to_string(),
    )]);
    // A UI screen whose expected text only an unscoped file renders still fails.
    assert_eq!(too_narrow(&counter_example(false), &sources).len(), 2);
    // The same screens reached by an http act are responses, not rendered controls.
    assert!(too_narrow(&counter_example(true), &sources).is_empty());
    // A flow with one http edge keeps judging its UI screens.
    let mut mixed = counter_example(true);
    mixed["screens"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
        {"id": "ui", "scope": ["apps/studio/dev-session.ts"],
         "expect": [{"role": "status", "name": "403"}]}));
    assert_eq!(too_narrow(&mixed, &sources).len(), 1);
}

#[test]
fn every_studio_screen_scopes_a_file_that_renders_its_expected_controls() {
    let mut sources = BTreeMap::new();
    studio_sources(&repo().join("apps/studio/src"), &mut sources);
    // The Studio v2 app (#1808) has about 26 non-test sources; a missing or moved tree reads 0.
    assert!(sources.len() > 20, "the Studio sources were not found");

    let mut flows = 0;
    let mut misses = Vec::new();
    for entry in std::fs::read_dir(repo().join(".graphhelm/journeys")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("studio2-") && name.ends_with(".journey.yaml")) {
            continue;
        }
        flows += 1;
        let flow: Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        misses.extend(too_narrow(&flow, &sources));
    }
    assert!(flows > 0, "expected at least one studio2-* flow");
    assert!(
        misses.is_empty(),
        "too-narrow scopes:\n{}",
        misses.join("\n")
    );
}
