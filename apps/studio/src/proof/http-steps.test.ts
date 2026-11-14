import { describe, expect, it } from "vitest";

import type { JourneyFlow, JourneyRun } from "../runtime/client";
import { HTTP_BODY_DRAW_LIMIT, journeyKind, journeySteps } from "./steps";

// #2945: the shape of studio2-auth-refusal: a start screen and one `http` act whose reply is a 403.
const flow: JourneyFlow = {
  id: "studio2-auth-refusal",
  title: null,
  status: "approved",
  screens: [{ id: "no-session", url: "/health" }, { id: "foreign-origin", url: "/health" }],
  edges: [{ id: "no-session.foreign-origin", from: "no-session", to: "foreign-origin", acts: [{ kind: "http", role: "GET", name: "/health" }] }],
  paths: { main: ["no-session.foreign-origin"] },
};

const sha = "9878f029" + "0".repeat(56);
const reply = { frame: true, kind: "text" as const, result: "pass" as const, status: 403, contentType: "application/json", body: "{\"ok\":false,\"reason\":\"session_required\"}", bodyTruncated: false, bodySha256: sha };
const run = (screens: JourneyRun["screens"]): JourneyRun => ({ state: "ready", result: "pass", screens, edges: { "no-session.foreign-origin": { result: "pass" } } });

describe("an HTTP step carries the reply its screen kept (#2945)", () => {
  it("reads the request, status, content type, text and a short digest from the run", () => {
    const steps = journeySteps(flow, run({ "no-session": reply, "foreign-origin": reply }));
    expect(steps.map((step) => step.http?.request)).toEqual(["GET /health", "GET /health"]);
    expect(steps[1]!.http).toEqual({ request: "GET /health", status: 403, contentType: "application/json", body: reply.body, truncated: false, sha: "9878f029" });
    // The reply is text, so no image frame is fetched for it.
    expect(steps.map((step) => step.frame)).toEqual([false, false]);
  });

  it("keeps the Runtime's cut mark, and cuts again to draw no more than the limit", () => {
    const [, step] = journeySteps(flow, run({ "no-session": reply, "foreign-origin": { ...reply, body: "x".repeat(HTTP_BODY_DRAW_LIMIT + 10), bodyTruncated: false } }));
    expect(step!.http!.body).toHaveLength(HTTP_BODY_DRAW_LIMIT);
    expect(step!.http!.truncated).toBe(true);
    const [, said] = journeySteps(flow, run({ "no-session": reply, "foreign-origin": { ...reply, bodyTruncated: true } }));
    expect(said!.http!.truncated).toBe(true);
  });

  it("falls back to today's step when the reply carries no status (an older Runtime)", () => {
    const old = { frame: true, kind: "text" as const, result: "pass" as const };
    expect(journeySteps(flow, run({ "no-session": old, "foreign-origin": old })).map((step) => step.http)).toEqual([null, null]);
    expect(journeySteps(flow, null).map((step) => step.http)).toEqual([null, null]);
  });

  it("draws nothing the Runtime did not send: no content type, no digest, a malformed digest", () => {
    const [, step] = journeySteps(flow, run({ "no-session": reply, "foreign-origin": { frame: true, kind: "text", result: "pass", status: 204, bodySha256: "<img src=x>" } }));
    expect(step!.http).toEqual({ request: "GET /health", status: 204, contentType: null, body: "", truncated: false, sha: null });
  });
});

describe("journey kind for HTTP (#2945)", () => {
  it("reads HTTP when every act is a request, and leaves the other kinds as they were", () => {
    expect(journeyKind(flow)).toBe("http");
    expect(journeyKind({ ...flow, edges: [{ id: "a", from: "no-session", to: "foreign-origin", acts: [{ kind: "activate", role: "button", name: "Go" }] }] })).toBe("browser");
    expect(journeyKind({ ...flow, edges: [...flow.edges!, { id: "b", from: "no-session", to: "foreign-origin", acts: [{ kind: "activate", role: "button", name: "Go" }] }] })).toBe("browser");
    expect(journeyKind({ ...flow, edges: [] })).toBe("browser");
  });
});
