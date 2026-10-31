import { useEffect, useState } from "react";

import type { AttentionReason, Runtime } from "../runtime/client";
import { cliRemedy, reasonText } from "./attention";

/** #2324: what a run waits on, read from its status (`attentionReasons`) each time it moves.
 * Node approval has no Studio surface yet, so a blocked node names the CLI line that approves it. */
export function RunAttention({ runtime, executionId, moved, onRead }: { runtime: Runtime; executionId: string; moved: string | null; onRead?: (waits: boolean) => void }) {
  const [reasons, setReasons] = useState<AttentionReason[] | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    runtime.runAttention(executionId).then(
      (status) => {
        if (!live) return;
        const listed = status.attention === "needs_you" ? status.attentionReasons ?? [] : [];
        setReasons(listed);
        setFailed(false);
        onRead?.(listed.length > 0);
      },
      () => { if (live) { setFailed(true); onRead?.(false); } },
    );
    return () => { live = false; };
  }, [runtime, executionId, moved, onRead]);
  if (failed) return <p className="muted small">Could not read what this run waits on.</p>;
  if (reasons === null || reasons.length === 0) return null;
  return (
    <section aria-label="Waits on you" className="run-attention">
      <strong className="mono">WAITS ON YOU</strong>
      <ul>
        {reasons.map((reason, index) => {
          const remedy = cliRemedy(reason, executionId);
          return (
            <li key={index}>
              {reasonText(reason)}
              {remedy !== null && <><span className="muted small"> The Studio cannot approve a node yet; approve it from the CLI:</span> <code>{remedy}</code></>}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
