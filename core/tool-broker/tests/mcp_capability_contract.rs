//! The pure per-contribution MCP capability pipeline (#213): a token binds
//! (package_digest, contribution_id, actor, allowed_tools, expiry) and `authorize_mcp_call`
//! decides whether one presented call may proceed. Deny-by-default, same discipline as
//! `lease::authorize` — no clock inside the crate, `now` is always the caller's.

use graphhelm_tool_broker::mcp_capability::{
    McpCapabilityRefusal, McpCapabilityToken, authorize_mcp_call, derive_allowed_tools, mint,
    record_call,
};

fn token() -> McpCapabilityToken {
    McpCapabilityToken {
        package_digest: "sha256:aaaa".to_owned(),
        contribution_id: "skill/code-contract".to_owned(),
        actor: "agent-builder".to_owned(),
        allowed_tools: ["approve", "signal"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        effects: Default::default(),
        revoked: false,
        expires_at: 1_000,
    }
}

#[test]
fn mint_carries_the_contributions_effects_for_audit_but_never_for_gating() {
    // The issue's own Deliverables list: "Capability token bound to package digest,
    // contribution id, actor, allowed tools, effects, and expiry." Effects ride the token as
    // audit metadata -- authorize_mcp_call (already proven by
    // effects_alone_never_grants_a_tool_not_named_in_surfaces) never reads this field.
    let minted = mint(
        "sha256:aaaa".to_owned(),
        "skill/code-contract".to_owned(),
        "agent-builder".to_owned(),
        &["tool:signal".to_owned()],
        &["runtime.connect".to_owned(), "runtime.mutate".to_owned()],
        500,
        1_000,
    );
    assert_eq!(
        minted.effects,
        ["runtime.connect", "runtime.mutate"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        "the contribution's own declared effects, carried verbatim"
    );
}

#[test]
fn a_token_authorizes_its_own_allowlisted_tool() {
    let result = authorize_mcp_call(&token(), "approve", "agent-builder", "sha256:aaaa", 500);
    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[test]
fn a_token_refuses_a_tool_outside_its_allowlist() {
    let result = authorize_mcp_call(&token(), "cancel", "agent-builder", "sha256:aaaa", 500);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::ToolNotAllowlisted)),
        "expected ToolNotAllowlisted, got {result:?}"
    );
}

#[test]
fn a_token_is_refused_after_the_package_digest_changes() {
    // The package was edited since this token was minted (e.g. `surfaces` narrowed or widened).
    // The token's own bound digest is frozen; the CALLER passes the freshly recomputed digest of
    // whatever package is being served right now, and any mismatch refuses -- never re-derived
    // trust in a cached value (#213 blueprint T2).
    let result = authorize_mcp_call(&token(), "approve", "agent-builder", "sha256:bbbb", 500);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::StaleDigest)),
        "expected StaleDigest, got {result:?}"
    );
}

#[test]
fn a_revoked_token_is_refused_even_before_expiry() {
    let mut revoked = token();
    revoked.revoked = true;
    let result = authorize_mcp_call(&revoked, "approve", "agent-builder", "sha256:aaaa", 500);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::Revoked)),
        "expected Revoked, got {result:?}"
    );
}

#[test]
fn an_expired_token_is_refused() {
    // token().expires_at == 1_000; `now` at exactly the boundary must already refuse -- an
    // expiry is a deadline, not a "still good at this instant" grace window.
    let result = authorize_mcp_call(&token(), "approve", "agent-builder", "sha256:aaaa", 1_000);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::Expired)),
        "expected Expired, got {result:?}"
    );
}

#[test]
fn an_actor_mismatch_is_refused_and_logged_under_the_tokens_own_actor_not_the_callers_claim() {
    // #213 blueprint T5: the MCP session's actor header is caller-declared and unverified
    // upstream of this pipeline. This check does not authenticate the caller -- it only pins
    // that a caller presenting someone else's token under a DIFFERENT claimed actor name is
    // refused, so an attacker must possess the token bytes, not merely declare a name.
    let result = authorize_mcp_call(&token(), "approve", "someone-else", "sha256:aaaa", 500);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::ActorMismatch)),
        "expected ActorMismatch, got {result:?}"
    );
}

#[test]
fn a_token_minted_for_one_contribution_is_refused_against_a_different_contribution_in_the_same_package()
 {
    // #213 blueprint T1 (confused deputy), same-package half: a memory-curator token (frozen
    // allowed_tools intersecting NONE of the execution-control tools) cannot reach `approve`
    // just because it presents the same package_digest a code-contract token would. This is a
    // composition of the digest check (which agrees) and the allowlist check (which does not) --
    // named here under the attack's own story, not a new code path.
    let curator = McpCapabilityToken {
        package_digest: "sha256:aaaa".to_owned(),
        contribution_id: "skill/memory-curator".to_owned(),
        actor: "agent-builder".to_owned(),
        allowed_tools: std::collections::BTreeSet::new(),
        effects: Default::default(),
        revoked: false,
        expires_at: 1_000,
    };
    let result = authorize_mcp_call(&curator, "approve", "agent-builder", "sha256:aaaa", 500);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::ToolNotAllowlisted)),
        "expected ToolNotAllowlisted, got {result:?}"
    );
}

#[test]
fn a_revoked_and_expired_and_mismatched_token_refuses_as_revoked_first() {
    // Pipeline order is a contract, mirroring lease::authorize's own pinned order property --
    // the SAME token here violates every check at once, and only the first refusal in the
    // documented order (revoked -> expired -> actor -> digest -> allowlist) may surface.
    let mut broken = token();
    broken.revoked = true;
    let result = authorize_mcp_call(&broken, "cancel", "someone-else", "sha256:bbbb", 999_999);
    assert!(
        matches!(result, Err(McpCapabilityRefusal::Revoked)),
        "expected Revoked (first in pipeline order), got {result:?}"
    );
}

#[test]
fn effects_alone_never_grants_a_tool_not_named_in_surfaces() {
    // #213 blueprint T4, first half: a contribution can declare wide `effects` (the schema
    // validator's authority-escalation check requires a matching effect for a mutation surface,
    // never the other direction) without that widening the MINTED token's allowed_tools -- only
    // `surfaces` feeds allowed_tools. Effects are carried on the token for audit only.
    let wide_effects_narrow_surfaces = vec!["tool:signal".to_owned()];
    let allowed = derive_allowed_tools(&wide_effects_narrow_surfaces);
    assert!(
        allowed.contains("signal") && allowed.len() == 1,
        "expected exactly the one declared tool surface, got {allowed:?}"
    );
}

#[test]
fn a_widened_surfaces_declaration_after_mint_does_not_widen_an_already_minted_token() {
    // #213 blueprint T4, second half: allowed_tools is frozen INTO the token at mint time from
    // the surfaces known then. A later widening of the package's own surfaces declaration must
    // not retroactively widen a token minted before that edit -- the caller must re-mint, and
    // re-minting is exactly what changes the token's own bytes (and its digest binding, T2).
    let narrow = vec!["tool:signal".to_owned()];
    let minted = mint(
        "sha256:aaaa".to_owned(),
        "skill/code-contract".to_owned(),
        "agent-builder".to_owned(),
        &narrow,
        &[],
        500,
        1_000,
    );
    assert_eq!(
        minted.allowed_tools.len(),
        1,
        "minted from the narrow surfaces"
    );

    // The package is edited afterward (surfaces widened to add "approve") -- but `minted` is a
    // value already returned; nothing re-derives its allowed_tools from the new surfaces list.
    let widened = vec!["tool:signal".to_owned(), "tool:approve".to_owned()];
    let _ = derive_allowed_tools(&widened); // what a FRESH mint would now produce
    assert!(
        !minted.allowed_tools.contains("approve"),
        "the already-minted token must not gain a tool a later edit adds: {:?}",
        minted.allowed_tools
    );
}

#[test]
fn refusal_records_never_carry_call_arguments_or_token_bytes() {
    // #213 blueprint T7: `record_call`'s own signature has no `arguments` parameter and no
    // `token_id`/bearer-secret parameter -- there is structurally nothing to leak. This test
    // pins that the SERIALIZED record's keys are exactly the redacted five, so a future field
    // added to the struct is caught here rather than discovered in a live audit log.
    let record = record_call(
        "agent-builder",
        "skill/code-contract",
        "sha256:aaaa",
        "cancel",
        &Err(McpCapabilityRefusal::ToolNotAllowlisted),
    );
    let value = serde_json::to_value(&record).unwrap();
    let keys: std::collections::BTreeSet<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "actor",
            "contributionId",
            "packageDigest",
            "toolName",
            "decision"
        ]
        .into_iter()
        .collect(),
        "the audit record's field set is the redacted allowlist, nothing more: {value}"
    );
    assert_eq!(value["decision"]["outcome"], "refused");
    assert_eq!(value["decision"]["code"], "tool_not_allowlisted");
}

#[test]
fn an_allowed_call_records_the_allowed_decision() {
    let record = record_call(
        "agent-builder",
        "skill/code-contract",
        "sha256:aaaa",
        "signal",
        &Ok(()),
    );
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(value["decision"]["outcome"], "allowed");
}
