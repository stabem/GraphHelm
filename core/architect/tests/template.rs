//! The prompt is a pure function of its inputs and carries the template's own hash (D6), so the
//! fixture key moves with the template; the recorded model answers only the prompt it recorded
//! and names the hash an operator would have to record.

use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, DraftModel, RecordedDraftModel, RepairContext,
    TaskProfile, assemble_prompt, prompt_sha256, template_sha256,
};
use graphhelm_protocols::Diagnostic;

#[test]
fn the_assembled_prompt_is_a_pure_function_of_its_inputs_and_carries_the_catalog() {
    let profile = TaskProfile::new("check that the repository builds");
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let a = assemble_prompt(&profile, &catalog, None);
    let b = assemble_prompt(&profile, &catalog, None);
    assert_eq!(a, b);
    assert!(a.contains("cargo"), "the allowlist reaches the prompt");
    assert!(a.contains("\"agent\""), "the node types reach the prompt");
    assert!(a.contains("maxNodes"), "the ceiling reaches the prompt");
    assert!(a.contains("check that the repository builds"));
    assert!(a.contains("supervised"), "the mode reaches the prompt");
    assert!(
        a.contains(&template_sha256()),
        "the template hash is IN the prompt, so the fixture key moves with the template"
    );
    assert!(!a.contains("{{"), "every placeholder is substituted: {a}");
    assert_eq!(template_sha256().len(), 64);
    assert_eq!(prompt_sha256(&a).len(), 64);
    assert_ne!(
        assemble_prompt(&TaskProfile::new("a different goal"), &catalog, None),
        a,
        "the goal is part of the prompt"
    );
    assert_ne!(
        assemble_prompt(&profile, &CapabilityCatalog::from_runtime(&[]), None),
        a,
        "the allowlist is part of the prompt"
    );
}

#[test]
fn a_repair_round_appends_the_diagnostics_verbatim() {
    let profile = TaskProfile::new("check that the repository builds");
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let diagnostics = vec![Diagnostic::error(
        "GHG003_EDGE_TARGET_UNKNOWN",
        "edge target does not name a graph node",
        "/spec/edges/0/to",
        "architect-draft",
    )];
    let draft = r#"{"entrypoints":["a"],"nodes":{}}"#;
    let repair = RepairContext {
        draft,
        diagnostics: &diagnostics,
    };
    let first = assemble_prompt(&profile, &catalog, None);
    let second = assemble_prompt(&profile, &catalog, Some(&repair));
    assert_ne!(first, second);
    assert!(second.contains("GHG003_EDGE_TARGET_UNKNOWN"), "{second}");
    assert!(second.contains("/spec/edges/0/to"), "{second}");
    assert!(
        second.contains("edge target does not name a graph node"),
        "{second}"
    );
    assert!(
        second.contains(draft),
        "the previous draft is quoted verbatim"
    );
    assert!(
        !first.contains("previous draft was refused"),
        "a first round carries no repair block"
    );
    assert!(second.contains("previous draft was refused"));
}

#[test]
fn a_recorded_model_answers_only_the_prompt_it_recorded_and_names_the_missing_hash() {
    let prompt = "hello";
    let model = RecordedDraftModel::single(&prompt_sha256(prompt), "{\"spec\":{}}");
    let reply = model.draft(prompt).unwrap();
    assert_eq!(reply.text, "{\"spec\":{}}");
    assert_eq!(reply.usage, None, "a recording reports no token usage");
    match model.draft("other") {
        Err(ArchitectRefusal::FixtureMissing {
            prompt_sha256: hash,
        }) => {
            assert_eq!(hash, prompt_sha256("other"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_recorded_model_file_is_bounded_and_shaped() {
    let key = prompt_sha256("hello");
    let shaped = format!(r#"{{"replies": {{"{key}": "{{\"spec\":{{}}}}"}}}}"#);
    let model = RecordedDraftModel::from_json(shaped.as_bytes()).unwrap();
    assert_eq!(model.draft("hello").unwrap().text, "{\"spec\":{}}");

    let too_big = vec![b' '; 4 * 1024 * 1024 + 1];
    assert!(matches!(
        RecordedDraftModel::from_json(&too_big),
        Err(ArchitectRefusal::ModelUnavailable { .. })
    ));
    for malformed in [
        "not json",
        "[]",
        r#"{"answers": {}}"#,
        r#"{"replies": []}"#,
        r#"{"replies": {"abc": "text"}}"#,
        &format!(r#"{{"replies": {{"{key}": 7}}}}"#),
    ] {
        match RecordedDraftModel::from_json(malformed.as_bytes()) {
            Err(ArchitectRefusal::ModelUnavailable { message }) => {
                assert!(!message.is_empty(), "{malformed}");
            }
            other => panic!("{malformed}: {other:?}"),
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replies.json");
    std::fs::write(&path, shaped.as_bytes()).unwrap();
    let from_file = RecordedDraftModel::from_file(&path).unwrap();
    assert_eq!(from_file.draft("hello").unwrap().text, "{\"spec\":{}}");
    match RecordedDraftModel::from_file(&directory.path().join("absent.json")) {
        Err(ArchitectRefusal::ModelUnavailable { message }) => {
            assert!(
                !message.contains(directory.path().to_str().unwrap()),
                "a refusal does not echo the caller's filesystem: {message}"
            );
        }
        other => panic!("{other:?}"),
    }
}
