//! Site 3 (spec D3, D4, D6): per-node judgments enter the compiler only as repairable
//! diagnostics, an absent judge is today's bytes, and an answer below the acting threshold does
//! nothing but is reported. The draft door is `RecordedDraftModel` and the judge door is
//! `RecordedJudgeModel`; both fixtures under `fixtures/judge/` carry an authored half (`rounds`)
//! and a derived half (`replies` / `answers`) re-recorded under `ARCHITECT_RECORD=1`, the way
//! `golden.rs` records (see `fixtures/README.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use graphhelm_architect::judgment::nodes::read;
use graphhelm_architect::judgment::policy::{
    ACT_THRESHOLD, NOUL_NO_THRESHOLD, NOUL_YES_THRESHOLD, acts, noul_is_no, noul_is_yes,
};
use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, Extras, MAX_REPAIR_ROUNDS, NODE_KIND_MISMATCH_CODE,
    RecordedDraftModel, RecordedJudgeModel, SynthesizedGraph, TaskProfile, synthesize,
    synthesize_with,
};
use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply};
use graphhelm_protocols::ExecutionGraph;

const RECORD_VARIABLE: &str = "ARCHITECT_RECORD";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn goal() -> String {
    std::fs::read_to_string(fixtures().join("first-compile").join("GOAL.txt"))
        .unwrap()
        .trim_end()
        .to_owned()
}

fn profile() -> TaskProfile {
    TaskProfile::new(&goal())
}

fn catalog_with_cargo() -> CapabilityCatalog {
    CapabilityCatalog::from_runtime(&["cargo".to_owned()])
}

fn recording() -> bool {
    std::env::var_os(RECORD_VARIABLE).is_some()
}

/// The golden document as the graph `nodes::read` is given: `build_check` (`tool`) and
/// `summarize` (`agent`).
fn golden_graph() -> ExecutionGraph {
    read_json(&fixtures().join("first-compile/expected.json"))
}

fn choice(choice: &str, confidence: f64) -> Answer {
    Answer::Choice {
        choice: choice.to_owned(),
        probabilities: BTreeMap::new(),
        confidence,
    }
}

fn noul(noul: f64) -> Answer {
    Answer::Noul { noul }
}

/// A hand-built reply for the golden graph: `build_check` on goal and `tool`, `summarize` as
/// the cell says.
fn golden_reply(summarize_on_goal: Answer, summarize_kind: Answer) -> JudgeReply {
    JudgeReply {
        model: JEV_LATEST.to_owned(),
        answers: BTreeMap::from([
            ("on_goal:build_check".to_owned(), noul(0.95)),
            ("kind:build_check".to_owned(), choice("tool", 0.9)),
            ("on_goal:summarize".to_owned(), summarize_on_goal),
            ("kind:summarize".to_owned(), summarize_kind),
        ]),
        usage: Usage::default(),
    }
}

fn expected_document_bytes(document: &serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(document).unwrap();
    bytes.push(b'\n');
    bytes
}

/// The draft fixture shape of `fixtures/README.md`.
#[derive(serde::Serialize, serde::Deserialize)]
struct DraftFixture {
    rounds: Vec<String>,
    replies: BTreeMap<String, String>,
}

/// The judge fixture shape: `rounds` authored, `answers` derived under the request digests.
#[derive(serde::Serialize, serde::Deserialize)]
struct JudgeFixture {
    rounds: Vec<JudgeReply>,
    answers: BTreeMap<String, JudgeReply>,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|error| {
        panic!("{}: {error}", path.display());
    }))
    .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    std::fs::write(path, bytes).unwrap();
}

/// Files both fixtures' `rounds` under the digests the compiler actually asks for, the way an
/// operator records by hand: ask, read the digest from `FixtureMissing` / `JudgeMissing`, file
/// that round's text or reply, ask again. One draft round and one judge call per compile round,
/// so the loop is bounded by twice the rounds the compiler can ask.
fn record(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    drafts: &DraftFixture,
    judged: &JudgeFixture,
) -> (DraftFixture, JudgeFixture) {
    let mut replies = BTreeMap::new();
    let mut answers = BTreeMap::new();
    let mut draft_round = 0_usize;
    let mut judge_round = 0_usize;
    for _ in 0..(2 * (usize::from(MAX_REPAIR_ROUNDS) + 1)) {
        let model = RecordedDraftModel::from_json(
            &serde_json::to_vec(&serde_json::json!({ "replies": replies })).unwrap(),
        )
        .unwrap();
        let judge = RecordedJudgeModel::from_json(
            &serde_json::to_vec(&serde_json::json!({ "answers": answers })).unwrap(),
        )
        .unwrap();
        let extras = Extras {
            judge: Some(&judge),
            ..Extras::default()
        };
        match synthesize_with(profile, catalog, &model, &extras) {
            Err(ArchitectRefusal::FixtureMissing { prompt_sha256 }) => {
                let text = drafts.rounds[draft_round.min(drafts.rounds.len() - 1)].clone();
                replies.insert(prompt_sha256, text);
                draft_round += 1;
            }
            Err(ArchitectRefusal::JudgeMissing { request_sha256 }) => {
                let reply = judged.rounds[judge_round.min(judged.rounds.len() - 1)].clone();
                answers.insert(request_sha256, reply);
                judge_round += 1;
            }
            _ => break,
        }
    }
    (
        DraftFixture {
            rounds: drafts.rounds.clone(),
            replies,
        },
        JudgeFixture {
            rounds: judged.rounds.clone(),
            answers,
        },
    )
}

/// Loads a draft fixture and a judge fixture (re-recording both under `ARCHITECT_RECORD`),
/// proves each file's two halves agree, and compiles with the judge named.
fn compile_judged(
    draft_relative: &str,
    judge_relative: &str,
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
) -> Result<SynthesizedGraph, ArchitectRefusal> {
    let draft_path = fixtures().join(draft_relative);
    let judge_path = fixtures().join(judge_relative);
    let mut drafts: DraftFixture = read_json(&draft_path);
    let mut judged: JudgeFixture = read_json(&judge_path);
    assert!(
        !drafts.rounds.is_empty(),
        "{draft_relative}: no authored rounds"
    );
    assert!(
        !judged.rounds.is_empty(),
        "{judge_relative}: no authored rounds"
    );
    if recording() {
        (drafts, judged) = record(profile, catalog, &drafts, &judged);
        // The golden `first-compile/replies.json` is owned by `golden.rs`'s recorder; only the
        // draft fixtures under `judge/` are rewritten here.
        if draft_relative.starts_with("judge/") {
            write_json(&draft_path, &drafts);
        }
        write_json(&judge_path, &judged);
    }
    let authored: BTreeSet<&String> = drafts.rounds.iter().collect();
    let recorded: BTreeSet<&String> = drafts.replies.values().collect();
    assert_eq!(
        recorded, authored,
        "{draft_relative}: `replies` must hold exactly the authored `rounds`; re-record (see fixtures/README.md)"
    );
    for reply in judged.answers.values() {
        assert!(
            judged.rounds.contains(reply),
            "{judge_relative}: every `answers` value must be one of the authored `rounds`; re-record"
        );
    }
    assert_eq!(
        judged.answers.len(),
        judged.rounds.len(),
        "{judge_relative}: one recorded answer per authored round; re-record"
    );
    let model = RecordedDraftModel::from_file(&draft_path).unwrap();
    let judge = RecordedJudgeModel::from_file(&judge_path).unwrap();
    let extras = Extras {
        judge: Some(&judge),
        ..Extras::default()
    };
    synthesize_with(profile, catalog, &model, &extras)
}

#[test]
fn policy_edges_are_exact() {
    assert!(noul_is_no(NOUL_NO_THRESHOLD - 1e-9));
    assert!(!noul_is_no(NOUL_NO_THRESHOLD + 1e-9));
    assert!(acts(ACT_THRESHOLD + 1e-9));
    assert!(!acts(ACT_THRESHOLD - 1e-9));
    // `NOUL_YES_THRESHOLD` bounds the unresolved band from above (`nodes::read`); it had no
    // edge cell on either side (#1125 second-pass finding).
    assert!(noul_is_yes(NOUL_YES_THRESHOLD + 1e-9));
    assert!(!noul_is_yes(NOUL_YES_THRESHOLD - 1e-9));
}

/// The boundaries are INCLUSIVE, and that is a separate claim from direction: the `± 1e-9` cells
/// above stay green if `>=` becomes `>`; these do not.
#[test]
fn policy_boundaries_are_inclusive() {
    assert!(acts(ACT_THRESHOLD));
    assert!(noul_is_no(NOUL_NO_THRESHOLD));
    assert!(noul_is_yes(NOUL_YES_THRESHOLD));
    // The band between the two noul thresholds is unresolved on both open sides.
    assert!(!noul_is_no(NOUL_NO_THRESHOLD + 1e-9) && !noul_is_yes(NOUL_YES_THRESHOLD - 1e-9));
}

/// Spec D4: `synthesize` and `synthesize_with(.., &Extras::default())` are one road, and that
/// road is the golden `first-compile/expected.json`, byte for byte, with no `judgments` key.
#[test]
fn no_judge_is_todays_bytes() {
    let model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    let with = synthesize_with(
        &profile(),
        &catalog_with_cargo(),
        &model,
        &Extras::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&plain).unwrap(),
        serde_json::to_vec(&with).unwrap()
    );
    let expected = std::fs::read(fixtures().join("first-compile/expected.json")).unwrap();
    assert_eq!(
        String::from_utf8(expected_document_bytes(&plain.document)).unwrap(),
        String::from_utf8(expected).unwrap(),
        "the no-judge road drifted from fixtures/first-compile/expected.json"
    );
    assert!(plain.judgments.is_none());
    let json = serde_json::to_value(&plain).unwrap();
    assert!(
        json.get("judgments").is_none(),
        "an absent report is omitted: {json}"
    );
}

/// `drafts` is bounded before any prompt: a model that would answer is never asked.
#[test]
fn drafts_outside_one_to_three_is_an_invalid_profile_before_any_prompt() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    for drafts in [0_u8, 4, u8::MAX] {
        let extras = Extras {
            drafts,
            ..Extras::default()
        };
        match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
            Err(ArchitectRefusal::InvalidProfile { pointer, .. }) => assert_eq!(pointer, "/drafts"),
            other => panic!("drafts={drafts}: {other:?}"),
        }
    }
}

/// Fixture `judge/nodes-off-goal.json`: the judge says node `summarize` is NOT on goal
/// (`noul` 0.10) with the first draft, then everything on goal with the repaired draft. The draft
/// recording `judge/nodes-off-goal-replies.json` carries both prompts (round 1, and round 2
/// whose repair block quotes GHA005).
#[test]
fn an_off_goal_node_is_a_repairable_gha005_and_round_two_wins() {
    let out = compile_judged(
        "judge/nodes-off-goal-replies.json",
        "judge/nodes-off-goal.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(out.rounds, 2);
    assert_eq!(out.prompt_sha256s.len(), 2);
    let judgments = out.judgments.expect("a judge was named");
    assert_eq!(judgments.nodes.len(), 2);
    assert!(judgments.nodes.iter().all(|node| !noul_is_no(node.on_goal)));
    assert!(judgments.unresolved.is_empty());
    assert_eq!(
        judgments.usage.input_tokens,
        Some(2),
        "usage is summed across both judge calls"
    );
    // The repair prompt of round 2 quoted the judged diagnostic, so the round-2 draft is one
    // the model wrote against GHA005: its objective differs from the golden's.
    let plain_model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &plain_model).unwrap();
    assert_ne!(
        out.document["spec"]["nodes"]["summarize"]["objective"],
        plain.document["spec"]["nodes"]["summarize"]["objective"]
    );
}

/// Fixture `judge/nodes-kind-mismatch.json` (#1120 review finding: GHA006 was emitted by no
/// test): the judge types the agent node `summarize` as `tool` at confidence 0.90 with the
/// first draft, everything else on goal, so the ONLY diagnostic of round 1 is
/// `GHA006_NODE_KIND_MISMATCH` and it is fed back for repair; with the repaired draft of
/// `judge/nodes-kind-mismatch-replies.json` the judge answers `agent`, and round 2 wins.
#[test]
fn a_confident_kind_mismatch_is_a_repairable_gha006_and_round_two_wins() {
    let out = compile_judged(
        "judge/nodes-kind-mismatch-replies.json",
        "judge/nodes-kind-mismatch.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(out.rounds, 2, "round 1 was repaired for the mismatch");
    assert_eq!(out.prompt_sha256s.len(), 2);
    let judgments = out.judgments.expect("a judge was named");
    assert!(judgments.unresolved.is_empty());
    let summarize = judgments
        .nodes
        .iter()
        .find(|node| node.node == "summarize")
        .unwrap();
    assert_eq!(
        summarize.kind, "agent",
        "the accepted round's answer matches"
    );
    assert!(acts(summarize.kind_confidence));
    assert_eq!(judgments.usage.input_tokens, Some(2));
    // The round-2 draft answered a prompt that quoted GHA006: it is not the golden draft.
    let plain_model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &plain_model).unwrap();
    assert_ne!(
        out.document["spec"]["nodes"]["summarize"]["objective"],
        plain.document["spec"]["nodes"]["summarize"]["objective"]
    );
    assert_eq!(out.document["spec"]["nodes"]["summarize"]["type"], "agent");
}

/// Pure cell over `nodes::read` (#1120 review finding): a confident `kind` that IS a catalog
/// type and differs from the draft's is GHA006 at the node's `/type`; a confident `kind` the
/// catalog does not offer is unresolved, with no diagnostic, never a mismatch.
#[test]
fn a_kind_outside_the_catalog_is_unresolved_not_a_mismatch() {
    let graph = golden_graph();
    let catalog = catalog_with_cargo();
    assert!(catalog.node_types.iter().any(|kind| kind == "tool"));
    assert!(!catalog.node_types.iter().any(|kind| kind == "oracle"));

    let mismatch = golden_reply(noul(0.95), choice("tool", 0.9));
    let (diagnostics, _, unresolved) = read(&mismatch, &graph, &catalog);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, NODE_KIND_MISMATCH_CODE);
    assert_eq!(diagnostics[0].path, "/spec/nodes/summarize/type");
    assert!(unresolved.is_empty());

    let outside = golden_reply(noul(0.95), choice("oracle", 0.9));
    let (diagnostics, judgments, unresolved) = read(&outside, &graph, &catalog);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(unresolved, vec!["summarize".to_owned()]);
    let summarize = judgments
        .iter()
        .find(|node| node.node == "summarize")
        .unwrap();
    assert_eq!(summarize.kind, "oracle", "the answer is reported verbatim");
}

/// Pure cell over `nodes::read` (#1120 review finding): `on_goal` is a probability, so a
/// `noul` outside `[0.0, 1.0]`, or NaN, is unresolved with no diagnostic; the edges `0.0` and
/// `1.0` are in range and read as "no" and "yes".
#[test]
fn an_on_goal_outside_zero_to_one_is_unresolved() {
    let graph = golden_graph();
    let catalog = catalog_with_cargo();
    for bad in [-0.01, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let reply = golden_reply(noul(bad), choice("agent", 0.9));
        let (diagnostics, _, unresolved) = read(&reply, &graph, &catalog);
        assert!(diagnostics.is_empty(), "noul {bad}: {diagnostics:?}");
        assert_eq!(unresolved, vec!["summarize".to_owned()], "noul {bad}");
    }
    let no = golden_reply(noul(0.0), choice("agent", 0.9));
    let (diagnostics, _, unresolved) = read(&no, &graph, &catalog);
    assert_eq!(
        diagnostics.len(),
        1,
        "0.0 is a resolved no: {diagnostics:?}"
    );
    assert!(unresolved.is_empty());
    let yes = golden_reply(noul(1.0), choice("agent", 0.9));
    let (diagnostics, _, unresolved) = read(&yes, &graph, &catalog);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(unresolved.is_empty());
}

/// Fixture `judge/nodes-below-threshold.json`: the judge answers `kind` = `tool` for the
/// cognitive node `summarize` with confidence 0.60 (< ACT_THRESHOLD). Nothing changes: one
/// round, the golden document, and the node is listed under `unresolved`.
#[test]
fn a_low_confidence_kind_changes_nothing_and_is_reported_unresolved() {
    let out = compile_judged(
        "first-compile/replies.json",
        "judge/nodes-below-threshold.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(out.rounds, 1);
    let model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(
        out.document, plain.document,
        "a judgment below threshold edits nothing"
    );
    let judgments = out.judgments.unwrap();
    assert_eq!(judgments.unresolved, vec!["summarize".to_owned()]);
    let summarize = judgments
        .nodes
        .iter()
        .find(|node| node.node == "summarize")
        .unwrap();
    assert_eq!(summarize.kind, "tool");
    assert!(!acts(summarize.kind_confidence));
}

/// The judge's diagnostics are appended to the round's diagnostics, never replace them: a draft
/// that is BOTH schema-invalid and off-goal is refused for the schema (the judge is not even
/// asked, because `compile_round` failed before the judgment hook). An empty judge recording
/// would refuse `JudgeMissing` on the first question; the refusal seen is `Invalid` under
/// GHG003 instead.
#[test]
fn the_judge_is_asked_only_about_a_draft_that_passed_every_deterministic_check() {
    let judge = RecordedJudgeModel::from_json(br#"{"answers":{}}"#).unwrap();
    let extras = Extras {
        judge: Some(&judge),
        ..Extras::default()
    };
    let model =
        RecordedDraftModel::from_file(&fixtures().join("sabotage/edge-to-missing-node.json"))
            .unwrap();
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
        Err(ArchitectRefusal::Invalid {
            rounds,
            diagnostics,
        }) => {
            assert_eq!(rounds, MAX_REPAIR_ROUNDS + 1);
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "GHG003_EDGE_TARGET_UNKNOWN"),
                "{diagnostics:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_judge_that_cannot_answer_names_the_request_and_ends_the_run() {
    let model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_json(br#"{"answers":{}}"#).unwrap();
    let extras = Extras {
        judge: Some(&judge),
        ..Extras::default()
    };
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
        Err(ArchitectRefusal::JudgeMissing { request_sha256 }) => {
            assert_eq!(request_sha256.len(), 64);
        }
        other => panic!("{other:?}"),
    }
}
