//! The judgment types serialize to exactly the TypeSafe `/v1/systemone` request and response
//! bodies (docs.typesafe.ai/api, read 2026-09-16). These are the documented examples, verbatim.

use std::collections::BTreeMap;

use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question, request_sha256,
};

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
