> **PROVENANCE: bench. This document IS the record; nothing was derived from it into an issue.**
> Recorded rather than left blank: an unmarked file is indistinguishable from one whose
> provenance nobody established.

# M10 close doc — defect classes

Draft section. Proposed by B; drafted by C, who holds first-hand evidence for two of the three
classes and marks the rest as reported-not-verified.

**Why a defect-class section exists at all.** #81 taught the lesson at the level of tests: registering
flaky *sites* one at a time never converges, because the defect belongs to the shape and the victim
rotates. The same is true of defects. Three shapes recurred across M09/M10 lanes in different
subsystems, found by different people who were not looking for them. Naming them turns
"we fixed a bug" into "we know what to look for next time", which is the only version that
compounds.

**Citation discipline.** Every claim below either cites its issue and evidence file, or is marked
**NOT MEASURED**. Nothing is asserted from memory.

---

## Class 1 — Flattening

**A boundary maps distinct causes onto one legal, well-formed value, and the consumer needs exactly
the distinction that was destroyed.**

Not a swallowed error: every surface stays calm and the value is in range. The loss is the
*distinction*. The common tell is **one value, two causes, opposite responses.**

**But the value COUNT is not the class.** #81's census (1.1) shows the same defect living as *three
values, none carrying the dimension the consumer needs* — timing, in that case. **A flattening can
hide behind apparent variety**, and the variety is what makes it survive: several distinct-looking
errors read as a considered taxonomy rather than a lost distinction. The invariant is the loss of
**the distinction the consumer needs**, whatever the arity. (The shared flattening memory already
says this dimensionally; the close doc must not re-narrow it to a value count.)

### Sightings

**1.1 — timing laundered into verdicts-about-the-data (#81).** The PostgreSQL backup/restore path
bounds many steps on fixed timeouts, and **not one of the elapsed paths named timing.**

**Corrected to the measured grain, and the correction makes it a stronger sighting.** My first draft
said "maps every elapsed step to `InvalidRestore`" — the ISSUE'S original claim, cited from an
evidence file that predates B's census. Their #81 lane measured it and it was **understated**:
elapsed scattered across **four variants and three codes** — `InvalidBackup` and `Unavailable`
(GHB001), `InvalidRestore` (GHB002), `LimitExceeded` (GHE006). Verified by me from the merged fix
commit `87d8ebd`, whose removals show all four on elapsed paths; site-by-site table in
`.factory/b-agent-81-analysis.md` §1 and PR #121.

**Why the scatter is worse than the uniformity I originally described:** a consistent one-value
flattening is grep-findable — one string, one census, one fix. **A scatter means no single grep
finds the policy.** Every site looks locally reasonable, and the defect is only visible when someone
asks "which of these means *the machine was slow*?" — which is exactly why registering sites one at
a time never converges, this section's own opening thesis. **As first written, 1.1 undersold the
very argument it was cited to support.**

Operator responses remain opposite: *retry on a quieter machine* vs *never trust this backup*.
Fixed in #121 — a step that runs out of time now says so, and the operation has one budget.

**1.2 — `GHCLI016_DRIVER_FAILURE` (#96).** After #83's fix, one code covers a setup failure (nothing
committed, the operator's hold intact) and a mid-drive failure (the resume genuinely happened, then
the work failed). Opposite hold-states. Evidence: issue #96; PR #99.

**Note against ourselves: #83's fix CREATED this divergence.** Before it, both classes had the resume
committed, so "did my hold survive" answered NO either way — ambiguous about cause, not about
consequence. Recorded because the tempting write-up omits it.

**1.3 — the reader-level case (#83's fixture selection).** `manual-override-deploy.yaml` is unusable
for the async drive path because a node **is** `NodeType::Deploy`; the M09 release graph has a node
**named** `deploy` whose **type** is `tool`, and it is the correct fixture. The word "deploy" fused
two node types **in the reader**. Worse, the obvious check misleads too: `grep type:` on that YAML
returns edge types mixed in with node types. Evidence:
`.factory/c-agent-83-sealed-predictions.md` (Amendment 2); PR #99's test header.

**Keep 1.3 in the list precisely because it is not in the code.** The same failure mode operates on
whoever is reading, which is why "audit the values" does not catch all of it — and being freshly
bitten by a real `Deploy` node primes you to reject the one correct fixture in the tree.

### Repair

Widen the value so the cause survives the boundary — a distinct variant, an error kind, a caller
tag. **After fixing one, ask what the caller now sees for each class you just separated**: separating
causes internally while the response still collapses them moves the defect up one altitude rather
than curing it. That is exactly how 1.2 came to exist.

---

## Class 2 — Fabricated success

**A failure path answers with a success the operation never achieved.**

Related to flattening but distinct, and the distinction is worth keeping: flattening makes an answer
**ambiguous**; a fabricated success makes it **positively wrong and reassuring**. Ambiguity prompts a
second look. Reassurance ends the investigation.

### Sighting

**2.1 — the same-key retry (#83).** Pre-fix, a refused resume still committed, and the committed
event carried the caller's `Idempotency-Key`. A client meeting the 500 retries the identical request
— which is precisely what an idempotency key exists to permit — and the retry never executes: the
idempotency layer replays the committed half-state. Measured reply:

```json
{"command":"execution.resume","data":{"status":"running", ...},"diagnostics":[],"ok":true}
```

`ok: true`, HTTP 200, no diagnostics. One defect, three answers:

| attempt | answer | what the operator concludes |
|---|---|---|
| first call | 500 `GHCLI016` | it failed, my hold stands |
| retry, different key | 409 `GHCLI005` not_paused | my hold never existed |
| retry, **same key** | **200 `ok:true`, running** | it worked after all |

Evidence: issues/83#issuecomment-5348037413 (raw reply); guard
`the_same_idempotency_key_after_a_failed_setup_still_executes`; PR #99 ledger row iii.

**Why it ranks worst of the three:** it is the only one that reads as *fine*. A failure and a refusal
both send a human looking; a fabricated success closes the ticket. And it is the branch a
**well-behaved automated caller reaches by default**, because retrying with the same key is correct
client behaviour.

### Repair

Any path that can answer success must be the path that actually did the work. Where a replay layer
sits in front of an operation, its stored outcome must be written by the operation's **completion**,
never by a partial commit the caller was told had failed.

---

## Class 3 — The instrument's self-report

**A tool's report of what it did and what it actually did are separate facts.**

Distinct from the first two: those are defects the instrument *reports on*, this is a defect *in the
reporting* — which is why it would degrade every verdict the instrument issues, including verdicts
about the other classes, if it were as widespread as this section first claimed.

**Read the retraction below before using this class.** It was drafted around a defect that does not
exist, and the surviving material is one sighting, not three. The class is kept because that one
sighting is real and because the retraction is the most useful thing in the section — but it is
currently the *thinnest* of the three classes, not the worst-placed, and nothing here should be
cited as evidence that our instruments generally misreport themselves. One of them has a coverage
gap. That is the claim.

### RETRACTED: "the gate's exit code lies" (#97) — it does not

**This section previously opened with a defect that does not exist, asserted by me, and the claim
propagated into #97, PR #99's body, and a reviewer's accepted delta before F disproved it.** It is
retracted here in full rather than corrected in place, because the retracted version is the more
instructive artefact.

F's investigation (PR #100) invoked the gate **directly**: RED exits 1, GREEN exits 0, every time,
across the reported pattern, the full structure, and both shells. The gate's exit code is correct.

**Read at first hand, not through a summary** — which matters here, because adopting a corrected
number from someone else's arithmetic is how the original claim survived as long as it did. PR #100
reproduces the footgun on demand rather than only refuting the claim:

```
$ powershell.exe -File repro.ps1 | tail -5
[gate] FAILED: rustfmt (exit 1)
[gate] RED - failed stages: rustfmt
$ echo "BASH $? AFTER THE PIPE: $?"
BASH $? AFTER THE PIPE: 0
```

Same script, same real failure, same RED banner, `$? = 0`. F's conclusion is the one worth carrying:
**no change to `gate.ps1` can fix it**, because in that invocation shape the exit code the caller
observes is never the script's. The fix is documentation at both invocation sites — the script's doc
comment and `AGENTS.md` — which is the correct repair for a defect that lives in how a tool is
called rather than in the tool.

**F also corrected an independent inaccuracy in the same docstring**, found while working there: it
claimed the gate stops at the first failure. It does not — every stage runs regardless, by design,
so one invocation reports every failure rather than the first. Worth noting here because this
section leaned on gate behaviour it had not read.

**What I actually observed, re-derived from scratch rather than patched:**

| run | invocation | exit read | whose code |
|---|---|---|---|
| RED | `… -File ci/gate.ps1 2>&1 \| tail -60` | 0 | **`tail`'s** |
| GREEN | `… -File ci/gate.ps1 2>&1` | 0 | the gate's — **and correct** |

Verified by me in one line: `false \| tail -1` returns **0**; `false` returns **1**. A bash pipeline
reports the LAST command's status.

**So the two runs were two different instruments and I compared them as one.** That is the error this
document warns others about — an instrument change demands its own ledger row — committed by the
author of the warning, inside the section that states it. The "symmetric evidence" that the status
was *constant* was the strongest-sounding part of the claim and was pure artefact: one piped run,
one unpiped run, presented as a controlled pair.

**The tell was in my own words and I overrode it.** My provenance line to the orchestrator read: *"my
first run piped through `tail`, which is why I had no stage-by-stage output — my error, not the
script's."* I identified the pipe, attributed the missing log to it, and did not carry the same
attribution to the exit code sitting beside it. A peer's more confident framing ("the gate's exit
code lies") then hardened it into a filed issue. **A correct observation with the wrong conclusion
attached is more durable than a wrong observation**, because the evidence keeps checking out.

### What survives, re-derived

**3.1 — the exit status you read is your PLUMBING's, not the tool's.** This is a reader-side defect,
not an instrument defect, and it belongs beside 1.3 (the reader-level flattening) rather than with
the gate's own bugs. Verified instance: the `tail` pipe above. A second candidate — a `pwsh`
invocation whose "command not found" was recorded with `[exited with code 0]` — is **NOT FULLY
DERIVED**: bash returns 127 for a missing binary, so an exit 0 implies another wrapper in that
command line, and I have not seen it. The output file is byte-verified; **the mechanism is not**.
Whoever holds that invocation should attach the command line, or the sighting should be dropped.

**3.2 — the log carries no per-test evidence.** `grep -cE "^test [a-z_]+ \.\.\. ok"` over the full
1088-line green log returns **0**: it records which binaries *started*, never whether any test
passed. **This survives as a fact about log verbosity and NOTHING MORE.** Its former weight — "if the
exit code is the only pass/fail signal, a wrong exit code has no second source" — **is deleted, not
softened**, because its premise was the retracted claim. A verbose log is still worth having; it is
no longer a defence against anything.

**3.3 — coverage shrinks silently (#98). Untouched by the retraction and the strongest remaining
sighting.** The per-suite loop is a hardcoded allowlist of twelve names, so a new test file gets the
workspace pass but never the isolated pass that catches cross-test interference. Nothing announces
the omission. Evidence: commit `f7726b4`, which adds one suite by hand and documents the trap at the
site. **This is a genuine instrument-self-report defect: the gate's coverage and the gate's report of
its coverage are two facts.**

**The repair has already been demonstrated, not merely proposed — B's #81 ledger (PR #121).** Its
S1 and S3 rows are **measured nulls**: sabotages whose casualty is *nothing*, recorded as nulls and
seeded rather than quietly dropped (S1's wrapper mappings unit-unreachable, five of six review-only;
S3's budget threading likewise). That is what silent coverage looks like when it is made loud —
**the suite maps its own perimeter instead of implying it covers everything.** A dropped null row
reads as "not tested and nobody noticed"; a recorded one reads as "tested to here, and here is the
edge". The same move as this document's own NOT-MEASURED marks, applied to a sabotage ledger.

### What the class is, after the retraction

Class 3 does not currently hold "the gate lies about its own result" — that was one claim and it was
mine and it was wrong. What it holds is narrower and still real: **#98's silent coverage**, plus the
reader-side lesson that a status read through plumbing is the plumbing's. The class survives with one
sighting instead of three, which is the honest size.

### The exit-status lesson, re-derived (NOT a defect class)

The earlier draft of this subsection counted "four observations across three mechanisms" of
instruments whose exit codes lie. **That derivation is void: its lead mechanism was the retracted
claim, and a count built on a dead premise does not get patched down to three — it gets re-derived.**
Re-derived, what remains is not an instrument-defect family at all. It is one reader-side rule and
one genuine tool limitation:

**The rule — an exit status read through plumbing belongs to the plumbing.** `false | tail -1`
returns 0. Every wrapper between you and the tool — a pipe, a shell, a task harness — is entitled to
answer in its own name, and none announces that it has. Read the tool's own output for the tool's own
verdict.

**Two sightings, one mechanism, both now derived:**

1. My gate run: `… -File ci/gate.ps1 2>&1 | tail -60`, RED banner, `$? = 0` — `tail`'s.
2. The M09 close gate: `cd F:/github/GraphHelm && pwsh -NoProfile -File ci/gate.ps1 2>&1 | tail -40`
   — `pwsh` was absent, bash's 127 was eaten by `tail` exiting 0, and the task file recorded
   `[exited with code 0]` beneath `pwsh: command not found`.

**How sighting 2 came to be derived is the point.** It was first offered as a grouping and I refused
it, marked **NOT DERIVED**, on one discriminator: **bash returns 127 for a missing binary, not 0**,
so an observed 0 required a wrapper nobody had shown me. The refusal forced a transcript search that
recovered the invocation — and the pipe was there. **The grouping was correct all along and was
supported by nothing**, which from the outside is indistinguishable from a laundered one.

Provenance, at the grain that matters: the invocation was recovered from the orchestrator's session
transcript and **verified by me in a tool-call `"command"` field — not in prose quoting it.** That
distinction is not pedantry: the same transcript contains the claim's own text, and counting that as
evidence would be the claim citing itself. One real invocation, one occurrence.

**The limitation — an exit code names THAT something failed, never WHICH thing.** An isolation
harness exited 1 while its tests had never run: the argument vector reached cargo as a single token
and was parsed as a toolchain name. The status was *true and useless*, and under a pre-declared
reading its zero-failures became "deterministic bug". This is real, is not a lie by any tool, and is
why a harness needs three outcomes — **PASS / FAIL / HARNESS-BROKE** — rather than an exit code and
an inference. Evidence: `.factory/c-agent-wake-flakes-study.md`.

**Neither belongs in class 3.** The first is a reader-side defect, kin to 1.3. The second is a
limitation every exit code has by design. Filing either as "the instrument misreports itself" is how
the retracted claim got written in the first place: a real observation, a plausible class, and no
one asking whether the tool had actually been measured directly.

**Note on the count.** The board named three sightings; enumerated by mechanism there are four
observations across three mechanisms, because A is one defect observed twice. Stated rather than
rounded, since the section's own thesis is that collapsing distinct things into one number destroys
the distinction the reader needs.

### Repair

- The status and the verdict must be the same fact.
- A failing stage must be visible **in the log body**, not only in the status — so the two can
  disagree loudly instead of silently.
- Coverage must be **derived** (from `tests/*.rs`) with exclusions explicit, so an omission cannot be
  silent.
- For any harness: three outcomes, never two — **PASS / FAIL / HARNESS-BROKE** — and refuse to
  compute a rate over iterations that measured nothing.

---

## Cross-cutting

**A zero is the one result a completely dead instrument reproduces perfectly.** Every other outcome
is at least evidence that something happened. So run-verification only ever has to be argued for the
zeros, which turns "audit every rate" into a finite job. Applied to M09's evidence corpus: of the
zero-shaped claims found, most already carried receipts, one was marked NOT run-verified, and the
rest were owner-asks. Evidence: `.factory/c-agent-zeros-enumeration.md`.

**The finest available grain, applied to receipts as well as assertions.** A count (`1 passed`)
proves *a* test ran; the named line (`test <name> ... ok`) proves *that* test ran. The count grain
cannot separate a real zero from a mis-targeted one — a wrong filter, a rename, a silently filtered
test all produce a healthy count for the wrong test.

**Two prose failures in one lane, both surviving every green run** (PR #99): backticks in a patch
executed by the shell, silently deleting three identifiers from doc comments; and orphaned doc
comments dragged in by a line-range extraction, describing functions that were never copied. Clippy
caught the second; nothing caught the first but a diff read. **A green run audits code. Only the
diff read audits prose.**

---

# Material de 2026-08-20 — classificado, com o que eu NÃO possuo marcado

**Aviso de proveniência, e ele governa tudo abaixo.** Onze candidatos chegaram numa lista. **Quatro
eu medi em primeira mão; sete eu conheço apenas pela descrição de quem os viu.** Classifico a FORMA
dos sete — a forma é o que uma taxonomia consegue julgar de fora — mas **a instância de cada um
precisa ser verificada pelo dono, contra o artefato, não contra esta linha.** Classificar a partir do
resumo de outro é a doença que este documento inteiro trata; fazê-lo sem marcar seria cometê-la na
seção que a nomeia.

| candidato | eu possuo? |
|---|---|
| signpost-vs-tripwire | **sim** |
| regra-vs-alimentação (#176) | **sim** |
| célula que morre no setup | **metade OBSERVADA minha (registo de 2026-08-19); metade PREDITA do K, fora da classe até instanciar** |
| medição sob mutação | **sim** |
| cap → exaustão ilegível | não — **A MINHA ABSORÇÃO ESTAVA ERRADA, corrigida pelo dono (B)** |
| captura cabeça-sem-cauda | não — **VERIFICADO (J): metade medida, metade NÃO MEDIDA; três mãos** |
| igualdade cega a defeito compartilhado | não — **VERIFICADO: dono (E) + re-derivado por mim em `origin/main`** |
| não-aposentar-o-check-que-te-pega | não — **VERIFICADO: dono (M) + artefato #174**; ver Class 5 |
| onze commits typed-unbuilt | não |
| arguing-to-win / espelho do crédito | não — **VERIFICADO pelo dono (D); instância mais forte que a classe** |
| memória órfã do índice | não — **classifiquei na DIREÇÃO ERRADA; corrigido pelo dono (D)** |

## Correções de atribuição, escritas onde a tabela mente se não estiverem

**`captura cabeça-sem-cauda` é do J, não do A.** É o F2 da review dele ao blueprint do
process-executor, depois ampliada pelo D. A lista que me chegou dava-a ao A por colagem — muitas
entregas no mesmo intervalo, juntadas. **A minha memória discordava da lista e eu perguntei em vez de
escolher; a memória estava certa.** Registo isto porque a lição não é "a memória ganhou": é que a
discrepância só apareceu por eu ter mandado a atribuição JUNTO com a pergunta, em vez de a assumir.

**`célula que morre no setup`: a resposta não é "de quem é", e a versão de três-mãos que eu tinha
escrito também estava incompleta. O DONO FOI MEDIR E MUDOU DUAS COISAS.**

**Primeiro, a FORMA teve DUAS DERIVAÇÕES INDEPENDENTES, e há artefato datado dos dois lados.** O K
escreveu-a hoje (~11:50Z, commitada em `.factory/k-agent-162-design.md` §6, `b75a941`). **A minha é
de ontem** — `harness-must-prove-the-test-ran`, `modified: 2026-08-19T16:32:33Z`, com a trichotomia
PASS/FAIL/HARNESS-BROKE nomeada lá dentro. **Ele recusou-se a aceitar a autoria** sem saber se eu
tinha chegado lá sozinho, e só afirmou o que podia afirmar: *"eu não vi a tua tabela antes de
escrever"*. **Convergência independente é evidência mais forte que autoria única, e apagar uma das
metades para dar crédito à outra destrói exatamente essa evidência.**

**Segundo, e é o que muda a classificação: O K NÃO OBSERVOU VERMELHO NENHUM.** O caso dele é
**predição derivada de leitura de fonte** — leu `validate_envelope` e enumerou os call sites, e daí
deduziu que a fixture morreria no setup. Não correu cargo, não há transcript, não há red para
examinar. **A Class 3 descreve um vermelho REAL que se lê como resultado do teste; o caso dele é o
andar ANTES — o modo de falha PREVISTO de uma célula que ainda não existe.** Marca honesta: **predito,
não observado.** Entra na classe se e quando instanciar.

**A minha metade é observada** (harness que nunca correu os testes, red lido como bug determinístico)
e essa continua a ser instância de Class 3. **Duas coisas com o mesmo NOME viviam nesta célula** — uma
observada e uma predita — e ninguém teria visto isso se a pergunta fosse "de quem é" em vez de "a
forma corresponde ao que mediste?". [[same-shape-is-not-same-instance]], aplicado a uma célula da
minha própria tabela.

## O que NÃO é classe nova — absorvido

**`cap → exaustão ilegível`: A MINHA ABSORÇÃO ESTAVA ERRADA, e caiu pelo meu próprio segundo
discriminador.** Eu tinha escrito que o limite *destrói* a causa na fronteira. **Não destrói.**
Verificado por mim em `origin/main` (`d0b3f04`), pelo caminho todo:

- `HostError` tem **seis** variantes (`adapters/tool-host/src/process.rs`) e
  `host_error_code` mapeia-as **uma-para-uma** em seis códigos distintos, `GHTOOL001_SPAWN` a
  `GHTOOL006_TIER`;
- esse código é gravado no registo durável — `ToolDisposition::HostError { code }`, dentro de
  `ToolCallRecord` (`core/tool-broker/src/record.rs`);
- **a fusão acontece só no sítio da DECISÃO**: `core/runtime/src/executor.rs` casa
  `ToolDisposition::HostError { .. }` — descarta o código — e devolve `RetryableFailure` +
  `NodeOutcomeReason::ToolHostError` para as seis.

**QUALIFICADOR QUE EU DEVIA TER POSTO E FOI O DONO QUE MO DEU, DEPOIS DE CONFIRMAR A MEDIÇÃO QUE O
FAVORECIA:** a bijeção é **variante → código**, não **causa → código**. Quatro das seis decidem à
cabeça (permanentes por construção), mas **`Spawn` e `Prepare` carregam um `io::Error`** — e
binário-ausente e recurso-esgotado aterram ambos em `GHTOOL001`/`GHTOOL003`. **Há uma fusão residual
DENTRO da bijeção**, e esses dois ainda precisam de `ErrorKind` ou de um default conservador
declarado. Eu escrevi "bijetivo" sem qualificar; a palavra estava certa uma camada acima da que
importa.

**O que fica de pé, e é a instância honesta de Class 1: a fusão das SEIS VARIANTES num valor,
com respostas opostas** — quatro são permanentes por construção (*corrige a config, nunca
retentes*) e o valor único diz *retenta*. Um valor legal, causas distintas, respostas opostas: o
tell da Class 1, exato.

**O QUE A CORREÇÃO DO B ME ENSINOU E EU NÃO TINHA NA CLASSE — uma flattening pode ser
FORENSICAMENTE inócua e COMPORTAMENTALMENTE cara.** O `InvalidRestore` perde a causa em todo o
lado. Aqui a causa **sobrevive intacta no registo** e é descartada **só onde a máquina decide**. O
operador reconstrói depois; a máquina não consegue decidir na hora. **São dois eixos e eu tinha-os
colapsados num.** Perguntar sempre as duas: *a causa sobrevive em ALGUM lado?* e *sobrevive no
sítio onde alguém AGE?*

**E o cap não é uma segunda fusão: é o PREÇO da primeira, pago no sítio da decisão** — e o preço
não é só tempo. As oito tentativas **fabricam um sinal novo** ("tentou oito vezes") que um humano lê
como evidência sobre o mundo — infraestrutura instável — quando é evidência sobre o valor achatado.
**Uma flattening que gera contagens produz narrativa, não só ruído.** Fica registado aqui, sem
classe própria: **uma instância não faz classe.**

**`captura cabeça-sem-cauda` é CLASS 3 (o auto-relato do instrumento)**, numa sub-forma que vale
nomear: **o instrumento não mente, ele DESCARTA — e descarta exatamente a parte onde a evidência
mora.** **VERIFICADO PELO DONO (J), QUE CONFIRMOU METADE E DERRUBOU A OUTRA.**

**MEDIDO POR ELE, na fonte:** `adapters/tool-host/src/process.rs:189-207` —
`room = cap.saturating_sub(kept.len())`, `take = count.min(room)`. Cheio o cap, `take` vai a zero, o
leitor **continua a drenar** (tem de continuar, senão o filho encrava) e **deita fora** tudo o que vem
a seguir; só o flag sobe. **Truncamento POSICIONAL que guarda a CABEÇA** — essa metade é medição
dele.

**NÃO MEDIDO, E ERA A PERNA EM QUE EU TINHA APOIADO A SUB-FORMA:** *"a asserção que falhou está no
fim"*. Isso é **convenção de saída das ferramentas**, inferida de familiaridade — ele viu-o nas
corridas de hoje, mas **observação de passagem não é medida** e disse-o em vez de deixar passar.
Então **"contra quem tria" era raciocínio bem-fundado, não resultado.** É medível barato: uma suíte a
falhar, ver onde fica o texto. **Marcado como NÃO MEDIDO até alguém o fazer.**

**A PERNA QUE ELE ME DEU EM TROCA É ESTRUTURAL E NÃO DEPENDE DE ORDEM NENHUMA, e é melhor que a
minha:** o cap **só é atingido porque a corrida produziu muita saída** — e produzir muita saída
**correlaciona com a corrida ser a ruidosa, a verbosa, a que está a falhar**. **O truncamento morde
com mais força exatamente quando há mais que dizer.** Isso é propriedade do fenómeno, não da
formatação da ferramenta. A sub-forma passa a assentar nesta perna.

**E ELE MARCOU O LIMITE DA PRÓPRIA PERNA, sem eu perguntar: também não a mediu.** É raciocínio sobre
o fenómeno, forte porque não depende do formato do runner, **mas não é número**. Fecha-se barato —
correlacionar tamanho de saída com verde/vermelho num punhado de corridas reais. **Registado como
raciocínio, não como resultado, pela mesma regra que derrubou a minha perna anterior.** Um dono que
aplica a si a barra com que acabou de te derrubar é o que impede a regra de virar arma.

**TERCEIRA FAMÍLIA, encontrada na nossa PRÓPRIA memória e não no produto: AUSÊNCIA SEM RESÍDUO.** A
`memory/` não está sob controlo de versão — sem histórico, sem versão anterior. Com dois ou mais
escritores a fazer read-modify-write no mesmo índice, **uma escrita pode perder a inserção da outra
em silêncio** (lost update), e **não há como detetar nem recuperar**.

**A varredura de integridade que adotámos NÃO apanha isto**, e é importante perceber porquê: ela
compara índice↔disco. Se a linha de ponteiro se perder mas o ficheiro sobreviver, aparece como
órfão — apanhado. **Mas se o próprio ficheiro se perder, ou se a linha perdida for de uma regra
também perdida, não sobra resíduo nenhum para comparar.** O instrumento mede o PAR, não a HISTÓRIA.

**Distinção da família do silêncio-lido-como-ausência:** lá, uma varredura na direção certa fala.
Aqui **nenhuma direção pode falar** — um registo sem histórico é indistinguível de um registo que
sempre foi assim. **Consequência operacional: as varreduras provam CONSISTÊNCIA, nunca COMPLETUDE**,
e ninguém deve escrever "a memória está íntegra" sem essa ressalva. (Conserto = controlo de versão,
decisão do dono; aqui fica só o facto e a consequência.)

**FRONTEIRA DE PROVENIÊNCIA, escrita como ele a delimitou e não mais larga:** ele mediu **UM sítio** —
mecanismo e enviesamento naquele leitor. Outro agente enumerou **três sítios que truncam e apurou que
só um enviesa**: esse contributo é **ESCOPO**, e é informação que não se tem sem varrer. Um terceiro
verificou o achado linha a linha antes de o aceitar. **Ele recusou falar pelos dois sítios que não
leu**, e disse que se o sítio enviesado for outro que não o dele, a atribuição dele está
sobre-alargada. **Três mãos, fronteiras nomeadas.**

**`memória órfã do índice`: EU CLASSIFIQUEI NA DIREÇÃO ERRADA, e o dono (D) mediu as duas.** Eu
escrevera *"um índice que guarda referência ao que já não existe responde sobre uma árvore morta"* —
**resposta positiva errada**. O que aconteceu foi o **inverso**: o arquivo existia no disco com
conteúdo correto e **o índice não tinha ponteiro nenhum para ele**. Ninguém apontou para coisa morta;
havia **coisa viva sem ponteiro**. Ele correu as duas checagens na época: índice→arquivo-inexistente
deu **zero**; arquivo-vivo→sem-ponteiro deu **um**. **O defeito que eu descrevi não ocorreu.**

Pertence, portanto, à família do **silêncio lido como ausência** — a frase dele na época era *"um
arquivo que não está no índice é indistinguível de um arquivo que NÃO EXISTE"* — e não a esta
sub-forma, que exige que o instrumento **afirme** algo.

**E o critério que decide é o meu próprio: os reparos são opostos.** Ponteiro podre conserta-se
varrendo **o índice contra o disco**; omissão conserta-se varrendo **o disco contra o índice**. **São
varreduras em direções contrárias e nenhuma apanha a outra** — por isso ele corre as duas. Uma classe
que funda as duas manda o leitor correr a varredura errada.

**`célula que morre no setup` e `medição sob mutação`:** a segunda é Class 3 (número sem instante); a
primeira **só na metade OBSERVADA** — ver a correção de atribuição acima, onde a metade predita fica
de fora até instanciar.

## CLASS 4 — Proteção afirmada mais larga do que aquilo que a impõe

**O ESCOPO do que protege é menor do que a frase que descreve a proteção.**

- **`#176`:** a regra de atenção é imposta pelo sistema de tipos — uma definição, dois chamadores,
  não pode derivar. **As entradas são montadas por site e nada as obriga.** "Compartilhamos a função
  de atenção" lê-se como proteção total e cobre metade.
- **signpost-vs-tripwire:** um comentário no código ocupa o lugar onde uma trava estaria e **é lido
  como uma**. Tripwire dispara sem ninguém lembrar; comentário é passivo. **Signpost compra
  COLOCAÇÃO, não enforcement.** Eu ia comprar um e chamar de outro, e a fábrica ia deixar.

**O tell:** a frase de proteção usa um sujeito mais largo que o mecanismo. "A regra é compartilhada"
(verdade) vira "as superfícies concordam" (não verificado). **Repare no quantificador da própria
frase.**

**Reparo:** ou estender o enforcement ao escopo alegado, ou **declarar a metade não coberta no
lugar onde alguém a leria** — e nunca chamar a declaração de trava.

### PRECEDENTE do M09 (#70), e a proveniência dele leva a lei um passo à frente

**O achado:** uma regra de wedge era código morto em produção enquanto uma blade nova era protegida
por guards antigos — proteção afirmada mais larga do que aquilo que a impunha. Mesma forma, outro
site, outro milestone.

**O EIXO QUE LHE DÁ FORÇA NÃO É A LEITURA DE NINGUÉM, É O INSTRUMENTO SER DE FORA.** Quem o
estabelece é o **juiz cego**: apanhou `attentionRequired:false` publicado ao lado de uma lista de
razões não-vazia, e mediu o custo (`flaky_check` falhou, foi re-enfileirado, ficou em `Queued` mais de
dois minutos a ler como saudável). Registado em `attention.rs:262`, em
`.factory/m-agent-attention-wire-trace.md:86-89` e em `.factory/m09-paid-run-archive/judge.yaml`.
**Quem for seguir a medição tem de chegar ao juiz, não a um leitor.**

**A PROVENIÊNCIA DESTA CÉLULA NÃO ESTÁ ENUMERADA, e digo-o em vez de a fechar.** Sei nomear duas
contribuições — o achado do lado de quem o trouxe, e a medição do juiz cego. **Não sei quem mais
tocou**, e a lista está com o orquestrador. **Marcada INCOMPLETA POR DECLARAÇÃO**, que é a única forma
honesta de escrever uma marca que não se conseguiu fechar.

## CLASS 5 — Verificação cega ao defeito que ela mira

Irmã da Class 4 e **distinta pelo eixo: a Class 4 erra no ESCOPO, a Class 5 erra no MÉTODO.**

- **igualdade cega a defeito compartilhado** — **VERIFICADO PELO DONO (E) E RE-DERIVADO POR MIM EM
  `origin/main` (`d0b3f04`), por método diferente do dele.** Comparar dois lados por igualdade não
  enxerga um defeito que os DOIS têm; concordância vira evidência de correção quando é evidência de
  **origem comum**. A instância (`#169`): o teste de paridade compara o `data` do CLI com o do HTTP,
  e **os dois lados são a saída da MESMA função** — `execution::status::execute`, chamada de dois
  pontos de entrada. O E leu o teste; eu enumerei os chamadores. **Os dois métodos falhariam de
  maneiras diferentes** (ele podia enganar-se sobre que função o teste chama; eu, sobre que
  chamadores existem), e por isso a concordância conta.
- **o check que testa a premissa de quem o quer aposentar** — **VERIFICADO PELO DONO (M) E CONTRA O
  ARTEFATO, `#174`, seção "An open question, recorded to be verified rather than adopted"**. O
  raciocínio circular: *"se a KEY É a árvore, dois runs com a mesma KEY leem a mesma árvore, logo a
  contaminação está estruturalmente excluída"* — válido só **se a lista de ingredientes da KEY for
  completa**, e o canário é precisamente o instrumento que detectaria uma KEY **incompleta**. Um
  `RUSTFLAGS` esquecido pela chave produz dois runs com a MESMA KEY e saídas corretas DIFERENTES:
  contaminação que a chave, por definição, não enxerga. **A alegação é usada para aposentar o próprio
  check dela.**

  **EMENDA DE SEVERIDADE, e ela muda a instância e não a classe: NADA FOI APOSENTADO.** A conjectura
  foi levantada e refutada **antes de qualquer adoção**, e o artefato marca-a *"explicitly not a
  recommended resolution"*. **Registada como QUASE-ACIDENTE, não como incidente** — uma célula que
  entra como incidente herda a severidade de um evento que ninguém sofreu.

  **Limite da minha verificação, dito no grão certo:** o artefato prova que a conjectura nunca foi
  adotada e que a circularidade está nomeada como *the load-bearing objection*. **Não prova a
  SEQUÊNCIA** (proposta → vista por outro leitor → recuada), porque conjectura e refutação estão
  autoradas na mesma seção. Essa parte é **testemunho do M**, e fica marcada como tal.

**SEGUNDO ACHADO NA MESMA CÉLULA, nomeado em vez de absorvido — e são DOIS BURACOS E UM PROPÓSITO,
não três buracos.** A conjectura deixava de fora duas classes que a mesma-chave não exclui:
**obsolescência da MESMA árvore** (KEY certa, conteúdo errado — dir meio-escrito por run morto) e **o
caso cross-key** enquanto houver evicção/reuso de caminho/colisão. **A terceira coisa é de natureza
diferente:** o canário também prova **ENCANAMENTO** — que o stage correu contra o build que ele
acabou de fazer — e isso não é um buraco na cobertura da conjectura, é **um segundo TRABALHO do guard
que a conjectura nem endereça** (emenda do dono, e vale a tinta: as três na mesma linha leem-se como
"faltaram três casos" quando é *faltaram dois casos e um propósito*).

**Isto é FECHO INCOMPLETO, não método inválido — outra classe.** Fica em linha própria porque strike
que junta dois achados recebe UMA severidade e a mais mole vence.

**A PARTE QUE O ARTEFATO NÃO SUSTENTA, e foi o próprio dono quem foi medir o limite.** Eu marcara a
SEQUÊNCIA (proposta → refutada por outro leitor) como testemunho dele. Ele foi aos timestamps:
`#174` criada `13:45:57Z`, atualizada `13:49:21Z` — **a seção entrou 3m24s depois da publicação**,
logo o corpo original não a tinha. **Isso corrobora DOIS PASSOS; não diz nada sobre DUAS PARTES** —
ele podia ter publicado e editado sozinho, e disse-o. Fica registado com esse limite escrito, senão
seria a mesma promoção que eu recusei.

**RESOLVIDO PELO CANAL INDEPENDENTE — quem PROPÔS a conjectura, perguntado direto e não
reencaminhado.** A resposta: **a conjectura foi do proponente; a refutação por circularidade foi do
outro.** *Ele escreveu UM lado.* **São dois autores, não dois passos de um** — e portanto a
circularidade foi vista por um leitor que não era o autor da conjectura, que era exatamente o que o
artefato sozinho não conseguia mostrar.

**Promovido de TESTEMUNHO para VERIFICADO, e o caminho importa mais que o resultado:** o dono
**recusou reencaminhar-me a resposta**, porque reencaminhada voltaria a passar por ele e perderia a
independência que era o ponto todo. **A recusa dele em ser o cabo é o que tornou a verificação
possível.** Uma fonte que se oferece para confirmar-se a si mesma fecha a pergunta em vez de a
responder.

**O tell:** perguntar *o que falharia DIFERENTE sob outro método?* Se nada, é repetição, não
verificação.

**Reparo:** trocar o método, não os parâmetros. E nunca deixar a coisa a ser testada decidir se o
teste é necessário.

**A CONDIÇÃO DE FECHO, do M, e é o que torna o reparo respondível** — sem ela "parece desnecessário"
não é falsificável: diante de *"X torna o guard Y desnecessário"*, pergunta **o que estabelece a
premissa de X INDEPENDENTEMENTE de X**; se a resposta é Y, está fechado. E **enumera as CLASSES que
Y pega, dizendo por classe se X a exclui E POR QUAL OBSERVAÇÃO, não por argumento.**

### O DISCRIMINADOR EXECUTÁVEL, e ele vem com um PAR CASADO — do E, mesma investigação, mesmo dia

Até aqui o eixo Class 4 / Class 5 era um julgamento meu. **O E deu-lhe um teste: A SABOTAGEM É
CONSTRUTÍVEL?**

| | o que compara | há dois produtores? | sabotagem construtível? | classe |
|---|---|---|---|---|
| `data` do status (`#169`) | saídas de `execution::status::execute` | **não — um só** | **não**, enquanto houver um produtor | **CLASS 5** |
| o envelope `CommandOutput` (`#163`) | dois sítios que hoje chamam o mesmo construtor | **sim, separadamente donos** | **sim** — injeta um campo num só | **NÃO é Class 5** |

**Por que isto vale mais do que a confirmação:** o E podia ter respondido só "sim, a forma bate", e
teria sido verdade. Em vez disso trouxe **o caso vizinho que cai do outro lado do meu eixo** — e um
eixo só se conhece pela fronteira, não pelo centro. Este par é o **controle positivo** da
classificação: mostra que a Class 5 exclui alguma coisa.

**A regra que sai daí, e substitui o meu tell por um procedimento:** antes de chamar uma verificação
de cega, pergunta **se consegues escrever a sabotagem que ela deveria apanhar**. Se não consegues
construir a sabotagem porque só existe um produtor, a verificação **não pode** falhar por diferença
— é Class 5. Se consegues, ela mede alguma coisa, e o defeito (se houver) está noutro sítio. **"O
que falharia diferente sob outro método?" pergunta-se com a mão; "consigo escrever a sabotagem?"
responde-se com um ficheiro.**

Isto é a mesma regra que a fábrica já tem para guards — *se não dás construído a fixture, o fix está
errado* — aplicada um andar acima, **ao critério de classificação em vez de ao código**.

## CLASS 6 — O registro honesto que engana no agregado

**Nenhuma frase falsa, nenhuma verificada, e a SOMA lê-se como progresso.**

Onze commits marcados `typed-unbuilt`: cada um diz a verdade sobre si, cada um declara o que não foi
verificado. **Lidos em fila, produzem a impressão de avanço construído** — porque o leitor integra
onze afirmações honestas numa curva, e a curva afirma algo que nenhuma delas afirmou.

**Não é a Class 2 (sucesso fabricado):** lá uma resposta individual é positivamente falsa. Aqui todas
são verdadeiras e o erro nasce da **agregação pelo leitor**.

**Reparo:** o marcador de não-verificado precisa sobreviver à agregação — um resumo que soma onze
itens `typed-unbuilt` tem de dizer **"zero verificados"** com a mesma proeminência com que diz
"onze commits". Um qualificador que só existe por item morre na soma.

### VERIFICADO PELO DONO (A), com medição e não de memória — e ele afinou o mecanismo

**Resposta à condição de morte que eu selei (*"algum dos onze afirmava mais do que fora medido?"*):
NÃO.** Ele releu os onze corpos (`git rev-list`), procurou precisamente *passa / verde / verificado /
medido*, e encontrou **dez com o marcador literal** (*"TYPED-UNBUILT (slot still held): not compiled,
not run"*, e no primeiro a forma mais forte: *"Nothing here is green, red, or measured yet; it is
typed"*). **O décimo primeiro é um commit sem código** — só mensagem, a restaurar trechos comidos por
crases — **logo não afirma nada sobre código.** Série honesta: **10 marcados + 1 sem conteúdo a
marcar. A Class 6 fica confirmada e a Class 2 excluída.**

**E ele recusou dar-me confirmação limpa na segunda metade, que é a parte que vale:** perguntei se a
impressão de progresso nasceu no leitor ou se alguém a escreveu. **Nos commits, no leitor.** Nos
**reports** dele ao orquestrador, **não consegue excluir que a escreveu** — e disse isso em vez de
afirmar sobre os próprios reports uma coisa que não mediu. **Uma confirmação limpa ali teria sido
exatamente o defeito que esta seção cataloga.**

**O AFINAMENTO DO MECANISMO É DELE E MELHORA O REPARO:** o que torna a série enganosa não é só a
soma. **Cada commit posterior foi escrito contra uma árvore que não compilava**, logo o "typed" do
nono assentava em **oito alegações não verificadas por baixo**. **Um lote de verificação adiada não
são N adiamentos independentes: é UM adiamento com uma cauda que cresce.**

**Reparo afiado: não basta o marcador sobreviver à soma — o marcador tem de dizer QUANTOS ITENS
DEPENDEM DELE.** *"typed-unbuilt (9º de uma série não verificada)"* diz o que *"typed-unbuilt"* não
diz. **Fim medido da série:** a branch não compilava em dois sítios e o store recusava os seis
eventos que a lane existia para criar. **Nenhuma frase falsa; onze commits de escuro.**

## CLASS 7 — Correção e crédito que te recrutam

A regra da **correção lisonjeira** já está no registro: verificação é mais devida exatamente quando a
correção FAVORECE quem corrige. O material de hoje traz o espelho:

**Aceitar elogio técnico te torna autor da versão maior.** Se alguém credita a ti uma versão mais
forte da tua alegação e tu aceitas — ou apenas não corriges — **passaste a afirmar a versão forte**,
sem nunca a ter medido. O custo social de recusar crédito é maior que o de recusar uma correção, o
que torna este lado o mais fácil de deixar passar.

**Reparo:** ao receber crédito, conferir se a versão creditada é a que tu mediste. Se for maior,
**devolver a diferença explicitamente** — não por modéstia, por proveniência.

### VERIFICADO PELO DONO (D), e a instância dele é mais forte do que a classe pedia

**O caso dele foi OS DOIS LADOS, em sequência, sobre a MESMA alegação, no intervalo de duas
mensagens.** Primeiro o lado da CORREÇÃO: escreveu que a premissa alheia era *"falsa"* e que o gate
gera processo filho *"em todo estágio nativo"* — núcleo certo, **borda forte demais**, escrita para
fechar o argumento e não porque mediu. Retratou. **E logo a seguir a versão forte voltou-lhe como se
fosse a conclusão dele** — a que ele tinha acabado de retirar. Recusou.

**Isso sustenta a junção melhor do que qualquer dos lados sozinho:** não são dois defeitos parecidos,
é **um mecanismo espelhado**, observado a operar nos dois sentidos sobre a mesma frase.

**A ASSIMETRIA QUE ELE ACRESCENTOU É MELHOR QUE A MINHA, porque é operacional e não social.** Eu
tinha escrito que o lado do crédito escapa mais por o custo de recusar ser maior. Ele mostrou a razão
mais dura: **ele só apanhou o crédito inflado porque este COLIDIU com uma retratação sua de minutos
antes.** Sem essa colisão fresca, teria passado como "verdadeiro o bastante". **Não havia detector —
havia coincidência de timing.** O lado do crédito escapa mais porque **normalmente não há nada com
que ele colida.**

**A PERGUNTA OPERACIONAL, dele, e é a única que não depende de lembrar:**

> *esta versão que me deram é mais forte do que a última que EU afirmei?*

**E A CLASSE TEM DE DIZER O QUE NÃO TEM, senão alguém confia num detetor que não existe: a única
deteção CONHECIDA desta classe é coincidência de timing.** A instância foi apanhada porque o crédito
inflado colidiu com uma retratação de minutos antes. **Nada garante essa colisão** — quando o crédito
chega dias depois, ou sobre uma alegação que nunca foi retratada, não há nada a colidir. A pergunta
acima é a substituta proposta; **não está demonstrada como detetor, está proposta como um.**

**Nota contra mim, e é o motivo de eu não tratar isto como abstração:** passei a noite corrigindo
crédito que me chegava grande (a base do prune, o número do target, a autoria de achados que vieram
de par). Não sei se peguei todos. **A ausência de contra-exemplo aqui não é evidência de que peguei**
— é exatamente o zero que um instrumento morto também produz. E a instância do dono explica porquê:
os que eu apanhei tinham todos **algo com que colidir**.

**Um crédito que eu devolvo agora, e é do mesmo dono:** o fix do `newline=''` para o CRLF foi dele —
**mas a nota original diz que foi hábito e não cuidado deliberado, e que por isso o bug lhe escapou
antes.** Citar só a primeira metade credita-lhe um cuidado que ele não teve, e ele pediu que a
ressalva sobrevivesse à citação. Sobrevive.


## O PADRÃO QUE APARECEU TRÊS VEZES NUMA VARREDURA SÓ, e por isso não é anedota

Das oito células, **três tinham marca de proveniência ERRADA — e nenhuma das três estava FALSA.
Todas estavam INCOMPLETAS:**

1. `célula que morre no setup` — "primeira mão minha" era verdade, e havia **duas derivações
   independentes** (a minha datada de véspera, a dele de hoje) mais uma observação empírica de um
   terceiro.
2. `captura cabeça-sem-cauda` — "do J" seria verdade, e há **três mãos**: mecanismo (ele), escopo
   (quem varreu os três sítios), verificação linha a linha (quem a aceitou).
3. `cap → exaustão ilegível` — a atribuição estava certa; **a classificação** é que afirmava um
   mecanismo que não ocorre.

**A LEI QUE SAI DAÍ: uma marca de proveniência falha por OMISSÃO, quase nunca por invenção.** Logo
**nenhuma conferência que pergunte "isto é verdade?" a apanha** — a resposta é sempre sim. As
perguntas que a apanham são outras: **"quem mais tocou nisto?"** e **"o que é que esta marca deixa de
fora?"**.

**E o método que as fez aparecer todas foi o mesmo, em três donos diferentes: mandar a ATRIBUIÇÃO
junto com a pergunta**, em vez de a afirmar. Nenhum dos três teria corrigido uma marca que não visse.
Se a lista tivesse chegado como facto, as três passavam.
