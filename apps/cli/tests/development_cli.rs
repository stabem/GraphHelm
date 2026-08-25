//! The development-contract CLI surface (#223), family 3: `development memory-status`.
//!
//! The command reports the governed memory vocabulary and the moves policy allows. It reads the
//! shipped `memory-transition.yaml` — a declared, digest-bound contribution of the built-in package
//! — rather than restating the vocabulary, so this adapter never becomes a second producer of one
//! closed set.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use graphhelm_governor::{MemoryRecord, MemoryState, MemoryTransition, apply_transition};
use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn shipped_policy() -> Value {
    // Read at RUN time, deliberately, rather than `include_str!`d. The production path embeds the
    // same file; this reads what is on disk, so the two guards below compare the SHIPPED artifact
    // against the running code rather than comparing the binary with itself.
    let path = repository_root()
        .join("extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the shipped policy is unreadable at {path:?}: {error}"));
    serde_yaml_ng::from_str(&text).unwrap_or_else(|error| panic!("the policy is not YAML: {error}"))
}

fn run_memory_status() -> Value {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["development", "memory-status"])
        .output()
        .expect("the built binary runs `development memory-status`");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not the JSON envelope ({error}): {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// The command answers with the shipped policy, not a restatement of it.
///
/// The production change this catches: transcribing the vocabulary into the adapter. A hand-copied
/// list stays green on the day the policy changes and reports the old answer confidently, which is
/// worse than failing — it is the shape of a second producer of one vocabulary.
#[test]
fn memory_status_reports_the_shipped_policy_rather_than_a_copy_of_it() {
    let envelope = run_memory_status();
    assert_eq!(
        envelope["ok"], true,
        "the command did not succeed: {envelope}"
    );
    assert_eq!(envelope["command"], "development.memory-status");

    let policy = shipped_policy();

    // Landmark: the policy really carries the fields compared below. Without this, a policy that
    // lost `states` would make the comparison pass over two absent values.
    for field in ["policyVersion", "states", "transitions", "allowed"] {
        assert!(
            !policy[field].is_null(),
            "HARNESS-BROKE: the shipped policy has no {field:?}, so comparing it below would \
             compare two nothings"
        );
    }

    assert_eq!(
        envelope["data"], policy,
        "the command's answer is not the shipped policy"
    );
}

/// **The shipped policy and the Runtime's own check are two copies of one rule, and nothing bound
/// them until this test.**
///
/// `policies/memory-transition.yaml` says *"Only the VERDICT is written here"*. `apply_transition`
/// says *"The allowed set is written out because it is POLICY."* Both are right, and both are
/// authoritative-sounding, which is the problem: a tuple added to one and forgotten in the other
/// leaves the document and the enforcement disagreeing with nobody watching.
///
/// The comparison is DERIVED on both sides. The runtime half is obtained by asking
/// `apply_transition` about every state-transition pair — the one oracle, probed, never a
/// transcription of it — and the policy half by reading the shipped document. A hand-written
/// expectation on either side would be a third copy.
///
/// The production change this catches: adding an allowed move to the policy without teaching the
/// Runtime, or the reverse. Both are silent today.
#[test]
fn the_shipped_policy_and_the_runtime_allow_exactly_the_same_moves() {
    let policy = shipped_policy();

    let declared: BTreeSet<(String, String, String)> = policy["allowed"]
        .as_array()
        .expect("the policy carries an `allowed` sequence")
        .iter()
        .map(|entry| {
            (
                entry["from"].as_str().expect("`from`").to_owned(),
                entry["transition"]
                    .as_str()
                    .expect("`transition`")
                    .to_owned(),
                entry["to"].as_str().expect("`to`").to_owned(),
            )
        })
        .collect();

    let mut enforced = BTreeSet::new();
    let mut refused = 0_usize;
    for state in MemoryState::every() {
        for transition in MemoryTransition::every() {
            let mut record = MemoryRecord::at(*state);
            if apply_transition(&mut record, *transition).is_ok() {
                enforced.insert((
                    wire_state(*state),
                    wire_transition(*transition),
                    wire_state(record.state()),
                ));
            } else {
                refused += 1;
            }
        }
    }

    // Landmarks, both directions: a matrix that allowed everything, or nothing, would make the
    // equality below say nothing about which moves are governed.
    assert!(
        !enforced.is_empty(),
        "HARNESS-BROKE: the Runtime allowed no transition at all"
    );
    assert!(
        refused > 0,
        "HARNESS-BROKE: the Runtime allowed every pair, so this comparison cannot distinguish a \
         governed matrix from an ungoverned one"
    );

    assert_eq!(
        enforced, declared,
        "the shipped policy and the Runtime disagree about which moves are allowed. Left is what \
         `apply_transition` actually does, right is what memory-transition.yaml declares."
    );
}

/// The wire spelling used by the shipped policy, for a state.
///
/// **Written here because `MemoryState` has none.** It is built by `closed_vocabulary!`'s arm
/// WITHOUT wire literals, so unlike `DevelopmentRefusalCode` — whose every variant carries its own
/// `=> "spelling"` — it has `every()` and `Debug` and nothing that names it on a wire. `Debug` is
/// not a contract: it changes with a rename and with a derive, and neither would fail a test that
/// leaned on it.
///
/// So this maps explicitly. **Two claims live here and only the first is true today — J's review
/// of #360, and they are worth keeping apart because the second is the one a reader rounds up to.**
///
/// **TRUE: drift is caught.** Rename on ONE side — the policy, or this map — and the comparison
/// above finds unequal sets and names the pair.
///
/// **NOT TRUE: the spelling is guarded.** A CO-drift passes. Rename `provisional` in
/// `memory-transition.yaml` *and* in this function together and every test here stays green, because
/// nothing else in the repository names these spellings. Measured, with a control:
///
/// * `policies/memory-transition.yaml` has **no reader at all** outside its manifest declaration —
///   `run_memory_status` is its first consumer — so nothing validates it against
///   `schemas/memory-transition.schema.json`, which does declare the enum.
/// * The control that makes that absence real rather than a bad search: the SIBLING policy,
///   `memory-admission.yaml`, **does** have readers, and `core/governor/tests/memory.rs` binds its
///   vocabulary three ways (schema, enum, hand-written list). That test can exist because
///   `MemoryRefusalCode` carries `wire_name()`. `MemoryState` cannot join it for exactly the reason
///   this function exists.
///
/// **So the cure upstream is not tidiness, it is the missing guard**: wire literals on `MemoryState`
/// and `MemoryTransition`, the way the development vocabularies already have them, would let the
/// state vocabulary be bound the same three ways and would close the co-drift. It belongs to
/// `core/governor`, which #223 does not have in scope.
fn wire_state(state: MemoryState) -> String {
    match state {
        MemoryState::Provisional => "provisional",
        MemoryState::Published => "published",
        MemoryState::Superseded => "superseded",
        MemoryState::Withdrawn => "withdrawn",
    }
    .to_owned()
}

/// The wire spelling used by the shipped policy, for a transition. See [`wire_state`] for why this
/// exists here rather than on the type.
fn wire_transition(transition: MemoryTransition) -> String {
    match transition {
        MemoryTransition::Publish => "publish",
        MemoryTransition::Supersede => "supersede",
        MemoryTransition::Withdraw => "withdraw",
    }
    .to_owned()
}
