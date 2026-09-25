//! Tier A of spec §5: the recorded reply for the first-compile goal compiles to a byte-stable
//! document; every sabotage reply is refused under its OWN refusal arm; the repair loop repairs;
//! stamping is load-bearing; the template hash rides the reply and the document.
//!
//! The model in every test is `RecordedDraftModel`, keyed by the sha256 of the full assembled
//! prompt. Recording follows `fixtures/README.md`: with `ARCHITECT_RECORD=1` a file's `replies`
//! are re-derived from its authored `rounds` by asking the compiler and reading the hash each
//! `FixtureMissing` refusal names; without it, the committed file must answer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, DraftModel, MAX_REPAIR_ROUNDS, RecordedDraftModel,
    SynthesizedGraph, TaskProfile, assemble_prompt, prompt_sha256, synthesize, template_sha256,
};

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

#[derive(serde::Serialize, serde::Deserialize)]
struct Fixture {
    rounds: Vec<String>,
    replies: BTreeMap<String, String>,
}

/// Files `rounds` under the prompt hashes the compiler actually asks for, the way an operator
/// records by hand: ask, read the hash from `FixtureMissing`, record, ask again. Bounded by the
/// number of rounds the compiler can ask.
fn record(profile: &TaskProfile, catalog: &CapabilityCatalog, rounds: &[String]) -> Fixture {
    let mut replies = BTreeMap::new();
    for round in 0..=usize::from(MAX_REPAIR_ROUNDS) {
        let model = RecordedDraftModel::from_json(
            &serde_json::to_vec(&serde_json::json!({ "replies": replies })).unwrap(),
        )
        .unwrap();
        match synthesize(profile, catalog, &model) {
            Err(ArchitectRefusal::FixtureMissing { prompt_sha256 }) => {
                let text = rounds[round.min(rounds.len() - 1)].clone();
                replies.insert(prompt_sha256, text);
            }
            _ => break,
        }
    }
    Fixture {
        rounds: rounds.to_vec(),
        replies,
    }
}

/// Loads a fixture (re-recording it first under `ARCHITECT_RECORD`), proves its two halves
/// agree, and compiles against it.
fn compile(
    relative: &str,
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
) -> Result<SynthesizedGraph, ArchitectRefusal> {
    let path = fixtures().join(relative);
    let mut fixture: Fixture =
        serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|error| {
            panic!("{}: {error}", path.display());
        }))
        .unwrap();
    assert!(!fixture.rounds.is_empty(), "{relative}: no authored rounds");
    if recording() {
        fixture = record(profile, catalog, &fixture.rounds);
        let mut bytes = serde_json::to_vec_pretty(&fixture).unwrap();
        bytes.push(b'\n');
        std::fs::write(&path, bytes).unwrap();
    }
    let authored: BTreeSet<&String> = fixture.rounds.iter().collect();
    let recorded: BTreeSet<&String> = fixture.replies.values().collect();
    assert_eq!(
        recorded, authored,
        "{relative}: `replies` must hold exactly the authored `rounds`; re-record (see fixtures/README.md)"
    );
    let model = RecordedDraftModel::from_file(&path).unwrap();
    synthesize(profile, catalog, &model)
}

fn expected_document_bytes(document: &serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(document).unwrap();
    bytes.push(b'\n');
    bytes
}

fn first_compile() -> SynthesizedGraph {
    compile(
        "first-compile/replies.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"))
}

fn relint(document: &serde_json::Value) -> graphhelm_graph::LintReport {
    let loaded =
        graphhelm_schema::load_graph_json(&serde_json::to_vec(document).unwrap(), "emitted")
            .unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));
    graphhelm_graph::lint(&loaded.graph, &loaded.source)
}

#[test]
fn the_recorded_reply_for_the_goal_compiles_to_a_byte_stable_document() {
    let synthesized = first_compile();
    let bytes = expected_document_bytes(&synthesized.document);
    let path = fixtures().join("first-compile").join("expected.json");
    if recording() {
        std::fs::write(&path, &bytes).unwrap();
    }
    let expected = std::fs::read(&path).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        String::from_utf8(expected).unwrap(),
        "the emitted document drifted from fixtures/first-compile/expected.json"
    );
    assert_eq!(synthesized.rounds, 1);
    assert_eq!(synthesized.usage, None, "a recording reports no usage");
    let again = first_compile();
    assert_eq!(
        again.document, synthesized.document,
        "two runs, one document"
    );
    assert_eq!(again, synthesized);
}

#[test]
fn the_document_carries_the_compilers_metadata_and_the_models_spec() {
    let synthesized = first_compile();
    let document = &synthesized.document;
    assert_eq!(document["apiVersion"], "p50.dev/graph/v1");
    assert_eq!(document["kind"], "ExecutionGraph");
    let metadata = &document["metadata"];
    let id = metadata["id"].as_str().unwrap();
    assert!(
        id.starts_with("arch_") && id.ends_with("_v1") && id.len() == "arch_".len() + 8 + 3,
        "{id}"
    );
    assert_eq!(
        metadata["executionId"].as_str().unwrap(),
        format!("exec_{}", &id["arch_".len()..][..8])
    );
    assert_eq!(metadata["version"], 1);
    assert_eq!(metadata["name"], goal());
    assert_eq!(metadata["labels"]["origin"], "architect");
    assert_eq!(metadata["labels"]["template"], template_sha256());
    let nodes = document["spec"]["nodes"].as_object().unwrap();
    assert_eq!(
        nodes.keys().collect::<Vec<_>>(),
        vec!["build_check", "summarize"]
    );
    assert_eq!(nodes["build_check"]["tool"]["call"]["program"], "cargo");
    assert_eq!(
        synthesized
            .rationale
            .iter()
            .map(|entry| entry.node.as_str())
            .collect::<Vec<_>>(),
        vec!["build_check", "summarize"]
    );
    for entry in &synthesized.rationale {
        assert!(
            entry
                .reason
                .contains(nodes[&entry.node]["objective"].as_str().unwrap()),
            "{entry:?}"
        );
    }
    let json = serde_json::to_value(&synthesized).unwrap();
    assert!(
        json.get("usage").is_none(),
        "absent usage is omitted: {json}"
    );
    assert_eq!(
        json["stampedCustoms"],
        serde_json::json!(["build_check", "summarize"])
    );
    assert_eq!(json["templateSha256"], template_sha256());
}

#[test]
fn stamping_is_load_bearing_every_parkable_node_leaves_with_customs_and_lint_sees_no_ghg102() {
    let synthesized = first_compile();
    assert_eq!(
        synthesized.stamped_customs,
        vec!["build_check", "summarize"]
    );
    for id in &synthesized.stamped_customs {
        let customs = &synthesized.document["spec"]["nodes"][id]["completion"]["customs"];
        assert_eq!(
            customs["budgets"]["waitWithinSeconds"],
            graphhelm_architect::DEFAULT_WAIT_WITHIN_SECONDS
        );
        assert_eq!(
            customs["budgets"]["clearanceWithinSeconds"],
            graphhelm_architect::DEFAULT_CLEARANCE_WITHIN_SECONDS
        );
        assert_eq!(customs["proofKinds"], serde_json::json!([]));
        assert!(
            synthesized
                .rationale
                .iter()
                .any(|entry| &entry.node == id && entry.reason.contains("customs")),
            "the rationale names the stamp"
        );
    }
    let report = relint(&synthesized.document);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let unbounded: Vec<_> = report
        .warnings
        .iter()
        .filter(|warning| warning.code == "GHG102_UNBOUNDED_CUSTOMS")
        .collect();
    assert!(unbounded.is_empty(), "{unbounded:?}");
}

#[test]
fn the_profiles_budgets_are_the_ones_stamped_not_the_models() {
    // The budgets never reach the prompt, so the SAME recording answers a profile with other
    // budgets, and the values that leave are the profile's: the stamp is the compiler's.
    let mut profile = profile();
    profile.wait_within_seconds = 120;
    profile.clearance_within_seconds = 30;
    let synthesized = compile(
        "first-compile/replies.json",
        &profile,
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(synthesized.prompt_sha256s, first_compile().prompt_sha256s);
    for id in ["build_check", "summarize"] {
        let customs = &synthesized.document["spec"]["nodes"][id]["completion"]["customs"];
        assert_eq!(customs["budgets"]["waitWithinSeconds"], 120);
        assert_eq!(customs["budgets"]["clearanceWithinSeconds"], 30);
    }
    assert_ne!(synthesized.document, first_compile().document);
}

#[test]
fn the_template_hash_rides_the_reply_and_the_document_and_a_foreign_key_is_fixture_missing() {
    let synthesized = first_compile();
    assert_eq!(synthesized.template_sha256, template_sha256());
    assert_eq!(
        synthesized.document["metadata"]["labels"]["template"],
        template_sha256()
    );
    assert_eq!(
        synthesized.prompt_sha256s.len(),
        usize::from(synthesized.rounds)
    );
    let expected_key = prompt_sha256(&assemble_prompt(
        &profile(),
        &catalog_with_cargo(),
        None,
        None,
        None,
    ));
    assert_eq!(synthesized.prompt_sha256s, vec![expected_key.clone()]);
    let foreign = RecordedDraftModel::single(&"0".repeat(64), "{}");
    match synthesize(&profile(), &catalog_with_cargo(), &foreign) {
        Err(ArchitectRefusal::FixtureMissing { prompt_sha256 }) => {
            assert_eq!(prompt_sha256, expected_key);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_invalid_profile_is_refused_before_any_model_is_asked() {
    let empty = RecordedDraftModel::from_json(br#"{"replies": {}}"#).unwrap();
    let mut profile = profile();
    profile.max_nodes = 0;
    match synthesize(&profile, &catalog_with_cargo(), &empty) {
        Err(ArchitectRefusal::InvalidProfile { pointer, .. }) => assert_eq!(pointer, "/maxNodes"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reply_that_is_not_json_on_every_round_is_not_json_at_the_last_round() {
    match compile("sabotage/not-json.json", &profile(), &catalog_with_cargo()) {
        Err(ArchitectRefusal::NotJson { round, message }) => {
            assert_eq!(round, MAX_REPAIR_ROUNDS + 1);
            assert!(!message.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_edge_to_a_missing_node_is_invalid_under_ghg003_after_every_repair() {
    match compile(
        "sabotage/edge-to-missing-node.json",
        &profile(),
        &catalog_with_cargo(),
    ) {
        Err(ArchitectRefusal::Invalid {
            rounds,
            diagnostics,
        }) => {
            assert_eq!(rounds, MAX_REPAIR_ROUNDS + 1);
            let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.as_str()).collect();
            assert!(
                codes.contains(&"GHG003_EDGE_TARGET_UNKNOWN"),
                "{diagnostics:?}"
            );
            assert!(
                diagnostics.iter().all(|d| d.source == "architect-draft"),
                "{diagnostics:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_deploy_node_named_innocently_is_invalid_under_its_own_code_not_laundered() {
    let refusal = compile(
        "sabotage/innocent-deploy.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .err()
    .unwrap_or_else(|| panic!("a deploy node compiled"));
    // A type the executor refuses is not a stamping failure. `deploy` cannot park, so no GHG102
    // could ever appear in `Invalid.diagnostics` (an error list; GHG102 is a warning) — the arm
    // that WOULD carry a stamping failure is `NotCompletable`, and that is the one refused here.
    assert!(
        !matches!(refusal, ArchitectRefusal::NotCompletable { .. }),
        "{refusal:?}"
    );
    match refusal {
        ArchitectRefusal::Invalid {
            rounds,
            diagnostics,
        } => {
            assert_eq!(rounds, MAX_REPAIR_ROUNDS + 1);
            let own = diagnostics
                .iter()
                .find(|d| d.code == "GHA002_NODE_TYPE_NOT_EXECUTABLE")
                .unwrap_or_else(|| panic!("{diagnostics:?}"));
            assert_eq!(own.path, "/spec/nodes/publish_summary/type");
            assert!(own.message.contains("deploy"), "{own:?}");
        }
        other => panic!("{other:?}"),
    }
}

/// F4: a draft the SCHEMA refuses (an `optionality` outside its enum) is `Invalid` after every
/// repair under the schema's own `GHS` code at the node's pointer — never laundered into a
/// neighbouring arm, never `NotJson` (it IS JSON), never a viability code.
#[test]
fn a_schema_broken_draft_is_invalid_under_a_ghs_code_at_the_nodes_pointer() {
    match compile(
        "sabotage/schema-broken.json",
        &profile(),
        &catalog_with_cargo(),
    ) {
        Err(ArchitectRefusal::Invalid {
            rounds,
            diagnostics,
        }) => {
            assert_eq!(rounds, MAX_REPAIR_ROUNDS + 1);
            let own = diagnostics
                .iter()
                .find(|d| d.code.starts_with("GHS"))
                .unwrap_or_else(|| panic!("{diagnostics:?}"));
            assert!(
                own.path.starts_with("/spec/nodes/"),
                "the schema names the node it refused: {own:?}"
            );
            assert_eq!(own.source, "architect-draft");
        }
        other => panic!("{other:?}"),
    }
}

/// F1: the tool call is parsed at the trust boundary by the broker's own checked parser, and a
/// shape it refuses is `GHA003_TOOL_CALL_MISSING` at the call's pointer. One fixture per shape:
/// a family without its action, a declared field of the wrong type, and a write action the
/// template never offered — each the same code at the same pointer, each on every round.
fn assert_tool_call_refused(relative: &str, expected_in_message: &str) {
    match compile(relative, &profile(), &catalog_with_cargo()) {
        Err(ArchitectRefusal::Invalid {
            rounds,
            diagnostics,
        }) => {
            assert_eq!(rounds, MAX_REPAIR_ROUNDS + 1, "{relative}");
            let own = diagnostics
                .iter()
                .find(|d| d.code == "GHA003_TOOL_CALL_MISSING")
                .unwrap_or_else(|| panic!("{relative}: {diagnostics:?}"));
            assert_eq!(own.path, "/spec/nodes/build_check/tool/call", "{relative}");
            assert!(
                own.message.contains(expected_in_message),
                "{relative}: {own:?}"
            );
            assert_eq!(
                diagnostics
                    .iter()
                    .filter(|d| d.code == "GHA003_TOOL_CALL_MISSING")
                    .count(),
                1,
                "{relative}: one call, one diagnostic: {diagnostics:?}"
            );
        }
        other => panic!("{relative}: {other:?}"),
    }
}

#[test]
fn a_repository_call_without_an_action_is_gha003_at_the_call() {
    assert_tool_call_refused(
        "sabotage/repository-without-action.json",
        "the call names no known tool/action",
    );
}

#[test]
fn a_tests_call_whose_arguments_are_not_an_array_is_gha003_at_the_call() {
    assert_tool_call_refused(
        "sabotage/tests-arguments-not-array.json",
        "the call's declared fields do not deserialize",
    );
}

#[test]
fn a_repository_write_is_gha003_at_the_call_because_the_template_offers_only_reads() {
    assert_tool_call_refused(
        "sabotage/repository-apply-patch.json",
        "repository write (apply_patch or commit)",
    );
}

/// F6: a draft whose `budgets.maxNodes` states more than the profile allows — while its node
/// COUNT is within the ceiling — is repairable under `GHA004_BUDGET_EXCEEDS_PROFILE` at
/// `/spec/budgets/maxNodes`, and the round-2 reply that corrects it wins with the golden
/// document. The round-2 prompt hash is recomputed here from that one diagnostic, which is the
/// measurement that round 1 was refused under exactly this code, pointer and message.
#[test]
fn a_budget_above_the_profile_is_repaired_under_gha004_and_round_two_wins() {
    let synthesized = compile(
        "sabotage/budget-exceeds-profile.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(synthesized.rounds, 2);
    assert_eq!(synthesized.document, first_compile().document);

    let path = fixtures().join("sabotage/budget-exceeds-profile.json");
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let diagnostic = graphhelm_protocols::Diagnostic::error(
        graphhelm_architect::BUDGET_EXCEEDS_PROFILE_CODE,
        format!(
            "budgets.maxNodes is 9; the profile allows at most {}",
            profile().max_nodes
        ),
        "/spec/budgets/maxNodes",
        graphhelm_architect::DRAFT_SOURCE,
    );
    let repair = graphhelm_architect::RepairContext {
        draft: &fixture.rounds[0],
        diagnostics: std::slice::from_ref(&diagnostic),
    };
    let round_two = prompt_sha256(&assemble_prompt(
        &profile(),
        &catalog_with_cargo(),
        Some(&repair),
        None,
        None,
    ));
    assert_eq!(
        synthesized.prompt_sha256s[1], round_two,
        "round 2 was asked with exactly the GHA004 diagnostic at /spec/budgets/maxNodes"
    );
}

#[test]
fn more_nodes_than_the_profile_allows_is_too_many_nodes_without_a_repair() {
    let mut fixture_profile = profile();
    fixture_profile.max_nodes = 6;
    match compile(
        "sabotage/too-many-nodes.json",
        &fixture_profile,
        &catalog_with_cargo(),
    ) {
        Err(ArchitectRefusal::TooManyNodes { count, max }) => {
            assert_eq!((count, max), (7, 6));
        }
        other => panic!("{other:?}"),
    }
    let model =
        RecordedDraftModel::from_file(&fixtures().join("sabotage/too-many-nodes.json")).unwrap();
    assert_eq!(
        model.recorded_prompts().len(),
        1,
        "the ceiling was in the prompt; a model that ignores it is not asked again"
    );

    // F3: the count is read off the parsed reply BEFORE the schema walk, so a reply that is
    // over the ceiling AND unloadable is refused for the count, not validated first. Seven
    // bare `{}` nodes fail the schema on every one of them; the refusal is still the count.
    let nodes: serde_json::Map<String, serde_json::Value> = (1..=7)
        .map(|index| {
            (
                format!("n{index}"),
                serde_json::Value::Object(serde_json::Map::new()),
            )
        })
        .collect();
    let reply = serde_json::json!({ "nodes": nodes }).to_string();
    let first = prompt_sha256(&assemble_prompt(
        &fixture_profile,
        &catalog_with_cargo(),
        None,
        None,
        None,
    ));
    let model = RecordedDraftModel::single(&first, &reply);
    match synthesize(&fixture_profile, &catalog_with_cargo(), &model) {
        Err(ArchitectRefusal::TooManyNodes { count, max }) => assert_eq!((count, max), (7, 6)),
        other => panic!("bounded before the expensive work: {other:?}"),
    }
}

#[test]
fn a_shell_program_outside_the_catalog_is_capability_missing_naming_node_and_program() {
    match compile(
        "sabotage/program-outside-catalog.json",
        &profile(),
        &catalog_with_cargo(),
    ) {
        Err(ArchitectRefusal::CapabilityMissing { node, program }) => {
            assert_eq!((node.as_str(), program.as_str()), ("build_check", "python"));
        }
        other => panic!("{other:?}"),
    }
    let model =
        RecordedDraftModel::from_file(&fixtures().join("sabotage/program-outside-catalog.json"))
            .unwrap();
    assert_eq!(
        model.recorded_prompts().len(),
        1,
        "the allowlist is never widened and the model is not asked to widen it"
    );
}

#[test]
fn the_repair_loop_repairs_a_round_two_reply_that_is_valid_wins() {
    let synthesized = compile(
        "sabotage/repairs-on-round-two.json",
        &profile(),
        &catalog_with_cargo(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(synthesized.rounds, 2);
    assert_eq!(synthesized.prompt_sha256s.len(), 2);
    assert_eq!(
        synthesized.prompt_sha256s[0],
        prompt_sha256(&assemble_prompt(
            &profile(),
            &catalog_with_cargo(),
            None,
            None,
            None
        ))
    );
    let golden = first_compile();
    assert_eq!(synthesized.document, golden.document);
    assert_eq!(synthesized.stamped_customs, golden.stamped_customs);
    assert_eq!(synthesized.rationale, golden.rationale);
}

#[test]
fn a_recording_that_answers_nothing_names_the_first_prompt() {
    let empty = RecordedDraftModel::from_json(br#"{"replies": {}}"#).unwrap();
    let refusal = empty.draft("anything").unwrap_err();
    assert!(matches!(refusal, ArchitectRefusal::FixtureMissing { .. }));
    match synthesize(&profile(), &catalog_with_cargo(), &empty) {
        Err(ArchitectRefusal::FixtureMissing {
            prompt_sha256: hash,
        }) => {
            assert_eq!(
                hash,
                prompt_sha256(&assemble_prompt(
                    &profile(),
                    &catalog_with_cargo(),
                    None,
                    None,
                    None
                ))
            );
        }
        other => panic!("{other:?}"),
    }
}

/// #1066: the goal of `docs/acceptance/useful-change-2026-09-13.md`, read from its own file so the
/// CLI journey and this test share one string.
fn useful_change_goal() -> String {
    std::fs::read_to_string(fixtures().join("useful-change").join("GOAL.txt"))
        .unwrap()
        .trim_end()
        .to_owned()
}

/// The recorded reply for the tools-only goal compiles to ONE `tests` node and nothing cognitive:
/// a graph a Runtime with no model credential runs end to end (`serve --staging --allow-program`,
/// #1066). The catalog offers `git` only — the runner is host configuration, not a program the
/// draft may name — and the draft names no shell program at all.
#[test]
fn the_useful_change_goal_compiles_to_one_tests_node_and_nothing_cognitive() {
    let goal = useful_change_goal();
    let synthesized = compile(
        "useful-change/replies.json",
        &TaskProfile::new(&goal),
        &CapabilityCatalog::from_runtime(&["git".to_owned()]),
    )
    .unwrap_or_else(|refusal| panic!("{refusal:?}"));
    assert_eq!(synthesized.rounds, 1);
    let nodes = synthesized.document["spec"]["nodes"]
        .as_object()
        .expect("nodes");
    assert_eq!(nodes.len(), 1, "{nodes:?}");
    let node = &nodes["run_tests"];
    assert_eq!(node["type"], "tool");
    assert_eq!(node["tool"]["call"]["tool"], "tests");
    assert_eq!(
        node["tool"]["call"]["arguments"],
        serde_json::json!(["grep", "-n", "FIXED", "--", "src/lib.rs"])
    );
    assert_eq!(
        synthesized.document["spec"]["completion"]["terminalNodes"],
        serde_json::json!(["run_tests"])
    );
    let report = relint(&synthesized.document);
    assert!(
        report
            .errors
            .iter()
            .chain(report.warnings.iter())
            .all(|diagnostic| diagnostic.code != "GHG102_UNBOUNDED_CUSTOMS"),
        "the stamped document must be completable"
    );
}
