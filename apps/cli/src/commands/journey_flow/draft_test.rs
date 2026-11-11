//! #2811: a refused journey draft never touches the flow file. Defect named: the posted bytes were
//! written over the flow (possibly the approved one) before they were validated, then put back, so a
//! refusal changed the file's bytes for a moment and always its modified time. Accepted drafts are
//! covered by `a_journey_credential_writes_a_draft_only_while_it_and_its_grant_are_live`
//! (tests/journey_flow_surfaces.rs). Cost: one tempdir per test.

use super::write_draft;
use std::time::Duration;

const FIXTURE: &str = include_str!("../../../tests/fixtures/journey_flow/checkout.journey.yaml");

fn refused_drafts(existing: bool) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".graphhelm/journeys")).unwrap();
    let file = project
        .path()
        .join(".graphhelm/journeys/checkout.journey.yaml");
    let old = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
    if existing {
        std::fs::write(&file, FIXTURE).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let refusals = [
        // Parses and names the id, but is not the canonical bytes.
        (format!("{FIXTURE}# a note\n"), "is not canonical YAML"),
        // Not a draft any more.
        (FIXTURE.replacen("status: draft", "status: approved", 1), ""),
    ];
    for (yaml, code) in refusals {
        let outcome = write_draft("checkout", project.path().to_path_buf(), "agent-a", &yaml);
        assert_ne!(outcome.exit_code, 0, "{:?}", outcome.output);
        assert!(
            code.is_empty()
                || outcome
                    .output
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains(code)),
            "{code}: {:?}",
            outcome.output
        );
        if existing {
            assert_eq!(std::fs::read_to_string(&file).unwrap(), FIXTURE);
            assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), old);
        } else {
            assert!(!file.exists(), "a refused draft left a flow file");
        }
    }
}

#[test]
fn a_refused_draft_leaves_the_existing_flow_bytes_and_modified_time() {
    refused_drafts(true);
}

#[test]
fn a_refused_draft_leaves_no_flow_file_when_there_was_none() {
    refused_drafts(false);
}
