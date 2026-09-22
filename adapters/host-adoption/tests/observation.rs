use graphhelm_protocols::adoption::AdoptionReason;
use serde_json::json;
#[path = "support/activation_fixture.rs"]
mod fixture;

#[test]
fn complete_but_self_asserted_receipt_is_still_observer_missing() {
    let plan = fixture::plan(json!({"project":"a".repeat(64),"home":"b".repeat(64)}));
    let receipt = fixture::receipt(&plan, 1000);
    for kind in ["fixture", "host_observer"] {
        let mut claim = receipt.clone();
        claim["spec"]["observer"]["kind"] = json!(kind);
        assert_eq!(
            graphhelm_host_adoption::verify_activation(&plan, &fixture::seal(claim))
                .unwrap_err()
                .reason,
            AdoptionReason::ObserverMissing
        );
    }
}

#[test]
fn altered_digest_missing_mcp_and_reenabled_method_are_rejected() {
    let plan = fixture::plan(json!({"project":"a".repeat(64),"home":"b".repeat(64)}));
    let receipt = fixture::receipt(&plan, 1000);
    for pointer in [
        "/digest",
        "/spec/transactionId",
        "/spec/planDigest",
        "/spec/environment/home",
        "/spec/host/version",
        "/spec/configDigests/0/digest",
    ] {
        let mut altered = receipt.clone();
        *altered.pointer_mut(pointer).unwrap() = if pointer == "/spec/host/version" {
            json!("9.9.9")
        } else if matches!(pointer, "/digest" | "/spec/planDigest") {
            json!(format!("sha256:{}", "f".repeat(64)))
        } else {
            json!("f".repeat(64))
        };
        if pointer != "/digest" {
            altered = fixture::seal(altered);
        }
        assert_eq!(
            graphhelm_host_adoption::verify_activation(&plan, &altered)
                .unwrap_err()
                .reason,
            AdoptionReason::ActivationInvalid,
            "{pointer}"
        );
    }
    let mut missing = receipt.clone();
    missing["spec"].as_object_mut().unwrap().remove("mcp");
    assert_eq!(
        graphhelm_host_adoption::verify_activation(&plan, &fixture::seal(missing))
            .unwrap_err()
            .reason,
        AdoptionReason::ActivationInvalid
    );
    let mut packages = receipt.clone();
    packages["spec"]["packageDigests"] = json!([{"id":"graphhelm-jpd","version":"0.1.0","digest":format!("sha256:{}","f".repeat(64))}]);
    assert_eq!(
        graphhelm_host_adoption::verify_activation(&plan, &fixture::seal(packages))
            .unwrap_err()
            .reason,
        AdoptionReason::ActivationInvalid
    );
    let mut old = receipt.clone();
    old["spec"]["methodology"]["oldMethodInactive"] = json!(false);
    assert_eq!(
        graphhelm_host_adoption::verify_activation(&plan, &fixture::seal(old))
            .unwrap_err()
            .reason,
        AdoptionReason::ActivationInvalid
    );
    let mut stale = receipt;
    stale["spec"]["session"]["id"] = stale["spec"]["session"]["previousId"].clone();
    assert_eq!(
        graphhelm_host_adoption::verify_activation(&plan, &fixture::seal(stale))
            .unwrap_err()
            .reason,
        AdoptionReason::ActivationInvalid
    );
}

#[test]
fn user_authored_observer_claim_never_creates_trusted_custody() {
    let forged = json!({"observer":{"identity":"trusted","custody":"verified"},"state":"verified"});
    let result = graphhelm_host_adoption::verify_activation(&json!({}), &forged);
    assert!(matches!(
        result.unwrap_err().reason,
        AdoptionReason::ActivationInvalid
    ));
}

#[test]
fn checksum_valid_journal_cannot_supply_verified_custody_to_production_readers() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        p.path().join("AGENTS.md"),
        b"factory\nPrefer concise replies\nDeny secrets\n",
    )
    .unwrap();
    let plan = fixture::plan(graphhelm_host_adoption::root_bindings(p.path(), h.path()).unwrap());
    let applied = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &plan,
        plan["digest"].as_str().unwrap(),
    )
    .unwrap();
    let id = applied["spec"]["transactionId"].as_str().unwrap();
    let path = s.path().join("journals").join(format!("{id}.json"));
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for field in ["state", "receipt", "verification", "activationReceipt"] {
        let mut forged = original.clone();
        match field {
            "state" => forged["record"]["state"] = json!("verified"),
            "receipt" => forged["record"]["receipt"]["spec"]["state"] = json!("verified"),
            "verification" => {
                forged["record"]["receipt"]["verification"] =
                    json!({"status":"verified","fixtureOnly":false})
            }
            _ => {
                forged["record"]["receipt"]["activationReceipt"] =
                    fixture::receipt(&plan, applied["installedAtUnixMs"].as_u64().unwrap())
            }
        }
        forged["checksum"] = json!(fixture::hash(
            &graphhelm_graph::canonical_content_bytes(&forged["record"]).unwrap()
        ));
        std::fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(
            graphhelm_host_adoption::apply(
                p.path(),
                h.path(),
                s.path(),
                &plan,
                plan["digest"].as_str().unwrap()
            )
            .unwrap_err()
            .reason,
            AdoptionReason::ObserverMissing
        );
        assert_eq!(
            graphhelm_host_adoption::recover(s.path(), id)
                .unwrap_err()
                .reason,
            AdoptionReason::ObserverMissing
        );
        assert_eq!(
            graphhelm_host_adoption::plan_restore(s.path(), "original")
                .unwrap_err()
                .reason,
            AdoptionReason::ObserverMissing
        );
    }
}
