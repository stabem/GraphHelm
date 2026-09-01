import { describe, expect, it } from "vitest";

import { newExecutionId } from "./draft";

describe("the minted execution id", () => {
  /**
   * THE STORE READS IDS TOO, and it reads them with a secret detector.
   *
   * The event store refuses to persist content containing secret-shaped strings, and its prefix
   * table includes `sk-` followed by a 20+ character token - the shape of an OpenAI API key. The
   * first minted prefix was `task-`, which CONTAINS `sk-` (ta**sk-**), and a UUID tail is more
   * than 20 id-safe characters: every id this module minted was refused by the Runtime as
   * `GHE009_EXTERNALIZATION_FAILED: repository content is not safe for persistence`.
   *
   * Measured live on 2026-08-30: `task-<uuid>` and `desk-<same tail>` both 500, `probe-...` 200.
   * Which means the composer's own "+ new task" send had NEVER once worked against a real
   * Runtime - every jsdom test stubbed the client, so the only reader of the minted id that
   * mattered was never in the loop.
   *
   * The list below is the store's own prefix table (`core/graph/src/persistence.rs`), checked as
   * substrings because that is how the store checks them. The tail of a minted id is hex and
   * dashes, so a prefix containing a non-hex letter cannot form in the TAIL - the prefix half is
   * the half that can regress, and this pins it.
   */
  it("mints ids the event store will not mistake for a secret", () => {
    const storePrefixTable = [
      "ghp_",
      "ghp-",
      "github_pat_",
      "github-pat-",
      "sk-",
      "sk_",
      "akia",
      "asia",
      "aida",
      "aroa",
      "aipa",
      "anpa",
      "anva",
      "glpat-",
      "glpat_",
      "xoxb-",
      "xoxb_",
      "xoxp-",
      "xoxp_",
      "xoxa-",
      "xoxa_",
      "xoxr-",
      "xoxr_",
      "xoxs-",
      "xoxs_",
    ];
    for (let round = 0; round < 20; round += 1) {
      const id = newExecutionId().toLowerCase();
      for (const prefix of storePrefixTable) {
        expect(id, `"${id}" contains the secret-shaped prefix "${prefix}"`).not.toContain(prefix);
      }
    }
  });

  it("mints distinct ids", () => {
    expect(newExecutionId()).not.toBe(newExecutionId());
  });

  it("still mints when the browser lacks randomUUID", () => {
    // Some browsers on plain http expose getRandomValues without randomUUID; the ONLY way to
    // start a task must not throw there (PR #467 review). Same store-safe shape either way.
    const original = crypto.randomUUID;
    Object.defineProperty(crypto, "randomUUID", { value: undefined, configurable: true });
    try {
      const id = newExecutionId();
      expect(id).toMatch(/^run-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
    } finally {
      Object.defineProperty(crypto, "randomUUID", { value: original, configurable: true });
    }
  });
});

describe("the draft's budgets", () => {
  it("grants the topology budget its own instructions promise", async () => {
    // The node tells its agent to "emit a graph signal for any topology you believe is needed";
    // maxMutations: 0 made the Governor refuse every such proposal with LimitExceeded, locking
    // each Studio task to its start node forever (PR #467 review). A guard, not a comment: the
    // day someone zeroes this again, the promise and the budget disagree HERE.
    const { draftGraph } = await import("./draft");
    const budgets = (draftGraph("run-x", "do the thing").spec as { budgets: { maxMutations: number } })
      .budgets;
    expect(budgets.maxMutations).toBeGreaterThan(0);
  });
});
