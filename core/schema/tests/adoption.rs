use serde_json::json;
#[test]
fn adoption_wire_schema_rejects_incomplete_and_new_major_mutation_documents() {
    let incomplete = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"x","digest":format!("sha256:{}","a".repeat(64)),"spec":{}});
    assert!(!graphhelm_schema::validate_adoption_plan(&incomplete).is_empty());
    let receipt = json!({"apiVersion":"p50.dev/adoption/v1","kind":"ApplyReceipt","id":"a".repeat(64),"spec":{"transactionId":"a".repeat(64),"planDigest":format!("sha256:{}","b".repeat(64)),"backupId":"c".repeat(64),"state":"installed_unverified"}});
    assert!(graphhelm_schema::validate_adoption_receipt(&receipt).is_empty());
    let mut wrong = receipt.clone();
    wrong["apiVersion"] = json!("p50.dev/adoption/v2");
    assert!(!graphhelm_schema::validate_adoption_receipt(&wrong).is_empty());
    assert!(
        !graphhelm_schema::validate_adoption_journal(
            &json!({"record":{},"checksum":"a".repeat(64)})
        )
        .is_empty()
    );
}
