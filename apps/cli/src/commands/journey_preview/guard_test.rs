//! #2890: when a draft's preview may play a mutating `http` act.

use super::*;
use serde_json::json;

fn draft(launched: bool) -> Guard {
    Guard {
        approved: false,
        launched,
        confirmed: false,
        cli_confirmed: false,
        cli_reviewed: false,
    }
}

/// A draft plays a mutating `http` act only on an app the preview launched AND when the owner
/// marked its edge safe. Defects named: a mark that lets a draft mutate an app the preview did
/// not start, a mark that frees a `cli` or destructive non-http act, and a mark that changes an
/// approved flow's hold. Cost: microseconds.
#[test]
fn a_mutating_http_act_in_a_draft_needs_a_launched_app_and_the_owners_mark() {
    let post = json!({"kind":"http","role":"POST","name":"/__studio/add"});
    for (launched, marked, plays) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let expected = if plays { None } else { Some("guard_refused") };
        assert_eq!(
            draft(launched).stops_before(&post, marked),
            expected,
            "launched={launched} marked={marked}"
        );
    }
    // The mark frees an http act only: a command still waits for review, a destructive
    // command stays refused, a destructive click or a page tool is judged as before.
    let cli = json!({"kind":"cli","role":"exec","name":"sh greet.sh"});
    let wipe = json!({"kind":"cli","role":"exec","name":"git push --force"});
    let pay = json!({"kind":"click","role":"button","name":"Pay now"});
    let tool = json!({"kind":"tool","role":"webmcp","name":"rename_heading"});
    assert_eq!(draft(true).stops_before(&cli, true), Some("review_needed"));
    let reviewed = Guard {
        cli_reviewed: true,
        ..draft(true)
    };
    assert_eq!(reviewed.stops_before(&wipe, true), Some("guard_refused"));
    assert_eq!(draft(true).stops_before(&pay, true), None);
    assert_eq!(draft(true).stops_before(&tool, true), None);
    assert_eq!(draft(false).stops_before(&pay, true), Some("guard_refused"));
    // An approved flow holds a mutating http act for the owner's click whatever a mark says.
    let approved = Guard {
        approved: true,
        launched: true,
        ..draft(true)
    };
    assert_eq!(approved.stops_before(&post, true), Some("confirm_needed"));
    assert_eq!(approved.stops_before(&post, false), Some("confirm_needed"));
}

/// An edge counts as marked only at its current digest AND with the owner's record in the
/// project's store. Defects named: a hand-written digest (anyone who can write the file can
/// compute it) read as the owner's mark, and a mark that survives an edit of the acts.
/// Cost: one tempdir with an event store.
#[test]
fn only_an_edge_the_owner_marked_at_its_current_digest_counts_as_marked() {
    let project = tempfile::tempdir().unwrap();
    let store_dir = super::super::journey_owner::store(project.path());
    std::fs::create_dir_all(&store_dir).unwrap();
    drop(super::super::event_store(&store_dir).unwrap());
    let mut flow = json!({
        "id": "add-refusal",
        "status": "draft",
        "base": "http://127.0.0.1:1",
        "screens": [
            {"id": "start", "url": "/"},
            {"id": "refused", "url": "/__studio/add"}
        ],
        "edges": [
            {"id": "send", "from": "start", "to": "refused",
             "acts": [{"kind":"http","role":"POST","name":"/__studio/add"}]},
            {"id": "look", "from": "start", "to": "refused",
             "acts": [{"kind":"http","role":"GET","name":"/__studio/add"}]}
        ],
    });
    assert!(owner_marked_edges(&flow, project.path()).is_empty());
    // A digest nobody recorded: the file alone does not make the mark the owner's.
    let digest = super::super::journey_flow::mark_digest(&flow, &flow["edges"][0]);
    flow["edges"][0]["safe"] = json!({"digest": digest});
    assert!(owner_marked_edges(&flow, project.path()).is_empty());
    // The owner's record makes it theirs, for that edge only.
    super::super::journey_owner::record_mark(project.path(), "add-refusal", "send", &digest)
        .unwrap();
    assert_eq!(
        owner_marked_edges(&flow, project.path()),
        std::collections::BTreeSet::from(["send"])
    );
    // Editing the edge's acts voids the mark.
    flow["edges"][0]["acts"][0]["name"] = json!("/__studio/other");
    assert!(owner_marked_edges(&flow, project.path()).is_empty());
}
