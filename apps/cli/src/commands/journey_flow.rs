//! Bounded journey-flow source files; the frozen JSON contract remains the reader boundary.
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;

use super::journey_validate::Finding;

const FLOW_SCHEMA_ID: &str = "https://p50.dev/schemas/journey-flow.schema.json";
const MAX_FLOW_BYTES: u64 = 32 * 1024;

fn flow_schemas() -> Option<&'static OfflineSchemaSet> {
    static SCHEMAS: OnceLock<Option<OfflineSchemaSet>> = OnceLock::new();
    SCHEMAS
        .get_or_init(|| {
            let document =
                serde_json::from_str(include_str!("../../../../schemas/journey-flow.schema.json"))
                    .ok()?;
            OfflineSchemaSet::compile(BTreeMap::from([(FLOW_SCHEMA_ID.to_owned(), document)])).ok()
        })
        .as_ref()
}

pub(crate) fn check(file: &Path, _project: &Path) -> Vec<Finding> {
    let Ok(metadata) = std::fs::metadata(file) else {
        return vec![Finding::new("flow.not_yaml", "", "flow could not be read")];
    };
    if metadata.len() > MAX_FLOW_BYTES {
        return vec![Finding::new("flow.too_large", "", "flow exceeds 32 KiB")];
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return vec![Finding::new(
            "flow.not_yaml",
            "",
            "flow is not readable UTF-8",
        )];
    };
    let Ok(value) = serde_yaml_ng::from_str::<Value>(&text) else {
        return vec![Finding::new(
            "flow.not_yaml",
            "",
            "flow is not a YAML document",
        )];
    };
    let Some(schemas) = flow_schemas() else {
        return vec![Finding::new(
            "flow.schema_invalid",
            "",
            "flow schema unavailable",
        )];
    };
    schemas
        .validate(FLOW_SCHEMA_ID, &value, "journey-flow")
        .into_iter()
        .map(|d| Finding::new("flow.schema_invalid", d.path, d.message))
        .collect()
}
