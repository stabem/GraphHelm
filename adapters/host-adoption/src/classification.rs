//! Conservative local proposal generation; no model is consulted in this slice.
//!
//! Two documents come out of here. `propose` is the REVIEW: one decision per item that exists,
//! `keep` for everything that is not an instruction file and `unresolved` for every instruction
//! file, because prose is not classified by a keyword and no judge is wired. `resolve` turns a
//! reviewed set of owner decisions into the APPLYABLE plan `apply` accepts: only `replace`
//! decisions become operations, every `unresolved` item must have been resolved, the plan is
//! sealed with the same canonical digest `apply` recomputes, and the after-bytes live in the
//! private file the caller writes, never in the public envelope.

use graphhelm_policy::adoption::{Decision, decision_allowed};
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

/// The instruction surfaces `apply` accepts an operation on. Mirrors the instruction half of the
/// allow-list in `apply::preflight` (#1208 F7). Rules trees are not backup surfaces, so they stay
/// out: a replaced rule could not be restored.
const OPERABLE: [&str; 6] = [
    "project/AGENTS.md",
    "project/CLAUDE.md",
    "project/.claude/CLAUDE.md",
    "project/CLAUDE.local.md",
    "home/AGENTS.md",
    "home/.claude/CLAUDE.md",
];

fn invalid() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    }
}

fn review_required() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::ReviewRequired,
    }
}

/// An owner's answer to one unresolved item. `after` carries the reviewed replacement bytes and
/// is required for `Replace`; it is ignored for `Keep`.
#[derive(Clone, Debug)]
pub struct Resolution {
    pub item: String,
    pub decision: Decision,
    pub after: Option<Vec<u8>>,
}

struct Reviewed {
    host: String,
    id: String,
    kind: String,
    root: String,
    path: String,
    digest: Option<String>,
    protected: bool,
    decision: Decision,
}

fn review(inventory: &Value) -> Result<Vec<Reviewed>, AdoptionError> {
    let hosts = inventory
        .pointer("/spec/hosts")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut reviewed = Vec::new();
    for host in hosts {
        let name = host
            .get("host")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let items = host
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        for item in items {
            // An absent surface is nothing to decide about; only what exists gets a decision.
            if item["status"] == "absent" {
                continue;
            }
            let id = item["id"].as_str().ok_or_else(invalid)?;
            let kind = item["kind"].as_str().ok_or_else(invalid)?;
            let surface_kind = item["surfaceKind"].as_str().unwrap_or(kind);
            let protected = item["protected"].as_bool().unwrap_or(false)
                || kind.ends_with("settings.json")
                || kind.ends_with("settings.local.json");
            // Instruction prose is never classified locally; everything else is left in place,
            // which is the one decision that cannot remove a capability the owner relies on.
            let decision = if matches!(surface_kind, "instructions" | "rule") {
                Decision::Unresolved
            } else {
                Decision::Keep
            };
            if !decision_allowed(protected, decision) {
                return Err(invalid());
            }
            reviewed.push(Reviewed {
                host: name.to_owned(),
                id: id.to_owned(),
                kind: kind.to_owned(),
                root: item["root"].as_str().unwrap_or("").to_owned(),
                path: item["path"].as_str().unwrap_or("").to_owned(),
                digest: item["digest"]
                    .as_str()
                    .and_then(|digest| digest.strip_prefix("sha256:"))
                    .map(str::to_owned),
                protected,
                decision,
            });
        }
    }
    Ok(reviewed)
}

fn decision_name(decision: Decision) -> &'static str {
    match decision {
        Decision::Keep => "keep",
        Decision::Disable => "disable",
        Decision::Replace => "replace",
        Decision::Unresolved => "unresolved",
    }
}

/// Creates the review-only plan. Existing items only; instruction files stay unresolved.
pub fn propose(inventory: &Value) -> Result<Value, AdoptionError> {
    let reviewed = review(inventory)?;
    let decisions = reviewed
        .iter()
        .map(|item| {
            let operable = OPERABLE.contains(&item.id.as_str());
            json!({
                "host": item.host, "item": item.id, "kind": item.kind,
                "decision": decision_name(item.decision),
                "reason": match item.decision {
                    Decision::Unresolved => "owner_review_required",
                    _ => "no_conflict_classified",
                },
                "protected": item.protected,
                "operable": operable,
            })
        })
        .collect::<Vec<_>>();
    let unresolved = reviewed
        .iter()
        .filter(|item| item.decision == Decision::Unresolved)
        .count();
    let operable_unresolved = reviewed
        .iter()
        .filter(|item| {
            item.decision == Decision::Unresolved && OPERABLE.contains(&item.id.as_str())
        })
        .count();
    Ok(json!({
        "apiVersion": "p50.dev/adoption/v1",
        "kind": "AdoptionPlan",
        "id": "plan/local-preview",
        "spec": {
            "decisions": decisions,
            "applyAllowed": false,
            "rootBindings": inventory["spec"]["rootBindings"],
            "coverage": inventory["spec"]["coverage"],
            "summary": {
                "keep": reviewed.len() - unresolved,
                "unresolved": unresolved,
                "operableUnresolved": operable_unresolved,
            },
            "resolution": {
                "instruction": "Every unresolved item needs an owner decision: --resolve <item>=keep or --resolve <item>=replace:<file with the reviewed bytes>. Only items marked operable can be replaced in this build. Write the result with --out; the private plan is what --apply takes.",
            }
        }
    }))
}

/// Builds the applyable plan from the owner's resolutions. Every unresolved item must be
/// answered; every `replace` needs reviewed after-bytes and an operable item; the result is
/// sealed with the digest `apply` recomputes.
pub fn resolve(inventory: &Value, resolutions: &[Resolution]) -> Result<Value, AdoptionError> {
    let reviewed = review(inventory)?;
    let mut answered = std::collections::BTreeSet::new();
    let mut operations = Vec::new();
    let mut decisions = Vec::new();
    let mut kept = Vec::new();
    let mut scopes = std::collections::BTreeSet::new();
    for resolution in resolutions {
        let item = reviewed
            .iter()
            .find(|item| item.id == resolution.item)
            .ok_or_else(review_required)?;
        if item.decision != Decision::Unresolved || !answered.insert(item.id.clone()) {
            return Err(review_required());
        }
        // `decision_allowed` bounds the CLASSIFIER, which may only keep or defer a protected
        // instruction file. A resolution is the owner's own decision on that file; what still
        // protects the security lines inside it is `apply`'s `protect_instructions`, which
        // refuses a replacement that drops a deny/secret/permission line.
        match resolution.decision {
            Decision::Keep => kept.push(json!({"item": item.id, "decision": "keep"})),
            Decision::Replace => {
                if !OPERABLE.contains(&item.id.as_str()) {
                    return Err(invalid());
                }
                let after = resolution.after.as_deref().ok_or_else(review_required)?;
                let after_text = std::str::from_utf8(after).map_err(|_| invalid())?;
                if after.len() > 1024 * 1024 {
                    return Err(AdoptionError {
                        reason: AdoptionReason::LimitExceeded,
                    });
                }
                let before = item.digest.clone().ok_or_else(invalid)?;
                let after_digest = hex::encode(Sha256::digest(after));
                if before == after_digest {
                    // A replace that changes nothing is a keep that asks for a mutation.
                    return Err(review_required());
                }
                scopes.insert(if item.root == "home" {
                    "user"
                } else {
                    "project"
                });
                decisions.push(json!({
                    "operationIndex": operations.len(),
                    "decision": "replace",
                    "protected": false,
                }));
                operations.push(json!({
                    "root": item.root,
                    "path": item.path,
                    "beforeDigest": before,
                    "afterDigest": after_digest,
                    "after": after_text,
                }));
            }
            Decision::Disable | Decision::Unresolved => return Err(review_required()),
        }
    }
    if reviewed
        .iter()
        .any(|item| item.decision == Decision::Unresolved && !answered.contains(&item.id))
    {
        return Err(review_required());
    }
    if operations.is_empty() {
        // `resolve` never adds release packages, so a plan with no replacement installs nothing
        // and changes nothing: there is nothing to apply. `apply` accepts a packages-only plan
        // (#1208 F6), but it is not produced here. Refused, not silently padded.
        return Err(invalid());
    }
    seal(json!({
        "apiVersion": "p50.dev/adoption/v1",
        "kind": "AdoptionPlan",
        "id": "plan/reviewed",
        "spec": {
            "coverage": inventory["spec"]["coverage"],
            "rootBindings": inventory["spec"]["rootBindings"],
            "scopes": scopes.into_iter().collect::<Vec<_>>(),
            "packages": [],
            "hostBoundary": "quiescent",
            "decisions": decisions,
            "operations": operations,
            "review": kept,
        }
    }))
}

/// Seals a document with the digest `apply` and `restore` recompute: canonical content bytes of
/// everything but the enclosing `digest`.
pub fn seal(mut value: Value) -> Result<Value, AdoptionError> {
    let object = value.as_object_mut().ok_or_else(invalid)?;
    object.remove("digest");
    let canonical =
        graphhelm_graph::canonical_content_bytes(&value).map_err(|_| AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        })?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&canonical)));
    value["digest"] = json!(digest);
    Ok(value)
}

/// The public face of a plan: the same document without any after-bytes. Digests stay, so the
/// reader can still bind what they review to what `--apply` will check.
pub fn redact(mut plan: Value) -> Value {
    if let Some(operations) = plan["spec"]["operations"].as_array_mut() {
        for operation in operations {
            if let Some(object) = operation.as_object_mut() {
                object.remove("after");
                object.insert(
                    "after".into(),
                    json!("<redacted: in the private plan file>"),
                );
            }
        }
    }
    plan
}

/// Writes a document as an owner-only file, atomically, under a directory that exists. The
/// parent is opened as an anchored root so a link in its place is refused, not followed.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AdoptionError> {
    let parent = path.parent().ok_or_else(crate::storage::unsafe_path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(crate::storage::unsafe_path)?;
    let root = crate::storage::Root::open(parent, false)?;
    crate::storage::write_atomic(&root.file, name, bytes)
}
