use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "graphhelm-extension-capability-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn replace_opened_file(path: &Path, replacement: &[u8]) -> bool {
    let retained = path.with_extension("retained");
    match fs::rename(path, &retained) {
        Ok(()) => {
            fs::write(path, replacement).unwrap();
            true
        }
        Err(error) if cfg!(windows) => {
            assert!(
                matches!(error.raw_os_error(), Some(5 | 32)),
                "Windows may deny replacement only because a retained handle blocks delete sharing: {error}"
            );
            false
        }
        Err(error) => panic!("opened file must remain replaceable by name on Unix: {error}"),
    }
}

#[test]
fn accepted_development_package_keeps_its_verdict_and_manifest_evidence_after_extraction() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts");
    assert!(
        package.join(MANIFEST_NAME).is_file(),
        "arrangement check: expected the committed development package at {}",
        package.display()
    );

    // This is the package verdict boundary that existed before contribution exposure: validate
    // the package and retain the manifest plus package records, without running the later
    // `validated_contributions` extraction step.
    let mut reference_diagnostics = DiagnosticCollector::default();
    let (manifest, records) = validate_package(&package, &mut reference_diagnostics)
        .expect("the committed package must pass the pre-extraction validation boundary");
    assert!(
        reference_diagnostics.into_diagnostics().is_empty(),
        "an accepted reference verdict must not retain diagnostics"
    );

    let expected_count = records.len();
    let expected_digest = package_digest(&manifest, &records);
    let declared = manifest
        .pointer("/spec/contracts/contributions")
        .and_then(serde_json::Value::as_array)
        .expect("the accepted manifest must declare its contribution array");
    let expected_authority = declared
        .iter()
        .map(|contribution| {
            let strings = |pointer: &str| {
                contribution
                    .pointer(pointer)
                    .map_or_else(Vec::new, |value| {
                        value
                            .as_array()
                            .expect("validated authority must be an array")
                            .iter()
                            .map(|member| {
                                member
                                    .as_str()
                                    .expect("validated authority members must be strings")
                                    .to_owned()
                            })
                            .collect::<Vec<_>>()
                    })
            };
            (
                contribution["id"]
                    .as_str()
                    .expect("a validated contribution must have an id")
                    .to_owned(),
                strings("/surfaces"),
                strings("/effects"),
                strings("/permissions"),
                strings("/requires/capabilities"),
            )
        })
        .collect::<Vec<_>>();

    let validated = validate_extension_package(&package)
        .expect("contribution extraction must not flip an accepted package verdict");
    let extracted_authority = validated
        .contributions
        .iter()
        .map(|contribution| {
            (
                contribution.id.clone(),
                contribution.surfaces.clone(),
                contribution.effects.clone(),
                contribution.permissions.clone(),
                contribution.required_capabilities.clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(validated.contribution_count, expected_count);
    assert_eq!(validated.package_digest, expected_digest);
    assert_eq!(validated.contributions.len(), declared.len());
    assert_eq!(extracted_authority, expected_authority);
}

#[test]
fn manifest_is_parsed_from_the_captured_file_handle() {
    let directory = TestDirectory::new("manifest-snapshot");
    let manifest_path = directory.0.join(MANIFEST_NAME);
    fs::write(
        &manifest_path,
        include_bytes!("../../../extensions/builtin/graphhelm-jpd/extension.json"),
    )
    .unwrap();
    let package = PackageCapability::open(&directory.0).unwrap();
    let mut manifest = package.open_manifest().unwrap();

    let replaced = replace_opened_file(&manifest_path, b"not json");
    let parsed = parse_opened_manifest(&mut manifest).unwrap();

    assert_eq!(parsed["metadata"]["id"], "graphhelm-jpd");
    if replaced {
        assert_eq!(fs::read(&manifest_path).unwrap(), b"not json");
    }
}

#[test]
fn retained_root_can_be_enumerated_after_manifest_capture() {
    let directory = TestDirectory::new("manifest-then-inventory");
    fs::write(
        directory.0.join(MANIFEST_NAME),
        include_bytes!("../../../extensions/builtin/graphhelm-jpd/extension.json"),
    )
    .unwrap();
    fs::write(directory.0.join("README.md"), b"read me").unwrap();
    let package = PackageCapability::open(&directory.0).unwrap();
    let manifest = package.open_manifest().unwrap();
    drop(manifest);

    let names = enumerate_directory(&package.root, MAX_INVENTORY_ENTRIES).unwrap();

    assert_eq!(names.len(), 2);
}

#[test]
fn empty_retained_directory_enumerates_as_empty() {
    let directory = TestDirectory::new("empty-inventory");
    let package = PackageCapability::open(&directory.0).unwrap();

    let names = enumerate_directory(&package.root, MAX_INVENTORY_ENTRIES).unwrap();

    assert!(names.is_empty());
}

#[test]
fn inventory_uses_the_retained_root_instead_of_a_replacement_name() {
    let directory = TestDirectory::new("retained-inventory-root");
    fs::write(directory.0.join("README.md"), b"captured inventory").unwrap();
    let package = PackageCapability::open(&directory.0).unwrap();
    let retained = directory.0.with_extension("retained-root");
    let replaced = match fs::rename(&directory.0, &retained) {
        Ok(()) => {
            fs::create_dir(&directory.0).unwrap();
            fs::write(directory.0.join("rogue.txt"), b"replacement inventory").unwrap();
            true
        }
        Err(error) if cfg!(windows) => {
            assert!(
                matches!(error.raw_os_error(), Some(5 | 32)),
                "Windows may deny root replacement only because the retained root blocks delete sharing: {error}"
            );
            false
        }
        Err(error) => panic!("retained root must remain replaceable by name on Unix: {error}"),
    };
    let mut diagnostics = DiagnosticCollector::default();

    validate_inventory(&package, &BTreeSet::new(), &mut diagnostics);

    assert!(diagnostics.into_diagnostics().is_empty());
    drop(package);
    if replaced {
        fs::remove_dir_all(&directory.0).unwrap();
        fs::rename(retained, &directory.0).unwrap();
    }
}

#[test]
fn opened_resource_never_reads_a_replacement_at_the_same_name() {
    let directory = TestDirectory::new("resource-snapshot");
    fs::create_dir(directory.0.join("skills")).unwrap();
    let resource_path = directory.0.join("skills/journey.md");
    fs::write(&resource_path, b"captured resource").unwrap();
    let package = PackageCapability::open(&directory.0).unwrap();
    let mut resource = package.open_regular("skills/journey.md").unwrap();

    let replaced = replace_opened_file(&resource_path, b"replacement resource");
    let bytes = resource.read_bounded(MAX_RESOURCE_BYTES).unwrap();

    assert_eq!(bytes, b"captured resource");
    if replaced {
        assert_eq!(fs::read(&resource_path).unwrap(), b"replacement resource");
    }
}

#[test]
fn ancestor_link_cannot_escape_the_package_capability() {
    let directory = TestDirectory::new("ancestor-link");
    let outside = TestDirectory::new("ancestor-outside");
    fs::write(outside.0.join("journey.md"), b"outside").unwrap();
    if !create_directory_link(&outside.0, &directory.0.join("skills")) {
        return;
    }
    let package = PackageCapability::open(&directory.0).unwrap();

    let error = package.open_regular("skills/journey.md").unwrap_err();

    assert!(matches!(error, CapabilityError::LinkedOrWrongType));
}

#[test]
fn inventory_rejects_a_linked_contribution_namespace() {
    let directory = TestDirectory::new("inventory-link");
    let outside = TestDirectory::new("inventory-outside");
    if !create_directory_link(&outside.0, &directory.0.join("skills")) {
        return;
    }
    let package = PackageCapability::open(&directory.0).unwrap();
    let mut diagnostics = DiagnosticCollector::default();

    validate_inventory(&package, &BTreeSet::new(), &mut diagnostics);

    assert!(
        diagnostics
            .into_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "GHEX004_PATH")
    );
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).unwrap();
    true
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(1314) => false,
        Err(error) => panic!("cannot create test directory link: {error}"),
    }
}
