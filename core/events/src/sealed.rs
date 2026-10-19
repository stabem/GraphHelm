//! Canonical metadata binding immutable journal segments to their predecessor and verified heads.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const LAYOUT_FORMAT_BYTES: &[u8] = b"{\"formatVersion\":\"1.1.0\"}\n";
pub(crate) const LAYOUT_FORMAT_VERSION: &str = "1.1.0";
pub(crate) const ROTATE_EVENTS: usize = 50_000;
pub(crate) const ROTATE_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_METADATA_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SegmentSeal {
    pub format_version: String,
    pub index: u64,
    pub bytes: u64,
    pub sha256: String,
    pub keys_sha256: String,
    pub previous_seal: String,
    pub end_state: SegmentEndState,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SegmentEndState {
    pub next_sequence: BTreeMap<String, u64>,
    pub last_hash: BTreeMap<String, String>,
    pub active_versions: BTreeMap<String, (u64, String, u64, String)>,
    pub counters: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SegmentKey {
    pub identity: String,
    pub offset: u64,
}

use super::*;

pub(super) fn require_rotation_platform() -> Result<(), EventRepositoryError> {
    #[cfg(windows)]
    {
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(EventRepositoryError::SegmentedPlatformUnsupported)
    }
}

pub(super) fn segmented_format(
    directory: &File,
    root: &Path,
) -> Result<bool, EventRepositoryError> {
    let mut marker = open_child_file(directory, root, "format.json", false, false)?;
    match read_bounded_file(&mut marker, 1024)?.as_slice() {
        super::FORMAT_BYTES => Ok(false),
        LAYOUT_FORMAT_BYTES => Ok(true),
        _ => Err(EventRepositoryError::UnsupportedFormat),
    }
}

pub(super) struct SealedSegment {
    pub(super) seal: SegmentSeal,
    pub(super) seal_hash: String,
    pub(super) identity: FileIdentity,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn segment_inventory(
    directory: &File,
    root: &Path,
    read_budget: &crate::ReadBudget,
) -> Result<BTreeMap<u64, BTreeSet<String>>, EventRepositoryError> {
    let mut inventory = BTreeMap::<u64, BTreeSet<String>>::new();
    let mut budget =
        DirectoryBudget::with_limits(MAX_REPOSITORY_ENTRIES, MAX_REPOSITORY_NAME_BYTES);
    for_each_child_name(directory, root, &mut budget, |name, _| {
        segment_read_budget(read_budget, inventory.len() as u64)?;
        let (index, suffix) = name
            .split_once('.')
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        let index = index
            .parse::<u64>()
            .map_err(|_| EventRepositoryError::UnsupportedFormat)?;
        if name != format!("{index}.{suffix}") || !matches!(suffix, "jsonl" | "seal.json" | "keys")
        {
            return Err(EventRepositoryError::UnsupportedFormat);
        }
        inventory
            .entry(index)
            .or_default()
            .insert(suffix.to_owned());
        Ok(())
    })?;
    if inventory.keys().copied().ne(0..inventory.len() as u64) {
        return Err(EventRepositoryError::IntegrityAt("segments:index-gap"));
    }
    Ok(inventory)
}

pub(super) fn read_segment_seal(
    directory: &File,
    root: &Path,
    index: u64,
    previous: &str,
    read_budget: &crate::ReadBudget,
) -> Result<(SegmentSeal, String), EventRepositoryError> {
    let mut file = open_child_file(directory, root, &format!("{index}.seal.json"), false, false)?;
    let bytes = read_segment_bytes(&mut file, MAX_METADATA_BYTES, read_budget)?;
    let raw: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| EventRepositoryError::IntegrityAt("segments:seal-json"))?;
    segment_read_budget(read_budget, 0)?;
    validate_segment_seal(&raw)?;
    segment_read_budget(read_budget, 0)?;
    let seal: SegmentSeal = serde_json::from_value(raw)
        .map_err(|_| EventRepositoryError::IntegrityAt("segments:seal-json"))?;
    if canonical_bytes(&seal)? != bytes
        || seal.format_version != LAYOUT_FORMAT_VERSION
        || seal.index != index
        || seal.previous_seal != previous
        || seal.bytes == 0
    {
        return Err(EventRepositoryError::IntegrityAt("segments:seal-chain"));
    }
    ensure_inclusive_limit(seal.bytes, MAX_JOURNAL_BYTES)?;
    Ok((seal, segment_digest(&bytes, read_budget)?))
}

pub(super) fn validate_segment_seal(raw: &serde_json::Value) -> Result<(), EventRepositoryError> {
    let schemas =
        graphhelm_schema::repository_schema_set().map_err(|_| EventRepositoryError::Integrity)?;
    let (attempt, diagnostics) = schemas.validate_journal_seal_attempted(raw);
    match attempt {
        ValidationAttempt::Refused(
            ValidationRefusal::InstanceComplexity | ValidationRefusal::ValidationWork,
        ) => Err(EventRepositoryError::LimitExceeded),
        ValidationAttempt::Refused(ValidationRefusal::UnregisteredRoot) => {
            Err(EventRepositoryError::Integrity)
        }
        ValidationAttempt::Ran if !diagnostics.is_empty() => {
            Err(EventRepositoryError::IntegrityAt("segments:seal-schema"))
        }
        ValidationAttempt::Ran => Ok(()),
    }
}

pub(super) fn segment_keys(
    bytes: &[u8],
    read_budget: &crate::ReadBudget,
) -> Result<Vec<SegmentKey>, EventRepositoryError> {
    let schemas =
        graphhelm_schema::repository_schema_set().map_err(|_| EventRepositoryError::Integrity)?;
    let mut keys = Vec::new();
    let mut offset = 0_u64;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        segment_read_budget(read_budget, keys.len() as u64)?;
        if line.last() != Some(&b'\n') || line.len() == 1 {
            return Err(EventRepositoryError::IntegrityAt("segments:line-boundary"));
        }
        let batch = parse_physical_batch(schemas, &line[..line.len() - 1])?;
        let stream = stream_key(&batch.scope, &batch.stream_id)?;
        for event in batch.events {
            if keys.len() >= MAX_READ_ALL {
                return Err(EventRepositoryError::LimitExceeded);
            }
            keys.push(SegmentKey {
                identity: format!("{stream}:{}", event.idempotency_key),
                offset,
            });
        }
        offset += line.len() as u64;
    }
    keys.sort();
    Ok(keys)
}

pub(super) fn read_segment_chain(
    root_handle: &File,
    root: &Path,
    read_budget: &crate::ReadBudget,
) -> Result<Vec<SealedSegment>, EventRepositoryError> {
    read_segment_chain_until_pending(root_handle, root, false, read_budget)
}

pub(super) fn read_segment_chain_until_pending(
    root_handle: &File,
    root: &Path,
    permit_pending: bool,
    read_budget: &crate::ReadBudget,
) -> Result<Vec<SealedSegment>, EventRepositoryError> {
    let path = root.join("segments");
    let directory = open_child_directory(root_handle, root, "segments")?;
    let inventory = segment_inventory(&directory, &path, read_budget)?;
    let mut previous = GENESIS_HASH.to_owned();
    let mut segments = Vec::new();
    let mut total = 0_u64;
    for (index, names) in &inventory {
        segment_read_budget(read_budget, segments.len() as u64)?;
        if !names.contains("seal.json") {
            return Err(EventRepositoryError::IntegrityAt("segments:seal-missing"));
        }
        let (seal, hash) = read_segment_seal(&directory, &path, *index, &previous, read_budget)?;
        if !names.contains("jsonl") || !names.contains("keys") {
            if *index + 1 == inventory.len() as u64 {
                if permit_pending {
                    break;
                }
                return Err(EventRepositoryError::RecoveryRequired);
            }
            return Err(EventRepositoryError::IntegrityAt(
                "segments:incomplete-history",
            ));
        }
        total = total
            .checked_add(seal.bytes)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        ensure_inclusive_limit(total, MAX_JOURNAL_BYTES)?;
        let mut file = open_child_file(&directory, &path, &format!("{index}.jsonl"), false, false)?;
        let identity = file_identity(&file)?;
        let bytes = read_segment_bytes(&mut file, seal.bytes, read_budget)?;
        if bytes.len() as u64 != seal.bytes || segment_digest(&bytes, read_budget)? != seal.sha256 {
            return Err(EventRepositoryError::IntegrityAt("segments:digest"));
        }
        let mut keys_file =
            open_child_file(&directory, &path, &format!("{index}.keys"), false, false)?;
        let keys = read_segment_bytes(&mut keys_file, MAX_METADATA_BYTES, read_budget)?;
        segment_read_budget(read_budget, segments.len() as u64)?;
        if segment_digest(&keys, read_budget)? != seal.keys_sha256
            || canonical_bytes(&segment_keys(&bytes, read_budget)?)? != keys
        {
            return Err(EventRepositoryError::IntegrityAt("segments:keys"));
        }
        previous = hash.clone();
        segments.push(SealedSegment {
            seal,
            seal_hash: hash,
            identity,
            bytes,
        });
    }
    Ok(segments)
}

pub(super) fn stage_rotation_metadata(
    root_handle: &File,
    root: &Path,
    bytes: &[u8],
) -> Result<(String, File), EventRepositoryError> {
    let temp = open_child_directory(root_handle, root, ".tmp")?;
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| EventRepositoryError::StorageAt {
        site: "rotation:temp-entropy",
        os: None,
    })?;
    let temp_name = format!("rotation-{}.tmp", hex::encode(nonce));
    let mut file = open_child_file(&temp, &root.join(".tmp"), &temp_name, true, true)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_directory_handle(&temp)?;
    Ok((temp_name, file))
}

pub(super) fn publish_rotation_metadata(
    root_handle: &File,
    root: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<(), EventRepositoryError> {
    let directory = open_child_directory(root_handle, root, "segments")?;
    let temp = open_child_directory(root_handle, root, ".tmp")?;
    let (temp_name, file) = stage_rotation_metadata(root_handle, root, bytes)?;
    rename_rotation_file(
        &temp,
        &root.join(".tmp"),
        &temp_name,
        &directory,
        &root.join("segments"),
        name,
        &file,
    )?;
    sync_directory_handle(&directory)
}

pub(super) fn recover_rotation(
    root_handle: &File,
    root: &Path,
    read_budget: &crate::ReadBudget,
) -> Result<(), EventRepositoryError> {
    read_segment_chain_until_pending(root_handle, root, true, read_budget)?;
    let path = root.join("segments");
    let directory = open_child_directory(root_handle, root, "segments")?;
    let inventory = segment_inventory(&directory, &path, read_budget)?;
    let Some((&index, names)) = inventory.last_key_value() else {
        return Ok(());
    };
    let root_names =
        collect_child_names_bounded(root_handle, root, MAX_SEGMENTED_ROOT_ENTRIES + 1)?;
    if names.contains("jsonl") && names.contains("keys") && root_names.contains("journal.jsonl") {
        return Ok(());
    }
    require_rotation_platform()?;
    // Earlier segments must already be complete, canonical and hash verified.
    let mut previous = GENESIS_HASH.to_owned();
    for earlier in 0..index {
        let (_, hash) = read_segment_seal(&directory, &path, earlier, &previous, read_budget)?;
        previous = hash;
    }
    let (seal, _) = read_segment_seal(&directory, &path, index, &previous, read_budget)?;
    let source_name = if names.contains("jsonl") {
        format!("{index}.jsonl")
    } else {
        "journal.jsonl".to_owned()
    };
    let (source_directory, source_path) = if names.contains("jsonl") {
        (&directory, path.as_path())
    } else {
        (root_handle, root)
    };
    let mut source = open_child_file(source_directory, source_path, &source_name, true, false)?;
    let bytes = read_segment_bytes(&mut source, seal.bytes, read_budget)?;
    if bytes.len() as u64 != seal.bytes || segment_digest(&bytes, read_budget)? != seal.sha256 {
        return Err(EventRepositoryError::IntegrityAt(
            "rotation:recovery-digest",
        ));
    }
    let keys = canonical_bytes(&segment_keys(&bytes, read_budget)?)?;
    if segment_digest(&keys, read_budget)? != seal.keys_sha256 {
        return Err(EventRepositoryError::IntegrityAt("rotation:recovery-keys"));
    }
    if !names.contains("keys") {
        publish_rotation_metadata(root_handle, root, &format!("{index}.keys"), &keys)?;
    }
    if !names.contains("jsonl") {
        source.sync_all()?;
        rename_rotation_file(
            root_handle,
            root,
            "journal.jsonl",
            &directory,
            &path,
            &format!("{index}.jsonl"),
            &source,
        )?;
    }
    let root_names =
        collect_child_names_bounded(root_handle, root, MAX_SEGMENTED_ROOT_ENTRIES + 1)?;
    if !root_names.contains("journal.jsonl") {
        open_child_file(root_handle, root, "journal.jsonl", true, true)?.sync_all()?;
    }
    sync_directory_handle(&directory)?;
    sync_directory_handle(root_handle)?;
    read_segment_chain(root_handle, root, read_budget)?;
    Ok(())
}

fn segment_read_budget(
    budget: &crate::ReadBudget,
    walked: u64,
) -> Result<(), EventRepositoryError> {
    budget
        .check_now(walked)
        .map_err(|exceeded| EventRepositoryError::ReadBudgetExceeded {
            walked: exceeded.walked,
            limit_millis: exceeded.limit_millis,
        })
}

fn segment_digest(
    bytes: &[u8],
    budget: &crate::ReadBudget,
) -> Result<String, EventRepositoryError> {
    use sha2::Digest as _;
    let mut digest = sha2::Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        segment_read_budget(budget, 0)?;
        digest.update(chunk);
    }
    Ok(format!("sha256:{}", hex::encode(digest.finalize())))
}

fn read_segment_bytes(
    file: &mut File,
    limit: u64,
    budget: &crate::ReadBudget,
) -> Result<Vec<u8>, EventRepositoryError> {
    ensure_inclusive_limit(file.metadata()?.len(), limit)?;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        segment_read_budget(budget, 0)?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure_inclusive_limit((bytes.len() + count) as u64, limit)?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rename_rotation_file(
    source_directory: &File,
    source_path: &Path,
    source_name: &str,
    target_directory: &File,
    target_path: &Path,
    target_name: &str,
    retained: &File,
) -> Result<(), EventRepositoryError> {
    require_rotation_platform()?;
    validate_child_name(source_name)?;
    validate_child_name(target_name)?;
    if file_identity(&open_child_file(
        source_directory,
        source_path,
        source_name,
        false,
        false,
    )?)? != file_identity(retained)?
    {
        return Err(EventRepositoryError::IntegrityAt(
            "rotation:source-identity",
        ));
    }
    #[cfg(windows)]
    {
        rename_retained_file(retained, target_directory, target_name)?;
    }
    if file_identity(&open_child_file(
        target_directory,
        target_path,
        target_name,
        false,
        false,
    )?)? != file_identity(retained)?
    {
        return Err(EventRepositoryError::IntegrityAt(
            "rotation:published-identity",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn rename_retained_file(
    retained: &File,
    target_directory: &File,
    target_name: &str,
) -> Result<(), EventRepositoryError> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    };
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    };
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        ReOpenFile,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    validate_child_name(target_name)?;
    // ReOpenFile grants rename rights to the already retained object, without resolving its
    // mutable name or requiring delete permission from ordinary legacy journal readers.
    let handle = unsafe {
        ReOpenFile(
            retained.as_raw_handle(),
            GENERIC_READ | GENERIC_WRITE | DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_WRITE_THROUGH,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(EventRepositoryError::StorageAt {
            site: "rotation:reopen-source",
            os: std::io::Error::last_os_error().raw_os_error(),
        });
    }
    // SAFETY: ReOpenFile returned a fresh, uniquely owned handle.
    let retained = unsafe { File::from_raw_handle(handle) };
    let name = std::ffi::OsStr::new(target_name)
        .encode_wide()
        .collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(2)
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let buffer_bytes = std::mem::size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes)
        .ok_or(EventRepositoryError::LimitExceeded)?;
    // u64 storage supplies the alignment of FILE_RENAME_INFORMATION on supported targets.
    let mut buffer = vec![0_u64; buffer_bytes.div_ceil(8)];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: the aligned zeroed allocation includes the fixed header and the complete UTF-16
    // name. The retained directory resolves that one validated child; overwrite stays false.
    unsafe {
        (*information).RootDirectory = target_directory.as_raw_handle();
        (*information).FileNameLength =
            u32::try_from(name_bytes).map_err(|_| EventRepositoryError::LimitExceeded)?;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.len(),
        );
    }
    retained
        .sync_all()
        .map_err(|error| EventRepositoryError::StorageAt {
            site: "rotation:source-flush-before",
            os: error.raw_os_error(),
        })?;
    // SAFETY: retained owns the source handle; the initialized buffer and target handle remain
    // live throughout the call. The operation never resolves the source name again.
    let mut io_status = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtSetInformationFile(
            retained.as_raw_handle(),
            &mut io_status,
            information.cast(),
            u32::try_from(buffer_bytes).map_err(|_| EventRepositoryError::LimitExceeded)?,
            FileRenameInformation,
        )
    };
    if status < 0 {
        return Err(EventRepositoryError::StorageAt {
            site: "rotation:nt-rename-status",
            os: Some(status),
        });
    }
    retained
        .sync_all()
        .map_err(|error| EventRepositoryError::StorageAt {
            site: "rotation:source-flush-after",
            os: error.raw_os_error(),
        })?;
    Ok(())
}
