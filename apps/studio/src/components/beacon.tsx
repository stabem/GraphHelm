/**
 * The needs-you beacon and its question cards (spec §4.3). Dark means nothing blocks; unknown is
 * its own colour and its own words. Phase 1 offers no Refuse (owner_refusal is phase 2).
 */
import type { ReactNode } from "react";

import type { BeaconState, DraftItem, NativeRequestItem, NeedsYouItem, QuestionItem, StepItem } from "../runtime/needs-you";

export const NEEDS_YOU_ID = "needs-you";

export function beaconLabel(state: BeaconState): string {
  if (state.kind === "lit") return state.count === 1 ? "1 decision needs you" : `${state.count} decisions need you`;
  if (state.kind === "dark") return "Nothing needs you";
  return state.reason;
}

export function Beacon({ state, onOpen }: { state: BeaconState; onOpen: () => void }) {
  return (
    <button type="button" className={`beacon beacon-${state.kind}`} onClick={onOpen} aria-live="polite">
      <span className="beacon-dot" aria-hidden="true" />
      {beaconLabel(state)}
    </button>
  );
}

export interface QuestionCardsProps {
  items: NeedsYouItem[];
  names: Record<string, string>;
  busy: boolean;
  onChoose: (item: QuestionItem, choice: string) => void;
  onAnswer: (item: QuestionItem) => void;
  onCheck: (item: NativeRequestItem) => void;
  stepActions: (item: StepItem | DraftItem) => ReactNode;
}

function stepSentence(item: StepItem | DraftItem): string {
  if (item.kind === "draft") return "A proposal is waiting for your review.";
  if (item.kind === "waiting_step") return `${item.name} is waiting for your input.`;
  return item.reason === "untriaged_interruption" ? `${item.name} was interrupted and awaits triage.` : `${item.name} is blocked.`;
}

export function QuestionCards({ items, names, busy, onChoose, onAnswer, onCheck, stepActions }: QuestionCardsProps) {
  if (items.length === 0) return null;
  return (
    <section className="question-cards" id={NEEDS_YOU_ID} aria-label="Needs you" tabIndex={-1}>
      {items.map((item) => {
        if (item.kind === "question") {
          const title = `${names[item.asker] ?? item.asker} asks`;
          return (
            <article key={item.key} className="question-card" aria-label={title}>
              <h3>{title}</h3>
              <p className="question-text">{item.text.length > 400 ? `${item.text.slice(0, 400)}…` : item.text}</p>
              <div className="question-actions">
                {item.recommendations.map((choice) => (
                  <button key={choice} type="button" disabled={busy} onClick={() => onChoose(item, choice)}>{choice}</button>
                ))}
                <button type="button" className="question-answer" disabled={busy} onClick={() => onAnswer(item)}>Answer</button>
              </div>
            </article>
          );
        }
        if (item.kind === "native_request") {
          return (
            <article key={item.key} className="question-card" aria-label={`Message to ${item.title}`}>
              <p className="question-text">{item.state === "blocked"
                ? `Your message to ${item.title} was refused. Check the original chat before sending another.`
                : `Your message to ${item.title} was not confirmed. It may not have arrived.`}</p>
              <div className="question-actions"><button type="button" disabled={busy} onClick={() => onCheck(item)}>Check it</button></div>
              <details className="question-details">
                <summary>Details</summary>
                <dl>
                  <dt>Request ID</dt><dd><code>{item.requestId}</code></dd>
                  <dt>Thread ID</dt><dd><code>{item.threadId}</code></dd>
                  <dt>Node ID</dt><dd><code>{item.nodeId}</code></dd>
                  <dt>State</dt><dd>{item.state}</dd>
                  {item.detail && <><dt>Diagnostic</dt><dd>{item.detail}</dd></>}
                </dl>
              </details>
            </article>
          );
        }
        return (
          <article key={item.key} className="question-card" aria-label={stepSentence(item)}>
            <p className="question-text">{stepSentence(item)}</p>
            <div className="question-actions">{stepActions(item)}</div>
          </article>
        );
      })}
    </section>
  );
}
