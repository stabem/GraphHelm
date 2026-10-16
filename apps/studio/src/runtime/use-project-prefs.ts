import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";

import { loadProjectName, loadRemovedRuns, saveProjectName, saveRemovedRuns, validProjectName } from "../studio-preferences";
import type { RuntimeClient } from "./client";
import type { ExecutionSummary } from "./types";

/** What the project preferences need from App: the client and the rail they restore rows into. */
export interface ProjectPrefsInputs {
  clientRef: MutableRefObject<RuntimeClient | null>;
  executionsRef: MutableRefObject<ExecutionSummary[]>;
  restoredOutsidePage: MutableRefObject<Set<string>>;
  setExecutions: Dispatch<SetStateAction<ExecutionSummary[]>>;
  listPageSize: number;
}

/** The project this browser shows: its name, folder and the runs removed from this view, kept per
 * Runtime identity in browser storage (#1395, moved out of App.tsx unchanged). */
export function useProjectPrefs({ clientRef, executionsRef, restoredOutsidePage, setExecutions, listPageSize: LIST_PAGE_SIZE }: ProjectPrefsInputs) {
  const [project, setProject] = useState<string | null>(null);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [onDemandLanes, setOnDemandLanes] = useState<string[]>([]);
  const [projectIdentity, setProjectIdentity] = useState<string | null>(null);
  const [removedRuns, setRemovedRuns] = useState<string[]>([]);
  const removedRunsRef = useRef<string[]>([]);
  removedRunsRef.current = removedRuns;
  const [projectPreferenceNotice, setProjectPreferenceNotice] = useState("");
  /** What a new connection learns about its project: `connect` calls this once the Runtime answered. */
  const openProject = useCallback((identity: string | null, folder: string | null, roster: string[]) => {
    setProjectIdentity(identity);
    setProject(identity === null ? null : loadProjectName(`${location.origin}:${identity}`, identity));
    setProjectPath(folder);
    setOnDemandLanes(roster);
    const removed = identity === null ? [] : loadRemovedRuns(`${location.origin}:${identity}`);
    removedRunsRef.current = removed;
    setRemovedRuns(removed);
    setProjectPreferenceNotice("");
  }, []);
  /** A disconnect forgets the project: another token may open another one. */
  const resetProject = useCallback(() => {
    setProject(null);
    setProjectPath(null);
    setProjectIdentity(null);
    setRemovedRuns([]);
    removedRunsRef.current = [];
    setProjectPreferenceNotice("");
  }, []);
  const runtimePreferenceKey = projectIdentity === null ? null : `${location.origin}:${projectIdentity}`;
  const renameProject = (name: string): boolean => {
    const valid = validProjectName(name);
    if (valid === null) return false;
    if (runtimePreferenceKey === null) {
      setProject(valid);
      setProjectPreferenceNotice("Project name changed for this browser view only; this Runtime has no public project identity.");
      return true;
    }
    const saved = saveProjectName(runtimePreferenceKey, valid);
    if (!saved) {
      setProjectPreferenceNotice("Project name changed for this view only; browser storage is unavailable.");
      setProject(valid);
      return true;
    }
    setProject(valid);
    setProjectPreferenceNotice("");
    return true;
  };
  const removeRunFromView = (id: string) => {
    const next = [...new Set([...removedRuns, id])];
    setRemovedRuns(next);
    if (runtimePreferenceKey === null) {
      setProjectPreferenceNotice("Removed from this browser view only; this Runtime has no public project identity.");
    } else if (!saveRemovedRuns(runtimePreferenceKey, next)) {
      setProjectPreferenceNotice("Removed from this view only; browser storage is unavailable or full.");
    }
  };
  const restoreRun = async (id: string) => {
    const client = clientRef.current;
    if (!client) return;
    if (!executionsRef.current.some((run) => run.executionId === id)) {
      try {
        // Read a bounded prefix to find the row. Keep the restore
        // entry until the Runtime has actually returned that run; an absent row is not success.
        let cursor: string | null = null;
        let restored: ExecutionSummary | undefined;
        for (let pageNumber = 0; pageNumber < 50; pageNumber += 1) {
          const page = await client.listExecutions({ limit: LIST_PAGE_SIZE, after: cursor ?? undefined });
          if (clientRef.current !== client) return;
          cursor = page.hasMore ? page.nextCursor : null;
          restored = page.executions.find((run) => run.executionId === id);
          if (restored || cursor === null) break;
        }
        if (!restored) {
          setProjectPreferenceNotice(cursor === null
            ? "This run is not in the Runtime list. It remains in the removed list; try restoring it again later."
            : "This run was not found in the first 50 pages. It remains in the removed list; try restoring it again later.");
          return;
        }
        // Restoring one run must not page every traversed row into the live rail: polling
        // follows its loaded row count. Preserve concurrent rows and the Show more cursor.
        const target = restored;
        if (!executionsRef.current.some((run) => run.executionId === id)) restoredOutsidePage.current.add(id);
        setExecutions((previous) => previous.some((run) => run.executionId === id)
          ? previous
          : [...previous, target]);
      } catch {
        if (clientRef.current === client) setProjectPreferenceNotice("The run could not be loaded. It remains in the removed list; try restoring it again.");
        return;
      }
    }
    const next = removedRunsRef.current.filter((runId) => runId !== id);
    removedRunsRef.current = next;
    setRemovedRuns(next);
    if (runtimePreferenceKey === null) {
      setProjectPreferenceNotice("Restored for this browser view only; this Runtime has no public project identity.");
    } else if (!saveRemovedRuns(runtimePreferenceKey, next)) {
      setProjectPreferenceNotice("Restored for this view only; browser storage is unavailable or full.");
    }
  };
  return {
    project, projectPath, onDemandLanes, projectIdentity, removedRuns, removedRunsRef, projectPreferenceNotice, setProjectPreferenceNotice,
    openProject, resetProject, renameProject, removeRunFromView, restoreRun,
  };
}
