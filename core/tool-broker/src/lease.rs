//! The capability lease and the pure `authorize` pipeline — §11.2's decision steps as one
//! total function: identity → capability → program allowlist → effect → tier. Deny by default
//! is the lease's shape (threat model §8.2): what is not granted does not exist.

use std::collections::BTreeSet;

use crate::call::ToolCall;
use crate::effect::{EffectUnsupported, IsolationTier, ToolEffect, required_tier};
use crate::path::validate_program_name;

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    RepositoryRead,
    RepositoryWrite,
    ShellExecute,
    TestsExecute,
}

/// What the caller was granted. By whom is 05d's concern (the node contract); here the lease
/// is an input. No implicit inheritance, deny by default (threat model §8.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolLease {
    pub actor: String,
    pub capabilities: BTreeSet<Capability>,
    /// Bare program names the Shell capability may spawn. Tests' runner is host config and
    /// deliberately not listed here — a caller cannot rename its way around the lease.
    pub programs: BTreeSet<String>,
}

/// A positive authorization: what the host may now route. The tier came from the effect and
/// nothing else — the host re-refuses a mismatch as defense in depth (Task 7).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BrokerPlan {
    pub capability: Capability,
    pub effect: ToolEffect,
    pub tier: IsolationTier,
}

/// A typed refusal. `Display` names the rule and at most lease-side identity — never a call
/// argument, a patch, or any caller content (`refusals_never_echo_call_arguments` pins this).
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum BrokerRefusal {
    #[error("the caller is not the lease's actor")]
    ActorMismatch { lease_actor: String },
    #[error("the lease does not grant {capability:?}")]
    CapabilityMissing { capability: Capability },
    #[error("the program is not in the lease's allowlist")]
    ProgramDenied,
    #[error("the actor identifier is not valid")]
    ActorInvalid,
    #[error(transparent)]
    EffectUnsupported(#[from] EffectUnsupported),
}

/// Actor charset: `[a-z][a-z0-9-]{0,63}` — stricter than the wire `ActorId` pattern,
/// deliberately: the broker's actors are machine identities this slice mints, and a narrow
/// charset keeps them argv-, env- and log-safe everywhere downstream.
fn valid_actor(candidate: &str) -> bool {
    let mut bytes = candidate.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && candidate.len() <= 64
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The pure §11.2 pipeline: identity → capability → program allowlist → effect → tier.
/// Order is a contract, pinned by the refusal variants: a malformed actor refuses before the
/// mismatch comparison (`a_malformed_actor_is_refused_before_any_other_check`), a mismatch
/// before capabilities, capabilities before the program allowlist.
///
/// # Errors
/// The first [`BrokerRefusal`] the call violates, in pipeline order.
pub fn authorize(
    call: &ToolCall,
    lease: &ToolLease,
    actor: &str,
) -> Result<BrokerPlan, BrokerRefusal> {
    if !valid_actor(actor) {
        return Err(BrokerRefusal::ActorInvalid);
    }
    if actor != lease.actor {
        return Err(BrokerRefusal::ActorMismatch {
            lease_actor: lease.actor.clone(),
        });
    }
    let capability = call.capability();
    if !lease.capabilities.contains(&capability) {
        return Err(BrokerRefusal::CapabilityMissing { capability });
    }
    if let ToolCall::Shell(action) = call {
        // A malformed program name is denied through the same door as an unlisted one: the
        // allowlist only ever holds bare validated names, so failing the shape check IS
        // failing the allowlist.
        if validate_program_name(&action.program).is_err()
            || !lease.programs.contains(&action.program)
        {
            return Err(BrokerRefusal::ProgramDenied);
        }
    }
    let effect = call.effect();
    let tier = required_tier(effect)?;
    Ok(BrokerPlan {
        capability,
        effect,
        tier,
    })
}
