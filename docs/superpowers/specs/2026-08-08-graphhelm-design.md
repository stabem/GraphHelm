# GraphHelm — design aprovado

## Status

Design consolidado em 2026-08-08 e aprovado como base normativa para planejamento e implementação incremental. Este repositório ainda não contém implementação.

## Visão

GraphHelm é um sistema operacional open source para agentes de IA. O Studio roda localmente e controla um Runtime instalado via SSH + Docker na VPS do usuário. Todo pedido passa por classificação, capability discovery, síntese/reuso de agentes, compilação de contexto, arquitetura de grafo, policies, lint e execução.

O harness é criado por tarefa a partir de capabilities atômicas; não existem packs fixos por domínio. O grafo é adaptativo, versionado e editável. O usuário é soberano e pode pausar, remover gates ou forçar deploy, com impact report e waiver. Agentes propostos após intervenção manual não iniciam sem confirmação.

O sistema usa Event Store imutável, Project Knowledge Graph e Living Documentation. Context Capsules minimizam tokens. Dreams mantém documentos, claims, memórias, agentes e skills em shadow workspace; achados de código geram tarefas normais.

Modelos entram por Universal Model Gateway: BYOK, APIs, agregadores, runtimes oficiais de assinatura e modelos locais. Limite de assinatura pausa a execução; não há fallback pago automático.

O projeto será integralmente open source, AGPLv3 + licença comercial alternativa, com CLA não exclusivo.

## Decisões completas

Consultar:

- `MASTER_PRD.md`
- `docs/DECISION_REGISTER.md`
- `docs/harness/HARNESS_SPEC.md`
- `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md`
- `docs/graph-engineer/GRAPH_DSL_SPEC.md`
- `docs/ux/STUDIO_SPEC.md`
- demais documentos listados em `docs/INDEX.md`.

## Gate de implementação

A implementação só deve começar após revisão explícita desta documentação pelo usuário e criação de um plano separado. Este repositório não contém scaffold, código de produto ou mudanças de infraestrutura.
