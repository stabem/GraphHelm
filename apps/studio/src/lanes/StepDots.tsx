import { useId } from "react";

import type { StepDot } from "./step-dot";

/** #2532: a row of step dots. Each dot is one focus target: a link when it has one, else a mark.
 * Only the live dot (in progress or stalled) is in the tab order. Hover or focus shows the tooltip,
 * which the focus target describes; the aria-label carries the same text. */
export function StepDots({ dots, label }: { dots: StepDot[]; label: string }) {
  const base = useId();
  return (
    <span role="group" aria-label={label} className="step-dots">
      {dots.map((dot, index) => {
        const tip = `${base}-tip-${index}`;
        const cls = `step-dot ${dot.state.replace(/ /g, "-")}${dot.state === "in progress" ? " curstep" : ""}`;
        const common = { className: cls, "aria-label": dot.text, "aria-describedby": tip, tabIndex: 0 };
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
