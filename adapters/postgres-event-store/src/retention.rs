use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, TimeDelta, Utc};
use graphhelm_events::{
    CleanupReceipt, CleanupRequest, EvidencePriorAvailability, FinalizedRetention, KeyError,
    LegalHoldChange, LegalHoldReceipt, PreparedRetention, PreparedRetentionTarget,
    RetentionBlockReason, RetentionError, RetentionPlan, RetentionPlanTarget,
    RetentionPrepareOutcome, RetentionRequest, RevocationReceipt, VerifyAuthenticationRequest,
    cleanup_request_digest, compute_event_hash, finalized_authentication_bytes,
    legal_hold_authentication_bytes, prepared_authentication_bytes,
    provider_revocation_idempotency_key, retention_authority_authentication_bytes,
    retention_request_digest, revocation_receipt_authentication_bytes, validate_envelope,
    validate_envelope_content,
};
use graphhelm_protocols::{
    ActorId, ErasedState, ErasurePendingState, EventEnvelope, EventHash, EventKind,
    EvidenceCiphertextDeleted, EvidenceErasureCompleted, EvidenceErasureRequested,
    EvidenceLegalHoldChanged, EvidencePriorState, LegalHoldState, NewEvent, OpaqueId,
    PendingPriorState, PersistedActor, PersistedActorType, PersistedTimestamp, RepositoryScope,
    Sensitivity,
};
use sha2::Digest;
use sqlx::{Postgres, Row, Transaction};

use crate::{PostgresEventStore, journal::GENESIS_HASH, rows::StoredEvidence, scope};

const RETENTION_STREAM: &str = "retention-events";
const MAX_LEGAL_HOLD_HISTORY_ROWS: usize = 20_000;

pub(crate) async fn dry_run(
    store: &PostgresEventStore,
    request: RetentionRequest,
    evaluated_at: PersistedTimestamp,
) -> Result<RetentionPlan, RetentionError> {
    verify_authority(store, &request).await?;
    let mut transaction = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut transaction).await?;
    store
        .set_scope(&mut transaction, request.scope())
        .await
        .map_err(repository)?;
    if let Some(existing) = load_by_idempotency(&mut transaction, &request).await? {
        verify_prepared(store, &existing).await?;
        transaction.commit().await.map_err(storage)?;
        return Ok(existing.plan().clone());
    }
    let ids = request
        .targets()
        .iter()
        .map(|target| target.evidence_id().as_str())
        .collect::<Vec<_>>();
    let held_ids = held_evidence_ids(store, &mut transaction, &ids).await?;
    let rows = sqlx::query("SELECT evidence_id,record,state,created_at FROM public.graphhelm_evidence e WHERE evidence_id=ANY($1) ORDER BY evidence_id COLLATE \"C\"")
        .bind(&ids).fetch_all(&mut *transaction).await.map_err(storage)?;
    if rows.len() != ids.len() {
        return Err(RetentionError::Ineligible);
    }
    let mut targets = Vec::with_capacity(rows.len());
    for row in rows {
        let state: String = row.try_get("state").map_err(decode)?;
        let created_at: DateTime<Utc> = row.try_get("created_at").map_err(decode)?;
        let held = held_ids.contains(&row.try_get::<String, _>("evidence_id").map_err(decode)?);
        let stored: StoredEvidence = serde_json::from_value(row.try_get("record").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?;
        let sealed = stored.into_sealed().map_err(repository)?;
        let prior = prior_state(&state)?;
        let age = evaluated_at
            .as_datetime()
            .signed_duration_since(created_at)
            .num_seconds();
        let blocked = if held {
            Some(RetentionBlockReason::LegalHold)
        } else if sealed.retention_class() != request.policy().retention_class() {
            Some(RetentionBlockReason::PolicyMismatch)
        } else if age < 0
            || u64::try_from(age).unwrap_or(0) < request.policy().minimum_age_seconds()
        {
            Some(RetentionBlockReason::MinimumAge)
        } else {
            None
        };
        targets.push(RetentionPlanTarget::new(
            sealed.reference().evidence_id().clone(),
            OpaqueId::parse(sealed.wrapped_key().handle())
                .map_err(|_| RetentionError::Integrity)?,
            sealed.reference().ciphertext_sha256().clone(),
            sealed.sensitivity(),
            prior,
            blocked,
        )?);
    }
    transaction.commit().await.map_err(storage)?;
    RetentionPlan::new(
        request.operation_id().clone(),
        retention_request_digest(&request),
        evaluated_at,
        targets,
    )
}

pub(crate) async fn prepare(
    store: &PostgresEventStore,
    prepared: PreparedRetention,
) -> Result<RetentionPrepareOutcome, RetentionError> {
    verify_authority(store, prepared.request()).await?;
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-prepared",
                prepared_authentication_bytes(&prepared),
                prepared.authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)?;
    let mut transaction = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut transaction).await?;
    store
        .set_scope(&mut transaction, prepared.request().scope())
        .await
        .map_err(repository)?;
    if let Some(existing) = load_by_idempotency(&mut transaction, prepared.request()).await? {
        verify_prepared(store, &existing).await?;
        if existing.request() != prepared.request() {
            return Err(RetentionError::Conflict);
        }
        let finalized = load_finalized(&mut transaction, &existing).await?;
        if let Some(value) = finalized.as_ref() {
            verify_finalized(store, value).await?;
        }
        transaction.commit().await.map_err(storage)?;
        return Ok(finalized.map_or(
            RetentionPrepareOutcome::Prepared(existing),
            RetentionPrepareOutcome::Finalized,
        ));
    }
    let ids = prepared
        .plan()
        .targets()
        .iter()
        .map(|target| target.evidence_id().as_str())
        .collect::<Vec<_>>();
    let locked = sqlx::query("SELECT evidence_id,state,record,created_at FROM public.graphhelm_evidence WHERE evidence_id=ANY($1) ORDER BY evidence_id COLLATE \"C\" FOR UPDATE")
        .bind(&ids).fetch_all(&mut *transaction).await.map_err(storage)?;
    if locked.len() != ids.len() {
        return Err(RetentionError::Ineligible);
    }
    if !held_evidence_ids(store, &mut transaction, &ids)
        .await?
        .is_empty()
    {
        return Err(RetentionError::LegalHold);
    }
    for (row, target) in locked.iter().zip(prepared.resolved_targets()) {
        let id: String = row.try_get("evidence_id").map_err(decode)?;
        let state: String = row.try_get("state").map_err(decode)?;
        let stored: StoredEvidence = serde_json::from_value(row.try_get("record").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?;
        let sealed = stored.into_sealed().map_err(repository)?;
        let created_at: DateTime<Utc> = row.try_get("created_at").map_err(decode)?;
        let age = prepared
            .plan()
            .evaluated_at()
            .as_datetime()
            .signed_duration_since(created_at)
            .num_seconds();
        if id != target.evidence_id().as_str()
            || prior_state(&state)? != target.prior_availability()
            || sealed.wrapped_key().handle() != target.key_handle_id().as_str()
            || sealed.reference().ciphertext_sha256() != target.ciphertext_sha256()
            || sealed.sensitivity() != target.classification()
            || sealed.retention_class() != prepared.request().policy().retention_class()
            || age < 0
            || u64::try_from(age).unwrap_or(0) < prepared.request().policy().minimum_age_seconds()
        {
            return Err(RetentionError::Conflict);
        }
    }
    let scoped = scope::parts(prepared.request().scope());
    sqlx::query("INSERT INTO public.graphhelm_retention_policies(workspace_id,project_id,execution_id,policy_id,policy_version,retention_class,minimum_age_seconds,cleanup_delay_seconds) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING")
        .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution)
        .bind(prepared.request().policy().id().as_str()).bind(prepared.request().policy().version().as_str())
        .bind(prepared.request().policy().retention_class()).bind(i64_of(prepared.request().policy().minimum_age_seconds())?).bind(i64_of(prepared.request().policy().cleanup_delay_seconds())?)
        .execute(&mut *transaction).await.map_err(storage)?;
    let stored_policy = sqlx::query("SELECT retention_class,minimum_age_seconds,cleanup_delay_seconds FROM public.graphhelm_retention_policies WHERE policy_id=$1 AND policy_version=$2")
        .bind(prepared.request().policy().id().as_str())
        .bind(prepared.request().policy().version().as_str())
        .fetch_one(&mut *transaction).await.map_err(storage)?;
    if stored_policy
        .try_get::<String, _>("retention_class")
        .map_err(decode)?
        != prepared.request().policy().retention_class()
        || u64_of(
            stored_policy
                .try_get("minimum_age_seconds")
                .map_err(decode)?,
        )? != prepared.request().policy().minimum_age_seconds()
        || u64_of(
            stored_policy
                .try_get("cleanup_delay_seconds")
                .map_err(decode)?,
        )? != prepared.request().policy().cleanup_delay_seconds()
    {
        return Err(RetentionError::Conflict);
    }
    sqlx::query("INSERT INTO public.graphhelm_retention_operations(workspace_id,project_id,execution_id,operation_id,idempotency_key,request_digest,policy_id,policy_version,authority,authority_key_id,authority_algorithm,authority_tag,reason_code,evaluated_at,requested_at,provider_epoch,prepared_key_id,prepared_algorithm,prepared_tag,state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,'prepared')")
        .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(prepared.request().operation_id().as_str())
        .bind(prepared.request().idempotency_key().as_str()).bind(prepared.plan().request_digest().as_str())
        .bind(prepared.request().policy().id().as_str()).bind(prepared.request().policy().version().as_str())
        .bind(prepared.request().authority().id().as_str())
        .bind(prepared.request().authority().authentication_tag().key_id()).bind(prepared.request().authority().authentication_tag().algorithm()).bind(prepared.request().authority().authentication_tag().bytes())
        .bind(prepared.request().reason_code().as_str()).bind(timestamp_wire(prepared.plan().evaluated_at()))
        .bind(timestamp_wire(prepared.prepared_at())).bind(i64_of(prepared.provider_epoch())?)
        .bind(prepared.authentication_tag().key_id()).bind(prepared.authentication_tag().algorithm()).bind(prepared.authentication_tag().bytes())
        .execute(&mut *transaction).await.map_err(constraint)?;
    for (ordinal, target) in prepared.resolved_targets().enumerate() {
        sqlx::query("INSERT INTO public.graphhelm_retention_targets(workspace_id,project_id,execution_id,operation_id,ordinal,evidence_id,key_handle_id,ciphertext_sha256,classification,prior_state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(prepared.request().operation_id().as_str()).bind(i32::try_from(ordinal).map_err(|_| RetentionError::LimitExceeded)?)
            .bind(target.evidence_id().as_str()).bind(target.key_handle_id().as_str()).bind(target.ciphertext_sha256().as_str()).bind(sensitivity_name(target.classification())).bind(prior_name(target.prior_availability()))
            .execute(&mut *transaction).await.map_err(constraint)?;
    }
    let pending_update = sqlx::query(
        "UPDATE public.graphhelm_evidence SET state='erasure_pending' WHERE evidence_id=ANY($1)",
    )
    .bind(&ids)
    .execute(&mut *transaction)
    .await
    .map_err(constraint)?;
    if pending_update.rows_affected()
        != u64::try_from(ids.len()).map_err(|_| RetentionError::LimitExceeded)?
    {
        return Err(RetentionError::Conflict);
    }
    let events = prepared
        .resolved_targets()
        .enumerate()
        .map(|(ordinal, target)| requested_event(&prepared, &target, ordinal))
        .collect::<Result<Vec<_>, _>>()?;
    append_events(
        store,
        &mut transaction,
        prepared.request().scope(),
        events,
        prepared.plan().request_digest().as_str(),
    )
    .await?;
    store
        .set_scope(&mut transaction, prepared.request().scope())
        .await
        .map_err(repository)?;
    transaction.commit().await.map_err(storage)?;
    Ok(RetentionPrepareOutcome::Prepared(prepared))
}

pub(crate) async fn finalize(
    store: &PostgresEventStore,
    prepared: PreparedRetention,
    receipts: Vec<RevocationReceipt>,
    completed_at: PersistedTimestamp,
    authentication_tag: graphhelm_events::AuthenticationTag,
) -> Result<FinalizedRetention, RetentionError> {
    let candidate = FinalizedRetention::new(
        prepared.clone(),
        receipts.clone(),
        completed_at.clone(),
        authentication_tag.clone(),
    )
    .map_err(|_| RetentionError::Integrity)?;
    verify_finalized(store, &candidate).await?;
    let mut transaction = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut transaction).await?;
    store
        .set_scope(&mut transaction, prepared.request().scope())
        .await
        .map_err(repository)?;
    let existing = load_by_idempotency(&mut transaction, prepared.request())
        .await?
        .ok_or(RetentionError::Conflict)?;
    if existing != prepared {
        return Err(RetentionError::Integrity);
    }
    if let Some(finalized) = load_finalized(&mut transaction, &existing).await? {
        verify_finalized(store, &finalized).await?;
        transaction.commit().await.map_err(storage)?;
        return Ok(finalized);
    }
    let scoped = scope::parts(prepared.request().scope());
    let delay = i64::try_from(prepared.request().policy().cleanup_delay_seconds())
        .map_err(|_| RetentionError::LimitExceeded)?;
    let eligible_at = completed_at
        .as_datetime()
        .checked_add_signed(TimeDelta::seconds(delay))
        .ok_or(RetentionError::LimitExceeded)?;
    for ((ordinal, target), receipt) in prepared.resolved_targets().enumerate().zip(&receipts) {
        let provider_idempotency = provider_revocation_idempotency_key(
            prepared.request().scope(),
            prepared.request().operation_id(),
            target.evidence_id(),
        );
        if receipt.handle() != target.key_handle_id().as_str()
            || receipt.idempotency_key() != provider_idempotency.as_str()
            || receipt.epoch() < prepared.provider_epoch()
        {
            return Err(RetentionError::Integrity);
        }
        let target_update = sqlx::query("UPDATE public.graphhelm_retention_targets SET provider_receipt_epoch=$6,provider_receipt_key_id=$7,provider_receipt_algorithm=$8,provider_receipt_tag=$9 WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND operation_id=$4 AND ordinal=$5 AND provider_receipt_epoch IS NULL")
            .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(prepared.request().operation_id().as_str()).bind(i32::try_from(ordinal).map_err(|_| RetentionError::LimitExceeded)?)
            .bind(i64_of(receipt.epoch())?).bind(receipt.authentication_tag().key_id()).bind(receipt.authentication_tag().algorithm()).bind(receipt.authentication_tag().bytes())
            .execute(&mut *transaction).await.map_err(constraint)?;
        if target_update.rows_affected() != 1 {
            return Err(RetentionError::Conflict);
        }
        sqlx::query("INSERT INTO public.graphhelm_evidence_tombstones(workspace_id,project_id,execution_id,evidence_id,operation_id,ciphertext_sha256,classification,retention_class,prior_state,policy_id,policy_version,authority,reason_code,requested_at,completed_at,provider_epoch) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
            .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(target.evidence_id().as_str()).bind(prepared.request().operation_id().as_str()).bind(target.ciphertext_sha256().as_str())
            .bind(sensitivity_name(target.classification())).bind(prepared.request().policy().retention_class()).bind(prior_name(target.prior_availability()))
            .bind(prepared.request().policy().id().as_str()).bind(prepared.request().policy().version().as_str()).bind(prepared.request().authority().id().as_str()).bind(prepared.request().reason_code().as_str())
            .bind(timestamp_wire(prepared.prepared_at())).bind(timestamp_wire(&completed_at)).bind(i64_of(receipt.epoch())?).execute(&mut *transaction).await.map_err(constraint)?;
        let evidence_update = sqlx::query("UPDATE public.graphhelm_evidence SET state='erased',cleanup_eligible_at=$2 WHERE evidence_id=$1 AND state='erasure_pending'")
            .bind(target.evidence_id().as_str()).bind(eligible_at).execute(&mut *transaction).await.map_err(constraint)?;
        if evidence_update.rows_affected() != 1 {
            return Err(RetentionError::Conflict);
        }
    }
    let events = prepared
        .resolved_targets()
        .zip(&receipts)
        .enumerate()
        .map(|(ordinal, (target, receipt))| {
            completed_event(&prepared, &target, receipt, &completed_at, ordinal)
        })
        .collect::<Result<Vec<_>, _>>()?;
    append_events(
        store,
        &mut transaction,
        prepared.request().scope(),
        events,
        prepared.plan().request_digest().as_str(),
    )
    .await?;
    store
        .set_scope(&mut transaction, prepared.request().scope())
        .await
        .map_err(repository)?;
    let operation_update = sqlx::query("UPDATE public.graphhelm_retention_operations SET state='finalized',completed_at=$5,finalized_key_id=$6,finalized_algorithm=$7,finalized_tag=$8 WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND operation_id=$4 AND state='prepared'")
        .bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(prepared.request().operation_id().as_str()).bind(timestamp_wire(&completed_at))
        .bind(authentication_tag.key_id()).bind(authentication_tag.algorithm()).bind(authentication_tag.bytes()).execute(&mut *transaction).await.map_err(constraint)?;
    if operation_update.rows_affected() != 1 {
        return Err(RetentionError::Conflict);
    }
    transaction.commit().await.map_err(storage)?;
    FinalizedRetention::new(prepared, receipts, completed_at, authentication_tag)
}

pub(crate) async fn pending(
    store: &PostgresEventStore,
    scope_value: RepositoryScope,
    limit: u32,
) -> Result<Vec<PreparedRetention>, RetentionError> {
    if limit == 0 || limit > 10_000 {
        return Err(RetentionError::LimitExceeded);
    }
    let mut tx = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut tx).await?;
    store
        .set_scope(&mut tx, &scope_value)
        .await
        .map_err(repository)?;
    let rows=sqlx::query("SELECT operation_id FROM public.graphhelm_retention_operations WHERE state='prepared' ORDER BY requested_at,operation_id LIMIT $1").bind(i64::from(limit)).fetch_all(&mut *tx).await.map_err(storage)?;
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("operation_id").map_err(decode)?;
        let prepared = load_operation(&mut tx, &id)
            .await?
            .ok_or(RetentionError::Integrity)?;
        verify_prepared(store, &prepared).await?;
        output.push(prepared)
    }
    tx.commit().await.map_err(storage)?;
    Ok(output)
}

pub(crate) async fn change_legal_hold(
    store: &PostgresEventStore,
    change: LegalHoldChange,
) -> Result<LegalHoldReceipt, RetentionError> {
    verify_legal_hold(store, &change).await?;
    let mut tx = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut tx).await?;
    store
        .set_scope(&mut tx, change.scope())
        .await
        .map_err(repository)?;
    let state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM public.graphhelm_evidence WHERE evidence_id=$1 FOR UPDATE",
    )
    .bind(change.evidence_id().as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(storage)?;
    let state = state.ok_or(RetentionError::Ineligible)?;
    if change.placed() && matches!(state.as_str(), "erasure_pending" | "erased") {
        return Err(RetentionError::Conflict);
    }
    let scoped = scope::parts(change.scope());
    let existing=sqlx::query("SELECT evidence_id,authority,reason_code,placed,changed_at,authentication_key_id,authentication_algorithm,authentication_tag FROM public.graphhelm_legal_holds WHERE hold_id=$1 ORDER BY placed DESC").bind(change.hold_id().as_str()).fetch_all(&mut *tx).await.map_err(storage)?;
    for row in &existing {
        let stored = legal_hold_from_row(change.scope().clone(), change.hold_id().clone(), row)?;
        verify_legal_hold(store, &stored).await?;
        if stored.evidence_id() != change.evidence_id()
            || stored.authority() != change.authority()
            || stored.reason_code() != change.reason_code()
        {
            return Err(RetentionError::Conflict);
        }
        if stored.placed() == change.placed() {
            if stored != change {
                return Err(RetentionError::Conflict);
            }
            tx.commit().await.map_err(storage)?;
            return Ok(LegalHoldReceipt::new(change));
        }
    }
    let has_placement = existing.iter().any(|row| row.get::<bool, _>("placed"));
    if change.placed() == has_placement {
        return Err(RetentionError::Conflict);
    }
    let revision = sqlx::query("UPDATE public.graphhelm_evidence SET hold_revision=hold_revision+1 WHERE evidence_id=$1 AND state NOT IN ('erasure_pending','erased')")
        .bind(change.evidence_id().as_str())
        .execute(&mut *tx)
        .await
        .map_err(constraint)?;
    if revision.rows_affected() != 1 {
        return Err(RetentionError::Conflict);
    }
    sqlx::query("INSERT INTO public.graphhelm_legal_holds(workspace_id,project_id,execution_id,hold_id,evidence_id,authority,reason_code,placed,changed_at,authentication_key_id,authentication_algorithm,authentication_tag) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(change.hold_id().as_str()).bind(change.evidence_id().as_str()).bind(change.authority().as_str()).bind(change.reason_code().as_str()).bind(change.placed()).bind(timestamp_wire(change.changed_at())).bind(change.authentication_tag().key_id()).bind(change.authentication_tag().algorithm()).bind(change.authentication_tag().bytes()).execute(&mut *tx).await.map_err(constraint)?;
    let event = hold_event(&change)?;
    append_events(
        store,
        &mut tx,
        change.scope(),
        vec![event],
        change.hold_id().as_str(),
    )
    .await?;
    store
        .set_scope(&mut tx, change.scope())
        .await
        .map_err(repository)?;
    tx.commit().await.map_err(storage)?;
    Ok(LegalHoldReceipt::new(change))
}

pub(crate) async fn cleanup(
    store: &PostgresEventStore,
    request: CleanupRequest,
) -> Result<CleanupReceipt, RetentionError> {
    let mut tx = store.pool().begin().await.map_err(storage)?;
    repeatable(&mut tx).await?;
    store
        .set_scope(&mut tx, request.scope())
        .await
        .map_err(repository)?;
    let performed_at: DateTime<Utc> = sqlx::query_scalar("SELECT CURRENT_TIMESTAMP")
        .fetch_one(&mut *tx)
        .await
        .map_err(storage)?;
    let deleted_at =
        PersistedTimestamp::from_datetime(performed_at).map_err(|_| RetentionError::Integrity)?;
    let scoped = scope::parts(request.scope());
    let ids = request
        .evidence_ids()
        .iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>();
    let request_digest = graphhelm_events::cleanup_request_digest(&request);
    let previous=sqlx::query("SELECT operation_id,request_digest,evidence_id,deleted_at FROM public.graphhelm_cleanup_receipts WHERE idempotency_key=$1 ORDER BY evidence_id COLLATE \"C\"").bind(request.idempotency_key().as_str()).fetch_all(&mut *tx).await.map_err(storage)?;
    if !previous.is_empty() {
        let found = previous
            .iter()
            .map(|row| row.get::<String, _>("evidence_id"))
            .collect::<Vec<_>>();
        if found != ids
            || previous
                .iter()
                .any(|row| row.get::<String, _>("request_digest") != request_digest.as_str())
        {
            return Err(RetentionError::Conflict);
        }
        let operation_id = OpaqueId::parse(
            previous[0]
                .try_get::<String, _>("operation_id")
                .map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?;
        let deleted_at = PersistedTimestamp::parse(
            &previous[0]
                .try_get::<String, _>("deleted_at")
                .map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?;
        let operation_wire = operation_id.as_str();
        let deleted_wire = timestamp_wire(&deleted_at);
        if previous.iter().any(|row| {
            row.get::<String, _>("operation_id") != operation_wire
                || row.get::<String, _>("deleted_at") != deleted_wire
        }) {
            return Err(RetentionError::Integrity);
        }
        let evidence_rows = sqlx::query("SELECT record,ciphertext_deleted_at FROM public.graphhelm_evidence WHERE evidence_id=ANY($1) ORDER BY evidence_id COLLATE \"C\" FOR UPDATE")
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(storage)?;
        if evidence_rows.len() != ids.len() {
            return Err(RetentionError::Integrity);
        }
        for row in &evidence_rows {
            let evidence_deleted_at = row
                .try_get::<Option<DateTime<Utc>>, _>("ciphertext_deleted_at")
                .map_err(decode)?
                .ok_or(RetentionError::Integrity)?;
            if row
                .try_get::<serde_json::Value, _>("record")
                .map_err(decode)?
                != serde_json::json!({})
                || evidence_deleted_at.timestamp_micros()
                    != deleted_at.as_datetime().timestamp_micros()
            {
                return Err(RetentionError::Integrity);
            }
        }
        tx.commit().await.map_err(storage)?;
        return CleanupReceipt::new(operation_id, request.evidence_ids().to_vec(), deleted_at);
    }
    let rows=sqlx::query("SELECT e.evidence_id,e.state,e.cleanup_eligible_at,t.operation_id,t.ciphertext_sha256,t.classification,t.retention_class,t.prior_state FROM public.graphhelm_evidence e JOIN public.graphhelm_evidence_tombstones t USING(workspace_id,project_id,execution_id,evidence_id) WHERE e.evidence_id=ANY($1) ORDER BY e.evidence_id COLLATE \"C\" FOR UPDATE OF e").bind(&ids).fetch_all(&mut *tx).await.map_err(storage)?;
    let held_ids = held_evidence_ids(store, &mut tx, &ids).await?;
    if rows.len() != ids.len() {
        return Err(RetentionError::Ineligible);
    }
    let mut observed: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let state: String = row.try_get("state").map_err(decode)?;
        let eligible: Option<DateTime<Utc>> = row.try_get("cleanup_eligible_at").map_err(decode)?;
        let held = held_ids.contains(&row.try_get::<String, _>("evidence_id").map_err(decode)?);
        if state != "erased" || held || eligible.is_none_or(|at| at > performed_at) {
            return Err(if held {
                RetentionError::LegalHold
            } else {
                RetentionError::Ineligible
            });
        }
        let operation_id = row.try_get::<String, _>("operation_id").map_err(decode)?;
        let prepared = load_operation(&mut tx, &operation_id)
            .await?
            .ok_or(RetentionError::Integrity)?;
        let finalized = load_finalized(&mut tx, &prepared)
            .await?
            .ok_or(RetentionError::Integrity)?;
        verify_finalized(store, &finalized).await?;
        let evidence_id = row.try_get::<String, _>("evidence_id").map_err(decode)?;
        let target = prepared
            .resolved_targets()
            .find(|target| target.evidence_id().as_str() == evidence_id)
            .ok_or(RetentionError::Integrity)?;
        let expected_eligible = finalized
            .completed_at()
            .as_datetime()
            .checked_add_signed(TimeDelta::seconds(
                i64::try_from(prepared.request().policy().cleanup_delay_seconds())
                    .map_err(|_| RetentionError::LimitExceeded)?,
            ))
            .ok_or(RetentionError::Integrity)?;
        if eligible.map(|value| value.timestamp_micros())
            != Some(expected_eligible.timestamp_micros())
            || row
                .try_get::<String, _>("ciphertext_sha256")
                .map_err(decode)?
                != target.ciphertext_sha256().as_str()
            || row.try_get::<String, _>("classification").map_err(decode)?
                != sensitivity_name(target.classification())
            || row
                .try_get::<String, _>("retention_class")
                .map_err(decode)?
                != prepared.request().policy().retention_class()
            || row.try_get::<String, _>("prior_state").map_err(decode)?
                != prior_name(target.prior_availability())
        {
            return Err(RetentionError::Integrity);
        }
        observed.push((
            evidence_id.clone(),
            row.try_get::<String, _>("ciphertext_sha256")
                .map_err(decode)?,
        ));
    }
    let digests = align_digests(&ids, &observed)?;
    for ((id, digest), ordinal) in ids.iter().zip(&digests).zip(0..) {
        let cleanup_update = sqlx::query("UPDATE public.graphhelm_evidence SET record='{}'::jsonb,ciphertext_deleted_at=$2 WHERE evidence_id=$1 AND ciphertext_deleted_at IS NULL").bind(id).bind(performed_at).execute(&mut *tx).await.map_err(constraint)?;
        if cleanup_update.rows_affected() != 1 {
            return Err(RetentionError::Conflict);
        }
        sqlx::query("INSERT INTO public.graphhelm_cleanup_receipts(workspace_id,project_id,execution_id,operation_id,idempotency_key,request_digest,evidence_id,ciphertext_sha256,requested_at,deleted_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(request.operation_id().as_str()).bind(request.idempotency_key().as_str()).bind(request_digest.as_str()).bind(id).bind(digest).bind(timestamp_wire(request.requested_at())).bind(timestamp_wire(&deleted_at)).execute(&mut *tx).await.map_err(constraint)?;
        let _ = ordinal;
    }
    let events = request
        .evidence_ids()
        .iter()
        .zip(&digests)
        .enumerate()
        .map(|(ordinal, (id, digest))| deleted_event(&request, id, digest, &deleted_at, ordinal))
        .collect::<Result<Vec<_>, _>>()?;
    append_events(
        store,
        &mut tx,
        request.scope(),
        events,
        request_digest.as_str(),
    )
    .await?;
    store
        .set_scope(&mut tx, request.scope())
        .await
        .map_err(repository)?;
    tx.commit().await.map_err(storage)?;
    CleanupReceipt::new(
        request.operation_id().clone(),
        request.evidence_ids().to_vec(),
        deleted_at,
    )
}

async fn repeatable(tx: &mut Transaction<'_, Postgres>) -> Result<(), RetentionError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    Ok(())
}

async fn load_by_idempotency(
    tx: &mut Transaction<'_, Postgres>,
    request: &RetentionRequest,
) -> Result<Option<PreparedRetention>, RetentionError> {
    let row=sqlx::query("SELECT operation_id,request_digest FROM public.graphhelm_retention_operations WHERE idempotency_key=$1").bind(request.idempotency_key().as_str()).fetch_optional(&mut **tx).await.map_err(storage)?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String, _>("request_digest").map_err(decode)?
        != retention_request_digest(request).as_str()
    {
        return Err(RetentionError::Conflict);
    }
    let id: String = row.try_get("operation_id").map_err(decode)?;
    let prepared = load_operation(tx, &id)
        .await?
        .ok_or(RetentionError::Integrity)?;
    if prepared.request() != request {
        return Err(RetentionError::Conflict);
    }
    Ok(Some(prepared))
}

async fn load_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
) -> Result<Option<PreparedRetention>, RetentionError> {
    let row=sqlx::query("SELECT operation_id,idempotency_key,request_digest,policy_id,policy_version,authority,authority_key_id,authority_algorithm,authority_tag,reason_code,evaluated_at,requested_at,provider_epoch,prepared_key_id,prepared_algorithm,prepared_tag FROM public.graphhelm_retention_operations WHERE operation_id=$1").bind(operation_id).fetch_optional(&mut **tx).await.map_err(storage)?;
    let Some(row) = row else { return Ok(None) };
    let policy_row=sqlx::query("SELECT retention_class,minimum_age_seconds,cleanup_delay_seconds FROM public.graphhelm_retention_policies WHERE policy_id=$1 AND policy_version=$2").bind(row.try_get::<String,_>("policy_id").map_err(decode)?).bind(row.try_get::<String,_>("policy_version").map_err(decode)?).fetch_one(&mut **tx).await.map_err(storage)?;
    let target_rows=sqlx::query("SELECT evidence_id,key_handle_id,ciphertext_sha256,classification,prior_state FROM public.graphhelm_retention_targets WHERE operation_id=$1 ORDER BY ordinal").bind(operation_id).fetch_all(&mut **tx).await.map_err(storage)?;
    let scope_value = current_scope(tx).await?;
    let policy = graphhelm_events::RetentionPolicy::new(
        OpaqueId::parse(row.try_get::<String, _>("policy_id").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?,
        graphhelm_protocols::SemanticVersion::parse(
            row.try_get::<String, _>("policy_version").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        policy_row
            .try_get::<String, _>("retention_class")
            .map_err(decode)?,
        u64_of(policy_row.try_get("minimum_age_seconds").map_err(decode)?)?,
        u64_of(
            policy_row
                .try_get("cleanup_delay_seconds")
                .map_err(decode)?,
        )?,
    )?;
    let request_targets = target_rows
        .iter()
        .map(|r| {
            graphhelm_events::RetentionTarget::new(
                graphhelm_protocols::EvidenceId::parse(r.get::<String, _>("evidence_id"))
                    .map_err(|_| RetentionError::Integrity)?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let authority = graphhelm_events::RetentionAuthority::new(
        OpaqueId::parse(row.try_get::<String, _>("authority").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?,
        scope_value.clone(),
        policy.id().clone(),
        policy.version().clone(),
        graphhelm_events::AuthenticationTag::new(
            row.try_get::<String, _>("authority_key_id")
                .map_err(decode)?,
            &row.try_get::<String, _>("authority_algorithm")
                .map_err(decode)?,
            row.try_get("authority_tag").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
    );
    let request = RetentionRequest::new(
        scope_value,
        OpaqueId::parse(operation_id).map_err(|_| RetentionError::Integrity)?,
        OpaqueId::parse(
            row.try_get::<String, _>("idempotency_key")
                .map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        policy,
        authority,
        graphhelm_protocols::SafeCode::parse(
            row.try_get::<String, _>("reason_code").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        request_targets,
    )?;
    let plan_targets = target_rows
        .into_iter()
        .map(|r| {
            RetentionPlanTarget::new(
                graphhelm_protocols::EvidenceId::parse(r.get::<String, _>("evidence_id"))
                    .map_err(|_| RetentionError::Integrity)?,
                OpaqueId::parse(r.get::<String, _>("key_handle_id"))
                    .map_err(|_| RetentionError::Integrity)?,
                graphhelm_protocols::RawSha256::parse(r.get::<String, _>("ciphertext_sha256"))
                    .map_err(|_| RetentionError::Integrity)?,
                parse_sensitivity(&r.get::<String, _>("classification"))?,
                prior_state(&r.get::<String, _>("prior_state"))?,
                None,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let evaluated_at =
        PersistedTimestamp::parse(&row.try_get::<String, _>("evaluated_at").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?;
    let at = PersistedTimestamp::parse(&row.try_get::<String, _>("requested_at").map_err(decode)?)
        .map_err(|_| RetentionError::Integrity)?;
    let plan = RetentionPlan::new(
        request.operation_id().clone(),
        graphhelm_protocols::RawSha256::parse(
            row.try_get::<String, _>("request_digest").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        evaluated_at,
        plan_targets,
    )?;
    PreparedRetention::new(
        request,
        plan,
        at,
        u64_of(row.try_get("provider_epoch").map_err(decode)?)?,
        graphhelm_events::AuthenticationTag::new(
            row.try_get::<String, _>("prepared_key_id")
                .map_err(decode)?,
            &row.try_get::<String, _>("prepared_algorithm")
                .map_err(decode)?,
            row.try_get("prepared_tag").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
    )
    .map(Some)
}

async fn current_scope(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<RepositoryScope, RetentionError> {
    let w: String = sqlx::query_scalar("SELECT current_setting('graphhelm.workspace_id')")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?;
    let p: String = sqlx::query_scalar("SELECT current_setting('graphhelm.project_id')")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?;
    let e: String = sqlx::query_scalar("SELECT current_setting('graphhelm.execution_id')")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage)?;
    Ok(RepositoryScope::new(
        graphhelm_protocols::WorkspaceId::parse(w).map_err(|_| RetentionError::Integrity)?,
        graphhelm_protocols::ProjectId::parse(p).map_err(|_| RetentionError::Integrity)?,
        if e.is_empty() {
            None
        } else {
            Some(
                graphhelm_protocols::ExecutionId::parse(e)
                    .map_err(|_| RetentionError::Integrity)?,
            )
        },
    ))
}

async fn load_finalized(
    tx: &mut Transaction<'_, Postgres>,
    prepared: &PreparedRetention,
) -> Result<Option<FinalizedRetention>, RetentionError> {
    let row=sqlx::query("SELECT state,completed_at,finalized_key_id,finalized_algorithm,finalized_tag FROM public.graphhelm_retention_operations WHERE operation_id=$1").bind(prepared.request().operation_id().as_str()).fetch_one(&mut **tx).await.map_err(storage)?;
    if row.try_get::<String, _>("state").map_err(decode)? != "finalized" {
        return Ok(None);
    }
    let receipt_rows=sqlx::query("SELECT evidence_id,key_handle_id,provider_receipt_epoch,provider_receipt_key_id,provider_receipt_algorithm,provider_receipt_tag FROM public.graphhelm_retention_targets WHERE operation_id=$1 ORDER BY ordinal").bind(prepared.request().operation_id().as_str()).fetch_all(&mut **tx).await.map_err(storage)?;
    let receipts = receipt_rows
        .into_iter()
        .map(|r| {
            let evidence_id =
                graphhelm_protocols::EvidenceId::parse(r.get::<String, _>("evidence_id"))
                    .map_err(|_| RetentionError::Integrity)?;
            let provider_idempotency = provider_revocation_idempotency_key(
                prepared.request().scope(),
                prepared.request().operation_id(),
                &evidence_id,
            );
            RevocationReceipt::new(
                r.get::<String, _>("key_handle_id"),
                provider_idempotency.as_str(),
                u64_of(r.try_get("provider_receipt_epoch").map_err(decode)?)?,
                graphhelm_events::AuthenticationTag::new(
                    r.try_get::<String, _>("provider_receipt_key_id")
                        .map_err(decode)?,
                    &r.try_get::<String, _>("provider_receipt_algorithm")
                        .map_err(decode)?,
                    r.try_get("provider_receipt_tag").map_err(decode)?,
                )
                .map_err(|_| RetentionError::Integrity)?,
            )
            .map_err(|_| RetentionError::Integrity)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let at = PersistedTimestamp::parse(&row.try_get::<String, _>("completed_at").map_err(decode)?)
        .map_err(|_| RetentionError::Integrity)?;
    let tag = graphhelm_events::AuthenticationTag::new(
        row.try_get::<String, _>("finalized_key_id")
            .map_err(decode)?,
        &row.try_get::<String, _>("finalized_algorithm")
            .map_err(decode)?,
        row.try_get("finalized_tag").map_err(decode)?,
    )
    .map_err(|_| RetentionError::Integrity)?;
    FinalizedRetention::new(prepared.clone(), receipts, at, tag).map(Some)
}

async fn append_events(
    store: &PostgresEventStore,
    tx: &mut Transaction<'_, Postgres>,
    evidence_scope: &RepositoryScope,
    events: Vec<NewEvent>,
    request_digest: &str,
) -> Result<(), RetentionError> {
    let event_count = u64::try_from(events.len()).map_err(|_| RetentionError::LimitExceeded)?;
    let project_scope = RepositoryScope::new(
        evidence_scope.workspace_id().clone(),
        evidence_scope.project_id().clone(),
        None,
    );
    store
        .set_scope(tx, &project_scope)
        .await
        .map_err(repository)?;
    let scoped = scope::parts(&project_scope);
    let stream = OpaqueId::parse(RETENTION_STREAM).map_err(|_| RetentionError::Integrity)?;
    let genesis = crate::integrity::authenticate_stream_head(
        store,
        &project_scope,
        stream.as_str(),
        1,
        GENESIS_HASH,
    )
    .await
    .map_err(repository)?;
    sqlx::query("INSERT INTO public.graphhelm_streams(workspace_id,project_id,execution_id,stream_id,next_sequence,last_event_hash,head_key_id,head_key_version,head_provider_epoch,head_algorithm,head_tag,head_canonical_sha256) VALUES($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(stream.as_str()).bind(GENESIS_HASH).bind(genesis.key_id).bind(genesis.key_version).bind(i64_of(genesis.provider_epoch)?).bind(genesis.algorithm).bind(genesis.tag).bind(genesis.canonical_sha256).execute(&mut **tx).await.map_err(storage)?;
    let head=sqlx::query("SELECT next_sequence,last_event_hash FROM public.graphhelm_streams WHERE stream_id=$1 FOR UPDATE").bind(stream.as_str()).fetch_one(&mut **tx).await.map_err(storage)?;
    let next = u64_of(head.try_get("next_sequence").map_err(decode)?)?;
    let mut previous: String = head.try_get("last_event_hash").map_err(decode)?;
    crate::integrity::verify_locked_stream(
        store,
        tx,
        &project_scope,
        stream.as_str(),
        next,
        &previous,
    )
    .await
    .map_err(repository)?;
    for (offset, event) in events.into_iter().enumerate() {
        let sequence = next
            .checked_add(u64::try_from(offset).map_err(|_| RetentionError::LimitExceeded)?)
            .ok_or(RetentionError::LimitExceeded)?;
        let occurred = event_time(&event)?;
        let event_id = OpaqueId::parse(format!("evt-{}", uuid::Uuid::new_v4().simple()))
            .map_err(|_| RetentionError::Storage)?;
        let placeholder = EventHash::parse(GENESIS_HASH).map_err(|_| RetentionError::Integrity)?;
        let mut envelope = EventEnvelope::new(
            event_id,
            project_scope.clone(),
            stream.clone(),
            sequence,
            occurred,
            event,
            EventHash::parse(previous.clone()).map_err(|_| RetentionError::Integrity)?,
            placeholder,
        );
        validate_envelope_content(&envelope).map_err(repository)?;
        let hash = compute_event_hash(&envelope, &previous).map_err(repository)?;
        envelope.event_hash =
            EventHash::parse(hash.clone()).map_err(|_| RetentionError::Integrity)?;
        validate_envelope(&envelope).map_err(repository)?;
        let json = serde_json::to_value(&envelope).map_err(|_| RetentionError::Integrity)?;
        sqlx::query("INSERT INTO public.graphhelm_idempotency(workspace_id,project_id,execution_id,stream_id,idempotency_key,request_digest,first_sequence,event_count) VALUES($1,$2,$3,$4,$5,$6,$7,1)").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(stream.as_str()).bind(envelope.idempotency_key.as_str()).bind(request_digest).bind(i64_of(sequence)?).execute(&mut **tx).await.map_err(constraint)?;
        sqlx::query("INSERT INTO public.graphhelm_events(workspace_id,project_id,execution_id,stream_id,sequence,event_id,idempotency_key,previous_hash,event_hash,request_digest,envelope) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(stream.as_str()).bind(i64_of(sequence)?).bind(envelope.event_id.as_str()).bind(envelope.idempotency_key.as_str()).bind(envelope.previous_hash.as_str()).bind(envelope.event_hash.as_str()).bind(request_digest).bind(json).execute(&mut **tx).await.map_err(constraint)?;
        previous = hash;
    }
    let final_next = next
        .checked_add(event_count)
        .ok_or(RetentionError::LimitExceeded)?;
    let proof = crate::integrity::authenticate_stream_head(
        store,
        &project_scope,
        stream.as_str(),
        final_next,
        &previous,
    )
    .await
    .map_err(repository)?;
    sqlx::query("UPDATE public.graphhelm_streams SET next_sequence=$5,last_event_hash=$6,head_key_id=$7,head_key_version=$8,head_provider_epoch=$9,head_algorithm=$10,head_tag=$11,head_canonical_sha256=$12 WHERE workspace_id=$1 AND project_id=$2 AND execution_id=$3 AND stream_id=$4").bind(scoped.workspace).bind(scoped.project).bind(scoped.execution).bind(stream.as_str()).bind(i64_of(final_next)?).bind(previous).bind(proof.key_id).bind(proof.key_version).bind(i64_of(proof.provider_epoch)?).bind(proof.algorithm).bind(proof.tag).bind(proof.canonical_sha256).execute(&mut **tx).await.map_err(storage)?;
    Ok(())
}

fn requested_event(
    prepared: &PreparedRetention,
    target: &PreparedRetentionTarget<'_>,
    ordinal: usize,
) -> Result<NewEvent, RetentionError> {
    Ok(NewEvent::new(
        event_key(
            prepared.request().scope(),
            prepared.request().operation_id().as_str(),
            "requested",
            ordinal,
        )?,
        actor()?,
        Sensitivity::Restricted,
        EventKind::EvidenceErasureRequested(EvidenceErasureRequested {
            evidence_scope: prepared.request().scope().clone(),
            operation_id: prepared.request().operation_id().clone(),
            evidence_id: target.evidence_id().clone(),
            key_handle_id: target.key_handle_id().clone(),
            retention_policy_id: prepared.request().policy().id().clone(),
            retention_policy_version: prepared.request().policy().version().clone(),
            authority: prepared.request().authority().id().clone(),
            reason_code: prepared.request().reason_code().clone(),
            prior_state: protocol_prior(target.prior_availability()),
            state: ErasurePendingState::ErasurePending,
            requested_at: prepared.prepared_at().clone(),
        }),
        vec![],
        vec![],
    ))
}
fn completed_event(
    prepared: &PreparedRetention,
    target: &PreparedRetentionTarget<'_>,
    receipt: &RevocationReceipt,
    at: &PersistedTimestamp,
    ordinal: usize,
) -> Result<NewEvent, RetentionError> {
    Ok(NewEvent::new(
        event_key(
            prepared.request().scope(),
            prepared.request().operation_id().as_str(),
            "completed",
            ordinal,
        )?,
        actor()?,
        Sensitivity::Restricted,
        EventKind::EvidenceErasureCompleted(EvidenceErasureCompleted {
            evidence_scope: prepared.request().scope().clone(),
            operation_id: prepared.request().operation_id().clone(),
            evidence_id: target.evidence_id().clone(),
            key_handle_id: target.key_handle_id().clone(),
            retention_policy_id: prepared.request().policy().id().clone(),
            retention_policy_version: prepared.request().policy().version().clone(),
            authority: prepared.request().authority().id().clone(),
            reason_code: prepared.request().reason_code().clone(),
            ciphertext_sha256: target.ciphertext_sha256().clone(),
            provider_receipt_id: event_key(
                prepared.request().scope(),
                prepared.request().operation_id().as_str(),
                "receipt",
                ordinal,
            )?,
            provider_epoch: receipt.epoch(),
            prior_state: PendingPriorState::ErasurePending,
            state: ErasedState::Erased,
            requested_at: prepared.prepared_at().clone(),
            completed_at: at.clone(),
        }),
        vec![],
        vec![],
    ))
}
fn hold_event(change: &LegalHoldChange) -> Result<NewEvent, RetentionError> {
    Ok(NewEvent::new(
        event_key(
            change.scope(),
            change.hold_id().as_str(),
            if change.placed() {
                "hold-placed"
            } else {
                "hold-released"
            },
            0,
        )?,
        actor()?,
        Sensitivity::Restricted,
        EventKind::EvidenceLegalHoldChanged(EvidenceLegalHoldChanged {
            evidence_scope: change.scope().clone(),
            hold_id: change.hold_id().clone(),
            evidence_id: change.evidence_id().clone(),
            authority: change.authority().clone(),
            reason_code: change.reason_code().clone(),
            state: if change.placed() {
                LegalHoldState::Placed
            } else {
                LegalHoldState::Released
            },
            changed_at: change.changed_at().clone(),
        }),
        vec![],
        vec![],
    ))
}
fn deleted_event(
    request: &CleanupRequest,
    id: &graphhelm_protocols::EvidenceId,
    digest: &str,
    deleted_at: &PersistedTimestamp,
    ordinal: usize,
) -> Result<NewEvent, RetentionError> {
    Ok(NewEvent::new(
        event_key(
            request.scope(),
            request.operation_id().as_str(),
            "deleted",
            ordinal,
        )?,
        actor()?,
        Sensitivity::Restricted,
        EventKind::EvidenceCiphertextDeleted(EvidenceCiphertextDeleted {
            evidence_scope: request.scope().clone(),
            operation_id: request.operation_id().clone(),
            evidence_id: id.clone(),
            ciphertext_sha256: graphhelm_protocols::RawSha256::parse(digest)
                .map_err(|_| RetentionError::Integrity)?,
            deleted_at: deleted_at.clone(),
        }),
        vec![],
        vec![],
    ))
}
fn event_time(event: &NewEvent) -> Result<PersistedTimestamp, RetentionError> {
    match &event.kind {
        EventKind::EvidenceErasureRequested(v) => Ok(v.requested_at.clone()),
        EventKind::EvidenceErasureCompleted(v) => Ok(v.completed_at.clone()),
        EventKind::EvidenceCiphertextDeleted(v) => Ok(v.deleted_at.clone()),
        EventKind::EvidenceLegalHoldChanged(v) => Ok(v.changed_at.clone()),
        _ => Err(RetentionError::Integrity),
    }
}
fn event_key(
    scope: &RepositoryScope,
    operation: &str,
    kind: &str,
    ordinal: usize,
) -> Result<OpaqueId, RetentionError> {
    let hash = sha2::Sha256::digest(format!(
        "{}:{}:{}:{operation}:{kind}:{ordinal}",
        scope.workspace_id().as_str(),
        scope.project_id().as_str(),
        scope.execution_id().map_or("", |id| id.as_str()),
    ));
    OpaqueId::parse(format!("ret-{}", hex::encode(&hash[..12])))
        .map_err(|_| RetentionError::Integrity)
}
fn actor() -> Result<PersistedActor, RetentionError> {
    Ok(PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system.retention").map_err(|_| RetentionError::Integrity)?,
    ))
}
fn timestamp_wire(value: &PersistedTimestamp) -> String {
    value
        .as_datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}
async fn verify_authority(
    store: &PostgresEventStore,
    request: &RetentionRequest,
) -> Result<(), RetentionError> {
    let authority = request.authority();
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-authority",
                retention_authority_authentication_bytes(
                    authority.id(),
                    authority.scope(),
                    request.policy(),
                ),
                authority.authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)
}
async fn verify_prepared(
    store: &PostgresEventStore,
    prepared: &PreparedRetention,
) -> Result<(), RetentionError> {
    verify_authority(store, prepared.request()).await?;
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-prepared",
                prepared_authentication_bytes(prepared),
                prepared.authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)
}
fn legal_hold_from_row(
    scope: RepositoryScope,
    hold_id: OpaqueId,
    row: &sqlx::postgres::PgRow,
) -> Result<LegalHoldChange, RetentionError> {
    Ok(LegalHoldChange::new(
        scope,
        hold_id,
        graphhelm_protocols::EvidenceId::parse(
            row.try_get::<String, _>("evidence_id").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        OpaqueId::parse(row.try_get::<String, _>("authority").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?,
        graphhelm_protocols::SafeCode::parse(
            row.try_get::<String, _>("reason_code").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
        row.try_get("placed").map_err(decode)?,
        PersistedTimestamp::parse(&row.try_get::<String, _>("changed_at").map_err(decode)?)
            .map_err(|_| RetentionError::Integrity)?,
        graphhelm_events::AuthenticationTag::new(
            row.try_get::<String, _>("authentication_key_id")
                .map_err(decode)?,
            &row.try_get::<String, _>("authentication_algorithm")
                .map_err(decode)?,
            row.try_get("authentication_tag").map_err(decode)?,
        )
        .map_err(|_| RetentionError::Integrity)?,
    ))
}
async fn verify_legal_hold(
    store: &PostgresEventStore,
    change: &LegalHoldChange,
) -> Result<(), RetentionError> {
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-legal-hold",
                legal_hold_authentication_bytes(change),
                change.authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)
}
async fn held_evidence_ids(
    store: &PostgresEventStore,
    tx: &mut Transaction<'_, Postgres>,
    evidence_ids: &[&str],
) -> Result<BTreeSet<String>, RetentionError> {
    let scope_value = current_scope(tx).await?;
    let rows = sqlx::query("SELECT hold_id,evidence_id,authority,reason_code,placed,changed_at,authentication_key_id,authentication_algorithm,authentication_tag FROM public.graphhelm_legal_holds WHERE evidence_id=ANY($1) ORDER BY hold_id,placed DESC LIMIT $2")
        .bind(evidence_ids)
        .bind(i64::try_from(MAX_LEGAL_HOLD_HISTORY_ROWS + 1).map_err(|_| RetentionError::LimitExceeded)?)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage)?;
    if rows.len() > MAX_LEGAL_HOLD_HISTORY_ROWS {
        return Err(RetentionError::LimitExceeded);
    }
    let mut states: BTreeMap<String, (String, bool, bool)> = BTreeMap::new();
    for row in rows {
        let hold_id_text = row.try_get::<String, _>("hold_id").map_err(decode)?;
        let change = legal_hold_from_row(
            scope_value.clone(),
            OpaqueId::parse(&hold_id_text).map_err(|_| RetentionError::Integrity)?,
            &row,
        )?;
        verify_legal_hold(store, &change).await?;
        let entry = states
            .entry(hold_id_text)
            .or_insert_with(|| (change.evidence_id().as_str().to_owned(), false, false));
        if entry.0 != change.evidence_id().as_str() {
            return Err(RetentionError::Integrity);
        }
        if change.placed() {
            entry.1 = true;
        } else {
            entry.2 = true;
        }
    }
    if states
        .values()
        .any(|(_, placed, released)| *released && !*placed)
    {
        return Err(RetentionError::Integrity);
    }
    Ok(states
        .into_values()
        .filter_map(|(evidence_id, placed, released)| (placed && !released).then_some(evidence_id))
        .collect())
}
async fn verify_finalized(
    store: &PostgresEventStore,
    finalized: &FinalizedRetention,
) -> Result<(), RetentionError> {
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-prepared",
                prepared_authentication_bytes(finalized.prepared()),
                finalized.prepared().authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)?;
    for receipt in finalized.receipts() {
        store
            .key_provider()
            .verify(
                VerifyAuthenticationRequest::new(
                    "revocation-receipt",
                    revocation_receipt_authentication_bytes(receipt),
                    receipt.authentication_tag().clone(),
                )
                .map_err(map_key_error)?,
            )
            .await
            .map_err(map_key_error)?;
    }
    store
        .key_provider()
        .verify(
            VerifyAuthenticationRequest::new(
                "retention-finalized",
                finalized_authentication_bytes(
                    finalized.prepared(),
                    finalized.receipts(),
                    finalized.completed_at(),
                ),
                finalized.authentication_tag().clone(),
            )
            .map_err(map_key_error)?,
        )
        .await
        .map_err(map_key_error)?;
    let metadata = store
        .key_provider()
        .metadata()
        .await
        .map_err(map_key_error)?;
    if finalized
        .receipts()
        .iter()
        .any(|receipt| receipt.epoch() > metadata.current_revocation_epoch())
    {
        return Err(RetentionError::Integrity);
    }
    Ok(())
}

pub(crate) async fn verify_restored_retention(
    store: &PostgresEventStore,
) -> Result<(), RetentionError> {
    const MAX_SCOPES: usize = 10_000;
    const MAX_OPERATIONS: usize = 100_000;
    let retained_rows: i64 = sqlx::query_scalar(
        "SELECT (\
         (SELECT count(*) FROM public.graphhelm_retention_operations)+\
         (SELECT count(*) FROM public.graphhelm_retention_policies)+\
         (SELECT count(*) FROM public.graphhelm_retention_targets)+\
         (SELECT count(*) FROM public.graphhelm_legal_holds)+\
         (SELECT count(*) FROM public.graphhelm_evidence_tombstones)+\
         (SELECT count(*) FROM public.graphhelm_cleanup_receipts))::bigint",
    )
    .fetch_one(store.pool())
    .await
    .map_err(storage)?;
    if !(0..=1_000_000).contains(&retained_rows) {
        return Err(RetentionError::LimitExceeded);
    }
    let scopes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT workspace_id,project_id,execution_id FROM (\
         SELECT workspace_id,project_id,execution_id FROM public.graphhelm_retention_operations \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_retention_policies \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_retention_targets \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_legal_holds \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_evidence_tombstones \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_cleanup_receipts \
         UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_evidence WHERE state='erased'\
         ) scopes ORDER BY workspace_id COLLATE \"C\",project_id COLLATE \"C\",execution_id COLLATE \"C\" LIMIT 10001",
    )
    .fetch_all(store.pool())
    .await
    .map_err(storage)?;
    if scopes.len() > MAX_SCOPES {
        return Err(RetentionError::LimitExceeded);
    }
    let mut operation_count = 0_usize;
    for (workspace, project, execution) in scopes {
        let scope_value = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse(workspace)
                .map_err(|_| RetentionError::Integrity)?,
            graphhelm_protocols::ProjectId::parse(project)
                .map_err(|_| RetentionError::Integrity)?,
            if execution.is_empty() {
                None
            } else {
                Some(
                    graphhelm_protocols::ExecutionId::parse(execution)
                        .map_err(|_| RetentionError::Integrity)?,
                )
            },
        );
        let mut tx = store.pool().begin().await.map_err(storage)?;
        repeatable(&mut tx).await?;
        store
            .set_scope(&mut tx, &scope_value)
            .await
            .map_err(repository)?;
        let policies: Vec<(String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT policy_id,policy_version,retention_class,minimum_age_seconds,cleanup_delay_seconds \
             FROM public.graphhelm_retention_policies ORDER BY policy_id,policy_version LIMIT 100001",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(storage)?;
        if policies.len() > MAX_OPERATIONS {
            return Err(RetentionError::LimitExceeded);
        }
        for (policy_id, version, class, minimum_age, cleanup_delay) in policies {
            graphhelm_events::RetentionPolicy::new(
                OpaqueId::parse(policy_id).map_err(|_| RetentionError::Integrity)?,
                graphhelm_protocols::SemanticVersion::parse(version)
                    .map_err(|_| RetentionError::Integrity)?,
                class,
                u64_of(minimum_age)?,
                u64_of(cleanup_delay)?,
            )?;
        }
        let operations: Vec<(String,)> = sqlx::query_as(
            "SELECT operation_id FROM public.graphhelm_retention_operations \
             ORDER BY operation_id LIMIT 100001",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(storage)?;
        operation_count = operation_count
            .checked_add(operations.len())
            .ok_or(RetentionError::LimitExceeded)?;
        if operation_count > MAX_OPERATIONS {
            return Err(RetentionError::LimitExceeded);
        }
        for (operation_id,) in operations {
            let prepared = load_operation(&mut tx, &operation_id)
                .await?
                .ok_or(RetentionError::Integrity)?;
            verify_prepared(store, &prepared).await?;
            if let Some(finalized) = load_finalized(&mut tx, &prepared).await? {
                verify_finalized(store, &finalized).await?;
                verify_finalized_rows(&mut tx, &finalized).await?;
            } else {
                let invalid: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM public.graphhelm_retention_targets t \
                     LEFT JOIN public.graphhelm_evidence e USING(workspace_id,project_id,execution_id,evidence_id) \
                     WHERE t.operation_id=$1 AND (t.provider_receipt_epoch IS NOT NULL OR e.state<>'erasure_pending')",
                )
                .bind(&operation_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(storage)?;
                if invalid != 0 {
                    return Err(RetentionError::Integrity);
                }
            }
        }
        verify_restored_holds(store, &mut tx, &scope_value).await?;
        verify_restored_cleanup(store, &mut tx, &scope_value).await?;
        tx.commit().await.map_err(storage)?;
    }
    Ok(())
}

async fn verify_finalized_rows(
    tx: &mut Transaction<'_, Postgres>,
    finalized: &FinalizedRetention,
) -> Result<(), RetentionError> {
    let rows = sqlx::query(
        "SELECT t.evidence_id,t.ciphertext_sha256,t.classification,t.retention_class,t.prior_state,\
         t.policy_id,t.policy_version,t.authority,t.reason_code,t.requested_at,t.completed_at,t.provider_epoch,\
         e.state,e.cleanup_eligible_at \
         FROM public.graphhelm_evidence_tombstones t \
         JOIN public.graphhelm_evidence e USING(workspace_id,project_id,execution_id,evidence_id) \
         WHERE t.operation_id=$1 ORDER BY t.evidence_id COLLATE \"C\" LIMIT 10001",
    )
    .bind(finalized.prepared().request().operation_id().as_str())
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    let targets = finalized.prepared().resolved_targets().collect::<Vec<_>>();
    if rows.len() != targets.len() || rows.len() != finalized.receipts().len() {
        return Err(RetentionError::Integrity);
    }
    let cleanup_delay = i64::try_from(
        finalized
            .prepared()
            .request()
            .policy()
            .cleanup_delay_seconds(),
    )
    .map_err(|_| RetentionError::LimitExceeded)?;
    let eligible_at = finalized
        .completed_at()
        .as_datetime()
        .checked_add_signed(TimeDelta::seconds(cleanup_delay))
        .ok_or(RetentionError::Integrity)?;
    for ((row, target), receipt) in rows.iter().zip(targets).zip(finalized.receipts()) {
        let evidence_id = row.try_get::<String, _>("evidence_id").map_err(decode)?;
        let cleanup_eligible_at: Option<DateTime<Utc>> =
            row.try_get("cleanup_eligible_at").map_err(decode)?;
        if evidence_id != target.evidence_id().as_str()
            || row
                .try_get::<String, _>("ciphertext_sha256")
                .map_err(decode)?
                != target.ciphertext_sha256().as_str()
            || row.try_get::<String, _>("classification").map_err(decode)?
                != sensitivity_name(target.classification())
            || row
                .try_get::<String, _>("retention_class")
                .map_err(decode)?
                != finalized.prepared().request().policy().retention_class()
            || row.try_get::<String, _>("prior_state").map_err(decode)?
                != prior_name(target.prior_availability())
            || row.try_get::<String, _>("policy_id").map_err(decode)?
                != finalized.prepared().request().policy().id().as_str()
            || row.try_get::<String, _>("policy_version").map_err(decode)?
                != finalized.prepared().request().policy().version().as_str()
            || row.try_get::<String, _>("authority").map_err(decode)?
                != finalized.prepared().request().authority().id().as_str()
            || row.try_get::<String, _>("reason_code").map_err(decode)?
                != finalized.prepared().request().reason_code().as_str()
            || row.try_get::<String, _>("requested_at").map_err(decode)?
                != timestamp_wire(finalized.prepared().prepared_at())
            || row.try_get::<String, _>("completed_at").map_err(decode)?
                != timestamp_wire(finalized.completed_at())
            || u64_of(row.try_get("provider_epoch").map_err(decode)?)? != receipt.epoch()
            || row.try_get::<String, _>("state").map_err(decode)? != "erased"
            || cleanup_eligible_at.map(|value| value.timestamp_micros())
                != Some(eligible_at.timestamp_micros())
        {
            return Err(RetentionError::Integrity);
        }
    }
    Ok(())
}

async fn verify_restored_holds(
    store: &PostgresEventStore,
    tx: &mut Transaction<'_, Postgres>,
    scope_value: &RepositoryScope,
) -> Result<(), RetentionError> {
    let rows = sqlx::query(
        "SELECT hold_id,evidence_id,authority,reason_code,placed,changed_at,\
         authentication_key_id,authentication_algorithm,authentication_tag \
         FROM public.graphhelm_legal_holds ORDER BY hold_id,placed DESC LIMIT 20001",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    if rows.len() > MAX_LEGAL_HOLD_HISTORY_ROWS {
        return Err(RetentionError::LimitExceeded);
    }
    type HoldHistory = (String, Option<DateTime<Utc>>, Option<DateTime<Utc>>);
    let mut history: BTreeMap<String, HoldHistory> = BTreeMap::new();
    for row in rows {
        let hold_id = row.try_get::<String, _>("hold_id").map_err(decode)?;
        let change = legal_hold_from_row(
            scope_value.clone(),
            OpaqueId::parse(&hold_id).map_err(|_| RetentionError::Integrity)?,
            &row,
        )?;
        verify_legal_hold(store, &change).await?;
        let entry = history
            .entry(hold_id)
            .or_insert_with(|| (change.evidence_id().as_str().to_owned(), None, None));
        if entry.0 != change.evidence_id().as_str()
            || (change.placed() && entry.1.is_some())
            || (!change.placed() && entry.2.is_some())
        {
            return Err(RetentionError::Integrity);
        }
        if change.placed() {
            entry.1 = Some(*change.changed_at().as_datetime());
        } else {
            entry.2 = Some(*change.changed_at().as_datetime());
        }
    }
    if history.values().any(|(_, placed, released)| {
        placed.is_none()
            || (*released)
                .zip(*placed)
                .is_some_and(|(released, placed)| released < placed)
    }) {
        return Err(RetentionError::Integrity);
    }
    Ok(())
}

async fn verify_restored_cleanup(
    store: &PostgresEventStore,
    tx: &mut Transaction<'_, Postgres>,
    scope_value: &RepositoryScope,
) -> Result<(), RetentionError> {
    let invalid_erased: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM public.graphhelm_evidence e \
         LEFT JOIN public.graphhelm_cleanup_receipts c USING(workspace_id,project_id,execution_id,evidence_id) \
         WHERE e.state='erased' AND (\
           (e.record='{}'::jsonb AND (c.evidence_id IS NULL OR e.ciphertext_deleted_at IS NULL)) OR \
           (e.record<>'{}'::jsonb AND (c.evidence_id IS NOT NULL OR e.ciphertext_deleted_at IS NOT NULL)))",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage)?;
    if invalid_erased != 0 {
        return Err(RetentionError::Integrity);
    }
    let rows = sqlx::query(
        "SELECT c.operation_id,c.idempotency_key,c.request_digest,c.evidence_id,c.ciphertext_sha256,c.requested_at,c.deleted_at,\
         e.state,e.record,e.ciphertext_deleted_at,t.ciphertext_sha256 AS tombstone_digest \
         FROM public.graphhelm_cleanup_receipts c \
         JOIN public.graphhelm_evidence e USING(workspace_id,project_id,execution_id,evidence_id) \
         JOIN public.graphhelm_evidence_tombstones t USING(workspace_id,project_id,execution_id,evidence_id) \
         ORDER BY c.operation_id COLLATE \"C\",c.evidence_id COLLATE \"C\" LIMIT 100001",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    if rows.len() > 100_000 {
        return Err(RetentionError::LimitExceeded);
    }
    let project_scope = RepositoryScope::new(
        scope_value.workspace_id().clone(),
        scope_value.project_id().clone(),
        None,
    );
    store
        .set_scope(tx, &project_scope)
        .await
        .map_err(repository)?;
    let execution = scope_value.execution_id().map_or("", |id| id.as_str());
    let event_rows: Vec<(serde_json::Value, String)> = sqlx::query_as(
        "SELECT envelope,request_digest FROM public.graphhelm_events \
         WHERE stream_id=$1 AND envelope#>>'{kind,type}'='evidence_ciphertext_deleted' \
           AND envelope#>>'{kind,data,evidenceScope,workspaceId}'=$2 \
           AND envelope#>>'{kind,data,evidenceScope,projectId}'=$3 \
           AND COALESCE(envelope#>>'{kind,data,evidenceScope,executionId}','')=$4 \
         ORDER BY sequence LIMIT 100001",
    )
    .bind(RETENTION_STREAM)
    .bind(scope_value.workspace_id().as_str())
    .bind(scope_value.project_id().as_str())
    .bind(execution)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage)?;
    if event_rows.len() > 100_000 {
        return Err(RetentionError::LimitExceeded);
    }
    let mut deleted_events = BTreeMap::new();
    for (value, physical_request_digest) in event_rows {
        let envelope: EventEnvelope =
            serde_json::from_value(value).map_err(|_| RetentionError::Integrity)?;
        validate_envelope(&envelope).map_err(repository)?;
        let EventKind::EvidenceCiphertextDeleted(deleted) = envelope.kind else {
            return Err(RetentionError::Integrity);
        };
        if deleted.evidence_scope != *scope_value
            || deleted_events
                .insert(
                    (
                        deleted.operation_id.as_str().to_owned(),
                        deleted.evidence_id.as_str().to_owned(),
                    ),
                    (
                        deleted.ciphertext_sha256.as_str().to_owned(),
                        timestamp_wire(&deleted.deleted_at),
                        physical_request_digest,
                    ),
                )
                .is_some()
        {
            return Err(RetentionError::Integrity);
        }
    }
    store.set_scope(tx, scope_value).await.map_err(repository)?;
    let mut receipts = Vec::with_capacity(rows.len());
    for row in rows {
        receipts.push(RestoredCleanupReceiptRow {
            operation: row.try_get::<String, _>("operation_id").map_err(decode)?,
            idempotency: row
                .try_get::<String, _>("idempotency_key")
                .map_err(decode)?,
            digest: row.try_get::<String, _>("request_digest").map_err(decode)?,
            requested: row.try_get::<String, _>("requested_at").map_err(decode)?,
            deleted: row.try_get::<String, _>("deleted_at").map_err(decode)?,
            evidence_wire: row.try_get::<String, _>("evidence_id").map_err(decode)?,
            ciphertext: row
                .try_get::<String, _>("ciphertext_sha256")
                .map_err(decode)?,
            state: row.try_get::<String, _>("state").map_err(decode)?,
            record: row
                .try_get::<serde_json::Value, _>("record")
                .map_err(decode)?,
            evidence_deleted_at: row.try_get("ciphertext_deleted_at").map_err(decode)?,
            tombstone_digest: row
                .try_get::<String, _>("tombstone_digest")
                .map_err(decode)?,
        });
    }
    correlate_restored_cleanup(scope_value, receipts, deleted_events)
}

/// One `graphhelm_cleanup_receipts` row joined to its Evidence and tombstone state.
struct RestoredCleanupReceiptRow {
    operation: String,
    idempotency: String,
    digest: String,
    requested: String,
    deleted: String,
    evidence_wire: String,
    ciphertext: String,
    state: String,
    record: serde_json::Value,
    evidence_deleted_at: Option<DateTime<Utc>>,
    tombstone_digest: String,
}

/// Physical `EvidenceCiphertextDeleted` events keyed by `(operation_id, evidence_id)` and carrying
/// `(ciphertext_sha256, deleted_at, request_digest)` exactly as persisted.
type DeletedEventIndex = BTreeMap<(String, String), (String, String, String)>;

/// Pure one-to-one correlation between restored cleanup receipts and the physical deletion events.
///
/// Every receipt must consume exactly one event, and each pairing must agree on the ciphertext
/// digest, the deletion timestamp, and the persisted `request_digest`. The last of those is the
/// only proof that a receipt describes the same cleanup request that actually deleted the
/// ciphertext; without it an archive can swap a receipt's operation and idempotency identity while
/// keeping the original event. Leftover events and receipt groups whose recomputed canonical digest
/// disagrees are rejected.
fn correlate_restored_cleanup(
    scope_value: &RepositoryScope,
    receipts: Vec<RestoredCleanupReceiptRow>,
    mut deleted_events: DeletedEventIndex,
) -> Result<(), RetentionError> {
    type CleanupGroup = (
        String,
        String,
        String,
        String,
        Vec<graphhelm_protocols::EvidenceId>,
    );
    let mut groups: BTreeMap<String, CleanupGroup> = BTreeMap::new();
    for row in receipts {
        let RestoredCleanupReceiptRow {
            operation,
            idempotency,
            digest,
            requested,
            deleted,
            evidence_wire,
            ciphertext,
            state,
            record,
            evidence_deleted_at,
            tombstone_digest,
        } = row;
        let operation_id = OpaqueId::parse(&operation).map_err(|_| RetentionError::Integrity)?;
        OpaqueId::parse(&idempotency).map_err(|_| RetentionError::Integrity)?;
        graphhelm_protocols::RawSha256::parse(&digest).map_err(|_| RetentionError::Integrity)?;
        PersistedTimestamp::parse(&requested).map_err(|_| RetentionError::Integrity)?;
        let deleted_at =
            PersistedTimestamp::parse(&deleted).map_err(|_| RetentionError::Integrity)?;
        let evidence_id = graphhelm_protocols::EvidenceId::parse(&evidence_wire)
            .map_err(|_| RetentionError::Integrity)?;
        let group = groups.entry(operation.clone()).or_insert_with(|| {
            (
                idempotency.clone(),
                digest.clone(),
                requested.clone(),
                deleted.clone(),
                Vec::new(),
            )
        });
        if (
            group.0.as_str(),
            group.1.as_str(),
            group.2.as_str(),
            group.3.as_str(),
        ) != (
            idempotency.as_str(),
            digest.as_str(),
            requested.as_str(),
            deleted.as_str(),
        ) || state != "erased"
            || record != serde_json::json!({})
            || evidence_deleted_at.map(|value| value.timestamp_micros())
                != Some(deleted_at.as_datetime().timestamp_micros())
            || ciphertext != tombstone_digest
            || deleted_events.remove(&(operation, evidence_wire))
                != Some((ciphertext, deleted, digest))
        {
            return Err(RetentionError::Integrity);
        }
        let _ = operation_id;
        group.4.push(evidence_id);
    }
    if !deleted_events.is_empty() {
        return Err(RetentionError::Integrity);
    }
    for (operation, (idempotency, digest, requested, _, evidence_ids)) in groups {
        let request = CleanupRequest::new(
            scope_value.clone(),
            OpaqueId::parse(operation).map_err(|_| RetentionError::Integrity)?,
            OpaqueId::parse(idempotency).map_err(|_| RetentionError::Integrity)?,
            evidence_ids,
            PersistedTimestamp::parse(&requested).map_err(|_| RetentionError::Integrity)?,
        )?;
        if cleanup_request_digest(&request).as_str() != digest {
            return Err(RetentionError::Integrity);
        }
    }
    Ok(())
}

/// Aligns each requested evidence id with the ciphertext digest observed for that exact id.
///
/// The requested ids are sorted in Rust byte order, while the rows arrive in the database's
/// default collation order. Those two orders diverge under any non-C collation, so pairing must be
/// keyed by evidence id and must never be positional: a positional pairing silently writes one
/// item's digest into another item's cleanup receipt and deletion event.
fn align_digests(
    ids: &[&str],
    observed: &[(String, String)],
) -> Result<Vec<String>, RetentionError> {
    if observed.len() != ids.len() {
        return Err(RetentionError::Integrity);
    }
    let by_id: BTreeMap<&str, &str> = observed
        .iter()
        .map(|(id, digest)| (id.as_str(), digest.as_str()))
        .collect();
    if by_id.len() != observed.len() {
        return Err(RetentionError::Integrity);
    }
    ids.iter()
        .map(|id| {
            by_id
                .get(id)
                .map(|digest| (*digest).to_owned())
                .ok_or(RetentionError::Integrity)
        })
        .collect()
}

fn map_key_error(error: KeyError) -> RetentionError {
    match error {
        KeyError::Integrity => RetentionError::Integrity,
        KeyError::Conflict => RetentionError::Conflict,
        KeyError::Invalid | KeyError::Unavailable | KeyError::Storage => {
            RetentionError::KeyUnavailable
        }
    }
}
fn prior_state(value: &str) -> Result<EvidencePriorAvailability, RetentionError> {
    match value {
        "available" => Ok(EvidencePriorAvailability::Available),
        "expired" => Ok(EvidencePriorAvailability::Expired),
        "missing_key" => Ok(EvidencePriorAvailability::MissingKey),
        "integrity_failed" => Ok(EvidencePriorAvailability::IntegrityFailed),
        _ => Err(RetentionError::Ineligible),
    }
}
fn parse_sensitivity(value: &str) -> Result<Sensitivity, RetentionError> {
    match value {
        "public" => Ok(Sensitivity::Public),
        "internal" => Ok(Sensitivity::Internal),
        "confidential" => Ok(Sensitivity::Confidential),
        "restricted" => Ok(Sensitivity::Restricted),
        _ => Err(RetentionError::Integrity),
    }
}
fn sensitivity_name(value: Sensitivity) -> &'static str {
    match value {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Confidential => "confidential",
        Sensitivity::Restricted => "restricted",
    }
}
fn prior_name(value: EvidencePriorAvailability) -> &'static str {
    match value {
        EvidencePriorAvailability::Available => "available",
        EvidencePriorAvailability::Expired => "expired",
        EvidencePriorAvailability::MissingKey => "missing_key",
        EvidencePriorAvailability::IntegrityFailed => "integrity_failed",
    }
}
fn protocol_prior(value: EvidencePriorAvailability) -> EvidencePriorState {
    match value {
        EvidencePriorAvailability::Available => EvidencePriorState::Available,
        EvidencePriorAvailability::Expired => EvidencePriorState::Expired,
        EvidencePriorAvailability::MissingKey => EvidencePriorState::MissingKey,
        EvidencePriorAvailability::IntegrityFailed => EvidencePriorState::IntegrityFailed,
    }
}
fn i64_of(value: u64) -> Result<i64, RetentionError> {
    i64::try_from(value).map_err(|_| RetentionError::LimitExceeded)
}
fn u64_of(value: i64) -> Result<u64, RetentionError> {
    u64::try_from(value).map_err(|_| RetentionError::Integrity)
}
/// PostgreSQL reports a lost serialization race as `40001` and a deadlock as `40P01`.
///
/// Retention runs at REPEATABLE READ and takes `FOR UPDATE` on a single per-project stream row, so
/// two operations against different executions of the same project genuinely contend. The loser can
/// succeed on a retry, which makes it categorically different from a storage fault.
const fn retryable_sqlstate(code: &str) -> bool {
    matches!(code.as_bytes(), b"40001" | b"40P01")
}

fn storage(error: sqlx::Error) -> RetentionError {
    // Flattening a serialization failure into `Storage` told the caller the operation had failed
    // terminally, when in fact retrying it is the correct response. `Conflict` already carries
    // exactly that meaning for durable-state contention, and `constraint` classifies unique
    // violations the same way.
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| retryable_sqlstate(&code))
    {
        return RetentionError::Conflict;
    }
    RetentionError::Storage
}
fn decode(_: sqlx::Error) -> RetentionError {
    RetentionError::Integrity
}
fn constraint(error: sqlx::Error) -> RetentionError {
    if error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| code == "23505")
    {
        RetentionError::Conflict
    } else {
        RetentionError::Storage
    }
}
fn repository(error: graphhelm_events::EventRepositoryError) -> RetentionError {
    match error {
        graphhelm_events::EventRepositoryError::Integrity => RetentionError::Integrity,
        graphhelm_events::EventRepositoryError::Invalid => RetentionError::Scope,
        graphhelm_events::EventRepositoryError::LimitExceeded => RetentionError::LimitExceeded,
        _ => RetentionError::Storage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{EvidenceId, ExecutionId, ProjectId, WorkspaceId};

    const OPERATION: &str = "cleanup-canary-operation";
    const IDEMPOTENCY: &str = "cleanup-canary-idempotency";
    const EVIDENCE: &str = "evidence-cleanup-canary";
    const CIPHERTEXT: &str = "11111111111111111111111111111111111111111111111111111111111111aa";
    const FOREIGN_DIGEST: &str = "22222222222222222222222222222222222222222222222222222222222222bb";
    const REQUESTED_AT: &str = "2026-08-10T00:00:00Z";
    const DELETED_AT: &str = "2026-08-11T00:00:00Z";

    fn canary_scope() -> RepositoryScope {
        RepositoryScope::new(
            WorkspaceId::parse("ws-cleanup-canary").unwrap(),
            ProjectId::parse("prj-cleanup-canary").unwrap(),
            Some(ExecutionId::parse("execution-cleanup-canary").unwrap()),
        )
    }

    /// The digest the receipt legitimately recomputes from its own persisted identity.
    fn canonical_digest(scope: &RepositoryScope) -> String {
        let request = CleanupRequest::new(
            scope.clone(),
            OpaqueId::parse(OPERATION).unwrap(),
            OpaqueId::parse(IDEMPOTENCY).unwrap(),
            vec![EvidenceId::parse(EVIDENCE).unwrap()],
            PersistedTimestamp::parse(REQUESTED_AT).unwrap(),
        )
        .unwrap();
        cleanup_request_digest(&request).as_str().to_owned()
    }

    fn receipt(digest: &str) -> RestoredCleanupReceiptRow {
        RestoredCleanupReceiptRow {
            operation: OPERATION.to_owned(),
            idempotency: IDEMPOTENCY.to_owned(),
            digest: digest.to_owned(),
            requested: REQUESTED_AT.to_owned(),
            deleted: DELETED_AT.to_owned(),
            evidence_wire: EVIDENCE.to_owned(),
            ciphertext: CIPHERTEXT.to_owned(),
            state: "erased".to_owned(),
            record: serde_json::json!({}),
            evidence_deleted_at: Some(
                *PersistedTimestamp::parse(DELETED_AT).unwrap().as_datetime(),
            ),
            tombstone_digest: CIPHERTEXT.to_owned(),
        }
    }

    /// The single physical `EvidenceCiphertextDeleted` event, persisted under `event_digest`.
    fn deleted_event(event_digest: &str) -> DeletedEventIndex {
        BTreeMap::from([(
            (OPERATION.to_owned(), EVIDENCE.to_owned()),
            (
                CIPHERTEXT.to_owned(),
                DELETED_AT.to_owned(),
                event_digest.to_owned(),
            ),
        )])
    }

    /// A lost serialization race and a deadlock are both retryable; nothing else is. Classifying
    /// them as storage faults told callers a retryable operation had failed terminally.
    #[test]
    fn only_serialization_failures_and_deadlocks_are_retryable() {
        assert!(retryable_sqlstate("40001"));
        assert!(retryable_sqlstate("40P01"));
        for terminal in ["23505", "23503", "42501", "22001", "08006", "", "40002"] {
            assert!(
                !retryable_sqlstate(terminal),
                "{terminal} must not be treated as retryable"
            );
        }
    }

    /// The requested ids are Rust byte-sorted; the rows come back in database collation order.
    /// Under any non-C collation those differ, and a positional pairing writes one item's digest
    /// into another item's receipt and deletion event.
    #[test]
    fn cleanup_digests_align_by_evidence_id_not_by_row_order() {
        let ids = ["Zeta", "alpha"];
        let observed = vec![
            ("alpha".to_owned(), "digest-alpha".to_owned()),
            ("Zeta".to_owned(), "digest-zeta".to_owned()),
        ];
        assert_eq!(
            align_digests(&ids, &observed).unwrap(),
            vec!["digest-zeta".to_owned(), "digest-alpha".to_owned()]
        );
    }

    #[test]
    fn cleanup_digests_reject_a_row_set_that_does_not_cover_every_requested_id() {
        let ids = ["alpha", "beta"];
        let observed = vec![
            ("alpha".to_owned(), "digest-alpha".to_owned()),
            ("gamma".to_owned(), "digest-gamma".to_owned()),
        ];
        assert_eq!(
            align_digests(&ids, &observed).unwrap_err(),
            RetentionError::Integrity
        );
    }

    #[test]
    fn receipt_correlates_with_its_own_physical_deletion_event() {
        let scope = canary_scope();
        let digest = canonical_digest(&scope);
        correlate_restored_cleanup(&scope, vec![receipt(&digest)], deleted_event(&digest)).unwrap();
    }

    /// Canary for the physical event <-> receipt binding.
    ///
    /// Everything else is deliberately valid: the ciphertext digests, deletion timestamps,
    /// Evidence state, tombstone and the receipt's own recomputed canonical digest all agree, and
    /// exactly one event is consumed. Only `graphhelm_events.request_digest` — the column that is
    /// outside the envelope hash chain and therefore the one an archive can rewrite — disagrees.
    /// This test must fail if that single comparison is ever neutralized.
    #[test]
    fn receipt_rejects_event_persisted_under_a_foreign_request_digest() {
        let scope = canary_scope();
        let digest = canonical_digest(&scope);
        assert_ne!(digest, FOREIGN_DIGEST);
        assert_eq!(
            correlate_restored_cleanup(
                &scope,
                vec![receipt(&digest)],
                deleted_event(FOREIGN_DIGEST)
            )
            .unwrap_err(),
            RetentionError::Integrity
        );
    }
}
