# Roadmap, critérios de aceite e métricas

## 1. Princípio de entrega

A arquitetura é generalista desde o início, mas a implementação futura deve evoluir por fatias verticais comprováveis. O objetivo não é produzir um editor visual sem motor real nem um motor poderoso sem experiência de controle.

Nenhuma implementação é parte do presente pacote; este documento organiza a sequência futura.

## 2. Fase 0 — especificação

**Resultado:** documentação integral, schemas iniciais, Graph DSL, threat model, decision register e exemplos.

Critérios:

- decisões não se contradizem;
- todos os módulos têm boundaries;
- nenhum workflow fixo por domínio é exigido;
- user override está definido;
- open-source/licensing strategy documentada;
- source references verificadas;
- schemas/examples passam validação básica.

## 3. Fase 1 — vertical slice developer-first

### 3.1 Objetivo

Executar uma mudança de software de ponta a ponta:

```text
prompt
→ task profile
→ dynamic harness
→ visual graph
→ context capsules
→ code edit
→ tests/review
→ docs update
→ audited result
```

### 3.2 Escopo

- Studio local;
- SSH/Docker bootstrap;
- single-node Runtime;
- Workspace/Project/Subproject;
- Codex/Claude native adapters quando oficialmente suportados;
- BYOK/OpenRouter adapter;
- Graph DSL subset completo para core nodes;
- Graph Engine;
- Graph Draft;
- Autopilot/Supervised/Manual;
- Project Agent Registry;
- Context Compiler;
- Event Store;
- initial Knowledge Graph;
- Living Docs;
- Tier 0/1;
- Tool Broker para repository/shell/tests;
- Policy Engine;
- quality gates;
- basic Dreams;
- export/replay;
- public API/CLI.

### 3.3 Fora da Fase 1

- multiuser UI;
- marketplace pago;
- multi-node scheduler;
- Tier 3 production-grade;
- todos os domínios;
- hosted cloud;
- enterprise SSO;
- full visual editor de plugins.

### 3.4 Acceptance scenario

1. usuário instala Runtime em VPS limpa;
2. conecta Studio;
3. autentica uma rota de modelo;
4. importa repositório;
5. pede uma feature;
6. sistema gera Graph v1;
7. execução mapeia, planeja, altera, testa e revisa;
8. usuário remove review e força deploy em ambiente de teste;
9. Graph Draft mostra riscos;
10. user confirma;
11. waiver é registrado;
12. execução pausa se rota atingir quota;
13. user retoma após capacidade;
14. docs e claims são atualizadas;
15. export reproduz timeline.

## 4. Fase 2 — generalização de capacidades

Adicionar tools/capabilities para:

- web research;
- source verification;
- documents;
- data analysis;
- browser automation;
- product/PRD;
- marketing/copy;
- design/image;
- operations;
- external integrations.

Critério: o core não recebe branch de código por domínio; apenas extensões e schemas.

## 5. Fase 3 — ecossistema

- public extension registry;
- signing/trust;
- conformance cloud opcional;
- package discovery;
- community agents/skills;
- alternate registries;
- documentation site;
- Graph Engineer tooling;
- benchmark packs.

## 6. Fase 4 — colaboração e enterprise

- multiuser;
- RBAC;
- approvals;
- comments;
- shared workspaces;
- SSO;
- audit export;
- policy administration;
- multi-VPS workers;
- HA stores;
- compliance controls;
- commercial license operations.

## 7. Fase 5 — distributed agent operating system

- federated runtimes;
- edge/local GPU scheduling;
- organization-wide knowledge boundaries;
- graph exchange/market;
- hosted control plane opcional;
- cross-workspace capability brokerage;
- advanced Dreams experiments;
- formal verification de policies/graphs onde viável.

## 8. Critérios de aceite globais

### 8.1 Harness

- task profile estruturado;
- graph customizado, não template fechado;
- capability snapshot;
- policies aplicadas deterministically;
- graph lint e simulation;
- minimal graph rationale;
- limits de expansion.

### 8.2 Grafo

- node/edge contracts;
- versioning;
- transactional draft;
- adaptive mutation;
- ghost nodes;
- pause/resume;
- user bypass;
- waiver;
- replay.

### 8.3 Contexto

- capsule por node;
- no full history default;
- provenance;
- conflicts;
- expansion request;
- cache/invalidation;
- blind review.

### 8.4 Agentes

- synthesize/reuse;
- Project Agent Registry;
- temporary overlays;
- memory with evidence/TTL;
- performance segmentation;
- no direct graph spawning.

### 8.5 Modelos

- multiple route types;
- official auth only;
- BYOK separated;
- credential isolation;
- capability routing;
- pause on quota;
- no paid fallback automatically.

### 8.6 Segurança

- Tier 0/1 minimum;
- no Docker socket;
- network policy;
- secret broker;
- tool leases;
- cross-project isolation;
- secret scanning;
- audit.

### 8.7 Knowledge/Dreams

- event immutability;
- claims with provenance;
- living docs diff;
- shadow dream;
- independent critic;
- rollback;
- code finding creates normal task.

### 8.8 Open source

- public source;
- public APIs;
- self-host;
- telemetry opt-in;
- schemas/docs;
- AGPL/commercial strategy;
- CLA process;
- reproducible export.

## 9. Métricas north star

### 9.1 Evidence-backed task success

Percentual de execuções aceitas pelo usuário que satisfazem completion contracts sem regressão conhecida dentro da janela definida.

### 9.2 Context efficiency

```text
1 - tokens_sent_with_compiler / estimated_tokens_full_context
```

Não deve ser otimizada isoladamente; acompanhar qualidade.

### 9.3 Orchestration efficiency

- nodes úteis / total nodes;
- evidence gain por node;
- coordination overhead;
- mutation count;
- time on critical path.

## 10. Métricas secundárias

- time to first useful graph;
- time to first evidence;
- human intervention rate;
- manual override rate;
- user correction of Command Router;
- agent reuse precision;
- task cost;
- subscription wait time;
- schema repair rate;
- reviewer disagreement;
- gate catch rate;
- post-completion incident rate;
- docs freshness;
- memory validation rate;
- Dreams rollback rate;
- plugin conformance pass rate.

## 11. Benchmarks

### 11.1 Harness benchmark

Conjunto de tasks de diferentes complexidades. Avaliar:

- graph adequacy;
- required gates;
- redundant nodes;
- cost estimate;
- risk classification;
- context plan.

### 11.2 Context benchmark

- answer/task quality;
- relevant evidence recall;
- irrelevant token ratio;
- conflict detection;
- stale information avoidance.

### 11.3 Reviewer benchmark

- seeded defects;
- clean changes;
- false-positive;
- evidence citation;
- independence benefit.

### 11.4 Graph mutation benchmark

- unexpected dependency;
- auth boundary discovered;
- tool failure;
- quota exhaustion;
- user bypass;
- stale draft;
- no-progress loop.

### 11.5 Security benchmark

- prompt injection;
- secret file;
- postinstall exfiltration;
- symlink escape;
- malicious plugin;
- cross-project retrieval;
- Docker socket access;
- log leakage.

## 12. Quality bars para releases

### Alpha

- data loss e secret leak blockers;
- core flow funciona;
- APIs podem mudar;
- explicit experimental warnings.

### Beta

- schema/API migration policy;
- conformance suite;
- backup/restore;
- security review;
- extension SDK;
- docs completas.

### 1.0

- stable public API/DSL;
- self-host upgrade path;
- threat model validated;
- reliable replay/export;
- contributor governance;
- commercial/legal docs;
- compatibility tests;
- supported platforms.

## 13. Riscos de execução do produto

### 13.1 Complexidade sistêmica

Mitigar com boundaries, vertical slice, contract-first e no fixed packs.

### 13.2 UI sobre motor imaturo

Mitigar desenvolvendo cada interação contra API real e event stream.

### 13.3 Orchestrator overthinking

Mitigar com minimal graph objective, budgets e deterministic gates.

### 13.4 Provider auth changes

Mitigar com adapters, official flows, health metadata, graceful disable e documented verification dates.

### 13.5 Token savings prejudicam qualidade

Mitigar com expansion, evidence recall metrics e benchmark.

### 13.6 Dreams corrompe conhecimento

Mitigar com shadow, tests, critic, atomic commit e rollback.

### 13.7 Owner override causa incidente

Mitigar com clear impact, waiver, rollback tools e incident correlation, sem retirar soberania.

### 13.8 Open-source contribution friction

Mitigar com CLA simples, governance pública e value claro para contributors.

## 14. Definition of Done da documentação

Este pacote é considerado integral quando:

- MASTER PRD existe;
- decision register contém todas as escolhas;
- telas e comportamentos estão especificados;
- harness e Graph Engineer docs são profundos;
- DSL e schemas existem;
- context/knowledge/Dreams estão definidos;
- model gateway cobre BYOK/assinatura/local;
- security e threat model existem;
- operations/recovery existem;
- open-source/licensing/governance existem;
- examples demonstram cenários;
- referências oficiais e data de verificação existem;
- arquivos passam validação de links locais, JSON e YAML.
