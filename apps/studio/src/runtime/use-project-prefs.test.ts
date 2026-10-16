import { act, renderHook } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";

import { loadProjectName, loadRemovedRuns, saveProjectName, saveRemovedRuns } from "../studio-preferences";
import type { RuntimeClient } from "./client";
import type { ExecutionSummary } from "./types";
import { useProjectPrefs } from "./use-project-prefs";

const runtimeKey = (identity: string) => `${location.origin}:${identity}`;
const row = (executionId: string) => ({ executionId }) as ExecutionSummary;

function mount(listExecutions = vi.fn(async () => ({ executions: [row("old-run")], hasMore: false, nextCursor: null }))) {
  const clientRef = { current: { listExecutions } as unknown as RuntimeClient };
  const executionsRef = { current: [] as ExecutionSummary[] };
  const restoredOutsidePage = { current: new Set<string>() };
  const setExecutions = vi.fn((update: ExecutionSummary[] | ((previous: ExecutionSummary[]) => ExecutionSummary[])) => {
    executionsRef.current = typeof update === "function" ? update(executionsRef.current) : update;
  });
  const hook = renderHook(() => useProjectPrefs({ clientRef, executionsRef, restoredOutsidePage, setExecutions, listPageSize: 20 }));
  return { hook, executionsRef, restoredOutsidePage, listExecutions };
}

beforeEach(() => window.localStorage.clear());

// #1395: the project preferences moved out of App; these pin what a connection opens and a
// disconnect forgets, and that each change is kept per Runtime identity.
it("opens a project's saved name and removed runs, and forgets them on reset", () => {
  saveProjectName(runtimeKey("shop"), "Shop front");
  saveRemovedRuns(runtimeKey("shop"), ["old-run"]);
  const { hook } = mount();
  act(() => hook.result.current.openProject("shop", "C:/work/shop", ["codex"]));
  expect(hook.result.current).toMatchObject({ project: "Shop front", projectPath: "C:/work/shop", onDemandLanes: ["codex"], removedRuns: ["old-run"] });
  expect(hook.result.current.removedRunsRef.current).toEqual(["old-run"]);

  act(() => hook.result.current.resetProject());
  expect(hook.result.current).toMatchObject({ project: null, projectPath: null, removedRuns: [], projectPreferenceNotice: "" });
  expect(hook.result.current.removedRunsRef.current).toEqual([]);
});

it("keeps a rename and a removal per Runtime identity, and only in the view without one", () => {
  const { hook } = mount();
  act(() => hook.result.current.openProject("shop", null, []));
  act(() => { expect(hook.result.current.renameProject("  Storefront  ")).toBe(true); });
  act(() => hook.result.current.removeRunFromView("run-1"));
  expect(loadProjectName(runtimeKey("shop"), "x")).toBe("Storefront");
  expect(loadRemovedRuns(runtimeKey("shop"))).toEqual(["run-1"]);
  expect(hook.result.current.renameProject("")).toBe(false);

  act(() => hook.result.current.openProject(null, null, []));
  act(() => { hook.result.current.renameProject("Anonymous"); });
  expect(hook.result.current.project).toBe("Anonymous");
  expect(hook.result.current.projectPreferenceNotice).toMatch(/no public project identity/);
});

it("restores a removed run only once the Runtime returns its row", async () => {
  const listExecutions = vi.fn(async () => ({ executions: [row("old-run")], hasMore: false, nextCursor: null }));
  const { hook, executionsRef, restoredOutsidePage } = mount(listExecutions);
  saveRemovedRuns(runtimeKey("shop"), ["old-run", "gone-run"]);
  act(() => hook.result.current.openProject("shop", null, []));

  await act(async () => { await hook.result.current.restoreRun("gone-run"); });
  expect(hook.result.current.removedRuns).toContain("gone-run");
  expect(hook.result.current.projectPreferenceNotice).toMatch(/not in the Runtime list/);

  await act(async () => { await hook.result.current.restoreRun("old-run"); });
  expect(executionsRef.current.map((run) => run.executionId)).toEqual(["old-run"]);
  expect(restoredOutsidePage.current.has("old-run")).toBe(true);
  expect(hook.result.current.removedRuns).toEqual(["gone-run"]);
  expect(loadRemovedRuns(runtimeKey("shop"))).toEqual(["gone-run"]);
});
