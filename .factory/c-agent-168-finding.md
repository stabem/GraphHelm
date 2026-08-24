> **PROVENANCE: this document closed #168 (premise disproved) and opened #176.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## A premissa não se sustenta na base nomeada — `monitor` **consome** a função compartilhada

**Base: main `d0b3f04`.** Toda citação abaixo é `git show d0b3f04:<path>`, nunca leitura de checkout
compartilhado — a árvore está nomeada porque a resposta depende dela.

Este issue foi aberto como MARKER, medindo que existem "caminhos separados" e declarando
explicitamente que **não foi verificado se hoje concordam**. Essa parte faltava e é a que decide.
Verifiquei. **Não são caminhos separados.**

### O que medi, e como

**Enumeração exaustiva das chamadas a `attention(` em `*.rs` na base** — não amostragem:

| site | papel |
|---|---|
| `core/execution/src/attention.rs:539` | a **definição**, única |
| `apps/cli/src/commands/execution/mod.rs:740` | consumidor CLI/HTTP |
| `apps/cli/src/commands/serve/monitor.rs:302` | consumidor **monitor** |
| 14 ocorrências em `core/execution/tests/**` | testes |

**Exatamente dois chamadores em produção, uma definição.** O `monitor` importa
`graphhelm_execution::{Attention, AttentionInputs, AttentionReason, attention}` (`monitor.rs:11`) e
chama a mesma função que o `render` da API chama. O código já diz isso de si mesmo em
`monitor.rs:65`:

> `attention` owns the rule now; this page consumes it and can no longer disagree.

A triagem também: `untriaged()` (`monitor.rs:66-71`) **filtra** o `Attention` devolvido, procurando
`AttentionReason::UntriagedInterruption`. Não recomputa nada.

### O que FORÇA a concordância — e não são comentários

**A regra é uma definição só, com o sistema de tipos entre ela e os dois chamadores.** Se a regra
mudar, muda num lugar e os dois recebem. Isso é a forma mais forte disponível: não depende de
ninguém lembrar de atualizar o segundo site, porque não existe segundo site.

Os **orçamentos** também: o `monitor` lê
`graphhelm_execution::effective_budgets(projection)` (`monitor.rs:308`) em vez de limiares próprios —
e o comentário ao lado nomeia por quê: *"the two-budgets defect this page already lost its thresholds
over"*.

### Onde eles diferem — e é decisão declarada, não deriva

Uma entrada difere de propósito: o `monitor` passa **`at_sequence: None`** (`monitor.rs:311-312`),
com a razão escrita no site: *"the page renders a snapshot it did not fetch by sequence, so it
reports no vantage point rather than inventing one."*

Isso é diferença no que cada superfície **sabe**, não na regra que cada uma aplica. A página não tem
ponto de observação por sequência, e diz isso em vez de inventar um.

### Proveniência: o caminho paralelo existiu e foi fechado em dois passos

- `9aa4075` (M07, #61) — o monitor passou a consumir `attention()`
- `5ec1614` (M08, #66) — perdeu os limiares privados, ganhou `effective_budgets`

O que este issue descreve é um estado **anterior a M07/M08**. A marca foi útil; a medição que faltava
mostra que a dívida já foi paga.

### O que SOBRA de risco real, e não é o que o issue diz

**A REGRA é compartilhada e não pode derivar. As ENTRADAS são construídas por site e podem.**

`AttentionInputs` é montado inline em cada chamador — o `monitor` monta o seu (`monitor.rs:303-313`);
o `render` da API **recebe** o dele por parâmetro (`execution/mod.rs:731`), então quem monta são os
chamadores dele. Hoje os dois convergem porque ambos passam por `effective_budgets`. **Nada no tipo
obriga isso.** Uma terceira superfície amanhã pode montar `AttentionInputs` com outra fonte de
orçamento e o compilador não reclama — os vereditos divergiriam com a mesma regra.

Esse é o site onde a família #101/#138 poderia reaparecer: **não no cálculo, na alimentação.**

### Veredito

**Não é violação de D-039 nesta base.** Não há segundo caminho: há uma função e dois adaptadores, que
é exatamente a forma que o D-039 pede.

**Sugestão, se alguém quiser fechar a folga que sobra:** o remédio do #170 (cada site DECLARA a
própria política e aponta pra outra) é bom para o `at_sequence`, que é uma divergência deliberada e
merece ser lida como decisão. Para as entradas, o que fecharia de verdade é um construtor
compartilhado de `AttentionInputs` — mas isso só vale a pena quando existir um terceiro consumidor;
com dois, e ambos documentados, é máquina contra um palpite.
