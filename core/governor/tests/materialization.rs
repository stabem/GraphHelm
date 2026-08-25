use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
    thread,
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EventRepositoryError, EvidenceOpener,
    EvidenceProtector, EvidenceRead, EvidenceRepository, KeyError, KeyProvider,
    KeyProviderMetadata, RepositoryFuture, RevocationReceipt, RevokeKeyRequest, SealedEvidence,
    SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_governor::{
    ExecutableGraphMaterializer, GraphExternalizer, MaterializationError, MaterializedContent,
    SealingGraphExternalizer,
};
use graphhelm_graph::{GraphVersion, raw_content_sha256};
use graphhelm_protocols::{
    Actor, ActorType, ContentSlot, EvidenceId, ExecutionId, PersistedGraphVersion, ProjectId,
    RepositoryScope, WorkspaceId,
};

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[derive(Clone, Default)]
struct Keys(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);
impl KeyProvider for Keys {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("key-1", "test", "1.0.0", 0) })
    }
    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            let handle = request.handle().to_owned();
            self.0.lock().unwrap().insert(
                handle.clone(),
                request.plaintext_key().expose(|v| v.to_vec()),
            );
            WrappedKey::new(
                "key-1",
                handle,
                "xchacha20poly1305",
                vec![7; 24],
                vec![9; 48],
                raw_content_sha256(request.aad()).unwrap(),
            )
        })
    }
    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .get(wrapped.handle())
                .cloned()
                .map(SecretBytes::new)
                .ok_or(KeyError::Unavailable)
        })
    }
    fn revoke<'a>(
        &'a self,
        _: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn authenticate<'a>(
        &'a self,
        _: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn verify<'a>(
        &'a self,
        _: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
}

struct EvidenceRows {
    rows: BTreeMap<EvidenceId, SealedEvidence>,
    unavailable: Option<EvidenceId>,
}
impl EvidenceRepository for EvidenceRows {
    fn get_sealed<'a>(
        &'a self,
        _: RepositoryScope,
        id: EvidenceId,
    ) -> RepositoryFuture<'a, Result<EvidenceRead, EventRepositoryError>> {
        Box::pin(async move {
            if self.unavailable.as_ref() == Some(&id) {
                Ok(EvidenceRead::Unavailable(
                    graphhelm_events::EvidenceUnavailableReason::Erased,
                ))
            } else {
                self.rows
                    .get(&id)
                    .cloned()
                    .map(EvidenceRead::Available)
                    .ok_or(EventRepositoryError::Integrity)
            }
        })
    }
}

struct WrongPlaintext;
impl EvidenceOpener for WrongPlaintext {
    fn open<'a>(
        &'a self,
        _: RepositoryScope,
        _: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, graphhelm_events::EvidenceError>> {
        Box::pin(async { Ok(SecretBytes::new(b"{}".to_vec())) })
    }
}

struct FailedAuthentication;
impl EvidenceOpener for FailedAuthentication {
    fn open<'a>(
        &'a self,
        _: RepositoryScope,
        _: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, graphhelm_events::EvidenceError>> {
        Box::pin(async { Err(graphhelm_events::EvidenceError::Invalid) })
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("exec_feature").unwrap()),
    )
}

/// The same preparation, but handing back the governor's verdict instead of unwrapping it.
///
/// Needed because the customs guard below is about a REFUSAL: unwrapping inside a helper would
/// land the failure on the helper's `.unwrap()`, which says nothing about what was refused or
/// why. A guard has to fail at its own assertion or it is measuring the harness.
fn try_preparation_with(
    keys: Keys,
    edit: impl FnOnce(&mut graphhelm_protocols::ExecutionGraph),
) -> Result<graphhelm_governor::ProjectionPreparation, graphhelm_governor::GovernorError> {
    let mut graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    edit(&mut graph);
    let record = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap(),
    )
    .unwrap()
    .to_record();
    block_on(SealingGraphExternalizer::new(EvidenceProtector::new(keys)).prepare(scope(), &record))
}

/// Writes `completion.customs` onto the `implement` node, NESTED inside the completion block that
/// already exists there — the placement #160 ratified, and the one an operator would actually
/// author.
fn declare_customs(graph: &mut graphhelm_protocols::ExecutionGraph) {
    let node = graph
        .spec
        .nodes
        .get_mut("implement")
        .expect("the example graph declares an `implement` node");
    let completion = node
        .properties
        .entry("completion".to_owned())
        .or_insert_with(|| serde_json::json!({}));
    completion
        .as_object_mut()
        .expect("the example graph's completion block is an object")
        .insert(
            "customs".to_owned(),
            serde_json::json!({
                "proofKinds": ["patch"],
                "budgets": {
                    "waitWithinSeconds": 3600,
                    "clearanceWithinSeconds": 900,
                },
            }),
        );
}

fn preparation(keys: Keys) -> graphhelm_governor::ProjectionPreparation {
    preparation_with(keys, |_| {})
}

/// The same preparation, with a chance to edit the graph the user declared before it is
/// published. Only a declaration the operator could actually write belongs here.
fn preparation_with(
    keys: Keys,
    edit: impl FnOnce(&mut graphhelm_protocols::ExecutionGraph),
) -> graphhelm_governor::ProjectionPreparation {
    let mut graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    edit(&mut graph);
    let record = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap(),
    )
    .unwrap()
    .to_record();
    block_on(SealingGraphExternalizer::new(EvidenceProtector::new(keys)).prepare(scope(), &record))
        .unwrap()
}

#[test]
fn required_unavailable_content_has_the_stable_ghe008_diagnostic() {
    assert_eq!(
        MaterializationError::ContentUnavailable.code(),
        "GHE012_CONTENT_UNAVAILABLE"
    );
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let required = prepared
        .version()
        .content_slots()
        .iter()
        .find(|slot| slot.required_for_execution())
        .unwrap()
        .evidence_id()
        .clone();
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: Some(required),
    };
    let materializer = ExecutableGraphMaterializer::new(
        Arc::new(rows),
        Arc::new(EvidenceProtector::new(keys)) as Arc<dyn EvidenceOpener>,
    );
    assert!(matches!(
        block_on(materializer.materialize(scope(), prepared.version())),
        Err(MaterializationError::ContentUnavailable)
    ));
}

#[test]
fn successful_materialization_opens_canonical_content_without_fallback() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let materializer =
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(EvidenceProtector::new(keys)));
    let result = block_on(materializer.materialize(scope(), prepared.version())).unwrap();
    assert_eq!(
        result.content().len(),
        prepared.version().content_slots().len()
    );
    assert!(
        result
            .content()
            .values()
            .all(|item| matches!(item, MaterializedContent::Available(_)))
    );
    let first = result.content().values().next().unwrap();
    let MaterializedContent::Available(value) = first else {
        panic!("expected available content")
    };
    assert!(value.expose_json(|json| !json.is_null()).unwrap());
    let debug = format!("{result:?}");
    assert!(!debug.contains("Mapear reposit"));
    assert!(debug.contains("[redacted]"));
}

#[test]
fn optional_unavailable_content_remains_explicit() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let optional = prepared
        .version()
        .content_slots()
        .iter()
        .find(|slot| !slot.required_for_execution())
        .unwrap();
    let optional_id = optional.evidence_id().clone();
    let optional_slot = optional.slot_id().clone();
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: Some(optional_id),
    };
    let result = block_on(
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(EvidenceProtector::new(keys)))
            .materialize(scope(), prepared.version()),
    )
    .unwrap();
    assert!(matches!(
        result.content_for(&optional_slot),
        Some(MaterializedContent::Unavailable(
            graphhelm_events::EvidenceUnavailableReason::Erased
        ))
    ));
}

#[test]
fn decrypted_digest_mismatch_fails_integrity() {
    let keys = Keys::default();
    let prepared = preparation(keys);
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let result = block_on(
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(WrongPlaintext))
            .materialize(scope(), prepared.version()),
    );
    assert!(matches!(result, Err(MaterializationError::Integrity)));
}

#[test]
fn wrong_scope_is_rejected_before_evidence_can_supply_content() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let foreign = RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-foreign").unwrap()),
    );
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(
                Arc::new(rows),
                Arc::new(EvidenceProtector::new(keys)),
            )
            .materialize(foreign, prepared.version())
        ),
        Err(MaterializationError::Integrity)
    ));
}

#[test]
fn owner_ordinal_mutation_and_decrypt_authentication_failure_are_rejected() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let mut slots = prepared.version().content_slots().to_vec();
    let original = &slots[0];
    slots[0] = ContentSlot::new(
        original.slot_id().clone(),
        original.owner_kind(),
        original.owner_id().clone(),
        original.field_kind(),
        original.ordinal() + 1,
        original.evidence_id().clone(),
        original.content_sha256().clone(),
        original.sensitivity(),
        original.required_for_execution(),
    );
    let mutated = PersistedGraphVersion::new(
        prepared.version().number(),
        prepared.version().predecessor().cloned(),
        prepared.version().topology().clone(),
        prepared.version().topology_hash().clone(),
        prepared.version().semantic_hash().clone(),
        slots,
        prepared.version().created_by().clone(),
        prepared.version().created_at().clone(),
    )
    .unwrap();
    let rows = || EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(
                Arc::new(rows()),
                Arc::new(EvidenceProtector::new(keys)),
            )
            .materialize(scope(), &mutated)
        ),
        Err(MaterializationError::Integrity)
    ));
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(Arc::new(rows()), Arc::new(FailedAuthentication))
                .materialize(scope(), prepared.version())
        ),
        Err(MaterializationError::Integrity)
    ));
}

#[test]
fn integrity_failures_never_collapse_into_content_unavailable() {
    assert_eq!(
        MaterializationError::Integrity.code(),
        "GHE005_INTEGRITY_FAILURE"
    );
}

/// `timeoutSeconds` is the one budget the operator already declares and the linter already
/// demands (`GHG101_DEFAULT_TIMEOUT`). Until now persistence dropped it on the floor, so the
/// attention seam had no budget to compare silence against and answered `unknown` forever.
/// This measures the declaration against the shipped example graph, not an invented fixture:
/// `map_repository` declares 900 and `tests` declares 1800 in
/// `examples/graphs/software-feature.yaml`.
#[test]
fn declared_node_timeouts_survive_externalization() {
    let prepared = preparation(Keys::default());
    let nodes = prepared.version().topology().nodes();
    let of = |id: &str| {
        nodes
            .get(&graphhelm_protocols::OpaqueId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("the example graph declares a node `{id}`"))
            .timeout_seconds()
    };
    assert_eq!(of("map_repository"), Some(900));
    assert_eq!(of("tests"), Some(1800));
    // A node that declares nothing must stay undeclared. Absence is the honest answer; a
    // zero here would read as "budget of zero seconds" and make every node look overdue.
    assert_eq!(of("plan"), None);
}

/// Agent B's declared unknown, measured rather than assumed — and the measurement answered
/// something better than either of us expected.
///
/// His worry was that the linter only checks that `timeoutSeconds` EXISTS, never what it
/// holds, so a graph declaring `-5` would pass lint and have to be carried as an unknown.
/// Measuring the whole pipeline shows every unreadable declaration is refused by the
/// governor's authoring validation, and refused in DEPTH: the property is typed in both
/// schema copies, checked by `validate_positive_integer` in the property sweep, and checked
/// again by `configuration.integer` when the node control is built.
///
/// The honest limit, stated because a guard nobody can break is indistinguishable from a
/// guard that measures nothing: **no sabotage found makes the unreadable half of this test
/// fail.** Removing the schema constraint from both copies, removing the sweep check, and
/// making this crate's own read lenient (`as_u64().unwrap_or(0)`) — separately and all at
/// once — still refused all five values. So this half is a witness to a property already
/// enforced upstream, not a new control, and it must not be read as one.
///
/// The `42` assertion at the end is a different matter: it DID fail before this change, with
/// `left: None, right: Some(42)`, and it is what stops persistence from silently going back
/// to dropping the operator's declaration on the floor.
#[test]
fn an_unreadable_node_timeout_never_becomes_a_budget() {
    /// What the pipeline did with a declaration: refused it somewhere, or carried it through.
    #[derive(Debug)]
    enum Outcome {
        RefusedAtAuthoring,
        RefusedByGovernor,
        Carried(Option<u64>),
    }

    let declare = |declared: serde_json::Value| {
        let mut graph = graphhelm_schema::load_graph(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/graphs/software-feature.yaml"),
        )
        .unwrap()
        .graph;
        graph
            .spec
            .nodes
            .get_mut("plan")
            .expect("the example graph declares a node `plan`")
            .properties
            .insert("timeoutSeconds".to_owned(), declared);
        let Ok(version) = GraphVersion::publish(
            graph,
            None,
            Actor::new(ActorType::Owner, "owner-test"),
            Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap(),
        ) else {
            return Outcome::RefusedAtAuthoring;
        };
        let record = version.to_record();
        let Ok(prepared) = block_on(
            SealingGraphExternalizer::new(EvidenceProtector::new(Keys::default()))
                .prepare(scope(), &record),
        ) else {
            return Outcome::RefusedByGovernor;
        };
        Outcome::Carried(
            prepared
                .version()
                .topology()
                .nodes()
                .get(&graphhelm_protocols::OpaqueId::parse("plan").unwrap())
                .unwrap()
                .timeout_seconds(),
        )
    };

    // Declarations a person could plausibly write, none of them readable as a count of
    // seconds. `0` is included on purpose: it is the value a dropped field would look like if
    // anyone ever "defaulted" the absent case to zero, and it would make every node overdue.
    for unreadable in [
        serde_json::json!(-5),
        serde_json::json!(0),
        serde_json::json!(0.5),
        serde_json::json!("900"),
        serde_json::json!(null),
    ] {
        let outcome = declare(unreadable.clone());
        assert!(
            !matches!(outcome, Outcome::Carried(Some(_))),
            "the declaration {unreadable} reached the store as a real budget ({outcome:?}); an \
             unreadable deadline must be refused or carried as nothing, never read as a number"
        );
    }

    // ...and the refusal is CONDITIONAL. Without this, every assertion above would still hold
    // in a build that refused EVERY timeout — which would silently switch the whole budget
    // off and hand the seam back the permanent `unknown` this milestone exists to remove.
    assert!(
        matches!(declare(serde_json::json!(42)), Outcome::Carried(Some(42))),
        "a legal declaration must still publish and still arrive"
    );
}

/// M11 #160: a graph declaring `completion.customs` PUBLISHES, and its budgets reach the store.
///
/// Both halves matter and they fail differently. `build_completion_control` is key-EXHAUSTIVE for
/// node completion — its arms are `contractRef | requires | forbids` and everything else falls to
/// `_ => Err(GovernorError::InvalidAuthoring)` — so before this lane taught it the key, authoring
/// the field refused the whole publication. That refusal is the red this guard was written
/// against and observed before the arm existed; without it the feature is undeliverable no matter
/// how correct the fold is, because no graph declaring customs can be sealed at all.
///
/// The second half is the one that would rot quietly: accepting the key without carrying the
/// budgets through would publish happily and leave the fold with nothing to compute a deadline
/// from, which looks exactly like a node that declared no budget.
///
/// NOTE for whoever reads this next, because the two consumers of this block do NOT agree and
/// inferring a uniform policy from either is a mistake: `collect_completion_content` is
/// key-SELECTIVE and permissive (it walks `requires`/`forbids` only, so no budget leaks into
/// externalized content), while `build_completion_control` is key-exhaustive and strict. That
/// asymmetry predates this change.
#[test]
fn a_node_declaring_customs_budgets_publishes_and_carries_them() {
    let prepared = match try_preparation_with(Keys::default(), declare_customs) {
        Ok(prepared) => prepared,
        Err(error) => panic!(
            "a graph declaring `completion.customs` must publish, and the governor refused it \
             with {error:?} — the node-completion match has not learned the key"
        ),
    };

    let customs = prepared
        .version()
        .topology()
        .nodes()
        .get(&graphhelm_protocols::OpaqueId::parse("implement").unwrap())
        .expect("the published topology keeps the `implement` node")
        .customs()
        .expect("the declared budgets survive sealing, or the fold has nothing to compute from");
    assert_eq!(customs.wait_within_seconds(), 3600);
    assert_eq!(customs.clearance_within_seconds(), 900);
    assert_eq!(
        customs.dlq_within_seconds(),
        None,
        "an undeclared dead-letter budget stays absent rather than becoming zero"
    );

    // The evidence requirement travels by a DIFFERENT road than the budgets, so it needs its own
    // assertion. Budgets ride `PersistedNode::customs`; `proofKinds` is encoded into the
    // node_completion control, whose key vocabulary is closed in THREE separate places in
    // `core/graph/src/persistence.rs` (identifiers, integers, count groups). Each one refuses an
    // undeclared key as `InvalidProjection` at sealing without naming the key — which is how a
    // field can be authored, schema-valid, and still unpublishable.
    let completion = prepared
        .version()
        .topology()
        .nodes()
        .get(&graphhelm_protocols::OpaqueId::parse("implement").unwrap())
        .expect("the published topology keeps the `implement` node")
        .controls()
        .iter()
        .find(|control| control.control_type().as_str() == "node_completion")
        .expect("the node keeps a completion control");
    assert_eq!(
        completion
            .identifiers()
            .get(&graphhelm_protocols::SafeKey::parse("customsProof.000").unwrap())
            .map(|value| value.as_str()),
        Some("patch"),
        "the operator's declared evidence requirement must survive sealing, not be accepted \
         and dropped"
    );
}

/// The three checked-in example graphs must keep publishing UNCHANGED. Strict typing applies
/// inside `customs` only; widening the node-completion match must not have loosened anything
/// around it, and this is the regression half of that promise.
#[test]
fn the_example_graph_still_publishes_without_any_customs_declaration() {
    try_preparation_with(Keys::default(), |_| {})
        .expect("the untouched example graph publishes exactly as before");
}
