import { createContext, useContext, useEffect, useState } from "react";

import type { AttentionReason, Runtime } from "../runtime/client";
import { cliRemedy, reasonText } from "./attention";

/** What `useRunWaits` knows: the reasons (empty when the run waits on nothing), `"reading"` before
 * the first read lands, `"failed"` when it did not. Neither of the last two is "nothing" (#2348). */
export type RunWaits = AttentionReason[] | "reading" | "failed";

/** #2348: the reasons per execution, given instead of read. Tests render Graph and Lanes as they
 * ship through it, so removing their wiring turns a test red; the app never provides it. */
export const RunWaitsGiven = createContext<Readonly<Record<string, RunWaits>> | null>(null);

/** #2324: what a run waits on, read from its status (`attentionReasons`) each time it moves. */
export function useRunWaits(runtime: Runtime | null, executionId: string | null, moved: string | null): RunWaits {
  const given = useContext(RunWaitsGiven);
  const [reasons, setReasons] = useState<RunWaits>("reading");
  useEffect(() => {
    if (given !== null) return;
    if (runtime === null || executionId === null) { setReasons("reading"); return; }
    let live = true;
    runtime.runAttention(executionId).then(
      (status) => { if (live) setReasons(status.attention === "needs_you" ? status.attentionReasons ?? [] : []); },
      () => { if (live) setReasons("failed"); },
    );
    return () => { live = false; };
  }, [given, runtime, executionId, moved]);
  if (given !== null) return executionId === null ? "reading" : given[executionId] ?? "reading";
  return reasons;
}

/** One reason as the approved design says it (docs/design/studio/2325-2324/): what waits, and for
 * a blocked node the CLI line that approves it, since node approval has no Studio surface yet. */
export function WaitsOn({ reason, executionId }: { reason: AttentionReason; executionId: string }) {
  const remedy = cliRemedy(reason, executionId);
  return <>{reasonText(reason)}{remedy !== null && <> Approve it from the CLI: <code>{remedy}</code></>}</>;
}
