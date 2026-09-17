//! Site 2 (spec D7): `drafts > 1` asks one draft per stance, judges each per node the way one
//! draft is judged, ranks them with ONE judge call, and returns the chosen document labelled
//! with its stance. `drafts == 1` is today's bytes (`judgment_nodes.rs` proves that road). The
//! fixtures under `fixtures/judge/ranking-*` carry an authored half (`rounds`) and a derived
//! half (`replies` / `answers`) re-recorded under `ARCHITECT_RECORD=1`, the way
//! `judgment_nodes.rs` records (see `fixtures/README.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use graphhelm_architect::judgment::ranking::read;
use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, Extras, MAX_REPAIR_ROUNDS, RecordedDraftModel,
    RecordedJudgeModel, Stance, SynthesizedGraph, TaskProfile, synthesize, synthesize_with,
};
use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply};

const RECORD_VARIABLE: &str = "ARCHITECT_RECORD";
const DRAFTS: &str = "judge/ranking-three-replies.json";

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

/// The recorder of `judgment_nodes.rs`, asked with `drafts` stances: each `FixtureMissing`
/// files the next authored draft (one per stance, in `Stance::ALL` order) and each
/// `JudgeMissing` files the next authored judge reply (one per draft's node judgment, then the
/// ranking). Bounded by one draft and one judge call per round, per draft, plus the ranking.
fn record(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    drafts: &DraftFixture,
    judged: &JudgeFixture,
    count: u8,
) -> (DraftFixture, JudgeFixture) {
    let mut replies = BTreeMap::new();
    let mut answers = BTreeMap::new();
    let mut draft_round = 0_usize;
    let mut judge_round = 0_usize;
    let rounds = usize::from(MAX_REPAIR_ROUNDS) + 1;
    for _ in 0..(2 * rounds * usize::from(count) + 1) {
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
            drafts: count,
            library: None,
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

/// Loads the shared three-stance draft fixture and one judge fixture (re-recording both under
/// `ARCHITECT_RECORD`), proves each file's two halves agree, and compiles three ranked drafts.
fn compile_ranked(judge_relative: &str) -> Result<SynthesizedGraph, ArchitectRefusal> {
    let draft_path = fixtures().join(DRAFTS);
    let judge_path = fixtures().join(judge_relative);
    let mut drafts: DraftFixture = read_json(&draft_path);
    let mut judged: JudgeFixture = read_json(&judge_path);
    assert_eq!(
        drafts.rounds.len(),
        Stance::ALL.len(),
        "{DRAFTS}: one authored draft per stance"
    );
    assert_eq!(
        judged.rounds.len(),
        Stance::ALL.len() + 1,
        "{judge_relative}: one per-node reply per draft, then the ranking reply"
    );
    if recording() {
        (drafts, judged) = record(&profile(), &catalog_with_cargo(), &drafts, &judged, 3);
        write_json(&draft_path, &drafts);
        write_json(&judge_path, &judged);
    }
    let authored: BTreeSet<&String> = drafts.rounds.iter().collect();
    let recorded: BTreeSet<&String> = drafts.replies.values().collect();
    assert_eq!(
        recorded, authored,
        "{DRAFTS}: `replies` must hold exactly the authored `rounds`; re-record (see fixtures/README.md)"
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
        drafts: 3,
        library: None,
    };
    synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras)
}

fn score(score: f64, confidence: f64) -> Answer {
    Answer::Score {
        score,
        legend: BTreeMap::new(),
        probabilities: BTreeMap::new(),
        confidence,
    }
}

fn noul(noul: f64) -> Answer {
    Answer::Noul { noul }
}

fn reply(answers: Vec<(&str, Answer)>) -> JudgeReply {
    JudgeReply {
        model: JEV_LATEST.to_owned(),
        usage: Usage::default(),
        answers: answers
            .into_iter()
            .map(|(id, answer)| (id.to_owned(), answer))
            .collect(),
    }
}

/// `drafts` is bounded before any prompt or judge: an empty model and no judge are never asked.
#[test]
fn drafts_outside_one_to_three_are_refused_before_any_prompt() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    for drafts in [0_u8, 4] {
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

/// Two drafts with no judge would have nothing to rank them: refused at `/drafts`, naming the
/// judge, before the (empty) model is asked.
#[test]
fn more_than_one_draft_needs_a_judge() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    let extras = Extras {
        drafts: 2,
        ..Extras::default()
    };
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
        Err(ArchitectRefusal::InvalidProfile { pointer, message }) => {
            assert_eq!(pointer, "/drafts");
            assert!(message.contains("judge"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

/// `judge/ranking-three-replies.json` holds one valid draft per stance (three prompts: the
/// golden draft without `summarize`, the golden draft, the golden draft with `summarize` split
/// in two); `judge/ranking-three.json` holds the per-node judgments of each (all on goal, kinds
/// matching) and the ranking reply: candidate 2 covers best (`score` 2.0 of levels 0..=2,
/// confidence 0.9), waste at most 0.1 everywhere.
#[test]
fn the_best_covered_candidate_is_chosen_and_the_report_says_why() {
    let out = compile_ranked("judge/ranking-three.json").unwrap_or_else(|refusal| {
        panic!("{refusal:?}");
    });
    let ranking = out.ranking.as_ref().expect("three drafts were ranked");
    assert_eq!(ranking.chosen, 2);
    assert!(!ranking.unresolved);
    assert_eq!(ranking.candidates.len(), 3);
    let labels: Vec<&str> = ranking
        .candidates
        .iter()
        .map(|candidate| candidate.stance.as_str())
        .collect();
    assert_eq!(labels, vec!["minimal", "verified", "explicit"]);
    for (index, candidate) in ranking.candidates.iter().enumerate() {
        assert_eq!(usize::from(candidate.index), index);
        assert!(
            (candidate.composite - (candidate.coverage - candidate.waste)).abs() < 1e-12,
            "{candidate:?}"
        );
    }
    assert!(ranking.candidates[2].composite > ranking.candidates[1].composite);
    assert_eq!(
        out.prompt_sha256s.len(),
        3,
        "one prompt per stance, no repair needed"
    );
    assert_eq!(out.rounds, 1);
    assert_eq!(out.document["metadata"]["labels"]["stance"], "explicit");
    assert_eq!(out.document["spec"]["nodes"].as_object().unwrap().len(), 3);
    let judgments = out.judgments.as_ref().expect("a judge was named");
    assert_eq!(judgments.nodes.len(), 3, "the chosen draft's own judgments");
    assert!(
        judgments.unresolved.is_empty(),
        "{:?}",
        judgments.unresolved
    );
    assert_eq!(
        judgments.usage.input_tokens,
        Some(1 + 3),
        "the chosen draft's per-node call plus the one ranking call"
    );
    // The label is the compiler's, after validation; the document is otherwise the draft's.
    assert_eq!(
        out.document["metadata"]["labels"]["origin"],
        graphhelm_architect::ORIGIN_LABEL
    );
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["ranking"]["chosen"], 2);
    assert_eq!(json["ranking"]["candidates"][2]["stance"], "explicit");
}

/// `judge/ranking-unresolved.json`: the same drafts and per-node judgments; the top candidate's
/// confidence is 0.50, under the acting threshold. Draft 1 (today's road, `minimal`) is kept
/// and the report says `unresolved`, with every candidate's scores still visible.
#[test]
fn a_low_confidence_ranking_keeps_the_first_draft() {
    let out = compile_ranked("judge/ranking-unresolved.json").unwrap_or_else(|refusal| {
        panic!("{refusal:?}");
    });
    let ranking = out.ranking.as_ref().unwrap();
    assert_eq!(ranking.chosen, 0);
    assert!(ranking.unresolved);
    assert_eq!(ranking.candidates.len(), 3);
    assert!(ranking.candidates[2].composite > ranking.candidates[0].composite);
    assert_eq!(out.document["metadata"]["labels"]["stance"], "minimal");
    assert_eq!(out.document["spec"]["nodes"].as_object().unwrap().len(), 1);
}

/// One draft is today's bytes: no stance block (the golden prompt key answers), no `ranking`
/// key, no stance label.
#[test]
fn one_draft_adds_no_stance_and_no_ranking_key() {
    let model =
        RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/nodes-below-threshold.json"))
        .unwrap();
    let extras = Extras {
        judge: Some(&judge),
        drafts: 1,
        library: None,
    };
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras).unwrap();
    assert!(out.ranking.is_none());
    let json = serde_json::to_value(&out).unwrap();
    assert!(json.get("ranking").is_none(), "{json}");
    assert!(
        json["document"]["metadata"]["labels"]
            .get("stance")
            .is_none()
    );
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(out.document, plain.document);
}

/// Pure cell over `ranking::read`: two candidates with identical scores and confidence 0.95;
/// the tie goes to the lower index. Then the other rules, one each: a missing answer is `-inf`
/// and never wins; a mistyped answer likewise; a top confidence under the threshold keeps 0
/// and says `unresolved`; the composite is `coverage - waste`, so waste can reorder.
#[test]
fn a_tie_on_composite_goes_to_the_lower_index() {
    let tie = reply(vec![
        ("coverage:0", score(2.0, 0.95)),
        ("waste:0", noul(0.1)),
        ("coverage:1", score(2.0, 0.95)),
        ("waste:1", noul(0.1)),
    ]);
    let report = read(&tie, 2);
    assert_eq!(report.chosen, 0);
    assert!(!report.unresolved);
    assert_eq!(report.candidates.len(), 2);
    assert!(report.candidates.iter().all(|c| c.stance.is_empty()));

    let missing = reply(vec![
        ("coverage:0", score(0.0, 0.95)),
        ("waste:0", noul(0.9)),
    ]);
    let report = read(&missing, 2);
    assert_eq!(report.chosen, 0);
    assert!(!report.unresolved, "candidate 0 answered and acts");
    assert_eq!(report.candidates[1].composite, f64::NEG_INFINITY);
    assert!(report.candidates[1].coverage.is_nan());

    let mistyped = reply(vec![
        ("coverage:0", noul(0.9)),
        ("waste:0", noul(0.1)),
        ("coverage:1", score(1.0, 0.95)),
        ("waste:1", score(0.0, 0.95)),
    ]);
    let report = read(&mistyped, 2);
    assert!(report.unresolved, "no candidate has a finite composite");
    assert_eq!(report.chosen, 0);
    assert!(
        report
            .candidates
            .iter()
            .all(|c| c.composite == f64::NEG_INFINITY)
    );

    let timid = reply(vec![
        ("coverage:0", score(0.0, 0.95)),
        ("waste:0", noul(0.0)),
        ("coverage:1", score(2.0, 0.79)),
        ("waste:1", noul(0.0)),
    ]);
    let report = read(&timid, 2);
    assert_eq!(report.chosen, 0);
    assert!(report.unresolved);

    let wasteful = reply(vec![
        ("coverage:0", score(1.5, 0.9)),
        ("waste:0", noul(0.0)),
        ("coverage:1", score(2.0, 0.9)),
        ("waste:1", noul(0.9)),
    ]);
    let report = read(&wasteful, 2);
    assert_eq!(report.chosen, 0, "2.0 - 0.9 < 1.5 - 0.0");
    assert!(!report.unresolved);
}
