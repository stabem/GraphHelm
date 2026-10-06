//! The bundled release manifest pins each builtin package's digest. Editing a package under
//! `extensions/builtin/` without re-pinning it makes every `setup --apply` built from the manifest
//! refuse with `/adoption/plan_stale` (#206, #323). This names every stale package directly.

#[test]
fn release_manifest_pins_the_current_digest_of_every_bundled_package() {
    let packages = graphhelm_host_adoption::hosts::release_packages().unwrap();
    assert!(!packages.is_empty());
    let stale: Vec<String> = packages
        .iter()
        .filter_map(|package| {
            let current = graphhelm_schema::validate_extension_package(&package.path).unwrap();
            (package.id != current.id || package.digest != current.package_digest)
                .then(|| format!("{} -> {}", package.id, current.package_digest))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "extensions/releases/adoption-0.1.1.json pins stale digests; re-pin: {stale:?}"
    );
}
