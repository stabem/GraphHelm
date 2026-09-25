use std::path::Path;

use graphhelm_schema_evolution::schema_digest;

use crate::output::Outcome;

use super::io::{Error, ReadBudget, failure, read_argument_value};

const COMMAND: &str = "schema.digest";

/// Prints the canonical digest of one schema document — the same function the catalog
/// verifier recomputes before refusing with GHC002_HASH_MISMATCH. This command exists so
/// nobody canonicalizes JSON by hand in a shell (or writes a throwaway test) to learn a
/// digest: one document in, one `sha256:...` out, no catalog context required.
pub(crate) fn run(file: &Path) -> Outcome {
    match execute(file) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(file: &Path) -> Result<serde_json::Value, Error> {
    let mut budget = ReadBudget::new();
    let (_path, document) =
        read_argument_value(file, &mut budget, "GHC001_CATALOG_INVALID", "schema-digest")?;
    let digest =
        schema_digest(&document).map_err(|error| Error::Domain(vec![error.diagnostic()]))?;
    Ok(serde_json::json!({ "digest": digest }))
}
