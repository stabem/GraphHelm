use std::path::Path;

use graphhelm_protocols::Diagnostic;

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

// CLI-level refusal, GHCLI0NN namespace (apps/cli's own, distinct from core/schema::extension's
// GHEX0NN validator codes -- 001-020 already taken there for package-content diagnostics; this
// command's refusals are about CLI arguments/files, matching GHCLI015_MCP_INVALID's sibling
// mcp.rs command exactly).
const MCP_TOKEN_INVALID: &str = "GHCLI020_MCP_TOKEN_INVALID";
const SOURCE: &str = "extension-mcp-token";

fn refuse(command: &'static str, message: &str, pointer: &str) -> Outcome {
    Outcome::domain(
        command,
        vec![Diagnostic::error(
            MCP_TOKEN_INVALID,
            message,
            pointer,
            SOURCE,
        )],
    )
}

/// #213: reads the package fresh (never a cached digest -- the SAME rule verification runs at
/// call time, applied here at mint time too, so a token is never minted with a digest already
/// stale relative to the package on disk), finds the one named contribution, and mints.
pub fn run_mint_mcp_token(
    package: &Path,
    contribution_id: &str,
    actor: &str,
    ttl_seconds: u64,
) -> Outcome {
    const COMMAND: &str = "extension.mint-mcp-token";
    let validated = match graphhelm_schema::validate_extension_package(package) {
        Ok(validated) => validated,
        Err(diagnostics) => return Outcome::domain(COMMAND, diagnostics),
    };
    let Some(contribution) = validated
        .contributions
        .iter()
        .find(|c| c.id == contribution_id)
    else {
        return refuse(
            COMMAND,
            "no contribution with this id is declared in the package",
            "/contribution",
        );
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(u64::MAX);
    let token = graphhelm_tool_broker::mcp_capability::mint(
        validated.package_digest,
        contribution.id.clone(),
        actor.to_owned(),
        &contribution.surfaces,
        &contribution.effects,
        now,
        ttl_seconds,
    );
    match serde_json::to_value(&token) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(_) => Outcome::internal(COMMAND, "the minted token could not be serialized"),
    }
}

pub fn run_revoke_mcp_token(token_file: &Path) -> Outcome {
    const COMMAND: &str = "extension.revoke-mcp-token";
    let bytes = match std::fs::read(token_file) {
        Ok(bytes) => bytes,
        Err(_) => {
            return refuse(
                COMMAND,
                "--token-file does not name a readable file",
                "/tokenFile",
            );
        }
    };
    let mut token: graphhelm_tool_broker::mcp_capability::McpCapabilityToken =
        match serde_json::from_slice(&bytes) {
            Ok(token) => token,
            Err(_) => {
                return refuse(
                    COMMAND,
                    "--token-file does not hold a valid capability token",
                    "/tokenFile",
                );
            }
        };
    token.revoked = true;
    match serde_json::to_value(&token) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(_) => Outcome::internal(COMMAND, "the revoked token could not be serialized"),
    }
}
