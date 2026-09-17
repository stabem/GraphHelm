//! Sites 4 and 1 (spec D8): the road decision over a caller-supplied library, and the typed fill
//! of a chosen template's closed-set parameters. State for the decision = the goal and every
//! template's `{id, summary, parameters}`; questions = `road` (Choice over the three roads) and
//! `template` (Choice over the ids plus `none`). State for the fill = the goal; one Choice per
//! parameter, its instructions the sidecar's `question`, its criteria the sidecar's `options`.
//! Under `ACT_THRESHOLD` nothing is acted on: the road falls to `create` and the report says
//! `unresolved`; a parameter the judge is unsure of is never guessed.

use std::collections::BTreeMap;

use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply, JudgeRequest, Question};

use super::policy::acts;
use crate::library::{GraphLibrary, Template};
use crate::profile::TaskProfile;

/// The three roads of spec D8. `Create` is today's road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Road {
    /// A template fits the goal as is, once its parameters are filled.
    Reuse,
    /// A template is the right starting point; the draft prompt is seeded with it.
    Adapt,
    /// No template is close; draft from nothing.
    Create,
}

impl Road {
    /// Every road, in the order the judge sees them.
    pub const ALL: [Road; 3] = [Road::Reuse, Road::Adapt, Road::Create];

    /// The wire word: the `road` criterion id and the report's `road` field.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reuse => "reuse",
            Self::Adapt => "adapt",
            Self::Create => "create",
        }
    }

    /// The criterion text the judge is shown for this road.
    #[must_use]
    pub const fn criterion(self) -> &'static str {
        match self {
            Self::Reuse => "a template fits the goal as is, once its parameters are filled",
            Self::Adapt => {
                "a template is the right starting point but the goal needs a node it lacks or \
                 one fewer"
            }
            Self::Create => "no template is close; draft from nothing",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|road| road.label() == label)
    }
}

/// The `template` answer that names no template.
pub const NO_TEMPLATE: &str = "none";

/// The decision request: the goal and every template's id, summary and parameter names.
#[must_use]
pub fn decide_request(profile: &TaskProfile, library: &GraphLibrary) -> JudgeRequest {
    let templates: Vec<serde_json::Value> = library
        .templates()
        .iter()
        .map(|template| {
            let parameters: Vec<&str> = template.parameters.keys().map(String::as_str).collect();
            serde_json::json!({
                "id": template.id,
                "summary": template.summary,
                "parameters": parameters,
            })
        })
        .collect();
    let roads: BTreeMap<String, Option<String>> = Road::ALL
        .into_iter()
        .map(|road| (road.label().to_owned(), Some(road.criterion().to_owned())))
        .collect();
    let mut ids: BTreeMap<String, Option<String>> = library
        .templates()
        .iter()
        .map(|template| (template.id.clone(), Some(template.summary.clone())))
        .collect();
    ids.insert(
        NO_TEMPLATE.to_owned(),
        Some("no template from `templates` is the starting point".to_owned()),
    );
    let mut questions = BTreeMap::new();
    questions.insert(
        "road".to_owned(),
        Question::Choice {
            instructions: "Which road should the compiler take for the `goal`, given the \
                           `templates` (each an authored graph with closed-set parameters)? \
                           Judge by whether a template's summary and parameters cover what the \
                           goal asks for."
                .to_owned(),
            criteria: roads,
        },
    );
    questions.insert(
        "template".to_owned(),
        Question::Choice {
            instructions: "Which template from `templates` is the closest starting point for \
                           the `goal`? Answer `none` when the road is `create`."
                .to_owned(),
            criteria: ids,
        },
    );
    JudgeRequest {
        state: serde_json::json!({ "goal": profile.goal, "templates": templates }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the decision: the road, the template it names (for `reuse` and `adapt`), the road
/// answer's confidence, and whether the answer was acted on. A road under the acting threshold,
/// a missing or mistyped answer, an unknown road, or a `reuse`/`adapt` whose template is `none`,
/// unknown, or itself under the threshold all fall to `Create` with `unresolved` set: nothing
/// was done, and that is visible (spec D6).
#[must_use]
pub fn read_decision<'a>(
    reply: &JudgeReply,
    library: &'a GraphLibrary,
) -> (Road, Option<&'a Template>, f64, bool) {
    let (road, confidence) = match reply.answers.get("road") {
        Some(Answer::Choice {
            choice, confidence, ..
        }) => (Road::from_label(choice), *confidence),
        _ => (None, 0.0),
    };
    let Some(road) = road else {
        return (Road::Create, None, confidence, true);
    };
    if !acts(confidence) {
        return (Road::Create, None, confidence, true);
    }
    if road == Road::Create {
        return (Road::Create, None, confidence, false);
    }
    let template = match reply.answers.get("template") {
        Some(Answer::Choice {
            choice,
            confidence: template_confidence,
            ..
        }) if choice != NO_TEMPLATE && acts(*template_confidence) => library.template(choice),
        _ => None,
    };
    match template {
        Some(template) => (road, Some(template), confidence, false),
        None => (Road::Create, None, confidence, true),
    }
}

/// The fill request for one template: one Choice per parameter, the sidecar's question as the
/// instructions and its options (value → description) as the criteria.
#[must_use]
pub fn fill_request(profile: &TaskProfile, template: &Template) -> JudgeRequest {
    let questions = template
        .parameters
        .iter()
        .map(|(name, parameter)| {
            (
                name.clone(),
                Question::Choice {
                    instructions: parameter.question.clone(),
                    criteria: parameter
                        .options
                        .iter()
                        .map(|(value, description)| (value.clone(), Some(description.clone())))
                        .collect(),
                },
            )
        })
        .collect();
    JudgeRequest {
        state: serde_json::json!({ "goal": profile.goal }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the fill: every parameter whose answer is one of its options at or above the acting
/// threshold, and the names of those that are not (missing, mistyped, outside the options, or
/// under the threshold). A value is never guessed.
#[must_use]
pub fn read_fill(
    reply: &JudgeReply,
    template: &Template,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut values = BTreeMap::new();
    let mut unresolved = Vec::new();
    for (name, parameter) in &template.parameters {
        match reply.answers.get(name) {
            Some(Answer::Choice {
                choice, confidence, ..
            }) if acts(*confidence) && parameter.options.contains_key(choice) => {
                values.insert(name.clone(), choice.clone());
            }
            _ => unresolved.push(name.clone()),
        }
    }
    (values, unresolved)
}
