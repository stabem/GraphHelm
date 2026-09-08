use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ArtifactRegistration, EventRepositoryError, LocalEventRepository, LocalFailpoint,
    LocalRepositoryInspection, PreparedAppend, SealedEvidence, WrappedKey,
};

#[test]
fn read_only_inspection_reports_a_missing_root_without_creating_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("missing-repository");

    assert_eq!(
        LocalEventRepository::inspect_repository(&root).unwrap(),
        LocalRepositoryInspection::Missing
    );
    assert!(
        !root.exists(),
        "inspection must not create the selected root"
    );
}

#[test]
fn read_only_inspection_rejects_an_ordinary_file_in_the_root_component_walk() {
    let directory = tempfile::tempdir().unwrap();
    let ancestor = directory.path().join("ordinary-file");
    std::fs::write(&ancestor, b"not a directory").unwrap();

    let error = LocalEventRepository::inspect_repository(&ancestor.join("repository")).unwrap_err();

    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert_eq!(std::fs::read(&ancestor).unwrap(), b"not a directory");
}

#[test]
fn read_only_inspection_does_not_treat_an_empty_selection_as_the_current_directory() {
    assert_eq!(
        LocalEventRepository::inspect_repository(std::path::Path::new("")).unwrap(),
        LocalRepositoryInspection::Missing
    );
}

#[test]
fn read_only_inspection_reports_a_format_only_root_as_incomplete_without_repair() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let format = b"{\"formatVersion\":\"1.0.0\"}\n";
    std::fs::write(root.join("format.json"), format).unwrap();

    assert_eq!(
        LocalEventRepository::inspect_repository(&root).unwrap(),
        LocalRepositoryInspection::Integrity
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    assert_eq!(std::fs::read(root.join("format.json")).unwrap(), format);
}

#[test]
fn read_only_inspection_rejects_a_nonempty_partial_directory_as_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let blobs = root.join("blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    std::fs::write(blobs.join("first"), b"one").unwrap();
    std::fs::write(blobs.join("second"), b"two").unwrap();

    let error = LocalEventRepository::inspect_repository(&root).unwrap_err();

    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert_eq!(std::fs::read(blobs.join("first")).unwrap(), b"one");
    assert_eq!(std::fs::read(blobs.join("second")).unwrap(), b"two");
}

#[test]
fn read_only_inspection_keeps_pre_format_wrong_slot_types_unsupported() {
    for (name, directory_slot) in [("blobs", true), ("journal.jsonl", false)] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        std::fs::create_dir(&root).unwrap();
        if directory_slot {
            std::fs::write(root.join(name), b"ordinary file").unwrap();
        } else {
            std::fs::create_dir(root.join(name)).unwrap();
        }

        let error = LocalEventRepository::inspect_repository(&root).unwrap_err();

        assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT", "{name}");
    }
}

#[test]
fn read_only_inspection_reports_directories_in_required_file_slots_as_integrity() {
    for name in ["journal.jsonl", "repository.lock"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        drop(repository(&root));
        std::fs::remove_file(root.join(name)).unwrap();
        std::fs::create_dir(root.join(name)).unwrap();

        let outcome = LocalEventRepository::inspect_repository(&root).unwrap();

        assert_eq!(outcome, LocalRepositoryInspection::Integrity, "{name}");
        assert!(root.join(name).is_dir(), "{name}");
    }
}

#[test]
fn read_only_inspection_reports_files_in_required_directory_slots_as_integrity() {
    for name in ["blobs", ".tmp", "active"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        drop(repository(&root));
        std::fs::remove_dir(root.join(name)).unwrap();
        std::fs::write(root.join(name), b"ordinary file").unwrap();

        let outcome = LocalEventRepository::inspect_repository(&root).unwrap();

        assert_eq!(outcome, LocalRepositoryInspection::Integrity, "{name}");
        assert!(root.join(name).is_file(), "{name}");
    }
}

#[test]
fn read_only_inspection_recognizes_complete_and_recoverable_layouts_without_mutation() {
    for missing in [
        &[][..],
        &[".tmp"][..],
        &["active"][..],
        &[".tmp", "active"][..],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        drop(repository(&root));
        for name in missing {
            std::fs::remove_dir(root.join(name)).unwrap();
        }
        let names_before = repository_root_names(&root);
        let format_before = std::fs::read(root.join("format.json")).unwrap();
        let journal_before = std::fs::read(root.join("journal.jsonl")).unwrap();
        let lock_before = std::fs::read(root.join("repository.lock")).unwrap();

        assert_eq!(
            LocalEventRepository::inspect_repository(&root).unwrap(),
            LocalRepositoryInspection::Recognized
        );
        assert_eq!(repository_root_names(&root), names_before);
        assert_eq!(
            std::fs::read(root.join("format.json")).unwrap(),
            format_before
        );
        assert_eq!(
            std::fs::read(root.join("journal.jsonl")).unwrap(),
            journal_before
        );
        assert_eq!(
            std::fs::read(root.join("repository.lock")).unwrap(),
            lock_before
        );
    }
}

#[test]
fn read_only_inspection_accepts_a_store_that_refuses_writer_access() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    drop(repository(&root));

    #[cfg(windows)]
    let _writer_denials = deny_writer_access(&root);
    #[cfg(unix)]
    make_store_read_only(&root);

    #[cfg(windows)]
    {
        let error = match LocalEventRepository::open(
            &root,
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        ) {
            Ok(_) => panic!("control failed: writer open bypassed the denied write share"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "GHE008_STORAGE_FAILURE");
    }

    #[cfg(unix)]
    {
        let error = match LocalEventRepository::open(
            &root,
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        ) {
            Ok(_) => {
                make_store_writable(&root);
                panic!("OBSERVER_MISSING: Unix process bypassed the denied writer permissions");
            }
            Err(error) => error,
        };
        assert_eq!(error.code(), "GHE008_STORAGE_FAILURE");
    }

    assert_eq!(
        LocalEventRepository::inspect_repository(&root).unwrap(),
        LocalRepositoryInspection::Recognized
    );

    #[cfg(unix)]
    make_store_writable(&root);
}

#[cfg(windows)]
fn deny_writer_access(root: &std::path::Path) -> Vec<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ};

    ["journal.jsonl", "repository.lock"]
        .into_iter()
        .map(|name| {
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                .open(root.join(name))
                .unwrap()
        })
        .collect()
}

#[cfg(unix)]
fn make_store_read_only(root: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    for name in ["format.json", "journal.jsonl", "repository.lock"] {
        std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o400)).unwrap();
    }
    for name in ["blobs", ".tmp", "active"] {
        std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o500)).unwrap();
}

#[cfg(unix)]
fn make_store_writable(root: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["blobs", ".tmp", "active"] {
        std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for name in ["format.json", "journal.jsonl", "repository.lock"] {
        std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn read_only_inspection_preserves_unsupported_format_and_unsafe_link_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let unsupported = br#"{"formatVersion":0}"#;
    std::fs::write(root.join("format.json"), unsupported).unwrap();

    let error = LocalEventRepository::inspect_repository(&root).unwrap_err();
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert_eq!(
        std::fs::read(root.join("format.json")).unwrap(),
        unsupported
    );

    let linked = directory.path().join("linked-repository");
    if let Err(error) = create_directory_link(&root, &linked) {
        if is_windows_symlink_privilege_error(&error) {
            panic!(
                "OBSERVER_MISSING: Windows cannot create the final-root reparse fixture: {error}"
            );
        }
        panic!("failed to construct root-link attack: {error}");
    }
    let error = LocalEventRepository::inspect_repository(&linked).unwrap_err();
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");

    let real_parent = directory.path().join("real-parent");
    std::fs::create_dir(&real_parent).unwrap();
    let nested = real_parent.join("repository");
    drop(repository(&nested));
    let linked_parent = directory.path().join("linked-parent");
    if let Err(error) = create_directory_link(&real_parent, &linked_parent) {
        if is_windows_symlink_privilege_error(&error) {
            panic!("OBSERVER_MISSING: Windows cannot create the ancestor reparse fixture: {error}");
        }
        panic!("failed to construct ancestor-link attack: {error}");
    }
    let error =
        LocalEventRepository::inspect_repository(&linked_parent.join("repository")).unwrap_err();
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
}

#[test]
fn read_only_inspection_accepts_absolute_and_relative_selected_paths() {
    let directory = tempfile::Builder::new()
        .prefix("graphhelm-inspection-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let absolute = directory.path().join("absolute-repository");
    drop(repository(&absolute));
    assert_eq!(
        LocalEventRepository::inspect_repository(&absolute).unwrap(),
        LocalRepositoryInspection::Recognized
    );

    let relative = absolute
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap();
    assert_eq!(
        LocalEventRepository::inspect_repository(relative).unwrap(),
        LocalRepositoryInspection::Recognized
    );
}

fn repository_root_names(root: &std::path::Path) -> Vec<String> {
    let mut names = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}
use graphhelm_protocols::{
    ActorId, ArtifactId, ArtifactLocator, ArtifactReference, Clock, DraftProposed, EventKind,
    EvidenceId, EvidenceReference, GraphImported, GraphSourceKind, IdGenerator, MediaType,
    NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    SemanticVersion, Sensitivity, WireHash, WorkspaceId,
};
use sha2::{Digest, Sha256};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
    )
}

fn event(reference: Option<EvidenceReference>, sensitivity: Sensitivity) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse("request-1").unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-1").unwrap(),
        ),
        sensitivity,
        EventKind::GraphImported(GraphImported {
            source_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
            source_kind: GraphSourceKind::GraphDocument,
        }),
        reference.into_iter().collect(),
        vec![],
    )
}

fn repository(path: &std::path::Path) -> LocalEventRepository {
    LocalEventRepository::open(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
        .unwrap()
}

fn prepared(reference: Option<EvidenceReference>, evidence: Vec<SealedEvidence>) -> PreparedAppend {
    PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![event(reference, Sensitivity::Internal)],
        evidence,
        vec![],
    )
    .unwrap()
}

fn artifact(id: &str, digest: char, bytes: u64) -> ArtifactReference {
    let digest = digest.to_string().repeat(64);
    ArtifactReference::new(
        ArtifactId::parse(id).unwrap(),
        ArtifactLocator::parse(format!("artifact://sha256/{digest}")).unwrap(),
        RawSha256::parse(digest).unwrap(),
        MediaType::parse("application/json").unwrap(),
        bytes,
        Sensitivity::Internal,
        SemanticVersion::parse("1.0.0").unwrap(),
    )
    .unwrap()
}

fn artifact_event(key: &str, references: Vec<ArtifactReference>) -> NewEvent {
    let mut event = event_with_key(key);
    event.artifact_refs = references;
    event
}

#[test]
fn artifact_registration_is_owned_by_its_exact_producing_event() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let reference = artifact("artifact-1", 'a', 7);
    let registration = ArtifactRegistration::new(reference.clone(), "producer-b").unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![
            artifact_event("producer-a", vec![reference]),
            artifact_event("producer-b", vec![]),
        ],
        vec![],
        vec![registration],
    )
    .unwrap();
    assert_eq!(
        repository.append_atomic(&request).unwrap_err().code(),
        "GHE004_INVALID_EVENT"
    );

    let unreferenced = artifact("artifact-2", 'b', 9);
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![artifact_event("producer-c", vec![])],
        vec![],
        vec![ArtifactRegistration::new(unreferenced, "producer-c").unwrap()],
    )
    .unwrap();
    assert_eq!(
        repository.append_atomic(&request).unwrap_err().code(),
        "GHE004_INVALID_EVENT"
    );
}

#[test]
fn artifact_catalog_rejects_divergent_reregistration_before_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let original = artifact("artifact-1", 'a', 7);
    repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![artifact_event("producer-a", vec![original.clone()])],
                vec![],
                vec![ArtifactRegistration::new(original, "producer-a").unwrap()],
            )
            .unwrap(),
        )
        .unwrap();
    let divergent = artifact("artifact-1", 'b', 8);
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        2,
        vec![artifact_event("producer-b", vec![divergent.clone()])],
        vec![],
        vec![ArtifactRegistration::new(divergent, "producer-b").unwrap()],
    )
    .unwrap();
    assert_eq!(
        repository.append_atomic(&request).unwrap_err().code(),
        "GHE004_INVALID_EVENT"
    );
    assert_eq!(
        repository
            .read_stream(&scope(), "stream-1", 10, None)
            .unwrap()
            .events
            .len(),
        1
    );
}

#[test]
fn artifact_identity_cannot_be_reused_from_a_different_producer_stream() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let reference = artifact("artifact-stream-bound", 'd', 11);
    repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![artifact_event("producer-a", vec![reference.clone()])],
                vec![],
                vec![ArtifactRegistration::new(reference.clone(), "producer-a").unwrap()],
            )
            .unwrap(),
        )
        .unwrap();

    let cross_stream = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-2").unwrap(),
        1,
        vec![artifact_event("producer-a", vec![reference.clone()])],
        vec![],
        vec![ArtifactRegistration::new(reference, "producer-a").unwrap()],
    )
    .unwrap();
    assert_eq!(
        repository.append_atomic(&cross_stream).unwrap_err().code(),
        "GHE004_INVALID_EVENT"
    );
}

/// A batch of tiny events is charged the bytes it actually serializes to, not a per-event
/// synthetic charge against `MAX_EVENT_BYTES`.
///
/// **The count moved from 4,096 to 128, and the reason is a defect this cell used to certify
/// (#744).** A 4,096-event batch was accepted here and then could not be read back at all: the
/// physical-batch schema validator declines to process a document that large, `parse_physical_batch`
/// reported the refusal as `Integrity`, and because `open` parses every journal line, the whole
/// repository became unopenable -- measured on `361080f5`, where sizes at and above roughly 160
/// tiny events all ended `open: GHE005_INTEGRITY_FAILURE`. This cell never re-read what it wrote,
/// so it passed on a store it had bricked, and the append it was proving legal is now correctly
/// refused with `GHE006_LIMIT_EXCEEDED`.
///
/// 128 still discriminates the property this cell is for. A synthetic per-event charge of
/// `MAX_EVENT_BYTES` (1 MiB) puts 128 events at 128 MiB against a 16 MiB `MAX_BATCH_BYTES`, so
/// the synthetic accounting this cell exists to forbid is refused eight times over at the reduced
/// count -- verified by restoring that accounting and watching this cell go red, not by arithmetic
/// alone.
///
/// The read-back is new. Its absence is why the count was wrong for as long as it was.
///
/// **Do not raise this count back "to be thorough."** 4,096 was not too big by accident; it was
/// too big BECAUSE nothing here re-read what it wrote, so the cell passed on a store it had
/// bricked. The read-back below is what makes a raised count fail loudly instead of quietly, and
/// the store now refuses such a batch at the door -- but a reviewer who sees a small number and
/// reads it as timidity is the person this paragraph is for.
#[test]
fn many_tiny_events_use_real_serialized_bytes_not_synthetic_charges() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let events = (0..128)
        .map(|index| event_with_key(&format!("request-{index}")))
        .collect::<Vec<_>>();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        events,
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(repository.append_atomic(&request).unwrap().len(), 128);
    assert_eq!(
        repository
            .read_stream(&scope(), "stream-1", 1_000, None)
            .unwrap()
            .events
            .len(),
        128,
        "a batch this store accepted must be one it can read back"
    );
}

#[test]
fn exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let repo = repository(directory.path());
    let request = prepared(None, vec![]);
    let first = repo.append_atomic(&request).unwrap();
    let retry = repo.append_atomic(&request).unwrap();
    assert_eq!(retry, first);

    let divergent = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![event(None, Sensitivity::Restricted)],
        vec![],
        vec![],
    )
    .unwrap();
    let error = repo.append_atomic(&divergent).unwrap_err();
    assert_eq!(error.code(), "GHE003_IDEMPOTENCY_CONFLICT");
    assert_eq!(
        repo.read_stream(&scope(), "stream-1", 100, None)
            .unwrap()
            .events,
        first
    );
}

/// The same key and the SAME events at a LATER sequence is a conflict, not a replay.
///
/// The guard above covers divergence by event CONTENT while the expected sequence stays put.
/// This covers the other axis, and nothing pinned it: identical events, identical key, only the
/// expected sequence moved. The two axes reach different arms of the same branch — the replay
/// arm returns the original batch and writes nothing, the conflict arm refuses — and a caller
/// cannot tell them apart from the journal afterwards, because neither one appends.
///
/// A caller does lean on the difference. The wake recorder keys each consumption with the
/// sequence it is appending at and reports how many it recorded; that count is honest only
/// because a second attempt at a later sequence is REFUSED rather than answered with the first
/// attempt's events. If this branch ever resolved by replay instead, the recorder would report
/// a count for events it never wrote, and nothing in its own crate would notice. The invariant
/// lives here; the code that depends on it lives two crates away and says so nowhere.
#[test]
fn the_same_key_at_a_later_sequence_conflicts_rather_than_replaying() {
    let directory = tempfile::tempdir().unwrap();
    let repo = repository(directory.path());
    let first = repo.append_atomic(&prepared(None, vec![])).unwrap();

    // POSITIVE CONTROL, and it must come before the question it makes answerable.
    //
    // The assertion below says a colliding key at a LATER sequence takes the conflict exit. That
    // says nothing unless the OTHER exit exists: a store that resolved nothing as a retry — a
    // digest salted per call, say — would satisfy it while the replay path was dead, and the
    // test would report a property it never measured. So the retry is exercised here, in this
    // fixture, rather than left to the neighbouring guard: an assertion whose control lives in
    // another test is one refactor of that test away from measuring nothing.
    let retry = repo.append_atomic(&prepared(None, vec![])).unwrap();
    assert_eq!(
        retry, first,
        "control: the exact retry must resolve as a replay, returning the first attempt's \
         events — without this the conflict assertion below cannot tell a working refusal \
         from a store that never replays at all"
    );

    // Identical events, identical idempotency key. Only the expected sequence differs.
    let later = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        2,
        vec![event(None, Sensitivity::Internal)],
        vec![],
        vec![],
    )
    .unwrap();

    let error = repo.append_atomic(&later).unwrap_err();
    assert_eq!(
        error.code(),
        "GHE003_IDEMPOTENCY_CONFLICT",
        "a key already spent must be refused at a later sequence, not resolved as a retry"
    );
    assert_eq!(
        repo.read_stream(&scope(), "stream-1", 100, None)
            .unwrap()
            .events,
        first,
        "and the stream is untouched by the refusal"
    );
}

#[test]
fn direct_append_rejects_secret_shaped_persistent_surfaces_without_mutation() {
    const CANARY: &str = "sk-abcdefghijklmnopqrst";
    type RequestMutation = (&'static str, Box<dyn Fn() -> PreparedAppend>);
    let cases: Vec<RequestMutation> = vec![
        (
            "scope",
            Box::new(|| {
                PreparedAppend::new(
                    RepositoryScope::new(
                        WorkspaceId::parse(CANARY).unwrap(),
                        ProjectId::parse("project-1").unwrap(),
                        Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
                    ),
                    OpaqueId::parse("stream-1").unwrap(),
                    1,
                    vec![event(None, Sensitivity::Internal)],
                    vec![],
                    vec![],
                )
                .unwrap()
            }),
        ),
        (
            "idempotency",
            Box::new(|| {
                PreparedAppend::new(
                    scope(),
                    OpaqueId::parse("stream-1").unwrap(),
                    1,
                    vec![event_with_key(CANARY)],
                    vec![],
                    vec![],
                )
                .unwrap()
            }),
        ),
        (
            "actor",
            Box::new(|| {
                let mut unsafe_event = event(None, Sensitivity::Internal);
                unsafe_event.actor = PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse(CANARY).unwrap(),
                );
                PreparedAppend::new(
                    scope(),
                    OpaqueId::parse("stream-1").unwrap(),
                    1,
                    vec![unsafe_event],
                    vec![],
                    vec![],
                )
                .unwrap()
            }),
        ),
        (
            "payload",
            Box::new(|| {
                let mut unsafe_event = event(None, Sensitivity::Internal);
                unsafe_event.kind = EventKind::DraftProposed(DraftProposed {
                    draft_id: OpaqueId::parse(CANARY).unwrap(),
                    expected_version: 1,
                    expected_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                    operation_count: 1,
                });
                PreparedAppend::new(
                    scope(),
                    OpaqueId::parse("stream-1").unwrap(),
                    1,
                    vec![unsafe_event],
                    vec![],
                    vec![],
                )
                .unwrap()
            }),
        ),
        (
            "artifact",
            Box::new(|| {
                let reference = artifact(CANARY, 'a', 7);
                PreparedAppend::new(
                    scope(),
                    OpaqueId::parse("stream-1").unwrap(),
                    1,
                    vec![artifact_event("producer-a", vec![reference.clone()])],
                    vec![],
                    vec![ArtifactRegistration::new(reference, "producer-a").unwrap()],
                )
                .unwrap()
            }),
        ),
        (
            "evidence-metadata",
            Box::new(|| {
                let valid = sealed_evidence();
                let wrapped = WrappedKey::new(
                    CANARY,
                    valid.wrapped_key().handle(),
                    valid.wrapped_key().algorithm(),
                    valid.wrapped_key().nonce().to_vec(),
                    valid.wrapped_key().ciphertext().to_vec(),
                    valid.wrapped_key().aad_sha256().clone(),
                )
                .unwrap();
                let unsafe_evidence = SealedEvidence::new(
                    valid.reference().clone(),
                    valid.scope().clone(),
                    valid.media_type().as_str(),
                    valid.sensitivity(),
                    valid.retention_class(),
                    valid.algorithm(),
                    valid.nonce().to_vec(),
                    valid.ciphertext().to_vec(),
                    wrapped,
                )
                .unwrap();
                prepared(
                    Some(unsafe_evidence.reference().clone()),
                    vec![unsafe_evidence],
                )
            }),
        ),
    ];

    for (name, request) in cases {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(directory.path());
        let journal = directory.path().join("journal.jsonl");
        let before = std::fs::read(&journal).unwrap();
        let error = repository.append_atomic(&request()).unwrap_err();
        assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED", "{name}");
        assert!(!format!("{error:?} {error}").contains(CANARY), "{name}");
        assert_eq!(std::fs::read(&journal).unwrap(), before, "{name}");
        assert_eq!(
            std::fs::read_dir(directory.path().join("blobs"))
                .unwrap()
                .count(),
            0,
            "{name}"
        );
    }
}

#[test]
fn generated_envelope_identity_is_scanned_before_persistence() {
    struct SecretEventIds;
    impl IdGenerator for SecretEventIds {
        fn next_id(&self, prefix: &'static str) -> String {
            if prefix == "event" {
                "sk-abcdefghijklmnopqrst".into()
            } else {
                format!("{prefix}-safe")
            }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SecretEventIds),
    )
    .unwrap();
    let error = repository
        .append_atomic(&prepared(None, vec![]))
        .unwrap_err();
    assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
    assert_eq!(
        std::fs::metadata(directory.path().join("journal.jsonl"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn ciphertext_bytes_are_not_scanned_as_plaintext_content() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let sealed = sealed_evidence_with_ciphertext(b"sk-abcdefghijklmnopqrst".to_vec());
    let reference = sealed.reference().clone();
    assert_eq!(
        repository
            .append_atomic(&prepared(Some(reference), vec![sealed]))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn identical_idempotency_key_is_independent_across_streams() {
    let directory = tempfile::tempdir().unwrap();
    let repo = repository(directory.path());
    repo.append_atomic(&prepared(None, vec![])).unwrap();
    let other_stream = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-2").unwrap(),
        1,
        vec![event(None, Sensitivity::Internal)],
        vec![],
        vec![],
    )
    .unwrap();
    let committed = repo.append_atomic(&other_stream).unwrap();
    assert_eq!(committed[0].stream_id.as_str(), "stream-2");
    assert_eq!(committed[0].sequence, 1);
    let other_scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-2").unwrap(),
        Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
    );
    let other_scope_request = PreparedAppend::new(
        other_scope,
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![event(None, Sensitivity::Internal)],
        vec![],
        vec![],
    )
    .unwrap();
    let committed = repo.append_atomic(&other_scope_request).unwrap();
    assert_eq!(committed[0].sequence, 1);
}

#[test]
fn physically_published_orphan_is_not_reported_as_committed_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let repo = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence();
    let reference = sealed.reference().clone();
    assert!(
        repo.append_atomic(&prepared(Some(reference.clone()), vec![sealed]))
            .is_err()
    );
    assert!(
        !repo
            .evidence_exists(&scope(), reference.evidence_id())
            .unwrap()
    );
}

#[test]
fn invalid_prepared_wrapped_key_aad_is_rejected_before_any_durable_write() {
    let directory = tempfile::tempdir().unwrap();
    let repo = repository(directory.path());
    let valid = sealed_evidence();
    let invalid_wrapped = WrappedKey::new(
        valid.wrapped_key().key_id(),
        valid.wrapped_key().handle(),
        valid.wrapped_key().algorithm(),
        valid.wrapped_key().nonce().to_vec(),
        valid.wrapped_key().ciphertext().to_vec(),
        RawSha256::parse("f".repeat(64)).unwrap(),
    )
    .unwrap();
    let invalid = SealedEvidence::new(
        valid.reference().clone(),
        valid.scope().clone(),
        valid.media_type().as_str(),
        valid.sensitivity(),
        valid.retention_class(),
        valid.algorithm(),
        valid.nonce().to_vec(),
        valid.ciphertext().to_vec(),
        invalid_wrapped,
    )
    .unwrap();
    let journal = directory.path().join("journal.jsonl");
    let before = std::fs::read(&journal).unwrap();
    assert_eq!(
        repo.append_atomic(&prepared(Some(invalid.reference().clone()), vec![invalid]))
            .unwrap_err()
            .code(),
        "GHE004_INVALID_EVENT"
    );
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(directory.path().join("blobs"))
            .unwrap()
            .count(),
        0
    );
    drop(repo);
    repository(directory.path());
}

#[test]
fn near_maximum_stored_evidence_can_be_verified_on_retry_after_blob_publish() {
    let directory = tempfile::tempdir().unwrap();
    let repo = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence_with_ciphertext_len(16 * 1024 * 1024 + 16);
    let request = prepared(Some(sealed.reference().clone()), vec![sealed]);
    assert_eq!(
        repo.append_atomic(&request).unwrap_err().code(),
        "GHE008_STORAGE_FAILURE"
    );
    assert_eq!(
        repo.append_atomic(&request).unwrap_err().code(),
        "GHE008_STORAGE_FAILURE"
    );
}

#[test]
fn every_stored_evidence_field_is_bound_to_the_committed_reference_and_request() {
    type EvidenceMutation = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);
    let mutations: Vec<EvidenceMutation> = vec![
        (
            "format",
            Box::new(|v| v["formatVersion"] = serde_json::json!("2.0.0")),
        ),
        (
            "scope",
            Box::new(|v| v["scope"]["projectId"] = serde_json::json!("project-other")),
        ),
        (
            "evidence-id",
            Box::new(|v| v["reference"]["evidenceId"] = serde_json::json!("evidence-2")),
        ),
        (
            "content-digest",
            Box::new(|v| v["reference"]["contentSha256"] = serde_json::json!("e".repeat(64))),
        ),
        (
            "cipher-digest",
            Box::new(|v| v["reference"]["ciphertextSha256"] = serde_json::json!("e".repeat(64))),
        ),
        (
            "media",
            Box::new(|v| v["mediaType"] = serde_json::json!("text/plain")),
        ),
        (
            "sensitivity",
            Box::new(|v| v["sensitivity"] = serde_json::json!("restricted")),
        ),
        (
            "retention",
            Box::new(|v| v["retentionClass"] = serde_json::json!("legal_hold")),
        ),
        (
            "algorithm",
            Box::new(|v| v["algorithm"] = serde_json::json!("other")),
        ),
        (
            "nonce",
            Box::new(|v| v["nonceHex"] = serde_json::json!("04".repeat(24))),
        ),
        (
            "ciphertext",
            Box::new(|v| v["ciphertextHex"] = serde_json::json!("08".repeat(16))),
        ),
        (
            "wrapped-key-id",
            Box::new(|v| v["wrappedKey"]["keyId"] = serde_json::json!("key-2")),
        ),
        (
            "wrapped-handle",
            Box::new(|v| v["wrappedKey"]["handle"] = serde_json::json!("evidence-2")),
        ),
        (
            "wrapped-algorithm",
            Box::new(|v| v["wrappedKey"]["algorithm"] = serde_json::json!("other")),
        ),
        (
            "wrapped-nonce",
            Box::new(|v| v["wrappedKey"]["nonceHex"] = serde_json::json!("05".repeat(24))),
        ),
        (
            "wrapped-ciphertext",
            Box::new(|v| v["wrappedKey"]["ciphertextHex"] = serde_json::json!("06".repeat(48))),
        ),
        (
            "wrapped-aad",
            Box::new(|v| v["wrappedKey"]["aadSha256"] = serde_json::json!("f".repeat(64))),
        ),
    ];
    for (name, mutate) in mutations {
        let directory = tempfile::tempdir().unwrap();
        let repo = repository(directory.path());
        let sealed = sealed_evidence();
        let reference = sealed.reference().clone();
        repo.append_atomic(&prepared(Some(reference), vec![sealed]))
            .unwrap();
        drop(repo);
        let blob = std::fs::read_dir(directory.path().join("blobs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&blob).unwrap()).unwrap();
        mutate(&mut value);
        std::fs::write(&blob, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = match LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        ) {
            Ok(_) => panic!("tampered evidence field opened: {name}"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE", "{name}");
    }
}

#[test]
fn reconciliation_makes_no_deletions_when_a_later_unknown_entry_exists() {
    let directory = tempfile::tempdir().unwrap();
    let repo = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence();
    let reference = sealed.reference().clone();
    assert!(
        repo.append_atomic(&prepared(Some(reference), vec![sealed]))
            .is_err()
    );
    drop(repo);
    let orphan = std::fs::read_dir(directory.path().join("blobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(
        directory.path().join("blobs").join("zz-unknown"),
        b"unknown",
    )
    .unwrap();
    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("repository with unknown entry opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert!(orphan.exists(), "orphan was deleted before full validation");
}

#[test]
fn noncanonical_orphan_blob_is_preserved_and_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let repo = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence();
    let reference = sealed.reference().clone();
    assert!(
        repo.append_atomic(&prepared(Some(reference), vec![sealed]))
            .is_err()
    );
    drop(repo);
    let orphan = std::fs::read_dir(directory.path().join("blobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&orphan).unwrap();
    bytes.push(b'\n');
    std::fs::write(&orphan, bytes).unwrap();

    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("noncanonical orphan repository opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert!(orphan.exists(), "noncanonical orphan was deleted");
}

#[test]
fn adapter_never_claims_or_deletes_a_prefix_only_temp_name() {
    let directory = tempfile::tempdir().unwrap();
    drop(repository(directory.path()));
    let foreign = directory.path().join(".tmp").join("blob-user-data.tmp");
    std::fs::write(&foreign, b"foreign").unwrap();

    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("prefix-only temp name was claimed"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign");
}

#[test]
fn recognized_empty_partial_initialization_is_completed_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    for name in ["blobs", ".tmp", "active"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    std::fs::write(root.join("journal.jsonl"), []).unwrap();
    std::fs::write(root.join("repository.lock"), []).unwrap();
    drop(repository(&root));
    assert_eq!(
        std::fs::read(root.join("format.json")).unwrap(),
        b"{\"formatVersion\":\"1.0.0\"}\n"
    );
    drop(repository(&root));
}

/// The three components a recognized repository will never recreate.
///
/// `.tmp` and `active` USED to be in this list and were deliberately removed (#76): they are
/// transient workspace holding nothing the journal does not already carry, and empty directories
/// are dropped by git, zip and rsync alike — which is how committed acceptance stores became
/// unopenable. The assertion is not deleted, it is SPLIT: what is still refused stays here, and
/// what is now recovered is pinned by the test below, including that recovery writes no file
/// bytes. `blobs` stays in this list on purpose. A blob is a tracked FILE, so a missing `blobs/`
/// means evidence is genuinely gone, and that is a signal rather than a shape to restore.
#[test]
fn complete_format_never_recreates_a_missing_required_component() {
    for name in ["blobs", "journal.jsonl", "repository.lock"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        drop(repository(&root));
        let target = root.join(name);
        if target.is_dir() {
            std::fs::remove_dir(&target).unwrap();
        } else {
            std::fs::remove_file(&target).unwrap();
        }
        let format_before = std::fs::read(root.join("format.json")).unwrap();

        let error = match LocalEventRepository::open(
            &root,
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        ) {
            Ok(_) => panic!("complete repository recreated missing component {name}"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE", "{name}");
        assert!(!target.exists(), "missing component {name} was recreated");
        assert_eq!(
            std::fs::read(root.join("format.json")).unwrap(),
            format_before
        );
    }
}

/// The other half of the split above: the two transient directories ARE recreated, and recovery
/// writes nothing else. `format.json` is compared byte-for-byte because the pre-existing partial
/// path rewrites it, and an archive is exactly the thing that may be checksummed or mounted
/// read-only.
#[test]
fn complete_format_recreates_only_the_transient_workspace_directories() {
    for name in [".tmp", "active"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        drop(repository(&root));
        let target = root.join(name);
        std::fs::remove_dir(&target).unwrap();
        let format_before = std::fs::read(root.join("format.json")).unwrap();

        drop(
            LocalEventRepository::open(
                &root,
                Arc::new(FixedClock),
                Arc::new(SequenceIds::default()),
            )
            .unwrap_or_else(|error| panic!("{name} is recoverable, got {error:?}")),
        );

        assert!(target.is_dir(), "{name} was not restored");
        assert_eq!(
            std::fs::read(root.join("format.json")).unwrap(),
            format_before,
            "recovering {name} rewrote format.json"
        );
    }
}

#[test]
fn unknown_partial_initialization_is_rejected_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("foreign.bin"), b"untouched").unwrap();
    let before = std::fs::read(root.join("foreign.bin")).unwrap();
    let error = match LocalEventRepository::open(
        &root,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("unknown partial layout opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    assert_eq!(std::fs::read(root.join("foreign.bin")).unwrap(), before);
    assert!(!root.join("format.json").exists());
}

#[test]
fn injected_publication_failures_never_expose_dangling_committed_references() {
    for failpoint in LocalFailpoint::all() {
        let directory = tempfile::tempdir().unwrap();
        let repo = LocalEventRepository::open_with_failpoint(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
            failpoint,
        )
        .unwrap();
        let sealed = sealed_evidence();
        let reference = sealed.reference().clone();
        let _ = repo.append_atomic(&prepared(Some(reference), vec![sealed]));
        drop(repo);

        let reopened = match LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        ) {
            Ok(repository) => repository,
            Err(error) => {
                assert_eq!(failpoint, LocalFailpoint::PhysicalBatchAppend);
                assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
                continue;
            }
        };
        let page = reopened
            .read_stream(&scope(), "stream-1", 100, None)
            .unwrap();
        for envelope in page.events {
            for reference in envelope.evidence_refs {
                assert!(
                    reopened
                        .evidence_exists(&scope(), reference.evidence_id())
                        .unwrap()
                );
            }
        }
    }
}

#[test]
fn concurrent_writers_serialize_and_only_one_claims_the_expected_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Arc::new(repository(directory.path()));
    let left = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![event_with_key("request-left")],
        vec![],
        vec![],
    )
    .unwrap();
    let right = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![event_with_key("request-right")],
        vec![],
        vec![],
    )
    .unwrap();
    let left_repository = Arc::clone(&repository);
    let right_repository = Arc::clone(&repository);
    let left = std::thread::spawn(move || left_repository.append_atomic(&left));
    let right = std::thread::spawn(move || right_repository.append_atomic(&right));
    let outcomes = [left.join().unwrap(), right.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code(),
        "GHE001_SEQUENCE_CONFLICT"
    );
    assert_eq!(
        repository
            .read_stream(&scope(), "stream-1", 10, None)
            .unwrap()
            .events
            .len(),
        1
    );
}

#[test]
fn public_count_page_cursor_and_safe_integer_bounds_fail_before_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(directory.path());
    let too_many = (0..10_001)
        .map(|index| event_with_key(&format!("request-{index}")))
        .collect::<Vec<_>>();
    assert_eq!(
        PreparedAppend::new(
            scope(),
            OpaqueId::parse("stream-1").unwrap(),
            1,
            too_many,
            vec![],
            vec![],
        )
        .unwrap_err()
        .code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    assert_eq!(
        PreparedAppend::new(
            scope(),
            OpaqueId::parse("stream-1").unwrap(),
            9_007_199_254_740_992,
            vec![event_with_key("request-limit")],
            vec![],
            vec![],
        )
        .unwrap_err()
        .code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    assert_eq!(
        repository
            .read_stream(&scope(), "stream-1", 1_001, None)
            .unwrap_err()
            .code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    assert_eq!(
        repository
            .read_stream(&scope(), "stream-1", 1, Some(&"x".repeat(4_097)))
            .unwrap_err()
            .code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    let mut oversized_event = event_with_key("request-oversized-event");
    oversized_event.evidence_refs = vec![sealed_evidence().reference().clone(); 8_192];
    let oversized = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        1,
        vec![oversized_event],
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(
        repository.append_atomic(&oversized).unwrap_err().code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("journal.jsonl"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn retained_root_and_lock_anchors_reject_path_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = repository(&root);
    let displaced = directory.path().join("displaced");
    if let Err(error) = std::fs::rename(&root, &displaced) {
        assert!(
            is_windows_anchor_denial(&error),
            "unexpected rename failure: {error}"
        );
        repository.append_atomic(&prepared(None, vec![])).unwrap();
        return;
    }
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("repository.lock"), []).unwrap();
    let error = repository
        .append_atomic(&prepared(None, vec![]))
        .unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
}

#[test]
fn retained_lock_anchor_rejects_lock_path_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = repository(&root);
    let lock = root.join("repository.lock");
    let displaced = root.join("displaced.lock");
    if let Err(error) = std::fs::rename(&lock, &displaced) {
        assert!(
            is_windows_anchor_denial(&error),
            "unexpected rename failure: {error}"
        );
        repository.append_atomic(&prepared(None, vec![])).unwrap();
        return;
    }
    std::fs::write(&lock, []).unwrap();
    let error = repository
        .append_atomic(&prepared(None, vec![]))
        .unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
}

#[test]
fn retained_component_anchors_reject_directory_and_journal_replacement() {
    for (name, directory_component) in [
        ("blobs", true),
        (".tmp", true),
        ("active", true),
        ("journal.jsonl", false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        let repository = repository(&root);
        let component = root.join(name);
        let displaced = root.join(format!("displaced-{}", name.replace('.', "dot")));
        if let Err(error) = std::fs::rename(&component, &displaced) {
            assert!(is_windows_anchor_denial(&error), "{name}: {error}");
            repository.append_atomic(&prepared(None, vec![])).unwrap();
            continue;
        }
        if directory_component {
            std::fs::create_dir(&component).unwrap();
        } else {
            std::fs::write(&component, []).unwrap();
        }
        let error = repository
            .append_atomic(&prepared(None, vec![]))
            .unwrap_err();
        assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE", "{name}");
    }
}

#[test]
fn repository_components_reject_symlinks_or_reparse_points() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    drop(repository(&root));
    let original = root.join("blobs");
    std::fs::remove_dir(&original).unwrap();
    let outside = directory.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    if let Err(error) = create_directory_link(&outside, &original) {
        if is_windows_symlink_privilege_error(&error) {
            panic!(
                "OBSERVER_MISSING: Windows cannot create the component reparse fixture: {error}"
            );
        }
        panic!("failed to construct component-link attack: {error}");
    }
    let error = match LocalEventRepository::open(
        &root,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("linked blob directory unexpectedly opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
}

#[test]
fn repository_inspection_rejects_a_broken_repository_link() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    std::fs::create_dir(&missing).unwrap();
    let root = directory.path().join("repository");
    if let Err(error) = create_directory_link(&missing, &root) {
        if is_windows_symlink_privilege_error(&error) {
            panic!(
                "OBSERVER_MISSING: Windows cannot create the broken-root reparse fixture: {error}"
            );
        }
        panic!("failed to construct root-link attack: {error}");
    }
    std::fs::remove_dir(&missing).unwrap();
    let error = LocalEventRepository::inspect_repository(&root).unwrap_err();
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
}

fn event_with_key(key: &str) -> NewEvent {
    let mut value = event(None, Sensitivity::Internal);
    value.idempotency_key = OpaqueId::parse(key).unwrap();
    value
}

#[cfg(windows)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => Ok(()),
        Err(error) if is_windows_symlink_privilege_error(&error) && target.exists() => {
            let status = std::process::Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if status.success() { Ok(()) } else { Err(error) }
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn is_windows_symlink_privilege_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(1314)
}

#[cfg(windows)]
fn is_windows_anchor_denial(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
}

#[cfg(not(windows))]
fn is_windows_anchor_denial(_: &std::io::Error) -> bool {
    false
}

#[cfg(not(windows))]
fn is_windows_symlink_privilege_error(_: &std::io::Error) -> bool {
    false
}

#[cfg(unix)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

fn sealed_evidence() -> SealedEvidence {
    sealed_evidence_with_ciphertext_len(16)
}

fn sealed_evidence_with_ciphertext_len(ciphertext_len: usize) -> SealedEvidence {
    sealed_evidence_with_ciphertext(vec![7_u8; ciphertext_len])
}

fn sealed_evidence_with_ciphertext(ciphertext: Vec<u8>) -> SealedEvidence {
    fn push(output: &mut Vec<u8>, field: &[u8]) {
        output.extend_from_slice(&u32::try_from(field.len()).unwrap().to_be_bytes());
        output.extend_from_slice(field);
    }
    let ciphertext_sha256 = RawSha256::parse(hex::encode(Sha256::digest(&ciphertext))).unwrap();
    let reference = EvidenceReference::new(
        EvidenceId::parse("evidence-1").unwrap(),
        RawSha256::parse("c".repeat(64)).unwrap(),
        ciphertext_sha256,
    );
    let mut aad = Vec::new();
    push(&mut aad, b"graphhelm-evidence-aad-v1");
    push(&mut aad, b"workspace-1");
    push(&mut aad, b"project-1");
    aad.push(1);
    push(&mut aad, b"execution-1");
    push(&mut aad, b"evidence-1");
    push(&mut aad, b"1.0.0");
    push(&mut aad, b"application/json");
    push(&mut aad, b"internal");
    push(&mut aad, b"standard");
    push(
        &mut aad,
        format!("{}", reference.content_sha256()).as_bytes(),
    );
    let wrapped = WrappedKey::new(
        "key-1",
        "evidence-1",
        "xchacha20poly1305",
        vec![1_u8; 24],
        vec![2_u8; 48],
        RawSha256::parse(hex::encode(Sha256::digest(&aad))).unwrap(),
    )
    .unwrap();
    SealedEvidence::new(
        reference,
        scope(),
        "application/json",
        Sensitivity::Internal,
        "standard",
        "xchacha20poly1305",
        vec![3_u8; 24],
        ciphertext,
        wrapped,
    )
    .unwrap()
}

/// A batch that fails its own checksum is a corrupt stored line, which is a different failure from
/// a broken event hash chain and has a different recovery path. Both used to report
/// `GHE005_INTEGRITY_FAILURE`, leaving an operator unable to tell them apart.
#[test]
fn a_batch_failing_its_own_checksum_reports_the_corrupt_batch_code() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    repository.append_atomic(&prepared(None, vec![])).unwrap();
    drop(repository);

    let journal = directory.path().join("journal.jsonl");
    let text = std::fs::read_to_string(&journal).unwrap();
    let line = text.lines().next_back().unwrap();
    let mut batch: serde_json::Value = serde_json::from_str(line).unwrap();
    let checksum = batch["checksum"].as_str().unwrap().to_owned();
    // Flip one digest character so only the checksum disagrees with the contents it covers.
    let flipped = if checksum.ends_with('a') {
        format!("{}b", &checksum[..checksum.len() - 1])
    } else {
        format!("{}a", &checksum[..checksum.len() - 1])
    };
    batch["checksum"] = serde_json::json!(flipped);
    let rewritten = text.replace(line, &serde_json::to_string(&batch).unwrap());
    std::fs::write(&journal, rewritten).unwrap();

    let Err(error) = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) else {
        panic!("reopening a repository with a corrupt batch checksum must fail");
    };
    assert_eq!(error.code(), "GHE002_CORRUPT_BATCH");
}

/// An orphan BLOB that nobody holds is removed by the reconciling open (#328).
///
/// **This population was empty.** `an_orphan_temp_nobody_holds_is_still_deleted` covers the
/// `.tmp` branch, and the two orphan-blob cells next to this one assert the opposite property --
/// that an orphan is PRESERVED when validation refuses the store. Nothing asserted that a healthy
/// store actually removes one, so the whole `delete_blobs` half of `apply_reconcile` was
/// unobserved: the plan could be built with a handle that cannot perform the deletion and every
/// test in the crate would still pass.
///
/// Measured before this cell existed: changing the blobs scan to open read-only -- which makes the
/// by-handle `SetFileInformationByHandle(FileDispositionInfo)` fail, because that call requires a
/// DELETE-capable handle -- left 76 of 76 crate tests green.
///
/// The orphan is born the way one is really born rather than staged: `BlobPublish` fails after
/// `publish_staged` has run and before the journal append, so the blob sits in `blobs/` with
/// nothing referencing it.
#[test]
fn an_orphan_blob_nobody_holds_is_deleted_on_the_next_open() {
    let directory = tempfile::tempdir().unwrap();
    let repo = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence();
    let reference = sealed.reference().clone();
    assert!(
        repo.append_atomic(&prepared(Some(reference), vec![sealed]))
            .is_err()
    );
    drop(repo);

    // ARRANGEMENT: the orphan has to EXIST before the open that must remove it, or "it is gone"
    // below is true of a store that never had one.
    let orphan = std::fs::read_dir(directory.path().join("blobs"))
        .unwrap()
        .next()
        .expect("ARRANGEMENT: the failed publish must leave a blob behind")
        .unwrap()
        .path();
    assert!(
        orphan.exists(),
        "ARRANGEMENT: no orphan blob at {}",
        orphan.display()
    );

    let repo = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .expect("a store whose only defect is an orphan blob opens");
    drop(repo);

    assert!(
        !orphan.exists(),
        "the reconciling open left the orphan blob at {}. The scan plans the deletion and \
         `apply_reconcile` performs it by HANDLE, so a handle opened without DELETE access makes \
         the removal fail while the open still reports success",
        orphan.display()
    );
}

/// THE WITNESS #823 MERGED WITHOUT (#834): the blobs scan requests no DELETE access.
///
/// #823's title is *"the read path stops taking DELETE on every blob it scans"*, and the cell it
/// added asserts only that orphan removal still works afterwards. Revert the one token the PR
/// changed -- `want_delete` back to `true` at the scan's open -- and every cell in the crate
/// stayed green, because nothing observed what access the scan asked for.
///
/// What a DELETE request costs is observable from OUTSIDE the store, and that is what this cell
/// holds up against it: a foreign handle that shares READ and not DELETE -- the antivirus,
/// backup-agent and sync-client case #823's first paragraph names. Under a DELETE open that
/// handle is a sharing violation, which the scan maps to `Ok(None)` and SKIPS, counting the file
/// as clean for this cycle. Under a read-only open the scan reads the file. So the difference is
/// whether the scan's content validation ever reaches a blob someone is reading -- and a
/// NONCANONICAL orphan makes that visible: judged, it refuses the store with
/// `GHE007_UNSUPPORTED_FORMAT`; skipped, the store opens and the corruption is invisible.
///
/// Red on the old token (the store opens), green on the new one. The refusal is the same one
/// `noncanonical_orphan_blob_is_preserved_and_rejected` asserts with nobody holding the file; the
/// holder is the only thing added, and the control cell next to this one shows the holder alone
/// does not change the open's answer.
///
/// Windows only, by the property's own asymmetry: Unix removes by directory handle and name and
/// requests no access right at open, so there is no DELETE for a read path to stop taking.
#[cfg(windows)]
#[test]
fn a_noncanonical_orphan_a_foreign_reader_holds_is_still_judged() {
    let directory = tempfile::tempdir().unwrap();
    let orphan = noncanonical_orphan(directory.path());

    // The foreign reader: shares READ, withholds DELETE. Held across the open.
    let holder = open_sharing_read_only(&orphan);

    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!(
            "the store opened over a noncanonical orphan a reader holds: the scan asked for DELETE \
             access, took a sharing violation, and skipped the file instead of judging it -- the \
             read path is taking destructive access again (#823, #834)"
        ),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE007_UNSUPPORTED_FORMAT");
    drop(holder);
    assert!(orphan.exists(), "the refused orphan was deleted");
}

/// The control for the cell above: the same foreign reader on a VALID orphan, and the open
/// answers exactly as it does with nobody holding the file. Without this, the refusal above
/// would be consistent with "a held blob breaks the open", which is not the property.
///
/// The orphan survives, deliberately: its removal happens by a DELETE-capable handle
/// (`apply_reconcile`), and the reader withholds DELETE, so the removal is deferred to a cycle
/// in which nobody holds it. That deferral is the documented price of the read path not taking
/// DELETE, and asserting it here keeps the two halves of #823 in one place.
#[cfg(windows)]
#[test]
fn a_valid_orphan_a_foreign_reader_holds_leaves_the_open_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let orphan = orphan_born_of_a_failed_publish(directory.path());

    let holder = open_sharing_read_only(&orphan);
    let repo = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .expect("a reader holding a valid orphan does not change the open's answer");
    drop(repo);
    drop(holder);
    assert!(
        orphan.exists(),
        "the orphan was removed while a handle without FILE_SHARE_DELETE held it, which is not \
         possible by handle -- something deleted by path"
    );
}

/// An orphan blob born the way one really is: `BlobPublish` fails after the blob is staged and
/// before the journal append, so it sits in `blobs/` with nothing referencing it.
#[cfg(windows)]
fn orphan_born_of_a_failed_publish(root: &std::path::Path) -> std::path::PathBuf {
    let repo = LocalEventRepository::open_with_failpoint(
        root,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::BlobPublish,
    )
    .unwrap();
    let sealed = sealed_evidence();
    let reference = sealed.reference().clone();
    assert!(
        repo.append_atomic(&prepared(Some(reference), vec![sealed]))
            .is_err()
    );
    drop(repo);
    let orphan = std::fs::read_dir(root.join("blobs"))
        .unwrap()
        .next()
        .expect("ARRANGEMENT: the failed publish must leave a blob behind")
        .unwrap()
        .path();
    assert!(orphan.exists(), "ARRANGEMENT: no orphan blob");
    orphan
}

/// The orphan above with one byte appended: still an orphan, no longer canonical, so a scan
/// that reads it refuses the store (`noncanonical_orphan_blob_is_preserved_and_rejected`).
#[cfg(windows)]
fn noncanonical_orphan(root: &std::path::Path) -> std::path::PathBuf {
    let orphan = orphan_born_of_a_failed_publish(root);
    let mut bytes = std::fs::read(&orphan).unwrap();
    bytes.push(b'\n');
    std::fs::write(&orphan, bytes).unwrap();
    orphan
}

/// A handle that shares READ and nothing else -- in particular not DELETE -- which is what a
/// scanner or a backup agent holds. `FILE_SHARE_READ` is `0x1`; spelled as a literal because
/// this test crate does not depend on `windows-sys`, and the value is a stable Win32 constant.
#[cfg(windows)]
fn open_sharing_read_only(path: &std::path::Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .expect("ARRANGEMENT: the foreign reader opens the orphan")
}

/// The site a storage failure names, or a legible panic when it names none.
///
/// The bare `Storage` variant is the thing under test here: it is not a wrong
/// name, it is the absence of one, so the panic has to say which provocation
/// produced it and what the error actually was.
#[track_caller]
fn storage_site(error: &EventRepositoryError, provocation: &str) -> &'static str {
    match error {
        EventRepositoryError::StorageAt { site, .. } => site,
        other => panic!(
            "{provocation} answered a storage failure that names no site. \
             The error was: {other:?}"
        ),
    }
}

/// One injected fault, two check sites, and today one indistinguishable error.
///
/// `LocalFailpoint::JournalSync` is checked twice on separate paths: once in
/// `append_locked`, after the journal line is flushed and before `sync_data`,
/// and once in `sync_loaded_journal`, which only the open path reaches when it
/// resyncs a journal left dirty by an earlier crash. Both answer
/// `GHE008_STORAGE_FAILURE`, so a caller holding the error cannot tell a failed
/// append from a failed recovery — the two want opposite responses.
///
/// The assertion is on the pair, not on either name alone: a single site could
/// be named while the other stayed silent and a per-error check would still
/// pass. Both names must also mention the fault that was actually injected, so
/// that naming the sites cannot degrade into two arbitrary distinct constants.
#[test]
fn one_injected_fault_checked_at_two_sites_names_the_site_it_fired_from() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::JournalSync,
    )
    .unwrap();

    // Site one: the append path. Two appends, matching the crash the recovery
    // test relies on, so the journal is left dirty for the reopen below.
    let append_error = repository
        .append_atomic(&prepared(None, vec![]))
        .unwrap_err();
    assert_eq!(append_error.code(), "GHE008_STORAGE_FAILURE");
    assert!(repository.append_atomic(&prepared(None, vec![])).is_err());
    let append_site = storage_site(&append_error, "the append path under JournalSync");
    drop(repository);

    // Site two: the open path, resyncing the journal the crash left behind.
    let reopen_error = match LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
        LocalFailpoint::JournalSync,
    ) {
        Ok(_) => panic!(
            "the reopen was expected to fail while resyncing the dirty journal; \
             it succeeded, so this cell no longer reaches the second site"
        ),
        Err(error) => error,
    };
    assert_eq!(reopen_error.code(), "GHE008_STORAGE_FAILURE");
    let reopen_site = storage_site(&reopen_error, "the reopen path under JournalSync");

    assert_ne!(
        append_site, reopen_site,
        "the same injected fault fired at two different sites and named them \
         identically ({append_site}), which leaves the caller where it started"
    );
    for site in [append_site, reopen_site] {
        assert!(
            site.contains("journal-sync"),
            "site {site} does not name the fault that produced it, so the name \
             distinguishes the two paths by accident rather than by cause"
        );
    }
}

/// #837: a provoked failure must NAME its kind, and two kinds must not share a name.
///
/// `#824` gave `Storage` a sited sibling. This cell is the criterion #837 asks for, and the reason
/// it takes a LOOP rather than an assertion: one provoked failure with one assertion passes just as
/// well against a variant that hard-codes a single string. **Distinctness across kinds is what one
/// case cannot buy.**
///
/// Every kind here answers with the SAME code -- `GHE008_STORAGE_FAILURE` -- and that is asserted
/// rather than assumed, because it is the whole reason the name has to carry the discrimination. A
/// consumer keying on the code cannot tell a poisoned mutex from a failed journal sync; the site
/// tag is the only thing that separates them, so nothing else may be trusted to.
///
/// The kinds come from `LocalFailpoint::all()` and not from a list written here. A new variant
/// enters this cell by existing -- a deny-list would let the next one through, which is the shape
/// #837 was opened about.
#[test]
fn every_provoked_storage_failure_names_its_own_kind_and_no_two_kinds_share_a_name() {
    // NEGATIVE CONTROL, and it runs FIRST. Without it every assertion below is satisfied by a
    // fixture that fails for a reason having nothing to do with the failpoint -- a broken request,
    // an unwritable tempdir -- and the cell would certify the provocation machinery by accident.
    let clean = tempfile::tempdir().unwrap();
    let control_repository = repository(clean.path());
    let control_evidence = sealed_evidence();
    let control_reference = control_evidence.reference().clone();
    control_repository
        .append_atomic(&prepared(Some(control_reference), vec![control_evidence]))
        .expect("CONTROL: the same request must SUCCEED when no failpoint is armed");

    let mut named: Vec<(String, String)> = Vec::new();
    for failpoint in LocalFailpoint::all() {
        let kind = format!("{failpoint:?}");
        let slug = kebab_case(&kind);
        let directory = tempfile::tempdir().unwrap();
        let repository = LocalEventRepository::open_with_failpoint(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
            failpoint,
        )
        .unwrap();
        let sealed = sealed_evidence();
        let reference = sealed.reference().clone();
        let error = repository
            .append_atomic(&prepared(Some(reference), vec![sealed]))
            .expect_err("an armed failpoint must produce a failure");

        assert_eq!(
            error.code(),
            "GHE008_STORAGE_FAILURE",
            "{kind}: the code is shared by every kind, which is why the NAME has to discriminate"
        );

        let site = match error {
            EventRepositoryError::StorageAt { site, .. } => site,
            EventRepositoryError::Storage => panic!(
                "{kind} produced a NAMELESS Storage. This is exactly #837: the failure crossed the \
                 boundary without saying which check raised it, and no amount of re-running \
                 separates it from the other kinds."
            ),
            other => panic!("{kind} produced {other:?}, which is not a storage failure at all"),
        };

        assert!(
            site.contains(&format!("failpoint:{slug}@")),
            "{kind} must name ITSELF, not merely carry some name: got {site:?}, expected a site \
             containing \"failpoint:{slug}@\". Distinctness alone would let two arms be SWAPPED \
             and stay green, so the cell checks identity as well."
        );

        named.push((kind, site.to_owned()));
    }

    let mut distinct: Vec<&str> = named.iter().map(|(_, site)| site.as_str()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        named.len(),
        "two kinds share a name, so a live failure cannot be told apart: {named:?}"
    );
}

/// `PhysicalBatchAppend` -> `physical-batch-append`, so the expected tag is DERIVED from the kind.
///
/// Written out rather than hand-listed on purpose: a table mapping kinds to tags would be a second
/// copy of the thing under test, and the copy is what a future variant would fail to update.
fn kebab_case(camel: &str) -> String {
    let mut out = String::with_capacity(camel.len() + 4);
    for (index, character) in camel.char_indices() {
        if character.is_ascii_uppercase() {
            if index != 0 {
                out.push('-');
            }
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// #837, item 3: the class of sites that still decide on a NAMELESS `Storage`, made visible.
///
/// The issue's own framing is why this is a census and not a conversion: *"something that can say
/// 'these N sites decide on `Storage` by name' is worth more than converting N sites once."* A
/// conversion is finite work that ends; a class that nobody can see grows back with the next
/// variant, which is what happened between `Integrity` and `IntegrityAt`.
///
/// **The count is EXACT, and it started as a ceiling.** A ceiling only reddens on growth, which
/// sounded right -- converting a site is progress and a guard that fires on the improvement it
/// encourages gets deleted. It decays instead: convert one site inside a file that stays in the
/// pinned set and the ceiling silently gains a site of slack, which a later bare construction can
/// spend without any failure (Codex P2 on #1015). Exact costs the converter one line and the
/// message tells them which; a ceiling costs a reader nothing and protects nothing after the first
/// conversion.
///
/// The FILE SET is exact, and that is the half the ceiling cannot carry: a ceiling alone stays
/// silent when the sites move to a new file, and silent when a file is emptied (leaving a stale
/// number nobody notices). Equality on the set makes both events say something.
///
/// **`#[cfg(test)]` REGIONS ARE EXCLUDED, and this cell's first version got that wrong** -- it
/// skipped `tests/` DIRECTORIES only, so five sites inside test modules in `src/` counted as
/// production while the docstring claimed they did not (G's pass on #1015). Two reasons the
/// exclusion is load-bearing rather than tidiness:
///
/// 1. A test that NAMES the variant is not a site that discards a cause. Counting it inflates the
///    class and makes the pinned number drift with test churn.
/// 2. **`core/governor/src/apply.rs` keeps `#824`'s controls, and they must stay BARE**:
///    `assert!(bare.is_io(), "CONTROL: the bare variant still is")`. A guard that told a reader to
///    shrink that number would be telling them to convert a control and destroy what it proves --
///    a guard teaching the wrong edit. The cell protects those sites instead; see the sibling.
///
/// The region detector is NOT a parser, so it carries its own controls: the
/// per-file count of EXCLUDED sites is pinned, and regions that never find their closer must
/// number zero. See `cfg_test_regions` for why brace counting was abandoned mid-fix.
#[test]
fn the_nameless_storage_sites_are_a_visible_class_pinned_by_count_and_by_file() {
    const PINNED_PRODUCTION: usize = 64;
    const PINNED_FILES: [&str; 7] = [
        "adapters/postgres-event-store/src/error.rs",
        "adapters/postgres-event-store/src/integrity.rs",
        "adapters/postgres-event-store/src/journal.rs",
        "adapters/postgres-event-store/src/lib.rs",
        "apps/cli/src/commands/events/verify.rs",
        "core/events/src/local.rs",
        "core/governor/src/apply.rs",
    ];
    // The DETECTOR's control: sites inside `#[cfg(test)]` regions in `src/`, which the census must
    // not count. `core/events/src/store.rs` is absent from PINNED_FILES for exactly this reason --
    // its only nameless site is a test one, so it carries no production class at all.
    const PINNED_EXCLUDED: [(&str, usize); 3] = [
        ("core/events/src/local.rs", 3),
        ("core/events/src/store.rs", 1),
        ("core/governor/src/apply.rs", 1),
    ];

    let root = workspace_root();
    // THE ROOTS COME FROM THE WORKSPACE, not from a list written here. `core`, `apps` and
    // `adapters` were hand-written and missed `tools/` -- `tools/acceptance-map` and
    // `tools/development-benchmark` are members that depend on `graphhelm-events`, so a nameless
    // site in either left every exact pin green and the no-growth promise was false for a whole
    // directory (Codex P2 on #1015). Adding "tools" by hand would close the instance and leave the
    // class open for the next root somebody adds; deriving them closes the class.
    let scanned_roots = workspace_member_roots(&root);
    assert!(
        ["adapters", "apps", "core", "tools"]
            .iter()
            .all(|known| scanned_roots.iter().any(|root| root == known)),
        "the workspace member parse lost roots that are known to exist: {scanned_roots:?}. The \
         census would then be silent about whatever it stopped walking (#837)"
    );
    let mut census = StorageCensus::default();
    for area in &scanned_roots {
        census.walk(&root.join(area), &root);
    }
    census.production_files.sort();
    census.excluded.sort();

    // INSTRUMENT CONTROL, in the same walk that produces the claim. `Storage` is a prefix of
    // `StorageAt`, so a counter that fails to exclude the sited variant reports every mention and
    // reads as a much larger class. If these two numbers are equal the counter is not
    // discriminating and every assertion below is about the wrong population.
    assert!(
        census.every_mention > census.nameless,
        "the counter does not tell `Storage` from `StorageAt`: {} mentions and {} nameless sites \
         are the same number, so nothing here measures the class",
        census.every_mention,
        census.nameless
    );

    // The REGION RULE's own control. Every top-level `#[cfg(test)]` module must find a closing
    // brace in column zero; one that does not means the rule stopped holding for some file, and
    // sites would move between production and excluded on the strength of a bug rather than a fact.
    assert_eq!(
        census.unclosed_regions, 0,
        "{} `#[cfg(test)]` module(s) in src/ never reached a column-zero closer. The census cannot \
         say which side their sites belong on -- fix the detector or the file before trusting any \
         number below (#837)",
        census.unclosed_regions
    );

    assert_eq!(
        census.excluded,
        PINNED_EXCLUDED
            .iter()
            .map(|(path, count)| ((*path).to_owned(), *count))
            .collect::<Vec<_>>(),
        "the `#[cfg(test)]` sites in src/ changed. If you did not add or remove a test that names \
         the variant, suspect the BRACE MATCHER first -- a lost brace ends a region early and \
         returns production sites to the count without anything else moving (#837)"
    );

    assert_eq!(
        census.production_files,
        PINNED_FILES
            .iter()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>(),
        "the set of files carrying a nameless `Storage` in production changed. A NEW file means \
         the class spread; a file that vanished means it was fully converted -- lower PINNED_PRODUCTION and \
         update this list in the same commit (#837)"
    );

    assert_eq!(
        census.production, PINNED_PRODUCTION,
        "the nameless `Storage` class is no longer {PINNED_PRODUCTION} production sites. If it \
         GREW, a new site discards which check raised it and a live failure names one cause for \
         many -- that is the thing #837 exists to stop. If it SHRANK, you converted a site: lower \
         this number in the same commit, and thank you. Either way the class moved and the number \
         must move with it (#837, #824)"
    );

    // No import may hide a site from the substring census. `use EventRepositoryError::Storage;`
    // followed by a bare `Err(Storage)` spells no occurrence of the scanned needle, so the class
    // could grow with every pin above still agreeing. The convention is asserted rather than
    // trusted. (Codex P2 on #1015.)
    assert!(
        census.budget_refusals.is_empty(),
        concat!(
            "the census walk refused its own budget, so the counts above describe only part of ",
            "the tree: {:?}"
        ),
        census.budget_refusals
    );

    assert!(
        census.oversized.is_empty(),
        concat!(
            "these files are past the census size bound and were not read, so the counts above ",
            "describe a SMALLER tree than the one on disk: {:?}. Raise the bound deliberately or ",
            "split the file -- do not let the class shrink because a file got big (#837)"
        ),
        census.oversized
    );

    assert!(
        census.variant_imports.is_empty(),
        "these files import the variant instead of spelling it, so the census cannot see their \
         sites: {:?}. Either qualify the uses as `EventRepositoryError::Storage` or teach this \
         cell to follow imports before trusting any number above (#837)",
        census.variant_imports
    );
}

/// `LocalFailpoint::all()` is a hand-written `[Self; 7]`, so a new variant could be added without
/// joining it -- and the loop above would silently stop being exhaustive while staying green.
///
/// This match has no wildcard arm. A new variant does not make it fail; it makes it **not
/// compile**, which is the only signal that cannot be ignored by a suite that still passes.
/// (Codex P2 on #1015.)
#[test]
fn every_failpoint_variant_reaches_the_provocation_loop() {
    let named = [
        LocalFailpoint::Validation,
        LocalFailpoint::EvidenceStaging,
        LocalFailpoint::BlobSync,
        LocalFailpoint::BlobPublish,
        LocalFailpoint::PhysicalBatchAppend,
        LocalFailpoint::JournalSync,
        LocalFailpoint::ActiveMarker,
    ];
    for failpoint in named {
        // The wildcard-free match is the compile-time half: a new variant makes THIS not compile.
        match failpoint {
            LocalFailpoint::Validation
            | LocalFailpoint::EvidenceStaging
            | LocalFailpoint::BlobSync
            | LocalFailpoint::BlobPublish
            | LocalFailpoint::PhysicalBatchAppend
            | LocalFailpoint::JournalSync
            | LocalFailpoint::ActiveMarker => {}
        }
        assert!(
            LocalFailpoint::all().contains(&failpoint),
            "{failpoint:?} is a variant this file names but `all()` omits, so the #837 loop never \
             provokes it and its site tag is never checked"
        );
    }
    assert_eq!(
        LocalFailpoint::all().len(),
        named.len(),
        "`all()` and this file's list disagree on how many kinds exist"
    );
}

/// #824's controls must stay BARE, and the census above is the reason this cell exists.
///
/// `apply.rs` proves `is_io` accepts BOTH shapes: a `StorageAt` that names its cause, and a bare
/// `Storage`. The bare one is the control -- without it the assertion says nothing about the
/// predicate having kept its old behaviour. A census that counted it as debt would invite a future
/// reader to "convert the last site in apply.rs" and delete the control while the suite stays
/// green. So the sites are named here, and converting one reddens THIS cell with the reason.
#[test]
fn the_bare_storage_witnesses_that_824_uses_as_controls_are_still_bare() {
    let root = workspace_root();
    let witnesses: [(&str, &str); 2] = [
        (
            "core/governor/src/apply.rs",
            "let bare = ApplyError::Repository(EventRepositoryError::Storage);",
        ),
        (
            "core/events/src/store.rs",
            "let error = EventRepositoryError::Storage;",
        ),
    ];
    for (path, line) in witnesses {
        let text = std::fs::read_to_string(root.join(path))
            .unwrap_or_else(|error| panic!("{path} must be readable: {error}"));
        assert!(
            text.contains(line),
            "{path} no longer carries `{line}`. If that was a conversion to `StorageAt`, it \
             DELETED a control: #824's proof that `is_io` still accepts the bare variant needs a \
             bare variant to accept. Convert production sites, never the witnesses (#837)"
        );
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("core/events is two levels below the workspace root")
        .to_path_buf()
}

/// First path segment of every workspace member, so the census walks what the workspace contains
/// rather than what somebody remembered when writing it.
fn workspace_member_roots(root: &std::path::Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .expect("the workspace manifest must be readable to know what to scan");
    let Some(start) = manifest.find("members") else {
        return Vec::new();
    };
    let Some(open) = manifest[start..].find('[').map(|at| start + at) else {
        return Vec::new();
    };
    let Some(close) = manifest[open..].find(']').map(|at| open + at) else {
        return Vec::new();
    };
    let mut roots: Vec<String> = manifest[open..close]
        .split('"')
        .filter(|piece| piece.contains('/') || !piece.trim().is_empty())
        .filter(|piece| !piece.contains(',') && !piece.contains('[') && !piece.trim().is_empty())
        .filter_map(|member| member.split('/').next().map(str::to_owned))
        .filter(|segment| !segment.trim().is_empty())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Whether a file imports the NAMELESS variant, matched as a whole path segment.
///
/// The boundary is the point: `Storage` is a prefix of `StorageAt`, so anything that merely looks
/// for the shorter name also fires on the sited one -- and this guard's false positive is a red on
/// the mandatory workspace suite for code that did nothing wrong.
/// STATEMENTS, not lines, and this is the second time the same defect arrived by a different door.
///
/// A rustfmt-valid grouped import splits the path from the name:
///
/// ```text
/// use graphhelm_events::EventRepositoryError::{
///     Storage,
/// };
/// ```
///
/// The line holding `Storage` does not hold `EventRepositoryError`, so a per-LINE predicate drops
/// it; a later bare `Err(Storage)` holds neither census needle. Both instruments then agree that
/// nothing is there, and every exact pin stays green with the site in the tree -- which is the
/// hole this guard exists to close, reopened by the shape of the import. Found by G on #1015, who
/// reproduced it in `verify.rs` rather than describing it.
fn imports_the_nameless_variant(text: &str) -> bool {
    use_statements(text)
        .into_iter()
        .filter(|statement| statement.contains("EventRepositoryError"))
        .any(|statement| {
            // AN ALIAS RENAMES THE THING THIS CENSUS LOOKS FOR. `use ...EventRepositoryError as
            // RepoError;` followed by `RepoError::Storage` spells neither needle: the import holds
            // no `Storage`, and the construction holds no `EventRepositoryError`. Both instruments
            // then agree nothing is there -- the third door into the same hole, after the
            // per-line predicate and the grouped import (Codex P2 on #1015).
            //
            // The conservative direction, deliberately: this REFUSES the alias rather than
            // following it. Resolving names is a compiler's job, and a lexical census that
            // pretends to resolve names is worse than one that states its limit -- it would be
            // confidently wrong about a population instead of admitting it cannot see one.
            statement.contains(" as ") || names_the_bare_variant(&statement)
        })
}

/// Every `use` statement, joined from its keyword to its `;` so a grouped import is ONE string.
fn use_statements(text: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if current.is_none() && !line.starts_with("use ") {
            continue;
        }
        let statement = current.get_or_insert_with(String::new);
        statement.push(' ');
        statement.push_str(line);
        if line.ends_with(';') {
            statements.push(current.take().unwrap_or_default());
        }
    }
    // An unterminated `use` at end of file: keep it rather than drop it. Dropping would be the
    // silent direction, and this predicate exists because silence is what let a site through.
    if let Some(statement) = current {
        statements.push(statement);
    }
    statements
}

/// `Storage` as a whole path segment: `Storage` is a prefix of `StorageAt`, and a guard that fires
/// on the sited variant reddens the mandatory suite for code that did nothing wrong.
fn names_the_bare_variant(statement: &str) -> bool {
    let needle = "Storage";
    statement.match_indices(needle).any(|(at, _)| {
        let before_is_boundary = statement[..at]
            .chars()
            .last()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        let after_is_boundary = statement[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        before_is_boundary && after_is_boundary
    })
}

/// Counts nameless `EventRepositoryError::Storage` sites in `src/`, splitting production from the
/// ones inside `#[cfg(test)]` regions.
#[derive(Default)]
struct StorageCensus {
    /// Every mention including `StorageAt`; only ever compared against `nameless` as a control.
    every_mention: usize,
    nameless: usize,
    production: usize,
    production_files: Vec<String>,
    excluded: Vec<(String, usize)>,
    /// `#[cfg(test)]` modules whose column-zero closer was never found; asserted to be zero.
    unclosed_regions: usize,
    /// Files importing the variant unqualified, which the substring census cannot see.
    variant_imports: Vec<String>,
    /// Files past the size bound, which were NOT read; asserted to be empty rather than skipped.
    oversized: Vec<String>,
    /// Directory entries visited; the aggregate half of the budget, checked INSIDE the walk.
    entries_seen: usize,
    /// Budget refusals, in the house's words. Asserted empty: a walk that stopped early would
    /// otherwise report a smaller class with every pin agreeing.
    budget_refusals: Vec<String>,
}

/// Aggregate bounds, in the shape `apps/cli/tests/source_invariants.rs` already uses.
///
/// The per-file byte cap bounds ONE read; it says nothing about ten thousand small files, which
/// cost the same authoritative gate unbounded time and I/O with no compiler ever touching them
/// (Codex P1 on #1015). Both halves REFUSE rather than truncate -- a walk that quietly stopped
/// would shrink the class exactly like a skipped oversized file, and every pin would agree.
const MAX_CENSUS_ENTRIES: usize = 20_000;
const MAX_CENSUS_DEPTH: usize = 24;

impl StorageCensus {
    fn walk(&mut self, area: &std::path::Path, root: &std::path::Path) {
        self.walk_bounded(area, root, 0);
    }

    fn walk_bounded(&mut self, area: &std::path::Path, root: &std::path::Path, depth: usize) {
        if depth > MAX_CENSUS_DEPTH {
            self.budget_refusals.push(format!(
                "HARNESS-BROKE: the Storage census exceeded depth {MAX_CENSUS_DEPTH} at {}",
                area.display()
            ));
            return;
        }
        let Ok(entries) = std::fs::read_dir(area) else {
            return;
        };
        for entry in entries.flatten() {
            // INSIDE the walk, never before it: a budget checked only at the top cannot see the
            // shape of what it is walking.
            self.entries_seen += 1;
            if self.entries_seen > MAX_CENSUS_ENTRIES {
                self.budget_refusals.push(format!(
                    "HARNESS-BROKE: the Storage census exceeded {MAX_CENSUS_ENTRIES} entries at {}",
                    area.display()
                ));
                return;
            }
            let path = entry.path();
            // `is_dir()` FOLLOWS symlinks, so a link pointing at an ancestor makes this walk recurse
            // until the stack ends -- in a test the local gate runs on every PR. `file_type()` comes
            // from the directory entry and does not follow. (Codex P1 on #1015.)
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("");
                // `<crate>/tests/` is Rust's integration-test directory and is not production.
                // `<crate>/src/tests/` is an ORDINARY MODULE -- `mod tests;` backed by
                // `src/tests/mod.rs` compiles into the crate like any other, and Rust attaches no
                // meaning to the directory's name. Skipping both counted the second as absent, so
                // a bare `Storage` compiled into production there was invisible to the count and
                // to the file set (Codex P2 on #1015). The boundary is whether `src` is already
                // above us: sibling of `src` is skipped, inside `src` is walked.
                let inside_src = path
                    .components()
                    .any(|component| component.as_os_str() == "src");
                if name == "target" || (name == "tests" && !inside_src) {
                    continue;
                }
                self.walk_bounded(&path, root, depth + 1);
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            // BOUNDED BEFORE READ, and the bound REFUSES rather than skips. Reading every `.rs`
            // whole lets one very large file stall or OOM the local gate (Codex P1 on #1015) --
            // but skipping an oversized file silently would be worse than the stall: the census
            // would report a smaller class and every pin would agree with it. So the size is
            // taken from METADATA, no bytes are read, and an oversized file is collected and
            // asserted against. A census that cannot read a file must say so, not shrink.
            const LARGEST_SOURCE_FILE: u64 = 4 * 1024 * 1024;
            let oversized = std::fs::metadata(&path)
                .map(|data| data.len() > LARGEST_SOURCE_FILE)
                .unwrap_or(false);
            if oversized {
                self.oversized.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // EXACT variant, not a prefix. `Storage` is a prefix of `StorageAt`, so the first
            // version of this guard also matched `use ...::EventRepositoryError::StorageAt;` and
            // would have reddened the mandatory workspace test with no nameless site anywhere --
            // a guard whose false positive is a red on somebody else's correct code. The `,
            // Storage,` fallback was worse still: it matched any import list with a `Storage`
            // token in it, from any crate. (Codex P2 on #1015.)
            if imports_the_nameless_variant(&text) {
                self.variant_imports.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
            }
            let (test_regions, unclosed) = cfg_test_regions(&text);
            self.unclosed_regions += unclosed;
            let needle = "EventRepositoryError::Storage";
            let (mut production, mut excluded) = (0usize, 0usize);
            for (index, _) in text.match_indices(needle) {
                self.every_mention += 1;
                if text[index + needle.len()..].starts_with("At") {
                    continue;
                }
                self.nameless += 1;
                if test_regions
                    .iter()
                    .any(|(from, to)| (*from..*to).contains(&index))
                {
                    excluded += 1;
                } else {
                    production += 1;
                }
            }
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if production > 0 {
                self.production += production;
                self.production_files.push(relative.clone());
            }
            if excluded > 0 {
                self.excluded.push((relative, excluded));
            }
        }
    }
}

/// Byte ranges of top-level `#[cfg(test)]` modules: attribute line to the next line that is a lone
/// closing brace in COLUMN ZERO.
///
/// **Brace counting was tried first and is wrong on this tree.** `core/events/src/local.rs` carries
/// a test module at line 5532 whose braces never balance for a naive counter -- braces inside
/// string and char literals -- so the region either swallowed the rest of the file or covered
/// nothing, and the three sites inside it landed on whichever side the bug fell. Two prototypes of
/// the same idea disagreed (3 excluded against 1), which is how the defect surfaced.
///
/// Column zero is what makes this robust without a parser: a brace inside a literal is indented,
/// and a top-level module's closing brace is not. The assumption is not left implicit -- the census
/// counts regions that never found their closer and the cell asserts that count is zero, so a file
/// that breaks the rule says so instead of quietly moving sites between the two populations.
fn cfg_test_regions(text: &str) -> (Vec<(usize, usize)>, usize) {
    let mut regions = Vec::new();
    let mut unclosed = 0usize;
    let mut offsets = Vec::new();
    let mut cursor = 0usize;
    let lines: Vec<&str> = text.split('\n').collect();
    for line in &lines {
        offsets.push(cursor);
        cursor += line.len() + 1;
    }
    let mut index = 0usize;
    while index < lines.len() {
        // ONLY a module opens a region. `#[cfg(test)]` also decorates a `use`, a `type` alias or a
        // `#[derive]`, and treating those as regions invents one: `core/events/src/local.rs:2768`
        // carries `#[cfg(test)] type LoadsByKind = ...`, and the rule without this guard produced a
        // region 2768..2778 that belongs to nothing. It happened to contain no site, so the counts
        // were right BY LUCK -- a spurious region one line earlier would have moved production
        // sites into the excluded pile with every pin still agreeing. (Codex P2 on #1015.)
        // A module DECLARATION (`mod x;`) opens no block, so it must not open a region either: the
        // scan would run to the next unrelated column-zero brace, or to the end of the file, and
        // either outcome moves sites. There are none in this tree today; the guard is here because
        // the residual was measured rather than imagined (M's pass on #1015).
        // NOT the immediate next line, and NOT only `mod`/`pub mod`. Another attribute may sit
        // between `#[cfg(test)]` and the module, and `pub(crate) mod tests` is an ordinary
        // declaration. Rejecting either counted a test module's BARE `Storage` controls as
        // production, so the mandatory gate failed on a test-only change -- a guard reddening for
        // correct code, which is the failure direction that gets guards deleted. (Codex P2 on
        // #1015.)
        let mut candidate = index + 1;
        while lines
            .get(candidate)
            .is_some_and(|line| line.starts_with("#["))
        {
            candidate += 1;
        }
        let opens_a_module = lines
            .get(candidate)
            .map(|line| {
                let declaration = line.trim_end_matches('\r').trim_end().ends_with(';');
                let head = line.trim_end_matches('\r');
                let is_module = head.starts_with("mod ")
                    || head.starts_with("pub mod ")
                    || (head.starts_with("pub(") && head.contains(") mod "));
                is_module && !declaration
            })
            .unwrap_or(false);
        if lines[index].starts_with("#[cfg(test)]") && opens_a_module {
            let mut end = index + 1;
            while end < lines.len() && lines[end].trim_end_matches('\r') != "}" {
                end += 1;
            }
            if end < lines.len() {
                regions.push((offsets[index], offsets[end]));
            } else {
                unclosed += 1;
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    (regions, unclosed)
}
