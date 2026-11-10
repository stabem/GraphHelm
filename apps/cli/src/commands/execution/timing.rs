//! #1560: opt-in per-request phase timing for `serve`. With `GRAPHHELM_SERVE_TIMING=1` in the
//! Runtime's environment, every timed request writes ONE JSON line to stderr:
//! `{"event":"serve.timing","route":"tasks","totalMs":12,"phases":{"envelope.open":{"count":3,"ms":4}}}`.
//! The line carries a static route name, phase names, counts and milliseconds only: no ids,
//! no payloads, no paths, no secrets. Off (the default) it costs one cached flag read per call.
//!
//! Phases accumulate on the calling thread, so a route wraps the blocking closure that does its
//! work in [`request`]; [`phase`] anywhere below it adds to that request and is free otherwise.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! {
    static PHASES: RefCell<Option<BTreeMap<&'static str, (u64, Duration)>>> =
        const { RefCell::new(None) };
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("GRAPHHELM_SERVE_TIMING").is_ok_and(|v| v == "1"))
}

/// #1649: whether `GRAPHHELM_SERVE_TIMING=1` is on, for the serve-level lines below.
pub(crate) fn is_enabled() -> bool {
    enabled()
}

/// #1649: one `serve.request` line: method, matched route template, status and milliseconds.
pub(crate) fn emit_request(method: &str, route: &str, status: u16, total: Duration) {
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "serve.request",
            "method": method,
            "route": route,
            "status": status,
            "totalMs": total.as_millis() as u64,
        })
    );
}

/// #1649: one `serve.lag` line over `samples` (how late each probe tick was woken), then clears it.
pub(crate) fn emit_lag(samples: &mut Vec<Duration>) {
    if samples.is_empty() {
        return;
    }
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let max = samples[samples.len() - 1];
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "serve.lag",
            "samples": samples.len(),
            "p50Ms": p50.as_millis() as u64,
            "maxMs": max.as_millis() as u64,
        })
    );
    samples.clear();
}

/// Adds the store's lock wait and anchor validation to a request's phases, each when it was
/// non-zero (#2694).
fn add_lock_phases(
    phases: &mut BTreeMap<&'static str, (u64, Duration)>,
    locks: graphhelm_events::LockTiming,
) {
    for (name, spent) in [("lock.wait", locks.wait), ("lock.anchors", locks.anchors)] {
        if !spent.is_zero() {
            let entry = phases.entry(name).or_default();
            entry.0 += 1;
            entry.1 += spent;
        }
    }
}

/// Runs `work` as one timed request named `route`, and logs its phases when timing is on.
pub(crate) fn request<T>(route: &'static str, work: impl FnOnce() -> T) -> T {
    if !enabled() {
        return work();
    }
    let outer = PHASES.with(|cell| cell.replace(Some(BTreeMap::new())));
    // #2694: the store's lock wait and anchor validation this request spent, reported as phases
    // of their own (they are also inside the phases that contain them).
    // The store reads its clock on the lock path only while this thread tracks, so with timing
    // off (the early return above) no clock is read there at all.
    graphhelm_events::start_lock_timing();
    let _stale = graphhelm_events::take_lock_timing();
    let _stale_phases = graphhelm_events::take_store_phases();
    let started = Instant::now();
    let result = work();
    let total = started.elapsed();
    graphhelm_events::stop_lock_timing();
    let locks = graphhelm_events::take_lock_timing();
    let store_phases = graphhelm_events::take_store_phases();
    PHASES.with(|cell| {
        if let Some(phases) = cell.borrow_mut().as_mut() {
            add_lock_phases(phases, locks);
            for (name, (count, spent)) in store_phases {
                let entry = phases.entry(name).or_default();
                entry.0 += count;
                entry.1 += spent;
            }
        }
    });
    let phases = PHASES.with(|cell| cell.replace(outer)).unwrap_or_default();
    eprintln!("{}", line(route, total, &phases));
    result
}

/// Logs one line for `route` whose total and phases the caller measured itself (#1623): a request
/// that spans threads (serve's async mutation around a blocking command body) cannot use
/// [`request`]'s thread-local phases. Same line shape; nothing is logged with timing off.
pub(crate) fn emit(route: &'static str, total: Duration, phases: &[(&'static str, Duration)]) {
    if !enabled() {
        return;
    }
    let phases = phases
        .iter()
        .map(|(name, spent)| (*name, (1, *spent)))
        .collect::<BTreeMap<_, _>>();
    eprintln!("{}", line(route, total, &phases));
}

/// Adds the time `work` takes to `name` in the current request, when one is being timed.
pub(crate) fn phase<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    let active = enabled() && PHASES.with(|cell| cell.borrow().is_some());
    if !active {
        return work();
    }
    let started = Instant::now();
    let result = work();
    let spent = started.elapsed();
    PHASES.with(|cell| {
        if let Some(phases) = cell.borrow_mut().as_mut() {
            let entry = phases.entry(name).or_default();
            entry.0 += 1;
            entry.1 += spent;
        }
    });
    result
}

fn line(route: &str, total: Duration, phases: &BTreeMap<&'static str, (u64, Duration)>) -> String {
    let phases: serde_json::Map<String, serde_json::Value> = phases
        .iter()
        .map(|(name, (count, spent))| {
            (
                (*name).to_owned(),
                serde_json::json!({"count": count, "ms": spent.as_millis() as u64}),
            )
        })
        .collect();
    serde_json::json!({
        "event": "serve.timing",
        "route": route,
        "totalMs": total.as_millis() as u64,
        "phases": phases,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    /// #2694: a store lock taken on this thread shows up as lock timing, once, then starts over.
    #[test]
    fn a_store_lock_is_reported_once_as_lock_timing() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::commands::event_store(directory.path()).unwrap();
        graphhelm_events::start_lock_timing();
        let _stale = graphhelm_events::take_lock_timing();
        store.with_shared_lock(|| Ok(())).unwrap();
        graphhelm_events::stop_lock_timing();
        let first = graphhelm_events::take_lock_timing();
        assert!(
            !first.anchors.is_zero(),
            "the anchors were validated under the lock"
        );
        assert_eq!(
            graphhelm_events::take_lock_timing(),
            graphhelm_events::LockTiming::default()
        );
    }

    /// Shino's condition 1: with timing off (the default) the lock path reads no clock, so
    /// nothing is accumulated however many locks are taken.
    #[test]
    fn the_lock_timing_stays_default_when_it_is_off() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::commands::event_store(directory.path()).unwrap();
        graphhelm_events::stop_lock_timing();
        let _stale = graphhelm_events::take_lock_timing();
        store.with_shared_lock(|| Ok(())).unwrap();
        store.with_shared_lock(|| Ok(())).unwrap();
        assert_eq!(
            graphhelm_events::take_lock_timing(),
            graphhelm_events::LockTiming::default()
        );
    }

    /// Shino's condition 2: the wait for the store's lock is measured. One thread holds the
    /// store's lock for a while; a second thread with timing on waits for it, and its `wait`
    /// covers (most of) that hold. Removing the wait measurement fails this.
    #[test]
    fn a_blocked_lock_is_reported_as_wait() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::commands::event_store(directory.path()).unwrap();
        let hold = std::time::Duration::from_millis(300);
        let (held, holding) = std::sync::mpsc::channel();
        let wait = std::thread::scope(|scope| {
            scope.spawn(|| {
                store
                    .with_shared_lock(|| {
                        held.send(()).unwrap();
                        std::thread::sleep(hold);
                        Ok(())
                    })
                    .unwrap();
            });
            holding.recv().unwrap();
            graphhelm_events::start_lock_timing();
            let _stale = graphhelm_events::take_lock_timing();
            store.with_shared_lock(|| Ok(())).unwrap();
            graphhelm_events::stop_lock_timing();
            graphhelm_events::take_lock_timing().wait
        });
        assert!(
            wait >= hold / 2,
            "the second lock waited for the first: {wait:?}"
        );
    }

    /// The reporting itself: a non-zero wait and anchors become `lock.wait` / `lock.anchors`
    /// phases; zero ones add nothing.
    #[test]
    fn the_lock_phases_are_reported_when_non_zero() {
        let mut phases = std::collections::BTreeMap::new();
        super::add_lock_phases(
            &mut phases,
            graphhelm_events::LockTiming {
                wait: std::time::Duration::from_millis(5),
                anchors: std::time::Duration::ZERO,
            },
        );
        assert_eq!(
            phases.get("lock.wait"),
            Some(&(1_u64, std::time::Duration::from_millis(5)))
        );
        assert!(!phases.contains_key("lock.anchors"));
    }

    #[test]
    fn the_line_carries_only_route_phase_names_counts_and_milliseconds() {
        let mut phases = std::collections::BTreeMap::new();
        phases.insert(
            "envelope.open",
            (3_u64, std::time::Duration::from_millis(7)),
        );
        let line: serde_json::Value = serde_json::from_str(&super::line(
            "tasks",
            std::time::Duration::from_millis(12),
            &phases,
        ))
        .unwrap();
        assert_eq!(
            line,
            serde_json::json!({"event": "serve.timing", "route": "tasks", "totalMs": 12,
                "phases": {"envelope.open": {"count": 3, "ms": 7}}})
        );
    }
}
