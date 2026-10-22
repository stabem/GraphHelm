import { describe, expect, it } from "vitest";

import { controlBody, draftFor, messageBody } from "./drafts";

describe("owner requests to a lane (#1837)", () => {
  it("resends a failed request with its first id and bytes, and draws a new id only for a new intent", () => {
    let drawn = 0;
    const make = (text: string) => () => {
      drawn += 1;
      const id = `reply-${drawn}`;
      return { id, body: messageBody("run-1", id, "lane-a", text, "q-1", new Date(Date.UTC(2026, 9, 22, 10, drawn)), "studio-operator") };
    };
    const first = draftFor(undefined, "hello", make("hello"));
    const retry = draftFor(first, "hello", make("hello"));
    expect(JSON.stringify(retry.body)).toBe(JSON.stringify(first.body));
    expect(retry.id).toBe("reply-1");
    const edited = draftFor(first, "hello again", make("hello again"));
    expect(edited.id).toBe("reply-2");
    expect((edited.body.signal as { emittedAt: string }).emittedAt).not.toBe((first.body.signal as { emittedAt: string }).emittedAt);
  });

  it("names each control's fields and an instant without milliseconds", () => {
    const now = new Date(Date.UTC(2026, 9, 22, 10, 5, 7, 123));
    expect(controlBody("wake", "why", now)).toEqual({ reason: "why", at: "2026-10-22T10:05:07Z" });
    expect(controlBody("stop", "why", now)).toEqual({ reason: "why", retired: true, at: "2026-10-22T10:05:07Z" });
    expect(controlBody("reassign", "why", now, { sliceKey: "issue:7", to: "lane-b" })).toEqual({ sliceKey: "issue:7", to: "lane-b", reason: "why", at: "2026-10-22T10:05:07Z" });
  });
});
