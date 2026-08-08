# Agentes, skills, tools e plugins

## 1. Visão

Programação 5.0 trata agentes como configurações temporárias de trabalho, não como personagens permanentes. O valor está na combinação de objetivo, capability, contexto, permissões, modelo, contrato e evidência. Agentes úteis podem ser persistidos no projeto, mas continuam versionados e auditáveis.

## 2. Taxonomia

### 2.1 Capability

Descrição atômica do que pode ser feito.

### 2.2 Tool

Mecanismo executável que oferece uma ou mais capabilities.

### 2.3 Skill

Conhecimento operacional/instrução que orienta como usar capabilities para um objetivo.

### 2.4 Agent Definition

Configuração persistente e reutilizável.

### 2.5 Agent Runtime

Instância efêmera ligada a node, graph version e execution.

### 2.6 Agent Experience

Memórias e métricas acumuladas, subordinadas a evidence e TTL.

### 2.7 Plugin

Pacote instalável que adiciona capability, tool, skill, evaluator, policy, model adapter, retriever, visualizer ou outro extension point.

## 3. Ciclo de vida do agente

```text
Need identified
→ Project Agent Registry search
→ match/reuse OR synthesize ephemeral
→ lint definition
→ bind model/context/tools
→ instantiate runtime
→ execute
→ evaluate
→ create memory candidates
→ optionally save/promote by explicit user action
→ active/suspended/deprecated/archived
```

## 4. Project Agent Registry

### 4.1 Escopo

- subproject-local;
- project-shared;
- workspace-shared;
- imported read-only;
- community package.

Promoção entre scopes é explícita.

### 4.2 Metadados

- purpose;
- versions;
- capabilities;
- contracts;
- default permissions;
- preferred model profiles;
- context strategy;
- memory policy;
- graph affinities;
- performance metrics;
- status;
- provenance;
- publisher/signature para externos.

### 4.3 Matching

O Agent Matcher retorna:

```yaml
agent_match:
  agent: project/payment-reviewer@4
  score: 0.91
  strengths:
    - objective_fit
    - strong_project_history
    - contract_compatible
  weaknesses:
    - model_route_currently_degraded
  required_overlays:
    - add_scope: refunds
  alternative: synthesize_new
```

### 4.4 Não reuso cego

Reuso é proibido quando:

- contract incompatível;
- permission insuficiente;
- memory contradita/expirada afeta objetivo;
- version incompatible;
- status suspended/deprecated sem explicit pin;
- project scope não permite;
- required isolation não suportado.

## 5. Agentes efêmeros

### 5.1 Geração

O Agent Synthesizer recebe subtask contract e capability catalog. Ele produz definição completa, não apenas system prompt.

### 5.2 Persistência

Por padrão, definição efêmera fica no Execution Manifest. Ela só entra no Project Agent Registry quando:

- usuário clica `Salvar como agente`;
- usuário salva o node como template;
- API explícita promove;
- import de manifest é confirmado.

Dreams pode criar e ativar uma nova versão de agente somente pelo workflow governado de shadow validation e conforme a policy do projeto. Ele nunca transforma automaticamente um overlay ad hoc de nó em definição estável apenas porque a execução teve sucesso.

## 6. Overlays de execução

Uma instância pode alterar:

- objective;
- instructions;
- model route/profile;
- context policy;
- tools;
- permissions;
- completion contract;
- retry;
- isolation;
- memory writes.

Essas alterações pertencem somente ao node/execution. Ao terminar, não mudam Agent Definition.

## 7. Memória

### 7.1 Tipos

- `strategy_outcome`;
- `known_pitfall`;
- `project_pattern`;
- `evaluation_feedback`;
- `tool_limitation`;
- `context_hint`;
- `self_limitation`.

### 7.2 Status

- candidate;
- validated;
- contradicted;
- deprecated;
- expired.

### 7.3 Leitura

Agent Experience entra na Context Capsule somente quando:

- scope corresponde;
- não expirou;
- relevance alta;
- evidence disponível;
- não conflita silenciosamente com claim atual;
- budget permite.

### 7.4 Escrita

Agente propõe memory candidate. Memory Validator e policy decidem persistência.

## 8. Avaliação de agentes

Métricas por task class:

- completion success;
- evidence completeness;
- output schema compliance;
- reviewer findings;
- regressions posteriores;
- false-positive/negative;
- token/cost;
- duration;
- context expansion;
- retries;
- user overrides;
- usefulness rating;
- calibration.

Scores devem ser segmentados; média global é enganosa.

## 9. Status e manutenção

### Active

Disponível para matching.

### Suspended

Não selecionado automaticamente; pode ser pinned manualmente.

### Deprecated

Substituído, mas reproduzível.

### Archived

Somente histórico/import.

### Quarantined

Suspeita de segurança/integridade.

Dreams pode alterar status, criar versões, fundir ou arquivar agentes após shadow validation, policy checks e rollback disponível. Toda mudança permanece versionada e auditável.

## 10. Skills

### 10.1 Conteúdo

Uma skill pode conter:

- purpose;
- applicability;
- method;
- checklists;
- examples;
- anti-patterns;
- required/recommended capabilities;
- context hints;
- completion requirements;
- evaluator recommendations;
- conformance tests.

### 10.2 Composição

O harness pode carregar múltiplas skills. Conflicts são detectados por declared constraints e semantic lint. Uma skill não pode mudar hard policy.

### 10.3 Context cost

Skills grandes são segmentadas. O agent recebe apenas sections relevantes, com ref para expandir.

### 10.4 Qualidade

Skill score usa:

- task success uplift;
- error reduction;
- token overhead;
- generalization;
- conformance;
- freshness;
- reviewer agreement.

## 11. Tools

### 11.1 Tool categories

- repository;
- filesystem;
- shell;
- tests/build;
- browser;
- web/search;
- database;
- cloud;
- design/media;
- communication;
- document;
- data analysis;
- deployment;
- security scanner;
- model runtime.

### 11.2 Tool Broker

Toda call passa por:

1. schema validation;
2. identity check;
3. capability lease;
4. policy;
5. path/network/secret validation;
6. sandbox routing;
7. execution;
8. redaction;
9. artifact persistence;
10. event emission.

### 11.3 Efeitos

Tool declara:

- read-only;
- reversible write;
- irreversible write;
- external side effect;
- production effect;
- secret use;
- network egress.

Isso influencia gate e isolation.

## 12. Plugins

### 12.1 Tipos

- `capability-provider`
- `tool`
- `skill-package`
- `agent-package`
- `model-adapter`
- `evaluator`
- `policy-pack`
- `retriever`
- `document-materializer`
- `sandbox-adapter`
- `trigger`
- `visualizer`
- `deployment-adapter`
- `dream-strategy`

### 12.2 Runtime models

- OCI container;
- WASI/WASM;
- local process com broker;
- remote HTTP/gRPC;
- MCP server;
- pure data package.

Plugins não rodam in-process no Runtime core por padrão.

### 12.3 Manifest

```yaml
apiVersion: p50.dev/v1
kind: Extension
metadata:
  id: community/playwright-tool
  version: 1.2.0
  publisher: did:key:...
spec:
  type: tool
  capabilities:
    - browser_navigation
    - screenshot_capture
    - dom_inspection
  permissions:
    network: required
    filesystem: optional
    secrets: none
  contracts:
    input: schema://BrowserCommand@1
    output: schema://BrowserArtifact@1
  runtime:
    kind: oci
    image: registry/...@sha256:...
    isolationMinimum: tier_2
  compatibility:
    framework: ">=0.1 <1.0"
    platforms:
      - linux_amd64
      - linux_arm64
  telemetry:
    external: false
```

### 12.4 Instalação

Fluxo:

1. resolver package e signature;
2. mostrar publisher/trust;
3. mostrar permission diff;
4. verificar vulnerability/license;
5. baixar por hash;
6. executar conformance sandbox;
7. habilitar no scope escolhido;
8. registrar event.

### 12.5 Atualização

Nunca autoampliar permissions. Se nova versão pede acesso adicional, exige confirmação.

## 13. Community Registry

O registry pode ser central ou federado, mas instalação não depende de serviço proprietário. Metadata pública:

- package/version/hash;
- source repository;
- license;
- publisher;
- signature;
- permissions;
- trust;
- compatibility;
- vulnerabilities;
- download count opcional;
- conformance results;
- reproducible build status.

Runtime aceita registries customizados e package local.

## 14. Supply chain

- pin por digest;
- signatures;
- SBOM;
- provenance attestation;
- reproducible builds desejáveis;
- vulnerability scan;
- dependency policy;
- quarantine/revoke;
- no mutable `latest` em execution manifests.

## 15. Agent-to-agent communication

Agentes não conversam por chat global. Comunicação acontece por:

- typed artifacts;
- node outputs;
- evidence refs;
- graph signals;
- human decisions;
- event triggers.

Um coordinator agent pode existir, mas também usa contratos e não recebe authority irrestrita.

## 16. Delegação

Agent pode solicitar subtask emitindo Graph Signal `delegation_requested`. Graph Governor decide criar node. Agent não spawna runtime arbitrariamente.

## 17. Identidade de agente

Cada Agent Runtime possui:

- runtime ID;
- definition/version;
- node/execution;
- actor identity;
- model route;
- leases;
- sandbox;
- context capsule;
- timestamps.

Tool Broker usa essa identidade para authorization e audit.

## 18. Segredos

Agentes recebem secret references, nunca valor no prompt quando avoidable. Tool/model runtime resolve no broker. Qualquer output é scanned/redacted antes de persistir.

## 19. Marketplace e comercialização futura

O ecossistema pode permitir packages gratuitos ou pagos, mas:

- formato permanece aberto;
- side-loading é permitido;
- runtime não exige marketplace oficial;
- package pago não pode esconder permissions;
- license precisa ser declarada;
- community edition continua capaz de executar extensions compatíveis.

## 20. Critérios de aceite

- agent definition não é sinônimo de prompt;
- reuso pesquisa Project Agent Registry antes de síntese;
- node overlay não muda agent salvo;
- memory possui evidence/TTL/status;
- tool calls passam por broker;
- agent não cria agent runtime diretamente;
- plugin permissions são visíveis;
- update com permission expansion pede confirmação;
- packages são pinned por version/hash;
- communication usa artifacts, não chat global.
