//! #211 Task 1: the harness must judge evidence it was not written for.
//!
//! The certification RULE already exists and is good — a gate is certified only by rejecting every
//! specimen, and growing the suite voids stale certifications by comparison. What it cannot do is
//! judge anything that is not a `Deliverable`, which is a geometry/HTML shape. JPD evidence is
//! typed JSON artifacts, so a JPD validator cannot be a `CandidateGate` at all.
//!
//! These arms pin the generalisation without touching the rule.

use pathogens::{EvidenceGate, Specimen, Verdict, certify};
use serde::Serialize;

/// Evidence deliberately unlike a `Deliverable`: no html, no claims, no journey steps.
#[derive(Serialize)]
struct Note {
    text: String,
}

/// A failure axis of this evidence's own kind, not of a rendered surface.
#[derive(Serialize)]
enum NoteAxis {
    Empty,
}

impl pathogens::FailureAxis<Note> for NoteAxis {
    fn is_defeated_by(&self, note: &Note) -> bool {
        match self {
            NoteAxis::Empty => note.text.is_empty(),
        }
    }
}

struct RejectsEmptyNotes;

impl EvidenceGate<Note> for RejectsEmptyNotes {
    fn id(&self) -> &str {
        "gate/rejects-empty-notes"
    }

    fn evaluate(&self, note: &Note) -> Verdict {
        if note.text.is_empty() {
            Verdict {
                passed: false,
                findings: vec!["the note is empty".to_owned()],
            }
        } else {
            Verdict {
                passed: true,
                findings: Vec::new(),
            }
        }
    }
}

/// A suite holding ONE specimen: a note whose text is empty.
///
/// Renamed from `empty_note_suite`, which read as "an empty suite of notes" -- the exact thing
/// `certify` now refuses with `RefusalCause::EmptySuite`. It is the opposite: the suite has one
/// member, and it is that member's TEXT that is empty.
///
/// The old name cost something real. While checking whether any test depended on `certify`
/// accepting an empty suite, this was the one name that could have inverted the answer, and it
/// had to be opened and read to rule out. A name that has to be disproved is a name that will
/// mislead the reader who does not think to check.
fn suite_with_one_empty_note() -> Vec<Specimen<Note, NoteAxis>> {
    vec![Specimen {
        id: "empty-note".to_owned(),
        axis: NoteAxis::Empty,
        evidence: Note {
            text: String::new(),
        },
    }]
}

/// The production change that would make this fail: `CandidateGate` naming a concrete
/// `Deliverable` instead of an associated evidence type.
#[test]
fn a_gate_can_be_certified_over_evidence_that_is_not_a_deliverable() {
    let certification = certify(&RejectsEmptyNotes, &suite_with_one_empty_note())
        .expect("the gate rejects the specimen");

    assert_eq!(certification.gate_id, "gate/rejects-empty-notes");
    assert_eq!(certification.specimens, 1);
}

/// The rule must survive the generalisation UNCHANGED: any pass anywhere refuses the whole
/// certification, and the refusal NAMES which specimen got through.
#[test]
fn one_specimen_slipping_through_still_refuses_the_whole_certification() {
    struct PassesEverything;

    impl EvidenceGate<Note> for PassesEverything {
        fn id(&self) -> &str {
            "gate/passes-everything"
        }

        fn evaluate(&self, _note: &Note) -> Verdict {
            Verdict {
                passed: true,
                findings: Vec::new(),
            }
        }
    }

    let refusal = certify(&PassesEverything, &suite_with_one_empty_note())
        .expect_err("a gate fooled once is not certified");

    assert_eq!(
        refusal.fooled_by,
        vec!["empty-note".to_owned()],
        "the refusal must NAME which specimen got through, not merely that one did"
    );
}

/// Fixture integrity must generalise WITH the harness, or it is dropped in silence.
///
/// Found by N: `is_useless_on_its_axis` is not a gate — it is the check that a specimen is
/// genuinely defeated on the axis it claims. It reads `d.interaction`, `d.claims`, `d.html`,
/// `d.reachable_ids`, so making the harness generic left it typed to geometry and therefore
/// inapplicable to any other evidence.
///
/// **Nothing goes red when that half disappears.** `certify` still works, `fooled_by` still
/// names what slipped, the suite still certifies — and nobody checks the specimens defeat
/// anything. A specimen that defeats nothing certifies a gate that caught nothing.
///
/// The production change that would make this fail: an axis that is a label rather than a
/// claim it can check.
#[test]
fn an_axis_can_say_whether_its_own_specimen_is_genuinely_defeated() {
    let real = Specimen {
        id: "empty-note".to_owned(),
        axis: NoteAxis::Empty,
        evidence: Note {
            text: String::new(),
        },
    };
    let fraudulent = Specimen {
        id: "not-actually-empty".to_owned(),
        axis: NoteAxis::Empty,
        evidence: Note {
            text: "this note has content".to_owned(),
        },
    };

    assert!(
        pathogens::is_defeated_on_its_axis(&real),
        "a specimen claiming the Empty axis with empty text IS defeated on it"
    );
    assert!(
        !pathogens::is_defeated_on_its_axis(&fraudulent),
        "a specimen claiming an axis it does not defeat must be caught: otherwise it certifies a gate for catching nothing"
    );
}
