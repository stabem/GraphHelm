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

/// The plugin `graphhelm setup` enables beside the main one, and the marketplace that serves it.
pub const JOURNEY_REVIEW_PLUGIN: &str = "journey-review@graphhelm";
const MARKETPLACE: &str = "graphhelm";
const MARKETPLACE_REPO: &str = "stabem/GraphHelm";

/// Declares the `journey-review` plugin in Claude Code's user settings: `enabledPlugins` gets the
/// one key set true, and `extraKnownMarketplaces` gets the `graphhelm` marketplace when it is not
/// there. Claude Code installs a declared plugin from a declared marketplace on its next start, so
/// this is the whole install. Everything else is kept, and the call is idempotent: `None` means the
/// settings already say it, so there is nothing to decide. `apply` recomputes this same function to
/// refuse any other change to the settings file.
pub fn enable_journey_review(before: &Value) -> Result<Option<Value>, AdoptionError> {
    let mut after = before.clone();
    let map = after.as_object_mut().ok_or_else(super::invalid)?;
    let plugins = map
        .entry("enabledPlugins")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(super::invalid)?;
    // An explicit `false` is the owner saying no: it is never turned back on.
    match plugins.get(JOURNEY_REVIEW_PLUGIN) {
        Some(Value::Bool(false)) => return Err(super::invalid()),
        Some(_) => {}
        None => {
            plugins.insert(JOURNEY_REVIEW_PLUGIN.to_owned(), Value::Bool(true));
        }
    }
    let known = map
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(super::invalid)?;
    known.entry(MARKETPLACE).or_insert_with(
        || serde_json::json!({"source": {"source": "github", "repo": MARKETPLACE_REPO}}),
    );
    Ok((after != *before).then_some(after))
}
