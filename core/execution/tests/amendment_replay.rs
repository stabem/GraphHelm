//! Step 2's whole contract, named as a failing test BEFORE any schema exists.
//!
//! The reviewer asked for this shape first, and it is the right order: this test says what
//! the feature MEANS — append-only, scoped by sequence, no retcon — and it cannot compile
//! until the type and the ordered lookup exist. A schema written first would let the meaning
//! be decided by whatever the code happened to do.
//!
//! The claim under test, in one sentence: **an amendment declares a bound from its own
//! sequence FORWARD, and the past stays honestly unjudged.** A replay positioned before the
//! amendment must still answer `Unknown`. The timeline reads "not judged for twelve minutes,
//! judged from here" — never "it was fine all along".
//!
//! Sabotage A1 (the reviewer's central one) attacks exactly this: patch the fold to apply
//! amendments from sequence zero and this test must go red. If it stays green, "forward" is
//! an adjective rather than a behaviour, and the whole idea is decoration.

use graphhelm_execution::{AttentionInputs, Verdict, attention};

/// A stream carrying: an execution start, a node left in flight, and NO declared bound for
/// it. The store state that makes the question exist (sabotage A7): a node genuinely running
/// and genuinely unbudgeted, not a story driven to quiescence where there is nothing to
/// amend and a guard would compare two empty answers.
fn projection_with_unbudgeted_node_in_flight() -> graphhelm_events::ExecutionProjection {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-amend".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection
        .node_states
        .insert("judge".to_owned(), graphhelm_protocols::NodeState::Running);
    projection.declared_form = Some(graphhelm_protocols::ExecutionFormDeclared {
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-amend").unwrap(),
        node_ids: vec![graphhelm_protocols::OpaqueId::parse("judge").unwrap()],
        // Declared with NO timeout: this is the judge's own situation.
        node_timeout_seconds: std::collections::BTreeMap::new(),
        name: None,
        objective: None,
        executor: None,
        // These fixtures are about the declared form's OTHER fields; a graph that declares no
        // customs produces an empty map, which is what keeps each cell asking its own question.
        node_customs_budgets: std::collections::BTreeMap::new(),
    });
    projection
}

#[test]
fn an_amendment_binds_forward_and_never_repaints_the_past() {
    let mut projection = projection_with_unbudgeted_node_in_flight();
    // Built per read, from whatever projection is being asked: the budgets are a FUNCTION of
    // the moment, which is the property under test.
    fn silent_for(
        projection: &graphhelm_events::ExecutionProjection,
        seconds: u64,
    ) -> AttentionInputs {
        AttentionInputs {
            node_silence_seconds: [("judge".to_owned(), seconds)].into_iter().collect(),
            silence_budget_seconds: graphhelm_execution::effective_budgets(projection),
            at_sequence: Some(projection.amendment_head()),
        }
    }

    // BEFORE: no declaration anywhere, so the answer is the honest unknown.
    let before = attention(&projection, &silent_for(&projection, 720));
    assert!(
        matches!(before.verdict, Verdict::Unknown { .. }),
        "an unbudgeted node in flight is unknown: {before:?}"
    );

    // The operator declares a bound, valid FROM THIS SEQUENCE ON.
    projection.apply_amendment(graphhelm_protocols::ExecutionFormAmended {
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-amend").unwrap(),
        computed_at_sequence: projection.amendment_head(),
        node_timeout_seconds: [(
            graphhelm_protocols::OpaqueId::parse("judge").unwrap(),
            60_u64,
        )]
        .into_iter()
        .collect(),
        observed_silence_seconds: [(
            graphhelm_protocols::OpaqueId::parse("judge").unwrap(),
            720_u64,
        )]
        .into_iter()
        .collect(),
    });

    // AFTER: the same silence is now judged, and it is over the declared bound -- so the
    // answer is NeedsYou. Sabotage A2 attacks this: a bound smaller than the silence already
    // accumulated must never resolve to calm, or an amendment becomes a tool for hiding a
    // stuck run.
    let after = attention(&projection, &silent_for(&projection, 720));
    assert!(
        matches!(after.verdict, Verdict::NeedsYou { .. }),
        "a bound smaller than the silence already accumulated resolves to NEEDS YOU, never \
         to calm -- an amendment is a declaration, not an eraser: {after:?}"
    );

    // AND THE PAST STAYS UNJUDGED. This is the sentence the whole step exists for.
    let replayed_before = projection.as_of(projection.amendment_head() - 1);
    let then = attention(&replayed_before, &silent_for(&replayed_before, 720));
    assert!(
        matches!(then.verdict, Verdict::Unknown { .. }),
        "replayed before the amendment, the answer is STILL unknown: the operator declared a \
         bound going forward, and nothing they type can make the system claim it knew \
         something it did not: {then:?}"
    );
}

fn amend(projection: &mut graphhelm_events::ExecutionProjection, budget: u64, observed: u64) {
    let node = graphhelm_protocols::OpaqueId::parse("judge").unwrap();
    projection.apply_amendment(graphhelm_protocols::ExecutionFormAmended {
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-amend").unwrap(),
        computed_at_sequence: projection.amendment_head(),
        node_timeout_seconds: [(node.clone(), budget)].into_iter().collect(),
        observed_silence_seconds: [(node, observed)].into_iter().collect(),
    });
}

fn read(projection: &graphhelm_events::ExecutionProjection, silence: u64) -> Verdict {
    attention(
        projection,
        &AttentionInputs {
            node_silence_seconds: [("judge".to_owned(), silence)].into_iter().collect(),
            silence_budget_seconds: graphhelm_execution::effective_budgets(projection),
            at_sequence: Some(projection.amendment_head()),
        },
    )
    .verdict
}

/// Sabotage A9, the roulette, reproduced exactly as the reviewer measured it.
///
/// He amended twice on a node silent for 900 seconds: 30s produced `NeedsYou`, then 100000s
/// produced `CanSleep`. The arithmetic is right — 900 < 100000 — and the system must not
/// refuse an operator's declaration. The defect was that `CanSleep` carried NOTHING, so two
/// different worlds left identical bytes: a calm nobody ever contested, and a calm that
/// replaced an alarm because someone raised the ceiling afterwards.
///
/// The log holds both amendments, so an auditor could reconstruct it. A surface that needs an
/// auditor to be honest is not honest. This is `attentionRequired: false` beside
/// `silenceUnevaluated: ['judge']` one level down.
#[test]
fn raising_the_ceiling_over_a_live_alarm_is_not_the_same_answer_as_untroubled_calm() {
    let mut projection = projection_with_unbudgeted_node_in_flight();
    let silence = 900;

    amend(&mut projection, 30, silence);
    assert!(
        matches!(read(&projection, silence), Verdict::NeedsYou { .. }),
        "past its declared bound, the node is silent"
    );

    amend(&mut projection, 100_000, silence);
    let bought = read(&projection, silence);
    assert!(
        !matches!(bought, Verdict::CanSleep),
        "a calm purchased by raising the ceiling must not leave the same bytes as a calm \
         nobody contested: {bought:?}"
    );
    assert_eq!(
        bought,
        Verdict::CalmedByAmendment {
            nodes: graphhelm_execution::NonEmpty::new(vec![graphhelm_execution::PurchasedCalm {
                node: "judge".to_owned(),
                silence_seconds: 900,
                superseded_budget_seconds: 30,
                budget_seconds: 100_000,
            }])
            .expect("one node"),
        },
        "and it names BOTH ENDS of the trade -- cause without magnitude is the jargon the \
         judge already refused once: {bought:?}"
    );

    // A THIRD spin, upward again. Compared against its immediate predecessor this would show
    // nothing; compared against the tightest promise ever made, the purchase is still
    // visible. That is why `strictest_declared_bound` takes a minimum over the whole chain.
    amend(&mut projection, 100_001, silence);
    assert!(
        matches!(
            read(&projection, silence),
            Verdict::CalmedByAmendment { .. }
        ),
        "a chain of raises does not launder the first one"
    );

    // And an untroubled calm stays bare: a node quiet inside a bound nobody ever tightened
    // has nothing to declare, which is why CanSleep still carries no payload.
    let mut untroubled = projection_with_unbudgeted_node_in_flight();
    amend(&mut untroubled, 5_000, 10);
    assert_eq!(
        read(&untroubled, 10),
        Verdict::CanSleep,
        "nobody raised anything here, so there is nothing to say"
    );
}
