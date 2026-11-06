import type { StepDot } from "./step-dot";

/** #2532: a row of step dots. Hover or keyboard focus shows the dot's line; the aria-label carries
 * the same text; a dot with a record or link opens it. */
export function StepDots({ dots, label }: { dots: StepDot[]; label: string }) {
  return (
    <span role="group" aria-label={label} className="step-dots">
      {dots.map((dot) => {
        // Only the live dot (in progress or stalled) is in the tab order; the others stay reachable by pointer.
        const live = dot.state === "in progress" || dot.state === "stalled";
        const cls = `step-dot ${dot.state.replace(/ /g, "-")}${dot.state === "in progress" ? " curstep" : ""}`;
        const mark = <span role="img" tabIndex={live ? 0 : -1} aria-label={dot.text} className={cls} />;
        const safe = dot.href !== null && /^https:\/\//.test(dot.href);
        return (
          <span key={dot.step} className="step-dot-wrap">
            {safe ? <a href={dot.href!} aria-label={dot.text}>{mark}</a> : mark}
            <span role="tooltip" aria-hidden="true" className="step-dot-tip">{dot.text}</span>
          </span>
        );
      })}
    </span>
  );
}
