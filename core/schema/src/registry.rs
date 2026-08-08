use graphhelm_protocols::Diagnostic;
use jsonschema::{Draft, Registry, Validator};

const GRAPH_SCHEMA: &str = include_str!("../../../schemas/graph.schema.json");
const NODE_SCHEMA: &str = include_str!("../../../schemas/node.schema.json");
const EDGE_SCHEMA: &str = include_str!("../../../schemas/edge.schema.json");
const AGENT_SCHEMA: &str = include_str!("../../../schemas/agent.schema.json");
const WAIVER_SCHEMA: &str = include_str!("../../../schemas/policy-waiver.schema.json");

const GRAPH_ID: &str = "https://p50.dev/schemas/graph.schema.json";
const NODE_ID: &str = "https://p50.dev/schemas/node.schema.json";
const EDGE_ID: &str = "https://p50.dev/schemas/edge.schema.json";
const AGENT_ID: &str = "https://p50.dev/schemas/agent.schema.json";
const WAIVER_ID: &str = "https://p50.dev/schemas/policy-waiver.schema.json";

/// Validates a raw graph using only embedded checked-in schema resources.
#[must_use]
pub fn validate_graph_value(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate(value, source, GRAPH_SCHEMA)
}

/// Validates a policy waiver using the embedded checked-in waiver schema.
#[must_use]
pub fn validate_waiver(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate(value, source, WAIVER_SCHEMA)
}

fn validate(value: &serde_json::Value, source: &str, root: &str) -> Vec<Diagnostic> {
    let graph = parse_schema(GRAPH_SCHEMA);
    let node = parse_schema(NODE_SCHEMA);
    let edge = parse_schema(EDGE_SCHEMA);
    let agent = parse_schema(AGENT_SCHEMA);
    let waiver = parse_schema(WAIVER_SCHEMA);

    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(GRAPH_ID, &graph)
        .and_then(|builder| builder.add(NODE_ID, &node))
        .and_then(|builder| builder.add(EDGE_ID, &edge))
        .and_then(|builder| builder.add(AGENT_ID, &agent))
        .and_then(|builder| builder.add(WAIVER_ID, &waiver))
        .and_then(|builder| builder.prepare());

    let Ok(registry) = registry else {
        return vec![Diagnostic::error(
            "GHS002_SCHEMA",
            "embedded schema registry is invalid",
            "/",
            source,
        )];
    };

    let schema = if root == GRAPH_SCHEMA {
        &graph
    } else {
        &waiver
    };
    let validator: Result<Validator, _> = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(schema);
    let Ok(validator) = validator else {
        return vec![Diagnostic::error(
            "GHS002_SCHEMA",
            "embedded root schema is invalid",
            "/",
            source,
        )];
    };

    let mut diagnostics: Vec<_> = validator
        .iter_errors(value)
        .map(|error| {
            let path = error.instance_path().as_str();
            Diagnostic::error(
                "GHS002_SCHEMA",
                error.to_string(),
                if path.is_empty() { "/" } else { path },
                source,
            )
        })
        .collect();
    diagnostics.sort_by(|left, right| {
        (&left.path, &left.code, &left.message).cmp(&(&right.path, &right.code, &right.message))
    });
    diagnostics
}

fn parse_schema(source: &str) -> serde_json::Value {
    serde_json::from_str(source).expect("checked-in JSON schemas must parse")
}

#[cfg(test)]
mod tests {
    use jsonschema::{Draft, Registry};

    #[test]
    fn unresolved_reference_fails_during_local_registry_preparation() {
        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://p50.dev/schemas/test-local-only.json",
            "$ref": "https://p50.dev/schemas/never-registered.json"
        });
        let result = Registry::new()
            .draft(Draft::Draft202012)
            .add("https://p50.dev/schemas/test-local-only.json", &schema)
            .and_then(|builder| builder.prepare());
        assert!(result.is_err());
    }
}
