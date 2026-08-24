> **PROVENANCE: this document became the review delivered on PR #195.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# FROZEN — critérios do #195, derivados ANTES de ler o corpo do PR e o diff

**Marcador de congelamento.** Derivado de: (a) o corpo da issue `#189` (a especificação), (b) a
fonte em `origin/main` `d0b3f04`, lida por mim, (c) a lista de ficheiros do PR. **Não li o corpo do
PR nem o diff.** PR em `c907b36`.

## CONTAMINAÇÃO DECLARADA, e é por isso que está no topo

Quem me encaminhou disse: *"aquilo é um guard que amarra uma LISTA a CÓDIGO — a mesma forma do que
acabaste de corrigir em ti, do outro lado"*, e pediu que eu marcasse a frase como contaminação dele.
**Marcada.**

**O risco concreto: eu venho de corrigir exatamente esse defeito num comentário meu há uma hora, logo
sou o pior leitor possível para o não encontrar aqui.** Contra isso, selo abaixo **um critério de
ABSOLVIÇÃO** — o que teria de ser verdade para a amarração estar CERTA — e comprometo-me a
reportá-lo com o mesmo destaque se for ele que se verificar.

## O que eu medi sozinho na fonte (`origin/main` `d0b3f04`)

| facto | onde |
|---|---|
| `Verdict` **não** é `#[non_exhaustive]` | `core/execution/src/attention.rs:268` |
| `verdict_tag` tem **quatro braços e NENHUM wildcard** | `apps/cli/src/commands/execution/mod.rs:628-635` |
| o doc de `verdict_tag` lista **três** | uma linha acima |
| `attention_line` **também** casa `Verdict`, e o comentário dele diz **"Three answers"** enquanto trata quatro | `apps/cli/src/commands/serve/monitor.rs:79-84` |

## A DERIVAÇÃO CENTRAL, e ela decide a review

**Uma quinta variante JÁ quebra a build hoje, sem teste nenhum** — porque `verdict_tag` casa
exaustivamente e sem wildcard, e o enum não é `non_exhaustive`. **O compilador já é o guard do lado
do CÓDIGO.**

**Logo o único trabalho NÃO-REDUNDANTE do guard novo é o lado da DOCUMENTAÇÃO** — que é prosa e que
nenhum compilador lê.

**E daí sai o teste que a review tem de fazer:** o guard **lê a documentação**, ou compara duas
listas que ele próprio escreve? **Se as duas metades estão à mão no ficheiro de teste, o guard afirma
que uma constante é igual a outra constante** — e uma quinta variante deixa-o **verde**, porque ele
só conhece as quatro que lhe deram.

## OS SEIS CRITÉRIOS — selados antes de abrir o PR

**C1 — DE ONDE VEM O LADO DO CÓDIGO?** Enumerado à mão no teste, ou derivado por construção (um
`match` exaustivo sobre um valor de cada variante, que **falha a compilar** quando uma aparece)?
**Só a segunda forma cresce sozinha.**

**C2 — DE ONDE VEM O LADO DA DOCUMENTAÇÃO?** O teste **lê `QUICKSTART.md`/`README.md`** e extrai os
valores, ou repete-os em Rust? **Se repete, a doc pode voltar a divergir sem nada ficar vermelho** —
que é a totalidade do defeito que o `#189` reporta.

**C3 — A ALEGAÇÃO "qualquer quinta variante quebra a build" É VERDADEIRA MAS PODE SER CREDITADA AO
MECANISMO ERRADO.** Ela já era verdadeira antes deste PR. **Se o corpo a apresentar como resultado do
guard novo, o crédito está mal posto** — e o leitor conclui que tem cobertura vinda de um sítio que
não a fornece. (É `guards-green-on-redundant-work` visto do lado da prosa.)

**C4 — ENUMERAÇÃO DOS SÍTIOS: a issue nomeia só o doc de `verdict_tag`.** Eu encontrei um **segundo**
comentário com o mesmo undercount, em `monitor.rs:80` (*"Three answers"* sobre quatro braços). **O PR
toca esse ficheiro.** Corrigiu os dois, ou só o que lhe foi apontado? **Predição: só o apontado.**

**C5 — E O QUE MAIS TEM DE MUDAR NA PROSA:** `QUICKSTART.md` diz **"exactly three"** e traz um
snippet de `jq` que ramifica nesses três. **Acrescentar a quarta linha à tabela não basta se o texto
do snippet continuar a sugerir três ramos** — o consumidor scriptado é o dano concreto que a issue
descreve.

**C6 — CRITÉRIO DE ABSOLVIÇÃO, escrito para não me deixar encontrar o que vim procurar.** A
amarração lista→código está **CERTA** se: o lado do código for derivado por exaustividade (C1) **e**
o lado da doc for lido do ficheiro (C2); **ou** se o teste for explicitamente um guard de PROSA que
diz que só cobre a prosa, com o compilador nomeado como o guard do código. **Se for isso, o achado é
"não há achado", e reporto-o com o mesmo destaque que daria a um defeito.**

## Predições seladas

1. **O lado do código do guard é enumerado à mão** (lista de strings no teste).
2. **O lado da doc é repetido em Rust, não lido do ficheiro** — logo a doc pode divergir outra vez sem
   vermelho.
3. **O corpo credita ao guard a propriedade "uma quinta variante quebra a build"**, que já era
   verdadeira por exaustividade.
4. **Só o comentário de `verdict_tag` foi corrigido; o `"Three answers"` do `monitor.rs` sobrevive.**
5. **O snippet de `jq` do QUICKSTART não é tocado**, só a tabela acima dele.

**Assinado antes de abrir o PR.** O valor da review são os DELTAS — incluindo o delta de eu ter vindo
enviesado e a predição ter falhado.
