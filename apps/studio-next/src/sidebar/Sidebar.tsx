import type { RunSummary } from "../runtime/client";
import { routeSearch, type View } from "../routing/route";

export interface ProjectEntry {
  key: string;
  name: string;
}

interface Props {
  projects: ProjectEntry[];
  current: string;
  runs: RunSummary[];
  selected: string | null;
  view: View;
  live: boolean;
  onProject: (project: string) => void;
  onSelect: (run: string) => void;
}

function Folder({ stroke }: { stroke: string }) {
  return <svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={stroke} strokeWidth="2"><path d="M3 7h6l2 2h10v10H3z" /></svg>;
}

/** The left rail of the design (Sidebar.dc.html): brand, live state, every project this Studio port
 * serves (#1800), and the open project's runs. Another project is a link that switches to it. */
export function Sidebar({ projects, current, runs, selected, view, live, onProject, onSelect }: Props) {
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
      <div className="projects">
        {projects.map((project) => project.key === current ? (
          <div key={project.key} className="project">
            <div className="project-head">
              <Folder stroke="#f5f5f6" />
              <strong>{project.name}</strong>
              <span className={live ? "dot on" : "dot"} />
            </div>
            <ul className="runs" aria-label={`Runs in ${project.name}`}>
              {runs.length === 0 ? <li className="empty">no run</li> : runs.map((run) => (
                <li key={run.executionId}>
                  <a
                    href={routeSearch({ project: project.key, run: run.executionId, view }) || "?"}
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
        ) : (
          <div key={project.key} className="project other">
            <a
              aria-label={project.name}
              href={routeSearch({ project: project.key, run: null, view }) || "?"}
              onClick={(event) => {
                event.preventDefault();
                onProject(project.key);
              }}
            >
              <Folder stroke="#a2a2ad" />
              <span>{project.name}</span>
              <span className="dot" />
            </a>
          </div>
        ))}
      </div>
    </nav>
  );
}
