# Studio — especificação funcional e de interação

## 1. Objetivo

O Studio é o control plane local do Programação 5.0. Ele reúne chat, grafo operacional, agentes em execução, documentação, arquivos, artefatos, eventos, políticas e configurações. A interface deve permitir que um usuário não especialista compreenda o workflow, enquanto oferece profundidade suficiente para um Graph Engineer editar contratos e políticas.

O grafo não é uma animação ilustrativa. Cada nó representa uma unidade real de execução e cada aresta representa dependência, dados, evidência, condição ou controle.

## 2. Estrutura principal

Layout desktop padrão:

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│ Top bar: workspace / projeto / modo / execução / conexão / busca / ações      │
├───────────────┬──────────────────────────────────────┬─────────────┬─────────────┤
│ Navegação     │ Canvas do grafo                      │ Agentes     │ Docs/Files  │
│ Workspace     │                                      │ rodando     │ Artefatos   │
│ Projetos      │                                      │             │ Imagens     │
│ Execuções     │                                      │             │ Claims      │
│               ├──────────────────────────────────────┤             │             │
│               │ Chat e Command Composer              │             │             │
└───────────────┴──────────────────────────────────────┴─────────────┴─────────────┘
```

Todos os painéis são redimensionáveis. O usuário pode recolher navegação, agentes ou docs para ampliar o canvas. O layout é salvo localmente por workspace.


## 2.1 Referências conceituais fornecidas

- [Esboço original do Studio](../assets/original-studio-sketch.png): projetos à esquerda, grafo central, chat inferior, agentes e arquivos/documentos à direita.
- [Referência de diagrama](../assets/graph-diagram-reference.png): nós com identidade e propriedades, arestas nomeadas e relações legíveis.

As referências orientam a organização funcional. O design visual final deve ser refinado, acessível e responsivo, sem copiar limitações do desenho manual.

### 2.2 Larguras recomendadas

- navegação esquerda: 240–320 px;
- painel de agentes: 260–360 px;
- painel docs/files: 320–480 px;
- chat inferior: 120–360 px de altura;
- canvas ocupa todo o restante.

Esses valores são defaults, não restrições fixas.

## 3. Top bar

Componentes:

- seletor de Workspace;
- breadcrumb `Projeto / Subprojeto`;
- seletor de execução ativa;
- indicador do modo: Autopilot, Supervised, Manual Graph;
- indicador de conexão com Runtime;
- status global: running, paused, waiting capacity, blocked, completed;
- custo/capacidade resumida;
- busca global;
- botão `Novo trabalho`;
- botão `Graph Draft` quando existir rascunho;
- menu de exportar, replay e settings.

### 3.1 Comportamento offline

Se o Studio perder conexão:

- mostra banner persistente;
- mantém leitura do último snapshot local;
- bloqueia mutações que exigem confirmação do Runtime;
- permite escrever drafts locais de comando;
- reconecta automaticamente;
- após reconectar, compara versões antes de aplicar qualquer draft.

## 4. Navegação lateral

Seções:

1. **Workspaces**
2. **Projetos**
3. **Subprojetos**
4. **Execuções**
5. **Agentes**
6. **Skills e capabilities**
7. **Documentação**
8. **Dreams**
9. **Modelos**
10. **Policies e segurança**
11. **Eventos e métricas**
12. **Configurações**

Cada projeto mostra badges de execuções ativas, blockers, docs desatualizados, dreams pendentes e conexão.

## 5. Canvas do grafo

### 5.1 Anatomia de um nó

```text
┌─────────────────────────────────────┐
│ ícone  Nome do nó              status│
│ Agente • Modelo • isolation tier    │
├─────────────────────────────────────┤
│ objetivo resumido                   │
│ progresso / etapa / última ação     │
├─────────────────────────────────────┤
│ contexto 12k | 3 tools | 04:21      │
│ evidência 4/6 | retries 0/2         │
└─────────────────────────────────────┘
```

Elementos opcionais:

- badge `GATE`;
- badge `MANUAL OVERRIDE`;
- badge `DREAM GENERATED`;
- badge `PROPOSED` para ghost node;
- indicador de output novo;
- indicador de contexto expandido;
- lock quando nó já está materializado e válido;
- warning de output invalidado.

### 5.2 Estados visuais

- draft: borda tracejada neutra;
- ghost/proposed: 50% de opacidade, tracejado;
- queued: indicador discreto;
- running: pulso ou progresso reduzido, respeitando reduced motion;
- waiting input: ícone humano;
- waiting capacity: ícone de relógio/quota;
- paused: ícone pause;
- blocked: warning com causa;
- succeeded: check;
- failed: erro;
- waived: check com ressalva;
- skipped: transparência;
- invalidated: hatch/risco e badge.

Cor nunca é o único meio de indicar estado.

### 5.3 Arestas

Cada aresta pode exibir:

- label de contrato;
- condição;
- tipo: data, control, evidence, event, failure, compensation;
- estado do payload;
- contagem de artefatos;
- incompatibilidade de schema;
- breakpoint manual.

Ao passar o mouse, mostra origem, destino, condição, payload e última travessia.

### 5.4 Organização

- auto-layout hierárquico, radial, swimlane ou livre;
- grupos colapsáveis por branch, fase, domínio ou subgraph;
- minimap;
- zoom semântico: em zoom baixo, mostrar apenas nomes e status;
- pin de nós importantes;
- filtros por status, agente, modelo, custo, risco, origem e tag;
- comparação lado a lado de Graph Versions.

### 5.5 Interações

- clique seleciona nó;
- duplo clique abre inspector completo;
- arrastar altera somente posição visual;
- arrastar porta cria aresta no Graph Draft;
- Delete cria remoção no Graph Draft;
- Shift+clique seleciona subgrafo;
- botão direito abre ações;
- Space+drag move canvas;
- Ctrl/Cmd+K abre command palette;
- Ctrl/Cmd+Enter envia chat;
- Ctrl/Cmd+Shift+Enter envia como nova execução.

## 6. Chat e Command Composer

### 6.1 Componentes

- campo multiline;
- selector de alvo atual;
- chips de contexto anexado;
- anexos;
- modo `perguntar`, `instruir`, `nova execução` opcional;
- estimativa de efeito operacional;
- botão enviar;
- histórico resumido.

O usuário não é obrigado a selecionar intenção manualmente. O Command Router classifica.

### 6.2 Resultado da classificação

Após mensagem operacional, mostrar uma faixa:

```text
Interpretado como: mutação do grafo atual
Alvo: exec-482
Confiança: alta
Ação proposta: remover Integration Tests e conectar Implementation → Deploy
[Revisar draft] [Corrigir interpretação] [Cancelar]
```

Mensagens consultivas recebem resposta sem mutação.

### 6.3 Menções

Autocomplete para:

- `@projeto`
- `@execução`
- `@graph`
- `@nó`
- `@agente`
- `@documento`
- `@arquivo`
- `@harness`

### 6.4 Perguntas do sistema

Perguntas genuinamente necessárias entram como `human_decision` no grafo e aparecem no chat. O usuário pode responder ali ou no inspector do nó.

## 7. Painel de agentes em execução

Lista compacta por status:

- nome e função;
- nó atual;
- modelo/rota;
- duração;
- capacidade/quota;
- tokens quando disponíveis;
- tool em uso;
- isolamento;
- último evento;
- botão pause/stop/open.

Ações:

- abrir agente;
- pausar após chamada atual;
- parar imediatamente;
- trocar modelo para próxima tentativa;
- visualizar contexto;
- visualizar tools;
- silenciar notificações;
- salvar definição após execução, por ação explícita.

Desativar um agente pausa a branch no checkpoint seguro. Substitutos aparecem como propostas, nunca iniciam automaticamente após intervenção manual.

## 8. Painel Docs, Files e Artifacts

Abas:

1. **Docs** — living documentation, status de atualização, claims relacionadas.
2. **Files** — repositórios, diretórios, arquivos anexados e remotos.
3. **Artifacts** — patches, relatórios, imagens, datasets, builds, exports.
4. **Evidence** — testes, fontes, logs, snapshots, diff.
5. **Claims** — afirmações e relações do Knowledge Graph.
6. **Pictures** — preview visual de imagens e screenshots.

Cada item mostra:

- origem;
- versão;
- quem produziu;
- nós consumidores;
- validade;
- hash;
- classificação de sensibilidade;
- ações: abrir, fixar no contexto, comparar, exportar, marcar obsoleto.

## 9. Node Inspector

O inspector pode abrir como drawer ou tela cheia.

### 9.1 Aba Overview

- nome, tipo, status;
- objetivo;
- justificativa para existir no grafo;
- origem: harness, user, dream, mutation;
- dependências e dependentes;
- progresso;
- blocker atual.

### 9.2 Aba Agent

- definição usada;
- versão;
- prompt/instructions;
- capabilities;
- prohibited actions;
- memory policy;
- histórico do agente no projeto;
- botão `Salvar como novo agente`.

Alterações são overlays exclusivos daquela execução.

### 9.3 Aba Model

- rota atual;
- candidatos e scores;
- disponibilidade;
- quota/custo;
- independência do executor;
- parâmetros suportados;
- ação de trocar manualmente.

### 9.4 Aba Context

- token budget;
- Project Kernel;
- Task Capsule;
- Node Capsule;
- Evidence Bundle;
- Dependency Outputs;
- Agent Experience;
- itens excluídos;
- pedidos de expansão;
- botão adicionar/remover item.

### 9.5 Aba Skills e Tools

- skills carregadas;
- tools e permissions;
- capability leases;
- chamadas realizadas;
- rede e filesystem permitidos;
- botão para editar.

### 9.6 Aba Contracts

- input schema;
- output schema;
- completion contract;
- evidence requirements;
- validação ao vivo;
- payload de exemplo.

### 9.7 Aba Runtime

- isolation tier;
- container/worktree/microVM;
- recursos;
- timeout;
- retries;
- checkpoint;
- logs redigidos;
- cleanup/quarantine.

### 9.8 Aba Events

Timeline filtrada do nó, incluindo chamadas, outputs, sinais, errors, retries, mutations e waivers.

## 10. Graph Draft Review

Ao editar operacionalmente, abrir painel com:

- versão base;
- diff visual;
- lista de nós adicionados/removidos;
- arestas alteradas;
- branches a pausar;
- outputs invalidados;
- gates ignorados;
- obrigações não satisfeitas;
- custo/tempo estimado;
- incompatibilidades técnicas;
- warnings não bloqueantes.

Ações:

- aplicar;
- salvar rascunho;
- descartar;
- pedir ao harness para reparar;
- editar novamente;
- aplicar somente seleção.

Aplicação é atômica. Se o Runtime mudou de versão desde a criação do draft, o Studio exige rebase visual.

## 11. Ghost node review

Ghost node mostra:

- função proposta;
- motivo;
- evidência que disparou;
- modelo sugerido;
- custo/tempo;
- permissões;
- gates atendidos;
- dependências.

Ações:

- aprovar;
- editar e aprovar;
- substituir por agente salvo;
- rejeitar;
- deixar para depois;
- salvar sem executar;
- marcar obrigação como waived.

## 12. Workspace Home

Widgets:

- projetos recentes;
- execuções ativas;
- blockers;
- uso de modelos;
- capacidade de assinaturas;
- dreams recentes;
- documentos desatualizados;
- agents de melhor/pior desempenho;
- riscos de segurança;
- runtime health.

Nenhum widget depende de telemetria externa.

## 13. Onboarding

Passos:

1. escolher idioma e nome local;
2. criar ou importar workspace;
3. conectar VPS via SSH;
4. revisar plano de instalação;
5. instalar Runtime;
6. criar cofre;
7. conectar pelo menos uma rota de modelo;
8. criar/importar projeto;
9. escolher repositório, arquivos ou fontes;
10. executar diagnóstico;
11. abrir primeiro prompt.

Cada etapa pode ser retomada. O usuário pode usar modelo local sem conta externa.

## 14. Model Connections

Cards por rota:

- provider;
- transport;
- auth type;
- perfil conectado;
- status;
- capabilities;
- quota observada;
- últimos throttles;
- privacy note;
- testar, reconectar, remover.

Conexões de assinatura usam login oficial. BYOK mostra escopo e custo configurado. Secrets nunca são exibidos após armazenamento.

## 15. Agent Registry

Lista com:

- nome;
- propósito;
- versão ativa;
- status;
- executions;
- success rate;
- false-positive rate;
- custo/duração;
- última validação;
- memories ativas;
- tags e scope.

Detalhe:

- definição;
- versões;
- performance por cenário;
- experiências;
- relações com skills;
- graphs em que apareceu;
- merge/derive/archive;
- botão testar em sandbox.

## 16. Skills e Capabilities

Browser com filtros por tipo, permission, runtime, publisher, trust level e compatibility. A instalação mostra manifest e permissões.

Views:

- installed;
- project-local;
- workspace-shared;
- community registry;
- quarantined;
- updates.

## 17. Docs e Knowledge

### 17.1 Documentation Browser

- árvore de docs;
- status: current, stale, conflicted, generated, manually edited;
- preview Markdown/diagram;
- claims e evidence sidecar;
- diff entre versões;
- freshness score;
- pin como canônico.

### 17.2 Knowledge Graph Explorer

Visualização por entidades e relações, com filtros por confidence, status, temporalidade, project scope e provenance.

O grafo de conhecimento é separado do grafo de execução, embora possam se referenciar.

## 18. Dreams Center

Mostra:

- próximo trigger;
- budget;
- idle policy;
- ciclos recentes;
- mudanças propostas/aplicadas;
- shadow tests;
- critic result;
- rollback;
- tarefas `dream_generated`;
- economias estimadas de contexto.

O usuário pode iniciar um dream manualmente, pausar o scheduler ou limitar categorias.

## 19. Policies e Segurança

Seções:

- global policies;
- project policies;
- hard constraints;
- dispensable gates;
- secrets;
- isolation defaults;
- network allowlists;
- production targets;
- waivers;
- plugin permissions;
- audit retention.

A interface diferencia:

- impossibilidade técnica;
- policy rígida definida pelo owner;
- recomendação do sistema;
- gate dispensado.

## 20. Events, Metrics e Replay

### 20.1 Timeline

Filtros por execução, graph version, node, agent, model, tool, severity, actor e event type.

### 20.2 Replay

- play/pause;
- velocidade;
- scrubber;
- graph version switch;
- abertura do payload em cada evento;
- comparação de estado antes/depois;
- esconder conteúdo sensível.

### 20.3 Métricas

- duração;
- tokens/custo;
- quota;
- context saved;
- retries;
- gates;
- errors;
- mutation count;
- agent performance;
- evidence coverage.

## 21. Fluxos críticos

### 21.1 Prompt novo

1. Usuário envia.
2. Studio mostra classificação em progresso.
3. Graph draft inicial aparece.
4. Em Autopilot, lint aprovado publica e executa.
5. Em Supervised/Manual, aguarda confirmação conforme política.
6. Agentes aparecem no painel.
7. Outputs surgem em docs/artifacts.

### 21.2 Pular testes e ir para deploy

1. Usuário arrasta aresta `Implementation → Deploy` ou escreve comando.
2. Draft mostra testes/review removidos.
3. Studio lista riscos e obrigações.
4. Usuário aplica.
5. Runtime cria waiver e Graph Version nova.
6. Branch segue sem recolocar nós.

### 21.3 Limite de assinatura

1. Nó recebe resposta de quota.
2. Checkpoint.
3. Status `waiting capacity`.
4. Studio mostra rota e opções.
5. Usuário espera ou escolhe outra rota.
6. Retomada preserva outputs.

### 21.4 Desativar agente

1. Usuário clica stop/disable.
2. Nó para no checkpoint escolhido.
3. Harness calcula cobertura perdida.
4. Alternativas aparecem como ghost nodes.
5. Nada inicia sem confirmação.

## 22. Acessibilidade

- todos os nós acessíveis por lista alternativa;
- navegação por teclado entre nós e arestas;
- labels textuais de status;
- modo alto contraste;
- reduced motion;
- descrição linear exportável do grafo;
- atalhos configuráveis;
- foco preservado após updates em tempo real.

## 23. Critérios de aceite do Studio

- usuário consegue operar sem abrir terminal após bootstrap;
- qualquer ação operacional deixa trilha auditável;
- graph draft nunca aplica sem confirmação quando iniciado por ação manual;
- ghost node não consome recursos antes da aprovação;
- closing/reopening Studio preserva layout e reconecta execução;
- cada nó permite abrir contexto, agente, modelo, tools, contracts e events;
- usuário consegue chegar de implementação a deploy por override explícito;
- canvas e lista alternativa representam o mesmo estado;
- nenhuma credencial aparece em UI, log ou export.
