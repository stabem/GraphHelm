import { useState } from "react";

import { When } from "../time/Stamp";
import type { AddRefusal } from "../login/session";
import { routeSearch, type View } from "../routing/route";
import { subtitle, type ProjectSummary } from "./project-summary";
import { restartDetail, type Restart } from "./use-health";

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
  refused: "The Studio refused to add that folder. The ticket may be missing or already used: run graphhelm studio open --project there for a new one.",
};

/** Under a project (Sidebar.dc.html): the open one shows its folder, then its team run and its
 * journeys in an indented box; another shows its folder and its run on one line. */
function Subtitles({ project, open }: { project: ProjectEntry; open: boolean }) {
  const lines = project.summary === null ? null : subtitle(project.summary);
  if (!open) {
    const parts = [project.folder, lines?.run].filter((part): part is string => part != null);
    return parts.length === 0 ? null : <span className="project-folder mono">{parts.join(" · ")}</span>;
  }
  return (
    <>
      {project.folder !== null && <span className="project-folder mono">{project.folder}</span>}
      {lines !== null && (
        <span className="project-counts">
          <span className="project-run">{lines.run}</span>
          {lines.journeys !== null && <span>{lines.journeys}</span>}
        </span>
      )}
    </>
  );
}

/** "Add project folder": one absolute path plus that project's own ticket, answered by the dev server's closed list of refusals. */
function AddFolder({ onAdd, open, setOpen }: { onAdd: (path: string, ticket: string) => Promise<AddRefusal | null>; open: boolean; setOpen: (open: boolean) => void }) {
  const [path, setPath] = useState("");
  const [ticket, setTicket] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (!open) return <button type="button" className="add-folder" aria-label="Add project folder" onClick={() => setOpen(true)}>+</button>;
  return (
    <form
      className="add-folder-form"
      aria-label="Add project folder"
      onSubmit={(event) => {
        event.preventDefault();
        if (path.trim().length === 0 || ticket.trim().length === 0 || busy) return;
        setBusy(true);
        void onAdd(path.trim(), ticket.trim()).then((refusal) => {
          setBusy(false);
          setTicket("");
          setError(refusal === null ? null : REFUSALS[refusal]);
        });
      }}
    >
      <input aria-label="Project folder" className="mono" placeholder="absolute folder path" value={path} onChange={(event) => setPath(event.target.value)} autoFocus />
      {/* #1808: a masked text field, not type=password, so it keeps the textbox role an agent or a screen reader addresses it by. */}
      <input aria-label="Ticket from graphhelm studio open --project" type="text" autoComplete="off" spellCheck={false} className="mono masked" placeholder="ticket" value={ticket} onChange={(event) => setTicket(event.target.value)} />
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
  view: View;
  live: boolean;
  onProject: (project: string) => void;
  /** Null when this tab holds no key that proves the owner. */
  onAdd: ((path: string, ticket: string) => Promise<AddRefusal | null>) | null;
  /** The running Runtime's restart (`/health` previousExit), shown as the design's notice. */
  restart: Restart | null;
  /** Opens a view of the open project: the footer's Runtime log and Models land on Lanes (#1904). */
  onView: (view: View) => void;
}

function Folder({ stroke }: { stroke: string }) {
  return <svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={stroke} strokeWidth="2"><path d="M3 7h6l2 2h10v10H3z" /></svg>;
}

/** The left rail of the design (Sidebar.dc.html): brand, live state, every project this Studio port
 * serves (#1800), one card each (#1901): no run list, the Studio opens the project's team run.
 * Another project is a link that switches to it. */
export function Sidebar({ projects, closed, current, view, live, onProject, onAdd, restart, onView }: Props) {
  const [adding, setAdding] = useState(false);
  return (
    <nav aria-label="Projects" className="sidebar">
      <div className="brand">
        <img alt="" aria-hidden="true" className="brand-mark" src="/favicon.png" />
        <strong>GraphHelm</strong>
        <span className={live ? "live" : "live off"}>{live ? "● LIVE" : "● OFFLINE"}</span>
      </div>
      {restart !== null && (
        <details className="restart" aria-label="Runtime restart">
          <summary>Runtime restarted <When at={restart.startedAt * 1000} /> · Details</summary>
          <p>{restartDetail(restart)}</p>
        </details>
      )}
      <div className="section-head">
        <span className="section-label">PROJECTS</span>
        {onAdd !== null && <AddFolder onAdd={onAdd} open={adding} setOpen={setAdding} />}
      </div>
      <div className="projects">
        {projects.map((project) => project.key === current ? (
          <section key={project.key} className="project" aria-label={`Project ${project.name}`}>
            <div className="project-head">
              <Folder stroke="#f5f5f6" />
              <strong>{project.name}</strong>
              <span className={live ? "dot on" : "dot"} />
            </div>
            <Subtitles project={project} open />
          </section>
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
            <Subtitles project={project} open={false} />
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
      {/* Sidebar.dc.html: the footer, pinned to the bottom of the rail (#1904). Runtime log and
          Models open Lanes, where the live feed and each agent's kind and model are shown. */}
      <div className="sidebar-foot" role="group" aria-label="Studio links">
        <button type="button" className="b foot-link" onClick={() => onView("lanes")}>Runtime log</button>
        <button type="button" className="b foot-link" onClick={() => onView("lanes")}>Models</button>
        {/* Named apart from the "+" button so a journey's "Add project folder" stays one control. */}
        {onAdd !== null && <button type="button" className="b foot-link" aria-label="Add a project folder" onClick={() => setAdding(true)}>Add project folder</button>}
      </div>
    </nav>
  );
}
