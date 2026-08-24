# FROZEN — critérios para a review do blueprint da #221, antes de abrir os comentários do autor

**Derivado de:** o corpo da issue `#221`; a secção *Task 005* do plano em `f0640cd` (linhas 253+);
`origin/main`. **Não abri os comentários `5392769204` nem `5392787280`.**

## Divulgações recebidas — o que me foi dado antes de eu ler, marcado como tal

**Regras da wave (idênticas para os sete):** ED-18; skills data-only; Governor-only; manifesto
append-only (`#216`); bytes-não-digests (`#217`).

**E DUAS DECISÕES DO COORDENADOR que o documento JÁ INCORPORA — marcadas como decisões RECEBIDAS,
não como achados do autor:** **T9 = recusa-inteira**, e **relógio-injetado (`#218`)**. Se aparecerem
no blueprint, **não contam como mérito de desenho dele nem como omissão minha se eu não as apontar**;
foram-lhe entregues.

**Coordenada, dada como PERGUNTA e não como resposta:** *onde é que uma RECUSA pode ser traduzida em
algo que se LÊ como sucesso?* **Foi-me dito explicitamente que não sabem se o blueprint a trata.**

## A minha derivação da coordenada — seis superfícies, da issue apenas

**L1 — `Nothing now` é o canal de lavagem, e é o mais perigoso.** O critério diz que um resultado
sem-decisão emite zero opções e diz `Nothing now`. **Isso é também o que um erro engolido produziria**,
se algum caminho de falha alcançar o ramo sem-decisão. **Duas causas, uma saída legal, respostas
OPOSTAS do dono** — a classe do achatamento. *"Não há ação"* e *"não foi possível determinar se há
ação"* têm de ser **distinguíveis nos bytes**; se não forem, o dono lê calma onde havia falha.

**L2 — o fallback determinístico.** *"Planos maliciosos/malformados são descartados; o render seguro
tem sucesso deterministicamente."* **Se o render de fallback for indistinguível do normal, um plano
RECUSADO lê-se como apresentação bem-sucedida.** *"Rejeitámos um ataque"* e *"correu tudo bem"* não
podem ser os mesmos bytes.

**L3 — a fronteira slot/prosa.** Os slots são copiados exatos; a frase à volta é estilável. **Um
estilista que não consegue mudar o slot ainda consegue mudar a frase que o contém** — `refused: X`
dentro de *"já tratámos do X por si"*. **A asserção tem de ser sobre o TODO renderizado, não sobre
igualdade de slots.**

**L4 — o serializador exclusivo.** *"exclusive final byte serialization"* — **se qualquer outro
caminho emitir bytes para o dono, o validador é uma porta com segunda entrada.** É a classe da
composição: o guard protege a função, não a porta.

**L5 — compressão do token de recusa.** *"não pode ser comprimido nem reescrito como sucesso"* exige
uma asserção de **PRESENÇA nos bytes de saída**, não *"a validação passou"*. Um validador estrutural
aceita uma apresentação de que o texto da recusa foi retirado.

**L6 — a recusa apresentada como OPÇÃO.** *"Decisões reais contêm exatamente duas opções verdadeiras"*.
**Uma recusa pode ser lavada aparecendo como uma das duas** — *"Opção A: prosseguir; Opção B: não
conseguimos"* — o que se lê como **escolha** e não como **falha**. A cardinalidade fica satisfeita e a
verdade não.

## Predições seladas — morrem se o documento as tratar

1. **`Nothing now` alcançável a partir de um caminho de erro não é tratado.**
2. **A indistinguibilidade do fallback não é abordada** — o descarte do estilista não aparece na saída.
3. **Os guards afirmam sobre o RESULTADO da validação e não sobre os BYTES da saída.**
4. **O serializador exclusivo é afirmado e não guardado** — nenhum teste em que um segundo emissor
   falhe.
5. **A recusa-como-opção não é considerada.**

**Assinado antes de abrir os dois comentários.** O valor da review são os DELTAS — e se as cinco
caírem a favor do autor, publico isso primeiro.
