//! #2156: `journey replay` of an approved flow whose acts are all `cli`. Such a flow has no page
//! for the browser driver to open, so the driver preflight refused its first act with
//! `driver.unsupported_act`. It is replayed the way `journey preview` plays it: the start screen is
//! judged against the start observation (exit 0, no output), each command runs through the same backend runner, and
//! each screen it reaches is judged against what the command printed. A command the preview guard
//! would hold as destructive is refused before anything runs.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::journey_backend as backend;
use super::journey_replay::{Failure, failure, safe_directory, safe_node};
use crate::args::JourneyReplayArgs;

type Result<T> = std::result::Result<T, Failure>;

/// A flow whose every act is a `cli` act (and that has at least one).
pub(super) fn cli_only(flow: &Value) -> bool {
    let mut acts = flow["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|edge| edge["acts"].as_array().into_iter().flatten())
        .peekable();
    acts.peek().is_some() && acts.all(|act| act["kind"] == "cli")
}

/// The approved flow `args.id` names when it is `cli`-only, else `None` (the browser replay
/// reports every other case, including an unreadable flow, exactly as before).
pub(super) fn flow_of(args: &JourneyReplayArgs) -> Option<(PathBuf, Value)> {
    let project = args
        .project
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .canonicalize()
        .ok()?;
    let file = project
        .join(".graphhelm/journeys")
        .join(format!("{}.journey.yaml", args.id));
    if !safe_node(&file) {
        return None;
    }
    let flow = super::journey_flow::read_for_replay(&file, &project).ok()?;
    cli_only(&flow).then_some((project, flow))
}

/// Replays every path of a `cli`-only approved flow, filling `data` as the browser replay does.
pub(super) fn replay(
    args: &JourneyReplayArgs,
    project: &std::path::Path,
    flow: &Value,
    data: &mut Value,
) -> Result<()> {
    safe_directory(&project.join(".graphhelm"), false)?;
    data["flowId"] = args.id.clone().into();
    data["flowDigest"] = super::journey_flow::approval_digest(flow).into();
    if args.heal {
        return Err(failure("replay.cli_heal_unsupported", "/heal", 3));
    }
    // Recorded proof is a chain of captured images; a command has none to capture.
    if args.record.events.is_some() {
        return Err(failure("replay.cli_unrecorded", "/recording", 3));
    }
    let edges: Vec<&Value> = flow["edges"].as_array().unwrap().iter().collect();
    // The same guard the preview applies to an approved flow's command: a destructive argv is
    // held, so replay refuses it before any command of the flow runs.
    for (index, edge) in edges.iter().enumerate() {
        for (act, value) in edge["acts"].as_array().unwrap().iter().enumerate() {
            if super::journey_live::would_destroy(value).is_some() {
                return Err(failure(
                    "replay.cli_destructive",
                    format!("/edges/{index}/acts/{act}"),
                    3,
                ));
            }
        }
    }
    let screen = |id: &str| {
        flow["screens"]
            .as_array()
            .unwrap()
            .iter()
            .find(|screen| screen["id"] == id)
            .unwrap()
    };
    let mut paths: Vec<_> = flow["paths"].as_object().unwrap().iter().collect();
    paths.sort_by(|(a, _), (b, _)| (a.as_str() != "main", a).cmp(&(b.as_str() != "main", b)));
    data["paths"] = json!([]);
    let mut failed = None;
    for (name, ids) in paths {
        let ids: Vec<&str> = ids
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        let mut path = json!({"name":name,"outcome":"passed","screens":{}});
        let first = edges.iter().find(|e| e["id"] == ids[0]).unwrap()["from"]
            .as_str()
            .unwrap();
        let origin = super::journey_preview::origin_frame();
        if let Some(unmet) =
            backend::unmet(screen(first), &super::journey_preview::origin_observation())
        {
            path["screens"][first] = json!({"result":"drift","seen":unmet,"evidence":origin});
            path["outcome"] = "drift".into();
            failed.get_or_insert(failure(
                "driver.expectation_failed",
                format!("/paths/{name}/screens/{first}"),
                1,
            ));
            data["paths"].as_array_mut().unwrap().push(path);
            continue;
        }
        path["screens"][first] = json!({"result":"pass","evidence":origin});
        for id in ids {
            let (index, edge) = edges
                .iter()
                .enumerate()
                .find(|(_, e)| e["id"] == id)
                .unwrap();
            let to = edge["to"].as_str().unwrap();
            let mut last = None;
            for act in edge["acts"].as_array().unwrap() {
                match backend::play(project, "", act) {
                    Ok(seen) => {
                        last = Some((
                            super::journey_preview::text_frame(&act["name"], &seen),
                            seen,
                        ))
                    }
                    Err(_) => {
                        last = None;
                        break;
                    }
                }
            }
            let Some((frame, seen)) = last else {
                path["screens"][to] = json!({"result":"fail","reason":"act_failed"});
                path["outcome"] = "failed".into();
                failed.get_or_insert(failure("replay.act_failed", format!("/edges/{index}"), 1));
                break;
            };
            match backend::unmet(screen(to), &seen) {
                None => path["screens"][to] = json!({"result":"pass","evidence":frame}),
                Some(unmet) => {
                    path["screens"][to] = json!({"result":"drift","seen":unmet,"evidence":frame});
                    path["outcome"] = "drift".into();
                    failed.get_or_insert(failure(
                        "driver.expectation_failed",
                        format!("/edges/{index}"),
                        1,
                    ));
                    break;
                }
            }
        }
        data["paths"].as_array_mut().unwrap().push(path);
    }
    failed.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flow mixing a browser act with a command is not `cli`-only: it keeps the browser replay
    /// and its refusal. An empty flow is not either. Cost: in memory.
    #[test]
    fn only_a_flow_of_commands_is_cli_only() {
        let cli = json!({"kind":"cli","role":"exec","name":"echo hi"});
        let click = json!({"kind":"activate","role":"button","name":"Go"});
        assert!(cli_only(&json!({"edges":[{"acts":[cli]}]})));
        assert!(!cli_only(
            &json!({"edges":[{"acts":[cli]},{"acts":[click]}]})
        ));
        assert!(!cli_only(&json!({"edges":[]})));
    }
}
