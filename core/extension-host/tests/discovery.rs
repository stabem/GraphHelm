//! Canonical executable discovery (#212), G1 and G10.
//!
//! The package is attacker-supplied by definition. An installer that looks for an executable INSIDE
//! it is executing untrusted content, and "next to the package" is the convenient implementation --
//! which is exactly why the invariant has to be a test rather than a sentence.

use graphhelm_extension_host::{ActivationRecord, DiscoveryRefusal, resolve_executable};

/// The name a planted executable would take. Present in the fixture BY CONSTRUCTION, which is what
/// makes the refusal below meaningful: a naive search would find this, so a refusal separates "the
/// design holds" from "the fixture was empty".
#[cfg(windows)]
const PLANTED: &str = "graphhelm.exe";
#[cfg(not(windows))]
const PLANTED: &str = "graphhelm";

/// The production change this catches: falling back to a package-relative search when the
/// activation record carries no recorded path. It is the fallback anyone writes, it makes the
/// feature "work", and it hands execution to whoever authored the package.
#[test]
fn discovery_refuses_an_executable_that_exists_only_inside_the_package() {
    let package = tempfile::tempdir().expect("a temp dir");
    let planted = package.path().join(PLANTED);
    std::fs::write(&planted, b"not really an executable").expect("plant the executable");

    // Landmark: the bait is really there. Without this the refusal could pass against an empty
    // directory, which would prove nothing about the search.
    assert!(
        planted.exists(),
        "HARNESS-BROKE: the planted executable was not created, so a refusal below would be a \
         statement about an empty directory"
    );

    let record = ActivationRecord::for_package_without_recorded_executable(package.path());

    let refusal = resolve_executable(&record)
        .expect_err("discovery resolved an executable that exists only inside the package");
    assert_eq!(refusal, DiscoveryRefusal::NoRecordedExecutable);
}

/// G10 of the blueprint: "not found" and "found several" are different refusals.
///
/// The deliverable names the property — canonical discovery "without PATH ambiguity" — and
/// ambiguity is the case that has to survive into the code. Collapsed into one code, the caller
/// cannot tell "nothing was ever recorded" from "the state disagrees with itself", and those call
/// for opposite actions: install the extension, versus stop and repair the record.
///
/// The production change this catches: resolving with `.first()`. Picking one of two silently makes
/// a broken activation state look like a working one, and picks by list order — which is not a
/// decision anyone made.
#[test]
fn discovery_refuses_a_record_that_names_two_executables_rather_than_picking_one() {
    let package = tempfile::tempdir().expect("a temp dir");
    let first = package.path().join("one").join(PLANTED);
    let second = package.path().join("two").join(PLANTED);

    let ambiguous = ActivationRecord::for_package_with_recorded_executables(
        package.path(),
        vec![first.clone(), second.clone()],
    );

    // Landmark: exactly one recorded path DOES resolve, so the refusal below is about the plurality
    // and not about resolution being broken for everyone.
    let single = ActivationRecord::for_package_with_recorded_executables(
        package.path(),
        vec![first.clone()],
    );
    assert_eq!(
        resolve_executable(&single).expect("a single recorded executable must resolve"),
        first,
        "HARNESS-BROKE: the unambiguous case does not resolve, so the refusal below says nothing"
    );

    let refusal = resolve_executable(&ambiguous)
        .expect_err("discovery picked one executable out of two recorded ones");
    assert_eq!(refusal, DiscoveryRefusal::AmbiguousRecord);
}
