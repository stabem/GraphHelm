use graphhelm_events::{
    EventRepositoryError, ProjectionGeneration, ProjectionRebuildRequest, StreamHead,
};
use graphhelm_protocols::{EventHash, RepositoryScope};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};

use crate::{PostgresEventStore, error};

const PROJECTION_FORMAT_VERSION: i32 = 1;
const MAX_PROJECTION_STATE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) async fn load_generation(
    store: &PostgresEventStore,
    request: &ProjectionRebuildRequest,
) -> Result<Option<ProjectionGeneration>, EventRepositoryError> {
    let mut transaction = store.pool().begin().await.map_err(error::storage)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(error::storage)?;
    store.set_scope(&mut transaction, request.scope()).await?;
    let row = sqlx::query(
        "SELECT workspace_id,project_id,execution_id,stream_id,projection_name,\
         projection_version,generation,last_sequence,last_event_hash,format_version,\
         CASE WHEN octet_length(state::text) <= 67108864 THEN state END AS state \
         FROM graphhelm_projection_checkpoints \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
           AND projection_name=$5 AND projection_version=$6 AND generation=$7 \
         ORDER BY last_sequence DESC LIMIT 1",
    )
    .bind(request.scope().workspace_id().as_str())
    .bind(request.scope().project_id().as_str())
    .bind(
        request
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(request.stream_id())
    .bind(request.projection_name())
    .bind(i32::try_from(request.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(request.generation()).map_err(|_| EventRepositoryError::Invalid)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    let generation = row.map(decode_generation).transpose()?;
    if let Some(value) = generation.as_ref() {
        authenticate_stored_generation(store, &mut transaction, value).await?;
    }
    transaction.commit().await.map_err(error::storage)?;
    Ok(generation)
}

pub(crate) async fn save_generation(
    store: &PostgresEventStore,
    generation: ProjectionGeneration,
) -> Result<(), EventRepositoryError> {
    let state = serde_json::to_value(&generation).map_err(error::decode)?;
    if serde_json::to_vec(&state).map_err(error::decode)?.len() > MAX_PROJECTION_STATE_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    let watermark = generation.watermark();
    let mut transaction = store.pool().begin().await.map_err(error::storage)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(error::storage)?;
    store.set_scope(&mut transaction, watermark.scope()).await?;
    authenticate_stored_generation(store, &mut transaction, &generation).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(projection_lock_key(watermark))
        .execute(&mut *transaction)
        .await
        .map_err(error::storage)?;
    let prior = sqlx::query(
        "SELECT last_sequence,last_event_hash,format_version,\
         CASE WHEN octet_length(state::text) <= 67108864 THEN state END AS state \
         FROM graphhelm_projection_checkpoints \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
           AND projection_name=$5 AND projection_version=$6 AND generation=$7 \
         ORDER BY last_sequence DESC LIMIT 1",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .bind(watermark.projection_name())
    .bind(i32::try_from(watermark.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.generation()).map_err(|_| EventRepositoryError::Invalid)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    if let Some(row) = prior {
        let prior_sequence: i64 = row.get("last_sequence");
        let prior_hash: Option<String> = row.get("last_event_hash");
        let prior_format: i32 = row.get("format_version");
        let prior_state: Value = row
            .try_get::<Option<Value>, _>("state")
            .map_err(error::decode)?
            .ok_or(EventRepositoryError::LimitExceeded)?;
        let sequence =
            i64::try_from(watermark.last_sequence()).map_err(|_| EventRepositoryError::Invalid)?;
        let hash = watermark.last_event_hash().map(EventHash::as_str);
        if prior_sequence > sequence {
            return Err(EventRepositoryError::Integrity);
        }
        if prior_sequence == sequence {
            if prior_format == PROJECTION_FORMAT_VERSION
                && prior_hash.as_deref() == hash
                && prior_state == state
            {
                transaction.commit().await.map_err(error::storage)?;
                return Ok(());
            }
            return Err(EventRepositoryError::Integrity);
        }
    }
    insert_checkpoint(&mut transaction, &generation, state).await?;
    transaction.commit().await.map_err(error::storage)
}

pub(crate) async fn load_active(
    store: &PostgresEventStore,
    requested_scope: RepositoryScope,
    stream_id: String,
    projection_name: String,
    projection_version: u32,
) -> Result<Option<ProjectionGeneration>, EventRepositoryError> {
    let mut transaction = store.pool().begin().await.map_err(error::storage)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(error::storage)?;
    store.set_scope(&mut transaction, &requested_scope).await?;
    let row = sqlx::query(
        "SELECT c.workspace_id,c.project_id,c.execution_id,c.stream_id,c.projection_name,\
         c.projection_version,c.generation,c.last_sequence,c.last_event_hash,c.format_version,\
         CASE WHEN octet_length(c.state::text) <= 67108864 THEN c.state END AS state \
         FROM graphhelm_projection_active a JOIN graphhelm_projection_checkpoints c USING \
         (workspace_id,project_id,execution_id,stream_id,projection_name,projection_version,generation,last_sequence) \
         WHERE a.workspace_id=$1 AND a.project_id=$2 AND a.execution_id=$3 \
           AND a.stream_id=$4 AND a.projection_name=$5 AND a.projection_version=$6",
    )
    .bind(requested_scope.workspace_id().as_str())
    .bind(requested_scope.project_id().as_str())
    .bind(requested_scope.execution_id().map_or("", |value| value.as_str()))
    .bind(&stream_id)
    .bind(&projection_name)
    .bind(i32::try_from(projection_version).map_err(|_| EventRepositoryError::Invalid)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    let generation = row.map(decode_generation).transpose()?;
    if generation.as_ref().is_some_and(|value| {
        value.watermark().scope() != &requested_scope
            || value.watermark().stream_id() != stream_id
            || value.watermark().projection_name() != projection_name
            || value.watermark().projection_version() != projection_version
    }) {
        return Err(EventRepositoryError::Integrity);
    }
    if let Some(value) = generation.as_ref() {
        authenticate_stored_generation(store, &mut transaction, value).await?;
    }
    transaction.commit().await.map_err(error::storage)?;
    Ok(generation)
}

async fn authenticate_stored_generation(
    store: &PostgresEventStore,
    transaction: &mut Transaction<'_, Postgres>,
    generation: &ProjectionGeneration,
) -> Result<(), EventRepositoryError> {
    let watermark = generation.watermark();
    let source = sqlx::query(
        "SELECT next_sequence,last_event_hash FROM graphhelm_streams \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(error::storage)?;
    match source {
        Some(row) => {
            let next_sequence = u64::try_from(row.get::<i64, _>("next_sequence"))
                .map_err(|_| EventRepositoryError::Integrity)?;
            let last_event_hash: String = row.get("last_event_hash");
            if watermark.last_sequence() >= next_sequence {
                return Err(EventRepositoryError::Integrity);
            }
            crate::integrity::verify_locked_stream(
                store,
                transaction,
                watermark.scope(),
                watermark.stream_id(),
                next_sequence,
                &last_event_hash,
            )
            .await?;
        }
        None if watermark.last_sequence() == 0 => {}
        None => return Err(EventRepositoryError::Integrity),
    }
    authenticate_generation(transaction, generation).await
}

pub(crate) async fn swap_active(
    store: &PostgresEventStore,
    generation: ProjectionGeneration,
    expected_source_head: Option<StreamHead>,
) -> Result<(), EventRepositoryError> {
    let watermark = generation.watermark();
    if !watermark_matches_head(
        watermark.last_sequence(),
        watermark.last_event_hash(),
        expected_source_head.as_ref(),
    ) {
        return Err(EventRepositoryError::SequenceConflict);
    }
    let state = serde_json::to_value(&generation).map_err(error::decode)?;
    let mut transaction = store.pool().begin().await.map_err(error::storage)?;
    store.set_scope(&mut transaction, watermark.scope()).await?;
    // This lock makes both present and absent source-head checks atomic with activation.
    sqlx::query("LOCK TABLE public.graphhelm_streams IN SHARE MODE")
        .execute(&mut *transaction)
        .await
        .map_err(error::storage)?;
    let source = sqlx::query(
        "SELECT next_sequence,last_event_hash FROM graphhelm_streams \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    let source_matches = match (source, expected_source_head.as_ref()) {
        (None, None) => true,
        (Some(row), Some(expected)) => {
            let next_sequence: i64 = row.get("next_sequence");
            let last_event_hash: String = row.get("last_event_hash");
            u64::try_from(next_sequence).ok() == Some(expected.next_sequence)
                && last_event_hash == expected.last_event_hash.as_str()
        }
        _ => false,
    };
    if !source_matches {
        return Err(EventRepositoryError::SequenceConflict);
    }
    if let Some(expected) = expected_source_head.as_ref() {
        crate::integrity::verify_locked_stream(
            store,
            &mut transaction,
            watermark.scope(),
            watermark.stream_id(),
            expected.next_sequence,
            expected.last_event_hash.as_str(),
        )
        .await?;
    }
    authenticate_generation(&mut transaction, &generation).await?;
    let stored = sqlx::query(
        "SELECT CASE WHEN octet_length(state::text) <= 67108864 THEN state END AS state \
         FROM graphhelm_projection_checkpoints \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
           AND projection_name=$5 AND projection_version=$6 AND generation=$7 AND last_sequence=$8",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .bind(watermark.projection_name())
    .bind(i32::try_from(watermark.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.generation()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.last_sequence()).map_err(|_| EventRepositoryError::Invalid)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    let stored = match stored {
        Some(row) => row
            .try_get::<Option<Value>, _>("state")
            .map_err(error::decode)?
            .ok_or(EventRepositoryError::LimitExceeded)?,
        None => return Err(EventRepositoryError::Integrity),
    };
    if stored != state {
        return Err(EventRepositoryError::Integrity);
    }
    let active = sqlx::query(
        "SELECT generation,last_sequence FROM graphhelm_projection_active \
         WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4 \
           AND projection_name=$5 AND projection_version=$6 FOR UPDATE",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .bind(watermark.projection_name())
    .bind(i32::try_from(watermark.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(error::storage)?;
    if let Some(row) = active {
        let active_generation: i64 = row.get("generation");
        let active_sequence: i64 = row.get("last_sequence");
        let requested_generation =
            i64::try_from(watermark.generation()).map_err(|_| EventRepositoryError::Invalid)?;
        let requested_sequence =
            i64::try_from(watermark.last_sequence()).map_err(|_| EventRepositoryError::Invalid)?;
        if active_generation == requested_generation && active_sequence == requested_sequence {
            transaction.commit().await.map_err(error::storage)?;
            return Ok(());
        }
        if active_generation >= requested_generation {
            return Err(EventRepositoryError::SequenceConflict);
        }
    }
    sqlx::query(
        "INSERT INTO graphhelm_projection_active \
         (workspace_id,project_id,execution_id,stream_id,projection_name,projection_version,generation,last_sequence) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         ON CONFLICT (workspace_id,project_id,execution_id,stream_id,projection_name,projection_version) \
         DO UPDATE SET generation=EXCLUDED.generation,last_sequence=EXCLUDED.last_sequence",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(watermark.scope().execution_id().map_or("", |value| value.as_str()))
    .bind(watermark.stream_id())
    .bind(watermark.projection_name())
    .bind(i32::try_from(watermark.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.generation()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.last_sequence()).map_err(|_| EventRepositoryError::Invalid)?)
    .execute(&mut *transaction)
    .await
    .map_err(error::storage)?;
    transaction.commit().await.map_err(error::storage)
}

async fn authenticate_generation(
    transaction: &mut Transaction<'_, Postgres>,
    supplied: &ProjectionGeneration,
) -> Result<(), EventRepositoryError> {
    const REPLAY_PAGE: u64 = 1_000;
    let watermark = supplied.watermark();
    let mut rebuilt = ProjectionGeneration::new(
        watermark.scope().clone(),
        watermark.stream_id().to_owned(),
        watermark.projection_name().to_owned(),
        watermark.projection_version(),
        watermark.generation(),
    )
    .map_err(|_| EventRepositoryError::Integrity)?;
    let mut start = 1_u64;
    while start <= watermark.last_sequence() {
        let end = start
            .checked_add(REPLAY_PAGE - 1)
            .ok_or(EventRepositoryError::LimitExceeded)?
            .min(watermark.last_sequence());
        let events = crate::integrity::fetch_envelopes_chunked(
            transaction,
            watermark.stream_id(),
            start,
            end,
            REPLAY_PAGE,
        )
        .await?;
        if events.is_empty() {
            return Err(EventRepositoryError::Integrity);
        }
        rebuilt.apply_page(&events).map_err(|error| match error {
            graphhelm_events::ReplayError::LimitExceeded => EventRepositoryError::LimitExceeded,
            graphhelm_events::ReplayError::Corrupt => EventRepositoryError::Integrity,
            // `apply_page` declares no budget, so this arm is unreachable from here; it is
            // mapped to its own error rather than folded onto `Integrity` for the reason
            // `map_replay_error` states - a read out of time is not a corrupt stream.
            graphhelm_events::ReplayError::BudgetExceeded {
                walked,
                limit_millis,
            } => EventRepositoryError::ReadBudgetExceeded {
                walked,
                limit_millis,
            },
        })?;
        start = rebuilt
            .watermark()
            .last_sequence()
            .checked_add(1)
            .ok_or(EventRepositoryError::LimitExceeded)?;
    }
    if rebuilt != *supplied {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

async fn insert_checkpoint(
    transaction: &mut Transaction<'_, Postgres>,
    generation: &ProjectionGeneration,
    state: Value,
) -> Result<(), EventRepositoryError> {
    let watermark = generation.watermark();
    let inserted = sqlx::query(
        // Every read gates on `octet_length(state::text)`, and PostgreSQL renders jsonb with
        // separators that make it strictly longer than the compact form measured client-side. A
        // state accepted only against the compact size could therefore insert and then be
        // permanently unreadable, which also blocks every later checkpoint for that generation.
        // Admission uses the exact expression the reads use, so anything stored stays readable.
        "INSERT INTO graphhelm_projection_checkpoints \
         (workspace_id,project_id,execution_id,stream_id,projection_name,projection_version,\
          generation,last_sequence,last_event_hash,format_version,state) \
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11 \
         WHERE octet_length(($11::jsonb)::text) <= 67108864",
    )
    .bind(watermark.scope().workspace_id().as_str())
    .bind(watermark.scope().project_id().as_str())
    .bind(
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
    )
    .bind(watermark.stream_id())
    .bind(watermark.projection_name())
    .bind(i32::try_from(watermark.projection_version()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.generation()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(i64::try_from(watermark.last_sequence()).map_err(|_| EventRepositoryError::Invalid)?)
    .bind(watermark.last_event_hash().map(EventHash::as_str))
    .bind(PROJECTION_FORMAT_VERSION)
    .bind(state)
    .execute(&mut **transaction)
    .await
    .map_err(error::storage)?;
    // The admission guard is a WHERE clause, so an oversized state inserts no row. Reporting
    // success here would be a fake success, so the rejection is surfaced explicitly.
    if inserted.rows_affected() != 1 {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(())
}

fn decode_generation(
    row: sqlx::postgres::PgRow,
) -> Result<ProjectionGeneration, EventRepositoryError> {
    let format_version: i32 = row.get("format_version");
    if format_version != PROJECTION_FORMAT_VERSION {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    let state: Value = row
        .try_get::<Option<Value>, _>("state")
        .map_err(error::decode)?
        .ok_or(EventRepositoryError::LimitExceeded)?;
    let generation: ProjectionGeneration = serde_json::from_value(state).map_err(error::decode)?;
    let watermark = generation.watermark();
    let execution = watermark
        .scope()
        .execution_id()
        .map_or("", |value| value.as_str());
    let row_sequence: i64 = row.get("last_sequence");
    let row_hash: Option<String> = row.get("last_event_hash");
    if watermark.scope().workspace_id().as_str() != row.get::<String, _>("workspace_id")
        || watermark.scope().project_id().as_str() != row.get::<String, _>("project_id")
        || execution != row.get::<String, _>("execution_id")
        || watermark.stream_id() != row.get::<String, _>("stream_id")
        || watermark.projection_name() != row.get::<String, _>("projection_name")
        || i64::from(watermark.projection_version())
            != i64::from(row.get::<i32, _>("projection_version"))
        || i64::try_from(watermark.generation()).ok() != Some(row.get("generation"))
        || i64::try_from(watermark.last_sequence()).ok() != Some(row_sequence)
        || watermark.last_event_hash().map(EventHash::as_str) != row_hash.as_deref()
    {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(generation)
}

fn watermark_matches_head(
    last_sequence: u64,
    last_event_hash: Option<&EventHash>,
    head: Option<&StreamHead>,
) -> bool {
    match head {
        None => last_sequence == 0 && last_event_hash.is_none(),
        Some(head) => {
            head.next_sequence.checked_sub(1) == Some(last_sequence)
                && last_event_hash == Some(&head.last_event_hash)
        }
    }
}

fn projection_lock_key(watermark: &graphhelm_events::ProjectionWatermark) -> String {
    let values = [
        watermark.scope().workspace_id().as_str(),
        watermark.scope().project_id().as_str(),
        watermark
            .scope()
            .execution_id()
            .map_or("", |value| value.as_str()),
        watermark.stream_id(),
        watermark.projection_name(),
    ];
    let mut key = String::from("graphhelm-projection-v1:");
    for value in values {
        key.push_str(&value.len().to_string());
        key.push(':');
        key.push_str(value);
    }
    key.push(':');
    key.push_str(&watermark.projection_version().to_string());
    key.push(':');
    key.push_str(&watermark.generation().to_string());
    key
}
