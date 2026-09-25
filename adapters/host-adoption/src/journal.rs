//! Private, bounded journal snapshots. A checksum and strict schema reject torn/corrupt intents.
use crate::storage::{self, Access, Root, RootRecord};
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason, TransactionState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::File;
const MAX_JOURNAL_BYTES: u64 = 10 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Planned,
    Intent,
    Published,
    Reverted,
    RestoreIntent,
    Detached,
    RestoreDetached,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    pub operation_index: usize,
    pub root: String,
    pub path: String,
    pub before_digest: String,
    pub after_digest: String,
    pub access: Access,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_access: Option<Access>,
    pub phase: Phase,
    #[serde(default)]
    pub guards: Vec<storage::GuardRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Journal {
    pub version: u8,
    pub transaction_id: String,
    pub sequence: u64,
    pub plan_digest: String,
    pub backup_id: String,
    pub project: RootRecord,
    pub home: RootRecord,
    pub entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<crate::hosts::PackageEntry>,
    pub state: TransactionState,
    pub receipt: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<crate::restore::RestoreIntent>,
}
pub(crate) struct Store {
    pub root: Root,
    dir: File,
    _lock: Option<storage::AuthorityLock>,
}
impl Store {
    pub fn active(&self) -> Result<Option<Journal>, AdoptionError> {
        let Some(bytes) = storage::read_child(&self.root.file, "active.json", 1024)? else {
            return Ok(None);
        };
        let active: Value = serde_json::from_slice(&bytes).map_err(|_| storage::failed())?;
        self.read(
            active["transactionId"]
                .as_str()
                .ok_or_else(storage::failed)?,
        )
    }
    pub fn save_payload(&self, bytes: &[u8]) -> Result<(), AdoptionError> {
        let dir = self.root.private_child("payloads")?;
        let id = crate::apply::digest(bytes);
        if let Some(existing) = storage::read_child(&dir, &id, 1024 * 1024)? {
            if existing != bytes {
                return Err(storage::failed());
            }
        } else {
            storage::write_atomic(&dir, &id, bytes)?;
        }
        Ok(())
    }
    pub fn payload(&self, digest: &str) -> Result<Vec<u8>, AdoptionError> {
        if !crate::valid_backup_id(digest) {
            return Err(storage::failed());
        }
        let dir = storage::directory_child(&self.root.file, "payloads")?;
        let bytes = storage::read_child(&dir, digest, 1024 * 1024)?.ok_or_else(storage::failed)?;
        if crate::apply::digest(&bytes) != digest {
            return Err(storage::failed());
        }
        Ok(bytes)
    }
    pub fn open(root: Root) -> Result<Self, AdoptionError> {
        let lock = root.lock(".adoption.lock")?;
        let dir = root.private_child("journals")?;
        Ok(Self {
            root,
            dir,
            _lock: Some(lock),
        })
    }
    pub fn reader(root: Root) -> Result<Self, AdoptionError> {
        let dir = storage::directory_child(&root.file, "journals")?;
        Ok(Self {
            root,
            dir,
            _lock: None,
        })
    }
    fn verify(&self) -> Result<(), AdoptionError> {
        self.root.verify()?;
        let current = storage::directory_child(&self.root.file, "journals")?;
        if storage::identity(&current)? != storage::identity(&self.dir)? {
            return Err(storage::failed());
        }
        Ok(())
    }
    pub fn ensure_idle(&self, candidate: &str) -> Result<(), AdoptionError> {
        if let Some(bytes) = storage::read_child(&self.root.file, "active.json", 1024)? {
            let active: Value = serde_json::from_slice(&bytes).map_err(|_| storage::failed())?;
            let id = active["transactionId"]
                .as_str()
                .ok_or_else(storage::failed)?;
            let prior = self.read(id)?.ok_or_else(storage::failed)?;
            if id != candidate
                && !matches!(
                    prior.state,
                    TransactionState::InstalledUnverified
                        | TransactionState::Verified
                        | TransactionState::Restored
                )
            {
                return Err(storage::failed());
            }
        }
        Ok(())
    }
    pub fn read(&self, id: &str) -> Result<Option<Journal>, AdoptionError> {
        if !crate::backup::valid_backup_id(id) {
            return Err(storage::unsafe_path());
        }
        self.verify()?;
        let Some(bytes) = storage::read_child(&self.dir, &format!("{id}.json"), MAX_JOURNAL_BYTES)?
        else {
            return Ok(None);
        };
        let envelope: Value = serde_json::from_slice(&bytes).map_err(|_| storage::failed())?;
        if !graphhelm_schema::validate_adoption_journal(&envelope).is_empty() {
            return Err(storage::failed());
        }
        let record = &envelope["record"];
        if envelope["checksum"]
            != crate::apply::digest(
                &graphhelm_graph::canonical_content_bytes(record).map_err(|_| storage::failed())?,
            )
        {
            return Err(storage::failed());
        }
        let value: Journal =
            serde_json::from_value(record.clone()).map_err(|_| storage::failed())?;
        // A checksum detects corruption, not observer custody. There is no production host
        // observer in this build, so no persisted JSON can authorize a verified result on
        // apply, recover, restore, or verification reads. Unit fixtures alone exercise the
        // in-memory custody path; copying their journal into a production build still refuses.
        #[cfg(not(test))]
        if value.state == TransactionState::Verified
            || value.receipt.as_ref().is_some_and(|receipt| {
                receipt["spec"]["state"] == "verified"
                    || receipt["verification"]["status"] == "verified"
                    || receipt.get("activationReceipt").is_some()
            })
        {
            return Err(AdoptionError {
                reason: AdoptionReason::ObserverMissing,
            });
        }
        if value.version != 1
            || value.sequence == 0
            || value.transaction_id != id
            || (value.entries.is_empty() && value.packages.is_empty() && value.restore.is_none())
            || value.entries.len() > 256
            || value.transaction_id != crate::apply::digest(value.plan_digest.as_bytes())
            || !crate::backup::valid_backup_id(&value.backup_id)
        {
            return Err(storage::failed());
        }
        for (i, entry) in value.entries.iter().enumerate() {
            if entry.operation_index != i
                || !matches!(entry.root.as_str(), "project" | "home")
                || !crate::backup::valid_backup_id(&entry.before_digest)
                || !crate::backup::valid_backup_id(&entry.after_digest)
            {
                return Err(storage::failed());
            }
            storage::relative_ok(&entry.path)?;
        }
        Ok(Some(value))
    }
    pub fn sync_with_hook(
        &self,
        record: &mut Journal,
        between: &mut dyn FnMut() -> Result<(), AdoptionError>,
    ) -> Result<(), AdoptionError> {
        self.verify()?;
        if self._lock.is_none() {
            return Err(storage::failed());
        }
        let initial = record.sequence == 0;
        record.sequence = record.sequence.checked_add(1).ok_or_else(storage::failed)?;
        let value = serde_json::to_value(&record).map_err(|_| storage::failed())?;
        let checksum = crate::apply::digest(
            &graphhelm_graph::canonical_content_bytes(&value).map_err(|_| storage::failed())?,
        );
        let envelope = json!({"record":value,"checksum":checksum});
        if !graphhelm_schema::validate_adoption_journal(&envelope).is_empty() {
            return Err(storage::failed());
        }
        let bytes = serde_json::to_vec(&envelope).map_err(|_| storage::failed())?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        storage::write_atomic(
            &self.dir,
            &format!("{}.json", record.transaction_id),
            &bytes,
        )?;
        let durable = self
            .read(&record.transaction_id)?
            .ok_or_else(storage::failed)?;
        if durable.sequence != record.sequence {
            return Err(storage::failed());
        }
        if initial {
            // A crash here leaves a self-contained planned journal, never a dangling pointer.
            // No source publication is allowed until this initial sync has returned successfully.
            between()?;
            storage::write_atomic(
                &self.root.file,
                "active.json",
                &serde_json::to_vec(&json!({"transactionId": record.transaction_id}))
                    .map_err(|_| storage::failed())?,
            )?;
        }
        Ok(())
    }
    pub fn original_once(
        &self,
        backup_id: &str,
        project: &RootRecord,
        home: &RootRecord,
    ) -> Result<(), AdoptionError> {
        if let Some(bytes) = storage::read_child(&self.root.file, "original.json", 64 * 1024)? {
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| storage::failed())?;
            if value["project"] != serde_json::to_value(project).map_err(|_| storage::failed())?
                || value["home"] != serde_json::to_value(home).map_err(|_| storage::failed())?
            {
                return Err(storage::failed());
            }
            crate::verify_backup(
                &self.root.record.path,
                value["backupId"].as_str().ok_or_else(storage::failed)?,
            )?;
            return Ok(());
        }
        storage::write_atomic(
            &self.root.file,
            "original.json",
            &serde_json::to_vec(&json!({"backupId":backup_id,"project":project,"home":home}))
                .map_err(|_| storage::failed())?,
        )
    }
}
