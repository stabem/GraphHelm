//! Wire-neutral types for one typed judgment call and its reply: a System One request
//! (`docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6). The JSON these serialize to IS the TypeSafe
//! `POST /v1/systemone` body and reply (docs.typesafe.ai/api), pinned by
//! `tests/judgment_wire.rs` against the documented examples. Like `call.rs`: plain serde, no
//! validation, no network, nothing here knows an adapter.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::call::Usage;

/// TypeSafe's flagship System One model id.
pub const JEV_LATEST: &str = "jev-latest";

/// What a yes and a no mean, when the question needs saying.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub r#true: String,
    #[serde(rename = "false")]
    pub r#false: String,
}

/// One closed question. The id it travels under is the caller's key in
/// [`JudgeRequest::questions`]; it is not sent to the model as meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Whether a condition holds: the reply is the probability of yes.
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// One option of a closed set; `None` when an option needs no rubric.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, Option<String>>,
    },
    /// A position on ordered levels; at least two levels.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeRequest {
    pub state: serde_json::Value,
    pub model: String,
    pub questions: BTreeMap<String, Question>,
}

/// One answer, under the id of its question. `Choice` and `Score` carry a `confidence` derived
/// from their distribution; `Noul` carries none (docs.typesafe.ai/confidence).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeReply {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(with = "usage_wire")]
    pub usage: Usage,
}

/// TypeSafe spells usage `input_tokens`/`output_tokens`; [`Usage`] is camelCase on every other
/// wire in this workspace, so the reply carries its own field names here and nowhere else.
mod usage_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::call::Usage;

    #[derive(Serialize, Deserialize)]
    struct Wire {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    }

    pub fn serialize<S: Serializer>(usage: &Usage, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Usage, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Ok(Usage {
            input_tokens: wire.input_tokens,
            output_tokens: wire.output_tokens,
        })
    }
}

/// The lowercase hex SHA-256 of the request's canonical JSON: `serde_json`'s `Map` is a
/// `BTreeMap`, so keys serialize sorted and the same request always yields the same bytes. This
/// is the key a recorded judge fixture is looked up under.
#[must_use]
pub fn request_sha256(request: &JudgeRequest) -> String {
    let bytes =
        serde_json::to_vec(request).expect("JudgeRequest is plain data and always serializes");
    hex::encode(Sha256::digest(bytes))
}
