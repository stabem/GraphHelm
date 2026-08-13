use chrono::Utc;
use graphhelm_events::{
    EventRepositoryError, PreparedAppend, compute_event_hash, prepared_append_digest,
    validate_envelope, validate_envelope_content, validate_prepared_append,
};
use graphhelm_protocols::{EventEnvelope, EventHash, OpaqueId, PersistedTimestamp};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};

use crate::{PostgresEventStore, rows::StoredEvidence, scope};

pub(crate) const GENESIS_HASH: &str =
    "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3";

pub(crate) async fn append(
    store: &PostgresEventStore,
    request: PreparedAppend,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    validate_prepared_append(&request)?;
    let digest = prepared_append_digest(&request)?;
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    store.set_scope(&mut transaction, request.scope()).await?;
    let scoped = scope::parts(request.scope());

    let keys = request
        .events()
        .iter()
        .map(|event| event.idempotency_key.as_str())
        .collect::<Vec<_>>();
    let existing = sqlx::query(
        "SELECT idempotency_key,request_digest,first_sequence,event_count FROM public.graphhelm_idempotency \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
         AND idempotency_key = ANY($5)",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .bind(&keys)
    .fetch_all(&mut *transaction)
    .await
    .map_err(crate::error::storage)?;
    if !existing.is_empty() {
        let exact = existing.len() == keys.len()
            && existing.iter().all(|row| {
                row.try_get::<String, _>("request_digest")
                    .is_ok_and(|value| value == digest)
            });
        if !exact {
            return Err(EventRepositoryError::IdempotencyConflict);
        }
    }

    let genesis_proof = crate::integrity::authenticate_stream_head(
        store,
        request.scope(),
        request.stream_id().as_str(),
        1,
        GENESIS_HASH,
    )
    .await?;
    sqlx::query(
        "INSERT INTO public.graphhelm_streams \
         (workspace_id,project_id,execution_id,stream_id,next_sequence,last_event_hash,head_key_id,head_key_version,head_provider_epoch,head_algorithm,head_tag,head_canonical_sha256) \
         VALUES ($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .bind(GENESIS_HASH)
    .bind(&genesis_proof.key_id)
    .bind(&genesis_proof.key_version)
    .bind(i64::try_from(genesis_proof.provider_epoch).map_err(|_| EventRepositoryError::LimitExceeded)?)
    .bind(&genesis_proof.algorithm)
    .bind(&genesis_proof.tag)
    .bind(&genesis_proof.canonical_sha256)
    .execute(&mut *transaction)
    .await
    .map_err(crate::error::storage)?;
    let head = sqlx::query(
        "SELECT next_sequence,last_event_hash FROM public.graphhelm_streams WHERE workspace_id=$1 \
         AND project_id=$2 AND execution_id=$3 AND stream_id=$4 FOR UPDATE",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(crate::error::storage)?;
    store.after_head_read().await;
    let next_sequence: i64 = head
        .try_get("next_sequence")
        .map_err(crate::error::decode)?;
    let next_sequence =
        u64::try_from(next_sequence).map_err(|_| EventRepositoryError::Integrity)?;
    let locked_hash: String = head
        .try_get("last_event_hash")
        .map_err(crate::error::decode)?;
    crate::integrity::verify_locked_stream(
        store,
        &mut transaction,
        request.scope(),
        request.stream_id().as_str(),
        next_sequence,
        &locked_hash,
    )
    .await?;
    if let Some(envelopes) = resolve_retry_after_lock(
        store,
        &mut transaction,
        &request,
        &digest,
        &keys,
        next_sequence,
        &locked_hash,
    )
    .await?
    {
        transaction.commit().await.map_err(crate::error::storage)?;
        return Ok(envelopes);
    }
    if next_sequence != request.expected_next_sequence() {
        return Err(EventRepositoryError::SequenceConflict);
    }

    validate_relations(store, &mut transaction, &request, next_sequence).await?;
    for evidence in request.evidence() {
        let stored = serde_json::to_value(StoredEvidence::from_sealed(evidence))
            .map_err(|_| EventRepositoryError::Invalid)?;
        sqlx::query(
            "INSERT INTO public.graphhelm_evidence \
             (workspace_id,project_id,execution_id,evidence_id,record) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(scoped.workspace)
        .bind(scoped.project)
        .bind(scoped.execution)
        .bind(evidence.reference().evidence_id().as_str())
        .bind(stored)
        .execute(&mut *transaction)
        .await
        .map_err(map_constraint)?;
    }
    if store.failpoint_after_evidence() {
        return Err(EventRepositoryError::Storage);
    }

    let event_count =
        i32::try_from(request.events().len()).map_err(|_| EventRepositoryError::LimitExceeded)?;
    for event in request.events() {
        sqlx::query(
            "INSERT INTO public.graphhelm_idempotency \
             (workspace_id,project_id,execution_id,stream_id,idempotency_key,request_digest,first_sequence,event_count) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(scoped.workspace)
        .bind(scoped.project)
        .bind(scoped.execution)
        .bind(request.stream_id().as_str())
        .bind(event.idempotency_key.as_str())
        .bind(&digest)
        .bind(i64::try_from(next_sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(event_count)
        .execute(&mut *transaction)
        .await
        .map_err(map_constraint)?;
    }
    for artifact in request.artifacts() {
        let reference = serde_json::to_value(artifact.reference())
            .map_err(|_| EventRepositoryError::Invalid)?;
        sqlx::query(
            "INSERT INTO public.graphhelm_artifacts \
             (workspace_id,project_id,execution_id,artifact_id,producer_stream_id,producer_idempotency_key,reference) \
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(scoped.workspace)
        .bind(scoped.project)
        .bind(scoped.execution)
        .bind(artifact.reference().artifact_id().as_str())
        .bind(request.stream_id().as_str())
        .bind(artifact.producer_idempotency_key().as_str())
        .bind(reference)
        .execute(&mut *transaction)
        .await
        .map_err(map_constraint)?;
    }

    let mut previous_hash = locked_hash;
    let mut envelopes = Vec::with_capacity(request.events().len());
    for (offset, event) in request.events().iter().cloned().enumerate() {
        let sequence = request
            .expected_next_sequence()
            .checked_add(u64::try_from(offset).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        let occurred_at = PersistedTimestamp::from_datetime(Utc::now())
            .map_err(|_| EventRepositoryError::Storage)?;
        let event_id = OpaqueId::parse(format!("evt-{}", uuid::Uuid::new_v4().simple()))
            .map_err(|_| EventRepositoryError::Storage)?;
        let previous =
            EventHash::parse(previous_hash.clone()).map_err(|_| EventRepositoryError::Integrity)?;
        let placeholder =
            EventHash::parse(GENESIS_HASH).map_err(|_| EventRepositoryError::Integrity)?;
        let mut envelope = EventEnvelope::new(
            event_id,
            request.scope().clone(),
            request.stream_id().clone(),
            sequence,
            occurred_at,
            event,
            previous,
            placeholder,
        );
        validate_envelope_content(&envelope)?;
        let hash = compute_event_hash(&envelope, &previous_hash)?;
        envelope.event_hash =
            EventHash::parse(hash.clone()).map_err(|_| EventRepositoryError::Integrity)?;
        validate_envelope(&envelope)?;
        let envelope_json =
            serde_json::to_value(&envelope).map_err(|_| EventRepositoryError::Invalid)?;
        sqlx::query(
            "INSERT INTO public.graphhelm_events \
             (workspace_id,project_id,execution_id,stream_id,sequence,event_id,idempotency_key,previous_hash,event_hash,request_digest,envelope) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(scoped.workspace)
        .bind(scoped.project)
        .bind(scoped.execution)
        .bind(request.stream_id().as_str())
        .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(envelope.event_id.as_str())
        .bind(envelope.idempotency_key.as_str())
        .bind(envelope.previous_hash.as_str())
        .bind(envelope.event_hash.as_str())
        .bind(&digest)
        .bind(envelope_json)
        .execute(&mut *transaction)
        .await
        .map_err(map_constraint)?;
        insert_references(&mut transaction, &envelope).await?;
        previous_hash = hash;
        envelopes.push(envelope);
    }
    let final_next = request
        .expected_next_sequence()
        .checked_add(
            u64::try_from(request.events().len())
                .map_err(|_| EventRepositoryError::LimitExceeded)?,
        )
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let head_proof = crate::integrity::authenticate_stream_head(
        store,
        request.scope(),
        request.stream_id().as_str(),
        final_next,
        &previous_hash,
    )
    .await?;
    sqlx::query(
        "UPDATE public.graphhelm_streams SET next_sequence=$5,last_event_hash=$6,head_key_id=$7,head_key_version=$8,head_provider_epoch=$9,head_algorithm=$10,head_tag=$11,head_canonical_sha256=$12 WHERE workspace_id=$1 \
         AND project_id=$2 AND execution_id=$3 AND stream_id=$4",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .bind(i64::try_from(final_next).map_err(|_| EventRepositoryError::LimitExceeded)?)
    .bind(&previous_hash)
    .bind(&head_proof.key_id)
    .bind(&head_proof.key_version)
    .bind(i64::try_from(head_proof.provider_epoch).map_err(|_| EventRepositoryError::LimitExceeded)?)
    .bind(&head_proof.algorithm)
    .bind(&head_proof.tag)
    .bind(&head_proof.canonical_sha256)
    .execute(&mut *transaction)
    .await
    .map_err(crate::error::storage)?;
    let tail = envelopes.last().ok_or(EventRepositoryError::Integrity)?;
    crate::integrity::checkpoint_head_if_due(
        store,
        &mut transaction,
        request.scope(),
        request.stream_id().as_str(),
        tail.sequence,
        &tail.event_hash,
    )
    .await?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(envelopes)
}

async fn resolve_retry_after_lock(
    store: &PostgresEventStore,
    transaction: &mut Transaction<'_, Postgres>,
    request: &PreparedAppend,
    digest: &str,
    keys: &[&str],
    head_next_sequence: u64,
    head_hash: &str,
) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
    let scoped = scope::parts(request.scope());
    let existing = sqlx::query(
        "SELECT idempotency_key,request_digest,first_sequence,event_count FROM public.graphhelm_idempotency \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
         AND idempotency_key = ANY($5)",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .bind(keys)
    .fetch_all(&mut **transaction)
    .await
    .map_err(crate::error::storage)?;
    if existing.is_empty() {
        return Ok(None);
    }
    let first_sequence = i64::try_from(request.expected_next_sequence())
        .map_err(|_| EventRepositoryError::LimitExceeded)?;
    let event_count =
        i32::try_from(request.events().len()).map_err(|_| EventRepositoryError::LimitExceeded)?;
    let mut persisted_keys = existing
        .iter()
        .filter_map(|row| row.try_get::<String, _>("idempotency_key").ok())
        .collect::<Vec<_>>();
    persisted_keys.sort();
    let mut expected_keys = keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>();
    expected_keys.sort();
    if existing.len() != keys.len()
        || persisted_keys != expected_keys
        || !existing.iter().all(|row| {
            row.try_get::<i64, _>("first_sequence")
                .is_ok_and(|value| value == first_sequence)
                && row
                    .try_get::<i32, _>("event_count")
                    .is_ok_and(|value| value == event_count)
        })
    {
        return Err(EventRepositoryError::Integrity);
    }
    if !existing.iter().all(|row| {
        row.try_get::<String, _>("request_digest")
            .is_ok_and(|value| value == digest)
    }) {
        return Err(EventRepositoryError::IdempotencyConflict);
    }
    let rows = sqlx::query(
        "SELECT sequence,idempotency_key,request_digest,event_hash FROM public.graphhelm_events \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 AND request_digest=$5 \
         ORDER BY sequence LIMIT $6",
    )
    .bind(scoped.workspace)
    .bind(scoped.project)
    .bind(scoped.execution)
    .bind(request.stream_id().as_str())
    .bind(digest)
    .bind(
        i64::try_from(request.events().len() + 1)
            .map_err(|_| EventRepositoryError::LimitExceeded)?,
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(crate::error::storage)?;
    if rows.len() != request.events().len() {
        return Err(EventRepositoryError::Integrity);
    }
    let last_sequence = request
        .expected_next_sequence()
        .checked_add(
            u64::try_from(request.events().len() - 1)
                .map_err(|_| EventRepositoryError::LimitExceeded)?,
        )
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let envelopes = crate::integrity::fetch_envelopes_chunked(
        transaction,
        request.stream_id().as_str(),
        request.expected_next_sequence(),
        last_sequence,
        u64::try_from(request.events().len()).map_err(|_| EventRepositoryError::LimitExceeded)?,
    )
    .await?;
    if envelopes.len() != request.events().len() {
        return Err(EventRepositoryError::Integrity);
    }
    for (offset, ((row, envelope), requested)) in rows
        .iter()
        .zip(&envelopes)
        .zip(request.events())
        .enumerate()
    {
        let expected_sequence = request
            .expected_next_sequence()
            .checked_add(u64::try_from(offset).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        let row_matches = row
            .try_get::<i64, _>("sequence")
            .ok()
            .and_then(|value| u64::try_from(value).ok())
            .is_some_and(|value| value == expected_sequence)
            && row
                .try_get::<String, _>("idempotency_key")
                .is_ok_and(|value| value == requested.idempotency_key.as_str())
            && row
                .try_get::<String, _>("request_digest")
                .is_ok_and(|value| value == digest)
            && row
                .try_get::<String, _>("event_hash")
                .is_ok_and(|value| value == envelope.event_hash.as_str());
        let request_matches = envelope.sequence == expected_sequence
            && envelope.scope == *request.scope()
            && envelope.stream_id == *request.stream_id()
            && envelope.idempotency_key == requested.idempotency_key
            && envelope.actor == requested.actor
            && envelope.sensitivity == requested.sensitivity
            && envelope.kind == requested.kind
            && envelope.evidence_refs == requested.evidence_refs
            && envelope.artifact_refs == requested.artifact_refs;
        if !row_matches
            || !request_matches
            || compute_event_hash(envelope, envelope.previous_hash.as_str())?
                != envelope.event_hash.as_str()
        {
            return Err(EventRepositoryError::Integrity);
        }
        if offset > 0 && envelope.previous_hash != envelopes[offset - 1].event_hash {
            return Err(EventRepositoryError::Integrity);
        }
    }
    let head = graphhelm_events::StreamHead {
        next_sequence: head_next_sequence,
        last_event_hash: EventHash::parse(head_hash.to_owned())
            .map_err(|_| EventRepositoryError::Integrity)?,
    };
    crate::integrity::verify_window_to_anchor(
        store,
        transaction,
        request.scope(),
        request.stream_id().as_str(),
        request.expected_next_sequence(),
        last_sequence,
        Some(&head),
    )
    .await?;
    Ok(Some(envelopes))
}

async fn validate_relations(
    store: &PostgresEventStore,
    transaction: &mut Transaction<'_, Postgres>,
    request: &PreparedAppend,
    head_next_sequence: u64,
) -> Result<(), EventRepositoryError> {
    let prepared_evidence = request
        .evidence()
        .iter()
        .map(|item| (item.reference().evidence_id().as_str(), item.reference()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut committed_evidence = std::collections::BTreeMap::new();
    for reference in request
        .events()
        .iter()
        .flat_map(|event| &event.evidence_refs)
    {
        if prepared_evidence.contains_key(reference.evidence_id().as_str())
            || committed_evidence.contains_key(reference.evidence_id().as_str())
        {
            continue;
        }
        if let Some(row) =
            sqlx::query("SELECT record,state FROM public.graphhelm_evidence WHERE evidence_id=$1")
                .bind(reference.evidence_id().as_str())
                .fetch_optional(&mut **transaction)
                .await
                .map_err(crate::error::storage)?
        {
            // `record` survives the transition into `erasure_pending`, so validating it alone
            // would accept a reference to Evidence whose ciphertext is already scheduled for
            // deletion. That reference can never be resolved afterwards: reads return
            // `Unavailable(ErasurePending)` for the life of the stream. Only Evidence that is
            // still available may be referenced by a new event.
            if row
                .try_get::<String, _>("state")
                .map_err(crate::error::decode)?
                != "available"
            {
                return Err(EventRepositoryError::Invalid);
            }
            let stored: StoredEvidence =
                serde_json::from_value(row.try_get("record").map_err(crate::error::decode)?)
                    .map_err(|_| EventRepositoryError::Integrity)?;
            let sealed = stored.into_sealed()?;
            graphhelm_events::validate_sealed_evidence(&sealed)?;
            committed_evidence.insert(
                reference.evidence_id().to_string(),
                sealed.reference().clone(),
            );
        }
    }
    graphhelm_events::validate_evidence_references(
        request.events(),
        &prepared_evidence,
        |reference| {
            Ok(committed_evidence
                .get(reference.evidence_id().as_str())
                .is_some_and(|stored| stored == reference))
        },
    )?;

    let mut committed_artifacts = std::collections::BTreeMap::new();
    for artifact_id in request
        .artifacts()
        .iter()
        .map(|item| item.reference().artifact_id())
        .chain(
            request
                .events()
                .iter()
                .flat_map(|event| &event.artifact_refs)
                .map(|reference| reference.artifact_id()),
        )
    {
        if committed_artifacts.contains_key(artifact_id.as_str()) {
            continue;
        }
        if let Some(row) = sqlx::query(
            "SELECT reference,producer_stream_id,producer_idempotency_key \
             FROM public.graphhelm_artifacts WHERE artifact_id=$1",
        )
        .bind(artifact_id.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(crate::error::storage)?
        {
            committed_artifacts.insert(
                artifact_id.to_string(),
                graphhelm_events::CommittedArtifact {
                    reference: serde_json::from_value(
                        row.try_get("reference").map_err(crate::error::decode)?,
                    )
                    .map_err(|_| EventRepositoryError::Integrity)?,
                    producer_stream_id: row
                        .try_get("producer_stream_id")
                        .map_err(crate::error::decode)?,
                    producer_idempotency_key: row
                        .try_get("producer_idempotency_key")
                        .map_err(crate::error::decode)?,
                },
            );
        }
    }
    graphhelm_events::validate_artifact_relations(request, &committed_artifacts)?;

    let through = head_next_sequence
        .checked_sub(1)
        .ok_or(EventRepositoryError::Integrity)?;
    let active = crate::integrity::derive_active_graph(
        store,
        transaction,
        request.scope(),
        request.stream_id().as_str(),
        through,
    )
    .await?;
    graphhelm_events::validate_graph_lineage(request, active.as_ref())
}

async fn insert_references(
    transaction: &mut Transaction<'_, Postgres>,
    envelope: &EventEnvelope,
) -> Result<(), EventRepositoryError> {
    let scoped = scope::parts(&envelope.scope);
    for (ordinal, reference) in envelope.evidence_refs.iter().enumerate() {
        sqlx::query("INSERT INTO public.graphhelm_evidence_refs (workspace_id,project_id,execution_id,stream_id,sequence,ordinal,evidence_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(envelope.stream_id.as_str())
            .bind(i64::try_from(envelope.sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .bind(i32::try_from(ordinal).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .bind(reference.evidence_id().as_str()).execute(&mut **transaction).await.map_err(map_constraint)?;
    }
    for (ordinal, reference) in envelope.artifact_refs.iter().enumerate() {
        sqlx::query("INSERT INTO public.graphhelm_artifact_refs (workspace_id,project_id,execution_id,stream_id,sequence,ordinal,artifact_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(envelope.stream_id.as_str())
            .bind(i64::try_from(envelope.sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .bind(i32::try_from(ordinal).map_err(|_| EventRepositoryError::LimitExceeded)?)
            .bind(reference.artifact_id().as_str()).execute(&mut **transaction).await.map_err(map_constraint)?;
    }
    Ok(())
}

pub(crate) fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, EventRepositoryError> {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
            Value::Object(values) => {
                let mut entries = values.into_iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(k, v)| (k, canonical(v)))
                        .collect(),
                )
            }
            scalar => scalar,
        }
    }
    let value = serde_json::to_value(value).map_err(|_| EventRepositoryError::Invalid)?;
    serde_json::to_vec(&canonical(value)).map_err(|_| EventRepositoryError::Invalid)
}

fn map_constraint(error: sqlx::Error) -> EventRepositoryError {
    if error
        .as_database_error()
        .and_then(|value| value.code())
        .is_some_and(|code| code == "23505")
    {
        EventRepositoryError::IdempotencyConflict
    } else {
        EventRepositoryError::Storage
    }
}
