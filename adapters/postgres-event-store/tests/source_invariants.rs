const MIGRATION: &str = include_str!("../migrations/0001_event_evidence.sql");
const SCOPE: &str = include_str!("../src/scope.rs");
const JOURNAL: &str = include_str!("../src/journal.rs");
const INTEGRITY: &str = include_str!("../src/integrity.rs");
const BACKUP: &str = include_str!("../src/backup.rs");
const RETENTION: &str = include_str!("../src/retention.rs");

/// Text columns whose ordering must never depend on the database's default collation.
///
/// Ordering these under a locale-sensitive collation is a defect class this adapter has already
/// suffered twice: a schema-contract hash that could never match across platforms, and a cleanup
/// path that paired evidence ids with foreign ciphertext digests. Both were invisible to CI while
/// the throwaway cluster ran in the C locale. Any new `ORDER BY` over one of these columns must
/// carry an explicit `COLLATE "C"`.
const COLLATION_SENSITIVE_ORDER_KEYS: &[&str] = &[
    "workspace_id",
    "project_id",
    "execution_id",
    "evidence_id",
    "stream_id",
    "projection_name",
    "to_jsonb(t)::text",
    "payload::text",
    "category",
    "object_name",
    "grantee_name",
];

#[test]
fn text_orderings_pin_the_c_collation() {
    for (name, source) in [
        ("backup.rs", BACKUP),
        ("retention.rs", RETENTION),
        ("integrity.rs", INTEGRITY),
        ("journal.rs", JOURNAL),
    ] {
        for (offset, _) in source.match_indices("ORDER BY ") {
            // The SQL lives inside both escaped and raw Rust string literals, so a quote is not a
            // reliable terminator. Bound the clause by its source line instead.
            let clause: String = source[offset..]
                .chars()
                .take(400)
                .take_while(|character| *character != '\n')
                .collect();
            let Some(key) = COLLATION_SENSITIVE_ORDER_KEYS
                .iter()
                .find(|key| clause.contains(**key))
            else {
                continue;
            };
            assert!(
                clause.contains("COLLATE \\\"C\\\"") || clause.contains("COLLATE \"C\""),
                "{name}: ordering by collation-sensitive column `{key}` without COLLATE \"C\": {clause}"
            );
        }
    }
}

#[test]
fn every_scoped_table_forces_rls() {
    assert!(MIGRATION.contains("ENABLE ROW LEVEL SECURITY"));
    assert!(MIGRATION.contains("FORCE ROW LEVEL SECURITY"));
    assert_eq!(MIGRATION.matches("graphhelm_scope").count(), 1);
}

#[test]
fn scope_is_always_transaction_local() {
    assert_eq!(SCOPE.matches(", true)").count(), 3);
    assert!(!SCOPE.contains(", false)"));
}

#[test]
fn idempotency_resolution_precedes_stream_lock_and_sequence_check() {
    let idempotency = JOURNAL.find("if !existing.is_empty()").unwrap();
    let lock = JOURNAL.find("FOR UPDATE").unwrap();
    let sequence = JOURNAL
        .find("EventRepositoryError::SequenceConflict")
        .unwrap();
    assert!(idempotency < lock && lock < sequence);
}

#[test]
fn append_uses_one_transaction_and_commits_after_head_update() {
    let begin = JOURNAL.find("pool().begin()").unwrap();
    let evidence = JOURNAL
        .find("INSERT INTO public.graphhelm_evidence")
        .unwrap();
    let event = JOURNAL.find("INSERT INTO public.graphhelm_events").unwrap();
    let head = JOURNAL
        .find("UPDATE public.graphhelm_streams SET next_sequence")
        .unwrap();
    let commit = JOURNAL.rfind("transaction.commit()").unwrap();
    assert!(begin < evidence && evidence < event && event < head && head < commit);
}

#[test]
fn reads_recompute_event_hashes() {
    assert!(INTEGRITY.contains("graphhelm_events::compute_event_hash(event, &expected_previous)?"));
}

#[test]
fn cursors_bind_all_scope_stream_head_and_format_fields() {
    for field in [
        "workspace_id",
        "project_id",
        "execution_id",
        "stream_id",
        "format",
        "head_hash",
    ] {
        assert!(INTEGRITY.contains(&format!("payload.{field}")), "{field}");
    }
}

#[test]
fn checkpoints_are_verified_by_key_provider_before_insert() {
    let verify = INTEGRITY.find("graphhelm-checkpoint-v1").unwrap();
    let insert = INTEGRITY
        .find("INSERT INTO public.graphhelm_checkpoints")
        .unwrap();
    assert!(verify < insert);
}

#[test]
fn database_json_is_byte_guarded_before_client_materialization() {
    assert!(INTEGRITY.matches("octet_length(envelope::text)").count() >= 3);
    assert!(INTEGRITY.contains("const CHUNK_EVENTS: u64 = 4"));
    assert!(INTEGRITY.contains("octet_length(envelope::text) <= 4194304"));
    assert!(!INTEGRITY.contains("cumulative_bytes"));
    assert!(!JOURNAL.contains("SELECT envelope"));
}
