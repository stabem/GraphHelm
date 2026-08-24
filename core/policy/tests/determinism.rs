//! G7 — repetition across PROCESSES, not permutation within one.
//!
//! The acceptance criterion for task-002 says identical inputs, clock, scopes and snapshots emit
//! byte-identical contracts. A permutation test (G2) shuffles the input order inside ONE process
//! and so shares that process's hash seed; it can therefore miss a `HashMap` on the path from
//! inputs to emitted bytes, or catch it only sometimes — and an intermittent guard gets quieted
//! rather than believed. This cell repeats the SAME input in two separate processes, where the
//! seeds genuinely differ, and compares the bytes.

use std::process::Command;

/// Enough distinct conflict keys that two independent hash orders coinciding is not a thing that
/// happens. The bar is named rather than felt: with N keys the chance of two independent orders
/// agreeing is 1/N!, and at N = 16 that is about 1 in 2e13. A smaller fixture would make this cell
/// pass sometimes for the wrong reason, which is the failure mode it exists to avoid.
const DISTINCT_KEYS: usize = 16;

fn resolve_in_a_fresh_process() -> Vec<u8> {
    let binary = assert_cmd::cargo::cargo_bin!("resolve_once");
    // The path is rendered BEFORE the spawn because the spawn consumes it. Clippy's literal
    // suggestion here (drop the `&`) moves `binary` into `Command::new`, and the panic below
    // still borrows it to name what failed to start - so the suggested edit does not compile.
    // Keeping the diagnostic matters more than the shortest diff: this message is the only thing
    // that says WHICH binary was missing when the harness cannot start.
    let shown = binary.display().to_string();
    let output = Command::new(binary)
        .arg(DISTINCT_KEYS.to_string())
        .output()
        .unwrap_or_else(|error| panic!("HARNESS: could not spawn {shown}: {error}"));

    assert!(
        output.status.success(),
        "HARNESS: the resolver process exited {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stdout.is_empty(),
        "HARNESS: the resolver process emitted no bytes, so the comparison below would compare \
         two empty vectors and pass without measuring anything"
    );
    output.stdout
}

#[test]
fn the_same_rules_resolve_to_identical_bytes_in_separate_processes() {
    let first = resolve_in_a_fresh_process();
    let second = resolve_in_a_fresh_process();

    assert_eq!(
        first,
        second,
        "the same rule set resolved to different bytes in two processes — something on the path \
         from inputs to emitted bytes iterates in hash order, and the byte-identity the acceptance \
         criterion promises does not hold\nfirst:  {}\nsecond: {}",
        String::from_utf8_lossy(&first),
        String::from_utf8_lossy(&second)
    );
}
