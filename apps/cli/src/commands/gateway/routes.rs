//! `graphhelm gateway routes --manifest <file>`: loads and validates a route manifest, then
//! reports every route it declares. A manifest that fails to load or validate never reaches this
//! far — `super::load_manifest` already turned that into a `GHCLI009_GATEWAY_INVALID` failure that
//! carries only `ManifestError`'s own route-id-and-rule text, never the file's bytes.

use std::path::Path;

use graphhelm_gateway::manifest::ModelRoute;
use serde_json::json;

use super::{Failure, finish, load_manifest};
use crate::output::Outcome;

const COMMAND: &str = "gateway.routes";

pub(in crate::commands) fn run(manifest: &Path) -> Outcome {
    finish(
        COMMAND,
        execute(manifest),
        |routes| json!({ "routes": routes }),
    )
}

fn execute(manifest: &Path) -> Result<Vec<serde_json::Value>, Failure> {
    let manifest = load_manifest(manifest)?;
    Ok(manifest.routes().iter().map(render_route).collect())
}

/// `Transport`, `BillingMode` and `WorkProfile` already derive `Serialize` with
/// `#[serde(rename_all = "snake_case")]` (`core/gateway/src/manifest.rs`), so embedding them
/// directly in `json!` reuses that wire vocabulary instead of hand-maintaining a second copy of it
/// here.
fn render_route(route: &ModelRoute) -> serde_json::Value {
    json!({
        "id": route.id(),
        "provider": route.provider(),
        "transport": route.transport(),
        "billingMode": route.billing_mode(),
        "authentication": route.authentication(),
        "baseUrl": route.base_url(),
        "credentialRef": route.credential_ref(),
        // The model NAME, which is what a person choosing a route is actually choosing. Without it
        // this listing can only offer route ids - deployer-chosen strings like `fast_route` that
        // say nothing about what would answer - and a picker built on ids alone asks an operator
        // to pick a model they cannot see. `null` where the manifest declares none, rather than
        // an invented default: the absence is the manifest's, and hiding it would be the listing
        // answering a question the manifest did not.
        "model": route.model(),
        "profiles": route.profiles(),
        "enabled": route.enabled(),
    })
}
