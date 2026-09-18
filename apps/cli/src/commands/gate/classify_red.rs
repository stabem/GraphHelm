//! `graphhelm gate classify-red` — SHADOW classification of a RED gate run (#1138, edge 1).
//!
//! The command is thin on purpose: the excerpt, the request and the reading live in
//! `core/architect/src/judgment/red.rs`, and this module only reads the log (UTF-8 or UTF-16,
//! BOM-detected: the runner's transcripts are UTF-16), reads the known-flake list, opens the
//! judge door the way `graph synthesize` does (`architect::build_judge`, so there is no new
//! credential path), prints the one JSON, and writes it to `--out` without ever overwriting.
//!
//! SHADOW MODE, stated once here and enforced by what the command cannot do: it has no handle on
//! a manifest's `runClass`, no queue, no verdict. A classification of any kind — `real_defect`,
//! `unresolved`, low confidence — exits 0. The command exits non-zero only for its OWN
//! failures: a log it cannot read, a flakes file it cannot parse, an `--out` it may not write,
//! a judge that cannot answer. A recording that holds no reply names the request digest on
//! stderr as well as in the diagnostic, so the reply can be authored under it.

use std::io::Write;
use std::path::Path;

use graphhelm_architect::ArchitectRefusal;
use graphhelm_architect::judgment::red::{self, KnownFlake};
use serde_json::Value;

use super::super::architect::{self, Failure, JudgeSource};
use crate::args::ClassifyRedArgs;
use crate::output::Outcome;

const COMMAND: &str = "gate.classify-red";
const ARGUMENT_CODE: &str = crate::error_codes::GHCLI001_ARGUMENT_INVALID;
/// A gate transcript is a few hundred KiB; a log past this is not one this command reads.
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
/// The known-flake list is a handful of rows.
const MAX_FLAKES_BYTES: u64 = 1024 * 1024;

fn argument(message: &str, pointer: &str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(crate) fn run(arguments: &ClassifyRedArgs) -> Outcome {
    match run_inner(arguments) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(failure) => failure.into_outcome(COMMAND),
    }
}

fn run_inner(arguments: &ClassifyRedArgs) -> Result<Value, Failure> {
    if let Some(out) = arguments.out.as_deref() {
        check_out_path(out)?;
    }
    let log = read_log(&arguments.log)?;
    let known = read_known_flakes(&arguments.known_flakes)?;
    let judge = architect::build_judge(&judge_source(arguments)?)?;

    let excerpt = red::excerpt(&log);
    let request = red::request(&excerpt, &known);
    let reply = judge.judge(&request).map_err(|refusal| {
        if let ArchitectRefusal::JudgeMissing { request_sha256 } = &refusal {
            eprintln!(
                "gate classify-red: the recording holds no reply for request {request_sha256}"
            );
        }
        architect::refused(&refusal)
    })?;
    let classification = red::read(&reply, &excerpt, &known);

    let mut value = serde_json::to_value(&classification)
        .map_err(|_| argument("the classification could not be serialized", "/log"))?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "excerptDigest".to_owned(),
            Value::String(red::excerpt_sha256(&excerpt)),
        );
        object.insert(
            "judgeUsage".to_owned(),
            serde_json::to_value(reply.usage).unwrap_or(Value::Null),
        );
    }
    if let Some(out) = arguments.out.as_deref() {
        write_record(out, &value)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "out".to_owned(),
                Value::String(out.to_string_lossy().into_owned()),
            );
        }
    }
    Ok(value)
}

/// Exactly one judge door: `--judge-fixture`, or `--manifest` with `--judge-route`. clap
/// already refuses the pair and a route without a manifest; this reads what survived.
fn judge_source(arguments: &ClassifyRedArgs) -> Result<JudgeSource<'_>, Failure> {
    match (
        arguments.judge_fixture.as_deref(),
        arguments.judge_route.as_deref(),
        arguments.manifest.as_deref(),
    ) {
        (Some(fixture), None, _) => Ok(JudgeSource::Fixture(fixture)),
        (None, Some(route), Some(manifest)) => Ok(JudgeSource::Gateway {
            manifest,
            route,
            broker: arguments.broker.as_deref(),
            keyring: arguments.keyring.as_deref(),
            key_id: arguments.key_id.as_deref(),
        }),
        _ => Err(argument(
            "one judge door is required: --judge-fixture, or --manifest with --judge-route",
            "/judgeFixture",
        )),
    }
}

/// The log's bytes as text. A UTF-16 BOM (either order) decodes as UTF-16; a UTF-8 BOM is
/// dropped; anything else is read as UTF-8, invalid sequences replaced, because a transcript
/// with one damaged byte is still a log worth classifying. Bounded by [`MAX_LOG_BYTES`].
fn read_log(path: &Path) -> Result<String, Failure> {
    let unreadable = |message: &str| argument(&format!("--log {message}"), "/log");
    let metadata = std::fs::metadata(path)
        .map_err(|error| unreadable(&format!("cannot be inspected: {}", error.kind())))?;
    if !metadata.is_file() {
        return Err(unreadable("is not a regular file"));
    }
    if metadata.len() > MAX_LOG_BYTES {
        return Err(unreadable("exceeds the 16 MiB limit"));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| unreadable(&format!("cannot be read: {}", error.kind())))?;
    decode(&bytes).map_err(unreadable)
}

fn decode(bytes: &[u8]) -> Result<String, &'static str> {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Ok(String::from_utf8_lossy(rest).into_owned()),
        _ => Ok(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, &'static str> {
    if !bytes.len().is_multiple_of(2) {
        return Err("has a UTF-16 BOM but an odd number of bytes");
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| unit([pair[0], pair[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

/// The known-flake list: a JSON array of `{issue, test, summary}`, bounded and shaped.
fn read_known_flakes(path: &Path) -> Result<Vec<KnownFlake>, Failure> {
    let invalid = |message: &str| argument(&format!("--known-flakes {message}"), "/knownFlakes");
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(&format!("cannot be inspected: {}", error.kind())))?;
    if !metadata.is_file() {
        return Err(invalid("is not a regular file"));
    }
    if metadata.len() > MAX_FLAKES_BYTES {
        return Err(invalid("exceeds the 1 MiB limit"));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| invalid(&format!("cannot be read: {}", error.kind())))?;
    let known: Vec<KnownFlake> = serde_json::from_slice(&bytes).map_err(|_| {
        invalid("must be a JSON array of {\"issue\", \"test\", \"summary\"} objects")
    })?;
    if known.iter().any(|flake| flake.test.trim().is_empty()) {
        return Err(invalid("names a flake with an empty test"));
    }
    Ok(known)
}

/// `--out` must be a `.json` path that does not exist yet. Checked before any judge is asked,
/// so a refused write costs no call; the write itself is `create_new`, so a file that appears
/// between this check and the write is still never overwritten.
fn check_out_path(out: &Path) -> Result<(), Failure> {
    if out.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(argument("--out must end in .json", "/out"));
    }
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(argument(
            "--out already exists; a classification record is never overwritten",
            "/out",
        ));
    }
    Ok(())
}

fn write_record(out: &Path, record: &Value) -> Result<(), Failure> {
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|_| argument("the record could not be serialized", "/out"))?;
    bytes.push(b'\n');
    let unwritable = |error: &std::io::Error| {
        argument(
            &format!("--out could not be written: {}", error.kind()),
            "/out",
        )
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|error| unwritable(&error))?;
    file.write_all(&bytes).map_err(|error| unwritable(&error))?;
    file.sync_all().map_err(|error| unwritable(&error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bom_decides_the_decoding_and_is_never_part_of_the_text() {
        let text = "[gate] RED - failed stages: workspace tests\n";
        let mut little = vec![0xFF, 0xFE];
        little.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&little).unwrap(), text);
        let mut big = vec![0xFE, 0xFF];
        big.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode(&big).unwrap(), text);
        let mut utf8 = vec![0xEF, 0xBB, 0xBF];
        utf8.extend_from_slice(text.as_bytes());
        assert_eq!(decode(&utf8).unwrap(), text);
        assert_eq!(decode(text.as_bytes()).unwrap(), text);
        assert!(
            decode(&[0xFF, 0xFE, 0x5B]).is_err(),
            "an odd UTF-16 body is refused"
        );
    }

    #[test]
    fn out_must_be_a_fresh_json_path() {
        let directory = tempfile::tempdir().unwrap();
        assert!(check_out_path(&directory.path().join("record.txt")).is_err());
        let taken = directory.path().join("taken.json");
        std::fs::write(&taken, b"x").unwrap();
        assert!(check_out_path(&taken).is_err());
        assert_eq!(std::fs::read(&taken).unwrap(), b"x");
        assert!(check_out_path(&directory.path().join("fresh.json")).is_ok());
    }

    #[test]
    fn the_known_flakes_file_is_shaped() {
        let directory = tempfile::tempdir().unwrap();
        let good = directory.path().join("good.json");
        std::fs::write(
            &good,
            br#"[{"issue": 886, "test": "eof_arriving_after_the_deadline_is_not_silently_accepted", "summary": "20 ms deadline race in api_http.rs"}]"#,
        )
        .unwrap();
        assert_eq!(read_known_flakes(&good).ok().unwrap()[0].issue, 886);
        let bad = directory.path().join("bad.json");
        std::fs::write(&bad, br#"{"issue": 886}"#).unwrap();
        assert_eq!(
            read_known_flakes(&bad).err().unwrap().pointer,
            "/knownFlakes"
        );
        let empty_test = directory.path().join("empty.json");
        std::fs::write(
            &empty_test,
            br#"[{"issue": 1, "test": " ", "summary": ""}]"#,
        )
        .unwrap();
        assert!(read_known_flakes(&empty_test).is_err());
    }
}
