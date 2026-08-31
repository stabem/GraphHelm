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
