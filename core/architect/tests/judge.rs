use std::collections::BTreeMap;

use graphhelm_architect::{ArchitectRefusal, JudgeModel, RecordedJudgeModel};
use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, Question, request_sha256,
};

fn request() -> JudgeRequest {
    JudgeRequest {
        state: serde_json::json!({ "goal": "check that the repository builds" }),
        model: JEV_LATEST.to_owned(),
        questions: BTreeMap::from([(
            "on_goal".to_owned(),
            Question::Noul {
                instructions: "Does the node serve the goal?".to_owned(),
                criteria: None,
            },
        )]),
    }
}

fn reply() -> JudgeReply {
    JudgeReply {
        model: JEV_LATEST.to_owned(),
        answers: BTreeMap::from([("on_goal".to_owned(), Answer::Noul { noul: 0.9 })]),
        usage: Usage::default(),
    }
}

#[test]
fn a_recorded_judge_answers_only_the_request_it_recorded() {
    let key = request_sha256(&request());
    let model = RecordedJudgeModel::single(&key, &reply());
    assert_eq!(model.judge(&request()).unwrap(), reply());
    let mut other = request();
    other.state = serde_json::json!({ "goal": "something else" });
    match model.judge(&other) {
        Err(ArchitectRefusal::JudgeMissing { request_sha256 }) => {
            assert_eq!(
                request_sha256,
                graphhelm_gateway::judgment::request_sha256(&other)
            );
        }
        other => panic!("{other:?}"),
    }
}

/// `RecordedJudgeModel::from_file` (#1116 review finding: the disk door had no cell): a file
/// holding one valid entry answers that request; a directory is refused `JudgeUnavailable`
/// with a message that names the failure class and never the path.
#[test]
fn a_recorded_judge_file_is_read_from_disk_and_a_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let key = request_sha256(&request());
    let path = dir.path().join("judge.json");
    std::fs::write(
        &path,
        serde_json::json!({ "answers": { key.clone(): reply() } }).to_string(),
    )
    .unwrap();
    let model = RecordedJudgeModel::from_file(&path).unwrap();
    assert_eq!(model.recorded_requests(), vec![key.as_str()]);
    assert_eq!(model.judge(&request()).unwrap(), reply());

    match RecordedJudgeModel::from_file(dir.path()) {
        Err(ArchitectRefusal::JudgeUnavailable { message }) => {
            assert!(message.contains("not a regular file"), "{message}");
            assert!(
                !message.contains(&dir.path().to_string_lossy().into_owned()),
                "{message}"
            );
        }
        other => panic!("{other:?}"),
    }
    match RecordedJudgeModel::from_file(&dir.path().join("absent.json")) {
        Err(ArchitectRefusal::JudgeUnavailable { message }) => {
            assert!(message.contains("cannot inspect"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_recorded_judge_file_is_bounded_and_shaped() {
    let key = request_sha256(&request());
    let json = serde_json::json!({ "answers": { key.clone(): reply() } }).to_string();
    let model = RecordedJudgeModel::from_json(json.as_bytes()).unwrap();
    assert_eq!(model.recorded_requests(), vec![key.as_str()]);

    let not_hex = serde_json::json!({ "answers": { "nope": reply() } }).to_string();
    match RecordedJudgeModel::from_json(not_hex.as_bytes()) {
        Err(ArchitectRefusal::JudgeUnavailable { message }) => {
            assert!(message.contains("sha256"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    let wrong_shape = serde_json::json!({ "replies": {} }).to_string();
    assert!(matches!(
        RecordedJudgeModel::from_json(wrong_shape.as_bytes()),
        Err(ArchitectRefusal::JudgeUnavailable { .. })
    ));

    let too_big = vec![b' '; graphhelm_architect::MAX_FIXTURE_BYTES + 1];
    assert!(matches!(
        RecordedJudgeModel::from_json(&too_big),
        Err(ArchitectRefusal::JudgeUnavailable { .. })
    ));
}
