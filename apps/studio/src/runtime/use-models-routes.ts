import { useCallback, useRef, useState, type MutableRefObject } from "react";

import type { RouteChoice } from "../components/compose";
import type { KeyDraft, ProbeState, RouteDraft, SaveOutcome } from "../components/models";
import type { RuntimeClient } from "./client";
import { messageOf } from "./message-of";

/** What the models screen needs from App: the client and the connection generation it checks. */
export interface ModelsRoutesInputs {
  clientRef: MutableRefObject<RuntimeClient | null>;
  connectionGeneration: MutableRefObject<number>;
}

/** The Runtime's model routes, the judge route a reply suggestion uses, and the models screen's
 * writes and checks (#1171; moved out of App.tsx unchanged). */
export function useModelsRoutes({ clientRef, connectionGeneration }: ModelsRoutesInputs) {
  const [judgeRoute, setJudgeRoute] = useState("");
  /** The Runtime's own model list, read once per draft. `null` while it is being read. */
  const [routes, setRoutes] = useState<RouteChoice | null>(null);
  const judgeRoutes = routes?.routes.filter((route) =>
    route.enabled && route.provider === "typesafe" && route.transport === "direct_api",
  ) ?? [];
  const activeJudgeRoute = judgeRoutes.some((route) => route.id === judgeRoute)
    ? judgeRoute
    : judgeRoutes.length === 1 ? judgeRoutes[0].id : null;
  // #1171: the models screen. `probes` starts empty on purpose - a route nobody has checked is
  // rendered "not checked", never green, because a dot that started green would be a claim
  // nobody measured.
  const [modelsOpen, setModelsOpen] = useState(false);
  const [modelsBusy, setModelsBusy] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [probes, setProbes] = useState<Record<string, ProbeState>>({});
  const modelsSaveGeneration = useRef(0);
  const probeGeneration = useRef(new Map<string, number>());
  /** A new connection's routes are unchecked: no probe state, no probe in flight counts. */
  const clearProbes = useCallback(() => {
    setProbes({});
    probeGeneration.current.clear();
  }, []);
  /** A disconnect forgets the models screen with the Runtime it described. */
  const resetModels = useCallback(() => {
    setRoutes(null);
    setModelsBusy(false);
    setModelsError("");
    setProbes({});
    probeGeneration.current.clear();
  }, []);
  // #1171: the three writes the models screen makes. Each one re-reads the listing afterwards
  // rather than patching local state from the reply: the manifest is a FILE the Runtime owns and
  // another operator (or a CLI in another window) can have moved it between two of these calls.
  const refreshRoutes = useCallback(async () => {
    const client = clientRef.current;
    if (!client) return;
    try {
      const listing = await client.listRoutes();
      if (clientRef.current === client) setRoutes(listing);
    } catch {
      // A listing that cannot be re-read leaves the previous one on screen. It is stale, and the
      // next successful read replaces it; inventing an empty list here would report that the
      // operator just deleted every provider they have.
    }
  }, []);

  const saveModels = useCallback(async (draft: RouteDraft, key: KeyDraft | null): Promise<SaveOutcome> => {
    const client = clientRef.current;
    if (!client) return "route_saved_key_failed";
    const generation = connectionGeneration.current;
    const saveGeneration = ++modelsSaveGeneration.current;
    const isCurrentSave = () =>
      clientRef.current === client &&
      connectionGeneration.current === generation &&
      modelsSaveGeneration.current === saveGeneration;
    probeGeneration.current.set(draft.id, (probeGeneration.current.get(draft.id) ?? 0) + 1);
    for (const routeId of key?.usableBy ?? []) {
      probeGeneration.current.set(routeId, (probeGeneration.current.get(routeId) ?? 0) + 1);
    }
    setModelsBusy(true);
    setModelsError("");
    try {
      await client.setRoute(draft);
      // A route may have landed before the connection changed, but a requested key that was not
      // sent is never a saved operation. Keep the draft retryable and do not let this completion
      // paint the replacement Runtime's Models panel.
      if (!isCurrentSave()) return "route_saved_key_failed";
      if (key !== null) {
        try {
          await client.setCredential(key);
        } catch {
          if (!isCurrentSave()) return "route_saved_key_failed";
          await refreshRoutes();
          if (!isCurrentSave()) return "route_saved_key_failed";
          setProbes({});
          setModelsError("The route was saved, but the API key was not stored. Retry to store it.");
          return "route_saved_key_failed";
        }
      }
      if (!isCurrentSave()) return "route_saved_key_failed";
      await refreshRoutes();
      if (!isCurrentSave()) return "route_saved_key_failed";
      setProbes({});
      return "saved";
    } catch (error) {
      if (isCurrentSave()) {
        setModelsError(messageOf(error, "The model could not be saved."));
      }
      throw error;
    } finally {
      if (isCurrentSave()) setModelsBusy(false);
    }
  }, [refreshRoutes]);

  const probeRoute = useCallback(async (routeId: string) => {
    const client = clientRef.current;
    if (!client) return;
    const generation = connectionGeneration.current;
    const requestGeneration = (probeGeneration.current.get(routeId) ?? 0) + 1;
    probeGeneration.current.set(routeId, requestGeneration);
    setProbes((current) => ({ ...current, [routeId]: { state: "checking" } }));
    try {
      const reply = await client.probeRoute(routeId);
      if (clientRef.current !== client || connectionGeneration.current !== generation || probeGeneration.current.get(routeId) !== requestGeneration) return;
      setProbes((current) => ({
        ...current,
        [routeId]:
          reply.health === "available"
            ? { state: "available" }
            : { state: "refused", message: reply.health },
      }));
    } catch (error) {
      if (clientRef.current !== client || connectionGeneration.current !== generation || probeGeneration.current.get(routeId) !== requestGeneration) return;
      setProbes((current) => ({
        ...current,
        [routeId]: { state: "refused", message: messageOf(error, "the check could not run") },
      }));
    }
  }, []);
  return {
    routes, setRoutes, judgeRoutes, activeJudgeRoute, setJudgeRoute,
    modelsOpen, setModelsOpen, modelsBusy, modelsError, setModelsError, probes, setProbes,
    clearProbes, resetModels, saveModels, probeRoute,
  };
}
