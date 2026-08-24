> **PROVENANCE: this document became the review published on PR #202.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# REVIEW — PR #202, frozen-then-diff

**Critérios congelados ANTES de abrir o PR:** `c-agent-202-frozen.md` (7 critérios, 5 predições
seladas), derivados do corpo da issue `#199` e dos nomes dos ficheiros. Base: `origin/main`
`d0b3f04`, PR em `2f79fff`.

## Pontuação das predições

| # | selada | resultado |
|---|---|---|
| 1 | a escrita do manifesto não está em `finally` → corrida morta não deixa registo | **FALSA, e a solução é melhor que a que eu pediria** |
| 2 | o artefacto não distingue condição VERIFICADA de AFIRMADA | **VERDADEIRA** — S2 |
| 3 | "ficheiro fora do diff" usado como evidência A FAVOR, sem ressalva | **FALSA na forma** — é usado como RECUSA; a ressalva sobrevive noutro sítio (S5) |
| 4 | o controlo das duas corridas reportado por contagem | **nem uma coisa nem outra** — declarado NÃO FEITO (ver S1) |
| 5 | `append-only` é convenção e não propriedade | verdadeira mas **sem severidade** — o código só apende, e diz que só apende |

**A predição 1 caiu pelo lado bom e vale nomear porquê:** eu ia pedir `finally`. O autor resolveu-o
**melhor**, escrevendo `RUN-START` **antes de qualquer estágio** — *"um START sem RUN-END correspondente
É o registo de uma corrida morta"*. **Transformou a ausência num PAR**, que é a forma forte de uma
asserção de ausência, aplicada a logging em vez de a testes.

---

## S1 — `Closes #199` com o critério de aceitação 3 por cumprir, e é o critério que a própria issue
## marcou como o mais provável de ser saltado

O corpo diz, corretamente e à vista: *"no gate has been run end-to-end with these changes. The
acceptance criterion in #199 — two runs, two manifests, four log lines — needs the slot"*.

E a `#199` diz da mesma linha: **"o controlo que prova que mede… é a linha mais provável de ser
saltada"**.

**Não é desonestidade — está declarado. É a consequência de fechar assim:** a issue fecha e a linha
que provava que o instrumento MEDE fica por correr, e ninguém volta a um issue fechado. **É o
mecanismo do próprio PR aplicado a si: um registo verdadeiro quando escrito, ilegível depois.**

**Reparo, à escolha e ambos baratos:** `Refs #199` até o controlo correr, **ou** manter `Closes` e
escrever no corpo que o fecho é deliberado com o critério 3 em aberto **e onde ele será registado**.

## S2 — O DOC distingue verificado de afirmado; O REGISTO não

`classify-run.ps1` sabe a diferença e escreve-a em prosa: `-UnrelatedTestFile` é *"VERIFIED HERE
against the diff"*, `-FailThenPassObserved` é *"Not checkable from here; required explicitly"*.

**O manifesto recebe os três como campos irmãos e planos:**

```
unrelatedTestFile     = <path>
unrelatedIssue        = <issue>
failThenPassObserved  = $true
relatedToDiff         = $false
```

**Um leitor de amanhã não consegue dizer qual foi conferido por máquina e qual foi afirmado por uma
pessoa.** O `$true` do terceiro tem exatamente a mesma forma do primeiro — e o primeiro passou por
`git log --name-only`, o terceiro por nada.

**É a classe deste repositório: proteção afirmada mais larga do que aquilo que a impõe** — só que
aqui a assimetria está CORRETA no comentário e **perde-se no artefacto que sobrevive**.

**Reparo mínimo:** os campos carregam a proveniência —
`unrelatedTestFileVerifiedAgainstDiff = true` e `failThenPassObserved = "asserted-by-holder"` —
ou um bloco `evidence: { checked: [...], asserted: [...] }`. **O custo é uma linha e paga-se na
primeira vez que alguém audita um `relatedToDiff: false`.**

## S3 — A divergência que o próprio doc chama "pior que nenhuma classe" é ALCANÇÁVEL, e a
## security review afirma o contrário

O doc de `classify-run.ps1`: *"a class that lands on one of two copies is worse than no class: the
two then disagree."*

**A ordem no código:** escreve a cópia do repo (`WriteAllText`), **depois** a cópia irmã, **sem
`try`/`catch`**, com `$ErrorActionPreference = 'Stop'`. **Se a segunda escrita falhar, a primeira já
lá está** — o estado que o doc classifica como pior que nada. E o relatório `updated:` só imprime no
fim, portanto o operador vê a exceção **e não vê que a primeira cópia já mudou**.

**E a mesma operação tem DUAS políticas diferentes dentro do mesmo PR:** em `gate.ps1` a cópia durável
está dentro de `try`/`catch` com aviso; em `classify-run.ps1` a cópia irmã **não está**. O `SLOT.log`
de ambos está.

**Isto torna falsa uma frase da security review:** *"both write paths are wrapped and report failure"*.
**A escrita da cópia irmã do manifesto não está wrapped.**

**Reparo, e há um que é estritamente melhor que envolver em `try`:** escrever **primeiro a cópia do
slot** e só depois a do repo. A do repo é a committável — **se o processo morrer a meio, é melhor que
a divergência deixe a cópia committável por classificar do que classificada sozinha.** Em qualquer dos
casos, **reportar PARCIAL com os nomes** em vez de deixar a exceção falar.

## S4 — `relatedToDiff` nunca pode ser `true`: "julgado e é meu" fica indistinguível de "ninguém julgou"

`Write-RunManifest` põe `$null`. `classify-run.ps1` põe `$false` **apenas** dentro do ramo das três
condições. **Não existe caminho que escreva `true`.**

Logo o holder que lê a falha e conclui *"isto é do meu diff"* **não tem como o dizer**, e o registo
fica `null` — o mesmo valor de quem nunca julgou.

**E isso re-introduz o defeito que o PR existe para curar:** um censo que queira contar *reds
atribuíveis ao seu diff* **não consegue separar não-julgado de julgado-e-meu**. O doc diz que os dois
eixos são ortogonais e que por isso são dois campos; **mas um dos eixos só tem um sentido gravável.**

**Reparo:** um `-RelatedToDiff` explícito (ou `-Mine`), que escreve `true` **sem exigir as três
condições** — as três são a barra para a alegação BARATA (*"não é meu"*), e a cara não precisa de
barra nenhuma.

## S5 — A condição CONFERIDA é a que prova menos; a que prova mais é a que se confia
### (a resposta direta à pergunta de encaminhamento: é guarda no barato e honra no decisivo)

- **`UnrelatedTestFile` — conferida.** Mas **uma regressão típica quebra um teste NOUTRO ficheiro**;
  passar esta condição prova quase nada sobre atribuição. O que ela faz bem é **RECUSAR** o caso
  óbvio, e como recusa está certa.
- **`FailThenPassObserved` — não conferida.** Falhar-e-passar **na mesma corrida** é a evidência
  **decisiva** de flakiness, logo de não-atribuição.

**Portanto o script confere a condição que decide pouco e confia na que decide tudo.** Não é defeito
escondido — o autor diz-o no comentário, e a conjunção obrigatória impede a alegação de uma só
perna. **Mas é exatamente por isto que o S2 importa:** se a decisiva é honra, **o registo tem de
dizer que é honra**, senão o `relatedToDiff: false` lê-se como conferido.

**E há um caminho barato para a mecanizar, se um dia valer:** *fail-then-pass na mesma corrida* é
visível no log por-teste — o mesmo nome com duas linhas de resultado diferentes. **Fica como nota, não
como pedido: não medi que a saída do runner o preserve.**

---

## O que passou, e um ponto merece destaque próprio

- **`RUN-START` antes de qualquer estágio**, com o par START/END a tornar a corrida morta
  **auto-evidente**. Melhor que o `finally` que eu ia pedir.
- **O controlo negativo achou uma coisa e o autor ESTREITOU-A em vez de a vender:** o `New-Item`/
  `Join-Path` só falha não-terminantemente sob `'Continue'`, logo **não havia quebra viva no caminho
  embarcado** — e está dito assim, com a razão pela qual a versão .NET é na mesma melhor (a
  propriedade passa a ser da função e não do chamador). **Um achado reduzido pelo próprio autor é o
  oposto do que uma review costuma ter de corrigir.**
- **Recusa de re-classificar**, com a razão certa (não sobrescrever um juízo alheio).
- **Sem BOM, verificado nos bytes** (`32 30 32`, não `ef bb bf`).
- **`UNCLASSIFIED` como valor LEGAL e VISÍVEL** em vez de ausência silenciosa.

## Verificação que EU fiz, e o que ela NÃO é

Leitura do diff e dos dois corpos contra `2f79fff` e `d0b3f04`. **Não executei nada** — nem os
scripts, nem o gate. **As alegações de execução ao vivo do autor não foram reproduzidas por mim**, e
não as contesto: contesto só o que é legível no código.

## Veredito

**A mudança faz o que a issue pedia e o desenho é bom** — o par START/END é melhor que a
especificação exigia. **Nada aqui pede que a abordagem mude.**

**S3 e S4 são código** e valem antes do merge: um deixa alcançável o estado que o próprio doc diz ser
pior que nada, o outro impede que metade de um eixo seja alguma vez gravada. **S1 é uma palavra**
(`Closes` → `Refs`, ou a declaração do fecho deliberado). **S2 é uma linha por campo** e é o que
impede o registo de mentir por omissão daqui a um mês. **S5 é a resposta à pergunta, não um pedido.**
