//! #824, failure #2: the two cross-platform root-initialisation lock sites, provoked.
//!
//! `initialize-root:lock-shared` (`local.rs`) and `initialize-root:lock-exclusive` are
//! `fs2::FileExt::lock_shared` / `lock_exclusive`. Lane D measured why no cell existed for
//! them: a conflicting lock makes those calls **block**, so a test that tried to provoke them
//! by holding the real lock would not go red -- it would HANG, and a hang has no colour. The
//! tree already shows the waiting behaviour on the same API
//! (`core/events/tests/shared_lock_blocks_writers.rs`) which is why the route taken here is
//! injection and not contention.
//!
//! **The limit, stated in the cells and not left for a reader to extract:** an injected fault
//! proves the PLUMBING -- that the site's own name reaches the caller across the `open()`
//! boundary instead of collapsing into a nameless `Storage` -- and not that a real `fs2` lock
//! failure routes to that site. Those are different claims, and only the first is measured
//! here. It is the same limit #870's injected-fault cell already lives under.

use std::sync::Arc;

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventRepositoryError, LocalEventRepository, LocalFailpoint};
use graphhelm_protocols::{Clock, IdGenerator};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }
}

struct FixedIds;

impl IdGenerator for FixedIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-00000000")
    }
}

fn site_of(error: EventRepositoryError) -> (&'static str, Option<i32>) {
    match error {
        EventRepositoryError::StorageAt { site, os } => (site, os),
        EventRepositoryError::Storage => panic!(
            "the failure crossed the `open()` boundary as a NAMELESS Storage, which is the \
             defect #824 is about: no consumer can tell which check raised it"
        ),
        other => panic!("{other:?} is not a storage failure at all"),
    }
}

/// The SHARED fast path: reached only by a layout that is already Complete with a lock file,
/// so the store is opened once for real first and the failpoint armed on the REOPEN.
#[test]
fn a_root_lock_failpoint_names_initialize_root_lock_shared() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");

    // ARRANGEMENT, asserted: the first open must SUCCEED, or the reopen below would take the
    // exclusive bootstrap branch and this cell would be measuring the other site.
    drop(
        LocalEventRepository::open(&root, Arc::new(FixedClock), Arc::new(FixedIds))
            .expect("ARRANGEMENT: a clean open must leave a Complete layout with a lock file"),
    );

    // NEGATIVE CONTROL: the same reopen with a DIFFERENT kind armed must still succeed. Without
    // it, a reopen that failed for any unrelated reason -- a stray handle, an unwritable
    // tempdir -- would satisfy the assertion below and certify nothing.
    drop(
        LocalEventRepository::open_with_failpoint(
            &root,
            Arc::new(FixedClock),
            Arc::new(FixedIds),
            LocalFailpoint::ActiveMarker,
        )
        .expect("CONTROL: a kind that fires on APPEND must leave this open untouched"),
    );

    let Err(error) = LocalEventRepository::open_with_failpoint(
        &root,
        Arc::new(FixedClock),
        Arc::new(FixedIds),
        LocalFailpoint::InitializeRootLockShared,
    ) else {
        panic!(
            "the armed shared root lock must FAIL the open: the failpoint never reached the site"
        )
    };

    let (site, os) = site_of(error);
    assert_eq!(
        site, "initialize-root:lock-shared",
        "the site must be the production literal of the check that would have run, not a \
         synthetic failpoint name: a synthetic name would prove the injection works and say \
         nothing about whether THIS site's name survives the boundary"
    );
    assert_eq!(
        os, None,
        "an injected fault has no operating-system error behind it, and saying so is how a \
         reader tells this plumbing measurement apart from a real lock failure"
    );
}

/// The EXCLUSIVE bootstrap path: a root that does not exist yet is not Complete, so the shared
/// fast path answers None and `initialize_root_locked` runs.
#[test]
fn a_root_lock_failpoint_names_initialize_root_lock_exclusive() {
    let directory = tempfile::tempdir().unwrap();

    // NEGATIVE CONTROL, and it runs FIRST, on its own root: the bootstrap open must SUCCEED
    // when the kind armed fires elsewhere.
    drop(
        LocalEventRepository::open_with_failpoint(
            directory.path().join("control"),
            Arc::new(FixedClock),
            Arc::new(FixedIds),
            LocalFailpoint::ActiveMarker,
        )
        .expect("CONTROL: a kind that fires on APPEND must leave the bootstrap untouched"),
    );

    let root = directory.path().join("repository");
    let Err(error) = LocalEventRepository::open_with_failpoint(
        &root,
        Arc::new(FixedClock),
        Arc::new(FixedIds),
        LocalFailpoint::InitializeRootLockExclusive,
    ) else {
        panic!(
            "the armed exclusive root lock must FAIL the open: the failpoint never reached the site"
        )
    };

    let (site, os) = site_of(error);
    assert_eq!(
        site, "initialize-root:lock-exclusive",
        "the site must be the production literal of the check that would have run"
    );
    assert_eq!(os, None, "an injected fault has no OS error behind it");
}

/// The two kinds must not be interchangeable: arming one must never fire at the other's site.
/// Distinctness alone would stay green with the two arms SWAPPED, and a swap is exactly the
/// defect this ficha recorded once already -- #907's PR table had these two site names written
/// with the words swapped, caught only by a review pass.
#[test]
fn the_two_root_lock_kinds_do_not_fire_at_each_others_site() {
    let directory = tempfile::tempdir().unwrap();

    // The SHARED kind armed against a root that does not exist yet reaches the EXCLUSIVE path,
    // where it must not fire at all.
    let bootstrap = LocalEventRepository::open_with_failpoint(
        directory.path().join("bootstrap"),
        Arc::new(FixedClock),
        Arc::new(FixedIds),
        LocalFailpoint::InitializeRootLockShared,
    );
    assert!(
        bootstrap.is_ok(),
        "the shared kind fired on the EXCLUSIVE path, so one arming reaches both sites and \
         neither name discriminates: {:?}",
        bootstrap.err()
    );
    drop(bootstrap);

    // And the EXCLUSIVE kind armed against an already-Complete root reaches the SHARED fast
    // path, where it must not fire either.
    let root = directory.path().join("complete");
    drop(
        LocalEventRepository::open(&root, Arc::new(FixedClock), Arc::new(FixedIds))
            .expect("ARRANGEMENT: a clean open must leave a Complete layout"),
    );
    let reopen = LocalEventRepository::open_with_failpoint(
        &root,
        Arc::new(FixedClock),
        Arc::new(FixedIds),
        LocalFailpoint::InitializeRootLockExclusive,
    )
    .err();
    assert!(
        reopen.is_none(),
        "the exclusive kind fired on the SHARED fast path: {reopen:?}"
    );
}
