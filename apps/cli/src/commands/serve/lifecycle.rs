use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Running,
    Clean,
    ServeError,
    Panicked,
}

#[derive(Clone, Deserialize, Serialize)]
struct Panic {
    location: String,
    thread: String,
    at: u64,
    fatal: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
    pid: u32,
    started_at: u64,
    version: String,
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fatal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_panic: Option<Panic>,
}

// The lifetime lock closes the read/replace race between two different listening ports.
// The hook keeps only the writer, never the ownership lock.
pub(super) struct Lifecycle {
    _lock: File,
    writer: Arc<Mutex<(PathBuf, Record)>>,
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        // A caught request-task panic does not drop the server owner. Unwinding this
        // owner does: it is the point at which a recorded panic is known to be fatal.
        if std::thread::panicking() {
            let mut writer = self
                .writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (path, record) = &mut *writer;
            if matches!(record.state, State::Panicked) {
                record.fatal = Some(true);
                let _ = write(path, record);
            }
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn invalid() -> super::Failure {
    super::serve_invalid(
        "runtime-lifecycle.json could not be read or written safely",
        "/events",
    )
}

fn regular(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            Err(std::io::ErrorKind::InvalidInput.into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// #1326: the shared atomic write with both strengths this record had (owner-only on unix, and a
/// target that exists must be a regular file); the JSON and its newline are composed here.
fn write(path: &Path, record: &Record) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    crate::commands::atomic::atomic_write_with(
        path,
        &bytes,
        crate::commands::atomic::Mode {
            private: true,
            regular_only: true,
        },
    )
}

impl Lifecycle {
    pub(super) fn start(
        events: &Path,
        instance: &str,
    ) -> Result<(Self, serde_json::Value, serde_json::Value), super::Failure> {
        let mut name = events.file_name().ok_or_else(invalid)?.to_os_string();
        name.push(".runtime-lifecycle.json");
        let path = events.with_file_name(name);
        let lock_path = path.with_extension("lock");
        regular(&lock_path).map_err(|_| invalid())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| invalid())?;
        lock.try_lock_exclusive().map_err(|_| {
            super::serve_invalid(
                "another Runtime owns this events directory (runtime-lifecycle.json)",
                "/events",
            )
        })?;
        regular(&path).map_err(|_| invalid())?;
        let note_path = path.with_extension("oom");
        regular(&note_path).map_err(|_| invalid())?;
        let previous = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| invalid())?;
                if bytes.len() > 16 * 1024 {
                    return Err(invalid());
                }
                let record: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                // #41: an allocation failure aborts without unwinding, so the record still
                // says `running`; the guard's note beside it is the only witness of the cause.
                let oom = matches!(record.state, State::Running)
                    .then(|| read_note(&note_path))
                    .flatten();
                // The acquired lifetime lock proves no previous owner remains. A live
                // recorded PID can have been reused and does not establish ownership.
                let state = match record.state {
                    State::Running if oom.is_some() => "out_of_memory",
                    State::Running => "vanished",
                    State::Clean => "clean",
                    State::ServeError => "serve_error",
                    State::Panicked if record.fatal != Some(true) => "vanished",
                    State::Panicked => "panicked",
                };
                let at = if let Some(note) = &oom {
                    note.at
                } else if state == "vanished" {
                    record.started_at
                } else {
                    record.at.unwrap_or(record.started_at)
                };
                let mut result = serde_json::json!({"state": state, "pid": record.pid,
                    "at": at});
                if let Some(note) = oom {
                    result["size"] = serde_json::Value::from(note.size);
                }
                result["instance"] = record
                    .instance
                    .map_or(serde_json::Value::Null, serde_json::Value::String);
                if let Some(location) = record.location {
                    if state == "vanished" {
                        result["lastPanic"] = serde_json::json!({"location": sanitize(&location)});
                    } else {
                        result["location"] = serde_json::Value::String(sanitize(&location));
                    }
                }
                result
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Value::Null,
            Err(_) => return Err(invalid()),
        };
        let record = Record {
            instance: Some(instance.to_owned()),
            pid: std::process::id(),
            started_at: now(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            state: State::Running,
            at: None,
            location: None,
            thread: None,
            fatal: None,
            last_panic: None,
        };
        let current_run = serde_json::json!({
            "instance": record.instance.as_deref(),
            "pid": record.pid,
            "startedAt": record.started_at,
        });
        write(&path, &record).map_err(|_| invalid())?;
        // Truncating consumes the previous run's note; the open handle is what the allocation
        // guard writes through, because opening a file on that path would itself allocate.
        let note = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&note_path)
            .map_err(|_| invalid())?;
        super::alloc_guard::arm(note);
        let writer = Arc::new(Mutex::new((path, record)));
        let hook_writer = Arc::clone(&writer);
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            {
                let mut writer = hook_writer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let (path, record) = &mut *writer;
                record.state = State::Panicked;
                record.location = Some(info.location().map_or_else(
                    || "unknown".to_owned(),
                    |location| sanitize(&format!("{}:{}", location.file(), location.line())),
                ));
                // Names are arbitrary user text; only the opaque thread id is recorded.
                record.thread = Some(format!("{:?}", std::thread::current().id()));
                record.at = Some(now());
                record.fatal = Some(false);
                let _ = write(path, record);
            }
            previous_hook(info);
        }));
        Ok((
            Self {
                _lock: lock,
                writer,
            },
            previous,
            current_run,
        ))
    }

    pub(super) fn finish(&self, clean: bool) -> Result<(), super::Failure> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (path, record) = &mut *writer;
        record.state = if clean {
            State::Clean
        } else {
            State::ServeError
        };
        record.last_panic = record.location.take().map(|location| Panic {
            location,
            thread: record.thread.take().unwrap_or_default(),
            at: record.at.unwrap_or(record.started_at),
            fatal: false,
        });
        record.fatal = None;
        record.at = Some(now());
        write(path, record).map_err(|_| invalid())
    }
}

#[derive(Deserialize)]
struct Note {
    size: u64,
    at: u64,
}

/// The guard's note, if the previous run left a well-formed one. Anything else (absent, empty,
/// torn by the abort) reads as no note, and the death stays "vanished".
fn read_note(path: &Path) -> Option<Note> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(256)
        .read_to_end(&mut bytes)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn sanitize(raw: &str) -> String {
    let normalized = raw.replace('\\', "/");
    if normalized.contains(".cargo") || normalized.contains("registry") {
        return "unknown".to_owned();
    }
    let Some((prefix, suffix)) = normalized.rsplit_once("/src/") else {
        return "unknown".to_owned();
    };
    let name = prefix.rsplit('/').next().unwrap_or_default();
    let result = format!("{name}/src/{suffix}");
    let Some((file, line)) = result.rsplit_once(':') else {
        return "unknown".to_owned();
    };
    if name.is_empty()
        || result.starts_with('/')
        || file.contains(':')
        || file
            .split('/')
            .any(|part| part == ".." || part == "." || part.is_empty())
        || !file.ends_with(".rs")
        || line.parse::<u32>().is_err()
        || result.chars().any(char::is_control)
    {
        return "unknown".to_owned();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    // axum does not expose a deterministic listener-error trigger. Exercise the actual
    // ending writer with an error outcome, not an invented HTTP failure or a live service.
    // Cost: one temporary file write; catches accidentally recording every return as clean.
    #[test]
    fn error_ending_is_persisted_as_serve_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime-lifecycle.json");
        let record = serde_json::from_value(serde_json::json!({
            "pid": 17, "startedAt": 123, "version": "test", "state": "running"
        }))
        .unwrap();
        let lifecycle = super::Lifecycle {
            _lock: std::fs::File::create(dir.path().join("runtime-lifecycle.lock")).unwrap(),
            writer: std::sync::Arc::new(std::sync::Mutex::new((path.clone(), record))),
        };
        assert!(lifecycle.finish(false).is_ok());
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(written["state"], "serve_error");
        assert_eq!(written["pid"], 17);
        assert!(written["at"].is_u64());
    }

    // #41: an allocation failure aborts past the panic hook, leaving the record `running`.
    // The allocation guard leaves its own note beside the record; the next start must name
    // the death as out of memory, with the size, instead of "vanished". Cost: two starts on
    // a temporary directory; no real allocation failure.
    #[test]
    fn allocation_failure_note_is_served_as_out_of_memory() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events.jsonl");
        let Ok((first, _, _)) = super::Lifecycle::start(&events, "first") else {
            panic!("first start failed");
        };
        drop(first);
        std::fs::write(
            dir.path().join("events.jsonl.runtime-lifecycle.oom"),
            b"{\"size\":1099511627776,\"at\":1234}\n",
        )
        .unwrap();
        let Ok((second, previous, _)) = super::Lifecycle::start(&events, "second") else {
            panic!("second start failed");
        };
        assert_eq!(previous["state"], "out_of_memory");
        assert_eq!(previous["size"], 1_099_511_627_776_u64);
        assert_eq!(previous["at"], 1234);
        assert_eq!(previous["instance"], "first");
        drop(second);
        // The note is consumed: a clean stop after it is not reported as out of memory again.
        let Ok((third, previous, _)) = super::Lifecycle::start(&events, "third") else {
            panic!("third start failed");
        };
        assert_eq!(previous["state"], "vanished");
        drop(third);
    }

    // Security boundary: panic locations exposed without auth must never carry build homes.
    // Cost: pure string cells; no process, filesystem or network.
    #[test]
    fn sanitiser_removes_build_homes_and_rejects_dependency_paths() {
        assert_eq!(
            sanitize(r"C:\Users\x\.cargo\registry\pkg\src\lib.rs:3"),
            "unknown"
        );
        assert_eq!(
            sanitize("apps/cli/src/commands/serve/mod.rs:9"),
            "cli/src/commands/serve/mod.rs:9"
        );
        assert_eq!(
            sanitize(r"C:\Users\x\project\apps\cli\src\main.rs:9"),
            "cli/src/main.rs:9"
        );
        assert_eq!(sanitize("cli/src/../../secret.rs:9"), "unknown");
        assert_eq!(sanitize("/private/secret.rs:9"), "unknown");
    }
}
