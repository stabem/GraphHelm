# Registro de decisões aprovadas

Este documento consolida as decisões de produto já tomadas. Elas são normativas para a versão 0.1 da especificação.

| ID | Tema | Decisão |
|---|---|---|
| D-001 | Topologia | Studio local como control plane; runtime e dados na VPS do usuário. |
| D-002 | Conexão da VPS | VPS existente conectada via SSH; instalação e atualização por Docker. |
| D-003 | Autonomia | Autonomia total governada pelo harness; sem aprovação obrigatória entre fases. |
| D-004 | Abrangência | Sistema operacional generalista de agentes, não apenas ferramenta de programação. |
| D-005 | Harness | Harness sintetizado por tarefa a partir de capacidades atômicas; nenhum pack fixo por domínio. |
| D-006 | Regras obrigatórias | Policy Engine determinístico impõe invariantes e gates conforme sinais de risco. |
| D-007 | Agentes | Agentes efêmeros são sintetizados com contratos e capacidades registradas. |
| D-008 | Persistência de agentes | Agentes úteis ficam salvos no Project Agent Registry e podem ser reutilizados. |
| D-009 | Memória de agente | Memória limitada, auditável, com evidência, confiança, validade e expiração. |
| D-010 | Verdade do projeto | Três camadas: Evidence/Event Store, Project Knowledge Graph e Living Documentation. |
| D-011 | Dreams Engine | Autonomia total governada sobre conhecimento, documentos, agentes, skills e índices. |
| D-012 | Dreams e código | Dreams não altera código diretamente; cria tarefa normal fundamentada para o Graph Engine. |
| D-013 | Isolamento | Isolamento adaptativo Tier 0–3, elevado conforme riscos descobertos. |
| D-014 | Modelos | Universal Model Gateway com BYOK, APIs diretas, agregadores, runtimes nativos e modelos locais. |
| D-015 | Assinaturas | Conexão oficial com conta ChatGPT/Codex e Claude/Claude Code quando suportada pelo provedor. |
| D-016 | Fallback de assinatura | Ao atingir limite de assinatura, pausar; não gastar BYOK/OpenRouter automaticamente. |
| D-017 | Contexto | Context Compiler em camadas com cápsulas específicas por nó e expansão justificada. |
| D-018 | Grafo em execução | Grafo adaptativo, versionado e alterado somente pelo Graph Governor. |
| D-019 | Soberania | Usuário pode pausar, remover gates, pular fases e conectar diretamente ao deploy. |
| D-020 | Substituição de agente | Agente substituto nunca inicia automaticamente após intervenção manual; exige confirmação. |
| D-021 | Ghost nodes | Expansões propostas aparecem como nós transparentes sem consumir tokens antes da aprovação. |
| D-022 | Modos | Autopilot, Supervised e Manual Graph, alternáveis durante a execução. |
| D-023 | Editor de nó | Editor integral: objetivo, prompt, modelo, contexto, skills, ferramentas, contratos, gates e retries. |
| D-024 | Edição de agente salvo | Alterações em nó valem só para a execução atual; promoção exige ação explícita. |
| D-025 | Edição em execução | Alterações visuais imediatas; alterações operacionais entram em Graph Draft transacional. |
| D-026 | Chat | Command Router contextual; mutações operacionais viram drafts para confirmação. |
| D-027 | Hierarquia | Workspace → Projeto → Subprojeto, com herança seletiva de contexto e políticas. |
| D-028 | Colaboração | Single-user first, mas identidade, autorização e auditoria prontas para equipes. |
| D-029 | Open source | Framework, Runtime e Studio integralmente open source; nenhuma função essencial fechada. |
| D-030 | Licenciamento | Dual license: AGPLv3 comunitária e licença comercial alternativa. |
| D-031 | Contribuições | CLA não exclusivo, com ICLA e CCLA, permitindo relicenciamento comercial. |
| D-032 | Primeira fatia | Fatia vertical developer-first completa, sem limitar a arquitetura generalista. |
| D-033 | Estado atual | Produzir documentação integral antes de qualquer implementação. |
| D-034 | Nome | GraphHelm é o nome de produto selecionado para desenvolvimento, sujeito a clearance jurídico e reserva de namespaces antes do lançamento público. |

## Consequências obrigatórias

- Nenhum componente pode presumir um workflow estático chamado “software pack”, “marketing pack” ou equivalente.
- Um agente nunca recebe, por padrão, todo o histórico do projeto.
- Um executor não pode aprovar sozinho seu próprio resultado quando houver obrigação de revisão independente.
- O usuário proprietário pode substituir ou ignorar gates, mas o sistema registra riscos, waivers e resultados posteriores.
- Credenciais de modelos não podem ficar acessíveis no mesmo sandbox que executa código não confiável.
- O desktop oficial não pode usar endpoints privados indisponíveis a clientes externos.
- Toda ação do Studio deve ser possível via API/CLI pública do Runtime.
- O Event Store não pode ser reescrito pelo Dreams Engine.
