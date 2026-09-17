//! The prompt is a pure function of its inputs and carries the template's own hash (D6), so the
//! fixture key moves with the template; the recorded model answers only the prompt it recorded
//! and names the hash an operator would have to record.

use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, DraftModel, RecordedDraftModel, RepairContext, Stance,
    TaskProfile, assemble_prompt, prompt_sha256, template_sha256,
};
use graphhelm_protocols::Diagnostic;

#[test]
fn the_assembled_prompt_is_a_pure_function_of_its_inputs_and_carries_the_catalog() {
    let profile = TaskProfile::new("check that the repository builds");
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let a = assemble_prompt(&profile, &catalog, None, None, None);
    let b = assemble_prompt(&profile, &catalog, None, None, None);
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
        assemble_prompt(
            &TaskProfile::new("a different goal"),
            &catalog,
            None,
            None,
            None
        ),
        a,
        "the goal is part of the prompt"
    );
    assert_ne!(
        assemble_prompt(
            &profile,
            &CapabilityCatalog::from_runtime(&[]),
            None,
            None,
            None
        ),
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
    let first = assemble_prompt(&profile, &catalog, None, None, None);
    let second = assemble_prompt(&profile, &catalog, Some(&repair), None, None);
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

/// Spec D7: `None` is the first compile's exact prompt bytes (its sha256 is the one key the
/// golden `first-compile/replies.json` files its reply under, read from the file rather than
/// hard-coded), and a stance is one fenced block right after the goal fence, so the key moves.
#[test]
fn a_prompt_without_a_stance_is_byte_identical_to_the_first_compile() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/first-compile");
    let goal = std::fs::read_to_string(fixtures.join("GOAL.txt")).unwrap();
    let profile = TaskProfile::new(goal.trim_end());
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let replies: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures.join("replies.json")).unwrap()).unwrap();
    let keys: Vec<&String> = replies["replies"].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 1, "the golden holds one round");
    let before = assemble_prompt(&profile, &catalog, None, None, None);
    assert_eq!(&prompt_sha256(&before), keys[0]);
    for stance in Stance::ALL {
        let with = assemble_prompt(&profile, &catalog, None, Some(&stance), None);
        assert!(
            with.contains("</goal>\n<stance>\n"),
            "the stance follows the goal fence: {with}"
        );
        assert!(with.contains(stance.text()));
        assert!(with.ends_with(&before[before.find("\nEXECUTION MODE").unwrap()..]));
        assert_ne!(prompt_sha256(&before), prompt_sha256(&with));
    }
    assert!(!before.contains("<stance>"));
}

/// Spec D8: a seed is one fenced `<seed>` block of compact JSON after the goal fence — after
/// the stance when both are given — and `None, None` is still the golden key. The seed is
/// inserted after substitution, so a placeholder it spells is quoted, never expanded.
#[test]
fn a_seed_is_one_fenced_block_after_the_goal_and_the_stance() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/first-compile");
    let goal = std::fs::read_to_string(fixtures.join("GOAL.txt")).unwrap();
    let profile = TaskProfile::new(goal.trim_end());
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let seed = serde_json::json!({"spec": {"nodes": {"a": {"objective": "{{GOAL}} {{MODE}}"}}}});
    let before = assemble_prompt(&profile, &catalog, None, None, None);
    let seeded = assemble_prompt(&profile, &catalog, None, None, Some(&seed));
    let block = format!(
        "</goal>\n<seed>\n{}\n</seed>\n",
        serde_json::to_string(&seed).unwrap()
    );
    assert!(seeded.contains(&block), "{seeded}");
    assert!(seeded.contains("{{GOAL}} {{MODE}}"), "the seed is data");
    assert!(seeded.ends_with(&before[before.find("\nEXECUTION MODE").unwrap()..]));
    assert_ne!(prompt_sha256(&before), prompt_sha256(&seeded));
    assert!(!before.contains("<seed>"));
    let both = assemble_prompt(
        &profile,
        &catalog,
        None,
        Some(&Stance::Minimal),
        Some(&seed),
    );
    let stance_at = both.find("<stance>").unwrap();
    let seed_at = both.find("<seed>").unwrap();
    assert!(stance_at < seed_at, "the stance comes first: {both}");
    assert!(both.contains("</stance>\n<seed>\n"));
}
