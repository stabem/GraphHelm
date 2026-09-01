/**
 * Tying a graph document to a run — the check that has to happen before a single arrow is drawn.
 *
 * THE PROBLEM. The Runtime's log records which graph an execution started from as a HASH
 * (`execution_started.graphHash`) and never records its shape. `POST /v1/graph/topology` reads a
 * FILE. A file path is a guess: the operator types it, the file may have been edited since the
 * run began, or it may be a different graph entirely. Drawing its edges over a run's nodes on
 * that basis would produce a confident, plausible picture of the wrong graph — and a drawn arrow
 * reads as evidence, which is exactly the failure this Studio refuses everywhere else.
 *
 * THE CHECK. The topology reply carries the same `semanticHash` the run recorded. Equal, and the
 * edges provably belong to this run. Not equal, and they provably do not. No hash in the log
 * yet — a page that has not read back to sequence 1 — and the answer is neither, which is its own
 * state and not a reason to draw.
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
    return "Connections verified: this file hashes to exactly the graph this run recorded.";
  }
  if (verified.match === "mismatched") {
    return "This file is not the graph this run started from: its hash differs from the one the log recorded.";
  }
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
