//! The wire stage vocabulary and the projection stage vocabulary must not drift apart.
//!
//! #162's amendment keeps TWO types on purpose, one per durability layer: the wire enum in
//! `graphhelm_protocols` rides `overdue_exception` into the journal and is never rewritten; the
//! projection enum in `graphhelm_events` labels the fold-derived customs timeline and is rebuilt
//! on every replay. Merging them would promote a disposable vocabulary into a permanent contract.
//!
//! The cost of keeping two is that they can drift, and this file is what that cost buys instead.
//!
//! THE GUARD READS BOTH REAL SOURCES. It serialises the actual enums from the two crates rather
//! than comparing either against a list written here: the defect it exists for lives only in the
//! RELATION between two modules, so a guard holding one side and a copy of the other cannot see
//! it — it would agree with itself while the real pair diverged.

use std::collections::BTreeSet;

use graphhelm_events::CustomsStage as ProjectionStage;
use graphhelm_protocols::CustomsStage as WireStage;

/// Every stage the WIRE can name, serialised from the real type.
fn wire_vocabulary() -> BTreeSet<String> {
    [
        WireStage::Parked,
        WireStage::Claimed,
        WireStage::DeadLettered,
    ]
    .into_iter()
    .map(|stage| {
        serde_json::to_value(stage)
            .expect("a unit-variant enum serialises")
            .as_str()
            .expect("a unit variant serialises to a string")
            .to_owned()
    })
    .collect()
}

/// Every stage the PROJECTION can name, serialised from the real type.
fn projection_vocabulary() -> BTreeSet<String> {
    [
        ProjectionStage::Parked,
        ProjectionStage::Claimed,
        ProjectionStage::Cleared,
        ProjectionStage::Rejected,
        ProjectionStage::Refused,
        ProjectionStage::Overdue,
        ProjectionStage::DeadLettered,
    ]
    .into_iter()
    .map(|stage| {
        serde_json::to_value(stage)
            .expect("a unit-variant enum serialises")
            .as_str()
            .expect("a unit variant serialises to a string")
            .to_owned()
    })
    .collect()
}

/// Every wire stage must exist, spelled identically, in the projection vocabulary.
///
/// Direction matters and is deliberate: the projection is allowed stages the wire never records
/// (`cleared`, `rejected`, `refused` are timeline facts, not lapse causes), but a stage the wire
/// can journal and the timeline cannot name would leave a permanent event the renderer cannot
/// label. The containment runs one way only.
#[test]
fn every_wire_stage_has_an_identically_spelled_projection_stage() {
    let wire = wire_vocabulary();
    let projection = projection_vocabulary();

    // Non-vacuity, not size. A subset assertion passes against an empty left side, so an
    // extraction that silently produced nothing would read as agreement — that is what this
    // guards.
    //
    // It deliberately does NOT assert the size. An earlier version said `wire.len() == 3`, and a
    // sabotage that added a fourth wire variant whose spelling already exists in the projection
    // felled this test too — at the COUNT, not at the containment. That made the two assertions
    // inseparable: a size claim is the other test's job, and holding it here meant one sabotage
    // could not tell the two apart.
    assert!(
        !wire.is_empty(),
        "an empty wire vocabulary would satisfy containment vacuously"
    );
    assert!(
        projection.len() >= wire.len(),
        "the projection vocabulary cannot be smaller than the wire's"
    );

    let missing: Vec<&String> = wire.difference(&projection).collect();
    assert!(
        missing.is_empty(),
        "wire stages with no identically-spelled projection stage: {missing:?} \
         (wire {wire:?} vs projection {projection:?})"
    );
}

/// The wire vocabulary is exactly the three stages an overdue exception can report.
///
/// Separate from the containment test on purpose: containment answers "can the timeline label
/// what the journal recorded", this one answers "is the journal's set still the set that was
/// designed". One assertion covering both would go green on a shared change to both sides.
#[test]
fn the_wire_vocabulary_is_exactly_the_three_designed_stages() {
    let expected: BTreeSet<String> = ["parked", "claimed", "dead_lettered"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    assert_eq!(
        wire_vocabulary(),
        expected,
        "an overdue exception reports the stage that lapsed; adding a fourth means the sweep \
         learned to watch something new, which is a design change and not a rename"
    );
}

/// The SCHEMA's legal subset must be the wire vocabulary, read from the file rather than
/// restated here.
///
/// This is the third real source in the relation, and the parity checker cannot stand in for it:
/// that tool compares property NAMES and required sets, and is blind to the CONTENTS of a
/// `$defs` enum. It reported PARITY OK against a schema saying `waiting` while the Rust enum
/// said `parked` — green, and wrong about the only thing that reaches a journal.
///
/// The schema is read at run time, never `include_str!`d: a source baked into the test binary
/// asserts against a copy from build time, which under a stale build passes while the file on
/// disk says something else.
#[test]
fn the_schema_stage_enum_is_exactly_the_wire_vocabulary() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root is two levels above core/events");

    for relative in [
        "schemas/event-envelope.schema.json",
        "schemas/releases/1.0.0/event-envelope.schema.json",
    ] {
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{relative} must be readable: {error}"));
        let document: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|error| panic!("{relative}: {error}"));

        let reference = document["$defs"]["overdueException"]["properties"]["stage"]["$ref"]
            .as_str()
            .unwrap_or_else(|| panic!("{relative}: overdueException.stage must be a $ref"));
        let key = reference
            .rsplit('/')
            .next()
            .expect("a $ref has a last segment");

        let declared: BTreeSet<String> = document["$defs"][key]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{relative}: $defs/{key} must be a closed enum"))
            .iter()
            .map(|value| value.as_str().expect("enum members are strings").to_owned())
            .collect();

        assert_eq!(
            declared,
            wire_vocabulary(),
            "{relative}: $defs/{key} must list exactly what the wire enum emits"
        );
    }
}

/// The fold's wire->projection mapping is TOTAL and spelling-preserving.
///
/// The two vocabularies agreeing (above) says the sets line up. It does not say the fold
/// actually crosses the boundary correctly: a mapping could be exhaustive, compile, and still
/// send `parked` to `Cleared`. Set agreement and mapping correctness are different claims and
/// the first does not imply the second.
#[test]
fn the_fold_maps_every_wire_stage_to_the_identically_spelled_projection_stage() {
    for wire in [
        WireStage::Parked,
        WireStage::Claimed,
        WireStage::DeadLettered,
    ] {
        let projected = graphhelm_events::project_customs_stage(wire);

        let wire_spelling = serde_json::to_value(wire).unwrap();
        let projected_spelling = serde_json::to_value(projected).unwrap();

        assert_eq!(
            wire_spelling, projected_spelling,
            "the fold must carry a stage across the layer boundary without renaming it"
        );
    }
}
