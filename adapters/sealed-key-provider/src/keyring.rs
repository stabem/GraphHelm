use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

use graphhelm_events::{KeyError, SecretBytes};
use serde::{Deserialize, Serialize};

use crate::{
    AnchoredDirectory, FileAccess, authenticate_internal, durable_sync_directory, verify_internal,
};

pub(crate) const KEYRING_FILE: &str = "keyring.v1.json";
const KEYRING_TEMP_FILE: &str = ".keyring.v1.pending";
const MAX_KEYRING_BYTES: u64 = 4 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyringDocument {
    format_version: u8,
    key_id: String,
    authentication_tag: String,
}

pub(crate) fn exists(directory: &AnchoredDirectory) -> Result<bool, KeyError> {
    Ok(directory
        .try_open_regular(KEYRING_FILE, FileAccess::ReadOnly)?
        .is_some())
}

pub(crate) fn publish(
    directory: &AnchoredDirectory,
    key_id: &str,
    key_material: &SecretBytes,
) -> Result<File, KeyError> {
    if exists(directory)? {
        return Err(KeyError::Conflict);
    }
    directory.remove_file_if_exists(KEYRING_TEMP_FILE)?;

    let canonical = canonical_keyring_bytes(key_id)?;
    let tag = authenticate_internal(key_material, "keyring-v1", &canonical)?;
    let document = KeyringDocument {
        format_version: 1,
        key_id: key_id.to_owned(),
        authentication_tag: hex::encode(tag),
    };
    let mut bytes = serde_json::to_vec(&document).map_err(|_| KeyError::Storage)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_KEYRING_BYTES {
        return Err(KeyError::Invalid);
    }

    let mut file = directory.create_linkable_regular(KEYRING_TEMP_FILE)?;
    let result = (|| {
        file.write_all(&bytes).map_err(|_| KeyError::Storage)?;
        file.flush().map_err(|_| KeyError::Storage)?;
        file.sync_all().map_err(|_| KeyError::Storage)?;
        directory.link_no_replace(KEYRING_TEMP_FILE, KEYRING_FILE)?;
        directory.verify_child_identity(KEYRING_FILE, &file)?;
        durable_sync_directory(directory)?;
        directory.remove_file_if_exists(KEYRING_TEMP_FILE)?;
        durable_sync_directory(directory)?;
        let mut retained = directory.open_regular(KEYRING_FILE, FileAccess::ReadOnly)?;
        if crate::file_identity(&retained)? != crate::file_identity(&file)? {
            return Err(KeyError::Integrity);
        }
        verify_file(&mut retained, key_id, key_material)?;
        Ok(retained)
    })();
    if result.is_err() {
        let _ = directory.remove_file_if_exists(KEYRING_TEMP_FILE);
    }
    result
}

pub(crate) fn open_and_verify(
    directory: &AnchoredDirectory,
    expected_key_id: &str,
    key_material: &SecretBytes,
) -> Result<File, KeyError> {
    let mut file = directory.open_regular(KEYRING_FILE, FileAccess::ReadOnly)?;
    verify_file(&mut file, expected_key_id, key_material)?;
    directory.verify_child_identity(KEYRING_FILE, &file)?;
    Ok(file)
}

fn verify_file(
    file: &mut File,
    expected_key_id: &str,
    key_material: &SecretBytes,
) -> Result<(), KeyError> {
    let bytes = read_bounded(file, MAX_KEYRING_BYTES)?;
    if !bytes.ends_with(b"\n") {
        return Err(KeyError::Integrity);
    }
    let document: KeyringDocument =
        serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|_| KeyError::Integrity)?;
    if document.format_version != 1 || document.key_id != expected_key_id {
        return Err(KeyError::Integrity);
    }
    let tag = hex::decode(document.authentication_tag).map_err(|_| KeyError::Integrity)?;
    if tag.len() != 32 {
        return Err(KeyError::Integrity);
    }
    verify_internal(
        key_material,
        "keyring-v1",
        &canonical_keyring_bytes(expected_key_id)?,
        &tag,
    )
}

pub(crate) fn read_bounded(file: &mut File, maximum: u64) -> Result<Vec<u8>, KeyError> {
    let metadata = file.metadata().map_err(|_| KeyError::Storage)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(KeyError::Integrity);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| KeyError::Storage)?;
    let mut bytes =
        Vec::with_capacity(usize::try_from(metadata.len()).map_err(|_| KeyError::Integrity)?);
    Read::by_ref(file)
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| KeyError::Storage)?;
    let after = file.metadata().map_err(|_| KeyError::Storage)?;
    if bytes.len() as u64 != metadata.len() || after.len() != metadata.len() {
        return Err(KeyError::Integrity);
    }
    Ok(bytes)
}

fn canonical_keyring_bytes(key_id: &str) -> Result<Vec<u8>, KeyError> {
    let mut bytes = b"graphhelm-keyring-v1".to_vec();
    let length = u32::try_from(key_id.len()).map_err(|_| KeyError::Invalid)?;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(key_id.as_bytes());
    Ok(bytes)
}
