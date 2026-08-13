use graphhelm_events::{
    AuthenticateRequest, AuthenticatedCheckpoint, EventPage, EventRepositoryError, IntegrityReport,
    ReadStart, ReadStreamRequest, StreamHead, VerifyAuthenticationRequest, VerifyRangeRequest,
};
use graphhelm_protocols::{EventEnvelope, EventHash, RepositoryScope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::{PostgresEventStore, journal, scope};

const MAX_VERIFY: u32 = 100_000;
const MAX_CURSOR_BYTES: usize = 4 * 1024;

pub(crate) struct HeadProof {
    pub key_id: String,
    pub key_version: String,
    pub provider_epoch: u64,
    pub algorithm: String,
    pub tag: Vec<u8>,
    pub canonical_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeadBytes<'a> {
    scope: &'a RepositoryScope,
    stream_id: &'a str,
    next_sequence: u64,
    last_event_hash: &'a str,
    repository_format_version: u32,
    key_version: &'a str,
    provider_epoch: u64,
}

pub(crate) async fn authenticate_stream_head(
    store: &PostgresEventStore,
    scope_value: &RepositoryScope,
    stream_id: &str,
    next_sequence: u64,
    last_event_hash: &str,
) -> Result<HeadProof, EventRepositoryError> {
    let metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    let bytes = journal::canonical_bytes(&HeadBytes {
        scope: scope_value,
        stream_id,
        next_sequence,
        last_event_hash,
        repository_format_version: 1,
        key_version: metadata.version(),
        provider_epoch: metadata.current_revocation_epoch(),
    })?;
    let tag = store
        .key_provider()
        .authenticate(
            AuthenticateRequest::new("graphhelm-stream-head-v1", bytes.clone())
                .map_err(|_| EventRepositoryError::Storage)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    let current = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    if current != metadata || tag.key_id() != metadata.key_id() {
        return Err(EventRepositoryError::Integrity);
    }
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "graphhelm-stream-head-v1",
                bytes.clone(),
                tag.clone(),
            )
            .map_err(|_| EventRepositoryError::Storage)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Integrity)?;
    Ok(HeadProof {
        key_id: tag.key_id().to_owned(),
        key_version: metadata.version().to_owned(),
        provider_epoch: metadata.current_revocation_epoch(),
        algorithm: tag.algorithm().to_owned(),
        tag: tag.bytes().to_vec(),
        canonical_sha256: format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
    })
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorPayload {
    workspace_id: String,
    project_id: String,
    execution_id: String,
    stream_id: String,
    sequence: u64,
    event_hash: String,
    head_hash: String,
    format: u32,
    checksum: String,
}

pub(crate) async fn stream_head(
    store: &PostgresEventStore,
    scope_value: RepositoryScope,
    stream_id: String,
) -> Result<Option<StreamHead>, EventRepositoryError> {
    graphhelm_protocols::OpaqueId::parse(&stream_id).map_err(|_| EventRepositoryError::Invalid)?;
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    set_repeatable_read(&mut transaction).await?;
    store.set_scope(&mut transaction, &scope_value).await?;
    let result = query_head(&mut transaction, &stream_id).await?;
    verify_stream_snapshot(
        store,
        &mut transaction,
        &scope_value,
        &stream_id,
        result.as_ref(),
    )
    .await?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(result)
}

pub(crate) async fn read(
    store: &PostgresEventStore,
    request: ReadStreamRequest,
) -> Result<EventPage, EventRepositoryError> {
    let (request_scope, request_stream, request_start, request_limit) = request.into_parts();
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    set_repeatable_read(&mut transaction).await?;
    store.set_scope(&mut transaction, &request_scope).await?;
    let head = query_head(&mut transaction, &request_stream).await?;
    store.after_head_read().await;
    verify_stream_snapshot(
        store,
        &mut transaction,
        &request_scope,
        &request_stream,
        head.as_ref(),
    )
    .await?;
    let (start, expected_previous) = match request_start {
        ReadStart::Beginning => (1, journal::GENESIS_HASH.to_owned()),
        ReadStart::After {
            sequence,
            event_hash,
        } => {
            verify_anchor(&mut transaction, &request_stream, sequence, &event_hash).await?;
            (
                sequence
                    .checked_add(1)
                    .ok_or(EventRepositoryError::LimitExceeded)?,
                event_hash.to_string(),
            )
        }
        ReadStart::Cursor(cursor) => {
            let payload = decode_cursor(store, &cursor).await?;
            bind_cursor(&payload, &request_scope, &request_stream, head.as_ref())?;
            (
                payload
                    .sequence
                    .checked_add(1)
                    .ok_or(EventRepositoryError::LimitExceeded)?,
                payload.event_hash,
            )
        }
    };
    let requested = u64::from(request_limit) + 1;
    let end = start
        .checked_add(requested - 1)
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let mut envelopes =
        fetch_envelopes_chunked(&mut transaction, &request_stream, start, end, requested).await?;
    let has_more = envelopes.len()
        > usize::try_from(request_limit).map_err(|_| EventRepositoryError::LimitExceeded)?;
    if has_more {
        envelopes.pop();
    }
    verify_events(
        &envelopes,
        &request_scope,
        &request_stream,
        start,
        &expected_previous,
    )?;
    if let Some(last) = envelopes.last() {
        verify_window_to_anchor(
            store,
            &mut transaction,
            &request_scope,
            &request_stream,
            start,
            last.sequence,
            head.as_ref(),
        )
        .await?;
    }
    let next_cursor = if has_more {
        let last = envelopes.last().ok_or(EventRepositoryError::Integrity)?;
        Some(
            encode_cursor(
                store,
                &request_scope,
                &request_stream,
                last,
                head.as_ref().ok_or(EventRepositoryError::Integrity)?,
            )
            .await?,
        )
    } else {
        None
    };
    verify_head_tail(&mut transaction, &request_stream, head.as_ref()).await?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(EventPage {
        events: envelopes,
        next_cursor,
        head,
    })
}

async fn verify_checkpoint_suffix(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    checkpoint: &AuthenticatedCheckpoint,
    head: Option<&StreamHead>,
) -> Result<(), EventRepositoryError> {
    verify_anchor(
        transaction,
        &checkpoint.stream_id,
        checkpoint.sequence,
        &checkpoint.event_hash,
    )
    .await?;
    if derive_active_graph(
        store,
        transaction,
        &checkpoint.scope,
        &checkpoint.stream_id,
        checkpoint.sequence,
    )
    .await?
        != checkpoint.active_graph
    {
        return Err(EventRepositoryError::Integrity);
    }
    let start = checkpoint
        .sequence
        .checked_add(1)
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let end = head
        .and_then(|value| value.next_sequence.checked_sub(1))
        .unwrap_or(checkpoint.sequence);
    let events = fetch_envelopes_chunked(
        transaction,
        &checkpoint.stream_id,
        start,
        end,
        u64::from(MAX_VERIFY) + 1,
    )
    .await?;
    if events.len() > usize::try_from(MAX_VERIFY).expect("bounded constant") {
        return Err(EventRepositoryError::LimitExceeded);
    }
    verify_events(
        &events,
        &checkpoint.scope,
        &checkpoint.stream_id,
        checkpoint
            .sequence
            .checked_add(1)
            .ok_or(EventRepositoryError::LimitExceeded)?,
        checkpoint.event_hash.as_str(),
    )?;
    let (tail_sequence, tail_hash) = events
        .last()
        .map_or((checkpoint.sequence, &checkpoint.event_hash), |event| {
            (event.sequence, &event.event_hash)
        });
    if head.is_none_or(|head| {
        head.next_sequence != tail_sequence.checked_add(1).unwrap_or(0)
            || head.last_event_hash != *tail_hash
    }) {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

async fn verify_head_tail(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stream_id: &str,
    head: Option<&StreamHead>,
) -> Result<(), EventRepositoryError> {
    let tail = sqlx::query(
        "SELECT CASE WHEN octet_length(envelope::text) <= 4194304 THEN envelope END AS envelope \
         FROM public.graphhelm_events WHERE stream_id=$1 ORDER BY sequence DESC LIMIT 1",
    )
    .bind(stream_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(crate::error::storage)?;
    match (head, tail) {
        (None, None) => Ok(()),
        (Some(head), None)
            if head.next_sequence == 1
                && head.last_event_hash.as_str() == journal::GENESIS_HASH =>
        {
            Ok(())
        }
        (Some(head), Some(row)) => {
            let value: serde_json::Value = row.try_get("envelope").map_err(crate::error::decode)?;
            let event: EventEnvelope =
                serde_json::from_value(value).map_err(|_| EventRepositoryError::Integrity)?;
            graphhelm_events::validate_envelope(&event)
                .map_err(|_| EventRepositoryError::Integrity)?;
            let expected_next = event
                .sequence
                .checked_add(1)
                .ok_or(EventRepositoryError::Integrity)?;
            if head.next_sequence != expected_next
                || head.last_event_hash != event.event_hash
                || graphhelm_events::compute_event_hash(&event, event.previous_hash.as_str())?
                    != event.event_hash.as_str()
            {
                return Err(EventRepositoryError::Integrity);
            }
            Ok(())
        }
        _ => Err(EventRepositoryError::Integrity),
    }
}

pub(crate) async fn verify_locked_stream(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    next_sequence: u64,
    last_event_hash: &str,
) -> Result<(), EventRepositoryError> {
    let head = StreamHead {
        next_sequence,
        last_event_hash: EventHash::parse(last_event_hash.to_owned())
            .map_err(|_| EventRepositoryError::Integrity)?,
    };
    verify_stream_snapshot(store, transaction, scope_value, stream_id, Some(&head)).await
}

async fn verify_stream_snapshot(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    head: Option<&StreamHead>,
) -> Result<(), EventRepositoryError> {
    if let Some(head) = head {
        verify_stream_head_authentication(store, transaction, scope_value, stream_id, head).await?;
    }
    if let Some(checkpoint) =
        query_verified_checkpoint(store, transaction, scope_value, stream_id).await?
    {
        return verify_checkpoint_suffix(store, transaction, &checkpoint, head).await;
    }
    let end = head
        .and_then(|value| value.next_sequence.checked_sub(1))
        .unwrap_or(0);
    let events =
        fetch_envelopes_chunked(transaction, stream_id, 1, end, u64::from(MAX_VERIFY) + 1).await?;
    if events.len() > usize::try_from(MAX_VERIFY).expect("bounded constant") {
        return Err(EventRepositoryError::Integrity);
    }
    verify_events(&events, scope_value, stream_id, 1, journal::GENESIS_HASH)?;
    let (expected_next, expected_hash) = events.last().map_or_else(
        || (1, journal::GENESIS_HASH.to_owned()),
        |event| {
            (
                event.sequence.saturating_add(1),
                event.event_hash.to_string(),
            )
        },
    );
    let matches_head = match head {
        Some(head) => {
            expected_next == head.next_sequence && expected_hash == head.last_event_hash.as_str()
        }
        None => expected_next == 1 && expected_hash == journal::GENESIS_HASH,
    };
    if !matches_head {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

async fn verify_stream_head_authentication(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    head: &StreamHead,
) -> Result<(), EventRepositoryError> {
    let row = sqlx::query(
        "SELECT head_key_id,head_key_version,head_provider_epoch,head_algorithm,head_tag,head_canonical_sha256 \
         FROM public.graphhelm_streams WHERE stream_id=$1",
    )
    .bind(stream_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(crate::error::storage)?
    .ok_or(EventRepositoryError::Integrity)?;
    let key_id: String = row.try_get("head_key_id").map_err(crate::error::decode)?;
    let key_version: String = row
        .try_get("head_key_version")
        .map_err(crate::error::decode)?;
    let provider_epoch = u64::try_from(
        row.try_get::<i64, _>("head_provider_epoch")
            .map_err(crate::error::decode)?,
    )
    .map_err(|_| EventRepositoryError::Integrity)?;
    let algorithm: String = row
        .try_get("head_algorithm")
        .map_err(crate::error::decode)?;
    let tag = graphhelm_events::AuthenticationTag::new(
        key_id,
        &algorithm,
        row.try_get("head_tag").map_err(crate::error::decode)?,
    )
    .map_err(|_| EventRepositoryError::Integrity)?;
    let bytes = journal::canonical_bytes(&HeadBytes {
        scope: scope_value,
        stream_id,
        next_sequence: head.next_sequence,
        last_event_hash: head.last_event_hash.as_str(),
        repository_format_version: 1,
        key_version: &key_version,
        provider_epoch,
    })?;
    let expected: String = row
        .try_get("head_canonical_sha256")
        .map_err(crate::error::decode)?;
    if expected != format!("sha256:{}", hex::encode(Sha256::digest(&bytes))) {
        return Err(EventRepositoryError::Integrity);
    }
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new("graphhelm-stream-head-v1", bytes, tag)
                .map_err(|_| EventRepositoryError::Integrity)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Integrity)?;
    let metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    if metadata.current_revocation_epoch() < provider_epoch {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

pub(crate) async fn verify_range(
    store: &PostgresEventStore,
    request: VerifyRangeRequest,
) -> Result<IntegrityReport, EventRepositoryError> {
    let (request_scope, request_stream, start_sequence, max_events) = request.into_parts();
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    set_repeatable_read(&mut transaction).await?;
    store.set_scope(&mut transaction, &request_scope).await?;
    let head = query_head(&mut transaction, &request_stream).await?;
    verify_stream_snapshot(
        store,
        &mut transaction,
        &request_scope,
        &request_stream,
        head.as_ref(),
    )
    .await?;
    if head
        .as_ref()
        .is_some_and(|head| start_sequence > head.next_sequence)
    {
        return Err(EventRepositoryError::Integrity);
    }
    let previous = if start_sequence == 1 {
        journal::GENESIS_HASH.to_owned()
    } else {
        let row = sqlx::query(
            "SELECT CASE WHEN octet_length(envelope::text) <= 4194304 THEN envelope END AS envelope \
             FROM public.graphhelm_events WHERE stream_id=$1 AND sequence=$2",
        )
        .bind(&request_stream)
        .bind(i64::try_from(start_sequence - 1).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(crate::error::storage)?
        .ok_or(EventRepositoryError::Integrity)?;
        let value = row.try_get("envelope").map_err(crate::error::decode)?;
        let event: EventEnvelope =
            serde_json::from_value(value).map_err(|_| EventRepositoryError::Integrity)?;
        graphhelm_events::validate_envelope(&event).map_err(|_| EventRepositoryError::Integrity)?;
        if graphhelm_events::compute_event_hash(&event, event.previous_hash.as_str())?
            != event.event_hash.as_str()
        {
            return Err(EventRepositoryError::Integrity);
        }
        event.event_hash.to_string()
    };
    let end = start_sequence
        .checked_add(u64::from(max_events).saturating_sub(1))
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let events = fetch_envelopes_chunked(
        &mut transaction,
        &request_stream,
        start_sequence,
        end,
        u64::from(max_events),
    )
    .await?;
    verify_events(
        &events,
        &request_scope,
        &request_stream,
        start_sequence,
        &previous,
    )?;
    if let Some(last) = events.last() {
        verify_window_to_anchor(
            store,
            &mut transaction,
            &request_scope,
            &request_stream,
            start_sequence,
            last.sequence,
            head.as_ref(),
        )
        .await?;
    }
    verify_head_tail(&mut transaction, &request_stream, head.as_ref()).await?;
    let report = IntegrityReport {
        verified_events: u32::try_from(events.len())
            .map_err(|_| EventRepositoryError::LimitExceeded)?,
        verified_through: events.last().map(|event| event.sequence),
        head,
    };
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(report)
}

pub(crate) async fn append_checkpoint(
    store: &PostgresEventStore,
    checkpoint: AuthenticatedCheckpoint,
) -> Result<AuthenticatedCheckpoint, EventRepositoryError> {
    graphhelm_protocols::OpaqueId::parse(&checkpoint.stream_id)
        .map_err(|_| EventRepositoryError::Invalid)?;
    if checkpoint.sequence == 0
        || checkpoint.sequence > 9_007_199_254_740_991
        || checkpoint.repository_format_version == 0
        || checkpoint.key_version.is_empty()
        || checkpoint.key_version.len() > 256
        || checkpoint.provider_epoch > 9_007_199_254_740_991
    {
        return Err(EventRepositoryError::LimitExceeded);
    }
    if let Some(active) = &checkpoint.active_graph {
        graphhelm_events::ActiveGraphIdentity::new(
            active.number(),
            active.semantic_hash().to_owned(),
        )
        .map_err(|_| EventRepositoryError::LimitExceeded)?;
    }
    let bytes = checkpoint_bytes(&checkpoint)?;
    let authentication_result = store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "graphhelm-checkpoint-v1",
                bytes.clone(),
                checkpoint.tag.clone(),
            )
            .map_err(|_| EventRepositoryError::Invalid)?,
        )
        .await;
    authentication_result.map_err(|_| EventRepositoryError::Integrity)?;
    let provider_metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    if provider_metadata.key_id() != checkpoint.tag.key_id()
        || provider_metadata.version() != checkpoint.key_version
        || provider_metadata.current_revocation_epoch() != checkpoint.provider_epoch
    {
        return Err(EventRepositoryError::Integrity);
    }
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    set_repeatable_read(&mut transaction).await?;
    store.set_scope(&mut transaction, &checkpoint.scope).await?;
    let head = query_head(&mut transaction, &checkpoint.stream_id).await?;
    verify_stream_snapshot(
        store,
        &mut transaction,
        &checkpoint.scope,
        &checkpoint.stream_id,
        head.as_ref(),
    )
    .await?;
    verify_anchor(
        &mut transaction,
        &checkpoint.stream_id,
        checkpoint.sequence,
        &checkpoint.event_hash,
    )
    .await?;
    if derive_active_graph(
        store,
        &mut transaction,
        &checkpoint.scope,
        &checkpoint.stream_id,
        checkpoint.sequence,
    )
    .await?
        != checkpoint.active_graph
    {
        return Err(EventRepositoryError::Integrity);
    }
    let scoped = scope::parts(&checkpoint.scope);
    sqlx::query("INSERT INTO public.graphhelm_checkpoints (workspace_id,project_id,execution_id,stream_id,sequence,event_hash,repository_format_version,created_at,key_id,key_version,provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(&checkpoint.stream_id)
        .bind(i64::try_from(checkpoint.sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.event_hash.as_str()).bind(i32::try_from(checkpoint.repository_format_version).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.created_at).bind(checkpoint.tag.key_id()).bind(&checkpoint.key_version)
        .bind(i64::try_from(checkpoint.provider_epoch).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.active_graph.as_ref().map(|value| i64::try_from(value.number())).transpose().map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.active_graph.as_ref().map(|value| value.semantic_hash()))
        .bind(checkpoint.tag.algorithm()).bind(checkpoint.tag.bytes())
        .bind(hex::encode(Sha256::digest(&bytes))).execute(&mut *transaction).await.map_err(crate::error::storage)?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(checkpoint)
}

pub(crate) async fn checkpoint_head_if_due(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    sequence: u64,
    event_hash: &EventHash,
) -> Result<(), EventRepositoryError> {
    const CHECKPOINT_INTERVAL: u64 = 50_000;
    let latest = query_verified_checkpoint(store, transaction, scope_value, stream_id).await?;
    let distance = sequence.saturating_sub(latest.as_ref().map_or(0, |value| value.sequence));
    if sequence < CHECKPOINT_INTERVAL || distance < CHECKPOINT_INTERVAL {
        return Ok(());
    }
    let metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    let active_graph =
        derive_active_graph(store, transaction, scope_value, stream_id, sequence).await?;
    let placeholder =
        graphhelm_events::AuthenticationTag::new(metadata.key_id(), "hmac-sha256", vec![0_u8; 32])
            .map_err(|_| EventRepositoryError::Storage)?;
    let mut checkpoint = AuthenticatedCheckpoint {
        scope: scope_value.clone(),
        stream_id: stream_id.to_owned(),
        sequence,
        event_hash: event_hash.clone(),
        repository_format_version: 1,
        created_at: chrono::Utc::now(),
        key_version: metadata.version().to_owned(),
        provider_epoch: metadata.current_revocation_epoch(),
        active_graph,
        tag: placeholder,
    };
    let bytes = checkpoint_bytes(&checkpoint)?;
    checkpoint.tag = store
        .key_provider()
        .authenticate(
            AuthenticateRequest::new("graphhelm-checkpoint-v1", bytes.clone())
                .map_err(|_| EventRepositoryError::Storage)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    let current_metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    if current_metadata != metadata || checkpoint.tag.key_id() != metadata.key_id() {
        return Err(EventRepositoryError::Integrity);
    }
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "graphhelm-checkpoint-v1",
                bytes.clone(),
                checkpoint.tag.clone(),
            )
            .map_err(|_| EventRepositoryError::Storage)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Integrity)?;
    let scoped = scope::parts(scope_value);
    sqlx::query("INSERT INTO public.graphhelm_checkpoints (workspace_id,project_id,execution_id,stream_id,sequence,event_hash,repository_format_version,created_at,key_id,key_version,provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(stream_id)
        .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(event_hash.as_str()).bind(1_i32).bind(checkpoint.created_at)
        .bind(checkpoint.tag.key_id()).bind(&checkpoint.key_version)
        .bind(i64::try_from(checkpoint.provider_epoch).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.active_graph.as_ref().map(|value| i64::try_from(value.number())).transpose().map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(checkpoint.active_graph.as_ref().map(|value| value.semantic_hash()))
        .bind(checkpoint.tag.algorithm()).bind(checkpoint.tag.bytes())
        .bind(hex::encode(Sha256::digest(bytes))).execute(&mut **transaction).await.map_err(crate::error::storage)?;
    Ok(())
}

pub(crate) async fn latest_checkpoint(
    store: &PostgresEventStore,
    scope_value: RepositoryScope,
    stream_id: String,
) -> Result<Option<AuthenticatedCheckpoint>, EventRepositoryError> {
    graphhelm_protocols::OpaqueId::parse(&stream_id).map_err(|_| EventRepositoryError::Invalid)?;
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    set_repeatable_read(&mut transaction).await?;
    store.set_scope(&mut transaction, &scope_value).await?;
    let result =
        query_verified_checkpoint(store, &mut transaction, &scope_value, &stream_id).await?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(result)
}

pub(crate) async fn verify_restored_checkpoints(
    store: &PostgresEventStore,
) -> Result<(), EventRepositoryError> {
    const PAGE_SIZE: i64 = 100;
    const MAX_CHECKPOINTS: usize = 100_000;
    let mut cursor: Option<(String, String, String, String, i64)> = None;
    let mut checked = 0_usize;
    loop {
        let (workspace, project, execution, stream, sequence) = cursor.clone().unwrap_or_default();
        let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT workspace_id,project_id,execution_id,stream_id,sequence \
             FROM public.graphhelm_checkpoints \
             WHERE ($1='' OR (workspace_id,project_id,execution_id,stream_id,sequence)>($1,$2,$3,$4,$5)) \
             ORDER BY workspace_id COLLATE \"C\",project_id COLLATE \"C\",execution_id COLLATE \"C\",stream_id COLLATE \"C\",sequence LIMIT $6",
        )
        .bind(&workspace)
        .bind(&project)
        .bind(&execution)
        .bind(&stream)
        .bind(sequence)
        .bind(PAGE_SIZE)
        .fetch_all(store.pool())
        .await
        .map_err(crate::error::storage)?;
        if rows.is_empty() {
            return Ok(());
        }
        checked = checked
            .checked_add(rows.len())
            .ok_or(EventRepositoryError::LimitExceeded)?;
        if checked > MAX_CHECKPOINTS {
            return Err(EventRepositoryError::LimitExceeded);
        }
        for (workspace, project, execution, stream, sequence) in rows {
            let scope_value = RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse(&workspace)
                    .map_err(|_| EventRepositoryError::Integrity)?,
                graphhelm_protocols::ProjectId::parse(&project)
                    .map_err(|_| EventRepositoryError::Integrity)?,
                if execution.is_empty() {
                    None
                } else {
                    Some(
                        graphhelm_protocols::ExecutionId::parse(&execution)
                            .map_err(|_| EventRepositoryError::Integrity)?,
                    )
                },
            );
            let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
            set_repeatable_read(&mut transaction).await?;
            store.set_scope(&mut transaction, &scope_value).await?;
            let exact = query_verified_checkpoint_selected(
                store,
                &mut transaction,
                &scope_value,
                &stream,
                CheckpointSelection::Exact(
                    u64::try_from(sequence).map_err(|_| EventRepositoryError::Integrity)?,
                ),
            )
            .await?;
            transaction.commit().await.map_err(crate::error::storage)?;
            if exact.as_ref().map(|value| value.sequence) != Some(sequence as u64) {
                return Err(EventRepositoryError::Integrity);
            }
            cursor = Some((workspace, project, execution, stream, sequence));
        }
    }
}

async fn query_verified_checkpoint(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
) -> Result<Option<AuthenticatedCheckpoint>, EventRepositoryError> {
    query_verified_checkpoint_selected(
        store,
        transaction,
        scope_value,
        stream_id,
        CheckpointSelection::Latest,
    )
    .await
}

enum CheckpointSelection {
    Latest,
    Exact(u64),
    AtOrBefore(u64),
    AtOrAfter(u64),
}

async fn query_verified_checkpoint_selected(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    selection: CheckpointSelection,
) -> Result<Option<AuthenticatedCheckpoint>, EventRepositoryError> {
    let row = match selection {
        CheckpointSelection::Latest => sqlx::query(
            "SELECT sequence,event_hash,repository_format_version,created_at,key_id,key_version, \
             provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256 FROM public.graphhelm_checkpoints \
             WHERE stream_id=$1 ORDER BY sequence DESC LIMIT 1",
        )
        .bind(stream_id)
        .fetch_optional(&mut **transaction)
        .await,
        CheckpointSelection::Exact(sequence) => sqlx::query(
            "SELECT sequence,event_hash,repository_format_version,created_at,key_id,key_version, \
             provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256 FROM public.graphhelm_checkpoints \
             WHERE stream_id=$1 AND sequence=$2 LIMIT 1",
        )
        .bind(stream_id)
        .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .fetch_optional(&mut **transaction)
        .await,
        CheckpointSelection::AtOrBefore(sequence) => sqlx::query(
            "SELECT sequence,event_hash,repository_format_version,created_at,key_id,key_version, \
             provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256 FROM public.graphhelm_checkpoints \
             WHERE stream_id=$1 AND sequence <= $2 ORDER BY sequence DESC LIMIT 1",
        )
        .bind(stream_id)
        .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .fetch_optional(&mut **transaction)
        .await,
        CheckpointSelection::AtOrAfter(sequence) => sqlx::query(
            "SELECT sequence,event_hash,repository_format_version,created_at,key_id,key_version, \
             provider_epoch,active_graph_number,active_graph_semantic_hash,algorithm,tag,canonical_sha256 FROM public.graphhelm_checkpoints \
             WHERE stream_id=$1 AND sequence >= $2 ORDER BY sequence ASC LIMIT 1",
        )
        .bind(stream_id)
        .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .fetch_optional(&mut **transaction)
        .await,
    }
    .map_err(crate::error::storage)?;
    let result = row
        .map(
            |row| -> Result<AuthenticatedCheckpoint, EventRepositoryError> {
                let tag = graphhelm_events::AuthenticationTag::new(
                    row.try_get::<String, _>("key_id")
                        .map_err(crate::error::decode)?,
                    &row.try_get::<String, _>("algorithm")
                        .map_err(crate::error::decode)?,
                    row.try_get("tag").map_err(crate::error::decode)?,
                )
                .map_err(|_| EventRepositoryError::Integrity)?;
                let active_number: Option<i64> = row
                    .try_get("active_graph_number")
                    .map_err(crate::error::decode)?;
                let active_hash: Option<String> = row
                    .try_get("active_graph_semantic_hash")
                    .map_err(crate::error::decode)?;
                let active_graph = match (active_number, active_hash) {
                    (Some(number), Some(semantic_hash)) => Some(
                        graphhelm_events::ActiveGraphIdentity::new(
                            u64::try_from(number).map_err(|_| EventRepositoryError::Integrity)?,
                            semantic_hash,
                        )
                        .map_err(|_| EventRepositoryError::Integrity)?,
                    ),
                    (None, None) => None,
                    _ => return Err(EventRepositoryError::Integrity),
                };
                let checkpoint = AuthenticatedCheckpoint {
                    scope: scope_value.clone(),
                    stream_id: stream_id.to_owned(),
                    sequence: u64::try_from(
                        row.try_get::<i64, _>("sequence")
                            .map_err(crate::error::decode)?,
                    )
                    .map_err(|_| EventRepositoryError::Integrity)?,
                    event_hash: EventHash::parse(
                        row.try_get::<String, _>("event_hash")
                            .map_err(crate::error::decode)?,
                    )
                    .map_err(|_| EventRepositoryError::Integrity)?,
                    repository_format_version: u32::try_from(
                        row.try_get::<i32, _>("repository_format_version")
                            .map_err(crate::error::decode)?,
                    )
                    .map_err(|_| EventRepositoryError::Integrity)?,
                    created_at: row.try_get("created_at").map_err(crate::error::decode)?,
                    key_version: row.try_get("key_version").map_err(crate::error::decode)?,
                    provider_epoch: u64::try_from(
                        row.try_get::<i64, _>("provider_epoch")
                            .map_err(crate::error::decode)?,
                    )
                    .map_err(|_| EventRepositoryError::Integrity)?,
                    active_graph,
                    tag,
                };
                let expected: String = row
                    .try_get("canonical_sha256")
                    .map_err(crate::error::decode)?;
                if hex::encode(Sha256::digest(checkpoint_bytes(&checkpoint)?)) != expected {
                    return Err(EventRepositoryError::Integrity);
                }
                Ok(checkpoint)
            },
        )
        .transpose()?;
    if let Some(checkpoint) = &result {
        store
            .key_provider()
            .verify(
                VerifyAuthenticationRequest::new(
                    "graphhelm-checkpoint-v1",
                    checkpoint_bytes(checkpoint)?,
                    checkpoint.tag.clone(),
                )
                .map_err(|_| EventRepositoryError::Integrity)?,
            )
            .await
            .map_err(|_| EventRepositoryError::Integrity)?;
        let metadata = store
            .key_provider()
            .metadata()
            .await
            .map_err(|_| EventRepositoryError::Storage)?;
        if metadata.current_revocation_epoch() < checkpoint.provider_epoch {
            return Err(EventRepositoryError::Integrity);
        }
    }
    Ok(result)
}

pub(crate) async fn verify_window_to_anchor(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    first_sequence: u64,
    last_sequence: u64,
    head: Option<&StreamHead>,
) -> Result<(), EventRepositoryError> {
    if last_sequence < first_sequence {
        return Ok(());
    }
    let predecessor = if first_sequence > 1 {
        query_verified_checkpoint_selected(
            store,
            transaction,
            scope_value,
            stream_id,
            CheckpointSelection::AtOrBefore(first_sequence - 1),
        )
        .await?
    } else {
        None
    };
    let successor = query_verified_checkpoint_selected(
        store,
        transaction,
        scope_value,
        stream_id,
        CheckpointSelection::AtOrAfter(last_sequence),
    )
    .await?;
    let start = predecessor.as_ref().map_or(1, |value| value.sequence + 1);
    let previous_hash = predecessor
        .as_ref()
        .map_or(journal::GENESIS_HASH, |value| value.event_hash.as_str());
    let (end, anchor_hash) = if let Some(checkpoint) = successor.as_ref() {
        (checkpoint.sequence, checkpoint.event_hash.as_str())
    } else if let Some(head) = head {
        (
            head.next_sequence
                .checked_sub(1)
                .ok_or(EventRepositoryError::Integrity)?,
            head.last_event_hash.as_str(),
        )
    } else {
        return Err(EventRepositoryError::Integrity);
    };
    let events = fetch_envelopes_chunked(
        transaction,
        stream_id,
        start,
        end,
        u64::from(MAX_VERIFY) + 1,
    )
    .await?;
    if events.len() > usize::try_from(MAX_VERIFY).expect("bounded constant") {
        return Err(EventRepositoryError::Integrity);
    }
    verify_events(&events, scope_value, stream_id, start, previous_hash)?;
    if events.first().is_none_or(|event| event.sequence != start)
        || events
            .last()
            .is_none_or(|event| event.sequence != end || event.event_hash.as_str() != anchor_hash)
    {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

pub(crate) async fn derive_active_graph(
    store: &PostgresEventStore,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope_value: &RepositoryScope,
    stream_id: &str,
    through_sequence: u64,
) -> Result<Option<graphhelm_events::ActiveGraphIdentity>, EventRepositoryError> {
    if through_sequence == 0 {
        return Ok(None);
    }
    let predecessor = if through_sequence > 1 {
        query_verified_checkpoint_selected(
            store,
            transaction,
            scope_value,
            stream_id,
            CheckpointSelection::AtOrBefore(through_sequence - 1),
        )
        .await?
    } else {
        None
    };
    let start = predecessor.as_ref().map_or(1, |value| value.sequence + 1);
    let previous_hash = predecessor
        .as_ref()
        .map_or(journal::GENESIS_HASH, |value| value.event_hash.as_str());
    let events = fetch_envelopes_chunked(
        transaction,
        stream_id,
        start,
        through_sequence,
        u64::from(MAX_VERIFY) + 1,
    )
    .await?;
    if events.len() > usize::try_from(MAX_VERIFY).expect("bounded constant") {
        return Err(EventRepositoryError::Integrity);
    }
    verify_events(&events, scope_value, stream_id, start, previous_hash)?;
    if events
        .last()
        .is_none_or(|event| event.sequence != through_sequence)
    {
        return Err(EventRepositoryError::Integrity);
    }
    let mut active = predecessor.and_then(|value| value.active_graph);
    for event in events {
        if let graphhelm_protocols::EventKind::GraphVersionPublished(payload) = event.kind {
            active = Some(
                graphhelm_events::ActiveGraphIdentity::new(
                    payload.version.number(),
                    payload.version.semantic_hash().to_string(),
                )
                .map_err(|_| EventRepositoryError::Integrity)?,
            );
        }
    }
    Ok(active)
}

async fn query_head(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stream_id: &str,
) -> Result<Option<StreamHead>, EventRepositoryError> {
    sqlx::query(
        "SELECT next_sequence,last_event_hash FROM public.graphhelm_streams WHERE stream_id=$1",
    )
    .bind(stream_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(crate::error::storage)?
    .map(|row| {
        Ok(StreamHead {
            next_sequence: u64::try_from(
                row.try_get::<i64, _>("next_sequence")
                    .map_err(crate::error::decode)?,
            )
            .map_err(|_| EventRepositoryError::Integrity)?,
            last_event_hash: EventHash::parse(
                row.try_get::<String, _>("last_event_hash")
                    .map_err(crate::error::decode)?,
            )
            .map_err(|_| EventRepositoryError::Integrity)?,
        })
    })
    .transpose()
}

async fn set_repeatable_read(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), EventRepositoryError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut **transaction)
        .await
        .map_err(crate::error::storage)?;
    Ok(())
}

async fn verify_anchor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stream_id: &str,
    sequence: u64,
    hash: &EventHash,
) -> Result<(), EventRepositoryError> {
    let row = sqlx::query(
        "SELECT event_hash FROM public.graphhelm_events WHERE stream_id=$1 AND sequence=$2",
    )
    .bind(stream_id)
    .bind(i64::try_from(sequence).map_err(|_| EventRepositoryError::LimitExceeded)?)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(crate::error::storage)?;
    if !row.is_some_and(|row| {
        row.try_get::<String, _>("event_hash")
            .is_ok_and(|value| value == hash.as_str())
    }) {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

pub(crate) async fn fetch_envelopes_chunked(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    stream_id: &str,
    start: u64,
    end: u64,
    max_events: u64,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    const CHUNK_EVENTS: u64 = 4;
    if start > end || max_events == 0 {
        return Ok(Vec::new());
    }
    let bounded_end = end.min(
        start
            .checked_add(max_events - 1)
            .ok_or(EventRepositoryError::LimitExceeded)?,
    );
    let capacity =
        usize::try_from(max_events.min(1_001)).map_err(|_| EventRepositoryError::LimitExceeded)?;
    let mut events = Vec::with_capacity(capacity);
    let mut cursor = start;
    while cursor <= bounded_end {
        let remaining = bounded_end - cursor + 1;
        let chunk = remaining.min(CHUNK_EVENTS);
        let rows = sqlx::query(
            "SELECT sequence,event_hash, \
             CASE WHEN octet_length(envelope::text) <= 4194304 THEN envelope END AS envelope \
             FROM public.graphhelm_events WHERE stream_id=$1 AND sequence BETWEEN $2 AND $3 \
             ORDER BY sequence LIMIT $4",
        )
        .bind(stream_id)
        .bind(i64::try_from(cursor).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(i64::try_from(bounded_end).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .bind(i64::try_from(chunk).map_err(|_| EventRepositoryError::LimitExceeded)?)
        .fetch_all(&mut **transaction)
        .await
        .map_err(crate::error::storage)?;
        if rows.is_empty() {
            break;
        }
        let row_count = rows.len();
        for row in rows {
            let envelope: EventEnvelope =
                serde_json::from_value(row.try_get("envelope").map_err(crate::error::decode)?)
                    .map_err(|_| EventRepositoryError::Integrity)?;
            if row
                .try_get::<i64, _>("sequence")
                .ok()
                .and_then(|value| u64::try_from(value).ok())
                != Some(envelope.sequence)
                || !row
                    .try_get::<String, _>("event_hash")
                    .is_ok_and(|value| value == envelope.event_hash.as_str())
            {
                return Err(EventRepositoryError::Integrity);
            }
            graphhelm_events::validate_envelope(&envelope)
                .map_err(|_| EventRepositoryError::Integrity)?;
            cursor = envelope
                .sequence
                .checked_add(1)
                .ok_or(EventRepositoryError::LimitExceeded)?;
            events.push(envelope);
        }
        if row_count < usize::try_from(chunk).expect("four-event chunk") {
            break;
        }
    }
    Ok(events)
}

fn verify_events(
    events: &[EventEnvelope],
    scope: &RepositoryScope,
    stream_id: &str,
    start: u64,
    previous: &str,
) -> Result<(), EventRepositoryError> {
    let mut expected_sequence = start;
    let mut expected_previous = previous.to_owned();
    for event in events {
        if &event.scope != scope
            || event.stream_id.as_str() != stream_id
            || event.sequence != expected_sequence
            || event.previous_hash.as_str() != expected_previous
            || graphhelm_events::compute_event_hash(event, &expected_previous)?
                != event.event_hash.as_str()
        {
            return Err(EventRepositoryError::Integrity);
        }
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        expected_previous = event.event_hash.to_string();
    }
    Ok(())
}

async fn encode_cursor(
    store: &PostgresEventStore,
    scope_value: &RepositoryScope,
    stream_id: &str,
    event: &EventEnvelope,
    head: &StreamHead,
) -> Result<String, EventRepositoryError> {
    let scoped = scope::parts(scope_value);
    let mut payload = CursorPayload {
        workspace_id: scoped.workspace.to_owned(),
        project_id: scoped.project.to_owned(),
        execution_id: scoped.execution.to_owned(),
        stream_id: stream_id.to_owned(),
        sequence: event.sequence,
        event_hash: event.event_hash.to_string(),
        head_hash: head.last_event_hash.to_string(),
        format: 1,
        checksum: String::new(),
    };
    payload.checksum = hex::encode(Sha256::digest(journal::canonical_bytes(&payload)?));
    let bytes = journal::canonical_bytes(&payload)?;
    let tag = store
        .key_provider()
        .authenticate(
            AuthenticateRequest::new("graphhelm-cursor-v1", bytes.clone())
                .map_err(|_| EventRepositoryError::Invalid)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Storage)?;
    let cursor = format!(
        "{}.{}.{}",
        hex::encode(bytes),
        hex::encode(tag.key_id()),
        hex::encode(tag.bytes())
    );
    if cursor.len() > MAX_CURSOR_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(cursor)
}

async fn decode_cursor(
    store: &PostgresEventStore,
    cursor: &str,
) -> Result<CursorPayload, EventRepositoryError> {
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    let parts = cursor.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(EventRepositoryError::Integrity);
    }
    let bytes = hex::decode(parts[0]).map_err(|_| EventRepositoryError::Integrity)?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    let key_id =
        String::from_utf8(hex::decode(parts[1]).map_err(|_| EventRepositoryError::Integrity)?)
            .map_err(|_| EventRepositoryError::Integrity)?;
    let tag = graphhelm_events::AuthenticationTag::new(
        key_id,
        "hmac-sha256",
        hex::decode(parts[2]).map_err(|_| EventRepositoryError::Integrity)?,
    )
    .map_err(|_| EventRepositoryError::Integrity)?;
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new("graphhelm-cursor-v1", bytes.clone(), tag)
                .map_err(|_| EventRepositoryError::Integrity)?,
        )
        .await
        .map_err(|_| EventRepositoryError::Integrity)?;
    let payload: CursorPayload =
        serde_json::from_slice(&bytes).map_err(|_| EventRepositoryError::Integrity)?;
    let actual_checksum = payload.checksum.clone();
    let mut checksum_payload = CursorPayload {
        checksum: String::new(),
        ..payload
    };
    let expected = hex::encode(Sha256::digest(journal::canonical_bytes(&checksum_payload)?));
    if actual_checksum != expected {
        return Err(EventRepositoryError::Integrity);
    }
    checksum_payload.checksum = expected;
    Ok(checksum_payload)
}

fn bind_cursor(
    payload: &CursorPayload,
    scope_value: &RepositoryScope,
    stream_id: &str,
    head: Option<&StreamHead>,
) -> Result<(), EventRepositoryError> {
    let scoped = scope::parts(scope_value);
    if payload.workspace_id != scoped.workspace
        || payload.project_id != scoped.project
        || payload.execution_id != scoped.execution
        || payload.stream_id != stream_id
        || payload.format != 1
        || head.is_none_or(|head| head.last_event_hash.as_str() != payload.head_hash)
    {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

fn checkpoint_bytes(checkpoint: &AuthenticatedCheckpoint) -> Result<Vec<u8>, EventRepositoryError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Bytes<'a> {
        scope: &'a RepositoryScope,
        stream_id: &'a str,
        sequence: u64,
        event_hash: &'a str,
        repository_format_version: u32,
        created_at: chrono::DateTime<chrono::Utc>,
        key_version: &'a str,
        provider_epoch: u64,
        active_graph: &'a Option<graphhelm_events::ActiveGraphIdentity>,
    }
    journal::canonical_bytes(&Bytes {
        scope: &checkpoint.scope,
        stream_id: &checkpoint.stream_id,
        sequence: checkpoint.sequence,
        event_hash: checkpoint.event_hash.as_str(),
        repository_format_version: checkpoint.repository_format_version,
        created_at: checkpoint.created_at,
        key_version: &checkpoint.key_version,
        provider_epoch: checkpoint.provider_epoch,
        active_graph: &checkpoint.active_graph,
    })
}
