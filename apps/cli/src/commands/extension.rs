use std::path::Path;

use crate::output::Outcome;

pub fn run(package: &Path) -> Outcome {
    match graphhelm_schema::validate_extension_package(package) {
        Ok(package) => Outcome::success(
            "extension.validate",
            serde_json::json!({
                "id": package.id,
                "version": package.version,
                "contributionCount": package.contribution_count,
                "packageDigest": package.package_digest,
            }),
        ),
        Err(diagnostics) => Outcome::domain("extension.validate", diagnostics),
    }
}
