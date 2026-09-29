//! #1333: specimens for Keel's test-proof rule (`keel.test.green_on_parent`).
//!
//! The gate under certification is `graphhelm keel check --prove-new-tests`'s core,
//! `graphhelm_policy::keel_prove::prove_new_tests`, run for real: each specimen is a tiny crate
//! with a bug on the base, the fix on the head, and one new test beside the fix. Every specimen
//! test is green on the head, so a gate that only runs the new tests calls all of them good. What
//! is wrong with a specimen is that its test would have been green on the base too.
//!
//! **The axis does not call the gate.** [`KeelProveFailureAxis::is_defeated_by`] reads the test's
//! source with its own few lines: whether it ever calls the subject, `total`. A specimen whose axis
//! ran the test on the parent would certify the gate against itself.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use graphhelm_policy::keel_prove::{ProveOptions, TestVerdict, prove_new_tests};
use serde::Serialize;

use crate::{EvidenceGate, FailureAxis, Specimen, Verdict};

static NEXT_REPO: AtomicUsize = AtomicUsize::new(0);

/// The subject every specimen shares: a sum over a price source. The base drops the first item.
const BASE_LIB: &str = "pub trait Prices {\nfn price(&self, item: u32) -> u32;\n}\n\n/// The sum of the prices of `items`.\npub fn total(prices: &dyn Prices, items: &[u32]) -> u32 {\nitems.iter().skip(1).map(|item| prices.price(*item)).sum()\n}\n";
const HEAD_LIB: &str = "pub trait Prices {\nfn price(&self, item: u32) -> u32;\n}\n\n/// The sum of the prices of `items`.\npub fn total(prices: &dyn Prices, items: &[u32]) -> u32 {\nitems.iter().map(|item| prices.price(*item)).sum()\n}\n";

/// One new test file added beside the fix.
#[derive(Clone, Debug, Serialize)]
pub struct KeelProveEvidence {
    /// `tests/total.rs` on the head: the only file the specimen's diff adds.
    pub test_source: String,
}

/// How a new test can look like proof and prove nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeelProveFailureAxis {
    /// The test asserts values it computed itself; the subject is never called.
    Tautology,
    /// The test builds a mock of the subject's input and asserts what the mock returns; the
    /// subject is never called.
    MockAssertsMock,
}

impl FailureAxis<KeelProveEvidence> for KeelProveFailureAxis {
    fn is_defeated_by(&self, evidence: &KeelProveEvidence) -> bool {
        let source = &evidence.test_source;
        let calls_subject = source.contains("total(");
        match self {
            Self::Tautology => !calls_subject && !source.contains("impl Prices"),
            Self::MockAssertsMock => !calls_subject && source.contains("impl Prices"),
        }
    }
}

/// `prove_new_tests` over a throwaway two-commit repository per evaluation.
pub struct KeelProveGate {
    root: PathBuf,
}

impl KeelProveGate {
    /// A gate whose repositories and cargo target live under `root`, which it removes on drop.
    #[must_use]
    pub fn under(root: PathBuf) -> Self {
        Self { root }
    }

    fn repository(&self, test_source: &str) -> Result<PathBuf, String> {
        let repo = self.root.join(format!(
            "repo-{}",
            NEXT_REPO.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&repo);
        let write = |path: &str, text: &str| {
            let full = repo.join(path);
            if let Some(dir) = full.parent() {
                std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
            }
            std::fs::write(full, text).map_err(|error| error.to_string())
        };
        write(
            "Cargo.toml",
            "[package]\nname = \"specimen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )?;
        write(".gitignore", "/target\nCargo.lock\n")?;
        write("src/lib.rs", BASE_LIB)?;
        git(&repo, &["init", "-q"])?;
        git(&repo, &["add", "."])?;
        git(&repo, &["commit", "-q", "-m", "base"])?;
        write("src/lib.rs", HEAD_LIB)?;
        write("tests/total.rs", test_source)?;
        git(&repo, &["add", "."])?;
        git(&repo, &["commit", "-q", "-m", "fix and its test"])?;
        Ok(repo)
    }
}

impl Drop for KeelProveGate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=keel",
            "-c",
            "user.email=keel@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl EvidenceGate<KeelProveEvidence> for KeelProveGate {
    fn id(&self) -> &str {
        "graphhelm-keel/prove-new-tests"
    }

    fn evaluate(&self, evidence: &KeelProveEvidence) -> Verdict {
        let refused = |finding: String| Verdict {
            passed: false,
            findings: vec![finding],
        };
        let repo = match self.repository(&evidence.test_source) {
            Ok(repo) => repo,
            Err(error) => return refused(format!("setup: {error}")),
        };
        let diff = match git(
            &repo,
            &["diff", "--src-prefix=a/", "--dst-prefix=b/", "HEAD~1..HEAD"],
        ) {
            Ok(diff) => diff,
            Err(error) => return refused(format!("setup: {error}")),
        };
        let options = ProveOptions {
            repo: repo.clone(),
            base: "HEAD~1".into(),
            head: "HEAD".into(),
            target_dir: self.root.join("target"),
            scratch_root: self.root.clone(),
            timeout: Duration::from_secs(300),
        };
        let report = match prove_new_tests(&diff, &options) {
            Ok(report) => report,
            Err(error) => return refused(format!("setup: {error}")),
        };
        let _ = std::fs::remove_dir_all(&repo);
        let mut findings: Vec<String> = report
            .findings
            .iter()
            .map(|finding| format!("{}: {}", finding.rule, finding.detail))
            .collect();
        if report.proofs.is_empty() {
            findings.push("keel.test.unproven: no new test was found".into());
        }
        let earned = report
            .proofs
            .iter()
            .all(|proof| proof.verdict == TestVerdict::Earned);
        Verdict {
            passed: earned && findings.is_empty(),
            findings,
        }
    }
}

/// The helper both mock specimens and the regression test share.
const FIXED: &str =
    "struct Fixed;\n\nimpl Prices for Fixed {\nfn price(&self, _item: u32) -> u32 {\n5\n}\n}\n";

/// A test that earns its place: red on the base (it counts the first item), green on the head.
#[must_use]
pub fn real_regression_test() -> KeelProveEvidence {
    KeelProveEvidence {
        test_source: format!(
            "use specimen::{{Prices, total}};\n\n{FIXED}\n#[test]\nfn total_counts_the_first_item() {{\nassert_eq!(total(&Fixed, &[1, 2]), 10);\n}}\n"
        ),
    }
}

/// Specimens for certifying the test-proof gate: each test is green on the head and would have
/// been green on the base, so only running it on the parent can refuse it.
#[must_use]
pub fn keel_prove_suite() -> Vec<Specimen<KeelProveEvidence, KeelProveFailureAxis>> {
    vec![
        Specimen {
            id: "keel-prove/tautology-asserts-its-own-arithmetic".to_owned(),
            axis: KeelProveFailureAxis::Tautology,
            evidence: KeelProveEvidence {
                test_source: "#[test]\nfn total_is_correct() {\nlet expected = 5 + 5;\nassert_eq!(expected, 10);\n}\n".to_owned(),
            },
        },
        Specimen {
            id: "keel-prove/mock-asserts-the-mock".to_owned(),
            axis: KeelProveFailureAxis::MockAssertsMock,
            evidence: KeelProveEvidence {
                test_source: format!(
                    "use specimen::Prices;\n\n{FIXED}\n#[test]\nfn total_uses_the_price_source() {{\nlet mock = Fixed;\nassert_eq!(mock.price(1) + mock.price(2), 10);\n}}\n"
                ),
            },
        },
    ]
}
