//! Agent-reported node deliveries, stored only as sealed signal evidence.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::{Failure, argument, finish, idempotency_key, owner_actor, signal};
use crate::output::Outcome;
use graphhelm_runtime::context::secret_shaped;

pub(crate) const MAX_DELIVERY_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryRecord {
    pub(crate) version: u8,
    pub(crate) project_id: String,
    pub(crate) summary: String,
    pub(crate) reason: String,
    pub(crate) documents: Vec<DeliveryDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) work: Option<DeliveryWork>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryWork {
    pub(crate) version: u8,
    pub(crate) session_id: String,
    pub(crate) stage: String,
    pub(crate) revision: String,
    pub(crate) skills: Vec<DeliverySkill>,
    pub(crate) checks: Vec<DeliveryCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) journey_verification: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliverySkill {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) digest: String,
    pub(crate) status: DeliveryStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryCheck {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) observer: String,
    pub(crate) outcome: DeliveryOutcome,
    pub(crate) attempt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evidence: Option<DeliveryEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeliveryStatus {
    Requested,
    Reported,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeliveryOutcome {
    Passed,
    Failed,
    Skipped,
    Unobserved,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryEvidence {
    pub(crate) evidence_id: String,
    pub(crate) content_hash: String,
    pub(crate) size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeliveryDocument {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) kind: DocumentKind,
    pub(crate) action: DocumentAction,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) journey_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) rule_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentKind {
    File,
    BusinessRule,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentAction {
    Created,
    Updated,
    Reviewed,
}

fn invalid() -> Failure {
    argument(
        "the delivery must be a bounded version 1 record with relative document paths",
        "/delivery",
    )
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.chars().any(char::is_control)
        && !secret_shaped(value)
}

fn digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn revision(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn validate_journey(value: &serde_json::Value, work_revision: &str) -> bool {
    static SCHEMAS: OnceLock<Option<graphhelm_schema::OfflineSchemaSet>> = OnceLock::new();
    let schemas = SCHEMAS.get_or_init(|| {
        let mut resources = BTreeMap::new();
        for (name, text) in [
            ("journey-verification-result.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/journey-verification-result.schema.json")),
            ("council-result.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/council-result.schema.json")),
            ("evidence-strength-lattice.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/evidence-strength-lattice.schema.json")),
            ("journey-contract.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json")),
            ("journey-defect-claim.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/journey-defect-claim.schema.json")),
            ("observation-obligation.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/observation-obligation.schema.json")),
            ("observer-capability.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/observer-capability.schema.json")),
            ("retry-chain.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/retry-chain.schema.json")),
            ("retry-lineage-validation-policy.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/retry-lineage-validation-policy.schema.json")),
            ("assurance-tier-policy.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/assurance-tier-policy.schema.json")),
            ("council-selection-policy.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/council-selection-policy.schema.json")),
            ("observer-catalog.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/observer-catalog.schema.json")),
            ("retry-chain-input.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/retry-chain-input.schema.json")),
            ("retry-classification-policy.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/retry-classification-policy.schema.json")),
            ("retry-lineage-input.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/retry-lineage-input.schema.json")),
            ("skill-capsule.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/skill-capsule.schema.json")),
            ("skill-evaluation.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/skill-evaluation.schema.json")),
            ("skill-promotion-policy.schema.json", include_str!("../../../../../extensions/builtin/graphhelm-jpd/schemas/skill-promotion-policy.schema.json")),
        ] {
            let Ok(document) = serde_json::from_str(text) else { return None };
            resources.insert(format!("https://p50.dev/extensions/graphhelm-jpd/schemas/{name}"), document);
        }
        graphhelm_schema::OfflineSchemaSet::compile(resources).ok()
    });
    let Some(schemas) = schemas else { return false };
    schemas.validate("https://p50.dev/extensions/graphhelm-jpd/schemas/journey-verification-result.schema.json", value, "/work/journeyVerification").is_empty()
        && value.pointer("/bindings/code/revision").and_then(serde_json::Value::as_str) == Some(work_revision)
}

pub(crate) fn validate_graph_binding(
    record: &DeliveryRecord,
    projection: &graphhelm_events::ExecutionProjection,
) -> Result<(), Failure> {
    let Some(work) = record.work.as_ref() else {
        return Ok(());
    };
    let Some(journey) = work.journey_verification.as_ref() else {
        return Ok(());
    };
    let Some(graph) = projection.current_graph.as_ref() else {
        return Err(invalid());
    };
    let matches = journey
        .pointer("/bindings/graph/graphId")
        .and_then(serde_json::Value::as_str)
        == Some(graph.topology().graph_id().as_str())
        && journey
            .pointer("/bindings/graph/version")
            .and_then(serde_json::Value::as_u64)
            == Some(graph.number())
        && journey
            .pointer("/bindings/graph/semanticHash")
            .and_then(serde_json::Value::as_str)
            == Some(graph.semantic_hash().as_str());
    matches.then_some(()).ok_or_else(invalid)
}

/// Host-local project identity. No absolute filesystem path leaves this boundary.
pub(crate) fn project_id(project: &Path) -> Result<String, Failure> {
    let canonical = project
        .canonicalize()
        .map_err(|_| argument("project must name an existing directory", "/project"))?;
    if !canonical.is_dir() {
        return Err(argument(
            "project must name an existing directory",
            "/project",
        ));
    }
    let identity = canonical
        .to_str()
        .ok_or_else(|| argument("project path must be valid Unicode", "/project"))?;
    #[cfg(windows)]
    // Keep canonical path case: per-directory case sensitivity makes differently cased
    // directories distinct identities even on a case-insensitive Windows volume. Only normalize
    // separators so verbatim drive and UNC paths retain their stable slash form.
    let identity = identity.replace('\\', "/");
    graphhelm_graph::raw_content_sha256(identity.as_bytes())
        .map(|digest| digest.as_str().to_owned())
        .map_err(|_| argument("project identity could not be derived", "/project"))
}

/// Lexical guard shared with the editor. Filesystem containment is a separate editor check.
pub(crate) fn validate_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 512
        || path.contains(['\\', ':', '\0'])
        || path.chars().any(char::is_control)
    {
        return false;
    }
    path.split('/').all(|part| {
        let lower = part.to_ascii_lowercase();
        let stem = lower.split('.').next().unwrap_or("");
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with(['.', ' '])
            && !part.contains(['<', '>', '"', '|', '?', '*'])
            && !matches!(
                lower.as_str(),
                ".git"
                    | ".ssh"
                    | ".aws"
                    | ".azure"
                    | ".graphhelm"
                    | ".docker"
                    | ".npmrc"
                    | ".pypirc"
                    | ".netrc"
                    | ".git-credentials"
                    | "credentials"
                    | "credentials.json"
                    | "credentials.yaml"
                    | "credentials.yml"
                    | "secrets"
                    | "secrets.json"
                    | "secrets.yaml"
                    | "secrets.yml"
                    | "id_rsa"
                    | "id_ed25519"
            )
            && !lower.starts_with(".env")
            && !lower.ends_with(".pem")
            && !lower.ends_with(".key")
            && !lower.ends_with(".token")
            && !matches!(
                stem,
                "con"
                    | "prn"
                    | "aux"
                    | "nul"
                    | "com1"
                    | "com2"
                    | "com3"
                    | "com4"
                    | "com5"
                    | "com6"
                    | "com7"
                    | "com8"
                    | "com9"
                    | "lpt1"
                    | "lpt2"
                    | "lpt3"
                    | "lpt4"
                    | "lpt5"
                    | "lpt6"
                    | "lpt7"
                    | "lpt8"
                    | "lpt9"
            )
    })
}

pub(crate) fn parse_record(bytes: &[u8]) -> Result<DeliveryRecord, Failure> {
    if bytes.len() > MAX_DELIVERY_BYTES {
        return Err(invalid());
    }
    let record: DeliveryRecord = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let text = |value: &str, max: usize| {
        !value.trim().is_empty()
            && value.len() <= max
            && !value.contains('\0')
            && !secret_shaped(value)
    };
    let ids = |values: &[String]| {
        values.len() <= 32
            && values.iter().all(|value| {
                value.len() <= 128
                    && graphhelm_protocols::OpaqueId::parse(value).is_ok()
                    && !secret_shaped(value)
            })
    };
    let work_valid = record.work.as_ref().is_none_or(|work| {
        work.version == 1
            && bounded_text(&work.session_id, 256)
            && bounded_text(&work.stage, 128)
            && revision(&work.revision)
            && work.skills.len() <= 16
            && work.checks.len() <= 32
            && {
                let mut attempts = BTreeSet::new();
                work.checks.iter().all(|check| {
                    attempts.insert((check.id.as_str(), check.attempt_id.as_str()))
                        && check.previous_attempt_id.as_deref() != Some(check.attempt_id.as_str())
                })
            }
            && work.skills.iter().all(|skill| {
                bounded_text(&skill.id, 256)
                    && bounded_text(&skill.version, 64)
                    && digest(&skill.digest)
            })
            && work.checks.iter().all(|check| {
                bounded_text(&check.id, 256)
                    && bounded_text(&check.command, 1024)
                    && bounded_text(&check.observer, 256)
                    && bounded_text(&check.attempt_id, 256)
                    && check
                        .previous_attempt_id
                        .as_deref()
                        .is_none_or(|id| bounded_text(id, 256))
                    && check.evidence.as_ref().is_none_or(|e| {
                        bounded_text(&e.evidence_id, 256)
                            && digest(&e.content_hash)
                            && e.size <= 16 * 1024 * 1024
                    })
                    && (matches!(
                        check.outcome,
                        DeliveryOutcome::Skipped | DeliveryOutcome::Unobserved
                    ) || check.evidence.as_ref().is_some_and(|e| {
                        bounded_text(&e.evidence_id, 256) && digest(&e.content_hash)
                    }))
            })
            && work
                .journey_verification
                .as_ref()
                .is_none_or(|journey| validate_journey(journey, &work.revision))
    });
    if record.version != 1
        || record.project_id.len() != 64
        || !record
            .project_id
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || !text(&record.summary, 2048)
        || !text(&record.reason, 4096)
        || record.documents.is_empty()
        || record.documents.len() > 32
        || !record.documents.iter().all(|doc| {
            validate_path(&doc.path)
                && !secret_shaped(&doc.path)
                && text(&doc.title, 256)
                && ids(&doc.journey_ids)
                && ids(&doc.rule_ids)
        })
        || !work_valid
    {
        return Err(invalid());
    }
    Ok(record)
}

/// The source node and all claims are reported by the caller. An explicit agent identity is
/// recorded as supplied; owner-entered reports preserve the existing CLI owner attribution.
pub(crate) fn run(
    events: &Path,
    execution: &str,
    node: &str,
    delivery: &Path,
    project: &Path,
    sealing: &signal::SignalKeyring,
    actor_id: Option<&str>,
) -> Outcome {
    finish(
        "execution.delivery",
        (|| {
            // Validate attribution before minting either event identity or touching evidence.
            let actor = match actor_id {
                Some(id) if secret_shaped(id) => {
                    return Err(argument(
                        "actor-id must not contain credential-shaped content",
                        "/actorId",
                    ));
                }
                Some(id) => graphhelm_protocols::PersistedActor::new(
                    graphhelm_protocols::PersistedActorType::Agent,
                    graphhelm_protocols::ActorId::parse(id).map_err(|_| {
                        argument("actor-id must be a valid agent identifier", "/actorId")
                    })?,
                ),
                None => owner_actor(),
            };
            let mut bytes = Vec::new();
            std::fs::File::open(delivery)
                .map_err(|_| invalid())?
                .take((MAX_DELIVERY_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid())?;
            if bytes.len() > MAX_DELIVERY_BYTES {
                return Err(invalid());
            }
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            value
                .as_object_mut()
                .ok_or_else(invalid)?
                .insert("projectId".into(), project_id(project)?.into());
            let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
            let record = parse_record(&bytes)?;
            let envelope = serde_json::json!({
                "id": idempotency_key("delivery").as_str(),
                "source": {"type": "node", "id": node},
                "type": "node_delivery", "severity": "low",
                "description": serde_json::to_string(&record).map_err(|_| invalid())?,
                "evidence": ["agent-reported-delivery"],
                "emittedAt": chrono::Utc::now().to_rfc3339(),
            });
            let mut result = signal::execute(
                events,
                Some(execution),
                &serde_json::to_vec(&envelope).map_err(|_| invalid())?,
                None,
                actor,
                idempotency_key("delivery-recorded"),
                Some(sealing),
            )?;
            result["provenance"] = "reported".into();
            Ok(result)
        })(),
        |value| value,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_preserves_canonical_case() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("Case-Preserved");
        std::fs::create_dir(&project).unwrap();
        let canonical = project.canonicalize().unwrap();
        let identity = canonical.to_str().unwrap();
        #[cfg(windows)]
        let identity = identity.replace('\\', "/");
        let expected = graphhelm_graph::raw_content_sha256(identity.as_bytes())
            .unwrap()
            .as_str()
            .to_owned();

        let actual = match project_id(&project) {
            Ok(actual) => actual,
            Err(_) => panic!("the existing project directory must have an identity"),
        };
        assert_eq!(actual, expected);
    }

    fn record() -> serde_json::Value {
        serde_json::json!({"version":1,"projectId":"a".repeat(64),"summary":"Created checkout rules","reason":"Clarify retries","documents":[{"path":"docs/prd/checkout.md","title":"Checkout","kind":"business_rule","action":"created","journeyIds":["checkout"]}]})
    }
    #[test]
    fn accepts_reported_record_and_refuses_malformed_contracts() {
        let value = record();
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_ok());
        for bad in [serde_json::json!(2), serde_json::json!(null)] {
            let mut value = record();
            value["version"] = bad;
            assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut value = record();
        value["documents"][0]["action"] = "verified".into();
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = record();
        value["extra"] = true.into();
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(parse_record(&vec![b' '; MAX_DELIVERY_BYTES + 1]).is_err());
    }

    #[test]
    fn accepts_bounded_work_and_rejects_unsupported_or_unbound_checks() {
        let mut value = record();
        value["work"] = serde_json::json!({
            "version": 1,
            "sessionId": "session-1",
            "stage": "implementation",
            "revision": "a".repeat(40),
            "skills": [{"id":"meaningful-tests","version":"1.0.0","digest":format!("sha256:{}", "b".repeat(64)),"status":"reported"}],
            "checks": [{"id":"focused","command":"cargo test","observer":"cli","outcome":"passed","attemptId":"attempt-1","evidence":{"evidenceId":"evidence-1","contentHash":format!("sha256:{}", "c".repeat(64)),"size":12}}]
        });
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["work"]["checks"][0]["outcome"] = "verified".into();
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
        value["work"]["checks"][0]["outcome"] = "passed".into();
        value["work"]["checks"][0]
            .as_object_mut()
            .unwrap()
            .remove("evidence");
        assert!(parse_record(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn graph_binding_requires_the_active_published_identity() {
        let mut value = record();
        let mut journey: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../extensions/builtin/graphhelm-jpd/fixtures/positive/journey-verification-first-pass.json"
        ))
        .unwrap();
        let graph_id = "graph-1";
        let graph_hash = format!("sha256:{}", "b".repeat(64));
        journey["bindings"]["graph"] =
            serde_json::json!({"graphId":graph_id,"version":1,"semanticHash":graph_hash});
        journey["bindings"]["code"]["revision"] = "a".repeat(40).into();
        value["work"] = serde_json::json!({"version":1,"sessionId":"session-1","stage":"implementation","revision":"a".repeat(40),"skills":[],"checks":[],"journeyVerification":journey});
        let delivery = parse_record(&serde_json::to_vec(&value).unwrap())
            .ok()
            .unwrap();
        let mut bad_revision = value.clone();
        bad_revision["work"]["journeyVerification"]["bindings"]["code"]["revision"] =
            "c".repeat(40).into();
        assert!(parse_record(&serde_json::to_vec(&bad_revision).unwrap()).is_err());
        let mut projection = graphhelm_events::ExecutionProjection::default();
        assert!(validate_graph_binding(&delivery, &projection).is_err());
        projection.current_graph = Some(test_graph_version());
        assert!(validate_graph_binding(&delivery, &projection).is_ok());
        for field in ["graphId", "version", "semanticHash"] {
            let mut changed = delivery.clone();
            changed
                .work
                .as_mut()
                .unwrap()
                .journey_verification
                .as_mut()
                .unwrap()["bindings"]["graph"][field] = match field {
                "graphId" => "other".into(),
                "version" => 2.into(),
                _ => format!("sha256:{}", "c".repeat(64)).into(),
            };
            assert!(
                validate_graph_binding(&changed, &projection).is_err(),
                "{field}"
            );
        }
    }

    fn test_graph_version() -> graphhelm_protocols::PersistedGraphVersion {
        use chrono::TimeZone;
        use std::collections::BTreeMap;
        let completion = graphhelm_protocols::PersistedControl::new(
            graphhelm_protocols::SafeValue::parse("all_terminal").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        let node = graphhelm_protocols::PersistedNode::new(
            graphhelm_protocols::NodeType::Agent,
            graphhelm_protocols::Optionality::Required,
            vec![],
            vec![],
            None,
            None,
        )
        .unwrap();
        let topology = graphhelm_protocols::PersistedTopology::new(
            graphhelm_protocols::OpaqueId::parse("graph-1").unwrap(),
            graphhelm_protocols::ExecutionId::parse("execution-1").unwrap(),
            BTreeMap::new(),
            vec![graphhelm_protocols::OpaqueId::parse("plan").unwrap()],
            BTreeMap::from([(graphhelm_protocols::OpaqueId::parse("plan").unwrap(), node)]),
            vec![],
            graphhelm_protocols::PersistedBudgets::default(),
            vec![],
            completion,
        )
        .unwrap();
        graphhelm_protocols::PersistedGraphVersion::new(
            1,
            None,
            topology,
            graphhelm_protocols::WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            graphhelm_protocols::WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            vec![],
            graphhelm_protocols::PersistedActor::new(
                graphhelm_protocols::PersistedActorType::System,
                graphhelm_protocols::ActorId::parse("system-test").unwrap(),
            ),
            graphhelm_protocols::PersistedTimestamp::from_datetime(
                chrono::Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn refuses_secret_shaped_delivery_prose_before_persistence() {
        for (field, secret) in [
            ("summary", "token=plain-value"),
            ("reason", "-----BEGIN PRIVATE KEY-----"),
            ("title", "sk-abcdefghijklmnopqrst"),
            ("path", "docs/token=plain-value.md"),
            ("journeyId", "token=plain-value"),
            ("ruleId", "sk-abcdefghijklmnopqrst"),
        ] {
            let mut value = record();
            match field {
                "summary" | "reason" => value[field] = secret.into(),
                "title" => value["documents"][0][field] = secret.into(),
                "path" => value["documents"][0][field] = secret.into(),
                "journeyId" => value["documents"][0]["journeyIds"] = serde_json::json!([secret]),
                "ruleId" => value["documents"][0]["ruleIds"] = serde_json::json!([secret]),
                _ => unreachable!(),
            }
            assert!(
                parse_record(&serde_json::to_vec(&value).unwrap()).is_err(),
                "secret-shaped {field} must be refused"
            );
        }
    }

    #[test]
    fn paths_cannot_address_credentials_or_escape_the_project() {
        for path in [
            "../x",
            "/etc/passwd",
            "C:/x",
            "docs/../x",
            "docs\\x",
            ".git/config",
            ".env",
            "docs/.env.local",
            ".ssh/id_rsa",
            "docs/con.txt",
            "docs/x.",
            "docs//x",
            "docs/x:key",
            "key.pem",
        ] {
            assert!(!validate_path(path), "{path}");
        }
        assert!(validate_path("docs/business rules/payment.md"));
    }
}
