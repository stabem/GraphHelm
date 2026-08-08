# Referências oficiais verificadas

**Data da verificação:** 2026-08-08

As integrações de providers e a estratégia jurídica são áreas sujeitas a mudanças. Revalidar os documentos oficiais antes de implementação, release ou comunicação comercial.

## OpenAI / Codex

### Uso com conta ChatGPT

A documentação oficial informa que o Codex pode ser acessado por clientes como Codex CLI e que o login pode ser feito com a conta ChatGPT; limites variam por plano.

- OpenAI Help Center — “Using Codex with your ChatGPT plan”  
  https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan

### Codex CLI login

A documentação oficial descreve o fluxo `codex --login` / Sign in with ChatGPT e armazenamento local de credenciais do cliente.

- OpenAI Help Center — “Codex CLI and Sign in with ChatGPT”  
  https://help.openai.com/en/articles/11381614-api-codex-cli-and-sign-in-with-chatgpt

### Consequência para o projeto

- integrar somente cliente/SDK/fluxo oficialmente suportado;
- não automatizar chat web;
- não importar cookies;
- tratar limites e termos como metadata atualizável;
- separar uso de assinatura de API BYOK.

## Anthropic / Claude Code

### Pro e Max

A documentação oficial informa que Claude Code pode ser autenticado com conta Claude associada aos planos Pro ou Max, enquanto uso de API Console é separado.

- Anthropic Help Center — “Use Claude Code with your Pro or Max plan”  
  https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan

- Anthropic Docs — “Set up Claude Code”  
  https://docs.anthropic.com/en/docs/claude-code/getting-started

### Consequência para o projeto

- usar login oficial do Claude Code;
- manter adapter específico para native runtime;
- não representar assinatura Pro/Max como API key genérica;
- tratar quotas como capacidade observada;
- separar credentials do execution sandbox.

## OpenRouter

A documentação oficial descreve API keys Bearer para endpoints principais e compatibilidade com formatos de API OpenAI em endpoints relevantes.

- OpenRouter Developer Documentation — FAQ/authentication  
  https://openrouter.ai/docs/faq

### Consequência para o projeto

- OpenRouter é route do tipo aggregator/BYOK;
- billing e rate limits pertencem ao adapter;
- capabilities de cada model precisam ser descobertas/registradas;
- não usar cookie web para API.

## GNU AGPLv3

A Free Software Foundation publica o texto oficial da GNU Affero General Public License v3 e explica o requisito adicional relacionado a versões modificadas usadas por usuários através de rede.

- Texto da licença  
  https://www.gnu.org/licenses/agpl-3.0.html.en

- Explicação “Why the GNU Affero GPL”  
  https://www.gnu.org/licenses/why-affero-gpl.en.html

### Consequência para o projeto

- incluir texto integral correto da licença;
- implementar mecanismo adequado de oferta do source quando aplicável;
- validar interação entre AGPL, dependencies e licença comercial com advogado.

## Contributor License Agreements

A Apache Software Foundation documenta ICLA e CCLA como acordos em que contribuidores preservam direitos sobre contribuições e concedem direitos para a fundação distribuir e desenvolver o trabalho.

- ASF Contributor Agreements  
  https://www.apache.org/licenses/contributor-agreements.html

- ASF CLA FAQ  
  https://www.apache.org/licenses/cla-faq.html

### Consequência para o projeto

- usar esses materiais apenas como referência de estrutura;
- redigir ICLA/CCLA próprios adequados ao dual licensing;
- validar patent grant, relicensing e privacidade;
- não copiar/adaptar sem revisão jurídica.

## Nota jurídica

Esta especificação não afirma que qualquer texto de CLA ou licença comercial já esteja pronto. A decisão de produto é usar AGPLv3 + licença comercial alternativa + CLA não exclusivo. A execução jurídica depende de documentos próprios revisados por profissional qualificado.
