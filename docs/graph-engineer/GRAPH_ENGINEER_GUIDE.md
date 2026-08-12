# Graph Engineer Guide

## 1. Role

The Graph Engineer is the one who expands the universe of possible decisions in GraphHelm. They do not write rigid workflows for each domain. They register reusable, verifiable units so the harness can assemble new workflows.

Responsibilities:

- modeling atomic capabilities;
- defining inputs, outputs, and evidence;
- creating operational skills;
- integrating tools and providers;
- writing evaluators and policies;
- defining node types or visualizers when needed;
- creating conformance tests;
- measuring cost, quality, and security;
- maintaining compatibility and migrations.

## 2. Mental model

```text
Capability: what can be done
Tool: mechanism that executes an action
Skill: guidance on how to apply capabilities
Agent: temporary worker with a goal and a contract
Node: unit of execution in the graph
Gate: verifiable obligation
Policy: rule that requires/restricts something
Artifact: persistent result
Evidence: referenceable proof
Evaluator: mechanism that judges a contract
Graph: task-specific composition
```

Avoid confusing:

- skill with permission;
- agent with model;
- node with persistent agent;
- textual output with evidence;
- graph template with policy;
- policy with prompt suggestion.

## 3. Design principles

### 3.1 Useful atomicity

A capability should be small enough to be composable, but large enough to have a meaningful contract.

Bad:

```text
software_engineering
```

Better:

```text
repository_symbol_search
trace_callers
apply_patch
execute_targeted_tests
inspect_dependency_update
```

### 3.2 Contracts before prompts

Define:

- what input is required;
- what output will be produced;
- how to validate;
- what evidence is required;
- what permissions are required;
- how it fails;
- how to cancel.

Then write instructions.

### 3.3 Least privilege

Capabilities and tools declare the minimum access. Don't use `filesystem:*` when `repository:read` suffices.

### 3.4 Explicit failure

A component must distinguish:

- input failure;
- tool failure;
- lack of permission;
- insufficient context;
- invalid output;
- uncertainty;
- valid negative conclusion.

### 3.5 Evidence external to discourse

A "everything is fine" report proves nothing. Require locations, test IDs, source refs, hashes, or artifacts.

### 3.6 Substitutability

A capability can have multiple providers. The graph depends on the contract, not on a specific implementation.

## 4. Creating a capability

### 4.1 Checklist

1. verb/action name;
2. single purpose;
3. input schema;
4. output schema;
5. permissions;
6. isolation minimum;
7. determinism;
8. latency/cost profile;
9. failure modes;
10. evidence produced;
11. conformance tests;
12. compatibility range.

### 4.2 Example

```yaml
apiVersion: p50.dev/v1
kind: Capability
metadata:
  id: repository.trace_callers
  version: 1.1.0
  scope: public
spec:
  description: Encontra chamadores diretos e indiretos de um símbolo.
  inputSchema: schemas/trace-callers-input.json
  outputSchema: schemas/trace-callers-output.json
  permissions:
    - repository.read
  isolationMinimum: tier_0
  deterministic: best_effort
  producesEvidence:
    - source_locations
    - dependency_edges
  providers:
    - tool: builtin.ast_index
    - tool: plugin.language_server
  conformance:
    - tests/trace-callers/basic.yaml
    - tests/trace-callers/indirect.yaml
```

### 4.3 Granularity

Split when:

- permissions differ;
- isolation differs;
- inputs/outputs don't form a unit;
- one part can be deterministically tested;
- providers differ;
- failures need distinct handling.

Don't split when the coordination cost outweighs the benefit and the contract only makes sense as a whole.

## 5. Creating a tool

A tool is an executable integration. It can be builtin, a local process, a container, a WASI module, an HTTP service, an MCP server, or an adapter.

### 5.1 Required manifest

```yaml
apiVersion: p50.dev/v1
kind: Tool
metadata:
  id: browser.playwright
  version: 1.0.0
spec:
  transport: container
  capabilities:
    - browser.navigate
    - browser.screenshot
    - browser.inspect_dom
  permissions:
    network: required
    filesystem: none
    secrets: optional_by_reference
  isolationMinimum: tier_2
  inputSchema: schemas/browser-command.json
  outputSchema: schemas/browser-result.json
  cancellation: cooperative
  timeoutSeconds: 120
  redaction:
    detectSecrets: true
    redactHeaders:
      - authorization
      - cookie
  platforms:
    - linux_amd64
    - linux_arm64
```

### 5.2 Rules

- never receive a raw secret when a reference will do;
- validate every path at the broker;
- produce artifacts for large payloads;
- structured and redacted logs;
- support cancellation when possible;
- don't rely on stdout as the sole contract;
- declare external effects;
- declare idempotency;
- declare compensation or irreversibility.

## 6. Creating a skill

A skill codifies technique and process. It can guide an agent, suggest tools, and define checks, but it does not grant permissions.

### 6.1 Structure

```yaml
apiVersion: p50.dev/v1
kind: Skill
metadata:
  id: security.review_auth_boundary
  version: 2.0.0
spec:
  purpose: Evaluate changes that alter authentication and session handling.
  applicability:
    anySignal:
      - touches_authentication
      - touches_session_management
  requiredCapabilities:
    - repository.trace_callers
    - repository.read
    - tests.inspect
  recommendedCapabilities:
    - security.static_scan
  inputSchema: schemas/auth-change-bundle.json
  outputSchema: schemas/security-review-report.json
  completion:
    requires:
      - changed_files_inspected
      - trust_boundaries_mapped
      - findings_reference_locations
      - negative_cases_considered
  instructionsRef: skill.md
  tests:
    - conformance/known-vulnerability.yaml
    - conformance/clean-change.yaml
```

### 6.2 Good skill

- describes goal and method;
- lists pitfalls;
- requires evidence;
- distinguishes absence of a problem from lack of inspection;
- supports structured outputs;
- avoids automatic-approval language;
- has positive and negative examples.

### 6.3 Bad skill

- is a generic prompt;
- requires "think step by step" as evidence;
- grants shell/network access;
- pins a model;
- mixes execution and approval;
- has no tests;
- always recommends more agents.

## 7. Creating an agent template

An agent template is optional. The harness can synthesize agents without a template. Templates are useful when there is a stable operational identity and relevant history.

```yaml
apiVersion: p50.dev/v1
kind: Agent
metadata:
  id: project.payment_regression_investigator
  version: 4.0.0
spec:
  purpose: Investigar regressões no fluxo de pagamentos.
  capabilities:
    - repository.search
    - repository.trace_callers
    - diff.inspect
    - tests.execute_targeted
  defaultPermissions:
    - repository.read
    - tests.execute
  prohibitedActions:
    - repository.write
    - production.access
  inputSchema: schemas/payment-change-context.json
  outputSchema: schemas/regression-analysis.json
  modelRequirements:
    profile: critical_reasoning
  contextStrategy:
    includeScopes:
      - payments
      - tests
      - architecture_decisions
    maxTokens: 24000
  memoryPolicy:
    writeCandidates: true
    defaultTtlDays: 90
  completionContract: contracts/payment-review.yaml
```

## 8. Creating an evaluator

An evaluator verifies an output, artifact, claim, or execution.

Types:

- deterministic;
- model-based;
- hybrid;
- human;
- external-system;
- statistical;
- visual.

### 8.1 Contract

```yaml
apiVersion: p50.dev/v1
kind: Evaluator
metadata:
  id: tests.targeted_pass
  version: 1.0.0
spec:
  inputSchema: schemas/test-report.json
  outputSchema: schemas/gate-result.json
  method: deterministic
  passWhen: input.failed == 0 && input.executed > 0
  failureCategories:
    - no_tests_executed
    - test_failure
    - malformed_report
```

### 8.2 Model evaluator

Must declare:

- model profile;
- independence requirements;
- blind context policy;
- evidence citation requirement;
- calibration dataset;
- disagreement handling;
- max cost.

## 9. Creating a policy

A policy must be small, readable, deterministic, and testable.

### 9.1 Example

```yaml
apiVersion: p50.dev/v1
kind: Policy
metadata:
  id: security.secret_handling
  version: 1.0.0
spec:
  scope: workspace
  when:
    any:
      - signal: handles_secrets
        equals: true
      - permissionRequested: secrets.read
  require:
    - isolationMinimum: tier_2
    - gate: security.secret_exposure_scan
  deny:
    - export.include_secrets
    - log.raw_secret_values
  override:
    ownerAllowed: false
```

### 9.2 Policy tests

Every policy needs fixtures for:

- must trigger;
- must not trigger;
- waiver allowed;
- waiver forbidden;
- conflict with another policy;
- version migration.

## 10. Node types

Built-in node types must cover universal primitives. Create a new type only when lifecycle, UI, or semantics differ substantially.

### 10.1 `agent`

Runs a model/runtime with tools.

### 10.2 `tool`

Executes a direct call without an agent.

### 10.3 `classifier`

Produces a structured classification.

### 10.4 `gate`

Evaluates a requirement and decides pass/fail.

### 10.5 `fork` / `join`

Controls parallelism and artifact merging.

### 10.6 `human_decision`

Pauses until a response or a timeout policy.

### 10.7 `subgraph`

Invokes a parameterized graph without hiding internal events.

### 10.8 `materializer`

Converts claims/evidence into a document or projection.

### 10.9 `deploy` / `rollback`

Represents an external effect with preconditions and compensation.

## 11. Edge design

### 11.1 Data edge

Transfers a typed payload. Avoid huge payloads; use artifact refs.

### 11.2 Evidence edge

Declares that an output proves a requirement of another node/gate.

### 11.3 Control edge

Orders execution without a payload.

### 11.4 Conditional edge

Uses a limited expression. Must cover missing/unknown.

### 11.5 Failure edge

Routes a failure category.

### 11.6 Compensation edge

Defines an action to undo an external effect.

## 12. Completion contracts

A completion contract must be verifiable.

```yaml
completion:
  requires:
    - artifactExists: implementation.patch
    - evidenceCount:
        type: test_result
        min: 1
    - expression: output.changed_files.length > 0
    - claimCoverage:
        requirements: acceptance_criteria
        minimum: 1.0
  forbids:
    - unresolvedBlockingFinding: true
    - unsupportedClaim: true
```

## 13. Context design

The Graph Engineer must declare:

- relevant scopes;
- preferred types;
- temporal window;
- max tokens;
- blind exclusions;
- conflict behavior;
- expansion rules;
- sensitive data handling.

Don't include entire documents when symbols/sections suffice.

## 14. Memory design

Agent memory is not a chat cache. Record only reusable observations with evidence and a TTL.

Good:

```text
Changes to PaymentService frequently require idempotency tests.
Evidence: exec-482, tests/payments/idempotency.spec.ts
Confidence: 0.87
Expires: 90 days
```

Bad:

```text
Eu acho que o backend costuma ser confuso.
```

## 15. Security review for extensions

Checklist:

- supply chain;
- publisher identity;
- signature;
- dependencies;
- requested permissions;
- network destinations;
- secret access;
- path traversal;
- command injection;
- sandbox escape;
- data retention;
- telemetry;
- update channel;
- rollback;
- license compatibility.

Trust levels:

- builtin;
- verified;
- community;
- untrusted;
- quarantined.

## 16. Conformance suite

Every extension must pass:

1. schema validation;
2. manifest lint;
3. permission diff;
4. deterministic fixtures;
5. timeout/cancellation;
6. malformed input;
7. output size;
8. secret redaction;
9. sandbox test;
10. compatibility test;
11. replay determinism when applicable;
12. documentation completeness.

## 17. Performance and cost

Declare:

- cold start;
- warm latency;
- expected token usage;
- output size;
- CPU/memory;
- network egress;
- concurrency limit;
- cache behavior.

The harness uses this data to decide composition.

## 18. Versioning

- patch: fix without a new contract;
- minor: backward-compatible capability;
- major: breaking schema/semantics;
- deprecated components remain reproducible;
- migration guide required for major;
- manifest declares a compatible framework range.

## 19. Publishing

A publishable package contains:

```text
manifest.yaml
README.md
LICENSE
schemas/
skills/ ou runtime/
tests/
examples/
SECURITY.md
CHANGELOG.md
SIGNATURE
```

The registry displays publisher, trust, permissions, supported platforms, versions, vulnerabilities, and compatibility.

## 20. Anti-patterns

- mega-agent with every tool;
- 20-node workflow for a trivial task;
- reviewer reading only the executor's summary;
- `output: string` schema for everything;
- tool with unrestricted host access;
- policy implemented in the prompt;
- graph edge with no behavior for missing output;
- infinite retry;
- memory without expiration;
- agent template hard-coding a provider;
- plugin requiring secrets without justification;
- docs without provenance;
- evaluator that always approves.

## 21. Graph Engineer review checklist

Before merging an extension:

- [ ] Does the problem require a new capability?
- [ ] Is the contract atomic and reusable?
- [ ] Are inputs and outputs typed?
- [ ] Is evidence explicit?
- [ ] Are permissions minimal?
- [ ] Is the minimum isolation correct?
- [ ] Are failure modes distinguishable?
- [ ] Do cancellation and timeout exist?
- [ ] Are there positive and negative tests?
- [ ] Is there protection against secret leakage?
- [ ] Is the extension substitutable?
- [ ] No provider/model hard-coded without necessity?
- [ ] Are versioning and migration defined?
- [ ] Can the UI explain what it does?
