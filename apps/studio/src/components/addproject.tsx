/**
 * What "add project folder" can honestly do today.
 *
 * A PROJECT IS A FOLDER WITH A RUNTIME SERVING IT. One Runtime serves one event store, so a
 * second folder means a second Runtime - and a browser page cannot start a process, ever. What
 * can is the local dev server already serving this page, and that launcher is not built.
 *
 * So this panel does the one true thing available: it tells the operator exactly what to run, in
 * the folder they have in mind. That is not a placeholder for the button - it IS how a project is
 * added right now, and the launcher, when it exists, replaces this panel rather than this panel
 * pretending the launcher is here.
 *
 * The alternative was a folder picker that opens, takes a path, and then cannot do anything with
 * it. A control that looks capable and is not costs more than an honest instruction.
 */

import { useState } from "react";
import { Check, Copy, FolderPlus, X } from "lucide-react";

/** The command that makes a folder into a project. `--events` is the only required argument: the
 * rest of the executor group is what a run needs to reach a model, and a folder is a project
 * before it can do that. */
function commandFor(folder: string): string {
  const target = folder.trim().length > 0 ? folder.trim() : "<path to the folder>";
  // `;` rather than `&&`: the panel shows a Windows-style path, and Windows PowerShell 5.1
  // rejects `&&` outright - the copied command has to run in the shell the example implies
  // (PR #467 review). If the cd fails, serve then refuses on the missing events dir, which is
  // the same stop one step later with a clearer message.
  return `cd "${target}"; graphhelm serve --events .graphhelm/events`;
}

export function AddProject({ onClose }: { onClose: () => void }) {
  const [folder, setFolder] = useState("");
  const [copied, setCopied] = useState(false);
  const command = commandFor(folder);

  const copy = () => {
    void navigator.clipboard
      ?.writeText(command)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1600);
      })
      // A clipboard the browser refuses is not an error worth a banner: the command is on screen
      // and selectable, which is the fallback every operator already knows.
      .catch(() => setCopied(false));
  };

  return (
    <section className="panel" aria-label="Add project folder">
      <header className="panel-head calm">
        <i aria-hidden="true" />
        <div style={{ minWidth: 0 }}>
          <h2>Add project folder</h2>
          <p className="lbl">one runtime per folder</p>
        </div>
        <button type="button" className="ghost close" onClick={onClose} aria-label="Close">
          <X aria-hidden="true" />
        </button>
      </header>

      <div className="composer">
        <p className="panel-foot" style={{ padding: 0 }}>
          A project is a folder with a Runtime serving it. Start one in the folder and the Studio
          picks it up.
        </p>

        <label className="lbl" htmlFor="add-project-folder">
          Folder
        </label>
        <input
          id="add-project-folder"
          name="project-folder"
          autoFocus
          value={folder}
          autoComplete="off"
          spellCheck={false}
          placeholder="F:/projects/example/dale-api-base"
          onChange={(event) => setFolder(event.target.value)}
        />

        <label className="lbl" htmlFor="add-project-command">
          Run this there
        </label>
        <code id="add-project-command" className="command">
          {command}
        </code>

        <button type="button" className="send" onClick={copy}>
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
          {copied ? "copied" : "copy the command"}
        </button>
        <span className="sr-only" role="status">{copied ? "Command copied" : ""}</span>
      </div>

      <p className="panel-foot">
        <FolderPlus aria-hidden="true" style={{ width: 12, height: 12, verticalAlign: "-1px" }} />{" "}
        One day the Studio will start the Runtime for you; today you run this command yourself.
      </p>
    </section>
  );
}
