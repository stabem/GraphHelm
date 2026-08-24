//! Steps 4 and 8: waivers, validity, and the record.
//!
//! The record is not bookkeeping. A contract that lists only what it INCLUDED cannot be audited:
//! a rule absent from the output is indistinguishable from a rule that was never declared, and
//! that is the question an owner asks when a rule they wrote did not take effect.

use chrono::{TimeZone, Utc};
use graphhelm_policy::{ResolutionRefusal, resolve_code_contract};
use graphhelm_protocols::{
    ArtifactId, DevelopmentEnvelope, DevelopmentKind, DevelopmentMetadata, DevelopmentScope,
    OpaqueId, ProjectId, SemanticVersion, WireHash, WorkspaceId,
};

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 24, 12, 0, 0)
        .single()
        .expect("fixed clock")
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

fn complete_waiver() -> serde_json::Value {
    serde_json::json!({
        "actor": "owner-local",
        "reason": "the billing module is frozen pending the migration",
        "acknowledgedRisks": ["coverage may regress while frozen"],
        "affectedContractVersion": "1.0.0",
        "affectedGraphVersion": "1.0.0",
        "resultStatus": "waived",
    })
}

// ---------------------------------------------------------------------------------------------
// G4 -- a forged or incomplete waiver is refused. TWO cells: completeness and class.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_waiver_missing_a_required_field_is_refused() {
    let mut waiver = complete_waiver();
    waiver
        .as_object_mut()
        .expect("object")
        .remove("acknowledgedRisks");

    let quality = rule(
        "rule-quality",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "quality", "selector": {}, "waiver": waiver,
        }),
    );

    let refusal = resolve_code_contract(&[quality], now())
        .expect_err("an incomplete waiver was accepted, so the override is unauditable");

    assert_eq!(
        refusal,
        ResolutionRefusal::WaiverInvalid {
            rule: "rule-quality".to_owned(),
            missing: "acknowledgedRisks".to_owned(),
        },
        "the refusal must name WHICH field is missing; a bare invalid-waiver sends the owner back \
         to diffing their override against a spec"
    );
}

#[test]
fn a_complete_waiver_against_a_structural_rule_is_refused_regardless() {
    // The waiver is complete. The CLASS refuses it, and the two must be separable: a cell that
    // only checked completeness would pass this, and a structural rule would become waivable by
    // anyone who filled the form in correctly.
    let structural = rule(
        "rule-structural",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "structural", "selector": {}, "waiver": complete_waiver(),
        }),
    );

    let refusal = resolve_code_contract(&[structural], now())
        .expect_err("a structural rule was waived, which the design says is impossible");

    assert_eq!(
        refusal,
        ResolutionRefusal::WaiverInvalid {
            rule: "rule-structural".to_owned(),
            missing: "<structural rules cannot be waived>".to_owned(),
        }
    );
}

#[test]
fn a_complete_waiver_against_a_quality_rule_is_honoured_and_recorded() {
    // POSITIVE CONTROL for both cells above: without it, a resolver that refused every waiver
    // would pass them, and waivers would simply not work.
    let quality = rule(
        "rule-quality",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "quality", "selector": {}, "waiver": complete_waiver(),
        }),
    );

    let contract = resolve_code_contract(&[quality], now()).expect("a complete quality waiver");

    assert_eq!(
        contract.requirements.get("coverage"),
        None,
        "a waived requirement must not still apply"
    );
    assert_eq!(contract.record.waived, vec!["rule-quality".to_owned()]);
    assert!(
        contract.record.included.is_empty(),
        "a waived rule is not an included one -- recording it in both makes the record useless for \
         answering whether a rule took effect"
    );
}

// ---------------------------------------------------------------------------------------------
// Step 8 -- the record distinguishes the ways a rule can fail to apply.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_expired_rule_is_recorded_as_expired_not_silently_absent() {
    let expired = rule(
        "rule-expired",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "structural", "selector": {},
            "validUntil": "2026-08-01T00:00:00Z",
        }),
    );
    let live = rule(
        "rule-live",
        serde_json::json!({
            "conflictKey": "lint", "operator": "minimum", "minimum": 1,
            "enforcement": "structural", "selector": {},
        }),
    );

    let contract = resolve_code_contract(&[expired, live], now()).expect("no conflict here");

    assert_eq!(contract.record.expired, vec!["rule-expired".to_owned()]);
    assert_eq!(contract.record.included, vec!["rule-live".to_owned()]);
    assert_eq!(
        contract.requirements.get("coverage"),
        None,
        "an expired rule must not contribute a requirement"
    );
}

#[test]
fn the_clock_is_an_input_so_one_rule_expires_only_against_a_later_reading() {
    // The acceptance criterion fixes inputs, CLOCK, scopes and snapshots. A resolver reading the
    // system clock would produce different contracts from identical inputs, and G7 would not
    // reliably catch it: two processes started in the same second agree by luck.
    let rules = [rule(
        "rule-timed",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "structural", "selector": {},
            "validUntil": "2026-08-24T13:00:00Z",
        }),
    )];

    let before = Utc
        .with_ymd_and_hms(2026, 8, 24, 12, 0, 0)
        .single()
        .expect("clock");
    let after = Utc
        .with_ymd_and_hms(2026, 8, 24, 14, 0, 0)
        .single()
        .expect("clock");

    assert_eq!(
        resolve_code_contract(&rules, before)
            .expect("live")
            .record
            .included,
        vec!["rule-timed".to_owned()]
    );
    assert_eq!(
        resolve_code_contract(&rules, after)
            .expect("expired")
            .record
            .expired,
        vec!["rule-timed".to_owned()]
    );
}

#[test]
fn a_rule_that_lost_to_a_more_specific_one_is_recorded_as_shadowed() {
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
            "conflictKey": "coverage", "operator": "minimum", "minimum": 95,
            "enforcement": "structural", "selector": { "module": "billing" },
        }),
    );

    let contract = resolve_code_contract(&[ancestor, descendant], now()).expect("strengthening");

    assert_eq!(contract.record.included, vec!["rule-descendant".to_owned()]);
    assert_eq!(
        contract.record.shadowed,
        vec!["rule-ancestor".to_owned()],
        "the ancestor did not fail and was not denied -- it was outranked, and the record has to \
         say which, because the next question is why that rule did not apply"
    );
}

#[test]
fn a_waiver_field_present_but_null_counts_as_missing() {
    // PRESENT is not the same question as SUPPLIED. The completeness check asked whether the key
    // exists, and `{"actor": null}` answers yes -- `get` returns `Some(Value::Null)`, so the field
    // reads as provided while carrying nothing.
    //
    // The cells above only ever REMOVE keys, so every one of them passes against a check that
    // cannot see an explicit null. That is the gap: the fixtures and the defect were shaped the
    // same way, and a whole family of malformed overrides sat outside what any of them could fail
    // on.
    let mut waiver = complete_waiver();
    waiver
        .as_object_mut()
        .expect("object")
        .insert("actor".to_owned(), serde_json::Value::Null);

    let quality = rule(
        "rule-quality",
        serde_json::json!({
            "conflictKey": "coverage", "operator": "minimum", "minimum": 90,
            "enforcement": "quality", "selector": {}, "waiver": waiver,
        }),
    );

    let refusal = resolve_code_contract(&[quality], now()).expect_err(
        "an override naming a null actor was accepted as complete, so the record would carry an \
         owner decision with nobody accountable for it",
    );

    assert_eq!(
        refusal,
        ResolutionRefusal::WaiverInvalid {
            rule: "rule-quality".to_owned(),
            missing: "actor".to_owned(),
        }
    );
}
