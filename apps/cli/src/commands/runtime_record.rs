//! Runtime discovery records: resolve a project-bound MCP session to its live loopback Runtime.
//!
//! `serve` writes a port record for legacy `--url --discover` and a project record under
//! `<registry>/projects/<project-id>/<instance>.json` for port-independent discovery. The record
//! names the token FILE, never the token. Project discovery verifies both the random `instance`
//! and the project identity reported by `/health` before reading that file.
//!
//! Rules:
//!
//! - **A record is believed only when the server on its port reports the same instance.** `serve`
//!   has no shutdown path (it runs until killed), so records are never removed on exit; a record
//!   left by a dead Runtime is simply not confirmed by whatever holds the port now, and is refused.
//!   A pid check would add a platform dependency and still be fooled by pid reuse; the instance
//!   echo is cross-platform and names the exact process that wrote the record.
//! - **No secret in the record.** The token file keeps its own rules (`secret_file`): not a
//!   symlink, a regular file, owner-only on Unix, 64 hex characters. The record itself is written
//!   owner-only on Unix anyway, since it names paths under the user's home.
//! - **Written atomically**: a private temp file in the same directory, then a rename over the old
//!   record, so a reader sees either the old record or the new one, never a torn one. Two Runtimes
//!   cannot race for one port's record, because only one of them can hold the port.
//! - **Symlinks are refused** for the registry directory and the record, on write and on read.
//! - Project records include the actual bound URL, so an OS-assigned loopback port works.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Overrides the registry directory. Tests use it; an operator may too.
pub(crate) const DIR_ENV: &str = "GRAPHHELM_RUNTIME_DIR";
const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_PROJECT_RECORDS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
    pub(crate) version: u32,
    pub(crate) url: String,
    pub(crate) token_file: PathBuf,
    pub(crate) events: PathBuf,
    pub(crate) pid: u32,
    /// Seconds since the Unix epoch.
    pub(crate) started_at: u64,
    pub(crate) instance: String,
    /// Canonical path-derived identity. Absent only on pre-discovery/legacy records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) project_id: Option<String>,
}

impl Record {
    pub(crate) fn new(
        url: String,
        token_file: PathBuf,
        events: PathBuf,
        instance: String,
        project_id: Option<String>,
    ) -> Self {
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self {
            version: VERSION,
            url,
            token_file,
            events,
            pid: std::process::id(),
            started_at,
            instance,
            project_id,
        }
    }
}

/// The registry directory, or `None` when neither the override nor a home directory exists.
pub(crate) fn registry_dir() -> Option<PathBuf> {
    match std::env::var_os(DIR_ENV) {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => std::env::home_dir().map(|home| home.join(".graphhelm").join("runtime")),
    }
}

pub(crate) fn record_path(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("{port}.json"))
}

fn project_dir(dir: &Path, project_id: &str) -> PathBuf {
    dir.join("projects").join(project_id)
}

fn project_record_path(dir: &Path, project_id: &str, instance: &str) -> PathBuf {
    project_dir(dir, project_id).join(format!("{instance}.json"))
}

/// A fresh random instance id: 16 bytes of OS randomness, hex.
pub(crate) fn new_instance() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "the OS random source is unavailable".to_owned())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn refuse_symlink(path: &Path, what: &str) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("the {what} must not be a symbolic link"))
        }
        _ => Ok(()),
    }
}

/// Writes the compatibility record as `<dir>/<port>.json`, atomically and owner-only.
pub(crate) fn publish(dir: &Path, port: u16, record: &Record) -> Result<PathBuf, String> {
    refuse_symlink(dir, "runtime registry directory")?;
    std::fs::create_dir_all(dir)
        .map_err(|_| "the runtime registry directory could not be created".to_owned())?;
    refuse_symlink(dir, "runtime registry directory")?;
    let target = record_path(dir, port);
    refuse_symlink(&target, "runtime record")?;
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| "the runtime record could not be serialized".to_owned())?;
    bytes.push(b'\n');
    let temp = dir.join(format!(".{port}.json.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let written = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err("the runtime record could not be written".to_owned());
    }
    Ok(target)
}

/// Writes a project record under the path-derived identity, independent of the chosen port.
pub(crate) fn publish_project(dir: &Path, record: &Record) -> Result<PathBuf, String> {
    let project_id = record
        .project_id
        .as_deref()
        .ok_or_else(|| "the Runtime has no project identity".to_owned())?;
    if project_id.len() != 64 || !project_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the project identity is malformed".to_owned());
    }
    let projects = dir.join("projects");
    refuse_symlink(dir, "runtime registry directory")?;
    std::fs::create_dir_all(&projects)
        .map_err(|_| "the project runtime registry could not be created".to_owned())?;
    refuse_symlink(&projects, "project runtime registry directory")?;
    let target_dir = project_dir(dir, project_id);
    std::fs::create_dir_all(&target_dir)
        .map_err(|_| "the project runtime record directory could not be created".to_owned())?;
    refuse_symlink(&target_dir, "project runtime record directory")?;
    let target = project_record_path(dir, project_id, &record.instance);
    refuse_symlink(&target, "project runtime record")?;
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| "the project runtime record could not be serialized".to_owned())?;
    bytes.push(b'\n');
    let temp = target_dir.join(format!(".{}.tmp", record.instance));
    let _ = std::fs::remove_file(&temp);
    let written = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err("the project runtime record could not be written".to_owned());
    }
    Ok(target)
}

/// Reads bounded records for exactly one project. Liveness and uniqueness are checked by the
/// caller; old or malformed files never broaden the selected project.
pub(crate) fn read_project(dir: &Path, project_id: &str) -> Result<Vec<Record>, String> {
    if project_id.len() != 64 || !project_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the project identity is malformed".to_owned());
    }
    refuse_symlink(dir, "runtime registry directory")?;
    let projects = dir.join("projects");
    refuse_symlink(&projects, "project runtime registry directory")?;
    let path = project_dir(dir, project_id);
    refuse_symlink(&path, "project runtime record directory")?;
    let entries = std::fs::read_dir(&path).map_err(|_| {
        "no Runtime has registered this project; start `graphhelm serve` for this project"
            .to_owned()
    })?;
    let mut records = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|_| "the project runtime records could not be listed".to_owned())?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if records.len() >= MAX_PROJECT_RECORDS {
            return Err("too many Runtime records are registered for this project".to_owned());
        }
        let record = read_project_record(&entry.path())?;
        if record.project_id.as_deref() != Some(project_id) {
            return Err("a project Runtime record names a different project".to_owned());
        }
        records.push(record);
    }
    if records.is_empty() {
        return Err(
            "no Runtime has registered this project; start `graphhelm serve` for this project"
                .to_owned(),
        );
    }
    Ok(records)
}

fn read_project_record(path: &Path) -> Result<Record, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "a project Runtime record could not be read".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("a project Runtime record must not be a symbolic link".to_owned());
    }
    if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
        return Err("a project Runtime record is not a small regular file".to_owned());
    }
    reject_insecure_permissions(&metadata)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| "a project Runtime record could not be read".to_owned())?;
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|_| "a project Runtime record is invalid".to_owned())?;
    if record.version != VERSION
        || record.instance.len() != 32
        || !record.instance.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(
            "a project Runtime record has an unsupported version or malformed instance".to_owned(),
        );
    }
    Ok(record)
}

/// Reads and validates `<dir>/<port>.json`. It says nothing about liveness: the caller confirms
/// the instance against the server before believing it.
pub(crate) fn read(dir: &Path, port: u16) -> Result<Record, String> {
    let path = record_path(dir, port);
    refuse_symlink(dir, "runtime registry directory")?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        format!(
            "no Runtime has published a discovery record for port {port}; start `graphhelm serve \
             --bind 127.0.0.1:{port}` for the project you want (a Runtime started by an older \
             build publishes none: restart it)"
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err("the runtime record must not be a symbolic link".to_owned());
    }
    if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
        return Err("the runtime record is not a small regular file".to_owned());
    }
    reject_insecure_permissions(&metadata)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| "the runtime record could not be read".to_owned())?;
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|_| "the runtime record is not a valid record".to_owned())?;
    if record.version != VERSION {
        return Err("the runtime record has an unsupported version".to_owned());
    }
    if record.instance.len() != 32 || !record.instance.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the runtime record has a malformed instance id".to_owned());
    }
    Ok(record)
}

#[cfg(unix)]
fn reject_insecure_permissions(metadata: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if metadata.mode() & 0o077 != 0 {
        return Err("the runtime record must not be group or world accessible".to_owned());
    }
    // SAFETY: geteuid(2) cannot fail and touches no memory (same call as `gateway`).
    let effective_uid = unsafe { libc::geteuid() };
    if metadata.uid() != effective_uid {
        return Err("the runtime record is not owned by this user".to_owned());
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_insecure_permissions(_: &std::fs::Metadata) -> Result<(), String> {
    // Windows ACLs are not evaluated, the same as `secret_file`; the record lives under the
    // user's profile, whose default ACL is owner-only, and holds no secret.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(instance: &str) -> Record {
        Record::new(
            "http://127.0.0.1:8791".into(),
            PathBuf::from("/p/.graphhelm/events.token"),
            PathBuf::from("/p/.graphhelm/events"),
            instance.into(),
            None,
        )
    }

    #[test]
    fn a_published_record_reads_back_and_a_second_publish_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let registry = dir.path().join("runtime");
        let first = record(&"a".repeat(32));
        publish(&registry, 8791, &first).unwrap();
        assert_eq!(read(&registry, 8791).unwrap(), first);
        let second = record(&"b".repeat(32));
        publish(&registry, 8791, &second).unwrap();
        assert_eq!(read(&registry, 8791).unwrap(), second);
        let names: Vec<_> = std::fs::read_dir(&registry)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("8791.json")]);
    }

    #[test]
    fn an_absent_record_names_the_port_and_the_remedy() {
        let dir = tempfile::tempdir().unwrap();
        let error = read(dir.path(), 8791).unwrap_err();
        assert!(
            error.contains("8791") && error.contains("graphhelm serve"),
            "{error}"
        );
    }

    #[test]
    fn a_malformed_or_unknown_record_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let bad_version = Record {
            version: 9,
            ..record(&"a".repeat(32))
        };
        std::fs::write(record_path(dir.path(), 1), b"{").unwrap();
        std::fs::write(
            record_path(dir.path(), 2),
            serde_json::to_vec(&bad_version).unwrap(),
        )
        .unwrap();
        std::fs::write(
            record_path(dir.path(), 3),
            serde_json::to_vec(&record("short")).unwrap(),
        )
        .unwrap();
        for port in 1..=3 {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    record_path(dir.path(), port),
                    std::fs::Permissions::from_mode(0o600),
                )
                .unwrap();
            }
            assert!(read(dir.path(), port).is_err(), "port {port}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_or_shared_record_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere.json");
        std::fs::write(&real, serde_json::to_vec(&record(&"a".repeat(32))).unwrap()).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&real, record_path(dir.path(), 1)).unwrap();
        assert!(read(dir.path(), 1).unwrap_err().contains("symbolic link"));
        assert!(publish(dir.path(), 1, &record(&"a".repeat(32))).is_err());
        std::fs::copy(&real, record_path(dir.path(), 2)).unwrap();
        std::fs::set_permissions(
            record_path(dir.path(), 2),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(read(dir.path(), 2).unwrap_err().contains("group or world"));
    }
}
