> **PROVENANCE: this document became the review published on PR #202.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# FROZEN — critérios do #202, derivados ANTES de ler o corpo do PR e o diff

**Marcador de congelamento.** Derivado de: (a) o corpo da issue `#199` (a ESPECIFICAÇÃO, escrita por
quem me pediu a review), (b) os nomes dos dois ficheiros tocados (`ci/classify-run.ps1` novo,
`ci/gate.ps1`), (c) o título do PR. **Não li o corpo do PR nem o diff.**

O que me foi dado foi a PERGUNTA e não a resposta: *das três condições para um RED ser aceite como
não-atribuível, só uma é conferível por script; as outras duas são exigidas explicitamente — isso é
guarda ou é honra?*

## O que a especificação exige (#199, aceitação)

1. manifesto gravado **sem ninguém escolher**, e que **sobrevive à branch ser apagada**;
2. `SLOT.log` **append-only**, com o dia contável a partir dele;
3. **o controlo que prova que mede**: duas corridas → **dois manifestos e quatro linhas de log**;
4. daí: corridas/dia, wall-clock/corrida, utilização do slot.

## OS SETE CRITÉRIOS — selados antes de abrir o PR

**C1 — "SEM NINGUÉM ESCOLHER" INCLUI A CORRIDA QUE MORRE.** O manifesto é escrito
incondicionalmente? **A corrida que mais interessa registar é a que rebentou a meio** — se a escrita
vive no caminho de sucesso, ou dentro de um `try` cujo `catch` a engole, as corridas ausentes do
registo são exatamente as que a issue quer contar. **Verificar: `finally`, e não `try`.**

**C2 — "FORA DA WORKTREE" TEM DE SER FORA DE TUDO O QUE É APAGÁVEL.** Sobreviver à branch é metade.
A outra metade é a lição do lock: **um marcador que vive dentro do recurso que o protocolo destrói
desaparece exatamente quando é preciso.** Se o destino for um target dir, `cargo clean` leva-o.

**C3 — "APPEND-ONLY" É PROPRIEDADE OU CONVENÇÃO?** Um ficheiro que também pode ser truncado é
append-only por educação. **Verificar o MODO de escrita**, e se alguma coisa impede a reescrita. Se
for convenção, tem de estar DITO como convenção.

**C4 — O CONTROLO DA ACEITAÇÃO 3 É O QUE A PRÓPRIA ISSUE DIZ SER O MAIS PROVÁVEL DE SALTAR.**
Verificar se foi feito **e com que grão**: *"dois manifestos e quatro linhas"* provado por **nomes de
ficheiro e linhas citadas**, ou por *"confirmado"*? **Contagem não separa duas corridas registadas de
uma corrida registada duas vezes.**

**C5 — A CONDIÇÃO CONFERÍVEL POR SCRIPT PODE ESTAR QUASE AO CONTRÁRIO, e esta é a minha aposta
principal.** *"O ficheiro que falha está fora do diff"* como sinal de não-atribuição **inverte o caso
normal de uma regressão**: uma mudança quebra tipicamente um teste **noutro** ficheiro. Se essa
condição empurra para "não-atribuível", **o mecanismo certifica regressões reais como pré-existentes**
— e fá-lo com um artefacto legível e citável, que é a pior forma de errar.

**C6 — AS DUAS CONDIÇÕES NÃO-CONFERÍVEIS: guarda ou honra?** Predição: são **asserções do autor**, e o
artefacto de saída **não distingue verificado de afirmado**. **Reparo mínimo: cada condição carrega a
sua PROVENIÊNCIA no registo** (machine-checked / asserted), senão o leitor recebe um veredito uniforme
"NOT ATTRIBUTABLE" que é meio medido. **É proteção afirmada mais larga do que aquilo que a impõe.**

**C7 — O VERMELHO CAI NA ASSERÇÃO QUE LHE PERTENCE?** O autor verificou quatro casos ao vivo. **Um
vermelho que não cai na asserção do próprio guard é decoração.** Verificar se cada recusa é nomeada
pelo **sítio/mensagem que lhe pertence**, ou se o registo diz apenas *"houve recusa"* — que é o
achatamento que torna quatro casos indistinguíveis de um.

## Predições seladas (morrem se o diff mostrar o contrário)

1. **A escrita do manifesto não está em `finally`** — logo a corrida que rebenta não deixa registo.
2. **O artefacto de saída não distingue condição VERIFICADA de condição AFIRMADA.**
3. **"Ficheiro fora do diff" é usado como evidência A FAVOR de não-atribuição**, sem a ressalva de
   que a regressão típica falha fora do diff.
4. **O controlo das duas corridas está reportado por CONTAGEM e não por nomes/linhas citadas.**
5. **`append-only` é convenção e não propriedade**, e não está dito que é convenção.

**Assinado antes de abrir o PR.** O valor da review são os DELTAS entre isto e o que o autor escreveu.
