use graphhelm_protocols::Diagnostic;

use crate::args::{AdoptionBackupArgs, AdoptionSetupArgs};
use crate::output::Outcome;

const COMMAND: &str = "setup";
const SOURCE: &str = "adoption-cli";
const REFUSED: &str = crate::error_codes::GHCLI029_ADOPTION_REFUSED;

pub(super) fn run(args: &AdoptionSetupArgs) -> Outcome {
    if let Some(path) = &args.plan {
        return reviewed(args, path);
    }
    if let Some(path) = &args.apply {
        return mutation(args, Some(path));
    }
    if args.recover.is_some() {
        return mutation(args, None);
    }
    let provisioning = match super::init::describe(&crate::args::InitArgs {
        project: Some(args.project.clone()),
        bind: "127.0.0.1:8791".into(),
        key_id: "studio".into(),
        harness: vec![
            crate::args::Harness::ClaudeCode,
            crate::args::Harness::Codex,
        ],
    }) {
        Ok(description) => description.public_description(),
        Err(_) => {
            return refused(graphhelm_protocols::adoption::AdoptionError {
                reason: graphhelm_protocols::adoption::AdoptionReason::InvalidConfiguration,
            });
        }
    };
    let result = (|| {
        use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
        let inventory = graphhelm_host_adoption::inventory(&args.project, &args.home)?;
        if args.resolve.is_empty() {
            let plan = graphhelm_host_adoption::propose(&inventory)?;
            if let Some(out) = &args.out {
                graphhelm_host_adoption::write_private(out, &pretty(&plan)?)?;
            }
            return Ok(
                serde_json::json!({"inventory": inventory, "plan": plan, "provisioning": provisioning}),
            );
        }
        // A resolved plan carries the reviewed after-bytes. They go to the private file only;
        // without --out there is nowhere safe to put them, so the run is refused before it reads
        // any replacement file.
        let out = args.out.as_deref().ok_or(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })?;
        let resolutions = args
            .resolve
            .iter()
            .map(|text| parse_resolution(text))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = graphhelm_host_adoption::resolve(&inventory, &resolutions)?;
        graphhelm_host_adoption::write_private(out, &pretty(&plan)?)?;
        let digest = plan["digest"].clone();
        Ok(serde_json::json!({
            "inventory": inventory,
            "plan": graphhelm_host_adoption::redact(plan),
            "provisioning": provisioning,
            "acceptance": {
                "mode": "explicit_digest",
                "digest": digest,
                "instruction": "The private plan file is written. Review it, then use --apply with that file, --state-root, and --accept with this exact digest. A pipe never confirms automatically.",
            }
        }))
    })();
    match result {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => refused(error),
    }
}

fn pretty(
    plan: &serde_json::Value,
) -> Result<Vec<u8>, graphhelm_protocols::adoption::AdoptionError> {
    let mut bytes = serde_json::to_vec_pretty(plan).map_err(|_| {
        graphhelm_protocols::adoption::AdoptionError {
            reason: graphhelm_protocols::adoption::AdoptionReason::LimitExceeded,
        }
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// `<item>=keep` or `<item>=replace:<file>`. The file is read with the same bound as a plan.
fn parse_resolution(
    text: &str,
) -> Result<graphhelm_host_adoption::Resolution, graphhelm_protocols::adoption::AdoptionError> {
    use graphhelm_policy::adoption::Decision;
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    let invalid = || AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    };
    let (item, decision) = text.split_once('=').ok_or_else(invalid)?;
    if item.is_empty() || item.len() > 4096 {
        return Err(invalid());
    }
    if decision == "keep" {
        return Ok(graphhelm_host_adoption::Resolution {
            item: item.to_owned(),
            decision: Decision::Keep,
            after: None,
        });
    }
    let file = decision.strip_prefix("replace:").ok_or_else(invalid)?;
    if file.is_empty() {
        return Err(invalid());
    }
    Ok(graphhelm_host_adoption::Resolution {
        item: item.to_owned(),
        decision: Decision::Replace,
        after: Some(read_bytes(std::path::Path::new(file))?),
    })
}

fn read_bytes(
    path: &std::path::Path,
) -> Result<Vec<u8>, graphhelm_protocols::adoption::AdoptionError> {
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    use std::io::Read;
    const MAX: u64 = 4 * 1024 * 1024;
    let invalid = || AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    };
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| invalid())?
        .take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > MAX {
        return Err(invalid());
    }
    Ok(bytes)
}

fn read_document(
    path: &std::path::Path,
) -> Result<serde_json::Value, graphhelm_protocols::adoption::AdoptionError> {
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    use std::io::Read;
    let invalid = || AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    };
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| invalid())?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn reviewed(args: &AdoptionSetupArgs, path: &std::path::Path) -> Outcome {
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    let result = (|| {
        let plan = read_document(path)?;
        let mut payload = plan.clone();
        payload
            .as_object_mut()
            .ok_or(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?
            .remove("digest");
        let actual =
            graphhelm_schema_evolution::schema_digest(&payload).map_err(|_| AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            })?;
        if plan["digest"] != actual.as_str() {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
        if !graphhelm_schema::validate_adoption_plan(&plan).is_empty()
            || plan["spec"]["rootBindings"]
                != graphhelm_host_adoption::root_bindings(&args.project, &args.home)?
        {
            return Err(AdoptionError {
                reason: AdoptionReason::PlanStale,
            });
        }
        if let Some(path) = &args.verify {
            let receipt = read_document(path)?;
            graphhelm_host_adoption::verify_activation_at(
                args.state_root.as_deref().ok_or(AdoptionError {
                    reason: AdoptionReason::InvalidConfiguration,
                })?,
                &plan,
                &receipt,
            )
            .map(|receipt| serde_json::json!({"receipt":receipt}))
        } else {
            Ok(
                serde_json::json!({"plan":preview(&plan),"acceptance":{"mode":"explicit_digest","instruction":"This is a redacted view: operation contents are shown only as digests and byte lengths. Review the private plan file itself for the full text, then use --apply with that file and --accept with this exact digest. A pipe never confirms automatically."}}),
            )
        }
    })();
    match result {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => refused(error),
    }
}

/// #1208: the `--plan` preview of a sealed plan. The plan file is private — it carries the full
/// `after` text of every instruction file and setting the owner resolved — so the envelope (and
/// the rendered face, which reads only the envelope) gets an allow-listed view instead of the
/// document: enough to review and to accept (`digest`), never an `after` body. A field the plan
/// grows later is not printed until it is named here.
fn preview(plan: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Value, json};
    let pick = |value: &Value, fields: &[&str]| -> Value {
        Value::Object(
            fields
                .iter()
                .filter_map(|field| value.get(*field).map(|v| ((*field).to_owned(), v.clone())))
                .collect(),
        )
    };
    let each = |pointer: &str, fields: &[&str]| -> Value {
        plan.pointer(pointer)
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(|row| pick(row, fields)).collect())
            .unwrap_or_default()
    };
    let spec = &plan["spec"];
    let operations = spec["operations"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let mut view = pick(
                        row,
                        &[
                            "root",
                            "path",
                            "beforeDigest",
                            "afterDigest",
                            "disableSkills",
                        ],
                    );
                    view["afterBytes"] = json!(row["after"].as_str().map(str::len));
                    view
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "apiVersion": plan["apiVersion"],
        "kind": plan["kind"],
        "id": plan["id"],
        "digest": plan["digest"],
        "redacted": true,
        "spec": {
            "coverage": spec["coverage"],
            "scopes": spec["scopes"],
            "hostBoundary": spec["hostBoundary"],
            "host": pick(&spec["host"], &["name", "version", "mode"]),
            "packages": each("/spec/packages", &["id", "version", "digest"]),
            "decisions": each("/spec/decisions", &["operationIndex", "item", "decision", "protected"]),
            "review": each("/spec/review", &["item", "decision"]),
            "operations": operations,
        }
    })
}

pub(super) fn backup(args: &AdoptionBackupArgs) -> Outcome {
    match graphhelm_host_adoption::backup(&args.project, &args.home, &args.state_root) {
        Ok(receipt) => Outcome::success(COMMAND_BACKUP, serde_json::json!({"receipt": receipt})),
        Err(error) => Outcome::domain(
            COMMAND_BACKUP,
            vec![Diagnostic::error(
                REFUSED,
                error.to_string(),
                error.reason.pointer(),
                SOURCE,
            )],
        ),
    }
}

const COMMAND_BACKUP: &str = "backup";

fn refused(error: graphhelm_protocols::adoption::AdoptionError) -> Outcome {
    Outcome::domain(
        COMMAND,
        vec![Diagnostic::error(
            REFUSED,
            error.to_string(),
            error.reason.pointer(),
            SOURCE,
        )],
    )
}

fn mutation(args: &AdoptionSetupArgs, plan_path: Option<&std::path::PathBuf>) -> Outcome {
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    use std::io::Read;
    let result = (|| {
        let state = args.state_root.as_deref().ok_or(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })?;
        if let Some(path) = plan_path {
            const MAX_PLAN_BYTES: u64 = 4 * 1024 * 1024;
            let metadata = std::fs::symlink_metadata(path).map_err(|_| AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_PLAN_BYTES
            {
                return Err(AdoptionError {
                    reason: AdoptionReason::LimitExceeded,
                });
            }
            let file = std::fs::File::open(path).map_err(|_| AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
            let mut bytes = Vec::new();
            file.take(MAX_PLAN_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| AdoptionError {
                    reason: AdoptionReason::InvalidConfiguration,
                })?;
            if bytes.len() as u64 > MAX_PLAN_BYTES {
                return Err(AdoptionError {
                    reason: AdoptionReason::LimitExceeded,
                });
            }
            let plan = serde_json::from_slice(&bytes).map_err(|_| AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
            graphhelm_host_adoption::apply_with_packages(
                &args.project,
                &args.home,
                state,
                &plan,
                args.accept.as_deref().unwrap_or(""),
                &args.packages,
            )
        } else {
            graphhelm_host_adoption::recover(state, args.recover.as_deref().unwrap_or(""))
        }
    })();
    match result {
        Ok(receipt) => Outcome::success(COMMAND, serde_json::json!({"receipt": receipt})),
        Err(error) => refused(error),
    }
}

pub(super) fn restore(args: &crate::args::AdoptionRestoreArgs) -> Outcome {
    use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
    use std::io::Read;
    let result = (|| {
        if let Some(id) = &args.recover {
            return graphhelm_host_adoption::recover(&args.state_root, id)
                .map(|receipt| serde_json::json!({"receipt":receipt}));
        }
        if let Some(path) = &args.apply {
            let invalid = || AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            };
            let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 4 * 1024 * 1024
            {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|_| invalid())?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid())?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(invalid());
            }
            let plan = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            graphhelm_host_adoption::apply_restore(
                &args.state_root,
                &plan,
                args.accept.as_deref().unwrap_or(""),
            )
            .map(|receipt| serde_json::json!({"receipt":receipt}))
        } else {
            graphhelm_host_adoption::plan_restore(&args.state_root, &args.backup)
                .map(|plan| serde_json::json!({"plan":plan}))
        }
    })();
    match result {
        Ok(data) if data["receipt"]["spec"]["state"] == "recovery_required" => {
            let error = AdoptionError {
                reason: AdoptionReason::RecoveryRequired,
            };
            let mut outcome = Outcome::domain(
                "restore",
                vec![Diagnostic::error(
                    REFUSED,
                    error.to_string(),
                    error.reason.pointer(),
                    SOURCE,
                )],
            );
            outcome.output.data = Some(data);
            outcome
        }
        Ok(data) => Outcome::success("restore", data),
        Err(error) => Outcome::domain(
            "restore",
            vec![Diagnostic::error(
                REFUSED,
                error.to_string(),
                error.reason.pointer(),
                SOURCE,
            )],
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// The rendered face of `--plan` (a terminal, no `--json`) cannot be reached from a piped test
    /// process, so it is held here: the preview built from a plan whose `after` carries a sentinel
    /// renders the path, the digests and the byte length, and never the sentinel.
    #[test]
    fn the_rendered_plan_preview_names_the_operation_and_never_its_contents() {
        let sentinel = "PRIVATE-AFTER-SENTINEL-1208";
        let after = format!("GraphHelm JPD\n{sentinel}\n");
        let digest = format!("sha256:{}", "1".repeat(64));
        let plan = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"unit",
            "digest":digest,
            "spec":{"coverage":"complete","scopes":["project"],"packages":[],"hostBoundary":"quiescent",
            "host":{"name":"codex","version":"0.114.0","program":"/home/owner/private/codex"},
            "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
            "operations":[{"root":"project","path":"AGENTS.md","beforeDigest":"b".repeat(64),
                "afterDigest":"a".repeat(64),"after":after}]}});
        assert!(
            plan.to_string().contains(sentinel),
            "control: the input carries it"
        );
        let output = crate::output::Outcome::success(
            super::COMMAND,
            json!({"plan": super::preview(&plan), "acceptance": {"instruction": "review"}}),
        )
        .output;
        let rendered = crate::human::render(&output, crate::palette::Palette::plain()).unwrap();
        let envelope = serde_json::to_string(&output).unwrap();
        for face in [&rendered, &envelope] {
            assert!(!face.contains(sentinel), "{face}");
            assert!(!face.contains("/home/owner/private"), "{face}");
            assert!(face.contains("AGENTS.md"), "{face}");
            assert!(face.contains(&"a".repeat(64)), "{face}");
            assert!(face.contains(&digest), "{face}");
        }
        assert!(
            rendered.contains(&format!("({} bytes)", after.len())),
            "{rendered}"
        );
    }
}
