# Graph DSL — specification v1

## 1. Purpose

The Graph DSL describes executable graphs in a typed, versioned, and implementation-independent manner. It is used by the Harness Compiler, Graph Engineer, CLI, SDKs, exports, and conformance tests.

The DSL does not include credentials, chain-of-thought, or large payloads. These items are referenced by secure IDs.

## 2. Base document

```yaml
apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_482_graph_v7
  name: Implementar idempotência de webhooks
  executionId: exec_482
  version: 7
  basedOn: exec_482_graph_v6
  labels:
    origin: user
    mode: autopilot
spec:
  entrypoints:
    - map_repository
  nodes: {}
  edges: []
  budgets: {}
  policies: []
  completion: {}
```

## 3. Metadata

Required:

- unique `id`;
- `executionId`;
- monotonic `version`;
- `name`;
- `createdAt` in the persisted representation;
- `createdBy` in the persisted representation.

Optional:

- labels;
- annotations;
- basedOn;
- mutationId;
- description.

Annotations do not alter semantics. Labels may be used by policies only when the schema allows it.

## 4. Node map

Nodes are a map keyed by ID for stable diffing.

```yaml
nodes:
  security_review:
    type: agent
    name: Review security
    objective: Identify vulnerabilities introduced in authentication.
    optionality: required
    agent:
      ref: project/security-reviewer@3
    model:
      profile: critical_reasoning
      requireIndependentFrom:
        - implementation
    input:
      schema: schema://AuthenticationChangeBundle@1
      bindings:
        change: artifact://implementation.diff
        architecture: context://auth_architecture
    output:
      schema: schema://SecurityReviewReport@1
      publishAs: artifact://security-review.json
    context:
      policyRef: context-policy://blind-security-review@1
      budget:
        initialTokens: 16000
        maxTokens: 28000
    permissions:
      - repository.read
      - tests.read
    isolation:
      minimum: tier_0
    completion:
      contractRef: contract://security-review@2
    retry:
      maxAttempts: 2
      backoff: exponential
      retryOn:
        - model_transient_error
        - malformed_output
    timeoutSeconds: 1800
```

## 5. Node common fields

- `type`
- `name`
- `objective`
- `description`
- `optionality`
- `tags`
- `input`
- `output`
- `context`
- `permissions`
- `isolation`
- `completion`
- `retry`
- `timeoutSeconds`
- `onCancel`
- `onFailure`
- `userEditable`
- `userOverrideAllowed`
- `resources`
- `memory`
- `ui`

## 6. Agent node

```yaml
implementation:
  type: agent
  agent:
    ephemeral:
      purpose: Implementar idempotência sem alterar API pública.
      capabilities:
        - repository.search
        - repository.write_patch
        - tests.execute_targeted
      prohibitedActions:
        - production.deploy
      instructionsRef: inline-or-artifact
  model:
    profile: software_execution
    routePolicy: dynamic
```

`agent.ref` and `agent.ephemeral` are mutually exclusive.

## 7. Tool node

```yaml
run_tests:
  type: tool
  tool:
    ref: builtin/test-runner@1
    action: execute
  input:
    schema: schema://TestRequest@1
  output:
    schema: schema://TestReport@1
```

A tool node does not call an LLM by default.

## 8. Classifier node

```yaml
classify_risk:
  type: classifier
  classifier:
    method: hybrid
    profile: fast_classification
    deterministicRulesRef: rules://risk-signals@2
  output:
    schema: schema://RiskProfile@1
```

## 9. Gate node

```yaml
security_gate:
  type: gate
  gate:
    requirements:
      - no_blocking_security_findings
      - evidence_coverage_complete
    evaluators:
      - evaluator://security-report-validator@1
    passWhen: all
  onFail:
    routeTo: remediation
  override:
    allowedRoles:
      - owner
    resultLabel: completed_with_security_waiver
```

## 10. Fork and join

```yaml
parallel_review:
  type: fork
  strategy: all

review_join:
  type: join
  strategy: all_completed
  merge:
    method: artifact_bundle
    outputSchema: schema://ReviewBundle@1
```

Join strategies:

- `all_completed`
- `all_succeeded`
- `any_succeeded`
- `quorum`
- `first_valid`
- `custom_evaluator`

## 11. Human decision

```yaml
choose_deploy_target:
  type: human_decision
  prompt: Selecione o ambiente de deploy.
  options:
    - staging
    - production
    - cancel
  timeout:
    seconds: 86400
    onTimeout: pause
  output:
    schema: schema://DeployTargetDecision@1
```

## 12. Subgraph

```yaml
security_subgraph:
  type: subgraph
  graphRef: graph-template://security-review@3
  parameters:
    scope: artifact://change-scope.json
  expose:
    outputs:
      - security_report
```

A subgraph does not hide events; the UI may collapse it visually.

## 13. Materializer

```yaml
update_docs:
  type: materializer
  materializer:
    target: document://architecture/authentication.md
    strategy: evidence_backed_patch
  input:
    schema: schema://DocumentationUpdateBundle@1
```

## 14. Deploy and rollback

```yaml
deploy:
  type: deploy
  adapterRef: deploy://docker-compose@1
  targetRef: environment://staging
  preconditions:
    - artifact://build.tar
  effects:
    reversible: true
    compensationNode: rollback

rollback:
  type: rollback
  adapterRef: deploy://docker-compose@1
  input:
    schema: schema://DeploymentReceipt@1
```

## 15. Edges

```yaml
edges:
  - id: implementation_to_tests
    from: implementation
    to: run_tests
    type: data
    map:
      patch: outputs.implementation.patch
      snapshot: outputs.implementation.source_snapshot

  - id: tests_to_security
    from: run_tests
    to: security_review
    type: evidence

  - id: gate_to_deploy
    from: security_gate
    to: deploy
    type: control
    condition: nodes.security_gate.output.passed == true
    onUnknown: pause
```

## 16. Expression language

The expression language is pure, restricted, and deterministic.

Allowed:

- comparison;
- boolean;
- null/missing check;
- access to typed output;
- safe aggregate functions;
- restricted regex;
- numeric/string operations;
- time relative to event metadata.

Prohibited:

- shell;
- network;
- filesystem;
- eval;
- dynamic imports;
- access to secrets;
- unbounded loops.

Examples:

```text
nodes.tests.output.failed == 0
count(nodes.review.output.findings[severity == "critical"]) == 0
exists(artifacts["rollback-plan"])
```

## 17. Bindings

Bindings reference:

- node outputs;
- artifacts;
- context items;
- claims;
- user decisions;
- project settings;
- environment references without secret values.

Large payloads use artifact refs.

## 18. Context policy

```yaml
context:
  include:
    - type: project_kernel
    - type: document
      ref: document://architecture/auth.md
    - type: source_scope
      paths:
        - src/auth/**
    - type: dependency_output
      node: impact_analysis
  exclude:
    - executor_subjective_summary
    - unrelated_marketing_docs
  conflicts: present_all
  freshness:
    maxAgeDays: 30
    requireRevalidationFor:
      - source_code_claims
  budget:
    initialTokens: 12000
    maxTokens: 24000
  expansion:
    allowed: true
    requiresReason: true
```

## 19. Permissions and leases

The DSL requests capabilities; the Runtime grants leases.

```yaml
permissions:
  - capability: repository.read
    scope:
      paths:
        - src/auth/**
        - tests/auth/**
    duration: node
  - capability: network.request
    scope:
      allowlist:
        - api.example.com
    duration: call
```

Inline secrets are not allowed.

## 20. Isolation

```yaml
isolation:
  minimum: tier_2
  filesystem:
    mode: execution_worktree
    writablePaths:
      - /workspace
  network:
    mode: allowlist
  secrets:
    mode: broker_only
  resources:
    cpu: 4
    memoryMb: 8192
    diskMb: 20480
```

## 21. Retry

```yaml
retry:
  maxAttempts: 3
  backoff: exponential
  maxBackoffSeconds: 120
  retryOn:
    - transient_provider_error
    - tool_timeout
  doNotRetryOn:
    - policy_denied
    - invalid_user_input
  beforeRetry:
    - reset_sandbox
    - refresh_context_delta
```

Retries count toward `maxAttempts`; graph remediation loops are different and are also limited.

Retry classification is fail-closed and verdict-aware. A completed non-zero tool exit is terminal
by default. `retryOn: [tool_exited_non_zero]` is the explicit opt-in that makes that cause retryable;
`doNotRetryOn: [tool_exited_non_zero]` explicitly keeps it terminal. A tool timeout remains
retryable because no tool verdict completed. A durable `HostError` is terminal under the current
record because its stable code does not preserve the operating system `ErrorKind` needed for a
deterministic transient/permanent classification. GateCheck semantics are unchanged.

`retryOn` and `doNotRetryOn` are sets and must be disjoint. Listing the same cause in both is an
invalid retry policy; neither field wins by precedence. GraphHelm must refuse the conflict
deterministically before any node effect, emit stable diagnostic evidence, and settle the affected
execution without leaving the node silently stranded in `Ready` or the execution running.

`refresh_context_delta` follows `CONTEXT_KNOWLEDGE_DREAMS.md` §8.3: a retry receives the delta
since the previous capsule plus stable references, never an unconditional recompilation — full
recompilation is reserved for failure categories whose failure invalidates the capsule itself
(for example a sandbox crash).

## 22. Completion

```yaml
completion:
  requires:
    - outputSchemaValid: true
    - evidence:
        type: source_location
        min: 1
    - expression: output.confidence >= 0.7
  forbids:
    - expression: output.unsupportedClaims > 0
```

## 23. Graph budgets

```yaml
budgets:
  maxNodes: 50
  maxDepth: 12
  maxMutations: 10
  maxRetriesPerNode: 2
  maxWallClockSeconds: 10800
  maxApiCostUsd: 20
  maxParallelModelCalls: 4
  maxContextTokensPerNode: 64000
```

The signature may not expose cost; the financial limit applies only to paid routes. Capacity controls still apply.

## 24. Policies

```yaml
policies:
  - ref: policy://workspace/security-baseline@2
  - ref: policy://project/deploy-rules@4
  - inlineConstraint:
      deny:
        - production.deploy
      reason: user_request_scope
```

Inline constraints can restrict, but not expand, permissions beyond the owner policy.

## 25. Graph completion

```yaml
completion:
  terminalNodes:
    - deliver_result
  requires:
    - document://task-summary.md
    - artifact://final-report.json
  allowWaivers: true
  statuses:
    full: completed
    waived: completed_with_waivers
```

## 26. Mutation operations

Graph Draft uses operations:

```yaml
operations:
  - op: addNode
    path: /spec/nodes/security_review
    value: {...}
  - op: removeNode
    path: /spec/nodes/performance_review
  - op: addEdge
    value: {...}
  - op: patchNode
    path: /spec/nodes/deploy/model
    value: {...}
```

Beyond basic JSON Patch, semantic operations may include:

- `splitNode`
- `mergeNodes`
- `replaceNode`
- `bypassNodes`
- `elevateIsolation`
- `invalidateOutputs`

## 27. User override

```yaml
manualOverride:
  actor: user://owner-local
  reason: Ir direto para deploy.
  bypassedRequirements:
    - integration_tests
    - independent_security_review
  acknowledgedRisks:
    - untested_regression
    - unreviewed_security_change
  scope: execution
```

The override stays in the mutation record and does not alter global policy.

## 28. Ghost nodes

A ghost node is Graph Draft metadata, not an active node:

```yaml
proposal:
  id: proposal_security_review
  proposedNode: {...}
  trigger: signal://unexpected_security_boundary
  state: awaiting_user
  resourceConsumption: none
```

## 29. UI hints

```yaml
ui:
  position:
    x: 420
    y: 180
  group: security
  collapsed: false
  accent: semantic/security
```

UI hints do not affect execution and can be changed without an operational Graph Version.

## 30. Lint rules

Errors:

- incompatible schema;
- node without a terminal path;
- cycle without a limit;
- missing edge behavior;
- impossible permission;
- unknown capability;
- hard policy violation;
- inline secret;
- deploy without a target;
- required compensation missing;
- nonexistent output binding.

Warnings:

- correlated reviewer;
- excessive context;
- redundant node;
- high cost;
- unstructured string output;
- optional node on the critical path;
- missing timeout when a default is used.

## 31. Canonicalization and hashing

Before hashing:

- sort maps by key;
- normalize whitespace;
- remove UI-only fields;
- resolve refs to version IDs;
- preserve semantic expressions;
- include policy hashes and schema versions.

## 32. Imports

```yaml
imports:
  schemas:
    - package://p50-standard-schemas@1.2.0
  policies:
    - package://p50-security-baseline@2.0.0
  graphs:
    - package://community/deploy-subgraph@1.0.0
```

Imports are pinned by version and hash in the Harness Manifest.

## 33. Compatibility

A v1-compatible implementation must:

- reject unknown required fields;
- preserve unknown annotations;
- support all common node states;
- expose graph versioning;
- validate expressions;
- not execute ghost nodes;
- not store secrets inline;
- record waivers;
- emit normative events.
