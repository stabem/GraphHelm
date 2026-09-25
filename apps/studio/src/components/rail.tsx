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
import { Activity, Check, Folder, FolderPlus, Pencil, Plus, RotateCcw, Trash2, X, SlidersHorizontal } from "lucide-react";

import type { ExecutionSummary } from "../runtime/types";
import { clock, hueOf, initialOf, readable, runLabel, verdictOf } from "./format";

export interface Project {
  /** The folder's name. Today: the Runtime's own store. */
  name: string;
  runs: ExecutionSummary[];
}

export function ProjectRail({
  projects,
  selected,
  connected,
  stale = false,
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
}: {
  projects: Project[];
  selected: string;
  connected: boolean;
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
}) {
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(projectName ?? "");
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
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

            <div className="project-runs">
              {project.runs.map((run) => {
                const verdict = verdictOf(run.attention);
                const on = run.executionId === selected;
                // #1083 F7: the row's own declared objective first (one index read names every
                // row), the briefing a selection already read as the fallback for an older
                // Runtime whose rows do not carry it.
                const label = runLabel(
                  run.executionId,
                  typeof run.objective === "string" ? { objective: run.objective } : briefings[run.executionId],
                );
                return (
                  <div key={run.executionId} className={`run-row ${verdict.key} ${on ? "on" : ""}`}>
                  <button type="button" className={`run ${verdict.key} ${on ? "on" : ""}`} aria-current={on ? "true" : undefined} onClick={() => onSelect(run.executionId)} title={readable(run.attention)}>
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
                        {clock(run.lastEventAt)}
                        {/* #1064: a run started under the fixture executor says so in the index
                          * too, not only once opened — the word, never a glyph. */}
                        {run.executor === "fixture" && (
                          <span className="run-demo"> · demonstration</span>
                        )}
                      </span>
                    </span>
                    {/* The hover word lives on the BUTTON's title; putting it here too made
                        every row utter its state twice ("needs you needs you"). */}
                    <span className="run-mark" aria-hidden="true" />
                    {/* The state reaches a screen reader as words: the mark alone would leave it
                        as a colour, which is not a name for anything. */}
                    <span className="sr-only">{readable(run.attention)}</span>
                  </button>
                  {onRemoveRun && (confirmRemove === run.executionId ? <span className="run-remove-confirm"><span>Remove from this browser’s list; history stays intact and execution continues.</span><button type="button" onClick={() => { onRemoveRun(run.executionId); setConfirmRemove(null); }}>Remove</button><button type="button" onClick={() => setConfirmRemove(null)} aria-label="Cancel remove">Cancel</button></span> : <button type="button" className="icon-action run-remove" onClick={() => setConfirmRemove(run.executionId)} aria-label={`Remove ${run.executionId} from this browser's list`} title="Remove from this browser’s list"><Trash2 aria-hidden="true" /></button>)}
                  </div>
                );
              })}

              {project.runs.length === 0 && (
                <button type="button" className="rail-empty" onClick={onNewTask}>
                  No task yet — start one
                </button>
              )}

              {hasMore && (
                <button type="button" className="show-more" onClick={onLoadMore} disabled={busy}>
                  show more
                </button>
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
