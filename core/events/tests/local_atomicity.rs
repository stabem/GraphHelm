use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ArtifactRegistration, LocalEventRepository, LocalFailpoint, LocalRepositoryInspection,
    PreparedAppend, SealedEvidence, WrappedKey,
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
