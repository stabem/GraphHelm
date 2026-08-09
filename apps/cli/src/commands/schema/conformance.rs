use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    ConformanceCase, ConformanceResources, ConformanceSuite, run_conformance,
};
use serde_json::Value;

use crate::output::Outcome;

use super::io::{Error, ReadBudget, failure, load_catalog};

const COMMAND: &str = "schema.conformance";

pub(crate) fn run(catalog: &Path, fixtures: &Path) -> Outcome {
    match execute(catalog, fixtures) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(catalog: &Path, fixtures: &Path) -> Result<Value, Error> {
    let mut budget = ReadBudget::new();
    let loaded = load_catalog(catalog, &mut budget)?;
    let suite: ConformanceSuite = loaded.repository.read_confined_json(
        fixtures,
        &mut budget,
        "GHCONF001_FIXTURE_FAILED",
        "schema-conformance",
    )?;

    let fixture_paths = suite
        .cases
        .iter()
        .flat_map(case_paths)
        .collect::<BTreeSet<_>>();
    let mut fixture_values = BTreeMap::new();
    for path in fixture_paths {
        let value = loaded.repository.read_declared_value(
            path,
            &mut budget,
            "GHCONF001_FIXTURE_FAILED",
            "/resources",
            "schema-conformance",
        )?;
        fixture_values.insert(path.to_owned(), value);
    }

    let mut versioned = BTreeMap::new();
    for (target, paths) in &suite.validator_resources {
        let mut resources = BTreeMap::new();
        for (index, path) in paths.iter().enumerate() {
            let document = loaded.repository.read_declared_value(
                path,
                &mut budget,
                "GHCONF001_FIXTURE_FAILED",
                &format!("/validatorResources/{index}"),
                "schema-conformance",
            )?;
            resources.insert(path.clone(), document);
        }
        validate_versioned_roots(&loaded, target, &resources)?;
        let validators = OfflineSchemaSet::compile(resources).map_err(Error::Domain)?;
        versioned.insert(target.clone(), validators);
    }

    let resources = ConformanceResources {
        fixtures: fixture_values,
    };
    let report = run_conformance(&suite, &resources, |target, document| {
        validate(&loaded, &versioned, target, document)
    });
    if !report.ok {
        return Err(Error::Domain(report.diagnostics));
    }
    serde_json::to_value(report).map_err(|_| Error::Internal)
}

fn validate_versioned_roots(
    loaded: &super::io::LoadedCatalog,
    target: &str,
    resources: &BTreeMap<String, Value>,
) -> Result<(), Error> {
    let (schema, version) = target
        .split_once('@')
        .ok_or_else(|| validator_binding_error(target, "versioned validator target is invalid"))?;
    let entry = loaded
        .resources
        .catalog
        .schemas
        .get(schema)
        .ok_or_else(|| validator_binding_error(target, "versioned validator schema is absent"))?;
    let roots = resources
        .values()
        .filter(|document| document.get("$id").and_then(Value::as_str) == Some(entry.id.as_str()))
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(validator_binding_error(
            target,
            "versioned validator must declare exactly one matching root schema",
        ));
    }
    if roots[0]
        .get("x-graphhelm-schema-version")
        .and_then(Value::as_str)
        != Some(version)
    {
        return Err(validator_binding_error(
            target,
            "versioned validator root version does not match its label",
        ));
    }
    Ok(())
}

fn validator_binding_error(target: &str, message: &str) -> Error {
    Error::domain(
        "GHCONF001_FIXTURE_FAILED",
        message,
        &format!("/validatorResources/{}", escape_pointer(target)),
        "schema-conformance",
    )
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn case_paths(case: &ConformanceCase) -> Vec<&str> {
    match case {
        ConformanceCase::Schema { input, .. }
        | ConformanceCase::Compatibility { input, .. }
        | ConformanceCase::Migration { input, .. } => vec![input],
        ConformanceCase::Release {
            input, comparison, ..
        } => comparison
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(input.as_str()))
            .collect(),
    }
}

fn validate(
    loaded: &super::io::LoadedCatalog,
    versioned: &BTreeMap<String, OfflineSchemaSet>,
    target: &str,
    document: &Value,
) -> Vec<Diagnostic> {
    let (schema, validators) = match target.split_once('@') {
        Some((schema, _)) => match versioned.get(target) {
            Some(validators) => (schema, validators),
            None => return unavailable_validator(),
        },
        None => (target, &loaded.validators),
    };
    let Some(entry) = loaded.resources.catalog.schemas.get(schema) else {
        return unavailable_validator();
    };
    validators.validate(&entry.id, document, "conformance")
}

fn unavailable_validator() -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        "GHS002_SCHEMA",
        "declared schema validator is unavailable",
        "/",
        "conformance",
    )]
}
