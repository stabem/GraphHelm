//! No production surface appends a clearance while nothing can verify one (#541, #161).
//!
//! WHAT THIS COMPLEMENTS, and it replaces nothing — the heading here said "replaces" until J
//! pointed out that no deletion accompanies it and none should (see the table below).
//!
//! #527 shipped a trap scoped to one crate: `apps/cli/tests/`, walking `apps/cli/src/` only.
//! Measured on that PR, the workspace has SIX crates that append events:
//!
//! ```text
//! apps/cli 22 | core/events 17 | core/runtime 6 | postgres-event-store 4 | governor 1 | simulation 1
//! ```
//!
//! Twenty-two of fifty-one sites, and the two most likely homes for an AUTOMATED clearance --
//! `core/runtime` and `core/governor` -- were not among them. The same construction moved one crate
//! over left that guard green and `core/runtime`'s own invariants green with it.
//!
//! **The subject moved from the VERIFIER to the EVENT KIND, and that is what closes the second
//! escape.** #527's sweep looked for `ClearanceVerifier::Countersign`, which a deserialized request
//! body never spells: parsing a `CompletionCleared` produces the variant with zero mentions in Rust,
//! measured. But the appending code still has to name the KIND to append it, whatever route the
//! verifier travelled. One symbol, both escapes.
//!
//! WHY A GUARD AND NOT A CHECK AT THE APPEND BOUNDARY, which would be better. There is no append
//! boundary to instrument: `append_event` exists twice, both inside `apps/cli`, and five other
//! crates call `NewEvent::new` directly. Creating one choke point is a design change belonging to
//! #161's second half, not to a guard. This is the second-best remedy and says so.
//!
//! # The sibling guard, and why two is not duplication here
//!
//! `apps/cli/tests/source_invariants.rs` carries
//! `no_apps_cli_surface_appends_an_unverifiable_countersignature` (#527). It is still there on
//! purpose, and merging the two would lose something. **They watch different symbols and fail
//! differently:**
//!
//! ```text
//! change                                          #527 (verifier)   this file (kind)
//! a Countersign verifier built, never appended         RED              green
//! a clearance arriving deserialized                    green            RED
//! an append from core/runtime                          green            RED
//! ```
//!
//! Code that constructs a countersign verifier and hands it elsewhere WITHOUT appending is #527's
//! and invisible here. Code that appends a clearance whose verifier arrived from anywhere -- a
//! parsed request, another crate, a helper -- is this file's and invisible there. Neither is a
//! superset, so a reader who finds one of them green has been told half a thing, and **deleting
//! either because the other exists loses a row of that table.**
//!
//! Measured with one sabotage rather than argued: a clearance appended through
//! `core/runtime/src/driver.rs`'s existing `WireEventKind` alias reddens HERE and leaves #527's
//! guard green. Two guards over one property is normally how oracles drift, and the rule that
//! keeps this from becoming that is written down rather than assumed: **if they are ever merged,
//! the merged guard keeps BOTH symbols.** Picking the broader-looking one silently drops the
//! narrower one's subject.

use std::collections::BTreeMap;

// ------------------------------------------------------------------------------------------
// The walk is shared with `dead_letter_is_declared_only.rs`, because two workspace sweeps that
// must agree on what they SKIP is a duplicated ORACLE and not a duplicated mechanism.
// ------------------------------------------------------------------------------------------

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/workspace-walk/walk.rs"
));

/// The bare identifier, so an ALIASED path cannot walk past it.
///
/// MEASURED, and this is the half #527 could not have known: `core/runtime/src/driver.rs:258`
/// already imports `EventKind as WireEventKind`. A sweep for the literal `EventKind::CompletionCleared`
/// is blind to `WireEventKind::CompletionCleared` in the one crate most likely to grow an automated
/// clearance. On #527 the same escape was hypothetical -- `apps/cli/src` has no aliased variant
/// imports at all -- so it stayed a bounded note there and becomes the form here.
///
/// The cost is stated rather than discovered later: the enum VARIANT and its payload STRUCT share
/// this name, so the identifier also matches a type position. A file that merely names the type in
/// a signature reddens without appending anything. That is the deliberate trade -- a false positive
/// costs one row and a question; a false negative is an unverifiable clearance nobody sees.
///
/// **THE SECOND FALSE POSITIVE IS THE ONE THAT MATTERS, and it is worse in kind (found by J):**
/// the needle is the EVENT KIND, so a `MachineReplay` clearance reddens here too — **and producing
/// one is SAFE.** Since #521 the fold re-derives its evidence digest, so machine replay is the half
/// of #161 that already works, which makes it the likelier of the two to gain a surface first.
///
/// This sweep cannot tell the two apart and must not try. The verifier variant is exactly what a
/// deserialized body does not spell in Rust, so a needle narrow enough to exclude `MachineReplay`
/// would reopen the escape this file exists to close. #527's guard can exclude it because it reads
/// the verifier; this one cannot, because reading the verifier is the thing that fails.
///
/// **So when a legitimate `MachineReplay` append reddens this: record the site in `allowed_sites`
/// with its reason. Do not widen the needle, and do not delete the guard.** The failure message
/// argues the countersign case because that is the dangerous one, and a reader holding a correct
/// MachineReplay change would otherwise be told by a guard that their change is unsafe.
fn names_the_kind(line: &str) -> bool {
    const NEEDLE: &str = "CompletionCleared";
    let mut rest = line;
    while let Some(at) = rest.find(NEEDLE) {
        let after = &rest[at + NEEDLE.len()..];
        let continues = after
            .chars()
            .next()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
        if !continues {
            return true;
        }
        rest = after;
    }
    false
}

/// A `//` line, doc comments included. Documenting the gap must never redden the guard: a check
/// that fires when someone EXPLAINS the rule teaches deleting the sentence rather than fixing the
/// code, and the message it prints would be about the wrong thing.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Every production site allowed to name the kind, and how many lines name it.
///
/// `src/` ONLY, and tests are out on purpose: `core/events/tests/execution_projection.rs` builds
/// clearances by the dozen and must keep doing so. The claim is about what SHIPS, and a guard that
/// forbade the fixtures would be refusing the coverage that makes the fold trustworthy.
fn allowed_sites() -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        // The declaration itself: the enum variant, the payload struct, and the wire-name row.
        ("core/protocols/src/event.rs", 3),
        // The fold READING a clearance. A match arm is not an append -- naming the kind in order to
        // interpret one already in the journal is the opposite of putting one there.
        ("core/events/src/projection.rs", 1),
        // The briefing READING a clearance (#1063): the decision digest's match arm names the
        // kind to fold "claim #N cleared by ..." out of a journal that already holds it -- the
        // projection.rs shape, one crate over. Nothing in `core/execution` opens a store. The
        // other two lines are the inline `#[cfg(test)]` block's import of the payload type and
        // the fixture that builds a clearance the fold REFUSED, to prove the digest reads the
        // fold's outcome rather than the event name: the walk covers `src/` whole, test blocks
        // included, so they count here as the serve/mod.rs row's fixture lines do.
        ("core/execution/src/briefing.rs", 3),
        // MACHINE REPLAY (#159): the customs verb `clear` appends a clearance, and this is the
        // case the module doc says to record rather than to narrow the needle around. One line is
        // the import of the payload type, the other the append itself, and the verifier on that
        // append is `ClearanceVerifier::MachineReplay` and nothing else: the verb's signature
        // accepts only a `WireHash`, and the CLI/HTTP door
        // (`apps/cli/src/commands/execution/clear.rs::verifier`) refuses `countersign` before the
        // store is opened. No countersignature can travel through this site; a change that
        // widened the signature to carry one would have to come back here and say so.
        ("core/events/src/customs.rs", 2),
        // NAMED (#159): `MutationDecisionKind::CompletionCleared` is the serve router's name for
        // the decision event `execution.clear` produces, and the match arm that carries it READS
        // a clearance already in the journal to recognise an idempotent retry -- the same shape
        // as projection.rs, one crate over. Three of the six lines are the inline `#[cfg(test)]`
        // fixture that builds a clearance to exercise that recognition: the walk covers `src/`
        // whole, test blocks included, so the fixture counts here even though tests are the
        // population this guard means to leave alone.
        ("apps/cli/src/commands/serve/mod.rs", 6),
    ])
}

#[test]
fn the_form_catches_an_aliased_path_and_leaves_its_siblings_alone() {
    assert!(names_the_kind("EventKind::CompletionCleared(payload) => {"));
    assert!(
        names_the_kind("WireEventKind::CompletionCleared(payload)"),
        "an aliased import is the escape this form exists for, and core/runtime already aliases \
         EventKind today"
    );
    assert!(
        names_the_kind("use graphhelm_protocols::EventKind::CompletionCleared;"),
        "a bare variant import is the other spelling of the same escape"
    );
    assert!(
        !names_the_kind("EventKind::CompletionClaimed(payload) => {"),
        "the claim is a different event and does not release a dependent; matching it would put \
         the real thing and a sibling inside one exemption"
    );
}

#[test]
fn no_production_surface_names_the_clearance_outside_the_declared_sites() {
    let mut files = Vec::new();
    for member in workspace_members() {
        let source = member.join("src");
        walk(&source, &["rs"], &mut files);
    }

    // Non-vacuity: an empty walk agrees with any allowlist by finding nothing. The floor is well
    // under today's count and exists to catch a broken walk, not to pin a population.
    assert!(
        files.len() > 100,
        "the walk found only {} production sources across the workspace members, so it is not \
         covering the tree it claims to cover",
        files.len()
    );

    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let hits = text
            .lines()
            .filter(|line| !is_comment(line) && names_the_kind(line))
            .count();
        if hits > 0 {
            found.insert(relative(path), hits);
        }
    }

    let expected: BTreeMap<String, usize> = allowed_sites()
        .into_iter()
        .map(|(path, count)| (path.to_owned(), count))
        .collect();

    assert_eq!(
        found, expected,
        "a production source names CompletionCleared outside the declared sites, or has stopped \
         naming it where it must.\n\nANSWER THIS BEFORE EDITING EITHER SIDE: at the site above, is \
         the clearance being NAMED, APPENDED AS MACHINE REPLAY, or APPENDED AS A \
         COUNTERSIGNATURE?\n\nNAMED -- a match arm reading the journal, a type in a signature, a \
         wire-name row -- is fine. Add the row with its reason, the way \
         core/events/src/projection.rs carries one.\n\nMACHINE REPLAY -- safe, and this guard \
         cannot tell it apart from the other kind because the needle is the EVENT KIND. Since #521 \
         the fold re-derives the evidence digest, so there is nothing unverifiable about it. Record \
         the site in allowed_sites with its reason. Do NOT narrow the needle to exclude it: a \
         needle that reads the verifier is blind to a deserialized body, which is the escape this \
         guard exists to close.\n\nCOUNTERSIGNATURE -- the case this guard exists for, and it must \
         not land alone. `ClearanceVerifier::Countersign` carries `identity` and `key_fingerprint` \
         and NO signature; the fold checks both against the registry and both are public journal \
         data, so anyone able to append can name any registered identity. Land the verification, or \
         the `signature_unverifiable` refusal #161 names, in the SAME change.\n\nThe question is \
         asked rather than the edits offered, because an offered set gets chosen by distance."
    );
}

/// What this sweep cannot see, written here and not only in a pull request body.
///
/// `EventKind` derives `Deserialize`. A future path that parses a WHOLE `NewEvent` or `EventKind`
/// from input appends without naming the variant anywhere, and no source sweep can follow it. That
/// residual is why observing at the APPEND BOUNDARY stays the right answer and this is second-best,
/// and it is the reason #161's second half is still open rather than closed by this file.
///
/// This cell exists so the residual is a fact somebody measured rather than a sentence somebody
/// wrote: it proves the parse succeeds, which is what makes the blindness real.
#[test]
fn the_residual_is_real_a_parsed_request_names_nothing() {
    let body = r#"{
        "executionId": "execution-test",
        "claimSeq": 7,
        "verifier": {
            "type": "countersign",
            "identity": "reviewer-1",
            "keyFingerprint": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
    }"#;
    let cleared: graphhelm_protocols::CompletionCleared =
        serde_json::from_str(body).expect("a request body parses into a clearance payload");
    assert!(
        matches!(
            cleared.verifier,
            graphhelm_protocols::ClearanceVerifier::Countersign { .. }
        ),
        "a deserialized request produces the unverifiable verifier with no Rust naming it"
    );
}
