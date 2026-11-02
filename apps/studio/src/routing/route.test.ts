import { describe, expect, it } from "vitest";

import { parseRoute, routeSearch } from "./route";

describe("route", () => {
  it("drops the spent login ticket and keeps project, run and view", () => {
    const id = "a".repeat(64);
    expect(routeSearch(parseRoute(`?session=studio-fixture&project=${id}&run=demo&view=proof`))).toBe(`?project=${id}&run=demo&view=proof`);
  });

  it("refuses a project, a run id that is not an id and an unknown view", () => {
    expect(parseRoute("?project=../x&run=../x&view=admin&journey=../x")).toEqual({ project: null, run: null, view: "graph", journey: null });
  });

  it("opens Proof on the journey a `studio open --journey` link names, and keeps it", () => {
    const route = parseRoute("?session=t&project=first&journey=studio2-proof-run");
    expect(route).toEqual({ project: "first", run: null, view: "proof", journey: "studio2-proof-run" });
    expect(routeSearch(route)).toBe("?project=first&view=proof&journey=studio2-proof-run");
    expect(parseRoute("?journey=checkout&view=lanes").view).toBe("lanes");
  });
});

// #2442: an earlier run of the opened journey rides in the URL, so it can be linked and reloaded.
describe("the past run in the URL", () => {
  it("is read and written only beside a journey, and only as a run id", () => {
    expect(parseRoute("?journey=buy&past=00000000000000001000").past).toBe("00000000000000001000");
    expect(parseRoute("?past=00000000000000001000").past).toBeUndefined();
    expect(parseRoute("?journey=buy&past=../x").past).toBeUndefined();
    expect(routeSearch({ project: null, run: null, view: "proof", journey: "buy", past: "00000000000000001000" })).toBe("?view=proof&journey=buy&past=00000000000000001000");
    expect(routeSearch({ project: null, run: null, view: "proof", journey: null, past: "00000000000000001000" })).toBe("?view=proof");
  });
});
