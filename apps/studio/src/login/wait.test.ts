import { afterEach, expect, it, vi } from "vitest";
import { loginWaiting } from "./session";

afterEach(() => vi.unstubAllGlobals());
const id = "b".repeat(64);
const identity = () => new Response(JSON.stringify({ projectId: id, projectIds: [] }));
const sessionFetch = (answers: Response[]) => vi.fn(async (url: string) => (url.includes("identity") ? identity() : answers.shift() ?? new Response("{}", { status: 503 })));
const ok = () => new Response(JSON.stringify({ ok: true, projectId: id }));
const sessionCalls = (fetcher: ReturnType<typeof vi.fn>) => fetcher.mock.calls.filter(([url]) => String(url).startsWith("/__studio/session")).length;

it("waits while the session answers 503, then lands on the project (#2696)", async () => {
  const fetcher = sessionFetch([new Response("{}", { status: 503 }), new Response("{}", { status: 503 }), ok()]);
  vi.stubGlobal("fetch", fetcher);
  const waiting = vi.fn();
  expect(await loginWaiting("", waiting, () => true, [1, 1, 1])).toHaveLength(1);
  expect(waiting).toHaveBeenCalledTimes(2);
  expect(sessionCalls(fetcher)).toBe(3);
});
it("sends a 404 for an unknown project straight to the gate, with no wait", async () => {
  const fetcher = sessionFetch([new Response("{}", { status: 404 })]);
  vi.stubGlobal("fetch", fetcher);
  const waiting = vi.fn();
  expect(await loginWaiting(`?project=${"a".repeat(64)}`, waiting, () => true, [1, 1, 1])).toBeNull();
  expect(waiting).not.toHaveBeenCalled();
  expect(sessionCalls(fetcher)).toBe(1);
});
it("gives up after the bounded retries", async () => {
  const fetcher = vi.fn(async (url: string) => (url.includes("identity") ? identity() : new Response("{}", { status: 503 })));
  vi.stubGlobal("fetch", fetcher);
  expect(await loginWaiting("", () => undefined, () => true, [1, 1, 1])).toBeNull();
  expect(sessionCalls(fetcher)).toBe(4);
});
it("stops waiting when the page goes away", async () => {
  const fetcher = vi.fn(async (url: string) => (url.includes("identity") ? identity() : new Response("{}", { status: 503 })));
  vi.stubGlobal("fetch", fetcher);
  let live = true;
  expect(await loginWaiting("", () => { live = false; }, () => live, [1, 1, 1])).toBeNull();
  expect(sessionCalls(fetcher)).toBe(1);
});
