# Verified Official References

**Verification date:** 2026-08-08

Provider integrations and legal strategy are areas subject to change. Revalidate official documents before implementation, release, or commercial communication.

## OpenAI / Codex

### Use with a ChatGPT account

The official documentation states that Codex can be accessed through clients such as Codex CLI and that login can be done with a ChatGPT account; limits vary by plan.

- OpenAI Help Center — "Using Codex with your ChatGPT plan"  
  https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan

### Codex CLI login

The official documentation describes the `codex --login` flow / Sign in with ChatGPT and local storage of client credentials.

- OpenAI Help Center — "Codex CLI and Sign in with ChatGPT"  
  https://help.openai.com/en/articles/11381614-api-codex-cli-and-sign-in-with-chatgpt

### Implication for the project

- integrate only officially supported client/SDK/flow;
- do not automate the web chat;
- do not import cookies;
- treat limits and terms as updatable metadata;
- separate subscription usage from BYOK API usage.

## Anthropic / Claude Code

### Pro and Max

The official documentation states that Claude Code can be authenticated with a Claude account associated with the Pro or Max plans, while API Console usage is separate.

- Anthropic Help Center — "Use Claude Code with your Pro or Max plan"  
  https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan

- Anthropic Docs — "Set up Claude Code"  
  https://docs.anthropic.com/en/docs/claude-code/getting-started

### Implication for the project

- use Claude Code's official login;
- maintain a specific adapter for the native runtime;
- do not represent a Pro/Max subscription as a generic API key;
- treat quotas as observed capacity;
- separate credentials from the execution sandbox.

## OpenRouter

The official documentation describes Bearer API keys for the main endpoints and compatibility with OpenAI API formats on relevant endpoints.

- OpenRouter Developer Documentation — FAQ/authentication  
  https://openrouter.ai/docs/faq

### Implication for the project

- OpenRouter is an aggregator/BYOK-type route;
- billing and rate limits belong to the adapter;
- each model's capabilities need to be discovered/registered;
- do not use web cookies for the API.

## MIT License

GraphHelm is distributed under the MIT license: use, modification, redistribution and sale are
permitted, including running a modified version as a network service, with no obligation to
return anything. The only condition is that the copyright and permission notice travel with
the software.

- License text (OSI)  
  https://opensource.org/license/mit

## Contributor agreements

Not used. A contribution is offered under the same MIT terms the project ships under, so there
is nothing left for an ICLA or CCLA to grant. Those instruments exist to let one party
relicense contributed code commercially, and that need disappeared with the commercial tier.

## Legal note

The product decision is MIT, single license, no CLA. The license text itself is standard and needs no drafting; the trademark policy and any terms for optional hosted services still require a qualified professional.
