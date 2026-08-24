> **PROVENANCE: this document became the review delivered on PR #188.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# FROZEN — critérios do #188 derivados ANTES de ler o corpo do PR e o diff

**Marcador de congelamento.** Tudo abaixo foi derivado de: (a) o título da issue #186, (b) a fonte em
`origin/main` (`d0b3f04`), lida por mim. **Não li o corpo do PR, não li o diff, não li o corpo da
issue.** O que me foi dado pelo orquestrador foi a PERGUNTA (campo cujo doc proíbe uma leitura e cujos
call sites a produzem) e três coordenadas, sem as conclusões do autor.

## O que eu medi sozinho, na fonte

| facto | onde | consequência |
|---|---|---|
| `at_sequence: Option<u64>`; doc: `None` = "esta superfície não disse onde olhou" | `core/execution/src/attention.rs:56-62` | o campo é sobre a LEITURA, não sobre a história |
| `at_sequence: Some(history.last().map_or(0, ...))` | `apps/cli/src/commands/execution/amend.rs:181` | história vazia -> `Some(0)` |
| `at_sequence: Some(head_sequence)`, `head_sequence = history.last().map_or(0, ...)` | `apps/cli/src/commands/execution/status.rs:43-44` | idem |
| `at_sequence: None` | `apps/cli/src/commands/serve/monitor.rs:312` | terceiro site JÁ escreve None |
| **sequências começam em 1** (`unwrap_or(1)`) | `core/events/src/local.rs:1381` | **zero NÃO é sequência legal** |
| `inputs.at_sequence` -> `Remedy::DeclareNodeBudget.computed_at_sequence: Option<u64>` | `attention.rs:583`, `:173` | unico consumidor de producao |
| doc do consumidor: *"Step 2 must REFUSE a remedy it cannot place in the history, and an absent sequence is exactly that"* | `attention.rs:170-172` | **`None` significa RECUSAR** |
| `computed_at_sequence: u64` (nao-opcional) no evento | `core/protocols/src/event.rs:352` | a wire nao tem a ausencia |
| `head = ...last().map_or(0, ...)`; `if computed_at_sequence != head { refuse }` | `amend.rs:81-85` | **os dois lados derivam 0 da MESMA convencao de vazio** |
| `value["headSequence"] = head_sequence` | `status.rs:47` | **segundo consumidor do mesmo valor achatado, na WIRE** |

## A CONSEQUENCIA QUE EU DERIVO, e e mais forte que a do titulo

O titulo diz que `Some(0)` "le-se como sequencia zero". **Como sequencias comecam em 1, nao existe
sequencia zero** — logo um leitor que conheca a numeracao nao se engana. **O dano real e outro e e
pior:** o doc do consumidor manda **RECUSAR** o remedio quando nao ha sequencia. Com `Some(0)`, o
remedio **e OFERECIDO** — o operador recebe um remedio que carrega uma coordenada impossivel. **Uma
recusa especificada foi apagada por um valor legal.** ([[declared-gap-vs-hidden-gap]])

**E ha uma segunda forma, do meu proprio eixo:** em `amend.rs`, `computed_at_sequence != head` compara
dois numeros que, no caso vazio, **derivam ambos do MESMO `map_or(0, ...)`**. Igualdade cega a defeito
partilhado: os dois lados concordam por origem comum, nao por correcao. **Se esse caminho for
alcancavel, a guarda de obsolescencia passa por coincidencia de dois zeros que significam "nada".**

## OS OITO CRITERIOS — selados antes de ler o PR

**C1 — ALCANCABILIDADE.** O caso vazio e alcancavel em CADA um dos dois sites? Em `amend.rs`,
`projection.execution_id` e checado ANTES e falha com "no execution has started" — suspeito que o
caminho vazio **nao chega** a `:181`. **Se nao chegar, o fix ali e defensivo e NENHUMA fixture o
exercita** — e entao a regra da fabrica manda dizer isso, nao inventar um guard. **Predicao: os dois
sites nao sao simetricos em alcancabilidade, e o PR trata-os como se fossem.**

**C2 — A MOLDURA.** O PR justifica o fix por "le-se como sequencia zero"? Se sim, a justificacao e
mais fraca que o defeito, porque zero nao e legal. **A justificacao correta e a recusa apagada.**

**C3 — ENUMERACAO POR SITE DE GASTO.** `status.rs:47` escreve o MESMO `head_sequence` em
`value["headSequence"]`. **Se o PR troca `at_sequence` para `None` mas deixa `headSequence: 0` na
wire, o achatamento continua no consumidor mais publico dos dois.** Verificar se foi enumerado ou se
so o campo tipado foi tratado. ([[enumerate-by-spend-site]])

**C4 — DESCENDENTES DA RETRATACAO.** O autor corrigiu a propria issue (terceiro site nao se aplica,
porque la `None` ja significa outra coisa e a substituicao FUNDIRIA DUAS AUSENCIAS). **Grep pela
alegacao morta** no corpo do PR, na mensagem de commit, nos comentarios do codigo e no corpo da issue.
**Uma retratacao so acaba quando tudo que ela produziu foi re-derivado**, e o corpo do PR foi escrito
DEPOIS. ([[retraction-descendants]])

**C5 — DEGENERESCENCIA DO CONTROLO POSITIVO.** `a_non_empty_history_reports_the_head_it_actually_read`
tem de FALHAR sob cada uma destas quatro implementacoes degeneradas:
(a) devolver sempre `None`; (b) devolver sempre `Some(0)`; (c) devolver `Some(history.len() as u64)`;
(d) devolver `Some(primeiro.sequence)`.
**Predicao: se a fixture tiver UM evento, (c) e (d) passam trivialmente.** Para o guard medir, a
fixture precisa de **>= 2 eventos e de uma sequencia que NAO seja igual ao comprimento** — senao o
valor esperado e derivavel sem fazer o trabalho. ([[adjacent-fixture-guessable-value]])

**C6 — SUPORTE DA ASSERCAO DE AUSENCIA.** O teste do caso vazio afirma `None` — **ausencia nua**.
Precisa de LANDMARK (algo que tem de estar na mesma leitura) ou de PAR (mesma leitura, vazio ->
nao-vazio). O controlo positivo so serve de PAR se percorrer **o mesmo caminho de leitura**; se for
outro caminho, sao dois testes e nao um par. ([[absence-guards-need-presence]])

**C7 — SITIO QUE ARMA.** O autor decidiu declarar o achatamento na fronteira em vez de o levar para
dentro. **A declaracao esta no sitio que ARMA** — a definicao do campo, o `map_or(0, ...)`, a
construcao do JSON — **ou apenas PERTO dele**? Um comentario no site que dispara le-se como
explicacao de um vermelho sem relacao. E: o tipo continua a permitir `Some(0)`, logo **nada impede um
QUARTO site de o escrever amanha**. ([[arming-site-not-firing-site]])

**C8 — O QUE ISTO NAO APANHA, dito por mim antes de o autor o oferecer.** Predicao do que vai ser
oferecido como cobertura e nao e: um guard por SITE nao e um invariante do CAMPO. A cobertura encolhe
em silencio a cada site novo, e nenhum teste falha quando isso acontece.

## Predicoes seladas (morrem se o diff mostrar o contrario)

1. **O PR nao toca `value["headSequence"]`.** (C3)
2. **A fixture do controlo positivo tem menos de dois eventos, ou sequencia == comprimento.** (C5)
3. **A alegacao retratada sobre o terceiro site sobrevive em pelo menos um artefato** — corpo da
   issue, corpo do PR, ou comentario. (C4)
4. **O caso vazio nao e alcancavel num dos dois sites, e o PR nao o distingue.** (C1)
5. **A justificacao escrita e "le-se como sequencia zero", nao "a recusa foi apagada".** (C2)

**Assinado antes de abrir o PR.** O valor da review sao os DELTAS entre isto e o que o autor escreveu.
