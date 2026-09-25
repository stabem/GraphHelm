//! The stable typed subset of `schemas/graph-signal.schema.json`.
//!
//! The schema closes `source.type` but leaves `type` an open string for forward compatibility, so
//! this models the recognized kinds and preserves the raw value. A signal is always a proposal:
//! nothing here mutates a graph, and only a recognized kind may reach the Governor at all.

use serde::Deserialize;

use graphhelm_protocols::{SignalSeverity, SignalSourceKind};

/// Signal kinds this milestone recognizes, from `HARNESS_SPEC.md` §19.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalKind {
    UnexpectedDependency,
    AuthBoundaryDiscovered,
    ToolFailure,
    QuotaExhausted,
    StaleDraft,
    NoProgress,
    /// A type outside the recognized set. Recorded as evidence, never actionable.
    Unrecognized,
}

impl SignalKind {
    fn parse(value: &str) -> Self {
        match value {
            "unexpected_dependency" => Self::UnexpectedDependency,
            "auth_boundary_discovered" => Self::AuthBoundaryDiscovered,
            "tool_failure" => Self::ToolFailure,
            "quota_exhausted" => Self::QuotaExhausted,
            "stale_draft" => Self::StaleDraft,
            "no_progress" => Self::NoProgress,
            _ => Self::Unrecognized,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignalSource {
    #[serde(rename = "type")]
    kind: SignalSourceKind,
    id: String,
}

impl SignalSource {
    #[must_use]
    pub const fn kind(&self) -> SignalSourceKind {
        self.kind
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    Invalid,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawSignal {
    id: String,
    source: SignalSource,
    #[serde(rename = "type")]
    kind: String,
    severity: SignalSeverity,
    description: String,
    evidence: Vec<String>,
    #[serde(default)]
    recommendations: Vec<String>,
    emitted_at: String,
    /// The actor this signal addresses, when it addresses one. Schema 1.1.0: with several agents
    /// conversing on one execution, "who is this for" lived only as a convention inside the free
    /// text, which nothing can thread on. Optional - every pre-1.1.0 emitter sends nothing here.
    ///
    /// PRESENT means a non-empty string, exactly as the schema says (`type: string, minLength:
    /// 1`). A bare `Option<String>` admitted `null` (folded to absent) and `""` (kept), so the
    /// Runtime could seal an envelope the checked-in schema rejects - persisted input diverging
    /// from the wire contract (PR #467 review).
    #[serde(default, deserialize_with = "present_nonempty")]
    to: Option<String>,
    /// The id of the signal this one answers, when it answers one. Same 1.1.0 rationale as `to`,
    /// and the same present-means-non-empty rule.
    #[serde(default, deserialize_with = "present_nonempty")]
    reply_to: Option<String>,
}

/// A field that, WHEN PRESENT, must be a non-empty string - `null` and `""` are refused rather
/// than normalized, because the schema refuses them and a sealed envelope must stay validatable.
fn present_nonempty<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom(
            "an address field, when present, must be a non-empty string",
        ));
    }
    Ok(Some(value))
}

/// A validated signal. Construction is the only way to obtain one, so an invalid signal cannot
/// reach the Governor.
#[derive(Clone, Debug)]
pub struct TypedSignal {
    raw: RawSignal,
    kind: SignalKind,
}

impl TypedSignal {
    /// Validates the envelope against the contract the schema fixes.
    ///
    /// The checks mirror the schema exactly and are deliberately no stricter. `type` and
    /// `description` carry `minLength: 1` and `evidence` carries `minItems: 1`; `id`, `source.id`
    /// and `emittedAt` do not, so an empty one of those is recorded rather than rejected. Being
    /// stricter than the schema would fail in the wrong direction here: decision 5.4 exists to keep
    /// a signal as evidence even when it is useless, and dropping it destroys the record.
    ///
    /// # Errors
    /// Returns `SignalError::Invalid` when a required field is absent, or is empty where the schema
    /// forbids empty, or falls outside one of the closed vocabularies the schema defines.
    pub fn parse(value: &serde_json::Value) -> Result<Self, SignalError> {
        let raw: RawSignal =
            serde_json::from_value(value.clone()).map_err(|_| SignalError::Invalid)?;
        if raw.kind.is_empty() || raw.description.is_empty() || raw.evidence.is_empty() {
            return Err(SignalError::Invalid);
        }
        let kind = SignalKind::parse(&raw.kind);
        Ok(Self { raw, kind })
    }

    #[must_use]
    pub const fn kind(&self) -> SignalKind {
        self.kind
    }

    #[must_use]
    pub fn raw_kind(&self) -> &str {
        &self.raw.kind
    }

    #[must_use]
    pub const fn severity(&self) -> SignalSeverity {
        self.raw.severity
    }

    #[must_use]
    pub const fn source(&self) -> &SignalSource {
        &self.raw.source
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.raw.id
    }

    /// When the emitter says it produced this signal, verbatim from the wire.
    ///
    /// This is the emitter's claim, not a reading of any clock, and nothing in this crate decides
    /// anything from it. It is carried so the evidence stays complete.
    #[must_use]
    pub fn emitted_at(&self) -> &str {
        &self.raw.emitted_at
    }

    /// Owner-facing suggestions carried with the signal. Never authoritative and empty by default.
    #[must_use]
    pub fn recommendations(&self) -> &[String] {
        &self.raw.recommendations
    }

    /// The actor this signal addresses, when it addresses one. Addressing is a reading aid for
    /// whoever threads the conversation - nothing routes on it, and nothing here enforces that the
    /// named actor exists.
    #[must_use]
    pub fn to(&self) -> Option<&str> {
        self.raw.to.as_deref()
    }

    /// The id of the signal this one answers, when it answers one. Same trust posture as `to`.
    #[must_use]
    pub fn reply_to(&self) -> Option<&str> {
        self.raw.reply_to.as_deref()
    }

    /// Whether this signal may be turned into a draft by the Governor.
    ///
    /// An unrecognized kind never can. This is the fail-closed boundary: the record is kept as
    /// evidence, but an agent cannot invent a signal type and thereby cause a mutation.
    #[must_use]
    pub const fn can_propose_mutation(&self) -> bool {
        !matches!(self.kind, SignalKind::Unrecognized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "signal-1",
            "source": {"type": "node", "id": "node-a"},
            "type": kind,
            "severity": "high",
            "description": "a dependency was discovered",
            "evidence": ["exec-1"],
            "emittedAt": "2026-08-13T00:00:00Z"
        })
    }

    #[test]
    fn a_recognized_signal_parses_into_the_typed_subset() {
        let signal = TypedSignal::parse(&envelope("unexpected_dependency")).unwrap();
        assert_eq!(signal.kind(), SignalKind::UnexpectedDependency);
        assert_eq!(signal.severity(), SignalSeverity::High);
        assert_eq!(signal.source().kind(), SignalSourceKind::Node);
        assert!(signal.can_propose_mutation());
    }

    /// An unrecognized type is kept as evidence, because the emitting agent is not authoritative
    /// and discarding the record would lose information. It can never produce a mutation.
    #[test]
    fn an_unrecognized_signal_is_recorded_but_can_never_propose_a_mutation() {
        let signal = TypedSignal::parse(&envelope("invented_by_an_agent")).unwrap();
        assert_eq!(signal.kind(), SignalKind::Unrecognized);
        assert_eq!(signal.raw_kind(), "invented_by_an_agent");
        assert!(!signal.can_propose_mutation());
    }

    /// The schema puts `minLength` on `type` and `description` only. A signal that is useless but
    /// schema-valid must still be recorded, because decision 5.4 keeps the evidence rather than
    /// letting a non-authoritative emitter cause a silent drop.
    #[test]
    fn a_schema_valid_signal_is_never_dropped_for_being_useless() {
        let mut sparse = envelope("unexpected_dependency");
        sparse["id"] = serde_json::json!("");
        sparse["source"]["id"] = serde_json::json!("");
        sparse["evidence"] = serde_json::json!([""]);
        sparse["emittedAt"] = serde_json::json!("");

        let signal = TypedSignal::parse(&sparse).expect("schema-valid signal must be recorded");
        assert_eq!(signal.id(), "");
        assert!(signal.can_propose_mutation());
    }

    /// A conversation needs an address. With several agents on one execution, "who is this for"
    /// and "which message does it answer" lived only as conventions inside the free text, which no
    /// tool can thread on. `to` names the addressed actor, `replyTo` names the answered signal id;
    /// both OPTIONAL, because every existing emitter sends neither and must stay valid.
    #[test]
    fn a_reply_carries_its_addressee_and_the_message_it_answers() {
        let mut reply = envelope("operator_note");
        reply["to"] = serde_json::json!("codex");
        reply["replyTo"] = serde_json::json!("signal-0");

        let signal = TypedSignal::parse(&reply).expect("an addressed reply is schema-valid");
        assert_eq!(signal.to(), Some("codex"));
        assert_eq!(signal.reply_to(), Some("signal-0"));

        // Absent stays absent - not empty-string, not defaulted.
        let plain = TypedSignal::parse(&envelope("operator_note")).unwrap();
        assert_eq!(plain.to(), None);
        assert_eq!(plain.reply_to(), None);
    }

    /// The new fields must not have loosened the envelope: a field NOBODY defined is still
    /// refused. Without this, "we added two optional fields" and "we opened the envelope to
    /// anything" would look identical in every other test.
    #[test]
    fn an_undefined_field_is_still_rejected_after_the_addressing_fields() {
        let mut smuggled = envelope("operator_note");
        smuggled["forwardTo"] = serde_json::json!("someone");
        assert_eq!(
            TypedSignal::parse(&smuggled).unwrap_err(),
            SignalError::Invalid
        );
    }

    #[test]
    fn a_signal_violating_the_schema_contract_is_rejected() {
        let mut missing_evidence = envelope("unexpected_dependency");
        missing_evidence["evidence"] = serde_json::json!([]);
        assert_eq!(
            TypedSignal::parse(&missing_evidence).unwrap_err(),
            SignalError::Invalid
        );

        let mut unknown_source = envelope("unexpected_dependency");
        unknown_source["source"]["type"] = serde_json::json!("oracle");
        assert_eq!(
            TypedSignal::parse(&unknown_source).unwrap_err(),
            SignalError::Invalid
        );
    }
}
