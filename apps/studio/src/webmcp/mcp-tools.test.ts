import { describe, expect, it } from "vitest";
import catalog from "../../../../schemas/mcp-tools.json";
import { DEPRECATED_ALIASES, MCP_TOOL_OF, registerStudioTools, type WebMcpToolDescriptor } from "./adapter";

/* #1330 (plan D2): one schema source. `schemas/mcp-tools.json` is the MCP `tools/list` as the Rust
 * `TOOLS` table serves it (pinned by `the_committed_tool_catalog_is_the_emitted_one`). Every page
 * tool that wraps an MCP tool must keep that tool's required fields and the bounds both declare;
 * before this, `events.limit` was 0..u64::MAX on MCP, 1..1000 on HTTP and 1..200 here, and two
 * page tools spelled MCP names differently. */

interface Schema { required?: string[]; properties?: Record<string, Record<string, unknown>> }
const mcp = new Map((catalog as { tools: { name: string; inputSchema: Schema }[] }).tools.map((tool) => [tool.name, tool.inputSchema]));

/** Inputs only the page offers: the idempotency key a browser retry needs, which MCP derives. */
const STUDIO_ONLY_FIELDS = new Set(["idempotencyKey"]);
/** Narrowings, each with its reason. The page resumes from a graph FILE only: a browser agent
 * has no inline graph to hand over, so `file` (one of MCP's two alternatives) is required here. */
const NARROWED_REQUIRED: Record<string, string[]> = { graphhelm_resume_execution: ["file"] };

function registered(): WebMcpToolDescriptor[] {
  const tools: WebMcpToolDescriptor[] = [];
  registerStudioTools({ connected: true } as never, { onActivity() {}, onSelect() {}, onMutation() {} } as never, {
    modelContext: { registerTool: (tool: WebMcpToolDescriptor) => void tools.push(tool) },
  });
  return tools;
}

describe("the page tools follow the MCP catalog (#1330)", () => {
  const tools = registered();
  const canonical = tools.filter((tool) => !(tool.name in DEPRECATED_ALIASES));

  it("reads the emitted catalog and maps every page tool, closed in both directions", () => {
    expect(mcp.size, "HARNESS-BROKE: the catalog listed no tools").toBeGreaterThan(20);
    expect(canonical.map((tool) => tool.name).sort()).toEqual(Object.keys(MCP_TOOL_OF).sort());
    for (const [page, name] of Object.entries(MCP_TOOL_OF)) {
      if (name !== null) expect(mcp.has(name), `${page} wraps ${name}, which the catalog does not list`).toBe(true);
    }
  });

  it("a mapped tool requires what its MCP tool requires, plus only its named narrowings", () => {
    for (const tool of canonical) {
      const name = MCP_TOOL_OF[tool.name];
      if (!name) continue;
      const want = [...(mcp.get(name)!.required ?? []), ...(NARROWED_REQUIRED[tool.name] ?? [])].sort();
      expect([...((tool.inputSchema.required as string[]) ?? [])].sort(), tool.name).toEqual(want);
    }
  });

  it("a mapped tool offers only MCP fields (or page-only ones) and keeps every bound both declare", () => {
    for (const tool of canonical) {
      const name = MCP_TOOL_OF[tool.name];
      if (!name) continue;
      const theirs = mcp.get(name)!.properties ?? {};
      const ours = (tool.inputSchema.properties ?? {}) as Record<string, Record<string, unknown>>;
      for (const [field, spec] of Object.entries(ours)) {
        if (STUDIO_ONLY_FIELDS.has(field)) continue;
        expect(theirs[field], `${tool.name}.${field} is not a field of MCP ${name}`).toBeDefined();
        for (const bound of ["minimum", "maximum"] as const) {
          if (bound in spec && bound in theirs[field]!) {
            expect(spec[bound], `${tool.name}.${field}.${bound}`).toBe(theirs[field]![bound]);
          }
        }
      }
    }
  });

  it("the two old names still answer, as aliases of the MCP spelling, for one release", () => {
    for (const [alias, target] of Object.entries(DEPRECATED_ALIASES)) {
      const old = tools.find((tool) => tool.name === alias);
      const now = tools.find((tool) => tool.name === target);
      expect(old && now, alias).toBeTruthy();
      expect(old!.execute).toBe(now!.execute);
      expect(old!.inputSchema).toEqual(now!.inputSchema);
      expect(old!.description.startsWith(`Deprecated alias of ${target}`)).toBe(true);
    }
  });
});
