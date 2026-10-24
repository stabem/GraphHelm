import { describe, expect, it } from "vitest";

import stylesheet from "../styles.css?raw";

/** The declarations every rule gives a selector (last one wins), read from the shipped stylesheet. */
function declared(selector: string): Record<string, string> {
  const css = stylesheet.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Record<string, string> = {};
  for (const rule of css.matchAll(/([^{}@]+)\{([^{}]*)\}/g)) {
    if (!rule[1]!.split(",").some((each: string) => each.trim() === selector)) continue;
    for (const part of rule[2]!.split(";")) {
      const at = part.indexOf(":");
      if (at > 0) out[part.slice(0, at).trim()] = part.slice(at + 1).trim();
    }
  }
  return out;
}

describe("the step frame shows the whole screenshot (#1966)", () => {
  it.each([".compare-frame img", ".phone img"])("%s scales to its box width, keeps its aspect ratio and is never cropped", (selector) => {
    const image = declared(selector);
    expect(image["width"]).toBe("100%");
    expect(image["height"]).toBe("auto");
    expect(image["max-width"]).toBe("100%");
    expect(image["object-fit"]).toBe("contain");
  });

  it.each([".compare-frame img", ".phone img"])("%s is also bounded by the viewport height so its bottom edge is in view (#1971)", (selector) => {
    const image = declared(selector);
    expect(image["max-height"]).toMatch(/100d?vh/);
    expect(image["object-fit"]).toBe("contain");
    expect(image["margin-inline"]).toBe("auto");
  });

  it("the phone box grows with the frame and gives it no scroll of its own", () => {
    const phone = declared(".phone");
    expect(phone["height"]).toBe("auto");
    expect(phone["overflow"]).toBe("hidden");
  });

  it("an overlay lays Now exactly over Before at the same size", () => {
    expect(declared(".compare-overlay img + img")).toMatchObject({ position: "absolute", left: "0", top: "0", height: "100%", "max-height": "none" });
  });
});
