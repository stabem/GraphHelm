//! #221: `OwnerOutputValidator` — red-first per the sealed plan in
//! the owner-output blueprint §7 (T1-T12). One test at a time, TDD literal.

use std::path::Path;

use graphhelm_runtime::owner_output::{
    AdvisoryDecisionResult, DecisionOption, OwnerOutputError, OwnerPresentationPlan,
    OwnerTaskResult, RecommendedOption, RiskFlags, SlotId, StyleToken, TaskOutcome, render,
};

/// T1: a completed no-action result says no action and emits no options.
#[test]
fn no_decision_says_nothing_now() {
    let result = OwnerTaskResult {
        summary: "checked the thing".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let presentation = render(&result, None).expect("a no-decision result renders");

    assert_eq!(
        presentation.text(SlotId::YourAction),
        Some("Nothing now"),
        "no-decision result must say Nothing now"
    );
    assert_eq!(
        presentation.count(SlotId::OptionA) + presentation.count(SlotId::OptionB),
        0,
        "no-decision result must emit zero options"
    );
    assert_eq!(
        presentation.count(SlotId::Recommendation),
        0,
        "no-decision result must emit zero recommendations"
    );
}

/// T2: a real decision emits exactly two options and one recommendation.
#[test]
fn real_decision_exact_cardinality() {
    let result = OwnerTaskResult {
        summary: "found two ways forward".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: true,
        decision: Some(AdvisoryDecisionResult {
            decision_statement: "pick a rollout strategy".to_owned(),
            options: [
                DecisionOption {
                    label: "ship behind a flag".to_owned(),
                    consequence: "reversible, slower rollout".to_owned(),
                },
                DecisionOption {
                    label: "ship directly".to_owned(),
                    consequence: "faster, harder to revert".to_owned(),
                },
            ],
            recommended: RecommendedOption::First,
            rationale: "flag lets us revert without a deploy".to_owned(),
        }),
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let presentation = render(&result, None).expect("a real decision renders");

    assert_eq!(
        presentation.count(SlotId::OptionA) + presentation.count(SlotId::OptionB),
        2,
        "a real decision must emit exactly two options"
    );
    assert_eq!(
        presentation.count(SlotId::Recommendation),
        1,
        "a real decision must emit exactly one recommendation"
    );
    assert_eq!(
        presentation.text(SlotId::OptionA),
        Some("ship behind a flag"),
        "OptionA must carry the first option's label verbatim"
    );
    assert_eq!(
        presentation.text(SlotId::Recommendation),
        Some("ship behind a flag"),
        "the recommendation must name the recommended option"
    );
}

/// T3: `owner_action_required` and `decision` disagreeing refuses before any AST is built.
#[test]
fn cardinality_mismatch_is_schema_invalid() {
    let claims_action_but_has_no_decision = OwnerTaskResult {
        summary: "did the thing".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: true,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };
    assert_eq!(
        render(&claims_action_but_has_no_decision, None),
        Err(OwnerOutputError::SchemaInvalid),
        "owner_action_required=true with no decision must refuse, not silently render"
    );

    let has_decision_but_claims_no_action = OwnerTaskResult {
        summary: "did the thing".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: Some(AdvisoryDecisionResult {
            decision_statement: "pick one".to_owned(),
            options: [
                DecisionOption {
                    label: "a".to_owned(),
                    consequence: "a-consequence".to_owned(),
                },
                DecisionOption {
                    label: "b".to_owned(),
                    consequence: "b-consequence".to_owned(),
                },
            ],
            recommended: RecommendedOption::First,
            rationale: "because".to_owned(),
        }),
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };
    assert_eq!(
        render(&has_decision_but_claims_no_action, None),
        Err(OwnerOutputError::SchemaInvalid),
        "a decision present with owner_action_required=false must refuse, the reverse mismatch"
    );
}

/// T4: a truthful defer option renders with its own consequence, same fidelity as any other
/// option — this validator cannot verify a defer option is REALLY the only safe path (that's the
/// caller's obligation, same boundary as F1), but it must not silently drop or favor either
/// option's consequence text.
#[test]
fn defer_option_renders_with_its_own_consequence() {
    let result = OwnerTaskResult {
        summary: "nothing else is safe right now".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: true,
        decision: Some(AdvisoryDecisionResult {
            decision_statement: "the only safe move".to_owned(),
            options: [
                DecisionOption {
                    label: "proceed".to_owned(),
                    consequence: "not actually safe today".to_owned(),
                },
                DecisionOption {
                    label: "wait and gather more evidence".to_owned(),
                    consequence: "delays the outcome by a day".to_owned(),
                },
            ],
            recommended: RecommendedOption::Second,
            rationale: "proceeding now is unsafe".to_owned(),
        }),
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let presentation = render(&result, None).expect("a real decision with a defer option renders");

    assert_eq!(
        presentation.text(SlotId::OptionAConsequence),
        Some("not actually safe today"),
        "OptionA's consequence must render, not just its label"
    );
    assert_eq!(
        presentation.text(SlotId::OptionBConsequence),
        Some("delays the outcome by a day"),
        "the defer option's consequence must render with the same fidelity as any other option"
    );
}

/// T5, the central invariant (blueprint §3): no sequence of valid inputs produces a `Result`
/// slot whose rendered phrase contradicts `result.status`. The phrase is byte-identical across
/// repeated calls with the same status, regardless of everything else in the result.
#[test]
fn failure_never_renders_as_success() {
    let failed = OwnerTaskResult {
        summary: "the deploy did not complete".to_owned(),
        status: TaskOutcome::Failure,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let first = render(&failed, None).expect("a failed result still renders");
    let second = render(&failed, None).expect("a failed result still renders, second call");

    let failure_phrase = first
        .text(SlotId::Result)
        .expect("a rendered result always carries a Result slot");
    assert_eq!(
        failure_phrase,
        second.text(SlotId::Result).unwrap(),
        "the Result slot's phrase must be byte-identical across repeated calls"
    );
    assert!(
        !failure_phrase.to_ascii_lowercase().contains("success"),
        "a Failure status must never render a phrase containing the word success"
    );

    let succeeded = OwnerTaskResult {
        summary: "the deploy completed".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };
    let success_presentation = render(&succeeded, None).expect("a succeeded result renders");
    assert_ne!(
        success_presentation.text(SlotId::Result),
        first.text(SlotId::Result),
        "Success and Failure must render different Result phrases"
    );
}

/// T6: a destructive-flagged result's evidence limitation survives Terse compression. Two
/// assertions, not one, so the test cannot pass vacuously: (a) proves Terse actually compresses
/// something when nothing is risk-flagged, (b) proves a risk flag overrides that compression for
/// the same content — without (a), (b) would pass even if Terse never compressed anything at all.
#[test]
fn destructive_consequence_survives_compression() {
    let terse_plan = OwnerPresentationPlan {
        order: vec![],
        style: StyleToken::Terse,
    };

    let safe_result = OwnerTaskResult {
        summary: "ran a read-only check".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: Some("only checked the last 30 days".to_owned()),
        rollback: None,
        risk_flags: RiskFlags::default(),
    };
    let terse_safe = render(&safe_result, Some(&terse_plan)).expect("a terse safe result renders");
    assert_eq!(
        terse_safe.text(SlotId::EvidenceLimitation),
        None,
        "Terse must actually compress evidence limitations when nothing is risk-flagged \
         (positive control: without this, the override below would be vacuous)"
    );

    let destructive_result = OwnerTaskResult {
        summary: "about to drop a table".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: true,
        decision: Some(AdvisoryDecisionResult {
            decision_statement: "confirm the drop".to_owned(),
            options: [
                DecisionOption {
                    label: "drop the table".to_owned(),
                    consequence: "irreversible data loss".to_owned(),
                },
                DecisionOption {
                    label: "stop".to_owned(),
                    consequence: "table stays, nothing lost".to_owned(),
                },
            ],
            recommended: RecommendedOption::Second,
            rationale: "irreversible, confirm first".to_owned(),
        }),
        evidence_limitation: Some("row count estimated, not exact".to_owned()),
        rollback: None,
        risk_flags: RiskFlags {
            destructive: true,
            ..RiskFlags::default()
        },
    };
    let terse_destructive = render(&destructive_result, Some(&terse_plan))
        .expect("a terse destructive result still renders, expanded");
    assert_eq!(
        terse_destructive.text(SlotId::EvidenceLimitation),
        Some("row count estimated, not exact"),
        "a destructive risk flag must override Terse compression for the evidence limitation"
    );
}

/// T7: a plan naming an immovable slot in `order` is discarded WHOLESALE — its style request is
/// discarded along with it, not applied selectively. Safe built-in rendering still succeeds.
#[test]
fn malicious_plan_discarded_safe_fallback_used() {
    let result = OwnerTaskResult {
        summary: "ran a read-only check".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: Some("only checked the last 30 days".to_owned()),
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let malicious_plan = OwnerPresentationPlan {
        // Result is structural/immovable — no legitimate plan ever names it in `order`.
        order: vec![SlotId::Result],
        style: StyleToken::Terse,
    };

    let presentation = render(&result, Some(&malicious_plan))
        .expect("a malicious plan does not fail the whole response");

    assert_eq!(
        presentation.text(SlotId::EvidenceLimitation),
        Some("only checked the last 30 days"),
        "the plan's Terse request must be discarded along with the rest of the malformed plan, \
         not applied selectively — evidence limitation renders as if no plan were given at all"
    );
}

// T8 is a compile-time claim (blueprint §1.5 / C's review): `OwnerPresentationPlan` has no
// field capable of holding rendered text, so slot-value injection cannot compile, not merely
// fail validation. Not a runnable test — verified by inspection of the type definition, same
// epistemic status the blueprint gave it ("not yet reachable from any sealed test... a
// structural claim"). Left undated deliberately rather than faked with a hollow assertion.

/// T9: a secret-shaped value anywhere in the result refuses the WHOLE response — decided by the
/// orchestrator (issue #221 comment `5392787280`, folded into the blueprint's §9 OQ1): never a
/// partial render with the offending slot swapped out, because a silent partial lies about its
/// own completeness the same way a wrong `present` lied on #200.
#[test]
fn secret_in_slot_value_refuses_whole_response() {
    let result = OwnerTaskResult {
        summary: "used token AKIAABCDEFGHIJKLMNOP to check status".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    assert_eq!(
        render(&result, None),
        Err(OwnerOutputError::SchemaInvalid),
        "a secret-shaped value anywhere in the result must refuse the entire response"
    );
}

/// F's cold-pass review on #254 (`5395700376`): `SlotId::Summary` and
/// `SlotId::RecommendationRationale` are declared structural — in the enum AND in
/// `policies/owner-output-policy.yaml`'s `structuralSlots` — but `render` never constructs
/// either as a `RenderedSlot`. `result.summary` and `decision.rationale` were read only inside
/// the secret scan, never rendered: the owner never saw the summary or the reason behind a
/// recommendation, and zero prior tests touched either slot (every earlier test asserted
/// specific OTHER slots and never checked these were absent).
#[test]
fn summary_and_recommendation_rationale_are_rendered() {
    let result = OwnerTaskResult {
        summary: "checked the deploy pipeline end to end".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: true,
        decision: Some(AdvisoryDecisionResult {
            decision_statement: "pick a rollout strategy".to_owned(),
            options: [
                DecisionOption {
                    label: "ship behind a flag".to_owned(),
                    consequence: "reversible, slower rollout".to_owned(),
                },
                DecisionOption {
                    label: "ship directly".to_owned(),
                    consequence: "faster, harder to revert".to_owned(),
                },
            ],
            recommended: RecommendedOption::First,
            rationale: "flag lets us revert without a deploy".to_owned(),
        }),
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let presentation = render(&result, None).expect("renders");

    assert_eq!(
        presentation.text(SlotId::Summary),
        Some("checked the deploy pipeline end to end"),
        "the summary must reach the owner, not just the secret scanner"
    );
    assert_eq!(
        presentation.text(SlotId::RecommendationRationale),
        Some("flag lets us revert without a deploy"),
        "the recommendation's rationale must reach the owner, not just the secret scanner"
    );
}

/// F's cold-pass review on #254 (`5395700376`): `contains_secret_shape` scans `summary`,
/// `evidence_limitation`, and the decision's fields — never `rollback`, exactly the field this
/// PR's own tests show carrying verbatim shell commands. A secret-shaped token in a rollback
/// command sails past the entire defense-in-depth scan.
#[test]
fn secret_in_rollback_refuses_whole_response() {
    let result = OwnerTaskResult {
        summary: "deployed successfully".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: Some(
            "curl -H 'Authorization: Bearer sk-abc123' https://deploy/rollback".to_owned(),
        ),
        risk_flags: RiskFlags::default(),
    };

    assert_eq!(
        render(&result, None),
        Err(OwnerOutputError::SchemaInvalid),
        "a secret-shaped value in rollback must refuse the entire response, exactly like every \
         other field the scan already covers"
    );
}

/// T10: only `owner_output.rs` emits owner-facing bytes. A grep is a derived key — its GREEN
/// (zero elsewhere) is indistinguishable from a pattern that matches nothing, unless the SAME
/// pattern first finds the known emitter (C's review, F2). Searches for the literal phrase this
/// module alone should ever produce (`"Nothing now"`) across every `.rs` file under `core/` and
/// `apps/`, skipping `target` AND `tests` directories — the first run of this exact test caught
/// a real false positive: this very test file legitimately asserts against that string as an
/// EXPECTED VALUE, which isn't the class of violation T10 exists to catch (a second production
/// emitter). Scoping to `src/` only is the fix; excluding this file by name would be narrower
/// than the actual defect (any test file asserting the phrase would collide the same way).
#[test]
fn only_this_module_emits_owner_bytes() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("core/runtime sits two levels under the workspace root")
        .to_path_buf();

    let marker = "Nothing now";
    let mut files_containing_marker = Vec::new();
    for scan_root in ["core", "apps"] {
        walk_rs_files(&workspace_root.join(scan_root), &mut |path, contents| {
            if contents.contains(marker) {
                files_containing_marker.push(path.to_path_buf());
            }
        });
    }

    let owner_output_rs = workspace_root.join("core/runtime/src/owner_output.rs");
    assert!(
        files_containing_marker.contains(&owner_output_rs),
        "positive control failed: the search itself did not find the known emitter \
         (core/runtime/src/owner_output.rs) — a zero elsewhere would be meaningless. Found in: \
         {files_containing_marker:?}"
    );
    assert_eq!(
        files_containing_marker.len(),
        1,
        "\"{marker}\" must appear in exactly one file (owner_output.rs); found in: \
         {files_containing_marker:?}"
    );
}

/// Recursively visits every `.rs` file under `root` (skipping `target` directories), calling
/// `visit` with each file's path and contents. Test-only helper, not production code — walking
/// the workspace to police its own boundary is this test's whole job.
fn walk_rs_files(root: &Path, visit: &mut dyn FnMut(&Path, &str)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name == "target" || name == "tests")
            {
                continue;
            }
            walk_rs_files(&path, visit);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(contents) = std::fs::read_to_string(&path)
        {
            visit(&path, &contents);
        }
    }
}

/// T11: a second call with a different result reflects THAT call's status, never the first's —
/// `render`'s signature has no cache/memoization parameter for a stale answer to hide behind
/// (the same "signature is the fence" argument `judge.rs`'s `assemble(work: &JudgeWork)` makes).
#[test]
fn stale_result_never_served() {
    let failed = OwnerTaskResult {
        summary: "first call".to_owned(),
        status: TaskOutcome::Failure,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };
    let succeeded = OwnerTaskResult {
        summary: "second call".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: None,
        risk_flags: RiskFlags::default(),
    };

    let first = render(&failed, None).expect("first call renders");
    let second = render(&succeeded, None).expect("second call renders");

    assert_ne!(
        first.text(SlotId::Result),
        second.text(SlotId::Result),
        "the second call must reflect its OWN status, not the first call's — no memoization \
         layer may serve call 1's answer for call 2's input"
    );
    assert!(
        !second
            .text(SlotId::Result)
            .unwrap()
            .to_ascii_lowercase()
            .contains("fail"),
        "the second (succeeded) call must not carry any trace of the first (failed) call's phrase"
    );
}

/// Closes a real gap against the issue's own acceptance criteria (found while drafting the PR,
/// not sealed in the original blueprint plan): rollback information is named explicitly
/// alongside evidence limits and material consequences as something that "cannot be compressed
/// away or rewritten as success". `RiskFlags` already forces full rendering for a risk-flagged
/// evidence limitation (T6); this proves the same holds for rollback.
#[test]
fn rollback_survives_compression_like_evidence_limitation() {
    let terse_plan = OwnerPresentationPlan {
        order: vec![],
        style: StyleToken::Terse,
    };

    let safe_result = OwnerTaskResult {
        summary: "ran a read-only check".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: Some("nothing to roll back, read-only".to_owned()),
        risk_flags: RiskFlags::default(),
    };
    let terse_safe = render(&safe_result, Some(&terse_plan)).expect("a terse safe result renders");
    assert_eq!(
        terse_safe.text(SlotId::Rollback),
        None,
        "Terse compresses rollback when nothing is risk-flagged (positive control, same shape \
         as the evidence-limitation guard)"
    );

    let production_result = OwnerTaskResult {
        summary: "deployed to production".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: Some("run `deploy --rollback v41` within 30 minutes".to_owned()),
        risk_flags: RiskFlags {
            production: true,
            ..RiskFlags::default()
        },
    };
    let terse_production = render(&production_result, Some(&terse_plan))
        .expect("a terse production result still renders, expanded");
    assert_eq!(
        terse_production.text(SlotId::Rollback),
        Some("run `deploy --rollback v41` within 30 minutes"),
        "a production risk flag must override Terse compression for rollback, exactly as it \
         does for evidence limitation"
    );
}

/// Closes another real gap against the issue's acceptance criteria: "exact paths/commands...
/// preserve exactly". This validator does no prose reformatting anywhere today, so byte-exact
/// preservation is currently true by the absence of any rewriting step — this test makes that an
/// EXPLICIT, checked guarantee rather than an accidental property of an incomplete serializer.
///
/// L's review (`5395616664`): the original version of this test used plain ASCII with no
/// trailing space and no unicode variance, so trimming or unicode normalization would have been
/// a silent no-op and the test would stay green either way. Strengthened to carry three
/// transformations that a "helpful" formatter commonly applies without meaning to: a TRAILING
/// SPACE (trim would eat it), a DECOMPOSED unicode sequence — `e` + COMBINING ACUTE ACCENT
/// (U+0301), not the precomposed `é` (U+00E9) — that NFC normalization would silently merge, and
/// Windows-style backslashes (this crate's sibling `normalise_path_separators` in
/// `core/protocols/src/development.rs` converts exactly these to forward slashes; this validator
/// must never call anything that does the same).
#[test]
fn exact_command_text_round_trips_byte_verbatim() {
    let exact_command = "powershell.exe -File C:\\deploy\\rollback.ps1 --note cafe\u{0301} ";
    let result = OwnerTaskResult {
        summary: "the fix is ready".to_owned(),
        status: TaskOutcome::Success,
        owner_action_required: false,
        decision: None,
        evidence_limitation: None,
        rollback: Some(exact_command.to_owned()),
        risk_flags: RiskFlags::default(),
    };

    let presentation = render(&result, None).expect("renders");
    let rendered = presentation
        .text(SlotId::Rollback)
        .expect("rollback slot is present");

    assert_eq!(
        rendered, exact_command,
        "an exact command string must round-trip byte-for-byte — no reformatting"
    );
    assert!(
        rendered.ends_with(' '),
        "the trailing space specifically must survive — a trim step would silently pass the \
         plain-ASCII version of this test while failing the acceptance criterion"
    );
    assert!(
        rendered.contains('\u{0301}') && !rendered.contains('\u{00e9}'),
        "the decomposed accent must survive un-normalized — NFC would silently merge it into \
         the precomposed form, changing the byte sequence while `assert_eq!` on a NORMALIZED \
         comparison could still pass"
    );
    assert!(
        rendered.contains('\\') && !rendered.contains('/'),
        "Windows path separators must survive untouched — this validator must never call \
         anything shaped like normalise_path_separators on rendered slot text"
    );
}

// L's review finding on #254 (`5395616664`): `recommended` used to be a `usize`, unvalidated,
// and `render` indexed `decision.options[decision.recommended]` with it directly. The doc
// comment claimed "always 0 or 1 by construction" — untrue of a `pub usize` with no validating
// constructor. `recommended: 2` compiled and PANICKED inside the renderer whose whole declared
// purpose is that a declared failure never renders as success — a panic isn't even that; it's a
// crash the central invariant says nothing about.
//
// Confirmed RED-first before fixing: a `catch_unwind`-based test constructed `recommended: 2`
// under the old `usize` field and observed the panic directly (sealed prediction, held). Now
// fixed the same way `[DecisionOption; 2]` already was: `recommended: RecommendedOption` is a
// two-case enum, so `recommended: 2` no longer compiles — this is a T8-shaped compile-time
// claim now, not a runtime test, and the panic-demonstrating test was deleted rather than kept
// green by accident (its own sealed prediction said exactly this would happen).
