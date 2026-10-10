/**
 * The projects rail: folders, and the tasks inside each one.
 *
 * A PROJECT IS A FOLDER AND ITS TASKS. The rail is built for many folders and shows the ones the
 * Studio can reach; the API carries no project dimension yet, so today that is one. Building the
 * shape for many and filling it with one is honest - inventing a hierarchy nothing enforces would
 * not be.
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
import { Activity, Check, Folder, FolderPlus, ListFilter, Pencil, Plus, RotateCcw, Trash2, X, SlidersHorizontal } from "lucide-react";

import type { ExecutionSummary, RuntimePreviousExit } from "../runtime/types";
import { hueOf, initialOf, readable, runLabel, verdictOf } from "./format";

/** How the rail groups its runs. "day" is the default: the work of each day together, the day
 * touched last on top, so the run being worked on now is the first thing on the list. */
export type RailGroup = "day" | "status" | "none";
/** Which clock orders runs inside a group: the last recorded event, or the start. */
export type RailSort = "activity" | "started";
const VIEW_KEY = "graphhelm.studio.rail-view";

function loadView(): { group: RailGroup; sort: RailSort } {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(VIEW_KEY) ?? "null");
    const value = (parsed ?? {}) as { group?: unknown; sort?: unknown };
    return {
      group: value.group === "status" || value.group === "none" ? value.group : "day",
      sort: value.sort === "started" ? "started" : "activity",
    };
  } catch {
    return { group: "day", sort: "activity" };
  }
}

function saveView(view: { group: RailGroup; sort: RailSort }): void {
  try {
    window.localStorage.setItem(VIEW_KEY, JSON.stringify(view));
  } catch {
    // A browser that refuses storage still gets the choice for this page; it just is not kept.
  }
}

function timeOf(value: string | null | undefined): number {
  const parsed = value ? Date.parse(value) : NaN;
  return Number.isFinite(parsed) ? parsed : -Infinity;
}

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

function sortedBy(runs: ExecutionSummary[], sort: RailSort): ExecutionSummary[] {
  const clock = (run: ExecutionSummary) => timeOf(sort === "started" ? run.startedAt : run.lastEventAt);
  return [...runs].sort((a, b) => clock(b) - clock(a) || (a.executionId < b.executionId ? -1 : a.executionId > b.executionId ? 1 : 0));
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

function activityTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
    second: "2-digit", timeZoneName: "short",
  }).format(new Date(value));
}

function hasRecordedActivity(run: ExecutionSummary): boolean {
  return typeof run.lastEventAt === "string" && Number.isFinite(Date.parse(run.lastEventAt));
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
  hasMore,
  busy,
  onSelect,
  onLoadMore,
  onNewTask,
  onAddProject,
  onOpenModels,
  projectName,
  onRenameProject,
  removedRuns = [],
  onRemoveRun,
  onRestoreRun,
  briefings = {},
  selectedPresentation = null,
}: {
  projects: Project[];
  selected: string;
  connected: boolean;
  previousExit?: RuntimePreviousExit | null;
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
  removedRuns?: string[];
  onRemoveRun?: (id: string) => void;
  onRestoreRun?: (id: string) => void;
  /** What each run is ABOUT, by id, read from its briefing (#1077). A generated `run-<uuid>`
   * wears its objective as its name; the id stays on the row as its address. Absent (older
   * Runtime, not read yet) the row says the id, which is what it always said. */
  briefings?: Record<string, { objective: string | null; name?: string | null } | null>;
  /** Selected-run display only. The Runtime attention value remains unchanged in Details. */
  selectedPresentation?: { key: string; status: string } | null;
}) {
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(projectName ?? "");
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [historyMode, setHistoryMode] = useState<"auto" | "shown" | "hidden">("auto");
  const [view, setView] = useState(loadView);
  const [viewMenu, setViewMenu] = useState(false);
  const chooseView = (next: Partial<{ group: RailGroup; sort: RailSort }>) => {
    const merged = { ...view, ...next };
    setView(merged);
    saveView(merged);
  };
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
        <span className={`rail-live ${connected && !stale ? "" : "off"}`}>
          <i aria-hidden="true" />
          {connected ? (stale ? "stale" : "live") : "offline"}
        </span>
      </div>

      {connected && previousExit && previousExit.state !== "clean" && (
        <p className="hint" aria-label="Previous Runtime exit" style={{ overflowWrap: "anywhere" }}>
          Runtime restarted · last stop <time dateTime={new Date(previousExit.at * 1000).toISOString()} title={`PID ${previousExit.pid} · ${new Date(previousExit.at * 1000).toISOString()}`}>
            {new Date(previousExit.at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })}
          </time> ({({ vanished: "killed", serve_error: "failed", panicked: "crashed" })[previousExit.state] ?? "stopped"})
          {previousExit.location !== undefined && <> · location: {previousExit.location}</>}
          {previousExit.lastPanic && <> · last panic: {previousExit.lastPanic.location}</>}
        </p>
      )}
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
                className={`icon-action ${viewMenu ? "on" : ""}`}
                onClick={() => setViewMenu((open) => !open)}
                aria-label="Group and sort tasks"
                aria-expanded={viewMenu}
                title="Group and sort"
              >
                <ListFilter aria-hidden="true" />
              </button>
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
            {viewMenu && (
              <div className="rail-view-menu" role="group" aria-label="Group and sort tasks">
                <p className="lbl">Group by</p>
                {([["day", "Day"], ["status", "Status"], ["none", "None"]] as const).map(([value, text]) => (
                  <button key={value} type="button" aria-pressed={view.group === value} onClick={() => chooseView({ group: value })}>
                    <span>{text}</span>{view.group === value && <Check aria-hidden="true" />}
                  </button>
                ))}
                <p className="lbl">Sort by</p>
                {([["activity", "Last activity"], ["started", "Started"]] as const).map(([value, text]) => (
                  <button key={value} type="button" aria-pressed={view.sort === value} onClick={() => chooseView({ sort: value })}>
                    <span>{text}</span>{view.sort === value && <Check aria-hidden="true" />}
                  </button>
                ))}
              </div>
            )}
            <div className="project-path" title={project.path ?? "Project folder path unavailable"}>
              {project.path ?? "Project folder path unavailable"}
            </div>

            <div className="project-runs">
              {(() => {
                const dated = project.runs.filter(hasRecordedActivity);
                const unknownTime = recentFirst(project.runs.filter((run) => !hasRecordedActivity(run)));
                const ongoing = recentFirst(dated.filter((run) => ["running", "paused", "blocked"].includes(run.status)));
                const history = recentFirst(dated.filter((run) => ["completed", "failed", "cancelled"].includes(run.status)));
                const unknown = recentFirst(dated.filter((run) => !["running", "paused", "blocked", "completed", "failed", "cancelled"].includes(run.status)));
                const selectedHistory = history.find((run) => run.executionId === selected);
                const historyExpanded = historyMode === "shown" || (historyMode === "auto" && ongoing.length === 0);
                const visibleHistory = historyExpanded ? history : selectedHistory ? [selectedHistory] : [];
                const renderRows = (runs: ExecutionSummary[]) => runs.map((run) => {
                const verdict = verdictOf(run.attention);
                const reviewCount = run.status === "completed" && run.executor !== "fixture" && typeof run.unverifiedResults === "number" && run.unverifiedResults > 0
                  ? run.unverifiedResults : 0;
                const on = run.executionId === selected;
                const rowKey = run.status === "cancelled" ? "cancelled" : reviewCount > 0 ? "review" : on && selectedPresentation ? selectedPresentation.key : verdict.key;
                const rowStatus = run.status === "cancelled" ? "cancelled" : reviewCount > 0 ? `${reviewCount} result${reviewCount === 1 ? " needs" : "s need"} review` : on && selectedPresentation ? selectedPresentation.status : readable(run.attention);
                // #1083 F7: the row's own declared objective first (one index read names every
                // row), the briefing a selection already read as the fallback for an older
                // Runtime whose rows do not carry it.
                const label = runLabel(
                  run.executionId,
                  typeof run.objective === "string" ? { objective: run.objective } : briefings[run.executionId],
                );
                const lastEventLabel = run.lastEventAt && Number.isFinite(Date.parse(run.lastEventAt))
                  ? `Last event ${activityTime(run.lastEventAt)}` : `Last event unknown · status ${readable(run.status)}`;
                return (
                  <div key={run.executionId} className={`run-row ${rowKey} ${on ? "on" : ""}`}>
                  <button type="button" className={`run ${rowKey} ${on ? "on" : ""}`} aria-current={on ? "true" : undefined} aria-label={`${label}${label !== run.executionId ? `, run ${run.executionId}` : ""}; ${lastEventLabel}; ${rowStatus}`} onClick={() => onSelect(run.executionId)} title={rowStatus}>
                    {/* Each room wears its own derived colour, like a contact in a messenger -
                        the same hue its actors' avatars key off nothing, but the ROOM's identity
                        comes from its id, stable across every view. */}
                    <span
                      className="avatar"
                      aria-hidden="true"
                      style={{ background: `hsl(${hueOf(run.executionId)} 52% 46%)` }}
                    >
                      {initialOf(run.executionId)}
                    </span>
                    <span className="run-lines">
                      {/* The objective when there is one, the id otherwise - and the id ALWAYS
                          on the row as its title, so a name never hides the address. */}
                      {/* #1098 D4: the hover says what is CUT. Two runs whose objectives share a
                          prefix ellipsise to the same words, and this title used to answer with
                          the execution id — which the next line already prints in full. The name
                          line titles the name; the address line titles the address. */}
                      <span className="run-id" title={label}>{label}</span>
                      {/* #1083 F7: a run named by its objective still shows its address, as
                          secondary text - never only on hover. */}
                      {label !== run.executionId && (
                        <span className="run-address" title={run.executionId}>{run.executionId}</span>
                      )}
                      <span className="run-when">
                        {run.lastEventAt && Number.isFinite(Date.parse(run.lastEventAt))
                          ? <time dateTime={run.lastEventAt} title={run.lastEventAt}>{lastEventLabel}</time>
                          : lastEventLabel}
                        {/* #1064: a run started under the fixture executor says so in the index
                          * too, not only once opened — the word, never a glyph. */}
                        {run.executor === "fixture" && (
                          <span className="run-demo"> · demonstration</span>
                        )}
                        {reviewCount > 0 && <span className="run-review"> · review needed</span>}
                      </span>
                    </span>
                    {/* The hover word lives on the BUTTON's title; putting it here too made
                        every row utter its state twice ("needs you needs you"). */}
                    <span className="run-mark" aria-hidden="true" />
                    {/* The state reaches a screen reader as words: the mark alone would leave it
                        as a colour, which is not a name for anything. */}
                    <span className="sr-only">{rowStatus}</span>
                  </button>
                  {onRemoveRun && (confirmRemove === run.executionId ? <span className="run-remove-confirm"><span>Remove from this browser’s list; history stays intact and execution continues.</span><button type="button" onClick={() => { onRemoveRun(run.executionId); setConfirmRemove(null); }}>Remove</button><button type="button" onClick={() => setConfirmRemove(null)} aria-label="Cancel remove">Cancel</button></span> : <button type="button" className="icon-action run-remove" onClick={() => setConfirmRemove(run.executionId)} aria-label={`Remove ${run.executionId} from this browser's list`} title="Remove from this browser’s list"><Trash2 aria-hidden="true" /></button>)}
                  </div>
                );
                });
                if (view.group !== "status") {
                  const ordered = sortedBy(project.runs, view.sort);
                  if (view.group === "none") return <div role="group" aria-label={`All runs (${ordered.length})`}>{renderRows(ordered)}</div>;
                  // DAY GROUPS follow the same clock as the sort, so a run sits under the day it
                  // was last touched (or started) and the newest day is always on top.
                  const days: { key: string; label: string; runs: ExecutionSummary[] }[] = [];
                  const undated: ExecutionSummary[] = [];
                  for (const run of ordered) {
                    const time = timeOf(view.sort === "started" ? run.startedAt : run.lastEventAt);
                    if (time === -Infinity) { undated.push(run); continue; }
                    const day = dayOf(time);
                    const last = days[days.length - 1];
                    if (last && last.key === day.key) last.runs.push(run);
                    else days.push({ ...day, runs: [run] });
                  }
                  return <>
                    {days.map((day) => <div key={day.key} role="group" aria-label={`${day.label} (${day.runs.length})`}><p className="lbl run-group-title">{day.label}</p>{renderRows(day.runs)}</div>)}
                    {undated.length > 0 && <div role="group" aria-label={`Date unknown (${undated.length})`}><p className="lbl run-group-title">Date unknown</p>{renderRows(undated)}</div>}
                  </>;
                }
                return <>
                  {ongoing.length > 0 && <div role="group" aria-label={`Ongoing runs (${ongoing.length})`}><p className="lbl run-group-title">Ongoing ({ongoing.length})</p>{renderRows(ongoing)}</div>}
                  {unknown.length > 0 && <div role="group" aria-label={`Status unknown (${unknown.length})`}><p className="lbl run-group-title">Status unknown ({unknown.length})</p>{renderRows(unknown)}</div>}
                  {history.length > 0 && <div role="group" aria-label={`Historical runs (${history.length})`}>
                    <button type="button" className="run-group-toggle" aria-expanded={historyExpanded} onClick={() => setHistoryMode(historyExpanded ? "hidden" : "shown")}>{historyExpanded ? "Hide history" : "Show history"} ({history.length})</button>
                    {renderRows(visibleHistory)}
                  </div>}
                  {unknownTime.length > 0 && <div role="group" aria-label={`Activity time unknown (${unknownTime.length})`}><p className="lbl run-group-title">Activity time unknown ({unknownTime.length})</p>{renderRows(unknownTime)}</div>}
                </>;
              })()}

              {project.runs.length === 0 && (
                <button type="button" className="rail-empty" onClick={onNewTask}>
                  No task yet — start one
                </button>
              )}

              {hasMore && (
                <><p className="run-page-note">Loaded runs only; more may have newer activity.</p><button type="button" className="show-more" onClick={onLoadMore} disabled={busy}>
                  show more
                </button></>
              )}
            </div>
          </div>
        ))}
        {removedRuns.length > 0 && (
          <div className="removed-runs" aria-label="Removed tasks">
            <p className="lbl">Removed from this browser</p>
            {removedRuns.map((id) => <button type="button" className="show-more" key={id} onClick={() => onRestoreRun?.(id)}><RotateCcw aria-hidden="true" /> restore {id}</button>)}
          </div>
        )}
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
