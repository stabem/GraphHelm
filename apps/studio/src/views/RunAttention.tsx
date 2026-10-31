import { useEffect, useState } from "react";

import type { AttentionReason, Runtime } from "../runtime/client";
import { cliRemedy, reasonText } from "./attention";

/** #2324: what a run waits on, read from its status (`attentionReasons`) each time it moves;
 * `null` until read or when the read failed (said by the caller as not known, never as nothing). */
export function useRunWaits(runtime: Runtime | null, executionId: string | null, moved: string | null): AttentionReason[] | null {
  const [reasons, setReasons] = useState<AttentionReason[] | null>(null);
  useEffect(() => {
    if (runtime === null || executionId === null) { setReasons(null); return; }
    let live = true;
    runtime.runAttention(executionId).then(
      (status) => { if (live) setReasons(status.attention === "needs_you" ? status.attentionReasons ?? [] : []); },
      () => { if (live) setReasons(null); },
    );
    return () => { live = false; };
  }, [runtime, executionId, moved]);
  return reasons;
}

/** One reason as the approved design says it (docs/design/studio/2325-2324/): what waits, and for
 * a blocked node the CLI line that approves it, since node approval has no Studio surface yet. */
export function WaitsOn({ reason, executionId }: { reason: AttentionReason; executionId: string }) {
  const remedy = cliRemedy(reason, executionId);
  return <>{reasonText(reason)}{remedy !== null && <> Approve it from the CLI: <code>{remedy}</code></>}</>;
}
