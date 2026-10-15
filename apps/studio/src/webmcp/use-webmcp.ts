import { useCallback, useEffect, useRef, useState } from "react";

import type { RuntimeClient } from "../runtime/client";
import { registerStudioTools, type AdapterHooks, type ModelContextLike, type RegisteredTools, type WebMcpAvailability } from "./adapter";

/**
 * The Studio's WebMCP tools for one connection (#1408; moved out of App.tsx unchanged): registered
 * when a connection opens, removed when it fails or closes, and on unmount. `webmcp` and `tools`
 * are what the page says about them.
 */
export function useWebMcp(
  modelContext: ModelContextLike | null | undefined,
  /** Tests pass a fake: the suite shares module state (`isolate: false`), so a module mock would leak. */
  register: typeof registerStudioTools = registerStudioTools,
) {
  const toolsRef = useRef<RegisteredTools | null>(null);
  const [webmcp, setWebmcp] = useState<WebMcpAvailability>("unavailable");
  const [tools, setTools] = useState<string[]>([]);
  const registerTools = useCallback((client: RuntimeClient, hooks: AdapterHooks) => {
    const registration = register(client, hooks, modelContext === undefined ? {} : { modelContext });
    toolsRef.current = registration;
    setWebmcp(registration.availability);
    setTools(registration.names);
    // A host whose registerTool settles ASYNCHRONOUSLY confirms its names only when `ready`
    // settles - the snapshot above would then say "no site tools" forever on such a host
    // (PR #467 review). Refreshed once settled, and only if this registration is still the
    // page's current one: a disconnect in between must not resurrect dead chips.
    void registration.ready.then(() => {
      if (toolsRef.current !== registration) return;
      setWebmcp(registration.availability);
      setTools(registration.names);
    });
  }, [modelContext, register]);
  /** unregister() is idempotent and safe when nothing registered. */
  const unregisterTools = useCallback(() => {
    toolsRef.current?.unregister();
    toolsRef.current = null;
    setWebmcp("unavailable");
    setTools([]);
  }, []);
  useEffect(() => () => toolsRef.current?.unregister(), []);
  return { webmcp, tools, registerTools, unregisterTools };
}
