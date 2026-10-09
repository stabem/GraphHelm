import { describe, expect, it, vi } from "vitest";
import { recordReviewAssigned, reviewAssignmentDocument } from "./review-assign";

// Independent wire fixture: task_record.py --issue 618 review_assigned --pr 619
// --head b9892b1a89441d15345086604d51ccf6fd41be3f --reviewer gh-claude-4 --dry-run.
// Protects revision/ordinal and current-head binding, absent from the previous note-only tests.
// Cost: milliseconds, mocked Runtime I/O only; no test-only production seam.
const fixture = { schema: "graphhelm-task-event-v1", taskId: "issue-618", revision: 3,
  at: "2026-10-09T23:00:00Z", pr: 619, headSha: "b9892b1a89441d15345086604d51ccf6fd41be3f",
  reviewer: "gh-claude-4", ordinal: 1 };
const task = { taskId: fixture.taskId, pr: fixture.pr, headSha: fixture.headSha };

describe("review assignment", () => {
  it("matches the task recorder document and sends it as a review record", async () => {
    expect(reviewAssignmentDocument(task, fixture.reviewer, fixture.at)).toEqual(fixture);
    const signal = vi.fn().mockResolvedValue({ result: "succeeded" });
    await recordReviewAssigned({ signal }, "gh-team", task, fixture.reviewer);
    const [run, message, options] = signal.mock.calls[0];
    expect(run).toBe("gh-team");
    expect(JSON.parse(message)).toEqual({ ...fixture, at: options.emittedAt });
    expect(options.kind).toBe("task.review_assigned");
  });
  it.each(["refused", "unknown"])("propagates a %s record instead of reporting success", async (result) => {
    const signal = vi.fn().mockResolvedValue({ result, diagnostics: [{ code: "GHCLI038_ACTOR_MISMATCH", message: "actor refused" }] });
    await expect(recordReviewAssigned({ signal }, "gh-team", task, fixture.reviewer)).rejects.toThrow("GHCLI038_ACTOR_MISMATCH: actor refused");
  });
});
