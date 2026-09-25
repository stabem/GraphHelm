---
name: graphhelm-resume
description: Use when the user asks what to do next, wants to resume GraphHelm work, or invokes graphhelm-resume to choose between concrete next actions.
---

# GraphHelm resume

Give the owner **exactly two** distinct, concrete next actions, with **one marked recommended**. This skill is read-only: selecting an option does not execute it, alter a graph, install anything, or change host settings.

1. Establish the latest goal and state from the conversation and the smallest relevant live evidence: task/issue, branch and PR head, execution status, failed proof, or release status. Check current sources when available; mark stale or missing evidence explicitly. A historical green result does not prove the current head.
2. Identify the nearest unresolved user promise. Prefer an action that removes its blocker or obtains the missing observer. Choose a second viable action with a real tradeoff, such as useful independent work while a blocker remains. Do not invent a task, cost saving, pass, merge, or deployment.
3. State status in one short sentence, then present exactly two numbered options. Each option names the action and its expected result. Mark one `Recommended` and give a brief reason. Keep the owner's language and use small words.

If there is not enough evidence to name two implementation tasks, make the options two **discovery** actions: one to inspect the current work record, the other to ask the owner for a goal. Never fill the gap with guesses. If work is already complete, say so before proposing two genuinely new next actions. Do not add a third option or silently proceed with either action.
