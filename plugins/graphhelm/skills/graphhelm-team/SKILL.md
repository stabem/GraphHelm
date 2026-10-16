---
name: graphhelm-team
description: Use when an orchestrator session puts another agent to work on a GraphHelm project (an author, a reviewer, any lane), so the agent is born inside the Studio, registered under its own lane name and visible on the Team tab from its first step.
---

# GraphHelm team

Spawn one agent (a **lane**) so the owner sees it in the Studio from birth to merge. The
orchestrator calls this once per agent. The orchestrator itself never takes a lane's work.

## Before the first agent

1. The Studio is open for the project (`graphhelm studio start` from the project root; see
   `graphhelm-setup`). Read the team execution and Runtime URL from the GraphHelm MCP `briefing` tool
   (or `graphhelm briefing --json`). Default execution `gh-team`, URL `http://127.0.0.1:8793`.
2. The recorder is `tools/task-record/task_record.py` in the GraphHelm clone that
   `graphhelm studio start` keeps up to date. It needs the agent session token
   `<project>/.graphhelm/events.agent.token`; never the owner's `events.token`.
3. One scoped agent credential per lane: a fresh 64-hex secret bound to the lane name in
   `GRAPHHELM_AGENT_CREDENTIALS` (`<secret>=<lane>|project-local|<exec>`, `;`-separated) in the
   Runtime's environment when it starts. It is the only way a lane records a journey draft or
   approves one as a reviewer (`POST /v1/journey-flows/<id>/draft` and `.../review`). Hand each
   lane only its own secret, in a file, never in a prompt or a log. When a journey needs the
   owner's approval, run `graphhelm studio open --journey <contractId> --no-browser`, open the
   link it prints in the Claude desktop app's browser pane, and hand the owner the Approve click;
   never press it for them.
4. The owner's roster: which model for each role and how many agents at once (the kickoff skill
   asks it once). Map each model to what the host can start: in Claude Code the subagent's
   `model` (for example `opus`, `sonnet`, `haiku`); for Runtime-driven nodes the gateway route of
   that tier (the GraphHelm MCP `routes` tool; tier-to-route binding is ADR-041). Never start more
   agents at once than the roster says.

## Spawn one lane

1. **Name it.** `<role>-<n>` (`author-1`, `review-2`), unique in the execution. The name is the
   lane, the `X-GraphHelm-Actor`, the `--reviewer`/`--author` id and the identity line.
2. **Register it before it works.** Mint the lane's credential (once per lane and execution; the
   Runtime refuses a `task.*` record under the shared agent session token, #1148), then record the
   claim and the plan as that lane, so the Team tab shows it at once:

   ```sh
   graphhelm tasks credential --lane <lane> --execution <exec> --token-file <project>/.graphhelm/events.token
   python <clone>/tools/task-record/task_record.py --lane <lane> --execution <exec> --repo <owner/name> --token-file <project>/.graphhelm/events.agent.token claimed --issue <N> --branch issue-<N>-<desc> --journeys <contractId> --assigned-by <orchestrator lane>
   python <clone>/tools/task-record/task_record.py --lane <lane> --execution <exec> --repo <owner/name> --token-file <project>/.graphhelm/events.agent.token planned --issue <N> --summary "<plan in one line>"
   ```

   `task_record.py` reads `<project>/.graphhelm/events.agent.<lane>.token` beside the
   `--token-file` it is given; the orchestrator records as its own lane with `--assigned-by`.

3. **Start it** with the host's subagent tool and the roster's model for its role. Its brief
   carries: its lane name, the issue, the project path and branch, the Runtime URL, execution
   and token path, the skill it runs (`implement`, `blind-review`, `journey-map` ...), the project's
   `KEEL.md` and the Keel file for its step (`keel/KEEL_PLAN.md`, `KEEL_EXEC.md`, `KEEL_REVIEW.md` or
   `KEEL_MERGE.md`) when the project has them, and the
   rule that it records its own steps with `task_record.py --lane <lane>` (`pr_opened` after
   every push, `review_verdict`, `merged`). A reviewer's brief follows `blind-review`: never the
   author's reasoning or an expected verdict. With the GraphHelm hooks installed,
   `SubagentStart`/`SubagentStop` also link the child to this session in the Studio.
4. **Reuse before you spawn.** When a lane of the same role already finished a task on the same
   scope, hand it the next one instead of a new agent (agent reuse, ADR-041/042). An author lane
   is never reused as the reviewer of its own work.
5. **Watch through the Studio, not the chat.** Read progress with the GraphHelm MCP `tasks` tool (every lane's step,
   PR, head and heartbeat), `status` and `events`; never script the CLI or HTTP for the same answer. A lane silent past its heartbeat is shown stale there; re-brief or release it
   (`task_record.py ... released --claim-sequence <seq> --release-reason "<why>"`).

## Never

- Open a visible console window per command. On Windows run children hidden
  (`CREATE_NO_WINDOW`) or in the one terminal the session already has (#943).
- Let a lane record as another lane, or review or approve what it wrote.
- Report a lane's work as done before its `merged` record exists.
