//! Synthetic evidence only. No real host observation is performed by this fixture.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn seal(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("digest");
    value["digest"] = json!(format!(
        "sha256:{}",
        hash(&serde_json::to_vec(&value).unwrap())
    ));
    value
}
pub fn plan(bindings: Value) -> Value {
    seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"fixture-only",
        "spec":{"coverage":"complete","rootBindings":bindings,"scopes":["project"],"packages":[],"hostBoundary":"quiescent",
        "host":{"name":"claude","version":"2.1.265"},
        "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
        "operations":[{"root":"project","path":"AGENTS.md","beforeDigest":hash(b"factory\nPrefer concise replies\nDeny secrets\n"),"afterDigest":hash(b"GraphHelm JPD\nPrefer concise replies\nDeny secrets\n"),"after":"GraphHelm JPD\nPrefer concise replies\nDeny secrets\n"}]}}),
    )
}
pub fn receipt(plan: &Value, installed_at: u64) -> Value {
    seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"ActivationReceipt",
        "spec":{"transactionId":hash(plan["digest"].as_str().unwrap().as_bytes()),"planDigest":plan["digest"],
        "host":{"name":plan["spec"]["host"]["name"],"version":plan["spec"]["host"]["version"]},
        "environment":plan["spec"]["rootBindings"],
        "configDigests":plan["spec"]["operations"].as_array().unwrap().iter().map(|o|json!({"root":o["root"],"path":o["path"],"digest":o["afterDigest"]})).collect::<Vec<_>>(),
        "packageDigests":plan["spec"]["packages"],
        "session":{"id":"fixture-fresh-session","previousId":"fixture-old-session","startedAtUnixMs":installed_at+1},
        "observer":{"identity":"fixture-observer","custodyId":"fixture-custody","kind":"fixture","evidenceDigest":format!("sha256:{}",hash(b"fixture-evidence"))},
        "mcp":{"tool":"graphhelm","operation":"list","requestId":"fixture-request","runtimeObservationDigest":format!("sha256:{}",hash(b"fixture-runtime")),"sessionId":"fixture-fresh-session","observedAtUnixMs":installed_at+2},
        "methodology":{"loaded":true,"oldMethodInactive":true,"evidenceDigest":format!("sha256:{}",hash(b"fixture-methodology"))},
        "timestamps":{"installedAtUnixMs":installed_at,"observedAtUnixMs":installed_at+3,"expiresAtUnixMs":installed_at+100}}}),
    )
}
