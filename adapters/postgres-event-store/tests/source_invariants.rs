const MIGRATION: &str = include_str!("../migrations/0001_event_evidence.sql");
const SCOPE: &str = include_str!("../src/scope.rs");
const JOURNAL: &str = include_str!("../src/journal.rs");
const INTEGRITY: &str = include_str!("../src/integrity.rs");
// `BACKUP` and `RETENTION` were here, and they are gone rather than silenced: the collation rule
// was their only reader, and it now walks `src/` instead of naming files. A const kept alive with
// an `#[allow(dead_code)]` would have left this file still LOOKING like it hand-lists six sources
// while only four remain load-bearing -- and the next reader would count the list, not the uses.

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

/// Every source this adapter writes SQL in: `.rs` under `src/`, and `.sql` under `migrations/`.
///
/// **Only this one invariant takes a walk, and the reason is which question it asks.** Eight of the
/// nine tests in this file name a specific file because they assert the shape of specific code --
/// that `scope.rs` sets three transaction-local flags, that `journal.rs` resolves idempotency
/// before taking the stream lock, that `integrity.rs` verifies a checkpoint before inserting it.
/// Pointing those at another file asserts nothing: there is no counterpart in it to be wrong.
///
/// The collation rule is different in kind. It says **any** SQL `ORDER BY` over a
/// collation-sensitive column must pin `COLLATE "C"` -- a property of every ordering this adapter
/// writes, wherever it writes it. A hand-list is the wrong population for that question, and it was
/// already short: `projection.rs` carries two `ORDER BY` clauses and was not among the four named.
/// Both sort by `last_sequence`, which is numeric and not in the list below, so this lands green --
/// but `projection_name` IS collation-sensitive, and `projection.rs` is precisely the file that
/// would one day order by it.
///
/// **`migrations/` is here for the same reason and it was missed for the same one (#387).** The
/// rule says *wherever it writes it*, and the walk stopped at `src/`; `0002`, `0003` and `0004`
/// were not even reachable as consts, so no test in this file had ever read them. What made the
/// gap visible is that `0004_scope_guard.sql:23` already carries a hand-written `COLLATE "C"` --
/// someone knew the rule reached there, and nothing checked it.
///
/// **What this extension does NOT buy, stated because a walk looks like coverage.** No live
/// migration clause is matched by `COLLATION_SENSITIVE_ORDER_KEYS` today: the three are
/// `ORDER BY sequence` twice and `ORDER BY c.relname`, and none of those is in the list. So the
/// collation loop's pass over `migrations/` is CAPACITY, not necessity, and its falsifier is the
/// floor-and-landmark test below rather than the loop itself. Planting a fixture `.sql` to
/// manufacture a red would be planting a defect to keep a guard honest; the honest form is to say
/// so here.
fn sql_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, extension: &str, out: &mut Vec<std::path::PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, extension, out);
            } else if path.extension().is_some_and(|found| found == extension) {
                out.push(path);
            }
        }
    }
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    walk(&crate_root.join("src"), "rs", &mut found);
    walk(&crate_root.join("migrations"), "sql", &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            (name, text)
        })
        .collect()
}

/// The walk must reach the whole adapter, and must still reach the file the rule was written for.
///
/// The floor is the REAL count rather than a round number under it: a loose floor tolerates exactly
/// the silent shrinkage this guard exists to stop. **Lowering it is legitimate only alongside a
/// NAMED removal in the same change.**
#[test]
fn the_collation_scan_covers_the_whole_adapter() {
    let found = sql_sources();
    assert!(
        found.len() >= 15,
        "HARNESS-BROKE: the walk found {} sources; this adapter has 15 (11 under src/, 4 under \
         migrations/). If one was deleted, lower this floor in the same change that removes it \
         and name the file here",
        found.len()
    );
    for landmark in ["backup.rs", "projection.rs", "0004_scope_guard.sql"] {
        assert!(
            found.iter().any(|(name, _)| name == landmark),
            "HARNESS-BROKE: {landmark} is known to exist and is absent from the walk"
        );
    }
}

#[test]
fn text_orderings_pin_the_c_collation() {
    for (name, source) in sql_sources() {
        for (offset, _) in source.match_indices("ORDER BY ") {
            // The SQL lives inside both escaped and raw Rust string literals, so a quote is not a
            // reliable terminator. Bound the clause by its source line instead.
            let clause: String = source[offset..]
                .chars()
                .take(400)
                .take_while(|character| *character != '\n')
                .collect();
            // Skipping is the right answer for an ordering over a column that is not
            // collation-sensitive -- and `0004_scope_guard.sql:23` is the case worth writing down,
            // because it LOOKS like one this loop should have caught. It orders by
            // `pg_class.relname`, which is `name`, and it carries a hand-written `COLLATE "C"`.
            //
            // MEASURED on PostgreSQL 13, 16 and 17, in throwaway clusters: `name` has
            // `pg_type.typcollation = C`, and ordering a `name` column by default is byte-identical
            // to ordering it `COLLATE "C"` -- with a POSITIVE CONTROL, because the first run used
            // the image's libc `en_US.utf8` on musl, where even `text` orders identically under
            // both and the comparison proves nothing. Under `COLLATE "en-US-x-icu"` the `text`
            // control DIVERGES, and under the same ICU collation a `name` column also diverges --
            // so `name` can be collated otherwise, and its agreement with C is a property of the
            // type's default collation rather than an inability to be ordered any other way.
            //
            // So `relname` is deliberately NOT in the list: the pin at `0004:23` is a no-op, correct
            // and harmless and not load-bearing. Adding `relname` here would make this loop demand
            // a `COLLATE "C"` that changes nothing, on every catalog ordering anyone ever writes.
            // (#387. The judgement travelled with its own evidence rather than inside a mechanical
            // population fix.)
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
