//! Paired token-efficiency benchmark harness (task-009, #225).
//!
//! The harness refuses more than it reports. A benchmark that can only produce a number produces
//! one when it should not, and that number is the artifact everyone downstream keeps.

use sha2::{Digest, Sha256};

/// One corpus case, addressed by ID.
///
/// The oracle is referenced by ID and never carried here: a case that contains its own answer is a
/// case the compiled arm can read.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub id: String,
    pub oracle_id: String,
}

/// A frozen benchmark manifest.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub manifest_version: u32,
    pub corpus_digest: String,
    pub cases: Vec<Case>,
}

/// Why the harness will not proceed.
///
/// Every variant carries what an operator needs in order to act. A benchmark refusal that does not
/// say what disagreed sends them to bisect a benchmark run, which is the most expensive bisection
/// in this repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BenchmarkRefusal {
    /// The manifest is not readable as one.
    Unreadable { detail: String },
    /// The corpus does not hash to the digest frozen into the manifest.
    CorpusDigestMismatch {
        declared: String,
        actual: String,
        cases: usize,
    },
    /// The two arms were not given the same thing, so there is nothing to compare.
    AsymmetricRun { fields: Vec<String> },
    /// A cost the runtime cannot see. NOT zero.
    CostUnavailable { field: String, arm: String },
    /// One or more cases did not retrieve their required evidence.
    ///
    /// A COUNT, never a term in an average. This is the one refusal whose absence would let the
    /// harness publish an arithmetically correct number about the wrong population.
    RequiredEvidenceMissing { cases: Vec<String> },
    /// A run with no cases. Not full recall, and not a score.
    EmptyRun,
    /// A path this check cannot reason about, so it will not vouch for it.
    ///
    /// Deliberately NOT `OracleReachable`. A path carrying `..` may or may not reach the oracle,
    /// and the remedies differ: one says remove the entry that reads the answers, the other says
    /// write the path without traversal. Folding them would send some operators hunting a leak
    /// that is not there -- the flattening this lane filed as #247.
    PathNotComparable { arm: String, path: String },
    /// An arm could reach the oracle it is graded against.
    ///
    /// Names the path AS GIVEN rather than the oracle, because the given entry is what has to be
    /// removed. Telling an operator that the oracle is reachable without saying through what leaves
    /// them to find the entry themselves.
    OracleReachable { arm: String, path: String },
    /// The baseline was OBSERVED to be zero, so there is nothing to be a fraction of.
    ///
    /// Deliberately NOT the same variant as `CostUnavailable`: that one says the number could not
    /// be seen, this one says it was seen and was zero. The remedies point in opposite directions
    /// -- go and find the counter, versus go and look at why the baseline did no work.
    BaselineZero { field: String },
}

/// What one arm was given.
///
/// The issue fixes ten items; this carries ELEVEN fields, because `model route/settings` names two
/// things that can differ independently -- a run can hold the route and move the temperature.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmDeclaration {
    pub snapshot: String,
    pub objective: String,
    pub permissions: Vec<String>,
    pub model_route: String,
    pub model_settings: String,
    pub clean_state: bool,
    /// The corpus order this arm ran. A sequence, not a set: cache warmth, budget exhaustion and
    /// early stop all depend on what came first.
    pub order: Vec<String>,
    pub seed: u64,
    pub clock: String,
    pub budget: u64,
    pub acceptance_contract: String,
}

/// Refuse unless both arms were given the same thing.
///
/// Reports EVERY field that differs, not the first. A comparer that stops at the first difference
/// costs one full benchmark run per field -- fix the seed, rerun, discover the clock, fix the
/// clock, rerun -- and a benchmark run is the most expensive unit in this repository to pay that
/// in. The cost of a refusal that under-reports is iterations, not information, which is the
/// defect this lane filed as #247.
///
/// The list is SORTED, so that two runs differing in the same way produce the same message. An
/// order that followed the struct layout would make the message an accident of declaration order.
pub fn compare_arms(
    baseline: &ArmDeclaration,
    compiled: &ArmDeclaration,
) -> Result<(), BenchmarkRefusal> {
    let mut fields: Vec<String> = Vec::new();
    let mut check = |name: &str, same: bool| {
        if !same {
            fields.push(name.to_owned());
        }
    };

    check("snapshot", baseline.snapshot == compiled.snapshot);
    check("objective", baseline.objective == compiled.objective);
    check("permissions", baseline.permissions == compiled.permissions);
    check("modelRoute", baseline.model_route == compiled.model_route);
    check(
        "modelSettings",
        baseline.model_settings == compiled.model_settings,
    );
    check("cleanState", baseline.clean_state == compiled.clean_state);
    check("order", baseline.order == compiled.order);
    check("seed", baseline.seed == compiled.seed);
    check("clock", baseline.clock == compiled.clock);
    check("budget", baseline.budget == compiled.budget);
    check(
        "acceptanceContract",
        baseline.acceptance_contract == compiled.acceptance_contract,
    );

    if fields.is_empty() {
        return Ok(());
    }
    fields.sort();
    Err(BenchmarkRefusal::AsymmetricRun { fields })
}

/// The digest that freezes a corpus.
///
/// Taken over the CANONICAL form, so that reformatting the manifest does not read as tampering.
/// Note what this does and does not cover: `canonical_json` sorts object KEYS and leaves ARRAYS
/// alone, so reordering the cases DOES change this digest. That is deliberate -- the corpus is a
/// sequence, and `order` is one of the items a paired run fixes and `compare_arms` checks.
#[must_use]
pub fn corpus_digest(cases: &[serde_json::Value]) -> String {
    let canonical = graphhelm_protocols::canonical_json(&serde_json::Value::Array(cases.to_vec()));
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Read a manifest, and refuse one whose corpus no longer matches the digest frozen into it.
///
/// The check is the whole point of the type. Parsing a manifest is not the service this offers --
/// `serde_json` already does that. What it offers is that a corpus edited after the freeze cannot
/// be read at all, so no number can be computed from it.
pub fn load_manifest(text: &str) -> Result<Manifest, BenchmarkRefusal> {
    let manifest =
        serde_json::from_str::<Manifest>(text).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: error.to_string(),
        })?;

    let cases: Vec<serde_json::Value> = manifest
        .cases
        .iter()
        .map(|case| serde_json::to_value(case).unwrap_or(serde_json::Value::Null))
        .collect();
    let actual = corpus_digest(&cases);

    if actual != manifest.corpus_digest {
        return Err(BenchmarkRefusal::CorpusDigestMismatch {
            declared: manifest.corpus_digest,
            actual,
            // The count is carried because it is the fastest signal an operator has for WHICH kind
            // of edit happened: a changed count means a case appeared or went missing, an unchanged
            // count means one was edited in place.
            cases: manifest.cases.len(),
        });
    }

    Ok(manifest)
}

/// A ratio between the two arms for one cost field, carrying how well it is known.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// compiled / baseline.
    pub ratio: f64,
    /// How observed the ratio is. A ratio is only as observed as its least observed term.
    pub provenance: graphhelm_runtime::context_accounting::CostProvenance,
}

/// One arm's value for a field, or a refusal naming that arm.
///
/// A field that is ABSENT is treated exactly like one that is `Unavailable`, and for the same
/// reason: in both cases the runtime cannot see the number. Reading either as zero is the whole
/// shape of shifting work outside the count.
fn arm_cost(
    field: &str,
    receipt: &graphhelm_runtime::context_accounting::AccountingReceipt,
    arm: &str,
) -> Result<(u64, graphhelm_runtime::context_accounting::CostProvenance), BenchmarkRefusal> {
    let unavailable = || BenchmarkRefusal::CostUnavailable {
        field: field.to_owned(),
        arm: arm.to_owned(),
    };

    let cost = receipt.field(field).ok_or_else(unavailable)?;
    let value = cost.observed().ok_or_else(unavailable)?;
    Ok((value, cost.provenance().clone()))
}

/// Compare one cost field across the two arms.
///
/// Refuses rather than compares when either arm cannot see the number, and carries the WEAKER
/// provenance of the two into the result: a ratio is only as observed as its least observed term.
///
/// The three provenances stay three. Refusing `Derived` would throw away a usable comparison;
/// reporting it as `Measured` would launder a computed number into an observed one. Collapsing
/// three facts into two is the defect this lane filed as #247, and #222 already declined to make
/// it -- this declines to undo it.
pub fn compare_cost(
    field: &str,
    baseline: &graphhelm_runtime::context_accounting::AccountingReceipt,
    compiled: &graphhelm_runtime::context_accounting::AccountingReceipt,
) -> Result<Comparison, BenchmarkRefusal> {
    use graphhelm_runtime::context_accounting::CostProvenance;

    let (baseline_value, baseline_provenance) = arm_cost(field, baseline, "baseline")?;

    // The denominator ONLY. A compiled arm that spent nothing is the best possible result and must
    // compare normally; a baseline that spent nothing leaves nothing to be a fraction of. The naive
    // version did not merely produce an infinity here -- it produced one labelled `Measured`,
    // which is a non-number carrying a claim that somebody observed it.
    if baseline_value == 0 {
        return Err(BenchmarkRefusal::BaselineZero {
            field: field.to_owned(),
        });
    }
    let (compiled_value, compiled_provenance) = arm_cost(field, compiled, "compiled")?;

    let provenance = if baseline_provenance == CostProvenance::Measured
        && compiled_provenance == CostProvenance::Measured
    {
        CostProvenance::Measured
    } else {
        CostProvenance::Derived
    };

    #[allow(clippy::cast_precision_loss)]
    Ok(Comparison {
        ratio: compiled_value as f64 / baseline_value as f64,
        provenance,
    })
}

/// What one case did.
#[derive(Clone, Debug, PartialEq)]
pub struct CaseOutcome {
    pub case_id: String,
    /// Whether the run retrieved the evidence the case requires. A COUNT input, not a score input.
    pub required_evidence_found: bool,
    /// Blind quality, in [0, 1].
    pub quality: f64,
}

/// What a run produced, once it earned the right to produce anything.
#[derive(Clone, Debug, PartialEq)]
pub struct RunVerdict {
    pub mean_quality: f64,
}

/// Evaluate a run.
///
/// THE ORDER IS THE GUARD. Required-evidence recall is a COUNT and it is settled before a mean
/// exists to be settled against. A run that misses one case does not produce a lower score -- it
/// produces no score, because there is nothing for a score to be about.
///
/// This is the only one of the threat assessment vectors where the published number is
/// ARITHMETICALLY CORRECT. The others produce a wrong number; averaging away a critical failure
/// produces a right number about the wrong population. The red that preceded this was
/// `mean_quality: 0.9022`, ABOVE the 0.90 the passing cases scored, because the case that missed
/// its evidence scored 0.99 -- the miss made the run look better.
pub fn evaluate_run(outcomes: &[CaseOutcome]) -> Result<RunVerdict, BenchmarkRefusal> {
    // Zero cases is not full recall. An empty corpus satisfies "every case found its evidence"
    // vacuously and means nothing at all, and the mean of nothing is a NaN that prints as a value.
    // Both readings flatter, so the empty run is checked rather than left to fall through.
    if outcomes.is_empty() {
        return Err(BenchmarkRefusal::EmptyRun);
    }

    let mut missing: Vec<String> = outcomes
        .iter()
        .filter(|outcome| !outcome.required_evidence_found)
        .map(|outcome| outcome.case_id.clone())
        .collect();

    if !missing.is_empty() {
        // Every missed case at once, sorted: one name per run costs a benchmark run per case, and
        // an unsorted list makes two runs that missed the same cases print differently.
        missing.sort();
        return Err(BenchmarkRefusal::RequiredEvidenceMissing { cases: missing });
    }

    #[allow(clippy::cast_precision_loss)]
    let mean = outcomes.iter().map(|outcome| outcome.quality).sum::<f64>() / outcomes.len() as f64;
    Ok(RunVerdict { mean_quality: mean })
}

/// What one arm was handed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmInputs {
    /// Files and directories the arm may read.
    pub paths: Vec<String>,
}

/// The one form a path must already be in before this check will compare it.
///
/// Separators are unified and a trailing slash trimmed, because both are noise: a manifest written
/// on one platform is read on another, and `dir/` and `dir` are the same directory.
///
/// CASE IS FOLDED, AND IT IS A TRADE RATHER THAN A FREE WIN. On Windows `ORACLE.JSON` and
/// `oracle.json` are the same file, so a case-sensitive comparison would miss a real leak on the
/// platform this most often runs on. On a case-SENSITIVE filesystem the opposite holds:
/// `Fixtures/notes` and `fixtures/notes` are DIFFERENT directories, and folding makes this check
/// call a leak on an innocent sibling.
///
/// Taken deliberately, in the direction that can only be wrong one way. A false refusal costs a
/// rename and stops a run that would have been fine; a missed leak lets an arm read the answers,
/// and every number produced afterwards is a measurement of nothing. **Only the second is silent.**
///
/// The residue is real and named rather than hidden: on a case-sensitive filesystem, a directory
/// whose name differs from the oracle's only in case is refused when it should not be. Closing that
/// would mean knowing which filesystem the manifest describes, which this crate does not read --
/// and guessing it is the kind of interpretation `comparable` exists to refuse.
///
/// Nothing else is interpreted. See `comparable`.
fn normalise(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Whether a normalised path can be compared to the oracle path at all.
///
/// THE `..` FINDING WAS ONE MOUTH OF A CLASS. A probe over this check found six live ones --
/// leading slash, drive letter, `.` segment, `..` segment, doubled separator, and case -- and
/// five of the six are the same shape: a path that means something a text comparison cannot see.
///
/// So this does not special-case `..`. It refuses to reason about any path that is not already in
/// the comparable form, which is the same instinct as the rest of this harness: refuse rather than
/// interpret. Resolving them instead would mean carrying a second, weaker implementation of the
/// filesystem inside a benchmark harness, and being wrong there is silent.
///
/// Segments rather than substrings, following `core/runtime/src/retrieval.rs`, whose comment gives
/// the reason: a substring test for `..` also rejects the ordinary `src/..foo.rs`, "and a rule that
/// fires on innocent input gets relaxed by the next person who hits it". A security check usually
/// dies by being annoying rather than by being argued with.
fn comparable(normalised: &str) -> bool {
    if normalised.starts_with('/') {
        return false;
    }
    let mut characters = normalised.chars();
    if matches!(
        (characters.next(), characters.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    ) {
        return false;
    }
    !normalised
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// Refuse if anything the arm was handed reaches the oracle.
///
/// Containment is checked at a COMPONENT BOUNDARY, not as a string prefix. Written as a bare
/// `starts_with`, `fixtures/bench` would read as containing `fixtures/benchmark/...` because it is
/// a prefix as text -- a false refusal that looks like rigour and makes the harness unusable.
pub fn check_oracle_isolation(
    oracle: &str,
    arm: &ArmInputs,
    arm_name: &str,
) -> Result<(), BenchmarkRefusal> {
    let oracle_path = normalise(oracle);

    for path in &arm.paths {
        let given = normalise(path);

        // BEFORE any comparison. A path this cannot reason about gets a refusal that says so,
        // rather than one claiming it reaches the oracle: a `..` path may or may not reach it, and
        // the two remedies point in opposite directions.
        if !comparable(&given) {
            return Err(BenchmarkRefusal::PathNotComparable {
                arm: arm_name.to_owned(),
                // AS GIVEN, not normalised: this is the entry the operator has to find and rewrite.
                path: path.clone(),
            });
        }

        // Handed the file itself, or handed a directory it sits under. Both reach it, and an
        // exact-match check sees only the first -- while the second is how the leak actually
        // arrives, as convenience rather than intent.
        let reaches = given == oracle_path || oracle_path.starts_with(&format!("{given}/"));
        if reaches {
            return Err(BenchmarkRefusal::OracleReachable {
                arm: arm_name.to_owned(),
                path: path.clone(),
            });
        }
    }
    Ok(())
}

/// The thresholds the shipped evaluator declares.
#[derive(Clone, Debug, PartialEq)]
pub struct EfficiencyPolicy {
    pub version: String,
    pub max_median_input_ratio: f64,
    pub max_median_session_ratio: f64,
    pub max_mean_quality_drop: f64,
}

/// The medians a paired run produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Medians {
    pub input_ratio: f64,
    pub session_ratio: f64,
    pub mean_quality_drop: f64,
}

/// A judgement, and the rules that made it.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub passed: bool,
    /// The evaluator version in force. Travels WITH the verdict so a published result names the
    /// rules it was judged under.
    pub policy_version: String,
    /// Every threshold broken, sorted.
    pub broken: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyFile {
    version: String,
    thresholds: PolicyThresholds,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyThresholds {
    max_median_input_ratio: f64,
    max_median_session_ratio: f64,
    max_mean_quality_drop: f64,
}

/// Read the evaluator.
pub fn load_policy(text: &str) -> Result<EfficiencyPolicy, BenchmarkRefusal> {
    let parsed: PolicyFile =
        serde_yaml_ng::from_str(text).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: error.to_string(),
        })?;
    Ok(EfficiencyPolicy {
        version: parsed.version,
        max_median_input_ratio: parsed.thresholds.max_median_input_ratio,
        max_median_session_ratio: parsed.thresholds.max_median_session_ratio,
        max_mean_quality_drop: parsed.thresholds.max_mean_quality_drop,
    })
}

/// Judge a run against the thresholds the POLICY declares.
///
/// Reads every threshold from the policy rather than knowing any of them. A judge that hard-coded
/// the shipped values would agree with the shipped policy on every run and diverge silently the
/// moment the policy changed -- and a cell that only checked agreement with the shipped values
/// could not tell the two apart.
///
/// Reports EVERY broken threshold, sorted, for the same economics as the asymmetry refusal: one
/// per run means one benchmark run per threshold.
///
/// The three thresholds stay three and are never folded into a score. Folding lets a large win on
/// one buy a loss on another, which is the averaging failure this harness exists to refuse.
#[must_use]
pub fn judge(policy: &EfficiencyPolicy, run: &Medians) -> Verdict {
    let mut broken = Vec::new();
    if run.input_ratio > policy.max_median_input_ratio {
        broken.push("maxMedianInputRatio".to_owned());
    }
    if run.session_ratio > policy.max_median_session_ratio {
        broken.push("maxMedianSessionRatio".to_owned());
    }
    if run.mean_quality_drop > policy.max_mean_quality_drop {
        broken.push("maxMeanQualityDrop".to_owned());
    }
    broken.sort();

    Verdict {
        passed: broken.is_empty(),
        // The version travels WITH the verdict. Without it, loosening a threshold and republishing
        // the same headline is indistinguishable from improving the work.
        policy_version: policy.version.clone(),
        broken,
    }
}
