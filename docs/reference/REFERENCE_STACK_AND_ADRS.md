# Stack de referência e ADRs iniciais

## 1. Status

A arquitetura normativa é independente de linguagem. Esta seção recomenda uma implementação de referência coerente com segurança, portabilidade, comunidade e extensibilidade. Não há código implementado.

## 2. Stack de referência

### Studio

- Tauri 2;
- React + TypeScript;
- React Flow ou canvas compatível por adapter;
- state management event-sourced/cache local;
- Monaco editor para DSL/schemas;
- Markdown/Mermaid renderers;
- OS keychain para identidade local.

### Runtime core

- Rust para daemon, Graph Engine, Policy Engine, Tool Broker, Credential Broker e sandbox orchestration;
- async runtime;
- gRPC/Connect-compatible API com Protobuf como contrato binário e JSON mapping;
- WebSocket/SSE-compatible event streaming.

### SDKs

- TypeScript;
- Python;
- CLI multiplataforma;
- generated clients a partir dos schemas/protocols.

### Extensões

- OCI containers como formato universal;
- WASI/WASM para componentes leves e mais restritos;
- MCP/HTTP/gRPC adapters;
- pure-data packages para skills/policies/schemas.

### Persistência

- PostgreSQL + JSONB + full-text + pgvector;
- artifact store content-addressed em filesystem, com adapter S3-compatible;
- Git para repositórios e docs;
- encrypted vault local com adapter para Vault/KMS.

### Observabilidade

- OpenTelemetry;
- logs estruturados;
- Prometheus-compatible metrics;
- exporters externos opcionais.

### Isolamento

- Docker/Podman rootless para Tier 1/2;
- seccomp/AppArmor/SELinux;
- gVisor/Kata/Firecracker adapter para Tier 3;
- Git worktrees/snapshots;
- egress proxy.

## 3. ADR-001 — Studio local, Runtime na VPS

**Status:** accepted.

**Contexto:** usuário quer interface local e execução/dados na própria infraestrutura.

**Decisão:** Studio atua como control plane; VPS como execution/data plane.

**Consequências positivas:** privacidade, runtime contínuo, recursos maiores, acesso remoto controlado.

**Consequências negativas:** bootstrap, rede, certificado e diagnóstico são mais complexos.

## 4. ADR-002 — SSH apenas para bootstrap/maintenance

**Status:** accepted.

**Decisão:** usar SSH para diagnóstico e instalação. Operação normal ocorre pela Public Runtime API com mTLS.

**Motivo:** não modelar controle em parsing de terminal e permitir SDKs.

## 5. ADR-003 — Graph DSL declarativa e tipada

**Status:** accepted.

**Decisão:** YAML/JSON representation com schemas e semantics próprias; Graph Version imutável.

**Alternativa rejeitada:** armazenar workflow apenas como código de aplicação ou prompts.

## 6. ADR-004 — Policy Engine separado de LLM

**Status:** accepted.

**Decisão:** LLM produz sinais/propostas; motor determinístico aplica invariantes.

**Motivo:** segurança e reprodutibilidade.

## 7. ADR-005 — Event Store append-only

**Status:** accepted.

**Decisão:** transições e evidências são eventos imutáveis. Projections podem ser reconstruídas.

**Consequência:** storage/retention precisam de política; auditoria e replay ficam robustos.

## 8. ADR-006 — Knowledge Graph em PostgreSQL inicialmente

**Status:** recommended.

**Decisão:** modelar entities/relations/claims em tabelas/JSONB e adapter interface. Não exigir graph database separado na primeira fatia.

**Motivo:** menos componentes operacionais, transações e backup simples.

**Evolução:** adapters para Neo4j/AGE/outros podem existir.

## 9. ADR-007 — Content-addressed artifacts

**Status:** accepted.

**Decisão:** artifacts imutáveis por hash, metadata no banco, bytes em filesystem/S3.

**Motivo:** dedup, provenance, reproducibility e cache.

## 10. ADR-008 — Core em Rust, extensões fora do processo

**Status:** recommended.

**Decisão:** core de confiança em Rust; extensões via container/WASI/process/remote protocols.

**Motivo:** segurança de memória, performance e não carregar plugin arbitrário no processo privilegiado.

## 11. ADR-009 — Protobuf semantics com JSON/YAML views

**Status:** recommended.

**Decisão:** protocolos de runtime definidos em Protobuf ou IDL equivalente; Graph DSL/manifests em YAML/JSON com JSON Schema.

**Motivo:** streaming, clients gerados e experiência humana.

## 12. ADR-010 — Queue inicialmente no PostgreSQL

**Status:** recommended.

**Decisão:** single-node scheduler usa durable jobs/leasing no PostgreSQL. Message bus externo é adapter futuro.

**Motivo:** reduzir complexidade da V1.

**Condição de evolução:** multi-node scale, throughput ou isolation demandando NATS/Kafka-like bus.

## 13. ADR-011 — Secret broker separado

**Status:** accepted.

**Decisão:** secrets criptografados e resolvidos por broker. Nunca Graph DSL/context artifact.

## 14. ADR-012 — Native model runtimes como adapters de primeira classe

**Status:** accepted.

**Decisão:** Codex/Claude Code não são tratados apenas como APIs de chat. Adapter modela session, tools, permissions e quota.

## 15. ADR-013 — Sem fallback pago automático

**Status:** accepted.

**Decisão:** quota de assinatura pausa execution. Manual switch obrigatório para BYOK.

## 16. ADR-014 — Graph mutation por Governor

**Status:** accepted.

**Decisão:** agents emitem signals; somente Graph Governor publica mutations.

## 17. ADR-015 — User graph edits transacionais

**Status:** accepted.

**Decisão:** layout visual é local/imediato; topology/config operacional vira Graph Draft atômico.

## 18. ADR-016 — Agent node overlays não promovem definição

**Status:** accepted.

**Decisão:** alterações em runtime valem somente para execution. Reuso exige save explícito.

## 19. ADR-017 — Dreams em shadow workspace

**Status:** accepted.

**Decisão:** cognitive changes são testadas em snapshot e committed atomicamente. Code changes viram normal task.

## 20. ADR-018 — AGPLv3 + commercial license

**Status:** accepted subject to legal review.

**Decisão:** dual licensing com non-exclusive CLA.

## 21. ADR-019 — Single-user first com actor identity

**Status:** accepted.

**Decisão:** V1 possui owner local, mas todo event/action inclui actor e authorization model extensível.

## 22. ADR-020 — No mandatory central service

**Status:** accepted.

**Decisão:** update registry, extension registry, hosted telemetry e provisioning são opcionais/substituíveis.

## 23. Repositório de referência

```text
graphhelm/
├── apps/
│   ├── studio/
│   └── runtime/
├── core/
│   ├── graph/
│   ├── harness/
│   ├── policy/
│   ├── context/
│   ├── knowledge/
│   ├── agents/
│   ├── models/
│   ├── tools/
│   ├── sandbox/
│   ├── dreams/
│   └── protocols/
├── sdk/
│   ├── typescript/
│   └── python/
├── extensions/
│   └── builtin/
├── schemas/
├── conformance/
├── examples/
├── docs/
├── rfcs/
└── adrs/
```

## 24. Dependency principles

- core modules depend on interfaces, not adapters;
- Studio does not import Runtime internals;
- Policy Engine has no model dependency;
- Event schemas are shared package;
- plugins cannot link privileged core internals;
- model/provider SDKs live in adapters;
- database queries behind repositories;
- sandbox backends behind interface;
- no circular module dependencies.

## 25. Build/release principles

- reproducible builds target;
- signed artifacts;
- SBOM;
- pinned toolchains;
- migration checks;
- conformance suite;
- cross-platform Studio builds;
- x86_64/arm64 Runtime images;
- release manifest with hashes.
