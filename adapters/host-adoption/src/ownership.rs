//! Durable exclusive ownership of shared user surfaces. Overlapping projects fail closed.
use crate::{
    journal,
    storage::{self, Root, RootRecord},
};
use graphhelm_protocols::adoption::{AdoptionError, TransactionState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const FILE: &str = ".graphhelm-adoption-owners.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owner {
    project: RootRecord,
    state: RootRecord,
    transaction_id: String,
}
pub(crate) struct Claims(BTreeMap<String, Owner>);
pub(crate) fn check(
    home: &Root,
    project: &Root,
    state: &std::path::Path,
    keys: &[String],
) -> Result<Claims, AdoptionError> {
    let owners: BTreeMap<String, Owner> = storage::read_child(&home.file, FILE, 1024 * 1024)?
        .map(|b| serde_json::from_slice(&b).map_err(|_| storage::failed()))
        .transpose()?
        .unwrap_or_default();
    if owners.len() > 256 {
        return Err(storage::failed());
    }
    for key in keys {
        if let Some(owner) = owners.get(key) {
            if owner.project == project.record && owner.state.path == state {
                continue;
            }
            let store = journal::Store::reader(Root::reopen(&owner.state)?)?;
            if !claim_chain_restored(&store, owner, &home.record, key)? {
                return Err(storage::failed());
            }
        }
    }
    Ok(Claims(owners))
}

fn claim_chain_restored(
    store: &journal::Store,
    owner: &Owner,
    home: &RootRecord,
    key: &str,
) -> Result<bool, AdoptionError> {
    let mut next = Some(owner.transaction_id.clone());
    let mut seen = BTreeSet::new();
    let mut found = false;
    while let Some(transaction_id) = next {
        if seen.len() == 256 || !seen.insert(transaction_id.clone()) {
            return Err(storage::failed());
        }
        let record = store.read(&transaction_id)?.ok_or_else(storage::failed)?;
        if record.project != owner.project || record.home != *home {
            return Ok(false);
        }
        let owns_key = record_claims_key(&record, key);
        if !found && !owns_key {
            return Ok(false);
        }
        if owns_key {
            found = true;
        }
        if owns_key && record.state != TransactionState::Restored {
            return Ok(false);
        }
        next = record.predecessor;
    }
    Ok(found)
}
fn record_claims_key(record: &journal::Journal, key: &str) -> bool {
    if let Some(path) = key.strip_prefix("file/") {
        return record
            .entries
            .iter()
            .any(|entry| entry.root == "home" && entry.path == path);
    }
    if let Some(id) = key.strip_prefix("package/") {
        return record.packages.iter().any(|package| package.id == id);
    }
    false
}
impl Claims {
    pub(crate) fn publish(
        mut self,
        home: &Root,
        project: &Root,
        store: &journal::Store,
        transaction_id: &str,
        keys: &[String],
    ) -> Result<(), AdoptionError> {
        if keys.is_empty() {
            return Ok(());
        }
        for key in keys {
            self.0.insert(
                key.clone(),
                Owner {
                    project: project.record.clone(),
                    state: store.root.record.clone(),
                    transaction_id: transaction_id.into(),
                },
            );
        }
        if self.0.len() > 256 {
            return Err(storage::failed());
        }
        home.verify()?;
        storage::write_atomic(
            &home.file,
            FILE,
            &serde_json::to_vec(&self.0).map_err(|_| storage::failed())?,
        )?;
        home.verify()
    }
}
