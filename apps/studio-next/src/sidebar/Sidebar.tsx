import type { RunSummary } from "../runtime/client";
import { useState } from "react";

import type { AddRefusal } from "../login/session";
import { routeSearch, type View } from "../routing/route";
import { subtitle, type ProjectSummary } from "./project-summary";

export interface ProjectEntry {
  key: string;
  name: string;
  /** Display form of the project's folder (#1833), shown only to the owner holding its token. */
  folder: string | null;
  summary: ProjectSummary | null;
}

const REFUSALS: Record<AddRefusal, string> = {
  not_a_project: "That folder is not a GraphHelm project.",
  runtime_not_running: "That project's Runtime is not running. Run graphhelm studio open there first.",
  refused: "The Studio refused to add that folder.",
};

/** The two subtitle lines under a project: folder, then run and journeys. */
function Subtitles({ project }: { project: ProjectEntry }) {
  const lines = project.summary === null ? null : subtitle(project.summary);
  return (
    <>
      {project.folder !== null && <span className="project-folder mono">{project.folder}</span>}
      {lines !== null && (
        <span className="project-counts">
          <span>{lines.run}</span>
          {lines.journeys !== null && <span>{lines.journeys}</span>}
        </span>
      )}
    </>
  );
}

/** "Add project folder": one absolute path, answered by the dev server's closed list of refusals. */
function AddFolder({ onAdd }: { onAdd: (path: string) => Promise<AddRefusal | null> }) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (!open) return <button type="button" className="add-folder" aria-label="Add project folder" onClick={() => setOpen(true)}>+</button>;
  return (
    <form
      className="add-folder-form"
      aria-label="Add project folder"
      onSubmit={(event) => {
        event.preventDefault();
        if (path.trim().length === 0 || busy) return;
        setBusy(true);
        void onAdd(path.trim()).then((refusal) => {
          setBusy(false);
          setError(refusal === null ? null : REFUSALS[refusal]);
        });
      }}
    >
      <input aria-label="Project folder" className="mono" placeholder="absolute folder path" value={path} onChange={(event) => setPath(event.target.value)} autoFocus />
      <button type="submit" disabled={busy}>Add</button>
      <button type="button" onClick={() => { setOpen(false); setError(null); }}>Cancel</button>
      {error !== null && <span role="alert" className="add-folder-error">{error}</span>}
    </form>
  );
}

interface Props {
  projects: ProjectEntry[];
  /** Joined projects this tab has no key for (#1800): listed, opened only by their own ticket. */
  closed: string[];
  current: string;
  runs: RunSummary[];
  selected: string | null;
  view: View;
  live: boolean;
  onProject: (project: string) => void;
  onSelect: (run: string) => void;
  /** Null when this tab holds no key that proves the owner. */
  onAdd: ((path: string) => Promise<AddRefusal | null>) | null;
}

function Folder({ stroke }: { stroke: string }) {
  return <svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={stroke} strokeWidth="2"><path d="M3 7h6l2 2h10v10H3z" /></svg>;
}

/** The left rail of the design (Sidebar.dc.html): brand, live state, every project this Studio port
 * serves (#1800), and the open project's runs. Another project is a link that switches to it. */
export function Sidebar({ projects, closed, current, runs, selected, view, live, onProject, onSelect, onAdd }: Props) {
  return (
    <nav aria-label="Projects" className="sidebar">
      <div className="brand">
        <span aria-hidden="true" className="brand-mark">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#0a0a0b" strokeWidth="2.5"><path d="M3 12h4l3-8 4 16 3-8h4" /></svg>
        </span>
        <strong>GraphHelm</strong>
        <span className={live ? "live" : "live off"}>{live ? "● LIVE" : "● OFFLINE"}</span>
      </div>
      <div className="section-head">
        <span className="section-label">PROJECTS</span>
        {onAdd !== null && <AddFolder onAdd={onAdd} />}
      </div>
      <div className="projects">
        {projects.map((project) => project.key === current ? (
          <div key={project.key} className="project">
            <div className="project-head">
              <Folder stroke="#f5f5f6" />
              <strong>{project.name}</strong>
              <span className={live ? "dot on" : "dot"} />
            </div>
            <Subtitles project={project} />
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
            <Subtitles project={project} />
          </div>
        ))}
        {closed.map((id) => (
          <div key={id} className="project other closed">
            <Folder stroke="#6b6b76" />
            <span>project {id.slice(0, 8)}</span>
            <span className="closed-hint">open with <code className="mono">graphhelm studio open</code></span>
          </div>
        ))}
      </div>
    </nav>
  );
}
