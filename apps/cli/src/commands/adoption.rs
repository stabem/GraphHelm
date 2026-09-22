use graphhelm_protocols::Diagnostic;

use crate::args::{AdoptionBackupArgs, AdoptionSetupArgs};
use crate::output::Outcome;

const COMMAND: &str = "setup";
const SOURCE: &str = "adoption-cli";
const REFUSED: &str = crate::error_codes::GHCLI029_ADOPTION_REFUSED;

pub(super) fn run(args: &AdoptionSetupArgs) -> Outcome {
    // `setup` is intentionally preview-only until a reviewed plan can be applied. The
    // compatibility `--dry-run` flag remains accepted, but preview is the safe default.
    let _ = args.dry_run;
    match graphhelm_host_adoption::inventory(&args.project, &args.home) {
        Ok(inventory) => match graphhelm_host_adoption::propose(&inventory) {
            Ok(plan) => Outcome::success(
                COMMAND,
                serde_json::json!({"inventory": inventory, "plan": plan}),
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
