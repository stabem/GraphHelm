# Observabilidade, operações e recuperação

## 1. Objetivo

Dar ao usuário visibilidade completa sobre o comportamento do sistema e permitir pause, resume, replay, diagnóstico e recuperação sem repetir trabalho válido ou ocultar falhas.

## 2. Pilares de observabilidade

### 2.1 Events

Fonte operacional append-only. Toda transição relevante gera evento tipado.

### 2.2 Metrics

Séries e agregações locais de desempenho, custo, qualidade, capacidade e segurança.

### 2.3 Logs

Logs estruturados para diagnóstico, redigidos por sensitivity.

### 2.4 Traces

Trace de ponta a ponta: prompt → harness → graph → nodes → model/tool → artifacts → docs.

### 2.5 Artifacts

Outputs persistentes que permitem inspeção e reprodução.

## 3. Correlação

IDs obrigatórios:

- workspace_id;
- project_id;
- execution_id;
- graph_version;
- node_id;
- attempt_id;
- agent_runtime_id;
- model_call_id;
- tool_call_id;
- trace_id;
- artifact_id.

## 4. Métricas do harness

- time_to_profile;
- time_to_first_graph;
- graph_nodes_initial;
- graph_nodes_final;
- mutation_count;
- unnecessary_node_estimate;
- policy_gates_added;
- graph_lint_errors;
- graph_simulation_failures;
- cost/time estimate error;
- capability gaps;
- agent reuse rate.

## 5. Métricas de execução

- queue time;
- runtime duration;
- node duration;
- concurrency;
- retry count;
- pause/wait time;
- success/failure by category;
- invalidated outputs;
- checkpoint duration;
- resume success;
- sandbox cold start;
- artifact throughput.

## 6. Métricas de modelos

- route availability;
- latency;
- input/output tokens quando disponíveis;
- monetary cost quando disponível;
- subscription throttles;
- waiting capacity time;
- schema compliance;
- tool success;
- cancellation latency;
- model switch count;
- quality by task class.

## 7. Métricas de contexto

- tokens allocated/consumed;
- retrieval count;
- cache hit;
- expansion requests;
- expansion accepted/denied;
- context redundancy;
- stale item rate;
- relevant evidence coverage;
- tokens saved vs full-context baseline;
- contradiction count presented.

## 8. Métricas de qualidade

- gate pass/fail;
- finding severity;
- reviewer disagreement;
- false positives confirmed later;
- regressions after completion;
- evidence coverage;
- unsupported claim count;
- user acceptance/rework;
- manual override rate;
- completed_with_waivers rate.

## 9. Métricas de Dreams

- cycles;
- duration;
- proposals;
- commit/discard;
- rollback;
- docs consolidated;
- claims updated;
- memories expired;
- agents merged;
- context savings;
- generated task precision.

## 10. SLOs iniciais

SLOs de referência, a serem calibrados por hardware/provider:

- Runtime API local/VPN availability: 99,5% mensal;
- event propagation ao Studio: p95 < 500 ms em rede saudável;
- graph state write: p95 < 250 ms, excluindo model/tool;
- durable checkpoint metadata: p95 < 2 s;
- resume de node checkpointável: sucesso > 99%;
- no secret in persisted logs: 100% esperado, tratado como incidente;
- graph mutation atomicity: 100%;
- event idempotency under retry: 100%;
- Studio canvas interaction: 60 fps alvo com 1.000 nós em máquina de referência;
- Runtime recovery after reboot: < 2 min para reconstruir scheduler, sem contar container/model cold start.

## 11. Checkpoints

### 11.1 Tipos

- node pre-call;
- model turn;
- tool call boundary;
- artifact produced;
- sandbox filesystem snapshot;
- graph mutation safe point;
- external effect receipt;
- human decision.

### 11.2 Conteúdo

- graph version;
- node state;
- attempt;
- input refs;
- context capsule ref;
- artifacts;
- tool/model session refs;
- leases;
- sandbox snapshot ref;
- pending timers;
- compensation state.

Secrets não entram no checkpoint; apenas references.

### 11.3 Resume

Antes de retomar:

- validar graph version;
- validar dependencies;
- validar source snapshot;
- renovar leases;
- revalidar route health;
- reabrir/recriar sandbox;
- invalidar session não segura;
- emitir event.

## 12. Pause semantics

### Graceful pause

Aguarda call atual terminar, persiste output e para próximo step.

### Immediate stop

Cancela model/tool, mata process/sandbox se necessário e retorna ao último checkpoint seguro.

### Branch pause

Pausa somente descendants afetados. Branches independentes continuam.

### Global pause

Impede novos nodes; running nodes obedecem chosen policy.

## 13. Cancel semantics

Cancel não apaga histórico. Efeitos externos já realizados exigem compensation. Status final registra partial effects.

## 14. Retry

Categorias:

- transient provider;
- rate limit;
- tool timeout;
- malformed output;
- sandbox crash;
- deterministic failure;
- policy denied;
- invalid input.

Somente categorias configuradas retry. Quota esgotada de assinatura entra em wait, não retry agressivo.

## 15. No-progress detection

Detectar:

- outputs semanticamente idênticos;
- retries sem mudança de contexto/strategy;
- remediation loop recorrente;
- graph mutation alternando estados;
- agent delegation chain;
- repeated tool failure;
- budget consumption sem evidence gain.

Ações:

- stop branch;
- change strategy;
- add critic;
- request human decision;
- mark blocked;
- preserve diagnostics.

## 16. Failure categories

- user_input;
- context_missing;
- schema_invalid;
- model_auth;
- model_quota;
- model_provider;
- model_output;
- tool_permission;
- tool_runtime;
- sandbox;
- policy;
- graph;
- storage;
- network;
- external_effect;
- internal_bug;
- cancelled.

Cada failure inclui retriable, severity, evidence e recommended actions.

## 17. Recovery scenarios

### 17.1 Studio fecha

Sem efeito no Runtime. Ao reabrir, Studio busca snapshot e stream desde último sequence.

### 17.2 Runtime reinicia

- lock de recovery;
- carregar nonterminal executions;
- verificar leases expiradas;
- marcar running nodes como recovering;
- reconciliar sandboxes;
- retomar ou rollback ao checkpoint;
- emitir recovery report.

### 17.3 Worker morre

Lease de worker expira. Scheduler reatribui attempt conforme idempotência e effect state.

### 17.4 Banco indisponível

Parar novas mutations/side effects; nodes podem terminar call atual, mas não confirmar sucesso sem durable event. Buffer local limitado não substitui persistência para efeitos críticos.

### 17.5 Artifact store indisponível

Node não conclui output grande até persistência. Pequenos payloads podem ficar pending dentro de limite seguro.

### 17.6 Model quota

`waiting_for_model_capacity`; sem fallback automático.

### 17.7 Sandbox cleanup falha

Quarantine; não reuse; alert; cleanup job privilegiado separado.

### 17.8 Graph draft stale

Studio recebe diff, rebase e precisa reconfirmar changes operacionais.

## 18. Replay

Replay reconstrói:

- Graph Version ao longo do tempo;
- node states;
- model/tool calls metadata;
- artifacts;
- user interventions;
- waivers;
- knowledge/doc updates.

Modos:

- visual timeline;
- deterministic simulation;
- re-execution with same refs;
- re-execution with substituted models;
- branch-only replay;
- failure reproduction.

Re-execution cria nova execution e não altera original.

## 19. Export e backup

### 19.1 Export

- project metadata;
- graphs;
- agents/skills;
- policies;
- docs;
- claims/evidence;
- artifacts optional;
- events optional;
- execution manifests;
- no secrets.

### 19.2 Backup

- encrypted database dump;
- artifact snapshot;
- documents/repositories;
- vault backup separately encrypted;
- config and certificates;
- restore test.

### 19.3 Disaster recovery

Runbook define RPO/RTO conforme deployment. Single-node default: daily full + frequent incremental events/artifacts. Produção empresarial pode usar streaming replicas/object versioning.

## 20. Retention

Configurable por data class:

- raw prompts;
- model outputs;
- tool logs;
- artifacts;
- traces;
- metrics;
- events;
- docs;
- claims;
- secrets access audit.

Event Store “imutável” significa não reescrever dentro do período de retenção. Expiração legal/owner pode criar tombstone/cryptographic erasure conforme design de compliance, preservando metadata mínima e audit do deletion.

## 21. Alertas

- runtime offline;
- auth required;
- quota exhausted;
- execution blocked;
- production effect failed;
- secret scan finding;
- sandbox quarantined;
- storage pressure;
- graph no-progress;
- Dreams regression/rollback;
- stale critical doc;
- plugin vulnerability;
- backup failed.

Canais são plugins; local notifications default.

## 22. Health dashboard

- API;
- database;
- artifact store;
- worker pool;
- sandbox host;
- model routes;
- credential broker;
- scheduler;
- Dreams;
- disk/memory/CPU;
- pending migrations;
- quarantines.

## 23. OpenTelemetry

Reference implementation usa OpenTelemetry semantics e export local. External collector é opcional. Sensitive attributes são filtered antes do exporter.

## 24. Runbooks obrigatórios

- install failure;
- update rollback;
- runtime recovery;
- database restore;
- vault recovery;
- model reconnect;
- sandbox quarantine;
- secret leakage;
- compromised plugin;
- graph stuck;
- production deploy failure;
- Dreams rollback;
- project export/import.

## 25. Critérios de aceite

- restart não perde completed outputs;
- duplicate event não duplica efeito;
- pause branch preserva outras branches;
- immediate stop retorna a checkpoint conhecido;
- replay mostra Graph Versions corretas;
- quota wait não gasta BYOK;
- secret não aparece em logs/export;
- quarantine impede reuse;
- backup/restore é testável;
- metrics são locais por padrão.
