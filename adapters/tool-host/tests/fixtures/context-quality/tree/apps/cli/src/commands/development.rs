//! CLI adapter for the development-contract Runtime services (#223).

use axum::http::StatusCode;
use graphhelm_protocols::{DevelopmentRefusalCode, Diagnostic};

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

/// Render the owner-facing presentation from #219's renderer, over the CLI surface.
///
/// **Existence-slice, not the full feature**, matching `run_resolve_contract` above: the task
/// result is a fixed no-decision one because there is no input argument yet, and adding one is
/// behavioral-parity work (blueprint §6 item 2) rather than the existence-parity this command
/// exists to give the three adapters something real to agree on.
///
/// The renderer is CONSUMED, not reimplemented. `owner_output::render` is the exclusive producer
/// of owner-facing bytes (its own `OWNER-FACING-BYTES-EMITTER` marker says so), and a second
/// place that assembles those bytes would be a second oracle for what the owner sees.
#[must_use]
pub fn run_present() -> Outcome {
    let result = graphhelm_runtime::owner_output::OwnerTaskResult {
        summary: "no task supplied".to_owned(),
        status: graphhelm_runtime::owner_output::TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: graphhelm_runtime::owner_output::RiskFlags::default(),
    };

    let presentation = graphhelm_runtime::owner_output::render(&result, None)
        .expect("a no-decision result with no risk flags always renders - see render's own guards");

    Outcome::success(
        "development.present",
        serde_json::json!({
            "text": String::from_utf8(presentation.to_owner_bytes())
                .expect("the renderer emits UTF-8 slot text"),
        }),
    )
}

/// Compile a context capsule from #222/#273's Runtime compiler, over the CLI surface.
///
/// **The budget is consulted before anything is compiled (#393).** `fit_within_budget` decides
/// this and always has; what was missing was a caller. Measured at base `9e90a29`, it had zero
/// call sites under `apps/` and no production caller anywhere -- its only non-test occurrence in
/// `core/` was its own definition. The command reached past it to `compile_capsule`, a pure
/// serializer with no budget parameter and no `Result`, so it compiled a capsule without ever
/// asking whether the capsule fit.
///
/// **Required context is refused, never trimmed.** The asymmetry is `fit_within_budget`'s own:
/// optional context is droppable and its drops are counted, required context is not droppable at
/// any budget. An implementation that trims required items to fit returns a capsule, under
/// budget, with every number healthy and without the evidence the caller was required to see.
///
/// The refusal carries the ALLOCATED code rather than a freshly minted CLI one. The envelope
/// schema that owns this vocabulary says why: codes needed by consuming tasks are allocated
/// there "rather than invented downstream, because a consumer minting its own code is how two
/// vocabularies drift while both look correct". So `wire_name()` is the diagnostic's code.
///
/// **SEALED, because an argument that reaches only half the command invites the wrong reading:**
/// `--require` sizes the budget decision and does NOT become capsule content. Capsule id,
/// version and sections remain the degenerate case, and wiring caller input into the capsule's
/// declared sections is behavioral-parity work (blueprint §6 item 2). Sections are a closed
/// vocabulary -- `projectKernel`, `task`, `node`, `evidence`, `dependencyOutputs`,
/// `agentExperience` -- and choosing which one required items land in is that item's decision to
/// make, not a side effect of giving this command a budget. `--optional` is unexposed for the
/// same reason, so the `Fits { dropped_optional }` half is not reachable from this surface yet.
///
/// Reports the compiled capsule's digest, not its raw bytes: `compile_capsule`'s own wire format
/// is an internal length-prefixed encoding (see its doc comment), not something meant to travel
/// as a JSON value. The digest is real, computed proof the compiler ran, in a shape every surface
/// can already carry.
#[must_use]
pub fn run_compile_context(budget: usize, require: &[String]) -> Outcome {
    match compile_context_decision(budget, require) {
        Ok(digest) => Outcome::success(
            COMPILE_CONTEXT_COMMAND,
            serde_json::json!({"digest": digest}),
        ),
        Err((code, message)) => context_refusal(code, message),
    }
}

const COMPILE_CONTEXT_COMMAND: &str = "development.compile-context";

/// The budget decision itself, in the vocabulary each surface maps FROM.
///
/// **One oracle, two mappings, and the split is the point.** The CLI turns a refusal into an exit
/// code and HTTP turns the same refusal into a status, and those two mappings are deliberately
/// different -- exit codes are injective, HTTP statuses are deliberately not. Letting each
/// surface ask `fit_within_budget` for itself would be a duplicated oracle: two call sites that
/// can drift into disagreeing about whether the same input fits. Letting HTTP reuse the CLI's
/// finished `Outcome` fails the other way, because `respond_outcome` maps every exit code above
/// 3 to 500 -- the allocated refusal code 32 would have been served as an internal server error,
/// which is a lie about whose fault it is. Measured: that is what happened before this split.
pub fn compile_context_decision(
    budget: usize,
    require: &[String],
) -> Result<String, (DevelopmentRefusalCode, String)> {
    // #1065: the SAME producer the node path runs (`context::compile_items` is `fit_within_budget`
    // + `compile_capsule` + `base_digest`, without a search in front). The digest is therefore
    // the digest of the capsule the required items actually compile to — it used to be the
    // digest of an empty capsule whatever was required. This closes #724's producer half; the
    // RetrievalPlan adapter half stays open there.
    match graphhelm_runtime::context::compile_items(
        COMPILE_CONTEXT_CAPSULE_ID,
        require,
        &[],
        budget,
    ) {
        Err((code, expansion)) => Err((
            code,
            format!(
                "required context needs a budget of {} and {budget} was given; required context is never dropped to fit",
                expansion.required_budget
            ),
        )),
        Ok(compiled) => Ok(compiled.digest),
    }
}

/// The capsule identity `compile-context` compiles under: no execution, no node, no attempt —
/// the command compiles caller-supplied items, so the id names the command.
const COMPILE_CONTEXT_CAPSULE_ID: &str = "development.compile-context";

/// The refusal envelope both surfaces publish, so the body cannot differ between them.
///
/// The exit code is set here rather than at the CLI call site so that it travels with the
/// envelope: a surface that ignores it (HTTP does, in favour of the status) is choosing to, and a
/// surface that forgets it cannot silently fall back to `domain`'s generic 2.
#[must_use]
pub fn context_refusal(code: DevelopmentRefusalCode, message: String) -> Outcome {
    let mut refusal = Outcome::domain(
        COMPILE_CONTEXT_COMMAND,
        vec![Diagnostic::error(
            code.wire_name(),
            message,
            "/require",
            COMPILE_CONTEXT_COMMAND,
        )],
    );
    refusal.exit_code = development_refusal_exit_code(code);
    refusal
}

/// Propose content for governed memory and report the admission verdict, over the CLI surface.
///
/// **Existence-slice, not the full feature**, matching its siblings: the content and the scope are
/// fixed because there is no input argument yet, and adding one is behavioral-parity work rather
/// than the existence-parity this command exists to give the three adapters something to agree on.
///
/// **It returns the VERDICT and no identifier, and that is a measurement rather than a choice.**
/// Nothing persists a `MemoryCandidate` or a `MemoryRecord` -- both exist only in `core/governor`,
/// built in memory -- so an id would name something no later call could resolve. The verdict is the
/// one thing the admission path can honestly answer today: admitted, or refused with its code.
///
/// The admission path is CONSUMED, not restated. `capture_memory` holds the opt-in above the first
/// boundary touch and `admit_memory_candidate` screens scope, origin and content; an adapter that
/// re-decided any of that would be a second authority on what memory is admissible.
#[must_use]
pub fn run_memory_propose() -> Outcome {
    let scope = graphhelm_protocols::DevelopmentScope {
        workspace_id: graphhelm_protocols::WorkspaceId::parse("workspace-local")
            .expect("a constant workspace id is valid"),
        project_id: graphhelm_protocols::ProjectId::parse("project-local")
            .expect("a constant project id is valid"),
        subproject_id: None,
        execution_id: None,
    };

    // `touches` is READ, not merely passed. The API hands it over so that "touched nothing" is
    // OBSERVABLE rather than inferred from an absent return value -- and a caller that creates the
    // vector, lends it, and never looks at it lets the property quietly regress to the inference
    // the mechanism exists to replace. Reported from outside by the first review of this adapter.
    let mut touches = Vec::new();
    let verdict = match graphhelm_governor::capture_memory(
        graphhelm_governor::CaptureOptIn::Enabled,
        &scope,
        "a proposal with no input argument yet",
        &mut touches,
    ) {
        Ok(candidate) => match graphhelm_governor::admit_memory_candidate(&candidate, &scope) {
            Ok(()) => serde_json::json!({ "admitted": true }),
            Err(refusal) => serde_json::json!({
                "admitted": false,
                "refusedWith": refusal.code().wire_name(),
            }),
        },
        Err(refusal) => serde_json::json!({
            "admitted": false,
            "refusedWith": refusal.code().wire_name(),
            // The refusal happened above the first boundary touch, and this says so from the
            // observation rather than from the design's promise.
            "touchedNothing": touches.is_empty(),
        }),
    };

    Outcome::success("development.memory-propose", verdict)
}

/// Report a context-accounting receipt, over the CLI surface.
///
/// **Existence-slice, not the full feature**, matching its siblings: there is no execution to
/// account for yet, because there is no input argument yet -- wiring one to a real execution's
/// measured costs is behavioral-parity work.
///
/// **The one field is marked `unavailable`, not `measured(0, ...)`, and that is the whole point
/// of `CostField`'s three-state design (`core/runtime/src/context_accounting.rs`).** Nothing
/// watched this call spend any tokens, because it did not run against a real execution -- and a
/// module whose entire acceptance criterion is "an unmeasured cost must never render as zero"
/// cannot report zero here without contradicting its own reason to exist. `unavailable` is the
/// field this existence-slice can HONESTLY report; `measured` and `derived` both require an
/// observation this call did not make.
///
/// **No identifier, and that is measured rather than assumed**, the same way `run_memory_status`
/// and `run_memory_propose` measured theirs: `AccountingReceipt` is a plain builder value
/// (`core/runtime/src/context_accounting.rs`), never written to the event log or any other store.
/// `git grep AccountingReceipt` across the WHOLE workspace finds it in exactly two places besides
/// its own module and this file: `tools/development-benchmark`'s `arm_cost`/`compare_cost`, which
/// take a receipt already built by the caller and read it in memory (never store or serialise
/// it) -- a second in-memory consumer, not a second producer or a store. An id would name a
/// receipt no later call could resolve.
#[must_use]
pub fn run_accounting() -> Outcome {
    let receipt = graphhelm_runtime::context_accounting::AccountingReceipt::new()
        .with_field(
            "total_tokens",
            graphhelm_runtime::context_accounting::CostField::unavailable(
                "no execution to account for yet - #223 existence-slice",
            ),
        )
        .expect(
            "`unavailable` never names the accounting module as its own observer - only a \
                 `measured` field claiming that producer can fail `with_field`",
        );

    let field = receipt
        .field("total_tokens")
        .expect("the field was just added under this exact name");

    Outcome::success(
        "development.accounting",
        serde_json::json!({
            "totalTokens": {
                "observed": field.observed(),
                "measured": field.is_measured(),
                "provenance": field.provenance().wire_name(),
            },
        }),
    )
}

/// Maps a `DevelopmentRefusalCode` to a distinct CLI exit code.
///
/// Injective by construction: base offset (20, clear of the existing 0/2/3/4 success/domain/
/// application/internal convention) plus the code's own position in `DevelopmentRefusalCode::
/// every()` - the same list that generates the enum and its wire spelling (core/protocols/src/
/// development.rs), so a new refusal code gets a new exit code automatically and two codes can
/// never collide by construction, not by enumeration kept in sync by hand.
///
/// **Wired in #393 by `run_compile_context`, and the `expect(dead_code)` that stood here is gone
/// rather than silenced.** That is the attribute working as designed: it was `expect` and not
/// `allow` precisely so that the first real caller would have to remove it instead of leaving a
/// stale suppression behind.
///
/// Its stated reason had also gone half-false without going red. It said the Runtime services
/// these codes come from "have not landed yet" and named #222 among them -- but #222's
/// `context_compiler::fit_within_budget` had landed, with its own tests, and was refusing with
/// `ContextBudgetInsufficient` for anyone who called it. What had not landed was a CALLER. A
/// suppression whose justification decays silently is the same class as a comment that goes
/// false without going red, which is why this note replaces it rather than merely deleting it.
#[must_use]
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
///
/// **Wired in #393 by `serve::routes::development_compile_context`, and its `expect(dead_code)`
/// is gone rather than silenced** -- the same retirement its sibling above went through, for the
/// same reason: `expect` was chosen over `allow` precisely so the first real caller would have to
/// remove it. Its stated reason had gone stale in the same way too, saying the Runtime services
/// these codes come from had not landed when #222's `fit_within_budget` had.
///
/// The route calls this instead of `respond_outcome`, and that is not a preference. That generic
/// mapping sends every exit code above 3 to 500, so the allocated refusal exit code 32 would have
/// been served as an internal server error -- the deliberate non-injectivity described above is
/// only safe because the exact refusal still travels in the body.
#[must_use]
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
