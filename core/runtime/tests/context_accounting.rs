//! Guards for Context Capsule accounting (#222).
//!
//! First property, and the one the issue's threat assessment turns on: **the cache key must carry
//! every semantic input**. The failure this blocks is not a collision — it is an *omission*, and an
//! omitted dimension produces a cache **HIT** across that dimension. A cross-scope or stale-policy
//! hit is a correct-looking cheap answer, which is the shape this milestone exists to prevent.
//!
//! So the property is deliberately NOT "same inputs produce the same key" (that would pass with a
//! constant). It is **"inputs differing in exactly one dimension produce different keys"**, one case
//! per dimension.

use graphhelm_runtime::context_accounting::ContextCacheKeyInputs;

/// The baseline every case mutates exactly one field of.
fn baseline() -> ContextCacheKeyInputs {
    ContextCacheKeyInputs {
        scope_project: "proj-a".to_owned(),
        permissions: vec!["repo.read".to_owned()],
        repo_snapshot: "snap-1".to_owned(),
        index_generation: "gen-1".to_owned(),
        schema_id: "https://p50.dev/schemas/context-capsule.schema.json".to_owned(),
        schema_version: "1.0.0".to_owned(),
        objective: "explain the wake path".to_owned(),
        capsule_digest: "sha256:aaaa".to_owned(),
        producer: "compiler-a".to_owned(),
        utilization_policy_version: "1.0.0".to_owned(),
    }
}

/// Every dimension, with the mutation that must change the key.
///
/// `producer` and `utilization_policy_version` are here for reasons that differ, and both were
/// established before this test was written:
///
/// - **`producer`** is not subsumed by `capsule_digest`. A digest is over CONTENT, so two producers
///   emitting byte-identical capsules share a digest — keying on content alone lets a less-trusted
///   producer hit an entry warmed by a more-trusted one.
/// - **`utilization_policy_version`** moves independently of everything else: change the policy and
///   the same capsule with the same inputs must yield a different verdict, while no other field
///   moves. Omitting it serves stale-policy entries.
///
/// `repo_snapshot` and `index_generation` are TWO cases rather than one because
/// `SnapshotBinding` carries them as independent identities — the bytes read versus what the index
/// was built from — and their *relation* is the freshness verdict. Folding them into one case would
/// be the flattening the guard exists to catch.
/// One dimension case: the name of the semantic input, and the mutation that changes exactly
/// that input and nothing else.
///
/// Named rather than written inline because clippy refuses the inline form at this depth --
/// and it was right to: the tuple-of-function-pointer says what the compiler needs and nothing
/// about what the case IS, which is the whole content of this table.
type DimensionCase = (&'static str, fn(&mut ContextCacheKeyInputs));

fn dimension_cases() -> Vec<DimensionCase> {
    vec![
        ("scope_project", |i| i.scope_project = "proj-b".to_owned()),
        ("permissions", |i| {
            i.permissions = vec!["repo.write".to_owned()]
        }),
        ("repo_snapshot", |i| i.repo_snapshot = "snap-2".to_owned()),
        ("index_generation", |i| {
            i.index_generation = "gen-2".to_owned()
        }),
        ("schema_id", |i| {
            i.schema_id = "https://p50.dev/schemas/other.schema.json".to_owned()
        }),
        ("schema_version", |i| i.schema_version = "2.0.0".to_owned()),
        ("objective", |i| {
            i.objective = "explain the sweep path".to_owned()
        }),
        ("capsule_digest", |i| {
            i.capsule_digest = "sha256:bbbb".to_owned()
        }),
        ("producer", |i| i.producer = "compiler-b".to_owned()),
        ("utilization_policy_version", |i| {
            i.utilization_policy_version = "2.0.0".to_owned()
        }),
    ]
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written (TDD skill's rule,
/// and the same discipline as a sealed prediction): **dropping any one field from the key's
/// derivation**. That mutation is invisible to a "same inputs, same key" test and invisible to a
/// collision test; only a per-dimension difference assertion sees it, and the red must name the
/// dimension that was dropped rather than failing somewhere adjacent.
#[test]
fn every_semantic_dimension_changes_the_cache_key() {
    let base = baseline();
    let base_key = base.cache_key();

    // LANDMARK, load-bearing: an empty or trivially-constant derivation would make the loop below
    // fail in a way that looks like a real finding. If this fires, the instrument is broken, not
    // the invariant.
    assert!(
        !base_key.is_empty(),
        "extraction is broken, not the invariant: the baseline key is empty"
    );

    for (dimension, mutate) in dimension_cases() {
        let mut varied = baseline();
        mutate(&mut varied);
        assert_ne!(
            varied.cache_key(),
            base_key,
            "cache key ignores the `{dimension}` dimension (#222).\n\
             Two requests differing only in `{dimension}` would share a cache entry, so the second \
             receives an answer computed for the first. An omitted dimension does not cause a \
             miss -- it causes a HIT across that dimension, which is a correct-looking cheap \
             success.\n\
             Either include `{dimension}` in the key derivation, or delete the field and its case \
             together."
        );
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **encoding a
/// variable-length field by joining its elements with a separator that can occur inside them.**
///
/// The per-dimension guard above cannot see this. It varies one field at a time and asks "did the
/// key change?", which a naive join answers correctly. This asks the harder question: **can two
/// DIFFERENT inputs produce the SAME key?** — the collision direction, which is the one that grants
/// a cache hit to a request that was never computed.
///
/// `permissions` is the field that exposes it because it is the only variable-length one: the set
/// `["repo.read,repo.write"]` (one permission whose name contains the separator) and the set
/// `["repo.read", "repo.write"]` (two permissions) are different authority and must never share an
/// entry.
#[test]
fn different_permission_sets_cannot_share_a_cache_key() {
    let mut one_permission = baseline();
    one_permission.permissions = vec!["repo.read,repo.write".to_owned()];

    let mut two_permissions = baseline();
    two_permissions.permissions = vec!["repo.read".to_owned(), "repo.write".to_owned()];

    assert_ne!(
        one_permission.permissions, two_permissions.permissions,
        "arrangement check: the two permission sets must actually differ, or this proves nothing"
    );

    assert_ne!(
        one_permission.cache_key(),
        two_permissions.cache_key(),
        "two DIFFERENT permission sets derive the SAME cache key (#222).\n\
         The list encoding is ambiguous: joining elements with a separator that can appear inside \
         an element loses the boundary. A request holding one broad permission would receive an \
         entry computed for a request holding two narrower ones, or the reverse.\n\
         Encode the list unambiguously (length-prefixed, or count plus per-element length)."
    );
}

// ---------------------------------------------------------------------------------------------
// Cost provenance. The issue's out-of-scope names the failure directly: "token estimates presented
// as observed usage". A tag is what makes that enforceable instead of aspirational.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::{CostField, CostProvenance};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **representing an
/// unobservable cost as `0`** — a bare integer with zero as the sentinel for "no data".
///
/// That is the flattening this whole task exists to refuse. `0` is a legitimate measured value
/// (a step that genuinely cost nothing), and "we could not observe this" is a different state with
/// a different consequence: one may be summed, the other must stop a cheap-success claim. A type
/// that cannot tell them apart makes every downstream total silently wrong in the flattering
/// direction.
#[test]
fn an_unavailable_cost_is_not_zero() {
    let unavailable = CostField::unavailable("provider does not report input tokens");
    let genuinely_zero = CostField::measured(0, "context_compiler");

    assert_eq!(
        genuinely_zero.observed(),
        Some(0),
        "arrangement check: a measured zero must read as an observed zero, or this proves nothing"
    );

    assert_eq!(
        unavailable.observed(),
        None,
        "an unobservable cost reads as an observed value (#222).\n\
         `0` is a legitimate measurement -- a step that really cost nothing -- and 'not observable' \
         is a different state with the opposite consequence: a measured zero may be summed, an \
         unavailable one must refuse the cheap-success claim.\n\
         Represent absence as absence, never as a sentinel value."
    );

    assert!(
        !unavailable.is_measured(),
        "an unavailable cost must not claim to be measured"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **letting a field be built as `measured` without
/// naming the component that observed it.**
///
/// The blueprint's rule is that a number is born where its inputs are born and a reader only reads;
/// a field whose producer is the accounting module itself is derived by construction. Unnamed
/// producers are how a recomputed number passes as an observed one.
#[test]
fn a_measured_cost_names_the_component_that_observed_it() {
    let measured = CostField::measured(1234, "retrieval");
    assert_eq!(measured.producer(), Some("retrieval"));

    let derived = CostField::derived(99, "amortized over declared window");
    assert_eq!(
        derived.producer(),
        None,
        "a derived value has no observer -- claiming one would make a computation look observed"
    );
    assert!(matches!(derived.provenance(), CostProvenance::Derived));
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **any string here that is
/// not exactly what `CostProvenance::X.wire_name()` says** -- a value drifting from what the type
/// declares while this stays hand-written and correct only by coincidence, or the type's own
/// spelling drifting while nothing else in the repository names it.
///
/// #381: `CostProvenance` carried no wire spelling at all, and the one caller that puts it on a
/// public surface (`run_accounting`, `apps/cli/src/commands/development.rs`) rendered it with
/// `format!("{:?}", ...)` -- Rust's `Debug` output, which is not a contract and changes with a
/// rename or a hand-written `Debug` impl (this repository already writes one of those elsewhere:
/// `core/governor/src/materialize.rs:50`). This pins the wire literal at its source, independently
/// of the adapter -- see `apps/cli/tests/development_cli.rs`'s
/// `accounting_reports_the_provenance_wire_spelling_not_the_debug_rendering` for the second,
/// independent pin at the actual public surface. Both are hand-written literals, not derived from
/// each other, so a rename on either side fails the one that did not move.
#[test]
fn cost_provenance_wire_names_are_the_lowercase_variant_spelling() {
    assert_eq!(CostProvenance::Measured.wire_name(), "measured");
    assert_eq!(CostProvenance::Derived.wire_name(), "derived");
    assert_eq!(CostProvenance::Unavailable.wire_name(), "unavailable");
}

// ---------------------------------------------------------------------------------------------
// The named trap: a receipt that counts capsules which never ran.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::Utilization;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **computing the ratio from
/// the emitted-item count alone**, so a capsule that was compiled and never used reports
/// `0 cited / N emitted` = 0% utilization.
///
/// That number is not wrong-looking. It reads as a real finding — "the compiler over-fetched" —
/// when the truth is that nothing was measured at all. **Utilization is a property of having been
/// USED, not of having been BUILT**, and the error runs in the direction of the metric's own
/// incentive: a task judged on utilization benefits from counting compilations it never spent.
///
/// Same family as `an_unavailable_cost_is_not_zero`, one level up: absence rendered as a measured
/// value, where the two states carry opposite consequences.
#[test]
fn a_capsule_that_never_ran_has_no_utilization_ratio() {
    let compiled_and_used = Utilization::from_run(3, 10);
    let compiled_never_used = Utilization::not_executed();

    assert!(
        matches!(
            compiled_and_used,
            Utilization::Ratio {
                cited: 3,
                emitted: 10
            }
        ),
        "arrangement check: a real run must produce a ratio, or this proves nothing"
    );

    assert!(
        matches!(compiled_never_used, Utilization::NotExecuted),
        "a capsule that was compiled but never executed reports a utilization ratio (#222).\n\
         `0 cited / N emitted` reads as a finding about the compiler when it is the absence of a \
         measurement. Utilization is a property of having been USED, not of having been BUILT.\n\
         Report not-executed as its own state; never as a ratio."
    );

    assert_eq!(
        compiled_never_used.ratio(),
        None,
        "not-executed must have no numeric ratio at all -- a caller that averages receipts must be \
         unable to fold it in as a zero"
    );
}

// ---------------------------------------------------------------------------------------------
// Byte-identity. The acceptance criterion says "byte-identical capsules and accounting receipts",
// and this is the guard that pins the INSTRUMENT rather than restating the property.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::capsules_identical;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **deciding byte-identity by
/// comparing canonical digests.**
///
/// A canonical digest is deliberately blind to key order — that is what makes it useful for asking
/// "is this the same MEANING?". It is therefore the wrong instrument for "are these the same
/// BYTES?", and the failure is silent: the check passes while the criterion it claims to verify is
/// violated. A sibling task found this trap live on this milestone, which is why the instrument is
/// pinned here rather than left to whoever implements it.
///
/// The two documents below carry the same fields with the same values in a different written order.
/// Same meaning, same canonical digest, different bytes. A byte comparison must call them
/// different; a digest comparison cannot.
#[test]
fn byte_identity_is_decided_on_bytes_not_on_canonical_meaning() {
    let written_one_way = br#"{"objective":"a","nodeId":"n1"}"#;
    let written_the_other = br#"{"nodeId":"n1","objective":"a"}"#;

    // Arrangement: they really are the same meaning, or the test proves nothing about the trap.
    let left: serde_json::Value = serde_json::from_slice(written_one_way).unwrap();
    let right: serde_json::Value = serde_json::from_slice(written_the_other).unwrap();
    assert_eq!(
        left, right,
        "arrangement check: the two documents must carry the same MEANING, or this does not \
         exercise the digest trap at all"
    );
    assert_ne!(
        written_one_way.as_slice(),
        written_the_other.as_slice(),
        "arrangement check: the two documents must differ in BYTES"
    );

    assert!(
        !capsules_identical(written_one_way, written_the_other),
        "byte-identity was decided by meaning rather than by bytes (#222).\n\
         These two documents canonicalize to the same value and differ in their written form. A \
         determinism check built on a canonical digest passes here, which means it would pass for \
         a compiler that silently changed its output ordering between runs -- exactly the \
         criterion it is supposed to enforce.\n\
         Compare the raw serialized bytes."
    );

    assert!(
        capsules_identical(written_one_way, written_one_way),
        "identical bytes must compare identical"
    );
}

// ---------------------------------------------------------------------------------------------
// Where a number is born. A number is born where its inputs are born; a reader only reads.
// ---------------------------------------------------------------------------------------------

use graphhelm_protocols::{
    ActorId, EventEnvelope, EventHash, EventKind, ExecutionId, ExecutionMode, ExecutionStarted,
    NewEvent, OpaqueId, PersistedActor, PersistedActorType, PersistedTimestamp, ProjectId,
    RepositoryScope, Sensitivity, WireHash, WorkspaceId,
};
use graphhelm_runtime::context_accounting::{
    ACCOUNTING_MODULE, AccountingReceipt, ExecutionAccountingReceipt,
};
use graphhelm_runtime::executor::WorkSummary;

const EXECUTION_ID: &str = "execution-accounting-fixed";
const MODEL_USAGE_PRODUCER: &str = "model_gateway";

fn execution_event(scoped_execution: Option<&str>, payload_execution: &str) -> EventEnvelope {
    execution_event_by_actor(
        scoped_execution,
        payload_execution,
        PersistedActorType::System,
    )
}

fn execution_event_by_actor(
    scoped_execution: Option<&str>,
    payload_execution: &str,
    actor_type: PersistedActorType,
) -> EventEnvelope {
    let previous_hash = EventHash::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
    let mut envelope = EventEnvelope::new(
        OpaqueId::parse("execution-start-event-1").unwrap(),
        RepositoryScope::new(
            WorkspaceId::parse("workspace-accounting").unwrap(),
            ProjectId::parse("project-accounting").unwrap(),
            scoped_execution.map(|value| ExecutionId::parse(value).unwrap()),
        ),
        OpaqueId::parse("execution-accounting-stream").unwrap(),
        1,
        PersistedTimestamp::parse("2026-08-28T00:00:00Z").unwrap(),
        NewEvent::new(
            OpaqueId::parse("execution-start-key").unwrap(),
            PersistedActor::new(actor_type, ActorId::parse("runtime-driver").unwrap()),
            Sensitivity::Internal,
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse(payload_execution).unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Autopilot,
            }),
            Vec::new(),
            Vec::new(),
        ),
        previous_hash.clone(),
        EventHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap(),
    );
    envelope.event_hash = EventHash::parse(
        graphhelm_events::compute_event_hash(&envelope, previous_hash.as_str()).unwrap(),
    )
    .unwrap();
    envelope
}

fn execution_binding() -> EventEnvelope {
    execution_event(Some(EXECUTION_ID), EXECUTION_ID)
}

#[test]
fn execution_usage_receipt_is_bound_and_byte_stable() {
    let summary = WorkSummary {
        input_tokens: Some(12),
        output_tokens: Some(5),
        exit_code: None,
    };

    let binding = execution_binding();
    let first = ExecutionAccountingReceipt::from_work_summary(&binding, &summary).unwrap();
    let second = ExecutionAccountingReceipt::from_work_summary(&binding, &summary).unwrap();

    assert_eq!(first.execution_id(), EXECUTION_ID);
    assert_eq!(
        first.stable_bytes().unwrap(),
        second.stable_bytes().unwrap()
    );
    assert_eq!(
        ExecutionAccountingReceipt::from_stable_bytes(&first.stable_bytes().unwrap(), &binding)
            .unwrap(),
        first
    );

    for (name, expected) in [("provider_reported_input_tokens", 12), ("output_tokens", 5)] {
        let field = first.field(name).expect("the measured model field exists");
        assert_eq!(field.observed(), Some(expected));
        assert_eq!(field.provenance(), &CostProvenance::Measured);
        assert_eq!(field.producer(), Some(MODEL_USAGE_PRODUCER));
    }

    let compiled = first
        .field("compiled_input_tokens")
        .expect("compiled input remains explicit");
    assert_eq!(compiled.observed(), None);
    assert_eq!(compiled.provenance(), &CostProvenance::Unavailable);
    assert_eq!(compiled.producer(), None);

    let provider_total = first
        .field("provider_total_input_tokens")
        .expect("provider total remains explicit even when unavailable");
    assert_eq!(provider_total.observed(), None);
    assert_eq!(provider_total.provenance(), &CostProvenance::Unavailable);
    assert_eq!(provider_total.producer(), None);

    let bytes = first.stable_bytes().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["executionBinding"]["scope"]["executionId"],
        EXECUTION_ID
    );
    assert_eq!(
        json["executionBinding"]["eventId"],
        "execution-start-event-1"
    );
    assert_eq!(
        json["executionBinding"]["bindingKind"],
        "execution_started_event"
    );
    assert_eq!(json["executionBinding"]["producerActor"]["type"], "system");
    assert_eq!(
        json["executionBinding"]["producerActor"]["id"],
        "runtime-driver"
    );
    assert!(json["executionBinding"].get("snapshots").is_none());
    assert_eq!(json["schemaVersion"], "1.0.0");
    let names: Vec<&str> = json["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "orientation_tokens",
            "zero_result_queries",
            "retrieval_pages",
            "retrieval_retries",
            "retrieval_fallbacks",
            "summary_tokens",
            "provider_reported_input_tokens",
            "provider_total_input_tokens",
            "compiled_input_tokens",
            "output_tokens",
            "formatting_tokens",
            INDEX_COST_COLD_FIELD,
            INDEX_COST_AMORTIZED_FIELD,
        ],
        "persistent receipt fields are a closed vocabulary"
    );
    let schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../schemas/execution-accounting-receipt.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let schema_names: Vec<&str> = schema["properties"]["fields"]["prefixItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field["properties"]["name"]["const"].as_str().unwrap())
        .collect();
    assert_eq!(
        names, schema_names,
        "runtime receipt bytes must use the registered schema's exact field order"
    );
    let reported = json["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "provider_reported_input_tokens")
        .unwrap();
    assert_eq!(reported["value"], 12);
    assert!(reported["note"].as_str().unwrap().contains("cache read"));
    assert!(
        reported["note"]
            .as_str()
            .unwrap()
            .contains("cache creation")
    );
    let total = json["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "provider_total_input_tokens")
        .unwrap();
    assert_eq!(total["value"], serde_json::Value::Null);
    assert_eq!(total["provenance"], "unavailable");
    assert!(total["note"].as_str().unwrap().contains("cache read"));
    assert!(total["note"].as_str().unwrap().contains("cache creation"));
    assert!(!String::from_utf8_lossy(&bytes).contains("prompt"));
    assert!(!String::from_utf8_lossy(&bytes).contains("outputContent"));
}

#[test]
fn missing_model_usage_is_unavailable_and_never_zero() {
    let summary = WorkSummary {
        input_tokens: None,
        output_tokens: None,
        exit_code: None,
    };
    let receipt =
        ExecutionAccountingReceipt::from_work_summary(&execution_binding(), &summary).unwrap();

    for name in [
        "provider_reported_input_tokens",
        "provider_total_input_tokens",
        "output_tokens",
    ] {
        let field = receipt.field(name).expect("the unavailable field exists");
        assert_eq!(field.observed(), None, "missing usage must not become zero");
        assert_eq!(field.provenance(), &CostProvenance::Unavailable);
        assert_eq!(field.producer(), None);
    }

    for name in [
        "orientation_tokens",
        "zero_result_queries",
        "retrieval_pages",
        "retrieval_retries",
        "retrieval_fallbacks",
        "summary_tokens",
        "compiled_input_tokens",
        "formatting_tokens",
        INDEX_COST_COLD_FIELD,
        INDEX_COST_AMORTIZED_FIELD,
    ] {
        let field = receipt
            .field(name)
            .expect("every unmeasured category is explicit");
        assert_eq!(
            field.observed(),
            None,
            "{name} must not be invented as zero"
        );
        assert_eq!(field.provenance(), &CostProvenance::Unavailable);
    }
}

#[test]
fn an_execution_receipt_refuses_a_binding_without_an_execution_scope() {
    let result = ExecutionAccountingReceipt::from_work_summary(
        &execution_event(None, EXECUTION_ID),
        &WorkSummary {
            input_tokens: Some(12),
            output_tokens: Some(5),
            exit_code: None,
        },
    );
    assert!(result.is_err());
}

#[test]
fn an_execution_receipt_refuses_a_mismatched_or_corrupt_execution_event() {
    let summary = WorkSummary {
        input_tokens: Some(12),
        output_tokens: Some(5),
        exit_code: None,
    };
    assert!(
        ExecutionAccountingReceipt::from_work_summary(
            &execution_event(Some(EXECUTION_ID), "another-execution"),
            &summary,
        )
        .is_err()
    );

    let mut corrupt = execution_binding();
    corrupt.event_hash = EventHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap();
    assert!(ExecutionAccountingReceipt::from_work_summary(&corrupt, &summary).is_err());
}

#[test]
fn producer_actor_type_is_identity_even_when_actor_id_is_the_same() {
    let summary = WorkSummary {
        input_tokens: Some(12),
        output_tokens: Some(5),
        exit_code: None,
    };
    let system = ExecutionAccountingReceipt::from_work_summary(
        &execution_event_by_actor(Some(EXECUTION_ID), EXECUTION_ID, PersistedActorType::System),
        &summary,
    )
    .unwrap();
    let agent = ExecutionAccountingReceipt::from_work_summary(
        &execution_event_by_actor(Some(EXECUTION_ID), EXECUTION_ID, PersistedActorType::Agent),
        &summary,
    )
    .unwrap();

    assert_ne!(
        system.stable_bytes().unwrap(),
        agent.stable_bytes().unwrap()
    );
}

#[test]
fn execution_receipt_deserialization_rejects_tokens_above_json_safe_integer() {
    let binding = execution_binding();
    let receipt = ExecutionAccountingReceipt::from_work_summary(
        &binding,
        &WorkSummary {
            input_tokens: Some(12),
            output_tokens: Some(5),
            exit_code: None,
        },
    )
    .unwrap();
    let mut json: serde_json::Value =
        serde_json::from_slice(&receipt.stable_bytes().unwrap()).unwrap();
    json["fields"][6]["value"] = serde_json::json!(9_007_199_254_740_992_u64);

    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(
            &serde_json::to_vec(&json).unwrap(),
            &binding,
        )
        .is_err()
    );
}

#[test]
fn execution_receipt_deserialization_rejects_wrong_binding_domain_kind_and_digest() {
    let binding = execution_binding();
    let receipt = ExecutionAccountingReceipt::from_work_summary(
        &binding,
        &WorkSummary {
            input_tokens: Some(12),
            output_tokens: Some(5),
            exit_code: None,
        },
    )
    .unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&receipt.stable_bytes().unwrap()).unwrap();

    for (field, wrong) in [
        ("bindingKind", serde_json::json!("artifact_binding")),
        (
            "schemaId",
            serde_json::json!("https://p50.dev/schemas/context-capsule.schema.json"),
        ),
        (
            "eventHash",
            serde_json::json!(format!("sha512:{}", "a".repeat(64))),
        ),
    ] {
        let mut candidate = original.clone();
        candidate["executionBinding"][field] = wrong;
        assert!(
            ExecutionAccountingReceipt::from_stable_bytes(
                &serde_json::to_vec(&candidate).unwrap(),
                &binding,
            )
            .is_err(),
            "wrong {field} must fail closed"
        );
    }

    let mut generic = original;
    generic["executionBinding"] = serde_json::json!({
        "artifactId": "execution-start-event-1",
        "schemaId": "https://p50.dev/schemas/event-envelope.schema.json",
        "documentVersion": "1.0.0",
        "schemaVersion": "1.0.0",
        "digest": format!("sha256:{}", "a".repeat(64)),
        "scope": {
            "workspaceId": "workspace-accounting",
            "projectId": "project-accounting",
            "executionId": EXECUTION_ID
        },
        "producer": "runtime-driver",
        "snapshots": {
            "repoSnapshot": "repo-snapshot",
            "indexGeneration": "repo-snapshot"
        }
    });
    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(
            &serde_json::to_vec(&generic).unwrap(),
            &binding,
        )
        .is_err()
    );

    let mut missing_required_nullable: serde_json::Value =
        serde_json::from_slice(&receipt.stable_bytes().unwrap()).unwrap();
    missing_required_nullable["fields"][0]
        .as_object_mut()
        .unwrap()
        .remove("value");
    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(
            &serde_json::to_vec(&missing_required_nullable).unwrap(),
            &binding,
        )
        .is_err()
    );

    let mut secret_note: serde_json::Value =
        serde_json::from_slice(&receipt.stable_bytes().unwrap()).unwrap();
    secret_note["fields"][0]["note"] = serde_json::json!("PRIVATE-PROMPT-SENTINEL");
    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(
            &serde_json::to_vec(&secret_note).unwrap(),
            &binding,
        )
        .is_err()
    );

    let mut invented_compiled: serde_json::Value =
        serde_json::from_slice(&receipt.stable_bytes().unwrap()).unwrap();
    invented_compiled["fields"][8]["value"] = serde_json::json!(12);
    invented_compiled["fields"][8]["provenance"] = serde_json::json!("measured");
    invented_compiled["fields"][8]["producer"] = serde_json::json!("model_gateway");
    invented_compiled["fields"][8]["note"] = serde_json::json!("");
    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(
            &serde_json::to_vec(&invented_compiled).unwrap(),
            &binding,
        )
        .is_err()
    );

    let valid_bytes = receipt.stable_bytes().unwrap();
    let different_authority =
        execution_event_by_actor(Some(EXECUTION_ID), EXECUTION_ID, PersistedActorType::Agent);
    assert!(
        ExecutionAccountingReceipt::from_stable_bytes(&valid_bytes, &different_authority).is_err(),
        "a structurally valid receipt must still authenticate against its exact journal event"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **accepting the accounting
/// module itself as the producer of a measured field.**
///
/// Rule-resolution cost is observed inside Task 002's resolver, retrieval counts inside the
/// retrieval port, output tokens at the response boundary. The accounting module *records* them. If
/// it can name itself as the observer, then a number it recomputed — from a log, from a re-parse,
/// from an estimate — is indistinguishable in the receipt from one that was actually watched.
///
/// This is the mechanical form of the blueprint's rule: **a field whose producer is the accounting
/// module is derived by construction**, so `measured` plus that producer is a contradiction the
/// type must refuse rather than a convention a reviewer must remember.
#[test]
fn the_accounting_module_cannot_be_the_observer_of_a_measured_cost() {
    let honest = AccountingReceipt::new().with_field(
        "ruleResolutionTokens",
        CostField::measured(412, "code_contract_resolver"),
    );
    assert!(
        honest.is_ok(),
        "arrangement check: a field observed by a real producer must be accepted"
    );

    let self_attributed = AccountingReceipt::new().with_field(
        "ruleResolutionTokens",
        CostField::measured(412, ACCOUNTING_MODULE),
    );
    assert!(
        self_attributed.is_err(),
        "the accounting module was accepted as the observer of a MEASURED cost (#222).\n\
         A number the recorder produced is derived, not observed. Allowing it to sign as the \
         observer makes a recomputed value indistinguishable from a watched one, which is the \
         whole distinction the provenance tag exists to carry.\n\
         Either name the component that did the work, or mark the field derived."
    );
}

// ---------------------------------------------------------------------------------------------
// Cold and amortized index cost, which the acceptance criterion requires reported SEPARATELY.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::{
    INDEX_COST_AMORTIZED_FIELD, INDEX_COST_COLD_FIELD, IndexCost,
};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **reporting an
/// amortized share as a measured cost** -- `CostField::measured(total / runs, "index")`.
///
/// It is the tempting implementation, because a share is a number and every other number in a
/// receipt is measured. But nobody observed *this* run paying it: the cost was paid earlier by a
/// run that is not this one, and the share is arithmetic over that. Marked measured, it acquires an
/// observer that never watched anything, and a reader auditing "which numbers were seen" gets a
/// yes for a number that was computed.
#[test]
fn an_amortized_share_is_derived_because_nobody_watched_this_run_pay_it() {
    let share = IndexCost::amortized(900, 3).expect("three runs is a usable denominator");
    let field = share.as_cost_field();

    assert!(
        !field.is_measured(),
        "the amortized share reported itself as MEASURED. It names {:?} as its observer, and that \
         component did not watch this run pay anything -- the cost was paid by an earlier run.",
        field.producer()
    );
    assert_eq!(
        field.provenance(),
        &CostProvenance::Derived,
        "an amortized share is computed from a cost paid elsewhere, so its provenance is Derived"
    );
    assert_eq!(
        field.observed(),
        Some(300),
        "the share of 900 tokens over 3 runs is 300; the value must still be present, since \
         Derived means computed, not absent"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **letting the
/// cold arm lose its observer** -- reporting it derived, or measured against a generic name rather
/// than the component that did the work.
///
/// This is the discriminating half. A `as_cost_field` that returns Derived for everything satisfies
/// the test above perfectly and destroys the distinction it exists to protect: cold cost IS
/// observed, and marking it derived means no receipt anywhere can say a real index build was
/// watched.
#[test]
fn a_cold_index_cost_keeps_the_observer_that_watched_it() {
    let field = IndexCost::cold(1200, "native_index_builder").as_cost_field();

    assert!(
        field.is_measured(),
        "a cold index cost was paid by this run and watched; it must be measured, not {:?}",
        field.provenance()
    );
    assert_eq!(
        field.producer(),
        Some("native_index_builder"),
        "the cold cost must name the component that did the work, not a generic label"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **accepting a
/// denominator of zero**, returning a share of zero or of the full total.
///
/// A share over zero runs is not small, it is undefined. Returning the full total quietly turns an
/// amortized line into a cold one; returning zero quietly deletes a real cost. Both read as a
/// number and neither is one.
#[test]
fn an_amortized_share_over_zero_runs_is_refused_rather_than_valued() {
    let refusal = IndexCost::amortized(900, 0);
    let message = refusal.expect_err("a zero denominator must be refused, not valued");
    assert!(
        message.contains("ZERO"),
        "the refusal must say what was wrong with the input; it said: {message}"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **adding the two
/// costs into a single receipt line** -- an `index_cost` field carrying cold plus amortized.
///
/// The merged number is neither of its parts: larger than what this run paid and smaller than what
/// the index cost, carrying whichever provenance the summing code picked. The absence assertion
/// below is paired with the presence of both real lines, so a receipt that simply lost its index
/// accounting cannot satisfy it -- an absence guard with nothing present is satisfied by an empty
/// receipt.
#[test]
fn index_cost_lands_as_two_lines_and_never_as_one() {
    let receipt = AccountingReceipt::new()
        .with_field(
            INDEX_COST_COLD_FIELD,
            IndexCost::cold(1200, "native_index_builder").as_cost_field(),
        )
        .expect("a cold index cost is a legal receipt line")
        .with_field(
            INDEX_COST_AMORTIZED_FIELD,
            IndexCost::amortized(900, 3)
                .expect("three runs is a usable denominator")
                .as_cost_field(),
        )
        .expect("an amortized index cost is a legal receipt line");

    assert_eq!(
        receipt
            .field(INDEX_COST_COLD_FIELD)
            .and_then(|f| f.observed()),
        Some(1200),
        "the cold line must be present and hold what this run paid"
    );
    assert_eq!(
        receipt
            .field(INDEX_COST_AMORTIZED_FIELD)
            .and_then(|f| f.observed()),
        Some(300),
        "the amortized line must be present and hold this run's share"
    );
    assert!(
        receipt.field("index_cost").is_none(),
        "the receipt carries a merged `index_cost` line. It is neither number: too large to be \
         what this run paid, too small to be what the index cost, and reported under whichever \
         provenance the summing code picked."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **changing the
/// rounding of an amortized share** — to `div_ceil`, to a float, or to anything that reports more
/// than was paid.
///
/// Raised by F's cold pass: every other case here divides exactly (900 over 3), so the truncation
/// was invisible and a reader could not tell a chosen policy from an unnoticed one. This case uses
/// a denominator that does not divide, so the policy is pinned by a number rather than by a
/// comment, and the bound is asserted rather than asserted-about: the shares sum to at most the
/// real cost, short by strictly less than one token per sharing run.
#[test]
fn an_amortized_share_truncates_and_the_loss_is_bounded() {
    let total = 901u64;
    let runs = 3u32;
    let share = IndexCost::amortized(total, runs)
        .expect("three runs is a usable denominator")
        .as_cost_field();

    assert_eq!(
        share.observed(),
        Some(300),
        "901 over 3 truncates to 300. If this says 301, the rounding now reports more than was \
         ever paid, and a cost line that can overstate cannot be used to bound anything."
    );

    let summed = share.observed().expect("the share has a value") * u64::from(runs);
    assert!(
        summed <= total,
        "the shares sum to {summed}, which is MORE than the {total} actually paid"
    );
    assert!(
        total - summed < u64::from(runs),
        "the shortfall is {}, which is not under one token per run ({runs}). The bound is what \
         makes truncation safe to choose; without it the understatement is unbounded.",
        total - summed
    );
}

/// THIS ONE DOES NOT REDDEN — IT FAILS TO COMPILE, deliberately, and that is the point.
///
/// Raised by L's primary review, and it is the ironic one: the dimension table above is the guard
/// against a cache key that silently omits an input, and the table itself was **maintained by
/// hand** against the struct. Add a field to `ContextCacheKeyInputs` and every existing case still
/// passes — the omitted dimension is exactly the failure the table exists to catch, arriving
/// through the table.
///
/// The destructuring below has no `..`, so a new field stops this file compiling. A compile error
/// cannot be read as a pass, cannot be skipped, and lands on the author rather than on whoever
/// reads the report later. Then the count assertion forces the new name into the case list rather
/// than letting it be named and left uncovered.
///
/// **If you are here because this stopped compiling, do not take rustc's advice.** Verified by
/// sabotage — adding a field produced `error[E0027]: pattern does not mention field ...`, exactly
/// once and here, with the tree restored afterwards. The compiler then offers two remedies:
/// *"you can explicitly ignore it"* and *"or always ignore missing fields here"*. Both mean adding
/// `..`, both make this compile again in one keystroke, and both silently delete the guard —
/// leaving a cache key with a dimension nothing covers, which is a HIT across that dimension and a
/// correct-looking cheap answer. The remedy is to add the field to the list below **and** give it
/// a case in `dimension_cases`.
#[test]
fn every_field_of_the_key_inputs_has_a_dimension_case() {
    let ContextCacheKeyInputs {
        scope_project,
        permissions,
        repo_snapshot,
        index_generation,
        schema_id,
        schema_version,
        objective,
        capsule_digest,
        producer,
        utilization_policy_version,
    } = baseline();

    // Bound so the destructuring cannot be dismissed as unused and quietly replaced with `..`.
    let fields: [(&str, bool); 10] = [
        ("scope_project", !scope_project.is_empty()),
        ("permissions", !permissions.is_empty()),
        ("repo_snapshot", !repo_snapshot.is_empty()),
        ("index_generation", !index_generation.is_empty()),
        ("schema_id", !schema_id.is_empty()),
        ("schema_version", !schema_version.is_empty()),
        ("objective", !objective.is_empty()),
        ("capsule_digest", !capsule_digest.is_empty()),
        ("producer", !producer.is_empty()),
        (
            "utilization_policy_version",
            !utilization_policy_version.is_empty(),
        ),
    ];

    for (name, populated) in fields {
        assert!(
            populated,
            "the baseline leaves `{name}` empty, so the case that mutates it may not be changing \
             anything observable"
        );
        assert!(
            dimension_cases().iter().any(|(case, _)| *case == name),
            "`{name}` is a field of the cache key with no dimension case. A dimension with no case \
             is not a weaker guard, it is no guard: an omitted dimension produces a cache HIT \
             across it, which is a correct-looking cheap answer."
        );
    }

    assert_eq!(
        dimension_cases().len(),
        fields.len(),
        "there are {} dimension cases for {} fields. A case naming something that is not a field \
         is dead weight claiming to be coverage.",
        dimension_cases().len(),
        fields.len()
    );
}
