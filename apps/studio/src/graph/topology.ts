/**
 * Tying a graph document to a run — the check that has to happen before a single arrow is drawn.
 *
 * THE PROBLEM. Older Runtime logs recorded only the graph HASH. A graph file path is a guess:
 * the file may have changed since the run began. Newer declarations carry a topology snapshot;
 * both sources need the run's own hash before an arrow can be drawn. A wrong arrow would be a
 * confident picture of the wrong work, exactly the failure this Studio refuses elsewhere.
 *
 * THE CHECK. A file reply carries `semanticHash`; a run snapshot carries `graphHash`. Compare
 * either to `execution_started.graphHash`. A partial page without the start hash draws nothing.
 *
 * Three verdicts, and only ONE of them yields edges. That is the whole module.
 */

import type { GraphTopology, RuntimeEvent } from "../runtime/types";

export type TopologyMatch = "matched" | "mismatched" | "unverified";

export interface VerifiedTopology {
  match: TopologyMatch;
  /** The hash the execution's own log recorded, when the page has read far enough to see it. */
  recordedHash: string | null;
  /** The hash of the file that was read. */
  fileHash: string;
  /** The file path the operator gave, echoed so the page can name it in a refusal. Never read,
   * never opened — it is a label here, exactly as it is on the wire. */
  file: string;
  /** Edges ONLY on a match. `mismatched` and `unverified` carry an empty list by construction,
   * so a component cannot draw them by forgetting to branch on `match`. */
  edges: Array<{ id: string; from: string; to: string; type: string }>;
  entrypoints: string[];
  source?: "journal";
}

/** A declaration and start are appended atomically. Even so, check the recorded hash and
 * roster before treating the declaration as a diagram; old or partial journals have no edges. */
export function topologyFromJournal(events: RuntimeEvent[]): VerifiedTopology | null {
  const started = events.find((event) => event.kind === "execution_started");
  const form = events.find((event) => event.kind === "execution_form_declared");
  if (!started || !form || !form.payload || typeof form.payload !== "object") return null;
  const declaration = form.payload as { executionId?: unknown; nodeIds?: unknown; topology?: unknown };
  // An absent field identifies an older journal. A present but malformed
  // field is a rejected claim and must not enable the manual-file fallback.
  if (!Object.prototype.hasOwnProperty.call(declaration, "topology")) return null;
  if (declaration.topology === null || typeof declaration.topology !== "object") {
    return {
      match: "unverified", recordedHash: recordedGraphHash(events), fileHash: "",
      file: "run journal", edges: [], entrypoints: [], source: "journal",
    };
  }
  const startId = started.payload && typeof started.payload === "object"
    ? (started.payload as { executionId?: unknown }).executionId : null;
  const snapshot = declaration.topology as { graphHash?: unknown; entrypoints?: unknown; edges?: unknown };
  const hash = recordedGraphHash(events);
  const nodes = declaration.nodeIds;
  const nodeIds = Array.isArray(nodes) && nodes.every((id) => typeof id === "string" && id.length > 0)
    ? new Set<string>(nodes) : null;
  const entrypoints = sanitiseIds(snapshot.entrypoints);
  const edges = sanitiseEdges(snapshot.edges as GraphTopology["edges"]);
  const valid = typeof startId === "string" && startId === declaration.executionId
    && typeof snapshot.graphHash === "string" && snapshot.graphHash === hash
    && nodeIds !== null && Array.isArray(nodes) && nodeIds.size === nodes.length
    && Array.isArray(snapshot.entrypoints) && entrypoints.length === snapshot.entrypoints.length
    && new Set(entrypoints).size === entrypoints.length
    && entrypoints.every((id) => nodeIds.has(id))
    && Array.isArray(snapshot.edges) && edges.length === snapshot.edges.length
    && new Set(edges.map((edge) => edge.id)).size === edges.length
    && snapshot.edges.every((edge) => edge !== null && typeof edge === "object"
      && typeof edge.id === "string" && edge.id.length > 0
      && typeof edge.type === "string" && edge.type.length > 0)
    && edges.every((edge) => nodeIds.has(edge.from) && nodeIds.has(edge.to));
  return {
    match: valid ? "matched" : "unverified",
    recordedHash: hash,
    fileHash: typeof snapshot.graphHash === "string" ? snapshot.graphHash : "",
    file: "run journal",
    edges: valid ? edges : [],
    entrypoints: valid ? entrypoints : [],
    source: "journal",
  };
}

/**
 * The hash an execution recorded for its own graph, or `null` when this page has not seen the
 * event that carries it.
 *
 * Reads `execution_started` and nothing else. The hash is written once, at sequence 1, and a
 * later event that happened to carry a `graphHash` field would be a different claim about a
 * different thing.
 */
export function recordedGraphHash(events: RuntimeEvent[]): string | null {
  for (const event of events) {
    if (event.kind !== "execution_started") continue;
    const payload = event.payload;
    if (payload === null || typeof payload !== "object") continue;
    const hash = (payload as { graphHash?: unknown }).graphHash;
    if (typeof hash === "string" && hash.length > 0) return hash;
  }
  return null;
}

/**
 * Decides whether a topology read may be drawn over this run.
 *
 * Comparison is exact string equality on the whole `sha256:...` value. Not a prefix, not a
 * case-fold, not "close enough": two graphs that differ by one edge produce two hashes that
 * differ completely, and any comparison looser than equality is a comparison that can say yes to
 * the wrong graph.
 */
export function verifyTopology(
  events: RuntimeEvent[],
  topology: GraphTopology,
  file: string,
): VerifiedTopology {
  const recordedHash = recordedGraphHash(events);
  const fileHash = typeof topology.semanticHash === "string" ? topology.semanticHash : "";
  const match: TopologyMatch =
    recordedHash === null || fileHash === ""
      ? "unverified"
      : recordedHash === fileHash
        ? "matched"
        : "mismatched";

  return {
    match,
    recordedHash,
    fileHash,
    file,
    // The empty list on anything but a match is the enforcement, not a default: a caller that
    // forgets to check `match` still cannot draw an edge.
    edges: match === "matched" ? sanitiseEdges(topology.edges) : [],
    entrypoints: match === "matched" ? sanitiseIds(topology.entrypoints) : [],
  };
}

/** What the page says about the check, in the operator's terms. */
export function topologyNote(verified: VerifiedTopology | null): string {
  if (verified === null) {
    return "point the field below at the graph file this run started from";
  }
  if (verified.match === "matched") {
    if (verified.source === "journal") return "Connections verified from this run's recorded graph.";
    return "Connections verified: this file hashes to exactly the graph this run recorded.";
  }
  if (verified.match === "mismatched") {
    return "This file is not the graph this run started from: its hash differs from the one the log recorded.";
  }
  if (verified.source === "journal") return "Recorded connections could not be verified against this run.";
  return "This run has not reported its graph hash yet, so the file could not be checked against it.";
}

/** The Runtime is trusted to be well-formed, but this is still cross-process input reaching a
 * renderer: anything not a pair of non-empty ids is dropped rather than drawn as a broken arrow. */
function sanitiseEdges(
  edges: GraphTopology["edges"],
): Array<{ id: string; from: string; to: string; type: string }> {
  if (!Array.isArray(edges)) return [];
  return edges
    .filter(
      (edge) =>
        edge !== null &&
        typeof edge === "object" &&
        typeof edge.from === "string" &&
        edge.from.length > 0 &&
        typeof edge.to === "string" &&
        edge.to.length > 0,
    )
    .map((edge, index) => ({
      id: typeof edge.id === "string" && edge.id.length > 0 ? edge.id : `edge-${index}`,
      from: edge.from,
      to: edge.to,
      type: typeof edge.type === "string" ? edge.type : "unknown",
    }));
}

function sanitiseIds(values: unknown): string[] {
  if (!Array.isArray(values)) return [];
  return values.filter((value): value is string => typeof value === "string" && value.length > 0);
}
