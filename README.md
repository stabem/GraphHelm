# Programação 5.0 — documentação integral do produto

> **Codinome:** Programação 5.0  
> **Categoria:** sistema operacional open source para agentes de IA  
> **Estado deste repositório:** especificação de produto e arquitetura; nenhuma implementação incluída  
> **Versão da especificação:** 0.1.0  
> **Data-base:** 2026-08-08

Programação 5.0 é uma plataforma local-first em que o usuário controla, por uma interface visual, uma infraestrutura agentiva executada em sua própria VPS. Cada pedido é classificado, decomposto e convertido em um grafo de execução específico para o cenário. O sistema seleciona ou cria agentes, modelos, ferramentas, contexto, isolamento, testes e revisões sem depender de workflows fixos por domínio.

O produto é composto por três superfícies abertas:

1. **Framework** — Graph Engine, Harness Compiler, Context Compiler, Policy Engine, Agent Registry, Model Gateway, Dreams Engine e protocolos.
2. **Runtime** — daemon instalado na VPS do usuário, responsável por execução, sandboxes, eventos, artefatos, credenciais e jobs.
3. **Studio** — aplicativo local para chat, grafo, agentes em execução, documentos, arquivos, auditoria e controle soberano do workflow.

## Como ler

- [MASTER_PRD.md](MASTER_PRD.md): documento consolidado e normativo.
- [docs/DECISION_REGISTER.md](docs/DECISION_REGISTER.md): todas as escolhas aprovadas durante a definição.
- [docs/product/PRODUCT_REQUIREMENTS.md](docs/product/PRODUCT_REQUIREMENTS.md): requisitos funcionais e não funcionais.
- [docs/ux/STUDIO_SPEC.md](docs/ux/STUDIO_SPEC.md): telas, componentes, estados e interações.
- [docs/architecture/SYSTEM_ARCHITECTURE.md](docs/architecture/SYSTEM_ARCHITECTURE.md): arquitetura de alto nível e topologia.
- [docs/harness/HARNESS_SPEC.md](docs/harness/HARNESS_SPEC.md): especificação completa do harness dinâmico.
- [docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md](docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md): guia para construir capacidades, agentes, gates e extensões.
- [docs/graph-engineer/GRAPH_DSL_SPEC.md](docs/graph-engineer/GRAPH_DSL_SPEC.md): DSL tipada de grafos.
- [docs/context/CONTEXT_KNOWLEDGE_DREAMS.md](docs/context/CONTEXT_KNOWLEDGE_DREAMS.md): contexto de baixo consumo, conhecimento e Dreams Engine.
- [docs/agents/AGENTS_SKILLS_PLUGINS.md](docs/agents/AGENTS_SKILLS_PLUGINS.md): ciclo de vida de agentes, skills, tools e plugins.
- [docs/models/UNIVERSAL_MODEL_GATEWAY.md](docs/models/UNIVERSAL_MODEL_GATEWAY.md): BYOK, assinaturas, rotas locais e política de capacidade.
- [docs/security/SECURITY_ISOLATION_THREAT_MODEL.md](docs/security/SECURITY_ISOLATION_THREAT_MODEL.md): isolamento, secrets e threat model.
- [docs/architecture/DATA_AND_PROTOCOLS.md](docs/architecture/DATA_AND_PROTOCOLS.md): entidades, eventos, APIs e contratos.
- [docs/operations/OBSERVABILITY_AND_RECOVERY.md](docs/operations/OBSERVABILITY_AND_RECOVERY.md): métricas, checkpoints, replay e recuperação.
- [docs/open-source/GOVERNANCE_AND_LICENSING.md](docs/open-source/GOVERNANCE_AND_LICENSING.md): AGPLv3, licença comercial, CLA e governança.
- [docs/product/ROADMAP_AND_ACCEPTANCE.md](docs/product/ROADMAP_AND_ACCEPTANCE.md): fases, critérios de aceite e métricas.
- [docs/reference/EXAMPLE_EXECUTIONS.md](docs/reference/EXAMPLE_EXECUTIONS.md): exemplos completos de grafos.
- [docs/reference/REFERENCE_STACK_AND_ADRS.md](docs/reference/REFERENCE_STACK_AND_ADRS.md): stack de referência e decisões arquiteturais.
- [docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md](docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md): fontes oficiais verificadas.
- [schemas/](schemas/): contratos JSON Schema.
- [examples/](examples/): grafos e manifests de exemplo.

## Princípios constitucionais

1. **Nenhuma função essencial depende de servidor proprietário.**
2. **O harness propõe e governa; o usuário continua soberano.**
3. **Cada tarefa recebe um grafo específico, não um pack fixo.**
4. **Toda afirmação importante precisa de proveniência e evidência.**
5. **Agentes compartilham artefatos e contexto compilado, não chats inteiros.**
6. **Qualidade é comprovada por gates e evidências, não por autoconfiança do executor.**
7. **Permissões, contexto e segredos seguem o mínimo privilégio.**
8. **Toda mutação operacional do grafo é versionada, transacional e reversível.**
9. **O sistema deve buscar o menor grafo capaz de produzir evidência suficiente.**
10. **O núcleo e os protocolos são públicos, documentados e substituíveis.**

## Escopo deste pacote

Este repositório descreve o produto inteiro, incluindo a visão generalista. A primeira fatia recomendada continua sendo developer-first, mas a arquitetura não depende do domínio de programação. Pesquisa, produto, marketing, dados, documentos, design, automação, operações e outros trabalhos usam o mesmo catálogo de capacidades atômicas e o mesmo compilador de grafos.

## Status jurídico

A estratégia proposta é AGPLv3 para a edição comunitária, licença comercial alternativa sobre a mesma base de código e CLA não exclusivo para contribuições. Os textos definitivos de licença comercial, ICLA e CCLA devem ser redigidos e validados por assessoria jurídica antes de publicação.
