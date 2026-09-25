use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    CatalogResources, MAX_CONFORMANCE_CASES, MAX_FILE_BYTES, MAX_JSON_DEPTH, MAX_RESOURCE_BYTES,
    MAX_SCHEMAS, SchemaCatalog, validate_catalog, validate_catalog_package,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::output::Outcome;

pub(super) struct ReadBudget {
    used: usize,
}

impl ReadBudget {
    pub(super) fn new() -> Self {
        Self { used: 0 }
    }

    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        if bytes > MAX_RESOURCE_BYTES.saturating_sub(self.used) {
            return Err(Error::Internal);
        }
        self.used += bytes;
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Repository {
    root: PathBuf,
}

pub(super) struct LoadedCatalog {
    pub(super) repository: Repository,
    pub(super) resources: CatalogResources,
    pub(super) validators: OfflineSchemaSet,
}

pub(super) enum Error {
    Domain(Vec<Diagnostic>),
    Internal,
}

impl Error {
    pub(super) fn domain(code: &str, message: &str, path: &str, source: &str) -> Self {
        Self::Domain(vec![Diagnostic::error(code, message, path, source)])
    }
}

pub(super) fn failure(command: &'static str, error: Error) -> Outcome {
    match error {
        Error::Domain(diagnostics) => Outcome::domain(command, diagnostics),
        Error::Internal => Outcome::internal(
            command,
            "schema resources could not be loaded or written safely",
        ),
    }
}

pub(super) fn load_catalog(path: &Path, budget: &mut ReadBudget) -> Result<LoadedCatalog, Error> {
    let catalog_path = canonical_file(path)?;
    let repository = Repository::discover(&catalog_path)?;
    load_catalog_at(repository, &catalog_path, budget)
}

pub(super) fn load_catalog_from_repository(
    repository: &Repository,
    relative: &str,
    budget: &mut ReadBudget,
) -> Result<LoadedCatalog, Error> {
    let path = repository.resolve_declared(
        relative,
        "GHC001_CATALOG_INVALID",
        "/catalog",
        "schema-catalog",
    )?;
    load_catalog_at(repository.clone(), &path, budget)
}

fn load_catalog_at(
    repository: Repository,
    catalog_path: &Path,
    budget: &mut ReadBudget,
) -> Result<LoadedCatalog, Error> {
    if !catalog_path.starts_with(&repository.root) {
        return Err(Error::Internal);
    }
    let catalog: SchemaCatalog = read_json(
        catalog_path,
        budget,
        "GHC001_CATALOG_INVALID",
        "/",
        "schema-catalog",
    )?;
    if catalog.schemas.len() > MAX_SCHEMAS {
        return Err(Error::domain(
            "GHC001_CATALOG_INVALID",
            "catalog exceeds maximum schema count",
            "/schemas",
            "schema-catalog",
        ));
    }
    let catalog_source = repository.relative_source(catalog_path);
    let package_diagnostics = validate_catalog_package(&catalog_source, &catalog);
    if !package_diagnostics.is_empty() {
        return Err(Error::Domain(package_diagnostics));
    }
    let mut schemas = BTreeMap::new();
    for (name, entry) in &catalog.schemas {
        let pointer = format!("/schemas/{}/path", escape_pointer(name));
        if !entry.path.starts_with("schemas/") {
            return Err(Error::domain(
                "GHC001_CATALOG_INVALID",
                "schema resource must remain below schemas/",
                &pointer,
                "schema-catalog",
            ));
        }
        let document = repository.read_declared_value(
            &entry.path,
            budget,
            "GHC001_CATALOG_INVALID",
            &pointer,
            "schema-catalog",
        )?;
        schemas.insert(name.clone(), document);
    }
    let resources = CatalogResources {
        catalog_source,
        catalog,
        schemas,
    };
    let report = validate_catalog(&resources);
    if !report.ok {
        return Err(Error::Domain(report.diagnostics));
    }
    let validators = OfflineSchemaSet::compile(resources.schemas.clone()).map_err(Error::Domain)?;
    Ok(LoadedCatalog {
        repository,
        resources,
        validators,
    })
}

impl Repository {
    fn discover(catalog: &Path) -> Result<Self, Error> {
        for ancestor in catalog.ancestors().skip(1) {
            let schemas = ancestor.join("schemas");
            let Ok(canonical_schemas) = fs::canonicalize(&schemas) else {
                continue;
            };
            let canonical_root = fs::canonicalize(ancestor).map_err(|_| Error::Internal)?;
            if canonical_schemas.starts_with(&canonical_root)
                && catalog.starts_with(&canonical_schemas)
            {
                return Ok(Self {
                    root: canonical_root,
                });
            }
        }
        Err(Error::Internal)
    }

    pub(super) fn same_root(&self, other: &Self) -> bool {
        self.root == other.root
    }

    pub(super) fn confine_argument(&self, path: &Path) -> Result<PathBuf, Error> {
        let path = canonical_file(path)?;
        path.starts_with(&self.root)
            .then_some(path)
            .ok_or(Error::Internal)
    }

    pub(super) fn read_confined_json<T: DeserializeOwned>(
        &self,
        path: &Path,
        budget: &mut ReadBudget,
        code: &str,
        source: &str,
    ) -> Result<T, Error> {
        let path = self.confine_argument(path)?;
        read_json(&path, budget, code, "/", source)
    }

    pub(super) fn read_declared_json<T: DeserializeOwned>(
        &self,
        relative: &str,
        budget: &mut ReadBudget,
        code: &str,
        path: &str,
        source: &str,
    ) -> Result<T, Error> {
        let resolved = self.resolve_declared(relative, code, path, source)?;
        read_json(&resolved, budget, code, path, source)
    }

    pub(super) fn read_declared_value(
        &self,
        relative: &str,
        budget: &mut ReadBudget,
        code: &str,
        path: &str,
        source: &str,
    ) -> Result<Value, Error> {
        self.read_declared_json(relative, budget, code, path, source)
    }

    pub(super) fn read_declared_text(
        &self,
        relative: &str,
        budget: &mut ReadBudget,
    ) -> Result<String, Error> {
        let path = self.resolve_declared(
            relative,
            "GHC001_CATALOG_INVALID",
            "/evidence/changelog",
            "schema-check",
        )?;
        let bytes = read_bytes(&path, budget)?;
        String::from_utf8(bytes).map_err(|_| Error::Internal)
    }

    pub(super) fn resolve_declared(
        &self,
        relative: &str,
        code: &str,
        path: &str,
        source: &str,
    ) -> Result<PathBuf, Error> {
        if !safe_relative(relative) {
            return Err(Error::domain(
                code,
                "resource path must be repository-relative and normalized",
                path,
                source,
            ));
        }
        let candidate = relative
            .split('/')
            .fold(self.root.clone(), |path, segment| path.join(segment));
        let canonical = canonical_file(&candidate)?;
        canonical
            .starts_with(&self.root)
            .then_some(canonical)
            .ok_or(Error::Internal)
    }

    pub(super) fn walk_json(&self, relative_directory: &str) -> Result<Vec<PathBuf>, Error> {
        if !safe_relative(relative_directory) {
            return Err(Error::Internal);
        }
        let directory = relative_directory
            .split('/')
            .fold(self.root.clone(), |path, segment| path.join(segment));
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let directory = fs::canonicalize(directory).map_err(|_| Error::Internal)?;
        if !directory.starts_with(&self.root) {
            return Err(Error::Internal);
        }
        let mut pending = vec![(directory, 0usize)];
        let mut files = Vec::new();
        let mut visited_directories = 0usize;
        let mut visited_entries = 0usize;
        while let Some((directory, depth)) = pending.pop() {
            visited_directories += 1;
            if visited_directories > MAX_CONFORMANCE_CASES || depth > MAX_JSON_DEPTH {
                return Err(Error::Internal);
            }
            let entries = fs::read_dir(directory).map_err(|_| Error::Internal)?;
            for entry in entries {
                visited_entries += 1;
                if visited_entries > MAX_CONFORMANCE_CASES {
                    return Err(Error::Internal);
                }
                let entry = entry.map_err(|_| Error::Internal)?;
                let file_type = entry.file_type().map_err(|_| Error::Internal)?;
                let entry_path = entry.path();
                let is_json = entry_path.extension().is_some_and(|value| value == "json");
                if file_type.is_dir() {
                    let path = fs::canonicalize(entry_path).map_err(|_| Error::Internal)?;
                    if !path.starts_with(&self.root) {
                        return Err(Error::Internal);
                    }
                    pending.push((path, depth + 1));
                } else if is_json && (file_type.is_file() || file_type.is_symlink()) {
                    let path = canonical_file(&entry_path)?;
                    if !path.starts_with(&self.root) {
                        return Err(Error::Internal);
                    }
                    files.push(path);
                    if files.len() > MAX_CONFORMANCE_CASES {
                        return Err(Error::Internal);
                    }
                }
            }
        }
        files.sort();
        Ok(files)
    }

    fn relative_source(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .filter(|relative| safe_relative(relative))
            .unwrap_or_else(|| "schema-catalog".into())
    }
}

pub(super) fn read_argument_value(
    path: &Path,
    budget: &mut ReadBudget,
    code: &str,
    source: &str,
) -> Result<(PathBuf, Value), Error> {
    let path = canonical_file(path)?;
    let value = read_json(&path, budget, code, "/", source)?;
    Ok((path, value))
}

fn read_json<T: DeserializeOwned>(
    path: &Path,
    budget: &mut ReadBudget,
    code: &str,
    diagnostic_path: &str,
    source: &str,
) -> Result<T, Error> {
    let bytes = read_bytes(path, budget)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::domain(code, "JSON resource is invalid", diagnostic_path, source))
}

fn read_bytes(path: &Path, budget: &mut ReadBudget) -> Result<Vec<u8>, Error> {
    let file = File::open(path).map_err(|_| Error::Internal)?;
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Internal)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Internal);
    }
    budget.charge(bytes.len())?;
    Ok(bytes)
}

fn canonical_file(path: &Path) -> Result<PathBuf, Error> {
    let canonical = fs::canonicalize(path).map_err(|_| Error::Internal)?;
    canonical
        .is_file()
        .then_some(canonical)
        .ok_or(Error::Internal)
}

fn safe_relative(value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path.components().all(|component| match component {
            Component::Normal(segment) => !segment.is_empty(),
            _ => false,
        })
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
