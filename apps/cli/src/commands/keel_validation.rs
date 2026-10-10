//! Isolated journey validation and conservative JavaScript coverage reporting.
//!
//! The validator is deliberately a supervisor: flow execution remains in the existing preview
//! runner, while this command inventories the input set and interprets only bounded collector
//! artifacts. A missing or incomplete collector never becomes negative coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel.test.validation";
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COVERAGE_BYTES: u64 = 16 * 1024 * 1024;
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "ts", "tsx", "jsx", "rs", "py", "go", "java", "kt", "swift", "c", "cc",
    "cpp", "h", "hpp", "css", "html", "vue", "svelte",
];

fn input_error(message: impl Into<String>, path: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        graphhelm_protocols::Diagnostic::error(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            message,
            path,
            "keel",
        ),
    )
}

fn digest(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hex::encode(hash.finalize())
}

fn inventory_digest(inventory: &BTreeMap<String, String>) -> String {
    let mut bytes = Vec::new();
    for (path, hash) in inventory {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(hash.as_bytes());
        bytes.push(0);
    }
    digest(&bytes)
}

fn tracked_files(repo: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .filter_map(|bytes| std::str::from_utf8(bytes).ok())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn inventory(repo: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for relative in tracked_files(repo)? {
        if !relative
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        {
            continue;
        }
        let mut absolute = repo.to_path_buf();
        for component in relative.components() {
            if !matches!(component, std::path::Component::Normal(_)) {
                return Err("source inventory contains an unsafe path".into());
            }
            absolute.push(component);
            if std::fs::symlink_metadata(&absolute).is_ok_and(|meta| meta.file_type().is_symlink())
            {
                return Err(format!(
                    "source inventory refuses symlink {}",
                    relative.display()
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(&absolute)
            .map_err(|error| format!("{}: {error}", relative.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "source inventory refuses symlink {}",
                relative.display()
            ));
        }
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_SOURCE_BYTES {
            return Err(format!("source file exceeds bound: {}", relative.display()));
        }
        let bytes =
            std::fs::read(&absolute).map_err(|error| format!("{}: {error}", relative.display()))?;
        result.insert(
            relative.to_string_lossy().replace('\\', "/"),
            digest(&bytes),
        );
    }
    Ok(result)
}

fn flows(repo: &Path) -> Result<Vec<(String, Value, PathBuf)>, String> {
    let directory = repo.join(".graphhelm").join("journeys");
    let entries = std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("yaml")
            || !path.to_string_lossy().ends_with(".journey.yaml")
        {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".journey.yaml"))
            .unwrap_or_default()
            .to_owned();
        match super::journey_flow::read_for_watch(&path, repo) {
            Ok(flow) => found.push((id, flow, path)),
            Err(findings) => found.push((
                id,
                json!({"status":"invalid", "findings": findings.iter().map(|f| f.code).collect::<Vec<_>>() }),
                path,
            )),
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(found)
}

fn coverage_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            files.extend(coverage_files(&path));
        } else if path.file_name().and_then(|name| name.to_str()) == Some("coverage.json") {
            if std::fs::symlink_metadata(&path)
                .ok()
                .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn read_coverage(path: &Path) -> Option<Value> {
    let metadata = std::fs::metadata(path).ok()?;
    (metadata.len() <= MAX_COVERAGE_BYTES).then_some(())?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn path_entry(flow: &str, path: &str, state: &str, collection: &str) -> Value {
    json!({"flow": flow, "path": path, "execution": state, "collection": collection,"negativeEvidenceEligible":false})
}

fn declared_path_names(flow: &Value) -> Vec<String> {
    if let Some(paths) = flow["paths"].as_object() {
        return paths.keys().cloned().collect();
    }
    flow["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|path| path["id"].as_str().or_else(|| path.as_str()))
        .map(str::to_owned)
        .collect()
}

fn collect_functions(
    artifact: &Value,
    current: &BTreeMap<String, String>,
    allow_negative: bool,
    used: &mut Vec<Value>,
    not_observed: &mut Vec<Value>,
    unknown: &mut Vec<Value>,
) {
    if artifact["schema"] != "graphhelm-js-coverage/1"
        || artifact.get("collection").and_then(Value::as_str) != Some("captured")
    {
        unknown.push(json!({"reason":"coverage_not_captured"}));
        return;
    }
    let navigation_count = artifact["navigationCount"].as_u64().unwrap_or(2);
    let complete = artifact["completeness"]["complete"]
        .as_bool()
        .unwrap_or(false);
    let negative_eligible = allow_negative
        && artifact["negativeEvidenceEligible"]
            .as_bool()
            .unwrap_or(false)
        && navigation_count <= 1
        && complete;
    for script in artifact["scripts"].as_array().into_iter().flatten() {
        let Some(generated) = script["generatedSha256"]
            .as_str()
            .filter(|hash| !hash.is_empty())
        else {
            unknown.push(json!({"reason":"generated_source_missing","flow":artifact["flow"],"path":artifact["path"]}));
            continue;
        };
        let Some(sources) = script["sources"].as_array() else {
            unknown.push(json!({"generatedSha256":generated,"reason":"source_mapping_missing"}));
            continue;
        };
        let associations: Vec<_> = sources
            .iter()
            .filter_map(|source| {
                let expected = source["sha256"].as_str()?;
                let matches: Vec<_> = current
                    .iter()
                    .filter_map(|(path, actual)| (actual == expected).then_some(path.clone()))
                    .collect();
                (matches.len() == 1).then(|| (matches[0].clone(), expected.to_owned()))
            })
            .collect();
        if sources.len() != 1 || associations.len() != 1 {
            unknown.push(
                json!({"generatedSha256":generated,"reason":"unknown_or_multiple_source_mapping"}),
            );
            continue;
        }
        let (source_path, source_sha256) = &associations[0];
        for function in script["functions"].as_array().into_iter().flatten() {
            if !function["ranges"]
                .as_array()
                .is_some_and(|ranges| !ranges.is_empty())
            {
                unknown
                    .push(json!({"reason":"function_ranges_missing","generatedSha256":generated}));
                continue;
            }
            let mut item = json!({"source":source_path,"sourceSha256":source_sha256,"generatedSha256":generated,
                "flow":artifact["flow"],"path":artifact["path"],"execution":artifact["execution"],
                "name":function["name"],"ranges":function["ranges"]});
            let count = function["ranges"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|range| range["count"].as_u64())
                .fold(0u64, u64::saturating_add);
            item["count"] = count.into();
            if count > 0 {
                used.push(item);
            } else if negative_eligible {
                not_observed.push(item);
            } else {
                unknown.push(json!({"source":source_path,"sourceSha256":source_sha256,"generatedSha256":generated,
                    "name":function["name"],"reason":"negative_evidence_ineligible"}));
            }
        }
    }
}

fn complete_artifact(artifact: &Value) -> bool {
    if artifact["schema"] != "graphhelm-js-coverage/1"
        || artifact["execution"] != "completed"
        || !artifact["flow"].is_string()
        || !artifact["path"].is_string()
        || artifact["collection"] != "captured"
        || artifact["completeness"]["complete"] != true
        || artifact["negativeEvidenceEligible"] != true
        || artifact["navigationCount"].as_u64() != Some(1)
    {
        return false;
    }
    let Some(scripts) = artifact["scripts"].as_array() else {
        return false;
    };
    if scripts.is_empty() {
        return false;
    }
    for script in scripts {
        if !script["generatedSha256"].is_string() || !script["sources"].is_array() {
            return false;
        }
        let Some(functions) = script["functions"].as_array() else {
            return false;
        };
        if functions.is_empty() {
            return false;
        }
        for function in functions {
            if !function["name"].is_string() {
                return false;
            }
            let Some(ranges) = function["ranges"].as_array() else {
                return false;
            };
            if ranges.is_empty() {
                return false;
            }
            for range in ranges {
                let (Some(start), Some(end), Some(_count)) = (
                    range["startOffset"].as_u64(),
                    range["endOffset"].as_u64(),
                    range["count"].as_u64(),
                ) else {
                    return false;
                };
                if start > end {
                    return false;
                }
            }
        }
    }
    true
}

// A function's outer range identifies it across runs. Inner branch ranges and counts vary.
fn function_key(item: &Value) -> String {
    format!(
        "{}:{}:{}:{}",
        item["generatedSha256"],
        item["name"],
        item["ranges"][0]["startOffset"],
        item["ranges"][0]["endOffset"]
    )
}

fn suppress_positive_candidates(used: &[Value], candidates: &mut Vec<Value>) {
    let positive: BTreeSet<_> = used.iter().map(function_key).collect();
    candidates.retain(|item| !positive.contains(&function_key(item)));
}

pub(crate) fn run(repo: &Path) -> Outcome {
    let repo = match repo.canonicalize() {
        Ok(path) if path.is_dir() => path,
        _ => return input_error("--repo must name a directory", "/repo"),
    };
    let before = match inventory(&repo) {
        Ok(value) => value,
        Err(error) => return input_error(error, "/repo"),
    };
    let before_hash = inventory_digest(&before);
    let discovered = match flows(&repo) {
        Ok(value) => value,
        Err(error) => return input_error(error, "/repo/.graphhelm/journeys"),
    };
    let flow_input_hash = digest(&serde_json::to_vec(&discovered).unwrap_or_default());
    let result_root = std::env::temp_dir().join(format!(
        "graphhelm-keel-validation-{}",
        uuid::Uuid::new_v4().simple()
    ));
    if std::fs::create_dir(&result_root).is_err() {
        return Outcome::internal(COMMAND, "could not create validation result directory");
    }
    let mut paths = Vec::new();
    let mut flows_report = Vec::new();
    let mut failures = Vec::new();
    for (id, flow, _) in &discovered {
        if flow["status"] == "invalid" {
            failures.push(json!({"flow":id,"reason":"flow_invalid"}));
            flows_report.push(json!({"id":id,"status":"invalid"}));
            continue;
        }
        let flow_dir = result_root.join(id);
        if let Err(error) = std::fs::create_dir_all(&flow_dir) {
            failures.push(json!({"flow":id,"reason":"validation_result_dir_failed","detail":error.to_string()}));
            flows_report.push(json!({"id":id,"status":"failed"}));
            continue;
        }
        let mut args = vec!["--json", "journey", "preview", id, "--run", "--project"];
        let project_text = repo.to_string_lossy().to_string();
        args.push(&project_text);
        let flow_dir_text = flow_dir.to_string_lossy().to_string();
        args.extend(["--validation-dir", &flow_dir_text]);
        let execution =
            Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("graphhelm")))
                .args(args)
                .stdin(Stdio::null())
                .output();
        let reply = execution
            .as_ref()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
            .unwrap_or(Value::Null);
        let result = &reply["data"];
        let mut successful = result["state"] == "ready" && result["result"] == "pass";
        for path in declared_path_names(flow) {
            let entry = &result["paths"][&path];
            let state = entry["execution"].as_str().unwrap_or("not_run");
            let collection = entry["collection"].as_str().unwrap_or("unavailable");
            successful &= state == "completed";
            paths.push(path_entry(id, &path, state, collection));
        }
        if !successful {
            failures.push(json!({"flow":id,"reason":"execution_failed","result":result["result"],"detail":result["reason"]}));
        }
        flows_report.push(json!({"id":id,"status":if successful {"completed"} else {"failed"},"resultDir":flow_dir}));
    }
    let after_result = inventory(&repo);
    let after_error = after_result.as_ref().err().cloned();
    let after = after_result.unwrap_or_default();
    let input_changed = after_error.is_some() || before != after;
    let journeys_changed = flows(&repo)
        .map(|current| digest(&serde_json::to_vec(&current).unwrap_or_default()) != flow_input_hash)
        .unwrap_or(true);
    let after_hash = inventory_digest(&after);
    let artifacts: Vec<Value> = coverage_files(&result_root)
        .iter()
        .filter_map(|path| read_coverage(path))
        .collect();
    let expected_paths: BTreeSet<String> = paths
        .iter()
        .filter_map(|path| {
            Some(format!(
                "{}\u{0}{}",
                path["flow"].as_str()?,
                path["path"].as_str()?
            ))
        })
        .collect();
    let collected_paths: BTreeSet<String> = artifacts
        .iter()
        .filter(|artifact| complete_artifact(artifact))
        .filter_map(|artifact| {
            Some(format!(
                "{}\u{0}{}",
                artifact["flow"].as_str()?,
                artifact["path"].as_str()?
            ))
        })
        .collect();
    for path in &mut paths {
        let key = format!(
            "{}\u{0}{}",
            path["flow"].as_str().unwrap_or_default(),
            path["path"].as_str().unwrap_or_default()
        );
        if let Some(artifact) = artifacts.iter().find(|artifact| {
            format!(
                "{}\u{0}{}",
                artifact["flow"].as_str().unwrap_or_default(),
                artifact["path"].as_str().unwrap_or_default()
            ) == key
        }) {
            path["negativeEvidenceEligible"] = complete_artifact(artifact).into();
            path["collection"] = if complete_artifact(artifact) {
                "captured"
            } else {
                "incomplete"
            }
            .into();
        }
    }
    let generated_contracts: BTreeSet<String> = discovered
        .iter()
        .flat_map(|(id, flow, _)| {
            declared_path_names(flow).into_iter().map(move |path| {
                if path == "main" {
                    format!("{id}.json")
                } else {
                    format!("{id}.{path}.json")
                }
            })
        })
        .collect();
    let standalone = std::fs::read_dir(repo.join(".graphhelm").join("journeys"))
        .ok().into_iter().flatten().filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter(|path| !generated_contracts.contains(path.file_name().and_then(|name| name.to_str()).unwrap_or_default()))
        .map(|path| json!({"path":path.strip_prefix(&repo).unwrap_or(&path).display().to_string(),"status":"unsupported"}))
        .collect::<Vec<_>>();
    let sweep_valid = !expected_paths.is_empty()
        && failures.is_empty()
        && standalone.is_empty()
        && artifacts.len() == expected_paths.len()
        && artifacts.iter().all(complete_artifact)
        && expected_paths.is_subset(&collected_paths)
        && !input_changed
        && !journeys_changed;
    let mut used = Vec::new();
    let mut not_observed = Vec::new();
    let mut unknown = Vec::new();
    for artifact in &artifacts {
        if !complete_artifact(artifact) {
            unknown.push(json!({"reason":"coverage_incomplete","flow":artifact["flow"],"path":artifact["path"],"limitations":artifact["limitations"]}));
        }
        collect_functions(
            artifact,
            &after,
            sweep_valid,
            &mut used,
            &mut not_observed,
            &mut unknown,
        );
    }
    suppress_positive_candidates(&used, &mut not_observed);
    let associated: BTreeSet<String> = used
        .iter()
        .chain(not_observed.iter())
        .filter_map(|item| item["source"].as_str().map(str::to_owned))
        .collect();
    for (source, source_sha256) in &after {
        if !associated.contains(source) {
            unknown.push(json!({"source":source,"sourceSha256":source_sha256,
                "reason":"unobserved_backend_or_mapping"}));
        }
    }
    if input_changed {
        unknown.push(json!({"reason":"source_changed_during_run"}));
        used.clear();
        not_observed.clear();
    }
    if let Some(error) = after_error {
        unknown.push(json!({"reason":"source_inventory_unavailable","detail":error}));
    }
    if journeys_changed {
        unknown.push(json!({"reason":"journeys_changed_during_run"}));
    }
    if artifacts.is_empty() {
        unknown.push(json!({"reason":"coverage_missing"}));
    }
    let used_count = used.len();
    let not_observed_count = not_observed.len();
    let failure_count = failures.len();
    let data = json!({
        "schema":"graphhelm-keel-validation/1",
        "repo": repo,
        "resultDir":result_root,
        "sweepComplete":sweep_valid,
        "flows": flows_report,
        "paths": paths,
        "declaredScopes": discovered.iter().flat_map(|(id, flow, _)| flow["screens"].as_array().into_iter().flatten().map(move |screen| json!({"flow":id,"screen":screen["id"],"scope":screen["scope"]}))).collect::<Vec<_>>(),
        "standaloneContracts": standalone,
        "used": used,
        "notObserved": not_observed,
        "unknown": unknown,
        "failures": failures,
        "sourceChanged": input_changed,
        "journeysChanged":journeys_changed,
        "sourceInventorySha256": {"before":before_hash,"after":after_hash},
        "deadConfirmed": [],
    });
    let unknown_count = data["unknown"].as_array().map_or(0, Vec::len);
    let blocking_uncertain = !sweep_valid || artifacts.is_empty() || input_changed;
    let summary = json!({"flows":data["flows"].as_array().map_or(0, Vec::len),
        "paths":data["paths"].as_array().map_or(0, Vec::len),
        "used":used_count,"notObserved":not_observed_count,"unknown":unknown_count,
        "failures":failure_count});
    let limitations = json!([
        "coverage is limited to captured browser JavaScript scripts",
        "backend, native, CSS, workers, missing maps and ambiguous source hashes remain unknown",
        "scope declarations are associations only and never execution evidence"
    ]);
    let next_check = if blocking_uncertain {
        "rerun after every declared path has isolated fixture execution and complete coverage"
    } else {
        "review unknown source associations and backend/native coverage separately"
    };
    let mut data = data;
    data["summary"] = summary;
    data["limitations"] = limitations;
    data["nextCheck"] = next_check.into();
    Outcome {
        output: CommandOutput {
            ok: failure_count == 0 && !blocking_uncertain,
            command: COMMAND,
            data: Some(data),
            diagnostics: Vec::new(),
        },
        exit_code: if failure_count == 0 && !blocking_uncertain {
            0
        } else {
            2
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(sources: Value) -> Value {
        json!({"schema":"graphhelm-js-coverage/1","flow":"fixture","path":"main","execution":"completed","collection":"captured","complete":true,
            "completeness":{"complete":true},"negativeEvidenceEligible":true,
            "navigationCount":1,"scripts":[{"generatedSha256":"gen",
                "sources":sources,"functions":[{"name":"called","ranges":[{"startOffset":0,"endOffset":1,"count":1}]},
                    {"name":"untouched","ranges":[{"startOffset":2,"endOffset":3,"count":0}]}]}]})
    }

    #[test]
    fn source_hash_only_mapping_is_associated_when_unique() {
        let mut current = BTreeMap::new();
        current.insert("src/app.js".to_owned(), "source".to_owned());
        let mut used = Vec::new();
        let mut untouched = Vec::new();
        let mut unknown = Vec::new();
        collect_functions(
            &artifact(json!([{"sha256":"source"}])),
            &current,
            true,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        assert_eq!(used[0]["source"], "src/app.js");
        assert_eq!(used[0]["sourceSha256"], "source");
        assert_eq!(untouched[0]["name"], "untouched");
        assert!(unknown.is_empty());
    }

    #[test]
    fn stale_or_multiple_source_hashes_are_unknown() {
        let mut current = BTreeMap::new();
        current.insert("src/a.js".to_owned(), "same".to_owned());
        current.insert("src/b.js".to_owned(), "same".to_owned());
        let mut used = Vec::new();
        let mut untouched = Vec::new();
        let mut unknown = Vec::new();
        collect_functions(
            &artifact(json!([{"sha256":"same"}])),
            &current,
            true,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        collect_functions(
            &artifact(json!([{"sha256":"stale"}])),
            &current,
            true,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        assert!(used.is_empty());
        assert!(untouched.is_empty());
        assert_eq!(unknown.len(), 2);
    }

    #[test]
    fn a_positive_hit_elsewhere_removes_the_global_zero_hit_candidate() {
        let mut current = BTreeMap::new();
        current.insert("src/app.js".to_owned(), "source".to_owned());
        let mut used = Vec::new();
        let mut untouched = Vec::new();
        let mut unknown = Vec::new();
        collect_functions(
            &artifact(json!([{"sha256":"source"}])),
            &current,
            true,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        let mut second = artifact(json!([{"sha256":"source"}]));
        second["scripts"][0]["functions"][0]["ranges"][0]["count"] = 0.into();
        collect_functions(
            &second,
            &current,
            true,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        suppress_positive_candidates(&used, &mut untouched);
        assert!(untouched.iter().all(|item| item["name"] != "called"));
    }

    #[test]
    fn incomplete_collection_never_creates_zero_hit_evidence() {
        let mut current = BTreeMap::new();
        current.insert("src/app.js".to_owned(), "source".to_owned());
        let mut incomplete = artifact(json!([{"sha256":"source"}]));
        incomplete["completeness"]["complete"] = false.into();
        let mut used = Vec::new();
        let mut untouched = Vec::new();
        let mut unknown = Vec::new();
        collect_functions(
            &incomplete,
            &current,
            false,
            &mut used,
            &mut untouched,
            &mut unknown,
        );
        assert_eq!(used.len(), 1, "positive hits survive incomplete collection");
        assert!(untouched.is_empty());
        assert!(
            unknown
                .iter()
                .any(|item| item["reason"] == "negative_evidence_ineligible")
        );
    }
    #[test]
    fn navigation_and_malformed_artifacts_cannot_complete_a_sweep() {
        let valid = artifact(json!([{"sha256":"source"}]));
        assert!(complete_artifact(&valid));
        let mut navigation = valid.clone();
        navigation["navigationCount"] = 2.into();
        navigation["negativeEvidenceEligible"] = false.into();
        assert!(!complete_artifact(&navigation));
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("scripts");
        assert!(!complete_artifact(&missing));
        let mut failed = valid;
        failed["execution"] = "held".into();
        assert!(!complete_artifact(&failed));
    }
}
