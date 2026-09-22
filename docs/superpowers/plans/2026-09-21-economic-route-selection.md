# Economic Route Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Select model routes by the lowest measured or explicitly estimated dollars per successfully proven action while preserving deterministic safety, quality, quota, and structured-output constraints.

**Architecture:** Add a closed, provenance-carrying cost observation at the gateway call boundary, normalize it into a protocol-owned attempt receipt, and rank only routes that already pass deterministic eligibility. Runtime maps adapter attempts into that protocol receipt; a trusted proof authority supplies an opaque verified proof before the receipt can join outcome evidence. Cost can be measured, estimated, or unknown; unknown is never treated as zero. Jev may provide a calibrated success-probability classification among an allowlisted set with abstention and bounded escalation, but deterministic code owns eligibility, cost arithmetic, proof joins, and all fallback behavior.

**Tech Stack:** Rust 1.97.1, serde JSON, existing `core/gateway`, `adapters/model-gateway`, `core/runtime`, `core/architect`, offline Rust tests, checked-in JSON/YAML contracts.

**Spec:** `docs/superpowers/specs/2026-09-21-graphhelm-methodology-adoption-design.md`

## Global Constraints

- Documentation in this repository is English.
- Core crates depend on interfaces, never concrete adapters; provider SDKs and credentials remain in adapter crates.
- `D-054` remains authoritative: Jev judgments may enter only as diagnostics, fixed ranking, or reports; a new routing role requires an ADR or RFC and cannot silently broaden the judgment contract.
- Deterministic code enforces schemas, permissions, policy, quota state, structured output, context limits, and proven-action gates.
- Unknown monetary cost is a refusal for a dollar-per-proven-action claim, never a zero-cost value.
- Subscription accounting distinguishes allocated cost, marginal cost, and quota consumption; no current provider prices are embedded or asserted.
- No automatic paid fallback is added when subscription capacity is exhausted; exhaustion pauses and requires the existing manual route decision.
- `ci/gate.ps1` is the authoritative local gate. Hosted GitHub Actions remain disabled, and no live paid calls are used.
- Tests remain offline, browser-free, credential-free, Docker-free, and production-free.
- Issue-first execution is required; implementation uses the assigned issue branch for this methodology adoption work. It must not reuse the Foundation Graph Kernel branch unless the issue explicitly assigns that branch.
- Before tests, verify the target-drive floors and the two-gate ceiling. Use `E:/_agent-scratch/graphhelm/methodology-adoption-economics/target` on the same line as every cargo command, for example: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --locked`.

## Review Focus

- A provider omits usage or reports only partial cache usage: the attempt receipt remains incomplete and route ranking refuses a cost claim. Test in Task 1 and Task 3.
- A subscription route has a quota estimate but no marginal monetary price: the record labels allocated, marginal, and quota values separately and never makes them interchangeable. Test in Task 1 and Task 2.
- A nanounit amount overflows or rounds differently across platforms: checked `i128` rational arithmetic and an explicit rounding rule refuse unsafe values. Test in Task 1.
- Jev abstains, returns an out-of-vocabulary class, or asks for more than the escalation bound: the router uses the deterministic baseline and records the abstention. Test in Task 4.
- A cheap route has lower proof probability, insufficient calibration support, or violates a hard policy: deterministic eligibility and support gates remove it before ranking, and cost cannot compensate for the refusal. Test in Task 4.

---

### Task 1: Define the cost and route-economics contracts

**Files:**
- Create: `core/protocols/src/economics.rs`
- Modify: `core/protocols/src/lib.rs`
- Modify: `core/gateway/src/call.rs:22-46` (`ModelCall`, `ModelReply`, `Usage`)
- Modify: `core/gateway/src/manifest.rs:120-224` (`ModelRoute`, `RouteManifest`)
- Modify: `core/gateway/src/lib.rs` to export the new contract module
- Create: `core/gateway/src/economics.rs`
- Test: `core/gateway/tests/economics_wire.rs`
- Test: `core/gateway/tests/source_invariants.rs`

**Interfaces:**
- Consumes: existing `Usage`, `ModelRoute`, `BillingMode`, `Transport`, and serde wire conventions.
- Produces: protocol-owned `CostGrade`, `CostBasis`, `SubscriptionCostKind`, `FixedMoney`, `UsageBreakdown`, `RateCardRef`, `CostObservation`, and `EconomicAttemptReceipt`; `core/gateway` re-exports the cost types and never becomes their authority.
- Exact signatures:

```rust
pub enum CostGrade { Measured, Estimated, Unknown }
pub enum CostBasis { ProviderSettled, RateCardDerived, QuotaAllocation, Unavailable }
pub enum SubscriptionCostKind { Allocated, Marginal, Quota }
pub const NANOUNITS_PER_UNIT: u64 = 1_000_000_000;
pub enum RoundingRule { Floor, Ceil, NearestEven }
pub struct FixedMoney { pub currency: CurrencyCode, pub nano_units: u64, pub rounding: RoundingRule }
pub struct CurrencyCode(String);
pub trait CurrencyRegistry { fn accepts(&self, code: &str) -> bool; }
pub struct UsageBreakdown {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}
pub struct RateCardRef { pub id: String, pub version: String }
pub struct CostObservation {
    pub grade: CostGrade,
    pub basis: CostBasis,
    pub amount: Option<FixedMoney>,
    pub subscription_kind: Option<SubscriptionCostKind>,
    pub usage: UsageBreakdown,
    pub rate_card: Option<RateCardRef>,
    pub note: String,
}
pub struct RouteEconomics {
    pub rate_card: Option<RateCardRef>,
    pub subscription_allocated_nano_units: Option<u64>,
    pub subscription_marginal_nano_units: Option<u64>,
    pub subscription_quota_units: Option<u64>,
}
```

- [ ] **Step 1: Write the failing wire tests.** Assert camelCase JSON, fixed `nanoUnits`, explicit nullable fields, all three grades, all three subscription kinds, and rejection by an injected `CurrencyRegistry` when a rate card does not recognize a code. Do not treat three uppercase letters as a currency registry. Assert a missing observation is represented as `grade: "unknown"` with `amount: null`.

```rust
#[test]
fn cost_observation_round_trips_without_turning_unknown_into_zero() {
    let observation = CostObservation::unknown(UsageBreakdown::default(), "provider omitted cost");
    let value = serde_json::to_value(&observation).unwrap();
    assert_eq!(value["grade"], "unknown");
    assert!(value["amount"].is_null());
    assert_eq!(serde_json::from_value::<CostObservation>(value).unwrap(), observation);
}

#[test]
fn fixed_money_uses_checked_nanounits() {
    assert!(FixedMoney::new("USD", u64::MAX, &usd_registry(), RoundingRule::NearestEven).is_ok());
    assert!(FixedMoney::from_rate("USD", 1, 3, &usd_registry(), RoundingRule::NearestEven).is_ok());
    assert!(FixedMoney::from_rate("USD", u128::MAX, 1, &usd_registry(), RoundingRule::NearestEven).is_err());
}

struct TestCurrencyRegistry;
impl CurrencyRegistry for TestCurrencyRegistry {
    fn accepts(&self, code: &str) -> bool { code == "USD" }
}
fn usd_registry() -> TestCurrencyRegistry { TestCurrencyRegistry }
```

- [ ] **Step 2: Run the focused tests and confirm they fail.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test economics_wire --locked`

Expected: FAIL because the economics types and constructors do not exist.

- [ ] **Step 3: Implement the closed types and constructors.** Store nonnegative currency values as nanounits (`1e9` per currency unit). Convert rate-card rational values through checked `i128` intermediates and apply the declared rounding rule exactly once. Validate a `CurrencyRegistry` at construction time; lexical shape alone is insufficient. Use `Option` for unobserved fields. Keep one cost authority in `ModelAttempt.cost` from Task 2; do not duplicate a separately mutable cost in `ModelReply`. Preserve `ModelReply` compatibility. A provider-settled amount is `Measured`; a rate-card calculation is `Estimated`, even when usage is complete.

The exact constructors are `FixedMoney::new(code: &str, nano_units: u64, registry: &dyn CurrencyRegistry, rounding: RoundingRule) -> Result<FixedMoney, MoneyError>` and `FixedMoney::from_rate(code: &str, numerator: u128, denominator: u128, registry: &dyn CurrencyRegistry, rounding: RoundingRule) -> Result<FixedMoney, MoneyError>`. The rational input is currency units; multiply by `NANOUNITS_PER_UNIT` with checked arithmetic before rounding. `MoneyError` is `UnknownCurrency | ZeroDenominator | Overflow`. `CurrencyCode` remains private and cannot bypass registry validation. Derive Clone/Debug/PartialEq/Eq plus camelCase Serde for wire structs; `UsageBreakdown` also derives Default.

The minimal unknown constructor is:

```rust
impl CostObservation {
    pub fn unknown(usage: UsageBreakdown, note: &str) -> Self {
        Self { grade: CostGrade::Unknown, basis: CostBasis::Unavailable, amount: None,
            subscription_kind: None, usage, rate_card: None, note: note.to_owned() }
    }
}
```

For tests below, `CostObservation::measured(usage, nano_units, currency)` and `CostObservation::estimated(usage, nano_units, rate_card_id, version)` are **local test helper functions**, not provider evidence constructors. Replace these calls with full struct literals inside the owning test module: measured uses `ProviderSettled`, estimated uses `RateCardDerived` plus `RateCardRef`, both carry a `FixedMoney` validated by `TestCurrencyRegistry`. Production parsers receive actual provider settlement or the supplied rate card, never a helper's hard-coded currency. Every cost receipt also contains separate `subscription_allocated: Option<FixedMoney>`, `subscription_marginal: Option<FixedMoney>`, and `subscription_quota_units: Option<u64>` fields; initialize all three to None in non-subscription fixtures. `amount` is the explicitly chosen comparison basis, never a sum of allocated and marginal values.

- [ ] **Step 4: Add structural guards.** Test that `RouteEconomics` is optional in a manifest, that its rate-card id and version are required together, that allocation, marginal, and quota fields remain independent, and that no manifest field can contain a floating-point price. Secret filtering remains an adapter/runtime responsibility; this contract only rejects malformed economics data.

- [ ] **Step 5: Run focused validation.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test economics_wire --locked`

Expected: PASS.

- [ ] **Step 6: Commit the contract slice.**

```text
git add core/protocols/src/economics.rs core/protocols/src/lib.rs core/gateway/src/call.rs core/gateway/src/economics.rs core/gateway/src/lib.rs core/gateway/src/manifest.rs core/gateway/tests/economics_wire.rs core/gateway/tests/source_invariants.rs
git commit -m "feat(gateway): define cost observation contract"
```

### Task 2: Produce per-attempt adapter cost observations

**Files:**
- Modify: `adapters/model-gateway/src/byok.rs:38-191` (`ByokAdapter::call`, provider parsers)
- Modify: `adapters/model-gateway/src/systemone.rs:24-84` (`SystemOneAdapter::call`)
- Modify: `adapters/model-gateway/src/runtime.rs:90-280` (`RuntimeAdapter::call`, runtime parsing)
- Modify: `adapters/model-gateway/src/lib.rs` for adapter-facing rate-card and quota interfaces
- Test: `adapters/model-gateway/tests/byok_adapters.rs`
- Test: `adapters/model-gateway/tests/systemone_adapter.rs`
- Test: `adapters/model-gateway/tests/runtime_adapters.rs`

**Interfaces:**
- Consumes: `CostObservation`, `UsageBreakdown`, `RouteEconomics`, provider response usage, and existing `GatewayError` mappings.
- Produces: an explicit `ModelAttempt` envelope for both successful and failed calls; no adapter loses a cost observation because the provider call returned an error.
- Exact signatures:

```rust
pub struct ModelAttempt {
    pub result: Result<ModelReply, GatewayError>,
    pub cost: CostObservation,
}

pub trait CostObserver: Send + Sync {
    fn observe(
        &self,
        route: &ModelRoute,
        usage: UsageBreakdown,
        response: &ProviderResponseMetadata,
    ) -> CostObservation;
}
pub struct ProviderResponseMetadata {
    pub status: Option<u16>,
    pub rate_card: Option<RateCardRef>,
    pub quota_delta: Option<u64>,
}
pub fn observe_subscription_cost(
    economics: &RouteEconomics,
    usage: UsageBreakdown,
    quota_delta: Option<u64>,
) -> CostObservation;
```

- [ ] **Step 1: Add failing adapter tests.** Define a local fake transport that returns a response or transport error, then call the adapter through the new `ModelAttempt` return type. Assert a provider-settled amount is `Measured` only when the provider supplies it; a rate-card calculation is `Estimated`; missing usage or rate card produces `Unknown`; subscription responses carry `Allocated`, `Marginal`, or `Quota` independently and never convert quota units into dollars; and a failed call still carries its observation envelope.

```rust
#[test]
fn failed_attempt_can_preserve_unknown_cost_without_inventing_usage() {
    let attempt = ModelAttempt {
        result: Err(GatewayError::ProviderUnavailable),
        cost: CostObservation::unknown(UsageBreakdown::default(), "transport failed before usage"),
    };
    assert!(attempt.result.is_err());
    assert_eq!(attempt.cost.grade, CostGrade::Unknown);
    assert!(attempt.cost.amount.is_none());
}
```

This wire test is necessary but is not adapter proof. Extend the existing fake HTTP response helpers in each named adapter test file to exercise the actual provider parser and call path; do not replace the adapter with a fake that returns its own expected grade. Add both a failed response with known billed usage (estimated from the versioned rate card) and a pre-response transport failure with no usage (unknown). Native-runtime failure output must be accounted for before returning the existing error.


- [ ] **Step 2: Run adapter tests and confirm failure.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-model-gateway --tests --locked`

Expected: FAIL because adapters do not construct cost observations.

- [ ] **Step 3: Implement BYOK observations.** Parse every provider usage field already available, preserve cache-read and cache-write categories, and apply a supplied immutable rate-card version through checked nanounit arithmetic. A provider-settled bill is `Measured`; a rate-card result is `Estimated`; return `Unknown` with a reason when any required unit is absent.

- [ ] **Step 4: Implement System One and native-runtime observations.** Preserve typed Jev output and native subscription isolation. System One uses the same observer seam; native runtime records independently supplied allocated, marginal, or quota observations, otherwise `Unknown`.

- [ ] **Step 5: Preserve failure semantics.** A transport, quota, malformed-output, timeout, or cancellation error remains an `Err` in `ModelAttempt.result`; the attempt envelope always carries the observed/estimated/unknown cost. No adapter or model reply can mark an action proven.

- [ ] **Step 6: Run focused tests and commit.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-model-gateway --tests --locked`

Expected: PASS.

```text
git add adapters/model-gateway
git commit -m "feat(model-gateway): record per-attempt economic observations"
```

### Task 3: Define protocol-owned attempt receipts and persist them from runtime

**Files:**
- Modify: `core/runtime/src/executor.rs:124-132,209-245,589-605` (`WorkSummary`, `cognitive_outcome`, `cognitive_work`)
- Modify: `core/runtime/src/context_accounting.rs:302-484` (`ExecutionAccountingReceipt`)
- Modify: `core/runtime/src/driver.rs:54-221` (execution receipt publication)
- Test: `core/protocols/src/economics.rs` unit tests (private proof-token constructors stay in their owning module)
- Test: `core/runtime/tests/context_accounting.rs`
- Test: `core/runtime/tests/driver_contract.rs`

**Interfaces:**
- Consumes: `ModelAttempt`, execution-start binding, and a `VerifiedProof` minted by the trusted proof authority.
- Produces: a protocol-owned attempt receipt that records cost independently from proof and can join proof only through an opaque trusted value.
- Exact signatures:

```rust
pub struct EconomicAttemptReceipt {
    pub execution_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub route_id: String,
    pub cost: CostObservation,
    pub usage: UsageBreakdown,
    pub proof: Option<VerifiedProof>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofBinding {
    pub execution_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub contract_digest: String,
    pub gate_evidence_digest: String,
}

pub struct VerifiedProof { binding: ProofBinding }
#[derive(Debug, PartialEq, Eq)]
pub enum EconomicReceiptError { CostUnavailable, BindingMismatch, Overflow }
#[derive(Debug)]
pub enum ProofRefusal { Missing, Invalid, Stale }
pub trait ProofAuthority: Send + Sync {
    fn lookup_verified(&self, binding: &ProofBinding) -> Result<VerifiedProof, ProofRefusal>;
}
// Production authority and token construction must share the owning module or
// use a protocol-owned checked constructor with a trusted receipt lookup port.
// No deserialized token or model-provided success flag is accepted.

impl VerifiedProof {
    #[cfg(test)]
    fn from_test_authority(binding: ProofBinding) -> Self { Self { binding } }
}

impl EconomicAttemptReceipt {
    fn fixture_unknown() -> Self {
        Self { execution_id: "e1".into(), node_id: "n1".into(), attempt: 1, route_id: "fixture".into(), cost: CostObservation::unknown(UsageBreakdown::default(), "fixture"), usage: UsageBreakdown::default(), proof: None }
    }
    fn fixture_measured() -> Self {
        Self { execution_id: "e1".into(), node_id: "n1".into(), attempt: 1, route_id: "fixture".into(), cost: CostObservation { grade: CostGrade::Measured, basis: CostBasis::ProviderSettled, amount: Some(FixedMoney::new("USD", 1, &usd_registry(), RoundingRule::NearestEven).unwrap()), subscription_kind: None, usage: UsageBreakdown::default(), rate_card: None, note: "fixture settlement".into() }, usage: UsageBreakdown::default(), proof: None }
    }
    fn cost(&self) -> &CostObservation { &self.cost }
    fn is_proven(&self) -> bool { self.proof.is_some() }
    fn total_money(&self) -> Result<u64, EconomicReceiptError> {
        if self.cost.grade == CostGrade::Unknown { return Err(EconomicReceiptError::CostUnavailable); }
        self.cost.amount.as_ref().map(|money| money.nano_units).ok_or(EconomicReceiptError::CostUnavailable)
    }
}

impl EconomicAttemptReceipt {
    pub fn attach_verified_proof(
        &self,
        proof: VerifiedProof,
    ) -> Result<Self, EconomicReceiptError>;
}
```

- [ ] **Step 1: Write failing receipt tests.** Assert that measured, estimated, and unknown costs round-trip with their grade and basis; a failed model attempt is still recorded; no model-call result is marked proven; and a receipt can become proven only after a separately verified gate-evidence binding joins on execution, node, attempt, contract digest, and evidence digest.

```rust
#[test]
fn unknown_cost_stays_unknown_and_failed_attempt_is_not_proven() {
    let receipt = EconomicAttemptReceipt::fixture_unknown();
    assert_eq!(receipt.cost().grade, CostGrade::Unknown);
    assert!(receipt.total_money().is_err());
}

#[test]
fn model_attempt_needs_independent_gate_evidence_to_become_proven() {
    let receipt = EconomicAttemptReceipt::fixture_measured();
    assert!(!receipt.is_proven());
    let proof = test_authority().lookup_verified(&ProofBinding { execution_id: "e1".into(), node_id: "n1".into(), attempt: 1, contract_digest: "sha256:c".into(), gate_evidence_digest: "sha256:g".into() }).unwrap();
    let joined = receipt.attach_verified_proof(proof).unwrap();
    assert!(joined.is_proven());
}

fn test_authority() -> TestProofAuthority { TestProofAuthority }
struct TestProofAuthority;
impl ProofAuthority for TestProofAuthority {
    fn lookup_verified(&self, binding: &ProofBinding) -> Result<VerifiedProof, ProofRefusal> {
        Ok(VerifiedProof::from_test_authority(binding.clone()))
    }
}
```

- [ ] **Step 2: Run the focused test and confirm failure.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-runtime --test context_accounting --locked`

Expected: FAIL because `WorkSummary` and the receipt have no cost or proof fields.

- [ ] **Step 3: Map runtime attempts into the protocol-owned `EconomicAttemptReceipt` from Task 1.** Add named fields for cost grade, basis, currency, nanounits, rate-card reference, usage breakdown, independent subscription fields, and optional proof binding. Keep unavailable fields explicit. `VerifiedProof` has private fields and no `Deserialize`; expose `VerifiedProof::resolve(authority: &dyn ProofAuthority, binding: &ProofBinding)` in its owning protocol module, which invokes the trusted injected authority lookup and validates exact identity before construction. Test-only constructors and receipt fixtures live in that module's unit tests, never as inherent implementations on foreign types in integration tests; `attach_verified_proof` accepts that opaque value, never public JSON supplied by the model or caller. Do not serialize a model-call success as a proven outcome.

- [ ] **Step 4: Wire every cognitive outcome.** `cognitive_outcome` copies the adapter observation into a protocol attempt receipt and keeps the model result unproven. Judge failures, empty replies, gateway errors, retries, and interruptions remain attempt outcomes. Runtime asks the trusted proof authority to look up a `VerifiedProof`; only that opaque value may join gate evidence to the attempt receipt. `core/gateway` never imports `core/runtime`.

- [ ] **Step 5: Add completeness checks.** Implement `total_money()` as a checked operation that returns a typed incomplete-cost error if any included attempt is unknown or if currencies/rate-card versions cannot be reconciled.

- [ ] **Step 6: Run focused runtime tests and commit.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-runtime --test context_accounting --test driver_contract --locked`

Expected: PASS.

```text
git add core/runtime/src/executor.rs core/runtime/src/context_accounting.rs core/runtime/src/driver.rs core/runtime/tests/context_accounting.rs core/runtime/tests/driver_contract.rs
git commit -m "feat(runtime): persist economic attempt receipts"
```

### Task 4: Add deterministic cost-per-proven-result route ranking and bounded Jev input

**Files:**
- Modify: `core/gateway/src/eligibility.rs:13-59` (`Requirements`, `eligible_routes`)
- Create: `core/gateway/src/ranking.rs`
- Modify: `core/gateway/src/lib.rs`
- Modify: `core/architect/src/judge.rs:14-21` only if the approved ADR/RFC adopts the optional classifier port
- Test: `core/gateway/tests/capacity_mapping.rs`
- Test: `core/gateway/tests/economics_ranking.rs`
- Test: `core/architect/tests/judgment_ranking.rs` only for the explicitly approved classifier boundary

**Interfaces:**
- Consumes: eligible `ModelRoute`s, `CostObservation`, deterministic policy checks, proven-action history, and an optional classifier result.
- Produces: stable route ordering and an abstaining classifier seam; no route is selected outside eligibility.
- Exact signatures:

```rust
pub struct ProvenRate {
    pub proven: u64,
    pub attempts: u64,
    pub lower_bound_ppm: u64,
    pub calibration_support: u64,
}
pub struct RouteCandidate<'a> {
    pub route: &'a ModelRoute,
    pub cost: CostObservation,
    pub proven_rate: ProvenRate,
}
pub struct RouteRank {
    pub route_id: String,
    pub expected_nanounits_per_proven: Option<u64>,
    pub grade: CostGrade,
    pub abstained: bool,
}
pub fn rank_by_cost_per_proven_result<'a>(
    candidates: &'a [RouteCandidate<'a>],
) -> Vec<RouteRank>;

pub enum ClassifierRefusal { Abstained, InvalidOutput, BudgetExhausted }
pub trait AllowedSuccessClassifier: Send + Sync {
    fn classify(&self, input: &SuccessClassificationInput)
        -> Result<SuccessClass, ClassifierRefusal>;
}
pub enum SuccessClass { ProvenProbabilityLow, ProvenProbabilityMedium, ProvenProbabilityHigh }
pub struct SuccessClassificationInput { pub profile: WorkProfile, pub route_ids: Vec<String> }
fn manifest() -> RouteManifest {
    RouteManifest::from_json(r#"{"manifestVersion":1,"routes":[{"id":"fixture","provider":"openai","transport":"direct_api","authentication":"api_key","billingMode":"per_token","baseUrl":"http://127.0.0.1:1","model":"fixture-model","credentialRef":"fixture-key","profiles":["balanced_reasoning"],"enabled":true}]}"#).unwrap()
}
fn health() -> HashMap<String, RouteHealth> { HashMap::from([("fixture".into(), RouteHealth::Available)]) }
fn requirements() -> Requirements { Requirements { profile: WorkProfile::BalancedReasoning, subscription_only: false } }
fn candidates_for<'a>(routes: Vec<&'a ModelRoute>) -> Vec<RouteCandidate<'a>> {
    routes.into_iter().map(|route| RouteCandidate { route, cost: CostObservation::unknown(UsageBreakdown::default(), "fixture"), proven_rate: ProvenRate { proven: 0, attempts: 0, lower_bound_ppm: 0, calibration_support: 0 } }).collect()
}
fn candidate_with_support(proven: u64, attempts: u64, lower_bound_ppm: u64) -> RouteCandidate<'static> {
    let route = Box::leak(Box::new(manifest())).routes().first().expect("fixture route");
    RouteCandidate { route, cost: CostObservation { grade: CostGrade::Estimated, basis: CostBasis::RateCardDerived, amount: Some(FixedMoney::new("USD", 1, &usd_registry(), RoundingRule::NearestEven).unwrap()), subscription_kind: None, usage: UsageBreakdown::default(), rate_card: Some(RateCardRef { id: "fixture".into(), version: "1".into() }), note: "fixture estimate".into() }, proven_rate: ProvenRate { proven, attempts, lower_bound_ppm, calibration_support: attempts } }
}
fn allowed_route_ids() -> BTreeSet<String> { ["fixture".to_owned()].into_iter().collect() }
```

- [ ] **Step 1: Write failing deterministic ranking tests.** Assert that ineligible routes never reach ranking, an unknown-cost route is marked non-actionable, a higher per-call cost can be cheaper per proven result only when its calibrated lower-bound success estimate and support threshold justify it, a `1/1` observation is rejected as insufficient calibration support, ties are stable by route id, and no paid fallback is synthesized.

```rust
#[test]
fn hard_eligibility_precedes_cost_and_unknown_cost_cannot_win() {
    let manifest = manifest();
    let health = health();
    let requirements = requirements();
    let eligible = eligible_routes(&manifest, &health, &requirements);
    let candidates = candidates_for(eligible);
    let ranked = rank_by_cost_per_proven_result(&candidates);
    assert!(ranked.iter().all(|row| allowed_route_ids().contains(&row.route_id)));
    assert!(ranked.iter().find(|row| row.grade == CostGrade::Unknown).unwrap().abstained);
}

#[test]
fn one_success_from_one_attempt_cannot_drive_route_choice() {
    let candidate = candidate_with_support(1, 1, 1_000_000);
    let ranked = rank_by_cost_per_proven_result(&[candidate]);
    assert!(ranked[0].abstained);
}
```

- [ ] **Step 2: Run the focused tests and confirm failure.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test economics_ranking --locked`

Expected: FAIL because the ranking function and classifier boundary are absent.

- [ ] **Step 3: Implement deterministic arithmetic.** Use checked nanounit arithmetic and the calibrated lower confidence bound, never a raw observed winner. Compute expected nanounits per proven result only when cost is measured or explicitly estimated, calibration support is at least 30 held-out observations in that profile (an initial policy floor, not a statistical guarantee), the lower bound is nonzero, and all hard checks passed. Preserve unknown and abstention as report values rather than coercing them into a score.

- [ ] **Step 4: Add the optional Jev classifier behind the D-054 decision.** Before implementation, land the required ADR/RFC defining this as a bounded routing report/ranking signal. Calibrate it on one frozen calibration set and evaluate it on a separate held-out set. The classifier may return only the three closed `SuccessClass` values or abstain, and must carry sample support and an uncertainty bound; a raw `1/1` winner is never actionable. It receives route ids and profile metadata, never credentials or unrestricted graph authority. Escalation is capped at two classifier calls total and the explicitly configured model-route allowlist; include those calls in total cost. Fix the confidence method and quality floor in the versioned calibration manifest before the experiment. A refusal falls back to deterministic historical rates; it never triggers a paid fallback.

- [ ] **Step 5: Run focused tests and commit.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test capacity_mapping --test economics_ranking --locked`

Expected: PASS.

```text
git add core/gateway/src/eligibility.rs core/gateway/src/ranking.rs core/gateway/src/lib.rs core/gateway/tests/capacity_mapping.rs core/gateway/tests/economics_ranking.rs core/architect/src/judge.rs core/architect/tests/judgment_ranking.rs docs/DECISION_REGISTER.md
git commit -m "feat(gateway): rank routes by proven-result cost"
```

### Task 5: Validate the pilot methodology and acceptance contract

**Files:**
- Create: `core/gateway/src/economic_pilot.rs`
- Create: `core/gateway/tests/economic_pilot.rs`
- Create: `docs/acceptance/economic-route-selection-recipe.md`
- Create: `extensions/builtin/graphhelm-development-contracts/evaluators/economic-efficiency.yaml`
- Create: `extensions/builtin/graphhelm-development-contracts/schemas/economic-efficiency-evaluator.schema.json`
- Modify: `extensions/builtin/graphhelm-development-contracts/extension.json` (contribution inventory and SHA-256 entries)
- Test: `apps/cli/tests/development_contract_schemas.rs`
- Test: `core/gateway/tests/economic_pilot.rs`

**Interfaces:**
- Consumes: execution accounting receipts, deterministic proof outcomes, frozen task corpus digest, route configuration, and the existing token-efficiency refusal vocabulary.
- Produces: an offline-verifiable pilot report whose primary metric is total measured or explicitly estimated nanounits divided by independently proven actions. The report also gives cost-estimation coverage; a rate-card estimate never claims literal provider funds were charged.
- Exact helper signatures used by the recipe tests:

```rust
pub struct PilotArm { pub route_id: String, pub label: String }
pub struct PilotCase { pub id: String, pub corpus_digest: String, pub profile: WorkProfile }
pub struct PilotObservation { pub case_id: String, pub arm: PilotArm, pub receipt: EconomicAttemptReceipt }
pub struct PilotReport {
    pub total_cost_nanounits: Option<u64>,
    pub proven_actions: u64,
    pub cost_per_proven_action_nanounits: Option<u64>,
    pub estimated_cost_coverage: f64,
    pub refusal: Option<String>,
}
pub fn evaluate_pilot(
    baseline: &[PilotObservation],
    candidate: &[PilotObservation],
    expected_corpus_digest: &str,
) -> PilotReport;
fn unknown_observation() -> PilotObservation {
    PilotObservation { case_id: "case-1".into(),
        arm: PilotArm { route_id: "fixture".into(), label: "baseline".into() },
        receipt: EconomicAttemptReceipt { execution_id: "e1".into(), node_id: "n1".into(),
            attempt: 1, route_id: "fixture".into(),
            cost: CostObservation::unknown(UsageBreakdown::default(), "cost unavailable"),
            usage: UsageBreakdown::default(), proof: None } }
}
```

- [ ] **Step 1: Write failing contract tests.** Assert schema validation for measured, estimated, unknown, allocated, marginal, and quota observations; reject a report with unavailable cost, asymmetric arms, corpus digest mismatch, missing required evidence, or zero proven actions. A rate-card estimate must report coverage and must never claim literal provider funds were charged.

```rust
#[test]
fn pilot_refuses_unavailable_cost_before_computing_savings() {
    let report = evaluate_pilot(&[unknown_observation()], &[unknown_observation()], "sha256:corpus");
    assert_eq!(report.cost_per_proven_action_nanounits, None);
    assert_eq!(report.refusal.as_deref(), Some("cost_unavailable"));
}
```

Add positive estimated and settled cases using the production checked proof-resolution entry point with a fake authoritative ledger, not a public `proven: true` flag or foreign crate's test-only constructor. For each paired arm, retain the same case IDs and independently known captures. Report each arm separately; the report fields above describe **one arm**, so the public comparison wraps `baseline: PilotReport`, `candidate: PilotReport`, and optional savings. `evaluate_pilot` first validates the paired inputs, then delegates to per-arm aggregation; never pool baseline and candidate costs or successes into one savings metric. The small unavailable-cost test exercises the shared preflight refusal before producing either arm.


- [ ] **Step 2: Run the report test and schema test and confirm failure.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test economic_pilot --locked`

Expected: FAIL because `core/gateway/src/economic_pilot.rs` and its report implementation are absent.

- [ ] **Step 3: Define the fixed-route pilot first.** The recipe must require paired baseline-versus-candidate runs over held-out tasks with identical snapshots, prompt, permissions, route settings, budget, clock, seed, and acceptance contract. Record every attempt, retry, usage field, cost grade, quota state, latency, deterministic proof result, and refusal reason. Do not compare routers until this fixed-route pilot establishes quality parity and cost observability.

- [ ] **Step 4: Define the router experiment.** After the fixed-route pilot passes, run the deterministic router on a separate held-out set with paired cases and a fixed budget. Primary metric: total monetary cost divided by actions that pass the actual deterministic proof gate. Secondary metrics: proven-action rate, unknown-cost rate, retries, p95 latency, quota consumption, and quality parity. Report strata by profile and context size; never replace them with one global mean.

- [ ] **Step 5: Implement `core/gateway/src/economic_pilot.rs::evaluate_pilot` over the protocol-owned `EconomicAttemptReceipt`; the gateway must not import `core/runtime`.** Sum measured or estimated nanounits with checked arithmetic, calculate cost per independently proven action only when the denominator is nonzero and required cost coverage is complete, and expose `estimated_cost_coverage` separately. Refuse when any required cost is unavailable, arms are asymmetric, corpus digest differs, required evidence is missing, the oracle is reachable, currencies cannot be reconciled, or no action is proven. A smaller prompt or token count cannot compensate for failed proof or missing monetary evidence.

- [ ] **Step 6: Register the extension contributions.** Add the evaluator and schema paths to `extensions/builtin/graphhelm-development-contracts/extension.json` with stable contribution ids, schema links, and freshly computed SHA-256 values. Update the inventory test so an unregistered or stale digest fails before the evaluator is accepted.

- [ ] **Step 7: Run the offline acceptance checks and commit.**

Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-gateway --test economic_pilot --locked`
Run: `$env:CARGO_TARGET_DIR='E:/_agent-scratch/graphhelm/methodology-adoption-economics/target'; cargo +1.97.1 test -p graphhelm-cli --test development_contract_schemas --locked`

Expected: PASS.

```text
git add core/gateway/src/economic_pilot.rs core/gateway/tests/economic_pilot.rs docs/acceptance/economic-route-selection-recipe.md extensions/builtin/graphhelm-development-contracts/evaluators/economic-efficiency.yaml extensions/builtin/graphhelm-development-contracts/schemas/economic-efficiency-evaluator.schema.json extensions/builtin/graphhelm-development-contracts/extension.json apps/cli/tests/development_contract_schemas.rs
git commit -m "docs: define economic route selection pilot"
```

### Verification before completion

- [ ] Confirm only the issue-approved files are changed and `git diff --check` is clean.
- [ ] Run the focused tests from each task with `CARGO_TARGET_DIR=E:/_agent-scratch/graphhelm/methodology-adoption-economics/target` after checking the `C:` and `E:` free-space floors and keeping no more than two concurrent gates.
- [ ] Run `cargo +1.97.1 fmt --all -- --check`, locked metadata, targeted Clippy, and the full `ci/gate.ps1` without piping its output; capture the exit code separately.
- [ ] Verify no test performs a live provider call, reads credentials, starts a browser, or enables hosted Actions.
- [ ] Review the final diff against the approved ADR/RFC for the D-054 routing-role boundary before any merge or deployment claim.
