//! Approved, guarded publications with a durable compensating journal.
use crate::{
    backup,
    journal::{self, Phase},
    storage::{self, Root},
};
use graphhelm_policy::adoption::approval_matches;
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason, TransactionState};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    BeforeJournal,
    AfterJournal,
    BeforePublish,
    AfterPublish,
    BetweenInitialWrites,
    FinalApplyPublication,
    FinalCompensationPublication,
    AfterApplyCapture,
    AfterCompensationCapture,
    #[cfg(windows)]
    AfterApplyDetach,
    #[cfg(windows)]
    AfterCompensationDetach,
}
struct Operation {
    root: String,
    path: String,
    before: String,
    after_digest: String,
    after: Vec<u8>,
    host_settings: Option<Value>,
}
pub fn apply(
    project: &Path,
    home: &Path,
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
) -> Result<Value, AdoptionError> {
    apply_engine(
        project,
        home,
        state_root,
        plan,
        accepted_digest,
        &mut |_| Ok(()),
        &[],
    )
}
/// Explicit local package paths still have to match the exact pins in the accepted plan.
pub fn apply_with_packages(
    project: &Path,
    home: &Path,
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
    packages: &[std::path::PathBuf],
) -> Result<Value, AdoptionError> {
    apply_engine(
        project,
        home,
        state_root,
        plan,
        accepted_digest,
        &mut |_| Ok(()),
        packages,
    )
}
#[cfg(test)]
fn apply_with_hook(
    project: &Path,
    home: &Path,
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    apply_engine(project, home, state_root, plan, accepted_digest, hook, &[])
}
fn apply_engine(
    project: &Path,
    home: &Path,
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
    package_overrides: &[std::path::PathBuf],
) -> Result<Value, AdoptionError> {
    let operations = preflight(plan, accepted_digest)?;
    let project = Root::open(project, false)?;
    let home = Root::open(home, false)?;
    if plan["spec"]["rootBindings"] != bindings(&project, &home)? {
        return Err(AdoptionError {
            reason: AdoptionReason::PlanStale,
        });
    }
    let state_path = std::path::absolute(state_root).map_err(|_| storage::unsafe_path())?;
    storage::require_disjoint_state(&state_path, &[&project, &home])?;
    if project.record.identity == home.record.identity {
        return Err(storage::unsafe_path());
    }
    let packages = crate::hosts::prepare_packages(plan, package_overrides)?;
    crate::hosts::preflight_host(plan)?;
    // Global/user authority is always acquired before project authority. Both are nonblocking.
    let _user_lock = home.lock(".graphhelm-adoption.lock")?;
    let _project_lock = project.lock(".graphhelm-adoption.lock")?;
    let owner_keys = operations
        .iter()
        .filter(|o| o.root == "home")
        .map(|o| format!("file/{}", o.path))
        .chain(packages.iter().map(|p| format!("package/{}", p.id)))
        .collect::<Vec<_>>();
    let claims = crate::ownership::check(&home, &project, &state_path, &owner_keys)?;
    let store = journal::Store::open(Root::open(&state_path, true)?)?;
    let transaction_id = digest(accepted_digest.as_bytes());
    store.ensure_idle(&transaction_id)?;
    if let Some(previous) = store.read(&transaction_id)? {
        if previous.project != project.record || previous.home != home.record {
            return Err(storage::failed());
        }
        if matches!(
            previous.state,
            TransactionState::InstalledUnverified | TransactionState::Verified
        ) {
            crate::hosts::verify_installed(&store, &previous)?;
            for entry in &previous.entries {
                let root = if entry.root == "project" {
                    &project
                } else {
                    &home
                };
                if digest(&root.source(&entry.path)?.read()?) != entry.after_digest {
                    return Err(storage::failed());
                }
            }
            return previous.receipt.ok_or_else(storage::failed);
        }
        return Err(storage::failed());
    }
    // Check every source before backup/publication, retain its access metadata, then compare each
    // backup blob to this same before-state. A later per-publication re-read closes source drift.
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        let root = if operation.root == "project" {
            &project
        } else {
            &home
        };
        let source = root.source(&operation.path)?;
        let bytes = source.read()?;
        if digest(&bytes) != operation.before {
            return Err(AdoptionError {
                reason: AdoptionReason::PlanStale,
            });
        }
        if let Some(settings) = &operation.host_settings {
            protect_settings(&bytes, &operation.after, settings)?;
        } else {
            protect_instructions(&bytes, &operation.after)?;
        }
        entries.push(journal::Entry {
            operation_index: index,
            root: operation.root.clone(),
            path: operation.path.clone(),
            before_digest: operation.before.clone(),
            after_digest: operation.after_digest.clone(),
            access: storage::access(&source.file)?,
            before_access: None,
            phase: Phase::Planned,
            guards: Vec::new(),
        });
        sources.push(source);
    }
    let checkpoint =
        backup::backup_locked(&project, &home, &store.root.record.path, 1024 * 1024 * 1024)?;
    let backup_id = checkpoint["id"]
        .as_str()
        .ok_or(AdoptionError {
            reason: AdoptionReason::BackupUnverified,
        })?
        .to_owned();
    for entry in &entries {
        let bytes = backup::verified_blob(
            &store.root.record.path,
            &backup_id,
            &format!("{}/{}", entry.root, entry.path),
        )?;
        if digest(&bytes) != entry.before_digest {
            return Err(AdoptionError {
                reason: AdoptionReason::PlanStale,
            });
        }
    }
    store.original_once(&backup_id, &project.record, &home.record)?;
    let mut record = journal::Journal {
        predecessor: store.active()?.map(|previous| previous.transaction_id),
        restore: None,
        version: 1,
        packages: crate::hosts::prepare_entries(&packages),
        transaction_id,
        sequence: 0,
        plan_digest: accepted_digest.into(),
        backup_id,
        project: project.record.clone(),
        home: home.record.clone(),
        entries,
        state: TransactionState::Planned,
        receipt: None,
    };
    let result = (|| {
        for operation in &operations {
            store.save_payload(&operation.after)?;
        }
        sync(&store, &mut record, hook)?;
        claims.publish(&home, &project, &store, &record.transaction_id, &owner_keys)?;
        record.state = TransactionState::BackedUp;
        sync(&store, &mut record, hook)?;
        record.state = TransactionState::Applying;
        sync(&store, &mut record, hook)?;
        for (index, package) in packages.iter().enumerate() {
            record.packages[index].phase = Phase::Intent;
            sync(&store, &mut record, hook)?;
            crate::hosts::create_package_root(
                &store,
                &record.transaction_id,
                &mut record.packages[index],
            )?;
            sync(&store, &mut record, hook)?;
            record.packages[index].activation_digest =
                Some(crate::hosts::install(&record.packages[index], package)?);
            record.packages[index].phase = Phase::Published;
            sync(&store, &mut record, hook)?;
        }
        for (index, (operation, source)) in operations.iter().zip(sources).enumerate() {
            let prepared = source.prepare(
                &operation.before,
                &operation.after,
                &record.entries[index].access,
                &record.entries[index].access,
                false,
            )?;
            record.entries[index].guards.push(prepared.record.clone());
            record.entries[index].phase = Phase::Intent;
            sync(&store, &mut record, hook)?;
            hook(Boundary::BeforePublish)?;
            prepared.publish(&mut |boundary| {
                use storage::PublicationBoundary;
                match boundary {
                    PublicationBoundary::Validated => hook(Boundary::FinalApplyPublication),
                    #[cfg(windows)]
                    PublicationBoundary::Detached => {
                        hook(Boundary::AfterApplyDetach)?;
                        record.entries[index].phase = Phase::Detached;
                        sync(&store, &mut record, hook)
                    }
                    PublicationBoundary::Published => hook(Boundary::AfterApplyCapture),
                }
            })?;
            hook(Boundary::AfterPublish)?;
            record.entries[index].phase = Phase::Published;
            sync(&store, &mut record, hook)?;
        }
        record.state = TransactionState::InstalledUnverified;
        crate::hosts::verify_installed(&store, &record)?;
        let mut result = receipt(&record);
        result["installedAtUnixMs"] = json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| storage::failed())?
                .as_millis()
                .min(9_007_199_254_740_991) as u64
        );
        if !packages.is_empty() {
            result["packages"] = crate::hosts::package_receipts(&store, &record);
            let args = result["packages"]
                .as_array()
                .ok_or_else(storage::failed)?
                .iter()
                .flat_map(|package| [json!("--plugin-dir"), package["path"].clone()])
                .collect::<Vec<_>>();
            result["hostAction"] = json!({"host":"claude","program":plan["spec"]["host"]["program"],"version":plan["spec"]["host"]["version"],"args":args,"instruction":"Start a fresh Claude Code session with this program and argument list. Installation has not proved activation."});
        }
        record.receipt = Some(result.clone());
        sync(&store, &mut record, hook)?;
        Ok(result)
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            // Re-read durable intent: the in-memory state may be ahead of a failed sync.
            if let Some(mut durable) = store.read(&record.transaction_id)?
                && !matches!(
                    durable.state,
                    TransactionState::InstalledUnverified | TransactionState::Verified
                )
                && compensate(&store, &project, &home, &mut durable, hook).is_err()
            {
                return Err(storage::failed());
            }
            Err(error)
        }
    }
}
fn sync(
    store: &journal::Store,
    record: &mut journal::Journal,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<(), AdoptionError> {
    hook(Boundary::BeforeJournal)?;
    store.sync_with_hook(record, &mut || hook(Boundary::BetweenInitialWrites))?;
    hook(Boundary::AfterJournal)
}
pub fn recover(state_root: &Path, transaction_id: &str) -> Result<Value, AdoptionError> {
    recover_engine(state_root, transaction_id, &mut |_| Ok(()))
}
fn recover_engine(
    state_root: &Path,
    transaction_id: &str,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    if !backup::valid_backup_id(transaction_id) {
        return Err(storage::unsafe_path());
    }
    // Read private root identities to locate lock domains, release state lock, then acquire the
    // same user -> project -> state ordering as apply and re-read everything under those locks.
    let initial = {
        let store = journal::Store::reader(Root::observe(state_root)?)?;
        store.read(transaction_id)?.ok_or_else(storage::failed)?
    };
    let home = Root::reopen(&initial.home)?;
    let project = Root::reopen(&initial.project)?;
    let _user_lock = home.lock(".graphhelm-adoption.lock")?;
    let _project_lock = project.lock(".graphhelm-adoption.lock")?;
    let store = journal::Store::open(Root::open(state_root, false)?)?;
    let mut record = store.read(transaction_id)?.ok_or_else(storage::failed)?;
    if record.home != initial.home || record.project != initial.project {
        return Err(storage::failed());
    }
    if record.restore.is_some() {
        return crate::restore::resume(&store, &project, &home, &mut record, hook);
    }
    if matches!(
        record.state,
        TransactionState::InstalledUnverified | TransactionState::Verified
    ) {
        return record.receipt.ok_or_else(storage::failed);
    }
    compensate(&store, &project, &home, &mut record, hook)
}
fn compensate(
    store: &journal::Store,
    project: &Root,
    home: &Root,
    record: &mut journal::Journal,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    let mut originals = Vec::new();
    for entry in &record.entries {
        let root = if entry.root == "project" {
            project
        } else {
            home
        };
        if let Some(guard) = entry.guards.last() {
            guard.reconcile(
                root,
                &entry.path,
                entry.before_access.as_ref().unwrap_or(&entry.access),
            )?;
        }
        for guard in &entry.guards {
            guard.verify(
                root,
                &entry.path,
                entry.before_access.as_ref().unwrap_or(&entry.access),
                &entry.access,
            )?;
        }
        let source = root.source(&entry.path)?;
        let current = digest(&source.read()?);
        if current != entry.before_digest
            && (current != entry.after_digest || entry.phase == Phase::Planned)
        {
            record.state = TransactionState::RecoveryRequired;
            sync(store, record, hook)?;
            return Err(storage::failed());
        }
        let bytes = backup::verified_blob(
            &store.root.record.path,
            &record.backup_id,
            &format!("{}/{}", entry.root, entry.path),
        )?;
        if digest(&bytes) != entry.before_digest || storage::access(&source.file)? != entry.access {
            return Err(storage::failed());
        }
        originals.push(bytes);
    }
    record.state = TransactionState::Restoring;
    sync(store, record, hook)?;
    if !record.packages.is_empty() {
        crate::hosts::compensate_packages(store, record)?;
        for package in &mut record.packages {
            package.phase = Phase::Reverted;
        }
        sync(store, record, hook)?;
    }
    for index in (0..record.entries.len()).rev() {
        let entry = &record.entries[index];
        let root = if entry.root == "project" {
            project
        } else {
            home
        };
        let source = root.source(&entry.path)?;
        let current = digest(&source.read()?);
        if current == entry.after_digest && current != entry.before_digest {
            if entry.guards.len() >= 16 {
                return Err(storage::failed());
            }
            let prepared = source.prepare(
                &entry.after_digest,
                &originals[index],
                &entry.access,
                &entry.access,
                true,
            )?;
            record.entries[index].guards.push(prepared.record.clone());
            record.entries[index].phase = Phase::RestoreIntent;
            sync(store, record, hook)?;
            prepared.publish(&mut |boundary| {
                use storage::PublicationBoundary;
                match boundary {
                    PublicationBoundary::Validated => hook(Boundary::FinalCompensationPublication),
                    #[cfg(windows)]
                    PublicationBoundary::Detached => {
                        hook(Boundary::AfterCompensationDetach)?;
                        record.entries[index].phase = Phase::RestoreDetached;
                        sync(store, record, hook)
                    }
                    PublicationBoundary::Published => hook(Boundary::AfterCompensationCapture),
                }
            })?;
        } else if current != entry.before_digest {
            record.state = TransactionState::RecoveryRequired;
            sync(store, record, hook)?;
            return Err(storage::failed());
        }
        record.entries[index].phase = Phase::Reverted;
        sync(store, record, hook)?;
    }
    record.state = TransactionState::Restored;
    let result = receipt(record);
    record.receipt = Some(result.clone());
    sync(store, record, hook)?;
    Ok(result)
}
pub(crate) fn receipt(record: &journal::Journal) -> Value {
    json!({"apiVersion":"p50.dev/adoption/v1","kind":"ApplyReceipt","id":record.transaction_id,"spec":{"transactionId":record.transaction_id,"planDigest":record.plan_digest,"backupId":record.backup_id,"state":record.state}})
}
fn preflight(plan: &Value, accepted: &str) -> Result<Vec<Operation>, AdoptionError> {
    let actual = plan["digest"].as_str().unwrap_or("");
    if !approval_matches(actual, accepted) {
        return Err(AdoptionError {
            reason: AdoptionReason::ReviewRequired,
        });
    }
    // Bound recursive clone/canonicalization before doing either on caller-owned JSON.
    let canonical = graphhelm_graph::canonical_content_bytes(plan).map_err(|_| AdoptionError {
        reason: AdoptionReason::LimitExceeded,
    })?;
    if canonical.len() > 4 * 1024 * 1024
        || plan["spec"]["operations"]
            .as_array()
            .is_some_and(|items| items.len() > 256)
    {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    let mut payload = plan.clone();
    payload
        .as_object_mut()
        .ok_or(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })?
        .remove("digest");
    if actual
        != format!(
            "sha256:{}",
            digest(
                &graphhelm_graph::canonical_content_bytes(&payload).map_err(|_| AdoptionError {
                    reason: AdoptionReason::LimitExceeded
                })?
            )
        )
    {
        return Err(AdoptionError {
            reason: AdoptionReason::ReviewRequired,
        });
    }
    if !graphhelm_schema::validate_adoption_plan(plan).is_empty() {
        return Err(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        });
    }
    if plan["spec"]["hostBoundary"] != "quiescent" {
        return Err(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        });
    }
    let decisions = plan["spec"]["decisions"]
        .as_array()
        .ok_or_else(storage::failed)?;
    let values = plan["spec"]["operations"]
        .as_array()
        .ok_or_else(storage::failed)?;
    if decisions.len() != values.len() {
        return Err(AdoptionError {
            reason: AdoptionReason::ReviewRequired,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let root = value["root"].as_str().ok_or_else(storage::failed)?;
        let path = value["path"].as_str().ok_or_else(storage::failed)?;
        storage::relative_ok(path)?;
        let host_settings = matches!(
            (root, path),
            ("project", ".claude/settings.local.json")
                | ("home", ".claude/settings.json" | ".codex/config.toml")
        );
        if !matches!(
            (root, path),
            ("project", "AGENTS.md" | "CLAUDE.md") | ("home", "AGENTS.md")
        ) && !host_settings
        {
            return Err(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            });
        }
        if !seen.insert((root, path.to_ascii_lowercase())) {
            return Err(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            });
        }
        let required_scope = if root == "home" { "user" } else { "project" };
        if !plan["spec"]["scopes"]
            .as_array()
            .is_some_and(|s| s.iter().any(|v| v == required_scope))
        {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
        let decision = &decisions[index];
        if decision["operationIndex"].as_u64() != Some(index as u64)
            || decision["decision"] != "replace"
            || decision["protected"] != false
        {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
        let before = value["beforeDigest"]
            .as_str()
            .ok_or_else(storage::failed)?
            .to_owned();
        let after = value["after"]
            .as_str()
            .ok_or_else(storage::failed)?
            .as_bytes()
            .to_vec();
        let after_digest = value["afterDigest"]
            .as_str()
            .ok_or_else(storage::failed)?
            .to_owned();
        if after.len() > 1024 * 1024 {
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        if digest(&after) != after_digest {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
        result.push(Operation {
            root: root.into(),
            path: path.into(),
            before,
            after_digest,
            after,
            host_settings: host_settings.then(|| value.clone()),
        });
    }
    Ok(result)
}
fn protect_settings(before: &[u8], after: &[u8], operation: &Value) -> Result<(), AdoptionError> {
    if operation["path"] == ".codex/config.toml" {
        let before = std::str::from_utf8(before).map_err(|_| storage::failed())?;
        let after: toml::Value =
            toml::from_str(std::str::from_utf8(after).map_err(|_| storage::failed())?)
                .map_err(|_| storage::failed())?;
        let accepted: Vec<String> = serde_json::from_value(operation["disableSkills"].clone())
            .map_err(|_| storage::failed())?;
        let expected = crate::hosts::codex::disable_skills(before, &accepted, true)?;
        let expected: toml::Value = toml::from_str(&expected).map_err(|_| storage::failed())?;
        if after != expected {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
        return Ok(());
    }
    let before: Value = serde_json::from_slice(before).map_err(|_| storage::failed())?;
    let after: Value = serde_json::from_slice(after).map_err(|_| storage::failed())?;
    let accepted: Vec<String> = serde_json::from_value(operation["disablePlugins"].clone())
        .map_err(|_| storage::failed())?;
    if !accepted.is_empty() {
        return Err(AdoptionError {
            reason: AdoptionReason::HostPolicyRequired,
        });
    }
    let expected = crate::hosts::claude::disable_plugins(&before, &accepted, &json!({}))?;
    if after != expected {
        return Err(AdoptionError {
            reason: AdoptionReason::ReviewRequired,
        });
    }
    Ok(())
}
fn protect_instructions(before: &[u8], after: &[u8]) -> Result<(), AdoptionError> {
    let before = std::str::from_utf8(before).map_err(|_| AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    })?;
    let after = std::str::from_utf8(after).map_err(|_| AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    })?;
    for line in before.lines() {
        let lower = line.to_ascii_lowercase();
        if [
            "deny",
            "permission",
            "secret",
            "credential",
            "security",
            "sandbox",
            "approval",
            ".env",
        ]
        .iter()
        .any(|word| lower.contains(word))
            && !after.lines().any(|candidate| candidate == line)
        {
            return Err(AdoptionError {
                reason: AdoptionReason::ReviewRequired,
            });
        }
    }
    Ok(())
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
/// Opaque preview bindings to physical roots. No paths or source bytes are returned.
pub fn root_bindings(project: &Path, home: &Path) -> Result<Value, AdoptionError> {
    bindings(&Root::observe(project)?, &Root::observe(home)?)
}
fn bindings(project: &Root, home: &Root) -> Result<Value, AdoptionError> {
    let binding = |root: &Root| -> Result<String, AdoptionError> {
        let record = serde_json::to_value(&root.record).map_err(|_| storage::failed())?;
        Ok(digest(
            &graphhelm_graph::canonical_content_bytes(&record).map_err(|_| storage::failed())?,
        ))
    };
    Ok(json!({"project": binding(project)?, "home": binding(home)?}))
}

#[cfg(test)]
#[path = "../tests/support/recovery_harness.rs"]
mod tests;
