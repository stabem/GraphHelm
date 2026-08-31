//! The paired benchmark runner (#225).
//!
//! Most of what this binary can do today is refuse, and that is by design rather than by
//! incompleteness of THIS crate: the blueprint's governing rule is that the runner READS
//! accounting receipts and never produces its own numbers, and the receipts it must read are
//! #222's open half -- `compiled_input_tokens` is constructed unavailable in every execution
//! accounting receipt on main. Until that lands, the honest output for any run is the typed
//! refusal naming the first quantity that has no receipt, per arm. When the receipts fill, the
//! ratio machinery in the library (`compare_cost`, `evaluate_run`, `judge`) is already tested and
//! waiting; this binary grows a receipts argument then, not new arithmetic.
//!
//! Exit codes: 0 a comparison was produced; 2 a typed refusal; 64 usage error.

use graphhelm_development_benchmark::{BenchmarkRefusal, load_manifest, verify_frozen_files};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let mut arguments = std::env::args().skip(1);
    let mut manifest_path: Option<String> = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--manifest" => manifest_path = arguments.next(),
            other => {
                eprintln!("unknown argument `{other}`; usage: --manifest <path>");
                return 64;
            }
        }
    }
    let Some(path) = manifest_path else {
        eprintln!("usage: graphhelm-development-benchmark --manifest <path>");
        return 64;
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            // The refusal type already has a word for a manifest that cannot be read as one; a
            // path that cannot be read at all is the same failure one layer down.
            return refuse(&BenchmarkRefusal::Unreadable {
                detail: format!("{path}: {error}"),
            });
        }
    };
    let manifest = match load_manifest(&text) {
        Ok(manifest) => manifest,
        Err(refusal) => return refuse(&refusal),
    };
    // The corpus check above froze the CASE LIST; this freezes the CONTENT the cases point
    // at. The benchmark root is the manifest's own directory -- the corpus travels as one
    // tree.
    let root = std::path::Path::new(&path).parent().map_or_else(
        || std::path::PathBuf::from("."),
        std::path::Path::to_path_buf,
    );
    if let Err(refusal) = verify_frozen_files(&manifest, &root) {
        return refuse(&refusal);
    }
    eprintln!(
        "manifest: {} cases, corpus {}",
        manifest.cases.len(),
        manifest.corpus_digest
    );

    // The quantities a comparison needs are born in execution accounting receipts (#222), and
    // nothing fills `compiled_input_tokens` yet. Naming the baseline arm first is arbitrary but
    // stable; the point is that a missing number is a refusal, never a zero and never an estimate.
    refuse(&BenchmarkRefusal::CostUnavailable {
        field: "compiled_input_tokens".to_owned(),
        arm: "baseline".to_owned(),
    })
}

/// One refusal on stdout, debug-typed so the variant name is the contract, exit 2.
fn refuse(refusal: &BenchmarkRefusal) -> i32 {
    println!("{refusal:?}");
    2
}
