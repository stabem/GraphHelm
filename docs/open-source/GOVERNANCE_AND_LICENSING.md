# Governança open source, licenciamento e contribuições

## 1. Objetivo

Construir GraphHelm como framework aberto, confiável e adotável, preservando ao mesmo tempo a possibilidade de licenciamento comercial alternativo sobre a mesma base de código.

Este documento é uma estratégia de produto e governança, não aconselhamento jurídico. Os textos finais de licença comercial, ICLA, CCLA, marca e termos precisam de advogado especializado.

## 2. Constituição open source

1. Framework, Runtime e Studio têm código-fonte público.
2. Nenhuma função essencial depende de cloud do mantenedor.
3. Tudo que o Studio faz usa API pública.
4. Self-host completo é suportado.
5. Telemetria externa é opt-in.
6. Protocolos e schemas são públicos.
7. Extensões podem ser instaladas fora de registry oficial.
8. Exportação não contém lock-in intencional.
9. Serviços comerciais são conveniência, suporte ou gestão, não desbloqueio do núcleo.
10. Roadmap, RFCs, ADRs e security policy são públicos.

## 3. Estratégia de licença dupla

### 3.1 Edição comunitária

**GNU Affero General Public License v3 (AGPLv3).**

Motivação:

- o produto é naturalmente executado por rede;
- modificações usadas para oferecer serviço em rede devem disponibilizar source correspondente aos usuários, conforme os termos da licença;
- protege a reciprocidade do núcleo.

### 3.2 Licença comercial

Contrato alternativo para organizações que desejam incorporar, modificar ou oferecer o software sem as obrigações da AGPLv3.

Possíveis ofertas:

- licença comercial self-hosted;
- suporte e SLA;
- deployment gerenciado;
- observabilidade hospedada;
- colaboração/SSO/compliance gerenciados;
- marketplace e billing;
- consultoria e implantação;
- indemnification contratual;
- long-term support.

A licença comercial não deve depender de manter recursos essenciais fora do repositório comunitário.

### 3.3 Mesma base de código

Objetivo: evitar “open core” em que o produto real fica fechado. Add-ons de serviço podem existir, mas o framework agentivo integral permanece aberto.

## 4. Contributor License Agreement

### 4.1 Modelo

CLA não exclusivo:

- contribuidor mantém copyright;
- concede direitos amplos, permanentes e irrevogáveis necessários para usar, modificar, distribuir, sublicenciar e relicenciar a contribuição;
- inclui patent grant relacionado;
- permite distribuição AGPL e comercial;
- mantém atribuição/autoria;
- declara direito de contribuir;
- separa ICLA e CCLA.

### 4.2 ICLA

Para pessoas físicas. Deve cobrir contribuições próprias e declarações sobre empregador quando aplicável.

### 4.3 CCLA

Para empresas que possuem contribuições de funcionários. Não substitui automaticamente declaração individual; o fluxo jurídico definitivo decide a combinação.

### 4.4 UX do CLA

- texto curto e legível;
- página pública explicando por que existe;
- assinatura eletrônica no primeiro PR relevante;
- status automatizado no CI;
- privacidade de dados do signatário;
- processo para correções;
- versão do CLA registrada.

### 4.5 Pequenas contribuições

A política jurídica deve definir se typo/docs pequenas exigem CLA. Para simplicidade de relicenciamento, a recomendação é exigir CLA para todo PR mergeado, com automação de baixo atrito.

## 5. Copyright e ownership

Para dual licensing sustentável, o mantenedor/entidade precisa controlar direitos suficientes sobre todo código distribuído comercialmente. Código sem CLA compatível não entra na base dual-license ou exige consentimento separado.

Third-party code deve ter license compatibility review. Dependências AGPL/GPL podem afetar distribuição comercial e precisam de análise específica.

## 6. Marca

Licença de código não concede automaticamente direito de usar nome/logo como produto oficial. Criar Trademark Policy separada:

- uso nominativo permitido;
- forks podem dizer “compatível com”;
- não podem se passar por release oficial;
- programa de parceiros/certificação opcional;
- proteção contra malware usando marca.

“GraphHelm” é o nome selecionado para desenvolvimento; o lançamento público depende de busca jurídica, reserva de namespaces e política de marca.

## 7. Estrutura de governança

### 7.1 Fase inicial

- Founder/Maintainer principal;
- Core Maintainers;
- Module Maintainers;
- Security Team;
- Release Managers;
- Community Moderators.

### 7.2 Evolução

- Technical Steering Committee;
- RFC process;
- transparent voting/consensus;
- conflict of interest policy;
- maintainer succession;
- independent foundation possível após maturidade.

## 8. RFC process

Obrigatório para:

- novo protocol/node type;
- breaking schema change;
- security boundary;
- license/governance change;
- public API major change;
- new trust model;
- critical dependency;
- hosted service coupling;
- data collection change.

Template:

```text
Summary
Motivation
Goals / non-goals
Detailed design
Security/privacy
Compatibility
Alternatives
Migration
Test strategy
Operational impact
Open-source impact
Reference implementation plan
```

States:

- proposed;
- discussion;
- accepted;
- rejected;
- withdrawn;
- implemented;
- superseded.

## 9. ADRs

ADRs registram decisões específicas da implementação de referência. Não substituem RFCs públicas para mudanças de ecossistema.

Formato:

- context;
- decision;
- alternatives;
- consequences;
- status;
- supersedes.

## 10. Versionamento e releases

- SemVer para packages e APIs;
- release train previsível;
- alpha/beta/RC;
- signed tags/artifacts;
- SBOM;
- provenance attestations;
- changelog;
- migration guides;
- LTS comercial/comunitário conforme capacidade;
- compatibility matrix.

## 11. Repositório

Estrutura sugerida:

```text
/apps/studio
/apps/runtime
/crates-or-packages/core
/packages/graph-dsl
/packages/sdk-typescript
/packages/sdk-python
/packages/schemas
/extensions/builtin
/docs
/rfcs
/adrs
/conformance
/examples
```

A linguagem concreta pode variar, mas boundaries precisam permanecer.

## 12. Contribution workflow

1. issue/discussion para mudanças grandes;
2. RFC quando necessário;
3. fork/branch;
4. CLA check;
5. tests/conformance;
6. security/license scans;
7. review por code owners;
8. public CI;
9. merge com changelog;
10. release notes.

## 13. Code review

Exigir:

- functional correctness;
- contracts/schemas;
- security boundaries;
- backward compatibility;
- observability;
- docs;
- tests;
- performance;
- license provenance.

Mudanças no core security/policy/credential broker requerem reviewer especializado e, idealmente, dois approvals.

## 14. Conformance program

Publicar suite para validar:

- Runtime compatibility;
- Studio/API compatibility;
- Graph DSL;
- plugins;
- model adapters;
- sandbox adapters;
- event semantics;
- export/replay.

Selo “GraphHelm Compatible” depende de trademark policy e testes públicos.

## 15. Security governance

- `SECURITY.md`;
- private disclosure channel;
- response targets;
- CVE process;
- supported versions;
- embargo/coordinated disclosure;
- security advisories;
- dependency alerts;
- incident postmortems redigidos quando seguro;
- bug bounty futuro.

## 16. Community norms

- Code of Conduct;
- technical disagreement by evidence;
- no hidden roadmap promises;
- transparent moderation;
- contributor recognition;
- public meeting notes;
- avoid maintainer capture by vendor.

## 17. Telemetria e dados comunitários

Default:

- local metrics on;
- external telemetry off.

Opt-in dataset pode coletar somente metadata claramente descrita, com anonymization e delete mechanism. Nunca incluir prompts/code/artifacts sem opt-in específico separado.

## 18. Marketplace/registry governance

Registry oficial pode moderar malware, trademark abuse e broken packages. Porém:

- protocol é aberto;
- alternate registries são permitidos;
- local install é permitido;
- removal/revocation tem reason público quando possível;
- paid package terms são visíveis;
- security metadata não pode ser escondida por pagamento.

## 19. Monetização compatível

- managed cloud;
- one-click VPS;
- enterprise support;
- SSO/RBAC service;
- hosted observability;
- curated verified registry;
- commercial license;
- compliance packs;
- training/certification;
- consulting;
- custom adapters.

A community edition continua plenamente funcional self-hosted.

## 20. Riscos

### CLA reduz contribuições

Mitigação: texto curto, explicação honesta, assinatura simples, governança transparente.

### AGPL afasta empresas

Mitigação: licença comercial clara e avaliação simples.

### Fork hostil

Mitigação: qualidade, comunidade, marca, release velocity e open governance; não fechamento do core.

### Contributor rights ambiguity

Mitigação: ICLA/CCLA, provenance e license scans.

### Dependência incompatível

Mitigação: automated license policy e legal review de critical dependencies.

## 21. Documentos jurídicos necessários antes do lançamento

- AGPLv3 LICENSE;
- commercial license agreement;
- ICLA;
- CCLA;
- CLA privacy notice;
- trademark policy;
- terms/privacy para serviços opcionais;
- DPA para hosting empresarial;
- export controls review, se aplicável;
- contributor guide;
- third-party notices.

## 22. Fontes de referência

- GNU AGPLv3 e explicação da cláusula de rede: Free Software Foundation.
- Modelo de ICLA/CCLA e explicação: Apache Software Foundation.

Links e data de verificação estão em `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md`.
