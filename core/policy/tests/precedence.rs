//! Steps 5 and 7 of the design's resolution order: COMPARE same-key requirements, then RESOLVE
//! precedence -- and refuse rather than invent an answer for either.
//!
//! The two refusals are deliberately distinct. An incomparable pair is fixed by reconciling the
//! values or registering an operator; an exhausted precedence is fixed by an owner task decision
//! or a declared priority. Opposite remedies, so one code for both would be the defect #247
//! records, committed inside a closed vocabulary.

use graphhelm_policy::{Dominance, ResolutionRefusal, dominance, resolve_code_contract};
use graphhelm_protocols::{
    ArtifactId, DevelopmentEnvelope, DevelopmentKind, DevelopmentMetadata, DevelopmentScope,
    OpaqueId, ProjectId, SemanticVersion, WireHash, WorkspaceId,
};

fn clock() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-08-24T12:00:00Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc)
}

fn rule(id: &str, spec: serde_json::Value) -> DevelopmentEnvelope {
    DevelopmentEnvelope {
        api_version: "p50.dev/development/v1alpha1".to_owned(),
        kind: DevelopmentKind::CodeRule,
        metadata: DevelopmentMetadata {
            id: ArtifactId::parse(id).expect("artifact id"),
            artifact_version: SemanticVersion::parse("1.0.0").expect("artifact version"),
            scope: DevelopmentScope {
                workspace_id: WorkspaceId::parse("workspace-1").expect("workspace"),
                project_id: ProjectId::parse("project-1").expect("project"),
                subproject_id: None,
                execution_id: None,
            },
        },
        producer: OpaqueId::parse("graphhelm-policy").expect("producer"),
        producer_version: SemanticVersion::parse("0.1.0").expect("producer version"),
        bindings: Vec::new(),
        spec,
        digest: WireHash::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        additional: serde_json::Map::new(),
    }
}

/// `enforcement` is one of structural | quality | preference; `selector` is a predicate map.
fn structural(
    id: &str,
    key: &str,
    minimum: u32,
    selector: serde_json::Value,
) -> DevelopmentEnvelope {
    rule(
        id,
        serde_json::json!({
            "conflictKey": key,
            "operator": "minimum",
            "minimum": minimum,
            "enforcement": "structural",
            "selector": selector,
        }),
    )
}

// ---------------------------------------------------------------------------------------------
// G1 -- a descendant cannot weaken a mandatory rule.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_descendant_weakening_a_structural_rule_is_refused_and_recorded_not_dropped() {
    let ancestor = structural("rule-ancestor", "coverage", 90, serde_json::json!({}));
    // Strictly more specific selector, but a WEAKER value under `minimum`.
    let descendant = structural(
        "rule-descendant",
        "coverage",
        70,
        serde_json::json!({ "module": "billing" }),
    );

    let refusal = resolve_code_contract(&[ancestor, descendant], clock())
        .expect_err("a descendant weakened a structural requirement and the resolver allowed it");

    assert_eq!(
        refusal,
        ResolutionRefusal::Conflict {
            conflict_key: "coverage".to_owned(),
            rules: vec!["rule-ancestor".to_owned(), "rule-descendant".to_owned()],
        },
        "the refusal must name the key AND both rules -- a refusal that drops the loser makes the \
         conflict unreconstructable from the record"
    );
}

#[test]
fn a_descendant_strengthening_the_same_rule_is_accepted() {
    // POSITIVE CONTROL for G1. Without it, a resolver that refused every same-key pair would pass
    // the cell above while making layered rules impossible.
    let ancestor = structural("rule-ancestor", "coverage", 90, serde_json::json!({}));
    let descendant = structural(
        "rule-descendant",
        "coverage",
        95,
        serde_json::json!({ "module": "billing" }),
    );

    let contract = resolve_code_contract(&[ancestor, descendant], clock())
        .expect("strengthening is the whole point of layering");

    assert_eq!(contract.requirements.get("coverage"), Some(&95));
}

// ---------------------------------------------------------------------------------------------
// G3 -- incomparable selectors refuse rather than pick.
// ---------------------------------------------------------------------------------------------

#[test]
fn selectors_differing_on_dimensions_neither_contains_refuse_instead_of_ordering() {
    // One predicates language, the other module. Neither contains the other. The design's sentence
    // is worth keeping verbatim: "language never silently outranks module."
    let by_language = structural(
        "rule-language",
        "coverage",
        80,
        serde_json::json!({ "language": "rust" }),
    );
    let by_module = structural(
        "rule-module",
        "coverage",
        85,
        serde_json::json!({ "module": "billing" }),
    );

    let refusal = resolve_code_contract(&[by_language, by_module], clock()).expect_err(
        "two selectors on different dimensions were ordered, so some precedence table decided \
         what the design says must not be decided",
    );

    assert_eq!(
        refusal,
        ResolutionRefusal::PrecedenceUnresolved {
            conflict_key: "coverage".to_owned(),
            rules: vec!["rule-language".to_owned(), "rule-module".to_owned()],
        },
        "incomparable SELECTORS are a precedence failure, not a value conflict: the values are \
         perfectly comparable, it is the ordering between the rules that is missing"
    );
}

// ---------------------------------------------------------------------------------------------
// Dominance is three-valued. A comparator that always answers is the failure mode.
// ---------------------------------------------------------------------------------------------

#[test]
fn dominance_has_a_third_answer_and_it_is_not_spelled_false() {
    let empty = serde_json::json!({});
    let language = serde_json::json!({ "language": "rust" });
    let language_and_module = serde_json::json!({ "language": "rust", "module": "billing" });
    let module = serde_json::json!({ "module": "billing" });

    // Strictly more predicates, same values: dominates.
    assert_eq!(
        dominance(&language_and_module, &language),
        Dominance::Dominates
    );
    assert_eq!(
        dominance(&language, &language_and_module),
        Dominance::DominatedBy
    );
    // The empty selector is dominated by anything that adds a predicate.
    assert_eq!(dominance(&language, &empty), Dominance::Dominates);
    // Same selector: neither adds a predicate, so neither dominates -- and this is NOT the same
    // answer as "different dimensions".
    assert_eq!(dominance(&language, &language), Dominance::Equal);
    // Different dimensions: incomparable. If this ever returns an ordering, G3 dies quietly.
    assert_eq!(dominance(&language, &module), Dominance::Incomparable);
    assert_eq!(dominance(&module, &language), Dominance::Incomparable);
}

#[test]
fn a_conflicting_value_on_the_same_dimension_is_incomparable_not_dominant() {
    // Same dimension, different value. More predicates does not help: the rules describe disjoint
    // populations, so neither can strengthen the other.
    let rust = serde_json::json!({ "language": "rust" });
    let python = serde_json::json!({ "language": "python" });

    assert_eq!(dominance(&rust, &python), Dominance::Incomparable);
}

// ---------------------------------------------------------------------------------------------
// The operator set is closed, and an unregistered one cannot establish strengthening.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unregistered_operator_cannot_establish_strengthening() {
    // The blueprint named this sabotage: "make the operator check return compatible on unknown".
    // An operator that defaults to permit lets a descendant weaken an ancestor while every other
    // cell stays green, because the values themselves look fine.
    let ancestor = rule(
        "rule-ancestor",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "structural", "selector": {},
        }),
    );
    let descendant = rule(
        "rule-descendant",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "lexicographic_vibes", "minimum": 95,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );

    let refusal = resolve_code_contract(&[ancestor, descendant], clock()).expect_err(
        "an unregistered operator established strengthening — a descendant can now weaken an \
         ancestor by naming an operator nobody registered",
    );

    assert_eq!(
        refusal,
        ResolutionRefusal::Conflict {
            conflict_key: "coverage".to_owned(),
            rules: vec!["rule-ancestor".to_owned(), "rule-descendant".to_owned()],
        },
        "the value 95 is NUMERICALLY stronger than 90, so a cell that only compared numbers would \
         pass here; the refusal has to come from the operator not being registered"
    );
}

#[test]
fn two_rules_with_the_same_selector_and_an_unregistered_operator_refuse_rather_than_sort() {
    // THE THIRD SITE. `Dominates` and `DominatedBy` both use `strengthens` as a GATE and refuse
    // when it answers false. `Equal` used the same function as a TIEBREAKER -- false simply meant
    // "keep the other one" -- so with an operator nobody registered, neither rule could establish
    // strengthening and the winner was whichever sorted first. That is the default-permit failure
    // the operator check exists to prevent, surviving in the one arm that had no gate.
    //
    // Same selector on purpose: the cell above uses DIFFERENT selectors, so it only ever exercises
    // the `Dominates` arm and cannot see this.
    let first = rule(
        "rule-aaa",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "lexicographic_vibes", "minimum": 70,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );
    let second = rule(
        "rule-zzz",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "lexicographic_vibes", "minimum": 95,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );

    let refusal = resolve_code_contract(&[first, second], clock()).expect_err(
        "two rules on one key, neither able to establish strengthening, resolved anyway -- the \
         winner was decided by sort order, which is what an unregistered operator must not be \
         allowed to decide",
    );

    assert_eq!(
        refusal,
        ResolutionRefusal::Conflict {
            conflict_key: "coverage".to_owned(),
            rules: vec!["rule-aaa".to_owned(), "rule-zzz".to_owned()],
        }
    );
}

#[test]
fn two_rules_with_the_same_selector_and_a_registered_operator_still_take_the_stronger() {
    // POSITIVE CONTROL for the cell above. Refusing whenever `strengthens` answers false would
    // break ordinary layering: two `minimum` rules on one key are perfectly comparable and the
    // stronger must simply win. The fix has to separate "cannot be compared" from "compared and
    // weaker" -- returning false for both is the flattening this lane filed as #247, committed
    // here against itself.
    let weaker = rule(
        "rule-aaa",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 70,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );
    let stronger = rule(
        "rule-zzz",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 95,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );

    let contract =
        resolve_code_contract(&[weaker, stronger], clock()).expect("both are plain minimums");

    assert_eq!(contract.requirements.get("coverage"), Some(&95));
    assert_eq!(contract.record.included, vec!["rule-zzz".to_owned()]);
    assert_eq!(contract.record.shadowed, vec!["rule-aaa".to_owned()]);
}

/// The same three rules under two namings. Only the identifiers differ.
fn incomparable_triple(wide: &str, language: &str, module: &str) -> Vec<DevelopmentEnvelope> {
    vec![
        rule(
            wide,
            serde_json::json!({
                "conflictKey": "coverage", "operator": "minimum", "minimum": 95,
                "enforcement": "structural",
                "selector": { "language": "rust", "module": "billing" },
            }),
        ),
        rule(
            language,
            serde_json::json!({
                "conflictKey": "coverage", "operator": "minimum", "minimum": 80,
                "enforcement": "structural", "selector": { "language": "rust" },
            }),
        ),
        rule(
            module,
            serde_json::json!({
                "conflictKey": "coverage", "operator": "minimum", "minimum": 70,
                "enforcement": "structural", "selector": { "module": "billing" },
            }),
        ),
    ]
}

#[test]
fn renaming_a_rule_cannot_change_whether_an_incomparable_pair_is_found() {
    // `Incomparable` is NOT TRANSITIVE, and the fold compared each rule only against the running
    // winner -- n-1 comparisons where incomparability is a property of all n(n-1)/2 pairs. The
    // language-scoped and module-scoped rules are incomparable with each other and BOTH dominated
    // by the wide one, so whether they ever meet depends on which the fold happens to be holding.
    //
    // The fold's order is the sorted rule ID, so RENAMING a rule -- an edit with no semantic
    // content whatsoever -- would decide whether step 7 fires. That was the defect: the same rule
    // set resolving two ways depending on what its rules are called.
    //
    // **It no longer works that way, and the comment used to say otherwise.** The refusal this
    // test observes does not come from step 7 at all. It comes from the PRE-PASS in
    // `resolve_code_contract` that scans every pair before the fold and sorts the pair it names,
    // which is what makes the outcome independent of the naming. Step 7 is never reached for this
    // fixture.
    //
    // Measured, not read: three sabotages were needed to find this. The first two changed the
    // refusal variant at the step 7 site and the test stayed GREEN -- not because it was blind,
    // but because that branch never executes here. **The pre-pass is the load-bearing half**, and
    // it is what a future regression would have to break for this cell to matter.
    let wide_first = incomparable_triple("rule-w", "rule-x", "rule-y");
    let narrow_first = incomparable_triple("rule-c", "rule-a", "rule-b");

    let first = resolve_code_contract(&wide_first, clock());
    let second = resolve_code_contract(&narrow_first, clock());

    // The property is that the same PAIR is found, named by ROLE rather than by identifier. Both
    // namings must refuse the same way about the same two rules -- the language-scoped one and the
    // module-scoped one -- whatever those two happen to be called.
    //
    // `first.is_err() && second.is_err()` was weaker than the message it carried: one naming could
    // refuse `Conflict` and the other `PrecedenceUnresolved` and this cell would stay green while
    // the property was dead.
    //
    // And `assert_eq!(first, second)` is NOT the repair, which is worth writing down because it is
    // the obvious one and it is wrong. The refusal carries the rule NAMES, and the names differ by
    // construction -- renaming them is the entire experiment. Measured before this was written: it
    // fails today on `["rule-x", "rule-y"]` vs `["rule-a", "rule-b"]`, a difference that is the
    // fixture working rather than a defect. Each side is pinned to its own role-mapped
    // expectation instead.
    let refuses_about = |language: &str, module: &str| {
        Err(ResolutionRefusal::PrecedenceUnresolved {
            conflict_key: "coverage".to_owned(),
            rules: vec![language.to_owned(), module.to_owned()],
        })
    };
    assert_eq!(
        first,
        refuses_about("rule-x", "rule-y"),
        "the wide-first naming did not refuse about the language/module pair. Incomparability is a property of a PAIR, so it cannot depend on which rule the fold is holding when the pair comes up"
    );
    assert_eq!(
        second,
        refuses_about("rule-a", "rule-b"),
        "the narrow-first naming did not refuse about the language/module pair. Same three rules, different names, and the same pair must come out"
    );
}
