//! Pinned local Extension packages. Host wrappers never acquire a second package identity.
use super::invalid;
use crate::{
    journal,
    storage::{self, Root},
};
use graphhelm_extension_host::{ActivationClaim, active_versions, install_package, switch_active};
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedPackage {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub path: PathBuf,
}

/// The versioned local bundle contains both authoritative packages. Packaged distributions place
/// `extensions` next to the executable; a source checkout uses its checked-in release bundle.
pub fn release_packages() -> Result<Vec<PinnedPackage>, AdoptionError> {
    let local = std::env::current_exe()
        .map_err(|_| invalid())?
        .parent()
        .ok_or_else(invalid)?
        .join("extensions/releases/adoption-0.1.0.json");
    let bundle = if local.is_file() {
        local
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .ok_or_else(invalid)?
            .join("extensions/releases/adoption-0.1.0.json")
    };
    let release = Root::observe(bundle.parent().ok_or_else(invalid)?)?;
    let bytes =
        storage::read_child(&release.file, "adoption-0.1.0.json", 16384)?.ok_or_else(invalid)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if value["version"] != "0.1.0" {
        return Err(invalid());
    }
    let mut packages: Vec<PinnedPackage> =
        serde_json::from_value(value["packages"].clone()).map_err(|_| invalid())?;
    for package in &mut packages {
        package.path = bundle.parent().ok_or_else(invalid)?.join(&package.path);
    }
    Ok(packages)
}

pub(crate) fn prepare_packages(
    plan: &Value,
    overrides: &[PathBuf],
) -> Result<Vec<PinnedPackage>, AdoptionError> {
    let pins = plan["spec"]["packages"].as_array().ok_or_else(invalid)?;
    if pins.is_empty() {
        if !overrides.is_empty() {
            return Err(invalid());
        }
        return Ok(Vec::new());
    }
    if pins.len() != 2
        || !pins.iter().any(|p| p["id"] == "graphhelm-jpd")
        || !pins
            .iter()
            .any(|p| p["id"] == "graphhelm-development-contracts")
    {
        return Err(invalid());
    }
    let mut sources = if overrides.is_empty() {
        release_packages()?
    } else {
        if overrides.len() != 2 {
            return Err(invalid());
        }
        overrides
            .iter()
            .map(|path| {
                let package =
                    graphhelm_schema::validate_extension_package(path).map_err(|_| invalid())?;
                Ok(PinnedPackage {
                    id: package.id,
                    version: package.version,
                    digest: package.package_digest,
                    path: path.clone(),
                })
            })
            .collect::<Result<Vec<_>, AdoptionError>>()?
    };
    for pin in pins {
        let source = sources
            .iter_mut()
            .find(|p| pin["id"] == p.id)
            .ok_or_else(invalid)?;
        let current = graphhelm_schema::validate_extension_package(&source.path).map_err(|_| {
            AdoptionError {
                reason: AdoptionReason::PlanStale,
            }
        })?;
        if pin["version"] != current.version
            || pin["digest"] != current.package_digest
            || source.digest != current.package_digest
            || source.id != current.id
        {
            return Err(AdoptionError {
                reason: AdoptionReason::PlanStale,
            });
        }
        source.path = std::fs::canonicalize(&source.path).map_err(|_| invalid())?;
    }
    Ok(sources)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackageEntry {
    pub id: String,
    pub digest: String,
    pub phase: journal::Phase,
    pub root: Option<storage::RootRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_digest: Option<String>,
}

pub(crate) fn prepare_entries(packages: &[PinnedPackage]) -> Vec<PackageEntry> {
    packages
        .iter()
        .map(|p| PackageEntry {
            id: p.id.clone(),
            digest: p.digest.clone(),
            phase: journal::Phase::Planned,
            root: None,
            activation_digest: None,
        })
        .collect()
}

pub(crate) fn create_package_root(
    store: &journal::Store,
    transaction: &str,
    entry: &mut PackageEntry,
) -> Result<(), AdoptionError> {
    let root = store
        .root
        .create_private_child(&format!("{transaction}-{}", entry.id))?;
    entry.root = Some(root.record);
    Ok(())
}

pub(crate) fn install(
    entry: &PackageEntry,
    package: &PinnedPackage,
) -> Result<String, AdoptionError> {
    let root = Root::reopen(entry.root.as_ref().ok_or_else(storage::failed)?)?;
    let claim = ActivationClaim::acquire(&root.record.path).map_err(|_| storage::failed())?;
    if active_versions(claim.install_root())
        .map_err(|_| storage::failed())?
        .is_some()
    {
        return Err(storage::failed());
    }
    let installed = install_package(&claim, &package.path).map_err(|_| storage::failed())?;
    if installed.digest != package.digest {
        return Err(AdoptionError {
            reason: AdoptionReason::PlanStale,
        });
    }
    switch_active(&claim, &package.digest).map_err(|_| storage::failed())?;
    root.verify()?;
    Ok(crate::apply::digest(
        &storage::read_child(&root.file, "active.json", 4096)?.ok_or_else(storage::failed)?,
    ))
}

pub(crate) fn verify_installed(
    store: &journal::Store,
    record: &journal::Journal,
) -> Result<(), AdoptionError> {
    for package in &record.packages {
        verify_root(store, record, package)?;
        let root = Root::reopen(package.root.as_ref().ok_or_else(storage::failed)?)?;
        if package.activation_digest.as_deref()
            != Some(&crate::apply::digest(
                &storage::read_child(&root.file, "active.json", 4096)?
                    .ok_or_else(storage::failed)?,
            ))
        {
            return Err(storage::failed());
        }
        let active = active_versions(&root.record.path)
            .map_err(|_| storage::failed())?
            .ok_or_else(storage::failed)?;
        if active.current != package.digest || active.previous.is_some() {
            return Err(storage::failed());
        }
        let path = root
            .record
            .path
            .join("versions")
            .join(package.digest.replace(':', "-"));
        let verified =
            graphhelm_schema::validate_extension_package(&path).map_err(|_| storage::failed())?;
        if verified.package_digest != package.digest {
            return Err(storage::failed());
        }
    }
    Ok(())
}

pub(crate) fn compensate_packages(
    store: &journal::Store,
    record: &journal::Journal,
) -> Result<(), AdoptionError> {
    preflight_compensation(store, record)?;
    for package in record.packages.iter().rev() {
        if package.phase == journal::Phase::Planned || package.root.is_none() {
            continue;
        }
        verify_root(store, record, package)?;
        let root = Root::reopen(package.root.as_ref().ok_or_else(storage::failed)?)?;
        let claim = ActivationClaim::acquire(&root.record.path).map_err(|_| storage::failed())?;
        if let Some(active) =
            active_versions(claim.install_root()).map_err(|_| storage::failed())?
        {
            if active.current != package.digest || active.previous.is_some() {
                return Err(storage::failed());
            }
            let bytes = storage::read_child(&root.file, "active.json", 4096)?
                .ok_or_else(storage::failed)?;
            root.retire_owned(
                "active.json",
                "inactive.json",
                &crate::apply::digest(&bytes),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn preflight_compensation(
    store: &journal::Store,
    record: &journal::Journal,
) -> Result<(), AdoptionError> {
    for package in &record.packages {
        if package.phase == journal::Phase::Planned || package.root.is_none() {
            continue;
        }
        verify_root(store, record, package)?;
        let root = Root::reopen(package.root.as_ref().ok_or_else(storage::failed)?)?;
        let active = storage::read_child(&root.file, "active.json", 4096)?;
        let inactive = storage::read_child(&root.file, "inactive.json", 4096)?;
        if active.is_some() && inactive.is_some() {
            return Err(storage::failed());
        }
        if let Some(expected) = &package.activation_digest {
            let bytes = active.or(inactive).ok_or_else(storage::failed)?;
            if crate::apply::digest(&bytes) != *expected {
                return Err(storage::failed());
            }
            let path = root
                .record
                .path
                .join("versions")
                .join(package.digest.replace(':', "-"));
            if graphhelm_schema::validate_extension_package(&path)
                .map_err(|_| storage::failed())?
                .package_digest
                != package.digest
            {
                return Err(storage::failed());
            }
        } else if package.phase == journal::Phase::Published {
            return Err(storage::failed());
        }
    }
    Ok(())
}

pub(crate) fn package_receipts(store: &journal::Store, record: &journal::Journal) -> Value {
    json!(record.packages.iter().map(|p| json!({"id":p.id,"digest":p.digest,"path":store.root.record.path.join(format!("{}-{}",record.transaction_id,p.id)).join("versions").join(p.digest.replace(':',"-"))})).collect::<Vec<_>>())
}

fn verify_root(
    store: &journal::Store,
    record: &journal::Journal,
    package: &PackageEntry,
) -> Result<(), AdoptionError> {
    if package.root.as_ref().ok_or_else(storage::failed)?.path
        != store
            .root
            .record
            .path
            .join(format!("{}-{}", record.transaction_id, package.id))
    {
        return Err(storage::failed());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intent_before_private_root_registration_never_claims_existing_state() {
        let value = json!({"id":"graphhelm-jpd","digest":format!("sha256:{}","0".repeat(64)),"phase":"intent","root":null});
        let entry: PackageEntry =
            serde_json::from_value(value).expect("a durable intent precedes directory creation");
        let temp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let store = journal::Store::open(Root::open(temp.path(), false).unwrap()).unwrap();
        let record = journal::Journal {
            predecessor: None,
            restore: None,
            version: 1,
            transaction_id: "0".repeat(64),
            sequence: 1,
            plan_digest: format!("sha256:{}", "0".repeat(64)),
            backup_id: "0".repeat(64),
            project: store.root.record.clone(),
            home: store.root.record.clone(),
            entries: vec![],
            packages: vec![entry],
            state: graphhelm_protocols::adoption::TransactionState::Applying,
            receipt: None,
        };
        assert!(compensate_packages(&store, &record).is_ok());
    }
}
