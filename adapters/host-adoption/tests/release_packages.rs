//! The bundled release manifest pins each builtin package's digest. Editing a package under
//! `extensions/builtin/` without re-pinning it makes every `setup --apply` built from the manifest
//! refuse with `/adoption/plan_stale` (#206, #323). This names the stale package directly.

#[test]
fn release_manifest_pins_the_current_digest_of_every_bundled_package() {
    let packages = graphhelm_host_adoption::hosts::release_packages().unwrap();
    assert!(!packages.is_empty());
    for package in packages {
        let current = graphhelm_schema::validate_extension_package(&package.path).unwrap();
        assert_eq!(
            (package.id.as_str(), package.digest.as_str()),
            (current.id.as_str(), current.package_digest.as_str()),
            "extensions/releases/adoption-0.1.1.json pins a stale digest for {}; set it to {}",
            package.id,
            current.package_digest
        );
    }
}
