//! `synthesize`: the compile loop (spec D1, D2, D5).
//!
//! One round is: assemble the prompt, ask the model for ONE draft, parse it, wrap the model's
//! `spec` in the compiler's metadata, STAMP customs onto every node the runtime would dispatch
//! and park, then validate through the SAME chain an authored graph takes (`load_graph_json`,
//! `lint`, executor viability) plus the catalog checks. A repairable failure feeds the draft and
//! its diagnostics back for at most [`MAX_REPAIR_ROUNDS`] further rounds; a refusal that no
//! repair could cure (a program the operator did not allow, more nodes than the ceiling, a
//! stamping failure) ends the loop at once.
//!
//! Nothing here publishes, starts, reads a clock, or touches the network: the document returned
//! is bytes for `execution start --file`, on the road every authored graph takes.

use graphhelm_gateway::call::Usage;
use graphhelm_protocols::{Diagnostic, GraphNode, NodeType};
use graphhelm_runtime::classify::{NodeWorkKind, work_kind};
use graphhelm_tool_broker::call::{RepositoryAction, ToolCall};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::catalog::CapabilityCatalog;
use crate::model::DraftModel;
use crate::profile::TaskProfile;
use crate::refusal::ArchitectRefusal;
use crate::template::{RepairContext, assemble_prompt, prompt_sha256, template_sha256};

/// How many times a refused draft is fed back for repair. Round 1 is the draft; rounds 2 and 3
/// are repairs; a third invalid draft is the refusal.
pub const MAX_REPAIR_ROUNDS: u8 = 2;
/// The wire `apiVersion` the compiler writes (the only one `graph.schema.json` accepts).
pub const API_VERSION: &str = "p50.dev/graph/v1";
/// The wire `kind` the compiler writes.
pub const KIND: &str = "ExecutionGraph";
/// The `source` every diagnostic of a draft carries: a draft has no path, and a path in a
/// diagnostic tells a remote caller about a disk they cannot see.
pub const DRAFT_SOURCE: &str = "architect-draft";
/// The value of `metadata.labels.origin` on every synthesized document.
pub const ORIGIN_LABEL: &str = "architect";
/// How much of the goal becomes `metadata.name` (D5: the first 80 characters).
pub const MAX_NAME_CHARS: usize = 80;
/// The largest reply parsed, matching the document bound `load_graph_json` enforces.
pub const MAX_REPLY_BYTES: usize = 4 * 1024 * 1024;
/// Synthetic diagnostic: the reply was not a JSON object. Repairable; the refusal at the last
/// round is [`ArchitectRefusal::NotJson`].
pub const NOT_JSON_CODE: &str = "GHA001_NOT_JSON";
/// Synthetic diagnostic: a node's type is legal for the schema but `classify::work_kind`
/// refuses to execute it. Repairable: the model chose a type outside the catalog it was shown.
pub const NODE_TYPE_NOT_EXECUTABLE_CODE: &str = "GHA002_NODE_TYPE_NOT_EXECUTABLE";
/// Synthetic diagnostic: a tool node carries no `tool.call` the driver could assemble — no call,
/// a call the tool broker's checked parser (`ToolCall::from_json`) refuses (an unknown family or
/// action, a stray field, a field of the wrong type), a shell call without a program, or a
/// repository WRITE (`apply_patch`, `commit`), which the template never offers. Repairable.
pub const TOOL_CALL_MISSING_CODE: &str = "GHA003_TOOL_CALL_MISSING";
/// Synthetic diagnostic: the draft's own `budgets.maxNodes` is above the profile's ceiling. The
/// node COUNT above the ceiling is [`ArchitectRefusal::TooManyNodes`] and is not repaired; a
/// budget line that merely states a larger number is a field the model can correct, so it is.
pub const BUDGET_EXCEEDS_PROFILE_CODE: &str = "GHA004_BUDGET_EXCEEDS_PROFILE";

/// Why one node is in the graph, in the compiler's words: the node's own objective, and the
/// stamp when the compiler added one.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRationale {
    pub node: String,
    pub reason: String,
}

/// The one reply every door returns (spec D8).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizedGraph {
    /// The complete graph document, ready for `execution start --file`.
    ///
    /// Its byte-stability across runs (the golden `expected.json`) rests on `serde_json`'s default
    /// `Map`, a `BTreeMap` that serializes keys in sorted order — the workspace enables no
    /// `preserve_order` feature — so the model's key order never reaches the bytes; enabling
    /// that feature anywhere in the workspace reddens the golden.
    pub document: Value,
    /// One entry per node, in node-id order.
    pub rationale: Vec<NodeRationale>,
    /// The nodes the compiler stamped `completion.customs` onto, in node-id order.
    pub stamped_customs: Vec<String>,
    /// The version of the template that assembled every prompt of this run.
    pub template_sha256: String,
    /// The round whose draft was accepted (1 when no repair was needed).
    pub rounds: u8,
    /// The prompt hash of every round asked, in order; `len() == rounds`.
    pub prompt_sha256s: Vec<String>,
    /// The usage the door reported for the accepted round, when it reported any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Compiles `profile.goal` into a graph document through `model`, or refuses.
///
/// # Errors
/// Every arm of [`ArchitectRefusal`]: the profile out of bounds (before any prompt), the door
/// unreachable or the fixture missing (propagated as-is), the last draft not JSON or still
/// invalid after every repair (with that draft's diagnostics), a program outside the catalog,
/// more nodes than the ceiling, or a node that can park after stamping.
pub fn synthesize(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    model: &dyn DraftModel,
) -> Result<SynthesizedGraph, ArchitectRefusal> {
    profile.validate()?;
    let mut prompt_sha256s = Vec::new();
    let mut previous: Option<(String, Vec<Diagnostic>)> = None;
    let mut round: u8 = 1;
    loop {
        let repair = previous
            .as_ref()
            .map(|(draft, diagnostics)| RepairContext { draft, diagnostics });
        let prompt = assemble_prompt(profile, catalog, repair.as_ref());
        prompt_sha256s.push(prompt_sha256(&prompt));
        let reply = model.draft(&prompt)?;
        match compile_round(profile, catalog, &reply.text) {
            Ok(compiled) => {
                return Ok(SynthesizedGraph {
                    document: compiled.document,
                    rationale: compiled.rationale,
                    stamped_customs: compiled.stamped,
                    template_sha256: template_sha256(),
                    rounds: round,
                    prompt_sha256s,
                    usage: reply.usage,
                });
            }
            Err(RoundFailure::Refused(refusal)) => return Err(refusal),
            Err(RoundFailure::NotJson(message)) => {
                if round > MAX_REPAIR_ROUNDS {
                    return Err(ArchitectRefusal::NotJson { round, message });
                }
                let diagnostic = Diagnostic::error(NOT_JSON_CODE, message, "/", DRAFT_SOURCE);
                previous = Some((reply.text, vec![diagnostic]));
            }
            Err(RoundFailure::Invalid(diagnostics)) => {
                if round > MAX_REPAIR_ROUNDS {
                    return Err(ArchitectRefusal::Invalid {
                        rounds: round,
                        diagnostics,
                    });
                }
                previous = Some((reply.text, diagnostics));
            }
        }
        round += 1;
    }
}

struct Compiled {
    document: Value,
    rationale: Vec<NodeRationale>,
    stamped: Vec<String>,
}

enum RoundFailure {
    /// Fed back to the model, unless this was the last round.
    NotJson(String),
    /// Fed back to the model, unless this was the last round.
    Invalid(Vec<Diagnostic>),
    /// Ends the loop now: no draft the model could produce cures it.
    Refused(ArchitectRefusal),
}

fn compile_round(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    text: &str,
) -> Result<Compiled, RoundFailure> {
    let spec = parse_spec(text)?;
    // Bounded BEFORE the expensive work: the count is read off the parsed reply, ahead of the
    // schema walk and the lint, so a draft with a thousand nodes costs one map length and no
    // validation. Not repaired: the ceiling was in the prompt.
    let count = spec
        .get("nodes")
        .and_then(Value::as_object)
        .map_or(0, Map::len);
    if count > profile.max_nodes {
        return Err(RoundFailure::Refused(ArchitectRefusal::TooManyNodes {
            count,
            max: profile.max_nodes,
        }));
    }
    let mut document = Map::new();
    document.insert(
        "apiVersion".to_owned(),
        Value::String(API_VERSION.to_owned()),
    );
    document.insert("kind".to_owned(), Value::String(KIND.to_owned()));
    document.insert("metadata".to_owned(), metadata(profile));
    document.insert("spec".to_owned(), Value::Object(spec));
    let stamped = stamp_customs(profile, &mut document);
    let document = Value::Object(document);

    let bytes = serde_json::to_vec(&document).map_err(|error| {
        RoundFailure::NotJson(format!(
            "the assembled document cannot be serialized: {error}"
        ))
    })?;
    let loaded =
        graphhelm_schema::load_graph_json(&bytes, DRAFT_SOURCE).map_err(RoundFailure::Invalid)?;
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    let mut errors = report.errors;
    errors.extend(viability(&loaded.graph, catalog));
    if let Some(max_nodes) = loaded.graph.spec.budgets.max_nodes
        && max_nodes > u64::try_from(profile.max_nodes).unwrap_or(u64::MAX)
    {
        errors.push(Diagnostic::error(
            BUDGET_EXCEEDS_PROFILE_CODE,
            format!(
                "budgets.maxNodes is {max_nodes}; the profile allows at most {}",
                profile.max_nodes
            ),
            "/spec/budgets/maxNodes",
            DRAFT_SOURCE,
        ));
    }
    if !errors.is_empty() {
        sort_diagnostics(&mut errors);
        return Err(RoundFailure::Invalid(errors));
    }

    // Every node is executable here, so a GHG102 is a node the compiler's stamp set did not
    // cover while the lint's park set did: the two lists drifted, and that is a defect to see,
    // never a model error to repair (#183).
    let unbounded: Vec<String> = loaded
        .graph
        .spec
        .nodes
        .keys()
        .filter(|id| {
            let path = format!("/spec/nodes/{}/completion/customs", escape(id));
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "GHG102_UNBOUNDED_CUSTOMS" && warning.path == path)
        })
        .cloned()
        .collect();
    if !unbounded.is_empty() {
        return Err(RoundFailure::Refused(ArchitectRefusal::NotCompletable {
            nodes: unbounded,
        }));
    }

    for (id, node) in &loaded.graph.spec.nodes {
        if let Some(program) = shell_program(node)
            && !catalog.allows_program(program)
        {
            return Err(RoundFailure::Refused(ArchitectRefusal::CapabilityMissing {
                node: id.clone(),
                program: program.to_owned(),
            }));
        }
    }

    let rationale = loaded
        .graph
        .spec
        .nodes
        .iter()
        .map(|(id, node)| {
            let mut reason = format!("why it exists: {}", node.objective);
            if stamped.contains(id) {
                reason.push_str(&format!(
                    "; customs stamped by the compiler (waitWithinSeconds {}, clearanceWithinSeconds {})",
                    profile.wait_within_seconds, profile.clearance_within_seconds
                ));
            }
            NodeRationale {
                node: id.clone(),
                reason,
            }
        })
        .collect();

    Ok(Compiled {
        document,
        rationale,
        stamped,
    })
}

/// The model's `spec`: the object under a `spec` key when there is one, else the whole reply
/// object. Anything that is not a JSON object is not a draft.
fn parse_spec(text: &str) -> Result<Map<String, Value>, RoundFailure> {
    if text.len() > MAX_REPLY_BYTES {
        return Err(RoundFailure::NotJson(format!(
            "the reply is {} bytes; at most {MAX_REPLY_BYTES} are parsed",
            text.len()
        )));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|error| RoundFailure::NotJson(format!("the reply is not JSON: {error}")))?;
    let Value::Object(mut object) = value else {
        return Err(RoundFailure::NotJson(
            "the reply is JSON but not an object".to_owned(),
        ));
    };
    match object.remove("spec") {
        Some(Value::Object(spec)) => Ok(spec),
        Some(other) => {
            object.insert("spec".to_owned(), other);
            Ok(object)
        }
        None => Ok(object),
    }
}

/// D5: the metadata is the compiler's. Two runs on one goal produce one identity.
fn metadata(profile: &TaskProfile) -> Value {
    let goal_sha8: String = hex::encode(Sha256::digest(profile.goal.as_bytes()))
        .chars()
        .take(8)
        .collect();
    let name: String = profile.goal.chars().take(MAX_NAME_CHARS).collect();
    let mut labels = Map::new();
    labels.insert("origin".to_owned(), Value::String(ORIGIN_LABEL.to_owned()));
    labels.insert("template".to_owned(), Value::String(template_sha256()));
    let mut metadata = Map::new();
    metadata.insert(
        "id".to_owned(),
        Value::String(format!("arch_{goal_sha8}_v1")),
    );
    metadata.insert("name".to_owned(), Value::String(name));
    metadata.insert(
        "executionId".to_owned(),
        Value::String(format!("exec_{goal_sha8}")),
    );
    metadata.insert("version".to_owned(), Value::from(1_u64));
    metadata.insert("labels".to_owned(), Value::Object(labels));
    Value::Object(metadata)
}

/// D2: stamps `completion.customs` onto every node the runtime dispatches as work that can
/// park (cognitive and tool kinds, per `classify::work_kind`) and that carries none. Existing
/// `completion` keys and an existing `customs` block are kept. Returns the stamped ids, sorted.
///
/// Public so the cross-crate witness (`tests/park_witness.rs`) can hold this stamp set against
/// the lint's park set for every `NodeType`: the two lists live in two crates, and a node type
/// that parks without being stamped is the #183 defect returning.
pub fn stamp_customs(profile: &TaskProfile, document: &mut Map<String, Value>) -> Vec<String> {
    let mut stamped = Vec::new();
    let Some(nodes) = document
        .get_mut("spec")
        .and_then(Value::as_object_mut)
        .and_then(|spec| spec.get_mut("nodes"))
        .and_then(Value::as_object_mut)
    else {
        return stamped;
    };
    for (id, node) in nodes.iter_mut() {
        let Some(node) = node.as_object_mut() else {
            continue;
        };
        let parks = node
            .get("type")
            .and_then(Value::as_str)
            .and_then(node_type_by_wire_name)
            .is_some_and(|node_type| {
                matches!(
                    work_kind(node_type),
                    Ok(NodeWorkKind::Cognitive | NodeWorkKind::Tool)
                )
            });
        if !parks {
            continue;
        }
        let completion = node
            .entry("completion")
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(completion) = completion.as_object_mut() else {
            // Not an object: the schema refuses it below, with the pointer, and the model
            // repairs it. Stamping over it would hide what the model wrote.
            continue;
        };
        if completion.contains_key("customs") {
            continue;
        }
        let mut budgets = Map::new();
        budgets.insert(
            "waitWithinSeconds".to_owned(),
            Value::from(profile.wait_within_seconds),
        );
        budgets.insert(
            "clearanceWithinSeconds".to_owned(),
            Value::from(profile.clearance_within_seconds),
        );
        let mut customs = Map::new();
        customs.insert("proofKinds".to_owned(), Value::Array(Vec::new()));
        customs.insert("budgets".to_owned(), Value::Object(budgets));
        completion.insert("customs".to_owned(), Value::Object(customs));
        stamped.push(id.clone());
    }
    stamped.sort();
    stamped
}

fn node_type_by_wire_name(name: &str) -> Option<&'static NodeType> {
    NodeType::EVERY_VARIANT
        .iter()
        .find(|node_type| node_type.as_str() == name)
}

/// Executor viability, as diagnostics the model can repair: every node type must be one
/// `work_kind` executes, and every tool node must carry a call the driver could assemble.
fn viability(
    graph: &graphhelm_protocols::ExecutionGraph,
    catalog: &CapabilityCatalog,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, node) in &graph.spec.nodes {
        match work_kind(&node.node_type) {
            Ok(NodeWorkKind::Tool) => {
                if let Some(message) = tool_call_refusal(node, catalog) {
                    diagnostics.push(Diagnostic::error(
                        TOOL_CALL_MISSING_CODE,
                        message,
                        format!("/spec/nodes/{}/tool/call", escape(id)),
                        DRAFT_SOURCE,
                    ));
                }
            }
            Ok(NodeWorkKind::Cognitive | NodeWorkKind::GateCheck) => {}
            Err(_) => diagnostics.push(Diagnostic::error(
                NODE_TYPE_NOT_EXECUTABLE_CODE,
                format!(
                    "node type {} is not executed by this runtime; use one of {}",
                    node.node_type.as_str(),
                    catalog.node_types.join(", ")
                ),
                format!("/spec/nodes/{}/type", escape(id)),
                DRAFT_SOURCE,
            )),
        }
    }
    diagnostics
}

/// The trust boundary for a tool node's call: the SAME checked parser the broker applies at its
/// own boundary (`ToolCall::from_json`, which refuses a stray field where the derived
/// `Deserialize` would swallow it), so a call the compiler accepts is a call the driver will
/// assemble, and a call with an unknown action, a wrong-typed field or an extra key is a
/// diagnostic the model repairs rather than a refusal the executor emits mid-run. The two
/// repository WRITES are refused here too: the template offers the repository's reads, shell
/// and tests, and a draft that asks to patch or commit asked for more than it was shown.
/// Returns the message of the diagnostic, or `None` when the call is one the driver runs.
fn tool_call_refusal(node: &GraphNode, catalog: &CapabilityCatalog) -> Option<String> {
    let families = catalog.tool_families.join(", ");
    let Some(call) = node
        .properties
        .get("tool")
        .and_then(|tool| tool.get("call"))
    else {
        return Some(format!(
            "a tool node needs tool.call with \"tool\" as one of {families}"
        ));
    };
    let text = match serde_json::to_string(call) {
        Ok(text) => text,
        Err(error) => return Some(format!("tool.call cannot be serialized: {error}")),
    };
    match ToolCall::from_json(&text) {
        Err(error) => Some(format!(
            "tool.call was refused by the tool broker's parser ({error}); it must be one of \
             {families} with exactly the fields that family declares"
        )),
        Ok(ToolCall::Repository(
            RepositoryAction::ApplyPatch { .. } | RepositoryAction::Commit { .. },
        )) => Some(
            "tool.call names a repository write (apply_patch or commit); this compile offers \
             only the repository's read actions (read_file, list_files, diff), shell and tests"
                .to_owned(),
        ),
        Ok(ToolCall::Shell(_)) if shell_program(node).is_none() => {
            Some("a shell call names a non-empty program".to_owned())
        }
        Ok(_) => None,
    }
}

/// The program a tool node's shell call names, when it is a shell call with a non-empty one.
fn shell_program(node: &GraphNode) -> Option<&str> {
    let call = node.properties.get("tool")?.get("call")?;
    if call.get("tool")?.as_str()? != "shell" {
        return None;
    }
    call.get("program")?
        .as_str()
        .filter(|program| !program.is_empty())
}

/// JSON Pointer escaping for one segment, as the lint does.
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// The lint's order (path, code, message), so merged diagnostics stay deterministic.
fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (&left.path, &left.code, &left.message).cmp(&(&right.path, &right.code, &right.message))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_key_is_unwrapped_and_a_bare_object_is_taken_whole() {
        let wrapped = parse_spec(r#"{"spec":{"nodes":{}}}"#).ok().unwrap();
        assert!(wrapped.contains_key("nodes"));
        let bare = parse_spec(r#"{"nodes":{}}"#).ok().unwrap();
        assert!(bare.contains_key("nodes"));
        assert!(matches!(parse_spec("[]"), Err(RoundFailure::NotJson(_))));
        assert!(matches!(parse_spec("prose"), Err(RoundFailure::NotJson(_))));
    }

    #[test]
    fn stamping_covers_exactly_the_kinds_the_runtime_dispatches_and_parks() {
        let profile = TaskProfile::new("g");
        let mut nodes = Map::new();
        for node_type in NodeType::EVERY_VARIANT {
            let mut node = Map::new();
            node.insert(
                "type".to_owned(),
                Value::String(node_type.as_str().to_owned()),
            );
            nodes.insert(node_type.as_str().to_owned(), Value::Object(node));
        }
        let mut spec = Map::new();
        spec.insert("nodes".to_owned(), Value::Object(nodes));
        let mut document = Map::new();
        document.insert("spec".to_owned(), Value::Object(spec));
        let stamped = stamp_customs(&profile, &mut document);
        let expected: Vec<String> = NodeType::EVERY_VARIANT
            .iter()
            .filter(|node_type| {
                matches!(
                    work_kind(node_type),
                    Ok(NodeWorkKind::Cognitive | NodeWorkKind::Tool)
                )
            })
            .map(|node_type| node_type.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(stamped, expected);
        assert!(
            !stamped.contains(&"gate".to_owned()),
            "a gate decides, it does not wait"
        );
        assert!(!stamped.contains(&"deploy".to_owned()));
    }

    #[test]
    fn an_existing_customs_block_is_kept_and_not_counted() {
        let profile = TaskProfile::new("g");
        let document: Value = serde_json::from_str(
            r#"{"spec":{"nodes":{"a":{"type":"agent","completion":{"requires":[],"customs":{"budgets":{"waitWithinSeconds":5,"clearanceWithinSeconds":5}}}}}}}"#,
        )
        .unwrap();
        let Value::Object(mut document) = document else {
            unreachable!()
        };
        assert!(stamp_customs(&profile, &mut document).is_empty());
        assert_eq!(
            document["spec"]["nodes"]["a"]["completion"]["customs"]["budgets"]["waitWithinSeconds"],
            5
        );
        assert!(document["spec"]["nodes"]["a"]["completion"]["requires"].is_array());
    }

    #[test]
    fn metadata_is_a_function_of_the_goal_alone() {
        let a = metadata(&TaskProfile::new("goal"));
        let mut other = TaskProfile::new("goal");
        other.max_nodes = 3;
        assert_eq!(a, metadata(&other));
        assert_ne!(a, metadata(&TaskProfile::new("goal!")));
        let long = "x".repeat(MAX_NAME_CHARS + 20);
        assert_eq!(
            metadata(&TaskProfile::new(&long))["name"]
                .as_str()
                .unwrap()
                .len(),
            MAX_NAME_CHARS
        );
    }
}
