# Glossário

**Agent Definition** — configuração persistente e versionada de um agente.

**Agent Experience** — memórias auditáveis e métricas de desempenho ligadas a um agente.

**Agent Runtime** — instância efêmera de uma Agent Definition ou agente sintetizado dentro de um node.

**Artifact** — resultado persistente e content-addressed, como patch, relatório, imagem ou build.

**Autopilot** — modo em que o harness executa e adapta o grafo autonomamente conforme policies.

**Capability** — unidade atômica que descreve algo que o sistema consegue fazer.

**Capability Lease** — autorização temporária, escopada e revogável para usar uma capability.

**Completion Contract** — condições e evidências necessárias para considerar node ou execution concluído.

**Context Capsule** — pacote mínimo, versionado e específico de contexto entregue a um node.

**Context Compiler** — componente que recupera, filtra, comprime e compila Context Capsules.

**Dreams Engine** — manutenção cognitiva assíncrona de documentos, claims, memórias, agentes, skills e índices.

**Evidence** — prova referenciável que sustenta uma claim ou completion requirement.

**Event Store** — registro append-only de eventos e evidências operacionais.

**Execution** — instância de trabalho iniciada por usuário, evento, schedule ou Dreams.

**Ghost Node** — node proposto visualmente, mas ainda não aprovado ou executado.

**Graph Architect** — componente que propõe topologia e composição do grafo.

**Graph Draft** — conjunto transacional de alterações operacionais ainda não aplicado.

**Graph Engineer** — pessoa que cria capabilities, tools, skills, policies, evaluators, adapters e contratos.

**Graph Governor** — componente autorizado a transformar signals em Graph Versions novas.

**Graph Signal** — descoberta estruturada emitida por agent, tool, test, runtime, user ou Dreams.

**Graph Version** — snapshot imutável e executável da topologia e configuração do grafo.

**Harness** — setup compilado para uma tarefa, incluindo grafo, agentes, modelos, contexto, policies, budgets e isolamento.

**Hard Constraint** — regra técnica ou policy explicitamente não dispensável.

**Knowledge Graph** — representação de entidades, claims, relações, temporalidade e provenance do projeto.

**Living Documentation** — documentos humanos versionados e materializados a partir de claims/evidence.

**Manual Graph** — modo em que o usuário monta e altera o workflow; harness atua como linter e assistente.

**Model Route** — conexão utilizável para um modelo ou runtime, com provider, auth, capabilities e capacity state.

**Node** — unidade de execução com objetivo, contrato, permissions e lifecycle.

**Overlay** — alteração temporária aplicada a uma instância de node sem mudar definição persistente.

**Policy Engine** — motor determinístico que aplica regras e invariantes.

**Project Agent Registry** — catálogo de agentes persistentes no escopo do projeto.

**Provenance** — origem e cadeia de derivação de uma informação, artifact ou decisão.

**Runtime** — daemon e serviços executados na VPS do usuário.

**Shadow Workspace** — snapshot isolado usado para testar mudanças do Dreams antes de commit.

**Skill** — orientação operacional versionada para aplicar capabilities.

**Studio** — aplicativo local que funciona como control plane e editor visual.

**Supervised** — modo em que expansões relevantes aguardam confirmação.

**Tool** — mecanismo executável que oferece capabilities.

**Tool Broker** — mediador de chamadas de tools, permissions, secrets e sandboxes.

**Waiver** — registro explícito de obrigação não atendida por decisão do usuário autorizado.
