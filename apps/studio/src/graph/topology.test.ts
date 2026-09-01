import { describe, expect, it } from "vitest";

import type { GraphTopology, RuntimeEvent } from "../runtime/types";
import { buildGraphModel } from "./model";
import { recordedGraphHash, topologyNote, verifyTopology } from "./topology";

const RUN_HASH = "sha256:aa9b0715df457c2a1a364c1ab5ddee2c88bc390742c078fe6725b4b823e8a9bb";
const OTHER_HASH = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

function event(sequence: number, kind: string, payload: unknown): RuntimeEvent {
  return {
    sequence,
    kind,
    payload,
    occurredAt: "2026-08-27T12:00:00Z",
    actorId: "system-cli",
    actorType: "system",
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: [],
  };
}

const STARTED = event(1, "execution_started", {
  executionId: "demo",
  graphHash: RUN_HASH,
  graphVersion: 13,
  mode: "supervised",
});

const ROSTER = event(2, "execution_form_declared", {
  executionId: "demo",
  nodeIds: ["deploy", "implementation"],
});

function topology(hash: string): GraphTopology {
  return {
    graphId: "exec_override_graph_v13",
    graphVersion: 13,
    executionId: "exec_override",
    semanticHash: hash,
    entrypoints: ["implementation"],
    nodes: [
      { id: "implementation" },
      { id: "deploy" },
    ],
    edges: [
      { id: "implementation_direct_to_deploy", from: "implementation", to: "deploy", type: "data" },
    ],
  };
}

describe("the hash the run recorded", () => {
  it("comes from execution_started and nowhere else", () => {
    expect(recordedGraphHash([STARTED, ROSTER])).toBe(RUN_HASH);
  });

  /** A page that has not read back to sequence 1 has no hash to compare against. That is its own
   * state, not a licence to skip the check. */
  it("is null when the page has not read the event that carries it", () => {
    expect(recordedGraphHash([ROSTER])).toBeNull();
  });

  /** Only `execution_started` is consulted. A later event that happened to carry a `graphHash`
   * field would be a different claim about a different thing. */
  it("ignores a graphHash on any other event kind", () => {
    const impostor = event(9, "node_outcome_recorded", { nodeId: "deploy", graphHash: OTHER_HASH });
    expect(recordedGraphHash([impostor])).toBeNull();
  });
});

describe("verifying a graph file against a run", () => {
  it("matches when the file hashes to what the run recorded, and yields its edges", () => {
    const verified = verifyTopology([STARTED, ROSTER], topology(RUN_HASH), "graphs/deploy.yaml");
    expect(verified.match).toBe("matched");
    expect(verified.edges).toHaveLength(1);
    expect(verified.entrypoints).toEqual(["implementation"]);
    expect(topologyNote(verified)).toMatch(/verified/i);
  });

  /**
   * THE ONE THAT MATTERS. A different graph must yield NO edges — not a warning beside them, not
   * a dimmed rendering. The empty list is the enforcement, so a caller that forgets to branch on
   * `match` still cannot draw the wrong graph.
   */
  it("yields no edges at all when the file is a different graph", () => {
    const verified = verifyTopology([STARTED, ROSTER], topology(OTHER_HASH), "graphs/other.yaml");
    expect(verified.match).toBe("mismatched");
    expect(verified.edges).toEqual([]);
    expect(verified.entrypoints).toEqual([]);
    expect(topologyNote(verified)).toMatch(/not the graph this run started from/i);
  });

  it("yields no edges when the run has not reported its hash yet", () => {
    const verified = verifyTopology([ROSTER], topology(RUN_HASH), "graphs/deploy.yaml");
    expect(verified.match).toBe("unverified");
    expect(verified.edges).toEqual([]);
    expect(topologyNote(verified)).toMatch(/could not be checked/i);
  });

  /** Equality on the whole value. A prefix comparison would say yes to a graph that merely starts
   * the same way, which is exactly the mistake a truncated hash invites. */
  it("does not accept a hash that merely starts the same way", () => {
    const truncated = topology(RUN_HASH.slice(0, 20));
    expect(verifyTopology([STARTED], truncated, "graphs/deploy.yaml").match).toBe("mismatched");
  });

  it("drops a malformed edge rather than drawing an arrow with one end", () => {
    const broken = topology(RUN_HASH);
    broken.edges = [
      { id: "good", from: "implementation", to: "deploy", type: "data" },
      { id: "bad", from: "implementation", to: "", type: "data" },
    ] as GraphTopology["edges"];
    expect(verifyTopology([STARTED], broken, "f").edges.map((edge) => edge.id)).toEqual(["good"]);
  });
});

describe("the model only takes edges through that door", () => {
  it("reports edgesKnown false and no edges without a verified topology", () => {
    const model = buildGraphModel([STARTED, ROSTER]);
    expect(model.edgesKnown).toBe(false);
    expect(model.edges).toEqual([]);
  });

  it("reports edgesKnown false and no edges when the topology mismatched", () => {
    const verified = verifyTopology([STARTED, ROSTER], topology(OTHER_HASH), "f");
    const model = buildGraphModel([STARTED, ROSTER], verified);
    expect(model.edgesKnown).toBe(false);
    expect(model.edges).toEqual([]);
  });

  it("carries the edges once the topology matched", () => {
    const verified = verifyTopology([STARTED, ROSTER], topology(RUN_HASH), "f");
    const model = buildGraphModel([STARTED, ROSTER], verified);
    expect(model.edgesKnown).toBe(true);
    expect(model.edges.map((edge) => `${edge.from}->${edge.to}`)).toEqual([
      "implementation->deploy",
    ]);
    expect(model.entrypoints).toEqual(["implementation"]);
  });

  /**
   * The run's own roster wins over the file's. An edge to a node this execution never declared
   * cannot be drawn — there is no card to draw it to — and a half-drawn arrow into empty space is
   * worse than none.
   */
  it("drops an edge whose endpoint this run never declared", () => {
    const extra = topology(RUN_HASH);
    extra.edges = [
      { id: "known", from: "implementation", to: "deploy", type: "data" },
      { id: "stranger", from: "deploy", to: "announce", type: "data" },
    ];
    const model = buildGraphModel([STARTED, ROSTER], verifyTopology([STARTED, ROSTER], extra, "f"));
    expect(model.edges.map((edge) => edge.id)).toEqual(["known"]);
    expect(model.nodes.map((node) => node.id).sort()).toEqual(["deploy", "implementation"]);
  });

  /**
   * With the shape proven, the default layout follows the WORK rather than the alphabet.
   * `deploy` sorts first and runs second; laying the board out alphabetically drew every arrow
   * doubling back on itself. Ranked by depth from the entrypoints, then by id so two nodes at the
   * same depth still land in the same places on every read.
   */
  it("lays the board out along the graph once the edges are proven", () => {
    const verified = verifyTopology([STARTED, ROSTER], topology(RUN_HASH), "f");
    const proven = buildGraphModel([STARTED, ROSTER], verified);
    expect(proven.nodes.map((node) => node.id)).toEqual(["implementation", "deploy"]);

    // Without a proof there is no graph to follow, so the id is the only stable key left.
    const unproven = buildGraphModel([STARTED, ROSTER]);
    expect(unproven.nodes.map((node) => node.id)).toEqual(["deploy", "implementation"]);
  });

  /** A cycle must not hang the walk, and a node no entrypoint reaches must still get a place. */
  it("places an unreachable node and survives a cycle", () => {
    const cyclic = topology(RUN_HASH);
    cyclic.edges = [
      { id: "a", from: "implementation", to: "deploy", type: "data" },
      { id: "b", from: "deploy", to: "implementation", type: "control" },
    ];
    const model = buildGraphModel(
      [STARTED, event(3, "node_outcome_recorded", { nodeId: "orphan", nextState: "ready" }), ROSTER],
      verifyTopology([STARTED, ROSTER], cyclic, "f"),
    );
    expect(model.nodes.map((node) => node.id)).toEqual(["implementation", "deploy", "orphan"]);
  });
});
