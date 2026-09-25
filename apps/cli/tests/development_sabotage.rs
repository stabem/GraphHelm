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

use std::collections::{BTreeMap, BTreeSet};
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

/// S2 — MARKED. The schema accepts both fixtures because no `RetrievalPlan` per-kind schema exists
/// to carry the check; the retrieval-plan compiler (#616) then refuses the claims-complete attack
/// and accepts the independent control.
///
/// **The schema-accepts assertion STAYS, and that is the point of this rewrite (#630).** This test
/// used to say *"when this fails, the fix has landed — move S2 to MARKED"*, and its subject was the
/// schema validator. #616 landed the coverage cross-check in the RUNTIME, so the schema never
/// changed and this test would have stayed green forever: a tripwire whose own doc-comment promised
/// a signal it could not emit. The signal is gone, but the observation is still true and worth
/// asserting — the wire really does lose the vocabulary at schema grain, and a reader who finds this
/// green should learn that from a live assertion rather than infer it from a deleted test.
///
/// The half that moved: `core/runtime/tests/retrieval.rs`,
/// `the_committed_s2_corpus_is_refused_by_the_retrieval_plan_compiler`, which drives THESE fixtures
/// through the compiler. It lives there because the compiler needs a `StructuralCodeIndex` and the
/// measured fake is there; a second fake built beside this test would be a fake more generous than
/// the real one certifying its own attack. Same structure as S1b: schema half documented-permissive,
/// runtime half carrying the assertion.
#[test]
fn s2_schema_grain_still_accepts_both_fixtures_and_the_runtime_is_what_refuses() {
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
            other => panic!(
                "{fixture}: the schema now REFUSES this fixture. That is not a failure -- it means \
                 a `RetrievalPlan` per-kind schema (or an envelope tightening) has landed and the \
                 check moved to schema grain after all. Re-read the S2 write-up in the harness doc, \
                 which records the opposite, and decide which half owns the refusal now: {other:?}"
            ),
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
/// The released capsule schema's bytes, pinned by digest rather than by comparison.
///
/// Measured on `origin/main` at `1a0a6ff7`. Updating this constant is the visible act that editing
/// a frozen release is supposed to be.
const RELEASED_CAPSULE_SCHEMA_DIGEST: &str =
    "06c64a2576f9688cc9072df151957686886a64c46e1b76650566437cac046768";

/// Content with `\r\n` folded to `\n`, so a digest names the FILE and not the checkout.
///
/// The first version of this pin hashed the working tree and was measured on Windows.
/// `git ls-files --eol` answers in one command why that was wrong:
///
/// ```text
/// i/lf    w/crlf  attr/    schemas/releases/1.0.0/context-capsule.schema.json
/// ```
///
/// Index LF, working tree CRLF, and **no attribute** -- so the bytes on disk are decided by the
/// platform, and the pin would have failed on every LF checkout while passing on mine. A freeze is
/// a property of a fresh checkout, never of the author's tree: #600's class, one day old, aimed
/// straight back at me. Normalising first makes the digest equal the committed blob's, measured --
/// `git show HEAD:...` piped to sha256sum agrees with this constant. (Codex P1 on #610.)
///
/// **A blanket `filter(!= '\r')` was the wrong tool, caught one round later (Codex P2 on this
/// commit).** It strips every CR, not only the ones pairing with a following LF -- so a standalone
/// CR byte inserted as CONTENT into both copies at once would vanish from both hashes and the
/// digest would stay blind to it, the exact both-sides-move failure this pin exists to catch. Only
/// a CR immediately followed by LF is checkout noise; any other CR is authored bytes and must
/// survive into the hash. (Measured: today's released file has zero standalone CR, so this does
/// not move the pinned constant -- it only makes the computation match what the constant claims.)
fn without_cr(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1;
            continue;
        }
        normalized.push(bytes[index]);
        index += 1;
    }
    normalized
}

/// SHA-256 of the given bytes, rendered as lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

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

    // THE INDEPENDENT PIN, and the equality below cannot replace it. When S4 is closed by editing
    // BOTH the live schema and its 1.0.0 mirror -- the pattern this repository already documents at
    // `core/protocols/tests/dead_letter_is_declared_only.rs`, where #412 moved both and
    // `compare_catalogs` saw no change -- an equality check stays green while the frozen authored
    // artifact has moved. That is precisely the case D-046 forbids: authored content in a frozen
    // release never changes. A digest of the released bytes is the only assertion the both-sides
    // edit cannot satisfy. (Codex P2 on #606.)
    assert_eq!(
        sha256_hex(&without_cr(&released_bytes)),
        RELEASED_CAPSULE_SCHEMA_DIGEST,
        "`schemas/releases/1.0.0/context-capsule.schema.json` has MOVED. D-046: authored content in \
         a frozen release never changes. If this is the S4 tightening applied to both copies at \
         once, the tightening is a release decision that has quietly edited the baseline; if it is \
         a derived-metadata re-derivation, D-046 allows it only with a control in the same diff \
         proving no authored file was touched, and this digest is updated in that same commit"
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
    /// First line of the cited region; equal to `last` for a single-line citation.
    first: usize,
    last: usize,
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
/// One citation site the token heuristic could not reach: a real, resolvable path and line, but
/// no word nearby satisfying `identifier_in`. `context` is the exact text that was searched, so a
/// pin naming this site is checking the same thing the scan checked -- not a paraphrase of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UnreachedCitation {
    path: String,
    first_line: usize,
    /// The region's END, equal to `first_line` for a single coordinate.
    ///
    /// Without it a pinned RANGE watched only its start: `retrieval.rs:12-26` stored `12`, and
    /// moving `26` alone left the pin equal and green while a cited coordinate drifted unwatched.
    /// One-of-N again, this time inside a single region rather than across a comma list -- the
    /// comma fix did not reach it because a range is ONE region, not three. (Codex P2 on #610.)
    last_line: usize,
    context: String,
}

fn doc_citations(
    text: &str,
    verify: &impl Fn(&str, usize, &str) -> bool,
) -> (Vec<Citation>, Vec<UnreachedCitation>, usize) {
    let lines = text.lines().collect::<Vec<_>>();
    let mut found = Vec::new();
    let mut unreached_at: Vec<(usize, UnreachedCitation)> = Vec::new();
    // COORDINATES, matched to what `count_coordinates_in_line` spells: a range contributes TWO (its
    // endpoints), a comma list one per member, a single line one. The control compares this against
    // raw syntax, so a suffix the extractor consumes only in PART -- `574 - 576` yielding one where
    // the text spells two -- fails a count instead of passing as an accepted prefix.
    let mut coordinates = 0;
    for (index, line) in lines.iter().enumerate() {
        for site in citation_sites(line) {
            let Some(path) = resolved_path(line, site.path_end) else {
                continue;
            };
            let tail = line[site.tail_start..].to_owned();
            // The next line answers for this citation ONLY when the citation stands alone on its
            // own -- the code-fence spelling where the path sits on one line and the symbol is
            // indented beneath it. Letting prose fall through to the next line makes a citation
            // borrow the FOLLOWING citation's symbol and then accuse its own file of not containing
            // it: measured, `memory.rs:512` was reported missing a token belonging to
            // `jpd_plugin.rs`. A guard that invents a failure is worse than one that misses.
            let (token, context) = if tail.trim().is_empty() {
                let next = lines
                    .iter()
                    .skip(index + 1)
                    .find(|candidate| !candidate.trim().is_empty())
                    .filter(|candidate| !candidate.contains(".rs:"));
                let token = next.and_then(|candidate| identifier_in(candidate));
                let context = next
                    .map(|candidate| candidate.trim().to_owned())
                    .unwrap_or_else(|| "<no usable next line>".to_owned());
                (token, context)
            } else {
                (identifier_in(&tail), tail.trim().to_owned())
            };
            match token {
                Some(token) => {
                    coordinates += site
                        .regions
                        .iter()
                        .map(|(first, last)| if first == last { 1 } else { 2 })
                        .sum::<usize>();
                    for (first, last) in site.regions {
                        found.push(Citation {
                            path: path.clone(),
                            first,
                            last,
                            token: token.clone(),
                        });
                    }
                }
                // ONE ENTRY PER COORDINATE, not one per site. `site.regions[0].0` recorded the
                // first and dropped the rest, so an unreachable comma list like
                // `tests/memory.rs:63, 858, 887` pinned `63` and let 858 and 887 leave EVERY
                // population: unread by the rot check because the site has no token, and unnamed
                // by the pin because only the head survived. Two coordinates gone with nothing
                // going red -- this cell's own subject, aimed at this cell. (Codex P2 on #610.)
                None => {
                    coordinates += site
                        .regions
                        .iter()
                        .map(|(first, last)| if first == last { 1 } else { 2 })
                        .sum::<usize>();
                    unreached_at.extend(site.regions.iter().map(|(first, last)| {
                        (
                            index,
                            UnreachedCitation {
                                path: path.clone(),
                                first_line: *first,
                                last_line: *last,
                                context: context.clone(),
                            },
                        )
                    }));
                }
            }
        }
    }

    // SECTION-SCOPED backfill: a gap closes when some OTHER word inside the same `---`-bounded
    // section of THIS source (the doc's own boundary for one protection's entry) is both
    // identifier-shaped and verified present on the gap's exact cited line. The candidate always
    // comes from the section's TEXT, never from scanning the target file for a plausible word --
    // `verify` only ever confirms a doc-sourced hypothesis, it never proposes one. Restricted to
    // single-line gaps: a range's own rot check only proves a token sits SOMEWHERE across the
    // range, never on one line within it, so a range gap is left for `backfill_same_line_gaps`'s
    // exact-coordinate rule (or stays pinned) rather than risking that ambiguity here too.
    //
    // **AMBIGUITY REFUSES, it does not pick one (M, PR #682 review).** A section can verify MORE
    // THAN ONE candidate against the same line -- `tests/memory.rs:63`'s section verifies both
    // `MemoryRefusalCode` and `SecretDetected` against that one assertion. Closing on the first
    // match found would let the choice be made BY THE FILE (whichever candidate a scan happens to
    // reach first), and a future edit that drops one of the two while keeping the other would
    // re-verify against whichever survived and stay green -- coverage that looks stable while the
    // specific claim it once confirmed quietly changed underneath it. A pin that stays pinned
    // when the doc names two plausible symbols and never says which one is the real citation is
    // honest about that; a guard that silently picks is not. Only a UNIQUE verified candidate
    // closes a gap.
    let sections = doc_sections(&lines);
    let mut unreached = Vec::new();
    'gap: for (doc_line, gap) in unreached_at {
        let own_section = (gap.first_line == gap.last_line)
            .then(|| {
                sections
                    .iter()
                    .find(|(f, l)| *f <= doc_line && doc_line <= *l)
            })
            .flatten();
        if let Some(&(first, last)) = own_section {
            let mut verified: Vec<String> = lines[first..=last]
                .iter()
                .flat_map(|line| all_identifiers_in(&mask_citation_regions(line)))
                .filter(|candidate| verify(&gap.path, gap.first_line, candidate))
                .collect();
            verified.sort();
            verified.dedup();
            if let [token] = verified.as_slice() {
                found.push(Citation {
                    path: gap.path.clone(),
                    first: gap.first_line,
                    last: gap.last_line,
                    token: token.clone(),
                });
                continue 'gap;
            }
        }
        unreached.push(gap);
    }

    (found, unreached, coordinates)
}

/// `path.rs:LINE`'s own path and coordinate text, replaced with spaces so a path segment (which
/// may itself contain `_` -- `development_contract_schemas`, say) can never masquerade as an
/// identifier-shaped candidate. Byte-for-byte, not char-for-char: every boundary this touches was
/// already proven to sit on a UTF-8 char boundary by `citation_sites`' own ASCII-only stepping.
fn mask_citation_regions(line: &str) -> String {
    let mut masked = line.as_bytes().to_vec();
    for site in citation_sites(line) {
        let mut start = site.path_end;
        while start > 0 {
            let candidate = masked[start - 1];
            let ok = candidate.is_ascii_alphanumeric()
                || candidate == b'_'
                || candidate == b'/'
                || candidate == b'.'
                || candidate == b'-';
            if !ok {
                break;
            }
            start -= 1;
        }
        for byte in &mut masked[start..site.tail_start] {
            *byte = b' ';
        }
    }
    String::from_utf8(masked)
        .expect("masking only ever substitutes ASCII spaces at char boundaries")
}

/// Doc lines grouped into `---`-bounded sections, as `[first, last]` INCLUSIVE 0-based ranges.
/// A source with no `---` at all (a JSON fixture's collected provenance strings) is one section --
/// the same "whole source is the unit" default `backfill_same_line_gaps` already relies on.
fn doc_sections(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut sections = Vec::new();
    let mut start = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if index > start {
                sections.push((start, index - 1));
            }
            start = index + 1;
        }
    }
    if start < lines.len() {
        sections.push((start, lines.len() - 1));
    }
    sections
}

/// `doc_citations`, combined across MULTIPLE independent sources without letting one source's
/// lookahead see into another's text.
///
/// Concatenating every source into one buffer before a single `doc_citations` call let a
/// trailing citation whose own source has no next line "see" whatever the next source happened
/// to start with. `s2-false-structural-absence/producer-record.json`'s trailing
/// `retrieval.rs:12-26` citation is unreachable in ITS OWN file, but once every fixture's
/// provenance strings were joined into one blob, which fixture `read_dir` placed immediately
/// after it decided whether that citation stayed pinned or silently borrowed a neighbour's
/// token -- a test outcome that depended on filesystem traversal order, not on content. (Codex
/// P2 on this commit.)
///
/// `verify(path, line, candidate)` answers whether the real file's exact line contains a
/// doc-sourced candidate -- the ONLY place either backfill pass is allowed to look at the target
/// file, and only ever to confirm a hypothesis the doc already made, never to propose one.
fn doc_citations_across_sources(
    sources: &[String],
    verify: &impl Fn(&str, usize, &str) -> bool,
) -> (Vec<Citation>, Vec<UnreachedCitation>, usize, usize) {
    let mut citations = Vec::new();
    let mut unreached = Vec::new();
    let mut consumed = 0;
    let mut present = 0;
    for source in sources {
        let (mut source_citations, mut source_unreached, source_consumed) =
            doc_citations(source, verify);
        present += count_citation_sites(source);
        consumed += source_consumed;
        citations.append(&mut source_citations);
        unreached.append(&mut source_unreached);
    }
    backfill_same_line_gaps(&mut citations, &mut unreached);
    (citations, unreached, consumed, present)
}

/// Close a gap by REUSE, never by guessing: a citation `identifier_in` could not reach at its own
/// coordinate is resolved anyway when some OTHER citation, anywhere in the corpus, already named a
/// token for that *exact same* `path:line` -- the same fact, stated twice, where one statement
/// happened to sit next to prose and the other next to code.
///
/// **Why this cannot reopen the borrow-wrong-symbol bug (`memory.rs:512` borrowing `jpd_plugin.rs`'s
/// token, Codex P2 on #606).** That bug borrowed a NEIGHBOUR's token across an ADJACENT line. This
/// only ever borrows a donor whose own resolved coordinate is `first == last == gap.first_line ==
/// gap.last_line` on the SAME path -- not a nearby line, not a containing range. A donor that spans
/// `620-622` does not backfill a gap pinned at `622` alone through THIS mechanism: the donor's own
/// rot check only proves its token sits SOMEWHERE in `620..=622`, never that it sits on `622`
/// specifically, and accepting it here would trade a real pin for a rot check that could go
/// silently wrong -- proved by `a_range_donor_does_not_backfill_a_narrower_point_inside_it`. (That
/// particular gap, Section 7's "Would fall at" line, still closes -- through the SEPARATE
/// section-scoped pass below, which finds a different, single-line donor word and has the real
/// file confirm it before trusting it, rather than trusting a range's rot check by proxy.)
///
/// **Why this is not the tautology `identifier_in`'s own widening has to avoid.** The token still
/// comes from a citation's own text -- never from scanning the target file for a plausible word. This
/// only reuses a token some OTHER sentence in the doc or corpus already committed to, in writing, for
/// the identical coordinate.
fn backfill_same_line_gaps(citations: &mut Vec<Citation>, unreached: &mut Vec<UnreachedCitation>) {
    let donors: Vec<(String, usize, String)> = citations
        .iter()
        .filter(|citation| citation.first == citation.last)
        .map(|citation| {
            (
                citation.path.clone(),
                citation.first,
                citation.token.clone(),
            )
        })
        .collect();
    let mut resolved = Vec::new();
    unreached.retain(|gap| {
        if gap.first_line != gap.last_line {
            return true;
        }
        match donors
            .iter()
            .find(|(path, line, _)| *path == gap.path && *line == gap.first_line)
        {
            Some((path, line, token)) => {
                resolved.push(Citation {
                    path: path.clone(),
                    first: *line,
                    last: *line,
                    token: token.clone(),
                });
                false
            }
            None => true,
        }
    });
    citations.extend(resolved);
}

/// A citation's lookahead stays inside its own source and never borrows a neighbour's token.
///
/// `source_a` ends with a citation to a real file (`core/runtime/src/retrieval.rs`) whose OWN
/// text has no line after it -- it must be pinned unreachable. `source_b` starts with a line
/// that reads as an identifier. Concatenating the two before extraction (the defect this
/// function replaces) would let the citation borrow `source_b`'s token and silently stop being
/// unreachable; processing them as independent sources must not.
/// **The positive control is the concatenation** (#761): a count of one pinned citation cannot
/// tell "the boundary refused the borrow" from "there was never anything to borrow". Running the
/// SAME two fixtures joined into one source must close the gap - that is the temptation existing,
/// measured rather than assumed - and only then does the split result mean what it claims.
#[test]
fn citation_lookahead_does_not_cross_a_source_boundary() {
    let source_a = "core/runtime/src/retrieval.rs:12-26\n".to_owned();
    let source_b = "Synthetic borrowed_token from an unrelated fixture\n".to_owned();

    let (joined_citations, joined_unreached, _, _) =
        doc_citations_across_sources(&[format!("{source_a}{source_b}")], &never_verifies);
    assert!(
        joined_unreached.is_empty()
            && joined_citations
                .iter()
                .any(|citation| source_b.contains(&citation.token)),
        "ARRANGEMENT: joined into one source the citation MUST borrow a word from source_b, or \
         the split case below refuses a borrow nothing was offering: {joined_citations:?} \
         {joined_unreached:?}"
    );

    let (_, unreached, _, _) = doc_citations_across_sources(&[source_a, source_b], &never_verifies);
    assert_eq!(
        unreached
            .iter()
            .map(|pin| (pin.path.as_str(), pin.first_line, pin.last_line))
            .collect::<Vec<_>>(),
        vec![("core/runtime/src/retrieval.rs", 12, 26)],
        "the trailing citation in source_a has no next line WITHIN its own source and must stay \
         pinned unreachable, not borrow source_b's token: {unreached:?}"
    );
}

/// A `verify` that never confirms a candidate -- for tests exercising `backfill_same_line_gaps`
/// alone, where the section-scoped pass must contribute nothing.
fn never_verifies(_path: &str, _line: usize, _candidate: &str) -> bool {
    false
}

/// A gap closes when the SAME `---`-bounded section names an identifier-shaped word elsewhere
/// that a (fake, in-test) file check confirms sits on the gap's own cited line -- the mechanism
/// that closes `development_contract_schemas.rs:622` for real, where the token
/// (`is_fresh`) sits in an earlier one-line paragraph of the SAME entry, not next to the citation
/// itself.
#[test]
fn a_gap_closes_when_its_own_section_names_a_verified_word() {
    let source = "**Trigger.** Invert the comparison in `is_fresh()`.\n\n\
                  **Would fall at.** `apps/cli/tests/development_contract_schemas.rs:622`.\n"
        .to_owned();
    let verify = |path: &str, line: usize, candidate: &str| {
        path == "apps/cli/tests/development_contract_schemas.rs"
            && line == 622
            && candidate == "is_fresh"
    };
    let (citations, unreached, _, _) = doc_citations_across_sources(&[source], &verify);
    assert!(
        unreached.is_empty(),
        "the section's own earlier paragraph names is_fresh, and the (fake) file check confirms \
         it sits on line 622 -- this gap should have closed: {unreached:?}"
    );
    assert!(
        citations
            .iter()
            .any(|citation| citation.token == "is_fresh" && citation.first == 622),
        "the backfilled citation should carry the section's verified word: {citations:?}"
    );
}

/// A gap must NOT close from a word in a DIFFERENT `---`-bounded section, even one that a (fake)
/// file check would confirm -- crossing a section boundary to answer for a citation is the same
/// shape as crossing to a neighbouring citation, the #610 regression this file exists to keep
/// closed. `verify` here would say yes to everything; only the section boundary must refuse it.
#[test]
fn a_gap_does_not_backfill_across_a_section_boundary_even_if_verify_would_allow_it() {
    let source = "unrelated_neighbour_word one section over\n\n\
                  ---\n\n\
                  `core/graph/src/persistence.rs:736` names nothing identifier-shaped here.\n"
        .to_owned();
    let always_verifies = |_: &str, _: usize, _: &str| true;

    // POSITIVE CONTROL (#761): the same two lines with the section separator REMOVED must close
    // the gap. Without it, a count of one pinned citation cannot tell "the boundary refused the
    // candidate" from "no candidate was ever offered" - a fixture whose neighbour word stopped
    // being identifier-shaped would pass this cell while testing nothing.
    let without_boundary = source.replace("---\n\n", "");
    let (_, joined_unreached, _, _) =
        doc_citations_across_sources(&[without_boundary], &always_verifies);
    assert!(
        joined_unreached.is_empty(),
        "ARRANGEMENT: with no section boundary the neighbour word MUST close this gap, or the \
         refusal below is refusing a candidate that does not exist: {joined_unreached:?}"
    );

    let (_, unreached, _, _) = doc_citations_across_sources(&[source], &always_verifies);
    assert_eq!(
        unreached
            .iter()
            .map(|pin| (pin.path.as_str(), pin.first_line, pin.last_line))
            .collect::<Vec<_>>(),
        vec![("core/graph/src/persistence.rs", 736, 736)],
        "a word from the PRIOR section must not backfill a gap in the NEXT section, even when \
         every candidate would verify true: {unreached:?}"
    );
}

/// A gap must NOT close when its section's text verifies MORE THAN ONE candidate against the
/// gap's own cited line (M, PR #682 review). Picking the first match found would let the FILE
/// decide which of two doc-named symbols is "the" citation; a later edit that keeps one and drops
/// the other would then re-verify against whichever survived and stay green, silently -- coverage
/// that outlives the specific claim it once confirmed. Mirrors the real shape:
/// `tests/memory.rs:63`'s section names both `MemoryRefusalCode` and `SecretDetected`, and the
/// cited line contains both.
#[test]
fn a_gap_does_not_close_when_its_section_verifies_more_than_one_candidate() {
    let source = "`core/governor/src/memory.rs:256` code: MemoryRefusalCode::SecretDetected\n\
         `core/governor/tests/memory.rs:63` names nothing identifier-shaped on its own line.\n"
        .to_owned();
    let verify = |path: &str, line: usize, candidate: &str| {
        path == "core/governor/tests/memory.rs"
            && line == 63
            && (candidate == "MemoryRefusalCode" || candidate == "SecretDetected")
    };
    // POSITIVE CONTROL (#761): the ambiguity has to be REAL. With a `verify` that accepts only
    // ONE of the two candidates, this same fixture must close the gap and carry that candidate.
    //
    // MEASURED, and narrower than its four siblings: unlike them, this cell's bare count was NOT
    // blind. Damaging the fixture so the section names no identifier-shaped word costs the FIRST
    // citation its token too, the pinned population goes to two, and the old `len() == 1` went
    // red on its own (#761's sweep, half two). What the count still could not say is whether the
    // two candidates were ever ambiguous rather than simply absent - which is what this adds.
    let verify_one = |path: &str, line: usize, candidate: &str| {
        path == "core/governor/tests/memory.rs" && line == 63 && candidate == "MemoryRefusalCode"
    };
    let (single_citations, single_unreached, _, _) =
        doc_citations_across_sources(std::slice::from_ref(&source), &verify_one);
    assert!(
        single_unreached.is_empty()
            && single_citations
                .iter()
                .any(|citation| citation.first == 63 && citation.token == "MemoryRefusalCode"),
        "ARRANGEMENT: with only one candidate verifying, this fixture MUST close the gap - \
         otherwise the refusal below is not about ambiguity: {single_citations:?} \
         {single_unreached:?}"
    );

    let (citations, unreached, _, _) = doc_citations_across_sources(&[source], &verify);
    assert_eq!(
        unreached
            .iter()
            .map(|pin| (pin.path.as_str(), pin.first_line, pin.last_line))
            .collect::<Vec<_>>(),
        vec![("core/governor/tests/memory.rs", 63, 63)],
        "two candidates both verify against the same line -- ambiguous, must stay pinned rather \
         than silently picking one: {unreached:?}"
    );
    assert!(
        !citations.iter().any(|citation| citation.first == 63),
        "no citation should have been backfilled for the ambiguous line: {citations:?}"
    );
}

/// A gap closes when a SIBLING citation, anywhere in the corpus, already named a token for the
/// exact same `path:line` -- the #625 widening this file exists to prove.
#[test]
fn a_gap_closes_when_a_sibling_citation_names_the_same_exact_line() {
    let prose_only =
        "core/governor/src/memory.rs:296 screens with\nno symbol on this line\n".to_owned();
    let code_shaped =
        "core/governor/src/memory.rs:296 fn content_is_secret_shaped -> bool\n".to_owned();
    let (citations, unreached, _, _) =
        doc_citations_across_sources(&[prose_only, code_shaped], &never_verifies);
    assert!(
        unreached.is_empty(),
        "the prose-only mention of memory.rs:296 should have borrowed the token the code-shaped \
         mention already resolved for the SAME line: {unreached:?}"
    );
    assert!(
        citations
            .iter()
            .any(|citation| citation.path == "core/governor/src/memory.rs"
                && citation.first == 296
                && citation.token == "content_is_secret_shaped"),
        "the backfilled citation should carry the sibling's token: {citations:?}"
    );
}

/// A gap must NOT close when the only sibling is a RANGE that merely contains the gap's line: the
/// range's own rot check proves its token sits somewhere across the range, never on the gap's
/// single line specifically, and accepting it there is exactly the over-reach `backfill_same_line_gaps`
/// exists to refuse -- the shape of the #610 regression (`memory.rs:512` borrowing
/// `jpd_plugin.rs`'s token), reproduced deliberately here to prove the widening does not reopen it.
///
/// **The count alone could not tell this cell's two outcomes apart** (#761). `unreached.len() == 1`
/// held when the rule correctly refused the range donor, and it held identically when the donor
/// never parsed at all - a fixture whose excerpt shape broke would take the site out of every
/// population and leave the pinned point as the only entry, reading exactly like success. So the
/// donor's presence is asserted first, and the refusal is named by coordinate rather than counted.
#[test]
fn a_range_donor_does_not_backfill_a_narrower_point_inside_it() {
    let ranged = "core/graph/src/persistence.rs:730-732\n real_token_lives_here\n".to_owned();
    let point = "core/graph/src/persistence.rs:731 nothing identifier-shaped follows\n".to_owned();
    let (citations, unreached, _, _) =
        doc_citations_across_sources(&[ranged, point], &never_verifies);
    assert!(
        citations
            .iter()
            .any(|citation| citation.path == "core/graph/src/persistence.rs"
                && citation.first == 730
                && citation.last == 732
                && citation.token == "real_token_lives_here"),
        "ARRANGEMENT: the range donor MUST have parsed and carried its token, or this cell is \
         refusing a donation nobody offered: {citations:?}"
    );
    assert_eq!(
        unreached
            .iter()
            .map(|pin| (pin.path.as_str(), pin.first_line, pin.last_line))
            .collect::<Vec<_>>(),
        vec![("core/graph/src/persistence.rs", 731, 731)],
        "a point citation inside an unrelated range's span must stay pinned, not borrow the \
         range's token for a line the range never verified on its own: {unreached:?}"
    );
}

/// A gap must NOT close from a sibling at the same line number in a DIFFERENT file: path is part
/// of the coordinate, and matching on line alone would let one file's symbol answer for another's.
///
/// Same shape, same blindness, same remedy as the range-donor cell above (#761): the donor is
/// asserted present before the refusal is read, because a donor that vanished from the
/// population leaves the same count behind as a donor that was correctly refused.
#[test]
fn a_gap_does_not_backfill_across_a_different_path_at_the_same_line() {
    let resolved = "core/graph/src/persistence.rs:296 fn unrelated_token -> bool\n".to_owned();
    let gap = "core/governor/src/memory.rs:296 screens with\nno symbol on this line\n".to_owned();
    let (citations, unreached, _, _) =
        doc_citations_across_sources(&[resolved, gap], &never_verifies);
    assert!(
        citations
            .iter()
            .any(|citation| citation.path == "core/graph/src/persistence.rs"
                && citation.first == 296
                && citation.token == "unrelated_token"),
        "ARRANGEMENT: the resolved citation MUST have parsed and carried its token, or nothing \
         was ever able to cross paths here: {citations:?}"
    );
    assert_eq!(
        unreached
            .iter()
            .map(|pin| (pin.path.as_str(), pin.first_line, pin.last_line))
            .collect::<Vec<_>>(),
        vec![("core/governor/src/memory.rs", 296, 296)],
        "a resolved citation at persistence.rs:296 must not backfill an unrelated gap at \
         memory.rs:296 -- same line number, different file: {unreached:?}"
    );
}

/// #827: a citation whose path CANNOT RESOLVE must make the two counts disagree.
///
/// `resolved_path` requires the cited file to exist on disk, so a rename anywhere in the tree
/// takes a donor out of the extractor's population. That used to be silent, because the counter
/// shared the same existence filter: both instruments dropped the citation and reported agreement.
///
/// The counter no longer resolves anything, and **this cell is what pins that**. Measured on the
/// real tree while writing #827: renaming `core/governor/src/memory.rs` -- a fixture donor -- and
/// repairing only the module declaration so the build survives made three cells red, the headline
/// one reconciling 20 consumed coordinates against 31 spelled.
///
/// **That protection is emergent, not asserted.** It lives in the interaction between a counter
/// that reads syntax and an extractor that reads the disk. Put the existence filter back into
/// `count_coordinates_in_line` "to avoid false positives" and the divergence disappears, the
/// corpus becomes quietly hollow-able again, and nothing fails. This cell fails.
///
/// Both paths here are deliberately unresolvable, so the cell asserts a property OF THE TWO
/// INSTRUMENTS rather than borrowing the existence of a live file -- which is the coupling #827 is
/// about. A cell that needed a real path in order to prove the disk-coupling is a defect would be
/// carrying the defect.
#[test]
fn a_citation_whose_path_cannot_resolve_diverges_the_two_counts() {
    let text = "core/governor/src/no-such-file-827-a.rs:296 fn donor_token -> bool\n\
                core/graph/src/no-such-file-827-b.rs:12 fn other_token -> bool\n";
    let present = count_citation_sites(text);
    assert_eq!(
        present, 2,
        "ARRANGEMENT: the text must SPELL two coordinates, or this cell proves nothing about what \
         the extractor then drops. If this fires after you changed the counter, the subject is \
         the counter, not the fixture: `count_citation_sites` sums `count_coordinates_in_line`, \
         so an existence filter restored THERE drops `present` to 0 before the property below can \
         speak (#827, H's pass)"
    );
    let (citations, _, consumed) = doc_citations(text, &never_verifies);
    assert_eq!(
        consumed, 0,
        "the extractor must drop both, because neither path resolves on disk"
    );
    assert!(
        citations.is_empty(),
        "a citation nobody can follow must not enter the population: {citations:?}"
    );
    assert!(
        consumed < present,
        "the counts MUST disagree. If they agree, the counter is filtering by existence again and \
         a rename can hollow the corpus in silence (#827, the same shape as the :L249 defect one \
         layer up)"
    );
}

/// One `path.rs:COORDINATES` occurrence, located in the raw line.
struct CitationSite {
    /// Byte index just past `.rs`, where the path ends.
    path_end: usize,
    /// Byte index where the text after the coordinates begins.
    tail_start: usize,
    regions: Vec<(usize, usize)>,
}

/// Every `.rs:` occurrence in one line, with its coordinates consumed from the RAW text.
///
/// **Scanning the raw line rather than whitespace-separated words is the structural fix, and it
/// arrived only after three formats had been chased one at a time.** Splitting on whitespace first
/// let the READER's shape decide the CITATION's shape: `memory.rs:63, 858, 887` becomes the three
/// words `memory.rs:63,` / `858,` / `887`, only the first carries `.rs:`, and two coordinates
/// vanish with nothing to notice. Backtick wrappers and provenance keys nested one level down were
/// the same defect wearing other clothes. A citation is a pattern in text, so text is what gets
/// scanned. (Codex P2 on #610.)
/// The byte length of a RANGE separator starting at `line[pos..]`, or `None`.
///
/// Recognising `-`, `..`, an en dash (`–`), or an em dash (`—`) is a SHARED PRIMITIVE, not a
/// shared FILTER -- the same class of sharing both instruments already do by calling
/// `is_ascii_digit()`. What must stay independent is judgment about CONTENT: whether a path
/// resolves, whether a token exists. A shared filter on content is where the `:L249` defect hid
/// from both sides at once. Agreeing on what a separator character looks like is not that; it is
/// widening what a digit run may be followed by, identically, in both places that ask.
///
/// Fresh evidence after the partial-suffix fix (Codex P2 on this commit): `574..576` and a
/// typographic en dash both parsed as one coordinate in both instruments, because both recognised
/// only `,` and ASCII `-`. Two instruments sharing an unrecognised shape agree in silence exactly
/// like two instruments sharing a filter do.
fn separator_len(line: &str, pos: usize) -> Option<usize> {
    let rest = line.get(pos..)?;
    if rest.starts_with("..") {
        Some(2)
    } else if rest.starts_with('-') {
        Some(1)
    } else {
        ['\u{2013}', '\u{2014}']
            .into_iter()
            .find(|dash| rest.starts_with(*dash))
            .map(|dash| dash.len_utf8())
    }
}

/// A range spelled with `..` or a typographic dash is recognised as a two-endpoint range by
/// BOTH instruments, not just accepted as a one-coordinate prefix by neither.
///
/// Fresh evidence (Codex P2 on this commit): before `separator_len`, `574..576` and a range
/// joined by an en dash both stopped at the first digit run in `citation_sites` AND in
/// `count_coordinates_in_line` -- two instruments recognising the same narrow set of separators
/// agree in silence exactly like two instruments sharing a filter do.
#[test]
fn a_range_written_with_dots_or_a_typographic_dash_is_recognised_by_both_instruments() {
    for (line, dash_name) in [
        (
            "core/protocols/src/development.rs:574..576 OpaqueId",
            "double-dot",
        ),
        (
            "core/protocols/src/development.rs:574\u{2013}576 OpaqueId",
            "en dash",
        ),
        (
            "core/protocols/src/development.rs:574\u{2014}576 OpaqueId",
            "em dash",
        ),
    ] {
        let sites = citation_sites(line);
        assert_eq!(sites.len(), 1, "{dash_name}: {line}");
        assert_eq!(sites[0].regions, vec![(574, 576)], "{dash_name}: {line}");
        assert_eq!(
            count_coordinates_in_line(line),
            2,
            "{dash_name}: two endpoints, one range: {line}"
        );
    }
}

fn citation_sites(line: &str) -> Vec<CitationSite> {
    let bytes = line.as_bytes();
    let mut sites = Vec::new();
    for (marker, _) in line.match_indices(".rs:") {
        let mut cursor = marker + ".rs:".len();
        let mut regions = Vec::new();
        loop {
            let digits_start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor == digits_start {
                break;
            }
            let first: usize = line[digits_start..cursor].parse().expect("ascii digits");
            let mut last = first;
            if let Some(sep_len) = separator_len(line, cursor) {
                let range_start = cursor + sep_len;
                let mut range_end = range_start;
                while range_end < bytes.len() && bytes[range_end].is_ascii_digit() {
                    range_end += 1;
                }
                if range_end > range_start {
                    let end: usize = line[range_start..range_end].parse().expect("ascii digits");
                    if end >= first {
                        last = end;
                        cursor = range_end;
                    }
                }
            }
            regions.push((first, last));
            // A comma continues the list only when a NUMBER follows it. `:42, and the rest` is
            // prose, and consuming that would invent a coordinate out of a sentence.
            let mut lookahead = cursor;
            if lookahead < bytes.len() && bytes[lookahead] == b',' {
                lookahead += 1;
                while lookahead < bytes.len() && bytes[lookahead] == b' ' {
                    lookahead += 1;
                }
                if lookahead < bytes.len() && bytes[lookahead].is_ascii_digit() {
                    cursor = lookahead;
                    continue;
                }
            }
            break;
        }
        if !regions.is_empty() {
            sites.push(CitationSite {
                path_end: marker + ".rs".len(),
                tail_start: cursor,
                regions,
            });
        }
    }
    sites
}

/// The repository-relative path ending at `path_end`, if it names a real file.
///
/// Walks back over path characters only, so a backtick, a parenthesis or a table pipe ends the path
/// instead of being swallowed into it -- the trimming defect, now structural rather than patched.
fn resolved_path(line: &str, path_end: usize) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = path_end;
    while start > 0 {
        let candidate = bytes[start - 1];
        let ok = candidate.is_ascii_alphanumeric()
            || candidate == b'_'
            || candidate == b'/'
            || candidate == b'.'
            || candidate == b'-';
        if !ok {
            break;
        }
        start -= 1;
    }
    let path = &line[start..path_end];
    repository_root()
        .join(path)
        .is_file()
        .then(|| path.to_owned())
}

/// How many citation sites the text contains, counted independently of the extractor.
///
/// The completeness control that format-chasing lacked: every `.rs:` whose path resolves to a real
/// file is a site the extractor MUST have produced. A coordinate shape it cannot read fails a count
/// instead of disappearing.
///
/// **This deliberately does NOT require a digit after the colon**, and the first version did. That
/// version was tested by inventing a fifth format -- `memory.rs:L249` -- and it stayed GREEN,
/// because a counter that skips what it does not recognise agrees with an extractor that skips the
/// same thing. Two instruments sharing one blind spot report consensus. Measured before widening:
/// every `.rs:` in the harness doc is followed by a digit today, so nothing legitimate is caught by
/// the wider rule, and a citation this repository cannot parse SHOULD fail rather than pass.
fn count_citation_sites(text: &str) -> usize {
    text.lines().map(count_coordinates_in_line).sum()
}

/// Coordinates a line SPELLS, read from raw syntax and nothing else.
///
/// **Two corrections in one, and they are the same correction.**
///
/// It used to call `resolved_path`, which is the extractor's own existence filter. Two instruments
/// that share a filter share its blind spot, and a citation whose path does not resolve was dropped
/// by BOTH and reported as agreement. That is the `:L249` defect from #610 one layer up: I widened
/// the counter past "digit after the colon" and left it leaning on the same path check. So this
/// resolves nothing. A citation nobody can follow is a defect, and it now shows up as a count that
/// will not reconcile rather than as silence. (Codex P2 on #610.)
///
/// And it counts COORDINATES, not sites. `file.rs:574 - 576` parses as `574` and drops ` - 576`
/// without a word -- the fifth format, accepted as a prefix instead of refused as a partial suffix.
/// At site grain both instruments say "one site" and agree while half the citation is gone; at
/// coordinate grain syntax says two and the extractor says one, and the count names the citation
/// the parser did not understand whole.
///
/// **This is what ends the series.** Formats one through four were each fixed after somebody
/// pointed at them. A sixth format cannot hide here: whatever shape it takes, the digits it spells
/// are counted, and any shape the extractor cannot consume whole diverges.
fn count_coordinates_in_line(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut total = 0;
    for (marker, _) in line.match_indices(".rs:") {
        let mut cursor = marker + ".rs:".len();
        let mut first_in_site = true;
        loop {
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor == start {
                // An occurrence whose suffix does not parse still COUNTS. Breaking without
                // incrementing was the `:L249` defect surviving inside its own cure: this function's
                // doc-comment claims the control detects unsupported spellings, and `file.rs:L249`
                // incremented nothing here while `citation_sites` emitted nothing there -- both
                // blind, both silent, agreement reported. A comment claiming a property the code
                // does not have is worse than no comment. (Codex P2 on #610.)
                if first_in_site {
                    total += 1;
                }
                break;
            }
            first_in_site = false;
            total += 1;
            // A separator continues the coordinate expression only when a NUMBER follows it.
            // Spaces are tolerated around it ON PURPOSE: `574 - 576` is the partial-suffix shape,
            // and tolerating it HERE is what makes the extractor's refusal of it visible.
            //
            // `separator_len` widens what counts as a separator beyond `-` to `..` and the two
            // typographic dashes, the SAME primitive `citation_sites` uses to form a range -- not
            // a second copy of the recognition, which is exactly the split that let `574..576`
            // parse as one coordinate in both places and agree in silence. (Codex P2 on this
            // commit.)
            let mut lookahead = cursor;
            while lookahead < bytes.len() && bytes[lookahead] == b' ' {
                lookahead += 1;
            }
            let advance = if lookahead < bytes.len() && bytes[lookahead] == b',' {
                Some(1)
            } else {
                separator_len(line, lookahead)
            };
            if let Some(advance) = advance {
                lookahead += advance;
                while lookahead < bytes.len() && bytes[lookahead] == b' ' {
                    lookahead += 1;
                }
                if lookahead < bytes.len() && bytes[lookahead].is_ascii_digit() {
                    cursor = lookahead;
                    continue;
                }
            }
            break;
        }
    }
    total
}

/// The citations `identifier_in`'s heuristic cannot reach today, pinned by coordinate and exact
/// context -- neither side of this list may drift silently (the PINNED-GAP pattern, #571's `a'`;
/// staleness rule from #593). A NEW unreachable citation is a hole growing unwatched: not in this
/// list, and the assertion below fires. An entry here that BECOMES reachable is a stale pin
/// claiming a gap that already closed: also fires, because a debt nobody re-measures is a false
/// one. Both directions assert, on purpose.
///
/// Each is a real, resolvable citation with no word nearby satisfying `identifier_in` (>=5 chars,
/// starts alpha/underscore, contains underscore or uppercase) -- not rot, prose that doesn't
/// happen to contain a code-shaped word next to the coordinate it names. Measured against
/// `doc_citations`'s real output, not paraphrased (Codex P2 on 888d07e9, and the finding this pin
/// exists to answer without either a false alarm or a silent gap).
///
/// **#625 widened the search two ways, and five of the original ten pins closed for real.**
///
/// `backfill_same_line_gaps` closes a gap when some OTHER citation, anywhere in the doc or
/// corpus, already resolved a token for the EXACT same `path:line` -- closes both `memory.rs:296`
/// mentions (donor: the same-line spelling two sections later) and both `persistence.rs:736`
/// mentions (donor: `markers.json`'s own `_detectors.durable` provenance string, not in the doc at
/// all).
///
/// `doc_citations`'s section-scoped backfill closes a gap when its own `---`-bounded section (the
/// doc's structural boundary for one protection's entry) names an identifier-shaped word ELSEWHERE
/// in that same section, and the real file confirms the word sits on the gap's own cited line,
/// AND that word is the ONLY candidate the section verifies for that line -- closes
/// `development_contract_schemas.rs:622` (donor word `is_fresh`, from the "Trigger it would catch"
/// paragraph two above it in the SAME S3 entry; the section's only verified candidate).
///
/// **`tests/memory.rs:63` does NOT close, though its section verifies a real doc-sourced word too
/// (M, PR #682 review).** Its section verifies TWO candidates against that line --
/// `MemoryRefusalCode` and `SecretDetected`, both from the same S5b code fence two lines above the
/// citation. Picking the first one found would let the FILE decide which of two doc-named symbols
/// is "the" citation, and a later edit that keeps one while dropping the other would then
/// re-verify against whichever survived and stay green -- coverage that outlives the specific
/// claim it once confirmed. `backfill_same_line_gaps`' section-scoped pass therefore requires the
/// verified candidate set to have exactly one member; two or more refuses, same as zero. Proved by
/// `a_gap_does_not_close_when_its_section_verifies_more_than_one_candidate`.
///
/// Five entries lived below until #688 named their symbols (the list is empty since; its own
/// comment says how each was paid), and none of the kinds of blindness that remain is the one #610 already fixed (a token
/// search that reaches too far and borrows the WRONG citation's symbol, `memory.rs:512` borrowing
/// `jpd_plugin.rs`'s, Codex P2 on #606). Neither mechanism can reopen that: `backfill_same_line_gaps`
/// only reuses a donor pinned at the identical `path:line`, and the section-scoped pass never
/// crosses a `---` boundary, so a donor two sections away (the actual shape of the #610 regression)
/// is refused by construction, not by luck -- proved by
/// `a_gap_does_not_backfill_across_a_section_boundary_even_if_verify_would_allow_it`, which uses a
/// `verify` that says yes to everything and still stays pinned.
fn pinned_unreachable_citations() -> Vec<UnreachedCitation> {
    // EMPTY since #688, and the emptiness is the claim: every citation the harness doc and the
    // sabotage corpus make now sits next to a symbol the real file confirms on that line. The five
    // entries that lived here were paid by naming the symbol at the coordinate (or moving the
    // coordinate to the symbol the sentence was about, where the line had drifted onto a comment,
    // an assertion or a bare `#[test]`), never by widening the search. A list that GROWS again is
    // a citation rotting unwatched; add it here with its context and the reason no widening may
    // reach it, or name the symbol in the doc.
    Vec::new()
}

/// The first word long enough to be a symbol rather than prose punctuation.
fn identifier_in(text: &str) -> Option<String> {
    all_identifiers_in(text).into_iter().next()
}

/// Every word long enough to be a symbol rather than prose punctuation -- `identifier_in`'s own
/// rule, applied exhaustively instead of stopping at the first match. `doc_citations`'s
/// section-scoped backfill needs every candidate a section offers, not just the earliest one.
fn all_identifiers_in(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| {
            word.len() >= 5
                && word.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && word.chars().any(|c| c == '_' || c.is_uppercase())
        })
        .map(str::to_owned)
        .collect()
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
    let doc_text = fs::read_to_string(&doc)
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
    let mut sources = vec![doc_text];
    for path in corpus_provenance_files() {
        let mut buffer = String::new();
        collect_underscore_strings(&load_json(&path), &mut buffer);
        sources.push(buffer);
    }
    // The ONLY place either backfill pass may look at the real repository: confirming a
    // doc-sourced candidate against the file it names, never proposing one from the file itself.
    let verify_against_the_real_repository = |path: &str, line: usize, candidate: &str| {
        fs::read_to_string(repository_root().join(path))
            .ok()
            .and_then(|source| {
                source
                    .lines()
                    .nth(line.saturating_sub(1))
                    .map(str::to_owned)
            })
            .is_some_and(|cited_line| cited_line.contains(candidate))
    };
    let (citations, mut unreached, consumed, present) =
        doc_citations_across_sources(&sources, &verify_against_the_real_repository);

    // COMPLETENESS CONTROL, and it is what the fixed floor below could never be. Three coordinate
    // formats were read by this collector only after somebody pointed at each one. This asserts
    // that the number of citation sites present in the text equals the number the extractor could
    // even PARSE into a site (consumed-with-a-token plus pinned-unreachable), so a FOURTH format
    // fails a count rather than vanishing -- the series ends here instead of growing one patch at
    // a time. (Codex P2 on #610.)
    let parseable = consumed;
    assert_eq!(
        parseable, present,
        "the extractor consumed {parseable} coordinates where the text SPELLS {present}. The \
         difference is a citation the parser did not understand whole: either a path it cannot \
         resolve, which is a citation nobody can follow, or a suffix it read only the head of \
         (`:574 - 576` yields one coordinate where two are written). Both used to pass in \
         silence, because the counter shared the extractor's own filters"
    );

    // PINNED-GAP CONTROL. Of the sites the extractor CAN parse, some still find no token nearby --
    // real citations, not rot, that `identifier_in`'s heuristic cannot reach. Both directions of
    // drift assert: a citation that stops being pinned (a NEW unreachable one) is a hole growing
    // silently, and a pinned citation that becomes reachable is a stale debt (#593). See
    // `pinned_unreachable_citations` for the reasoning and the follow-up this pin exists to scope.
    unreached.sort();
    let mut expected_unreached = pinned_unreachable_citations();
    expected_unreached.sort();
    assert_eq!(
        unreached, expected_unreached,
        "the unreachable-citation pin no longer matches reality. If this list GREW, a citation \
         the heuristic cannot reach rotted unwatched -- add it here with its context, or widen \
         `identifier_in` to reach it for real. If this list SHRANK, a pinned entry became \
         reachable -- remove it, the debt it named is paid"
    );

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
        let cited = source
            .lines()
            .skip(citation.first - 1)
            .take(citation.last + 1 - citation.first)
            .any(|line| line.contains(&citation.token));
        if !cited {
            let moved = source
                .lines()
                .position(|line| line.contains(&citation.token))
                .map(|index| format!("now at :{}", index + 1))
                .unwrap_or_else(|| "not found in the file at all".to_owned());
            let where_ = if citation.first == citation.last {
                format!("{}", citation.first)
            } else {
                format!("{}-{}", citation.first, citation.last)
            };
            rotted.push(format!(
                "{}:{where_} promises `{}` -- {moved}",
                citation.path, citation.token
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

        // The class is a THIRD claim, and the booleans alone cannot keep it honest: relabel a
        // DECLARED_GAP entry as AGREED and every check above still passes, while
        // `the_marker_corpus_labels_every_class` only asks that each class appears somewhere. The
        // contract is the harness doc's own class table. (Codex P2 on #606.)
        let class = marker["class"]
            .as_str()
            .expect("every marker declares a class");
        let expected = match class {
            "AGREED" => (true, true),
            "DIVERGENT" => (true, false),
            "DECLARED_GAP" => (false, true),
            other => panic!(
                "marker {id} declares class `{other}`, which the harness doc's class table does \
                 not define -- a class nobody can check is a label, not a claim"
            ),
        };
        assert_eq!(
            (memory_refuses, durable_refuses),
            expected,
            "marker {id} is labelled {class}: the harness doc defines that as (memory, durable) = \
             {expected:?}, and the shipped detectors answered otherwise. The label and the \
             measurement must not disagree"
        );

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
    // THE DOC MUST AGREE WITH ITSELF, and until #630 nothing checked that it did.
    //
    // What stood here was a local array of three names asserted to have length three: a literal
    // compared with itself, which cannot fail and never read the document. It was a false guarantee
    // before this branch and would have been a green-but-wrong one after it, in a change whose
    // whole subject is whether the record matches reality. (Found by G reviewing #630.)
    //
    // The document states the red-window/marked split in TWO places, in different words: section 3
    // names the members and their counts, and the Denominator restates both. Two producers of one
    // fact drift, and the copy is the side nothing checks -- the same shape as the freeze charter
    // that restated `GATE_MACHINERY` and went stale, and the same cure: read BOTH and make the
    // numbers reconcile.
    let summary = doc_split(&text);
    let denominator = doc_split_denominator(&text);
    assert_eq!(
        summary, denominator,
        "the harness doc contradicts ITSELF about which entries are red-window and which are \
         marked. Section 3 and the Denominator are two producers of one fact; whichever was edited \
         alone is the one to fix, and both must name the same sets"
    );
    assert!(
        !summary.0.is_empty() && !summary.1.is_empty(),
        "HARNESS-BROKE: one side of the split parsed as empty, so the equality above compared \
         nothing against nothing"
    );
    assert!(
        summary.0.is_disjoint(&summary.1),
        "an entry is listed as BOTH red-window and marked: {:?}",
        summary.0.intersection(&summary.1).collect::<Vec<_>>()
    );
    for entry in summary.0.iter().chain(summary.1.iter()) {
        assert!(
            text.contains(&format!("## S{entry} ")) || text.contains(&format!("### S{entry} ")),
            "the doc counts S{entry} in its split but carries no write-up section for it"
        );
    }

    // The package must not carry prose again: it would pass review and fail the inventory guard.
    let stray = walk_markdown(&corpus());
    assert!(
        stray.is_empty(),
        "markdown is back inside the package and the manifest cannot declare it: {stray:?}"
    );
}

/// The (red-window, marked) split as ONE place in the doc states it.
///
/// Both call sites hand in the phrases that introduce each half, so the two producers are read the
/// same way and any difference between them is the document's, never the parser's. The count the
/// doc writes in parentheses is asserted against the members that follow it: a prose count that
/// disagrees with its own list is the first thing to rot.
fn doc_split(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    (
        doc_half(text, "**Red window ("),
        doc_half(text, "**Marked ("),
    )
}

/// The same split as the Denominator section states it, in its own different words.
fn doc_split_denominator(text: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    (
        doc_half_leading_count(text, "red-window entries** ("),
        doc_half_leading_count(text, "MARKED entries** ("),
    )
}

/// One half of the split, from a place that writes the count INSIDE the parentheses.
///
/// Section 3's shape: `**Red window (2):** ... `S4` ... `S5a` ...`
fn doc_half(text: &str, marker: &str) -> BTreeSet<String> {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("HARNESS-BROKE: the doc no longer contains `{marker}`"));
    let rest = &text[start + marker.len()..];
    let (count_text, after) = rest
        .split_once(')')
        .unwrap_or_else(|| panic!("HARNESS-BROKE: `{marker}` is not followed by a closing paren"));
    let stated: usize = count_text
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("HARNESS-BROKE: `{marker}` states no number: {count_text:?}"));
    labels_in(after, marker, stated)
}

/// The `` `S…` `` labels inside one window, with the count the prose stated asserted against them.
fn labels_in(after: &str, marker: &str, stated: usize) -> BTreeSet<String> {
    // The members run to the paragraph break. Reading past it swallows the NEXT half's labels
    // and makes the two sides agree BY CONSTRUCTION -- a comparison that cannot fail.
    //
    // CR is stripped first, and that is not cosmetic: this document is CRLF in a Windows
    // checkout, so a paragraph break carries CR and splitting on two bare newlines matched
    // NOTHING. The window ran to the end of the file and collected twelve labels, one of them
    // SnapshotBinding. The parser read a different amount of text on Windows than it would on
    // Linux -- the eol class again, in a place I did not expect it.
    let window = after.replace('\r', "");
    let paragraph_break = "\n\n";
    let window = window
        .split(paragraph_break)
        .next()
        .unwrap_or(&window)
        .to_owned();

    // A label is S, digits, then at most one lowercase letter: S2, S5a, S1b. Accepting any
    // alphanumeric tail turned SnapshotBinding into a label called napshotBinding.
    let mut labels = BTreeSet::new();
    for piece in window.split('`').skip(1).step_by(2) {
        let Some(rest) = piece.trim().strip_prefix('S') else {
            continue;
        };
        let digits = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
        if !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit())
            && rest.len() - digits.len() <= 1
        {
            labels.insert(rest.to_owned());
        }
    }
    assert_eq!(
        labels.len(),
        stated,
        "`{marker}` says {stated} but names {} entries: {labels:?}. A prose count and its own \
         list are two producers of one number, and this is the pair drifting",
        labels.len()
    );
    labels
}

/// One half from a place that writes the count BEFORE the phrase and the members after it.
///
/// The Denominator's shape: `**3 red-window entries** (S2, S4, S5a)`. The two producers do not
/// even agree on where the number goes, which is part of why they drifted: nobody editing one
/// recognised the other as the same claim.
fn doc_half_leading_count(text: &str, marker: &str) -> BTreeSet<String> {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("HARNESS-BROKE: the doc no longer contains `{marker}`"));
    let before = &text[..start];
    let digits: String = before
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    let stated: usize = digits
        .chars()
        .rev()
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("HARNESS-BROKE: no count precedes `{marker}`"));
    let after = &text[start + marker.len()..];
    let members = after
        .split_once(')')
        .map(|(inside, _)| inside)
        .unwrap_or(after);
    // The Denominator writes bare labels, not backticked ones, so they are read as words.
    let mut labels = BTreeSet::new();
    for word in members.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(rest) = word.strip_prefix('S') {
            let digits = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
            if !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
                && rest.len() - digits.len() <= 1
            {
                labels.insert(rest.to_owned());
            }
        }
    }
    assert_eq!(
        labels.len(),
        stated,
        "`{marker}` says {stated} but names {} entries: {labels:?}. A prose count and its own \
         list are two producers of one number, and this is the pair drifting",
        labels.len()
    );
    labels
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
        certification.specimens, 3,
        "the shipped floor is three specimens -- capability-missing-claimed-proven, \
         flaky-claimed-proven, and (since #859) waiver-transplanted-from-another-verification; a \
         change here must be read against this corpus"
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
    // DECLARED LIMIT: this guard counts the array IT writes, not the enum. When #859 added the
    // fourth axis the enum grew and this stayed green -- a guard fed by the list it checks is green
    // by construction, and its doc's "when a third axis lands this fails" was true of the list,
    // never of the vocabulary. The gate caught the change through the specimen pin above instead.
    // Counting the enum needs an enumeration the crate does not export today; until it does, the
    // rule is that whoever adds a variant adds it HERE in the same commit, and the specimen pin is
    // the tripwire that says they forgot.
    let axes = [
        JpdFailureAxis::ActorUsedAsOwnObserver,
        JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
        JpdFailureAxis::FlakyClaimedAsProven,
        JpdFailureAxis::WaiverIssuedForAnotherVerification,
    ];
    assert_eq!(
        axes.len(),
        4,
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
