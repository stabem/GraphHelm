/**
 * Settings → Models: the provider cards, and the only surface that can add one.
 *
 * Until #1171 the Studio could LIST the Runtime's routes and start a task on one, and an operator
 * who wanted a new provider had to leave the page, hand-edit a manifest and run a CLI command with
 * six flags. This screen is the write: base URL, model, enabled, and the API key, each landing on
 * `PUT /v1/gateway/routes` and `PUT /v1/gateway/credentials/{reference}`.
 *
 * THREE PROPERTIES THIS SURFACE HOLDS, and each one is a thing it deliberately does NOT do:
 *
 * - **The key is write-only, here as in the broker.** No stored value is ever rendered, because
 *   none is ever served: the Runtime has no read path for a credential at all. The field says a
 *   new value replaces the old one, which is the whole truth about what typing in it does.
 * - **"Configured" is probed, never assumed.** The listing carries no credential reference, so
 *   this screen cannot know from it whether a route has a key. `GET /v1/gateway/probe` can: it
 *   leases the credential out of the broker and reports `available`, or says `auth_required` when
 *   nothing leases. It places no model call, so a green dot here means the KEY works, never that
 *   the provider answered — and the screen says so rather than letting a green dot imply it.
 * - **A `native_runtime` route is shown and not edited.** It names a CLI that owns its own
 *   authentication and carries no credential; the write refuses it by name, and a screen that
 *   offered an API key field for one would be offering to key something that has no key.
 */

import { useState, type FormEvent } from "react";
import { KeyRound, Plus, TriangleAlert } from "lucide-react";

import type { ModelRouteSummary } from "../runtime/types";
import type { RouteChoice } from "./compose";

/** What one card sends when the operator applies it. Absent fields are absent on purpose: the
 * Runtime owns every default, and a screen that filled them in would be a second place they are
 * decided. */
export interface RouteDraft {
  id: string;
  provider: string;
  baseUrl: string;
  model: string;
  enabled: boolean;
  /** True for a card that edits a route the listing already holds. */
  replace: boolean;
  credentialRef?: string;
  profiles?: string[];
}

/** The API key for one route, on its way to the broker. `reference` is the broker name; the
 * Runtime defaults it to `secret_<route id>` when a route is written without one, so this screen
 * uses the same shape rather than inventing a second convention. */
export interface KeyDraft {
  reference: string;
  provider: string;
  usableBy: string[];
  value: string;
}

/** What a probe said about one route, as this screen renders it. `unknown` is the honest initial
 * state: nobody has asked yet, and a dot that started green would be a claim nobody measured. */
export type ProbeState =
  | { state: "unknown" }
  | { state: "checking" }
  | { state: "available" }
  | { state: "refused"; message: string };

export type SaveOutcome = "saved" | "route_saved_key_failed";

const WIRE_FORMATS = ["openai", "anthropic", "typesafe"] as const;

function freshCredentialRef(): string | undefined {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi?.randomUUID) return `secret_route_${cryptoApi.randomUUID()}`;
  if (cryptoApi?.getRandomValues) {
    const bytes = cryptoApi.getRandomValues(new Uint8Array(16));
    return `secret_route_${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  }
  return undefined;
}

function blankDraft(): RouteDraft {
  return {
    id: "",
    provider: "openai",
    baseUrl: "",
    model: "",
    enabled: true,
    replace: false,
    credentialRef: freshCredentialRef(),
  };
}

function draftOf(route: ModelRouteSummary): RouteDraft {
  return {
    id: route.id,
    provider: route.provider,
    baseUrl: route.baseUrl ?? "",
    model: route.model ?? "",
    enabled: route.enabled,
    replace: true,
    credentialRef: route.credentialRef ?? undefined,
    profiles: route.profiles,
  };
}

/** The dot beside a route, and the sentence under it. Both come from the same probe, and neither
 * ever says more than the probe measured. */
function ProbeBadge({ probe, transport }: { probe: ProbeState; transport: string }) {
  const label =
    probe.state === "available" && transport === "native_runtime"
      ? "native command succeeded; no broker lease or provider login measured"
      : probe.state === "available"
      ? "the key leases from the broker"
      : probe.state === "refused"
        ? probe.message
        : probe.state === "checking"
          ? "checking"
          : "not checked";
  return (
    <span className={`probe ${probe.state}`} role="status" aria-label={`probe: ${label}`}>
      {label}
    </span>
  );
}

export function Models({
  choice,
  busy,
  error,
  probes,
  onApply,
  onProbe,
  onClose,
}: {
  choice: RouteChoice | null;
  busy: boolean;
  error: string;
  probes: Record<string, ProbeState>;
  onApply: (draft: RouteDraft, key: KeyDraft | null) => Promise<SaveOutcome>;
  onProbe: (routeId: string) => void;
  onClose: () => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<RouteDraft>(blankDraft);
  const [key, setKey] = useState("");
  const [validationError, setValidationError] = useState("");
  const [openedBaseline, setOpenedBaseline] = useState<
    { provider: string; baseUrl: string; credentialRef?: string } | null
  >(null);
  const [partialBaseline, setPartialBaseline] = useState<
    { provider: string; baseUrl: string; credentialRef?: string } | null
  >(null);

  const routes = choice?.routes ?? [];
  const open = (route: ModelRouteSummary | null) => {
    setEditing(route === null ? "" : route.id);
    setDraft(route === null ? blankDraft() : draftOf(route));
    setOpenedBaseline(
      route === null
        ? null
        : { provider: route.provider, baseUrl: route.baseUrl ?? "", credentialRef: route.credentialRef ?? undefined },
    );
    setPartialBaseline(null);
    setValidationError("");
    // The key box is cleared on every open. A value left in it from the card before would be one
    // keystroke away from landing on a different route's reference.
    setKey("");
  };

  const apply = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setValidationError("");
    const trimmed = {
      ...draft,
      id: draft.id.trim(),
      provider: draft.provider.trim(),
      baseUrl: draft.baseUrl.trim(),
      model: draft.model.trim(),
    };
    if (trimmed.id === "" || trimmed.baseUrl === "" || trimmed.model === "") return;
    const baseline = trimmed.replace ? routes.find((route) => route.id === trimmed.id) : undefined;
    if (trimmed.replace) {
      if (partialBaseline !== null) {
        const liveMatches = (
          expected: { provider: string; baseUrl: string; credentialRef?: string },
        ) =>
          baseline !== undefined &&
          baseline.transport === "direct_api" &&
          baseline.provider === expected.provider &&
          (baseline.baseUrl ?? "") === expected.baseUrl &&
          baseline.credentialRef === expected.credentialRef;
        const liveIsExpected =
          liveMatches(partialBaseline) ||
          (openedBaseline !== null && liveMatches(openedBaseline)) ||
          (openedBaseline === null && baseline === undefined);
        if (
          trimmed.provider !== partialBaseline.provider ||
          trimmed.baseUrl !== partialBaseline.baseUrl ||
          trimmed.credentialRef !== partialBaseline.credentialRef ||
          !liveIsExpected
        ) {
          setValidationError(
            "Retry stopped because this route or endpoint changed. Close it and start the edit again.",
          );
          return;
        }
      } else if (
        openedBaseline === null ||
        baseline === undefined ||
        baseline.transport !== "direct_api" ||
        baseline.provider !== openedBaseline.provider ||
        (baseline.baseUrl ?? "") !== openedBaseline.baseUrl ||
        baseline.credentialRef !== openedBaseline.credentialRef ||
        trimmed.provider !== openedBaseline.provider
      ) {
        setValidationError(
          "This route changed while it was open. Close it and start the edit again.",
        );
        return;
      }
    }
    const baseUrlChanged =
      partialBaseline === null && openedBaseline !== null && openedBaseline.baseUrl !== trimmed.baseUrl;
    const hasKey = key.trim() !== "";
    if ((!trimmed.replace || baseUrlChanged || partialBaseline !== null) && !hasKey) {
      setValidationError(
        baseUrlChanged
          ? "Changing an existing route to a different endpoint needs a new API key. Add a new route for a different endpoint instead."
          : "A new model needs an API key before it can be saved.",
      );
      return;
    }
    const credentialRef = partialBaseline !== null
      ? partialBaseline.credentialRef
      : (!trimmed.replace && trimmed.credentialRef === undefined) ||
          (baseUrlChanged &&
            (trimmed.credentialRef === undefined || trimmed.credentialRef === openedBaseline?.credentialRef))
        ? freshCredentialRef()
        : trimmed.credentialRef;
    if (hasKey && credentialRef === undefined) {
      setValidationError("This Runtime cannot create a fresh credential reference for this key. Try again on a secure browser.");
      return;
    }
    const routeToSave = hasKey ? { ...trimmed, credentialRef } : trimmed;
    const keyDraft = !hasKey ? null : {
      reference: routeToSave.credentialRef as string,
      provider: trimmed.provider,
      usableBy: [trimmed.id],
      value: key.trim(),
    };
    try {
      const outcome = await onApply(routeToSave, keyDraft);
      if (outcome === "route_saved_key_failed") {
        setPartialBaseline({
          provider: routeToSave.provider,
          baseUrl: routeToSave.baseUrl,
          credentialRef: routeToSave.credentialRef,
        });
        setDraft({ ...routeToSave, replace: true });
        return;
      }
      setPartialBaseline(null);
    } catch {
      return;
    }
    // The key is cleared only after the whole ordered operation confirms success. On refusal it
    // stays in the controlled field so the operator can see the draft and retry without losing it.
    setKey("");
    setEditing(null);
  };

  return (
    <section className="panel" aria-label="Models">
      <header className="panel-head calm">
        <i aria-hidden="true" />
        <div style={{ minWidth: 0 }}>
          <h2>Models</h2>
          <p className="lbl">the providers this Runtime can reach</p>
        </div>
        <button type="button" className="ghost close" onClick={onClose} aria-label="Close models">
          close
        </button>
      </header>

      {choice !== null && !choice.configured && (
        <p className="hint" role="status">
          This Runtime was started without a gateway manifest, so there is nowhere to write a route.
          Restart <code>graphhelm serve</code> with one, and this screen can fill it.
        </p>
      )}

      <ul className="models">
        {routes.map((route) => (
          <li key={route.id} className={route.enabled ? "model" : "model off"}>
            <div className="model-head">
              <strong>{route.id}</strong>
              <span className="lbl">
                {route.provider}
                {route.model === null ? "" : ` · ${route.model}`}
                {route.enabled ? "" : " · disabled"}
              </span>
              <ProbeBadge probe={probes[route.id] ?? { state: "unknown" }} transport={route.transport} />
              <button
                type="button"
                className="ghost"
                disabled={busy}
                onClick={() => onProbe(route.id)}
              >
                check
              </button>
              {route.transport === "direct_api" ? (
                <button
                  type="button"
                  className="ghost"
                  disabled={busy}
                  onClick={() => open(route)}
                  aria-label={`Edit ${route.id}`}
                >
                  edit
                </button>
              ) : (
                <span className="lbl" aria-label={`${route.id} is a command route`}>
                  a command route — native success is not a broker lease or provider login
                </span>
              )}
            </div>
          </li>
        ))}
      </ul>

      {editing === null ? (
        <button
          type="button"
          className="ghost add-model"
          disabled={busy || (choice !== null && !choice.configured)}
          onClick={() => open(null)}
        >
          <Plus aria-hidden="true" />
          Add model
        </button>
      ) : (
        <form className="composer" onSubmit={apply} aria-label="Edit route">
          <label className="lbl" htmlFor="models-id">
            Route id
          </label>
          <input
            id="models-id"
            value={draft.id}
            disabled={busy || draft.replace}
            onChange={(event) => setDraft({ ...draft, id: event.target.value })}
          />

          <label className="lbl" htmlFor="models-provider">
            Wire format
          </label>
          <select
            id="models-provider"
            value={draft.provider}
            disabled={busy || draft.replace}
            onChange={(event) => setDraft({ ...draft, provider: event.target.value })}
          >
            {WIRE_FORMATS.map((format) => (
              <option key={format} value={format}>
                {format}
              </option>
            ))}
          </select>
          <p className="lbl">
            The format the adapter speaks, not the vendor. A DeepSeek endpoint is an
            <code> openai</code> route with its own base URL.
          </p>

          <label className="lbl" htmlFor="models-base-url">
            Base URL
          </label>
          <input
            id="models-base-url"
            value={draft.baseUrl}
            placeholder="https://api.deepseek.com"
            disabled={busy}
            onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })}
          />

          <label className="lbl" htmlFor="models-model">
            Model
          </label>
          <input
            id="models-model"
            value={draft.model}
            disabled={busy}
            onChange={(event) => setDraft({ ...draft, model: event.target.value })}
          />

          <label className="lbl" htmlFor="models-key">
            <KeyRound aria-hidden="true" /> API key
          </label>
          <input
            id="models-key"
            type="password"
            value={key}
            autoComplete="off"
            placeholder="enter a new value to replace the stored one"
            disabled={busy}
            onChange={(event) => setKey(event.target.value)}
          />
          <p className="lbl">
            Stored sealed in the Runtime's broker, usable by this route alone. Nothing reads it
            back — not this page, not the Runtime, not the audit.
          </p>

          <label className="lbl" htmlFor="models-enabled">
            <input
              id="models-enabled"
              type="checkbox"
              checked={draft.enabled}
              disabled={busy}
              onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })}
            />
            Enabled
          </label>

          {(validationError || error) !== "" && (
            <p className="notice bad" role="alert">
              <TriangleAlert aria-hidden="true" />
              <span>{validationError || error}</span>
            </p>
          )}

          <div className="model-actions">
            <button type="button" className="ghost" disabled={busy} onClick={() => setEditing(null)}>
              cancel
            </button>
            <button
              type="submit"
              className="send"
              disabled={
                busy ||
                draft.id.trim() === "" ||
                draft.baseUrl.trim() === "" ||
                draft.model.trim() === ""
              }
            >
              {busy ? "applying" : "apply"}
            </button>
          </div>
        </form>
      )}

      <p className="panel-foot">
        A check leases this route's key out of the broker and places no model call, so it spends
        nothing — and a green check says the key works, never that the provider answered.
      </p>
    </section>
  );
}
