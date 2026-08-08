# Requisitos de produto

## 1. Escopo normativo

Este documento transforma a visão do MASTER PRD em requisitos verificáveis. Os identificadores devem ser preservados em issues, testes de aceite e RFCs.

## 2. Requisitos funcionais

### FR-001 — Instalação self-hosted

O usuário deve conseguir instalar o Runtime em uma VPS Linux existente por SSH e Docker, sem criar conta em serviço central.

**Aceite:** o bootstrap valida arquitetura, disco, memória, Docker/Podman, portas, Git e persistência; mostra plano antes de alterar a VPS; permite atualizar e desinstalar.

### FR-002 — Studio local

O Studio deve operar como control plane local, reconectando-se ao Runtime sem interromper execuções.

### FR-003 — Hierarquia de contexto

Deve existir Workspace, Projeto e Subprojeto, com herança seletiva para políticas, agentes, skills, documentos, fontes e credenciais.

### FR-004 — Intake universal

O usuário deve poder enviar linguagem natural, arquivos, imagens, links permitidos, seleção de nós e referências a documentos.

### FR-005 — Command Router

Cada mensagem deve produzir `intent`, `target`, `confidence`, `operational_effect` e `interpretation`. Mutações do grafo criam draft, não alteração silenciosa.

### FR-006 — Task Profile

O sistema deve classificar, no mínimo:

- domínio provável e multidomínio;
- complexidade;
- profundidade;
- superfície afetada;
- risco de regressão;
- risco de segurança;
- reversibilidade;
- incerteza;
- necessidade de pesquisa;
- sensibilidade de dados;
- custo e duração estimados;
- necessidade de decisão humana;
- isolation tier mínimo.

### FR-007 — Catálogo de capacidades

Capabilities devem ser pesquisáveis por objetivo, input/output, permissões, custo, histórico, runtime, isolamento e compatibilidade.

### FR-008 — Harness por tarefa

Cada execução deve possuir um Harness Manifest imutável por versão. Não deve existir dependência obrigatória de packs fixos por domínio.

### FR-009 — Graph DSL tipada

Nós e arestas devem declarar contratos. O linter deve bloquear incompatibilidades técnicas antes da execução.

### FR-010 — Agentes sintetizados

O sistema deve criar agentes específicos quando nenhum agente salvo atender. Toda definição precisa de objetivo, capabilities, permissions, schemas, evidence requirements e completion criteria.

### FR-011 — Project Agent Registry

Agentes devem poder ser salvos, versionados, avaliados, derivados, suspensos, arquivados e reutilizados no escopo do projeto.

### FR-012 — Alterações temporárias de nó

Editar uma instância de agente no grafo não altera a definição persistente. Salvar/promover exige comando explícito.

### FR-013 — Universal Model Gateway

Deve suportar agregadores, BYOK, APIs diretas, runtimes nativos oficiais e modelos locais por adapters substituíveis.

### FR-014 — Pausa por limite de assinatura

Quando uma rota de assinatura atingir limite ou perder sessão, os nós dependentes entram em `waiting_for_model_capacity`; não deve existir fallback pago automático.

### FR-015 — Context Capsules

Cada nó recebe uma cápsula versionada, com itens incluídos, excluídos, provenance, token budget e política de expansão.

### FR-016 — Solicitação de expansão

O agente deve poder pedir mais contexto declarando motivo, informação faltante e impacto esperado. O Context Compiler aceita, reduz ou rejeita.

### FR-017 — Event Store

Prompts, decisões, eventos, diffs, testes, modelos, custos, artefatos, waivers e mutações devem ser registrados de forma append-only.

### FR-018 — Knowledge Graph

Claims devem possuir status, confiança, validade temporal, provenance e relações `supports`, `contradicts`, `supersedes`, `derived_from` e `applies_to`.

### FR-019 — Living Documentation

Documentos legíveis devem ser versionados e vinculados a claims/evidências. Atualizações precisam produzir diff e justificativa.

### FR-020 — Graph Engine

O motor deve executar DAGs e grafos governados com forks, joins, condições, retries, timeouts, compensações, checkpoints e human decisions.

### FR-021 — Graph Governor

Agentes não podem alterar a topologia diretamente. Devem emitir `graph_signal`. O Governor publica nova versão após policy, dependency, cost e lint checks.

### FR-022 — Ghost nodes

Quando a política de interação exigir aprovação, uma expansão aparece como proposta visual sem runtime, contexto ou consumo de modelo.

### FR-023 — Modos de operação

Deve haver Autopilot, Supervised e Manual Graph, alternáveis durante a execução.

### FR-024 — Controle soberano

O proprietário pode pausar, cancelar, remover nó, trocar modelo, editar aresta, pular gate e ir direto para deploy quando tecnicamente executável.

### FR-025 — Waiver

Pular obrigação cria waiver com ator, escopo, riscos, versão do grafo e duração. O sistema não deve recolocar automaticamente o gate ignorado.

### FR-026 — Substituição confirmada

Após o usuário desativar um agente, qualquer substituto proposto permanece parado até confirmação explícita.

### FR-027 — Graph Draft transacional

Alterações operacionais devem ser agrupadas, analisadas e aplicadas atomicamente. Somente branches afetadas são pausadas/invalidadas.

### FR-028 — Editor integral de nó

O usuário pode editar objetivo, instruções, agente, modelo, skills, tools, contexto, schemas, completion criteria, isolamento, recursos, retries, gates, memory policy e edge conditions.

### FR-029 — Isolamento adaptativo

Cada nó recebe tier, filesystem, network, secret scope e resource limits. O runtime pode elevar o tier conforme sinais.

### FR-030 — Capability leases

Acesso a tool, rede, filesystem, segredo ou produção deve ter lease escopada, revogável e registrada.

### FR-031 — Tool Broker

Agentes devem acessar o ambiente por tools mediadas e tipadas. Código não confiável não recebe credenciais de modelos.

### FR-032 — Gates de qualidade

O sistema deve suportar testes determinísticos, avaliação por modelo, review independente, segurança, performance, consistência, documentação, fonte e critérios customizados.

### FR-033 — Evidência de conclusão

Cada nó e execução deve declarar o que prova conclusão. Saída sem evidência pode ser marcada como parcial, não como plenamente validada.

### FR-034 — Controle de viés

O harness deve poder impor reviewer independente, blind review, provider diversity, prompt diversity e disagreement resolution.

### FR-035 — Dreams Engine

Deve operar em Shadow Workspace, validar mudanças, receber crítica independente e realizar commit atômico ou descarte.

### FR-036 — Dreams sem alteração direta de código

Achados que exigem código devem virar tarefas normais com origem `dream_generated`.

### FR-037 — Documentação e agente em paralelo

O grafo pode atualizar claims, índices e documentação enquanto outra branch executa, desde que dependências e snapshots evitem ler estado inconsistente.

### FR-038 — Painel de agentes

O Studio deve mostrar status, função, modelo, nó, tokens/capacidade, duração, ferramentas, contexto e último evento de cada agente.

### FR-039 — Painel de docs e arquivos

Documentos, arquivos, imagens, artefatos, diffs, testes e fontes devem ser vinculados ao projeto e aos nós que os produziram ou consumiram.

### FR-040 — Auditoria e replay

O usuário deve poder reproduzir a timeline, comparar Graph Versions, abrir inputs/outputs e exportar Execution Manifest sem secrets.

### FR-041 — APIs públicas

Tudo que o Studio faz deve estar disponível por API e, quando aplicável, CLI e SDK.

### FR-042 — Extensões

Plugins devem declarar type, capabilities, contracts, permissions, isolation minimum, platforms e version compatibility.

### FR-043 — Observabilidade local

Tokens, quotas, custos, latência, failures, graph mutations, context usage e quality scores devem ficar disponíveis localmente.

### FR-044 — Colaboração futura

Mesmo em single-user, toda ação deve possuir `actor`. O modelo de autorização deve aceitar user, service account e agent identity.

### FR-045 — Exportação

Projeto, agentes, skills, grafos, policies e execution manifests devem poder ser exportados. Secrets nunca entram por padrão.

## 3. Requisitos não funcionais

### NFR-001 — Privacidade

Nenhum dado é enviado ao mantenedor por padrão. Telemetria externa é opt-in, documentada e desligável.

### NFR-002 — Segurança de secrets

Nenhum secret em logs, artifacts, context capsules, prompts exportados ou crash dumps. O scanner de segredo deve rodar em saídas persistentes.

### NFR-003 — Resiliência

O Runtime sobrevive ao fechamento do Studio. Reboot da VPS recupera execuções de checkpoints duráveis.

### NFR-004 — Idempotência

Comandos de mutação, eventos e retries devem possuir IDs idempotentes.

### NFR-005 — Compatibilidade

Schemas e APIs seguem SemVer. Breaking changes exigem migração e changelog.

### NFR-006 — Desempenho do Studio

Canvas deve manter interação fluida com 1.000 nós e virtualizar detalhes. Atualização de estado deve ser incremental.

### NFR-007 — Eficiência de contexto

Context Compiler deve medir precisão, recall, redundância e tokens evitados por cápsula.

### NFR-008 — Expansão limitada

Toda execução possui limites de nós, profundidade, mutations, retries, custo e wall-clock; loops sem progresso são detectados.

### NFR-009 — Auditabilidade

Toda decisão de modelo relevante deve registrar candidatos, score, restrições e rota escolhida, respeitando confidencialidade do provedor.

### NFR-010 — Acessibilidade

Navegação por teclado, foco visível, contraste, labels, text alternatives e modo de movimento reduzido.

### NFR-011 — Portabilidade

Runtime de referência suporta Linux x86_64 e arm64. Studio suporta Windows, macOS e Linux.

### NFR-012 — Substituibilidade

Stores, model adapters, sandbox adapters, evaluators e retrievers devem possuir interfaces públicas.

## 4. Regras de negócio

- Um gate pode ser `required`, `recommended` ou `optional`.
- `required` pode ser dispensável por owner, salvo impossibilidade técnica ou policy rígida configurada pelo próprio owner.
- Waiver não converte evidência inexistente em evidência satisfeita.
- Um nó `succeeded` pode ser invalidado por mutação posterior se seu input semântico mudou.
- Uma memória expirada não entra automaticamente em Context Capsule.
- Um agente suspenso não é selecionado pelo matcher, mas continua reproduzível por versão.
- Graph Governor preserva outputs concluídos apenas quando dependency hash permanece válido.
- Model route de assinatura não pode gastar API BYOK sem escolha manual.
- Dreams nunca apaga Event Store nem amplia suas próprias permissões.
- Um plugin não recebe network ou secrets sem manifestação explícita e policy compatível.
