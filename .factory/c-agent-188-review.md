> **PROVENANCE: this document became the review delivered on PR #188.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# REVIEW — PR #188, frozen-then-diff

**Critérios congelados ANTES de ler o PR:** `c-agent-188-frozen.md` (8 critérios, 5 predições
seladas). Base de todas as citações: `origin/main` `d0b3f04` e o commit `1ccba4b`.

## PONTUAÇÃO DAS MINHAS PREDIÇÕES — duas falsificadas A FAVOR do autor

| # | predição selada | resultado |
|---|---|---|
| 1 | o PR não toca `headSequence` | **FALSA** — tocou, e declarou o achatamento na linha que o escreve |
| 2 | a fixture do controlo positivo é adivinhável (<2 eventos, ou seq == comprimento) | **FALSA** — `[event_at(1), event_at(7)]` |
| 3 | a alegação retratada sobre o terceiro site sobrevive nalgum artefato | **VERDADEIRA** — ver S4 |
| 4 | o caso vazio é inalcançável num dos sites e o PR não distingue | **VERDADEIRA, e mais forte do que eu selei** — ver S2 |
| 5 | a justificação é "lê-se como sequência zero" e não "a recusa foi apagada" | **VERDADEIRA** — ver S3 |

**As duas que perdi, perdi-as bem.** O controlo positivo mata as QUATRO implementações degeneradas
que eu selei antes de o ver: `sempre None` (falha em `Some(1)`), `sempre Some(0)`, `Some(len)`
(comprimento 2 contra esperado 7) e `Some(primeiro.sequence)` (1 contra 7). **Não é adivinhável e não
é decorativo.** E o `headSequence` ficou com a fusão DECLARADA no sítio que a constrói, que é a regra
do sítio-que-arma cumprida sem eu a pedir.

---

## S1 — A GUARDA NÃO CONSEGUE FALHAR A SABOTAGEM QUE A ISSUE SELOU (o achado da review)

Os dois testes exercitam **o helper**, diretamente: `at_sequence(&[])` e `at_sequence(&[...])`.

**Reverter QUALQUER um dos dois call sites para o `Some(history.last().map_or(0, …))` inline deixa os
dois testes VERDES.** O helper continua correto; ninguém o chama; nada falha.

E a issue selou exatamente essa sabotagem, com o vermelho esperado escrito:

> | Revert one site to `map_or(0, …)` | **RED at the assert that the empty case yields `None`** |

**Esse vermelho não acontece.** A asserção vive no helper, e reverter um call site não toca no
helper. A mesma issue escreve, uma linha abaixo: *"A red that does not land on the empty-history
assertion is not evidence about this fix"* — aqui não há vermelho nenhum onde quer que se olhe.

**Porque isto é a review e não uma nota de rodapé:** o PR faz desta a sua alegação central — *"The
guard is the point, not the fix"*. A guarda protege **o comportamento do helper**, não **o uso do
helper pelos sites**, e era nos sites que o defeito vivia.

**E o reparo honesto pode ser "declarar", não "construir"** — ver S2: num dos dois sites a fixture é
inconstruível por construção. Opções, por ordem de preferência:

1. um teste ao nível do comando que conduza `status::execute` contra um stream vazio (**depende de
   alcançabilidade — ver S2**);
2. se inconstruível: **comentário no sítio que ARMA de cada call site** ("não voltes a inline isto; a
   guarda está em `mod.rs` e não te vê") — compra COLOCAÇÃO e não enforcement, **e tem de ser dito
   assim**;
3. **declarar a lacuna no corpo do PR** e retirar da issue a linha de sabotagem, que hoje promete um
   vermelho que o código não produz.

**Nenhuma das três é "acrescenta mais um teste ao helper".**

## S2 — Uma "não estabelecida" que ESTÁ estabelecida, e decide o S1 num dos sites

O PR e a issue listam a alcançabilidade do caso vazio como não traçada **nos dois sites**. Num deles
está decidida por cinco linhas da mesma função:

`amend.rs` — `append_atomic(&request)` (~`:169`), e só DEPOIS `read_replay_stream` (~`:176`), cujo
resultado alimenta `at_sequence` (`:181`). **A história ali contém, no mínimo, a emenda que acabou de
ser escrita: nunca é vazia.**

Duas consequências:

- a correção em `amend.rs` é **defensiva por construção** — correta, e sem fixture possível;
- portanto **a opção 1 do S1 só existe para `status.rs`**, e mesmo aí depende de `resolve_stream`
  poder devolver um stream vazio, que continua por traçar (essa metade é mesmo não-estabelecida).

**Não muda o fix. Muda o que o PR pode afirmar** — e evita que alguém tente construir uma fixture que
não existe.

## S3 — O PR SUB-ALEGA: o consumidor está a um grep, e é ele que torna o fix carga-portante

PR e issue dizem *"No consumer of `at_sequence` was traced"*. **Há exatamente um consumidor de
produção**, e o doc dele nomeia a consequência:

- `core/execution/src/attention.rs:583` — `computed_at_sequence: inputs.at_sequence`
- `attention.rs:170-172`, doc do campo recetor: *"Step 2 must REFUSE a remedy it cannot place in the
  history, and an absent sequence is exactly that."*

**Com `Some(0)`, o remédio deixa de ser RECUSADO e passa a ser OFERECIDO** — com uma coordenada que
`amend.rs:85` (`if computed_at_sequence != head`) vai rejeitar a jusante. **O dano não é "um leitor
confunde zero"; é "uma recusa especificada foi apagada por um valor legal, e o operador recebe um
remédio que não pode ser colocado".**

Isto **fortalece** o PR: o fix restaura uma recusa que o design mandava existir. Custo de o
estabelecer: um grep. **Recomendo que entre no corpo, porque é a frase que responde a "quem é que
isto prejudica"** — hoje sem resposta.

## S4 — Descendente de retratação: o CORPO da issue ainda prescreve o fix errado

O corpo do `#186`, secção *"What would fix it"*, ainda diz:

> The same substitution applies to `current_head`.

A correção existe — **num COMENTÁRIO** — e está bem escrita. Mas o PR diz *"That correction is
recorded on the issue"*, e o sítio onde ela está **não é o sítio que as pessoas citam**. Quem abre a
issue, lê o corpo e age, aplica a substituição que funde *"a stream está vazia"* com *"o repositório
está indisponível"* — que é o próprio defeito, mudado de sítio.

**Reparo, barato:** editar o CORPO — riscar a linha e apontar para o comentário. Uma retratação só
termina quando tudo o que ela produziu foi re-derivado, e o corpo é o descendente mais visitado.

## S5 — Menor, prosa: o doc do helper descreve mal o que `status` faz

O doc diz que o `headSequence` da wire *"keeps its own spelling — see `status`"*. **Em `status.rs`
não mantém grafia própria nenhuma:** passou a ser `at_sequence.unwrap_or(0)`. Quem grepar por um
cálculo de head independente não encontra nada, e o doc diz que existe.

**Sugestão:** *"keeps its own SHAPE (zero for empty), re-derived from this helper at the boundary"*.
**Classificado como prosa e não como cobertura, de propósito** — é uma frase imprecisa, não um
buraco, e juntá-la aos outros achados dar-lhe-ia uma severidade que não tem.

---

## O QUE PASSOU, dito porque uma review que só lista defeitos mente sobre o trabalho

- **Controlo positivo não-degenerado**, contra as quatro degenerescências que selei antes de o ver.
- **A asserção de ausência não precisa de landmark nem de par** — e vale dizer PORQUÊ, para não
  parecer omissão: a regra protege contra uma LEITURA mal-apontada, e aqui a entrada é uma fatia
  literal construída no teste. Não há leitura que possa falhar em silêncio.
- **Decisão da wire declarada no sítio que a constrói**, com a razão (clientes já leem 0 como "nada
  ainda") e o custo (mudar a wire precisa de justificação própria).
- **A recusa em levar a substituição ao `current_head`** está certa, e é o oposto do erro comum: o
  autor recusou generalizar um fix correto para onde o `None` já significa outra coisa.
- **O estado de verificação está marcado com honestidade**: *typed and NOT COMPILED*, **um** commit,
  com a verificação nomeada como o passo seguinte e não como o primeiro de uma sequência.

## Verificação que EU fiz, e o que ela NÃO é

Sem toolchain, contra `1ccba4b`: `EventEnvelope`, `NodeState`, `NodeOutcomeRecorded` e os restantes já
estão importados ao nível do módulo (`mod.rs:21-25`); o `use` explícito dentro de `mod tests`
sombreia o glob do `use super::*` sem conflito; `&Vec<EventEnvelope>` coage para `&[EventEnvelope]` no
call site. **Isto NÃO é uma verificação de compilação** — é a ausência de três erros previsíveis, e
nada mais.

## Veredito

**A mudança está correta e é mais estreita do que a issue propunha, o que é mérito.** Nada aqui pede
que o fix mude.

**S1 tem de ser resolvido antes de a alegação central do PR ficar de pé** — e "resolvido" inclui
**declarar a lacuna**, que pode ser a única saída em `amend.rs`. **S4 é barato e deve ser feito no
corpo da issue.** S3 é um ganho que o PR está a deixar na mesa. S5 é uma frase.


---

# DESFECHO — verificado por mim contra `154c5bb`, não aceite por relato

O autor aceitou os cinco e agiu. **Fui verificar em vez de registar o relato**, porque um autor a
descrever a própria correção é a cópia corrompida do que ele fez.

**VERIFICADO E FEITO — melhor do que eu tinha pedido em dois pontos:**

- **S1, saída 3+2 e não "mais um teste":** cada call site leva um comentário **DO NOT INLINE THIS
  BACK**, que diz que a guarda vive nos testes do `mod.rs`, **chama o helper direto e por isso NÃO VÊ
  aquela linha** — e que *"what protects this call site is this comment, and nothing else"*. O doc do
  helper fecha: *"That placement buys attention, not enforcement, and the difference is stated
  because pretending otherwise is how a guard gets trusted for work it does not do."*
- **E acrescentou o que eu não tinha visto:** no site do `amend`, o comentário diz que **o caso vazio
  é inalcançável ALI — e que é exatamente por isso que uma regressão nesse sítio passaria
  despercebida.** Inalcançabilidade como razão para MAIS aviso, não menos. Melhor que o meu S2.
- **S4 feito no corpo da issue, com a razão inline** e não só um ponteiro para o comentário.
- **S5 adotado com a formulação sugerida:** *"keeps its own SHAPE — zero for an empty history —
  re-derived from this helper at the boundary"*.
- **S3 no corpo do commit**, com o enquadramento da recusa apagada.

## S6 — A MESMA CLASSE DO S4, UM NÍVEL ACIMA, DENTRO DA CORREÇÃO DO S4

**O CORPO DO PR não foi atualizado.** Continua a ter a secção **"The guard is the point, not the
fix"** — a alegação que o autor diz ter retirado — e, na tabela, a linha:

> `an_empty_history_reports_no_vantage_point…` | **"Fails the moment anyone restores `map_or(0, …)`"**

**Essa frase é falsa exatamente no caso que o S1 estabeleceu:** restaurado num CALL SITE, não falha.
A retratação existe — **num COMENTÁRIO do PR.**

**É a forma do S4 repetida um nível acima, dentro da própria correção do S4:** a correção no
comentário, a alegação errada no corpo. E aqui pesa mais, porque **o corpo do PR é o que quem faz o
merge lê**, e a linha da tabela é uma promessa de cobertura.

**Reparo: riscar a linha e a secção no corpo, com a razão inline — o mesmo tratamento que a issue já
levou.** Uma retratação termina quando todos os descendentes foram re-derivados, e o corpo do PR é o
descendente de maior tráfego.

## Nota de instrumento, contra mim, apanhada nesta verificação

Usei `git show FETCH_HEAD:<path>` para ler o commit amendado. **`FETCH_HEAD` já não apontava para
`154c5bb`** — um `gh`/`fetch` posterior tinha-o movido — e o git respondeu sobre outra árvore com um
erro que eu quase li como "o ficheiro não existe no commit". **`FETCH_HEAD` não é uma base nomeada:
é uma variável.** Re-derivei por `ls-tree` + `cat-file blob` contra o SHA escrito à mão, que é o
único nome que não se move.
