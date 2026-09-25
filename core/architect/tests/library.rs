//! Sites 4 and 1 (spec D8): a caller-supplied graph library, the reuse/adapt/create road
//! decision, and the typed fill of a template's closed-set parameters. Every judge fixture under
//! `fixtures/judge/reuse-*` carries an authored half (`rounds`) and a derived half (`answers`)
//! re-recorded under `ARCHITECT_RECORD=1`, the way `judgment_nodes.rs` records (see
//! `fixtures/README.md`); the `adapt` case pairs with its own draft fixture because the seeded
//! prompt has its own key. The `create` and `unsure` cases draft through the golden
//! `first-compile/replies.json`, which this file never rewrites.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use graphhelm_architect::judgment::reuse::{NO_TEMPLATE, read_decision, read_fill};
use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, Extras, GraphLibrary, MAX_REPAIR_ROUNDS,
    RecordedDraftModel, RecordedJudgeModel, Road, SynthesizedGraph, TaskProfile, synthesize,
    synthesize_with,
};
use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply};

const RECORD_VARIABLE: &str = "ARCHITECT_RECORD";
const GOLDEN_DRAFTS: &str = "first-compile/replies.json";

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

fn library() -> GraphLibrary {
    GraphLibrary::load(&fixtures().join("library")).unwrap()
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

/// The recorder of `judgment_nodes.rs` with a library named: each `JudgeMissing` files the
/// next authored judge reply (the decision, then the fill or the per-node judgments) and each
/// `FixtureMissing` files the next authored draft. With no draft fixture of its own, the case
/// drafts through the golden file, which is read and never written here. Bounded by the
/// decision, one fill, and one draft plus one judge call per round.
fn record(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    library: &GraphLibrary,
    drafts: Option<&DraftFixture>,
    judged: &JudgeFixture,
) -> (Option<DraftFixture>, JudgeFixture) {
    let mut replies = BTreeMap::new();
    let mut answers = BTreeMap::new();
    let mut draft_round = 0_usize;
    let mut judge_round = 0_usize;
    for _ in 0..(2 * (usize::from(MAX_REPAIR_ROUNDS) + 1) + 2) {
        let model = match drafts {
            Some(_) => RecordedDraftModel::from_json(
                &serde_json::to_vec(&serde_json::json!({ "replies": replies })).unwrap(),
            )
            .unwrap(),
            None => RecordedDraftModel::from_file(&fixtures().join(GOLDEN_DRAFTS)).unwrap(),
        };
        let judge = RecordedJudgeModel::from_json(
            &serde_json::to_vec(&serde_json::json!({ "answers": answers })).unwrap(),
        )
        .unwrap();
        let extras = Extras {
            judge: Some(&judge),
            library: Some(library),
            ..Extras::default()
        };
        match synthesize_with(profile, catalog, &model, &extras) {
            Err(ArchitectRefusal::FixtureMissing { prompt_sha256 }) => {
                let drafts = drafts.expect("a case without a draft fixture drafts the golden");
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
        drafts.map(|drafts| DraftFixture {
            rounds: drafts.rounds.clone(),
            replies,
        }),
        JudgeFixture {
            rounds: judged.rounds.clone(),
            answers,
        },
    )
}

/// Loads a judge fixture and, when the case has one, its draft fixture (re-recording both
/// under `ARCHITECT_RECORD`), proves each file's two halves agree, and compiles with the judge
/// and the fixture library named.
/// Where a case's drafts come from: the golden file (read, never written here), a draft
/// fixture of its own (re-recorded), or NO reply at all, for a road that must never ask.
#[derive(Clone, Copy)]
enum Drafts {
    Golden,
    Own(&'static str),
    Empty,
}

fn compile_with_library(
    judge_relative: &str,
    source: Drafts,
) -> Result<SynthesizedGraph, ArchitectRefusal> {
    let judge_path = fixtures().join(judge_relative);
    let mut judged: JudgeFixture = read_json(&judge_path);
    let draft_relative = match source {
        Drafts::Own(relative) => Some(relative),
        Drafts::Golden | Drafts::Empty => None,
    };
    let draft_path = fixtures().join(draft_relative.unwrap_or(GOLDEN_DRAFTS));
    let mut drafts: Option<DraftFixture> = draft_relative.map(|_| read_json(&draft_path));
    let library = library();
    if recording() {
        (drafts, judged) = record(
            &profile(),
            &catalog_with_cargo(),
            &library,
            drafts.as_ref(),
            &judged,
        );
        write_json(&judge_path, &judged);
        if let Some(drafts) = &drafts {
            write_json(&draft_path, drafts);
        }
    }
    if let (Some(drafts), Some(draft_relative)) = (&drafts, draft_relative) {
        let authored: BTreeSet<&String> = drafts.rounds.iter().collect();
        let recorded: BTreeSet<&String> = drafts.replies.values().collect();
        assert_eq!(
            recorded, authored,
            "{draft_relative}: `replies` must hold exactly the authored `rounds`; re-record (see fixtures/README.md)"
        );
    }
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
    let model = match source {
        Drafts::Empty => RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap(),
        Drafts::Golden | Drafts::Own(_) => RecordedDraftModel::from_file(&draft_path).unwrap(),
    };
    let judge = RecordedJudgeModel::from_file(&judge_path).unwrap();
    let extras = Extras {
        judge: Some(&judge),
        library: Some(&library),
        ..Extras::default()
    };
    synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras)
}

fn choice(choice: &str, confidence: f64) -> Answer {
    Answer::Choice {
        choice: choice.to_owned(),
        probabilities: BTreeMap::new(),
        confidence,
    }
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

#[test]
fn a_library_loads_every_template_with_its_sidecar_and_refuses_a_bad_one() {
    let library = library();
    let ids: Vec<&str> = library
        .templates()
        .iter()
        .map(|template| template.id.as_str())
        .collect();
    assert_eq!(ids, vec!["build-and-summarize", "run-tests"]);
    let template = library.template("build-and-summarize").unwrap();
    assert_eq!(template.parameters.len(), 2);
    assert_eq!(template.document["metadata"]["labels"]["origin"], "library");
    assert!(library.template("missing").is_none());

    // A document that is not YAML refuses, naming the file and never its contents.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.yaml"), "not: [valid").unwrap();
    std::fs::write(
        dir.path().join("x.template.json"),
        r#"{"id":"x","summary":"","parameters":{}}"#,
    )
    .unwrap();
    match GraphLibrary::load(dir.path()) {
        Err(ArchitectRefusal::LibraryInvalid { path, message }) => {
            assert_eq!(path, "x.yaml");
            assert!(!message.contains("valid"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A sidecar without a document; a sidecar with an unknown key; a non-directory.
    std::fs::remove_file(dir.path().join("x.yaml")).unwrap();
    assert!(matches!(
        GraphLibrary::load(dir.path()),
        Err(ArchitectRefusal::LibraryInvalid { path, .. }) if path == "x.template.json"
    ));
    std::fs::write(dir.path().join("x.json"), "{}").unwrap();
    assert!(
        GraphLibrary::load(dir.path()).is_ok(),
        "a JSON document serves"
    );
    std::fs::write(
        dir.path().join("x.template.json"),
        r#"{"id":"x","summary":"","parameters":{},"extra":1}"#,
    )
    .unwrap();
    assert!(matches!(
        GraphLibrary::load(dir.path()),
        Err(ArchitectRefusal::LibraryInvalid { path, .. }) if path == "x.template.json"
    ));
    assert!(matches!(
        GraphLibrary::load(&dir.path().join("x.json")),
        Err(ArchitectRefusal::LibraryInvalid { .. })
    ));

    let empty = tempfile::tempdir().unwrap();
    assert!(
        GraphLibrary::load(empty.path())
            .unwrap()
            .templates()
            .is_empty()
    );
}

/// `none` is `reuse::NO_TEMPLATE`, the `template` answer that names no template, so a template
/// with that id could never be chosen: `load` refuses it (#1126 review finding), naming the
/// sidecar and saying the id is reserved.
#[test]
fn a_template_named_none_is_refused_at_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.json"), "{}").unwrap();
    std::fs::write(
        dir.path().join("x.template.json"),
        format!(r#"{{"id":"{NO_TEMPLATE}","summary":"","parameters":{{}}}}"#),
    )
    .unwrap();
    match GraphLibrary::load(dir.path()) {
        Err(ArchitectRefusal::LibraryInvalid { path, message }) => {
            assert_eq!(path, "x.template.json");
            assert!(message.contains("reserved"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // The same sidecar under any other id loads.
    std::fs::write(
        dir.path().join("x.template.json"),
        r#"{"id":"not-none","summary":"","parameters":{}}"#,
    )
    .unwrap();
    assert_eq!(GraphLibrary::load(dir.path()).unwrap().templates().len(), 1);
}

#[test]
fn fill_substitutes_only_declared_closed_values_and_refuses_the_rest() {
    let library = library();
    let template = &library.templates()[0];
    let values = BTreeMap::from([
        ("program".to_owned(), "cargo".to_owned()),
        ("audience".to_owned(), "maintainer".to_owned()),
    ]);
    let filled = template.fill(&values).unwrap();
    assert!(!serde_json::to_string(&filled).unwrap().contains("{{"));
    assert_eq!(
        filled["spec"]["nodes"]["build_check"]["tool"]["call"]["program"],
        "cargo"
    );
    assert_eq!(
        filled["spec"]["nodes"]["summarize"]["objective"],
        "Summarize the build outcome for the maintainer."
    );
    let bad = BTreeMap::from([
        ("program".to_owned(), "rm".to_owned()),
        ("audience".to_owned(), "maintainer".to_owned()),
    ]);
    assert!(matches!(
        template.fill(&bad),
        Err(ArchitectRefusal::LibraryInvalid { .. })
    ));
    let missing = BTreeMap::from([("program".to_owned(), "cargo".to_owned())]);
    assert!(matches!(
        template.fill(&missing),
        Err(ArchitectRefusal::LibraryInvalid { .. })
    ));
}

/// `judge/reuse-reuse.json`: road `reuse` (0.92), template `build-and-summarize` (0.90),
/// program `cargo` (0.95), audience `maintainer` (0.88). No draft model is asked: the recorded
/// draft model is EMPTY and the run still succeeds. The decision reply reports usage 3/5 and
/// the fill 7/11, and the report sums them (#1126 review finding: the library road's judge
/// calls were reported nowhere).
#[test]
fn reuse_fills_a_template_and_never_asks_the_draft_model() {
    let out = compile_with_library("judge/reuse-reuse.json", Drafts::Empty)
        .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    let reuse = out.reuse.as_ref().unwrap();
    assert_eq!(reuse.road, "reuse");
    assert_eq!(reuse.template.as_deref(), Some("build-and-summarize"));
    assert_eq!(reuse.parameters["program"], "cargo");
    assert_eq!(reuse.parameters["audience"], "maintainer");
    assert!(!reuse.unresolved);
    assert_eq!(
        reuse.usage,
        Usage {
            input_tokens: Some(3 + 7),
            output_tokens: Some(5 + 11),
        },
        "the decision's and the fill's usage are summed on the report"
    );
    assert!(out.prompt_sha256s.is_empty());
    assert_eq!(out.rounds, 0);
    assert!(out.usage.is_none());
    assert!(out.judgments.is_none());
    assert_eq!(out.document["metadata"]["labels"]["origin"], "architect");
    assert_eq!(
        out.document["spec"]["nodes"]["build_check"]["tool"]["call"]["program"],
        "cargo"
    );
    assert!(
        !out.stamped_customs.is_empty(),
        "a filled template is stamped like a draft"
    );
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["reuse"]["road"], "reuse");
    assert_eq!(json["reuse"]["parameters"]["audience"], "maintainer");
    assert_eq!(json["reuse"]["usage"]["inputTokens"], 10);
}

/// A filled template that names a program outside the allowlist is `CapabilityMissing`, exactly
/// as a draft would be: the library does not widen the allowlist.
#[test]
fn a_filled_template_outside_the_allowlist_is_capability_missing() {
    match compile_with_library("judge/reuse-npm.json", Drafts::Empty) {
        Err(ArchitectRefusal::CapabilityMissing { program, node }) => {
            assert_eq!(program, "npm");
            assert_eq!(node, "build_check");
        }
        other => panic!("{other:?}"),
    }
}

/// `judge/reuse-adapt.json`: road `adapt` (0.90), template `build-and-summarize`. The draft
/// prompt carries a `<seed>` block; `judge/reuse-adapt-replies.json` holds the reply to THAT
/// prompt, under a key the golden fixture does not have.
#[test]
fn adapt_seeds_the_draft_prompt_with_the_template() {
    let out = compile_with_library(
        "judge/reuse-adapt.json",
        Drafts::Own("judge/reuse-adapt-replies.json"),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    let reuse = out.reuse.as_ref().unwrap();
    assert_eq!(reuse.road, "adapt");
    assert_eq!(reuse.template.as_deref(), Some("build-and-summarize"));
    assert!(!reuse.unresolved);
    assert_eq!(out.rounds, 1);
    assert_eq!(out.prompt_sha256s.len(), 1);
    let golden: DraftFixture = read_json(&fixtures().join(GOLDEN_DRAFTS));
    assert!(
        !golden.replies.contains_key(&out.prompt_sha256s[0]),
        "the seeded prompt is not the golden prompt"
    );
    assert!(
        out.judgments.is_some(),
        "the adapted draft is judged per node"
    );
}

/// `judge/reuse-create.json`: road `create` (0.85). Today's road, today's bytes (plus the
/// report).
#[test]
fn create_is_todays_road() {
    let out = compile_with_library("judge/reuse-create.json", Drafts::Golden)
        .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    let model = RecordedDraftModel::from_file(&fixtures().join(GOLDEN_DRAFTS)).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(out.document, plain.document);
    let reuse = out.reuse.as_ref().unwrap();
    assert_eq!(reuse.road, "create");
    assert!(reuse.template.is_none());
    assert!(!reuse.unresolved);
    assert_eq!(out.rounds, 1);
}

/// `judge/reuse-unsure.json`: road confidence 0.55. Below threshold: `create`, `unresolved`.
#[test]
fn an_unsure_road_falls_to_create_and_says_so() {
    let out = compile_with_library("judge/reuse-unsure.json", Drafts::Golden)
        .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    let reuse = out.reuse.as_ref().unwrap();
    assert_eq!(reuse.road, "create");
    assert!(reuse.unresolved);
    assert!(reuse.template.is_none());
    assert!((reuse.confidence - 0.55).abs() < 1e-12);
    assert_eq!(
        reuse.usage.input_tokens,
        Some(1),
        "the decision call's usage is reported even when the road is unresolved"
    );
    let model = RecordedDraftModel::from_file(&fixtures().join(GOLDEN_DRAFTS)).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(out.document, plain.document);
}

#[test]
fn a_library_without_a_judge_is_ignored_and_says_nothing() {
    let model = RecordedDraftModel::from_file(&fixtures().join(GOLDEN_DRAFTS)).unwrap();
    let library = library();
    let extras = Extras {
        library: Some(&library),
        ..Extras::default()
    };
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras).unwrap();
    assert!(out.reuse.is_none());
    let json = serde_json::to_value(&out).unwrap();
    assert!(json.get("reuse").is_none(), "{json}");
    let expected = std::fs::read(fixtures().join("first-compile/expected.json")).unwrap();
    assert_eq!(
        String::from_utf8(expected_document_bytes(&out.document)).unwrap(),
        String::from_utf8(expected).unwrap(),
        "a library without a judge drifted from fixtures/first-compile/expected.json"
    );
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(
        serde_json::to_vec(&plain).unwrap(),
        serde_json::to_vec(&out).unwrap()
    );
}

/// Pure cells over `reuse::read_decision` and `reuse::read_fill`: every way an answer fails to
/// act falls to `create` and says `unresolved`; a parameter is never guessed.
#[test]
fn an_unacted_decision_or_fill_is_unresolved_never_guessed() {
    let library = library();
    let (road, template, _, unresolved) = read_decision(&reply(vec![]), &library);
    assert_eq!((road, unresolved), (Road::Create, true));
    assert!(template.is_none());

    let none = reply(vec![
        ("road", choice("reuse", 0.95)),
        ("template", choice(NO_TEMPLATE, 0.95)),
    ]);
    let (road, template, confidence, unresolved) = read_decision(&none, &library);
    assert_eq!((road, unresolved), (Road::Create, true));
    assert!(template.is_none());
    assert!((confidence - 0.95).abs() < 1e-12);

    let unknown = reply(vec![
        ("road", choice("adapt", 0.95)),
        ("template", choice("not-in-the-library", 0.95)),
    ]);
    assert_eq!(read_decision(&unknown, &library).0, Road::Create);

    let timid_template = reply(vec![
        ("road", choice("adapt", 0.95)),
        ("template", choice("run-tests", 0.79)),
    ]);
    assert!(read_decision(&timid_template, &library).3);

    let adapt = reply(vec![
        ("road", choice("adapt", 0.80)),
        ("template", choice("run-tests", 0.80)),
    ]);
    let (road, template, _, unresolved) = read_decision(&adapt, &library);
    assert_eq!((road, unresolved), (Road::Adapt, false));
    assert_eq!(template.unwrap().id, "run-tests");

    let create = reply(vec![("road", choice("create", 0.85))]);
    assert_eq!(read_decision(&create, &library).0, Road::Create);
    assert!(!read_decision(&create, &library).3);

    let template = library.template("build-and-summarize").unwrap();
    let fill = reply(vec![
        ("program", choice("cargo", 0.95)),
        ("audience", choice("operator", 0.79)),
    ]);
    let (values, unresolved) = read_fill(&fill, template);
    assert_eq!(
        values,
        BTreeMap::from([("program".to_owned(), "cargo".to_owned())])
    );
    assert_eq!(unresolved, vec!["audience".to_owned()]);
    let outside = reply(vec![
        ("program", choice("rm", 0.99)),
        ("audience", choice("operator", 0.99)),
    ]);
    let (values, unresolved) = read_fill(&outside, template);
    assert_eq!(
        values.len(),
        1,
        "a value outside the options is not a value"
    );
    assert_eq!(unresolved, vec!["program".to_owned()]);
}

/// Spec D8: an EMPTY library makes the decision step a no-op even with a judge named. The
/// recorded judge here answers only the per-node request (`nodes-below-threshold.json`): a road
/// decision would refuse `JudgeMissing`, so the run succeeding proves no decision request was
/// built, and the `reuse` key is absent (#1126 second-pass finding: the
/// `!templates().is_empty()` conjunct had no cell).
#[test]
fn an_empty_library_with_a_judge_costs_no_judge_request_and_says_nothing() {
    let model = RecordedDraftModel::from_file(&fixtures().join(GOLDEN_DRAFTS)).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/nodes-below-threshold.json"))
        .unwrap();
    let empty = tempfile::tempdir().unwrap();
    let library = GraphLibrary::load(empty.path()).unwrap();
    assert!(library.templates().is_empty());
    let extras = Extras {
        judge: Some(&judge),
        library: Some(&library),
        ..Extras::default()
    };
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras).unwrap();
    assert!(out.reuse.is_none());
    assert!(serde_json::to_value(&out).unwrap().get("reuse").is_none());
    assert!(out.judgments.is_some(), "the per-node site still ran");
}
