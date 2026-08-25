//! CLI adapter for the development-contract Runtime services (#223).

use axum::http::StatusCode;
use graphhelm_protocols::DevelopmentRefusalCode;

use crate::output::Outcome;

/// Resolve the code-rule contract from #218's Runtime resolver, over the CLI surface.
///
/// **Existence-slice, not the full feature.** Sources are always empty here -- there is no file
/// argument yet, and adding one is behavioral-parity work (blueprint §6 item 2), not the
/// existence-parity guard this command exists to give the three adapters something real to agree
/// on. An empty source list cannot produce a [`graphhelm_policy::ResolutionRefusal`] --
/// `resolve_code_contract`'s own loop never runs over zero sources, so every branch that could
/// return `Err` is unreached -- which is why the refusal side has no mapping to
/// `DevelopmentRefusalCode` yet: there is nothing here that could exercise it.
#[must_use]
pub fn run_resolve_contract() -> Outcome {
    let contract = graphhelm_policy::resolve_code_contract(&[], chrono::Utc::now())
        .expect("an empty source list always resolves Ok - see resolve_code_contract's own loop");
    let value: serde_json::Value = serde_json::from_slice(&contract.canonical_bytes())
        .expect("canonical_bytes() always produces valid JSON");
    Outcome::success("development.resolve-contract", value)
}

/// The governed memory transition policy, as shipped (#220).
///
/// Embedded rather than read from disk, following `core/schema/src/registry.rs`, which embeds the
/// root schemas the same way: the binary answers offline, with no package discovery and no path to
/// get wrong. Discovery of an installed package is #212's job and is not wired here.
///
/// The policy is a DECLARED contribution of the built-in package — `policies/memory-transition.yaml`,
/// carried in `extension.json` with its own `sha256` — so what this embeds is the same artifact the
/// package guard already binds, not a copy of it.
const MEMORY_TRANSITION_POLICY: &str = include_str!(
    "../../../../extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml"
);

/// Report the governed memory vocabulary and the transitions policy allows, over the CLI surface.
///
/// **Existence-slice, not the full feature — and the shape of the slice was a decision.** The
/// original scope list carried `GET /v1/development/memory/{id}`. Measured while building this:
/// nothing persists a `MemoryRecord`. It exists only in `core/governor`, constructed in memory, so
/// an `{id}` had nothing to resolve against. Serving it would have needed either a refusal code
/// that does not exist (`MemoryRefusalCode` has no "not found") or a constant answer that ignores
/// the parameter — and a parameter that cannot change the response is a lie with a route attached.
///
/// So the slice reads what IS real: the shipped policy. That mirrors `run_resolve_contract`, which
/// makes a real call over an empty source list rather than returning something invented.
///
/// **The `{id}` returns as a FEATURE the day a store lands**, and whoever writes that store is the
/// consumer of this sentence.
///
/// The policy is reported as it ships, not re-derived: the states, the transitions and the allowed
/// moves live in one authoritative document, and restating them here would make this adapter a
/// second producer of one vocabulary — which drifts in silence, because a rename on one side still
/// compiles on the other.
#[must_use]
pub fn run_memory_status() -> Outcome {
    let policy: serde_json::Value = serde_yaml_ng::from_str(MEMORY_TRANSITION_POLICY).expect(
        "the shipped memory-transition policy is valid YAML - it is a checked-in, \
                 digest-bound contribution validated by the extension package guard",
    );
    Outcome::success("development.memory-status", policy)
}

/// Maps a `DevelopmentRefusalCode` to a distinct CLI exit code.
///
/// Injective by construction: base offset (20, clear of the existing 0/2/3/4 success/domain/
/// application/internal convention) plus the code's own position in `DevelopmentRefusalCode::
/// every()` - the same list that generates the enum and its wire spelling (core/protocols/src/
/// development.rs), so a new refusal code gets a new exit code automatically and two codes can
/// never collide by construction, not by enumeration kept in sync by hand.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "not yet wired into any command dispatch - the Runtime services these codes \
                  come from (#219/#221/#222/#213/#212) have not landed yet, so there is no real \
                  call site to wire it into outside this file's own tests. #expect (not #allow) \
                  so wiring it in later demands removing this rather than leaving a stale \
                  suppression. cfg_attr(not(test), ...) because the test build DOES use it - a \
                  bare #expect there would itself be unfulfilled and fail the same -D warnings."
    )
)]
pub fn development_refusal_exit_code(code: DevelopmentRefusalCode) -> i32 {
    let ordinal = DevelopmentRefusalCode::every()
        .iter()
        .position(|candidate| *candidate == code)
        .expect("every() is exhaustive by construction - code is always found");
    20 + i32::try_from(ordinal).expect("refusal code count fits in i32")
}

/// Maps a `DevelopmentRefusalCode` to a semantically-appropriate HTTP status.
///
/// DELIBERATELY NOT INJECTIVE, revising the blueprint's (#235) original "distinct HTTP status"
/// wording after hitting the same constraint on the MCP side (B's review, approved amendment):
/// HTTP status codes are externally spec-constrained the same way JSON-RPC's numeric error codes
/// are - a status is a claim with ecosystem-wide meaning (proxies, caches, monitoring, and
/// clients all interpret it), so minting a non-standard code per refusal, or overloading a
/// standard one against its meaning, would be worse than the coarseness it tries to fix. This
/// codebase already draws that line itself: `apps/cli/src/commands/serve/routes.rs` uses
/// `StatusCode::CONFLICT` (409) specifically for a sequence conflict, not a flat 400 for
/// everything - status choice here follows that same semantic-fit precedent, not raw injectivity.
///
/// The fine-grained identity travels in the response body instead, via `code.wire_name()` in the
/// diagnostic - already guaranteed distinct by construction (see
/// `wire_name_alone_already_carries_the_fine_grained_identity` below), so nothing is lost: a
/// consumer that reads the body (which every JSON API consumer does) gets the exact refusal:
/// a consumer that only inspects the status code gets the right coarse category and nothing
/// misleading.
#[must_use]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "not yet wired into any route handler - serve/routes.rs entered #223's scope \
                  via Scope amendment 2, but the Runtime services these codes come from have not \
                  landed yet. #expect so wiring it in later demands removing this rather than \
                  leaving a stale suppression. cfg_attr(not(test), ...): see the sibling function \
                  above for why a bare #expect would be unfulfilled in the test build."
    )
)]
pub fn development_refusal_http_status(code: DevelopmentRefusalCode) -> StatusCode {
    match code {
        DevelopmentRefusalCode::UnknownMajorVersion
        | DevelopmentRefusalCode::SchemaInvalid
        | DevelopmentRefusalCode::CardinalityViolation
        | DevelopmentRefusalCode::ArtifactTooLarge
        // A declared code-rule source that cannot be read as a rule (#218) is the same shape as
        // SchemaInvalid: the request named an input the server cannot even interpret, not one it
        // interpreted and rejected.
        | DevelopmentRefusalCode::CodeRuleSourceUnavailable
        // An owner waiver that is incomplete, or aimed at a class that cannot be waived (#218), is
        // malformed INPUT to the resolver in the same sense a bad schema is - the override itself
        // does not parse into something actionable.
        | DevelopmentRefusalCode::CodeRuleWaiverInvalid
        // A style plan requesting compression the content's own risk flags forbid (#221) is the
        // caller asking for something the request's own data makes unsafe to honor - the same
        // "cannot be actioned as given" shape as a waiver aimed at an unwaivable class.
        | DevelopmentRefusalCode::UnsafeCompression => StatusCode::BAD_REQUEST,
        DevelopmentRefusalCode::ScopeMismatch
        | DevelopmentRefusalCode::BindingSchemaMismatch
        | DevelopmentRefusalCode::BindingProducerMismatch => StatusCode::FORBIDDEN,
        DevelopmentRefusalCode::BindingDigestMismatch
        | DevelopmentRefusalCode::DigestMismatch
        // Two code rules on one key that cannot be reconciled under the registered operator
        // (#218) is a genuine conflict between two otherwise-valid inputs - the same shape as a
        // digest mismatch, not a malformed request.
        | DevelopmentRefusalCode::CodeRuleConflict => StatusCode::CONFLICT,
        DevelopmentRefusalCode::BindingSnapshotMissing
        | DevelopmentRefusalCode::IndexStale
        | DevelopmentRefusalCode::NegativeClaimUnverified
        // The rules are comparable and nothing orders them (#218): the request is well-formed,
        // nothing conflicts in value, and the server still cannot proceed without a precedence
        // decision it does not have - the same "missing prerequisite" shape as an unverified
        // negative claim or a stale index, not a client error in the request itself.
        | DevelopmentRefusalCode::CodeRulePrecedenceUnresolved
        // A result that never cited a capsule item it was required to rely on (#222) is
        // semantically incomplete, not malformed - the request parsed fine, the answer just does
        // not connect to evidence it was handed. Same "well-formed, cannot proceed as submitted"
        // shape as the other members of this arm.
        | DevelopmentRefusalCode::RequiredCitationMissing
        // A citation naming an item the capsule does not contain (#222) is a semantic integrity
        // failure of the submitted answer's own references, not a malformed request shape -
        // grouped with its sibling above rather than with BAD_REQUEST because the defect is in
        // what the answer CLAIMS, not in how the request was written.
        | DevelopmentRefusalCode::CitationUnresolved
        // Required context does not fit the declared token budget (#222): the request is
        // well-formed and nothing in it conflicts, it simply cannot be honored at the budget
        // given - the same "well-formed, cannot proceed as submitted" shape as its neighbours,
        // and the same reason it carries an expansion request rather than a flat refusal.
        | DevelopmentRefusalCode::ContextBudgetInsufficient => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

#[cfg(test)]
mod tests {
    use graphhelm_protocols::DevelopmentRefusalCode;

    use super::development_refusal_exit_code;

    #[test]
    fn every_development_refusal_code_maps_to_a_distinct_exit_code() {
        let mut seen = std::collections::BTreeSet::new();
        for code in DevelopmentRefusalCode::every() {
            let exit_code = development_refusal_exit_code(*code);
            assert!(
                seen.insert(exit_code),
                "exit code {exit_code} reused for {code:?} - a second refusal already claimed it"
            );
        }
    }

    #[test]
    fn every_development_refusal_code_has_a_named_http_status() {
        use super::development_refusal_http_status;
        // Total function, not injective by design (§2 amendment, HTTP/MCP both externally
        // spec-constrained at the transport layer): every code must map to SOMETHING, and the
        // mapping must not panic. The fine-grained identity lives in the diagnostic body
        // (`code.wire_name()`), asserted separately below - this test only proves the function
        // is total and its output is a real status code axum can serve.
        for code in DevelopmentRefusalCode::every() {
            let status = development_refusal_http_status(*code);
            assert!(
                status.is_client_error() || status.is_server_error(),
                "{code:?} mapped to {status} - a refusal must not report success"
            );
        }
    }

    #[test]
    fn wire_name_alone_already_carries_the_fine_grained_identity() {
        // Not a new guard - this documents an existing one. `wire_name()` is generated by the
        // SAME `wire_vocabulary!` list that builds the enum (core/protocols/src/development.rs),
        // so two variants sharing a wire string would be a duplicate `$wire` literal in that one
        // macro invocation - a compile-time match arm collision, not a runtime property this
        // test could fail to observe. Written here so a reader of THIS file sees the reasoning
        // beside the HTTP/MCP decisions it justifies, without re-proving what #217 already
        // guarantees by construction.
        let mut seen = std::collections::BTreeSet::new();
        for code in DevelopmentRefusalCode::every() {
            assert!(
                seen.insert(code.wire_name()),
                "wire_name {:?} reused - would already be a compile error upstream",
                code.wire_name()
            );
        }
    }
}
