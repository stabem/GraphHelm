//! Agent-reported node deliveries, stored only as sealed signal evidence.
use std::io::Read;
use std::path::Path;

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
