/**
 * The Team tab (spec 4.2): bots in a row, their tasks below, a line between two bots that
 * addressed each other. Motion means new records - a line animates only while its pair spoke in
 * the last minute. Positions are a per-viewer convenience in localStorage, never shared state.
 */
import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { FileCode2, RotateCcw } from "lucide-react";

import type { Camera } from "../graph/board";
import type { GraphNode } from "../graph/model";
import { BOT_RECENT_MS, type Bot, type BotTask, type OtherRecorder, type TeamLink } from "../runtime/team";
import { ago, readable } from "./format";

export interface TeamCanvasProps {
  storageKey: string;
  bots: Bot[];
  otherRecorders: OtherRecorder[];
  links: TeamLink[];
  unassignedSteps: GraphNode[];
  selectedBot: string | null;
  onSelectBot: (key: string) => void;
  onOpenBotDetails: (key: string) => void;
  onOpenNode: (nodeId: string) => void;
  onOpenTask: (bot: Bot, task: BotTask) => void;
  graphFileRow: ReactNode;
  graphFileOpen: boolean;
  onGraphFileOpenChange: (open: boolean) => void;
  /** Spec 4.1: the owner names an unnamed bot. Absent hides the control. */
  onNameBot?: (actorId: string, displayName: string) => void | Promise<unknown>;
}

function NameBot({ bot, onSave }: { bot: Bot; onSave: (actorId: string, displayName: string) => void | Promise<unknown> }) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  if (bot.actorId === null) return null;
  const actorId = bot.actorId;
  if (!open) return <button type="button" className="team-bot-name-it" onClick={() => setOpen(true)}>Name this bot</button>;
  const name = value.trim();
  return (
    <form className="team-bot-name-form" onSubmit={(event) => { event.preventDefault(); if (name.length === 0) return; void onSave(actorId, name); setOpen(false); setValue(""); }}>
      <input aria-label={`Name for ${bot.name}`} placeholder="Display name" maxLength={80} value={value} onChange={(event) => setValue(event.target.value)} autoFocus />
      <button type="submit" disabled={name.length === 0}>Save</button>
      <button type="button" onClick={() => { setOpen(false); setValue(""); }}>Cancel</button>
    </form>
  );
}

const BOT_W = 220;
const BOT_GAP = 48;
const BOT_FACE_H = 120;
type Point = { x: number; y: number };
type Positions = Record<string, Point>;

export function defaultBotPosition(index: number): Point {
  return { x: 24 + index * (BOT_W + BOT_GAP), y: 32 };
}

export function botStateLabel(bot: Bot): string {
  switch (bot.state) {
    case "working": return "Working";
    case "waiting_for_you": return "Waiting for you";
    case "done": return "Done";
    case "quiet":
      if (bot.quietMinutes === null) return "No record yet";
      // #532: a lane quiet past the recent window reads as idle since its last record.
      return bot.quietMinutes * 60_000 > BOT_RECENT_MS ? `Idle since ${ago(bot.lastRecordAt)} ago` : `No new record for ${bot.quietMinutes} min`;
  }
}

function readPositions(key: string): Positions {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Positions = {};
    for (const [id, value] of Object.entries(parsed as Record<string, unknown>)) {
      const point = value as { x?: unknown; y?: unknown } | null;
      if (point && Number.isFinite(point.x) && Number.isFinite(point.y)) out[id] = { x: point.x as number, y: point.y as number };
    }
    return out;
  } catch {
    return {};
  }
}

function writePositions(key: string, positions: Positions): void {
  try { localStorage.setItem(key, JSON.stringify(positions)); } catch { /* per-viewer convenience only */ }
}

/** Keep a drag alive when the pointer leaves the sheet: the release still reaches us and persists. */
function capture(event: ReactPointerEvent<HTMLElement>): void {
  try { event.currentTarget.setPointerCapture?.(event.pointerId); } catch { /* not capturable: the drag ends on the sheet only */ }
}

const HOME: Camera = { x: 0, y: 0, zoom: 1 };

export function TeamCanvas(props: TeamCanvasProps) {
  const { bots, links } = props;
  const [positions, setPositionsState] = useState<Positions>(() => readPositions(props.storageKey));
  const positionsRef = useRef(positions);
  const setPositions = (next: Positions) => { positionsRef.current = next; setPositionsState(next); };
  // The camera shares board.ts's Camera shape. fitCamera frames a rectangle and this row layout
  // has no fit action, so the pan and the ctrl-wheel zoom stay local and minimal.
  const [view, setView] = useState<Camera>(HOME);
  const sheetRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ kind: "bot" | "pan"; id: string; start: Point; origin: Point } | null>(null);
  useEffect(() => { setPositions(readPositions(props.storageKey)); }, [props.storageKey]);

  useEffect(() => {
    const sheet = sheetRef.current;
    if (sheet === null) return;
    // React delegates wheel events passively; a native listener can consume browser zoom.
    const zoom = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      setView((current) => ({ ...current, zoom: Math.min(2, Math.max(0.4, current.zoom * (1 - event.deltaY * 0.0015))) }));
    };
    sheet.addEventListener("wheel", zoom, { passive: false });
    return () => sheet.removeEventListener("wheel", zoom);
  }, []);

  const placeOf = (key: string, index: number): Point => positions[key] ?? defaultBotPosition(index);
  const centre = (key: string): Point | null => {
    const index = bots.findIndex((bot) => bot.key === key);
    if (index < 0) return null;
    const at = placeOf(key, index);
    return { x: at.x + BOT_W / 2, y: at.y + BOT_FACE_H / 2 };
  };
  const nameOf = (key: string) => bots.find((bot) => bot.key === key)?.name ?? key;

  const onSheetDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    // .team-world fills the sheet, so empty space is the sheet or the world; bots, tasks and buttons are not.
    if ((event.target as Element).closest("article, button")) return;
    capture(event);
    drag.current = { kind: "pan", id: "", start: { x: event.clientX, y: event.clientY }, origin: { x: view.x, y: view.y } };
  };
  const onMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const held = drag.current;
    if (held === null) return;
    const dx = event.clientX - held.start.x;
    const dy = event.clientY - held.start.y;
    if (held.kind === "pan") setView((current) => ({ ...current, x: held.origin.x + dx, y: held.origin.y + dy }));
    else setPositions({ ...positionsRef.current, [held.id]: { x: held.origin.x + dx / view.zoom, y: held.origin.y + dy / view.zoom } });
  };
  const onUp = () => {
    if (drag.current?.kind === "bot") writePositions(props.storageKey, positionsRef.current);
    drag.current = null;
  };

  const stepsX = defaultBotPosition(bots.length).x;
  return (
    <section className="team-canvas" aria-label="Team">
      <div className="team-toolbar">
        <button type="button" onClick={() => { setPositions({}); writePositions(props.storageKey, {}); setView(HOME); }}>
          <RotateCcw aria-hidden="true" /> Reset layout
        </button>
        <button type="button" aria-expanded={props.graphFileOpen} onClick={() => props.onGraphFileOpenChange(!props.graphFileOpen)}>
          <FileCode2 aria-hidden="true" /> Graph file
        </button>
        {props.graphFileOpen && props.graphFileRow}
      </div>
      <div ref={sheetRef} className="team-sheet" aria-label="Team sheet" onPointerDown={onSheetDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} onLostPointerCapture={onUp}>
        <div className="team-world" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}>
          <svg className="team-links" aria-hidden="true" width={stepsX + BOT_W} height={640}>
            {links.map((link) => {
              const a = centre(link.a);
              const b = centre(link.b);
              if (a === null || b === null) return null;
              return <line key={`${link.a}+${link.b}`} className={`team-link ${link.live ? "team-link-live" : ""}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} />;
            })}
          </svg>
          <ul className="sr-only" aria-label="Who talks to whom">
            {links.map((link) => <li key={`${link.a}+${link.b}`}>{nameOf(link.a)} and {nameOf(link.b)}: {link.count} recorded messages{link.live ? ", just now" : ""}</li>)}
          </ul>
          {bots.map((bot, index) => {
            const at = placeOf(bot.key, index);
            return (
              <article key={bot.key} data-testid={`team-bot-${bot.key}`} className={`team-bot team-bot-${bot.state} ${props.selectedBot === bot.key ? "team-bot-selected" : ""}`}
                style={{ left: `${at.x}px`, top: `${at.y}px`, "--bot-hue": String(bot.hue) } as CSSProperties}>
                <button type="button" className="team-bot-grip" aria-label={`Move ${bot.name}`} title="Use arrow keys to move this bot"
                  onKeyDown={(event) => {
                    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
                    event.preventDefault();
                    const current = positionsRef.current[bot.key] ?? defaultBotPosition(index);
                    const next = { ...positionsRef.current, [bot.key]: {
                      x: current.x + (event.key === "ArrowRight" ? 10 : event.key === "ArrowLeft" ? -10 : 0),
                      y: current.y + (event.key === "ArrowDown" ? 10 : event.key === "ArrowUp" ? -10 : 0),
                    } };
                    setPositions(next);
                    writePositions(props.storageKey, next);
                  }}
                  onPointerDown={(event) => { event.stopPropagation(); capture(event); drag.current = { kind: "bot", id: bot.key, start: { x: event.clientX, y: event.clientY }, origin: at }; }}>⋮⋮</button>
                <button type="button" className="team-bot-face" aria-label={`${bot.name}, ${botStateLabel(bot)}`} onClick={() => props.onSelectBot(bot.key)}>
                  <span className="team-bot-avatar" aria-hidden="true">{bot.name.slice(0, 1).toUpperCase()}</span>
                  <span className="team-bot-name">{bot.name}</span>
                  <span className="team-bot-state">{botStateLabel(bot)}</span>
                </button>
                {bot.role && <p className="team-bot-role">{bot.role}</p>}
                <p className="team-bot-doing">{bot.doingNow}</p>
                {bot.shared && <p className="team-bot-note">Shared actor: its records cannot be attributed to one chat.</p>}
                <p className="team-bot-when">{bot.lastRecordAt === null ? "no record" : `last record ${ago(bot.lastRecordAt)}`}</p>
                <button type="button" className="team-bot-details" onClick={() => props.onOpenBotDetails(bot.key)}>Details</button>
                {props.onNameBot && bot.actorId !== null && bot.role === null && !bot.native && !bot.shared && <NameBot bot={bot} onSave={props.onNameBot} />}
                {bot.tasks.length > 0 && (
                  <ol className="team-tasks" aria-label={`${bot.name} tasks`}>
                    {bot.tasks.map((task) => (
                      <li key={task.id} className={task.done ? "team-task-done" : ""}>
                        <button type="button" onClick={() => props.onOpenTask(bot, task)}>{task.title}</button>
                      </li>
                    ))}
                  </ol>
                )}
              </article>
            );
          })}
          {props.unassignedSteps.length > 0 && (
            <article className="team-steps" style={{ left: `${stepsX}px`, top: "32px" }} aria-label="Steps without a bot">
              <h3>Steps without a bot</h3>
              <ol>
                {props.unassignedSteps.map((node) => (
                  <li key={node.id}><button type="button" onClick={() => props.onOpenNode(node.id)}>{`${node.declaredName ?? node.id} · ${readable(node.state)}`}</button></li>
                ))}
              </ol>
            </article>
          )}
        </div>
      </div>
      {props.otherRecorders.length > 0 && (
        <details className="team-others">
          <summary>{props.otherRecorders.length} other recorders</summary>
          <ul>{props.otherRecorders.map((other) => <li key={other.actorId}>{other.actorId} · {other.count} records · {ago(other.lastRecordAt)}</li>)}</ul>
        </details>
      )}
    </section>
  );
}
