use std::collections::BTreeMap;
use std::path::Path;

use crate::output::Outcome;

use super::io::{ReadBudget, failure, load_catalog};

const COMMAND: &str = "schema.catalog";

pub(crate) fn run(catalog: &Path) -> Outcome {
    match execute(catalog) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(catalog: &Path) -> Result<serde_json::Value, super::io::Error> {
    let mut budget = ReadBudget::new();
    let loaded = load_catalog(catalog, &mut budget)?;
    let digests = loaded
        .resources
        .catalog
        .schemas
        .iter()
        .map(|(name, entry)| (name.clone(), entry.sha256.as_str().to_owned()))
        .collect::<BTreeMap<_, _>>();
    Ok(serde_json::json!({
        "releaseVersion": loaded.resources.catalog.release_version,
        "schemaCount": loaded.resources.catalog.schemas.len(),
        "digests": digests
    }))
}
