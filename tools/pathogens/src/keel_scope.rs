//! #1330: specimens for the Keel scope rule (`keel.scope.path_outside_card`).
//!
//! The gate under certification is `graphhelm keel check`'s core, `graphhelm_policy::keel::check`,
//! run under the shipped policy. Each specimen is a diff that a card-blind counter would call
//! green: it adds nothing over any surface budget, parses cleanly and names a valid card. What is
//! wrong with it is only WHERE it writes.
//!
//! **The axis does not call the gate.** [`KeelScopeFailureAxis::is_defeated_by`] reads the `+++`
//! and `diff --git` headers with its own few lines and compares them with the card literally. A
//! specimen whose axis asked the gate "is this outside?" would certify the gate against itself.

use graphhelm_policy::keel::{Card, KeelPolicy, check};
use serde::Serialize;

use crate::{EvidenceGate, FailureAxis, Specimen, Verdict};

/// One diff and the card it was written against.
#[derive(Clone, Debug, Serialize)]
pub struct KeelScopeEvidence {
    /// A unified diff as `git diff --src-prefix=a/ --dst-prefix=b/` prints it.
    pub diff: String,
    /// The card, as the author declared it.
    pub card: Card,
}

/// How a scope check can be fooled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeelScopeFailureAxis {
    /// The diff edits a source file the card does not list.
    PathOutsideCard,
    /// The diff adds a test file the card does not list: a test nobody planned, in a place nobody
    /// reviews as part of the promise.
    SneakedTestFile,
}

/// The paths a diff writes, read from `+++ b/` lines only (every specimen here has one per file).
fn written_paths(diff: &str) -> Vec<&str> {
    diff.lines()
        .filter_map(|line| line.strip_prefix("+++ b/"))
        .collect()
}

/// Literal containment: equal to a scope path, or under it as a directory.
fn listed(path: &str, card: &Card) -> bool {
    card.scope_paths.iter().any(|scope| {
        path == scope
            || path
                .strip_prefix(scope.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

impl FailureAxis<KeelScopeEvidence> for KeelScopeFailureAxis {
    fn is_defeated_by(&self, evidence: &KeelScopeEvidence) -> bool {
        let outside: Vec<&str> = written_paths(&evidence.diff)
            .into_iter()
            .filter(|path| !listed(path, &evidence.card))
            .collect();
        match self {
            Self::PathOutsideCard => !outside.is_empty(),
            Self::SneakedTestFile => {
                evidence.diff.contains("new file mode")
                    && outside
                        .iter()
                        .any(|path| path.starts_with("tests/") || path.contains("/tests/"))
            }
        }
    }
}

/// `graphhelm keel check`'s core under one policy.
pub struct KeelScopeGate {
    policy: KeelPolicy,
}

impl KeelScopeGate {
    /// The gate under the shipped policy, read from the package fixture that
    /// `core/policy/tests/keel.rs` proves equal to `keel.yaml`. Embedded so a moved path breaks
    /// the build instead of emptying the gate.
    #[must_use]
    pub fn shipped() -> Self {
        const POLICY: &str = include_str!(
            "../../../extensions/builtin/graphhelm-development-contracts/fixtures/keel/valid/policy-shipped-shape.json"
        );
        Self {
            policy: serde_json::from_str(POLICY).expect("shipped keel policy fixture parses"),
        }
    }
}

impl EvidenceGate<KeelScopeEvidence> for KeelScopeGate {
    fn id(&self) -> &str {
        "graphhelm-keel/check"
    }

    fn evaluate(&self, evidence: &KeelScopeEvidence) -> Verdict {
        let bytes = serde_json::to_vec(&evidence.card).map_or(0, |bytes| bytes.len() as u64);
        let report = check(&evidence.diff, Some((&evidence.card, bytes)), &self.policy);
        Verdict {
            passed: !report.refused,
            findings: report
                .findings
                .iter()
                .filter(|finding| finding.blocking)
                .map(|finding| format!("{}: {}", finding.rule, finding.detail))
                .collect(),
        }
    }
}

fn card(scope: &[&str], symbols: &[&str]) -> Card {
    Card {
        promise: "parse() accepts a trailing comma".to_owned(),
        scope_paths: scope.iter().map(|path| (*path).to_owned()).collect(),
        proof: "cargo test -p parser".to_owned(),
        exported_symbols: symbols.iter().map(|symbol| (*symbol).to_owned()).collect(),
        allowance: None,
    }
}

/// Specimens for certifying a Keel scope gate: each is within every surface budget, so only the
/// scope rule can refuse it.
#[must_use]
pub fn keel_scope_suite() -> Vec<Specimen<KeelScopeEvidence, KeelScopeFailureAxis>> {
    let in_card_edit = "diff --git a/src/parser.rs b/src/parser.rs\n--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -1,1 +1,2 @@\n fn parse() {}\n+// accepts a trailing comma\n";
    vec![
        Specimen {
            id: "keel-scope/edit-to-a-file-the-card-does-not-list".to_owned(),
            axis: KeelScopeFailureAxis::PathOutsideCard,
            evidence: KeelScopeEvidence {
                diff: format!(
                    "{in_card_edit}diff --git a/src/config.rs b/src/config.rs\n--- a/src/config.rs\n+++ b/src/config.rs\n@@ -1,1 +1,2 @@\n const LIMIT: u32 = 1;\n+const RETRIES: u32 = 3;\n"
                ),
                card: card(&["src/parser.rs"], &[]),
            },
        },
        Specimen {
            id: "keel-scope/extra-test-file-sneaked-in-beside-the-planned-one".to_owned(),
            axis: KeelScopeFailureAxis::SneakedTestFile,
            evidence: KeelScopeEvidence {
                diff: format!(
                    "{in_card_edit}diff --git a/tests/parser.rs b/tests/parser.rs\n--- a/tests/parser.rs\n+++ b/tests/parser.rs\n@@ -1,1 +1,3 @@\n use parser::parse;\n+#[test]\n+fn trailing_comma() {{}}\ndiff --git a/tests/unplanned.rs b/tests/unplanned.rs\nnew file mode 100644\n--- /dev/null\n+++ b/tests/unplanned.rs\n@@ -0,0 +1,2 @@\n+#[test]\n+fn sneaked() {{}}\n"
                ),
                card: card(&["src/parser.rs", "tests/parser.rs"], &[]),
            },
        },
    ]
}
