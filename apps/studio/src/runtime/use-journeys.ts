import { useCallback, useEffect, useMemo, useState, type MutableRefObject } from "react";

import type { JourneyRunSource } from "../components/journey-flows";
import type { RuntimeClient } from "./client";
import { messageOf } from "./message-of";
import type { JourneyFlowsView, JourneysView } from "./types";

/** What the project's journey reads need from App. */
export interface JourneyInputs {
  clientRef: MutableRefObject<RuntimeClient | null>;
  selectedRef: MutableRefObject<string>;
  selected: string;
  lastJpdSequence: number;
}

/** The project's journeys and journey flows, the owner's actions on them, and the run source the
 * Journey and Graph tabs share (#1389, moved out of App.tsx unchanged). */
export function useJourneys({ clientRef, selectedRef, selected, lastJpdSequence }: JourneyInputs) {
  const client = clientRef.current;
  // Journeys are a project property (#332): one map folded from every run, read from
  // `GET /v1/journeys` and kept across run switches. A run switch or a new jpd record re-reads it.
  // Journey-flow review (#353): the flow sources and the owner's Approve. An approval bumps
  // `flowsRevision`, which re-reads the flows and the journey map its compiled contracts feed.
  const [flowsRevision, setFlowsRevision] = useState(0);
  const [flowsRead, setFlowsRead] = useState<{ view: JourneyFlowsView | null; failure: string | null }>({ view: null, failure: null });
  useEffect(() => {
    if (client === null) return undefined;
    let cancelled = false;
    Promise.resolve().then(() => client.journeyFlows()).then((view) => {
      if (!cancelled) setFlowsRead({ view, failure: null });
    }, (reason: unknown) => {
      if (!cancelled) setFlowsRead((prior) => ({ view: prior.view, failure: prior.view === null ? messageOf(reason, "") : null }));
    });
    return () => { cancelled = true; };
  }, [client, selected, lastJpdSequence, flowsRevision]);
  const approveFlow = useCallback(async (flowId: string) => {
    const client = clientRef.current;
    if (client === null) throw new Error("Not connected to the Runtime.");
    try {
      await client.approveJourneyFlow(flowId);
    } finally {
      setFlowsRevision((revision) => revision + 1);
    }
  }, []);
  // #1658: the owner's Reject, then the flows are re-read so the list shows it.
  const rejectFlow = useCallback(async (flowId: string, note: string) => {
    const client = clientRef.current;
    if (client === null) throw new Error("Not connected to the Runtime.");
    try {
      await client.rejectJourneyFlow(flowId, note);
    } finally {
      setFlowsRevision((revision) => revision + 1);
    }
  }, []);
  const [journeysRead, setJourneysRead] = useState<{ view: JourneysView | null; failed: boolean; failure?: string } | null>(null);
  useEffect(() => {
    if (client === null) return undefined;
    let cancelled = false;
    Promise.resolve().then(() => client.journeys()).then((view) => {
      if (!cancelled) setJourneysRead({ view, failed: false });
    }, (reason: unknown) => {
      const failure = messageOf(reason, "");
      if (!cancelled) setJourneysRead((prior) => prior !== null ? { ...prior, failed: prior.view === null, failure } : { view: null, failed: true, failure });
    });
    return () => { cancelled = true; };
  }, [client, selected, lastJpdSequence, flowsRevision]);
  const journeysView = journeysRead?.view ?? null;
  const journeysFailed = journeysRead?.failed ?? false;
  // #465: Watch plays a flow in a headed browser (#462); the call answers when the play ends, and
  // the sessions read above lights the current step meanwhile.
  // #519: opening a journey runs its test; the flowchart reads the run and its frames from here.
  const journeyRun = useMemo<JourneyRunSource>(() => {
    const client = () => {
      if (clientRef.current === null) throw new Error("Not connected.");
      return clientRef.current;
    };
    return {
      // The selected run is where an approved flow's replay records its proof.
      // `confirm` rides only on the owner's click on a held step (#548); an ordinary start is the
      // three-argument call it always was.
      start: async (flowId, force, confirm) => (confirm === true
        ? client().startJourneyRun(flowId, force, selectedRef.current || undefined, true)
        : client().startJourneyRun(flowId, force, selectedRef.current || undefined)),
      read: async (flowId) => client().journeyRun(flowId),
      screenFrame: async (flowId, screenId) => client().journeyScreenFrame(flowId, screenId),
      liveFrame: async (sessionId, etag) => client().liveFrame(sessionId, etag),
      history: async (flowId) => client().journeyHistory(flowId),
      historyFrame: async (flowId, run, screenId) => client().journeyHistoryFrame(flowId, run, screenId),
    };
  }, []);
  // #518: the owner marks a draft's skipped step safe; the flows are read again to show the mark.
  const markFlowEdgeSafe = useCallback(async (flowId: string, edgeId: string) => {
    const client = clientRef.current;
    if (client === null) throw new Error("Not connected.");
    try {
      await client.markJourneyEdgeSafe(flowId, edgeId);
    } finally {
      setFlowsRevision((revision) => revision + 1);
    }
  }, []);
  const watchFlow = useCallback(async (flowId: string, path?: string) => {
    const client = clientRef.current;
    if (client === null) throw new Error("Not connected.");
    try {
      await client.watchFlow(flowId, path);
    } finally {
      setFlowsRevision((revision) => revision + 1);
    }
  }, []);
  return { flowsRead, journeysView, journeysFailed, approveFlow, rejectFlow, markFlowEdgeSafe, watchFlow, journeyRun };
}
