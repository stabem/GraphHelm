/**
 * Guards on the stylesheet itself.
 *
 * WHY THESE EXIST. A scripted edit to `styles.css` left 181 opening braces against 182 closing
 * ones. A stray `}` closes a rule early and the parser drops everything after it up to the next
 * selector - silently, with no error anywhere - and in the same edit `.blob` lost its base rule,
 * leaving an element with a colour and no size. Both shipped. All 127 tests stayed green, because
 * not one of them read the stylesheet.
 *
 * None of what follows is a matter of taste. Braces balance or they do not; a `var()` names a
 * token that exists or it does not; a class a component renders has a rule or it does not. These
 * are the mechanical half of "the interface is not broken", and the mechanical half is the half a
 * test can hold.
 *
 * What they deliberately do NOT check is whether anything LOOKS right. jsdom lays nothing out, so
 * a clipped control and a buried block are invisible here exactly as they were before. That gap is
 * real and is not closed by this file.
 */

import { describe, expect, it } from "vitest";

// Vite's own raw import rather than `node:fs`. The filesystem version needed `@types/node`, which
// this package does not carry - so it ran green under vitest and broke `tsc`, and a test that only
// half-compiles is a test that gets deleted the first time someone hits it. This also removes the
// path handling entirely: the earlier attempt failed on Windows drive letters in a file URL.
import CSS from "./styles.css?raw";

/** Comments are stripped before any structural count: prose legitimately contains braces, and a
 * sentence about `{` must not read as a rule. */
const CODE = CSS.replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * Custom properties this stylesheet READS but deliberately does not DEFINE, each with the reason.
 * A bare allowlist would let a typo in a token name pass as intentional.
 */
const SET_ELSEWHERE: Record<string, string> = {
  "--rail": "App.tsx writes it inline from the operator's dragged width; the CSS reads it with a fallback",
};

/** Classes the components render that intentionally have no rule of their own. Empty today, and
 * kept so that adding one is a deliberate line in a diff rather than a silent omission. */
const NO_RULE_NEEDED: Record<string, string> = {};

/**
 * Every component's source, as text.
 *
 * `import.meta.glob` is resolved by Vite at build time, so the set is fixed when this test is
 * compiled - a component added later is picked up on the next run without anyone listing it here,
 * and a component deleted cannot leave a stale path behind.
 */
function componentSource(): string {
  const modules = import.meta.glob("./**/*.tsx", { query: "?raw", import: "default", eager: true });
  return Object.values(modules).join("\n");
}

describe("the stylesheet parses at all", () => {
  /** The one that shipped. An unbalanced brace is not a style opinion - it is a file the browser
   * reads differently from how it was written. */
  it("balances its braces", () => {
    const open = (CODE.match(/\{/g) ?? []).length;
    const close = (CODE.match(/\}/g) ?? []).length;
    expect(
      { open, close },
      "a stray brace closes a rule early and the parser silently drops everything after it",
    ).toEqual({ open: close, close });
  });

  /** Where the imbalance is, so the failure above is actionable rather than a number. */
  it("never closes a rule that was not open", () => {
    let depth = 0;
    const offenders: string[] = [];
    CODE.split("\n").forEach((line, index) => {
      for (const character of line) {
        if (character === "{") depth += 1;
        else if (character === "}") {
          depth -= 1;
          if (depth < 0) {
            offenders.push(`line ${index + 1}: ${line.trim()}`);
            depth = 0;
          }
        }
      }
    });
    expect(offenders, "these braces close a rule that was never opened").toEqual([]);
  });
});

describe("every token is real", () => {
  const defined = new Set(
    [...CSS.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((match) => match[1] as string),
  );
  const used = new Set([...CSS.matchAll(/var\((--[a-z0-9-]+)/g)].map((match) => match[1] as string));

  /** A `var()` naming nothing renders as nothing, which on a background or a colour is an
   * invisible element rather than a visible error. */
  it("defines every custom property it reads", () => {
    const missing = [...used].filter((token) => !defined.has(token) && !(token in SET_ELSEWHERE));
    expect(missing, "these are read by a rule and defined by nobody").toEqual([]);
  });

  /** A token defined and never read is a claim the file cannot keep - twice already, the header
   * described a scale step or a meaning colour that nothing used. */
  it("reads every custom property it defines", () => {
    const unused = [...defined].filter((token) => !used.has(token));
    expect(unused, "these are defined and never read; use them or drop them with their claim").toEqual([]);
  });
});

describe("the stylesheet and the components agree", () => {
  /** The second half of the same defect: `.blob` survived as modifiers with no base rule, so the
   * element had a colour and no size and would have rendered as nothing. */
  /**
   * ONLY THE STATIC HALF, and that limit is the point. A `className={`node ${mood}`}` carries a
   * real class and a modifier chosen at runtime; reading identifiers out of the `${...}` yields
   * VARIABLE NAMES - `tone`, `tool` - and reports them as missing rules. A guard that cries wolf
   * is a guard somebody switches off, so this reads the literal text and nothing else.
   *
   * What it therefore cannot see: a class that only ever exists as an interpolated value. That is
   * a declared hole, not an oversight - the defect this exists for (`.blob` losing its base rule)
   * was a plain string, and so is every class that names an element rather than a state.
   */
  it("has a rule for every class the components render", () => {
    const source = componentSource();
    const rendered = new Set(
      [...source.matchAll(/className=(?:"([^"]*)"|\{`([^`]*)`\})/g)]
        // Drop the interpolations, keep the literal text around them.
        .flatMap((match) => (match[1] ?? match[2] ?? "").replace(/\$\{[^}]*\}/g, " ").split(/\s+/))
        // A fragment ending in `-` is a PREFIX (`tool-${tool}`), never a whole class name.
        .filter((name) => /^[a-z][a-z0-9-]*$/.test(name) && !name.endsWith("-")),
    );
    const orphans = [...rendered].filter(
      (name) => !(name in NO_RULE_NEEDED) && !new RegExp(`\\.${name}[\\s,{:.]`).test(CSS),
    );
    expect(orphans, "these classes are rendered and styled by nothing").toEqual([]);
  });

  /**
   * EVERY ELEMENT IS STYLED BY SOMETHING THAT ALWAYS APPLIES.
   *
   * The check above only asks whether a class is MENTIONED, and that is not enough. When `.blob`
   * lost its base rule it kept three modifiers - `.node.waiting .blob { background }` and two
   * siblings - so it was still mentioned everywhere while having a colour and no size, which
   * renders as nothing at all. The sabotage walked straight past the guard written for it.
   *
   * The property that separates that from healthy code is CONDITIONALITY, not shape. `.blob`'s
   * every rule sat under a compound carrying a state (`.node.waiting`), so in the ordinary case -
   * a node that is not waiting - nothing styled it. `.topstrip .run-name` and `.composer .send`
   * are descendants of a structural ancestor and always apply; requiring those to own a base rule
   * as well would report four healthy elements and teach the reader to ignore this test.
   *
   * So: a class rendered on its own must have at least one rule that is unconditional - a base
   * rule, or a descendant rule whose ancestors are plain elements rather than states.
   */
  it("styles every solo class with a rule that always applies", () => {
    const source = componentSource();
    const solo = new Set(
      [...source.matchAll(/className="([a-z][a-z0-9-]*)"/g)].map((match) => match[1] as string),
    );

    /** Every selector in the file, one per rule, comments already gone. */
    const selectors = [...CODE.matchAll(/(^|\})\s*([^{}]+?)\s*\{/g)].map((match) =>
      (match[2] as string).replace(/\s+/g, " ").trim(),
    );

    /** `.a.b` - two classes on one element - is a STATE compound: it applies only sometimes. */
    const conditional = (selector: string) => /\.[a-z0-9-]+\.[a-z0-9-]+/.test(selector);

    const unstyled = [...solo].filter((name) => {
      if (name in NO_RULE_NEEDED) return false;
      const mentions = selectors.filter((selector) =>
        new RegExp(`\\.${name}(?![a-z0-9-])`).test(selector),
      );
      return !mentions.some((selector) => !conditional(selector));
    });

    expect(
      unstyled,
      "every rule for these sits under a state, so in the ordinary case nothing styles them",
    ).toEqual([]);
  });
});
