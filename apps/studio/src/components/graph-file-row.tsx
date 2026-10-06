import type { Ref } from "react";
import { FileCode2, Waypoints } from "lucide-react";

export interface GraphFileRowProps {
  graphFile: string;
  onGraphFileChange: (value: string) => void;
  onDrawConnections: () => void;
  busy: boolean;
  demonstration?: boolean;
  fixtureFile?: string;
  onFixtureFileChange?: (value: string) => void;
  inputRef?: Ref<HTMLInputElement>;
}

/** The graph file on the Runtime's host: one field for both connections and resume. */
export function GraphFileRow({ graphFile, onGraphFileChange, onDrawConnections, busy, demonstration = false, fixtureFile = "", onFixtureFileChange, inputRef }: GraphFileRowProps) {
  return (
    <div className="graph-file">
      <span className="wrap">
        <FileCode2 aria-hidden="true" />
        <label>
          <span className="sr-only">Graph file path on the Runtime host</span>
          <input ref={inputRef} value={graphFile} onChange={(event) => onGraphFileChange(event.target.value)} placeholder="Graph file on the Runtime host…" />
        </label>
      </span>
      <button type="button" onClick={onDrawConnections} disabled={busy || graphFile.trim().length === 0} title="Read this file's shape and check it against the hash this run recorded">
        <Waypoints aria-hidden="true" />
        connect
      </button>
      {demonstration && onFixtureFileChange && (
        <span className="wrap">
          <FileCode2 aria-hidden="true" />
          <label>
            <span className="sr-only">Fixture file path on the Runtime host, sent with resume</span>
            <input value={fixtureFile} onChange={(event) => onFixtureFileChange(event.target.value)} placeholder="Fixture file for resume (optional)…"
              title="A demonstration run's outcomes come from a fixture file. Name one here and resume sends it; leave it empty and the resumed node waits for input." />
          </label>
        </span>
      )}
    </div>
  );
}
