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

/// #824: the two open-time kinds sit on DIFFERENT acquisition paths, so one arrangement cannot
/// reach both. The shared fast path runs only over a layout that is already Complete; the
/// exclusive bootstrap runs only over one that is not. A loop that armed either kind on a bare
/// tempdir would reach the shared site never and call that "no failure", which is a green bought
/// by the arrangement rather than by the code.
fn root_reaching_the_open_time_site(
    directory: &std::path::Path,
    failpoint: LocalFailpoint,
) -> std::path::PathBuf {
    let root = directory.join("repository");
    if failpoint == LocalFailpoint::InitializeRootLockShared {
        drop(
            LocalEventRepository::open(
                &root,
                Arc::new(FixedClock),
                Arc::new(SequenceIds::default()),
            )
            .expect("ARRANGEMENT: the shared fast path needs a Complete layout to reach"),
        );
    }
    root
}

#[test]
fn injected_publication_failures_never_expose_dangling_committed_references() {
    for failpoint in LocalFailpoint::all() {
        let directory = tempfile::tempdir().unwrap();
        if failpoint.fires_during_open() {
            // A kind that fails the OPEN never reaches a publication, so there is nothing here
            // to leave dangling. The refusal is asserted rather than skipped: a kind that
            // wrongly opened would fall through to the append path unarmed and this loop would
            // certify it green without ever provoking anything.
            let error = LocalEventRepository::open_with_failpoint(
                root_reaching_the_open_time_site(directory.path(), failpoint),
                Arc::new(FixedClock),
                Arc::new(SequenceIds::default()),
                failpoint,
            )
            .err()
            .unwrap_or_else(|| {
                panic!("{failpoint:?}: an open-time failpoint must refuse the open")
            });
            assert_eq!(error.code(), "GHE008_STORAGE_FAILURE");
            continue;
        }
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
fn create_directory_junction(
    target: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("mklink /J failed"))
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
///
/// **Windows only, like its `.tmp` twin** (`held_file_contention.rs` is `#![cfg(windows)]`).
/// Removal is BY HANDLE, and only Windows has that primitive. POSIX has no compare-and-unlink by
/// descriptor, so the Unix `remove_reconciled_file` deliberately preserves the validated inode
/// rather than unlink a name that may no longer be it, and
/// `reconciliation_preserves_the_validated_inode_without_name_based_unlink` (`local.rs`) pins that
/// on Unix. On Linux this cell was asserting the opposite of that pinned contract, and it failed
/// every run.
#[cfg(windows)]
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
        let error = if failpoint.fires_during_open() {
            // An open-time kind answers on the OPEN. Its site is the production literal of the
            // check that would have run -- not a `failpoint:` name -- so the identity assertion
            // below does not apply to it; `core/events/tests/root_lock_failpoints.rs` pins each
            // of these two to its exact site. What this cell still owes them is the shared code
            // and the distinctness census, which is what the loop carries them here for.
            LocalEventRepository::open_with_failpoint(
                root_reaching_the_open_time_site(directory.path(), failpoint),
                Arc::new(FixedClock),
                Arc::new(SequenceIds::default()),
                failpoint,
            )
            .err()
            .unwrap_or_else(|| {
                panic!("{kind}: an armed open-time failpoint must produce a failure")
            })
        } else {
            let repository = LocalEventRepository::open_with_failpoint(
                directory.path(),
                Arc::new(FixedClock),
                Arc::new(SequenceIds::default()),
                failpoint,
            )
            .unwrap();
            let sealed = sealed_evidence();
            let reference = sealed.reference().clone();
            repository
                .append_atomic(&prepared(Some(reference), vec![sealed]))
                .expect_err("an armed failpoint must produce a failure")
        };

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
            failpoint.fires_during_open() || site.contains(&format!("failpoint:{slug}@")),
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
        census.refusals.is_empty(),
        "the census encountered filesystem failures, so the counts above are incomplete: {:?}",
        census.refusals
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

/// `LocalFailpoint::all()` is a hand-written `[Self; 9]`, so a new variant could be added without
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
        LocalFailpoint::InitializeRootLockShared,
        LocalFailpoint::InitializeRootLockExclusive,
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
            | LocalFailpoint::ActiveMarker
            | LocalFailpoint::InitializeRootLockShared
            | LocalFailpoint::InitializeRootLockExclusive => {}
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

/// #837: the five latent gaps #1015 declared and did not close, each with its own cell.
///
/// Every one of them fails in the SAME direction -- a guard reddening for correct code, or a guard
/// blind to a real site -- and every one was measured as latent before being deferred: zero
/// instances of the damage in the tree, with the shape present for two of them. They land together
/// because they are one class, not because they arrived together.
#[test]
fn the_five_latent_census_gaps_are_closed() {
    let root = std::path::Path::new("/w");

    // 1. THE CHECKOUT'S OWN ANCESTORS MUST NOT DECIDE. Under `C:\src\GraphHelm` an absolute-path
    // component search finds `src` in every path, so every crate-level `tests/` reads as production
    // and the mandatory gate reddens on correct code -- on somebody else's machine, never on the
    // author's.
    assert!(
        is_inside_src(root, std::path::Path::new("/w/core/events/src/tests")),
        "a directory genuinely under a crate's src is inside src"
    );
    assert!(
        !is_inside_src(root, std::path::Path::new("/w/core/events/tests")),
        "a crate's integration directory is not"
    );
    assert!(
        !is_inside_src(
            std::path::Path::new("/src/GraphHelm"),
            std::path::Path::new("/src/GraphHelm/core/events/tests")
        ),
        "and a checkout whose own ancestor is called `src` does not make every tests/ production -- \
         the judgement is relative to the workspace, not to the disk"
    );

    // 2. THE ALIAS MUST BE ON THE ERROR TYPE, not on a sibling in the same group.
    assert!(
        imports_the_nameless_variant("use graphhelm_events::EventRepositoryError as RepoError;"),
        "aliasing the error type hides every later `RepoError::Storage` from the census"
    );
    assert!(
        imports_the_nameless_variant(
            "use graphhelm_events::EventRepositoryError::{self as RepoError};"
        ),
        "a grouped self alias of the error type hides every later `RepoError::Storage`"
    );
    assert!(
        !imports_the_nameless_variant(
            "use graphhelm_events::{EventRepositoryError, EventStore as Store};"
        ),
        "but aliasing a SIBLING hides nothing -- the type and `Storage` stay spelled, and refusing \
         this reddens the mandatory suite for code that did nothing"
    );
    assert!(
        !imports_the_nameless_variant(
            "use graphhelm_events::{EventRepositoryError, EventStore::{self as Store}};"
        ),
        "a grouped self alias of a SIBLING hides nothing -- the error type stays spelled"
    );

    // 3. A VISIBILITY PREFIX STILL OPENS A `use`.
    assert!(
        imports_the_nameless_variant(
            "pub(crate) use graphhelm_events::EventRepositoryError as RepoError;"
        ),
        "a re-export with a visibility prefix is still an import that renames the type"
    );

    // 4. ANY `cfg` PREDICATE CONTAINING `test` OPENS A TEST REGION.
    // NO INDENTATION INSIDE THE LITERAL. A run of spaces in an authored string is what
    // `authored_strings_across_the_workspace` forbids, and the region detector cares about the
    // column-zero closer, never about what the body is indented by.
    let compound = "#[cfg(all(test, target_os = \"linux\"))]\nmod tests {\nlet x = 1;\n}\n";
    let (regions, unclosed) = cfg_test_regions(compound);
    assert_eq!(
        unclosed, 0,
        "the compound-attribute module finds its closer"
    );
    assert_eq!(
        regions.len(),
        1,
        "a compound cfg predicate containing `test` opens a region: {regions:?}"
    );
    // AND THE CONTROL: a cfg that does not mention test opens nothing. Without this the fix reads
    // as "any cfg is a test module", which would swallow production code whole.
    let unrelated = "#[cfg(windows)]\nmod platform {\nlet x = 1;\n}\n";
    assert_eq!(
        cfg_test_regions(unrelated).0.len(),
        0,
        "a cfg without `test` opens no region -- `latest` and `testing` must not match either"
    );
    let lookalike = "#[cfg(feature = \"latest\")]\nmod platform {\nlet x = 1;\n}\n";
    assert_eq!(
        cfg_test_regions(lookalike).0.len(),
        0,
        "`latest` contains the letters of `test` and is not a test predicate"
    );
    let negated = "#[cfg(not(test))]\nmod platform {\nlet x = 1;\n}\n";
    assert_eq!(
        cfg_test_regions(negated).0.len(),
        0,
        "cfg(not(test)) is production code and must stay in the census"
    );
    let either = "#[cfg(any(test, unix))]\nmod platform {\nlet x = 1;\n}\n";
    assert_eq!(
        cfg_test_regions(either).0.len(),
        0,
        "cfg(any(test, unix)) also includes production and is not test-only"
    );
    let quoted_comma = "#[cfg(all(feature = \"x,test,y\"))]\nmod platform {\nlet x = 1;\n}\n";
    assert_eq!(
        cfg_test_regions(quoted_comma).0.len(),
        0,
        "commas inside a cfg string literal do not make production code test-only"
    );

    // 5. `#[cfg(test)] mod NAME;` MAKES `NAME` TEST-ONLY, whatever it is called.
    let temporary = std::env::temp_dir().join(format!("h837-{}", std::process::id()));
    std::fs::create_dir_all(&temporary).expect("the probe directory must be creatable");
    std::fs::write(
        temporary.join("mod.rs"),
        "pub fn live() {}\n#[cfg(test)]\nmod probe_tests;\n",
    )
    .expect("the probe module file must be writable");
    std::fs::write(
        temporary.join("ordinary.rs"),
        "#[cfg(test)]\nmod probe;\n#[cfg(test)]\npub(super) mod restricted_probe;\n#[cfg(test)]\npub(in crate::tests) mod crate_probe;\n#[cfg(test)]\n#[path = \"ordinary_probe.rs\"]\npub(crate) mod ordinary_probe;\n",
    )
    .expect("the ordinary parent module must be writable");
    std::fs::create_dir_all(temporary.join("ordinary"))
        .expect("the ordinary module directory must be creatable");
    std::fs::write(
        temporary.join("ordinary").join("probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the nested test module file must be writable");
    std::fs::write(
        temporary.join("ordinary").join("restricted_probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the restricted-visibility test module file must be writable");
    std::fs::write(
        temporary.join("ordinary").join("crate_probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the crate-visibility test module file must be writable");
    std::fs::write(
        temporary.join("probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the production sibling file must be writable");
    std::fs::write(temporary.join("ordinary_probe.rs"), "")
        .expect("the path-attribute target must be writable");
    std::fs::write(
        temporary.join("large.rs"),
        vec![b'x'; (LARGEST_SOURCE_FILE + 1) as usize],
    )
    .expect("the oversized module file must be writable");
    let entries: Vec<_> = std::fs::read_dir(&temporary)
        .expect("the probe directory must be readable")
        .flatten()
        .collect();
    let mut oversized = Vec::new();
    let mut refusals = Vec::new();
    let declared = test_only_modules(&temporary, &entries, &mut oversized, &mut refusals);
    assert!(
        declared.contains(&temporary.join("probe_tests.rs"))
            || declared.contains(&temporary.join("probe_tests")),
        "an external cfg-test module declaration resolves its child path: {declared:?}"
    );
    assert!(
        declared.contains(&temporary.join("ordinary").join("probe.rs")),
        "an ordinary parent module resolves its child below the parent's module directory: {declared:?}"
    );
    assert!(
        declared.contains(&temporary.join("ordinary").join("restricted_probe.rs")),
        "a pub(super) test module resolves its child below the parent's module directory: {declared:?}"
    );
    assert!(
        declared.contains(&temporary.join("ordinary").join("crate_probe.rs")),
        "a pub(in ...) test module resolves its child below the parent's module directory: {declared:?}"
    );
    assert!(
        !declared.contains(&temporary.join("probe.rs")),
        "the same-named production sibling must remain visible: {declared:?}"
    );
    assert!(
        declared.contains(&temporary.join("ordinary_probe.rs")),
        "a path attribute resolves relative to the declaring file: {declared:?}"
    );
    let mut census = StorageCensus::default();
    census.walk(&temporary, &temporary);
    assert_eq!(
        census.production, 1,
        "the nested test child is excluded while the same-named sibling remains production: {:?}",
        census.production_files
    );
    assert_eq!(
        census.production_files,
        vec!["probe.rs".to_owned()],
        "the production sibling is the only fixture counted"
    );
    let _ = std::fs::remove_dir_all(&temporary);
    assert_eq!(
        oversized,
        vec!["large.rs".to_owned()],
        "module-file discovery reports an oversized file without reading it"
    );
    assert!(
        refusals.is_empty(),
        "fixture had unexpected refusals: {refusals:?}"
    );
}

#[test]
fn grouped_self_alias_of_event_repository_error_is_visible() {
    assert!(
        imports_the_nameless_variant(
            "use graphhelm_events::EventRepositoryError::{self as RepoError};"
        ),
        "a grouped self alias must not hide the error type from the census"
    );
    assert!(
        !imports_the_nameless_variant(
            "use graphhelm_events::{EventRepositoryError, EventStore::{self as Store}};"
        ),
        "a grouped self alias of a sibling must not hide the visible error type"
    );
    // A SUBSTRING IS NOT A NAME. `WrappedEventRepositoryError as Wrapped` contains the needle and
    // renames a DIFFERENT type; without an identifier boundary in front of the match the guard
    // reports an alias of the error type that nobody wrote, and the mandatory suite reddens on
    // correct source. Measured at this head:
    // `grep -rnE "[A-Za-z0-9_]EventRepositoryError" --include=*.rs core apps adapters tools` -> 0,
    // so this is a seal placed before the first instance, not a repair of a live one.
    // (Codex P2, thread PRRT_kwDOTyQgUM6hxMqc.)
    assert!(
        !imports_the_nameless_variant("use crate::WrappedEventRepositoryError as Wrapped;"),
        "a type whose name merely ENDS with the error type's name is not an alias of it"
    );
    assert!(
        !imports_the_nameless_variant("use crate::WrappedEventRepositoryError::{self as Wrapped};"),
        "the grouped form of a suffix-named sibling is not an alias of the error type either"
    );
    assert!(
        imports_the_nameless_variant("use crate::inner::EventRepositoryError as RepoError;"),
        "a path-qualified alias of the error type itself is still an alias"
    );
}

#[test]
fn restricted_visibility_external_test_modules_are_discovered() {
    let temporary = std::env::temp_dir().join(format!("h837-restricted-{}", std::process::id()));
    std::fs::create_dir_all(temporary.join("ordinary"))
        .expect("the restricted-visibility fixture must be creatable");
    std::fs::write(
        temporary.join("ordinary.rs"),
        "#[cfg(test)]\npub(super) mod restricted_probe;\n#[cfg(test)]\npub(in crate::tests) mod crate_probe;\n",
    )
    .expect("the restricted-visibility parent must be writable");
    for name in ["restricted_probe.rs", "crate_probe.rs"] {
        std::fs::write(
            temporary.join("ordinary").join(name),
            "EventRepositoryError::Storage\n",
        )
        .expect("the restricted-visibility child must be writable");
    }
    let entries: Vec<_> = std::fs::read_dir(&temporary)
        .expect("the restricted-visibility fixture must be readable")
        .flatten()
        .collect();
    let mut oversized = Vec::new();
    let mut refusals = Vec::new();
    let declared = test_only_modules(&temporary, &entries, &mut oversized, &mut refusals);
    let _ = std::fs::remove_dir_all(&temporary);
    assert!(
        declared.contains(&temporary.join("ordinary").join("restricted_probe.rs")),
        "pub(super) test modules must resolve from the declaring module file: {declared:?}"
    );
    assert!(
        declared.contains(&temporary.join("ordinary").join("crate_probe.rs")),
        "pub(in ...) test modules must resolve from the declaring module file: {declared:?}"
    );
    assert!(
        oversized.is_empty() && refusals.is_empty(),
        "restricted-visibility discovery must not create refusals: oversized={oversized:?}, refusals={refusals:?}"
    );
}

/// One indentation level of generated Rust, composed at run time.
///
/// The indentation these fixtures need belongs to the GENERATED source, not to this file, so it
/// cannot be written as `\n` followed by spaces inside an authored literal:
/// `authored_strings_carry_no_collapsed_indentation` in `core/events/tests/source_invariants.rs`
/// rejects any run of three or more spaces inside a string literal, and that guard carries no
/// per-file exemption by design. A backslash continuation would not help either -- it removes THIS
/// file's indentation from a continued literal, while the runs here are wanted in the VALUE.
/// Composing the run at run time is what keeps both true.
fn generated_indent() -> String {
    " ".repeat(4)
}

/// Join already-indented lines into a newline-terminated Rust source fixture.
fn generated_source(lines: &[&str]) -> String {
    let mut source = lines.join("\n");
    source.push('\n');
    source
}

#[test]
fn rustfmt_wrapped_cfg_external_test_modules_are_discovered() {
    let temporary = std::env::temp_dir().join(format!("h837-wrapped-cfg-{}", std::process::id()));
    std::fs::create_dir_all(temporary.join("ordinary"))
        .expect("the wrapped-cfg fixture must be creatable");
    let indent = generated_indent();
    std::fs::write(
        temporary.join("ordinary.rs"),
        generated_source(&[
            "#[cfg(all(",
            &format!("{indent}test,"),
            &format!("{indent}feature = \"wrapped-cfg\","),
            "))]",
            "pub(super) mod probe;",
        ]),
    )
    .expect("the wrapped-cfg parent must be writable");
    std::fs::write(
        temporary.join("ordinary").join("probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the wrapped-cfg child must be writable");
    std::fs::write(
        temporary.join("probe.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the production sibling must be writable");

    let entries: Vec<_> = std::fs::read_dir(&temporary)
        .expect("the wrapped-cfg directory must be readable")
        .flatten()
        .collect();
    let mut oversized = Vec::new();
    let mut refusals = Vec::new();
    let declared = test_only_modules(&temporary, &entries, &mut oversized, &mut refusals);
    assert!(
        declared.contains(&temporary.join("ordinary").join("probe.rs")),
        "a rustfmt-wrapped cfg(test) declaration must resolve its child: {declared:?}"
    );

    let mut census = StorageCensus::default();
    census.walk(&temporary, &temporary);
    assert_eq!(
        census.production_files,
        vec!["probe.rs".to_owned()],
        "the wrapped test child is excluded while its same-named production sibling remains: {:?}",
        census.production_files
    );
    let _ = std::fs::remove_dir_all(&temporary);
    assert!(
        oversized.is_empty(),
        "fixture had oversized files: {oversized:?}"
    );
    assert!(refusals.is_empty(), "fixture had refusals: {refusals:?}");
}

#[test]
fn cfg_comments_do_not_hide_inline_or_external_production_modules() {
    let temporary = std::env::temp_dir().join(format!("h837-cfg-comments-{}", std::process::id()));
    std::fs::create_dir_all(temporary.join("ordinary"))
        .expect("the cfg-comment fixture must be creatable");
    let indent = generated_indent();
    let commented_predicate = [
        "#[cfg(all(".to_owned(),
        format!("{indent}unix /*,"),
        format!("{indent}test,"),
        format!("{indent}*/"),
        "))]".to_owned(),
    ];
    let predicate: Vec<&str> = commented_predicate.iter().map(String::as_str).collect();
    let inline_body = format!("{indent}EventRepositoryError::Storage");
    let mut inline_lines = predicate.clone();
    inline_lines.extend_from_slice(&["mod inline_production {", &inline_body, "}"]);
    std::fs::write(temporary.join("inline.rs"), generated_source(&inline_lines))
        .expect("the inline production fixture must be writable");
    let mut external_lines = predicate.clone();
    external_lines.push("pub(super) mod external_production;");
    std::fs::write(
        temporary.join("ordinary.rs"),
        generated_source(&external_lines),
    )
    .expect("the external production fixture must be writable");
    std::fs::write(
        temporary.join("ordinary").join("external_production.rs"),
        "EventRepositoryError::Storage\n",
    )
    .expect("the external production child must be writable");

    let entries: Vec<_> = std::fs::read_dir(&temporary)
        .expect("the cfg-comment directory must be readable")
        .flatten()
        .collect();
    let mut oversized = Vec::new();
    let mut refusals = Vec::new();
    let declared = test_only_modules(&temporary, &entries, &mut oversized, &mut refusals);
    assert!(
        !declared.contains(&temporary.join("ordinary").join("external_production.rs")),
        "a cfg attribute containing comments must not classify the external production child as test-only: {declared:?}"
    );

    let mut census = StorageCensus::default();
    census.walk(&temporary, &temporary);
    assert_eq!(
        census.production, 2,
        "comment-bearing cfg predicates keep both inline and external production sites: {:?}",
        census.production_files
    );
    let _ = std::fs::remove_dir_all(&temporary);
    assert!(
        oversized.is_empty(),
        "fixture had oversized files: {oversized:?}"
    );
    assert!(refusals.is_empty(), "fixture had refusals: {refusals:?}");
}

#[test]
fn census_refuses_invalid_utf8_instead_of_dropping_a_source() {
    let temporary = std::env::temp_dir().join(format!("h837-invalid-{}", std::process::id()));
    std::fs::create_dir_all(&temporary).expect("the invalid-source fixture must be creatable");
    let path = temporary.join("invalid.rs");
    std::fs::write(&path, [0xff, 0xfe]).expect("the invalid-source fixture must be writable");
    let refusal = match read_bounded_source(&path) {
        Err(refusal) => refusal,
        Ok(_) => panic!("invalid UTF-8 must be explicit"),
    };
    assert!(
        refusal.contains("not UTF-8"),
        "unexpected refusal: {refusal}"
    );
    let _ = std::fs::remove_dir_all(&temporary);
}

#[test]
fn census_prepass_refuses_file_type_failures() {
    let mut refusals = Vec::new();
    let kind = prepass_entry_kind(
        std::path::Path::new("synthetic/entry.rs"),
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "synthetic refusal",
        )),
        &mut refusals,
    );

    assert!(
        kind.is_none(),
        "a failed prepass lookup cannot classify an entry"
    );
    assert_eq!(refusals.len(), 1, "the prepass failure must be visible");
    assert!(
        refusals[0].contains("HARNESS-BROKE: file_type failed for synthetic/entry.rs"),
        "unexpected refusal: {:?}",
        refusals
    );
}

#[cfg(windows)]
#[test]
fn census_rejects_a_root_junction_before_following_it() {
    let temporary = tempfile::tempdir().expect("the junction probe directory must be creatable");
    let target = temporary.path().join("target");
    std::fs::create_dir(&target).expect("the junction target must be creatable");
    std::fs::write(target.join("probe.rs"), "EventRepositoryError::Storage\n")
        .expect("the junction target source must be writable");
    let linked = temporary.path().join("linked");
    if let Err(error) = create_directory_junction(&target, &linked) {
        panic!("OBSERVER_MISSING: Windows cannot create the root junction fixture: {error}");
    }

    let mut direct = StorageCensus::default();
    direct.walk(&target, temporary.path());
    assert_eq!(
        direct.production, 1,
        "the direct directory proves the fixture contains a production source"
    );

    let mut through_junction = StorageCensus::default();
    through_junction.walk(&linked, temporary.path());
    assert_eq!(
        through_junction.production, 0,
        "a root junction must not make the census read outside its selected area"
    );
    assert!(
        through_junction
            .refusals
            .iter()
            .any(|refusal| refusal.contains("root area") && refusal.contains("reparse")),
        "root junction refusal was not explicit: {:?}",
        through_junction.refusals
    );
}

#[test]
fn census_refuses_before_external_module_discovery_when_budget_is_exhausted() {
    let temporary = std::env::temp_dir().join(format!("h837-budget-{}", std::process::id()));
    std::fs::create_dir_all(&temporary).expect("the budget probe directory must be creatable");
    std::fs::write(temporary.join("ordinary.rs"), "#[cfg(test)]\nmod probe;\n")
        .expect("the budget probe module must be writable");

    let mut census = StorageCensus {
        entries_seen: MAX_CENSUS_ENTRIES,
        ..Default::default()
    };
    census.walk(&temporary, &temporary);

    assert_eq!(
        census.entries_seen,
        MAX_CENSUS_ENTRIES + 1,
        "the shared entry budget refuses the first entry beyond its limit"
    );
    assert_eq!(
        census.budget_refusals.len(),
        1,
        "budget exhaustion is recorded as a refusal"
    );
    assert!(
        census.test_only_paths.is_empty(),
        "external-module discovery does not run after the shared budget refuses the directory"
    );
    let _ = std::fs::remove_dir_all(&temporary);
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

const LARGEST_SOURCE_FILE: u64 = 4 * 1024 * 1024;

/// Module files in a directory can declare external `#[cfg(test)] mod NAME;` children.
///
/// The external form declares a test module without opening a block. Its child path depends on the
/// declaring file (`ordinary.rs` resolves `mod probe;` to `ordinary/probe.rs`), and a path attribute
/// can override that rule. Returning resolved paths keeps a same-named production sibling visible.
fn test_only_modules(
    root: &std::path::Path,
    entries: &[std::fs::DirEntry],
    oversized: &mut Vec<String>,
    refusals: &mut Vec<String>,
) -> Vec<std::path::PathBuf> {
    let mut names = Vec::new();
    for entry in entries {
        let Some(kind) = prepass_entry_kind(&entry.path(), entry.file_type(), refusals) else {
            continue;
        };
        if kind.is_symlink() || !kind.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
            continue;
        }
        let source = match read_bounded_source(&path) {
            Ok(Some(source)) => source,
            Ok(None) => continue,
            Err(error) => {
                refusals.push(error);
                continue;
            }
        };
        let BoundedSource::Text(text) = source else {
            record_oversized(&path, root, oversized);
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for index in 0..lines.len() {
            let Some((implies_test, attribute_end)) = cfg_attribute_at(&lines, index) else {
                continue;
            };
            if !implies_test {
                continue;
            }
            let mut explicit_path = None;
            let mut candidate = attribute_end + 1;
            while lines
                .get(candidate)
                .is_some_and(|line| line.trim_start().starts_with("#["))
            {
                if explicit_path.is_none() {
                    explicit_path = module_path_attribute(lines[candidate]);
                }
                candidate += 1;
            }
            let Some(next) = lines.get(candidate) else {
                continue;
            };
            let declaration = next.trim();
            if !declaration.ends_with(';') {
                continue;
            }
            let Some(rest) = declaration
                .strip_prefix("mod ")
                .or_else(|| declaration.strip_prefix("pub mod "))
                .or_else(|| declaration.strip_prefix("pub(crate) mod "))
                .or_else(|| {
                    declaration
                        .strip_prefix("pub(")
                        .and_then(|rest| rest.split_once(") mod ").map(|(_, rest)| rest))
                })
            else {
                continue;
            };
            let module = rest.trim_end_matches(';').trim();
            names.extend(resolved_external_module_paths(&path, module, explicit_path));
        }
    }
    names.sort_unstable();
    names.dedup();
    names
}

fn prepass_entry_kind(
    path: &std::path::Path,
    kind: Result<std::fs::FileType, std::io::Error>,
    refusals: &mut Vec<String>,
) -> Option<std::fs::FileType> {
    match kind {
        Ok(kind) => Some(kind),
        Err(error) => {
            refusals.push(format!(
                "HARNESS-BROKE: file_type failed for {}: {error}",
                path.display()
            ));
            None
        }
    }
}

enum BoundedSource {
    Text(String),
    Oversized,
}

fn read_bounded_source(path: &std::path::Path) -> Result<Option<BoundedSource>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "HARNESS-BROKE: metadata failed for {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.len() > LARGEST_SOURCE_FILE {
        return Ok(Some(BoundedSource::Oversized));
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "HARNESS-BROKE: open failed for {}: {error}",
                path.display()
            ));
        }
    };
    let mut limited = std::io::Read::take(file, LARGEST_SOURCE_FILE.saturating_add(1));
    let mut bytes = Vec::new();
    if let Err(error) = std::io::Read::read_to_end(&mut limited, &mut bytes) {
        return Err(format!(
            "HARNESS-BROKE: read failed for {}: {error}",
            path.display()
        ));
    }
    if bytes.len() as u64 > LARGEST_SOURCE_FILE {
        return Ok(Some(BoundedSource::Oversized));
    }
    String::from_utf8(bytes)
        .map(BoundedSource::Text)
        .map(Some)
        .map_err(|_| format!("HARNESS-BROKE: source is not UTF-8: {}", path.display()))
}

fn record_oversized(path: &std::path::Path, root: &std::path::Path, oversized: &mut Vec<String>) {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    if !oversized.iter().any(|existing| existing == &relative) {
        oversized.push(relative);
    }
}

fn module_path_attribute(line: &str) -> Option<&str> {
    let body = line
        .trim()
        .strip_prefix("#[path")?
        .strip_suffix(']')?
        .trim();
    let value = body.strip_prefix('=')?.trim();
    value.strip_prefix('"')?.strip_suffix('"')
}

fn resolved_external_module_paths(
    declaring_file: &std::path::Path,
    module: &str,
    explicit_path: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let parent = declaring_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    if let Some(path) = explicit_path {
        return vec![parent.join(path)];
    }

    let stem = declaring_file
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("");
    let base = if matches!(stem, "mod" | "lib" | "main") {
        parent.join(module)
    } else {
        parent.join(stem).join(module)
    };
    vec![base.clone(), base.with_extension("rs"), base.join("mod.rs")]
}

/// Whether a path sits under a crate's `src`, judged RELATIVE to the workspace.
///
/// Extracted so a cell can drive it with fabricated paths: the hazard is a checkout whose own
/// ancestors contain a directory called `src`, and no test can relocate the real workspace.
fn is_inside_src(root: &std::path::Path, path: &std::path::Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str() == "src")
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
            // THE ALIAS MUST BE ON THE ERROR TYPE. A grouped import may rename a SIBLING --
            // `use graphhelm_events::{EventRepositoryError, EventStore as Store};` -- while the
            // error type and `Storage` stay fully qualified and perfectly visible to the census.
            // A bare `" as "` test refused that import and reddened the mandatory suite for code
            // that hid nothing. Zero instances today; the narrowing lands before one exists.
            // (Codex P2 on #1015.)
            // THE MATCH MUST BE A WHOLE NAME, NOT A SUFFIX OF ONE. `match_indices` finds the needle
            // inside `WrappedEventRepositoryError` too, and that type's own `as Wrapped` then reads
            // as an alias of the error type -- a finding on source that hid nothing, which reddens
            // the mandatory suite. The check is on the character BEFORE the match, which is the
            // half the text after it cannot supply. Zero instances at this head; the seal lands
            // before the first one. (Codex P2, thread PRRT_kwDOTyQgUM6hxMqc.)
            let error_type_matches = |statement: &str| {
                statement
                    .match_indices("EventRepositoryError")
                    .filter(|(at, _)| {
                        statement[..*at]
                            .chars()
                            .next_back()
                            .is_none_or(|before| before != '_' && !before.is_alphanumeric())
                    })
                    .map(|(at, _)| at)
                    .collect::<Vec<_>>()
            };
            let aliases_the_error_type = error_type_matches(&statement).into_iter().any(|at| {
                let after = statement[at + "EventRepositoryError".len()..].trim_start();
                after.starts_with("as ")
            });
            // `EventRepositoryError::{self as RepoError}` is the grouped form of the same alias.
            // Inspect only the group immediately following the error type: a sibling's
            // `self as Store` must remain a non-finding control.
            let grouped_self_aliases_error_type =
                error_type_matches(&statement).into_iter().any(|at| {
                    let after = statement[at + "EventRepositoryError".len()..].trim_start();
                    let Some(group) = after
                        .strip_prefix("::")
                        .map(str::trim_start)
                        .and_then(|rest| rest.strip_prefix('{'))
                    else {
                        return false;
                    };
                    let Some((members, _)) = group.split_once('}') else {
                        return false;
                    };
                    members.split(',').any(|member| {
                        let member = member.trim_start();
                        member
                            .strip_prefix("self")
                            .is_some_and(|rest| rest.trim_start().starts_with("as "))
                    })
                });
            aliases_the_error_type
                || grouped_self_aliases_error_type
                || names_the_bare_variant(&statement)
        })
}

/// Every `use` statement, joined from its keyword to its `;` so a grouped import is ONE string.
fn use_statements(text: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines().map(str::trim) {
        // A VISIBILITY PREFIX STILL OPENS A `use`. `pub(crate) use ...EventRepositoryError as
        // RepoError;` followed by `RepoError::Storage` spells neither needle, and a filter that
        // accepts only a bare `use ` never sees it. One such re-export exists in this tree
        // (`core/events/src/lib.rs`), hiding nothing -- the shape is live, the damage was not.
        // (Codex P2 on #1015.)
        let opens_a_use = line.starts_with("use ")
            || line.starts_with("pub use ")
            || (line.starts_with("pub(") && line.contains(") use "));
        if current.is_none() && !opens_a_use {
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
    /// Resolved external test-module paths discovered in ancestor directories. A declaration in
    /// `ordinary.rs` is discovered beside the `ordinary/` directory, then applies while that
    /// directory is walked.
    test_only_paths: Vec<std::path::PathBuf>,
    /// Filesystem failures that would otherwise make the lexical census incomplete.
    refusals: Vec<String>,
}

/// Aggregate bounds, in the shape `apps/cli/tests/source_invariants.rs` already uses.
///
/// The per-file byte cap bounds ONE read; it says nothing about ten thousand small files, which
/// cost the same authoritative gate unbounded time and I/O with no compiler ever touching them
/// (Codex P1 on #1015). Both halves REFUSE rather than truncate -- a walk that quietly stopped
/// would shrink the class exactly like a skipped oversized file, and every pin would agree.
const MAX_CENSUS_ENTRIES: usize = 20_000;
const MAX_CENSUS_DEPTH: usize = 24;
const MAX_CFG_ATTRIBUTE_LINES: usize = 128;

fn collect_bounded_entries(
    directory: &std::path::Path,
    entries_seen: &mut usize,
    budget_refusals: &mut Vec<String>,
) -> Option<Vec<std::fs::DirEntry>> {
    let directory_entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            budget_refusals.push(format!(
                "HARNESS-BROKE: read_dir failed for {}: {error}",
                directory.display()
            ));
            return None;
        }
    };
    let mut entries = Vec::new();
    for entry in directory_entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                budget_refusals.push(format!(
                    "HARNESS-BROKE: directory entry failed for {}: {error}",
                    directory.display()
                ));
                return None;
            }
        };
        *entries_seen += 1;
        if *entries_seen > MAX_CENSUS_ENTRIES {
            budget_refusals.push(format!(
                "HARNESS-BROKE: the Storage census exceeded {MAX_CENSUS_ENTRIES} entries at {}",
                directory.display()
            ));
            return None;
        }
        entries.push(entry);
    }
    Some(entries)
}

impl StorageCensus {
    fn walk(&mut self, area: &std::path::Path, root: &std::path::Path) {
        let metadata = match std::fs::symlink_metadata(area) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.refusals.push(format!(
                    "HARNESS-BROKE: root area metadata failed for {}: {error}",
                    area.display()
                ));
                return;
            }
        };
        if metadata.file_type().is_symlink() {
            self.refusals.push(format!(
                "HARNESS-BROKE: root area is a symlink or reparse point: {}",
                area.display()
            ));
            return;
        }
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
        // The prepass and the walk share this count. Collecting first means discovery cannot read
        // an unbounded second directory stream before the walk's budget sees it.
        let Some(entries) =
            collect_bounded_entries(area, &mut self.entries_seen, &mut self.budget_refusals)
        else {
            return;
        };
        self.test_only_paths.extend(test_only_modules(
            root,
            &entries,
            &mut self.oversized,
            &mut self.refusals,
        ));
        self.test_only_paths.sort_unstable();
        self.test_only_paths.dedup();
        for entry in entries {
            let path = entry.path();
            // `is_dir()` FOLLOWS symlinks, so a link pointing at an ancestor makes this walk recurse
            // until the stack ends -- in a test the local gate runs on every PR. `file_type()` comes
            // from the directory entry and does not follow. (Codex P1 on #1015.)
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    self.refusals.push(format!(
                        "HARNESS-BROKE: file_type failed for {}: {error}",
                        path.display()
                    ));
                    continue;
                }
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
                //
                // RELATIVE TO THE WORKSPACE, not absolute. Searching the absolute path for a
                // component named `src` makes the answer depend on where somebody cloned the
                // repository: under `C:\src\GraphHelm` every path has one, every crate-level
                // `tests/` reads as production, and the census scans the integration suites --
                // the mandatory gate red on correct code, on any machine whose checkout happens to
                // sit under a directory called `src`. (Codex P1 on #1015.)
                let inside_src = is_inside_src(root, &path);
                if name == "target" || (name == "tests" && !inside_src) {
                    continue;
                }
                // A `#[cfg(test)] mod NAME;` in this directory's own module file makes its
                // resolved child path test-only. Compare full paths: `ordinary.rs` may declare
                // `ordinary/probe.rs` while a production `probe.rs` remains beside it.
                if self.test_only_paths.iter().any(|module| module == &path) {
                    continue;
                }
                self.walk_bounded(&path, root, depth + 1);
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            if self.test_only_paths.iter().any(|module| module == &path) {
                continue;
            }
            // BOUNDED BEFORE READ, and the bound REFUSES rather than skips. The helper checks
            // metadata first, then reads at most one byte past the cap so a file that grows after
            // metadata is still reported as oversized instead of being read without a bound.
            let source = match read_bounded_source(&path) {
                Ok(Some(source)) => source,
                Ok(None) => continue,
                Err(error) => {
                    self.refusals.push(error);
                    continue;
                }
            };
            let BoundedSource::Text(text) = source else {
                record_oversized(&path, root, &mut self.oversized);
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

/// Byte ranges of top-level test-only `#[cfg]` modules: attribute line to the next line that is a lone
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
fn cfg_predicate_implies_test(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if predicate == "test" {
        return true;
    }
    let Some(arguments) = predicate
        .strip_prefix("all(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                if cfg_predicate_implies_test(&arguments[start..index]) {
                    return true;
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    cfg_predicate_implies_test(&arguments[start..])
}

fn cfg_attribute_implies_test(line: &str) -> bool {
    let head = line.trim();
    let Some(predicate) = head
        .strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
    else {
        return false;
    };
    cfg_predicate_implies_test(predicate)
}

/// Returns whether the cfg attribute at `index` implies test and its final line.
///
/// rustfmt wraps long predicates across lines. The bounded line window prevents malformed source
/// from making this lexical census scan unbounded while retaining the existing conservative
/// fail-closed behavior when no complete attribute is found.
fn cfg_attribute_at(lines: &[&str], index: usize) -> Option<(bool, usize)> {
    let first = lines.get(index)?.trim();
    if !first.starts_with("#[cfg(") {
        return None;
    }

    let mut attribute = String::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for offset in 0..MAX_CFG_ATTRIBUTE_LINES {
        let line = lines.get(index + offset)?.trim_end_matches('\r');
        // Comments can contain commas, parentheses, and the word `test` without affecting cfg
        // semantics. Reject the whole attribute conservatively before scanning its boundaries; a
        // false production count is safer than excluding a real production site.
        if line.contains("/*") || line.contains("//") {
            return Some((false, index + offset));
        }
        if offset > 0 {
            attribute.push('\n');
        }
        attribute.push_str(line);
        for character in line.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    in_string = false;
                }
                continue;
            }
            match character {
                '"' => in_string = true,
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if !in_string && depth == 0 && attribute.trim_end().ends_with(")]") {
            return Some((cfg_attribute_implies_test(&attribute), index + offset));
        }
    }
    None
}

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
        let cfg_attribute = cfg_attribute_at(&lines, index);
        let mut candidate = cfg_attribute.map_or(index + 1, |(_, end)| end + 1);
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
        // A TEST-ONLY predicate must imply `test`. `all(test, unix)` does; `not(test)` and
        // `any(test, unix)` also include production and therefore remain in the census. Parsing the
        // small predicate grammar prevents a token search from silently excluding production code.
        let opens_a_test_region = cfg_attribute.is_some_and(|(implies_test, _)| implies_test);
        if opens_a_test_region && opens_a_module {
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
