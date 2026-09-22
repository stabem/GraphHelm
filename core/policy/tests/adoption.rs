use graphhelm_policy::adoption::{Decision, decision_allowed};

#[test]
fn successful_file_installation_is_not_verified_adoption() {
    use graphhelm_policy::adoption::adoption_verified;
    assert!(!adoption_verified(&[]));
    assert!(!adoption_verified(&[true, true, false]));
    assert!(adoption_verified(&[true, true, true]));
}

#[test]
fn classifier_cannot_disable_security_even_with_high_confidence() {
    assert!(!decision_allowed(true, Decision::Disable));
    assert!(!decision_allowed(true, Decision::Replace));
    assert!(decision_allowed(true, Decision::Keep));
    assert!(decision_allowed(true, Decision::Unresolved));
    assert!(decision_allowed(false, Decision::Disable));
}

#[test]
fn approval_is_for_one_nonempty_exact_plan() {
    use graphhelm_policy::adoption::approval_matches;

    assert!(approval_matches("sha256:abc", "sha256:abc"));
    assert!(!approval_matches("sha256:abc", "sha256:def"));
    assert!(!approval_matches("", ""));
}

#[test]
fn restore_does_not_overwrite_a_later_user_choice() {
    use graphhelm_policy::adoption::restore_value;

    let old = serde_json::json!("factory");
    let installed = serde_json::json!("graphhelm");
    let current = serde_json::json!("my-new-method");
    assert_eq!(
        restore_value(Some(&old), Some(&installed), Some(&current)),
        Err("restore_conflict")
    );
    assert_eq!(
        restore_value(Some(&old), Some(&installed), Some(&installed)),
        Ok(Some(old.clone()))
    );
    assert_eq!(
        restore_value(Some(&old), Some(&old), Some(&current)),
        Ok(Some(current))
    );
}
