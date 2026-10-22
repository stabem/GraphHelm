import type { RunSummary } from "../runtime/client";
import { routeSearch, type View } from "../routing/route";

interface Props {
  project: string;
  runs: RunSummary[];
  selected: string | null;
  view: View;
  live: boolean;
  onSelect: (run: string) => void;
}

/** The left rail of the design (Sidebar.dc.html): brand, live state, the project and its runs. */
export function Sidebar({ project, runs, selected, view, live, onSelect }: Props) {
  return (
    <nav aria-label="Projects" className="sidebar">
      <div className="brand">
        <span aria-hidden="true" className="brand-mark">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#0a0a0b" strokeWidth="2.5"><path d="M3 12h4l3-8 4 16 3-8h4" /></svg>
        </span>
        <strong>GraphHelm</strong>
        <span className={live ? "live" : "live off"}>{live ? "● LIVE" : "● OFFLINE"}</span>
      </div>
      <span className="section-label">PROJECTS</span>
      <div className="project">
        <div className="project-head">
          <svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#f5f5f6" strokeWidth="2"><path d="M3 7h6l2 2h10v10H3z" /></svg>
          <strong>{project}</strong>
          <span className={live ? "dot on" : "dot"} />
        </div>
        <ul className="runs" aria-label={`Runs in ${project}`}>
          {runs.length === 0 ? <li className="empty">no run</li> : runs.map((run) => (
            <li key={run.executionId}>
              <a
                href={routeSearch({ run: run.executionId, view }) || "?"}
                aria-current={run.executionId === selected ? "page" : undefined}
                onClick={(event) => {
                  event.preventDefault();
                  onSelect(run.executionId);
                }}
              >
                {run.executionId}
              </a>
            </li>
          ))}
        </ul>
      </div>
    </nav>
  );
}
