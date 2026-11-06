import { useId, useState, type KeyboardEvent } from "react";

import type { StepDot } from "./step-dot";

/** #2532: a row of step dots. Each dot is one focus target: a link when it has one, else a mark.
 * Only the live dot (in progress or stalled) is in the tab order. Hover or focus shows the tooltip,
 * which the focus target describes; the aria-label carries the same text. */
export function StepDots({ dots, label }: { dots: StepDot[]; label: string }) {
  const base = useId();
  // #2548: roving tabindex. One dot per row is in the tab order (the live one, else the first);
  // ArrowLeft and ArrowRight move focus between the dots, Home and End go to the ends.
  const live = Math.max(0, dots.findIndex((dot) => dot.state === "in progress" || dot.state === "stalled"));
  // #2567: the tab stop is the dot that has focus, so leaving and re-entering the row returns to it.
  const [focused, setFocused] = useState<number | null>(null);
  const stop = focused !== null && focused < dots.length ? focused : live;
  const move = (event: KeyboardEvent<HTMLElement>, from: number) => {
    const to = event.key === "ArrowRight" ? from + 1 : event.key === "ArrowLeft" ? from - 1 : event.key === "Home" ? 0 : event.key === "End" ? dots.length - 1 : -1;
    if (to < 0 || to >= dots.length) return;
    event.preventDefault();
    const row = event.currentTarget.closest(".step-dots");
    row?.querySelectorAll<HTMLElement>("[data-dot]")[to]?.focus();
  };
  return (
    <span role="group" aria-label={label} className="step-dots">
      {dots.map((dot, index) => {
        const tip = `${base}-tip-${index}`;
        const cls = `step-dot ${dot.state.replace(/ /g, "-")}${dot.state === "in progress" ? " curstep" : ""}`;
        const common = { className: cls, "aria-label": dot.text, "aria-describedby": tip, tabIndex: index === stop ? 0 : -1, "data-dot": "", onFocus: () => setFocused(index), onKeyDown: (event: KeyboardEvent<HTMLElement>) => move(event, index) };
        const safe = dot.href !== null && /^https:\/\//.test(dot.href);
        const withRecord = safe && dot.record !== null && dot.text.endsWith(dot.record);
        return (
          <span key={dot.step} className="step-dot-wrap">
            {safe ? <a href={dot.href!} {...common} /> : <span role="img" {...common} />}
            <span role="tooltip" id={tip} className="step-dot-tip">
              {withRecord ? (
                <>{dot.text.slice(0, dot.text.length - dot.record!.length)}<a href={dot.href!} tabIndex={-1}>{dot.record}</a></>
              ) : dot.text}
            </span>
          </span>
        );
      })}
    </span>
  );
}
