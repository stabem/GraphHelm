//! Activation claims and observer custody are distinct. This build has no production observer.
use graphhelm_policy::adoption::adoption_verified;
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde_json::{Value, json};

fn invalid() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::ActivationInvalid,
    }
}

fn content_digest(value: &Value) -> Result<String, AdoptionError> {
    let bytes = graphhelm_graph::canonical_content_bytes(value).map_err(|_| invalid())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid());
    }
    Ok(format!("sha256:{}", crate::apply::digest(&bytes)))
}

fn sealed(value: &Value) -> Result<bool, AdoptionError> {
    // Bound before cloning untrusted nested input.
    content_digest(value)?;
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .ok_or_else(invalid)?
        .remove("digest");
    Ok(value["digest"] == content_digest(&payload)?)
}

fn validate_claims(plan: &Value, receipt: &Value) -> Result<(), AdoptionError> {
    if !graphhelm_schema::validate_adoption_plan(plan).is_empty()
        || !graphhelm_schema::validate_activation_receipt(receipt).is_empty()
        || !sealed(plan)?
        || !sealed(receipt)?
    {
        return Err(invalid());
    }
    let spec = &receipt["spec"];
    let configs: Vec<_> = plan["spec"]["operations"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|op| json!({"root":op["root"],"path":op["path"],"digest":op["afterDigest"]}))
        .collect();
    let start = spec["session"]["startedAtUnixMs"]
        .as_u64()
        .ok_or_else(invalid)?;
    let times = &spec["timestamps"];
    let installed = times["installedAtUnixMs"].as_u64().ok_or_else(invalid)?;
    let observed = times["observedAtUnixMs"].as_u64().ok_or_else(invalid)?;
    let expires = times["expiresAtUnixMs"].as_u64().ok_or_else(invalid)?;
    let mcp_time = spec["mcp"]["observedAtUnixMs"]
        .as_u64()
        .ok_or_else(invalid)?;
    if !adoption_verified(&[
        spec["planDigest"] == plan["digest"],
        spec["transactionId"]
            == crate::apply::digest(plan["digest"].as_str().ok_or_else(invalid)?.as_bytes()),
        spec["environment"] == plan["spec"]["rootBindings"],
        spec["host"]["name"] == plan["spec"]["host"]["name"],
        spec["host"]["version"] == plan["spec"]["host"]["version"],
        spec["configDigests"] == json!(configs),
        spec["packageDigests"] == plan["spec"]["packages"],
        spec["session"]["id"] != spec["session"]["previousId"],
        spec["session"]["id"] == spec["mcp"]["sessionId"],
        installed < start && start <= mcp_time && mcp_time <= observed,
        observed < expires && expires - observed <= 300_000,
    ]) {
        return Err(invalid());
    }
    Ok(())
}

/// Validate the wire claim without treating it as trusted observation. User-authored JSON never
/// grants custody. A real host observer adapter must be added before production can verify adoption.
pub fn verify_activation(plan: &Value, receipt: &Value) -> Result<Value, AdoptionError> {
    validate_claims(plan, receipt)?;
    Err(AdoptionError {
        reason: AdoptionReason::ObserverMissing,
    })
}

// Deliberately private and not Deserialize. There is no production constructor or adapter.
// Adding a host observer requires a separate reviewed custody boundary, not a JSON flag.
struct Custody {
    receipt_digest: Value,
    observer: Value,
    session: Value,
    environment: Value,
    now_ms: u64,
}

/// Re-read the active transaction under its original anchors. This build records only
/// `observer_missing`; even a structurally complete receipt cannot supply observer custody.
pub fn verify_activation_at(
    state_root: &std::path::Path,
    plan: &Value,
    receipt: &Value,
) -> Result<Value, AdoptionError> {
    verify_at(state_root, plan, receipt, None)
}

fn verify_at(
    state_root: &std::path::Path,
    plan: &Value,
    receipt: &Value,
    custody: Option<&Custody>,
) -> Result<Value, AdoptionError> {
    use crate::{journal::Store, storage::Root};
    use graphhelm_protocols::adoption::TransactionState;
    validate_claims(plan, receipt)?;
    let reader = Store::reader(Root::open(state_root, false)?)?;
    let initial = reader.active()?.ok_or_else(invalid)?;
    let project = Root::reopen(&initial.project)?;
    let home = Root::reopen(&initial.home)?;
    let state_record = reader.root.record.clone();
    drop(reader);
    // Match apply/restore's user -> project -> state authority ordering.
    let _user_lock = home.lock(".graphhelm-adoption.lock")?;
    let _project_lock = project.lock(".graphhelm-adoption.lock")?;
    let store = Store::open(Root::reopen(&state_record)?)?;
    let mut record = store.active()?.ok_or_else(invalid)?;
    if record.transaction_id != initial.transaction_id
        || record.sequence != initial.sequence
        || !matches!(
            record.state,
            TransactionState::InstalledUnverified | TransactionState::Verified
        )
        || record.plan_digest != plan["digest"].as_str().ok_or_else(invalid)?
        || record.transaction_id != receipt["spec"]["transactionId"]
        || plan["spec"]["rootBindings"]
            != crate::root_bindings(&project.record.path, &home.record.path)?
    {
        return Err(invalid());
    }
    let mut result = record.receipt.clone().ok_or_else(invalid)?;
    if result["installedAtUnixMs"].as_u64().is_none()
        || result["installedAtUnixMs"] != receipt["spec"]["timestamps"]["installedAtUnixMs"]
    {
        return Err(invalid());
    }
    let expected_configs: Vec<_> = record
        .entries
        .iter()
        .map(|entry| json!({"root":entry.root,"path":entry.path,"digest":entry.after_digest}))
        .collect();
    let expected_packages: Vec<_> = record
        .packages
        .iter()
        .map(|entry| json!({"id":entry.id,"digest":entry.digest}))
        .collect();
    let observed_packages: Vec<_> = receipt["spec"]["packageDigests"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|entry| json!({"id":entry["id"],"digest":entry["digest"]}))
        .collect();
    if receipt["spec"]["configDigests"] != json!(expected_configs)
        || observed_packages != expected_packages
    {
        return Err(invalid());
    }
    let verified = if let Some(custody) = custody {
        let spec = &receipt["spec"];
        if !adoption_verified(&[
            receipt["digest"] == custody.receipt_digest,
            spec["observer"] == custody.observer,
            spec["session"] == custody.session,
            spec["environment"] == custody.environment,
            spec["timestamps"]["observedAtUnixMs"]
                .as_u64()
                .is_some_and(|time| time <= custody.now_ms),
            spec["timestamps"]["expiresAtUnixMs"]
                .as_u64()
                .is_some_and(|time| custody.now_ms < time),
        ]) {
            return Err(invalid());
        }
        true
    } else {
        false
    };
    // Persist only after reading current names/bytes and package activation under retained roots.
    // This is point-in-time evidence; it does not promise that an unrelated writer cannot edit later.
    project.verify()?;
    home.verify()?;
    for entry in &record.entries {
        let root = if entry.root == "project" {
            &project
        } else {
            &home
        };
        let source = root.source(&entry.path)?;
        if crate::apply::digest(&source.read()?) != entry.after_digest {
            return Err(invalid());
        }
    }
    crate::hosts::verify_installed(&store, &record)?;
    record.state = if verified {
        TransactionState::Verified
    } else {
        TransactionState::InstalledUnverified
    };
    result["spec"]["state"] = json!(record.state);
    result["verification"] = json!({
        "status":if verified { "verified" } else { "observer_missing" },
        "fileState":"matches_accepted_plan",
        "fixtureOnly":verified && receipt["spec"]["observer"]["kind"] == "fixture",
        "receiptDigest":receipt["digest"],
    });
    if verified {
        result["activationReceipt"] = receipt.clone();
    } else {
        result
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("activationReceipt");
    }
    if !graphhelm_schema::validate_adoption_receipt(&result).is_empty() {
        return Err(invalid());
    }
    record.receipt = Some(result.clone());
    store.sync_with_hook(&mut record, &mut || Ok(()))?;
    Ok(result)
}

#[cfg(test)]
#[path = "../tests/support/observation_harness.rs"]
mod tests;
