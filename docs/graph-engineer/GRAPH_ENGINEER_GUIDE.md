# Guia do Graph Engineer

## 1. Papel

Graph Engineer é quem amplia o universo de decisões possíveis do GraphHelm. Ele não escreve workflows rígidos para cada domínio. Ele registra unidades reutilizáveis e verificáveis para que o harness possa montar workflows novos.

Responsabilidades:

- modelar capabilities atômicas;
- definir inputs, outputs e evidências;
- criar skills operacionais;
- integrar tools e providers;
- escrever evaluators e policies;
- definir node types ou visualizers quando necessário;
- criar conformance tests;
- medir custo, qualidade e segurança;
- manter compatibilidade e migrações.

## 2. Modelo mental

```text
Capability: o que pode ser feito
Tool: mecanismo que executa uma ação
Skill: orientação de como aplicar capabilities
Agent: trabalhador temporário com objetivo e contrato
Node: unidade de execução no grafo
Gate: obrigação verificável
Policy: regra que exige/restringe algo
Artifact: resultado persistente
Evidence: prova referenciável
Evaluator: mecanismo que julga um contrato
Graph: composição específica da tarefa
```

Evite confundir:

- skill com permissão;
- agent com modelo;
- node com agent persistente;
- output textual com evidência;
- template de grafo com policy;
- policy com sugestão de prompt.

## 3. Princípios de design

### 3.1 Atomicidade útil

Uma capability deve ser pequena o suficiente para ser composta, mas grande o suficiente para possuir contrato significativo.

Ruim:

```text
software_engineering
```

Melhor:

```text
repository_symbol_search
trace_callers
apply_patch
execute_targeted_tests
inspect_dependency_update
```

### 3.2 Contratos antes de prompts

Defina:

- que entrada é necessária;
- que saída será produzida;
- como validar;
- qual evidência é exigida;
- que permissões são necessárias;
- como falha;
- como cancelar.

Depois escreva instruções.

### 3.3 Menor privilégio

Capabilities e tools declaram o mínimo acesso. Não use `filesystem:*` quando `repository:read` é suficiente.

### 3.4 Falha explícita

Um componente deve distinguir:

- falha de input;
- falha de tool;
- falta de permissão;
- contexto insuficiente;
- output inválido;
- incerteza;
- conclusão negativa válida.

### 3.5 Evidência externa ao discurso

Relatório “está tudo certo” não prova nada. Requerer locations, test IDs, source refs, hashes ou artifacts.

### 3.6 Substituibilidade

Uma capability pode ter vários providers. O grafo depende do contrato, não de uma implementação específica.

## 4. Criando uma capability

### 4.1 Checklist

1. nome em verbo/ação;
2. propósito único;
3. input schema;
4. output schema;
5. permissions;
6. isolation minimum;
7. determinismo;
8. latency/cost profile;
9. failure modes;
10. evidence produced;
11. conformance tests;
12. compatibility range.

### 4.2 Exemplo

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

### 4.3 Granularidade

Divida quando:

- permissions diferem;
- isolation difere;
- inputs/outputs não formam unidade;
- uma parte pode ser deterministicamente testada;
- providers são diferentes;
- falhas precisam de tratamento distinto.

Não divida quando o custo de coordenação supera o benefício e o contrato só faz sentido como um todo.

## 5. Criando uma tool

Tool é integração executável. Pode ser builtin, processo local, container, WASI module, HTTP service, MCP server ou adapter.

### 5.1 Manifest obrigatório

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

### 5.2 Regras

- nunca receber secret bruto se referência serve;
- validar todos os paths no broker;
- produzir artifacts para payloads grandes;
- logs estruturados e redigidos;
- suportar cancellation quando possível;
- não depender de stdout como único contrato;
- declarar efeitos externos;
- declarar idempotência;
- declarar compensação ou irreversibilidade.

## 6. Criando uma skill

Skill codifica técnica e processo. Ela pode orientar um agente, sugerir tools e definir checks, mas não concede permissões.

### 6.1 Estrutura

```yaml
apiVersion: p50.dev/v1
kind: Skill
metadata:
  id: security.review_auth_boundary
  version: 2.0.0
spec:
  purpose: Avaliar mudanças que alteram autenticação e sessão.
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

### 6.2 Boa skill

- descreve objetivo e método;
- lista armadilhas;
- exige evidência;
- diferencia ausência de problema de falta de inspeção;
- suporta outputs estruturados;
- evita linguagem de aprovação automática;
- possui exemplos positivos e negativos.

### 6.3 Má skill

- é um prompt genérico;
- exige “pense passo a passo” como evidência;
- concede shell/network;
- fixa modelo;
- mistura execução e aprovação;
- não possui tests;
- sempre recomenda mais agentes.

## 7. Criando um agent template

Agent template é opcional. O harness pode sintetizar agents sem template. Templates são úteis quando há identidade operacional estável e histórico relevante.

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

## 8. Criando um evaluator

Evaluator verifica output, artifact, claim ou execution.

Tipos:

- deterministic;
- model-based;
- hybrid;
- human;
- external-system;
- statistical;
- visual.

### 8.1 Contrato

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

Deve declarar:

- model profile;
- independence requirements;
- blind context policy;
- evidence citation requirement;
- calibration dataset;
- disagreement handling;
- max cost.

## 9. Criando uma policy

Policy deve ser pequena, legível, determinística e testável.

### 9.1 Exemplo

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

### 9.2 Testes de policy

Cada policy precisa de fixtures:

- deve disparar;
- não deve disparar;
- waiver permitido;
- waiver proibido;
- conflito com outra policy;
- migration de versão.

## 10. Node types

Node types built-in devem cobrir primitives universais. Crie novo tipo apenas quando lifecycle, UI ou semantics diferirem substancialmente.

### 10.1 `agent`

Executa modelo/runtimes com tools.

### 10.2 `tool`

Executa chamada direta sem agente.

### 10.3 `classifier`

Produz classificação estruturada.

### 10.4 `gate`

Avalia requirement e decide passagem.

### 10.5 `fork` / `join`

Controla paralelismo e merge de artifacts.

### 10.6 `human_decision`

Pausa até resposta ou policy de timeout.

### 10.7 `subgraph`

Invoca graph parametrizado sem esconder eventos internos.

### 10.8 `materializer`

Converte claims/evidence em documento ou projection.

### 10.9 `deploy` / `rollback`

Representa efeito externo com preconditions e compensação.

## 11. Edge design

### 11.1 Data edge

Transfere payload tipado. Evite payload gigante; use artifact refs.

### 11.2 Evidence edge

Declara que output prova requirement de outro nó/gate.

### 11.3 Control edge

Ordena execução sem payload.

### 11.4 Conditional edge

Usa expressão limitada. Deve cobrir missing/unknown.

### 11.5 Failure edge

Roteia failure category.

### 11.6 Compensation edge

Define ação para desfazer efeito externo.

## 12. Completion contracts

Um contrato de conclusão deve ser verificável.

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

Graph Engineer deve declarar:

- scopes relevantes;
- types preferidos;
- temporal window;
- max tokens;
- blind exclusions;
- conflict behavior;
- expansion rules;
- sensitive data handling.

Não inclua documentos inteiros quando symbols/sections bastam.

## 14. Memory design

Memória de agente não é cache de chat. Grave apenas observações reutilizáveis com evidência e TTL.

Bom:

```text
Alterações no PaymentService frequentemente exigem testes de idempotência.
Evidence: exec-482, tests/payments/idempotency.spec.ts
Confidence: 0.87
Expires: 90 dias
```

Ruim:

```text
Eu acho que o backend costuma ser confuso.
```

## 15. Security review para extensões

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

Toda extensão deve passar:

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
11. replay determinism quando aplicável;
12. documentation completeness.

## 17. Performance e custo

Declare:

- cold start;
- warm latency;
- expected token usage;
- output size;
- CPU/memory;
- network egress;
- concurrency limit;
- cache behavior.

O harness usa esses dados para decidir composição.

## 18. Versionamento

- patch: correção sem contrato novo;
- minor: capability backward-compatible;
- major: schema/semantics breaking;
- deprecated components permanecem reproduzíveis;
- migration guide obrigatório para major;
- manifest declara compatible framework range.

## 19. Publicação

Um pacote publicável contém:

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

Registry exibe publisher, trust, permissions, supported platforms, versions, vulnerabilities e compatibility.

## 20. Anti-patterns

- mega-agent com todas as tools;
- workflow de 20 nós para tarefa trivial;
- reviewer lendo somente resumo do executor;
- schema `output: string` para tudo;
- tool com acesso irrestrito ao host;
- policy implementada no prompt;
- graph edge sem comportamento para missing output;
- retry infinito;
- memory sem expiração;
- agent template fixando provider;
- plugin exigindo secrets sem justificativa;
- docs sem provenance;
- evaluator que sempre aprova.

## 21. Checklist de revisão do Graph Engineer

Antes de mergear uma extensão:

- [ ] O problema exige uma capability nova?
- [ ] O contrato é atômico e reutilizável?
- [ ] Inputs e outputs são tipados?
- [ ] Evidência é explícita?
- [ ] Permissions são mínimas?
- [ ] Isolation mínimo está correto?
- [ ] Failure modes são distinguíveis?
- [ ] Cancellation e timeout existem?
- [ ] Há testes positivos e negativos?
- [ ] Há proteção contra secret leakage?
- [ ] A extensão é substituível?
- [ ] Sem provider/model hard-coded sem necessidade?
- [ ] Versionamento e migration estão definidos?
- [ ] UI consegue explicar o que ela faz?
