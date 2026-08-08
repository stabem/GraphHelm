# GraphHelm — decisão de nome

## Status

**Nome de produto selecionado para desenvolvimento:** `GraphHelm`.

Esta decisão define o nome de trabalho do framework, do Runtime e do Studio. Ela não substitui pesquisa jurídica de marca, aquisição de domínios ou reserva de namespaces em registries antes do lançamento público.

## Por que GraphHelm

- **Graph** representa a abstração central do produto: cada demanda é compilada em um grafo executável, adaptativo, versionado e editável.
- **Helm** representa direção e soberania: o harness governa a execução por padrão, enquanto o usuário permanece no comando e pode alterar o fluxo.
- O nome funciona para software, pesquisa, automação, documentos, operações e outros domínios; não limita o sistema a agentes de programação.
- É curto, pronunciável, técnico e adequado a um framework open source global.

## Posicionamento

**Categoria:** open-source agent operating system and control plane.

**Tagline principal:**

> The open-source control plane for governed AI agents.

**Linha curta de produto:**

> Compose agents. Govern every run.

**Descrição de uma frase:**

> GraphHelm dynamically compiles, executes, governs, and visualizes task-specific AI agent graphs on infrastructure controlled by the user.

## Arquitetura da marca

- `GraphHelm Core` — Graph Engine, Harness Compiler, Policy Engine e protocolos.
- `GraphHelm Runtime` — daemon executado na VPS do usuário.
- `GraphHelm Studio` — interface local de chat, grafo, execução e documentação.
- `GraphHelm CLI` — interface automatizável para projetos, grafos e runtime.
- `GraphHelm SDK` — SDKs TypeScript e Python.
- `GraphHelm Registry` — catálogo aberto e substituível de extensões, skills, tools e schemas.

## Convenções técnicas propostas

Estas convenções somente devem ser publicadas após a reserva dos respectivos namespaces:

```text
GitHub organization: graphhelm
CLI command: graphhelm
JavaScript scope: @graphhelm/*
Rust crates: graphhelm-*
Python packages: graphhelm-*
Project directory: .graphhelm/
Primary config: graphhelm.yaml
```

Os identificadores wire-format existentes com `p50.dev` permanecem provisoriamente válidos até uma ADR específica aprovar a migração de namespace. A primeira implementação não deve alterar identificadores públicos silenciosamente.

## Checklist antes do anúncio público

1. pesquisa de marca nas jurisdições relevantes;
2. reserva do domínio principal e variantes defensivas;
3. reserva da organização no GitHub;
4. reserva dos namespaces npm, PyPI e crates.io;
5. pesquisa de nomes semelhantes em projetos de agentes, grafos, DevTools e Kubernetes;
6. ADR de migração de `p50.dev` para o namespace definitivo;
7. atualização atômica de schemas, exemplos, documentação e contratos gerados.
