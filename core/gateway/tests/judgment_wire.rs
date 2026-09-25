//! The judgment types serialize to exactly the TypeSafe `/v1/systemone` request and response
//! bodies (docs.typesafe.ai/api, read 2026-09-16). These are the documented examples, verbatim.

use std::collections::BTreeMap;

use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question, request_sha256,
};
use sha2::Digest;

const PINNED_DOCUMENTED_REQUEST_SHA256: &str =
    "4aa024add9093e9cdecfe767baa0bf5eeacc8375417725abd097a5caed1788ee";

fn documented_request() -> JudgeRequest {
    let mut questions = BTreeMap::new();
    questions.insert(
        "department".to_owned(),
        Question::Choice {
            instructions: "Which team should handle this?".to_owned(),
            criteria: BTreeMap::from([
                (
                    "billing".to_owned(),
                    Some("Payments, invoicing, refunds".to_owned()),
                ),
                (
                    "technical".to_owned(),
                    Some("Bugs, outages, integrations".to_owned()),
                ),
                (
                    "sales".to_owned(),
                    Some("Pricing, upgrades, new accounts".to_owned()),
                ),
            ]),
        },
    );
    questions.insert(
        "frustration".to_owned(),
        Question::Score {
            instructions: "How frustrated is the customer?".to_owned(),
            criteria: vec![
                "Calm".to_owned(),
                "Frustrated".to_owned(),
                "Very angry".to_owned(),
            ],
        },
    );
    questions.insert(
        "is_urgent".to_owned(),
        Question::Noul {
            instructions: "Does this convey urgency?".to_owned(),
            criteria: Some(NoulCriteria {
                r#true: "Explicitly time-sensitive".to_owned(),
                r#false: "No urgency expressed".to_owned(),
            }),
        },
    );
    JudgeRequest {
        state: serde_json::json!("Help! My payouts have been failing for 3 days."),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

#[test]
fn the_request_serializes_to_the_documented_body() {
    let value = serde_json::to_value(documented_request()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "state": "Help! My payouts have been failing for 3 days.",
            "model": "jev-latest",
            "questions": {
                "department": {
                    "type": "choice",
                    "instructions": "Which team should handle this?",
                    "criteria": {
                        "billing": "Payments, invoicing, refunds",
                        "technical": "Bugs, outages, integrations",
                        "sales": "Pricing, upgrades, new accounts"
                    }
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated is the customer?",
                    "criteria": ["Calm", "Frustrated", "Very angry"]
                },
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this convey urgency?",
                    "criteria": { "true": "Explicitly time-sensitive", "false": "No urgency expressed" }
                }
            }
        })
    );
}

#[test]
fn a_noul_without_criteria_omits_the_field() {
    let question = Question::Noul {
        instructions: "Does this convey urgency?".to_owned(),
        criteria: None,
    };
    assert_eq!(
        serde_json::to_value(question).unwrap(),
        serde_json::json!({ "type": "noul", "instructions": "Does this convey urgency?" })
    );
}

#[test]
fn the_documented_response_parses_to_typed_answers() {
    let body = r#"{
      "model": "jev-latest",
      "answers": {
        "department": {
          "type": "choice",
          "choice": "technical",
          "probabilities": { "billing": 0.08, "technical": 0.85, "sales": 0.07 },
          "confidence": 0.82
        },
        "frustration": {
          "type": "score",
          "score": 1.6,
          "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
          "probabilities": { "0": 0.05, "1": 0.3, "2": 0.65 },
          "confidence": 0.78
        },
        "is_urgent": { "type": "noul", "noul": 0.92 }
      },
      "usage": { "input_tokens": 312, "output_tokens": 48 }
    }"#;
    let reply: JudgeReply = serde_json::from_str(body).unwrap();
    assert_eq!(reply.model, "jev-latest");
    assert_eq!(
        reply.usage,
        Usage {
            input_tokens: Some(312),
            output_tokens: Some(48)
        }
    );
    match &reply.answers["department"] {
        Answer::Choice {
            choice,
            probabilities,
            confidence,
        } => {
            assert_eq!(choice, "technical");
            assert_eq!(probabilities["technical"], 0.85);
            assert_eq!(*confidence, 0.82);
        }
        other => panic!("{other:?}"),
    }
    match &reply.answers["frustration"] {
        Answer::Score {
            score,
            legend,
            confidence,
            ..
        } => {
            assert_eq!(*score, 1.6);
            assert_eq!(legend["2"], "Very angry");
            assert_eq!(*confidence, 0.78);
        }
        other => panic!("{other:?}"),
    }
    match &reply.answers["is_urgent"] {
        Answer::Noul { noul } => assert_eq!(*noul, 0.92),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_answer_of_an_unknown_type_is_refused_not_defaulted() {
    let body = r#"{"model":"jev-latest","answers":{"x":{"type":"essay","text":"no"}},"usage":{"input_tokens":1,"output_tokens":1}}"#;
    assert!(serde_json::from_str::<JudgeReply>(body).is_err());
}

#[test]
fn the_request_digest_is_a_pure_function_of_canonical_bytes() {
    let a = request_sha256(&documented_request());
    let b = request_sha256(&documented_request());
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    let mut other = documented_request();
    other.state = serde_json::json!("a different state");
    assert_ne!(a, request_sha256(&other));
}

/// The digest's canonicality is a property of `serde_json`'s `Map` (a `BTreeMap`, keys sorted),
/// which the module documents and which this cell now guards: the same request built with its
/// questions inserted in two different orders must hash to one digest. A later `IndexMap`, or the
/// `preserve_order` feature enabled anywhere in the workspace, turns this cell red here instead of
/// surfacing as a recorded-fixture miss under spec D5 (#1114 review finding).
#[test]
fn the_request_digest_does_not_depend_on_insertion_order() {
    let question = |text: &str| Question::Noul {
        instructions: text.to_owned(),
        criteria: None,
    };
    let mut forward = BTreeMap::new();
    forward.insert("a_first".to_owned(), question("Is it urgent?"));
    forward.insert("b_second".to_owned(), question("Is it billing?"));
    let mut reverse = BTreeMap::new();
    reverse.insert("b_second".to_owned(), question("Is it billing?"));
    reverse.insert("a_first".to_owned(), question("Is it urgent?"));
    let build = |questions: BTreeMap<String, Question>| JudgeRequest {
        state: serde_json::json!({ "z": 1, "a": 2 }),
        model: JEV_LATEST.to_owned(),
        questions,
    };
    assert_eq!(
        request_sha256(&build(forward)),
        request_sha256(&build(reverse))
    );
    let state_reordered = JudgeRequest {
        state: serde_json::json!({ "a": 2, "z": 1 }),
        ..build(BTreeMap::new())
    };
    let state_forward = JudgeRequest {
        state: serde_json::json!({ "z": 1, "a": 2 }),
        ..build(BTreeMap::new())
    };
    assert_eq!(
        request_sha256(&state_reordered),
        request_sha256(&state_forward)
    );
}

/// The digest is the recorded-fixture lookup key (spec D5), so its VALUE is pinned, not only its
/// properties: a field reorder in `JudgeRequest`, a renamed field, or `preserve_order` enabled
/// anywhere in the workspace changes this literal and reddens here, instead of every recorded
/// judge fixture silently missing (#1114 review finding). Re-derive the literal on purpose only.
#[test]
fn the_documented_request_has_a_pinned_digest() {
    let digest = request_sha256(&documented_request());
    let bytes = serde_json::to_vec(&documented_request()).unwrap();
    let expected = hex::encode(sha2::Sha256::digest(&bytes));
    assert_eq!(
        digest, expected,
        "the digest is the sha256 of the canonical bytes"
    );
    assert_eq!(
        digest, PINNED_DOCUMENTED_REQUEST_SHA256,
        "the canonical bytes of the documented request changed; re-pin deliberately"
    );
}

/// A `Choice` option with no rubric serializes as JSON `null` (the API's `string | null`), and a
/// reply's usage round-trips through `usage_wire` in snake_case: the serialized value is read
/// back into an equal `JudgeReply` (#1116 review finding: the cell said "round-trips" but only
/// serialized).
#[test]
fn a_null_rubric_and_the_usage_serializer_are_pinned() {
    let question = Question::Choice {
        instructions: "Which?".to_owned(),
        criteria: BTreeMap::from([
            ("a".to_owned(), None),
            ("b".to_owned(), Some("bee".to_owned())),
        ]),
    };
    assert_eq!(
        serde_json::to_value(question).unwrap(),
        serde_json::json!({ "type": "choice", "instructions": "Which?", "criteria": { "a": null, "b": "bee" } })
    );
    let reply = JudgeReply {
        model: JEV_LATEST.to_owned(),
        answers: BTreeMap::from([("x".to_owned(), Answer::Noul { noul: 0.5 })]),
        usage: Usage {
            input_tokens: Some(7),
            output_tokens: None,
        },
    };
    let wire = serde_json::to_value(&reply).unwrap();
    assert_eq!(
        wire["usage"],
        serde_json::json!({ "input_tokens": 7, "output_tokens": null })
    );
    let back: JudgeReply = serde_json::from_value(wire).unwrap();
    assert_eq!(
        back, reply,
        "the wire value reads back as the reply it was written from"
    );
}
