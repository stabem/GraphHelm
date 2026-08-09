use std::path::Path;

use graphhelm_schema_evolution::canonical_view;

use crate::output::Outcome;

use super::io::{Error, ReadBudget, failure, load_catalog};

const COMMAND: &str = "schema.view";

pub(crate) fn run(catalog: &Path, schema: &str) -> Outcome {
    match execute(catalog, schema) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(catalog: &Path, schema: &str) -> Result<serde_json::Value, Error> {
    let mut budget = ReadBudget::new();
    let loaded = load_catalog(catalog, &mut budget)?;
    let view = canonical_view(&loaded.resources, schema).map_err(Error::Domain)?;
    serde_json::to_value(view).map_err(|_| Error::Internal)
}
