//! Points the package validator at the real `graphhelm-development-contracts` package.
//!
//! **Why this file exists, measured rather than assumed.** The wave rule for the shared package
//! (#216) says a content change to a declared file updates that entry's digest in the same PR. The
//! validator already enforces that (`GHEX005_DIGEST` over declared content, `GHEX012_INVENTORY`
//! over discoverable files) — and **nothing was pointing it at this package**.
//!
//! Demonstrated live while adding `context_budget_insufficient` in #222: editing
//! `schemas/development-envelope.schema.json` left its manifest digest stale
//! (`sha256:3485102f…` declared against `sha256:22c22aa7…` actual) and **57 tests stayed green** —
//! 26 in `development_contract_schemas`, 31 in `extension_cli`. The rule had no sweeper: the
//! instrument existed and was aimed at fixture packages, never at the real one.
//!
//! Seven tasks write into this package (#218, #219, #220, #221, #222, #225, #226). Any of them can
//! edit a declared file and leave a stale digest. From here on that is red, and it is red **here**
//! rather than in whichever lane happens to notice.

use std::path::PathBuf;

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts")
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **editing any
/// file declared in `extension.json` without updating that entry's `sha256` in the same commit**,
/// or adding a discoverable file under `policies/` or `fixtures/` without declaring it at all.
///
/// The failure this blocks is silent by construction. A stale digest breaks nothing at runtime and
/// no other suite reads the manifest, so the package ships describing content it no longer has —
/// and every downstream consumer that trusts the digest is trusting a number about different bytes.
#[test]
fn the_development_contracts_package_validates_against_its_own_manifest() {
    let package = package_root();
    assert!(
        package.join("extension.json").is_file(),
        "arrangement check: the package manifest must exist at {} -- if this fires the test is \
         looking in the wrong place and proves nothing about the package",
        package.display()
    );

    match graphhelm_schema::validate_extension_package(&package) {
        Ok(validated) => {
            assert!(
                validated.contribution_count > 0,
                "the package validated with ZERO contributions, which would make this guard \
                 vacuous: an empty manifest declares nothing, so nothing can be stale. Expected \
                 the real declared set."
            );
        }
        Err(diagnostics) => {
            let detail = diagnostics
                .iter()
                .map(|d| format!("  {d:?}"))
                .collect::<Vec<_>>()
                .join("\n");
            panic!(
                "the shared development-contracts package no longer validates against its own \
                 manifest.\n\n{detail}\n\n\
                 If a diagnostic names a DIGEST: you edited a declared file and did not update its \
                 entry. The #216 wave rule requires the digest to move in the same PR as the \
                 content. Recompute and replace that entry's `sha256`:\n\
                 \n    sha256sum extensions/builtin/graphhelm-development-contracts/<path>\n\n\
                 If a diagnostic names the INVENTORY: you added a file under `policies/` or \
                 `fixtures/` without declaring it in `contributions[]`. Append your own entry with \
                 its digest -- append only, never edit another task's entry.\n\n\
                 This is the shared package: seven tasks write into it, so a stale entry is not \
                 yours alone to leave behind."
            );
        }
    }
}
