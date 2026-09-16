use std::collections::{BTreeMap, BTreeSet};

use graphhelm_events::{
    EvidenceError, EvidenceInput, EvidenceSealer, RepositoryFuture, SealedEvidence, SecretBytes,
};
use graphhelm_graph::{
    DurableContentError, GraphVersion, PersistenceHashes, canonical_content_bytes,
    derive_content_slot_id, derive_content_slot_profile, encode_persisted_reference,
    is_valid_isolation_tier, parse_persisted_binding, parse_persisted_nominal_identifier,
    persisted_hashes, persisted_node_control_order, preflight_graph_version_record_values,
    raw_content_sha256, validate_context_include_reference,
    validate_durable_content as validate_graph_durable_content, validate_evidence_bijection,
    validate_persisted_control_references, validate_persisted_projection,
};
use graphhelm_protocols::{
    ActorId, ActorType, ContentFieldKind, ContentOwnerKind, ContentSlot, EvidenceId,
    EvidenceReference, ExecutionId, GraphBudgets, GraphEdge, GraphNode, GraphVersionRecord,
    OpaqueId, PersistedActor, PersistedActorType, PersistedBudgets, PersistedControl,
    PersistedEdge, PersistedGraphVersion, PersistedGraphVersionRef, PersistedNode,
    PersistedTimestamp, PersistedTopology, RawSha256, RepositoryScope, SafeKey, SafeValue,
    Sensitivity, WireHash,
};
use serde_json::Value;
use thiserror::Error;

const MAX_CONTENT_ITEMS: usize = 8192;
const MAX_CONTENT_ITEM_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONTENT_BATCH_BYTES: usize = 64 * 1024 * 1024;

/// Stable, redacted failure at the Governor externalization boundary.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum GovernorError {
    #[error("authoring graph cannot be externalized safely")]
    InvalidAuthoring,
    #[error("safe graph projection is invalid")]
    InvalidProjection,
    #[error("externalizable graph content exceeds a deterministic limit")]
    LimitExceeded,
    #[error("graph content could not be sealed")]
    SealingFailed,
}

impl GovernorError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidAuthoring | Self::SealingFailed => "GHE009_EXTERNALIZATION_FAILED",
            Self::InvalidProjection => "GHE005_INTEGRITY_FAILURE",
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
        }
    }
}

/// A complete safe projection prepared for Task 6 atomic publication.
///
/// This boundary returns sealed Evidence and its exact references only. Artifact
/// registration requires real artifact bytes, media metadata, and an owning
/// append operation, none of which graph-content externalization possesses.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPreparation {
    pub version: PersistedGraphVersion,
    pub evidence: Vec<SealedEvidence>,
    pub evidence_refs: Vec<EvidenceReference>,
}

impl ProjectionPreparation {
    #[must_use]
    pub const fn version(&self) -> &PersistedGraphVersion {
        &self.version
    }

    #[must_use]
    pub fn evidence(&self) -> &[SealedEvidence] {
        &self.evidence
    }

    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceReference] {
        &self.evidence_refs
    }
}

/// Governor-only translation from authoring graph to encrypted safe projection.
pub trait GraphExternalizer: Send + Sync {
    /// Prepares the first persisted version. Any authoring predecessor fails closed.
    fn prepare<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>>;

    /// Prepares a successor using an explicit safe persisted predecessor identity.
    fn prepare_with_predecessor<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
        predecessor: PersistedGraphVersionRef,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>>;
}

/// Deterministic externalizer backed by an adapter-neutral Evidence sealer.
pub struct SealingGraphExternalizer<S> {
    sealer: S,
}

impl<S> SealingGraphExternalizer<S> {
    #[must_use]
    pub const fn new(sealer: S) -> Self {
        Self { sealer }
    }
}

impl<S: EvidenceSealer> SealingGraphExternalizer<S> {
    /// Prepares a genesis projection while observing the first stage after the
    /// complete borrowed record preflight. Limit failures never invoke it.
    pub fn prepare_observed<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
        after_preflight: &'a (dyn Fn() + Sync),
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        externalize_with_sealer(&self.sealer, scope, version, None, Some(after_preflight))
    }
}

impl<S: EvidenceSealer> GraphExternalizer for SealingGraphExternalizer<S> {
    fn prepare<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        externalize_with_sealer(&self.sealer, scope, version, None, None)
    }

    fn prepare_with_predecessor<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
        predecessor: PersistedGraphVersionRef,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        externalize_with_sealer(&self.sealer, scope, version, Some(predecessor), None)
    }
}

fn externalize_with_sealer<'a, S: EvidenceSealer>(
    sealer: &'a S,
    scope: RepositoryScope,
    version: &'a GraphVersionRecord,
    predecessor: Option<PersistedGraphVersionRef>,
    after_preflight: Option<&'a (dyn Fn() + Sync)>,
) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
    Box::pin(async move {
        let (safe_version, pending) =
            prepare_projection(&scope, version, predecessor, after_preflight)?;
        let mut evidence = Vec::with_capacity(pending.len());
        for content in pending {
            let evidence_id = content.evidence_id.clone();
            let content_sha256 = content.content_sha256.clone();
            let sensitivity = content.sensitivity;
            let plaintext_byte_length = content.plaintext.len();
            let input = EvidenceInput::new(
                evidence_id.as_str(),
                "application/json",
                sensitivity,
                "standard",
                content.plaintext,
            )
            .map_err(map_evidence_error)?;
            let sealed = sealer
                .seal(scope.clone(), input)
                .await
                .map_err(map_evidence_error)?;
            validate_sealed_evidence(
                &sealed,
                &scope,
                &evidence_id,
                sensitivity,
                plaintext_byte_length,
                &content_sha256,
            )?;
            evidence.push(sealed);
        }
        let evidence_refs = evidence
            .iter()
            .map(|item| item.reference().clone())
            .collect::<Vec<_>>();
        validate_evidence_bijection(safe_version.content_slots(), &evidence_refs)
            .map_err(|_| GovernorError::InvalidProjection)?;
        Ok(ProjectionPreparation {
            version: safe_version,
            evidence,
            evidence_refs,
        })
    })
}

struct PendingContent {
    slot_id: OpaqueId,
    owner_kind: ContentOwnerKind,
    owner_id: OpaqueId,
    field_kind: ContentFieldKind,
    ordinal: u32,
    evidence_id: EvidenceId,
    content_sha256: RawSha256,
    sensitivity: Sensitivity,
    required_for_execution: bool,
    plaintext: SecretBytes,
}

impl PendingContent {
    fn position(&self) -> (ContentOwnerKind, &OpaqueId, ContentFieldKind, u32) {
        (
            self.owner_kind,
            &self.owner_id,
            self.field_kind,
            self.ordinal,
        )
    }
}

#[derive(Default)]
struct ContentCollector {
    items: Vec<PendingContent>,
    total_bytes: usize,
}

impl ContentCollector {
    #[allow(clippy::too_many_arguments)]
    fn register(
        &mut self,
        owner_kind: ContentOwnerKind,
        owner_id: &str,
        field_kind: ContentFieldKind,
        ordinal: u32,
        value: &Value,
    ) -> Result<(), GovernorError> {
        if self.items.len() >= MAX_CONTENT_ITEMS {
            return Err(GovernorError::LimitExceeded);
        }
        let encoded_len = encoded_len_bounded(value, MAX_CONTENT_ITEM_BYTES)?;
        let next_total = self
            .total_bytes
            .checked_add(encoded_len)
            .ok_or(GovernorError::LimitExceeded)?;
        if next_total > MAX_CONTENT_BATCH_BYTES {
            return Err(GovernorError::LimitExceeded);
        }

        let bytes = canonical_content_bytes(value).map_err(|_| GovernorError::InvalidProjection)?;
        if bytes.len() != encoded_len {
            return Err(GovernorError::InvalidProjection);
        }
        let content_sha256 =
            raw_content_sha256(&bytes).map_err(|_| GovernorError::InvalidProjection)?;
        let owner_id = OpaqueId::parse(owner_id).map_err(|_| GovernorError::InvalidAuthoring)?;
        let slot_id = derive_content_slot_id(owner_kind, &owner_id, field_kind, ordinal)
            .map_err(|_| GovernorError::InvalidProjection)?;
        let profile = derive_content_slot_profile(owner_kind, &owner_id, field_kind, ordinal)
            .map_err(|_| GovernorError::InvalidProjection)?;
        let evidence_id = EvidenceId::parse(format!("evidence-{}", slot_id.as_str()))
            .map_err(|_| GovernorError::InvalidProjection)?;

        self.total_bytes = next_total;
        self.items.push(PendingContent {
            slot_id,
            owner_kind,
            owner_id,
            field_kind,
            ordinal,
            evidence_id,
            content_sha256,
            sensitivity: profile.sensitivity(),
            required_for_execution: profile.required_for_execution(),
            plaintext: SecretBytes::new(bytes),
        });
        Ok(())
    }

    fn finish(mut self) -> Result<(Vec<ContentSlot>, Vec<PendingContent>), GovernorError> {
        self.items
            .sort_by(|left, right| left.position().cmp(&right.position()));
        if self
            .items
            .windows(2)
            .any(|pair| pair[0].position() == pair[1].position())
        {
            return Err(GovernorError::InvalidProjection);
        }
        let slots = self
            .items
            .iter()
            .map(|item| {
                Ok(ContentSlot::new(
                    item.slot_id.clone(),
                    item.owner_kind,
                    item.owner_id.clone(),
                    item.field_kind,
                    item.ordinal,
                    item.evidence_id.clone(),
                    item.content_sha256.clone(),
                    item.sensitivity,
                    item.required_for_execution,
                ))
            })
            .collect::<Result<Vec<_>, GovernorError>>()?;
        Ok((slots, self.items))
    }
}

fn push_position_part(output: &mut Vec<u8>, value: &[u8]) -> Result<(), GovernorError> {
    let length = u32::try_from(value.len()).map_err(|_| GovernorError::InvalidProjection)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

fn prepare_projection(
    scope: &RepositoryScope,
    record: &GraphVersionRecord,
    predecessor: Option<PersistedGraphVersionRef>,
    after_preflight: Option<&(dyn Fn() + Sync)>,
) -> Result<(PersistedGraphVersion, Vec<PendingContent>), GovernorError> {
    validate_projection_lineage(record, predecessor.as_ref())?;
    let material = prepare_projection_material(scope, record, after_preflight)?;
    let persisted = PersistedGraphVersion::new(
        material.number,
        predecessor,
        material.topology,
        material.hashes.topology_hash().clone(),
        material.hashes.semantic_hash().clone(),
        material.slots,
        material.created_by,
        material.created_at,
    )
    .map_err(|_| GovernorError::InvalidProjection)?;
    validate_persisted_projection(&persisted).map_err(|_| GovernorError::InvalidProjection)?;
    Ok((persisted, material.pending))
}

pub(super) fn projected_version_for(
    scope: &RepositoryScope,
    record: &GraphVersionRecord,
    predecessor: Option<PersistedGraphVersionRef>,
) -> Result<PersistedGraphVersion, GovernorError> {
    prepare_projection(scope, record, predecessor, None).map(|(version, _)| version)
}

struct ProjectionMaterial {
    number: u64,
    topology: PersistedTopology,
    hashes: PersistenceHashes,
    slots: Vec<ContentSlot>,
    created_by: PersistedActor,
    created_at: PersistedTimestamp,
    pending: Vec<PendingContent>,
}

fn prepare_projection_material(
    scope: &RepositoryScope,
    record: &GraphVersionRecord,
    after_preflight: Option<&(dyn Fn() + Sync)>,
) -> Result<ProjectionMaterial, GovernorError> {
    preflight_authoring_values(record)?;
    if let Some(after_preflight) = after_preflight {
        after_preflight();
    }
    let version =
        GraphVersion::from_record(record.clone()).map_err(|_| GovernorError::InvalidAuthoring)?;
    let record = version.to_record();
    let graph = &record.graph;

    let graph_value = serde_json::to_value(graph).map_err(|_| GovernorError::InvalidAuthoring)?;
    if !graphhelm_schema::validate_graph_value(&graph_value, "governor-candidate").is_empty()
        || scope.execution_id().map(|id| id.as_str()) != Some(graph.metadata.execution_id.as_str())
    {
        return Err(GovernorError::InvalidAuthoring);
    }
    validate_durable_content(scope, &record, &graph_value)?;

    let mut collector = ContentCollector::default();
    collect_graph_content(graph, &mut collector)?;
    let (provisional_slots, mut pending) = collector.finish()?;
    let topology = build_topology(graph, &provisional_slots)?;
    let hashes = persisted_hashes(&topology, &provisional_slots)
        .map_err(|_| GovernorError::InvalidProjection)?;
    bind_publication_evidence_ids(
        scope,
        record.graph.metadata.version,
        hashes.semantic_hash(),
        &mut pending,
    )?;
    let slots = content_slots(&pending);
    let final_hashes =
        persisted_hashes(&topology, &slots).map_err(|_| GovernorError::InvalidProjection)?;
    if final_hashes != hashes {
        return Err(GovernorError::InvalidProjection);
    }
    let actor_type = match record.created_by.actor_type {
        ActorType::Owner => PersistedActorType::Owner,
        ActorType::Human => PersistedActorType::Human,
        ActorType::Agent => PersistedActorType::Agent,
        ActorType::System => PersistedActorType::System,
    };
    let created_by = PersistedActor::new(
        actor_type,
        ActorId::parse(record.created_by.id).map_err(|_| GovernorError::InvalidAuthoring)?,
    );
    let created_at = PersistedTimestamp::from_datetime(record.created_at)
        .map_err(|_| GovernorError::InvalidAuthoring)?;
    Ok(ProjectionMaterial {
        number: record.graph.metadata.version,
        topology,
        hashes,
        slots,
        created_by,
        created_at,
        pending,
    })
}

fn preflight_authoring_values(record: &GraphVersionRecord) -> Result<(), GovernorError> {
    preflight_graph_version_record_values(record).map_err(|error| match error {
        DurableContentError::LimitExceeded => GovernorError::LimitExceeded,
        DurableContentError::Unsafe => GovernorError::InvalidAuthoring,
    })
}

fn validate_projection_lineage(
    record: &GraphVersionRecord,
    predecessor: Option<&PersistedGraphVersionRef>,
) -> Result<(), GovernorError> {
    match (
        record.graph.metadata.version,
        record.predecessor.as_ref(),
        predecessor,
    ) {
        (1, None, None) => Ok(()),
        (number, Some(authoring), Some(safe))
            if authoring.number == safe.number()
                && safe.number().checked_add(1) == Some(number) =>
        {
            Ok(())
        }
        _ => Err(GovernorError::InvalidProjection),
    }
}

fn content_slots(items: &[PendingContent]) -> Vec<ContentSlot> {
    items
        .iter()
        .map(|item| {
            ContentSlot::new(
                item.slot_id.clone(),
                item.owner_kind,
                item.owner_id.clone(),
                item.field_kind,
                item.ordinal,
                item.evidence_id.clone(),
                item.content_sha256.clone(),
                item.sensitivity,
                item.required_for_execution,
            )
        })
        .collect()
}

fn bind_publication_evidence_ids(
    scope: &RepositoryScope,
    version_number: u64,
    semantic_hash: &WireHash,
    items: &mut [PendingContent],
) -> Result<(), GovernorError> {
    for item in items {
        let slot = ContentSlot::new(
            item.slot_id.clone(),
            item.owner_kind,
            item.owner_id.clone(),
            item.field_kind,
            item.ordinal,
            item.evidence_id.clone(),
            item.content_sha256.clone(),
            item.sensitivity,
            item.required_for_execution,
        );
        item.evidence_id = graphhelm_graph::derive_publication_evidence_id(
            scope,
            version_number,
            semantic_hash,
            &slot,
        )
        .map_err(|_| GovernorError::InvalidProjection)?;
    }
    Ok(())
}

fn validate_sealed_evidence(
    sealed: &SealedEvidence,
    scope: &RepositoryScope,
    evidence_id: &EvidenceId,
    sensitivity: Sensitivity,
    plaintext_byte_length: usize,
    content_sha256: &RawSha256,
) -> Result<(), GovernorError> {
    let ciphertext_sha256 =
        raw_content_sha256(sealed.ciphertext()).map_err(|_| GovernorError::InvalidProjection)?;
    let aad_sha256 = evidence_aad_sha256(
        scope,
        evidence_id,
        "application/json",
        sensitivity,
        "standard",
        content_sha256,
    )?;
    if sealed.reference().evidence_id() != evidence_id
        || sealed.scope() != scope
        || sealed.media_type().as_str() != "application/json"
        || sealed.sensitivity() != sensitivity
        || sealed.retention_class() != "standard"
        || sealed.algorithm() != "xchacha20poly1305"
        || sealed.plaintext_byte_length() != plaintext_byte_length
        || sealed.reference().content_sha256() != content_sha256
        || sealed.reference().ciphertext_sha256() != &ciphertext_sha256
        || sealed.wrapped_key().handle() != evidence_id.as_str()
        || sealed.wrapped_key().algorithm() != "xchacha20poly1305"
        || sealed.wrapped_key().aad_sha256() != &aad_sha256
    {
        return Err(GovernorError::InvalidProjection);
    }
    Ok(())
}

fn evidence_aad_sha256(
    scope: &RepositoryScope,
    evidence_id: &EvidenceId,
    media_type: &str,
    sensitivity: Sensitivity,
    retention_class: &str,
    content_sha256: &RawSha256,
) -> Result<RawSha256, GovernorError> {
    let mut aad = Vec::with_capacity(512);
    push_position_part(&mut aad, b"graphhelm-evidence-aad-v1")?;
    push_position_part(&mut aad, scope.workspace_id().as_str().as_bytes())?;
    push_position_part(&mut aad, scope.project_id().as_str().as_bytes())?;
    match scope.execution_id() {
        Some(execution_id) => {
            aad.push(1);
            push_position_part(&mut aad, execution_id.as_str().as_bytes())?;
        }
        None => aad.push(0),
    }
    push_position_part(&mut aad, evidence_id.as_str().as_bytes())?;
    push_position_part(&mut aad, b"1.0.0")?;
    push_position_part(&mut aad, media_type.as_bytes())?;
    push_position_part(
        &mut aad,
        match sensitivity {
            Sensitivity::Public => b"public",
            Sensitivity::Internal => b"internal",
            Sensitivity::Confidential => b"confidential",
            Sensitivity::Restricted => b"restricted",
        },
    )?;
    push_position_part(&mut aad, retention_class.as_bytes())?;
    push_position_part(&mut aad, content_sha256.as_str().as_bytes())?;
    raw_content_sha256(&aad).map_err(|_| GovernorError::InvalidProjection)
}

pub(super) fn safe_semantic_hash_for(
    scope: &RepositoryScope,
    record: &GraphVersionRecord,
) -> Result<WireHash, GovernorError> {
    let material = prepare_projection_material(scope, record, None)?;
    Ok(material.hashes.semantic_hash().clone())
}

fn collect_graph_content(
    graph: &graphhelm_protocols::ExecutionGraph,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let graph_id = graph.metadata.id.as_str();
    collector.register(
        ContentOwnerKind::Graph,
        graph_id,
        ContentFieldKind::DisplayName,
        0,
        &Value::String(graph.metadata.name.clone()),
    )?;
    for (key, value) in &graph.metadata.properties {
        match key.as_str() {
            "description" => collector.register(
                ContentOwnerKind::Graph,
                graph_id,
                ContentFieldKind::Description,
                0,
                value,
            )?,
            "source" | "sourcePath" | "ui" | "annotations" | "mutationId" => {}
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }

    collect_completion_content(
        ContentOwnerKind::Graph,
        graph_id,
        &graph.spec.completion,
        collector,
    )?;
    for (ordinal, policy) in graph.spec.policies.iter().enumerate() {
        collect_policy_content(graph_id, ordinal, policy, collector)?;
    }
    for (node_id, node) in &graph.spec.nodes {
        collector.register(
            ContentOwnerKind::Node,
            node_id,
            ContentFieldKind::DisplayName,
            0,
            &Value::String(node.name.clone()),
        )?;
        collector.register(
            ContentOwnerKind::Node,
            node_id,
            ContentFieldKind::Objective,
            0,
            &Value::String(node.objective.clone()),
        )?;
        collect_node_properties(node_id, node, collector)?;
    }
    for edge in &graph.spec.edges {
        if let Some(condition) = &edge.condition {
            collector.register(
                ContentOwnerKind::Edge,
                &edge.id,
                ContentFieldKind::PolicyText,
                0,
                condition,
            )?;
        }
        if let Some(on_false) = &edge.on_false {
            collector.register(
                ContentOwnerKind::Edge,
                &edge.id,
                ContentFieldKind::PolicyText,
                1,
                on_false,
            )?;
        }
    }
    Ok(())
}

fn collect_node_properties(
    node_id: &str,
    node: &GraphNode,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    for (key, value) in &node.properties {
        match key.as_str() {
            "description" => collector.register(
                ContentOwnerKind::Node,
                node_id,
                ContentFieldKind::Description,
                0,
                value,
            )?,
            "completion" => {
                collect_completion_content(ContentOwnerKind::Node, node_id, value, collector)?
            }
            "agent" => collect_agent_content(node_id, value, collector)?,
            "agents" => collect_crew_references(value).map(drop)?,
            "input" | "output" => validate_schema_container(value)?,
            "prompt" => {
                validate_string(value)?;
                collector.register(
                    ContentOwnerKind::Node,
                    node_id,
                    ContentFieldKind::Instructions,
                    0,
                    value,
                )?;
            }
            "context" => collect_context_paths(node_id, value, collector)?,
            "permissions" => collect_permission_paths(node_id, value, collector)?,
            "isolation" => collect_isolation_paths(node_id, value, collector)?,
            "model" | "retry" | "resources" | "memory" | "tags" | "onCancel" | "onFailure"
            | "tool" | "classifier" | "gate" | "onFail" | "override" | "strategy" | "merge"
            | "options" | "timeout" | "graphRef" | "parameters" | "expose" | "materializer"
            | "adapterRef" | "preconditions" | "effects" => {}
            "loop" => validate_loop_control(value)?,
            "timeoutSeconds" => validate_positive_integer(value)?,
            "userEditable" | "userOverrideAllowed" => validate_bool(value)?,
            "targetRef" => validate_string(value)?,
            "ui" => {}
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    Ok(())
}

fn nested_path_ordinal(parent: usize, child: usize) -> Result<u32, GovernorError> {
    parent
        .checked_mul(64)
        .and_then(|value| value.checked_add(child))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(GovernorError::LimitExceeded)
}

fn registered_path_array(value: &Value) -> Result<&[Value], GovernorError> {
    let paths = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if paths.is_empty() {
        return Err(GovernorError::InvalidAuthoring);
    }
    if paths.len() > 64 {
        return Err(GovernorError::LimitExceeded);
    }
    if paths
        .iter()
        .any(|path| path.as_str().is_none_or(str::is_empty))
    {
        return Err(GovernorError::InvalidAuthoring);
    }
    Ok(paths)
}

fn collect_context_paths(
    node_id: &str,
    value: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let object = nonempty_object(value)?;
    if let Some(includes) = object.get("include") {
        let includes = includes.as_array().ok_or(GovernorError::InvalidAuthoring)?;
        for (include_index, include) in includes.iter().enumerate() {
            let include = nonempty_object(include)?;
            if include.get("type").and_then(Value::as_str) == Some("source_scope") {
                let paths = registered_path_array(
                    include
                        .get("paths")
                        .ok_or(GovernorError::InvalidAuthoring)?,
                )?;
                for (path_index, path) in paths.iter().enumerate() {
                    collector.register(
                        ContentOwnerKind::Node,
                        node_id,
                        ContentFieldKind::ContextPath,
                        nested_path_ordinal(include_index, path_index)?,
                        path,
                    )?;
                }
            } else if include.contains_key("paths") {
                return Err(GovernorError::InvalidAuthoring);
            }
        }
    }
    Ok(())
}

fn collect_permission_paths(
    node_id: &str,
    value: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let permissions = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    for (permission_index, permission) in permissions.iter().enumerate() {
        let Some(scope) = permission.as_object().and_then(|value| value.get("scope")) else {
            continue;
        };
        let scope = nonempty_object(scope)?;
        if let Some(value) = scope.get("paths") {
            for (path_index, path) in registered_path_array(value)?.iter().enumerate() {
                collector.register(
                    ContentOwnerKind::Node,
                    node_id,
                    ContentFieldKind::PermissionPath,
                    nested_path_ordinal(permission_index, path_index)?,
                    path,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_isolation_paths(
    node_id: &str,
    value: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let isolation = nonempty_object(value)?;
    let Some(filesystem) = isolation.get("filesystem") else {
        return Ok(());
    };
    let filesystem = nonempty_object(filesystem)?;
    let Some(paths) = filesystem.get("writablePaths") else {
        return Ok(());
    };
    for (index, path) in registered_path_array(paths)?.iter().enumerate() {
        collector.register(
            ContentOwnerKind::Node,
            node_id,
            ContentFieldKind::IsolationPath,
            u32::try_from(index).map_err(|_| GovernorError::LimitExceeded)?,
            path,
        )?;
    }
    Ok(())
}

fn collect_completion_content(
    owner_kind: ContentOwnerKind,
    owner_id: &str,
    value: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    let requires_len = object
        .get("requires")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    for (group, base) in [("requires", 0usize), ("forbids", requires_len)] {
        let Some(items) = object.get(group) else {
            continue;
        };
        let items = items.as_array().ok_or(GovernorError::InvalidAuthoring)?;
        if items.len() > 64 {
            return Err(GovernorError::LimitExceeded);
        }
        for (index, item) in items.iter().enumerate() {
            if let Some(expression) = item.as_object().and_then(|object| object.get("expression")) {
                validate_string(expression)?;
                collector.register(
                    owner_kind,
                    owner_id,
                    ContentFieldKind::CompletionContract,
                    u32::try_from(base + index).map_err(|_| GovernorError::LimitExceeded)?,
                    expression,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_policy_content(
    graph_id: &str,
    ordinal: usize,
    policy: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    if policy.is_string() {
        return Ok(());
    }
    let object = nonempty_object(policy)?;
    if object.contains_key("ref") {
        if object.len() != 1 {
            return Err(GovernorError::InvalidAuthoring);
        }
        validate_string(&object["ref"])?;
        return Ok(());
    }
    if object.len() != 1 {
        return Err(GovernorError::InvalidAuthoring);
    }
    let inline = object
        .get("inlineConstraint")
        .ok_or(GovernorError::InvalidAuthoring)
        .and_then(nonempty_object)?;
    if inline.contains_key("manualOverride") && inline.len() != 1 {
        return Err(GovernorError::InvalidAuthoring);
    }
    for (key, value) in inline {
        let offset = match key.as_str() {
            "ruleText" => Some(0),
            "explanation" => Some(1),
            "deny" | "reason" | "manualOverride" => None,
            _ => return Err(GovernorError::InvalidAuthoring),
        };
        if let Some(offset) = offset {
            validate_string(value)?;
            collector.register(
                ContentOwnerKind::Policy,
                graph_id,
                ContentFieldKind::PolicyText,
                policy_text_ordinal(ordinal, offset)?,
                value,
            )?;
        }
    }
    Ok(())
}

fn policy_text_ordinal(policy: usize, offset: usize) -> Result<u32, GovernorError> {
    policy
        .checked_mul(2)
        .and_then(|value| value.checked_add(offset))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(GovernorError::LimitExceeded)
}

fn collect_agent_content(
    node_id: &str,
    value: &Value,
    collector: &mut ContentCollector,
) -> Result<(), GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    if let Some(reference) = object.get("ref") {
        validate_string(reference)?;
        if object.keys().any(|key| key != "ref") {
            return Err(GovernorError::InvalidAuthoring);
        }
        return Ok(());
    }
    let ephemeral = object
        .get("ephemeral")
        .and_then(Value::as_object)
        .ok_or(GovernorError::InvalidAuthoring)?;
    if object.keys().any(|key| key != "ephemeral") {
        return Err(GovernorError::InvalidAuthoring);
    }
    for (key, value) in ephemeral {
        match key.as_str() {
            "purpose" => collector.register(
                ContentOwnerKind::Agent,
                node_id,
                ContentFieldKind::Purpose,
                0,
                value,
            )?,
            "instructions" => collector.register(
                ContentOwnerKind::Agent,
                node_id,
                ContentFieldKind::Instructions,
                0,
                value,
            )?,
            "completionContract" => collector.register(
                ContentOwnerKind::Agent,
                node_id,
                ContentFieldKind::CompletionContract,
                0,
                value,
            )?,
            "capabilities" | "allowedTools" | "prohibitedActions" => validate_string_array(value)?,
            "inputSchema" | "outputSchema" | "instructionsRef" | "isolationMinimum" => {
                validate_string(value)?
            }
            "modelRequirements" | "contextStrategy" | "evidenceRequirements" | "memoryPolicy" => {}
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    Ok(())
}

fn build_topology(
    graph: &graphhelm_protocols::ExecutionGraph,
    slots: &[ContentSlot],
) -> Result<PersistedTopology, GovernorError> {
    let graph_id =
        OpaqueId::parse(&graph.metadata.id).map_err(|_| GovernorError::InvalidAuthoring)?;
    let execution_id = ExecutionId::parse(&graph.metadata.execution_id)
        .map_err(|_| GovernorError::InvalidAuthoring)?;
    let labels = graph
        .metadata
        .labels
        .iter()
        .map(|(key, value)| {
            Ok((
                SafeKey::parse(key).map_err(|_| GovernorError::InvalidAuthoring)?,
                SafeValue::parse(value).map_err(|_| GovernorError::InvalidAuthoring)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, GovernorError>>()?;
    let entrypoints = graph
        .spec
        .entrypoints
        .iter()
        .map(|id| OpaqueId::parse(id).map_err(|_| GovernorError::InvalidAuthoring))
        .collect::<Result<Vec<_>, _>>()?;
    let nodes = graph
        .spec
        .nodes
        .iter()
        .map(|(node_id, node)| {
            let id = OpaqueId::parse(node_id).map_err(|_| GovernorError::InvalidAuthoring)?;
            let content_slot_ids = slots
                .iter()
                .filter(|slot| {
                    matches!(
                        slot.owner_kind(),
                        ContentOwnerKind::Node | ContentOwnerKind::Agent
                    ) && slot.owner_id() == &id
                })
                .map(|slot| slot.slot_id().clone())
                .collect::<Vec<_>>();
            let controls = build_node_controls(node_id, node, slots)?;
            Ok((
                id,
                PersistedNode::new(
                    node.node_type.clone(),
                    node.optionality.clone(),
                    controls,
                    content_slot_ids,
                    // The operator's own declaration, the one `GHG101_DEFAULT_TIMEOUT` warns
                    // about when it is missing. Persistence used to drop it here, which left
                    // the attention seam with no budget to compare a node's silence against —
                    // so it answered `unknown` forever and no surface could ever say "sleep".
                    // Read through the single definition in `protocols`, which the CLI also
                    // uses to record the declared form at start. Two readings of one rule is
                    // how the first divergence becomes invisible.
                    graphhelm_protocols::declared_timeout_seconds(node),
                    // M11 #160, and the same lesson as the line above: the customs budgets are
                    // declared in the graph and used by the FOLD, which reads the sealed version
                    // — so they have to cross here or they never arrive. Read through the single
                    // definition on the node type, for the reason the neighbouring comment gives:
                    // two readings of one rule is how the first divergence becomes invisible.
                    //
                    // A malformed block REFUSES publication rather than publishing without a
                    // budget. A budget that silently vanishes reads downstream exactly like a
                    // stage with infinite patience, which is the failure this milestone exists
                    // to end.
                    node.customs()
                        .map_err(|_| GovernorError::InvalidAuthoring)?
                        .map(|customs| {
                            graphhelm_protocols::PersistedCustoms::new(
                                customs.budgets.wait_within_seconds,
                                customs.budgets.clearance_within_seconds,
                                customs.budgets.dlq_within_seconds,
                            )
                        }),
                )
                .map_err(|_| GovernorError::InvalidProjection)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, GovernorError>>()?;
    let edges = graph
        .spec
        .edges
        .iter()
        .map(|edge| build_edge(edge, slots))
        .collect::<Result<Vec<_>, _>>()?;
    let budgets = build_budgets(&graph.spec.budgets)?;
    let policies = graph
        .spec
        .policies
        .iter()
        .enumerate()
        .map(|(ordinal, policy)| build_policy_control(&graph.metadata.id, ordinal, policy, slots))
        .collect::<Result<Vec<_>, _>>()?;
    let derived_terminals = if graph.spec.completion.get("terminalNodes").is_none() {
        let nodes_with_outgoing = graph
            .spec
            .edges
            .iter()
            .filter(|edge| {
                graph.spec.nodes.contains_key(&edge.from) && graph.spec.nodes.contains_key(&edge.to)
            })
            .map(|edge| edge.from.as_str())
            .collect::<BTreeSet<_>>();
        Some(
            graph
                .spec
                .nodes
                .keys()
                .filter(|node| !nodes_with_outgoing.contains(node.as_str()))
                .cloned()
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let completion = build_completion_control(
        "graph_completion",
        ContentOwnerKind::Graph,
        &graph.metadata.id,
        &graph.spec.completion,
        slots,
        derived_terminals.as_deref(),
    )?;
    PersistedTopology::new(
        graph_id,
        execution_id,
        labels,
        entrypoints,
        nodes,
        edges,
        budgets,
        policies,
        completion,
    )
    .map_err(|_| GovernorError::InvalidProjection)
}

fn build_node_controls(
    node_id: &str,
    node: &GraphNode,
    slots: &[ContentSlot],
) -> Result<Vec<PersistedControl>, GovernorError> {
    validate_node_kind_property_ownership(node)?;
    let mut controls = Vec::new();
    if let Some(agent) = node.properties.get("agent") {
        controls.extend(build_agent_controls(agent)?);
    }
    if let Some(crew) = node.properties.get("agents") {
        controls.push(build_crew_control(crew)?);
    }
    for key in ["input", "output"] {
        if let Some(value) = node.properties.get(key) {
            controls.push(build_contract_control(key, value)?);
        }
    }
    for (field, control_type, builder) in [
        (
            "model",
            "node_model",
            build_model_control as fn(&str, &Value) -> _,
        ),
        ("retry", "node_retry", build_retry_control),
        ("resources", "node_resources", build_resources_control),
        ("memory", "node_memory", build_memory_control),
    ] {
        if let Some(value) = node.properties.get(field) {
            controls.push(builder(control_type, value)?);
        }
    }
    if let Some(value) = node.properties.get("context") {
        controls.push(build_context_control(node_id, value, slots)?);
    }
    if let Some(value) = node.properties.get("permissions") {
        controls.push(build_permissions_control(node_id, value, slots)?);
    }
    if let Some(value) = node.properties.get("isolation") {
        controls.push(build_isolation_control(node_id, value, slots)?);
    }
    if let Some(value) = node.properties.get("completion") {
        controls.push(build_completion_control(
            "node_completion",
            ContentOwnerKind::Node,
            node_id,
            value,
            slots,
            None,
        )?);
    }
    if let Some(common) = build_common_control(node)? {
        controls.push(common);
    }
    if let Some(value) = node.properties.get("loop") {
        controls.push(build_loop_control(value)?);
    }
    if let Some(kind_control) = build_node_kind_control(node_id, node, slots)? {
        controls.push(kind_control);
    }
    let mut configuration = ControlBuilder::default();
    if let Some(target) = node.properties.get("targetRef") {
        configuration.reference("targetRef", target)?;
    }
    if let Some(timeout) = node.properties.get("timeoutSeconds") {
        configuration.integer("timeoutSeconds", timeout)?;
    }
    for key in ["userEditable", "userOverrideAllowed"] {
        if let Some(value) = node.properties.get(key) {
            configuration.flag(key, value)?;
        }
    }
    if !configuration.is_empty() {
        controls.push(configuration.finish("node_configuration")?);
    }
    controls.sort_by_key(|control| {
        persisted_node_control_order(control.control_type().as_str()).unwrap_or(u8::MAX)
    });
    Ok(controls)
}

fn validate_loop_control(value: &Value) -> Result<(), GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    if object.len() != 1 || !object.contains_key("maxIterations") {
        return Err(GovernorError::InvalidAuthoring);
    }
    validate_positive_integer(&object["maxIterations"])
}

fn build_loop_control(value: &Value) -> Result<PersistedControl, GovernorError> {
    validate_loop_control(value)?;
    let mut builder = ControlBuilder::default();
    builder.integer("maxIterations", &value["maxIterations"])?;
    builder.finish("node_loop")
}

fn validate_node_kind_property_ownership(node: &GraphNode) -> Result<(), GovernorError> {
    const KIND_FIELDS: &[&str] = &[
        "agent",
        "agents",
        "tool",
        "classifier",
        "gate",
        "onFail",
        "override",
        "strategy",
        "merge",
        "prompt",
        "options",
        "timeout",
        "graphRef",
        "parameters",
        "expose",
        "materializer",
        "adapterRef",
        "preconditions",
        "effects",
        "targetRef",
    ];
    let allowed = match node.node_type.as_str() {
        "agent" => &["agent", "agents"][..],
        "tool" => &["tool"][..],
        "classifier" => &["classifier"][..],
        "gate" => &["gate", "onFail", "override"][..],
        "fork" => &["strategy"][..],
        "join" => &["strategy", "merge"][..],
        "human_decision" => &["prompt", "options", "timeout"][..],
        "subgraph" => &["graphRef", "parameters", "expose"][..],
        "materializer" => &["materializer"][..],
        "deploy" => &["adapterRef", "preconditions", "effects", "targetRef"][..],
        "rollback" => &["adapterRef"][..],
        "planner" | "evaluator" | "timer" | "trigger" | "artifact_transform" => &[][..],
        _ => return Err(GovernorError::InvalidAuthoring),
    };
    if node
        .properties
        .keys()
        .any(|key| KIND_FIELDS.contains(&key.as_str()) && !allowed.contains(&key.as_str()))
    {
        return Err(GovernorError::InvalidAuthoring);
    }
    Ok(())
}

fn build_common_control(node: &GraphNode) -> Result<Option<PersistedControl>, GovernorError> {
    let mut builder = ControlBuilder::default();
    if let Some(tags) = node.properties.get("tags") {
        builder.token_array_unique("tag", tags)?;
    }
    for key in ["onCancel", "onFailure"] {
        if let Some(value) = node.properties.get(key) {
            builder.token(key, value)?;
        }
    }
    if builder.is_empty() {
        Ok(None)
    } else {
        builder.finish("node_common").map(Some)
    }
}

fn build_node_kind_control(
    node_id: &str,
    node: &GraphNode,
    slots: &[ContentSlot],
) -> Result<Option<PersistedControl>, GovernorError> {
    let mut builder = ControlBuilder::default();
    match node.node_type.as_str() {
        "tool" => {
            if let Some(value) = node.properties.get("tool") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "ref" => builder.reference("toolRef", value)?,
                        "action" => builder.enum_token("action", value, &["execute"])?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "tool_configuration")
        }
        "classifier" => {
            if let Some(value) = node.properties.get("classifier") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "method" => builder.enum_token("method", value, &["hybrid"])?,
                        "profile" => builder.token("profile", value)?,
                        "deterministicRulesRef" => builder.reference("rulesRef", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "classifier_configuration")
        }
        "gate" => {
            if let Some(value) = node.properties.get("gate") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "requirements" => builder.token_array("requirement", value)?,
                        "evaluators" => builder.reference_array("evaluator", value)?,
                        "passWhen" => builder.enum_token("passWhen", value, &["all"])?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            if let Some(value) = node.properties.get("onFail") {
                let object = nonempty_object(value)?;
                if object.len() != 1 {
                    return Err(GovernorError::InvalidAuthoring);
                }
                builder.token(
                    "failureRoute",
                    object
                        .get("routeTo")
                        .ok_or(GovernorError::InvalidAuthoring)?,
                )?;
            }
            if let Some(value) = node.properties.get("override") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "allowedRoles" => builder.token_array_unique("overrideRole", value)?,
                        "resultLabel" => builder.token("overrideResult", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "gate_configuration")
        }
        "fork" => {
            if let Some(value) = node.properties.get("strategy") {
                builder.enum_token("strategy", value, &["all"])?;
            }
            finish_optional(builder, "fork_configuration")
        }
        "join" => {
            if let Some(value) = node.properties.get("strategy") {
                builder.enum_token(
                    "strategy",
                    value,
                    &[
                        "all_completed",
                        "all_succeeded",
                        "any_succeeded",
                        "quorum",
                        "first_valid",
                        "custom_evaluator",
                    ],
                )?;
            }
            if let Some(value) = node.properties.get("merge") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "method" => builder.token("mergeMethod", value)?,
                        "outputSchema" => builder.reference("resultSchema", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "join_configuration")
        }
        "human_decision" => {
            if node.properties.contains_key("prompt") {
                let slot = find_slot(
                    slots,
                    ContentOwnerKind::Node,
                    node_id,
                    ContentFieldKind::Instructions,
                    0,
                )?;
                builder.slot("directiveSlot", slot)?;
            }
            if let Some(value) = node.properties.get("options") {
                builder.token_array_unique("option", value)?;
            }
            if let Some(value) = node.properties.get("timeout") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "seconds" => builder.integer("timeoutSeconds", value)?,
                        "onTimeout" => builder.enum_token("timeoutAction", value, &["pause"])?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "human_decision")
        }
        "subgraph" => {
            if let Some(value) = node.properties.get("graphRef") {
                builder.reference("graphRef", value)?;
            }
            if let Some(value) = node.properties.get("parameters") {
                let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
                if object.len() > 64 {
                    return Err(GovernorError::LimitExceeded);
                }
                builder.count("parameterCount", object.len())?;
                for (index, (key, value)) in object.iter().enumerate() {
                    builder.token(
                        &format!("parameterKey.{index:03}"),
                        &Value::String(key.clone()),
                    )?;
                    builder.reference(&format!("parameterValue.{index:03}"), value)?;
                }
            }
            if let Some(value) = node.properties.get("expose") {
                let object = nonempty_object(value)?;
                if object.len() != 1 {
                    return Err(GovernorError::InvalidAuthoring);
                }
                builder.token_array_unique(
                    "exposedResult",
                    object
                        .get("outputs")
                        .ok_or(GovernorError::InvalidAuthoring)?,
                )?;
            }
            finish_optional(builder, "subgraph_configuration")
        }
        "materializer" => {
            if let Some(value) = node.properties.get("materializer") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "target" => builder.reference("target", value)?,
                        "strategy" => {
                            builder.enum_token("strategy", value, &["evidence_backed_patch"])?
                        }
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "materializer_configuration")
        }
        "deploy" => {
            if let Some(value) = node.properties.get("adapterRef") {
                builder.reference("adapterRef", value)?;
            }
            if let Some(value) = node.properties.get("preconditions") {
                builder.reference_array("precondition", value)?;
            }
            if let Some(value) = node.properties.get("effects") {
                let object = nonempty_object(value)?;
                for (key, value) in object {
                    match key.as_str() {
                        "reversible" | "compensationRequired" => builder.flag(key, value)?,
                        "compensationNode" => builder.token("compensationNode", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            finish_optional(builder, "deploy_configuration")
        }
        "rollback" => {
            if let Some(value) = node.properties.get("adapterRef") {
                builder.reference("adapterRef", value)?;
            }
            finish_optional(builder, "rollback_configuration")
        }
        "agent" | "planner" | "evaluator" | "timer" | "trigger" | "artifact_transform" => Ok(None),
        _ => Err(GovernorError::InvalidAuthoring),
    }
}

fn finish_optional(
    builder: ControlBuilder,
    control_type: &str,
) -> Result<Option<PersistedControl>, GovernorError> {
    if builder.is_empty() {
        Ok(None)
    } else {
        builder.finish(control_type).map(Some)
    }
}

fn nonempty_object(value: &Value) -> Result<&serde_json::Map<String, Value>, GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    if object.is_empty() {
        return Err(GovernorError::InvalidAuthoring);
    }
    Ok(object)
}

fn build_completion_control(
    control_type: &str,
    owner_kind: ContentOwnerKind,
    owner_id: &str,
    value: &Value,
    slots: &[ContentSlot],
    derived_terminals: Option<&[String]>,
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    if control_type == "graph_completion" {
        if let Some(terminals) = derived_terminals {
            builder.token_array_unique("terminal", &serde_json::json!(terminals))?;
        }
        for (key, value) in object {
            match key.as_str() {
                "terminalNodes" => builder.token_array_unique("terminal", value)?,
                "requires" => builder.reference_array("requirement", value)?,
                "allowWaivers" => builder.flag("allowWaivers", value)?,
                "statuses" => {
                    let statuses = nonempty_object(value)?;
                    for (status, value) in statuses {
                        match status.as_str() {
                            "full" => builder.token("statusFull", value)?,
                            "waived" => builder.token("statusWaived", value)?,
                            _ => return Err(GovernorError::InvalidAuthoring),
                        }
                    }
                }
                _ => return Err(GovernorError::InvalidAuthoring),
            }
        }
        return builder.finish(control_type);
    }

    let requires_len = object
        .get("requires")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    for (key, value) in object {
        match key.as_str() {
            "contractRef" => builder.reference("contractRef", value)?,
            "requires" | "forbids" => {
                let items = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
                if items.len() > 64 {
                    return Err(GovernorError::LimitExceeded);
                }
                builder.count(&format!("{key}Count"), items.len())?;
                let base = if key == "requires" { 0 } else { requires_len };
                for (index, item) in items.iter().enumerate() {
                    let ordinal = base + index;
                    match item {
                        Value::String(_) => builder.token(&format!("{key}.{index:03}"), item)?,
                        Value::Object(item) if !item.is_empty() && item.len() == 1 => {
                            let (field, value) = item.iter().next().unwrap();
                            match field.as_str() {
                                "outputSchemaValid" => {
                                    builder.flag(&format!("{key}SchemaValid.{index:03}"), value)?
                                }
                                "artifactExists" => {
                                    builder.artifact(&format!("{key}Artifact.{index:03}"), value)?
                                }
                                "expression" => {
                                    let slot = find_slot(
                                        slots,
                                        owner_kind,
                                        owner_id,
                                        ContentFieldKind::CompletionContract,
                                        u32::try_from(ordinal)
                                            .map_err(|_| GovernorError::LimitExceeded)?,
                                    )?;
                                    builder.slot(&format!("{key}Slot.{index:03}"), slot)?;
                                }
                                "evidence" => {
                                    let evidence = nonempty_object(value)?;
                                    for (child, value) in evidence {
                                        match child.as_str() {
                                            "type" => builder.token(
                                                &format!("{key}EvidenceType.{index:03}"),
                                                value,
                                            )?,
                                            "min" => builder.integer(
                                                &format!("{key}EvidenceMin.{index:03}"),
                                                value,
                                            )?,
                                            _ => return Err(GovernorError::InvalidAuthoring),
                                        }
                                    }
                                }
                                _ => return Err(GovernorError::InvalidAuthoring),
                            }
                        }
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            // M11 #160. This match is key-EXHAUSTIVE, so until it learned `customs` a graph
            // declaring the field was refused outright at publication with `InvalidAuthoring` —
            // the whole feature undeliverable regardless of how correct the fold was.
            //
            // The two children are treated differently ON PURPOSE, and the asymmetry is the
            // interesting part:
            //
            // `proofKinds` is encoded here because this control is its ONLY carrier. Drop
            // it and the operator's declaration of what a claim must present dies at sealing,
            // silently, which is the exact defect `timeout_seconds` was added to fix one
            // milestone earlier.
            //
            // `budgets` is deliberately NOT encoded here, and that is not an oversight: it
            // already crosses to the sealed form as `PersistedNode::customs`, which is where the
            // fold reads it. Encoding it a second time would put two spellings of one
            // declaration in the topology, and two spellings of one fact drift — the reader of
            // the second copy has no way to know which one the deadline was computed from.
            //
            // The other consumer of this block does NOT share this strictness. Both are correct
            // for their jobs and neither generalizes: `collect_completion_content` is
            // key-SELECTIVE and permissive, walking `requires`/`forbids` only, which is why no
            // budget leaks into externalized content. That difference predates this change, and
            // anyone reading either half alone will infer a uniform policy that does not exist.
            "customs" => {
                let customs = nonempty_object(value)?;
                for (child, value) in customs {
                    match child.as_str() {
                        "proofKinds" => {
                            let items = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
                            if items.len() > 64 {
                                return Err(GovernorError::LimitExceeded);
                            }
                            builder.count("customsProofCount", items.len())?;
                            builder.token_array_unique("customsProof", value)?;
                        }
                        "budgets" => {}
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish(control_type)
}

fn build_policy_control(
    graph_id: &str,
    ordinal: usize,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<PersistedControl, GovernorError> {
    let mut builder = ControlBuilder::default();
    let mut has_enforcement_identity = false;
    match value {
        Value::String(_) => {
            has_enforcement_identity = true;
            builder.token("mode", &Value::String("ref".into()))?;
            builder.reference("policyRef", value)?;
        }
        Value::Object(object) if object.len() == 1 && object.contains_key("ref") => {
            has_enforcement_identity = true;
            builder.token("mode", &Value::String("ref".into()))?;
            builder.reference("policyRef", &object["ref"])?;
        }
        Value::Object(object) if object.len() == 1 && object.contains_key("inlineConstraint") => {
            builder.token("mode", &Value::String("inline".into()))?;
            let inline = nonempty_object(&object["inlineConstraint"])?;
            if inline.contains_key("manualOverride") && inline.len() != 1 {
                return Err(GovernorError::InvalidAuthoring);
            }
            for (child, value) in inline {
                match child.as_str() {
                    "deny" => {
                        has_enforcement_identity = true;
                        builder.token_array_unique("deny", value)?;
                    }
                    "reason" => builder.token("reasonCode", value)?,
                    "manualOverride" => {
                        has_enforcement_identity = true;
                        let override_value = nonempty_object(value)?;
                        if override_value.len() != 3
                            || !override_value.contains_key("bypassedRequirements")
                            || !override_value.contains_key("acknowledgedRisks")
                            || !override_value.contains_key("resultLabel")
                            || override_value["bypassedRequirements"]
                                .as_array()
                                .is_none_or(Vec::is_empty)
                            || override_value["acknowledgedRisks"]
                                .as_array()
                                .is_none_or(Vec::is_empty)
                        {
                            return Err(GovernorError::InvalidAuthoring);
                        }
                        builder.token("mode", &Value::String("manual_override".into()))?;
                        for (field, value) in override_value {
                            match field.as_str() {
                                "bypassedRequirements" => {
                                    builder.token_array_unique("bypassedRequirement", value)?
                                }
                                "acknowledgedRisks" => {
                                    builder.token_array_unique("acknowledgedRisk", value)?
                                }
                                "resultLabel" => builder.token("resultLabel", value)?,
                                _ => return Err(GovernorError::InvalidAuthoring),
                            }
                        }
                    }
                    "ruleText" | "explanation" => {
                        let (key, offset) = if child == "ruleText" {
                            ("ruleTextSlot", 0)
                        } else {
                            ("explanationSlot", 1)
                        };
                        let slot = find_slot(
                            slots,
                            ContentOwnerKind::Policy,
                            graph_id,
                            ContentFieldKind::PolicyText,
                            policy_text_ordinal(ordinal, offset)?,
                        )?;
                        builder.slot(key, slot)?;
                    }
                    _ => return Err(GovernorError::InvalidAuthoring),
                }
            }
        }
        _ => return Err(GovernorError::InvalidAuthoring),
    }
    if !has_enforcement_identity {
        return Err(GovernorError::InvalidAuthoring);
    }
    builder.finish("policy_control")
}

#[derive(Default)]
struct ControlBuilder {
    identifiers: BTreeMap<SafeKey, SafeValue>,
    digests: BTreeMap<SafeKey, RawSha256>,
    integers: BTreeMap<SafeKey, i64>,
    flags: BTreeMap<SafeKey, bool>,
}

impl ControlBuilder {
    fn is_empty(&self) -> bool {
        self.identifiers.is_empty()
            && self.digests.is_empty()
            && self.integers.is_empty()
            && self.flags.is_empty()
    }

    fn token(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        self.insert_identifier(safe_key(key)?, safe_token(value)?)
    }

    fn enum_token(
        &mut self,
        key: &str,
        value: &Value,
        allowed: &[&str],
    ) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        if !allowed.contains(&value) {
            return Err(GovernorError::InvalidAuthoring);
        }
        self.insert_identifier(safe_key(key)?, safe_token(value)?)
    }

    fn reference(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        self.insert_identifier(safe_key(key)?, encode_safe_reference(value)?)
    }

    fn artifact(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        let locator = if value.starts_with("artifact://") {
            value.to_owned()
        } else {
            if value.is_empty()
                || value.len() > 64
                || value.contains('/')
                || value.contains('\\')
                || value.contains(':')
            {
                return Err(GovernorError::InvalidAuthoring);
            }
            format!("artifact://{value}")
        };
        self.insert_identifier(safe_key(key)?, encode_safe_reference(&locator)?)
    }

    fn binding(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        self.insert_identifier(safe_key(key)?, encode_safe_binding(value)?)
    }

    fn digest(&mut self, key: &str, value: RawSha256) -> Result<(), GovernorError> {
        if !self.digests.contains_key(&safe_key(key)?) && self.digests.len() >= 128 {
            return Err(GovernorError::LimitExceeded);
        }
        self.digests.insert(safe_key(key)?, value);
        Ok(())
    }

    fn reference_or_nominal(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
        let encoded = if value.contains([':', '/']) {
            encode_safe_reference(value)?
        } else {
            parse_persisted_nominal_identifier(value)
                .map_err(|_| GovernorError::InvalidAuthoring)?
        };
        self.insert_identifier(safe_key(key)?, encoded)
    }

    fn integer(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        let value = value.as_u64().ok_or(GovernorError::InvalidAuthoring)?;
        if value > 9_007_199_254_740_991 {
            return Err(GovernorError::LimitExceeded);
        }
        self.integers.insert(
            safe_key(key)?,
            i64::try_from(value).map_err(|_| GovernorError::InvalidAuthoring)?,
        );
        Ok(())
    }

    fn flag(&mut self, key: &str, value: &Value) -> Result<(), GovernorError> {
        self.flags.insert(
            safe_key(key)?,
            value.as_bool().ok_or(GovernorError::InvalidAuthoring)?,
        );
        Ok(())
    }

    fn count(&mut self, key: &str, value: usize) -> Result<(), GovernorError> {
        if value > 9_007_199_254_740_991 {
            return Err(GovernorError::LimitExceeded);
        }
        self.integers.insert(
            safe_key(key)?,
            i64::try_from(value).map_err(|_| GovernorError::LimitExceeded)?,
        );
        Ok(())
    }

    fn slot(&mut self, key: &str, slot: &ContentSlot) -> Result<(), GovernorError> {
        self.insert_identifier(
            safe_key(key)?,
            SafeValue::parse(slot.slot_id().as_str())
                .map_err(|_| GovernorError::InvalidProjection)?,
        )
    }

    fn token_array(&mut self, prefix: &str, value: &Value) -> Result<(), GovernorError> {
        self.string_array(prefix, value, safe_token)
    }

    fn token_array_unique(&mut self, prefix: &str, value: &Value) -> Result<(), GovernorError> {
        let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
        let mut seen = BTreeSet::new();
        for value in values {
            let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
            if !seen.insert(value) {
                return Err(GovernorError::InvalidAuthoring);
            }
        }
        self.string_array(prefix, value, safe_token)
    }

    fn reference_array(&mut self, prefix: &str, value: &Value) -> Result<(), GovernorError> {
        self.string_array(prefix, value, encode_safe_reference)
    }

    fn string_array(
        &mut self,
        prefix: &str,
        value: &Value,
        parse: fn(&str) -> Result<SafeValue, GovernorError>,
    ) -> Result<(), GovernorError> {
        let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
        if values.len() > 64 {
            return Err(GovernorError::LimitExceeded);
        }
        self.count(&format!("{prefix}Count"), values.len())?;
        for (index, value) in values.iter().enumerate() {
            let value = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
            self.insert_identifier(indexed_key(prefix, index)?, parse(value)?)?;
        }
        Ok(())
    }

    fn insert_identifier(&mut self, key: SafeKey, value: SafeValue) -> Result<(), GovernorError> {
        if !self.identifiers.contains_key(&key) && self.identifiers.len() >= 128 {
            return Err(GovernorError::LimitExceeded);
        }
        self.identifiers.insert(key, value);
        Ok(())
    }

    fn finish(mut self, control_type: &str) -> Result<PersistedControl, GovernorError> {
        self.flags.insert(safe_key("present")?, true);
        control(
            control_type,
            self.identifiers,
            self.digests,
            self.integers,
            self.flags,
        )
    }
}

fn indexed_key(prefix: &str, index: usize) -> Result<SafeKey, GovernorError> {
    if index > 999 {
        return Err(GovernorError::LimitExceeded);
    }
    safe_key(&format!("{prefix}.{index:03}"))
}

fn safe_token(value: &str) -> Result<SafeValue, GovernorError> {
    SafeValue::parse(value).map_err(|_| GovernorError::InvalidAuthoring)
}

fn encode_safe_reference(value: &str) -> Result<SafeValue, GovernorError> {
    const MAX_REFERENCE_BYTES: usize = 91;
    if value.len() > MAX_REFERENCE_BYTES {
        return Err(GovernorError::LimitExceeded);
    }
    encode_persisted_reference(value).map_err(|_| GovernorError::InvalidAuthoring)
}

fn encode_safe_binding(value: &str) -> Result<SafeValue, GovernorError> {
    if value.starts_with("outputs.") || value.starts_with("nodes.") {
        return parse_persisted_binding(value).map_err(|_| GovernorError::InvalidAuthoring);
    }
    let encoded = encode_safe_reference(value)?;
    parse_persisted_binding(encoded.as_str()).map_err(|_| GovernorError::InvalidAuthoring)
}

/// The other agents working one task, each NAMED by a reference.
///
/// A crew member is never defined inline: an `ephemeral` definition externalizes into an
/// `agent_configuration` control, and a persisted node carries at most one control of each type,
/// so a second inline definition on the same node has nowhere to be recorded. The node schema
/// admits exactly this shape; this is the same contract, enforced where the projection is built.
fn collect_crew_references(value: &Value) -> Result<Vec<Value>, GovernorError> {
    let members = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if members.is_empty() {
        return Err(GovernorError::InvalidAuthoring);
    }
    if members.len() > 64 {
        return Err(GovernorError::LimitExceeded);
    }
    members
        .iter()
        .map(|member| {
            let member = member.as_object().ok_or(GovernorError::InvalidAuthoring)?;
            if member.len() != 1 {
                return Err(GovernorError::InvalidAuthoring);
            }
            let reference = member.get("ref").ok_or(GovernorError::InvalidAuthoring)?;
            validate_string(reference)?;
            Ok(reference.clone())
        })
        .collect()
}

fn build_crew_control(value: &Value) -> Result<PersistedControl, GovernorError> {
    let references = Value::Array(collect_crew_references(value)?);
    let mut builder = ControlBuilder::default();
    builder.reference_array("agentRef", &references)?;
    builder.finish("node_agents")
}

fn build_agent_controls(value: &Value) -> Result<Vec<PersistedControl>, GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    let mut controls = Vec::new();
    let mut agent = ControlBuilder::default();
    if let Some(reference) = object.get("ref") {
        if object.len() != 1 {
            return Err(GovernorError::InvalidAuthoring);
        }
        agent.token("mode", &Value::String("ref".into()))?;
        agent.reference("agentRef", reference)?;
    } else {
        if object.len() != 1 {
            return Err(GovernorError::InvalidAuthoring);
        }
        let ephemeral = object
            .get("ephemeral")
            .and_then(Value::as_object)
            .ok_or(GovernorError::InvalidAuthoring)?;
        agent.token("mode", &Value::String("ephemeral".into()))?;
        for (key, value) in ephemeral {
            match key.as_str() {
                "purpose" | "instructions" | "completionContract" => {}
                "capabilities" => agent.token_array_unique("capability", value)?,
                "allowedTools" => agent.token_array("allowedTool", value)?,
                "prohibitedActions" => agent.token_array("prohibitedAction", value)?,
                "inputSchema" => agent.reference("inputSchema", value)?,
                "outputSchema" => agent.reference("resultSchema", value)?,
                "instructionsRef" => agent.reference_or_nominal("directiveRef", value)?,
                "isolationMinimum" => {
                    let tier = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
                    if !is_valid_isolation_tier(tier) {
                        return Err(GovernorError::InvalidAuthoring);
                    }
                    agent.token("isolation", value)?;
                }
                "modelRequirements" => {
                    controls.push(build_model_control("agent_model_requirements", value)?)
                }
                "contextStrategy" => controls.push(build_context_strategy_control(value)?),
                "evidenceRequirements" => {
                    controls.push(build_evidence_requirements_control(value)?)
                }
                "memoryPolicy" => {
                    controls.push(build_memory_control("agent_memory_policy", value)?)
                }
                _ => return Err(GovernorError::InvalidAuthoring),
            }
        }
    }
    controls.insert(0, agent.finish("agent_configuration")?);
    Ok(controls)
}

fn build_model_control(
    control_type: &str,
    value: &Value,
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "profile" | "routePolicy" => builder.token(key, value)?,
            "requireIndependentFrom" => builder.token_array("independent", value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish(control_type)
}

fn build_context_strategy_control(value: &Value) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "includeScopes" => builder.token_array("includeScope", value)?,
            "maxTokens" => builder.integer("maxUnits", value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish("agent_context_strategy")
}

fn build_evidence_requirements_control(value: &Value) -> Result<PersistedControl, GovernorError> {
    let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if values.len() > 32 {
        return Err(GovernorError::LimitExceeded);
    }
    let mut builder = ControlBuilder::default();
    builder.count("requirementCount", values.len())?;
    for (index, value) in values.iter().enumerate() {
        match value {
            Value::String(_) => builder.token(&format!("requirement.{index:03}"), value)?,
            Value::Object(object) => {
                if object.is_empty() {
                    return Err(GovernorError::InvalidAuthoring);
                }
                for (key, value) in object {
                    match key.as_str() {
                        "type" => builder.token(&format!("type.{index:03}"), value)?,
                        "ref" => builder.reference(&format!("ref.{index:03}"), value)?,
                        "min" => builder.integer(&format!("min.{index:03}"), value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish("agent_evidence_requirements")
}

fn build_contract_control(kind: &str, value: &Value) -> Result<PersistedControl, GovernorError> {
    validate_schema_container(value)?;
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "schema" if value.is_string() => builder.reference("schema", value)?,
            "schema" if value.is_object() || value.is_boolean() => {
                builder.digest("schema", structural_schema_digest(value)?)?
            }
            "schema" => return Err(GovernorError::InvalidAuthoring),
            "$id" => builder.reference("schemaId", value)?,
            "$schema" => builder.reference("schemaDialect", value)?,
            "bindings" => {
                let bindings = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
                builder.count("bindingCount", bindings.len())?;
                for (index, (key, value)) in bindings.iter().enumerate() {
                    builder.token(
                        &format!("bindingKey.{index:03}"),
                        &Value::String(key.clone()),
                    )?;
                    builder.binding(&format!("bindingValue.{index:03}"), value)?;
                }
            }
            "publishAs" => builder.reference("publishAs", value)?,
            key if is_schema_annotation_key(key) => validate_schema_annotation(key, value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish(if kind == "input" {
        "input_contract"
    } else {
        "output_contract"
    })
}

fn build_context_control(
    node_id: &str,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "policyRef" => builder.reference("policyRef", value)?,
            "include" => build_context_includes(&mut builder, node_id, value, slots)?,
            "exclude" => builder.token_array("exclude", value)?,
            "conflicts" => builder.token("conflicts", value)?,
            "freshness" => {
                let object = nonempty_object(value)?;
                builder.flag("freshnessPresent", &Value::Bool(true))?;
                for (key, value) in object {
                    match key.as_str() {
                        "maxAgeDays" => builder.integer("freshnessMaxAgeDays", value)?,
                        "requireRevalidationFor" => builder.token_array("revalidate", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            "budget" => {
                let object = nonempty_object(value)?;
                builder.flag("budgetPresent", &Value::Bool(true))?;
                for (key, value) in object {
                    match key.as_str() {
                        "initialTokens" => builder.integer("initialUnits", value)?,
                        "maxTokens" => builder.integer("maxUnits", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            "expansion" => {
                let object = nonempty_object(value)?;
                builder.flag("expansionPresent", &Value::Bool(true))?;
                for (key, value) in object {
                    match key.as_str() {
                        "allowed" => builder.flag("expansionAllowed", value)?,
                        "requiresReason" => builder.flag("expansionRequiresReason", value)?,
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish("node_context")
}

fn build_context_includes(
    builder: &mut ControlBuilder,
    node_id: &str,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<(), GovernorError> {
    let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if values.len() > 32 {
        return Err(GovernorError::LimitExceeded);
    }
    builder.count("includeCount", values.len())?;
    for (index, value) in values.iter().enumerate() {
        let object = nonempty_object(value)?;
        for (key, value) in object {
            match key.as_str() {
                "type" => builder.token(&format!("includeType.{index:03}"), value)?,
                "ref" => {
                    let include_type = object
                        .get("type")
                        .and_then(Value::as_str)
                        .ok_or(GovernorError::InvalidAuthoring)?;
                    let reference = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
                    validate_context_include_reference(include_type, reference)
                        .map_err(|_| GovernorError::InvalidAuthoring)?;
                    builder.reference(&format!("includeRef.{index:03}"), value)?;
                }
                "node" => builder.token(&format!("includeNode.{index:03}"), value)?,
                "paths" => {
                    if object.get("type").and_then(Value::as_str) != Some("source_scope") {
                        return Err(GovernorError::InvalidAuthoring);
                    }
                    let paths = registered_path_array(value)?;
                    builder.count(&format!("scope.{index:03}Count"), paths.len())?;
                    for path_index in 0..paths.len() {
                        let slot = find_slot(
                            slots,
                            ContentOwnerKind::Node,
                            node_id,
                            ContentFieldKind::ContextPath,
                            nested_path_ordinal(index, path_index)?,
                        )?;
                        builder.slot(&format!("scopeSlot.{index:03}.{path_index:03}"), slot)?;
                    }
                }
                _ => return Err(GovernorError::InvalidAuthoring),
            }
        }
    }
    Ok(())
}

fn build_permissions_control(
    node_id: &str,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<PersistedControl, GovernorError> {
    let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if values.len() > 32 {
        return Err(GovernorError::LimitExceeded);
    }
    let mut builder = ControlBuilder::default();
    builder.count("permissionCount", values.len())?;
    for (index, value) in values.iter().enumerate() {
        match value {
            Value::String(_) => builder.token(&format!("capability.{index:03}"), value)?,
            Value::Object(object) => {
                if object.is_empty() {
                    return Err(GovernorError::InvalidAuthoring);
                }
                for (key, value) in object {
                    match key.as_str() {
                        "capability" => builder.token(&format!("capability.{index:03}"), value)?,
                        "duration" => builder.token(&format!("duration.{index:03}"), value)?,
                        "scope" => {
                            build_permission_scope(&mut builder, node_id, index, value, slots)?
                        }
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish("node_permissions")
}

fn build_permission_scope(
    builder: &mut ControlBuilder,
    node_id: &str,
    permission_index: usize,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<(), GovernorError> {
    let object = nonempty_object(value)?;
    for (key, value) in object {
        match key.as_str() {
            "allowlist" => {
                let values = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
                if values.len() > 16 {
                    return Err(GovernorError::LimitExceeded);
                }
                builder.token_array(&format!("allow.{permission_index:03}"), value)?;
            }
            "paths" => {
                let paths = registered_path_array(value)?;
                builder.count(&format!("scope.{permission_index:03}Count"), paths.len())?;
                for path_index in 0..paths.len() {
                    let slot = find_slot(
                        slots,
                        ContentOwnerKind::Node,
                        node_id,
                        ContentFieldKind::PermissionPath,
                        nested_path_ordinal(permission_index, path_index)?,
                    )?;
                    builder.slot(
                        &format!("scopeSlot.{permission_index:03}.{path_index:03}"),
                        slot,
                    )?;
                }
            }
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    Ok(())
}

fn build_isolation_control(
    node_id: &str,
    value: &Value,
    slots: &[ContentSlot],
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "minimum" => {
                let tier = value.as_str().ok_or(GovernorError::InvalidAuthoring)?;
                if !is_valid_isolation_tier(tier) {
                    return Err(GovernorError::InvalidAuthoring);
                }
                builder.token("minimum", value)?;
            }
            "filesystem" | "network" | "secrets" => {
                let nested = nonempty_object(value)?;
                builder.flag(
                    match key.as_str() {
                        "filesystem" => "filesystemPresent",
                        "network" => "networkPresent",
                        "secrets" => "brokerPresent",
                        _ => return Err(GovernorError::InvalidAuthoring),
                    },
                    &Value::Bool(true),
                )?;
                for (child, value) in nested {
                    match (key.as_str(), child.as_str()) {
                        (_, "mode") => builder.token(
                            match key.as_str() {
                                "filesystem" => "filesystemMode",
                                "network" => "networkMode",
                                "secrets" => "brokerMode",
                                _ => return Err(GovernorError::InvalidAuthoring),
                            },
                            value,
                        )?,
                        ("network", "allowlist") => builder.token_array("networkAllow", value)?,
                        ("filesystem", "writablePaths") => {
                            let paths = registered_path_array(value)?;
                            builder.count("writeScopeCount", paths.len())?;
                            for path_index in 0..paths.len() {
                                let slot = find_slot(
                                    slots,
                                    ContentOwnerKind::Node,
                                    node_id,
                                    ContentFieldKind::IsolationPath,
                                    u32::try_from(path_index)
                                        .map_err(|_| GovernorError::LimitExceeded)?,
                                )?;
                                builder.slot(&format!("writeScopeSlot.{path_index:03}"), slot)?;
                            }
                        }
                        _ => return Err(GovernorError::InvalidAuthoring),
                    }
                }
            }
            "resources" => add_resource_integers(&mut builder, value, "isolation")?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish("node_isolation")
}

fn build_retry_control(
    control_type: &str,
    value: &Value,
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "maxAttempts" | "maxBackoffSeconds" => builder.integer(key, value)?,
            "backoff" => builder.token("backoff", value)?,
            "retryOn" => builder.token_array("retryOn", value)?,
            "doNotRetryOn" => builder.token_array("noRetry", value)?,
            "beforeRetry" => builder.token_array("beforeRetry", value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish(control_type)
}

fn build_resources_control(
    control_type: &str,
    value: &Value,
) -> Result<PersistedControl, GovernorError> {
    let mut builder = ControlBuilder::default();
    add_resource_integers(&mut builder, value, "")?;
    builder.finish(control_type)
}

fn add_resource_integers(
    builder: &mut ControlBuilder,
    value: &Value,
    prefix: &str,
) -> Result<(), GovernorError> {
    let object = nonempty_object(value)?;
    for (key, value) in object {
        let output_key = match (prefix, key.as_str()) {
            ("", "cpu") => "cpu",
            ("", "memoryMb") => "memoryMb",
            ("", "diskMb") => "diskMb",
            ("isolation", "cpu") => "isolationCpu",
            ("isolation", "memoryMb") => "isolationMemoryMb",
            ("isolation", "diskMb") => "isolationDiskMb",
            _ => return Err(GovernorError::InvalidAuthoring),
        };
        builder.integer(output_key, value)?;
    }
    Ok(())
}

fn build_memory_control(
    control_type: &str,
    value: &Value,
) -> Result<PersistedControl, GovernorError> {
    let object = nonempty_object(value)?;
    let mut builder = ControlBuilder::default();
    for (key, value) in object {
        match key.as_str() {
            "writeCandidates" => builder.flag("writeCandidates", value)?,
            "defaultTtlDays" => builder.integer("defaultTtlDays", value)?,
            "policyRef" => builder.reference("policyRef", value)?,
            "includeScopes" => builder.token_array("includeScope", value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    builder.finish(control_type)
}

fn build_edge(edge: &GraphEdge, slots: &[ContentSlot]) -> Result<PersistedEdge, GovernorError> {
    let id = OpaqueId::parse(&edge.id).map_err(|_| GovernorError::InvalidAuthoring)?;
    let bindings = edge
        .bindings
        .iter()
        .map(|(key, value)| {
            Ok((
                SafeKey::parse(key).map_err(|_| GovernorError::InvalidAuthoring)?,
                encode_safe_binding(value)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, GovernorError>>()?;
    let condition_slot = edge
        .condition
        .as_ref()
        .map(|_| {
            find_slot(
                slots,
                ContentOwnerKind::Edge,
                &edge.id,
                ContentFieldKind::PolicyText,
                0,
            )
        })
        .transpose()?;
    let on_false_slot = edge
        .on_false
        .as_ref()
        .map(|_| {
            find_slot(
                slots,
                ContentOwnerKind::Edge,
                &edge.id,
                ContentFieldKind::PolicyText,
                1,
            )
        })
        .transpose()?;
    let condition = if edge.payload_schema.is_some()
        || edge.on_unknown.is_some()
        || condition_slot.is_some()
        || on_false_slot.is_some()
    {
        let mut identifiers = BTreeMap::new();
        if let Some(schema) = &edge.payload_schema {
            identifiers.insert(safe_key("schema")?, encode_safe_reference(schema)?);
        }
        if let Some(behavior) = &edge.on_unknown {
            let value =
                serde_json::to_value(behavior).map_err(|_| GovernorError::InvalidProjection)?;
            identifiers.insert(
                safe_key("unknownBehavior")?,
                SafeValue::parse(value.as_str().ok_or(GovernorError::InvalidProjection)?)
                    .map_err(|_| GovernorError::InvalidProjection)?,
            );
        }
        for (index, slot) in [(0, condition_slot), (1, on_false_slot)] {
            let Some(slot) = slot else {
                continue;
            };
            identifiers.insert(
                safe_key(&format!("slot{index}"))?,
                SafeValue::parse(slot.slot_id().as_str())
                    .map_err(|_| GovernorError::InvalidProjection)?,
            );
        }
        Some(control(
            "edge_condition",
            identifiers,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )?)
    } else {
        None
    };
    PersistedEdge::new(
        id,
        OpaqueId::parse(&edge.from).map_err(|_| GovernorError::InvalidAuthoring)?,
        OpaqueId::parse(&edge.to).map_err(|_| GovernorError::InvalidAuthoring)?,
        edge.edge_type.clone(),
        edge.priority,
        bindings,
        condition,
    )
    .map_err(|_| GovernorError::InvalidProjection)
}

fn build_budgets(budgets: &GraphBudgets) -> Result<PersistedBudgets, GovernorError> {
    PersistedBudgets::new(
        budgets.max_nodes,
        budgets.max_depth,
        budgets.max_mutations,
        budgets.max_retries_per_node,
        budgets.max_wall_clock_seconds,
        budgets.max_api_cost_usd,
        budgets.max_parallel_model_calls,
    )
    .map_err(|_| GovernorError::InvalidProjection)
}

fn find_slot<'a>(
    slots: &'a [ContentSlot],
    owner_kind: ContentOwnerKind,
    owner_id: &str,
    field_kind: ContentFieldKind,
    ordinal: u32,
) -> Result<&'a ContentSlot, GovernorError> {
    let mut matching = slots.iter().filter(|slot| {
        slot.owner_kind() == owner_kind
            && slot.owner_id().as_str() == owner_id
            && slot.field_kind() == field_kind
            && slot.ordinal() == ordinal
    });
    let slot = matching.next().ok_or(GovernorError::InvalidProjection)?;
    if matching.next().is_some() {
        return Err(GovernorError::InvalidProjection);
    }
    Ok(slot)
}

fn control(
    control_type: &str,
    identifiers: BTreeMap<SafeKey, SafeValue>,
    digests: BTreeMap<SafeKey, RawSha256>,
    integers: BTreeMap<SafeKey, i64>,
    flags: BTreeMap<SafeKey, bool>,
) -> Result<PersistedControl, GovernorError> {
    let control = PersistedControl::new(
        SafeValue::parse(control_type).map_err(|_| GovernorError::InvalidProjection)?,
        identifiers,
        digests,
        integers,
        flags,
    )
    .map_err(|_| GovernorError::InvalidProjection)?;
    validate_persisted_control_references(&control)
        .map_err(|_| GovernorError::InvalidProjection)?;
    Ok(control)
}

fn safe_key(value: &str) -> Result<SafeKey, GovernorError> {
    SafeKey::parse(value).map_err(|_| GovernorError::InvalidProjection)
}

fn validate_schema_container(value: &Value) -> Result<(), GovernorError> {
    let object = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
    let mut semantic_members = 0usize;
    for (key, value) in object {
        match key.as_str() {
            "schema" if value.is_object() || value.is_boolean() => {
                structural_schema_digest(value)?;
                semantic_members += 1;
            }
            "schema" | "publishAs" | "$id" | "$schema" => {
                validate_string(value)?;
                semantic_members += 1;
            }
            "bindings" => {
                let bindings = value.as_object().ok_or(GovernorError::InvalidAuthoring)?;
                if bindings.len() > 128 || bindings.values().any(|value| value.as_str().is_none()) {
                    return Err(GovernorError::InvalidAuthoring);
                }
                semantic_members = semantic_members
                    .checked_add(bindings.len())
                    .ok_or(GovernorError::LimitExceeded)?;
            }
            key if is_schema_annotation_key(key) => validate_schema_annotation(key, value)?,
            _ => return Err(GovernorError::InvalidAuthoring),
        }
    }
    if semantic_members == 0 {
        Err(GovernorError::InvalidAuthoring)
    } else {
        Ok(())
    }
}

fn structural_schema_digest(value: &Value) -> Result<RawSha256, GovernorError> {
    graphhelm_schema::compile_inline_schema(value).map_err(|error| match error {
        graphhelm_schema::InlineSchemaError::LimitExceeded => GovernorError::LimitExceeded,
        graphhelm_schema::InlineSchemaError::Invalid => GovernorError::InvalidAuthoring,
    })?;

    fn strip_schema(value: &mut Value) -> Result<(), GovernorError> {
        match value {
            Value::Bool(_) => Ok(()),
            Value::Object(_) => strip_schema_node(value),
            Value::Null | Value::Array(_) | Value::Number(_) | Value::String(_) => {
                Err(GovernorError::InvalidAuthoring)
            }
        }
    }

    fn strip_schema_map(value: &mut Value) -> Result<(), GovernorError> {
        let Value::Object(members) = value else {
            return Err(GovernorError::InvalidAuthoring);
        };
        for schema in members.values_mut() {
            strip_schema(schema)?;
        }
        Ok(())
    }

    fn strip_schema_array(value: &mut Value) -> Result<(), GovernorError> {
        let Value::Array(items) = value else {
            return Err(GovernorError::InvalidAuthoring);
        };
        for schema in items {
            strip_schema(schema)?;
        }
        Ok(())
    }

    fn strip_schema_node(value: &mut Value) -> Result<(), GovernorError> {
        let Value::Object(object) = value else {
            return Err(GovernorError::InvalidAuthoring);
        };
        object.retain(|key, _| !is_schema_annotation_key(key));
        for (keyword, child) in object {
            match keyword.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions"
                | "dependentSchemas" => strip_schema_map(child)?,
                "additionalProperties"
                | "unevaluatedProperties"
                | "unevaluatedItems"
                | "propertyNames"
                | "contains"
                | "contentSchema"
                | "not"
                | "if"
                | "then"
                | "else" => strip_schema(child)?,
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    strip_schema_array(child)?;
                }
                "items" => strip_schema(child)?,
                // Unknown/custom keyword values and literal-bearing keywords
                // (`const`, `enum`, and the removed `examples`/`default`) are
                // data, not implicit schema positions.
                _ => {}
            }
        }
        Ok(())
    }

    let mut structural = value.clone();
    strip_schema(&mut structural)?;
    if structural
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        return Err(GovernorError::InvalidAuthoring);
    }
    validate_graph_durable_content(&structural, &[]).map_err(|error| match error {
        DurableContentError::Unsafe => GovernorError::InvalidAuthoring,
        DurableContentError::LimitExceeded => GovernorError::LimitExceeded,
    })?;
    let bytes =
        canonical_content_bytes(&structural).map_err(|_| GovernorError::InvalidAuthoring)?;
    raw_content_sha256(&bytes).map_err(|_| GovernorError::InvalidProjection)
}

fn validate_schema_annotation(keyword: &str, value: &Value) -> Result<(), GovernorError> {
    let valid = match keyword {
        "title" | "description" | "$comment" => value.is_string(),
        "examples" => value.is_array(),
        "deprecated" | "readOnly" | "writeOnly" => value.is_boolean(),
        "default" => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(GovernorError::InvalidAuthoring)
    }
}

fn validate_string_array(value: &Value) -> Result<(), GovernorError> {
    let items = value.as_array().ok_or(GovernorError::InvalidAuthoring)?;
    if items.len() > 1024 || items.iter().any(|value| value.as_str().is_none()) {
        return Err(GovernorError::InvalidAuthoring);
    }
    Ok(())
}

fn validate_string(value: &Value) -> Result<(), GovernorError> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(|_| ())
        .ok_or(GovernorError::InvalidAuthoring)
}

fn validate_positive_integer(value: &Value) -> Result<(), GovernorError> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .map(|_| ())
        .ok_or(GovernorError::InvalidAuthoring)
}

fn validate_bool(value: &Value) -> Result<(), GovernorError> {
    value
        .as_bool()
        .map(|_| ())
        .ok_or(GovernorError::InvalidAuthoring)
}

fn is_schema_annotation_key(key: &str) -> bool {
    matches!(
        key,
        "title"
            | "description"
            | "examples"
            | "$comment"
            | "default"
            | "deprecated"
            | "readOnly"
            | "writeOnly"
    )
}

fn validate_durable_content(
    scope: &RepositoryScope,
    record: &GraphVersionRecord,
    graph: &Value,
) -> Result<(), GovernorError> {
    let mut additional = vec![
        record.created_by.id.as_str(),
        scope.workspace_id().as_str(),
        scope.project_id().as_str(),
    ];
    if let Some(execution_id) = scope.execution_id() {
        additional.push(execution_id.as_str());
    }
    validate_graph_durable_content(graph, &additional).map_err(|error| match error {
        DurableContentError::Unsafe => GovernorError::InvalidAuthoring,
        DurableContentError::LimitExceeded => GovernorError::LimitExceeded,
    })
}

fn encoded_len_bounded(value: &Value, limit: usize) -> Result<usize, GovernorError> {
    struct CountingWriter {
        count: usize,
        limit: usize,
    }
    impl std::io::Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.count = self
                .count
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("limit"))?;
            if self.count > self.limit {
                return Err(std::io::Error::other("limit"));
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = CountingWriter { count: 0, limit };
    serde_json::to_writer(&mut writer, value).map_err(|_| GovernorError::LimitExceeded)?;
    Ok(writer.count)
}

fn map_evidence_error(error: EvidenceError) -> GovernorError {
    match error {
        EvidenceError::TooLarge | EvidenceError::BatchTooLarge => GovernorError::LimitExceeded,
        EvidenceError::Unavailable | EvidenceError::Invalid | EvidenceError::SealingFailed => {
            GovernorError::SealingFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_rejects_item_count_before_registering_an_extra_value() {
        let mut collector = ContentCollector::default();
        for ordinal in 0..MAX_CONTENT_ITEMS {
            collector
                .register(
                    ContentOwnerKind::Policy,
                    "graph-test",
                    ContentFieldKind::PolicyText,
                    u32::try_from(ordinal).unwrap(),
                    &Value::Null,
                )
                .unwrap();
        }

        let error = collector
            .register(
                ContentOwnerKind::Policy,
                "graph-test",
                ContentFieldKind::PolicyText,
                u32::try_from(MAX_CONTENT_ITEMS).unwrap(),
                &Value::Null,
            )
            .unwrap_err();

        assert_eq!(error, GovernorError::LimitExceeded);
        assert_eq!(collector.items.len(), MAX_CONTENT_ITEMS);
    }

    #[test]
    fn collector_rejects_aggregate_before_allocating_the_overflowing_item() {
        let mut collector = ContentCollector::default();
        let value = Value::String("x".repeat(13 * 1024 * 1024));
        for ordinal in 0..4 {
            collector
                .register(
                    ContentOwnerKind::Policy,
                    "graph-test",
                    ContentFieldKind::PolicyText,
                    ordinal,
                    &value,
                )
                .unwrap();
        }

        let error = collector
            .register(
                ContentOwnerKind::Policy,
                "graph-test",
                ContentFieldKind::PolicyText,
                4,
                &value,
            )
            .unwrap_err();

        assert_eq!(error, GovernorError::LimitExceeded);
        assert_eq!(collector.items.len(), 4);
    }
}
