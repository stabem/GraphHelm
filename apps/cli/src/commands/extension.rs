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
const MCP_TOKEN_INVALID: &str = crate::error_codes::GHCLI020_MCP_TOKEN_INVALID;
const SOURCE: &str = "extension-mcp-token";

/// One code for the lifecycle's refusals (#212), because the consumer is a script's if-statement:
/// what varies per refusal is the MESSAGE, which names the exact variant, while the code answers
/// the only question the script asks -- "did the lifecycle refuse?". Splitting codes per variant
/// would force every consumer to enumerate them to make that one decision.
///
/// The variants do NOT share a remedy (AlreadyHeld wants backoff, Invalid wants the package
/// fixed, VersionRetained wants a switch first), and what decides for one code anyway is the
/// asymmetry of reversibility: promoting later is cheap ONLY as a machine-readable FIELD added
/// beside this code with the code intact -- splitting the code itself later breaks the first
/// script that leaned on it. Whoever needs per-variant dispatch: add the field, keep the code.
const LIFECYCLE_REFUSED: &str = crate::error_codes::GHCLI024_EXTENSION_LIFECYCLE_REFUSED;
const LIFECYCLE_SOURCE: &str = "extension-lifecycle";

fn lifecycle_refusal(command: &'static str, message: &str, pointer: &str) -> Outcome {
    Outcome::domain(
        command,
        vec![Diagnostic::error(
            LIFECYCLE_REFUSED,
            message,
            pointer,
            LIFECYCLE_SOURCE,
        )],
    )
}

fn claim_refusal(
    command: &'static str,
    refusal: graphhelm_extension_host::ClaimRefusal,
) -> Outcome {
    use graphhelm_extension_host::ClaimRefusal;
    let message = match refusal {
        ClaimRefusal::AlreadyHeld => "another activation already holds this root's claim",
        ClaimRefusal::LegacyUnverifiable => {
            "a pre-metadata claim cannot be proven stale and is left untouched"
        }
        ClaimRefusal::UnsafeClaimPath => "the claim path is a link or otherwise unsafe",
        ClaimRefusal::Unwritable => "the claim could not be written",
        ClaimRefusal::UnsupportedPlatform => "this operating system has no proven claim authority",
    };
    lifecycle_refusal(command, message, "/root")
}

/// `target` is the pointer of the argument that NAMED the tree for this command -- `/package`
/// for install, `/digest` for switch and uninstall, `/root` for rollback, whose only argument
/// is the root. A variant-fixed pointer pointed clients at arguments the invoking command does
/// not have (switch has no `--package`), which is exactly what a pointer exists to prevent.
/// (Codex P2 on #591.)
fn install_refusal(
    command: &'static str,
    refusal: graphhelm_extension_host::InstallRefusal,
    target: &'static str,
) -> Outcome {
    use graphhelm_extension_host::InstallRefusal;
    let (message, pointer) = match refusal {
        InstallRefusal::Invalid => ("the tree does not validate", target),
        InstallRefusal::AdoptedBytesChanged => (
            "the tree no longer matches the digest it was adopted under",
            target,
        ),
        InstallRefusal::UnknownVersion => ("this digest was never adopted under this root", target),
        InstallRefusal::NoPreviousVersion => {
            ("no previous version is recorded to roll back to", "/root")
        }
        InstallRefusal::CorruptPointer => (
            "the active-version pointer exists but cannot be read",
            "/root",
        ),
        InstallRefusal::UnsafePackagePath => (
            "the package tree carries a link the copier refuses to follow",
            target,
        ),
        InstallRefusal::VersionRetained => (
            "the active pointer still names this version as current or previous",
            target,
        ),
        InstallRefusal::UnsafeLayoutPath => (
            "the install root or its versions directory is a link",
            "/root",
        ),
        InstallRefusal::Unwritable => ("the install layout could not be written", "/root"),
    };
    lifecycle_refusal(command, message, pointer)
}

/// #212: stage, verify, adopt. The claim is acquired first -- authority before any write, the
/// same order the library enforces at compile level.
pub fn run_install(root: &Path, package: &Path) -> Outcome {
    const COMMAND: &str = "extension.install";
    let claim = match graphhelm_extension_host::ActivationClaim::acquire(root) {
        Ok(claim) => claim,
        Err(refusal) => return claim_refusal(COMMAND, refusal),
    };
    match graphhelm_extension_host::install_package(&claim, package) {
        // The digest ONLY. The adopted path is an absolute path under the caller's root, and
        // normal CLI JSON is prohibited from exposing user-home paths (AGENTS.md): stdout gets
        // persisted into logs by automation, and whoever knows --root can derive the layout
        // locally. The digest is the identity every later command speaks.
        Ok(installed) => {
            Outcome::success(COMMAND, serde_json::json!({ "digest": installed.digest }))
        }
        Err(refusal) => install_refusal(COMMAND, refusal, "/package"),
    }
}

/// #212: the atomic flip. Refusals leave the previous version active, and that property is the
/// library's, pinned by its own tests -- this handler only carries it across the binary boundary.
pub fn run_switch(root: &Path, digest: &str) -> Outcome {
    const COMMAND: &str = "extension.switch";
    let claim = match graphhelm_extension_host::ActivationClaim::acquire(root) {
        Ok(claim) => claim,
        Err(refusal) => return claim_refusal(COMMAND, refusal),
    };
    match graphhelm_extension_host::switch_active(&claim, digest) {
        Ok(active) => Outcome::success(
            COMMAND,
            serde_json::json!({
                "current": active.current,
                "previous": active.previous,
            }),
        ),
        Err(refusal) => install_refusal(COMMAND, refusal, "/digest"),
    }
}

/// #212: return to the previous known-good.
pub fn run_rollback(root: &Path) -> Outcome {
    const COMMAND: &str = "extension.rollback";
    let claim = match graphhelm_extension_host::ActivationClaim::acquire(root) {
        Ok(claim) => claim,
        Err(refusal) => return claim_refusal(COMMAND, refusal),
    };
    match graphhelm_extension_host::roll_back(&claim) {
        Ok(active) => Outcome::success(
            COMMAND,
            serde_json::json!({
                "current": active.current,
                "previous": active.previous,
            }),
        ),
        Err(refusal) => install_refusal(COMMAND, refusal, "/root"),
    }
}

/// #212: remove one adopted version the pointer no longer names.
pub fn run_uninstall(root: &Path, digest: &str) -> Outcome {
    const COMMAND: &str = "extension.uninstall";
    let claim = match graphhelm_extension_host::ActivationClaim::acquire(root) {
        Ok(claim) => claim,
        Err(refusal) => return claim_refusal(COMMAND, refusal),
    };
    match graphhelm_extension_host::uninstall_version(&claim, digest) {
        Ok(()) => Outcome::success(COMMAND, serde_json::json!({ "digest": digest })),
        Err(refusal) => install_refusal(COMMAND, refusal, "/digest"),
    }
}

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
