//! The judge port: the compiler's SECOND model door (spec D1). A `JudgeModel` answers closed
//! questions over state the compiler hands it and never drafts. `RecordedJudgeModel` is the
//! keyless door every test uses, keyed by `request_sha256` exactly as `RecordedDraftModel` is
//! keyed by the prompt hash.

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_gateway::judgment::{JudgeReply, JudgeRequest, request_sha256};

use crate::model::MAX_FIXTURE_BYTES;
use crate::refusal::ArchitectRefusal;

/// A model the compiler may ask closed questions.
pub trait JudgeModel {
    /// Answers every question in `request`, or refuses with a reason the operator can act on.
    ///
    /// # Errors
    /// [`ArchitectRefusal::JudgeUnavailable`] when the door cannot answer;
    /// [`ArchitectRefusal::JudgeMissing`] when a recording holds no reply for this request.
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal>;
}

/// The recorded door: `{"answers": {"<request sha256>": <judge reply>, ...}}`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedJudgeModel {
    answers: BTreeMap<String, JudgeReply>,
}

impl RecordedJudgeModel {
    /// A recording holding exactly one reply.
    #[must_use]
    pub fn single(request_sha256: &str, reply: &JudgeReply) -> Self {
        Self {
            answers: BTreeMap::from([(request_sha256.to_owned(), reply.clone())]),
        }
    }

    /// Parses the documented shape from bytes already in memory.
    ///
    /// # Errors
    /// [`ArchitectRefusal::JudgeUnavailable`] when the bytes exceed [`MAX_FIXTURE_BYTES`], are
    /// not a JSON object with an `answers` object, or a key is not a lowercase hex sha256 or a
    /// value is not a judge reply.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ArchitectRefusal> {
        let refuse = |message: &str| ArchitectRefusal::JudgeUnavailable {
            message: format!("recorded judge: {message}"),
        };
        if bytes.len() > MAX_FIXTURE_BYTES {
            return Err(refuse("the file exceeds the 4 MiB limit"));
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| refuse("the file is not JSON"))?;
        let answers = value
            .as_object()
            .and_then(|object| object.get("answers"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| refuse("expected an object with an \"answers\" object"))?;
        let mut parsed = BTreeMap::new();
        for (key, reply) in answers {
            if !crate::model::is_sha256_hex(key) {
                return Err(refuse(
                    "a key of \"answers\" must be the lowercase hex sha256 of the request",
                ));
            }
            let reply: JudgeReply = serde_json::from_value(reply.clone())
                .map_err(|_| refuse("a value of \"answers\" must be a judge reply"))?;
            parsed.insert(key.clone(), reply);
        }
        Ok(Self { answers: parsed })
    }

    /// Reads and parses a recording from disk.
    ///
    /// # Errors
    /// As [`Self::from_json`], plus a path that cannot be inspected or read, or is not a regular
    /// file (the message names the failure class, never the path).
    pub fn from_file(path: &Path) -> Result<Self, ArchitectRefusal> {
        let unavailable = |message: String| ArchitectRefusal::JudgeUnavailable {
            message: format!("recorded judge: {message}"),
        };
        let metadata = std::fs::metadata(path)
            .map_err(|error| unavailable(format!("cannot inspect the file: {}", error.kind())))?;
        if !metadata.is_file() {
            return Err(unavailable("not a regular file".to_owned()));
        }
        if metadata.len() > MAX_FIXTURE_BYTES as u64 {
            return Err(unavailable("the file exceeds the 4 MiB limit".to_owned()));
        }
        let bytes = std::fs::read(path)
            .map_err(|error| unavailable(format!("cannot read the file: {}", error.kind())))?;
        Self::from_json(&bytes)
    }

    /// The request hashes this recording can answer, sorted.
    #[must_use]
    pub fn recorded_requests(&self) -> Vec<&str> {
        self.answers.keys().map(String::as_str).collect()
    }
}

impl JudgeModel for RecordedJudgeModel {
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal> {
        let key = request_sha256(request);
        self.answers
            .get(&key)
            .cloned()
            .ok_or(ArchitectRefusal::JudgeMissing {
                request_sha256: key,
            })
    }
}
