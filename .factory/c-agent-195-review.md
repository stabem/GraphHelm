> **PROVENANCE: this document became the review delivered on PR #195.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# REVIEW — PR #195, frozen-then-diff

**Critérios congelados ANTES de abrir o PR:** `c-agent-195-frozen.md` (6 critérios, 5 predições
seladas), derivados da issue `#189` e da fonte em `origin/main` `d0b3f04`. PR em `c907b36`.

## AS CINCO PREDIÇÕES SELADAS FORAM TODAS FALSIFICADAS, A FAVOR DO AUTOR

| # | selada | resultado |
|---|---|---|
| 1 | o lado do CÓDIGO do guard é enumerado à mão | **FALSA** — é fatiado da fonte por `include_str!` e os literais são extraídos do corpo de `verdict_tag` |
| 2 | o lado da DOC é repetido em Rust, não lido do ficheiro | **FALSA** — parseia a tabela do `QUICKSTART.md` e o texto do `README.md` |
| 3 | o corpo credita ao guard a propriedade "uma quinta variante quebra a build" | **FALSA** — o corpo não faz essa alegação |
| 4 | só o comentário de `verdict_tag` foi corrigido; o `"Three answers"` do `monitor.rs` sobrevive | **FALSA** — corrigido, **e a cegueira do guard declarada no próprio sítio** |
| 5 | o snippet de `jq` do QUICKSTART não é tocado | **FALSA** — ganhou um parágrafo a exigir ramo por omissão |

**Cinco em cinco. Registo isto primeiro porque é o resultado da review**, e porque um revisor que só
publica os acertos não diz nada sobre a taxa dele.

## O CRITÉRIO DE ABSOLVIÇÃO DISPAROU, E EU TINHA-ME COMPROMETIDO A REPORTÁ-LO COM O MESMO DESTAQUE

O `C6` do congelado dizia: *a amarração lista→código está CERTA se o lado do código for derivado por
exaustividade **e** o lado da doc for lido do ficheiro.* **É exatamente o que está construído**, e
por um caminho melhor do que o que eu tinha em mente:

- **`emitted_tags()`** fatia o corpo de `fn verdict_tag` da própria fonte e recolhe os literais.
  **Não há lista em Rust nenhuma para envelhecer.** E o doc explica porque fatia da FUNÇÃO e não do
  comentário: *"letting prose feed this scan would make the test agree with a comment instead of with
  the code"* — **a única armadilha óbvia, evitada de propósito.**
- **`documented_tags()`** parseia as linhas da tabela **dentro da secção** do QUICKSTART, e a razão do
  escopo é medida: um scan global apanharia a linha `jq` da tabela de pré-requisitos e **falharia
  pelo motivo errado** — *"a guard that trips on an unrelated table teaches people to loosen it"*,
  que é a regra do detetor que se auto-desliga, aplicada antes de alguém a pedir.
- **DOIS controlos positivos**, um por lado, com a razão certa: sem eles um scan que deixasse de
  encontrar seria o **conjunto vazio**, e *"an empty set is a subset of everything"* — a igualdade
  passaria a medir nada.

**E eu vim contaminado a este PR** — quem mo encaminhou disse-me que era *"um guard que amarra uma
LISTA a CÓDIGO"*, e eu vinha de corrigir esse defeito num comentário meu uma hora antes. **Marquei a
contaminação no congelado e selei o critério que me absolveria de a encontrar. Foi esse que
disparou.**

## Verificação independente do ESCOPO, que era o que podia invalidar tudo

O guard protege `verdict_tag`. **Se houvesse outro produtor do campo, o guard estava a garantir uma
porta de duas.** Medido por mim no head (`c907b36`):

```
"attention": verdict_tag(&answer.verdict)     -> mod.rs:756, ÚNICA emissão na wire
"needs_you" / "can_sleep" / "calmed_by_amendment" -> só dentro de verdict_tag
```

**Produtor único, guard no sítio certo.**

---

## D1 — o guard fecha o CONTRATO de uma função e não fecha a PORTA (não bloqueante)

Nada no guard afirma que `verdict_tag` **continua** a ser o único produtor. Um segundo caminho de
emissão — outra função, outro campo — nasce **fora do alcance do teste**, e o teste continua verde
porque a função que ele lê continua correta.

**É a mesma forma que apanhei no `#202`:** o guard protegia o comportamento do helper, não o uso
dele. Aqui protege o contrato de `verdict_tag`, não a exclusividade dele.

**E há um fecho barato e DERIVADO, sem lista à mão:** o teste já tem `CLI_SOURCE` carregado —
afirmar que **nenhum literal de tag ocorre fora do corpo fatiado**. Um segundo produtor passa a
falhar no sítio onde nasce. **Custo: uma asserção; e continua a crescer sozinho.**

## D2 — o `README` é verificado numa direção só (menor, e é uma frase)

`the_readme_names_every_tag_the_cli_can_emit` afirma **emitidos ⊆ README**. Não afirma o inverso,
logo **um README que liste um valor que ninguém emite fica verde** — e é precisamente metade do
defeito que o `#189` reporta, do outro lado (*"a documented value nobody can receive sends the reader
looking for a state that does not occur"*, palavras do próprio teste, aplicadas só ao QUICKSTART).

**A escolha é defensável e o autor até explica porque não parseia o README** (é prosa, e inventar
gramática faria o guard falhar em reescritas). **O que falta é dizer que a direção inversa fica por
cobrir** — senão o nome do teste sugere simetria que ele não tem.

## Nota de estado, não achado: é o SEGUNDO commit typed-not-compiled em aberto

O corpo di-lo sozinho — *"This is the second typed-not-compiled commit I have open"* — e isso é o
marcador a sobreviver, que é o comportamento certo. **Vale só apontar a versão afiada do reparo:
um lote de verificação adiada não são N adiamentos independentes, é UM com uma cauda que cresce**,
logo o marcador ganha em dizer **de quantos** faz parte. **Dois já é uma série.**

## Verificação que EU fiz, e o que ela NÃO é

Leitura do diff e dos dois corpos contra `c907b36` e `d0b3f04`, mais a enumeração dos produtores por
`git grep` com o ref no comando. **Não executei nada** — nem os testes nem o gate. **O guard nunca
correu**, o que o próprio corpo declara, e **um guard que nunca executou é indistinguível de um que
nunca falha**.

## Veredito

**Nada aqui pede que a abordagem mude, e a construção é melhor do que a issue exigia.** O `#189`
pedia *"um teste que assere que o conjunto é igual ao documentado"*; **o que aterrou lê os dois lados
da fonte e traz um controlo positivo por lado.**

**D1 é a única que eu gostaria de ver antes do merge**, e é uma asserção. **D2 é uma frase no doc do
teste.** Nenhuma bloqueia.
