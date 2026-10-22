import { describe, expect, it } from "vitest";

import type { JourneyFlow } from "../runtime/client";
import { decisionOf, offersDecision, rejectNote } from "./decision";

const draft: JourneyFlow = { id: "buy", title: null, status: "draft" };

describe("journey decision", () => {
  it("reads approved from the status and rejected only from a rejection of this content", () => {
    expect(decisionOf({ ...draft, status: "approved" })).toBe("approved");
    expect(decisionOf({ ...draft, rejected: { note: "wrong page" } })).toBe("rejected");
    expect(decisionOf({ ...draft, rejected: null })).toBe("undecided");
    expect(decisionOf({ ...draft, status: "approval_stale" })).toBe("undecided");
  });

  it("offers Approve and Reject only on a finished run of a journey not yet approved", () => {
    expect(offersDecision(draft, true)).toBe(true);
    expect(offersDecision({ ...draft, rejected: { note: "x" } }, true)).toBe(true);
    expect(offersDecision(draft, false)).toBe(false);
    expect(offersDecision({ ...draft, status: "approved" }, true)).toBe(false);
  });

  it("refuses a blank reason instead of sending it", () => {
    expect(rejectNote("  ")).toHaveProperty("refusal");
    expect(rejectNote(" off by one ")).toEqual({ note: "off by one" });
  });
});
