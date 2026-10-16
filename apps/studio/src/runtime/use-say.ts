import { useCallback, useState, type MutableRefObject } from "react";

import type { QuestionItem } from "./needs-you";
import type { RuntimeClient } from "./client";
import { messageOf } from "./message-of";

/** What the message boxes need from App: the client, the run on screen, and its reload. */
export interface SayInputs {
  clientRef: MutableRefObject<RuntimeClient | null>;
  selected: string;
  selectedRef: MutableRefObject<string>;
  loadExecution: (id: string) => Promise<unknown>;
}

/** Saying something into the selected run, from any of its message boxes (#1413, moved out of
 * App.tsx unchanged): the send, its per-box busy and error, and who the next message is aimed at. */
export function useSay({ clientRef, selected, selectedRef, loadExecution }: SayInputs) {
  /** Which composer is mid-send / holds the failure — "run", "talk" or "agent" — so an error
   * lands only in the box that produced it. `null` means idle / no failure. */
  const [saying, setSaying] = useState<string | null>(null);
  const [sayError, setSayError] = useState<{ via: string; text: string } | null>(null);
  /** Bumped when an attention action asks the message box to take the cursor. A nonce rather
   * than a boolean: the second click must focus again even though the panel is already open. */
  const [sayFocusNonce, setSayFocusNonce] = useState(0);
  /** Who that action wants the message addressed to — the asker of the pending question, or
   * `null` for the room. Set BEFORE the nonce bumps, so the composer retargets every time:
   * five independent reviewers found the answer shipping as a DM to a stale recipient. */
  const [sayTo, setSayTo] = useState<string | null>(null);
  /** The question the next message ANSWERS, when the attention block chose the target. The
   * ledger retires a debt only via a reply whose replyTo names the question's signalId — and
   * the banner's own button used to send none, leaving the debt immortal from this UI (two
   * blind round-4 reviewers). Rides only while the recipient is still that asker. */
  const [sayAnswer, setSayAnswer] = useState<{ asker: string; signalId: string | null } | null>(
    null,
  );
  /** A run switch drops everything a message box held for the last run: its recipient, the
   * question it answered, an in-flight send's busy display and a failure. */
  const resetSay = useCallback(() => {
    setSayTo(null);
    setSayAnswer(null);
    setSaying(null);
    setSayError(null);
  }, []);
  /** Says something into the selected run. Its own busy/error pair rather than the shared ones —
   * and TAGGED BY SURFACE: with the run panel and a chat column open side by side, one shared
   * error painted "could not be sent" into every composer at once, so the operator could not tell
   * which of their messages actually failed (round-3 markets). */
  const say = useCallback(
    async (
      message: string,
      to: string | null = null,
      via: string = "run",
      replyTo: string | null = null,
      kind: "operator_note" | "actor_alias" | "owner_refusal" = "operator_note",
      task: string | null = null,
    ): Promise<boolean> => {
      const client = clientRef.current;
      if (!client || selected === "") return false;
      // The run this send belongs to. Its completion may only touch the say state while that
      // run is still on screen: the rail stays selectable during a send, and an unconditional
      // finally cleared `saying` under the NEXT run - whose box then read the busy-to-idle
      // edge as ITS delivery and deleted a draft that was never sent (PR #467 review, P1).
      const forRun = selected;
      setSaying(via);
      setSayError(null);
      const fail = (text: string) => {
        if (clientRef.current === client && selectedRef.current === forRun) {
          setSayError({ via, text });
        }
      };
      try {
        const evidence = await client.signal(selected, message, {
          ...(to === null ? {} : { to }),
          // The receipt the ledger settles by. Without it, an answer sent through the banner's
          // own button never retired the question it answered.
          ...(replyTo === null ? {} : { replyTo }),
          ...(task === null ? {} : { task }),
          ...(kind === "operator_note" ? {} : { kind }),
        });
        if (evidence.result === "refused") {
          fail(evidence.diagnostics[0]?.message ?? "The Runtime refused the message.");
          return false;
        }
        if (evidence.result === "unknown") {
          // Deliberately not reported as sent. The append may well have landed, and saying "sent"
          // about something unverified is the one error that cannot be walked back later.
          fail(
            "The Runtime accepted this but it could not be confirmed in the log. Reload before sending it again.",
          );
          await loadExecution(selected);
          return false;
        }
        await loadExecution(selected);
        return true;
      } catch (reason) {
        fail(messageOf(reason, "The message could not be sent."));
        return false;
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setSaying(null);
      }
    },
    [loadExecution, selected],
  );

  /** Spec 4.1: the owner names a bot. Recorded, so every Studio on this run reads the same name. */
  const nameBot = useCallback(
    (actorId: string, displayName: string) =>
      say(JSON.stringify({ protocol: "graphhelm-actor-alias-v1", displayName }), actorId, "chat", null, "actor_alias"),
    [say],
  );
  /** Spec 4.3: a final no to one question. Its replyTo retires the question like an answer does. */
  const refuse = useCallback(
    async (item: QuestionItem, reason?: string): Promise<boolean> => {
      if (item.signalId === null) return false;
      const trimmed = reason?.trim();
      return say(JSON.stringify({ protocol: "graphhelm-owner-refusal-v1", ...(trimmed ? { reason: trimmed } : {}) }),
        item.asker, "chat", item.signalId, "owner_refusal");
    },
    [say],
  );
  return {
    saying, sayError, sayFocusNonce, setSayFocusNonce, sayTo, setSayTo, sayAnswer, setSayAnswer,
    resetSay, say, nameBot, refuse,
  };
}
