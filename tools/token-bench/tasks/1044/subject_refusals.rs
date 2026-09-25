//! The instrument refuses rather than measuring the wrong product (M08 step 4).
//!
//! ONE test, three phases, deliberately sequential. The first draft split this in two and
//! they raced: the staleness case ages a source one hour into the future, and the freshness
//! case — running in parallel — read that aged world and saw a refusal it had not caused.
//! An instrument whose own tests fabricate each other's state is the defect this milestone
//! keeps paying for, so the phases share one thread and one timeline.
//!
//! `apps/cli` depends on `pathogens` (its `quality certify` path), so a source under this
//! crate IS embodied by the binary — ageing one is a legitimate way to make the question
//! exist, not a trick.

use pathogens::subject::{SubjectRefusal, measurable_binary};

#[test]
fn fresh_measures_stale_refuses_and_the_refusal_is_not_unconditional() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    // The precondition ASKS THE RESOLVER, and that is the whole point of the change. This used to
    // repeat `<root>/target/debug` by hand, so both sides made the identical wrong assumption
    // about where the build went and the test could not see the bug it stood next to (#349).
    //
    // A first attempt at the repair updated both sides to the same NEW expression and left the
    // duplication intact — a comment claiming a repair the structure had not had (found by D
    // reviewing #422). It also scheduled the next occurrence: when the resolver grows
    // `build.target-dir` support, the declared limit of this very PR, a hand-written copy would
    // not follow and the divergence would return.
    //
    // `measurable_binary` already answers `Absent` for "nothing built", which is exactly the
    // question this phase asks — so the question goes to it and there is no second copy to drift.
    if let Err(refusal @ SubjectRefusal::Absent { .. }) = measurable_binary() {
        // State that makes the question exist: nothing built. Absent must refuse by name.
        assert!(refusal.to_string().contains("cannot ask"));
        eprintln!("PHASE absent: {refusal}");
        return;
    }

    // PHASE 1 — fresh: the instrument must actually MEASURE. Without this the refusal
    // could be unconditional, which reads exactly like a working guard and never measures
    // anything.
    let fresh = measurable_binary();
    assert!(
        fresh.is_ok(),
        "with a binary newer than every source, the instrument must measure, not refuse: \
         {fresh:?}. A guard that always refuses is not a guard."
    );

    // PHASE 2 — stale: a source written AFTER the binary was linked. This is the case that
    // used to be a confident green measuring a previous version of the product.
    let witness = root
        .join("tools")
        .join("pathogens")
        .join("src")
        .join("subject.rs");
    let restore = std::fs::metadata(&witness)
        .and_then(|meta| meta.modified())
        .expect("the witness source is readable");
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    filetime::set_file_mtime(&witness, filetime::FileTime::from_system_time(future))
        .expect("the witness mtime is settable");

    let aged = measurable_binary();

    // PHASE 3 — restore before asserting, so a failing assertion cannot leave the tree
    // ageing for every later run.
    filetime::set_file_mtime(&witness, filetime::FileTime::from_system_time(restore))
        .expect("the witness mtime is restorable");

    match aged {
        Err(SubjectRefusal::Stale { newer_source, .. }) => {
            assert!(
                newer_source.ends_with("subject.rs"),
                "the refusal names the source that outran the binary: {}",
                newer_source.display()
            );
        }
        other => panic!("a binary older than its own source must be REFUSED: {other:?}"),
    }

    // PHASE 4 — and measuring works again afterwards, so phase 2 proved a CONDITION, not a
    // permanent state.
    assert!(
        measurable_binary().is_ok(),
        "after restoring the timeline the instrument must measure again"
    );

    // PHASE 5 — a source OUTSIDE the binary's dependency closure must not age it (#1044).
    //
    // This is the mirror of phase 2 and it has to live in this test rather than beside the
    // population cells, for the reason the module doc gives: two tests that both age the shared
    // timeline fabricate each other's state, so the phases share one thread. Phase 2 proves the
    // refusal still fires for a source the binary contains; this proves it does NOT fire for one
    // it does not. Together they say the predicate discriminates, which neither says alone.
    //
    // `ci-canary` is the real subject, not a stand-in: `ci/gate.ps1` rewrites this exact file at
    // the start of every run, and `apps/cli` does not depend on the crate. Cold that was invisible
    // because everything was rebuilt after the write; with artifact reuse the binary keeps its
    // mtime and every warm run refused.
    let outsider = root
        .join("tools")
        .join("ci-canary")
        .join("src")
        .join("nonce.rs");
    assert!(
        outsider.is_file(),
        "the outsider must exist, or ageing it proves nothing: {}",
        outsider.display()
    );
    let outsider_restore = std::fs::metadata(&outsider)
        .and_then(|meta| meta.modified())
        .expect("the outsider is readable");
    filetime::set_file_mtime(&outsider, filetime::FileTime::from_system_time(future))
        .expect("the outsider mtime is settable");

    let with_outsider_aged = measurable_binary();

    filetime::set_file_mtime(
        &outsider,
        filetime::FileTime::from_system_time(outsider_restore),
    )
    .expect("the outsider mtime is restorable");

    assert!(
        with_outsider_aged.is_ok(),
        "a source the binary does not link must not make it stale: {with_outsider_aged:?}"
    );
}
