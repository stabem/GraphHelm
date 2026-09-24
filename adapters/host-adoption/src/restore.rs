//! Offline, exact-review restoration. Plans contain digests, never configuration bytes.
use crate::{
    apply::{self, Boundary},
    backup,
    journal::{self, Phase},
    storage::{self, Root},
};
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason, TransactionState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RestoreIntent {
    pub selected_backup: String,
    pub sources: Vec<String>,
}
struct PreparedPlan {
    value: Value,
    entries: Vec<journal::Entry>,
    targets: Vec<Vec<u8>>,
    sources: Vec<journal::Journal>,
}
fn error(reason: AdoptionReason) -> AdoptionError {
    AdoptionError { reason }
}
fn value_digest(value: &Value) -> Result<String, AdoptionError> {
    Ok(apply::digest(
        &graphhelm_graph::canonical_content_bytes(value)
            .map_err(|_| error(AdoptionReason::LimitExceeded))?,
    ))
}
fn journal_binding(store: &journal::Store) -> Result<String, AdoptionError> {
    let active = store.active()?.map(|record| {
        Ok(json!({
            "transactionId": record.transaction_id,
            "recordDigest": value_digest(&serde_json::to_value(record).map_err(|_| storage::failed())?)?
        }))
    }).transpose()?;
    value_digest(&json!({"active": active}))
}
fn seal(mut value: Value) -> Result<Value, AdoptionError> {
    value
        .as_object_mut()
        .ok_or_else(storage::failed)?
        .remove("digest");
    value["digest"] = json!(format!("sha256:{}", value_digest(&value)?));
    Ok(value)
}
fn chain(store: &journal::Store) -> Result<Vec<journal::Journal>, AdoptionError> {
    chain_with_empty(store, false)
}
fn chain_with_empty(
    store: &journal::Store,
    allow_empty: bool,
) -> Result<Vec<journal::Journal>, AdoptionError> {
    let mut next = store.active()?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(record) = next {
        if seen.len() == 256 || !seen.insert(record.transaction_id.clone()) {
            return Err(storage::failed());
        }
        next = record
            .predecessor
            .as_deref()
            .map(|id| store.read(id).and_then(|r| r.ok_or_else(storage::failed)))
            .transpose()?;
        if record.state == TransactionState::Restored {
            if record
                .receipt
                .as_ref()
                .is_none_or(|receipt| receipt["spec"]["state"] != "restored")
            {
                return Err(storage::failed());
            }
            continue;
        }
        if record.restore.is_some() {
            return Err(storage::failed());
        }
        if !matches!(
            record.state,
            TransactionState::InstalledUnverified | TransactionState::Verified
        ) {
            return Err(storage::failed());
        }
        result.push(record);
    }
    if result.is_empty() && !allow_empty {
        return Err(storage::failed());
    }
    Ok(result)
}

fn sources_for_checkpoint(store: &journal::Store) -> Result<Vec<journal::Journal>, AdoptionError> {
    chain_with_empty(store, true)
}

/// Reads only: no locks, new snapshots, provider, host process, or Runtime calls.
pub fn plan_restore(state_root: &Path, backup_id: &str) -> Result<Value, AdoptionError> {
    storage::diagnostic_reset();
    storage::diagnostic_stage("restore.plan.open_store");
    let store = journal::Store::reader(Root::observe(state_root)?)?;
    if backup_id != "original" {
        let linked = match store.active()? {
            Some(_) => Some(sources_for_checkpoint(&store)?),
            None => None,
        };
        if !linked
            .as_ref()
            .is_some_and(|records| records.iter().any(|record| record.backup_id == backup_id))
        {
            let Some(provenance) =
                backup::checkpoint_provenance(&store.root.record.path, backup_id)?
            else {
                return Err(error(AdoptionReason::BackupUnverified));
            };
            let project = manual_root(&provenance, "project", &store)?;
            let home = manual_root(&provenance, "home", &store)?;
            let _state = manual_root(&provenance, "state", &store)?;
            let sources = linked.unwrap_or_else(Vec::new);
            return Ok(prepare(&store, &project, &home, backup_id, sources, true)?.value);
        }
    }
    let sources = chain(&store)?;
    let project = Root::reopen(&sources[0].project)?;
    let home = Root::reopen(&sources[0].home)?;
    Ok(prepare(&store, &project, &home, backup_id, sources, false)?.value)
}

fn manual_root(
    provenance: &Value,
    name: &str,
    store: &journal::Store,
) -> Result<Root, AdoptionError> {
    let record: crate::storage::RootRecord =
        serde_json::from_value(provenance.get(name).cloned().ok_or_else(storage::failed)?)
            .map_err(|_| error(AdoptionReason::BackupCorrupt))?;
    let root = Root::reopen(&record)?;
    let binding =
        value_digest(&serde_json::to_value(&root.record).map_err(|_| storage::failed())?)?;
    if provenance["bindings"][name] != binding {
        return Err(error(AdoptionReason::BackupCorrupt));
    }
    if name == "state" && root.record != store.root.record {
        return Err(error(AdoptionReason::PlanStale));
    }
    Ok(root)
}
fn prepare(
    store: &journal::Store,
    project: &Root,
    home: &Root,
    selection: &str,
    mut sources: Vec<journal::Journal>,
    manual: bool,
) -> Result<PreparedPlan, AdoptionError> {
    let backup_id = if selection == "original" {
        let original: Value = serde_json::from_slice(
            &storage::read_child(&store.root.file, "original.json", 64 * 1024)?
                .ok_or_else(storage::failed)?,
        )
        .map_err(|_| storage::failed())?;
        if original["project"]
            != serde_json::to_value(&project.record).map_err(|_| storage::failed())?
            || original["home"]
                != serde_json::to_value(&home.record).map_err(|_| storage::failed())?
        {
            return Err(storage::failed());
        }
        original["backupId"]
            .as_str()
            .ok_or_else(storage::failed)?
            .to_owned()
    } else {
        selection.to_owned()
    };
    backup::verify_backup(&store.root.record.path, &backup_id)?;
    if selection != "original" && !manual {
        let end = sources
            .iter()
            .position(|r| r.backup_id == backup_id)
            .ok_or(error(AdoptionReason::BackupUnverified))?;
        sources.truncate(end + 1);
    }
    for source in &sources {
        if source.project != project.record || source.home != home.record {
            return Err(storage::failed());
        }
    }
    let mut current = Vec::new();
    let mut effects = Vec::new();
    let mut conflicts = Vec::new();
    let mut entries = Vec::new();
    let mut targets = Vec::new();
    for surface in crate::surfaces::ALL {
        let scope = surface.scope;
        let path = surface.path;
        let root = if scope == "project" { project } else { home };
        let bytes = backup::read_supported(root, path)?;
        let access = bytes
            .as_ref()
            .map(|_| root.source(path).and_then(|s| storage::access(&s.file)))
            .transpose()?;
        let current_digest = bytes.as_ref().map(|b| apply::digest(b));
        current.push(json!({"root":scope,"path":path,"digest":current_digest,"accessDigest":access.as_ref().map(|a| serde_json::to_value(a).map_err(|_| storage::failed()).and_then(|v| value_digest(&v))).transpose()?}));
        let base = backup::verified_optional_blob(
            &store.root.record.path,
            &backup_id,
            &format!("{scope}/{path}"),
        )?;
        if manual {
            let Some(target) = base.clone() else {
                effects.push(json!({"root":scope,"path":path,"action":"retain_unowned","beforeDigest":current_digest,"afterDigest":current_digest}));
                continue;
            };
            let target_access = backup::verified_surface_access(
                &store.root.record.path,
                &backup_id,
                &format!("{scope}/{path}"),
            )?
            .ok_or_else(|| error(AdoptionReason::BackupUnverified))?;
            let after = apply::digest(&target);
            let unchanged = current_digest.as_deref() == Some(&after)
                && access.as_ref() == Some(&target_access);
            effects.push(json!({"root":scope,"path":path,"action":if unchanged {"unchanged"} else {"restore"},"beforeDigest":current_digest,"afterDigest":after}));
            if unchanged {
                continue;
            }
            entries.push(journal::Entry {
                operation_index: entries.len(),
                root: scope.into(),
                path: path.into(),
                before_digest: current_digest.ok_or_else(storage::failed)?,
                after_digest: after,
                access: target_access,
                before_access: Some(access.ok_or_else(storage::failed)?),
                phase: Phase::Planned,
                guards: Vec::new(),
            });
            targets.push(target);
            continue;
        }
        let owned = sources
            .iter()
            .flat_map(|source| {
                source
                    .entries
                    .iter()
                    .filter(move |entry| entry.root == scope && entry.path == path)
                    .map(move |entry| (source, entry))
            })
            .collect::<Vec<_>>();
        let Some((_, installed)) = owned.first() else {
            effects.push(json!({"root":scope,"path":path,"action":if bytes != base {"retain_unowned"} else {"unchanged"},"beforeDigest":current_digest,"afterDigest":current_digest}));
            continue;
        };
        let target = if let Some(current) = bytes.as_deref() {
            let transitions = owned
                .iter()
                .map(|(source, entry)| {
                    Ok((
                        backup::verified_optional_blob(
                            &store.root.record.path,
                            &source.backup_id,
                            &format!("{scope}/{path}"),
                        )?
                        .ok_or_else(storage::failed)?,
                        store.payload(&entry.after_digest)?,
                    ))
                })
                .collect::<Result<Vec<_>, AdoptionError>>()?;
            let transitions = transitions
                .iter()
                .map(|(base, installed)| (base.as_slice(), installed.as_slice()))
                .collect::<Vec<_>>();
            restore_transitions(path, current, &transitions).ok()
        } else {
            None
        };
        let target = if owned
            .iter()
            .all(|(_, entry)| access.as_ref() == Some(&entry.access))
        {
            target
        } else {
            None
        };
        match target {
            None => {
                effects.push(json!({"root":scope,"path":path,"action":"conflict","beforeDigest":current_digest,"afterDigest":current_digest}));
                conflicts.push(json!({"root":scope,"path":path,"reason":"restore_conflict"}));
            }
            Some(target) => {
                let after = apply::digest(&target);
                effects.push(json!({"root":scope,"path":path,"action":if current_digest.as_deref() == Some(&after) {"unchanged"} else {"restore"},"beforeDigest":current_digest,"afterDigest":after}));
                entries.push(journal::Entry {
                    operation_index: entries.len(),
                    root: scope.into(),
                    path: path.into(),
                    before_digest: current_digest.ok_or_else(storage::failed)?,
                    after_digest: after,
                    access: installed.access.clone(),
                    before_access: None,
                    phase: Phase::Planned,
                    guards: Vec::new(),
                });
                targets.push(target);
            }
        }
    }
    let source_bindings = sources.iter().map(|r| Ok(json!({"transactionId":r.transaction_id,"digest":value_digest(&serde_json::to_value(r).map_err(|_| storage::failed())?)?}))).collect::<Result<Vec<_>, AdoptionError>>()?;
    let mut packages = Vec::new();
    for record in sources.iter().filter(|_| !manual) {
        let valid = crate::hosts::verify_installed(store, record).is_ok();
        for package in &record.packages {
            let root = Root::reopen(package.root.as_ref().ok_or_else(storage::failed)?)?;
            let current =
                storage::read_child(&root.file, "active.json", 4096)?.map(|b| apply::digest(&b));
            packages.push(json!({"transactionId":record.transaction_id,"id":package.id,"digest":package.digest,"currentDigest":current,"action":if valid {"deactivate_owned"} else {"retain_modified"}}));
            if !valid {
                conflicts.push(json!({"root":"home","path":format!("package/{}",package.id),"reason":"restore_conflict"}));
            }
        }
    }
    let mut spec = json!({"selection":if selection == "original" {"original"} else {"checkpoint"},"manual":manual,"backupId":backup_id,"rootBindings":crate::root_bindings(&project.record.path,&home.record.path)?,"stateBinding":value_digest(&serde_json::to_value(&store.root.record).map_err(|_| storage::failed())?)?,"sources":source_bindings,"current":current,"effects":effects,"packages":packages,"conflicts":conflicts});
    if manual {
        spec["journalBinding"] = json!(journal_binding(store)?);
    }
    let value = seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"RestorePlan","id":"restore","spec":spec}),
    )?;
    Ok(PreparedPlan {
        value,
        entries,
        targets,
        sources,
    })
}

fn parse_document(path: &str, bytes: &[u8]) -> Result<Value, AdoptionError> {
    if path.ends_with(".json") {
        return serde_json::from_slice(bytes).map_err(|_| storage::failed());
    }
    if path.ends_with(".toml") {
        let parsed: toml::Value =
            toml::from_str(std::str::from_utf8(bytes).map_err(|_| storage::failed())?)
                .map_err(|_| storage::failed())?;
        return serde_json::to_value(parsed).map_err(|_| storage::failed());
    }
    Err(storage::failed())
}
fn restore_document(
    path: &str,
    base: &[u8],
    installed: &[u8],
    current: &[u8],
) -> Result<Vec<u8>, AdoptionError> {
    if current == installed || current == base {
        return Ok(base.to_vec());
    }
    if base == installed {
        return Ok(current.to_vec());
    }
    // toml::Value does not retain comments. Fail closed instead of erasing a later annotation.
    if path.ends_with(".toml") && current.contains(&b'#') {
        return Err(storage::failed());
    }
    let (base, installed, current) = (
        parse_document(path, base)?,
        parse_document(path, installed)?,
        parse_document(path, current)?,
    );
    let restored =
        restore_keys(Some(&base), Some(&installed), Some(&current))?.ok_or_else(storage::failed)?;
    if path.ends_with(".json") {
        serde_json::to_vec_pretty(&restored).map_err(|_| storage::failed())
    } else {
        let value = toml::Value::try_from(restored).map_err(|_| storage::failed())?;
        toml::to_string(&value)
            .map(String::into_bytes)
            .map_err(|_| storage::failed())
    }
}
fn restore_transitions(
    path: &str,
    current: &[u8],
    transitions: &[(&[u8], &[u8])],
) -> Result<Vec<u8>, AdoptionError> {
    let mut restored = current.to_vec();
    for (base, installed) in transitions {
        restored = restore_document(path, base, installed, &restored)?;
    }
    Ok(restored)
}
fn restore_keys(
    base: Option<&Value>,
    installed: Option<&Value>,
    current: Option<&Value>,
) -> Result<Option<Value>, AdoptionError> {
    if base == installed {
        return Ok(current.cloned());
    }
    if current == installed || current == base {
        return Ok(base.cloned());
    }
    if base.is_none_or(Value::is_object)
        && installed.is_none_or(Value::is_object)
        && current.is_some_and(Value::is_object)
    {
        let mut result = current
            .and_then(Value::as_object)
            .ok_or_else(storage::failed)?
            .clone();
        let keys = base
            .into_iter()
            .chain(installed)
            .filter_map(Value::as_object)
            .flat_map(|o| o.keys())
            .collect::<BTreeSet<_>>();
        for key in keys {
            let value = restore_keys(
                base.and_then(|v| v.get(key)),
                installed.and_then(|v| v.get(key)),
                current.and_then(|v| v.get(key)),
            )?;
            if let Some(value) = value {
                result.insert(key.clone(), value);
            } else {
                result.remove(key);
            }
        }
        return Ok(Some(Value::Object(result)));
    }
    graphhelm_policy::adoption::restore_value(base, installed, current)
        .map_err(|_| storage::failed())
}

/// Applies one exact reviewed plan; source drift requires a fresh preview and approval.
pub fn apply_restore(
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
) -> Result<Value, AdoptionError> {
    apply_engine(state_root, plan, accepted_digest, &mut |_| Ok(()))
}
fn apply_engine(
    state_root: &Path,
    plan: &Value,
    accepted_digest: &str,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    storage::diagnostic_reset();
    storage::diagnostic_stage("restore.apply.validate_plan");
    let bytes = graphhelm_graph::canonical_content_bytes(plan)
        .map_err(|_| error(AdoptionReason::LimitExceeded))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(error(AdoptionReason::LimitExceeded));
    }
    if accepted_digest.is_empty()
        || plan["digest"] != accepted_digest
        || seal(plan.clone())? != *plan
    {
        return Err(error(AdoptionReason::ReviewRequired));
    }
    if !graphhelm_schema::validate_restore_plan(plan).is_empty() {
        return Err(error(AdoptionReason::InvalidConfiguration));
    }
    let transaction_id = apply::digest(accepted_digest.as_bytes());
    storage::diagnostic_stage("restore.apply.open_store");
    let initial = journal::Store::reader(Root::observe(state_root)?)?;
    let manual = plan["spec"]["manual"] == true
        && backup::checkpoint_provenance(
            &initial.root.record.path,
            plan["spec"]["backupId"]
                .as_str()
                .ok_or_else(storage::failed)?,
        )?
        .is_some();
    storage::diagnostic_stage("restore.apply.reopen_roots");
    let (project, home) = if manual {
        let provenance = backup::checkpoint_provenance(
            &initial.root.record.path,
            plan["spec"]["backupId"]
                .as_str()
                .ok_or_else(storage::failed)?,
        )?
        .ok_or_else(|| error(AdoptionReason::BackupUnverified))?;
        (
            manual_root(&provenance, "project", &initial)?,
            manual_root(&provenance, "home", &initial)?,
        )
    } else {
        let source_id = plan["spec"]["sources"][0]["transactionId"]
            .as_str()
            .ok_or_else(storage::failed)?;
        let first = initial.read(source_id)?.ok_or_else(storage::failed)?;
        (Root::reopen(&first.project)?, Root::reopen(&first.home)?)
    };
    if manual {
        let _state = manual_root(
            &backup::checkpoint_provenance(
                &initial.root.record.path,
                plan["spec"]["backupId"]
                    .as_str()
                    .ok_or_else(storage::failed)?,
            )?
            .ok_or_else(|| error(AdoptionReason::BackupUnverified))?,
            "state",
            &initial,
        )?;
    }
    drop(initial);
    let state_path = std::path::absolute(state_root).map_err(|_| storage::unsafe_path())?;
    let _user = home.lock(".graphhelm-adoption.lock")?;
    let _project = project.lock(".graphhelm-adoption.lock")?;
    storage::diagnostic_stage("restore.apply.acquire_locks");
    let store = journal::Store::open(Root::open(state_root, false)?)?;
    if let Some(mut prior) = store.read(&transaction_id)? {
        if prior.restore.is_none() || prior.project != project.record || prior.home != home.record {
            return Err(storage::failed());
        }
        return resume(&store, &project, &home, &mut prior, hook);
    }
    if manual {
        let expected_binding = journal_binding(&store)?;
        if plan["spec"]["journalBinding"].as_str() != Some(expected_binding.as_str()) {
            return Err(error(AdoptionReason::PlanStale));
        }
        let keys = plan["spec"]["effects"]
            .as_array()
            .ok_or_else(storage::failed)?
            .iter()
            .filter(|effect| effect["root"] == "home" && effect["action"] == "restore")
            .map(|effect| {
                effect["path"]
                    .as_str()
                    .map(|path| format!("file/{path}"))
                    .ok_or_else(storage::failed)
            })
            .collect::<Result<Vec<_>, AdoptionError>>()?;
        let _claims = crate::ownership::check(&home, &project, &state_path, &keys)?;
    }
    store.ensure_idle(&transaction_id)?;
    let selection = if plan["spec"]["selection"] == "original" {
        "original"
    } else {
        plan["spec"]["backupId"]
            .as_str()
            .ok_or_else(storage::failed)?
    };
    storage::diagnostic_stage("restore.apply.prepare_sources");
    let sources = if manual {
        sources_for_checkpoint(&store)?
    } else {
        chain(&store)?
    };
    let prepared = prepare(&store, &project, &home, selection, sources, manual)?;
    if prepared.value != *plan {
        return Err(error(AdoptionReason::PlanStale));
    }
    if !plan["spec"]["conflicts"]
        .as_array()
        .ok_or_else(storage::failed)?
        .is_empty()
    {
        return Ok(
            json!({"apiVersion":"p50.dev/adoption/v1","kind":"ApplyReceipt","id":transaction_id,"spec":{"transactionId":transaction_id,"planDigest":accepted_digest,"backupId":plan["spec"]["backupId"],"state":"recovery_required"},"conflicts":plan["spec"]["conflicts"]}),
        );
    }
    storage::diagnostic_stage("restore.apply.backup");
    let checkpoint =
        backup::backup_locked(&project, &home, &store.root.record.path, 1024 * 1024 * 1024)?;
    for (entry, target) in prepared.entries.iter().zip(&prepared.targets) {
        if apply::digest(&backup::verified_blob(
            &store.root.record.path,
            checkpoint["id"].as_str().ok_or_else(storage::failed)?,
            &format!("{}/{}", entry.root, entry.path),
        )?) != entry.before_digest
        {
            return Err(error(AdoptionReason::PlanStale));
        }
        store.save_payload(target)?;
    }
    let mut record = journal::Journal {
        version: 1,
        transaction_id,
        sequence: 0,
        plan_digest: accepted_digest.into(),
        backup_id: checkpoint["id"]
            .as_str()
            .ok_or_else(storage::failed)?
            .into(),
        project: project.record.clone(),
        home: home.record.clone(),
        entries: prepared.entries,
        packages: Vec::new(),
        state: TransactionState::Restoring,
        receipt: None,
        predecessor: store.active()?.map(|r| r.transaction_id),
        restore: Some(RestoreIntent {
            selected_backup: plan["spec"]["backupId"]
                .as_str()
                .ok_or_else(storage::failed)?
                .into(),
            sources: if manual {
                Vec::new()
            } else {
                prepared
                    .sources
                    .iter()
                    .map(|r| r.transaction_id.clone())
                    .collect()
            },
        }),
    };
    sync(&store, &mut record, hook)?;
    resume(&store, &project, &home, &mut record, hook)
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
pub(crate) fn resume(
    store: &journal::Store,
    project: &Root,
    home: &Root,
    record: &mut journal::Journal,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    storage::diagnostic_stage("restore.resume");
    if record.state == TransactionState::Restored {
        for entry in &record.entries {
            let root = if entry.root == "project" {
                project
            } else {
                home
            };
            let source = root.source(&entry.path)?;
            if apply::digest(&source.read()?) != entry.after_digest
                || storage::access(&source.file)? != entry.access
            {
                return Err(error(AdoptionReason::PlanStale));
            }
        }
        for id in &record.restore.as_ref().ok_or_else(storage::failed)?.sources {
            let source = store.read(id)?.ok_or_else(storage::failed)?;
            crate::hosts::preflight_compensation(store, &source)?;
            for package in source.packages {
                let root = Root::reopen(package.root.as_ref().ok_or_else(storage::failed)?)?;
                if storage::read_child(&root.file, "active.json", 4096)?.is_some() {
                    return Err(error(AdoptionReason::PlanStale));
                }
            }
        }
        return record.receipt.clone().ok_or_else(storage::failed);
    }
    let result = resume_inner(store, project, home, record, hook);
    if result.is_err() {
        record.state = TransactionState::RecoveryRequired;
        let receipt = apply::receipt(record);
        record.receipt = Some(receipt.clone());
        sync(store, record, hook)?;
        return Ok(receipt);
    }
    result
}
fn resume_inner(
    store: &journal::Store,
    project: &Root,
    home: &Root,
    record: &mut journal::Journal,
    hook: &mut dyn FnMut(Boundary) -> Result<(), AdoptionError>,
) -> Result<Value, AdoptionError> {
    storage::diagnostic_stage("restore.resume.preflight");
    let restore = record.restore.clone().ok_or_else(storage::failed)?;
    backup::verify_backup(&store.root.record.path, &restore.selected_backup)?;
    backup::verify_backup(&store.root.record.path, &record.backup_id)?;
    if restore.sources.is_empty()
        && backup::checkpoint_provenance(&store.root.record.path, &restore.selected_backup)?
            .is_some()
    {
        let keys = record
            .entries
            .iter()
            .filter(|entry| entry.root == "home")
            .map(|entry| format!("file/{}", entry.path))
            .collect::<Vec<_>>();
        let _claims = crate::ownership::check(home, project, &store.root.record.path, &keys)?;
    }
    for id in &restore.sources {
        crate::hosts::preflight_compensation(store, &store.read(id)?.ok_or_else(storage::failed)?)?;
    }
    // Preflight every destination before the first publication, including recovery guards.
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
        let current = apply::digest(&source.read()?);
        let current_access = storage::access(&source.file)?;
        let before_access = entry.before_access.as_ref().unwrap_or(&entry.access);
        let at_before = current == entry.before_digest && current_access == *before_access;
        let at_after = current == entry.after_digest && current_access == entry.access;
        if ![&entry.before_digest, &entry.after_digest].contains(&&current)
            || (!at_before && !at_after)
        {
            return Err(storage::failed());
        }
        store.payload(&entry.after_digest)?;
    }
    for index in 0..record.entries.len() {
        let entry = &record.entries[index];
        let root = if entry.root == "project" {
            project
        } else {
            home
        };
        let source = root.source(&entry.path)?;
        if apply::digest(&source.read()?) != entry.after_digest
            || storage::access(&source.file)? != entry.access
        {
            if entry.guards.len() >= 16 {
                return Err(storage::failed());
            }
            let prepared = source.prepare(
                &entry.before_digest,
                &store.payload(&entry.after_digest)?,
                entry.before_access.as_ref().unwrap_or(&entry.access),
                &entry.access,
                true,
            )?;
            record.entries[index].guards.push(prepared.record.clone());
            record.entries[index].phase = Phase::RestoreIntent;
            sync(store, record, hook)?;
            hook(Boundary::BeforePublish)?;
            prepared.publish(&mut |boundary| match boundary {
                storage::PublicationBoundary::Validated => {
                    hook(Boundary::FinalCompensationPublication)
                }
                #[cfg(windows)]
                storage::PublicationBoundary::Detached => {
                    hook(Boundary::AfterCompensationDetach)?;
                    record.entries[index].phase = Phase::RestoreDetached;
                    sync(store, record, hook)
                }
                storage::PublicationBoundary::Published => hook(Boundary::AfterCompensationCapture),
            })?;
            hook(Boundary::AfterPublish)?;
        }
        record.entries[index].phase = Phase::Reverted;
        sync(store, record, hook)?;
    }
    for id in &restore.sources {
        let mut source = store.read(id)?.ok_or_else(storage::failed)?;
        crate::hosts::compensate_packages(store, &source)?;
        source.state = TransactionState::Restored;
        source.receipt = Some(apply::receipt(&source));
        sync(store, &mut source, hook)?;
    }
    record.state = TransactionState::Restored;
    let receipt = apply::receipt(record);
    record.receipt = Some(receipt.clone());
    sync(store, record, hook)?;
    Ok(receipt)
}

#[cfg(test)]
#[path = "../tests/support/restore_harness.rs"]
mod tests;
