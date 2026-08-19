// DRAFT — staged for the SECOND PR (the rendezvous-EQUAL burn). Deliberately NOT in
// apps/cli/src/commands/serve/wake.rs, and this file is not compiled by anything.
//
// WHY IT IS PARKED HERE. This test is defect-driven, so it is RED today by construction —
// that is its whole purpose. The window-3 PR has to pass `cargo test --workspace` to land.
// Dropping a knowingly-red test into that crate would make PR 1 fail its own gate on a
// defect PR 1 does not claim to fix, and the usual escape (`#[ignore]`) is worse: an ignored
// test measures nothing while looking like coverage, and ignored tests rot.
//
// So it waits here, typed and reviewable, and PR 2 moves it verbatim into the `tests` module
// in wake.rs next to its siblings. Nothing is lost: it is unbuilt and unrun either way, which
// is the standing discipline while H holds the cargo queue.
//
// EXPECTED RED, registered before running: `recorded == 1` (the stale capture's session and
// rendezvous both match the FRESH lease, so the filter passes and the consumption lands) and
// the fresh lease is gone from the projection. Both assertions below fail. If it comes back
// GREEN, the defect does not exist as described and the seed dies rather than being patched.
//
// IT MUST TARGET THE OUT-OF-WINDOW SLICE, and this fixture does. The window-3 pin fix closed
// the in-window half of this hazard for free: a re-arm landing AFTER the recorder's decision
// read now makes the pinned sequence stale, so the append is refused and the replacement
// lease survives. What that fix cannot touch is a re-arm that landed BEFORE the read — the
// sweep rang, the sleeper woke and re-armed, and only then did the delayed phase 3 look. The
// decision sees a live lease whose session and rendezvous both match the capture, and the pin
// it takes from that same read is perfectly current, so nothing refuses the burn.
//
// The two arms below are appended BEFORE the recorder is called, on purpose. Writing the
// in-window variant instead would produce a test that passes after the window-3 fix and
// proves nothing about this defect.
//
// One more thing this fixture cannot assert, and it is a limit of the STORE rather than of
// the test: `WakeLeaseConsumed` carries execution, session and reason, so nothing on the
// event says WHICH arming was burned. The assertion below is therefore "the replacement
// survived", not "the burn named the old arming". Closing that is the arming-identity field,
// which is this PR's other half.

/// A capture from BEFORE the sleeper woke must not burn the lease the sleeper armed AFTER it.
///
/// The sequence, which is this factory's own daily shape rather than a contrived one: a lease
/// is armed on rendezvous X; a sweep captures it and rings it; the sleeper wakes, does its
/// work, and re-arms — on the SAME rendezvous, because our agents use fixed rendezvous ids;
/// then the first sweep's delayed phase 3 finally runs. Session matches. Rendezvous matches.
/// Nothing else is compared, so the stale consumption burns a lease whose sleeper is at that
/// moment asleep on it.
///
/// Nothing downstream catches this. Consuming a LIVE lease is legal, so the fold accepts it
/// and every replay succeeds — no `Corrupt`, no refused stream, none of the noise the #55
/// family makes. The sleeper simply is never rung again: it blocks until its declared horizon
/// and `wake-wait` exits 3, which reads as "my deadline passed and nothing happened", while
/// the store's own receipt for that session says `rung`. Two surfaces answer one question
/// differently and the operator acts on the calm one.
///
/// The discriminator has to be the ARMING'S IDENTITY, and the obvious cheaper ones do not
/// work: the rendezvous is equal by construction here, and the cursor can be equal too, since
/// re-arming at the head the surface reported is a documented fixed point. The arming's
/// sequence is strictly monotone per stream, so it is the one that separates them.
#[test]
fn a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path();
    let scope = graphhelm_protocols::RepositoryScope::new(
        graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
        graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
        Some(graphhelm_protocols::ExecutionId::parse("exec-rearm").unwrap()),
    );
    let stream = OpaqueId::parse("exec-rearm").unwrap();
    let actor = PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    );

    {
        let store = crate::commands::event_store(events).unwrap();
        let arm = |key: &str, cursor: u64, next: u64| {
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-rearm").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor,
                        // THE SAME rendezvous both times: the fixed-id convention.
                        rendezvous_id: OpaqueId::parse("rdv-fixed").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };
        // The lease the sweep captured, and then the sleeper's re-arm after being rung.
        arm("arm-before", 0, 1);
        arm("arm-after", 1, 2);
    }

    // What the sweep is still holding: the lease as it was BEFORE the wake. Its ring already
    // crossed, which is why the reason is `Rung`.
    let stale = vec![(
        DueLease {
            execution_id: "exec-rearm".to_owned(),
            session_id: "session-p".to_owned(),
            rendezvous_id: "rdv-fixed".to_owned(),
        },
        WakeConsumeReason::Rung,
    )];

    let recorded = record_consumptions(events, "exec-rearm", &stale);
    assert_eq!(
        recorded, 0,
        "a consumption for the lease that existed before the wake must not be recorded \
         against the lease armed after it"
    );

    // The assertion that measures. `recorded == 0` alone would also hold if the recorder had
    // refused for some unrelated reason; the property is that the sleeper's CURRENT lease
    // survives, because that lease is what a future append will ring.
    let store = crate::commands::event_store(events).unwrap();
    let history = store.read_replay_stream(&scope, "exec-rearm").unwrap();
    let projection = graphhelm_events::replay(&scope, "exec-rearm", &history).unwrap();
    assert!(
        projection.wake_leases.contains_key("session-p"),
        "the lease the sleeper is asleep on must still be live — burned here, no later \
         append can ever ring it and the sleeper waits out its full horizon believing \
         nothing happened"
    );
}
