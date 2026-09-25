/**
 * The graph a new task starts as: one node, and the operator's own sentence as its objective.
 *
 * WHY THE STUDIO COMPOSES THIS AT ALL. Agents cannot alter a graph's topology - they emit a
 * `graph_signal` and only the Graph Governor publishes (FR-021, D-018). So "the chat builds the
 * rest of the graph" cannot mean the chat writing nodes. What it CAN mean, and what this is, is
 * the operator authoring the first node themselves: the run starts with one agent node whose
 * objective is what they typed, and everything after it goes through the Governor like any other
 * proposal.
 *
 * ONE NODE, NOT A TEMPLATE OF SIX. A guessed shape is a shape the operator has to undo, and this
 * surface has nothing to guess from - the message has not been read by anything yet. The honest
 * starting point is the smallest graph that can legally run.
 */

/** The node every draft starts with. Named here rather than inlined so the board, the panel and
 * the composed document cannot drift on what "the start node" is called. */
export const DRAFT_NODE_ID = "start";

/** Bounds the objective before it becomes part of a document the Runtime parses. The Runtime
 * bounds the whole document at 4 MiB; this is the smaller, human bound - a task objective that
 * runs past it is a plan, and belongs in the graph the first node produces. */
export const MAX_OBJECTIVE_LENGTH = 2000;

export interface DraftTask {
  /** The execution id this draft will run under, fixed when the draft is created so the board and
   * the eventual run agree on it. */
  executionId: string;
  /** The route id the operator picked, or `null` while they have not. `null` means "the server's
   * own default" - not "no model". */
  route: string | null;
}

/**
 * A URL-safe, collision-resistant execution id.
 *
 * `crypto.randomUUID` rather than a counter or a timestamp: two tabs open on the same Runtime must
 * not compose the same id, and a timestamp collides exactly when two people start work at once.
 *
 * `run-`, NOT `task-`, and the word matters: the event store refuses to persist secret-shaped
 * strings, its prefix table includes `sk-` + a 20-character token (an OpenAI key's shape), and
 * `task-` CONTAINS `sk-` - so with a UUID tail, every id the first version minted was refused as
 * `GHE009_EXTERNALIZATION_FAILED` (measured live 2026-08-30; `desk-` fails the same way). The
 * tail is hex and dashes, which no entry of that table can form; the prefix is the half that can
 * regress, and `draft.test.ts` pins it against the store's own table.
 */
export function newExecutionId(): string {
  // The same compatibility rule as `newIdempotencyKey` (runtime/client.ts): some browsers on
  // plain http expose `getRandomValues` but not `randomUUID`, and a surface whose ONLY way to
  // start a task throws synchronously there is a dead composer (PR #467 review). The tail stays
  // hex-and-dashes either way, which the store's secret-shape table cannot form.
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return `run-${uuid}`;
  const bytes = new Uint8Array(16);
  globalThis.crypto?.getRandomValues?.(bytes);
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `run-${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/**
 * The one-node graph document, ready to be posted inline.
 *
 * Every field here is required by the graph schema; none of it is decoration. The shape follows
 * the repository's own examples rather than being invented - a document this surface composes and
 * the Runtime then refuses would put the operator in front of a schema error they did not write
 * and cannot act on.
 */
export function draftGraph(executionId: string, objective: string): Record<string, unknown> {
  const trimmed = objective.trim();
  if (trimmed.length === 0) {
    throw new Error("a task needs an objective");
  }
  if (trimmed.length > MAX_OBJECTIVE_LENGTH) {
    throw new Error(`an objective must be at most ${MAX_OBJECTIVE_LENGTH} characters`);
  }
  return {
    apiVersion: "p50.dev/graph/v1",
    kind: "ExecutionGraph",
    metadata: {
      id: `${executionId}_v1`,
      name: "New task",
      executionId,
      version: 1,
    },
    spec: {
      entrypoints: [DRAFT_NODE_ID],
      nodes: {
        [DRAFT_NODE_ID]: {
          type: "agent",
          name: "Start",
          // The operator's sentence, verbatim. This is the whole point of the surface: the
          // objective IS the message, not a summary of it and not a prompt wrapped around it.
          objective: trimmed,
          optionality: "required",
          agent: {
            ephemeral: {
              purpose: "Open the task the operator described.",
              capabilities: ["change.plan"],
              inputSchema: "schema://TaskRequest@1",
              outputSchema: "schema://ImplementationPlan@1",
              instructions:
                "Read the objective, then propose how the work should be broken down. Emit a graph signal for any topology you believe is needed; you may not publish one yourself.",
              completionContract: { requires: ["acceptance_criteria"] },
              isolationMinimum: "tier_0",
            },
          },
          completion: { requires: [{ outputSchemaValid: true }] },
        },
      },
      edges: [],
      budgets: {
        maxNodes: 5,
        maxDepth: 2,
        // A POSITIVE mutation budget, because the node's instructions promise one: "emit a graph
        // signal for any topology you believe is needed" against maxMutations: 0 made the
        // Governor refuse every proposal with LimitExceeded, locking each Studio task to its
        // start node forever (PR #467 review). Four mutations pace the four nodes the maxNodes
        // budget leaves beyond the start.
        maxMutations: 4,
        maxRetriesPerNode: 1,
        maxWallClockSeconds: 600,
        maxApiCostUsd: 1,
        maxParallelModelCalls: 1,
      },
      completion: {
        terminalNodes: [DRAFT_NODE_ID],
        allowWaivers: false,
      },
    },
  };
}
