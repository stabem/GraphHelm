use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Seek, SeekFrom, Write},
};

use graphhelm_events::{
    AuthenticationTag, KeyError, RevocationReceipt, RevokeKeyRequest, SecretBytes,
};
use graphhelm_protocols::OpaqueId;
use serde::{Deserialize, Serialize};

use crate::{
    AnchoredDirectory, FileAccess, authenticate_internal, durable_sync_directory, keyring,
    verify_internal,
};

pub(crate) const JOURNAL_FILE: &str = "revocations.v1.jsonl";
const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 10_000;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalHeader {
    format_version: u8,
    key_id: String,
    authentication_tag: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalRecord {
    epoch: u64,
    handle: String,
    idempotency_key: String,
    previous_tag: String,
    receipt_tag: String,
    record_tag: String,
}

pub(crate) struct JournalState {
    header_tag: Vec<u8>,
    records: Vec<JournalRecord>,
    idempotency: BTreeMap<String, usize>,
    revoked_handles: BTreeSet<String>,
    byte_length: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct JournalFloor {
    epoch: u64,
    authenticated_head: [u8; 32],
}

impl JournalState {
    pub(crate) fn epoch(&self) -> u64 {
        self.records.last().map_or(0, |record| record.epoch)
    }

    pub(crate) fn is_revoked(&self, handle: &str) -> bool {
        self.revoked_handles.contains(handle)
    }

    pub(crate) fn floor(&self) -> Result<JournalFloor, KeyError> {
        Ok(JournalFloor {
            epoch: self.epoch(),
            authenticated_head: self.authenticated_head_at(self.epoch())?,
        })
    }

    pub(crate) fn require_extension_of(&self, floor: &JournalFloor) -> Result<(), KeyError> {
        if self.epoch() < floor.epoch
            || self.authenticated_head_at(floor.epoch)? != floor.authenticated_head
        {
            return Err(KeyError::Integrity);
        }
        Ok(())
    }

    fn authenticated_head_at(&self, epoch: u64) -> Result<[u8; 32], KeyError> {
        if epoch == 0 {
            return self
                .header_tag
                .as_slice()
                .try_into()
                .map_err(|_| KeyError::Integrity);
        }
        let index = usize::try_from(epoch.checked_sub(1).ok_or(KeyError::Integrity)?)
            .map_err(|_| KeyError::Integrity)?;
        decode_tag(
            &self
                .records
                .get(index)
                .ok_or(KeyError::Integrity)?
                .record_tag,
        )?
        .as_slice()
        .try_into()
        .map_err(|_| KeyError::Integrity)
    }
}

pub(crate) fn create_or_recover_empty(
    directory: &AnchoredDirectory,
    key_id: &str,
    key_material: &SecretBytes,
) -> Result<File, KeyError> {
    if let Some(mut file) = directory.try_open_regular(JOURNAL_FILE, FileAccess::ReadWrite)? {
        let state = load(&mut file, key_id, key_material)?;
        return if state.epoch() == 0 {
            Ok(file)
        } else {
            Err(KeyError::Conflict)
        };
    }

    let canonical = canonical_header_bytes(key_id)?;
    let tag = authenticate_internal(key_material, "revocation-journal-header", &canonical)?;
    let header = JournalHeader {
        format_version: 1,
        key_id: key_id.to_owned(),
        authentication_tag: hex::encode(tag),
    };
    let mut bytes = serde_json::to_vec(&header).map_err(|_| KeyError::Storage)?;
    bytes.push(b'\n');
    let mut file = directory.create_regular(JOURNAL_FILE)?;
    file.write_all(&bytes).map_err(|_| KeyError::Storage)?;
    file.flush().map_err(|_| KeyError::Storage)?;
    file.sync_all().map_err(|_| KeyError::Storage)?;
    durable_sync_directory(directory)?;
    directory.verify_child_identity(JOURNAL_FILE, &file)?;
    load(&mut file, key_id, key_material)?;
    Ok(file)
}

pub(crate) fn open(directory: &AnchoredDirectory) -> Result<File, KeyError> {
    directory.open_regular(JOURNAL_FILE, FileAccess::ReadWrite)
}

pub(crate) fn load(
    file: &mut File,
    expected_key_id: &str,
    key_material: &SecretBytes,
) -> Result<JournalState, KeyError> {
    let mut bytes = keyring::read_bounded(file, MAX_JOURNAL_BYTES)?;
    // A record becomes committed only when its terminating newline is durable. An append that was
    // interrupted between `write_all` and `sync_all` therefore leaves a tail that was never
    // acknowledged to any caller, and keeping it would make every later operation - including
    // unwrapping keys that were never revoked - fail forever. Discard exactly that tail and
    // reconcile the file length so the next append writes at the right offset.
    //
    // This is not a general repair. Only a tail that was never a complete record is removed: an
    // interrupted write leaves truncated JSON that cannot deserialize. A tail that still parses as
    // a well-formed record is a committed record whose terminator was stripped, which is tampering
    // and a revocation rollback, so it fails closed instead. Corruption inside a committed record
    // likewise fails closed below, because every record is bound into the HMAC chain through its
    // predecessor's tag.
    let committed = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if committed != bytes.len() {
        if serde_json::from_slice::<JournalRecord>(&bytes[committed..]).is_ok() {
            return Err(KeyError::Integrity);
        }
        file.set_len(u64::try_from(committed).map_err(|_| KeyError::Integrity)?)
            .map_err(|_| KeyError::Storage)?;
        file.sync_all().map_err(|_| KeyError::Storage)?;
        bytes.truncate(committed);
    }
    if bytes.is_empty() {
        return Err(KeyError::Integrity);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| KeyError::Integrity)?;
    let mut lines = text.lines();
    let header: JournalHeader = serde_json::from_str(lines.next().ok_or(KeyError::Integrity)?)
        .map_err(|_| KeyError::Integrity)?;
    if header.format_version != 1 || header.key_id != expected_key_id {
        return Err(KeyError::Integrity);
    }
    let header_tag = decode_tag(&header.authentication_tag)?;
    verify_internal(
        key_material,
        "revocation-journal-header",
        &canonical_header_bytes(expected_key_id)?,
        &header_tag,
    )?;

    let mut records = Vec::new();
    let mut idempotency = BTreeMap::new();
    let mut revoked_handles = BTreeSet::new();
    let mut previous_tag = header_tag.clone();
    for line in lines {
        if records.len() >= MAX_RECORDS {
            return Err(KeyError::Integrity);
        }
        let record: JournalRecord = serde_json::from_str(line).map_err(|_| KeyError::Integrity)?;
        let expected_epoch = u64::try_from(records.len())
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(KeyError::Integrity)?;
        if record.epoch != expected_epoch
            || OpaqueId::parse(record.handle.clone()).is_err()
            || OpaqueId::parse(record.idempotency_key.clone()).is_err()
            || idempotency.contains_key(&record.idempotency_key)
            || decode_tag(&record.previous_tag)? != previous_tag
        {
            return Err(KeyError::Integrity);
        }
        let receipt_tag = decode_tag(&record.receipt_tag)?;
        verify_internal(
            key_material,
            "revocation-receipt",
            &canonical_receipt_bytes(&record.handle, &record.idempotency_key, record.epoch)?,
            &receipt_tag,
        )?;
        let record_tag = decode_tag(&record.record_tag)?;
        verify_internal(
            key_material,
            "revocation-journal-record",
            &canonical_record_bytes(
                record.epoch,
                &record.handle,
                &record.idempotency_key,
                &previous_tag,
                &receipt_tag,
            )?,
            &record_tag,
        )?;
        previous_tag = record_tag;
        idempotency.insert(record.idempotency_key.clone(), records.len());
        revoked_handles.insert(record.handle.clone());
        records.push(record);
    }
    Ok(JournalState {
        header_tag,
        records,
        idempotency,
        revoked_handles,
        byte_length: u64::try_from(bytes.len()).map_err(|_| KeyError::Integrity)?,
    })
}

pub(crate) fn append_revocation(
    file: &mut File,
    directory: &AnchoredDirectory,
    key_id: &str,
    key_material: &SecretBytes,
    state: JournalState,
    request: RevokeKeyRequest,
) -> Result<RevocationReceipt, KeyError> {
    if let Some(index) = state.idempotency.get(request.idempotency_key()) {
        let record = &state.records[*index];
        if record.handle != request.handle() {
            return Err(KeyError::Conflict);
        }
        return receipt_from_record(key_id, record);
    }
    if state.records.len() >= MAX_RECORDS {
        return Err(KeyError::Invalid);
    }

    let (handle, idempotency_key) = request.into_parts();
    let epoch = state.epoch().checked_add(1).ok_or(KeyError::Conflict)?;
    let previous_tag = if let Some(last) = state.records.last() {
        decode_tag(&last.record_tag)?
    } else {
        state.header_tag
    };
    let receipt_tag = authenticate_internal(
        key_material,
        "revocation-receipt",
        &canonical_receipt_bytes(&handle, &idempotency_key, epoch)?,
    )?;
    let record_tag = authenticate_internal(
        key_material,
        "revocation-journal-record",
        &canonical_record_bytes(
            epoch,
            &handle,
            &idempotency_key,
            &previous_tag,
            &receipt_tag,
        )?,
    )?;
    let record = JournalRecord {
        epoch,
        handle,
        idempotency_key,
        previous_tag: hex::encode(previous_tag),
        receipt_tag: hex::encode(&receipt_tag),
        record_tag: hex::encode(record_tag),
    };
    let mut bytes = serde_json::to_vec(&record).map_err(|_| KeyError::Storage)?;
    bytes.push(b'\n');
    if bytes.len() > 4096 {
        return Err(KeyError::Invalid);
    }

    let current_length = file.metadata().map_err(|_| KeyError::Storage)?.len();
    if current_length != state.byte_length {
        return Err(KeyError::Integrity);
    }
    file.seek(SeekFrom::Start(state.byte_length))
        .map_err(|_| KeyError::Storage)?;
    file.write_all(&bytes).map_err(|_| KeyError::Storage)?;
    file.flush().map_err(|_| KeyError::Storage)?;
    file.sync_all().map_err(|_| KeyError::Storage)?;
    durable_sync_directory(directory)?;
    receipt_from_record(key_id, &record)
}

fn receipt_from_record(
    key_id: &str,
    record: &JournalRecord,
) -> Result<RevocationReceipt, KeyError> {
    RevocationReceipt::new(
        record.handle.clone(),
        record.idempotency_key.clone(),
        record.epoch,
        AuthenticationTag::new(key_id, "hmac-sha256", decode_tag(&record.receipt_tag)?)?,
    )
}

fn decode_tag(value: &str) -> Result<Vec<u8>, KeyError> {
    let tag = hex::decode(value).map_err(|_| KeyError::Integrity)?;
    if tag.len() == 32 {
        Ok(tag)
    } else {
        Err(KeyError::Integrity)
    }
}

fn canonical_header_bytes(key_id: &str) -> Result<Vec<u8>, KeyError> {
    let mut bytes = b"graphhelm-revocation-journal-header-v1".to_vec();
    push_field(&mut bytes, key_id.as_bytes())?;
    Ok(bytes)
}

fn canonical_receipt_bytes(
    handle: &str,
    idempotency_key: &str,
    epoch: u64,
) -> Result<Vec<u8>, KeyError> {
    let mut bytes = b"graphhelm-revocation-receipt-v1".to_vec();
    push_field(&mut bytes, handle.as_bytes())?;
    push_field(&mut bytes, idempotency_key.as_bytes())?;
    bytes.extend_from_slice(&epoch.to_be_bytes());
    Ok(bytes)
}

fn canonical_record_bytes(
    epoch: u64,
    handle: &str,
    idempotency_key: &str,
    previous_tag: &[u8],
    receipt_tag: &[u8],
) -> Result<Vec<u8>, KeyError> {
    let mut bytes = b"graphhelm-revocation-journal-record-v1".to_vec();
    bytes.extend_from_slice(&epoch.to_be_bytes());
    push_field(&mut bytes, handle.as_bytes())?;
    push_field(&mut bytes, idempotency_key.as_bytes())?;
    push_field(&mut bytes, previous_tag)?;
    push_field(&mut bytes, receipt_tag)?;
    Ok(bytes)
}

fn push_field(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KeyError> {
    let length = u32::try_from(bytes.len()).map_err(|_| KeyError::Invalid)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}
