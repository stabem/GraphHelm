# Journey-Proven Development

## Status

Accepted design for issue #210. This specification defines how GraphHelm plans, implements, and
verifies changes from the user's observable journey instead of applying one universal testing
ritual to every behavior.

## 1. Purpose

Journey-Proven Development (JPD) turns each user promise into typed observation obligations. The
harness then selects the smallest set of deterministic checks, focused tests, observers, and
independent reviewers that can prove those obligations at the required strength.

JPD does not remove unit, property, integration, concurrency, or browser tests. It rejects the
assumption that one of those methods is always the right starting point. The proof method follows
the promise and risk.

## 2. Constitutional rules

1. A promise is not complete until every required observation obligation has adequate evidence.
2. If no installed observer can produce evidence of the required strength, compilation stops with
   `OBSERVER_MISSING`. A proxy is never silently upgraded into direct proof.
3. A later successful retry never erases an earlier failure. All attempts remain linked and the
   result states whether success was first-pass, recovered, flaky, or unresolved.
4. Agent agreement is advice, not proof. Deterministic policy and evidence decide gates.
5. Only the Graph Governor may publish an operational graph or promote a generated project skill.
6. Skills and host wrappers use public GraphHelm CLI, MCP, or HTTP contracts only. They never import
   Runtime internals.
7. Browser journeys target semantic user-visible elements by role, label, accessible name, text, or
   stable product identity. Coordinates are allowed only when geometry is the behavior under test.
8. Every bundle contribution is explicit. Discovery on disk does not activate it.
9. The owner may authorize continuation through the existing recorded waiver path, but a missing
   fact remains unproven. The result is `accepted_with_waiver`, never JPD-proven; it retains actor,
   reason exactly as recorded, acknowledged risks, affected Graph Versions, and waiver
   reference. Structural
   impossibility is never waivable, and an agent or skill cannot create the waiver for itself.
10. Repository content, journey artifacts, external evidence, and agent reports are untrusted data,
    never executable instructions or authority. They cannot expand permissions or bypass policy.
11. JPD artifacts bind redacted evidence references and digests. Secrets and raw sensitive browser,
    user, or provider evidence never enter contracts, fixtures, Graph DSL, manifests, prompts, or
    logs; unsafe input is refused and suspected instruction injection is routed through policy and
    typed signals.

## 3. One extension model

GraphHelm has one `Extension` manifest format, and each bundle has one authoritative
`extension.json`. A JPD bundle is a `skill-package` extension with a `data` runtime. Its manifest
composes ordinary contributions: skills, agent definitions, schemas, policies, evaluators,
observers, graphs, and fixtures.

The bundle is not a second plugin format. Installation, version pinning, dependency resolution,
activation, and later unload belong to the normal extension lifecycle. Host-specific Claude or
Codex files are deletable adapters over the same GraphHelm MCP server.

`extension.json` is authoritative for package identity, version, permissions, declared surfaces,
and the digest inventory. Host manifests are derived views. Claude and Codex adapters must
cross-match the Extension identity and version when those fields are present. MCP registration
must cross-match the declared public server. Duplicating a value for host syntax does not create a
second source of truth.

Package-local resource references use exactly
`extension://{metadata.id}/{contribution.id}`. The immutable package version comes from the
Extension envelope and lock, so resource references do not carry an `@version` suffix.
`extension validate` must resolve every such reference to an exact contribution id.

Each contribution declares:

- a stable id and immutable resource digest;
- its kind and package-relative path;
- the public surfaces it may use;
- effects and required permissions;
- required capabilities or observers;
- its family, when it is a smaller part of an entry workflow.

Activation is explicit and atomic. A future loader must keep the previous good version active if a
replacement fails validation. Capability increases require a new approval.

## 4. The eight entry families

The bundle exposes eight discoverable entry skills. They are selection points, not an eight-stage
workflow that always runs in full.

### 4.1 `journey-contract`

Converts the request into actors, preconditions, semantic actions, visible states, success
promises, failure contracts, recovery expectations, and out-of-scope behavior.

### 4.2 `observation-compiler`

Lowers promises into typed observations and matches them against the observer catalog and evidence
strength lattice. It produces either a complete observation plan or `OBSERVER_MISSING`.

### 4.3 `plan-council`

Selects the smallest useful set of risk-specific personas. It preserves arguments and dissent and
asks for a discriminating test when claims conflict. It never treats a vote as a gate.

### 4.4 `defect-bounty`

Accepts a typed `JourneyDefectClaim`, normalizes its semantic action trace, asks an advocate to
falsify it, and preserves confirmed minimal counterexamples as regression candidates. Novelty does
not outweigh severity or repeated evidence.

### 4.5 `skill-synthesizer`

Composes installed atomic capabilities into a task-local Skill Capsule. A capsule is a draft,
immutable package version. It cannot activate itself or publish graph mutations.

### 4.6 `skill-evaluator`

Measures evidence coverage, error reduction, token overhead, generalization, freshness, and later
regressions. Repeated strong evidence may create a promotion proposal; only governed explicit
publication makes a Project Skill operational.

### 4.7 `retry-provenance`

Requires `journeyRunId`, `rootAttemptId`, `attemptId`, `retryOf`, a closed cause tag, and an evidence
delta. It classifies the chain without laundering the first failure.

### 4.8 `journey-verifier`

Runs the compiled journey through available public surfaces and observers, captures evidence for
each obligation, and reports the strongest justified result. It refuses unsupported observations.

## 5. Persona council

Personas are agent definitions, not permanent characters and not hard-coded workflow packs. The
initial bundle includes:

- `defect-hunter`: searches for the shortest reproducible counterexample;
- `idea-generator`: proposes materially different solutions;
- `adversarial-critic`: attacks assumptions, edge cases, and evidence gaps;
- `evidence-advocate`: presents the strongest supported case and tries to falsify defect claims;
- `accessibility-user`: exercises keyboard, focus, semantics, zoom, and assistive expectations;
- `recovery-operator`: exercises refresh, back, retry, restart, partial failure, and safe recovery;
- `disagreement-resolver`: isolates conflicting claims and requests the cheapest discriminating
  evidence.

The Task Profiler selects personas from risk signals. A unique severe finding cannot be dismissed
by a majority of correlated agents.

For divergent work, first-pass roles receive the same bounded contract and work in parallel
isolation before seeing one another's output. The council records model, prompt, Context Capsule,
capability, and evidence-source lineage, normalizes and deduplicates claims, and only then exposes
distinct ideas to critics, advocates, and the resolver. Correlated opinions never count as
independent evidence.

## 6. Evidence strength

Evidence types form a partial order, not one global numeric score. Examples:

- an accepted HTTP request does not prove provider delivery;
- a queued message does not prove rendering or reading;
- a DOM node does not prove it was perceivable or operable;
- process exit zero does not prove the user-visible outcome;
- a screenshot does not prove focus order or keyboard reachability.

An observer capability declares the fact it observes, evidence type, trust level, version,
configuration, environment, freshness window, and custody chain. Absence-based evidence expires at
the end of its observation window. Static catalog support is never runtime availability: matching
requires a fresh, environment-bound capability receipt.

## 7. Journey and failure contracts

Every step may define:

- semantic action;
- expected visible and durable states;
- maximum time to a settled state;
- required observer and evidence type;
- timeout behavior;
- user-visible error behavior;
- safe-stop condition;
- recovery action;
- prohibited side effects.

Loading, disabled, empty, error, retrying, success, partial-success, and recovery states are first-
class when the product can expose them. The assurance compiler includes only states reachable from
the change and required risks.

## 8. Retry outcomes

Allowed retry cause tags are:

- `product_fix`;
- `environment_recovery`;
- `test_repair`;
- `dependency_recovery`;
- `unexplained`.

The deterministic projection returns exactly one of:

- `first_pass_success`;
- `recovered_success`;
- `flaky_pass`;
- `unresolved_failure`.

An unexplained retry can never project as first-pass success. Release evidence contains the full
attempt chain and the difference between evidence sets.

`flaky_pass` is a logical proof blocker: it can produce only `unresolved` or
`accepted_with_waiver` with a retry-local owner waiver. It is never JPD-proven. A
`recovered_success` may be proven only when its material delta and every other proof clause pass.

## 9. Skill lifecycle

This is the target lifecycle for a future Project Skill registry. Issue #210 implements only the
task-local draft and validation boundary.

```text
Atomic installed capability
  -> task-local Skill Capsule (draft)
  -> evaluated candidate
  -> governed promotion proposal
  -> versioned Project Skill
  -> suspended / deprecated / archived
```

Generated skills default to task scope. Promotion requires repeated evidence across distinct runs,
freshness, bounded token cost, no unresolved severe counterexample, and explicit Governor
publication. Dreams may propose a version but cannot bypass this path.

## 10. Assurance tiers

- `direct`: deterministic local check or focused test proves a low-risk promise.
- `deliberative`: multiple components or ambiguous behavior need an independent critic and broader
  integration evidence.
- `adversarial`: security, irreversible effects, production impact, or critical UX needs separate
  execution, counterexample search, recovery proof, and final verification.

Tier selection is deterministic from risk signals. The proof requirement remains fixed; the method
may change when an equivalent observer is cheaper.

## 11. Completion

JPD completes only when:

- all required promises have fresh, adequate evidence;
- every earlier failure remains represented in retry provenance;
- unresolved disagreements and waivers are explicit;
- required recovery behavior was exercised;
- the current Graph Version, code revision, configuration, fixtures, observer versions, and
  evidence digests are bound together;
- an applicable registered deterministic gate accepts the result; when no public gate covers the
  compiled obligations, the result remains unresolved with the gate capability gap named.

An owner waiver can authorize work or release to continue without satisfying those proof clauses,
but the verification result must remain `accepted_with_waiver`. Waiver status is separate from the
retry outcome, so a recovered run cannot hide either its first failure or an unproven obligation.

An agent summary, test count, green retry, loaded plugin, HTTP success, or reviewer quorum alone is
never sufficient.

## 12. Initial implementation boundary

Issue #210 delivers the validated built-in data bundle, its public-surface-only skills, persona and
contract resources, deterministic package validation, and a contract-level dogfood graph with a
deterministic simulation fixture. Together, `extension validate` and graph
validate/lint/simulate prove package integrity and Graph DSL/simulation conformance; they do not
execute the eight skill instruction files or an external observer end to end.

The current Extension host supports validated local package installation and active-version
switching. The adoption CLI adds reviewed backup/apply/restore for supported local configuration;
it does not prove that a running host loaded the method. Production observation remains
`observer_missing` until a trusted host observer exists; see the
[separate rehearsal](../acceptance/adoption-rehearsal.md). Remote registry, hot reload, marketplace,
browser observers, and Project Skill publication remain separate obligations. The package can emit a
task-local Skill Capsule draft and validate the packaged capsule schema and containing extension,
not the emitted instance. The instance remains advisory until a registered validator returns a
receipt. The current `quality certify` surface includes registered geometry, retry-lineage and
journey-contract gates; their individual contracts do not establish generic JPD certification or
real host activation.
Per-contribution surface declarations are validated but are not yet host-enforced tool subsets: the
current MCP server exposes its fixed tool table to the session token. Missing runtime capabilities
are reported, never simulated by the skill text.

The shipped policy, evaluator, evidence-lattice, and observer-catalog files are declarative data
contracts. Package validation checks their schema, paths, public surfaces, and digests; it does not
execute their rules. Until a registered deterministic implementation returns a versioned receipt,
LLM-produced matching, tier selection, council selection, retry classification, and skill scoring
remain advisory candidates and the applicable assurance result stays unresolved.

Journey Verification Result schema validation is shape-only. It does not register evaluator ids,
authenticate receipts, recompute input or coverage digests, or determine evidence freshness. A
candidate may become authoritative only when a registered deterministic JPD validator performs
those checks. Issue #210 ships the contract for that future validator, not the validator itself.
