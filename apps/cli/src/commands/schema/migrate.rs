use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use graphhelm_schema_evolution::{
    MAX_FILE_BYTES, MigrationCatalogs, MigrationManifest, apply_migration, schema_digest,
    validate_migration_manifest,
};

use crate::output::Outcome;

use super::io::{
    Error, ReadBudget, failure, load_catalog, load_catalog_from_repository, read_argument_value,
};

const COMMAND: &str = "schema.migrate";

pub(crate) fn run(catalog: &Path, migration: &Path, input: &Path, output: &Path) -> Outcome {
    match execute(catalog, migration, input, output) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(
    catalog: &Path,
    migration_path: &Path,
    input_path: &Path,
    output_path: &Path,
) -> Result<serde_json::Value, Error> {
    let mut budget = ReadBudget::new();
    let target = load_catalog(catalog, &mut budget)?;
    let manifest: MigrationManifest = target.repository.read_confined_json(
        migration_path,
        &mut budget,
        "GHM001_MIGRATION_UNSUPPORTED",
        "schema-migration",
    )?;
    validate_migration_manifest(&manifest, None, &target.resources.catalog)
        .map_err(|diagnostic| Error::Domain(vec![diagnostic]))?;
    let target_entry = target
        .resources
        .catalog
        .schemas
        .get(&manifest.schema)
        .ok_or_else(|| unsupported("target schema is unavailable", "/schema"))?;
    let source_relative = format!("schemas/releases/{}/catalog.json", manifest.from_version);
    let source = load_catalog_from_repository(&target.repository, &source_relative, &mut budget)?;
    if !source.repository.same_root(&target.repository) {
        return Err(Error::Internal);
    }

    let (input, document) = read_argument_value(
        input_path,
        &mut budget,
        "GHM001_MIGRATION_UNSUPPORTED",
        "schema-migration",
    )?;
    let output = destination(&input, output_path)?;
    let source_entry = source
        .resources
        .catalog
        .schemas
        .get(&manifest.schema)
        .ok_or_else(|| unsupported("source schema is unavailable", "/schema"))?;
    let result = apply_migration(
        &document,
        &manifest,
        &MigrationCatalogs {
            source: &source.resources.catalog,
            target: &target.resources.catalog,
        },
        |document| {
            source
                .validators
                .validate(&source_entry.id, document, "migration")
        },
        |document| {
            target
                .validators
                .validate(&target_entry.id, document, "migration")
        },
    );
    if !result.ok {
        return Err(Error::Domain(result.diagnostics));
    }
    let migrated = result.document.ok_or(Error::Internal)?;
    let source_digest =
        schema_digest(&document).map_err(|error| Error::Domain(vec![error.diagnostic()]))?;
    let output_digest =
        schema_digest(&migrated).map_err(|error| Error::Domain(vec![error.diagnostic()]))?;
    let bytes = serde_json::to_vec_pretty(&migrated).map_err(|_| Error::Internal)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::domain(
            "GHM003_PATCH_INVALID",
            "migration output exceeds the file-size limit",
            "/destination",
            "migration",
        ));
    }
    write_atomic(&output, &bytes)?;
    Ok(serde_json::json!({
        "schema": manifest.schema,
        "fromVersion": manifest.from_version,
        "toVersion": manifest.to_version,
        "sourceDigest": source_digest,
        "outputDigest": output_digest
    }))
}

fn destination(input: &Path, output: &Path) -> Result<PathBuf, Error> {
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|_| Error::Internal)?;
    let name = output
        .file_name()
        .ok_or_else(|| unsupported("migration output path is unavailable", "/output"))?;
    let output = parent.join(name);
    if input == output {
        return Err(unsupported(
            "migration input and output must be distinct",
            "/output",
        ));
    }
    if fs::symlink_metadata(&output).is_ok() {
        return Err(unsupported(
            "migration output must not already exist",
            "/output",
        ));
    }
    Ok(output)
}

fn write_atomic(output: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = output.parent().ok_or(Error::Internal)?;
    let temporary = parent.join(format!(".graphhelm-migrate-{}.tmp", uuid::Uuid::new_v4()));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| Error::Internal)?;
        created = true;
        file.write_all(bytes).map_err(|_| Error::Internal)?;
        file.sync_all().map_err(|_| Error::Internal)?;
        drop(file);
        if fs::symlink_metadata(output).is_ok() {
            return Err(unsupported(
                "migration output must remain absent until publication",
                "/output",
            ));
        }
        if rename_no_replace(&temporary, output).is_err() {
            return if fs::symlink_metadata(output).is_ok() {
                Err(unsupported(
                    "migration output must remain absent until publication",
                    "/output",
                ))
            } else {
                Err(Error::Internal)
            };
        }
        created = false;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated path strings for the duration of the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::MoveFileW;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both pointers address NUL-terminated UTF-16 path buffers for the duration of the
    // call. MoveFileW has no replacement flag and therefore atomically fails if the destination
    // already exists.
    let result = unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) };
    if result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn rename_no_replace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn unsupported(message: &str, path: &str) -> Error {
    Error::domain("GHM001_MIGRATION_UNSUPPORTED", message, path, "migration")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prevents the final publication primitive from replacing a destination created after the
    // earlier absence check.
    #[test]
    fn final_no_replace_primitive_preserves_an_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.tmp");
        let destination = directory.path().join("destination.json");
        fs::write(&source, b"candidate").unwrap();
        fs::write(&destination, b"sentinel").unwrap();

        assert!(rename_no_replace(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"sentinel");
        assert_eq!(fs::read(&source).unwrap(), b"candidate");
    }
}
