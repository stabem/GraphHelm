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

/// Runs `work` as one timed request named `route`, and logs its phases when timing is on.
pub(crate) fn request<T>(route: &'static str, work: impl FnOnce() -> T) -> T {
    if !enabled() {
        return work();
    }
    let outer = PHASES.with(|cell| cell.replace(Some(BTreeMap::new())));
    let started = Instant::now();
    let result = work();
    let total = started.elapsed();
    let phases = PHASES.with(|cell| cell.replace(outer)).unwrap_or_default();
    eprintln!("{}", line(route, total, &phases));
    result
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
