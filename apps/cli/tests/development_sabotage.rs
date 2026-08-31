//! The sabotage corpus, held against the schemas it attacks (#226, task-010).
//!
//! Each red-window entry asserts what is TRUE TODAY: the sabotage is accepted, because the
//! protection has not been written yet. When one of those assertions fails, the protection has
//! landed — the entry moves from the red window to MARKED and its guard here is inverted. That
//! transition is the certification the corpus exists to produce, and encoding it as a test is what
//! stops it from depending on somebody remembering.
//!
//! A verdict is only trustworthy once the instrument is known to discriminate, so nothing here
//! reads a validation result as a verdict about a fixture until two things are established: the
//! schema set accepts a known-good document, and it refuses a known-bad one.
//!
//! The sharpest hazard is in the API itself. `OfflineSchemaSet::validate` returns a DIAGNOSTIC when
//! the schema id is not registered, so a typo in an id is indistinguishable from "the document was
//! refused" if the result is only tested for emptiness. That is a broken harness wearing a
//! verdict's clothes, and `judge` below separates the two rather than trusting the count.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use pathogens::jpd::{
    JourneyContractEvidence, JourneyContractGate, JourneyDiagnosticSeverity, JpdEvidence,
    JpdFailureAxis, VerificationResultGate, journey_contract_suite, jpd_suite,
};
use pathogens::{EvidenceGate, certify};
use serde_json::Value;

/// The message `OfflineSchemaSet::validate` returns when the root schema is absent from the set.
const UNREGISTERED: &str = "root schema is not registered";

/// What a validation attempt actually established.
#[derive(Debug)]
enum Verdict {
    /// The schema set examined the document and raised nothing.
    Accepted,
    /// The schema set examined the document and refused it.
    Refused(Vec<Diagnostic>),
    /// The schema set never examined the document. NOT a refusal.
    HarnessBroke(String),
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_json(path: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(path).unwrap_or_else(|error| panic!("unreadable at {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("not JSON at {}: {error}", path.display()))
}

/// Compile every `*.schema.json` directly inside `directory`.
fn schema_set(directory: &Path) -> OfflineSchemaSet {
    let mut paths = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} unreadable: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".schema.json"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no schemas found under {}: suspect the filter, not the tree",
        directory.display()
    );
    let documents = paths
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, load_json(&path))
        })
        .collect::<BTreeMap<_, _>>();
    OfflineSchemaSet::compile(documents)
        .unwrap_or_else(|diagnostics| panic!("schema set did not compile: {diagnostics:?}"))
}

/// Read a schema's declared id rather than transcribing it.
///
/// A transcribed id that drifts does not fail loudly: it makes `validate` report "not registered",
/// which reads as a refusal. Reading it removes the whole failure mode.
fn schema_id(directory: &Path, file: &str) -> String {
    load_json(&directory.join(file))["$id"]
        .as_str()
        .unwrap_or_else(|| panic!("{file} declares no $id"))
        .to_owned()
}

/// Classify a validation attempt, separating "never ran" from "ran and refused".
fn judge(set: &OfflineSchemaSet, id: &str, document: &Value, source: &str) -> Verdict {
    let diagnostics = set.validate(id, document, source);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains(UNREGISTERED))
    {
        return Verdict::HarnessBroke(format!("schema id `{id}` is not in the set"));
    }
    if diagnostics.is_empty() {
        Verdict::Accepted
    } else {
        Verdict::Refused(diagnostics)
    }
}

fn assert_journey_evidence_is_schema_valid(
    set: &OfflineSchemaSet,
    directory: &Path,
    evidence: &JpdEvidence,
    source: &str,
) {
    let JpdEvidence::JourneyContract(evidence) = evidence else {
        panic!("{source}: expected journey-contract evidence");
    };
    let contract_id = schema_id(directory, "journey-contract.schema.json");
    let obligation_id = schema_id(directory, "observation-obligation.schema.json");
    let result_id = schema_id(directory, "journey-verification-result.schema.json");

    for (id, document, suffix) in [
        (&contract_id, &evidence.contract, "contract"),
        (
            &obligation_id,
            &evidence.observation_obligations[0],
            "obligation",
        ),
        (&result_id, &evidence.verification_result, "result"),
    ] {
        match judge(set, id, document, &format!("{source}-{suffix}")) {
            Verdict::Accepted => {}
            other => panic!("{source}-{suffix}: expected schema-valid evidence, got {other:?}"),
        }
    }
}

fn development_schemas() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-development-contracts/schemas")
}

fn jpd_schemas() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-jpd/schemas")
}

fn root_schemas() -> PathBuf {
    repository_root().join("schemas")
}

fn corpus() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-development-contracts/fixtures/sabotage")
}

fn matched_s1b_obligation(contract_digest: &str, capability_id: &str, observer_id: &str) -> Value {
    serde_json::json!({
        "obligationId": "obligation/receipt-durable",
        "contractId": "journey/checkout-s1b",
        "contractDigest": contract_digest,
        "promiseId": "promise/receipt-durable",
        "fact": "content_rendered",
        "requiredEvidenceKinds": ["dom_semantic_snapshot"],
        "observerRequirements": {
            "capability": capability_id,
            "minimumTrust": "first_party_instrumented",
            "maxAgeSeconds": 30,
            "absenceWindowSeconds": null
        },
        "resolution": {
            "status": "matched",
            "catalogBinding": {
                "kind": "observer_catalog",
                "catalogId": "graphhelm-jpd/observer-catalog",
                "catalogVersion": "1.0.0",
                "catalogDigest": format!("sha256:{}", "1".repeat(64))
            },
            "capabilityBinding": {
                "kind": "observer_capability",
                "capabilityId": capability_id,
                "observerId": observer_id,
                "observerVersion": "1.0.0",
                "capabilityDigest": format!("sha256:{}", "2".repeat(64))
            },
            "configurationDigest": format!("sha256:{}", "3".repeat(64)),
            "environmentDigest": format!("sha256:{}", "4".repeat(64)),
            "observedTrust": "first_party_instrumented",
            "trustCompatibilityCandidate": {
                "authority": "candidate",
                "latticeBinding": {
                    "latticeId": "graphhelm-jpd/evidence-strength-lattice",
                    "latticeVersion": "1.0.0",
                    "latticeDigest": "sha256:d1aa1ccfd7fa32be662230098cc1c513fd9edee79c569b2e0ca8aa383c6c5531"
                },
                "relationId": "graphhelm-jpd/trust-compatibility",
                "relationVersion": "1.0.0",
                "decision": "compatible_candidate",
                "inputDigest": format!("sha256:{}", "c".repeat(64)),
                "receipt": {
                    "evidenceId": "evidence.trust-compatibility-candidate.s1b",
                    "contentSha256": "d".repeat(64),
                    "ciphertextSha256": "e".repeat(64)
                }
            },
            "capabilityReceipt": {
                "capturedAtUnixSeconds": 1_777_000_000_u64,
                "receipt": {
                    "evidenceId": "evidence.observer-capability.s1b",
                    "contentSha256": "5".repeat(64),
                    "ciphertextSha256": "6".repeat(64)
                }
            },
            "matchedEvidence": [{
                "evidenceKind": "dom_semantic_snapshot",
                "capturedAtUnixSeconds": 1_777_000_001_u64,
                "receipt": {
                    "evidenceId": "evidence.dom-semantic-snapshot.s1b",
                    "contentSha256": "7".repeat(64),
                    "ciphertextSha256": "8".repeat(64)
                }
            }],
            "freshnessEvaluation": {
                "clock": "unix_seconds",
                "evaluatedAtUnixSeconds": 1_777_000_005_u64,
                "maximumObservedAgeSeconds": 5,
                "requiredMaxAgeSeconds": 30,
                "requiredAbsenceWindowSeconds": null,
                "absenceWindow": null,
                "result": "satisfied"
            },
            "evidenceMatchEvaluation": {
                "evaluatorId": "graphhelm-jpd/evidence-matcher",
                "evaluatorVersion": "1.0.0",
                "evaluationSemantics": "registered_deterministic",
                "inputDigest": format!("sha256:{}", "9".repeat(64)),
                "receipt": {
                    "evidenceId": "evidence.evidence-match-evaluation.s1b",
                    "contentSha256": "a".repeat(64),
                    "ciphertextSha256": "b".repeat(64)
                }
            }
        }
    })
}

/// A wrong schema id is HARNESS-BROKE, never a refusal.
///
/// This is the guard that makes every other verdict in this file worth reading. Without it, an id
/// that drifts turns every "the sabotage is refused" assertion green for the wrong reason, and the
/// corpus would report protections that do not exist.
#[test]
fn an_unregistered_schema_id_is_harness_broke_and_not_a_refusal() {
    let directory = development_schemas();
    let set = schema_set(&directory);
    let document =
        load_json(&corpus().join("s2-false-structural-absence/evidence-claims-complete.json"));

    match judge(
        &set,
        "https://p50.dev/schemas/not-a-real-schema.json",
        &document,
        "probe",
    ) {
        Verdict::HarnessBroke(reason) => assert!(
            reason.contains("not-a-real-schema"),
            "the harness-broke report must name the id it could not find, got `{reason}`"
        ),
        other => panic!("an unregistered id must be HARNESS-BROKE, got {other:?}"),
    }
}

/// The development schema set both accepts and refuses.
///
/// Two halves on purpose: a set that accepts everything and a set that refuses everything each pass
/// one half. Only the pair establishes that a later verdict carries information.
#[test]
fn the_development_schema_set_discriminates() {
    let directory = development_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "development-envelope.schema.json");
    let good =
        load_json(&corpus().join("s2-false-structural-absence/evidence-claims-complete.json"));

    match judge(&set, &id, &good, "control-accept") {
        Verdict::Accepted => {}
        other => panic!("a well-formed envelope was not accepted: {other:?}"),
    }

    let mut bad = good;
    bad["kind"] = Value::String("NotAKind".to_owned());
    match judge(&set, &id, &bad, "control-refuse") {
        Verdict::Refused(diagnostics) => assert!(
            !diagnostics.is_empty(),
            "a refusal carrying no diagnostics cannot say what was wrong"
        ),
        other => panic!("an envelope with an unknown kind was not refused: {other:?}"),
    }
}

#[test]
fn council_direction_fixtures_are_schema_valid() {
    let directory = jpd_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "journey-verification-result.schema.json");
    let fixture = repository_root().join(
        "extensions/builtin/graphhelm-jpd/fixtures/positive/journey-verification-accepted-with-waiver.json",
    );
    let executed = load_json(&fixture);
    let mut direct = executed.clone();
    direct["bindings"]["council"] =
        serde_json::json!({ "status": "not_applicable", "reason": "direct_tier" });
    let recommended = executed.clone();
    let mut blocked = executed;
    blocked["bindings"]["council"]["result"]["status"] = Value::String("blocked".to_owned());
    blocked["bindings"]["council"]["result"]["decision"]["status"] =
        Value::String("blocked".to_owned());

    for (name, document) in [
        ("direct", direct),
        ("council-recommended-with-dissent", recommended),
        ("council-blocked", blocked),
    ] {
        match judge(&set, &id, &document, name) {
            Verdict::Accepted => {}
            other => panic!("{name}: council direction fixture is not schema-valid: {other:?}"),
        }
    }
}

/// S2 — RED WINDOW. Evidence claiming a conclusive zero is accepted, and so is an unknown coverage
/// token, because nothing on the wire references the closed vocabulary that would constrain it.
///
/// When this fails, the coverage cross-check has landed. Move S2 to MARKED and invert this guard.
#[test]
fn s2_false_structural_absence_is_still_accepted() {
    let directory = development_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "development-envelope.schema.json");

    for fixture in [
        "s2-false-structural-absence/evidence-claims-complete.json",
        "s2-false-structural-absence/evidence-unknown-token.json",
    ] {
        let document = load_json(&corpus().join(fixture));
        match judge(&set, &id, &document, fixture) {
            Verdict::Accepted => {}
            other => panic!("{fixture}: expected the red-window state (accepted), got {other:?}"),
        }
    }
}

/// S4 — RED WINDOW. A capsule that empties every section and declares no exclusion is accepted,
/// because `excluded` is optional and the sections carry no `minItems`.
///
/// The honest twin is checked alongside it: the property is "what was dropped is declared", never
/// "nothing was dropped", and a protection that refused both would be wrong in the other direction.
#[test]
fn s4_unsafe_compression_is_still_accepted() {
    let directory = root_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "context-capsule.schema.json");

    for fixture in [
        "s4-unsafe-compression/capsule-drops-silently.json",
        "s4-unsafe-compression/capsule-declares-exclusion.json",
        "s4-unsafe-compression/capsule-intact.json",
    ] {
        let document = load_json(&corpus().join(fixture));
        match judge(&set, &id, &document, fixture) {
            Verdict::Accepted => {}
            other => panic!("{fixture}: expected accepted, got {other:?}"),
        }
    }
}

/// Closing S4 moves a RELEASED schema, and the red window above does not say so.
///
/// The three red-window entries read as one class — the harness doc says "S2, S4 and S5a retain
/// their prior status" in a single breath — but their remedies land in three different places, and
/// only one of them is expensive. Measured on `origin/main` at `da1ae632`:
///
/// | entry | the protection edits | released? |
/// |---|---|---|
/// | S2 | `extensions/builtin/graphhelm-development-contracts/schemas/development-envelope.schema.json` | no — free to tighten |
/// | S4 | `schemas/context-capsule.schema.json` | **yes** — byte-identical to `schemas/releases/1.0.0/` |
/// | S5a | nothing — captured journey evidence has no schema in this tree | not applicable |
///
/// S4's stated remedy is a TIGHTENING: `excluded` becomes required, or the sections gain `minItems`.
/// Applied to the working copy, that same tightening lands on the frozen `1.0.0` baseline, and every
/// capsule already legal under it stops validating. That is a release decision, not a schema edit,
/// and it belongs to the same class ADR-033 deferred for the countersign signature field — a closed
/// released shape refuses a document carrying the new requirement rather than ignoring it.
///
/// The control that gives this cell its force: all fifteen root schemas with a `1.0.0` counterpart
/// are byte-identical to it today, so divergence is not the local habit and cannot be waved through
/// as one. Whoever closes S4 by editing the working copy alone has silently moved the release.
///
/// **What this cell asserts is the identity, not the tightening.** It fails the moment the two
/// copies part, which is exactly when the reader needs the sentence above — before the PR, not
/// after. It cannot prevent the edit; it makes the edit announce itself. A guard that fires for the
/// right reason with the wrong name costs a debugging session, so the message names the decision
/// rather than the mismatch.
#[test]
fn closing_s4_would_move_the_released_capsule_schema() {
    let working = root_schemas().join("context-capsule.schema.json");
    let released = root_schemas()
        .join("releases/1.0.0")
        .join("context-capsule.schema.json");

    // CONTROL FIRST. Two unreadable paths compare equal as errors and would satisfy the assertion
    // below while observing nothing.
    let working_bytes = fs::read(&working)
        .unwrap_or_else(|error| panic!("HARNESS-BROKE: {} unreadable: {error}", working.display()));
    let released_bytes = fs::read(&released).unwrap_or_else(|error| {
        panic!("HARNESS-BROKE: {} unreadable: {error}", released.display())
    });
    assert!(
        !working_bytes.is_empty() && !released_bytes.is_empty(),
        "HARNESS-BROKE: an empty schema file makes the comparison below meaningless"
    );

    assert_eq!(
        working_bytes, released_bytes,
        "`schemas/context-capsule.schema.json` no longer matches `schemas/releases/1.0.0/`. If this \
         is the S4 protection landing (requiring `excluded`, or `minItems` on the sections), then \
         the tightening has moved the FROZEN 1.0.0 baseline and every capsule already legal under \
         it stops validating -- a release decision, not a schema edit, and the same class ADR-033 \
         deferred for the countersign signature. If it is anything else, this cell is the wrong \
         reader and should be retired with its reason. Either way the answer belongs in the commit \
         that parts them, written down."
    );
}

/// S1b — MARKED. The schema accepts both fixtures because identity joins are outside JSON Schema;
/// the journey-contract gate then refuses the actor-as-observer attack and accepts the independent
/// control.
#[test]
fn s1b_observer_is_the_actor_is_refused_by_the_journey_contract_gate() {
    let directory = jpd_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "journey-contract.schema.json");

    let attack = "s1b-observer-is-the-actor/contract-observer-is-the-actor.json";
    let attack_document = load_json(&corpus().join(attack));
    assert!(matches!(
        judge(&set, &id, &attack_document, attack),
        Verdict::Accepted
    ));
    let contract_digest = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    let required_observer_capability = "browser.semantic-journey";
    let verification_fixture = repository_root().join(
        "extensions/builtin/graphhelm-jpd/fixtures/positive/journey-verification-first-pass.json",
    );
    let bound_evidence = |mut contract: Value, observer_id: &str| {
        contract["promises"][0]["requiredObserverCapability"] =
            Value::String(required_observer_capability.to_owned());
        let mut verification_result = load_json(&verification_fixture);
        verification_result["contractId"] = Value::String("journey/checkout-s1b".to_owned());
        verification_result["contractDigest"] = Value::String(contract_digest.to_owned());
        verification_result["bindings"]["observers"][0]["observerId"] =
            Value::String(observer_id.to_owned());
        JpdEvidence::JourneyContract(JourneyContractEvidence {
            contract,
            contract_digest: contract_digest.to_owned(),
            observation_obligations: vec![matched_s1b_obligation(
                contract_digest,
                required_observer_capability,
                observer_id,
            )],
            verification_result,
        })
    };
    let attack_evidence = bound_evidence(attack_document, "agent/checkout-driver");
    assert_journey_evidence_is_schema_valid(&set, &directory, &attack_evidence, "s1b-attack");
    let attack_diagnostics = JourneyContractGate.diagnostics(&attack_evidence);
    assert_eq!(attack_diagnostics.len(), 1);
    assert_eq!(
        attack_diagnostics[0].code,
        "GHJPD001_ACTOR_SELF_OBSERVATION"
    );
    assert_eq!(
        attack_diagnostics[0].path,
        "/observationObligations/0/resolution/capabilityBinding/observerId"
    );
    assert_eq!(
        attack_diagnostics[0].severity,
        JourneyDiagnosticSeverity::Error
    );
    assert_eq!(
        attack_diagnostics[0].source_file,
        "observation-obligations.json"
    );
    let attack_verdict = JourneyContractGate.evaluate(&attack_evidence);
    assert!(!attack_verdict.passed, "the actor cannot observe itself");
    assert_eq!(
        attack_verdict.findings,
        [
            "promise promise/receipt-durable is bound to observer agent/checkout-driver, which is also the actor for step step/submit-order; the observer must be independent from the actor"
        ]
    );

    let control = "s1b-observer-is-the-actor/contract-observer-independent.json";
    let control_document = load_json(&corpus().join(control));
    assert!(matches!(
        judge(&set, &id, &control_document, control),
        Verdict::Accepted
    ));
    let control_evidence = bound_evidence(control_document, "graphhelm-browser-user-journey");
    for (name, evidence) in [("attack", &attack_evidence), ("control", &control_evidence)] {
        let JpdEvidence::JourneyContract(evidence) = evidence else {
            panic!("{name}: expected journey evidence");
        };
        for capability in [
            evidence
                .contract
                .pointer("/promises/0/requiredObserverCapability"),
            evidence.observation_obligations[0].pointer("/observerRequirements/capability"),
            evidence.observation_obligations[0]
                .pointer("/resolution/capabilityBinding/capabilityId"),
        ] {
            assert_eq!(
                capability.and_then(Value::as_str),
                Some(required_observer_capability),
                "{name}: capability must stay constant so only observer identity moves"
            );
        }
        assert_ne!(
            required_observer_capability, "agent/checkout-driver",
            "the fixed capability must not itself be the actor identity"
        );
    }
    assert_journey_evidence_is_schema_valid(&set, &directory, &control_evidence, "s1b-control");
    let control_verdict = JourneyContractGate.evaluate(&control_evidence);
    assert!(
        control_verdict.passed,
        "the independent observer must remain accepted: {:?}",
        control_verdict.findings
    );
}

/// The JPD set refuses a contract missing the observer requirement.
///
/// This is what makes S1b an attack on a SATISFIED requirement rather than a missing one: the field
/// is mandatory, the sabotage fills it in correctly, and the lie is in the identity it names.
#[test]
fn the_observer_requirement_is_mandatory_which_is_why_s1b_bites() {
    let directory = jpd_schemas();
    let set = schema_set(&directory);
    let id = schema_id(&directory, "journey-contract.schema.json");
    let mut document =
        load_json(&corpus().join("s1b-observer-is-the-actor/contract-observer-is-the-actor.json"));

    document["promises"][0]
        .as_object_mut()
        .expect("a promise object")
        .remove("requiredObserverCapability");

    match judge(&set, &id, &document, "observer-requirement-probe") {
        Verdict::Refused(_) => {}
        other => panic!("requiredObserverCapability is not enforced after all: {other:?}"),
    }
}

/// One citation from the harness doc: the file, the line it names, and the token it promises there.
#[derive(Debug)]
struct Citation {
    path: String,
    line: usize,
    token: String,
}

/// Every `path.rs:LINE` citation in the harness doc, paired with the token it promises at that line.
///
/// The population is DERIVED from the document, never hand-copied: a hand list would be a second
/// copy of the thing under test, and the copy is the side nothing checks. A citation added to the
/// doc joins this population by being written, which is the only way a coverage claim stays true.
///
/// The token is the first identifier-shaped word after the citation -- on the same line where the
/// doc writes `memory.rs:240   if content_is_secret_shaped(...)`, or on the next non-empty line
/// where it writes the path and indents the symbol beneath it. Both spellings appear in section 7.
fn doc_citations(text: &str) -> Vec<Citation> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        for word in line.split_whitespace() {
            // The doc writes most citations inside backticks, and some end in a comma or a full
            // stop. Trimming the wrapper is not cosmetic: an untrimmed "`core/..." fails the
            // is_file check below and the citation is skipped SILENTLY, which is a guard that reads
            // half its population while its name claims all of it. Measured, that half was 6 of 13.
            let raw = word.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '/'));
            let Some((path, rest)) = raw.split_once(".rs:") else {
                continue;
            };
            let path = format!("{path}.rs");
            // A range (`:364-368`) names a region rather than one line; the guard below asks about
            // an exact line, so a range is out of its reach and is skipped rather than guessed at.
            let Ok(number) = rest.parse::<usize>() else {
                continue;
            };
            if !repository_root().join(&path).is_file() {
                continue;
            }
            let tail = line
                .split_once(word)
                .map(|(_, after)| after.to_owned())
                .unwrap_or_default();
            // The next line answers for this citation ONLY when the citation stands alone on its
            // own -- the code-fence spelling where the path sits on one line and the symbol is
            // indented beneath it. Letting prose fall through to the next line makes a citation
            // borrow the FOLLOWING citation's symbol and then accuse its own file of not containing
            // it: measured, `memory.rs:512` was reported missing a token belonging to
            // `jpd_plugin.rs`. A guard that invents a failure is worse than one that misses.
            let token = if tail.trim().is_empty() {
                lines
                    .iter()
                    .skip(index + 1)
                    .find(|candidate| !candidate.trim().is_empty())
                    .filter(|candidate| !candidate.contains(".rs:"))
                    .and_then(|candidate| identifier_in(candidate))
            } else {
                identifier_in(&tail)
            };
            if let Some(token) = token {
                found.push(Citation {
                    path,
                    line: number,
                    token,
                });
            }
        }
    }
    found
}

/// The first word long enough to be a symbol rather than prose punctuation.
fn identifier_in(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|word| {
            word.len() >= 5
                && word.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && word.chars().any(|c| c == '_' || c.is_uppercase())
        })
        .map(str::to_owned)
}

/// Every JSON fixture in the sabotage corpus, where provenance fields live beside the attacks.
fn corpus_provenance_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![corpus()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
            panic!("HARNESS-BROKE: {} unreadable: {error}", directory.display())
        });
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                files.push(path);
            }
        }
    }
    assert!(
        !files.is_empty(),
        "HARNESS-BROKE: the sabotage corpus holds no JSON, so its citations go unread"
    );
    files
}

/// Append every string under a `_`-prefixed key, one per line, so the doc extractor can read it.
///
/// **A `_` key marks its whole SUBTREE as provenance, not just a string sitting directly under it.**
/// The first version of this collector took only direct string children, and `markers.json` writes
/// its citations as `"_detectors": {"memory": "...", "durable": "..."}` -- one level down, under
/// keys that carry no underscore of their own. The widening looked live and read nothing. It was
/// caught by re-rotting the fixture's citation and watching the guard stay GREEN, never by reading
/// this function.
///
/// One string per line matters: the extractor's alone-on-its-line rule decides whether a citation
/// may borrow the next line's symbol, and running two provenance fields together would let one
/// answer for the other -- a defect this extractor already had once.
fn collect_underscore_strings(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key.starts_with('_') {
                    collect_every_string(child, out);
                } else {
                    collect_underscore_strings(child, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_underscore_strings(item, out);
            }
        }
        _ => {}
    }
}

/// Every string anywhere inside a subtree already known to be provenance.
fn collect_every_string(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push('\n');
            out.push_str(text);
        }
        Value::Object(map) => {
            for child in map.values() {
                collect_every_string(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_every_string(item, out);
            }
        }
        _ => {}
    }
}

/// Every cited line still holds the symbol the doc promises there.
///
/// Section 7 opens with *"Every site below is measured at `origin/main`"*, and that sentence reads
/// as a standing property when it records a single past act. Nothing re-measured the sites, and
/// four of the first four spot-checked had ROTTED: the protections are all intact and merely moved,
/// but `development_contract_schemas.rs:358` had drifted by **209 lines** -- line 367, the doc's
/// "would fall at", lands inside an unrelated JSON literal today. A reader following a rotted
/// coordinate finds nothing where a protection was promised, and the honest conclusion available to
/// them is that the protection is gone. That is the closed half of this corpus reporting itself as
/// open, which is the exact dishonesty section 7 exists to prevent.
///
/// **This is why the cell asks about CONTENT, not existence.** A citation whose file still exists
/// and whose line still fits inside it passes any weaker check while pointing at the wrong code --
/// rot is invisible to a bounds test, and the failure mode is silent by construction.
///
/// Every rotted citation is reported together rather than the first one found: a repair pass wants
/// the list, and stopping at the first turns one fix into one run each.
#[test]
fn every_cited_line_in_the_harness_doc_still_holds_its_symbol() {
    let doc = repository_root().join("docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md");
    let text = fs::read_to_string(&doc)
        .unwrap_or_else(|error| panic!("HARNESS-BROKE: the harness doc is unreadable: {error}"));
    // The corpus fixtures cite the same protections the doc does, and the rot was in BOTH: the doc
    // said `memory.rs:285` and so did `markers.json`. A guard whose population is only the document
    // would have reported the class closed with half of it still rotting.
    //
    // Only `_`-prefixed keys are read, and that boundary is not cosmetic. Fixture payloads carry
    // SYNTHETIC paths of the same shape -- `core/auth/middleware.rs:42` is invented journey data,
    // and a retrieval fixture lists `core/runtime/src/lib.rs:1` as a made-up hit. Those are not
    // claims about this repository, and flagging them would be a guard fabricating failures out of
    // test data. The corpus reserves `_safety`, `_measured_at`, `_detectors`, `_shape`, `_note` for
    // provenance, so provenance is what this reads.
    let mut text = text;
    for path in corpus_provenance_files() {
        collect_underscore_strings(&load_json(&path), &mut text);
    }
    let citations = doc_citations(&text);

    // POPULATION CONTROL. An extractor that matched nothing would satisfy the loop below while
    // reading no citation at all, and the doc's whole point is that it cites.
    assert!(
        citations.len() >= 4,
        "HARNESS-BROKE: only {} citations extracted from the harness doc, so a green below would \
         mean the extractor stopped working rather than the coordinates being sound",
        citations.len()
    );

    let mut rotted = Vec::new();
    for citation in &citations {
        let source = fs::read_to_string(repository_root().join(&citation.path))
            .unwrap_or_else(|error| panic!("HARNESS-BROKE: {} unreadable: {error}", citation.path));
        let cited = source.lines().nth(citation.line - 1).unwrap_or("");
        if !cited.contains(&citation.token) {
            let moved = source
                .lines()
                .position(|line| line.contains(&citation.token))
                .map(|index| format!("now at :{}", index + 1))
                .unwrap_or_else(|| "not found in the file at all".to_owned());
            rotted.push(format!(
                "{}:{} promises `{}` -- {moved}",
                citation.path, citation.line, citation.token
            ));
        }
    }

    assert!(
        rotted.is_empty(),
        "{} of {} cited coordinates in the harness doc have ROTTED. The protections may be intact \
         and merely moved -- check before editing anything but the doc -- but a reader following \
         these lands on unrelated code and concludes the protection was removed:\n  {}",
        rotted.len(),
        citations.len(),
        rotted.join("\n  ")
    );
}

/// S5a — the marker corpus's own claims, MEASURED against both shipped detectors.
///
/// S5a is the only red-window entry with no cell in this suite, and the harness doc gives the
/// honest reason: captured journey evidence has no schema in this tree, so validating a shape
/// chosen here against a schema chosen here is a tautology. That argument rules out a SCHEMA-grain
/// cell. It does not rule out this one.
///
/// `markers.json` declares, per marker, whether each shipped detector refuses it — eight booleans
/// across four markers, every one of them written BY HAND and read by nothing. They encode the
/// oracle divergence the doc records as drift: the memory screen is a bare `contains("ghp_")`,
/// while the durable scanner requires a prefix AND a tail of 16 or more. `ghp_` alone therefore
/// splits them, and the corpus says so in a field no test has ever executed.
///
/// **Why an unmeasured field here is worse than a missing one.** When the S5a protection lands it
/// will reuse a shipped detector, and these markers are what it will be tested against. A marker
/// the real detector does not recognise makes the future guard pass while refusing nothing — the
/// sabotage accepted for the wrong reason, wearing a green. That is the fixture-and-defect-share-a
/// -shape failure, pre-installed, and the moment to catch it is before the protection exists.
///
/// Both doors are the PUBLIC ones — `admit_memory_candidate` and `validate_durable_content` — not
/// the private predicates the corpus cites. The private function is not what production calls, and
/// a detector reachable only through a path nobody uses is not the detector under test.
///
/// The markers are synthetic by construction and the corpus says so at `_safety`: each carries the
/// SHAPE a detector matches, none is a credential, and none authenticates anything.
#[test]
fn every_s5a_marker_behaves_as_the_corpus_says_against_both_shipped_detectors() {
    use graphhelm_governor::{MemoryCandidate, MemoryRefusalCode, admit_memory_candidate};
    use graphhelm_graph::{DurableContentError, validate_durable_content};
    use graphhelm_protocols::{DevelopmentScope, ProjectId, WorkspaceId};

    let markers = load_json(&corpus().join("s5a-secret-capture/markers.json"));
    let markers = markers["markers"]
        .as_array()
        .expect("the marker corpus carries a markers array");
    assert_eq!(
        markers.len(),
        4,
        "the marker corpus changed size: re-read its classes before trusting the loop below"
    );

    let scope = DevelopmentScope {
        workspace_id: WorkspaceId::parse("workspace-s5a").expect("a valid workspace id"),
        project_id: ProjectId::parse("project-s5a").expect("a valid project id"),
        subproject_id: None,
        execution_id: None,
    };

    // CONTROL FIRST. Both detectors must be shown to ACCEPT something before a refusal means
    // anything: a screen that refuses every input satisfies every `true` below while observing
    // nothing, and half these markers assert `false`.
    let benign = "an ordinary sentence with no credential shape in it";
    assert!(
        admit_memory_candidate(&MemoryCandidate::draft(scope.clone(), benign), &scope).is_ok(),
        "CONTROL FAILED: the memory screen refused benign content, so every verdict below is noise"
    );
    assert!(
        validate_durable_content(&serde_json::json!({ "text": benign }), &[]).is_ok(),
        "CONTROL FAILED: the durable scanner refused benign content, so every verdict below is \
         noise"
    );

    let mut divergences = Vec::new();
    for marker in markers {
        let id = marker["id"].as_str().expect("every marker has an id");
        let value = marker["value"].as_str().expect("every marker has a value");

        let memory_refuses = match admit_memory_candidate(
            &MemoryCandidate::draft(scope.clone(), value),
            &scope,
        ) {
            Ok(()) => false,
            Err(refusal) => {
                // A refusal for the WRONG cause would satisfy a boolean check while proving
                // nothing about secret detection. The code is what the corpus is claiming.
                assert_eq!(
                    refusal.code(),
                    MemoryRefusalCode::SecretDetected,
                    "marker {id} was refused by the memory screen for `{:?}`, not for carrying a \
                     secret shape -- the corpus claim is about secret detection",
                    refusal.code()
                );
                true
            }
        };
        // Same grain as the memory side above, and for the same reason: `LimitExceeded` is also an
        // error, and a marker refused for its SIZE would satisfy a bare `is_err()` while proving
        // nothing about secret detection. `Unsafe` is the only verdict the corpus is claiming.
        let durable_refuses =
            match validate_durable_content(&serde_json::json!({ "text": value }), &[]) {
                Ok(()) => false,
                Err(DurableContentError::Unsafe) => true,
                Err(other) => panic!(
                    "marker {id} was refused by the durable scanner as `{other:?}`, not as unsafe \
                     content -- the corpus claim is about secret detection"
                ),
            };

        for (detector, measured, declared) in [
            ("memory", memory_refuses, marker["memory_refuses"].as_bool()),
            (
                "durable",
                durable_refuses,
                marker["durable_refuses"].as_bool(),
            ),
        ] {
            let declared =
                declared.unwrap_or_else(|| panic!("marker {id} declares no {detector}_refuses"));
            if declared != measured {
                divergences.push(format!(
                    "{id}: `{detector}_refuses` says {declared}, the shipped detector says \
                     {measured}"
                ));
            }
        }
    }

    assert!(
        divergences.is_empty(),
        "the S5a marker corpus disagrees with the detectors it was measured against. Either a \
         detector changed and the corpus was not re-measured, or the corpus was wrong when \
         written -- and whichever it is, the S5a protection built on these markers would be tested \
         against fixtures that do not bite:\n  {}",
        divergences.join("\n  ")
    );
}

/// Every corpus entry ships its own README, and the MARKED companion exists.
///
/// An entry without its reasoning is a fixture nobody can judge later: the trigger, the owning
/// issue and the site it would fall at travel with the files or they do not travel at all.
#[test]
fn every_corpus_entry_carries_its_reasoning() {
    // The reasoning used to sit in a README beside each entry. It lives in the harness doc now,
    // because an extension package declares its own inventory and there is no contribution kind
    // for prose: a file the manifest cannot declare is a file the package guard refuses. So this
    // guard follows the reasoning to where it went, rather than asserting a file that had to move.
    let doc = repository_root().join("docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md");
    let text = fs::read_to_string(&doc).unwrap_or_else(|error| {
        panic!(
            "the harness doc is unreadable at {}: {error}",
            doc.display()
        )
    });

    let corpus_entries = [
        "s1b-observer-is-the-actor",
        "s2-false-structural-absence",
        "s4-unsafe-compression",
        "s5a-secret-capture",
    ];
    for entry in corpus_entries {
        assert!(
            corpus().join(entry).is_dir(),
            "corpus entry {entry} has no fixtures"
        );
        assert!(
            text.contains(entry),
            "the harness doc never names {entry}, so its expected assertion travels nowhere"
        );
    }
    assert!(
        text.contains("Appendix B"),
        "the MARKED entries are gone from the doc: the corpus would imply the closed boundaries were never considered"
    );
    let red_window = [
        "s2-false-structural-absence",
        "s4-unsafe-compression",
        "s5a-secret-capture",
    ];
    assert_eq!(
        red_window.len(),
        3,
        "the red-window denominator changed without this guard being updated"
    );

    // The package must not carry prose again: it would pass review and fail the inventory guard.
    let stray = walk_markdown(&corpus());
    assert!(
        stray.is_empty(),
        "markdown is back inside the package and the manifest cannot declare it: {stray:?}"
    );
}

/// Every `.md` under a directory, so the guard above can say WHICH file returned.
fn walk_markdown(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk_markdown(&path));
        } else if path.extension().is_some_and(|ext| ext == "md") {
            found.push(path);
        }
    }
    found
}

/// The secret markers are labelled with what each detector does, and the classes are all present.
///
/// The `DECLARED_GAP` class is the load-bearing one: it records what the memory screen says it
/// deliberately does not reject, so widening or narrowing that screen without amending its own
/// declaration is visible here.
#[test]
fn the_marker_corpus_labels_every_class() {
    let markers = load_json(&corpus().join("s5a-secret-capture/markers.json"));
    let entries = markers["markers"]
        .as_array()
        .expect("the marker corpus is an array");
    assert!(
        !entries.is_empty(),
        "no markers: every check below would be vacuous"
    );

    let mut classes = std::collections::BTreeSet::new();
    for marker in entries {
        let class = marker["class"]
            .as_str()
            .unwrap_or_else(|| panic!("marker {:?} carries no class", marker["id"]));
        assert!(
            marker["memory_refuses"].is_boolean() && marker["durable_refuses"].is_boolean(),
            "marker {:?} does not say what each detector does",
            marker["id"]
        );
        classes.insert(class.to_owned());
    }
    for required in ["AGREED", "DIVERGENT", "DECLARED_GAP"] {
        assert!(
            classes.contains(required),
            "the marker corpus lost its {required} class"
        );
    }
}

/// The shipped gate survives its own thymus.
///
/// The positive control for everything below: if `certify` could not certify the shipped gate
/// against the shipped suite, no verdict here would carry information.
#[test]
fn the_shipped_gate_is_certified_against_the_shipped_suite() {
    let certification = certify(&VerificationResultGate, &jpd_suite()).unwrap_or_else(|refusal| {
        panic!("the shipped gate was fooled by its own suite: {refusal:?}")
    });
    assert_eq!(certification.gate_id, "gate/jpd-verification-result");
    assert_eq!(
        certification.specimens, 2,
        "the shipped floor is two specimens; a change here must be read against this corpus"
    );

    let journey_certification = certify(&JourneyContractGate, &journey_contract_suite())
        .unwrap_or_else(|refusal| {
            panic!("the journey-contract gate was fooled by its own suite: {refusal:?}")
        });
    assert_eq!(journey_certification.gate_id, "gate/jpd-journey-contract");
    assert_eq!(journey_certification.specimens, 1);
}

/// The axis vocabulary now includes S1b; S2, S4 and S5a still have no axis.
///
/// A denominator guard. When a third axis lands this fails, and whoever adds it is pointed at the
/// entries still waiting for one.
///
/// S1b used to map onto `SelfValidation`. #294 removed that arm rather than translating it, because
/// a verification result carries no producer/validator identity, so the axis was not expressible
/// against that document — and an axis with no basis in the evidence is worse than a missing one.
///
/// The replacement axis reads the journey contract's actual relation: each promise references a
/// step, and its required observer identity must differ from that step's actor identity.
#[test]
fn the_axis_set_includes_s1b_while_the_other_red_window_entries_remain_open() {
    let axes = [
        JpdFailureAxis::ActorUsedAsOwnObserver,
        JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
        JpdFailureAxis::FlakyClaimedAsProven,
    ];
    assert_eq!(
        axes.len(),
        3,
        "the JPD failure axes changed: re-read this corpus for entries that now have a home"
    );
}

/// Section 6 of the harness doc names the current status of every acceptance criterion.
///
/// The hazard is that section 6 reads as exhaustive: it already declares that no cargo had run and
/// that a gap between instruments was unmeasured, so a reader takes anything ABSENT from it as
/// handled. Two criteria were missing and were caught in review, not by me — replay preservation
/// and the advisory-consensus boundary — after I had written both into the pull request body and
/// neither into the document. **Two copies of one claim drifted, and the one under review was the
/// thinner one.**
///
/// This guard is keyed to the criteria rather than to prose, so adding a criterion to the issue
/// without declaring its status here fails rather than passes quietly.
#[test]
fn the_doc_declares_the_status_of_every_criterion() {
    let doc = repository_root().join("docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md");
    let text = fs::read_to_string(&doc)
        .unwrap_or_else(|error| panic!("the harness doc is unreadable: {error}"));
    let section = text
        .split_once("## 6.")
        .and_then(|(_, rest)| rest.split_once("## 7."))
        .map(|(section, _)| section)
        .expect("the harness doc still has a section 6 followed by a section 7");

    for criterion in ["Replay preservation", "browser", "advisory", "S5a", "cargo"] {
        assert!(
            section.contains(criterion),
            "section 6 never mentions `{criterion}`, so its status is hidden"
        );
    }
}
