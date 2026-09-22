//! #163: the scan history as ONE typed value, rendered by `render()` for both `execution status`
//! and `GET /v1/executions/{id}`. Nothing here derives: every number is copied from the fold.
//! `quarantined_nodes` is the deduplicated set of NODES named by `open_claims`' values — that map
//! is keyed by CLAIM SEQUENCE, not by node. The fold maintains it (inserted on
//! `completion_claimed`, removed on cleared/rejected), and a second computation over the scan
//! history would be a duplicate oracle that could disagree with it after a fold change.

use std::collections::{BTreeMap, BTreeSet};

use graphhelm_events::{ClearanceOutcome, CustomsScan, ExecutionProjection, OpenWait};
use graphhelm_protocols::{PersistedTimestamp, WireHash};
use serde::{Deserialize, Serialize};

/// An open claim as the status surface shows it: the fold's `OpenClaim` plus the sequence that
/// keys it in `open_claims`, which is the claim's identity and the number `clear` takes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClaimView {
    pub claim_seq: u64,
    pub completes_wait_seq: u64,
    pub stage_entered_at: u64,
    /// Copied from the fold, never recomputed here; `None` on every stream `execution start`
    /// creates today (declared gap, spec §5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<PersistedTimestamp>,
    pub evidence_digest: WireHash,
}

/// One node's customs timeline: every scan the fold recorded, and whatever is open on it now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCustomsView {
    pub scans: Vec<CustomsScan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_wait: Option<OpenWait>,
    /// The node's OLDEST open claim — lowest claim sequence, `open_claims` being a `BTreeMap`.
    ///
    /// Singular because the `claim` verb refuses `duplicate_completion` on a node that already
    /// has one, which is the ONLY thing that keeps a node to one claim. The fold does not: its
    /// `completion_claimed` arm inserts whenever the event answers the node's open wait, and a
    /// claim does not close that wait, so a journal carrying two such events replays into two
    /// open claims on one node. One-per-node is therefore a property of the door, not an
    /// invariant of the log — so the extras are NAMED rather than dropped, below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_claim: Option<OpenClaimView>,
    /// Every OTHER open claim's sequence on this node, ascending. Empty — and absent from the
    /// wire — in every journal the `claim` verb wrote, which is why the field does not widen the
    /// shape a reader normally sees. Non-empty says the log holds more claims on this node than
    /// `open_claim` shows, and gives the sequences to go read them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_open_claim_seqs: Vec<u64>,
}

/// The whole execution's customs state, as `render()` publishes it under `customs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomsView {
    /// The nodes an open claim names — the quarantine, read off `open_claims` and nowhere else.
    pub quarantined_nodes: Vec<String>,
    pub nodes: BTreeMap<String, NodeCustomsView>,
    /// Every clearance verdict by the claim sequence it judged.
    pub clearances: BTreeMap<u64, ClearanceOutcome>,
}

/// Build the view from the fold's own maps. A node appears when any of the three maps names it,
/// so a node whose only trace is a refused claim still has a timeline to show.
#[must_use]
pub fn customs_view(projection: &ExecutionProjection) -> CustomsView {
    let mut names: BTreeSet<&str> = projection
        .customs_scans
        .keys()
        .map(String::as_str)
        .collect();
    names.extend(projection.open_waits.keys().map(String::as_str));
    names.extend(
        projection
            .open_claims
            .values()
            .map(|claim| claim.node.as_str()),
    );

    let mut nodes = BTreeMap::new();
    for name in names {
        // Ascending by claim sequence, `open_claims` being a `BTreeMap`: the first is the node's
        // oldest open claim and the rest are the ones a single `Option` would have dropped in
        // silence. See `NodeCustomsView::open_claim` for why "the rest" is reachable at all.
        let mut open_on_node = projection
            .open_claims
            .iter()
            .filter(|(_, claim)| claim.node == name);
        let open_claim = open_on_node.next().map(|(seq, claim)| OpenClaimView {
            claim_seq: *seq,
            completes_wait_seq: claim.completes_wait_seq,
            stage_entered_at: claim.stage_entered_at,
            deadline: claim.deadline.clone(),
            evidence_digest: claim.evidence_digest.clone(),
        });
        let additional_open_claim_seqs: Vec<u64> = open_on_node.map(|(seq, _)| *seq).collect();
        nodes.insert(
            name.to_owned(),
            NodeCustomsView {
                scans: projection
                    .customs_scans
                    .get(name)
                    .cloned()
                    .unwrap_or_default(),
                open_wait: projection.open_waits.get(name).cloned(),
                open_claim,
                additional_open_claim_seqs,
            },
        );
    }

    let mut quarantined: Vec<String> = projection
        .open_claims
        .values()
        .map(|claim| claim.node.clone())
        .collect();
    quarantined.sort();
    quarantined.dedup();

    CustomsView {
        quarantined_nodes: quarantined,
        nodes,
        clearances: projection.clearances.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// #1184 review (pass B): ONE customs completion policy, because this repository has TWO dispatch
// drivers and they had already drifted.
//
// `core/runtime/src/driver.rs::drive_to_quiescence_async` serves the Public Runtime API; the
// synchronous `apps/cli/src/commands/execution/driver.rs::drive_to_quiescence` serves
// `execution start` on the CLI. The park was added to the first and not the second, so a customs
// node started from the CLI reached `Succeeded` and completed without ever opening a wait -- the
// declaration inert on exactly the path an operator uses by hand.
//
// The predicate and the refusal vocabulary live HERE, in the crate both drivers already depend on,
// so a future change to either cannot move one without the other. The DIAGNOSTIC each driver
// raises stays with that driver, because the error types differ; what must not differ is the
// answer to "does this node gate its own completion" and the code that names an unreadable block.
// ---------------------------------------------------------------------------------------------

/// The refusal code for a `completion.customs` block that does not deserialize.
///
/// Shared rather than repeated: two drivers refusing the same condition under two codes is a
/// distinction with no meaning that an operator would have to learn.
pub const CUSTOMS_DECLARATION_INVALID_CODE: &str = "GHG017_CUSTOMS_DECLARATION_INVALID";

/// Whether this node's own declaration says its completion must clear customs.
///
/// A node that names proof kinds does not get to declare itself done. Its work runs, its evidence
/// is sealed, and then it PARKS (`NodeOutcome::NeedsInput` -> `NodeState::WaitingInput`) until a
/// claim presenting that proof is cleared -- at which point the fold's `CompletionCleared` arm
/// writes `Succeeded` directly, so the work never re-runs.
///
/// An EMPTY `proofKinds` list is not a gate. The field's own serde default is an empty vector, so
/// treating empty as "park" would gate every node that declared budgets and nothing else, on a
/// requirement its author never wrote down.
///
/// A malformed block answers TRUE -- fail-closed. Both drivers refuse such a graph before any node
/// runs (see [`unreadable_customs_nodes`]), so this arm is unreachable on either dispatch path
/// today; it is written this way so that a future caller which skips the preflight fails into a
/// node that waits for a person rather than one that quietly certifies itself.
#[must_use]
pub fn completion_is_gated(node: &graphhelm_protocols::GraphNode) -> bool {
    match node.customs() {
        Ok(Some(customs)) => !customs.proof_kinds.is_empty(),
        Ok(None) => false,
        Err(_) => true,
    }
}

/// The same question against a spec and a node id, for a driver that holds the graph rather than
/// the node. An id the spec does not carry is NOT gated: there is no declaration to read, and
/// inventing a gate for an absent node would refuse work nobody described.
#[must_use]
pub fn completion_is_gated_in(spec: &graphhelm_protocols::GraphSpec, node: &str) -> bool {
    spec.nodes.get(node).is_some_and(completion_is_gated)
}

/// Every node whose `completion.customs` block is present and does not deserialize, in spec order.
///
/// REFUSED BEFORE THE EXECUTION STARTS, which is why this returns the names rather than a verdict:
/// each driver owns its own diagnostic type and path grammar, and only the CODE and the CONDITION
/// are shared. By the time a park decision runs the node's work has already happened, so a block
/// that cannot be read there has no honest answer left -- completing the node would spend the
/// declaration silently, parking it would invent a gate nobody could have declared.
#[must_use]
pub fn unreadable_customs_nodes(spec: &graphhelm_protocols::GraphSpec) -> Vec<String> {
    spec.nodes
        .iter()
        .filter(|(_, node)| node.customs().is_err())
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::{CustomsStage, OpenClaim};

    fn digest(fill: &str) -> WireHash {
        WireHash::parse(format!("sha256:{}", fill.repeat(64))).unwrap()
    }

    #[test]
    fn a_projection_with_no_customs_activity_renders_an_empty_view() {
        let projection = ExecutionProjection::default();
        let view = customs_view(&projection);
        assert!(view.quarantined_nodes.is_empty());
        assert!(view.nodes.is_empty());
        assert!(view.clearances.is_empty());
    }

    #[test]
    fn an_open_claim_names_its_node_as_quarantined_and_the_view_copies_the_folds_deadline_verbatim()
    {
        let mut projection = ExecutionProjection::default();
        let deadline = PersistedTimestamp::parse("2026-09-11T00:00:00Z").unwrap();
        projection.open_claims.insert(
            7,
            OpenClaim {
                node: "implementation".to_owned(),
                completes_wait_seq: 4,
                stage_entered_at: 7,
                deadline: Some(deadline.clone()),
                evidence_digest: digest("c"),
            },
        );
        projection.customs_scans.insert(
            "implementation".to_owned(),
            vec![CustomsScan {
                at_sequence: 7,
                stage: CustomsStage::Claimed,
                claim_seq: Some(7),
                reason_code: None,
                deadline: Some(deadline.clone()),
            }],
        );
        let view = customs_view(&projection);
        assert_eq!(view.quarantined_nodes, vec!["implementation".to_owned()]);
        let node = &view.nodes["implementation"];
        assert_eq!(node.open_claim.as_ref().unwrap().claim_seq, 7);
        assert_eq!(node.open_claim.as_ref().unwrap().deadline, Some(deadline));
        assert_eq!(node.scans.len(), 1);
    }

    /// Two open claims on ONE node: the fold accepts them (its `completion_claimed` arm inserts
    /// whenever the event answers the node's still-open wait, and a claim does not close that
    /// wait), so the view must not answer with one and say nothing about the other.
    #[test]
    fn a_second_open_claim_on_one_node_is_named_rather_than_silently_dropped() {
        let mut projection = ExecutionProjection::default();
        for seq in [7_u64, 9] {
            projection.open_claims.insert(
                seq,
                OpenClaim {
                    node: "implementation".to_owned(),
                    completes_wait_seq: 4,
                    stage_entered_at: seq,
                    deadline: None,
                    evidence_digest: digest("c"),
                },
            );
        }

        let view = customs_view(&projection);
        assert_eq!(
            view.quarantined_nodes,
            vec!["implementation".to_owned()],
            "the quarantine is a set of NODES, so two claims on one node name it once"
        );
        let node = &view.nodes["implementation"];
        assert_eq!(
            node.open_claim.as_ref().unwrap().claim_seq,
            7,
            "the oldest claim is the one shown"
        );
        assert_eq!(
            node.additional_open_claim_seqs,
            vec![9],
            "the second claim is NAMED: a bare Option would have dropped it in silence"
        );

        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(
            value["nodes"]["implementation"]["additionalOpenClaimSeqs"],
            serde_json::json!([9]),
            "and it reaches the wire"
        );
    }

    #[test]
    fn serialization_is_camel_case_and_omits_absent_optionals() {
        let mut projection = ExecutionProjection::default();
        // A node whose only trace is a refused claim: no open wait, no open claim, so both
        // optionals are absent and must STAY absent on the wire.
        projection.customs_scans.insert(
            "implementation".to_owned(),
            vec![CustomsScan {
                at_sequence: 2,
                stage: CustomsStage::Refused,
                claim_seq: None,
                reason_code: Some("not_waiting".to_owned()),
                deadline: None,
            }],
        );
        // One verdict of each arm, so `clearances` is pinned NON-EMPTY on the wire: an empty map
        // serializes the same whatever the verdict's tag spelling is, and the tag is the thing the
        // Studio and the CLI test (`replayed["data"]["clearances"][seq]["type"]`) read.
        projection.clearances.insert(3, ClearanceOutcome::Cleared);
        projection.clearances.insert(
            5,
            ClearanceOutcome::Refused {
                reason_code: graphhelm_protocols::SafeCode::parse("hash_mismatch").unwrap(),
            },
        );
        let view = customs_view(&projection);
        assert!(view.nodes["implementation"].open_wait.is_none());
        assert!(view.nodes["implementation"].open_claim.is_none());

        let value = serde_json::to_value(&view).unwrap();
        let top: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(top, ["clearances", "nodes", "quarantinedNodes"]);
        let node: Vec<&String> = value["nodes"]["implementation"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(node, ["scans"], "absent optionals are omitted, not nulled");
        assert_eq!(
            value["clearances"],
            serde_json::json!({
                "3": {"type": "cleared"},
                "5": {"type": "refused", "reasonCode": "hash_mismatch"},
            }),
            "the fold's verdicts are copied verbatim, keyed by claim sequence, tagged by `type`"
        );
    }
}
