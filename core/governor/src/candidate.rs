use graphhelm_graph::{
    DurableContentError, GraphVersion, MAX_DRAFT_OPERATIONS, PersistencePreflight, lint,
};
use graphhelm_policy::{evaluate_transition, validate_manual_override_limits};
use graphhelm_protocols::{
    Actor, ActorId, ActorType, Diagnostic, DraftOperation, EdgeType, ExecutionGraph, GraphDraft,
    GraphEdge, GraphNode, ManualOverride, OpaqueId, Optionality, PolicyReport,
    UnknownConditionBehavior, WaiverScope,
};

const MAX_OVERRIDE_REQUIREMENTS: usize = 64;
const MAX_OVERRIDE_RISKS: usize = 64;
const MAX_OVERRIDE_REASON_CHARS: usize = 2048;
const MAX_OVERRIDE_RISK_CHARS: usize = 512;

/// Pure draft analysis result before durable publication.
#[derive(Clone, Debug)]
pub struct DraftAnalysis {
    pub candidate: Option<ExecutionGraph>,
    pub diagnostics: Vec<Diagnostic>,
    pub policy_report: Option<PolicyReport>,
}

/// Applies typed operations to an isolated clone and evaluates it without persistence.
#[must_use]
pub fn analyze_draft(base: &GraphVersion, draft: &GraphDraft) -> DraftAnalysis {
    if let Some(request) = &draft.manual_override
        && let Err(mut diagnostic) = validate_manual_override_limits(request)
    {
        // The policy validator is reusable and reports its own field path. At this caller the
        // diagnostic source is the draft being analyzed, just like operation diagnostics below.
        diagnostic.source = draft.id.clone();
        return DraftAnalysis {
            candidate: None,
            diagnostics: vec![diagnostic],
            policy_report: None,
        };
    }
    let mut candidate = base.graph().clone();
    let mut diagnostics = Vec::new();
    if let Err(message) = apply_operations(&mut candidate, &draft.operations) {
        diagnostics.push(Diagnostic::error(
            "GHD003_OPERATION_INVALID",
            message,
            "/operations",
            draft.id.clone(),
        ));
        return DraftAnalysis {
            candidate: None,
            diagnostics,
            policy_report: None,
        };
    }
    candidate.metadata.version = base.number() + 1;
    candidate.metadata.based_on = Some(base.graph().metadata.id.clone());
    let report = lint(&candidate, &draft.id);
    diagnostics.extend(report.errors);
    let policy_report = evaluate_transition(base, &candidate, draft.manual_override.as_ref());
    DraftAnalysis {
        candidate: Some(candidate),
        diagnostics,
        policy_report: Some(policy_report),
    }
}

pub(super) fn apply_operations(
    graph: &mut ExecutionGraph,
    operations: &[DraftOperation],
) -> Result<(), String> {
    for operation in operations {
        match operation {
            DraftOperation::AddNode { id, node } => {
                if graph.spec.nodes.contains_key(id) {
                    return Err(format!("node {id} already exists"));
                }
                graph.spec.nodes.insert(id.clone(), node.clone());
            }
            DraftOperation::RemoveNode { id } => {
                if graph.spec.nodes.remove(id).is_none() {
                    return Err(format!("node {id} does not exist"));
                }
            }
            DraftOperation::PatchNode { id, patch } => {
                let node = graph
                    .spec
                    .nodes
                    .get(id)
                    .ok_or_else(|| format!("node {id} does not exist"))?;
                let patch = patch
                    .as_object()
                    .ok_or_else(|| "node patch must be an object".to_string())?;
                let mut value = serde_json::to_value(node).map_err(|error| error.to_string())?;
                let object = value
                    .as_object_mut()
                    .ok_or_else(|| "node cannot be patched".to_string())?;
                for (key, value) in patch {
                    object.insert(key.clone(), value.clone());
                }
                let patched = serde_json::from_value(value)
                    .map_err(|error| format!("invalid node patch: {error}"))?;
                graph.spec.nodes.insert(id.clone(), patched);
            }
            DraftOperation::AddEdge { edge } => {
                if graph.spec.edges.iter().any(|item| item.id == edge.id) {
                    return Err(format!("edge {} already exists", edge.id));
                }
                graph.spec.edges.push(edge.clone());
            }
            DraftOperation::RemoveEdge { id } => {
                let before = graph.spec.edges.len();
                graph.spec.edges.retain(|edge| edge.id != *id);
                if graph.spec.edges.len() == before {
                    return Err(format!("edge {id} does not exist"));
                }
            }
        }
    }
    Ok(())
}

/// Bounds and validates the complete borrowed draft audit boundary before cloning.
pub(super) fn preflight_draft(
    draft: &GraphDraft,
    authoritative_actor: &Actor,
    max_mutations: Option<u64>,
    usage: &mut PersistencePreflight,
) -> Result<(), DurableContentError> {
    if draft.operations.len() > MAX_DRAFT_OPERATIONS
        || max_mutations.is_some_and(|maximum| {
            u64::try_from(draft.operations.len()).map_or(true, |count| count > maximum)
        })
    {
        return Err(DurableContentError::LimitExceeded);
    }
    usage.account_container(5, 5)?;
    for key in [
        "id",
        "expectedVersion",
        "expectedHash",
        "operations",
        "manualOverride",
    ] {
        usage.account_string(key)?;
    }
    usage.account_scalar()?;
    usage.account_container(draft.operations.len(), MAX_DRAFT_OPERATIONS)?;
    enforce_max_chars(&draft.id, 128)?;
    enforce_max_chars(&authoritative_actor.id, 256)?;
    usage.account_string(&draft.id)?;
    usage.account_string(draft.expected_hash.as_str())?;
    usage.account_container(2, 2)?;
    usage.account_string("type")?;
    usage.account_string("id")?;
    usage.account_string(actor_type_name(&authoritative_actor.actor_type))?;
    usage.account_string(&authoritative_actor.id)?;

    if let Some(manual_override) = &draft.manual_override {
        preflight_override_limits(manual_override, usage)?;
    } else {
        usage.account_scalar()?;
    }

    for operation in &draft.operations {
        usage.account_container(
            match operation {
                DraftOperation::AddNode { .. } | DraftOperation::PatchNode { .. } => 3,
                DraftOperation::RemoveNode { .. }
                | DraftOperation::AddEdge { .. }
                | DraftOperation::RemoveEdge { .. } => 2,
            },
            3,
        )?;
        usage.account_string("op")?;
        usage.account_string(match operation {
            DraftOperation::AddNode { .. } => "addNode",
            DraftOperation::RemoveNode { .. } => "removeNode",
            DraftOperation::PatchNode { .. } => "patchNode",
            DraftOperation::AddEdge { .. } => "addEdge",
            DraftOperation::RemoveEdge { .. } => "removeEdge",
        })?;
        match operation {
            DraftOperation::AddNode { id, node } => {
                usage.account_string("path")?;
                usage.account_string(id)?;
                usage.account_string("value")?;
                account_node(node, usage)?;
            }
            DraftOperation::RemoveNode { id } | DraftOperation::RemoveEdge { id } => {
                usage.account_string("path")?;
                usage.account_string(id)?
            }
            DraftOperation::PatchNode { id, patch } => {
                usage.account_string("path")?;
                usage.account_string(id)?;
                usage.account_string("value")?;
                usage.account_value(patch)?;
            }
            DraftOperation::AddEdge { edge } => {
                usage.account_string("value")?;
                account_edge(edge, usage)?;
            }
        }
    }

    OpaqueId::parse(&draft.id).map_err(|_| DurableContentError::Unsafe)?;
    ActorId::parse(&authoritative_actor.id).map_err(|_| DurableContentError::Unsafe)?;
    if let Some(manual_override) = &draft.manual_override {
        validate_override_shape(manual_override, authoritative_actor)?;
    }
    Ok(())
}

fn account_node(
    node: &GraphNode,
    usage: &mut PersistencePreflight,
) -> Result<(), DurableContentError> {
    usage.account_container(4 + node.properties.len(), 132)?;
    for key in ["type", "name", "objective", "optionality"] {
        usage.account_string(key)?;
    }
    usage.account_container(node.properties.len(), 128)?;
    usage.account_string(node.node_type.as_str())?;
    usage.account_string(&node.name)?;
    usage.account_string(&node.objective)?;
    usage.account_string(match node.optionality {
        Optionality::Required => "required",
        Optionality::Recommended => "recommended",
        Optionality::Optional => "optional",
    })?;
    for (key, value) in &node.properties {
        usage.account_string(key)?;
        usage.account_value(value)?;
    }
    Ok(())
}

fn account_edge(
    edge: &GraphEdge,
    usage: &mut PersistencePreflight,
) -> Result<(), DurableContentError> {
    let optional_fields = usize::from(edge.payload_schema.is_some())
        + usize::from(edge.condition.is_some())
        + usize::from(edge.on_false.is_some())
        + usize::from(edge.on_unknown.is_some())
        + usize::from(edge.priority.is_some());
    usage.account_container(5 + optional_fields, 10)?;
    for key in ["id", "from", "to", "type", "map"] {
        usage.account_string(key)?;
    }
    for value in [&edge.id, &edge.from, &edge.to] {
        usage.account_string(value)?;
    }
    usage.account_string(match edge.edge_type {
        EdgeType::Control => "control",
        EdgeType::Data => "data",
        EdgeType::Evidence => "evidence",
        EdgeType::Event => "event",
        EdgeType::Failure => "failure",
        EdgeType::Compensation => "compensation",
        EdgeType::HumanApproval => "human_approval",
    })?;
    usage.account_container(edge.bindings.len(), 128)?;
    for (key, value) in &edge.bindings {
        usage.account_string(key)?;
        usage.account_string(value)?;
    }
    if let Some(value) = &edge.payload_schema {
        usage.account_string("payloadSchema")?;
        usage.account_string(value)?;
    }
    if let Some(value) = &edge.condition {
        usage.account_string("condition")?;
        usage.account_value(value)?;
    }
    if let Some(value) = &edge.on_false {
        usage.account_string("onFalse")?;
        usage.account_value(value)?;
    }
    if let Some(value) = &edge.on_unknown {
        usage.account_string("onUnknown")?;
        usage.account_string(match value {
            UnknownConditionBehavior::Pause => "pause",
            UnknownConditionBehavior::Fail => "fail",
            UnknownConditionBehavior::Skip => "skip",
            UnknownConditionBehavior::Route => "route",
        })?;
    }
    if edge.priority.is_some() {
        usage.account_string("priority")?;
        usage.account_scalar()?;
    }
    Ok(())
}

fn preflight_override_limits(
    manual_override: &ManualOverride,
    usage: &mut PersistencePreflight,
) -> Result<(), DurableContentError> {
    validate_manual_override_limits(manual_override)
        .map_err(|_| DurableContentError::LimitExceeded)?;
    usage.account_collection(
        manual_override.waived_requirements.len(),
        MAX_OVERRIDE_REQUIREMENTS,
    )?;
    usage.account_collection(manual_override.acknowledged_risks.len(), MAX_OVERRIDE_RISKS)?;
    enforce_max_chars(&manual_override.actor.id, 256)?;
    enforce_max_chars(&manual_override.reason, MAX_OVERRIDE_REASON_CHARS)?;
    usage.account_container(5, 5)?;
    for key in [
        "actor",
        "reason",
        "waivedRequirements",
        "acknowledgedRisks",
        "scope",
    ] {
        usage.account_string(key)?;
    }
    usage.account_container(2, 2)?;
    usage.account_string("type")?;
    usage.account_string("id")?;
    usage.account_string(actor_type_name(&manual_override.actor.actor_type))?;
    usage.account_string(&manual_override.actor.id)?;
    usage.account_string(&manual_override.reason)?;
    usage.account_string(waiver_scope_name(&manual_override.scope))?;
    usage.account_container(
        manual_override.waived_requirements.len(),
        MAX_OVERRIDE_REQUIREMENTS,
    )?;
    for requirement in &manual_override.waived_requirements {
        enforce_max_chars(requirement, 128)?;
        usage.account_string(requirement)?;
    }
    usage.account_container(manual_override.acknowledged_risks.len(), MAX_OVERRIDE_RISKS)?;
    for risk in &manual_override.acknowledged_risks {
        enforce_max_chars(risk, MAX_OVERRIDE_RISK_CHARS)?;
        usage.account_string(risk)?;
    }
    Ok(())
}

fn validate_override_shape(
    manual_override: &ManualOverride,
    authoritative_actor: &Actor,
) -> Result<(), DurableContentError> {
    ActorId::parse(&manual_override.actor.id).map_err(|_| DurableContentError::Unsafe)?;
    if !authoritative_actor.is_owner()
        || !manual_override.actor.is_owner()
        || manual_override.actor != *authoritative_actor
        || manual_override.reason.trim().is_empty()
        || manual_override.waived_requirements.is_empty()
        || manual_override.acknowledged_risks.is_empty()
        || manual_override
            .waived_requirements
            .iter()
            .any(|requirement| OpaqueId::parse(requirement).is_err())
        || manual_override
            .acknowledged_risks
            .iter()
            .any(String::is_empty)
    {
        return Err(DurableContentError::Unsafe);
    }
    Ok(())
}

fn enforce_max_chars(value: &str, maximum: usize) -> Result<(), DurableContentError> {
    if value.chars().count() > maximum {
        Err(DurableContentError::LimitExceeded)
    } else {
        Ok(())
    }
}

const fn actor_type_name(actor_type: &ActorType) -> &'static str {
    match actor_type {
        ActorType::Owner => "owner",
        ActorType::Human => "human",
        ActorType::Agent => "agent",
        ActorType::System => "system",
    }
}

const fn waiver_scope_name(scope: &WaiverScope) -> &'static str {
    match scope {
        WaiverScope::Node => "node",
        WaiverScope::Branch => "branch",
        WaiverScope::Execution => "execution",
    }
}
