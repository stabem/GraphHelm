/* #636: polls the build-slot queues every SLOT_POLL_MS while mounted (App mounts it only with the
 * Graph/Lanes page). A 403/404 (no owner token, an older Runtime) is ignored quietly: no slots. */
import { useEffect, useState, type ReactNode } from "react";
import type { SlotView } from "../runtime/slots";

export const SLOT_POLL_MS = 10_000;
interface SlotSource { workspaceSlots(): Promise<SlotView[]> }

export function useSlotPoll(client: SlotSource | null, everyMs = SLOT_POLL_MS): SlotView[] {
  const [slots, setSlots] = useState<SlotView[]>([]);
  useEffect(() => {
    if (!client) { setSlots([]); return; }
    let live = true;
    let latestRead = 0;
    setSlots([]);
    const orderedRead = () => {
      const readId = ++latestRead;
      void Promise.resolve().then(() => client.workspaceSlots()).then((s) => {
        if (live && readId === latestRead) setSlots(s);
      }, (e: unknown) => {
        if (!live || readId !== latestRead) return;
        const status = (e as { status?: unknown } | null)?.status;
        if (status === 403 || status === 404) { setSlots([]); return; }
        setSlots((known) => known.map((slot) => ({ root: slot.root, ok: false, errorCodes: [] })));
      });
    };
    orderedRead();
    const id = setInterval(orderedRead, everyMs);
    return () => { live = false; clearInterval(id); };
  }, [client, everyMs]);
  return slots;
}

export function SlotPoll({ client, children }: { client: SlotSource | null; children(slots: SlotView[]): ReactNode }) {
  return <>{children(useSlotPoll(client))}</>;
}
