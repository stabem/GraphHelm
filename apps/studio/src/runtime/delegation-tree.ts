import { isUnclaimedCritic, type TaskState } from "./team-tasks";

type ClaimSource = "claimed.assignedBy (reported by lane)" | null;
export interface DelegationRoot {
  assigner: string | null;
  lanes: {
    lane: string | null;
    source: ClaimSource;
    tasks: {
      task: TaskState;
      source: ClaimSource;
      reviewers: { reviewer: string; source: "review_assigned" }[];
    }[];
  }[];
}

/** Group recorded task slices, without inventing parentage from order, verdicts or host sessions. */
export function buildDelegationTree(tasks: TaskState[]): DelegationRoot[] {
  const roots = new Map<string | null, DelegationRoot>();
  for (const task of tasks) {
    if (isUnclaimedCritic(task)) continue;
    const assigner = task.assignedBy ?? null;
    const source = assigner === null ? null : "claimed.assignedBy (reported by lane)";
    let root = roots.get(assigner);
    if (!root) {
      root = { assigner, lanes: [] };
      roots.set(assigner, root);
    }
    let lane = root.lanes.find((entry) => entry.lane === task.lane);
    if (!lane) {
      lane = { lane: task.lane, source, tasks: [] };
      root.lanes.push(lane);
    }
    lane.tasks.push({ task, source,
      reviewers: (task.assignedReviewers ?? []).map((reviewer) => ({ reviewer, source: "review_assigned" })),
    });
  }
  // Explicit assignments remain visible above the usually much larger legacy group.
  return [...roots.values()].sort((a, b) => Number(a.assigner === null) - Number(b.assigner === null));
}
