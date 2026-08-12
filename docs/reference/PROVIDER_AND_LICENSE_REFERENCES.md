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

## GNU AGPLv3

The Free Software Foundation publishes the official text of the GNU Affero General Public License v3 and explains the additional requirement related to modified versions used by users over a network.

- License text  
  https://www.gnu.org/licenses/agpl-3.0.html.en

- "Why the GNU Affero GPL" explanation  
  https://www.gnu.org/licenses/why-affero-gpl.en.html

### Implication for the project

- include the correct, full license text;
- implement an adequate mechanism for offering the source when applicable;
- validate the interaction between AGPL, dependencies, and the commercial license with a lawyer.

## Contributor License Agreements

The Apache Software Foundation documents the ICLA and CCLA as agreements under which contributors retain rights over their contributions and grant the foundation rights to distribute and develop the work.

- ASF Contributor Agreements  
  https://www.apache.org/licenses/contributor-agreements.html

- ASF CLA FAQ  
  https://www.apache.org/licenses/cla-faq.html

### Implication for the project

- use these materials only as a structural reference;
- draft our own ICLA/CCLA appropriate to the dual licensing model;
- validate patent grant, relicensing, and privacy;
- do not copy/adapt without legal review.

## Legal note

This specification does not claim that any CLA text or commercial license is already finalized. The product decision is to use AGPLv3 + an alternative commercial license + a non-exclusive CLA. Legal execution depends on our own documents, reviewed by a qualified professional.
