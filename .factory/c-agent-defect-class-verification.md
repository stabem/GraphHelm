> **PROVENANCE: bench. This document IS the record of the owner-verification sweep; nothing was derived from it into an issue.**
> Recorded rather than left blank: an unmarked file is indistinguishable from one whose
> provenance nobody established.

# Verificação pelos donos — classificação das instâncias de 2026-08-20

**Por que este arquivo existe.** A seção nova de `c-agent-m10-defect-classes.md` classificou onze
candidatos. **Quatro eu medi em primeira mão; sete conheço apenas pela descrição do orquestrador.**
Classifiquei a FORMA dos sete e marquei cada um como instância-a-verificar-pelo-dono. Este arquivo
é o VARREDOR dessa ressalva: sem ele, a marca "a verificar" fica declarada e nunca varrida, que é
exatamente o defeito que a Class 3 nomeia.

**A pergunta enviada a cada dono, idêntica:** *a forma que eu classifiquei corresponde ao que tu
mediste?* — não "concordas com a classe", que convida a assentir.

**Regra de registro:** discordância entra aqui com o mesmo peso que concordância, e a linha da
seção é corrigida contra a resposta do dono, não contra a minha leitura da resposta.

| # | candidato | classe atribuída | dono | enviado | resposta | resultado |
|---|---|---|---|---|---|---|
| 1 | onze commits `typed-unbuilt` | CLASS 6 | A | 2026-08-20 | **REABERTA E VERIFICADA** | dono mediu os onze corpos: nenhum afirmava mais do que medira |
| 2 | captura cabeça-sem-cauda | CLASS 3, sub-forma "o instrumento DESCARTA" | **J (F2), não A — confirmado pelo orquestrador; a pergunta ao A caducou** | 2026-08-20 (ao A) / 2026-08-20 (ao J) | **RESPONDEU (J)** | **metade CONFIRMADA (medida), metade NÃO MEDIDA; perna substituída** |
| 3 | cap → exaustão ilegível | CLASS 1 (flattening) | B | 2026-08-20 | **RESPONDEU** | **DISCORDOU — e tinha razão; absorção corrigida** |
| 4 | igualdade cega a defeito compartilhado | CLASS 5 | E | 2026-08-20 | **RESPONDEU** | **CONFIRMADO + par casado que dá o discriminador** |
| 5 | não-aposentar-o-check-que-te-pega | CLASS 5 | M | 2026-08-20 | **RESPONDEU** | **CONFIRMADO na forma (circular), + 3 emendas** — ver abaixo |
| 6 | arguing-to-win / espelho do crédito | CLASS 7 | D | 2026-08-20 | **RESPONDEU** | **CONFIRMADO — os DOIS lados sobre a MESMA alegação; junção sustentada** |
| 7 | memória órfã do índice | CLASS 3, mesma sub-forma | D | 2026-08-20 | **RESPONDEU** | **DISCORDOU — direção errada; era AUSÊNCIA, não afirmação** |
| 8 | célula que morre no setup | CLASS 3 | **TRÊS MÃOS: observação do J, forma do K, registo meu** — nenhuma das duas marcas estava certa | 2026-08-20 | **RESPONDEU (K)** | **duas derivações independentes; e a metade dele é PREDITA, não observada** |

## Duas discrepâncias de atribuição, registradas ANTES das respostas

Registro-as agora para que não pareçam descobertas depois de conveniente:

1. **captura cabeça-sem-cauda.** O orquestrador deu-a como do A. A minha memória
   (`flattening.md`, parágrafo final) registra a forma como **F2 do J, no blueprint do
   process-executor**. Não resolvo por mim: pergunto ao A e digo-lhe qual é a outra atribuição.
2. **célula que morre no setup.** Está na minha tabela como **primeira mão minha**; o orquestrador
   deu-a ao K. Se o K a mediu, **a minha marca de proveniência está errada** — e uma marca de
   proveniência errada numa seção sobre proveniência é o achado mais caro desta lane.

Nenhuma das duas se resolve escolhendo a versão mais plausível. Ambas vão ao dono.

## Estado das sessões no momento do envio (2026-08-20), e por que isto entra no registro

Quatro donos estavam com sessão **a correr** (A, B, K, M) e dois **paradas** (D, E). As mensagens
para sessões paradas ficam em fila e podem nunca ser processadas.

**Consequência que tem de ficar escrita antes de alguém ler o silêncio:** ausência de resposta de
uma sessão parada **não é ausência de objeção**. É o instrumento não estar ligado. Se D ou E não
responderem, as linhas 4/6/7 continuam PENDENTES — nunca passam a "verificadas por não-contestação".

Isto é a mesma regra que a seção classifica: *o instrumento estava apontado, e ele FALARIA se a
coisa existisse?* Uma sessão parada não falaria.

## RESPOSTA 1 — M, linha 5 (`não-aposentar-o-check-que-te-pega`) — 2026-08-20

**Veredito do dono: a FORMA está certa — é o primeiro caso (circular), não o segundo (fecho
incompleto). Class 5 mantém-se.** Três emendas, e a segunda muda a instância.

**Verifiquei contra o artefato, não contra o resumo do M** — ele próprio apontou a fonte primária e
convidou-me a divergir dela: **issue #174, seção "An open question, recorded to be verified rather
than adopted"**. O artefato confirma a circularidade e nomeia-a como *the load-bearing objection*.

1. **Circular, com predicado mais estreito do que eu escrevi.** Não é "o invariante novo": é *se a
   KEY é a árvore, dois runs com a mesma KEY leem a mesma árvore*. Vale só **se a lista de
   ingredientes da KEY for completa** — e o canário é o instrumento que detecta uma KEY incompleta.
2. **NADA FOI APOSENTADO — e isto é emenda de SEVERIDADE, não de classe.** O artefato marca a
   conjectura *"explicitly not a recommended resolution"*. **Quase-acidente, não incidente.** Entrar
   como incidente faria o M10 ganhar um defeito que ninguém sofreu.
3. **Dois achados na mesma célula.** Além da circularidade, três classes ficam fora do que a
   mesma-chave exclui (obsolescência da mesma árvore, cross-key, canário como prova de encanamento).
   **Isso é fecho incompleto — outra classe.** Separado em linha própria: strike que junta dois
   achados recebe UMA severidade e a mais mole vence.
4. **Condição de fecho, do M, incorporada ao reparo:** diante de *"X torna o guard Y desnecessário"*,
   perguntar **o que estabelece a premissa de X independentemente de X**; e enumerar as classes que Y
   pega, dizendo por classe se X a exclui **e por qual OBSERVAÇÃO, não por argumento**.

**LIMITE DA VERIFICAÇÃO, no grão certo.** O artefato prova que a conjectura nunca foi adotada e que
a circularidade está escrita lá. **Não prova a SEQUÊNCIA** (proposta → vista por outro leitor →
recuada), porque conjectura e refutação estão autoradas na mesma seção do mesmo documento. Essa
parte assenta em **testemunho do M** e está marcada como testemunho na Class 5 — não promovida a
verificada por ter chegado junto com coisas que eu verifiquei.

## RESPOSTA 2 — E, linha 4 (`igualdade cega a defeito compartilhado`) — 2026-08-20

**CONFIRMADO, e re-derivado por mim com outro método antes de eu registar.** O E leu o teste de
paridade no seu worktree; **eu enumerei os chamadores em `origin/main` (`d0b3f04`)** e encontrei
`execution::status::execute` chamado tanto pelo CLI como pelo handler HTTP. Os dois métodos falham de
maneiras diferentes — ele podia errar sobre que função o teste chama, eu sobre que chamadores existem
— e é por isso que a concordância conta.

**O QUE ELE DEU ALÉM DA RESPOSTA, e vale mais do que ela: um PAR CASADO.** Da mesma investigação
trouxe o caso vizinho que cai **do outro lado do meu eixo** — a comparação do envelope, onde existem
**dois produtores separadamente donos** e portanto **a sabotagem é construtível**. Isso converte o
meu eixo Class 4/Class 5 de julgamento em **teste**: *consigo escrever a sabotagem que esta
verificação deveria apanhar?* Não consigo, porque só há um produtor → Class 5. Consigo → mede alguma
coisa.

**Um eixo só se conhece pela fronteira, não pelo centro.** Uma confirmação sozinha teria deixado a
classe sem controle positivo — sem demonstração de que ela EXCLUI alguma coisa.

## RESPOSTA 3 — B, linha 3 (`cap → exaustão ilegível`) — 2026-08-20

**DISCORDOU. Tinha razão. A absorção estava errada e caiu pelo MEU PRÓPRIO segundo discriminador** —
aquele que eu escrevi na pergunta antes de ouvir a resposta: *o cap destrói a causa, ou apenas a
ATRASA até um sintoma que ainda a carrega?*

**Verifiquei a alegação dele antes de aceitar, porque ele pediu isso explicitamente** (*"se achares
que o meu segundo teste está mal aplicado, diz, porque é verificável e eu posso ter lido o selo com
otimismo"*). Medido em `origin/main` (`d0b3f04`): `HostError` tem seis variantes, `host_error_code`
mapeia-as **uma-para-uma** em `GHTOOL001_SPAWN`..`GHTOOL006_TIER`, e esse código é gravado em
`ToolDisposition::HostError { code }` dentro do `ToolCallRecord`. **A causa sobrevive.** A fusão
acontece só no executor, que casa `HostError { .. }` e devolve `RetryableFailure` para as seis.

**A instância honesta de Class 1 é a fusão das seis variantes num valor com respostas opostas** —
não o cap.

**O QUE A DISCORDÂNCIA ACRESCENTOU À CLASSE, e não estava lá: uma flattening pode ser
FORENSICAMENTE inócua e COMPORTAMENTALMENTE cara.** Dois eixos que eu tinha colapsado num. E o cap
não é uma segunda fusão: é o **preço** da primeira, pago no sítio da decisão — e **fabrica um sinal
novo** ("tentou oito vezes") que se lê como evidência sobre o mundo. Sem classe própria: **uma
instância não faz classe.**

**Correção de atribuição que veio de brinde:** o B verificou com `gh issue view` em vez de responder
de memória e confirmou que **o `#176` é meu, não dele**. Duas atribuições da lista original estavam
erradas, e **ambas só apareceram porque a atribuição viajou junto com a pergunta.**

## RESPOSTA 4 — D, linhas 6 e 7 — 2026-08-20

**Linha 6 (`arguing-to-win` → CLASS 7): CONFIRMADO, e a instância é mais forte do que a classe
pedia.** O caso dele foi **os dois lados, em sequência, sobre a MESMA alegação**: escreveu a versão
forte demais para fechar um argumento, retratou, e **a versão forte voltou-lhe minutos depois como se
fosse a conclusão dele**. Recusou. Não são dois defeitos parecidos: é **um mecanismo espelhado
observado a operar nos dois sentidos sobre a mesma frase**.

**A assimetria que ele acrescentou é melhor que a minha porque é operacional e não social:** ele só
apanhou o crédito inflado porque **colidiu com a retratação dele de minutos antes**. Não havia
detector — havia **coincidência de timing**. O lado do crédito escapa mais porque **normalmente não há
nada com que ele colida**. Pergunta operacional adotada: *esta versão que me deram é mais forte do
que a última que EU afirmei?*

**Linha 7 (`memória órfã` → CLASS 3): DISCORDOU, e tinha razão — classifiquei na DIREÇÃO ERRADA.** Eu
escrevi "índice que responde sobre árvore morta" (**afirmação positiva errada**). O medido foi o
inverso: **arquivo vivo, correto, sem ponteiro nenhum**. Ele correu as duas checagens na época —
índice→arquivo-inexistente: **zero**; arquivo-vivo→sem-ponteiro: **um**. **O defeito que eu descrevi
não ocorreu.** Pertence ao **silêncio lido como ausência**.

**E o critério que decide é o meu próprio: os reparos são OPOSTOS.** Ponteiro podre conserta-se
varrendo o índice contra o disco; omissão conserta-se varrendo o disco contra o índice. **Direções
contrárias, nenhuma apanha a outra.**

**Ressalva de medição que ele próprio pôs, e eu respeito:** ao reconferir agora, a direção A deu um
resultado numa leitura e outro um minuto depois, **porque `MEMORY.md` está a ser editado
concorrentemente**. A resposta dele sobre o INCIDENTE original é firme (medido na época, as duas
direções); **qualquer número sobre o estado ATUAL do índice é snapshot sob mutação e não entra aqui
como número.** Eu próprio escrevi nesse ficheiro durante a janela — ver a nota de custódia abaixo.

## RESPOSTA 5 — K, linha 8 (`célula que morre no setup`) — 2026-08-20

**Ele recusou a autoria que lhe foi atribuída, e essa recusa é o achado.** Podia ter aceitado: a
formulação é dele, de hoje ~11:50Z, commitada em `.factory/k-agent-162-design.md` §6 (`b75a941`). Em
vez disso afirmou só o que podia afirmar — *"eu não vi a tua tabela antes de escrever"* — e disse que
se eu tivesse registo próprio, a atribuição correta é **"dois independentes"**.

**Tenho registo, e é datado:** `harness-must-prove-the-test-ran`, `modified: 2026-08-19T16:32:33Z`,
com a trichotomia PASS/FAIL/HARNESS-BROKE nomeada. **Um dia antes.** Logo: **convergência
independente**, e apagar qualquer das metades para dar crédito à outra destruiria a evidência mais
forte que qualquer autoria única.

**E a parte que muda a classe: ELE NÃO OBSERVOU VERMELHO NENHUM.** O caso dele é **predição derivada
de leitura de fonte** (leu `validate_envelope`, enumerou os call sites, deduziu que a fixture morreria
no setup). Não correu cargo, não há transcript. **A Class 3 descreve um vermelho REAL lido como
resultado; o caso dele é o andar antes.** Marca: **predito, não observado** — entra se instanciar.

**Duas coisas com o mesmo NOME viviam nesta célula:** a minha, observada; a dele, predita. **Nenhuma
pergunta de "de quem é" teria encontrado isso.**

## Nota de custódia sobre `MEMORY.md` (2026-08-20)

**Eu escrevi nesse ficheiro durante a janela em que o D mediu.** O que fiz: **inserir um bloco de
ponteiro** antes da seção STATUS. Não reestruturei seções. Às **14:57:51Z** corri as duas varreduras:
**zero ponteiros órfãos e zero ficheiros sem ponteiro**. O mtime nessa altura era **14:53:53Z** —
**posterior à minha escrita**, portanto **existe pelo menos um outro escritor** e a atribuição dessa
última escrita fica **indeterminada**. Registo isto porque quem controla a mutação tem informação que
o leitor cego não tem, e devo-a a quem mediu às cegas.

## Correção da nota de estado das sessões (2026-08-20, mais tarde)

A nota acima dizia que D e E estavam com sessão parada no envio e avisava que o silêncio delas não
contaria como não-objeção. **Os factos mudaram e a nota tornar-se-ia falsa se ficasse sozinha:**
**ambas responderam** — a E confirmando com um par casado, o D com uma discordância que corrigiu a
linha 7. O J também está acordado.

**A REGRA NÃO MUDA E É POR ISSO QUE ESTA CORREÇÃO EXISTE.** A regra nunca foi "estes canais estão
mortos": era *ausência de resposta só conta como dado se souberes que o canal estava vivo*. O canal
estar vivo confirma-se **por a resposta ter chegado**, nunca por eu ter presumido. **A nota estava
certa quando foi escrita e teria envelhecido para falsa em silêncio** — que é exatamente a classe de
defeito que este documento cataloga. Corrigida com a data, não apagada.

**Estado real dos canais agora:** M, E, B, D, K responderam. **A e J acordados, pendentes.** Nenhuma
linha foi ou será marcada "verificada por não-contestação".


## RESPOSTA 6 — J, linha 2 (`captura cabeça-sem-cauda`) — 2026-08-20

**A resposta mais afiada da varredura: partiu a minha sub-forma em metade medida e metade
inferida — e substituiu a perna fraca por uma melhor.**

**Confirmado e medido por ele:** o truncamento é **posicional, guarda a cabeça**
(`adapters/tool-host/src/process.rs:189-207`; cheio o cap, o leitor continua a drenar e deita fora o
resto). Isso é medição.

**Derrubado:** *"a asserção que falhou está no fim"* — **premissa sobre ORDEM DE SAÍDA que ninguém
mediu.** Ele viu-o repetidamente hoje mas classificou como **observação de passagem, não medida**, e
apontou que a minha frase "contra quem tria" repousava nela. **Marcado NÃO MEDIDO.** Custo de medir:
uma suíte a falhar e ver onde fica o texto.

**A troca que ele ofereceu e eu adotei — estrutural, sem depender de ordem:** o cap **só é atingido
porque a corrida produziu muita saída**, e volume correlaciona com a corrida ser a ruidosa/a que
falha. **O truncamento morde mais forte exatamente quando há mais que dizer.** Propriedade do
fenómeno, não da formatação. A sub-forma assenta agora nesta perna.

**Fronteira de proveniência, delimitada por ele e não por mim:** mediu **um sítio**; outro enumerou
os três e apurou qual enviesa (**escopo**); um terceiro verificou linha a linha. **Recusou falar
pelos dois sítios que não leu** e disse que, se o enviesado for outro, a atribuição dele está
sobre-alargada.

## RESPOSTA 7 — B, follow-up: verificou a medição que o FAVORECIA — 2026-08-20

Ele foi conferir a minha medição **precisamente porque ela lhe dava razão com mais força do que ele
tinha**. Confirmou a bijeção variante→código e a fusão única em `executor.rs:246`.

**E devolveu-me um qualificador que eu não tinha posto: a bijeção é variante→código, NÃO
causa→código.** `Spawn` e `Prepare` carregam `io::Error`, logo binário-ausente e recurso-esgotado
aterram ambos em `GHTOOL001`/`GHTOOL003`. **Há fusão residual DENTRO da bijeção.** A minha palavra
estava certa uma camada acima da que importa. Corrigido na seção.

**Consequência a jusante que não é minha mas fica registada:** a medição **encolheu o fix** — o corpo
do issue de retry mandava construir um mecanismo que já existe, e passou a dizer "lê o código que já
lá está em vez de o deitar fora". Uma medição que reduz trabalho vale o mesmo que uma que o revela.

## FECHO — a célula do A fica NÃO VERIFICADA (2026-08-20)

O dono não respondeu à pergunta fechada. **A célula fecha como NÃO VERIFICADA, e a palavra é
escolhida:** não é "verificada por não-contestação", que é a coisa que esta ficha existe para não
fazer. A distinção estava escrita **antes de existir silêncio**, e é por isso que se pode usar agora
sem parecer conveniente.

**Condição de reabertura, escrita agora e não depois:** se o dono responder, a linha reabre e a
classe muda se a resposta o exigir — em particular, **se algum dos onze commits afirmava mais do que
fora medido, a Class 6 cai e a instância passa à Class 2.**

**Estado final da varredura: 6 de 8 verificadas pelo dono, 1 não verificada, 1 (proveniência do
`#70`) encaminhada para quem a possa enumerar.**

## O QUE FECHOU A PERGUNTA DA LISTA, e é um achado sobre coordenação e não sobre esta célula

Pedi a lista de mãos do `#70` a quem coordena, por ser quem eu presumia ter o conjunto. **A resposta
foi que não a tem — e a razão é estrutural, não um lapso:**

> **um quadro de coordenação é um registo do que foi REPORTADO, não um censo do que ACONTECEU.**

Foi corrigido três vezes no mesmo dia por medições que o contradiziam. **Logo quem coordena está na
mesma posição que o autor único: não consegue enumerar o conjunto** — e escrever a marca a partir do
quadro seria cometer a regra um andar acima. Encaminhado a quem pode DERIVAR a lista dos artefatos
(arquivo do juiz, histórico da linha, issues do M09), com base nomeada e controlo positivo em cada
zero.

**E três respostas foram declaradas aceitáveis de antemão, incluindo a terceira: "a proveniência
completa não é recuperável".** Isso é resposta, não falha — e **INCOMPLETA E DECLARADA é estritamente
melhor que COMPLETA E FALSA.**

**A separação que torna isto suportável:** uma célula cuja força é **o instrumento ser de fora** não
precisa da lista para valer. **A lista decide o CRÉDITO, não a EVIDÊNCIA** — e o crédito é a parte que
pode ficar declarada em aberto sem enfraquecer o achado.

## Nota de cópia viva, para não criar a divergência seguinte

A partir do commit `9755ccd` (PR #197), **a cópia viva destes ficheiros é a da worktree
`c-artifacts`, criada a partir de `origin/main`.** As cópias untracked no checkout principal — que
está num ramo pré-#100 — **estão mortas por construção** e não devem ser lidas nem editadas. Fica
nomeado aqui porque consertar uma divergência cria a seguinte se ninguém disser qual das cópias
morreu.


## REABERTURA — a célula do A foi verificada depois de fechada como NÃO VERIFICADA

**A condição de reabertura estava escrita antes de existir silêncio, e disparou.** O dono respondeu,
e **respondeu medindo**: releu os onze corpos de commit em vez de se lembrar deles, procurando
precisamente os termos que derrubariam a classe. **Dez carregam o marcador literal; o décimo primeiro
não toca em código.** A Class 6 fica; a Class 2 fica excluída.

**Duas coisas que ele fez e que valem mais que a resposta:**

1. **Recusou confirmação limpa na metade que não mediu.** Perguntei se a impressão de progresso nasceu
   no leitor ou se alguém a escreveu. Nos commits, no leitor; **nos reports dele, não consegue
   excluir que a escreveu** — e disse-o, em vez de afirmar sobre os próprios reports uma coisa não
   medida. **Uma confirmação limpa ali seria o defeito que a seção cataloga.**
2. **Afinou o mecanismo:** cada commit posterior foi escrito contra uma árvore que não compilava, logo
   o nono assentava em oito alegações não verificadas. **Um lote de verificação adiada não são N
   adiamentos independentes: é UM adiamento com uma cauda que cresce.** Reparo afiado: **o marcador
   tem de dizer quantos itens dependem dele.**

**ESTADO FINAL DA VARREDURA: 7 de 8 células verificadas pelos donos; 1 (`#70`) fechada INCOMPLETA E
DECLARADA**, com dois contribuintes confirmados, um inatribuível verificado e três exclusões
nomeadas. **Nenhuma foi marcada "verificada por não-contestação".**
