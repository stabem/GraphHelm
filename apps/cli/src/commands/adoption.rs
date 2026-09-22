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
    match graphhelm_host_adoption::inventory(&args.project, &args.home) {
        Ok(inventory) => match graphhelm_host_adoption::propose(&inventory) {
            Ok(plan) => Outcome::success(
                COMMAND,
                serde_json::json!({"inventory": inventory, "plan": plan, "provisioning": provisioning}),
            ),
            Err(error) => Outcome::domain(
                COMMAND,
                vec![Diagnostic::error(
                    REFUSED,
                    error.to_string(),
                    error.reason.pointer(),
                    SOURCE,
                )],
            ),
        },
        Err(error) => Outcome::domain(
            COMMAND,
            vec![Diagnostic::error(
                REFUSED,
                error.to_string(),
                error.reason.pointer(),
                SOURCE,
            )],
        ),
    }
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
                serde_json::json!({"plan":plan,"acceptance":{"mode":"explicit_digest","instruction":"Review this plan, then use --apply with its file and --accept with its exact digest. A pipe never confirms automatically."}}),
            )
        }
    })();
    match result {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => refused(error),
    }
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
