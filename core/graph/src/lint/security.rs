use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if let Ok(serde_json::Value::Object(root)) = serde_json::to_value(graph) {
        for (key, value) in root {
            inspect(
                &key,
                &value,
                &format!("/{}", escape(&key)),
                source,
                &mut diagnostics,
            );
        }
    }
    diagnostics
}

fn inspect(
    key: &str,
    value: &serde_json::Value,
    path: &str,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_secret_key(key) && !is_reference(key, value) {
        diagnostics.push(error(
            "GHG008_INLINE_SECRET",
            "secret-shaped value must use an external reference",
            path,
            source,
        ));
    }
    match value {
        serde_json::Value::Object(object) => {
            for (child_key, child) in object {
                inspect(
                    child_key,
                    child,
                    &format!("{path}/{}", escape(child_key)),
                    source,
                    diagnostics,
                );
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                inspect(key, child, &format!("{path}/{index}"), source, diagnostics);
            }
        }
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password" | "secret" | "token" | "api_key" | "apikey" | "private_key" | "privatekey"
    )
}

fn is_reference(key: &str, value: &serde_json::Value) -> bool {
    if key.ends_with("Ref") || key.ends_with("_ref") {
        return true;
    }
    value.as_str().is_some_and(|text| {
        ["secret://", "environment://", "env://"]
            .iter()
            .any(|prefix| text.starts_with(prefix))
    })
}
