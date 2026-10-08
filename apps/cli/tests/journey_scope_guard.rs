//! #431: a journey screen's `scope` is what `keel check` and `keel plan` compare a diff against, so
//! a scope that misses the file rendering one of the screen's own controls is the dangerous
//! direction: a change to that file maps no journey and is never replayed. `studio-refuse-question`
//! expected the `Team` region (rendered by `team-canvas.tsx`) but scoped only `beacon.tsx` and
//! `needs-you.ts`.
//!
//! The rule, for every `studio-*` flow in this repository: each `expect` name that occurs literally
//! in some non-test Studio source file must occur in at least one file of that screen's scope.
//! Names that occur nowhere (fixture data such as a bot's name, or text built from a template) are
//! not judged; this guard can only see literal names. Cost: reads ~200 small files, no build output.

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

#[test]
fn every_studio_screen_scopes_a_file_that_renders_its_expected_controls() {
    let mut sources = BTreeMap::new();
    studio_sources(&repo().join("apps/studio/src"), &mut sources);
    assert!(sources.len() > 50, "the Studio sources were not found");

    let mut flows = 0;
    let mut misses = Vec::new();
    for entry in std::fs::read_dir(repo().join(".graphhelm/journeys")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("studio-") && name.ends_with(".journey.yaml")) {
            continue;
        }
        flows += 1;
        let flow: Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for screen in flow["screens"].as_array().unwrap() {
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
    }
    assert!(flows >= 20, "expected the studio-* flows, found {flows}");
    assert!(
        misses.is_empty(),
        "too-narrow scopes:\n{}",
        misses.join("\n")
    );
}
