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
// deny_unknown_fields is K's #504 finding 2, measured with a probe: an injected field was
// ACCEPTED with the digest byte-identical, because serde dropped it and the digest is taken over
// a re-serialisation of the parsed struct -- unmodelled content invisible twice.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub oracle_id: String,
}

/// A frozen benchmark manifest.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub corpus_digest: String,
    /// Raw-bytes freeze of `oracle/<id>.json` per case (K's #504 finding 1): Bar 2 judges recall
    /// against `requiredEvidence`, so an oracle that nothing digests can be softened without
    /// moving the freeze -- and the blueprint predicts the oracle weakens exactly when retrieval
    /// is optimised.
    pub oracle_digest: String,
    /// Same freeze for what the arms are HANDED: an objective edited after the freeze changes
    /// what the run measures just as silently.
    pub objectives_digest: String,
    /// The fourth freeze (#225): `retrieval/<id>.json` per case, raw bytes. The compiled arm's
    /// context is BUILT from these artifacts, so a regeneration after the freeze is a treatment
    /// change with no trace in the other three digests -- the exact silence the first three
    /// exist to close, one input further downstream.
    pub retrieval_digest: String,
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
    /// A frozen file digest does not match the files on disk.
    ///
    /// Separate from `CorpusDigestMismatch` because the remedies differ: that one says the
    /// manifest's CASE LIST moved, this one says the case list is intact and the CONTENT it
    /// points at moved -- an oracle softened, an objective rewritten.
    FrozenFilesMismatch {
        kind: String,
        declared: String,
        actual: String,
    },
    /// The baseline was OBSERVED to be zero, so there is nothing to be a fraction of.
    ///
    /// Deliberately NOT the same variant as `CostUnavailable`: that one says the number could not
    /// be seen, this one says it was seen and was zero. The remedies point in opposite directions
    /// -- go and find the counter, versus go and look at why the baseline did no work.
    BaselineZero { field: String },
    /// The shipped plan discipline refused a frozen retrieval artifact.
    ///
    /// Carries the WIRE code (`index_stale`, `scope_mismatch`, ...) rather than a paraphrase, so
    /// the operator lands on the same vocabulary `compile_plan` speaks everywhere else.
    RetrievalRefused { code: String, case: String },
}

/// What one arm was given.
///
/// The issue fixes ten items; this carries FIFTEEN fields: `model route/settings` names two
/// things that can differ independently, the blueprint splits "snapshots" into repository and
/// index (index freshness is not repository freshness -- #219's INDEX_STALE), and it adds three
/// axes the issue's list never held: build identity (B10), environment record (B11), and cache
/// discipline (B3).
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmDeclaration {
    pub snapshot: String,
    /// The INDEX snapshot, split from the repository snapshot on purpose (blueprint SS3, K's
    /// finding 3 on #504): index freshness is not repository freshness -- #219's INDEX_STALE is a
    /// distinct failure -- and a run where only one arm has an index is not a pairing.
    pub index_snapshot: String,
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
    /// Digest of the ONE build both arms execute (#225 blueprint B10, F's finding). A rebuild
    /// between arms that stayed deterministic passes the receipt-equality check while leaving a
    /// difference between arms explainable by a code change rather than by the treatment.
    pub binary_digest: String,
    /// The environment record the binary ran under (B11). Separate from the digest ON PURPOSE:
    /// a digest fixes the code, not what the code READS -- the same artifact under a different
    /// RUST_LOG, locale, or target dir is a different treatment with an identical digest.
    pub environment: String,
    /// The cache warmth protocol this arm followed (B3). Held EQUAL like every other axis: a warm
    /// compiled arm against a cold baseline is the single cheapest way to manufacture a good
    /// ratio, and it leaves no trace in a mean. Cache STORES must still not cross arms -- that is
    /// a receipt-level property (#222), not this field's.
    pub cache_discipline: String,
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
    check(
        "indexSnapshot",
        baseline.index_snapshot == compiled.index_snapshot,
    );
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
    check(
        "binaryDigest",
        baseline.binary_digest == compiled.binary_digest,
    );
    check("environment", baseline.environment == compiled.environment);
    check(
        "cacheDiscipline",
        baseline.cache_discipline == compiled.cache_discipline,
    );

    if fields.is_empty() {
        return Ok(());
    }
    fields.sort();
    Err(BenchmarkRefusal::AsymmetricRun { fields })
}

/// Digest the FILES a corpus points at, raw bytes bound to case ids (K's #504 finding 1).
///
/// Per case, in manifest order: `sha256(file bytes)` bound to the case id; then one digest over
/// the ordered id:hash lines. Raw bytes rather than canonical JSON, per the SS7b decision -- these
/// files exist to be REPLAYED, and a canonical digest is deliberately blind to bytes that differ.
/// `kind` is the directory the corpus keeps them in ("oracle" or "objectives"); each case's file
/// is `<kind>/<case id>.json`.
///
/// A case whose file cannot be read refuses naming it: a digest over the readable subset would be
/// a digest of a DIFFERENT corpus.
pub fn frozen_files_digest(
    root: &std::path::Path,
    kind: &str,
    cases: &[Case],
) -> Result<String, BenchmarkRefusal> {
    let mut lines = String::new();
    for case in cases {
        let path = root.join(kind).join(format!("{}.json", case.id));
        // Bounded BEFORE the read that allocates it. This is the SECOND time this bound has gone
        // missing from a commit of mine: the first attempt's patch did not match and I did not
        // check, then reported it landed. Codex found the absence both times by reading the final
        // verifier. 4 MiB is three orders of magnitude above the shipped files.
        const FROZEN_FILE_MAX_BYTES: u64 = 4 * 1024 * 1024;
        let declared = std::fs::metadata(&path)
            .map_err(|error| BenchmarkRefusal::Unreadable {
                detail: format!("{kind}/{}.json: {error}", case.id),
            })?
            .len();
        if declared > FROZEN_FILE_MAX_BYTES {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!(
                    "{kind}/{}.json is {declared} bytes, beyond the {FROZEN_FILE_MAX_BYTES}-byte \
                     bound",
                    case.id
                ),
            });
        }
        let bytes = std::fs::read(&path).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("{kind}/{}.json: {error}", case.id),
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        // The id is bound BESIDE the hash, so the same bytes under another case cannot collide
        // with the original: which bytes belong to which case is part of the freeze.
        lines.push_str(&format!(
            "{}:{}
",
            case.id,
            hex::encode(hasher.finalize())
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(lines.as_bytes());
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Verify a manifest's frozen file digests against the files on disk.
///
/// Split from `load_manifest` because the loader owns TEXT and this owns the FILESYSTEM: callers
/// that only need the corpus identity (most tests) keep a pure function, and the runner -- which
/// is about to hand these files to two arms -- calls this beside it. Refuses with the declared
/// and actual digests, same shape as the corpus check, so an operator sees WHICH freeze moved.
pub fn verify_frozen_files(
    manifest: &Manifest,
    root: &std::path::Path,
) -> Result<(), BenchmarkRefusal> {
    for (kind, declared) in [
        ("oracle", &manifest.oracle_digest),
        ("objectives", &manifest.objectives_digest),
        ("retrieval", &manifest.retrieval_digest),
    ] {
        let actual = frozen_files_digest(root, kind, &manifest.cases)?;
        if &actual != declared {
            return Err(BenchmarkRefusal::FrozenFilesMismatch {
                kind: kind.to_owned(),
                declared: declared.clone(),
                actual,
            });
        }
    }

    // THE GAP BETWEEN THE DIGESTS. Each of the three binds its own directory and nothing binds
    // them to EACH OTHER: edit an objective, refresh its digest, leave retrieval alone, and all
    // three checks pass while the artifact's `query` still describes the old objective. The drive
    // then sends the NEW objective to the model and compiles hits that were selected for the old
    // one (Codex P2). Three intact freezes, one incoherent corpus.
    //
    // The producer's own mechanical rule is what makes this checkable: the query IS the objective,
    // verbatim. So equality per case is not a new convention, it is the existing one enforced.
    for case in &manifest.cases {
        let read = |kind: &str| -> Result<serde_json::Value, BenchmarkRefusal> {
            let path = root.join(kind).join(format!("{}.json", case.id));
            let text =
                std::fs::read_to_string(&path).map_err(|error| BenchmarkRefusal::Unreadable {
                    detail: format!("{kind}/{}.json: {error}", case.id),
                })?;
            serde_json::from_str(&text).map_err(|error| BenchmarkRefusal::Unreadable {
                detail: format!("{kind}/{}.json: {error}", case.id),
            })
        };
        let objective = read("objectives")?;
        let artifact = read("retrieval")?;
        let stated =
            objective["objective"]
                .as_str()
                .ok_or_else(|| BenchmarkRefusal::Unreadable {
                    detail: format!("objectives/{}.json: no objective string", case.id),
                })?;
        let queried = artifact["query"]
            .as_str()
            .ok_or_else(|| BenchmarkRefusal::Unreadable {
                detail: format!("retrieval/{}.json: no query string", case.id),
            })?;
        if stated != queried {
            return Err(BenchmarkRefusal::FrozenFilesMismatch {
                kind: "retrieval".to_owned(),
                declared: format!("query selected for: {queried}"),
                actual: format!("objective now reads: {stated}"),
            });
        }
    }
    Ok(())
}

/// The one bar a run directory's receipts can carry today, with its anti-gaming tail.
#[derive(Clone, Debug, PartialEq)]
pub struct InputRatioReport {
    /// median(compiled) / median(baseline) -- a ratio of MEDIANS (blueprint SS2), never a median
    /// of per-case ratios: the derived quantity is defined over the two measured columns.
    pub input_ratio: f64,
    pub cases_measured: usize,
    /// The worst per-case ratio, published BESIDE the median (Bar 1's tail rule): a design that
    /// halves fifty easy cases and doubles five hard ones passes a median bar comfortably.
    pub worst_case_ratio: f64,
    pub cases_where_compiled_exceeded_baseline: usize,
}

/// Read a paired driver's run directory against the frozen case list.
///
/// `arms.json` carries the two `ArmDeclaration`s and is compared BEFORE any number; each
/// `cases/<id>.json` carries one `CostField` per arm under the receipt vocabulary
/// (`provider_reported_input_tokens`), so unavailability arrives with provenance and is refused
/// through the same defence #222 built -- never read as zero. A missing case is a partial run
/// (a DIFFERENT corpus); an extra case is a receipt nothing froze; both refuse by name.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArmsFile {
    baseline: ArmDeclaration,
    compiled: ArmDeclaration,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunCaseFile {
    case_id: String,
    baseline: RunCaseArm,
    compiled: RunCaseArm,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RunCaseArm {
    /// Snake on the wire ON PURPOSE: this key is the receipt vocabulary, verbatim.
    provider_reported_input_tokens: graphhelm_runtime::context_accounting::CostField,
    /// Whether this arm's inputs contained every oracle-required path. Bar 2 consumes it: a
    /// recorded miss refuses the whole comparison before any number exists.
    #[serde(rename = "requiredEvidenceFound")]
    required_evidence_found: bool,
}

const PROVIDER_INPUT_FIELD: &str = "provider_reported_input_tokens";

pub fn read_run_directory(
    root: &std::path::Path,
    cases: &[Case],
) -> Result<InputRatioReport, BenchmarkRefusal> {
    let arms_text = std::fs::read_to_string(root.join("arms.json")).map_err(|error| {
        BenchmarkRefusal::Unreadable {
            detail: format!("arms.json: {error}"),
        }
    })?;
    let arms: RunArmsFile =
        serde_json::from_str(&arms_text).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("arms.json: {error}"),
        })?;
    // BEFORE any number, same as the in-memory path: a run whose arms differ has nothing to
    // compare, and reading its receipts first would put a ratio in scrollback anyway.
    compare_arms(&arms.baseline, &arms.compiled)?;

    // The stranger check first: receipts for a case nothing froze. A directory listing is the
    // only way to SEE the extra file, so the sweep is explicit rather than lookup-driven.
    let frozen: std::collections::BTreeSet<&str> =
        cases.iter().map(|case| case.id.as_str()).collect();
    let entries =
        std::fs::read_dir(root.join("cases")).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("cases/: {error}"),
        })?;
    for entry in entries {
        let entry = entry.map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("cases/: {error}"),
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name.strip_suffix(".json").unwrap_or(&name);
        if !frozen.contains(id) {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!("cases/{name}: receipt for a case the manifest never froze"),
            });
        }
    }

    let mut baseline_values: Vec<u64> = Vec::new();
    let mut compiled_values: Vec<u64> = Vec::new();
    let mut worst_case_ratio: f64 = 0.0;
    let mut exceeded = 0usize;
    // Bar 2 lands HERE, before any number exists to be settled against (the same order
    // evaluate_run enforces for quality): a recorded miss on either arm refuses the whole
    // comparison. Without this, a paid live run could publish a ratio about a blind arm --
    // Codex's #531 P1, and precisely the promotion the recall bar exists to refuse.
    let mut blind_cases: Vec<String> = Vec::new();
    for case in cases {
        let path = root.join("cases").join(format!("{}.json", case.id));
        let text = std::fs::read_to_string(&path).map_err(|error| {
            // A missing case is a PARTIAL RUN -- a different corpus, never a lower N.
            BenchmarkRefusal::Unreadable {
                detail: format!("cases/{}.json: {error}", case.id),
            }
        })?;
        let record: RunCaseFile =
            serde_json::from_str(&text).map_err(|error| BenchmarkRefusal::Unreadable {
                detail: format!("cases/{}.json: {error}", case.id),
            })?;
        if record.case_id != case.id {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!(
                    "cases/{}.json: carries caseId `{}`",
                    case.id, record.case_id
                ),
            });
        }
        // Rebuild real receipts and let compare_cost decide: provenance, unavailability and the
        // zero-baseline rule all live there, and a second copy here would drift.
        let baseline_receipt = graphhelm_runtime::context_accounting::AccountingReceipt::new()
            .with_field(
                PROVIDER_INPUT_FIELD,
                record.baseline.provider_reported_input_tokens,
            )
            .map_err(|_| BenchmarkRefusal::Unreadable {
                detail: format!("cases/{}.json: unbuildable baseline receipt", case.id),
            })?;
        let compiled_receipt = graphhelm_runtime::context_accounting::AccountingReceipt::new()
            .with_field(
                PROVIDER_INPUT_FIELD,
                record.compiled.provider_reported_input_tokens,
            )
            .map_err(|_| BenchmarkRefusal::Unreadable {
                detail: format!("cases/{}.json: unbuildable compiled receipt", case.id),
            })?;
        if !record.baseline.required_evidence_found || !record.compiled.required_evidence_found {
            blind_cases.push(case.id.clone());
            continue;
        }
        let comparison = compare_cost(PROVIDER_INPUT_FIELD, &baseline_receipt, &compiled_receipt)?;
        if comparison.ratio > worst_case_ratio {
            worst_case_ratio = comparison.ratio;
        }
        if comparison.ratio > 1.0 {
            exceeded += 1;
        }
        baseline_values.push(comparison.baseline);
        compiled_values.push(comparison.compiled);
    }
    if !blind_cases.is_empty() {
        return Err(BenchmarkRefusal::RequiredEvidenceMissing { cases: blind_cases });
    }
    if baseline_values.is_empty() {
        return Err(BenchmarkRefusal::EmptyRun);
    }

    // Ratio of MEDIANS (blueprint SS2): the derived quantity is defined over the two measured
    // columns, and a median of per-case ratios is a different number the fixture separates.
    let median = |values: &mut Vec<u64>| -> u64 {
        values.sort_unstable();
        values[values.len() / 2]
    };
    let baseline_median = median(&mut baseline_values);
    let compiled_median = median(&mut compiled_values);
    if baseline_median == 0 {
        return Err(BenchmarkRefusal::BaselineZero {
            field: PROVIDER_INPUT_FIELD.to_owned(),
        });
    }
    #[allow(clippy::cast_precision_loss)]
    Ok(InputRatioReport {
        input_ratio: compiled_median as f64 / baseline_median as f64,
        cases_measured: cases.len(),
        worst_case_ratio,
        cases_where_compiled_exceeded_baseline: exceeded,
    })
}

/// One frozen retrieval, exactly as the index answered it (#506 selector design).
///
/// Flat on purpose: this file is committed and digest-frozen beside the corpus, so its shape is
/// wire, not convenience. `repo_snapshot`/`index_generation` are the `SnapshotBinding` halves;
/// the four bounds are `DeclaredLimits`; `coverage` is the closed `CoverageState` vocabulary.
/// Rebuilding the runtime types at read time keeps the SHIPPED plan discipline the only judge.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalArtifact {
    pub case_id: String,
    /// The one query, the case objective verbatim -- the mechanical rule that squeezes the
    /// harness's discretion out of the selection.
    pub query: String,
    pub repo_snapshot: String,
    pub index_generation: String,
    pub hits: Vec<RetrievalHit>,
    pub coverage: String,
    pub pages: u32,
    pub max_results: u32,
    pub max_pages: u32,
    pub max_bytes: u64,
    pub max_tokens: u32,
}

/// One hit as the index returned it: a path, and optionally the 1-based line range of the
/// construct. The range is the index's own grain -- rows are FUNCTIONS -- and the real-corpus dry
/// run measured what aggregating to whole files does: ratio 3.9 against naive, recall 0/12.
/// `lines: None` reads the whole file, kept for hits that genuinely are whole-file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalHit {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<String>,
}

/// The crate's own sha256 hex, exposed so cells can compute an expected digest the way an
/// operator would (the arming-site guards need a CORRECT digest to isolate the check under test).
#[must_use]
pub fn sha256_hex_of(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Decode a provider page's `cols` and `rows` BY SHAPE, refusing anything that does not fit.
///
/// The version this replaces used `filter_map` on both, and that is a repair rather than a read:
/// a non-string column name was removed while the row arrays kept their length, shifting the
/// computed `file` index onto a different cell — a label or a rank could freeze as a repository
/// path. A non-array row was dropped outright, silently shrinking the hit set recall is measured
/// from. Both turn a malformed page into a plausible one, and the result is frozen (Codex P2 pair
/// on #579).
///
/// # Errors
/// [`BenchmarkRefusal::Unreadable`] naming which part of the page does not fit.
pub fn page_of(
    structured: &serde_json::Value,
    case_id: &str,
) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>), BenchmarkRefusal> {
    let refuse = |what: &str| BenchmarkRefusal::Unreadable {
        detail: format!("case {case_id}: the provider page {what}"),
    };
    let cols_value = structured
        .get("cols")
        .ok_or_else(|| refuse("has no cols"))?;
    let cols_array = cols_value
        .as_array()
        .ok_or_else(|| refuse("has a cols that is not an array"))?;
    let mut cols = Vec::with_capacity(cols_array.len());
    for entry in cols_array {
        cols.push(
            entry
                .as_str()
                .ok_or_else(|| refuse("has a column name that is not a string"))?
                .to_owned(),
        );
    }
    // Duplicate names make the mapping AMBIGUOUS, and `position` resolves ambiguity by silently
    // preferring the first -- so `cols: ["file", "file"]` freezes whichever string happened to
    // come first as a repository path (Codex P2). A page that names one column twice has not said
    // which column it means, and guessing is what every other refusal here exists to prevent.
    let mut unique = std::collections::BTreeSet::new();
    for name in &cols {
        if !unique.insert(name.as_str()) {
            return Err(refuse(&format!("names the column {name:?} more than once")));
        }
    }
    // `total` is the page's own account of its result set, and the production decoder requires
    // it. Accepting a page without one — or one claiming FEWER results than it returned — let the
    // generator freeze rows the provider never coherently described (Codex P2). Once again the
    // generator was more permissive than the decoder it feeds.
    let total = structured
        .get("total")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| refuse("states no numeric total"))?;
    // GROUPED pages carry their rows inside `groups`, with no top-level `rows` at all. I fixed
    // this encoding in the producer and left the generator refusing it outright — the same defect,
    // in the second of the two places that read provider pages, found because the reviewer checked
    // the OTHER reader after I reported the first one fixed.
    if structured.get("rows").is_none()
        && let Some(groups) = structured
            .get("groups")
            .and_then(serde_json::Value::as_array)
    {
        let mut rows = Vec::new();
        for group in groups {
            let file = group
                .get("file")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| refuse("has a group with no file"))?;
            let group_rows = group
                .get("rows")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| refuse("has a group with no rows array"))?;
            for entry in group_rows {
                let mut row = entry
                    .as_array()
                    .ok_or_else(|| refuse("has a group row that is not an array"))?
                    .clone();
                // The flattened `file` column is appended at index `cols.len()`, so a row WIDER
                // than cols would leave its extra cell sitting at that index and the mapping
                // would read the wrong value as the path (Codex P1). A row that does not match
                // the declared width is a page whose shape nobody stated.
                if row.len() != cols.len() {
                    return Err(refuse(&format!(
                        "has a group row of {} cells against {} declared columns",
                        row.len(),
                        cols.len()
                    )));
                }
                // The group header carries the path once; flattening puts it back on every row so
                // the by-NAME column mapping downstream sees the same shape either encoding takes.
                row.push(serde_json::Value::String(file.to_owned()));
                rows.push(row);
            }
        }
        // The early return skipped the total reconciliation the flat path performs, so a grouped
        // page could declare fewer results than it carried and freeze them anyway (Codex P1).
        if (rows.len() as u64) > total {
            return Err(refuse(&format!(
                "returned {} grouped rows while declaring a total of {total}",
                rows.len()
            )));
        }
        let mut flattened = cols;
        flattened.push("file".to_owned());
        return Ok((flattened, rows));
    }
    let rows_value = structured
        .get("rows")
        .ok_or_else(|| refuse("has no rows"))?;
    let rows_array = rows_value
        .as_array()
        .ok_or_else(|| refuse("has a rows that is not an array"))?;
    let mut rows = Vec::with_capacity(rows_array.len());
    for entry in rows_array {
        rows.push(
            entry
                .as_array()
                .ok_or_else(|| refuse("has a row that is not an array"))?
                .clone(),
        );
    }
    if (rows.len() as u64) > total {
        return Err(refuse(&format!(
            "returned {} rows while declaring a total of {total}",
            rows.len()
        )));
    }
    Ok((cols, rows))
}

/// Read a page's `has_more` as the provider STATED it, refusing when it did not.
///
/// A default here is not a convenience, it is a fabricated negative with a permanent
/// consequence: these artifacts freeze under a digest and become the corpus every future run is
/// measured against, so a page silently read as exhausted becomes `coverage: "complete"` and
/// that overclaim can no longer be revised. "The provider did not say" and "the provider said
/// no" are different facts and only one of them may freeze.
///
/// # Errors
/// [`BenchmarkRefusal::Unreadable`] naming the field when it is absent or not a boolean.
pub fn has_more_of(
    structured: &serde_json::Value,
    case_id: &str,
) -> Result<bool, BenchmarkRefusal> {
    structured
        .get("has_more")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| BenchmarkRefusal::Unreadable {
            detail: format!(
                "case {case_id}: the provider page does not state has_more as a boolean, and a \
                 page read as exhausted would freeze that overclaim into the corpus"
            ),
        })
}

/// Build one frozen retrieval artifact from a `search_graph` structured answer (#225 wiring).
///
/// The provider's measured wire shape (codebase-memory-mcp 0.10.8, `format:"json"`): a `cols`
/// name list and per-row value lists. The mapping is mechanical -- `file` and `lines` columns by
/// NAME, never by position -- and a page without a `file` column refuses rather than guessing,
/// because a fallback that grabs "the last string cell" fabricates paths the moment the provider
/// reorders its columns. Duplicate (path, lines) pairs collapse to their first appearance; rows
/// beyond the first `limits.max_results` are never consulted.
pub fn artifact_from_search_rows(
    case_id: &str,
    query: &str,
    snapshot: &str,
    cols: &[String],
    rows: &[Vec<serde_json::Value>],
    limits: &graphhelm_protocols::DeclaredLimits,
) -> Result<RetrievalArtifact, BenchmarkRefusal> {
    let file_column = cols.iter().position(|name| name == "file").ok_or_else(|| {
        BenchmarkRefusal::Unreadable {
            detail: format!(
                "case {case_id}: the provider page has no `file` column (cols: {cols:?})"
            ),
        }
    })?;
    let lines_column = cols.iter().position(|name| name == "lines");

    let mut seen: std::collections::BTreeSet<(String, Option<String>)> =
        std::collections::BTreeSet::new();
    let mut hits: Vec<RetrievalHit> = Vec::new();
    for row in rows.iter().take(limits.max_results as usize) {
        let path = row
            .get(file_column)
            .and_then(|cell| cell.as_str())
            .ok_or_else(|| BenchmarkRefusal::Unreadable {
                detail: format!("case {case_id}: a row's `file` cell is not a string"),
            })?
            .to_owned();
        // An absent column and an explicit null are legitimate whole-file hits; a PRESENT cell
        // that is a number or an object is provider schema drift, and mapping it to `None` would
        // silently widen the hit to the entire file -- inflating the token measurement and
        // changing the evidence rather than refusing (Codex P2).
        // A SHORT ROW is not an absent column. `row.get(index)` returns `None` both when the page
        // declares no `lines` column and when it declares one but a row ends before it, and my
        // previous fix merged the two: the malformed case widened the hit to the WHOLE FILE,
        // exactly like the legitimate one. Codex found it by reading the final code after I had
        // reported this fixed — the second time in one review that a repair of mine covered the
        // case I was looking at and not the case beside it.
        let cell = match lines_column {
            None => None,
            Some(index) => Some(row.get(index).ok_or_else(|| BenchmarkRefusal::Unreadable {
                detail: format!(
                    "case {case_id}: the page declares a `lines` column but a row is too short to \
                     carry one"
                ),
            })?),
        };
        let lines = match cell {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(value)) => Some(value.clone()),
            Some(other) => {
                return Err(BenchmarkRefusal::Unreadable {
                    detail: format!(
                        "case {case_id}: a row's `lines` cell is present but not a string ({other})"
                    ),
                });
            }
        };
        if seen.insert((path.clone(), lines.clone())) {
            hits.push(RetrievalHit { path, lines });
        }
    }

    Ok(RetrievalArtifact {
        case_id: case_id.to_owned(),
        query: query.to_owned(),
        repo_snapshot: snapshot.to_owned(),
        index_generation: snapshot.to_owned(),
        hits,
        // ALWAYS partial -- the same rule the production adapter
        // enforces at `provider.rs:156-160`. `has_more == false` means the PAGE ended, never
        // that the SEARCH was exhaustive, and a best-effort provider cannot support a
        // completeness claim at all. Freezing `complete` here manufactured one: for a zero-result
        // query it reaches `compile_plan` as VERIFIED ABSENCE -- a fabricated "the repository
        // does not contain this", frozen under a digest so every future run measures against it
        // (Codex P2).
        //
        // `has_more` no longer reaches this function at all, because a parameter that decides
        // nothing is theatre. It stays REQUIRED at the generator (`has_more_of`) for a different
        // reason, stated there: a provider that will not describe its own page is a provider
        // whose answer should not be frozen.
        coverage: "partial".to_owned(),
        pages: 1,
        max_results: limits.max_results,
        max_pages: limits.max_pages,
        max_bytes: limits.max_bytes,
        max_tokens: limits.max_tokens,
    })
}

/// A compiled arm's context, plus the paths that went into it (recall is judged against these).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledContext {
    pub capsule: Vec<u8>,
    pub evidence_paths: Vec<String>,
}

/// Compile one case's capsule from its frozen retrieval artifact.
///
/// The artifact passes through `compile_plan_within` -- the SHIPPED discipline: a stale binding
/// refuses, an escaping hit refuses, over-budget refuses rather than truncates -- and only the
/// surviving hits are read. The section decision is made HERE, explicitly (`task` for the
/// objective, `evidence` for the hits), which is what the #404 seal said content wiring needs.
pub fn capsule_for_case(
    objective: &str,
    artifact: &RetrievalArtifact,
    repo_root: &std::path::Path,
) -> Result<CompiledContext, BenchmarkRefusal> {
    // Rebuilt through serde so the runtime types' own vocabulary is the parser -- a hand-rolled
    // match over coverage strings would be a second copy of a closed set.
    let binding: graphhelm_protocols::SnapshotBinding = serde_json::from_value(serde_json::json!({
        "repoSnapshot": artifact.repo_snapshot,
        "indexGeneration": artifact.index_generation,
    }))
    .map_err(|error| BenchmarkRefusal::Unreadable {
        detail: format!("artifact {}: binding: {error}", artifact.case_id),
    })?;
    let coverage: graphhelm_protocols::CoverageState = serde_json::from_value(
        serde_json::Value::String(artifact.coverage.clone()),
    )
    .map_err(|error| BenchmarkRefusal::Unreadable {
        detail: format!("artifact {}: coverage: {error}", artifact.case_id),
    })?;
    let hit_paths: Vec<String> = artifact.hits.iter().map(|hit| hit.path.clone()).collect();
    let response = graphhelm_runtime::retrieval::IndexResponse::new(hit_paths, coverage)
        .after_pages(artifact.pages);
    let limits = graphhelm_protocols::DeclaredLimits {
        max_results: artifact.max_results,
        max_pages: artifact.max_pages,
        max_bytes: artifact.max_bytes,
        max_tokens: artifact.max_tokens,
    };
    let hits = match graphhelm_runtime::retrieval::compile_plan_within(&binding, &response, &limits)
    {
        graphhelm_runtime::retrieval::RetrievalOutcome::Claim { hits, .. } => hits,
        graphhelm_runtime::retrieval::RetrievalOutcome::VerifiedAbsence => {
            // For a benchmark case a verified nothing is still nothing to compile against.
            return Err(BenchmarkRefusal::RetrievalRefused {
                code: "verified_absence".to_owned(),
                case: artifact.case_id.clone(),
            });
        }
        graphhelm_runtime::retrieval::RetrievalOutcome::Refused { code } => {
            return Err(BenchmarkRefusal::RetrievalRefused {
                code: code.wire_name().to_owned(),
                case: artifact.case_id.clone(),
            });
        }
    };

    // The PLAN gates (stale, escape, bounds) over the canonicalised path SET; the ITEMS are then
    // built from the artifact's own declarations, one per hit -- a by-path lookup here was Codex's
    // #531 P1: five ranges of one file collapsed into the first range five times, biasing the
    // fn-grain dry run downward on every case with duplicate paths. `hits` (the gated set) decides
    // WHICH paths may be read; `artifact.hits` decides WHAT of each is read.
    let gated: std::collections::BTreeSet<&str> = hits.iter().map(String::as_str).collect();
    let mut items: Vec<String> = Vec::with_capacity(artifact.hits.len());
    let mut contents: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    for declared in &artifact.hits {
        if !gated.contains(declared.path.as_str()) {
            // A declared hit the plan did not admit (canonicalisation collapsed it, or the plan
            // refused earlier and we never get here) -- skipping silently would under-measure.
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!(
                    "artifact {}: {}: declared hit not admitted by the plan",
                    artifact.case_id, declared.path
                ),
            });
        }
        if !contents.contains_key(declared.path.as_str()) {
            let content =
                std::fs::read_to_string(repo_root.join(&declared.path)).map_err(|error| {
                    BenchmarkRefusal::Unreadable {
                        detail: format!(
                            "artifact {}: {}: {error}",
                            artifact.case_id, declared.path
                        ),
                    }
                })?;
            contents.insert(declared.path.as_str(), content);
        }
        let content = &contents[declared.path.as_str()];
        let item = match &declared.lines {
            None => format!("{}\n{content}", declared.path),
            Some(range) => {
                let (start, end) = range
                    .split_once('-')
                    .and_then(|(start, end)| {
                        Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                    })
                    .filter(|(start, end)| *start >= 1 && start <= end)
                    .ok_or_else(|| BenchmarkRefusal::Unreadable {
                        detail: format!(
                            "artifact {}: {}: unparseable line range `{range}`",
                            artifact.case_id, declared.path
                        ),
                    })?;
                // A range past the end of the file is a lying artifact: refused by name, never a
                // silent short slice (Codex #531 P2 -- and the bound also forecloses the
                // arithmetic edge the checked parse alone would leave).
                let line_count = content.lines().count();
                if end > line_count {
                    return Err(BenchmarkRefusal::Unreadable {
                        detail: format!(
                            "artifact {}: {}: range `{range}` ends past the file's {line_count} \
                             lines",
                            artifact.case_id, declared.path
                        ),
                    });
                }
                let slice: Vec<&str> = content
                    .lines()
                    .skip(start - 1)
                    .take(end - start + 1)
                    .collect();
                format!("{}:{range}\n{}", declared.path, slice.join("\n"))
            }
        };
        items.push(item);
    }
    // The section decision, made explicitly: this is the half the #404 seal reserved for the
    // item's owner, and here the owner is the benchmark caller.
    let capsule = graphhelm_runtime::context_compiler::compile_capsule(
        &artifact.case_id,
        1,
        &[
            ("task".to_owned(), vec![objective.to_owned()]),
            ("evidence".to_owned(), items),
        ],
    );
    Ok(CompiledContext {
        capsule,
        evidence_paths: hits,
    })
}

/// The naive arm's context: the objective, then every evidence file in full, in order.
///
/// A declared LOWER bound on what a traditional agent reads -- the real one greps, opens
/// neighbours and retries. The conservative side of the ratio is the publishable side.
pub fn naive_context(
    objective: &str,
    evidence: &[String],
    repo_root: &std::path::Path,
) -> Result<String, BenchmarkRefusal> {
    let mut context = objective.to_owned();
    for path in evidence {
        let content = std::fs::read_to_string(repo_root.join(path)).map_err(|error| {
            BenchmarkRefusal::Unreadable {
                detail: format!("naive evidence {path}: {error}"),
            }
        })?;
        context.push_str("\n\n");
        context.push_str(path);
        context.push('\n');
        context.push_str(&content);
    }
    Ok(context)
}

/// Read an oracle file and return ONLY its evidence paths (plus the raw text the caller may hash
/// for provenance -- with the answer field stripped, so nothing downstream can carry it).
pub fn oracle_evidence_paths(
    oracle: &std::path::Path,
) -> Result<(Vec<String>, String), BenchmarkRefusal> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    // NOT deny_unknown_fields, deliberately: the oracle file carries the answer, and this reader
    // exists precisely so the answer never reaches the arms -- it deserializes only the two
    // fields below and drops the rest on the floor.
    struct OracleEvidence {
        oracle_id: String,
        required_evidence: Vec<String>,
    }
    let text = std::fs::read_to_string(oracle).map_err(|error| BenchmarkRefusal::Unreadable {
        detail: format!("{}: {error}", oracle.display()),
    })?;
    let parsed: OracleEvidence =
        serde_json::from_str(&text).map_err(|error| BenchmarkRefusal::Unreadable {
            detail: format!("{}: {error}", oracle.display()),
        })?;
    // The "raw" the caller may hash for provenance is REBUILT from what this function is allowed
    // to see, never the file text: returning the file text would hand the answer to whoever
    // concatenates carelessly.
    let stripped = format!(
        "{}\n{}",
        parsed.oracle_id,
        parsed.required_evidence.join("\n")
    );
    Ok((parsed.required_evidence, stripped))
}

/// Map a provider's reported input usage into a cost field, faithfully.
///
/// Present becomes MEASURED with the ROUTE as producer -- the label separating a live number from
/// a fake one. Absent becomes unavailable: never zero, never an estimate (#222's rule at the arm).
#[must_use]
pub fn usage_to_cost(
    input_tokens: Option<u64>,
    route: &str,
) -> graphhelm_runtime::context_accounting::CostField {
    match input_tokens {
        Some(value) => graphhelm_runtime::context_accounting::CostField::measured(value, route),
        None => graphhelm_runtime::context_accounting::CostField::unavailable(
            "the provider reported no input usage for this call",
        ),
    }
}

/// One live arm call: prompt in, the provider's input count out, through the byok adapter.
///
/// A gateway error is a typed refusal naming the route -- the run does not limp on without the
/// case, because a partial run is a different corpus.
pub fn live_arm_cost(
    adapter: &graphhelm_model_gateway::byok::ByokAdapter<'_>,
    key: &graphhelm_events::SecretBytes,
    prompt: &str,
    max_tokens: u32,
    route: &str,
) -> Result<graphhelm_runtime::context_accounting::CostField, BenchmarkRefusal> {
    let call = graphhelm_gateway::call::ModelCall {
        prompt: prompt.to_owned(),
        max_tokens,
    };
    match adapter.call(key, &call) {
        Ok(reply) => Ok(usage_to_cost(reply.usage.input_tokens, route)),
        // The taxonomy's own Display is the diagnosis; the route name is the address. Nothing
        // here reads the provider's prose, and no credential is in scope to leak.
        Err(error) => Err(BenchmarkRefusal::Unreadable {
            detail: format!("route {route}: {error}"),
        }),
    }
}

/// The settings string a receipt records, DERIVED from what the call transmits per provider.
///
/// K's #531 hold: an asserted literal on a held-equal axis is a check that cannot fail -- both
/// arms copy the same constant and `compare_arms` compares the constant to itself. This function
/// is the single producer of the label, and its cells pin the per-provider truth: anthropic
/// binds `max_tokens` as a required top-level field; the openai adapter deliberately does not
/// forward it (`byok.rs`); nothing transmits a temperature anywhere, so the label says
/// `provider-default` instead of asserting a zero nobody sent.
#[must_use]
pub fn transmitted_settings(provider: &str, max_tokens: u32) -> String {
    match provider {
        // Anthropic: `max_tokens` is a required top-level field, so the cap genuinely binds.
        "anthropic" => format!("max_tokens={max_tokens};temperature=provider-default"),
        // The openai adapter deliberately does not forward the cap (byok.rs documents it), so
        // recording it as bound would claim a control that was not executed.
        "openai" => "max_tokens=not-transmitted;temperature=provider-default".to_owned(),
        "fake" => "input=ceil(bytes/4)".to_owned(),
        other => format!("provider={other};controls=unknown"),
    }
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

    // The schema CHANGED when `retrievalDigest` became required, so the number changed with it: a
    // version-1 manifest is not a version-2 one wearing the same label, and the loader refuses to
    // guess which fields a corpus was written with (Codex P2). Enforced rather than documented —
    // a version field nothing checks is a comment.
    const MANIFEST_VERSION: u32 = 2;
    if manifest.manifest_version != MANIFEST_VERSION {
        return Err(BenchmarkRefusal::Unreadable {
            detail: format!(
                "manifest version {} is not {MANIFEST_VERSION}; the fourth freeze made \
                 retrievalDigest required, so the older schema cannot be read as this one",
                manifest.manifest_version
            ),
        });
    }

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

    // A case id BECOMES A PATH COMPONENT (`<kind>/<id>.json` in `frozen_files_digest`, and the
    // same join where the generator WRITES). The manifest is untrusted input and the corpus
    // digest is perfectly happy to bind a traversal: it hashes what is there, whatever that is.
    // So an id carrying a separator or a parent hop would read -- and write -- outside the
    // corpus directory (Codex P1 on #579).
    //
    // Rejected by SHAPE, never sanitised, the same posture the restore archive takes with blob
    // names (`apps/cli/src/commands/events/restore.rs`): an id that is not a plain file name is
    // not an id, and repairing it would be guessing at intent. Checked AFTER the digest so the
    // freeze still speaks first about a corpus that was edited, and here rather than at each
    // consumer so one guard covers digest, verify, generator and driver alike.
    for case in &manifest.cases {
        let id = case.id.as_str();
        if id.is_empty()
            || std::path::Path::new(id).file_name() != Some(std::ffi::OsStr::new(id))
            || id.contains(['/', '\\'])
        {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!("case id {id:?} is not a plain file name"),
            });
        }
        // A SECOND property on the same field, and the traversal guard above was MEASURED to miss
        // it: "where does this path go" is not "is this a filename at all". On Windows -- where
        // this repository is developed -- `CON`, `bad?name`, and a trailing dot or space all pass
        // the check above and then fail at the `.json` read the id feeds, or worse RESOLVE
        // through reserved-device semantics (Codex on #579, fresh evidence beyond the traversal
        // finding).
        const RESERVED: &[&str] = &[
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ];
        let stem = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
        let portable = id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
        if !portable || id.ends_with('.') || id.ends_with(' ') || RESERVED.contains(&stem.as_str())
        {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!("case id {id:?} is not a portable filename"),
            });
        }
    }

    // RESTORED. This guard existed and an amend of mine dropped it while I was fixing something
    // else, and I reported it as landed. Codex caught the absence by reading the FINAL loader
    // rather than trusting the reply — the control that reads what is THERE rather than what the
    // author expects to be there.
    //
    // A digest-valid manifest can still REPEAT an id: the digest binds the list as written, and a
    // list can say the same thing twice. Both artifacts then write to one filename, and the driver
    // reads that single receipt twice — biasing the medians and inflating `cases_measured` with a
    // case that ran once.
    let mut seen_ids = std::collections::BTreeSet::new();
    for case in &manifest.cases {
        if !seen_ids.insert(case.id.as_str()) {
            return Err(BenchmarkRefusal::Unreadable {
                detail: format!("case id {:?} appears more than once", case.id),
            });
        }
    }

    Ok(manifest)
}

/// How a counter column over the whole corpus reads (#225 blueprint B8).
///
/// Two renderings, and the word matters: `NotExercised` is not a zero. A zero says the cost was
/// paid and measured at nothing; not-exercised says the corpus never made the counter move, so
/// the column proves nothing about the cost. Publishing the second as the first is the flattering
/// misread the blueprint's zero bar exists to refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnReading {
    /// At least one case moved the counter; the column carries signal.
    Exercised {
        field: String,
        total: u64,
        nonzero_cases: usize,
    },
    /// Every case observed zero. The counter has no control in this corpus.
    NotExercised { field: String },
}

/// Render one counter column, refusing the empty case rather than defaulting it.
pub fn read_counter_column(field: &str, values: &[u64]) -> Result<ColumnReading, BenchmarkRefusal> {
    // No observations is not a quiet column -- it is a run with no cases, and that refusal
    // already has a name. An empty slice falling through would render NotExercised, which
    // flatters twice: it hides the missing run AND reads as a statement about the corpus.
    if values.is_empty() {
        return Err(BenchmarkRefusal::EmptyRun);
    }
    let nonzero_cases = values.iter().filter(|value| **value != 0).count();
    if nonzero_cases == 0 {
        return Ok(ColumnReading::NotExercised {
            field: field.to_owned(),
        });
    }
    Ok(ColumnReading::Exercised {
        field: field.to_owned(),
        total: values.iter().sum(),
        nonzero_cases,
    })
}

/// A ratio between the two arms for one cost field, carrying how well it is known.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// compiled / baseline.
    pub ratio: f64,
    /// The two observed terms, carried so a consumer can aggregate COLUMNS (a ratio of medians)
    /// without re-reading receipts -- a bare ratio cannot be re-aggregated honestly.
    pub baseline: u64,
    pub compiled: u64,
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
    Ok((value, *cost.provenance()))
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
        baseline: baseline_value,
        compiled: compiled_value,
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
/// **The one canonical form for comparing paths in this crate.**
///
/// It existed here, private, serving the oracle-isolation check alone — while two other sites
/// compared paths as RAW STRINGS: the working-tree cleanliness check and, far worse, the RECALL
/// comparison. So `./src/lib.rs` and `src/lib.rs` — one file, two spellings — read as different
/// paths, and recall is the gate on the paired run's spend (Codex P1 on #579).
///
/// This is the third finding in one review about comparing paths as if a path had one spelling.
/// The answer to a third instance is a shared form, not a third local patch — the same move the
/// symlink finding needed, where the seam already existed in a sibling module and I had not looked.
///
/// A leading `./` is stripped for the same reason a trailing slash is: it names the same file and
/// carries no information. Nothing else is interpreted — see `comparable`.
#[must_use]
pub fn canonical_path(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let stripped = slashed.strip_prefix("./").unwrap_or(&slashed);
    stripped.trim_end_matches('/').to_lowercase()
}

fn normalise(path: &str) -> String {
    canonical_path(path)
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
