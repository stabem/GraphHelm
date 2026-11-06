import type { StepDot } from "./step-dot";

/** #2532: a row of step dots. Hover or keyboard focus shows the dot's line; the aria-label carries
 * the same text; a dot with a record or link opens it. */
export function StepDots({ dots, label }: { dots: StepDot[]; label: string }) {
  return (
    <span role="group" aria-label={label} className="step-dots">
      {dots.map((dot) => {
        const mark = <span role="img" tabIndex={0} aria-label={dot.text} className={`step-dot ${dot.state.replace(/ /g, "-")}`} />;
        return (
          <span key={dot.step} className="step-dot-wrap">
            {dot.href === null ? mark : <a href={dot.href} aria-label={dot.text}>{mark}</a>}
            <span role="tooltip" aria-hidden="true" className="step-dot-tip">{dot.text}</span>
          </span>
        );
      })}
    </span>
  );
}
