import { When } from "../time/Stamp";
import { VIEWS, type View } from "../routing/route";

const LABEL: Record<View, string> = { graph: "Graph", lanes: "Lanes", proof: "Proof" };

interface Props {
  project: string;
  run: string | null;
  view: View;
  updated: number | null;
  onView: (view: View) => void;
  onStart: (() => void) | null;
}

/** The top bar of the design (Topbar.dc.html): where you are, the three views, and the one action. */
export function Topbar({ project, run, view, updated, onView, onStart }: Props) {
  return (
    <header aria-label="Top bar" className="topbar">
      <h2 className="crumb-project">{project}</h2>
      <span className="crumb-sep">/</span>
      <span className="crumb-run">{run ?? "no run"}</span>
      <div role="tablist" aria-label="Views" className="views">
        {VIEWS.map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            className="b"
            id={`tab-${key}`}
            aria-controls={`panel-${key}`}
            aria-selected={key === view}
            onClick={() => onView(key)}
          >
            {LABEL[key]}
          </button>
        ))}
      </div>
      <span className="spacer" />
      {updated !== null && <span className="updated">updated <When at={updated} /> · live</span>}
      {onStart !== null && <button type="button" className="b act" onClick={onStart}>Start a team run</button>}
    </header>
  );
}
