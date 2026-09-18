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

## TypeSafe AI (System One models)

GraphHelm was accepted into TypeSafe early access on 2026-09-16. TypeSafe's flagship model,
Jev, returns typed judgments with calibrated probabilities rather than generated text.

- Product page: https://typesafe.ai/
- Live documentation index (source of truth, read per task): https://docs.typesafe.ai/llms.txt
- Primitives: `Choice`, `Noul`, `Score` — https://docs.typesafe.ai/primitives.md
- Confidence guidance: https://docs.typesafe.ai/confidence.md
- HTTP API: https://docs.typesafe.ai/api.md
- Skill (MIT): https://github.com/typesafe-ai/skills/blob/main/skills/typesafe-ai/SKILL.md

Published pricing at acceptance: "$42 per billion input tokens" (verify on the product page
before relying on it).

### Installing the skill

Claude Code (also enabled at project scope by `.claude/settings.json` in this repository):

```bash
claude plugin marketplace add typesafe-ai/skills
claude plugin install typesafe@typesafe-ai
```

Any other agent host:

```bash
npx skills add typesafe-ai/skills --skill typesafe-ai
```

Use one installation method. The skill is an advisory skill under D-041: it calls nothing in
the Runtime and grants no authority; it tells an agent how to shape a judgment question.

### Implication for the project

- TypeSafe is a direct API / BYOK-type route (§2.2) whose output contract is the System One
  judgment family described in `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6;
- the API key belongs to the user and lives in the Credential Broker; it never appears in Graph
  DSL, capsules, artifacts, fixtures or logs;
- a judgment is a typed signal on the classify/propose side; deterministic policy still decides;
- the route is chosen explicitly, never as an automatic paid fallback;
- the Runtime adapter is `adapters/model-gateway/src/systemone.rs` (`SystemOneAdapter`, judge
  door only). Wiring it is one command in a project `graphhelm init` provisioned (#1139), the
  key pasted at a hidden prompt or piped on stdin, never an argument and never in a tracked file:

  ```sh
  graphhelm gateway setup --provider typesafe
  ```

  It writes this route into `.graphhelm/manifest.json` (or merges it into the manifest already
  there), stores the key in the Credential Broker under `secret_typesafe` usable by `judge`
  alone, probes the route, and prints the `graph synthesize --judge-route judge` command:

  ```json
  { "id": "judge", "provider": "typesafe", "transport": "direct_api", "authentication": "api_key",
    "billingMode": "per_token", "baseUrl": "https://api.typesafe.ai", "model": "jev-latest",
    "credentialRef": "secret_typesafe", "profiles": ["balanced_reasoning"], "enabled": true }
  ```

- the thresholds the architect acts on are named constants in
  `core/architect/src/judgment/policy.rs`, chosen conservatively and unmeasured; they move only
  with a recorded run under `docs/acceptance/architect-judgments-recipe.md`;
- `.claude/settings.json` pins the `typesafe-ai/skills` marketplace to the tag `v0.5.7` (the
  version reviewed on 2026-09-16: six files, no hooks, no scripts). Claude Code's marketplace
  source accepts `ref` (branch or tag) and not a commit sha, so a tag is the tightest pin the
  format admits; moving it is a deliberate edit to that file, never a silent upstream change.

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
