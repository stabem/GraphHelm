import { describe, expect, it, vi } from "vitest";

import { RuntimeClient, RuntimeError } from "./client";
import type { ClaimEvidence } from "./types";

/**
 * #1187 review: the pure verdict readers in `customs.test.ts` never observe the WIRE.
 *
 * They take a `MutationEvidence` and read it, which is the right grain for what they do and
 * leaves the whole request unmeasured: the path, the method, the body's field names, and whether
 * a sequence of ZERO survives the journey. Those are the parts a Runtime refuses on, and a
 * refusal there looks nothing like a wrong verdict here — so this file drives the real client
 * against a fetch that records what it was asked to send.
 */

const STATUS = {
  ok: true,
  data: {
    executionId: "exec-1",
    status: "running",
    headSequence: 7,
    attention: "needs_you",
    attentionReasons: [],
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    nodeStateCounts: {},
    nodeStates: { implementation: "waiting_input" },
    startedAt: null,
    lastEventAt: null,
    nodeLastEventAt: {},
    customs: { nodes: {}, clearances: {}, quarantinedNodes: [] },
  },
};

interface Sent {
  url: string;
  method: string;
  body: Record<string, unknown> | null;
  headers: Record<string, string>;
}

/** A Runtime that answers every read and records every write. */
function recordingClient() {
  const sent: Sent[] = [];
  const fetchImpl = vi.fn(async (input: unknown, init?: RequestInit) => {
    const url = String(input);
    const method = (init?.method ?? "GET").toUpperCase();
    let body: Record<string, unknown> | null = null;
    if (typeof init?.body === "string" && init.body.length > 0) {
      body = JSON.parse(init.body) as Record<string, unknown>;
    }
    sent.push({
      url,
      method,
      body,
      headers: (init?.headers ?? {}) as Record<string, string>,
    });
    const json = (value: unknown) =>
      new Response(JSON.stringify(value), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    if (url.includes("/events")) return json({ ok: true, data: { events: [], nextAfter: null } });
    if (method === "POST") return json({ ok: true, data: {} });
    return json(STATUS);
  });
  const client = new RuntimeClient("token", {
    baseUrl: "http://runtime.test",
    fetch: fetchImpl as never,
  });
  return { client, sent, fetchImpl };
}

const GOOD_HASH = `sha256:${"a".repeat(64)}`;
const evidence = (kind = "test_report"): ClaimEvidence[] => [
  { kind, contentHash: GOOD_HASH, size: 12 },
];

function writeOf(sent: Sent[], fragment: string): Sent {
  const write = sent.find((row) => row.method === "POST" && row.url.includes(fragment));
  if (write === undefined) {
    throw new Error(`no POST to ${fragment}; saw ${sent.map((r) => `${r.method} ${r.url}`).join(", ")}`);
  }
  return write;
}

describe("claimNode on the wire", () => {
  it("posts the graph, the node, the wait sequence and the bundle to the claim route", async () => {
    const { client, sent } = recordingClient();
    await client.claimNode("exec-1", {
      file: "examples/graphs/customs-acting.yaml",
      node: "implementation",
      waitSeq: 9,
      evidence: evidence(),
    });
    const write = writeOf(sent, "/claim");
    expect(write.url).toBe("http://runtime.test/v1/executions/exec-1/claim");
    expect(write.body).toEqual({
      file: "examples/graphs/customs-acting.yaml",
      node: "implementation",
      waitSeq: 9,
      evidence: [{ kind: "test_report", contentHash: GOOD_HASH, size: 12 }],
    });
  });

  // ZERO IS A SEQUENCE, and this is the cell that says so on the wire. A guard written as
  // `if (waitSeq)` drops it, the field never reaches the Runtime, and the claim then answers
  // whichever wait happens to be open — the stale rendezvous the field exists to prevent, arriving
  // as a silent omission rather than an error.
  it("sends a wait sequence of zero rather than omitting it", async () => {
    const { client, sent } = recordingClient();
    await client.claimNode("exec-1", {
      file: "g.yaml",
      node: "implementation",
      waitSeq: 0,
      evidence: evidence(),
    });
    const write = writeOf(sent, "/claim");
    expect(write.body).toHaveProperty("waitSeq", 0);
  });

  // An EMPTY bundle is legal and must travel as an empty array rather than as an absent field:
  // a node declaring no proof kinds is answerable, and "no evidence" and "no evidence field" are
  // different requests.
  it("sends an empty bundle as an empty array", async () => {
    const { client, sent } = recordingClient();
    await client.claimNode("exec-1", { file: "g.yaml", node: "implementation", evidence: [] });
    const write = writeOf(sent, "/claim");
    expect(write.body).toHaveProperty("evidence", []);
    expect(write.body).not.toHaveProperty("waitSeq");
  });
});

describe("clearClaim on the wire", () => {
  // THE EVIDENCE TRAVELS, NOT A HASH THIS CLIENT COMPUTED. The Runtime re-derives the digest and
  // compares it against what the claim journaled; sending a hash from here would move that
  // comparison to the side that wants it to pass.
  it("posts the claim sequence and the bundle, and no locally computed digest", async () => {
    const { client, sent } = recordingClient();
    await client.clearClaim("exec-1", {
      file: "g.yaml",
      claimSeq: 11,
      evidence: evidence(),
      node: "implementation",
    });
    const write = writeOf(sent, "/clear");
    expect(write.url).toBe("http://runtime.test/v1/executions/exec-1/clear");
    expect(write.body).toEqual({
      file: "g.yaml",
      claimSeq: 11,
      evidence: [{ kind: "test_report", contentHash: GOOD_HASH, size: 12 }],
    });
    expect(write.body).not.toHaveProperty("manifestHash");
  });

  it("sends a claim sequence of zero rather than omitting it", async () => {
    const { client, sent } = recordingClient();
    await client.clearClaim("exec-1", { file: "g.yaml", claimSeq: 0, evidence: evidence() });
    expect(writeOf(sent, "/clear").body).toHaveProperty("claimSeq", 0);
  });
});

describe("the customs bounds refuse locally, with zero requests", () => {
  // Every one of these was unmeasured: the bounds exist in client.ts and no cell reached them,
  // so raising MAX_EVIDENCE_ITEMS to a million or gutting the hash pattern left the suite green.
  const cases: Array<[string, (client: RuntimeClient) => Promise<unknown>]> = [
    [
      "a bundle past the item bound",
      (client) =>
        client.claimNode("exec-1", {
          file: "g.yaml",
          node: "n",
          evidence: Array.from({ length: 33 }, () => evidence()[0]),
        }),
    ],
    [
      "a content hash that is not sha256 hex",
      (client) =>
        client.claimNode("exec-1", {
          file: "g.yaml",
          node: "n",
          evidence: [{ kind: "test_report", contentHash: "sha256:nothex", size: 1 }],
        }),
    ],
    [
      "an uppercase content hash, because the wire form is lowercase",
      (client) =>
        client.claimNode("exec-1", {
          file: "g.yaml",
          node: "n",
          evidence: [{ kind: "test_report", contentHash: `sha256:${"A".repeat(64)}`, size: 1 }],
        }),
    ],
    [
      "a negative size",
      (client) =>
        client.claimNode("exec-1", {
          file: "g.yaml",
          node: "n",
          evidence: [{ kind: "test_report", contentHash: GOOD_HASH, size: -1 }],
        }),
    ],
    [
      "an empty proof kind",
      (client) =>
        client.claimNode("exec-1", {
          file: "g.yaml",
          node: "n",
          evidence: [{ kind: "", contentHash: GOOD_HASH, size: 1 }],
        }),
    ],
    ["an empty graph path", (client) => client.claimNode("exec-1", { file: "", node: "n" })],
    [
      "a negative wait sequence",
      (client) => client.claimNode("exec-1", { file: "g.yaml", node: "n", waitSeq: -1 }),
    ],
    [
      "a fractional claim sequence",
      (client) => client.clearClaim("exec-1", { file: "g.yaml", claimSeq: 1.5, evidence: [] }),
    ],
  ];

  for (const [name, call] of cases) {
    it(`refuses ${name}`, async () => {
      const { client, fetchImpl } = recordingClient();
      await expect(call(client)).rejects.toBeInstanceOf(RuntimeError);
      expect(fetchImpl).not.toHaveBeenCalled();
    });
  }

  // THE CONTROL. Without it every cell above is satisfied by a client that refuses everything.
  it("accepts the shapes just inside every bound and goes to the Runtime", async () => {
    const { client, fetchImpl } = recordingClient();
    await client.claimNode("exec-1", {
      file: "g.yaml",
      node: "n",
      waitSeq: 0,
      evidence: Array.from({ length: 32 }, () => evidence()[0]),
    });
    expect(fetchImpl).toHaveBeenCalled();
  });
});
