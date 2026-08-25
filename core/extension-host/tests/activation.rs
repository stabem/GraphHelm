//! Activation state and its exclusive claim (#212), G5.

use graphhelm_extension_host::{ActivationClaim, ActivationRecord, ClaimRefusal};

/// The production change this catches: `File::create` where `create_new` was meant.
///
/// Both are "make the file". Only one of them FAILS when the file is already there, and a
/// create-if-absent that succeeds twice is not a claim -- it is two writers each believing they
/// hold the switch. The house rule (ED-22) names the same distinction on the PowerShell side:
/// `CreateNew`, never `New-Item`.
#[test]
fn a_second_claim_is_refused_while_the_first_is_held_and_granted_after_it_is_released() {
    let root = tempfile::tempdir().expect("a temp dir");

    let first = ActivationClaim::acquire(root.path()).expect("the first claim must be granted");

    // Landmark: the claim is really held. Without this, the refusal below could be a claim path
    // that fails for everyone, and "the second one failed" would prove nothing.
    assert!(
        first.path().exists(),
        "HARNESS-BROKE: the first claim left nothing on disk, so a second refusal would say \
         nothing about exclusivity"
    );

    let second = ActivationClaim::acquire(root.path());
    assert_eq!(
        second.err(),
        Some(ClaimRefusal::AlreadyHeld),
        "a second claim was granted while the first was held"
    );

    // The other half of the pair: a refusal that never lifts is a broken claim, not a safe one.
    drop(first);
    let third = ActivationClaim::acquire(root.path());
    assert!(
        third.is_ok(),
        "HARNESS-BROKE: the claim was never released, so the refusal above may be permanent \
         breakage rather than exclusivity"
    );
}

/// G3 of the blueprint: a failed activation leaves the PREVIOUS version active.
///
/// Every prefix of the durable write sequence is checked, not one hand-picked crash point. A crash
/// test that chooses its own boundary tests the boundary the author thought of.
///
/// The production change this catches: retiring the previous version before recording the new one.
/// Both orders complete identically; they differ only in what a crash leaves behind, and one of
/// them leaves a machine with NO active version -- which is not a rollback, it is an outage.
#[test]
fn no_crash_point_leaves_the_machine_without_an_active_version() {
    let steps = graphhelm_extension_host::activation_steps();

    // Landmarks: the sequence must actually do both things, or the invariant below holds
    // vacuously over a sequence that never retires anything.
    assert!(
        steps.contains(&graphhelm_extension_host::ActivationStep::NewVersionRecorded),
        "HARNESS-BROKE: the sequence never records a new version"
    );
    assert!(
        steps.contains(&graphhelm_extension_host::ActivationStep::PreviousVersionRetired),
        "HARNESS-BROKE: the sequence never retires the previous version"
    );

    for crash_after in 0..=steps.len() {
        let survived = &steps[..crash_after];
        let retired =
            survived.contains(&graphhelm_extension_host::ActivationStep::PreviousVersionRetired);
        let recorded =
            survived.contains(&graphhelm_extension_host::ActivationStep::NewVersionRecorded);

        assert!(
            recorded || !retired,
            "a crash after {crash_after} step(s) left no active version at all: the previous one \
             was retired before the new one was recorded. The survivor reads as {survived:?}"
        );
    }
}

/// Built as a literal because `ValidatedExtensionPackage` has no constructor. That couples this
/// test to the struct's field list -- #325 added `contributions` upstream and this helper stopped
/// compiling, which is the coupling announcing itself. A compile error is the right failure mode
/// for it: it names the field and the line, and it cannot be mistaken for a behaviour change.
fn validated(id: &str, version: &str, digest: &str) -> graphhelm_schema::ValidatedExtensionPackage {
    graphhelm_schema::ValidatedExtensionPackage {
        id: id.to_owned(),
        version: version.to_owned(),
        contribution_count: 1,
        package_digest: digest.to_owned(),
        contributions: Vec::new(),
    }
}

/// G2 of the blueprint: validation alone grants no runtime authority.
///
/// The record is minted from ONE validated package and authorizes that one. A second package that
/// also validates is still not the one that was activated.
///
/// The production change this catches: authorizing by IDENTITY. Comparing the extension id is the
/// natural shortcut -- it reads as "is this the same extension?" -- and every version of an
/// extension shares its id, so a record minted for v1 would authorize v2. Validation says
/// well-formed; identity says same family; neither says "this is the artifact that was activated".
#[test]
fn a_record_authorizes_the_package_it_was_activated_from_and_no_other() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let activated = validated("graphhelm-example", "1.0.0", "sha256:aaaa");
    let record = ActivationRecord::activate(&claim, &activated, root.path().join("graphhelm"));

    // Landmark: the record authorizes its own package. Without this the refusal below could be a
    // record that authorizes nothing at all, which would be safe and useless.
    assert!(
        record.authorizes(&activated),
        "HARNESS-BROKE: the record does not authorize the package it was activated from"
    );

    // Same extension, same id, different artifact. It validates; it was not activated.
    let other_version = validated("graphhelm-example", "1.1.0", "sha256:bbbb");
    assert!(
        !record.authorizes(&other_version),
        "a record minted for one package authorized a different one that merely validates"
    );
}
