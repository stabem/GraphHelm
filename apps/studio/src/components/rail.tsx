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

import { Activity, Folder, FolderPlus, Plus } from "lucide-react";

import type { ExecutionSummary } from "../runtime/types";
import { clock, hueOf, initialOf, readable, verdictOf } from "./format";

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
}) {
  return (
    <nav className="rail" aria-label="Projects">
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
              <span className="project-name">{project.name}</span>
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
                return (
                  <button
                    type="button"
                    key={run.executionId}
                    className={`run ${verdict.key} ${on ? "on" : ""}`}
                    aria-current={on ? "true" : undefined}
                    onClick={() => onSelect(run.executionId)}
                    title={readable(run.attention)}
                  >
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
                      <span className="run-id">{run.executionId}</span>
                      <span className="run-when">{clock(run.lastEventAt)}</span>
                    </span>
                    {/* The hover word lives on the BUTTON's title; putting it here too made
                        every row utter its state twice ("needs you needs you"). */}
                    <span className="run-mark" aria-hidden="true" />
                    {/* The state reaches a screen reader as words: the mark alone would leave it
                        as a colour, which is not a name for anything. */}
                    <span className="sr-only">{readable(run.attention)}</span>
                  </button>
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
      </div>

      {/* Closing the list rather than heading it: this acts on the LIST, not on any one folder. */}
      <button type="button" className="add-project" onClick={onAddProject}>
        <FolderPlus aria-hidden="true" />
        add project folder
      </button>
    </nav>
  );
}
