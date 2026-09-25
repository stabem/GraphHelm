//! The mold guard (M08 step 3): only the interaction-cost pair carries a trace.
//!
//! The suite digest is sha256 over the canonical JSON of the WHOLE specimen list. If a
//! trace appeared on one of the original ten, the digest would move because the MOLD
//! changed rather than because the suite grew — and since adding specimens voids
//! certification anyway, that error would arrive inside a green. `skip_serializing_if` is
//! what keeps the ten byte-identical; this test is what keeps the claim true.
//!
//! `None` here means "makes NO CLAIM about interaction", never "zero calls". Asserted, not
//! merely documented: prose and check diverge, and review reads the prose.

use pathogens::{UselessnessMode, suite};

#[test]
fn only_the_interaction_cost_pair_carries_a_trace() {
    for specimen in suite() {
        let is_interaction_cost = matches!(
            specimen.axis,
            UselessnessMode::ExpensiveButCorrect | UselessnessMode::DumpedButUnanswered
        );
        assert_eq!(
            specimen.evidence.interaction.is_some(),
            is_interaction_cost,
            "{}: only the interaction-cost pair may carry a trace — a trace on one of the \
             original ten moves the suite digest by changing the mold, not by growing the \
             suite",
            specimen.id
        );
    }
}

#[test]
fn an_absent_trace_serializes_to_nothing_at_all() {
    let ten: Vec<_> = suite()
        .into_iter()
        .filter(|specimen| specimen.evidence.interaction.is_none())
        .collect();
    // Bound, with a message that stays true if the suite grows: a thirteenth specimen
    // WITHOUT a trace is legitimate, and the old wording ("the original ten") would have
    // described something else while failing. Agent B's note.
    assert_eq!(
        ten.len(),
        suite().len() - 2,
        "every specimen except the interaction-cost pair makes no interaction claim"
    );
    let json = serde_json::to_string(&ten).expect("specimens serialize");
    assert!(
        !json.contains("interaction"),
        "an absent trace must not appear on the wire at all: without \
         skip_serializing_if the ten would carry \"interaction\":null and their bytes — and \
         therefore the suite digest — would move for the wrong reason"
    );
}
