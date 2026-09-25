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
/// grows later is not printed until it is named here. `host.program` is named: it is the program
/// `--apply` runs, a path rather than a secret, and a digest accepted without it is accepted blind.
/// For the same reason a project `.mcp.json` operation carries `registration` (see
/// [`registration`]): the binary the host will start, read out of an otherwise redacted `after`.
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
                    if row["root"] == "project" && row["path"] == ".mcp.json" {
                        view["registration"] = registration(&row["after"]);
                    }
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
            "host": pick(&spec["host"], &["name", "version", "program", "mode"]),
            "packages": each("/spec/packages", &["id", "version", "digest"]),
            "decisions": each("/spec/decisions", &["operationIndex", "item", "decision", "protected"]),
            "review": each("/spec/review", &["item", "decision"]),
            "operations": operations,
        }
    })
}

/// #1208: the one part of a project `.mcp.json` `after` the preview may show — the `command` and
/// `args` of `mcpServers.graphhelm`, the program the host will start when it opens the project.
/// Nothing else in that file is read out (another server's `env` can hold a credential). The entry
/// is shown only when the shape check `--apply` enforces accepts it
/// ([`graphhelm_host_adoption::is_graphhelm_registration`]); an entry that check refuses shows as
/// `refused` and none of its values (an unknown flag can carry a credential). When the entry
/// cannot be read at all the preview says `unreadable` instead of saying nothing, so an approver
/// never mistakes a missing line for a harmless one.
fn registration(after: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Value, json};
    let read = || -> Option<Value> {
        let text = after.as_str()?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let file: Value = serde_json::from_str(text).ok()?;
        let entry = file.get("mcpServers")?.get("graphhelm")?;
        let command = entry.get("command")?.as_str()?;
        let args = entry
            .get("args")?
            .as_array()?
            .iter()
            .map(|arg| arg.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        if !graphhelm_host_adoption::is_graphhelm_registration(entry) {
            return Some(json!("refused"));
        }
        Some(json!({"command": command, "args": args}))
    };
    read().unwrap_or_else(|| json!("unreadable"))
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
    /// renders the path, the digests, the byte length and the program `--apply` runs, and never the
    /// sentinel.
    #[test]
    fn the_rendered_plan_preview_names_the_operation_and_never_its_contents() {
        let sentinel = "PRIVATE-AFTER-SENTINEL-1208";
        let after = format!("GraphHelm JPD\n{sentinel}\n");
        let digest = format!("sha256:{}", "1".repeat(64));
        let program = "/home/owner/tools/codex";
        let plan = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"unit",
            "digest":digest,
            "spec":{"coverage":"complete","scopes":["project"],"packages":[],"hostBoundary":"quiescent",
            "host":{"name":"codex","version":"0.114.0","program":program},
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
            assert!(face.contains(program), "{face}");
            assert!(face.contains("AGENTS.md"), "{face}");
            assert!(face.contains(&"a".repeat(64)), "{face}");
            assert!(face.contains(&digest), "{face}");
        }
        assert!(
            rendered.contains(&format!("({} bytes)", after.len())),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("Runs: \"{program}\"\n")),
            "{rendered}"
        );
    }

    /// Both faces of a preview over a plan with one project `.mcp.json` operation.
    fn mcp_faces(after: &str) -> (String, String) {
        let plan = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"unit",
            "digest":format!("sha256:{}", "1".repeat(64)),
            "spec":{"coverage":"complete","scopes":["project"],"packages":[],"hostBoundary":"quiescent",
            "host":{"name":"claude","version":"2.1.265"},
            "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
            "operations":[{"root":"project","path":".mcp.json","beforeDigest":"b".repeat(64),
                "afterDigest":"a".repeat(64),"after":after}]}});
        let output = crate::output::Outcome::success(
            super::COMMAND,
            json!({"plan": super::preview(&plan), "acceptance": {"instruction": "review"}}),
        )
        .output;
        (
            crate::human::render(&output, crate::palette::Palette::plain()).unwrap(),
            serde_json::to_string(&output).unwrap(),
        )
    }

    /// #1208 after #1281: `--apply` may register `mcpServers.graphhelm` in the project `.mcp.json`,
    /// which the host starts when it opens the project. The preview shows the command and args the
    /// plan sets, in both faces, and nothing else from that file: a sentinel in another server's
    /// `env` stays out.
    #[test]
    fn the_plan_preview_shows_the_mcp_registration_and_nothing_else_of_the_file() {
        let sentinel = "PRIVATE-MCP-SENTINEL-1208";
        let command = INIT_COMMAND;
        let args = [
            "mcp",
            "--url",
            "http://127.0.0.1:7433",
            "--token-file",
            "/home/owner/.graphhelm/token",
            "--actor",
            "owner",
        ];
        let after = serde_json::to_string_pretty(&json!({"mcpServers":{
            "graphhelm":{"command":command,"args":args},
            "other":{"command":"other-server","env":{"API_KEY":sentinel}}}}))
        .unwrap();
        assert!(after.contains(sentinel), "control: the input carries it");
        let (rendered, envelope) = mcp_faces(&after);
        for face in [&rendered, &envelope] {
            assert!(!face.contains(sentinel), "{face}");
            assert!(!face.contains("other-server"), "{face}");
            assert!(face.contains(command), "{face}");
            for arg in args {
                assert!(face.contains(arg), "{arg}: {face}");
            }
        }
        let quoted: Vec<String> = args.iter().map(|arg| format!("\"{arg}\"")).collect();
        assert!(
            rendered.contains(&format!(
                "Registers MCP: \"{command}\" {}\n",
                quoted.join(" ")
            )),
            "{rendered}"
        );
        let envelope: serde_json::Value = serde_json::from_str(&envelope).unwrap();
        assert_eq!(
            envelope["data"]["plan"]["spec"]["operations"][0]["registration"],
            json!({"command": command, "args": args})
        );
    }

    /// An absolute path to a `graphhelm` binary on the platform the test runs on: the shape
    /// `graphhelm init` writes, which the `--apply` check accepts.
    const INIT_COMMAND: &str = if cfg!(windows) {
        "C:/Users/owner/bin/graphhelm.exe"
    } else {
        "/home/owner/bin/graphhelm"
    };

    /// #1208 follow-up to #1288: the preview shows a registration only when the shape check
    /// `--apply` enforces accepts it. An entry that check refuses (here an unknown flag carrying a
    /// credential, or a relative command) shows `refused` in both faces, and none of its values.
    #[test]
    fn a_registration_the_apply_check_refuses_shows_refused_and_none_of_its_values() {
        let sentinel = "PRIVATE-MCP-SENTINEL-1208";
        for entry in [
            json!({"command": INIT_COMMAND, "args": ["mcp", "--api-key", sentinel]}),
            json!({"command": INIT_COMMAND, "args": ["mcp", "--actor", "owner", sentinel]}),
            json!({"command": format!("bin/graphhelm-{sentinel}"), "args": ["mcp"]}),
            json!({"command": INIT_COMMAND, "args": ["mcp"], "env": {"TOKEN": sentinel}}),
            // `--url` must be a loopback Runtime URL: no userinfo, no other host, no other scheme.
            json!({"command": INIT_COMMAND,
                "args": ["mcp", "--url", format!("http://user:{sentinel}@127.0.0.1:7433")]}),
            json!({"command": INIT_COMMAND,
                "args": ["mcp", "--url", format!("http://{sentinel}.evil.example:7433")]}),
            json!({"command": INIT_COMMAND, "args": ["mcp", "--url", format!("file:///{sentinel}")]}),
        ] {
            assert!(
                !graphhelm_host_adoption::is_graphhelm_registration(&entry),
                "control: the apply check refuses {entry}"
            );
            let after =
                serde_json::to_string(&json!({"mcpServers": {"graphhelm": entry}})).unwrap();
            let (rendered, envelope) = mcp_faces(&after);
            assert!(rendered.contains("Registers MCP: refused\n"), "{rendered}");
            for face in [&rendered, &envelope] {
                assert!(!face.contains(sentinel), "{face}");
                assert!(
                    !face.contains("graphhelm.exe") && !face.contains("/bin/"),
                    "{face}"
                );
            }
            let envelope: serde_json::Value = serde_json::from_str(&envelope).unwrap();
            assert_eq!(
                envelope["data"]["plan"]["spec"]["operations"][0]["registration"], "refused",
                "{after}"
            );
        }
    }

    /// An `.mcp.json` `after` the preview cannot read as a registration says so, in both faces,
    /// rather than printing no line an approver could read as "nothing registered".
    #[test]
    fn an_unreadable_mcp_registration_is_named_not_omitted() {
        for after in [
            "not json PRIVATE-MCP-SENTINEL-1208",
            r#"{"mcpServers":{"other":{"command":"x"}}}"#,
            r#"{"mcpServers":{"graphhelm":{"command":["sh"],"args":[]}}}"#,
        ] {
            let (rendered, envelope) = mcp_faces(after);
            assert!(
                rendered.contains("Registers MCP: unreadable\n"),
                "{rendered}"
            );
            let envelope: serde_json::Value = serde_json::from_str(&envelope).unwrap();
            assert_eq!(
                envelope["data"]["plan"]["spec"]["operations"][0]["registration"], "unreadable",
                "{after}"
            );
            assert!(
                !rendered.contains("PRIVATE-MCP-SENTINEL-1208"),
                "{rendered}"
            );
        }
    }
}
