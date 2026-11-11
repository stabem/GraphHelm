import { useEffect, useRef } from "react";

/**
 * WebMCP page tools (#2839, first slice of #1870): a component hands the browser's agent host the
 * actions its own buttons offer.
 *
 * A tool is not a second way to act. The component that owns a button registers the tool, and the
 * tool's handler calls the very function the button's `onClick` calls. Where the button is disabled
 * or absent the handler refuses; it never goes around a disabled state, a confirmation or a busy
 * lock. A handler answers `{ ok: true }` to say the action was dispatched, never what came of it:
 * the rendered screen is the outcome.
 *
 * Tool calls reach the Runtime through the same client as clicks, so they are recorded as the Studio
 * operator, exactly like a click. That is why no tool here approves, rejects, merges or does
 * anything else that is irreversible or carries authority.
 */

export type PageToolResult = { ok: true } | { ok: false; reason: string };

export interface PageTool {
  name: string;
  /** One sentence: what a person would do with the button. */
  description: string;
  /** A closed JSON Schema object. */
  inputSchema: Record<string, unknown>;
  handler: (input: Record<string, unknown>) => PageToolResult | Promise<PageToolResult>;
}

/** The part of the browser's `ModelContext` the hook uses, declared structurally: the API is still a
 * proposal, and the Studio must not pin one draft of it. */
interface ModelContextLike {
  registerTool: (
    tool: { name: string; description: string; inputSchema: Record<string, unknown>; execute: (input: Record<string, unknown>) => Promise<PageToolResult> },
    options?: { signal?: AbortSignal },
  ) => unknown;
}

const FAILED: PageToolResult = { ok: false, reason: "The Studio could not run this action." };

function findHost(): ModelContextLike | null {
  const candidate = (typeof document === "undefined" ? undefined : (document as unknown as { modelContext?: unknown }).modelContext)
    ?? (typeof navigator === "undefined" ? undefined : (navigator as unknown as { modelContext?: unknown }).modelContext);
  return typeof (candidate as ModelContextLike | undefined)?.registerTool === "function" ? (candidate as ModelContextLike) : null;
}

/**
 * Registers `tools` while the calling component is mounted and removes them when it unmounts, by the
 * `AbortSignal` the browser's `registerTool` takes (the host has no `unregisterTool`). With no host
 * it does nothing. A tool's `execute` always calls the latest handler the component passed.
 */
export function usePageTools(tools: PageTool[]): void {
  const latest = useRef(tools);
  latest.current = tools;
  const names = tools.map((tool) => tool.name).join("|");
  useEffect(() => {
    const host = findHost();
    if (host === null) return;
    // One signal for every tool of this mount: StrictMode's unmount aborts it before the next mount
    // registers the same names again, and the host takes the second registration.
    const controller = new AbortController();
    for (const { name, description, inputSchema } of latest.current) {
      const execute = async (input: Record<string, unknown>): Promise<PageToolResult> => {
        const current = latest.current.find((tool) => tool.name === name);
        if (current === undefined) return FAILED;
        try { return await current.handler(input ?? {}); }
        catch { return FAILED; }
      };
      try {
        // A registration the host refuses (an aborted mount, a name already taken) is not the page's failure.
        Promise.resolve(host.registerTool({ name, description, inputSchema, execute }, { signal: controller.signal })).catch(() => undefined);
      } catch { /* a host that throws instead of rejecting is treated the same way */ }
    }
    return () => controller.abort();
  }, [names]);
}
