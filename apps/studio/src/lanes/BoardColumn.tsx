import type { ReactNode } from "react";

interface Props {
  /** The column key, used for the dot colour (backlog, implement, ...). */
  name: string;
  /** The column name as shown ("Backlog"). */
  label: string;
  count: number;
  /** The cards; only drawn when the column has some. */
  children: ReactNode;
}

/** One kanban column of the Lanes board (docs/design/studio/Lanes.dc.html, #1996). A column with no
 * cards folds to a 44px strip - dot, count, the name written down the strip - and opens again as
 * soon as it gets a card, so the columns that hold cards share the width and never break a word. */
export function BoardColumn({ name, label, count, children }: Props) {
  if (count === 0) {
    return (
      <li className={`board-column folded ${name}`} aria-label={`${label}, empty`} title={`${label} · none`}>
        <span className={`col-dot ${name}`} />
        <span className="mono count">0</span>
        <strong className="folded-name mono">{label}</strong>
      </li>
    );
  }
  return (
    <li className={`board-column ${name}`}>
      <h3><span className={`col-dot ${name}`} />{label}<span className="mono count">{count}</span></h3>
      {children}
    </li>
  );
}
