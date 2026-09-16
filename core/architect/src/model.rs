//! The architect's model port (spec D7) and its provider-less door.
//!
//! `DraftModel` is the one seam through which a model reaches the compiler: a prompt in, text
//! out. `RecordedDraftModel` answers from a JSON file keyed by the sha256 of the FULL assembled
//! prompt, so a recording can only ever answer the exact question it was recorded for; a
//! different prompt (a different goal, a different allowlist, a different template) is a named
//! refusal carrying the hash an operator would have to record under.
//!
//! No clock, no network, no credentials: the gateway-backed doors live in `apps/cli`.

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_gateway::call::Usage;

use crate::refusal::ArchitectRefusal;
use crate::template::prompt_sha256;

/// The largest recorded-replies file accepted, matching the graph document bound.
pub const MAX_FIXTURE_BYTES: usize = 4 * 1024 * 1024;

/// One model reply: the text the compiler parses, and whatever usage the door reported (`None`
/// for a recording: a figure nobody measured is never invented).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftReply {
    pub text: String,
    pub usage: Option<Usage>,
}

/// A model the compiler may ask for one draft.
pub trait DraftModel {
    /// Answers `prompt` with one draft, or refuses with a reason the operator can act on.
    ///
    /// # Errors
    /// [`ArchitectRefusal::ModelUnavailable`] when the door cannot answer;
    /// [`ArchitectRefusal::FixtureMissing`] when a recording holds no reply for this prompt.
    fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal>;
}

/// The recorded door: `{"replies": {"<prompt sha256>": "<reply text>", ...}}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedDraftModel {
    replies: BTreeMap<String, String>,
}

impl RecordedDraftModel {
    /// A recording holding exactly one reply.
    #[must_use]
    pub fn single(prompt_sha256: &str, text: &str) -> Self {
        Self {
            replies: BTreeMap::from([(prompt_sha256.to_owned(), text.to_owned())]),
        }
    }

    /// Parses the documented shape from bytes already in memory.
    ///
    /// # Errors
    /// [`ArchitectRefusal::ModelUnavailable`] when the bytes exceed [`MAX_FIXTURE_BYTES`], are not
    /// a JSON object with a `replies` object, or a key is not a lowercase hex sha256 or a value
    /// is not a string. A key that could never match a prompt hash is refused rather than kept,
    /// so a mistyped recording fails when it is loaded, not silently when it is asked.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ArchitectRefusal> {
        let refuse = |message: &str| ArchitectRefusal::ModelUnavailable {
            message: format!("recorded replies: {message}"),
        };
        if bytes.len() > MAX_FIXTURE_BYTES {
            return Err(refuse("the file exceeds the 4 MiB limit"));
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| refuse("the file is not JSON"))?;
        let replies = value
            .as_object()
            .and_then(|object| object.get("replies"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| refuse("expected an object with a \"replies\" object"))?;
        let mut parsed = BTreeMap::new();
        for (key, text) in replies {
            if !is_sha256_hex(key) {
                return Err(refuse(
                    "a key of \"replies\" must be the lowercase hex sha256 of the prompt",
                ));
            }
            let text = text
                .as_str()
                .ok_or_else(|| refuse("a value of \"replies\" must be a string"))?;
            parsed.insert(key.clone(), text.to_owned());
        }
        Ok(Self { replies: parsed })
    }

    /// Reads and parses a recording from disk.
    ///
    /// # Errors
    /// [`ArchitectRefusal::ModelUnavailable`] when the file cannot be read (the message names the
    /// failure class, never the path: a refusal travels to callers who cannot see this disk) or
    /// [`from_json`](Self::from_json) refuses it.
    pub fn from_file(path: &Path) -> Result<Self, ArchitectRefusal> {
        let unavailable = |message: String| ArchitectRefusal::ModelUnavailable {
            message: format!("recorded replies: {message}"),
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

    /// The prompt hashes this recording can answer, sorted.
    #[must_use]
    pub fn recorded_prompts(&self) -> Vec<&str> {
        self.replies.keys().map(String::as_str).collect()
    }
}

impl DraftModel for RecordedDraftModel {
    fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal> {
        let key = prompt_sha256(prompt);
        match self.replies.get(&key) {
            Some(text) => Ok(DraftReply {
                text: text.clone(),
                usage: None,
            }),
            None => Err(ArchitectRefusal::FixtureMissing { prompt_sha256: key }),
        }
    }
}

pub(crate) fn is_sha256_hex(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
