# FROZEN — critérios do #209, derivados ANTES de ler o corpo do PR e o diff

**Marcador de congelamento.** Derivado de: (a) o corpo da issue `#201` (a especificação), (b) a fonte
em `origin/main` `d0b3f04`, medida por mim, (c) **metadados** do PR (base, head, lista de ficheiros,
contagem) — que não são o corpo nem o diff. **Não li o corpo do PR nem o diff.**
PR `#209`, head `deff9dd`, base `main`, **31 ficheiros, +6453 −286**.

## CONTAMINAÇÃO DECLARADA — três frases de quem me encaminhou, marcadas como pediu

1. *"o autor alega sete guards observados vermelhos na asserção deles próprios, em sete panic sites
   distintos, antes de qualquer verde"*;
2. *"alega uma matriz de mutação com três conjuntos de falha distintos (3, 2, 1)"*;
3. *"o corpo declara o que NÃO é citável (durações mortas por sobreposição de janelas; alegação
   máxima 'esta suíte, neste dir', nunca gate)"*.

**Foi-me dito explicitamente que nenhuma delas é uma conclusão sobre o conteúdo — são as dimensões
onde uma falha seria silenciosa.** Registo-as aqui porque **saber o que o autor ALEGA antes de ler o
diff enviesa-me a procurar confirmação**, e o antídoto é o mesmo que usei no `#195`: **selo abaixo o
que faria cada alegação cair, e comprometo-me a reportar com igual destaque se elas se aguentarem.**

## O QUE EU MEDI SOZINHO, e a primeira medição contradiz a issue

**No `origin/main` (`d0b3f04`) a maquinaria de claim/clearance NÃO EXISTE.**

```
core/events/src/projection.rs               existe, blob aff0392a, 1834 linhas
  grep "pub"                    -> 92      (CONTROLO POSITIVO: o instrumento vê)
  grep "open_claims"            -> 0
  grep "CompletionClaimed"      -> 0
  grep "CompletionCleared"      -> 0
schemas/ : "completion_claimed" / "completion_cleared" -> 0
```

**Confirmado por dois métodos** (`git grep <ref>` e `cat-file blob | grep`), com controlo positivo no
mesmo ficheiro e no mesmo ref.

**A `#201` cita `CompletionClaimed` em `projection.rs:1153` e `CompletionCleared` em `:1195` como
código JÁ IMPLEMENTADO.** Contra `main`, essas coordenadas não existem. **Logo a issue está a citar
outra árvore** — provavelmente o ramo do `#193` ou o `j-161`. **Não é erro fatal e pode ser
deliberado**, mas muda o que o PR tem de carregar: **contra `main`, este PR não acrescenta validação a
uma maquinaria existente — traz a maquinaria toda.**

## O QUE OS METADADOS JÁ DIZEM, e cada um vira critério

- **O PR carrega o seu próprio BLOQUEADOR.** A `#201` diz que está *bloqueada na lane 1 (#159)*
  declarar os dois kinds nos **quatro** sítios do schema. O diff toca
  `schemas/event-envelope.schema.json`, `schemas/catalog.json` **e as duas cópias em
  `schemas/releases/1.0.0/`**. **O bloqueador vem dentro.**
- **`tools/ci-canary/src/nonce.rs` está no diff.** Esse ficheiro **muda a cada corrida por
  construção** — é churn, não conteúdo.
- **Dois manifestos `.factory/gate-runs/*.json`**, um deles (`c449d9b128dc-20260820T144732Z.json`) é o
  mesmo que a `#199` cita como manifesto **de outra lane** (`issue-160`).
- **+6453 −286 em 31 ficheiros** para o que a issue descreve como *"a metade da validação"*.
- **`.factory/bin/j-201-mutate.py` está commitado** — a matriz de mutação é **re-executável por um
  revisor**, o que a torna verificável em vez de relatada.

## A DERIVAÇÃO QUE DECIDE A REVIEW: quais dos sete guards DISCRIMINAM

A propriedade é *"julgado pelo registo à SEQUÊNCIA DELE"*. A implementação errada plausível é
*"julgado pelo registo FINAL"*. Passando os sete da issue por essa mutação:

| guard | sob per-sequência | sob registo FINAL | discrimina? |
|---|---|---|---|
| 1 — identidade nunca registada | recusa | recusa | **não** (a issue diz que é controlo) |
| **2 — registada DEPOIS** | **recusa** | **ACEITA** | **SIM** |
| 3 — fingerprint não bate | recusa | recusa | não |
| **4 — sobrevive à revogação posterior** | **aceita** | **RECUSA** | **SIM, na direção oposta** |
| 5 — revogada antes, não pode limpar | recusa | recusa | não |
| 6 — recusa é dado, log ainda replay | ortogonal | ortogonal | não |
| 7 — interleaved replay idêntico | determinismo | determinismo | não |

**Só DOIS dos sete separam as duas semânticas, e apontam em direções OPOSTAS.** É esse par que fixa a
propriedade; os outros cinco são cobertura de casos, não evidência da tese.

## OS SEIS CRITÉRIOS — selados antes de abrir o PR

**C1 — A MATRIZ TEM DE COBRIR OS SETE.** Três conjuntos de falha distintos não bastam: **a UNIÃO dos
conjuntos tem de conter os sete guards.** Um guard que não aparece em conjunto de falha nenhum **não
foi mostrado a medir nada** por esta matriz — é verde por acompanhar, e a matriz não o distingue de
decoração.

**C2 — A MUTAÇÃO "REGISTO FINAL" TEM DE FALHAR EXATAMENTE `{2, 4}`.** É a mutação que a issue existe
para excluir. Se o conjunto de tamanho 2 for outro par, **ou a minha derivação está errada — e digo-o
— ou a matriz não testa a tese.**

**C3 — O VERMELHO É NO PANIC SITE DELE, E ISSO É VERIFICÁVEL OU NÃO É.** *"Sete vermelhos em sete
panic sites distintos"* só é evidência se **cada um estiver nomeado** — ficheiro e linha, ou a
mensagem da asserção. **Sete nomes ou é uma contagem.** E a `#201` diz que o vermelho de hoje cai em
`append_atomic`, **a montante de toda a asserção** — logo o teste é: os panic sites citados são das
asserções, ou de `append_atomic`?

**C4 — O HARNESS DA MATRIZ TEM DE PROVAR QUE CORREU.** Uma matriz é uma tabela de contagens, e
**contagem não separa "falhou" de "não correu"**. O script está commitado, o que é o passo certo;
falta ver se a saída registada distingue **PASS / FAIL / HARNESS-BROKE**, ou se uma célula vazia se lê
como passagem.

**C5 — O QUE O CORPO DECLARA COMO NÃO-CITÁVEL TEM DE SER CUMPRIDO PELO PRÓPRIO CORPO.** Se declara que
durações não são citáveis e que a alegação máxima é *"esta suíte, neste dir"*, então **nenhum número
de tempo nem nenhuma palavra "gate" pode aparecer como evidência noutro parágrafo.** Declarar um
limite e violá-lo três secções abaixo é pior que não o declarar.

**C6 — ESCOPO: o PR traz o próprio bloqueador e mais.** Schema nos quatro sítios, duas cópias de
release, manifestos de corrida de **outra lane**, e o **nonce do canário**. Cada um destes é
defensável — **mas cada um tem de estar defendido no corpo**, senão um leitor não distingue *decisão*
de *arrasto*. **O nonce é o mais suspeito: muda por construção e não é conteúdo.**

## Predições seladas

1. **A união dos conjuntos de falha da matriz NÃO cobre os sete guards.**
2. **O conjunto de tamanho 2 é `{2, 4}`** (se for, a matriz testa a tese; se não, ver C2).
3. **Os sete panic sites são citados como CONTAGEM, não como sete nomes/linhas distintos.**
4. **O `nonce.rs` está no diff sem justificação no corpo.**
5. **Os dois manifestos de gate-run não são desta lane e o corpo não diz de quem são.**

**Assinado antes de abrir o PR.** O valor da review são os DELTAS — incluindo o delta de eu ter vindo
a saber o que o autor alega e as predições caírem a favor dele.
