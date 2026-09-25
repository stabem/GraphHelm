//! A committed journal that contains a `graph_version_published` event, so the store's own
//! re-serialize-and-compare check has a `PersistedNode` to judge (#172).
//!
//! The guard was real and its population was empty. Opening a committed store re-parses every
//! stored batch, re-serializes it canonically and compares against the stored bytes
//! (`canonical_bytes(&batch) != line` -> `Integrity`), so a serialization change to any event kind
//! present in a committed journal fails loudly. Measured across every journal under
//! `docs/acceptance/` — seventeen files — not one carries a `graph_version_published` event, so
//! no stored bytes have ever exercised `PersistedNode`. The protection for its two
//! `skip_serializing_if` fields was the precedent's technique plus review, and a field landing
//! without the attribute would have left every green green until it met a real store.
//!
//! **Both directions have to be in the fixture, or it guards only one.** A node whose optional
//! fields are ABSENT is what catches a dropped `skip_serializing_if`: re-serialization would then
//! emit `"customs":null` and the bytes would move. A node whose optional fields are PRESENT is
//! what catches the field being dropped from serialization altogether. One node cannot be both,
//! so the fixture carries two.
//!
//! **Why the version is grown from the conformance document rather than built by hand.** A
//! publication the store accepts has to satisfy `validate_persisted_projection` in full: a
//! `graph_completion` control naming every terminal, a graph display-name slot, a display-name AND
//! an objective slot per node, slot ids derived from their position, evidence ids derived from the
//! scope and semantic hash, and one sealed evidence blob per slot in the same append. A hand-built
//! topology was refused as a bare `Invalid` four times before the rule that mattered was found,
//! and every hand-callable validator had said `Ok` first. `conformance/schemas/valid/
//! persisted-graph-version.json` already satisfies all of it, so it is patched, not replaced,
//! following the recipe `sweep_verb.rs` proved.
//!
//! The fixture lives beside this file rather than under `docs/acceptance/`, because those
//! directories are byte-archives of real runs with README and `SHA256SUMS` conventions, and a
//! store built by a test is not evidence of a run. It borrows their one load-bearing rule: a
//! directory-local `.gitattributes` marks it `-text`, so no checkout can rewrite the journal's
//! line endings and hand the store bytes that disagree with their own checksum.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend, SealedEvidence, WrappedKey};
use graphhelm_protocols::{
    Clock, ContentFieldKind, ContentOwnerKind, ContentSlot, EventKind, EvidenceReference,
    ExecutionId, GraphVersionPublished, IdGenerator, NewEvent, OpaqueId, PersistedCustoms,
    PersistedGraphVersion, ProjectId, RawSha256, RepositoryScope, Sensitivity, WorkspaceId,
};
use sha2::{Digest, Sha256};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// Where the committed fixture lives, relative to this crate.
fn fixture_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graph-version-published/events")
}

const STREAM: &str = "stream-172";
const EXECUTION: &str = "execution-172";
const FIRST_VERSION: u64 = 1;
/// The conformance document's own node. It becomes the ABSENT shape.
const NODE_WITH_NOTHING_DECLARED: &str = "start";
/// A clone of that node with both optional fields set. The PRESENT shape.
const NODE_WITH_BOTH_DECLARED: &str = "start-declares-both";
const DECLARED_TIMEOUT: u64 = 90;

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-172").unwrap(),
        ProjectId::parse("project-172").unwrap(),
        Some(ExecutionId::parse(EXECUTION).unwrap()),
    )
}

fn declared_customs() -> PersistedCustoms {
    PersistedCustoms::new(30, 60, Some(120))
}

/// The conformance document, patched to carry both node shapes, republished as version 1 into
/// this fixture's scope, with one sealed evidence blob per content slot.
///
/// Adapted from `sweep_verb.rs`'s `version_and_evidence`, which proved the recipe. Two things are
/// deliberately inherited from it rather than re-derived: the `(1, None)` override, because the
/// document is a SUCCESSOR and the first publication on a stream must be version 1 with no
/// predecessor; and the evidence AAD layout, which `SealedEvidence::verify` checks on every open.
fn version_and_evidence() -> (PersistedGraphVersion, Vec<SealedEvidence>) {
    fn push(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        output.extend_from_slice(value);
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root is two levels above core/events");
    let text = std::fs::read_to_string(
        root.join("conformance/schemas/valid/persisted-graph-version.json"),
    )
    .expect("the conformance fixture must be readable");
    let mut document: serde_json::Value =
        serde_json::from_str(&text).expect("the conformance fixture must parse");

    // The ABSENT shape: strip both optional fields from the document's own node.
    let original = document["topology"]["nodes"][NODE_WITH_NOTHING_DECLARED]
        .as_object_mut()
        .expect("the conformance topology carries its node");
    original.remove("customs");
    original.remove("timeoutSeconds");
    let mut declares_both = document["topology"]["nodes"][NODE_WITH_NOTHING_DECLARED].clone();

    // The PRESENT shape: the same node under a new id, with both fields set. It needs its own
    // display-name and objective slots -- the store's minimum projection image demands them per
    // node -- with ids derived from their new position and the original content digests reused.
    declares_both["timeoutSeconds"] = serde_json::json!(DECLARED_TIMEOUT);
    declares_both["customs"] = serde_json::to_value(declared_customs()).unwrap();
    let clone_id = OpaqueId::parse(NODE_WITH_BOTH_DECLARED).unwrap();
    let mut cloned_slot_ids = Vec::new();
    let mut cloned_slots = Vec::new();
    for slot in document["contentSlots"].as_array().unwrap() {
        if slot["ownerKind"] != "node" {
            continue;
        }
        let field_kind = match slot["fieldKind"].as_str().unwrap() {
            "display_name" => ContentFieldKind::DisplayName,
            "objective" => ContentFieldKind::Objective,
            other => panic!("the conformance node carries an unexpected slot field {other}"),
        };
        let slot_id = graphhelm_graph::derive_content_slot_id(
            ContentOwnerKind::Node,
            &clone_id,
            field_kind,
            0,
        )
        .unwrap();
        let mut cloned = slot.clone();
        cloned["ownerId"] = serde_json::json!(NODE_WITH_BOTH_DECLARED);
        cloned["slotId"] = serde_json::json!(slot_id.as_str());
        // A PLACEHOLDER, replaced below once the semantic hash exists. It only has to be unique:
        // `PersistedGraphVersion::new` refuses two slots sharing an evidence id, and a clone that
        // kept the original's would be refused at deserialisation as "invalid persisted graph
        // version" -- before any derivation could run.
        cloned["evidenceId"] = serde_json::json!(format!(
            "{}-declares-both",
            slot["evidenceId"].as_str().unwrap()
        ));
        cloned_slot_ids.push(serde_json::json!(slot_id.as_str()));
        cloned_slots.push(cloned);
    }
    assert_eq!(
        cloned_slots.len(),
        2,
        "HARNESS-BROKE: the conformance node should own exactly a display-name and an objective slot"
    );
    declares_both["contentSlotIds"] = serde_json::Value::Array(cloned_slot_ids);
    document["contentSlots"]
        .as_array_mut()
        .unwrap()
        .extend(cloned_slots);
    document["topology"]["nodes"][NODE_WITH_BOTH_DECLARED] = declares_both;
    document["topology"]["entrypoints"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(NODE_WITH_BOTH_DECLARED));
    let completion = &mut document["topology"]["completion"];
    completion["identifiers"]["terminal.001"] = serde_json::json!(NODE_WITH_BOTH_DECLARED);
    completion["integers"]["terminalCount"] = serde_json::json!(2);
    document["topology"]["executionId"] = serde_json::json!(EXECUTION);
    // The conformance document budgets ONE node (`maxNodes: 1`), and `validate_persisted_budgets`
    // refuses a topology that exceeds its own declared cap. Lifted to exactly the two this fixture
    // carries; every other budget stays as the document declares it.
    document["topology"]["budgets"]["maxNodes"] = serde_json::json!(2);

    let patched: PersistedGraphVersion =
        serde_json::from_value(document).expect("the patched version must deserialise");
    let hashes = graphhelm_graph::persisted_hashes(patched.topology(), patched.content_slots())
        .expect("the patched topology must hash");

    let slots = patched
        .content_slots()
        .iter()
        .map(|slot| {
            ContentSlot::new(
                slot.slot_id().clone(),
                slot.owner_kind(),
                slot.owner_id().clone(),
                slot.field_kind(),
                slot.ordinal(),
                graphhelm_graph::derive_publication_evidence_id(
                    &scope(),
                    FIRST_VERSION,
                    hashes.semantic_hash(),
                    slot,
                )
                .unwrap(),
                slot.content_sha256().clone(),
                slot.sensitivity(),
                slot.required_for_execution(),
            )
        })
        .collect::<Vec<_>>();

    let version = PersistedGraphVersion::new(
        FIRST_VERSION,
        None,
        patched.topology().clone(),
        hashes.topology_hash().clone(),
        hashes.semantic_hash().clone(),
        slots,
        patched.created_by().clone(),
        patched.created_at().clone(),
    )
    .expect("the republished version must build");

    let evidence = version
        .content_slots()
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let ciphertext = vec![u8::try_from(index + 1).unwrap(); 16];
            let reference = EvidenceReference::new(
                slot.evidence_id().clone(),
                slot.content_sha256().clone(),
                RawSha256::parse(hex::encode(Sha256::digest(&ciphertext))).unwrap(),
            );
            let scope = scope();
            let mut aad = Vec::new();
            push(&mut aad, b"graphhelm-evidence-aad-v1");
            push(&mut aad, scope.workspace_id().as_str().as_bytes());
            push(&mut aad, scope.project_id().as_str().as_bytes());
            aad.push(1);
            push(&mut aad, scope.execution_id().unwrap().as_str().as_bytes());
            for value in [
                slot.evidence_id().as_str(),
                "1.0.0",
                "application/json",
                match slot.sensitivity() {
                    Sensitivity::Public => "public",
                    Sensitivity::Internal => "internal",
                    Sensitivity::Confidential => "confidential",
                    Sensitivity::Restricted => "restricted",
                },
                "standard",
                slot.content_sha256().as_str(),
            ] {
                push(&mut aad, value.as_bytes());
            }
            let wrapped = WrappedKey::new(
                "key-1",
                slot.evidence_id().as_str(),
                "xchacha20poly1305",
                vec![1; 24],
                vec![2; 48],
                RawSha256::parse(hex::encode(Sha256::digest(&aad))).unwrap(),
            )
            .unwrap();
            SealedEvidence::new(
                reference,
                scope,
                "application/json",
                slot.sensitivity(),
                "standard",
                "xchacha20poly1305",
                vec![3; 24],
                ciphertext,
                wrapped,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();

    (version, evidence)
}

/// The one append this fixture holds.
fn the_publication() -> PreparedAppend {
    let (version, evidence) = version_and_evidence();
    // The envelope's actor must equal the version's own `createdBy`, or the append is refused.
    let published = NewEvent::new(
        OpaqueId::parse("request-172").unwrap(),
        version.created_by().clone(),
        Sensitivity::Internal,
        EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
        evidence
            .iter()
            .map(|item| item.reference().clone())
            .collect(),
        vec![],
    );
    PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        vec![published],
        evidence,
        vec![],
    )
    .unwrap()
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("the destination directory is creatable");
    for entry in std::fs::read_dir(source).expect("the archive is readable") {
        let entry = entry.expect("a directory entry reads");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("a file type reads").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("an archive file copies");
        }
    }
}

/// Relative path -> bytes, for every file under an `events/` archive except the transient
/// directories the store recreates on open. Sorted by path so two archives compare in order.
fn archive_bytes(events: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("the archive is readable") {
            let entry = entry.expect("a directory entry reads");
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if matches!(name.as_str(), ".tmp" | "active") {
                    continue;
                }
                walk(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(
                    relative,
                    std::fs::read(&path).expect("an archive file reads"),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(events, events, &mut out);
    out
}

fn open_fresh(events: &Path) -> LocalEventRepository {
    LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap()
}

/// The published version as the store REPLAYS it, not as this file built it.
fn replayed_version(repository: &LocalEventRepository) -> PersistedGraphVersion {
    let page = repository.read_stream(&scope(), STREAM, 16, None).unwrap();
    let mut versions = page
        .events
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::GraphVersionPublished(published) => Some(published.version),
            _ => None,
        });
    let version = versions
        .next()
        .expect("LANDMARK: the fixture stream replays a graph_version_published event");
    assert!(
        versions.next().is_none(),
        "the fixture holds exactly one publication, so a second one means the wrong store opened"
    );
    version
}

/// Regenerates the committed fixture from this file's own recipe. Ignored by default: it WRITES
/// into the source tree, and it exists so the fixture's provenance is a program rather than a
/// story. Run it when the recipe above changes on purpose, then commit what it wrote:
///
///     cargo test -p graphhelm-events --test persisted_node_bytes -- --ignored regenerate
///
/// Deterministic on purpose -- fixed clock, fixed ids, fixed actor, fixed ciphertexts -- so two
/// regenerations from the same recipe produce the same bytes, and a diff against the committed
/// copy means the recipe moved.
#[test]
#[ignore = "writes the committed fixture into the source tree; run deliberately, then commit"]
fn regenerate_the_committed_fixture_from_this_recipe() {
    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("events");
    {
        let repository = open_fresh(&events);
        repository.append_atomic(&the_publication()).unwrap();
    }
    let archive = fixture_archive();
    if archive.exists() {
        std::fs::remove_dir_all(&archive).unwrap();
    }
    // The transient directories are recreated on open and git cannot carry them empty, so they are
    // deliberately left out -- exactly the layout the committed acceptance archives have.
    std::fs::create_dir_all(&archive).unwrap();
    for name in ["format.json", "journal.jsonl", "repository.lock"] {
        std::fs::copy(events.join(name), archive.join(name)).unwrap();
    }
    copy_tree(&events.join("blobs"), &archive.join("blobs"));
    std::fs::write(
        archive.parent().unwrap().join(".gitattributes"),
        "# A store's bytes are checked against their own checksum on every open. No eol conversion.\n* -text\n",
    )
    .unwrap();
    // THE JOURNAL IS IGNORED BY THE REPOSITORY'S TOP-LEVEL RULE (`.gitignore:8`, `*.jsonl`), and
    // the first version of this fixture shipped without it: `git add` skipped the file silently,
    // the commit carried seven of eight files, and the cells here passed against an ignored copy
    // that a clean checkout does not have. Found in review (#838). Every other committed
    // `.jsonl` went in by `git add -f`; this rule makes the un-ignore part of the fixture rather
    // than a step someone has to remember, so a regeneration cannot repeat the silence.
    std::fs::write(
        archive.parent().unwrap().join(".gitignore"),
        "# The store's journal is fixture bytes, not a log: the top-level `*.jsonl` rule does not apply.\n!events/*.jsonl\n",
    )
    .unwrap();
}

/// The guard. Opening the committed fixture runs the store's re-serialize-and-compare over a batch
/// that contains a `PersistedNode` -- for the first time anywhere in the suite.
#[test]
fn a_committed_journal_carries_a_persisted_node_so_its_bytes_are_finally_judged() {
    let archive = fixture_archive();
    // POPULATION, FIRST. This is the exact absence #172 is about, so it is asserted rather than
    // assumed: a fixture that lost its publication would open cleanly and guard nothing.
    let journal = std::fs::read_to_string(archive.join("journal.jsonl")).unwrap_or_else(|error| {
        panic!(
            "HARNESS-BROKE: the committed fixture is missing at {} ({error}); regenerate it with the \
             ignored test in this file",
            archive.display()
        )
    });
    assert!(
        journal.contains("graph_version_published"),
        "HARNESS-BROKE: the committed journal carries no graph_version_published event, which is \
         the empty population this cell exists to fill"
    );

    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("events");
    copy_tree(&archive, &events);

    // THE MEASUREMENT: the open re-serializes the stored batch and compares. A drift in how
    // `PersistedNode` serializes -- an attribute lost, a field dropped, a rename -- is an
    // `Integrity` refusal here.
    let repository = LocalEventRepository::open(
        &events,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .expect("the fixture store opens: its stored bytes must match their own re-serialization");

    // And the landmark, so the green above cannot be a store that quietly replayed nothing: both
    // shapes come back, each with exactly what it declared.
    let version = replayed_version(&repository);
    let nodes = version.topology().nodes();
    assert_eq!(
        nodes.len(),
        2,
        "the fixture topology carries both node shapes"
    );
    let nothing = &nodes[&OpaqueId::parse(NODE_WITH_NOTHING_DECLARED).unwrap()];
    assert_eq!(nothing.timeout_seconds(), None);
    assert!(nothing.customs().is_none());
    let both = &nodes[&OpaqueId::parse(NODE_WITH_BOTH_DECLARED).unwrap()];
    assert_eq!(both.timeout_seconds(), Some(DECLARED_TIMEOUT));
    assert_eq!(both.customs(), Some(declared_customs()));
}

/// The fixture must be what the recipe produces. Otherwise "the bytes are judged" is a statement
/// about a file nobody can regenerate, and the day it fails nobody can tell a serializer change
/// from a stale fixture.
#[test]
fn the_committed_fixture_is_what_the_recipe_produces_today() {
    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("events");
    {
        let repository = open_fresh(&events);
        repository.append_atomic(&the_publication()).unwrap();
    }
    // EVERY BYTE THE ARCHIVE COMMITS, not the journal alone. The first version compared only
    // `journal.jsonl`; the five sealed-evidence blobs and `format.json` had no oracle, so a
    // serialisation drift in the evidence would have left this cell green. Found in review
    // (#838). The set of paths is compared too: a file that stops being generated, or one that
    // appears, is a recipe change as much as a byte change is.
    //
    // THE BOUNDARY OF WHAT THIS FILE OBSERVES, so the next reader does not assume more (M,
    // reviewing #838): this is a REGENERATION oracle, not a READ oracle. A drift in how the
    // evidence blobs are WRITTEN reddens here, because committed and regenerated bytes diverge.
    // A drift in how they are READ does not: nothing in this file decodes a blob -- the replay
    // path above filters `graph_version_published` out of the journal and never opens
    // `blobs/`. The five committed blobs are compared, not interpreted.
    let committed = archive_bytes(&fixture_archive());
    let fresh = archive_bytes(&events);
    let committed_paths: Vec<&String> = committed.keys().collect();
    let fresh_paths: Vec<&String> = fresh.keys().collect();
    assert_eq!(
        committed_paths, fresh_paths,
        "the committed archive and a fresh regeneration carry different files"
    );
    for (path, bytes) in &committed {
        assert!(
            bytes == &fresh[path],
            "{path} differs from what this file's recipe produces now. Either the recipe changed \
             (regenerate and commit) or serialization changed (which is the finding)"
        );
    }
    assert!(
        committed.contains_key("journal.jsonl"),
        "HARNESS-BROKE: the committed archive has no journal, so nothing above judged a PersistedNode"
    );
}
