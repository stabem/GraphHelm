//! The capability lease and the pure `authorize` pipeline — §11.2's decision steps as one
//! total function: identity → capability → program allowlist → effect → tier. Deny by default
//! is the lease's shape (threat model §8.2): what is not granted does not exist.

use std::collections::BTreeSet;

use crate::call::ToolCall;
use crate::effect::{EffectUnsupported, IsolationTier, ToolEffect, required_tier};
use crate::path::validate_program_name;

/// Maximum number of distinct bare program names one lease may grant. Each accepted name is at
/// most 64 bytes, so this also keeps the complete authority record far below Evidence's 16 MiB
/// item bound before any tool effect can run.
pub const MAX_PROGRAM_ALLOWLIST_MEMBERS: usize = 256;

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

impl ToolLease {
    /// Returns the complete canonical program authority only when every member is a valid bare
    /// program name and the set stays within its deterministic member bound. A malformed or
    /// oversized set invalidates the whole authority: callers must never execute under, or
    /// durably record, a partly trusted lease.
    ///
    /// # Errors
    /// [`BrokerRefusal::ProgramAllowlistInvalid`] without echoing the rejected member.
    pub fn validated_program_allowlist(&self) -> Result<&BTreeSet<String>, BrokerRefusal> {
        if self.programs.len() <= MAX_PROGRAM_ALLOWLIST_MEMBERS
            && self
                .programs
                .iter()
                .all(|program| validate_program_name(program).is_ok())
        {
            Ok(&self.programs)
        } else {
            Err(BrokerRefusal::ProgramAllowlistInvalid)
        }
    }
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
    /// The program name failed the SHAPE rule (`[a-z0-9_-]{1,64}`), so it could never have been
    /// in any allowlist. Split from `ProgramDenied` (#247): the two have opposite remedies -- fix
    /// the spelling at the call site, versus change the lease -- and one refusal for both cost a
    /// lane two full fix-and-rerun cycles, each a reasonable reading of the same text. Names the
    /// rule and nothing else: the offending string is caller content and is never echoed.
    #[error("the program name is not a bare validated name")]
    ProgramNameInvalid,
    #[error("the lease's program allowlist is invalid")]
    ProgramAllowlistInvalid,
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
    let programs = lease.validated_program_allowlist()?;
    if let ToolCall::Shell(action) = call {
        // TWO DOORS, BECAUSE THE TWO CAUSES HAVE OPPOSITE REMEDIES (#247). This used to deny a
        // malformed name through the allowlist door, on the argument that the allowlist only
        // ever holds bare validated names, so failing the shape check IS failing the allowlist.
        // That is true and it is the wrong thing to optimise for: the refusal loses no
        // information about WHETHER the call is permitted, and loses exactly the information
        // about what to do next. "Fix the spelling" and "change the lease, or you are being
        // denied on purpose" are the two readings, and a lane spent two runs trying each
        // against the same refusal. The shape check comes first, so an unlisted name is only
        // ever reported as unlisted once it is a name the allowlist could have held.
        if validate_program_name(&action.program).is_err() {
            return Err(BrokerRefusal::ProgramNameInvalid);
        }
        if !programs.contains(&action.program) {
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
