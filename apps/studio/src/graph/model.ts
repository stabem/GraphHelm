/**
 * The execution, folded into something a board can draw.
 *
 * EVERYTHING HERE COMES OUT OF THE EVENT LOG. The node roster is what
 * `execution_form_declared` declared; each node's current state is the `nextState` of the last
 * `node_outcome_recorded` that named it; each node's history is the events that named it, in
 * order. No shape is invented and no field is guessed.
 *
 * EDGES ARRIVE ONLY WITH A PROOF. The stream itself never carries the topology - it records the
 * graph's HASH and nothing about its shape. `POST /v1/graph/topology` reads a FILE, and a file is
 * a guess about which graph ran. So edges enter this model through exactly one door: a
 * `VerifiedTopology` whose hash equals the one the run recorded (`graph/topology.ts`). Anything
 * else - no file read, a mismatched hash, a run that has not reported its hash yet - leaves
 * `edges` empty and `edgesKnown` false, and the board says so on its face. A drawn arrow reads as
 * evidence, so an unproven one is the single worst thing this surface could render.
 */

import type { RuntimeEvent } from "../runtime/types";
import type { VerifiedTopology } from "./topology";

/** The sixteen lifecycle states, plus `unknown` for a node that has been declared and has not
 * yet had an outcome recorded. `unknown` is a REAL answer here, not a placeholder: a node the
 * scheduler has not reached has no state to report, and inventing `ready` would claim it is
 * about to run. */
export type NodeStateName = string;

export interface NodeHistoryEntry {
  sequence: number;
  kind: string;
  /** The state this event moved the node into, when it names one. */
  nextState: NodeStateName | null;
  /** What the executor or the owner reported, when the event names it. */
  outcome: string | null;
  occurredAt: string | null;
  actorId: string | null;
  actorType: string | null;
  /** How many sealed evidence refs the event carried. A settling with zero is a lint finding:
   * "done" is a claim, and D-036 says claims travel with their evidence. */
  evidence: number;
}

/**
 * A disagreement the model itself can attest, shown on the board's face rather than buried.
 *
 * Every finding cites the log (`sequence`) when the log holds the accused line - the reference
 * project prints its plan lint on the board and that is worth copying, but OURS accuses events,
 * never a plan file, because the log is the only truth this surface has.
 */
/**
 * Whether a finding is one a fixture run EXPLAINS (#1083, Codex on PR #1091).
 *
 * Only `done-without-evidence`: a fixture executor supplies outcomes from a file, so a settled
 * node carrying no evidence is exactly what a demonstration run produces. `reopened-after-done`
 * (a settled node reopened) and `orphan-edge` (a verified graph whose endpoints the roster does not
 * hold) are real disagreements on any run, and must keep the attention treatment.
 */
export function fixtureExplained(finding: LintFinding): boolean {
  return finding.kind === "done-without-evidence";
}

/** A run's lint split into what its executor explains and what still needs attention. On a run
 * that is not a demonstration nothing is explained: every finding is a disagreement. */
export function splitLint(
  lint: readonly LintFinding[],
  demonstration: boolean,
): { expected: LintFinding[]; disagreements: LintFinding[] } {
  if (!demonstration) return { expected: [], disagreements: [...lint] };
  return {
    expected: lint.filter(fixtureExplained),
    disagreements: lint.filter((finding) => !fixtureExplained(finding)),
  };
}

export interface LintFinding {
  kind: "reopened-after-done" | "done-without-evidence" | "orphan-edge";
  /** One readable sentence naming the node or edge and what is wrong. */
  detail: string;
  /** The accused event's sequence, when an event (rather than a file's edge) is accused. */
  sequence: number | null;
}

export interface GraphNode {
  id: string;
  state: NodeStateName;
  /** What kind of attempt evidence the latest successful outcome sealed. This identifies the
   * producer's output, not an acceptance verdict or a named agent. Older streams may have none. */
  resultSource?: "model_reply" | "tool_record" | "judge_verdict" | "gate_verdict" | "other" | "none" | null;
  /** How many times an event has named this node. The board shows it because a node retried
   * eight times and a node run once look identical from their state alone. */
  touches: number;
  lastEventAt: string | null;
  history: NodeHistoryEntry[];
  /** "Quietly reopened the part it already called done" — the disagreement signal, derived from
   * the log alone: the node reached a settled terminal state at `settledAt`, and a LATER event
   * named it at `reopenedAt`, by `by`. Both coordinates cite the log; no watcher, no guess. */
  reopened: { settledAt: number; reopenedAt: number; by: string | null } | null;
}

/** A successful lifecycle transition says the attempt returned; it never proves the
 * agent's stated goal. The source comes from the Runtime's documented attempt-evidence
 * suffixes, and older/unknown evidence stays unknown rather than being assigned a model. */
export function nodeResult(node: GraphNode): { executor: string; verification: string; short: string } | null {
  if (node.state !== "succeeded") return null;
  if (node.resultSource === "model_reply") {
    return {
      executor: "Model call · model identity not recorded",
      verification: "Reply received · acceptance not verified",
      short: "Reply received · unverified",
    };
  }
  if (node.resultSource === "tool_record") {
    return {
      executor: "Tool call · command in evidence",
      verification: "Tool exited successfully · goal not verified",
      short: "Tool exited 0",
    };
  }
  if (node.resultSource === "judge_verdict") {
    return {
      executor: "Judge call · model identity not recorded",
      verification: "Structured judgment passed",
      short: "Judgment passed",
    };
  }
  if (node.resultSource === "gate_verdict") {
    return {
      executor: "Deterministic gate",
      verification: "Gate check passed",
      short: "Gate passed",
    };
  }
  return {
    executor: "Executor not recorded",
    verification: node.resultSource === "none"
      ? "Completion recorded without attempt evidence"
      : "Completion recorded · acceptance not verified",
    short: "Completion · unverified",
  };
}

/** The states after which nothing more is owed on a node. A later event against one of these is
 * a reopening — the one disagreement the append-only log can attest by itself. */
function isSettled(state: string | null): boolean {
  return state === "succeeded" || state === "waived" || state === "skipped";
}

/** The newest history line that settled the node (a hand-rolled findLast: the build's lib
 * predates ES2023). */
function lastSettling(history: NodeHistoryEntry[]): NodeHistoryEntry | undefined {
  for (let index = history.length - 1; index >= 0; index -= 1) {
    if (isSettled(history[index].nextState)) return history[index];
  }
  return undefined;
}

export interface GraphEdge {
  id: string;
  from: string;
  to: string;
  type: string;
}

export interface GraphModel {
  nodes: GraphNode[];
  /** Empty unless a topology was verified against this run's recorded hash. */
  edges: GraphEdge[];
  /** The nodes the graph starts from, when a verified topology named them. */
  entrypoints: string[];
  /** True when the roster came from a declaration rather than from whatever the events happened
   * to mention. A board built from mentions alone is missing every node that has not run yet,
   * and the difference has to be visible. */
  rosterDeclared: boolean;
  /** True only when a verified topology supplied the edges. The board branches on THIS, never on
   * `edges.length` - a graph with one node and no edges is not the same fact as a graph whose
   * shape was never proven, and conflating them is how "we could not check" starts rendering as
   * "there is nothing to show". */
  edgesKnown: boolean;
  /** The disagreements this fold could attest, in node order then edge order. */
  lint: LintFinding[];
}

/** Reads the node id out of an event payload, whatever the event kind. */
function nodeIdOf(event: RuntimeEvent): string | null {
  const payload = event.payload;
  if (payload === null || typeof payload !== "object") return null;
  const id = (payload as { nodeId?: unknown }).nodeId;
  return typeof id === "string" && id.length > 0 ? id : null;
}

function stringField(event: RuntimeEvent, field: string): string | null {
  const payload = event.payload;
  if (payload === null || typeof payload !== "object") return null;
  const value = (payload as Record<string, unknown>)[field];
  return typeof value === "string" && value.length > 0 ? value : null;
}

/**
 * Folds a page of events into the board's model.
 *
 * The page may be PARTIAL - the timeline loads 50 at a time - and the fold is written to survive
 * that: a roster that has not arrived yet leaves `rosterDeclared` false and the nodes are those
 * the page mentions, which is the honest answer to "what can be shown from what has been read".
 */
export function buildGraphModel(
  events: RuntimeEvent[],
  verified?: VerifiedTopology | null,
): GraphModel {
  const nodes = new Map<string, GraphNode>();
  let rosterDeclared = false;

  const ensure = (id: string): GraphNode => {
    const existing = nodes.get(id);
    if (existing) return existing;
    const created: GraphNode = {
      id,
      state: "unknown",
      resultSource: null,
      touches: 0,
      lastEventAt: null,
      history: [],
      reopened: null,
    };
    nodes.set(id, created);
    return created;
  };

  for (const event of events) {
    if (event.kind === "execution_form_declared") {
      const payload = event.payload;
      const declared =
        payload !== null && typeof payload === "object"
          ? (payload as { nodeIds?: unknown }).nodeIds
          : null;
      if (Array.isArray(declared)) {
        rosterDeclared = true;
        for (const id of declared) {
          if (typeof id === "string" && id.length > 0) ensure(id);
        }
      }
      continue;
    }

    const id = nodeIdOf(event);
    if (id === null) continue;
    const node = ensure(id);
    const nextState = stringField(event, "nextState");
    const outcome = stringField(event, "outcome");
    // A settled node being named again IS the signal; the first reopening wins (later ones are
    // the same story continuing), and a fresh settling clears the mark - the record moved on.
    if (isSettled(node.state) && node.reopened === null) {
      const settled = lastSettling(node.history);
      node.reopened = {
        settledAt: settled?.sequence ?? node.history.at(-1)?.sequence ?? event.sequence,
        reopenedAt: event.sequence,
        by: event.actorId,
      };
    } else if (isSettled(nextState)) {
      node.reopened = null;
    }
    node.touches += 1;
    if (nextState !== null) node.state = nextState;
    if (event.kind === "node_outcome_recorded" && nextState === "succeeded") {
      node.resultSource = event.evidenceRefs.some((id) => id.endsWith("-reply"))
        ? "model_reply"
        : event.evidenceRefs.some((id) => id.endsWith("-record"))
          ? "tool_record"
          : event.evidenceRefs.some((id) => id.endsWith("-judgment"))
            ? "judge_verdict"
            : event.evidenceRefs.some((id) => id.endsWith("-verdict"))
              ? "gate_verdict"
              : event.evidenceRefs.length > 0 ? "other" : "none";
    } else if (nextState !== null) {
      node.resultSource = null;
    }
    if (event.occurredAt !== null) node.lastEventAt = event.occurredAt;
    node.history.push({
      sequence: event.sequence,
      kind: event.kind,
      nextState,
      outcome,
      occurredAt: event.occurredAt,
      actorId: event.actorId,
      actorType: event.actorType,
      evidence: event.evidenceRefs.length,
    });
  }

  const proven = verified?.match === "matched";
  // An edge whose endpoints are not both on this board cannot be drawn, and a half-drawn arrow
  // into empty space is worse than none. Dropped rather than clamped: the roster is the run's own
  // account of its nodes, and it wins over a file's.
  const edges = proven
    ? verified.edges.filter((edge) => nodes.has(edge.from) && nodes.has(edge.to))
    : [];

  // THE LINT: disagreements this fold can attest, on the board's face rather than buried in a
  // dropped-silently branch. Node findings first (in id order, matching the layout's stable key),
  // then edge findings - deterministic, so the strip does not reshuffle between reads.
  const lint: LintFinding[] = [];
  for (const node of [...nodes.values()].sort((a, b) => a.id.localeCompare(b.id))) {
    if (node.reopened !== null) {
      lint.push({
        kind: "reopened-after-done",
        detail: `${node.id} was reopened after it settled${node.reopened.by !== null ? ` by ${node.reopened.by}` : ""}`,
        sequence: node.reopened.reopenedAt,
      });
    }
    if (isSettled(node.state)) {
      const settling = lastSettling(node.history);
      if (settling !== undefined && settling.evidence === 0) {
        lint.push({
          kind: "done-without-evidence",
          detail: `${node.id} settled as ${node.state} carrying no evidence`,
          sequence: settling.sequence,
        });
      }
    }
  }
  if (proven) {
    for (const edge of verified.edges) {
      if (nodes.has(edge.from) && nodes.has(edge.to)) continue;
      const missing = !nodes.has(edge.from) ? edge.from : edge.to;
      lint.push({
        kind: "orphan-edge",
        detail: `the graph file draws ${edge.from} → ${edge.to}, but ${missing} is not on this run's roster`,
        sequence: null,
      });
    }
  }

  return {
    // Ordered so the board's layout is a pure function of the data: the same execution draws the
    // same board on every read, and a node does not jump because a page arrived in a different
    // order. WITH proven edges that order follows the graph, so the default layout reads along
    // the work; without them there is nothing to follow and the id is the only stable key.
    nodes: orderForLayout([...nodes.values()], edges, proven ? verified.entrypoints : []),
    edges,
    entrypoints: proven ? verified.entrypoints.filter((id) => nodes.has(id)) : [],
    rosterDeclared,
    edgesKnown: proven,
    lint,
  };
}

/**
 * The order cards are laid out in before anyone moves them.
 *
 * Alphabetical is the right answer with no edges - it is the only stable key the data offers.
 * With PROVEN edges it is the wrong one: `deploy` sorts before `implementation`, so a graph that
 * runs implementation-then-deploy draws right-to-left, and every arrow doubles back. So nodes are
 * ranked by DEPTH from the entrypoints (breadth-first over the edges) and only then by id, which
 * puts the work in reading order and keeps the result deterministic - two nodes at the same depth
 * always land in the same places.
 *
 * A cycle cannot hang this: every node is ranked at most once, and anything the walk never
 * reaches (an unreachable node, or one on a cycle with no path from an entrypoint) keeps its
 * alphabetical place after the ranked ones rather than being dropped.
 */
function orderForLayout(nodes: GraphNode[], edges: GraphEdge[], entrypoints: string[]): GraphNode[] {
  const byId = [...nodes].sort((left, right) => left.id.localeCompare(right.id));
  if (edges.length === 0) return byId;

  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge.to]);
  }

  const depth = new Map<string, number>();
  // Entrypoints when the topology named them; otherwise every node nothing points at, which is
  // the same set for a well-formed graph and a usable fallback for one that named none.
  const targets = new Set(edges.map((edge) => edge.to));
  const roots = entrypoints.length > 0 ? entrypoints : byId.filter((node) => !targets.has(node.id)).map((node) => node.id);

  let frontier = roots.filter((id) => byId.some((node) => node.id === id));
  let level = 0;
  while (frontier.length > 0) {
    const next: string[] = [];
    for (const id of frontier) {
      if (depth.has(id)) continue;
      depth.set(id, level);
      for (const to of outgoing.get(id) ?? []) {
        if (!depth.has(to)) next.push(to);
      }
    }
    frontier = next;
    level += 1;
  }

  return byId.sort((left, right) => {
    const leftDepth = depth.get(left.id) ?? Number.MAX_SAFE_INTEGER;
    const rightDepth = depth.get(right.id) ?? Number.MAX_SAFE_INTEGER;
    return leftDepth === rightDepth ? left.id.localeCompare(right.id) : leftDepth - rightDepth;
  });
}

/** The events that name one node, for its conversation panel. */
export function conversationFor(events: RuntimeEvent[], nodeId: string): RuntimeEvent[] {
  return events.filter((event) => {
    if (nodeIdOf(event) === nodeId) return true;
    const payload = event.payload as { sourceKind?: unknown; sourceId?: unknown } | null;
    return event.kind === "signal_recorded" && payload?.sourceKind === "node" && payload.sourceId === nodeId;
  });
}

/**
 * Which of the three voices an event speaks in.
 *
 * The actor TYPE, not the id: the board colours by role because that is the distinction an
 * operator has to make at a glance - did a person do this, did an agent, or did the runtime.
 */
export function voiceOf(actorType: string | null): "owner" | "agent" | "system" {
  return actorType === "owner" ? "owner" : actorType === "agent" ? "agent" : "system";
}

/** How a node's state reads on the board: is this one waiting on a person, moving, or done. */
export function moodOf(state: NodeStateName): "waiting" | "moving" | "done" | "dead" | "idle" {
  if (state === "blocked" || state === "waiting_input" || state === "waiting_capacity" || state === "ghost") {
    return "waiting";
  }
  if (state === "running" || state === "queued" || state === "linting") return "moving";
  if (state === "succeeded" || state === "waived") return "done";
  // FAILED is an alarm, not an ending: isAlarming says it needs a person, so the card must
  // not wear the finished colour. Cancelled and skipped were decided; they may rest.
  if (state === "failed") return "dead";
  if (state === "cancelled" || state === "invalidated" || state === "skipped") {
    return "done";
  }
  return "idle";
}
