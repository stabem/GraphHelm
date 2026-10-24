---
name: graphhelm-team
description: Use when an orchestrator session puts another agent to work on a GraphHelm project (an author, a reviewer, any lane), with truthful task records under its own lane: admit implementation authors before work and preserve exact-head reviewer assignments.
---

# GraphHelm team

Start or resume one agent (a **lane**) with truthful task evidence in Studio. Bootstrap presence
does not mean task work has started. The orchestrator never takes a lane's identity or work.

## Before the first agent

1. The Studio is open for the project (`graphhelm studio start` from the project root; see
   `graphhelm-setup`). Read the team execution and Runtime URL from the GraphHelm MCP `briefing` tool
   (or `graphhelm execution briefing --events <events dir> --json`). Use the execution shown by
   the briefing or by `<project>/.graphhelm/team.json`; there is no implicit execution fallback.
   The default Runtime URL is `http://127.0.0.1:8793` when no configured or explicit URL is given.
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
   Within the roster, pick the cheapest model and effort that can do each subagent's task, and never
   go above it:
   - Mechanical (rebase plus `plugin_version_guard.py --write`, post-merge cleanup, rerunning a named
     test set, a status check): smallest model, low effort.
   - Bounded implementation, or a blind review that reuses the author's receipt: mid model, low or
     medium effort.
   - Design, security, an unknown-cause bug, or a review of persistence, permissions or
     compatibility: top model, higher effort.
   Resume an existing agent before spawning one. Run a long wait (a reached plan, the slot) as a
   background shell, not inside an agent. Record kind, tier and effort in the delegation record
   ("Delegation choice: implementer · standard · medium").

## Spawn one lane

1. **Name it.** `<role>-<n>` (`author-1`, `review-2`), unique in the execution. The name is the
   lane, the `X-GraphHelm-Actor`, the `--reviewer`/`--author` id and the identity line.
2. **Bootstrap implementation authors without the task payload.** Trusted local setup must already have provisioned
   the lane's credential for this execution. A missing credential is a blocker; neither a cloud
   orchestrator nor a child gains authority to mint it or use the owner's token. Start or resume
   the actual child with only its lane, execution, issue, branch, repository, Runtime binding,
   own credential-file reference and registration instructions. Keep the implementation
   payload until admission. Never record a child claim as the coordinator or sign as the child.
3. **The implementation child claims and proves admission.** It reads MCP `tasks` for the issue, then generates
   its own `task.claimed` envelope with `task_record.py --lane <actual-lane> ... --dry-run`.
   Send that envelope with MCP `signal`, supplying `lane: <actual-lane>` and
   `credentialFile: <that lane's existing credential path>`. Read the task back; an HTTP success,
   a recorded note, membership, or a working report alone is insufficient. Call MCP `task_admit`
   with all six exact fields: `executionId`, `lane`, `issue`, `branch`, `repo`, `claimSequence`.
   Only an `admitted: true` receipt for that binding allows the coordinator to send the task
   payload. Missing credentials, refusals, conflicting holders and uncertain writes stop dispatch;
   resolve the blocker without changing identity or taking another lane's claim.

   The CLI equivalent is read-only:

   ```sh
   graphhelm --json tasks admit --execution <exec> --lane <actual-lane> --issue <N> --branch <branch> --repo <owner/name> --claim-sequence <sequence> --url <Runtime-url> --token-file <existing-read-token-file>
   ```

   Save the admitted claim sequence with the assignment. Admission observes the current claim;
   it does not reserve a host process or prove work started. The child records its accepted plan
   before code, and records its own later steps with `task_record.py --lane <actual-lane>`:
   `pr_opened` after every push. Its task payload carries the
   project's `KEEL.md` and the Keel file for its step, project path, branch and Runtime binding.
   With hooks installed, `SubagentStart`/`SubagentStop` additionally record lifecycle links;
   they do not replace admission. Host APIs called outside this workflow are not intercepted.
4. **Assign reviewers through the existing review flow.** Claim admission above and the
   `run-codex` launcher are for pre-PR implementation authors only. A reviewer never claims the
   author's issue or calls `task_admit` to take its claim. The coordinator retains one actual
   reviewer in `review_assigned` for the current PR and exact head, confirms that record and any
   review of record, then sends that independent lane the `blind-review` payload. Include the PR,
   exact head, card and author receipt; exclude author reasoning and an expected verdict. The
   reviewer records its own `review_verdict` and, if approving, its own `merged` result. A changed
   head requires the review assignment to be refreshed before continuing. Missing reviewer
   credentials block writes without author impersonation. Flow-only reviews retain their own
   scoped flow-author/reviewer contract; pre-PR claim admission does not apply to them.
5. **Reuse before you spawn.** When a lane of the same role already finished a task on the same
   scope, hand it the next one instead of a new agent (agent reuse, ADR-041/042). An author lane
   is never reused as the reviewer of its own work.
6. **Watch through the Studio, not the chat.** Read progress with the GraphHelm MCP `tasks` tool (every lane's step,
   PR, head and heartbeat), `status` and `events`; never script the CLI or HTTP for the same answer. A lane silent past its heartbeat is shown stale there; re-brief or release it
   (`task_record.py ... released --claim-sequence <seq> --release-reason "<why>"`).

## Never

- Open a visible console window per command. On Windows run children hidden
  (`CREATE_NO_WINDOW`) or in the one terminal the session already has (#943).
- Let a lane record as another lane, or review or approve what it wrote.
- Report a lane's work as done before its `merged` record exists.
