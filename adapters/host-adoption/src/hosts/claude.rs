use super::HostOperation;
use graphhelm_protocols::adoption::AdoptionError;
use serde_json::Value;
use std::path::Path;

/// Argument construction never grants execution authority. The runner validates IDs and scope.
pub fn plugin_install(program: &Path, plugin_id: &str, scope: &str) -> HostOperation {
    HostOperation {
        program: program.into(),
        args: ["plugin", "install", plugin_id, "--scope", scope]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        timeout_ms: 10_000,
    }
}

/// Edit only the explicitly accepted plugin keys. Hook and permission values are opaque data.
pub fn disable_plugins(
    before: &Value,
    accepted: &[String],
    managed: &Value,
) -> Result<Value, AdoptionError> {
    let mut after = before.clone();
    if accepted.is_empty() {
        return Ok(after);
    }
    let plugins = after
        .get_mut("enabledPlugins")
        .and_then(Value::as_object_mut)
        .ok_or_else(super::invalid)?;
    for id in accepted {
        if !super::valid_id(id)
            || managed["enabledPlugins"].get(id).is_some()
            || plugins.get(id).and_then(Value::as_bool) != Some(true)
        {
            return Err(super::invalid());
        }
        plugins.insert(id.clone(), Value::Bool(false));
    }
    Ok(after)
}
