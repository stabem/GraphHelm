/**
 * The projects rail: folders, and the tasks inside each one.
 *
 * A PROJECT IS A FOLDER AND ITS TASKS. The rail is built for many folders and shows the ones the
 * Studio can reach; the API carries no project dimension yet, so today that is one. Building the
 * shape for many and filling it with one is honest - inventing a hierarchy nothing enforces would
 * not be.
 *
 * ONE CARD PER PROJECT (#1654, D-060). The rail shows each project as one card and no run rows;
 * the card is labelled with the project name and opens the project's work directly (`homeRun`);
 * there is no run history in the UI (#1696).
 *
 * ONE LONG THING PER LINE, and that rule is the whole layout. The folder name owns its line; the
 * task id owns its own. Everything beside them is fixed width, so when the rail narrows the
 * ellipsis lands on a name instead of the layout breaking. The version this replaces put icon,
 * name, count and a labelled button on ONE line inside 248px: the button clipped to "+ new ta" and
 * the rail grew a horizontal scrollbar.
 *
 * THE STATE IS A MARK, NOT A WORD. "can sleep" and "needs you" were text sharing a line with ids
 * long enough to eat them, and arrived as "can sl" and "needs". A 6px square with the word on
 * hover - and in the accessible name - cannot be truncated at any width.
 *
 * Runs are buttons, not links: choosing one changes what this page shows, it does not navigate,
 * and calling it a link would promise a URL that does not exist.
 */

import { useEffect, useState } from "react";
import { Activity, Check, Folder, FolderPlus, Pencil, Plus, X, SlidersHorizontal } from "lucide-react";

import type { ExecutionSummary, RuntimePreviousExit } from "../runtime/types";
import { ago, readable } from "./format";

/** The local calendar day a time falls on, as a sortable key and a label ("Today", "Oct 2"). */
export function dayOf(time: number, now: Date = new Date()): { key: string; label: string } {
  const date = new Date(time);
  const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const dayStart = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const daysAgo = Math.round((midnight - dayStart) / 86_400_000);
  const label = daysAgo === 0 ? "Today" : daysAgo === 1 ? "Yesterday"
    : new Intl.DateTimeFormat(undefined, {
      month: "short", day: "numeric", ...(date.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
    }).format(date);
  return { key, label };
}

function recentFirst(runs: ExecutionSummary[]): ExecutionSummary[] {
  return [...runs].sort((a, b) => {
    const at = a.lastEventAt ? Date.parse(a.lastEventAt) : NaN;
    const bt = b.lastEventAt ? Date.parse(b.lastEventAt) : NaN;
    const timeA = Number.isFinite(at) ? at : -Infinity;
    const timeB = Number.isFinite(bt) ? bt : -Infinity;
    return timeB - timeA || (a.executionId < b.executionId ? -1 : a.executionId > b.executionId ? 1 : 0);
  });
}

function relativeActivity(value: string | null | undefined): string {
  if (!value || !Number.isFinite(Date.parse(value))) return "Time unknown";
  if (dayOf(Date.parse(value)).label === "Yesterday") return "yesterday";
  const age = ago(value);
  if (age === "now") return age;
  if (age.endsWith("m")) return `${age.slice(0, -1)} min ago`;
  if (age.endsWith("h")) return `${age.slice(0, -1)} h ago`;
  if (age.endsWith("d")) return `${age.slice(0, -1)} d ago`;
  return age;
}

export interface Project {
  /** The folder's name. Today: the Runtime's own store. */
  name: string;
  /** The real source folder, when the local session can identify it. */
  path?: string | null;
  runs: ExecutionSummary[];
}

export function ProjectRail({
  projects,
  selected,
  connected,
  stale = false,
  previousExit = null,
  startupTiming = null,
  currentRun = null,
  runtimeDetailsRefreshed = false,
  runtimeDetailsRefreshing = false,
  runtimeDetailsRefreshError = "",
  hasMore,
  busy,
  onSelect,
  onLoadMore,
  onNewTask,
  onAddProject,
  onOpenModels,
  projectName,
  onRenameProject,
  onRefreshRuntimeDetails,
  briefings = {},
  teamRuns,
  selectedPresentation = null,
  onClose,
}: {
  projects: Project[];
  selected: string;
  connected: boolean;
  previousExit?: RuntimePreviousExit | null;
  startupTiming?: { journalVerifyMs: number; lifecycleStartMs: number } | null;
  currentRun?: { instance: string; pid: number; startedAt: number } | null;
  runtimeDetailsRefreshed?: boolean;
  runtimeDetailsRefreshing?: boolean;
  runtimeDetailsRefreshError?: string;
  /** True when background reads have failed repeatedly: the screen may be aging. "live" under
   * a dead Runtime was indistinguishable from a quiet room (round-4, 3am). */
  stale?: boolean;
  hasMore: boolean;
  busy: boolean;
  onSelect: (id: string) => void;
  onLoadMore: () => void;
  onNewTask: () => void;
  onAddProject: () => void;
  /** #1171: the models screen. It sits here rather than in a task because it is about the
   * RUNTIME, not about any one run: the providers it can reach outlive every task on this list. */
  onOpenModels: () => void;
  projectName?: string;
  onRenameProject?: (name: string) => boolean;
  onRefreshRuntimeDetails?: () => void;
  /** What each run is ABOUT, by id, read from its briefing (#1077). A generated `run-<uuid>`
   * wears its objective as its name; the id stays visible below it. Absent
   * (older Runtime, not read yet) the row says the id, which is what it always said. */
  briefings?: Record<string, { objective: string | null; name?: string | null } | null>;
  /** Runs seen carrying task records: named by their execution, never by their form's objective. */
  teamRuns?: ReadonlySet<string>;
  /** Selected-run display only. The Runtime attention value remains unchanged in Details. */
  selectedPresentation?: { key: string; status: string } | null;
  onClose?: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(projectName ?? "");
  useEffect(() => setDraftName(projectName ?? ""), [projectName]);
  const saveName = () => {
    if (onRenameProject?.(draftName) !== false) setEditing(false);
  };
  const cancelName = () => {
    setDraftName(projectName ?? "");
    setEditing(false);
  };
  return (
    <nav id="projects-rail" className="rail" aria-label="Projects">
      <div className="rail-head">
        {/* A tile, not a glyph in a line of text: the one solid block of colour in the interface,
            which is what lets the rail have a header without a rule under it. */}
        <span className="mark" aria-hidden="true">
          <Activity />
        </span>
        <span className="wordmark">GraphHelm</span>
        {onClose && <button type="button" className="ghost rail-close" aria-label="Close projects" onClick={onClose}><X aria-hidden="true" /></button>}
        <span className={`rail-live ${connected && !stale ? "" : "off"}`}>
          <i aria-hidden="true" />
          {connected ? (stale ? "stale" : "live") : "offline"}
        </span>
      </div>

      <div className="rail-runtime-status">
        {connected && previousExit && previousExit.state !== "clean" && (
          <div className="hint" role="note" aria-label="Previous Runtime exit" style={{ overflowWrap: "anywhere" }}>
            Runtime restarted · {previousExit.state === "vanished" ? "previous run started" : "last stop"} <time dateTime={new Date(previousExit.at * 1000).toISOString()} title={`PID ${previousExit.pid} · ${new Date(previousExit.at * 1000).toISOString()}`}>
              {new Date(previousExit.at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })}
            </time>{previousExit.state === "vanished" ? " and stopped without recording why" : ` (${({ serve_error: "failed", panicked: "crashed", out_of_memory: "ran out of memory" } as Record<string, string>)[previousExit.state] ?? "stopped"})`}
            {previousExit.journalTailRecovered && <> · recovered an unfinished write ({previousExit.journalTailRecovered.trimmedBytes} bytes set aside)</>}
            {/* The ids are for whoever debugs the Runtime, not the sentence the owner reads. */}
            <details>
              <summary>Details</summary>
              {previousExit.instance ? `instance: ${previousExit.instance}` : "instance unavailable"}
              {previousExit.location !== undefined && <> · location: {previousExit.location}</>}
              {previousExit.lastPanic && <> · last panic: {previousExit.lastPanic.location}</>}
              {previousExit.journalTailRecovered && <> · quarantined: {previousExit.journalTailRecovered.quarantine}</>}
            </details>
          </div>
        )}
        {connected && (
          <details aria-label="Runtime startup">
            <summary>Runtime startup</summary>
            {currentRun ? <div>
              <p>{runtimeDetailsRefreshed ? "Runtime at last successful read" : "Runtime at connection"}</p>
              <p>Instance: {currentRun.instance}</p>
              <p>PID: {currentRun.pid}</p>
              <p>Started: <time dateTime={new Date(currentRun.startedAt * 1000).toISOString()}>{new Date(currentRun.startedAt * 1000).toISOString()}</time></p>
            </div> : <p>Runtime identity unavailable</p>}
            {startupTiming ? <div>
              <p>Journal verification: {startupTiming.journalVerifyMs} ms</p>
              <p>Lifecycle initialization: {startupTiming.lifecycleStartMs} ms</p>
            </div> : <p>Startup timing unavailable</p>}
            <button type="button" onClick={onRefreshRuntimeDetails} disabled={runtimeDetailsRefreshing}>
              Refresh Runtime details
            </button>
            {runtimeDetailsRefreshError && <p role="status">{runtimeDetailsRefreshError}</p>}
          </details>
        )}
      </div>
      <p className="lbl rail-section">Projects</p>

      <div className="projects">
        {projects.map((project) => (
          <div key={project.name}>
            <div className="project-head">
              <Folder aria-hidden="true" />
              {editing ? (
                <input
                  className="project-name-input"
                  value={draftName}
                  aria-label="Project name"
                  maxLength={120}
                  onChange={(event) => setDraftName(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") saveName();
                    if (event.key === "Escape") cancelName();
                  }}
                  autoFocus
                />
              ) : <span className="project-name">{project.name}</span>}
              {onRenameProject && !editing && (
                <button type="button" className="icon-action" onClick={() => setEditing(true)} aria-label={`Rename ${project.name}`} title="Rename project">
                  <Pencil aria-hidden="true" />
                </button>
              )}
              {editing && (
                <>
                  <button type="button" className="icon-action" onClick={saveName} aria-label="Save project name" title="Save"><Check aria-hidden="true" /></button>
                  <button type="button" className="icon-action" onClick={cancelName} aria-label="Cancel rename" title="Cancel"><X aria-hidden="true" /></button>
                </>
              )}
              {/* Icon only, and on the folder row: a task belongs to the PROJECT, and a labelled
                  button here is exactly what did not fit. The label lives in the accessible name
                  and the tooltip, where it costs no width. */}
              <button
                type="button"
                className="icon-action"
                onClick={onNewTask}
                disabled={!connected}
                aria-label={`New task in ${project.name}`}
                title={connected ? "New task in this project" : "Connect first"}
              >
                <Plus aria-hidden="true" />
              </button>
            </div>
            <div className="project-path" title={project.path ?? "Project folder path unavailable"}>
              {project.path ?? "Project folder path unavailable"}
            </div>
            {(() => {
              const home = homeRun(project.runs, teamRuns);
              const on = home !== null && home.executionId === selected;
              const state = home ? (on && selectedPresentation ? selectedPresentation.status : readable(home.attention)) : "nothing running";
              return (
                <button
                  type="button"
                  className={`project-card ${on ? "on" : ""}`}
                  aria-current={on ? "true" : undefined}
                  aria-label={`Open project ${project.name}; ${state}`}
                  title={state}
                  onClick={() => (home ? onSelect(home.executionId) : onNewTask())}
                >
                  <span className="run-lines">
                    <span className="run-id">{project.name}</span>
                    <span className="run-when">{home ? `${relativeActivity(home.lastEventAt)} · ${readable(home.status)}` : "start a task"}</span>
                  </span>
                </button>
              );
            })()}
          </div>
        ))}
      </div>

      <button type="button" className="add-project" onClick={onOpenModels}>
        <SlidersHorizontal aria-hidden="true" />
        models
      </button>

      {/* Closing the list rather than heading it: this acts on the LIST, not on any one folder. */}
      <button type="button" className="add-project" onClick={onAddProject}>
        <FolderPlus aria-hidden="true" />
        add project folder
      </button>
    </nav>
  );
}

/** The run the project card opens (#1696, D-060). Only the project's real work counts: a run
 * carrying task records (the work guided by issues) wins; otherwise the newest ongoing run;
 * otherwise the newest run. A demonstration run (the fixture executor) is never the project's
 * view; with nothing but demonstrations the card starts a task instead. */
export function homeRun(runs: ExecutionSummary[], teamRuns?: ReadonlySet<string>): ExecutionSummary | null {
  const real = runs.filter((run) => teamRuns?.has(run.executionId) || run.executor !== "fixture");
  const team = recentFirst(real.filter((run) => teamRuns?.has(run.executionId)));
  if (team.length > 0) return team[0];
  const ongoing = recentFirst(real.filter((run) => ["running", "paused", "blocked"].includes(run.status)));
  return ongoing[0] ?? recentFirst(real)[0] ?? null;
}
