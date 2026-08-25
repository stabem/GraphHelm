//! The pathogen suite and the thymus harness (M06 Task 2).
//!
//! A gate earns the right to gate by REJECTING every specimen in a bred suite of
//! useless-but-green deliverables. A gate that passes even one pathogen is itself fake
//! and certification is refused. The suite digest travels in `GateCertified`, so growing
//! the suite voids old certifications by comparison, never by cleanup.
//!
//! Certification is NECESSARY, never sufficient: it proves a gate knows how to reject
//! uselessness, not that it accepts good work — that half lives in each gate's own
//! positive tests (Task 3).

pub mod subject;

pub mod jpd;
pub mod retry_lineage;

use std::collections::BTreeSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// One claimed feature of a deliverable, as a spec would state it. `feature` doubles as
/// the user-visible label the rendered element should carry.
#[derive(Clone, Debug, Serialize)]
pub struct Claim {
    /// What the spec says exists — also the label the element renders.
    pub feature: String,
    /// The element id that renders the feature, when the spec names one.
    pub element_id: Option<String>,
    /// The artifact backing the claim, when one exists.
    pub artifact: Option<String>,
}

/// One step of a recorded user journey.
#[derive(Clone, Debug, Serialize)]
pub struct JourneyStep {
    /// The action taken.
    pub action: String,
    /// The assertion text recorded after the action, if any.
    pub assertion: Option<String>,
    /// Whether the step exercises an error path.
    pub exercises_error_path: bool,
}

/// One test case as a correctness measure would see it.
#[derive(Clone, Debug, Serialize)]
pub struct TestCase {
    /// The test name.
    pub name: String,
    /// Whether the test passed.
    pub passed: bool,
    /// How many assertions the test body actually makes.
    pub assertions: u32,
}

/// A summary of the change that produced the deliverable.
#[derive(Clone, Debug, Serialize)]
pub struct DiffSummary {
    /// Files the change touched.
    pub files_touched: u32,
    /// Lines that change behavior (not comments, not formatting).
    pub behavior_lines: u32,
}

/// Everything a gate may inspect about a delivered feature.
#[derive(Clone, Debug, Serialize)]
pub struct Deliverable {
    /// The spec claims.
    pub claims: Vec<Claim>,
    /// The rendered surface.
    pub html: String,
    /// Element ids reachable by navigation from the root.
    pub reachable_ids: BTreeSet<String>,
    /// The recorded journey.
    pub journey: Vec<JourneyStep>,
    /// The test suite as run.
    pub tests: Vec<TestCase>,
    /// The change summary.
    pub diff: DiffSummary,
    /// What answering one declared operator question cost, when this specimen makes a
    /// claim about interaction at all.
    ///
    /// `None` means this specimen makes NO CLAIM about interaction — never "zero calls".
    /// The distinction is asserted, not merely documented: prose and check diverge and
    /// review reads the prose (paid for twice on 2026-08-18, by both agents).
    ///
    /// `skip_serializing_if` is REQUIRED, not stylistic (Agent B's condition): the suite
    /// digest is sha256 over the canonical JSON of the WHOLE specimen list, so without it
    /// the specimens that already existed would start serializing `"interaction":null` and their
    /// bytes would move. The digest would then shift because the MOLD changed rather than
    /// because the suite grew — and since adding specimens voids certification anyway, that
    /// error would be invisible inside a green.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<InteractionTrace>,
}

/// One call an operator made while trying to answer a declared question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InteractionCall {
    /// The surface called, in the vocabulary the operator sees.
    pub tool: String,
    /// Bytes the reply carried — the cost that does not show up as a round trip.
    pub payload_bytes: u64,
    /// Whether the ANSWER to the declared question was in this reply. A call can be
    /// correct, large and still not answer.
    pub answered: bool,
}

/// What one operator question cost: the budget it declared and the calls it actually took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InteractionTrace {
    /// The operator question, declared with its budget — a budget nobody wrote down is a
    /// budget nobody can miss.
    pub question: String,
    /// Calls the question is allowed to cost.
    pub budget_calls: u64,
    /// Calls it actually took, in order.
    pub calls: Vec<InteractionCall>,
}

/// The ways a deliverable can be green by correctness measures and useless by
/// construction. One specimen per mode; the mode names the axis the specimen defeats.
///
/// The count is not stated, for the reason recorded on `suite()`: it said "ten" over twelve
/// variants, which is the same wrong number the suite's own doc carried (#272).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum UselessnessMode {
    /// Claimed and backed by an artifact — the element never renders anywhere.
    DeadFeature,
    /// The element is in the HTML but outside the reachable set.
    UnreachableUi,
    /// Every journey step asserts something that references nothing delivered.
    TautologicalJourney,
    /// Tests green, claims present — the screen renders nothing.
    BlankScreen,
    /// The view renders but nothing links to it.
    OrphanView,
    /// Tests pass because their assertions were removed.
    GuttedAssertion,
    /// The journey never leaves the happy path.
    HappyPathOnly,
    /// The spec claims a feature no artifact backs.
    SpecClaimWithoutArtifact,
    /// The diff touches files without changing behavior.
    MinimalDiffNoBehavior,
    /// Both labels render — swapped, so each element names the other's feature.
    LabelSwappedUi,
    /// Right, in six calls, where one would do. Green by every correctness measure we own:
    /// the answer IS there and the operator paid six round trips to assemble it.
    ExpensiveButCorrect,
    /// The mirror, and the one this architecture produces under cost pressure: one call,
    /// everything inside, nothing answered. A lone call-counter TEACHES this shape, which
    /// is why the pair enters together or not at all.
    DumpedButUnanswered,
}

/// One pathogen: a deliverable that is green by correctness measures and useless by
/// construction, bred to fool a specific plausible gate.
#[derive(Clone, Debug, Serialize)]
pub struct Specimen<E, A> {
    /// Stable id, part of the canonical digest.
    pub id: String,
    /// The axis this specimen defeats.
    ///
    /// Generic alongside the evidence, and deliberately so: a JPD failure axis and a UI
    /// uselessness axis are different KINDS of thing. Folding both into one closed enum would
    /// give every exhaustive match over it arms it cannot mean, and that failure is silent
    /// because the enum still compiles everywhere.
    pub axis: A,
    /// The evidence a candidate gate is shown.
    pub evidence: E,
}

/// The geometry suite's instantiation — what every existing call site means by `Specimen`.
pub type GeometrySpecimen = Specimen<Deliverable, UselessnessMode>;

/// A candidate gate's verdict over one deliverable — the same refusal-with-findings
/// shape the `GateVerdict` kind carries on the wire: a refusal always carries findings.
#[derive(Clone, Debug)]
pub struct Verdict {
    /// Whether the gate passes the deliverable.
    pub passed: bool,
    /// The findings behind a refusal.
    pub findings: Vec<String>,
}

impl Verdict {
    fn pass() -> Self {
        Self {
            passed: true,
            findings: Vec::new(),
        }
    }

    fn refuse(finding: &str) -> Self {
        Self {
            passed: false,
            findings: vec![finding.to_owned()],
        }
    }
}

/// Anything that wants to gate deliverables and must first survive the thymus.
/// Anything that gates EVIDENCE of some kind and must first survive the thymus.
///
/// The generic trait, and the one `certify` speaks. `Deliverable` is one instantiation;
/// typed JPD artifacts are another.
pub trait EvidenceGate<E> {
    /// The gate's stable id — what `GateCertified` names.
    fn id(&self) -> &str;
    /// Evaluate one piece of evidence.
    fn evaluate(&self, evidence: &E) -> Verdict;
}

/// The geometry-facing spelling, kept EXACTLY as it was.
///
/// This is a bridge, not a second oracle: there is still one `certify` and one certification
/// rule. Keeping this shape means every existing geometry gate compiles untouched, which is
/// what lets the harness change land WITHOUT dragging its consumers into the same branch —
/// M06's binding decision 5 forbids the judge and the judged travelling together, and a
/// signature change that forces its consumers to move is exactly that collision.
pub trait CandidateGate {
    /// The gate's stable id — what `GateCertified` names.
    fn id(&self) -> &str;
    /// Evaluate one deliverable.
    fn evaluate(&self, deliverable: &Deliverable) -> Verdict;
}

impl<G: CandidateGate + ?Sized> EvidenceGate<Deliverable> for G {
    fn id(&self) -> &str {
        CandidateGate::id(self)
    }

    fn evaluate(&self, deliverable: &Deliverable) -> Verdict {
        CandidateGate::evaluate(self, deliverable)
    }
}

/// The thymus receipt: this gate rejected every specimen of the digested suite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certification {
    /// The certified gate.
    pub gate_id: String,
    /// The canonical digest of the suite the gate rejected, `sha256:<64 hex>`.
    pub suite_digest: String,
    /// How many specimens the suite held.
    pub specimens: u32,
}

/// Certification refused: the gate passed at least one pathogen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificationRefusal {
    /// The refused gate.
    pub gate_id: String,
    /// The specimens that fooled it.
    pub fooled_by: Vec<String>,
    /// Why certification was refused.
    pub cause: RefusalCause,
}

/// Why a certification was refused.
///
/// A CLOSED set. Kept apart from `fooled_by` because the two causes are different KINDS of
/// failure: one is a fact about the GATE, the other about the SUITE handed to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalCause {
    /// The gate passed at least one specimen; `fooled_by` names them.
    GatePassedSpecimens,
    /// The suite held no specimens, so nothing ever attacked the gate.
    EmptySuite,
}

/// The bred suite: one specimen per uselessness mode, deterministic.
///
/// The size is deliberately not stated here. It said "ten" while the `vec![]` below held twelve,
/// and `certification.specimens` is already asserted against the real count -- a number repeated
/// in prose is a second producer of it, and the prose is the copy nothing checks (#272).
#[must_use]
pub fn suite() -> Vec<GeometrySpecimen> {
    vec![
        dead_feature(),
        unreachable_ui(),
        tautological_journey(),
        blank_screen(),
        orphan_view(),
        gutted_assertion(),
        happy_path_only(),
        spec_claim_without_artifact(),
        minimal_diff_no_behavior(),
        label_swapped_ui(),
        expensive_but_correct(),
        dumped_but_unanswered(),
    ]
}

/// Right, in six calls, where one would do — the shape the blind judge complained about
/// while calling the surface correct. Every correctness measure we own passes it.
fn expensive_but_correct() -> GeometrySpecimen {
    Specimen {
        id: "expensive-but-correct".to_owned(),
        axis: UselessnessMode::ExpensiveButCorrect,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Sleep answer".to_owned(),
                element_id: Some("status".to_owned()),
                artifact: Some("apps/cli/src/commands/execution/mod.rs".to_owned()),
            }],
            html: "<main id=\"home\"><a href=\"#status\">status</a>                   <section id=\"status\">running</section></main>"
                .to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "status".to_owned()]),
            journey: asserted_journey("Sleep answer"),
            tests: green_tests(),
            diff: behavior_diff(),
            interaction: Some(InteractionTrace {
                question: "can I go back to sleep?".to_owned(),
                budget_calls: 1,
                calls: vec![
                    call("status", 900, false),
                    call("events", 4_200, false),
                    call("events", 4_200, false),
                    call("wake_status", 300, false),
                    call("probe", 250, false),
                    // The sixth carries it: correct, and paid for six times over.
                    call("status", 900, true),
                ],
            }),
        },
    }
}

/// The mirror: one call, everything inside, nothing answered. This is what a lone call
/// counter teaches a system to build, which is why the two enter together.
fn dumped_but_unanswered() -> GeometrySpecimen {
    Specimen {
        id: "dumped-but-unanswered".to_owned(),
        axis: UselessnessMode::DumpedButUnanswered,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Sleep answer".to_owned(),
                element_id: Some("status".to_owned()),
                artifact: Some("apps/cli/src/commands/execution/mod.rs".to_owned()),
            }],
            html: "<main id=\"home\"><a href=\"#status\">status</a>                   <section id=\"status\">running</section></main>"
                .to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "status".to_owned()]),
            journey: asserted_journey("Sleep answer"),
            tests: green_tests(),
            diff: behavior_diff(),
            interaction: Some(InteractionTrace {
                question: "can I go back to sleep?".to_owned(),
                budget_calls: 1,
                // Within budget by a mile, and the answer is nowhere in the 6 KB.
                calls: vec![call("events", 6_400, false)],
            }),
        },
    }
}

fn call(tool: &str, payload_bytes: u64, answered: bool) -> InteractionCall {
    InteractionCall {
        tool: tool.to_owned(),
        payload_bytes,
        answered,
    }
}

/// Canonical digest of a suite: sha256 over the canonical JSON of the specimen list,
/// rendered in the `WireHash` wire shape (`sha256:<64 hex>`).
#[must_use]
pub fn suite_digest<E: Serialize, A: Serialize>(suite: &[Specimen<E, A>]) -> String {
    let canonical =
        serde_json::to_string(suite).expect("specimens are plain data and always serialize");
    format!("sha256:{}", hex::encode(Sha256::digest(canonical)))
}

/// Run a candidate gate against every specimen; refuse certification on ANY pass.
///
/// # Errors
/// [`CertificationRefusal`] naming every specimen the gate passed.
pub fn certify<E, A, G>(
    gate: &G,
    suite: &[Specimen<E, A>],
) -> Result<Certification, CertificationRefusal>
where
    G: EvidenceGate<E> + ?Sized,
    E: Serialize,
    A: Serialize,
{
    // A suite with no specimens cannot fool a gate, so `fooled_by` comes back empty and this
    // function used to fall through to `Ok` -- certifying a gate against nothing at all. The
    // count was recorded as `specimens: 0` and nothing refused.
    //
    // Enforced HERE rather than at each call site, and that is the point: two test files had
    // already hand-written this check (`jpd_gates.rs`, `retry_lineage_gates.rs`), both of them
    // mine. Finding one hole twice and patching it locally both times is how the third caller
    // inherits it -- the one who never knew to look.
    if suite.is_empty() {
        return Err(CertificationRefusal {
            gate_id: gate.id().to_owned(),
            fooled_by: Vec::new(),
            cause: RefusalCause::EmptySuite,
        });
    }
    let fooled_by: Vec<String> = suite
        .iter()
        .filter(|specimen| gate.evaluate(&specimen.evidence).passed)
        .map(|specimen| specimen.id.clone())
        .collect();
    if fooled_by.is_empty() {
        Ok(Certification {
            gate_id: gate.id().to_owned(),
            suite_digest: suite_digest(suite),
            specimens: u32::try_from(suite.len()).expect("suites are small"),
        })
    } else {
        Err(CertificationRefusal {
            gate_id: gate.id().to_owned(),
            fooled_by,
            cause: RefusalCause::GatePassedSpecimens,
        })
    }
}

/// Whether a recorded certification still binds against the current suite. Growing the
/// suite changes the digest, so old immunity dies by comparison, never by cleanup.
#[must_use]
pub fn certification_is_current<E: Serialize, A: Serialize>(
    recorded_digest: &str,
    current_suite: &[Specimen<E, A>],
) -> bool {
    recorded_digest == suite_digest(current_suite)
}

/// The correctness battery: what a naive CI would check — tests exist and pass, the
/// diff touched something, the spec lists claims. Every specimen passes it, which is
/// exactly why the battery itself can never be certified as a gate.
#[must_use]
pub fn correctness_battery() -> Box<dyn CandidateGate> {
    Box::new(FnGate {
        id: "correctness-battery",
        check: |d: &Deliverable| {
            !d.tests.is_empty()
                && d.tests.iter().all(|test| test.passed)
                && d.diff.files_touched >= 1
                && !d.claims.is_empty()
        },
        finding: "a correctness measure failed",
    })
}

/// The plausible-but-blind gate each specimen was bred to fool: it checks everything a
/// hurried reviewer would, except the one axis its pathogen is useless on.
#[must_use]
pub fn paired_trivial_gate(mode: UselessnessMode) -> Box<dyn CandidateGate> {
    match mode {
        // The plausible gate for an expensive answer is a call COUNTER — and it is fooled
        // by the mirror, which spends one call and answers nothing. The plausible gate for
        // the mirror is "did one call answer it" — fooled by six calls that do answer.
        UselessnessMode::ExpensiveButCorrect => Box::new(FnGate {
            id: "the-answer-is-present",
            check: |d| {
                d.interaction
                    .as_ref()
                    .is_none_or(|trace| trace.calls.iter().any(|call| call.answered))
            },
            finding: "no call carried the answer",
        }),
        UselessnessMode::DumpedButUnanswered => Box::new(FnGate {
            id: "calls-within-budget",
            check: |d| {
                d.interaction
                    .as_ref()
                    .is_none_or(|trace| trace.calls.len() as u64 <= trace.budget_calls)
            },
            finding: "the question cost more calls than its budget",
        }),
        UselessnessMode::DeadFeature => Box::new(FnGate {
            id: "claims-have-artifacts",
            check: |d| !d.claims.is_empty() && d.claims.iter().all(|c| c.artifact.is_some()),
            finding: "a claim has no backing artifact",
        }),
        UselessnessMode::UnreachableUi => Box::new(FnGate {
            id: "elements-present-in-html",
            check: |d| {
                !d.claims.is_empty()
                    && d.claims.iter().all(|c| {
                        c.element_id
                            .as_ref()
                            .is_none_or(|id| d.html.contains(&format!("id=\"{id}\"")))
                    })
            },
            finding: "a claimed element is missing from the page",
        }),
        UselessnessMode::TautologicalJourney => Box::new(FnGate {
            id: "journey-has-assertions",
            check: |d| {
                !d.journey.is_empty() && d.journey.iter().all(|step| step.assertion.is_some())
            },
            finding: "a journey step asserts nothing",
        }),
        UselessnessMode::BlankScreen => Box::new(FnGate {
            id: "tests-all-green",
            check: |d| !d.tests.is_empty() && d.tests.iter().all(|test| test.passed),
            finding: "a test failed",
        }),
        UselessnessMode::OrphanView => Box::new(FnGate {
            id: "view-renders-content",
            check: |d| d.html.contains("<section") && d.html.contains("</section>"),
            finding: "no view renders",
        }),
        UselessnessMode::GuttedAssertion => Box::new(FnGate {
            id: "tests-exist-and-pass",
            check: |d| !d.tests.is_empty() && d.tests.iter().all(|test| test.passed),
            finding: "tests missing or failing",
        }),
        UselessnessMode::HappyPathOnly => Box::new(FnGate {
            id: "journey-completes-with-assertions",
            check: |d| {
                !d.journey.is_empty() && d.journey.iter().all(|step| step.assertion.is_some())
            },
            finding: "the journey does not complete",
        }),
        UselessnessMode::SpecClaimWithoutArtifact => Box::new(FnGate {
            id: "spec-lists-claims",
            check: |d| !d.claims.is_empty(),
            finding: "the spec claims nothing",
        }),
        UselessnessMode::MinimalDiffNoBehavior => Box::new(FnGate {
            id: "diff-touches-files",
            check: |d| d.diff.files_touched >= 1,
            finding: "the diff touches nothing",
        }),
        UselessnessMode::LabelSwappedUi => Box::new(FnGate {
            id: "all-labels-present",
            check: |d| !d.claims.is_empty() && d.claims.iter().all(|c| d.html.contains(&c.feature)),
            finding: "a claimed label is missing from the page",
        }),
    }
}

/// A gate that rejects everything — the minimal certifiable subject, and the proof that
/// certification is necessary, never sufficient.
#[must_use]
pub fn reject_everything_gate() -> Box<dyn CandidateGate> {
    Box::new(FnGate {
        id: "reject-everything",
        check: |_| false,
        finding: "rejected by construction",
    })
}

struct FnGate {
    id: &'static str,
    check: fn(&Deliverable) -> bool,
    finding: &'static str,
}

impl CandidateGate for FnGate {
    fn id(&self) -> &str {
        self.id
    }

    fn evaluate(&self, deliverable: &Deliverable) -> Verdict {
        if (self.check)(deliverable) {
            Verdict::pass()
        } else {
            Verdict::refuse(self.finding)
        }
    }
}

/// An axis is not a label. It is a claim that some evidence defeats a gate in a particular
/// way, and the axis is the only thing that can check that claim.
///
/// Generic because the check must generalise WITH the harness. Before this trait existed the
/// integrity check was typed to geometry, so making the harness generic silently dropped it for
/// every other evidence type — and **nothing went red**: `certify` still worked, `fooled_by`
/// still named what slipped, the suite still certified, and no one checked the specimens
/// defeated anything. A specimen that defeats nothing certifies a gate that caught nothing.
/// (Found by N in cross-review of #211.)
///
/// A new axis for a new evidence type cannot be added without answering "what does defeated
/// mean here", because this trait will not let it.
pub trait FailureAxis<E> {
    /// Whether this specimen's evidence is genuinely defeated on the axis it names.
    fn is_defeated_by(&self, evidence: &E) -> bool;
}

impl FailureAxis<Deliverable> for UselessnessMode {
    fn is_defeated_by(&self, d: &Deliverable) -> bool {
        match self {
            // Useless on the interaction axis: the answer arrives, but the asking is the cost.
            UselessnessMode::ExpensiveButCorrect => d.interaction.as_ref().is_some_and(|trace| {
                trace.calls.len() as u64 > trace.budget_calls
                    && trace.calls.iter().any(|call| call.answered)
            }),
            // Useless the mirrored way: within budget, and the answer is nowhere in it.
            UselessnessMode::DumpedButUnanswered => d.interaction.as_ref().is_some_and(|trace| {
                trace.calls.len() as u64 <= trace.budget_calls
                    && !trace.calls.iter().any(|call| call.answered)
            }),
            UselessnessMode::DeadFeature => d.claims.iter().any(|c| {
                c.artifact.is_some()
                    && c.element_id
                        .as_ref()
                        .is_some_and(|id| !d.html.contains(&format!("id=\"{id}\"")))
            }),
            UselessnessMode::UnreachableUi => d.claims.iter().any(|c| {
                c.element_id.as_ref().is_some_and(|id| {
                    d.html.contains(&format!("id=\"{id}\"")) && !d.reachable_ids.contains(id)
                })
            }),
            UselessnessMode::TautologicalJourney => {
                !d.journey.is_empty()
                    && d.journey.iter().all(|step| {
                        step.assertion.as_ref().is_some_and(|assertion| {
                            !d.claims.iter().any(|c| assertion.contains(&c.feature))
                        })
                    })
            }
            UselessnessMode::BlankScreen => d.html.is_empty(),
            UselessnessMode::OrphanView => d
                .html
                .split("<section id=\"")
                .skip(1)
                .filter_map(|rest| rest.split('"').next())
                .any(|view_id| {
                    !d.reachable_ids.contains(view_id)
                        && !d.html.contains(&format!("href=\"#{view_id}\""))
                }),
            UselessnessMode::GuttedAssertion => d
                .tests
                .iter()
                .any(|test| test.passed && test.assertions == 0),
            UselessnessMode::HappyPathOnly => {
                !d.journey.is_empty() && d.journey.iter().all(|step| !step.exercises_error_path)
            }
            UselessnessMode::SpecClaimWithoutArtifact => {
                d.claims.iter().any(|c| c.artifact.is_none())
            }
            UselessnessMode::MinimalDiffNoBehavior => {
                d.diff.files_touched >= 1 && d.diff.behavior_lines == 0
            }
            UselessnessMode::LabelSwappedUi => d.claims.iter().any(|a| {
                d.claims.iter().any(|b| {
                    a.feature != b.feature
                        && a.element_id.as_ref().is_some_and(|id| {
                            d.html.contains(&format!("id=\"{id}\">{}", b.feature))
                        })
                })
            }),
        }
    }
}

/// Whether a specimen is genuinely defeated on the axis it claims — fixture integrity, not
/// gate behaviour. A suite of specimens that defeat nothing certifies gates for catching
/// nothing, and the digest cannot see the difference: it notices CHANGE, never QUALITY.
#[must_use]
pub fn is_defeated_on_its_axis<E, A: FailureAxis<E>>(specimen: &Specimen<E, A>) -> bool {
    specimen.axis.is_defeated_by(&specimen.evidence)
}

fn green_tests() -> Vec<TestCase> {
    vec![TestCase {
        name: "feature_works".to_owned(),
        passed: true,
        assertions: 3,
    }]
}

fn behavior_diff() -> DiffSummary {
    DiffSummary {
        files_touched: 4,
        behavior_lines: 120,
    }
}

fn asserted_journey(feature: &str) -> Vec<JourneyStep> {
    vec![
        JourneyStep {
            action: format!("open {feature}"),
            assertion: Some(format!("{feature} panel is visible")),
            exercises_error_path: false,
        },
        JourneyStep {
            action: format!("submit {feature} with a bad input"),
            assertion: Some(format!("{feature} refuses with a message")),
            exercises_error_path: true,
        },
    ]
}

fn dead_feature() -> GeometrySpecimen {
    Specimen {
        id: "dead-feature".to_owned(),
        axis: UselessnessMode::DeadFeature,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><p>Welcome</p></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned()]),
            journey: asserted_journey("Export"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn unreachable_ui() -> GeometrySpecimen {
    Specimen {
        id: "unreachable-ui".to_owned(),
        axis: UselessnessMode::UnreachableUi,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned()]),
            journey: asserted_journey("Export"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn tautological_journey() -> GeometrySpecimen {
    Specimen {
        id: "tautological-journey".to_owned(),
        axis: UselessnessMode::TautologicalJourney,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "btn-export".to_owned()]),
            journey: vec![
                JourneyStep {
                    action: "load the page".to_owned(),
                    assertion: Some("the page is the page".to_owned()),
                    exercises_error_path: false,
                },
                JourneyStep {
                    action: "wait".to_owned(),
                    assertion: Some("time passed".to_owned()),
                    exercises_error_path: true,
                },
            ],
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn blank_screen() -> GeometrySpecimen {
    Specimen {
        id: "blank-screen".to_owned(),
        axis: UselessnessMode::BlankScreen,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Dashboard".to_owned(),
                element_id: Some("view-dashboard".to_owned()),
                artifact: Some("src/dashboard.rs".to_owned()),
            }],
            html: String::new(),
            reachable_ids: BTreeSet::new(),
            journey: asserted_journey("Dashboard"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn orphan_view() -> GeometrySpecimen {
    Specimen {
        id: "orphan-view".to_owned(),
        axis: UselessnessMode::OrphanView,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Report".to_owned(),
                element_id: Some("view-report".to_owned()),
                artifact: Some("src/report.rs".to_owned()),
            }],
            html: "<main id=\"home\"><p>Welcome</p></main>\
                   <section id=\"view-report\"><h2>Report</h2></section>"
                .to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned()]),
            journey: asserted_journey("Report"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn gutted_assertion() -> GeometrySpecimen {
    Specimen {
        id: "gutted-assertion".to_owned(),
        axis: UselessnessMode::GuttedAssertion,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "btn-export".to_owned()]),
            journey: asserted_journey("Export"),
            tests: vec![TestCase {
                name: "exports_csv".to_owned(),
                passed: true,
                assertions: 0,
            }],
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn happy_path_only() -> GeometrySpecimen {
    Specimen {
        id: "happy-path-only".to_owned(),
        axis: UselessnessMode::HappyPathOnly,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "btn-export".to_owned()]),
            journey: vec![JourneyStep {
                action: "click Export".to_owned(),
                assertion: Some("Export downloads a file".to_owned()),
                exercises_error_path: false,
            }],
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn spec_claim_without_artifact() -> GeometrySpecimen {
    Specimen {
        id: "spec-claim-without-artifact".to_owned(),
        axis: UselessnessMode::SpecClaimWithoutArtifact,
        evidence: Deliverable {
            claims: vec![
                Claim {
                    feature: "Export".to_owned(),
                    element_id: Some("btn-export".to_owned()),
                    artifact: Some("src/export.rs".to_owned()),
                },
                Claim {
                    feature: "Import".to_owned(),
                    element_id: None,
                    artifact: None,
                },
            ],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "btn-export".to_owned()]),
            journey: asserted_journey("Export"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn minimal_diff_no_behavior() -> GeometrySpecimen {
    Specimen {
        id: "minimal-diff-no-behavior".to_owned(),
        axis: UselessnessMode::MinimalDiffNoBehavior,
        evidence: Deliverable {
            claims: vec![Claim {
                feature: "Export".to_owned(),
                element_id: Some("btn-export".to_owned()),
                artifact: Some("src/export.rs".to_owned()),
            }],
            html: "<main id=\"home\"><button id=\"btn-export\">Export</button></main>".to_owned(),
            reachable_ids: BTreeSet::from(["home".to_owned(), "btn-export".to_owned()]),
            journey: asserted_journey("Export"),
            tests: green_tests(),
            diff: DiffSummary {
                files_touched: 3,
                behavior_lines: 0,
            },
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}

fn label_swapped_ui() -> GeometrySpecimen {
    Specimen {
        id: "label-swapped-ui".to_owned(),
        axis: UselessnessMode::LabelSwappedUi,
        evidence: Deliverable {
            claims: vec![
                Claim {
                    feature: "Export".to_owned(),
                    element_id: Some("btn-export".to_owned()),
                    artifact: Some("src/export.rs".to_owned()),
                },
                Claim {
                    feature: "Delete".to_owned(),
                    element_id: Some("btn-delete".to_owned()),
                    artifact: Some("src/delete.rs".to_owned()),
                },
            ],
            html: "<main id=\"home\">\
                   <button id=\"btn-export\">Delete</button>\
                   <button id=\"btn-delete\">Export</button>\
                   </main>"
                .to_owned(),
            reachable_ids: BTreeSet::from([
                "home".to_owned(),
                "btn-export".to_owned(),
                "btn-delete".to_owned(),
            ]),
            journey: asserted_journey("Export"),
            tests: green_tests(),
            diff: behavior_diff(),
            // No claim about interaction: these ten are about what a screen shows.
            interaction: None,
        },
    }
}
