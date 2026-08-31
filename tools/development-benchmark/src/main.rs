//! The paired benchmark runner (#225).
//!
//! Most of what this binary can do today is refuse, and that is by design rather than by
//! incompleteness of THIS crate: the blueprint's governing rule is that the runner READS
//! accounting receipts and never produces its own numbers.
//!
//! The receipts argument its first version promised now exists: `--receipts <dir>` points at a
//! run directory a paired driver wrote (`arms.json` + `cases/<id>.json`, the receipt vocabulary
//! verbatim). What it buys is ONE bar of three -- the provider-reported input ratio, printed with
//! its tail -- and the other two bars (session tokens, blind quality) are refused BY NAME on the
//! same output, exit 2 still: a one-bar report is not a verdict. Without `--receipts` the honest
//! output is unchanged -- the typed refusal naming the first quantity nothing recorded.
//!
//! Exit codes: 0 a full verdict (unreachable until every bar has a number); 2 a typed refusal or
//! a partial report; 64 usage error.

use graphhelm_development_benchmark::{
    BenchmarkRefusal, load_manifest, read_run_directory, verify_frozen_files,
};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let mut arguments = std::env::args().skip(1);
    let mut manifest_path: Option<String> = None;
    let mut receipts_path: Option<String> = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--manifest" => manifest_path = arguments.next(),
            "--receipts" => receipts_path = arguments.next(),
            other => {
                eprintln!(
                    "unknown argument `{other}`; usage: --manifest <path> [--receipts <dir>]"
                );
                return 64;
            }
        }
    }
    let Some(path) = manifest_path else {
        eprintln!("usage: graphhelm-development-benchmark --manifest <path> [--receipts <dir>]");
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

    // Without a run directory there is nothing to read: the quantities a comparison needs are
    // born in execution accounting receipts (#222). Naming the baseline arm first is arbitrary
    // but stable; the point is that a missing number is a refusal, never a zero or an estimate.
    let Some(receipts) = receipts_path else {
        return refuse(&BenchmarkRefusal::CostUnavailable {
            field: "compiled_input_tokens".to_owned(),
            arm: "baseline".to_owned(),
        });
    };

    // A run directory carries exactly ONE of the three bars (provider-reported input tokens per
    // arm). The report prints WITH its tail, and the two bars the receipts cannot carry are
    // refused by name -- a one-bar report is not a verdict, so the exit stays 2. Nothing is
    // promoted until every bar has a number.
    match read_run_directory(std::path::Path::new(&receipts), &manifest.cases) {
        Ok(report) => {
            println!("{report:?}");
            println!(
                "{:?}",
                BenchmarkRefusal::CostUnavailable {
                    field: "session_tokens".to_owned(),
                    arm: "both".to_owned(),
                }
            );
            println!(
                "{:?}",
                BenchmarkRefusal::CostUnavailable {
                    field: "blind_quality".to_owned(),
                    arm: "both".to_owned(),
                }
            );
            2
        }
        Err(refusal) => refuse(&refusal),
    }
}

/// One refusal on stdout, debug-typed so the variant name is the contract, exit 2.
fn refuse(refusal: &BenchmarkRefusal) -> i32 {
    println!("{refusal:?}");
    2
}
