import { describe, expect, it, vi } from "vitest";

import { otherStudioProjects } from "./projects";
import { devSession, SESSION_KEY_STORAGE } from "./session";

/** #1767: one Studio serves several projects; each page keeps one project's key and Runtime. */
const FIRST = "a".repeat(64);
const SECOND = "b".repeat(64);

function memoryStore() {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
  };
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

describe("a joined project's session", () => {
  it("asks for that project, keeps its key apart, and routes its Runtime calls through /p/<id>", async () => {
    const store = memoryStore();
    store.setItem(SESSION_KEY_STORAGE, "f".repeat(64));
    const fetchImpl = vi.fn(async () => json({ ok: true, token: "second-token", project: "zuvemo", projectId: SECOND, sessionKey: "c".repeat(64) }));
    const session = await devSession(fetchImpl as never, `?session=ticket&project=${SECOND}`, store);
    expect(fetchImpl).toHaveBeenCalledWith(`/__studio/session?nonce=ticket&project=${SECOND}`, expect.anything());
    expect(session).toMatchObject({ token: "second-token", project: "zuvemo", projectId: SECOND, baseUrl: `/p/${SECOND}` });
    // The Studio's own project's key is untouched; the joined one is kept under its own name.
    expect(store.values.get(SESSION_KEY_STORAGE)).toBe("f".repeat(64));
    expect(store.values.get(`${SESSION_KEY_STORAGE}.${SECOND}`)).toBe("c".repeat(64));
  });

  it("presents only that project's remembered key on a reload, never the Studio's own", async () => {
    const store = memoryStore();
    store.setItem(SESSION_KEY_STORAGE, "f".repeat(64));
    store.setItem(`${SESSION_KEY_STORAGE}.${SECOND}`, "c".repeat(64));
    const fetchImpl = vi.fn(async () => json({ ok: true, token: "second-token", projectId: SECOND }));
    await devSession(fetchImpl as never, `?project=${SECOND}`, store);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    expect(fetchImpl).toHaveBeenCalledWith(`/__studio/session?nonce=${"c".repeat(64)}&project=${SECOND}`, expect.anything());
  });

  it("refuses an answer for another project", async () => {
    const fetchImpl = vi.fn(async () => json({ ok: true, token: "first-token", projectId: FIRST }));
    expect(await devSession(fetchImpl as never, `?session=ticket&project=${SECOND}`, memoryStore())).toBeNull();
  });

  it("ignores a project parameter that is not a project id", async () => {
    const fetchImpl = vi.fn(async () => json({ ok: true, token: "first-token" }));
    const session = await devSession(fetchImpl as never, "?session=ticket&project=../x", memoryStore());
    expect(fetchImpl).toHaveBeenCalledWith("/__studio/session?nonce=ticket", expect.anything());
    expect(session).not.toHaveProperty("baseUrl");
  });
});

describe("the project switcher's list", () => {
  it("lists every other project, opening the Studio's own at the root and a joined one by id", async () => {
    const store = memoryStore();
    const identity = vi.fn(async () => json({ projectId: FIRST, projectIds: [FIRST, SECOND] }));
    // On the Studio's own project: its name is remembered, the joined one is offered.
    expect(await otherStudioProjects(null, "GraphHelm", identity as never, store))
      .toEqual([{ id: SECOND, name: `project ${SECOND.slice(0, 8)}`, href: `/?project=${SECOND}` }]);
    // On the joined project: the Studio's own is offered by the name this tab learned.
    expect(await otherStudioProjects(SECOND, "zuvemo", identity as never, store))
      .toEqual([{ id: FIRST, name: "GraphHelm", href: "/" }]);
    expect(identity).toHaveBeenCalledWith("/__studio/identity", expect.anything());
  });

  it("offers nothing when the Studio has no identity route or answers something else", async () => {
    expect(await otherStudioProjects(null, null, vi.fn(async () => new Response("", { status: 404 })) as never, memoryStore())).toBeNull();
    expect(await otherStudioProjects(null, null, vi.fn(async () => json({ projectIds: ["../etc", 7] })) as never, memoryStore())).toEqual([]);
    expect(await otherStudioProjects(null, null, vi.fn(async () => { throw new Error("offline"); }) as never, memoryStore())).toBeNull();
  });
});
