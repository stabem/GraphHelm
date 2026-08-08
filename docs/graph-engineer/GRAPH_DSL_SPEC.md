# Graph DSL — especificação v1

## 1. Objetivo

A Graph DSL descreve grafos executáveis de maneira tipada, versionada e independente da implementação do Studio. Ela é usada pelo Harness Compiler, Graph Engineer, CLI, SDKs, exports e conformance tests.

A DSL não inclui credenciais, chain-of-thought ou payloads grandes. Esses itens são referenciados por IDs seguros.

## 2. Documento base

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

Obrigatório:

- `id` único;
- `executionId`;
- `version` monotônica;
- `name`;
- `createdAt` na representação persistida;
- `createdBy` na representação persistida.

Opcional:

- labels;
- annotations;
- basedOn;
- mutationId;
- description.

Annotations não alteram semantics. Labels podem ser usadas por policies apenas quando schema permitir.

## 4. Node map

Nós são um map por ID para diff estável.

```yaml
nodes:
  security_review:
    type: agent
    name: Revisar segurança
    objective: Identificar vulnerabilidades introduzidas na autenticação.
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

`agent.ref` e `agent.ephemeral` são mutuamente exclusivos.

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

Tool node não chama LLM por padrão.

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

## 10. Fork e join

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

Subgraph não esconde eventos; UI pode colapsar visualmente.

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

## 14. Deploy e rollback

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

A expression language é pura, limitada e determinística.

Permitido:

- comparação;
- boolean;
- null/missing check;
- acesso a output tipado;
- funções agregadas seguras;
- regex limitada;
- numeric/string operations;
- time relative a event metadata.

Proibido:

- shell;
- network;
- filesystem;
- eval;
- dynamic imports;
- acesso a secrets;
- loops não limitados.

Exemplos:

```text
nodes.tests.output.failed == 0
count(nodes.review.output.findings[severity == "critical"]) == 0
exists(artifacts["rollback-plan"])
```

## 17. Bindings

Bindings referenciam:

- node outputs;
- artifacts;
- context items;
- claims;
- user decisions;
- project settings;
- environment references sem secret value.

Payloads grandes usam artifact refs.

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

## 19. Permissions e leases

DSL pede capabilities; Runtime concede leases.

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

Não é permitido secret inline.

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
    - recompile_context
```

Retries contam `maxAttempts`; graph loops de remediation são diferentes e também limitados.

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

Assinatura pode não expor custo; limite financeiro aplica somente a rotas pagas. Capacity controls ainda aplicam.

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

Inline constraints podem restringir, não ampliar permissões acima do owner policy.

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

Graph Draft usa operações:

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

Além de JSON Patch básico, operações semânticas podem incluir:

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

Override fica no mutation record, não altera policy global.

## 28. Ghost nodes

Ghost node é Graph Draft metadata, não node ativo:

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

UI hints não afetam execução e podem ser alterados sem Graph Version operacional.

## 30. Lint rules

Erros:

- schema incompatível;
- node sem terminal path;
- cycle sem limit;
- missing edge behavior;
- permission impossível;
- unknown capability;
- hard policy violation;
- secret inline;
- deploy sem target;
- compensation required e ausente;
- output binding inexistente.

Warnings:

- reviewer correlacionado;
- contexto excessivo;
- nó redundante;
- cost alto;
- output string não estruturado;
- optional node em critical path;
- timeout ausente quando default usado.

## 31. Canonicalization e hashing

Antes de hash:

- ordenar maps por key;
- normalizar whitespace;
- remover UI-only fields;
- resolver refs para version IDs;
- preservar semantic expressions;
- incluir policy hashes e schema versions.

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

Imports são pinned por versão e hash no Harness Manifest.

## 33. Compatibilidade

Implementação compatível com v1 deve:

- rejeitar unknown required fields;
- preservar unknown annotations;
- suportar todos os common node states;
- expor graph versioning;
- validar expressions;
- não executar ghost nodes;
- não armazenar secrets inline;
- registrar waivers;
- emitir events normativos.
