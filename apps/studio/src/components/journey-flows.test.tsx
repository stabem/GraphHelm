// #353 / #465: the owner approves journeys in the Journey tab. Regressions caught: a long card list
// that buries the one to approve, raw finding codes, pointers and URLs shown to the owner, steps
// without what they do and show, an Approve offered on a flow the Runtime would refuse, a refusal
// swallowed, a settled approval offered again, and a Watch that does not light the step being
// played. Cost: jsdom render, no network.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { JourneyFlowView, JourneyFlowsView, LiveSession } from "../runtime/types";
import { JourneyFlows, type JourneyRunSource } from "./journey-flows";

const userEvent = fastUserEvent();
afterEach(cleanup);

const flow = (id: string, extra: Partial<JourneyFlowView> = {}): JourneyFlowView => ({
  id, title: `Flow ${id}`, status: "draft", approved: null, drift: [], findings: [], approvable: true,
  screens: [
    { id: "cart", url: "/cart?session=studio-fixture", state: "stable", expect: [{ role: "heading", name: "Cart" }] },
    { id: "pay", url: "/checkout", state: "stable", expect: [{ role: "textbox", name: "Password" }] },
    { id: "done", url: "/orders/:id", state: "success", expect: [{ role: "heading", name: "Order placed" }] },
  ],
  edges: [
    { id: "cart.checkout", from: "cart", to: "pay", acts: [{ kind: "activate", role: "button", name: "Checkout" }] },
    { id: "pay.submit", from: "pay", to: "done", acts: [{ kind: "enter_text", role: "textbox", name: "Password" }, { kind: "submit", role: "button", name: "Pay now" }] },
  ],
  paths: { main: ["cart.checkout", "pay.submit"] },
  ...extra,
});

const view: JourneyFlowsView = { flows: [
  flow("done", { status: "approved", approvable: false, approved: { revision: "a".repeat(40), digest: `sha256:${"b".repeat(64)}` } }),
  flow("checkout"),
  flow("broken", { approvable: false, findings: [{ code: "flow.contract_stale", pointer: "/journeys/broken.json", message: "generated contract differs or is missing", severity: "error" }] }),
  flow("edited", { status: "approval_stale", drift: [{ edge: "cart.checkout", code: "drift.locator_missing" }],
    findings: [{ code: "flow.approval_stale", pointer: "/approved/digest", message: "approval does not bind this flow projection", severity: "error" }] }),
] };

const watchRow = (extra: Partial<LiveSession>): LiveSession => ({
  sessionId: "w1", contractId: "checkout", flowId: "checkout", path: "main", stepId: "cart", state: "playing", code: null, at: null,
  screen: "cart", since: "2026-10-08T00:00:00Z", lastActAt: null, expiresAt: null, mode: "watch", edge: null, actIndex: null,
  stepIndex: 0, stepCount: 3, ...extra,
});

const pick = (name: string) => userEvent.click(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button", { name: new RegExp(`^Flow ${name}`) }));

describe("JourneyFlows", () => {
  it("lists every journey compactly and opens the first one waiting for approval", () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} />);
    const rows = within(screen.getByRole("list", { name: "Journeys" })).getAllByRole("button");
    expect(rows.map((row) => row.textContent)).toEqual([
      "Flow doneApproved3 steps",
      "Flow checkoutWaiting for your approval3 steps",
      "Flow brokenWaiting for your approval3 steps",
      "Flow editedChanged since you approved it3 steps",
    ]);
    expect(rows[1]).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByRole("article")).toHaveLength(1);
    expect(screen.getByRole("article", { name: "Journey Flow checkout" })).toBeInTheDocument();
  });

  it("shows the selected journey's steps as what it does and what it sees, never codes or paths", async () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} />);
    const steps = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(steps.map((step) => step.textContent)).toEqual([
      "1Sees the “Cart” heading",
      "2Does Clicks “Checkout”Sees the “Password” field",
      "3Does Fills in “Password”, then Submits with “Pay now”Sees the “Order placed” heading",
    ]);
    await pick("broken");
    await pick("edited");
    const section = screen.getByRole("region", { name: "Journey flows" });
    for (const raw of ["flow.", "drift.", "/journeys/", "/approved", "?session=", "/checkout", "cart.checkout"]) {
      expect(section.textContent).not.toContain(raw);
    }
  });

  // #517: a titled screen read as its selector list ("the “Cart” heading, …") instead of its
  // plain title; the list is what the replay checks and belongs behind a toggle.
  it("says what a titled screen shows by its title and folds the checked controls under details", async () => {
    const base = flow("checkout");
    const titled: JourneyFlowsView = { flows: [{ ...base, screens: base.screens.map((item) => item.id === "cart" ? { ...item, title: "Your cart, with the total" } : item) }] };
    render(<JourneyFlows view={titled} onApprove={vi.fn()} />);
    const [first, second] = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(within(first!).getByText("Sees").parentElement).toHaveTextContent(/^Sees Your cart, with the total$/);
    // A screen without a title still reads by what is checked.
    expect(second).toHaveTextContent("Sees the “Password” field");
    // #519: the checked controls fold inside the expanded card.
    await userEvent.click(within(first!).getByRole("button"));
    const details = within(screen.getByRole("dialog", { name: "Step 1: Your cart, with the total" })).getByRole("group");
    expect(details).not.toHaveAttribute("open");
    expect(details).toHaveTextContent(/^detailsthe “Cart” heading$/);
  });

  it("offers Approve only where the Runtime would accept it, and says why not in words", async () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} />);
    expect(within(screen.getByRole("article")).getByRole("button", { name: "Approve" })).toBeEnabled();

    await pick("broken");
    const broken = screen.getByRole("article", { name: "Journey Flow broken" });
    expect(within(broken).getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(broken).toHaveTextContent("The agent still has to fix this journey before you can approve it.");

    await pick("done");
    expect(within(screen.getByRole("article", { name: "Journey Flow done" })).queryByRole("button", { name: "Approve" })).toBeNull();

    await pick("edited");
    const edited = screen.getByRole("article", { name: "Journey Flow edited" });
    expect(edited).toHaveTextContent("The app changed since this journey was recorded");
    expect(within(edited).getByRole("button", { name: "Approve" })).toBeEnabled();
  });

  it("approves through the callback and shows the Runtime's refusal", async () => {
    const onApprove = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("approve requires a canonical flow"));
    render(<JourneyFlows view={view} onApprove={onApprove} />);
    await userEvent.click(within(screen.getByRole("article")).getByRole("button", { name: "Approve" }));
    expect(onApprove).toHaveBeenCalledWith("checkout");
    expect(screen.queryByRole("alert")).toBeNull();

    await pick("edited");
    await userEvent.click(within(screen.getByRole("article")).getByRole("button", { name: "Approve" }));
    expect(onApprove).toHaveBeenLastCalledWith("edited");
    expect(await screen.findByRole("alert")).toHaveTextContent("approve requires a canonical flow");
  });

  it("renders nothing without flows and the failure when the read failed", () => {
    const { container } = render(<JourneyFlows view={{ flows: [] }} onApprove={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
    cleanup();
    render(<JourneyFlows view={null} failure="journey flows require an explicit --project on this Runtime" onApprove={vi.fn()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("--project");
  });
});

// #465: a title's parenthetical is a note, not the name; the selection is reported upward so the
// proof map follows it.
describe("JourneyFlows titles and selection", () => {
  it("moves a title's draft note out of the name into a why-line", () => {
    const noted: JourneyFlowsView = { flows: [flow("route", { title: "Owner adds a model route (draft: the fixture Runtime has no gateway manifest)" })] };
    render(<JourneyFlows view={noted} onApprove={vi.fn()} />);
    expect(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button")).toHaveTextContent(/^Owner adds a model routeWaiting/);
    const detail = screen.getByRole("article", { name: "Journey Owner adds a model route" });
    expect(within(detail).getByRole("heading")).toHaveTextContent(/^Owner adds a model route$/);
    expect(detail).toHaveTextContent("Why it is a draft: the fixture Runtime has no gateway manifest");
  });

  it("reports the selected journey and its status", async () => {
    const onSelect = vi.fn();
    render(<JourneyFlows view={view} onApprove={vi.fn()} onSelect={onSelect} />);
    expect(onSelect).toHaveBeenLastCalledWith("checkout", "draft");
    await pick("done");
    expect(onSelect).toHaveBeenLastCalledWith("done", "approved");
  });
});

// #466 review: Approve approves every path of a flow, so every path is shown before approval —
// a second path (`cancel`) was approved unread when only `main` was drawn. Cost: jsdom.
describe("JourneyFlows paths", () => {
  const twoWays: JourneyFlowsView = { flows: [flow("checkout", {
    screens: [...flow("x").screens, { id: "cancelled", url: "/cart", state: "stable", expect: [{ role: "heading", name: "Order cancelled" }] }],
    edges: [...flow("x").edges, { id: "cart.cancel", from: "cart", to: "cancelled", acts: [{ kind: "activate", role: "button", name: "Cancel order" }] }],
    paths: { main: ["cart.checkout", "pay.submit"], cancel: ["cart.cancel"] },
  })] };

  it("draws every path in one flowchart, a second way as a side branch, and watches the one asked for", async () => {
    const onWatch = vi.fn(async () => undefined);
    render(<JourneyFlows view={twoWays} onApprove={vi.fn()} onWatch={onWatch} />);
    expect(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button")).toHaveTextContent("3 steps · 2 ways");
    expect(screen.getByRole("heading", { name: "Also approved: the “cancel” way" })).toBeInTheDocument();
    // The shared first screen is drawn once; the cancel way adds its own screen below the main row.
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(cards.map((card) => card.textContent)).toEqual([
      "1Sees the “Cart” heading",
      "2Does Clicks “Checkout”Sees the “Password” field",
      "3Does Fills in “Password”, then Submits with “Pay now”Sees the “Order placed” heading",
      "4Does Clicks “Cancel order”Sees the “Order cancelled” heading",
    ]);
    expect(cards.map((card) => card.style.top)).toEqual(["0px", "0px", "0px", cards[3]!.style.top]);
    expect(cards[3]!.style.top).not.toBe("0px");
    expect(cards[3]!.style.left).toBe(cards[1]!.style.left);
    await userEvent.click(screen.getByRole("button", { name: "Watch “cancel”" }));
    expect(onWatch).toHaveBeenCalledWith("checkout", "cancel");
  });

  it("lights the watched step only on the path being played", () => {
    render(<JourneyFlows view={twoWays} onApprove={vi.fn()} onWatch={vi.fn()}
      sessions={[watchRow({ path: "cancel", edge: "cart.cancel", actIndex: 0, stepCount: 2 })]} />);
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem").map((step) => step.getAttribute("aria-current")))
      .toEqual([null, null, null, "step"]);
    expect(screen.getByRole("status")).toHaveTextContent("Playing step 2 of 2…");
  });
});

// #465: Watch plays the journey in a headed browser (#462) while the step being played lights here.
describe("JourneyFlows watch", () => {
  it("starts a watch of the selected journey and lights the step its session is on", async () => {
    const onWatch = vi.fn(() => new Promise<void>(() => {}));
    const { rerender } = render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={onWatch} sessions={[]} />);
    await userEvent.click(screen.getByRole("button", { name: "Watch" }));
    expect(onWatch).toHaveBeenCalledWith("checkout");

    // The act of the second edge is about to run: step 3 is the one being played.
    rerender(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={onWatch}
      sessions={[watchRow({ flowId: "other" }), watchRow({ edge: "pay.submit", actIndex: 0, screen: "pay", stepIndex: 1 })]} />);
    const steps = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(steps.map((step) => step.getAttribute("aria-current"))).toEqual([null, null, "step"]);
    expect(screen.getByRole("status")).toHaveTextContent("Playing step 3 of 3…");
    expect(screen.getByRole("button", { name: "Playing…" })).toBeDisabled();

    // Between edges the session names the screen just seen.
    rerender(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={onWatch} sessions={[watchRow({ screen: "pay", stepIndex: 1 })]} />);
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem").map((step) => step.getAttribute("aria-current")))
      .toEqual([null, "step", null]);
  });

  it("says how the play ended and offers Watch again", () => {
    const { rerender } = render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()} sessions={[watchRow({ state: "pass", screen: "done", stepIndex: 2 })]} />);
    expect(screen.getByRole("status")).toHaveTextContent("Played to the end");
    expect(screen.getByRole("button", { name: "Watch" })).toBeEnabled();
    rerender(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()} sessions={[watchRow({ state: "drift", edge: "cart.checkout", actIndex: 0 })]} />);
    expect(screen.getByRole("status")).toHaveTextContent("Stopped at step 2: the app no longer matches this step.");
  });

  // #515: a draft's destructive-looking act is not performed in a watch. The owner must read WHAT
  // was skipped and that it did not happen; defect: the row's new state falls to the default
  // words ("Step 2 of 3") and the play looks like it merely paused.
  it("says which act a watch of a draft skipped, on the step that act leads to", () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()} sessions={[watchRow({ state: "skipped", code: "watch.act_skipped_destructive", at: "pay.submit/1", screen: "pay", stepIndex: 1,
      skipped: { edge: "pay.submit", actIndex: 1, kind: "submit", role: "button", name: "Pay now", would: "pay" } })]} />);
    expect(screen.getByRole("status")).toHaveTextContent("Stopped at step 3 — skipped: would pay (“Pay now”). A draft never does that when watched.");
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem").map((step) => step.getAttribute("aria-current"))).toEqual([null, null, "step"]);
    expect(screen.getByRole("button", { name: "Watch" })).toBeEnabled();
  });

  // Owner report: Watch answered 400 `replay.observer_missing` and the button "did nothing": the
  // message sat under the whole step list in a scrolling box, in the Runtime's own words.
  it("says why a watch could not start right under the buttons, in words, and retries", async () => {
    const refusal = Object.assign(new Error("OBSERVER_MISSING: run setup --install-observer playwright"), { code: "replay.observer_missing" });
    const onWatch = vi.fn().mockRejectedValueOnce(refusal).mockResolvedValueOnce(undefined);
    render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={onWatch} />);
    await userEvent.click(screen.getByRole("button", { name: "Watch" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Can't play this journey: the browser player isn't installed in this project yet.");
    expect(alert).not.toHaveTextContent("OBSERVER_MISSING");
    const buttons = screen.getByRole("button", { name: "Watch" }).parentElement!;
    expect(buttons.nextElementSibling).toBe(alert);
    await userEvent.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(onWatch).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  // #475's `watch.app_down` (the app is down and the project declares no launcher) read as the
  // Runtime's raw message; it is the same owner fact as an unreachable app. Cost: jsdom.
  it("says the app isn't running when the Runtime refuses with watch.app_down", async () => {
    const refusal = Object.assign(new Error("watch.app_down: http://127.0.0.1:5184 did not answer"), { code: "watch.app_down" });
    render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn().mockRejectedValue(refusal)} />);
    await userEvent.click(screen.getByRole("button", { name: "Watch" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Can't play this journey: the app it opens isn't running.");
  });

  it("shows the Runtime's refusal of a watch and hides Watch on a Runtime without it", async () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn().mockRejectedValue(new Error("the app did not start"))} />);
    await userEvent.click(screen.getByRole("button", { name: "Watch" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("the app did not start");
    cleanup();
    render(<JourneyFlows view={view} onApprove={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Watch" })).toBeNull();
  });
});

// #421: a task graph's journey chip opens the Journey tab ON that journey, by flow id or by a
// path's contract id `<flow>.<path>`. Cost: jsdom.
describe("JourneyFlows focus", () => {
  it("selects and focuses the journey a chip named", () => {
    for (const focusId of ["broken", "broken.alt"]) {
      render(<JourneyFlows view={view} onApprove={vi.fn()} focusFlowId={focusId} />);
      expect(screen.getByRole("article", { name: "Journey Flow broken" })).toBeInTheDocument();
      expect(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button", { name: /^Flow broken/ })).toHaveAttribute("aria-pressed", "true");
      expect(document.activeElement).toHaveClass("journey-flow-detail");
      cleanup();
    }
  });

  it("keeps the default selection for an id no flow has", () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} focusFlowId="no-such-flow" />);
    expect(screen.getByRole("article", { name: "Journey Flow checkout" })).toBeInTheDocument();
  });
});

// #519 (owner: opening a journey runs its test; each card is the emulated screen; a click expands
// it inside the Studio). Regressions caught: a journey opened without its test starting, a card
// without the picture or the result of its step, a failure shown as a code instead of words, a
// card that opens a window instead of an in-app view, a Watch whose live page never shows, and a
// Run again that reads the cache. Cost: jsdom render, fake timers for the one-second poll, no network.
describe("JourneyFlows run", () => {
  const one: JourneyFlowsView = { flows: [flow("checkout")] };
  const frame = (name: string) => ({ blob: new Blob([name], { type: "image/jpeg" }), etag: `"${name}"` });
  const source = (extra: Partial<JourneyRunSource> = {}): JourneyRunSource => ({
    start: vi.fn(async () => ({ state: "ready" as const, kind: "preview" as const, digest: "d1", result: "fail" as const, ranAt: "2026-10-08T21:00:00Z",
      screens: { cart: { frame: true, result: "pass" as const }, pay: { frame: true, result: "fail" as const, reason: "expect_missing", seen: "Sign in" }, done: { frame: false, reason: "not_reached" } },
      edges: { "cart.checkout": { result: "pass" as const } } })),
    read: vi.fn(async () => ({ state: "none" as const })),
    screenFrame: vi.fn(async (_flow: string, screenId: string) => frame(screenId)),
    liveFrame: vi.fn(async () => null),
    ...extra,
  });
  const urls = () => {
    const made: string[] = [];
    vi.stubGlobal("URL", Object.assign(URL, {
      createObjectURL: vi.fn((blob: Blob) => { const url = `blob:frame-${made.length}-${blob.size}`; made.push(url); return url; }),
      revokeObjectURL: vi.fn(),
    }));
    return made;
  };
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

  it("runs the journey's test when it is opened and shows each card's picture and result in words", async () => {
    urls();
    const run = source();
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={run} />);
    expect(run.start).toHaveBeenCalledWith("checkout", false);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    await waitFor(() => expect(cards[0]!.querySelector("img")).not.toBeNull());
    expect(cards.map((card) => card.textContent)).toEqual([
      "1PassedSees the “Cart” heading",
      "2FailedDoes Clicks “Checkout”Sees the “Password” field",
      "3Not reachedDoes Fills in “Password”, then Submits with “Pay now”Sees the “Order placed” heading",
    ]);
    expect(cards[2]!.querySelector("img")).toBeNull();
    expect(run.screenFrame).toHaveBeenCalledTimes(2);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent(/^Test failed \(a preview: a draft's run is never proof\) · ran /);
    expect(screen.getByRole("region", { name: "Journey flows" }).textContent).not.toContain("expect_missing");
  });

  it("expands a card inside the Studio with the reason in words, and closes on Escape or a click outside", async () => {
    urls();
    const open = vi.spyOn(window, "open");
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={source()} />);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    await waitFor(() => expect(cards[1]!.querySelector("img")).not.toBeNull());
    await userEvent.click(within(cards[1]!).getByRole("button"));
    const dialog = screen.getByRole("dialog", { name: "Step 2: pay" });
    expect(dialog).toHaveTextContent("Why: something this screen should show was not there (the page showed: Sign in).");
    expect(within(dialog).getByRole("img", { name: "The screen at step 2" })).toHaveAttribute("src", cards[1]!.querySelector("img")!.getAttribute("src"));
    expect(open).not.toHaveBeenCalled();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    await userEvent.click(within(cards[0]!).getByRole("button"));
    await userEvent.click(screen.getByRole("dialog").parentElement!);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("reads a running test once a second, fills cards as screens are reached, and Run again forces a new run", async () => {
    urls();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const running = { state: "running" as const, digest: "d1", current: "pay", screens: { cart: { frame: true, result: "pass" as const } } };
    const ready = { state: "ready" as const, kind: "replay" as const, digest: "d1", result: "pass" as const,
      screens: { cart: { frame: true, result: "pass" as const }, pay: { frame: true, result: "pass" as const }, done: { frame: true, result: "pass" as const } } };
    const run = source({ start: vi.fn(async () => running), read: vi.fn(async () => ready) });
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={run} />);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    await waitFor(() => expect(cards[1]).toHaveTextContent("Running…"));
    expect(screen.getByRole("status")).toHaveTextContent("Running this journey's test…");
    expect(screen.getByRole("button", { name: "Run again" })).toBeDisabled();
    expect(run.read).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1000);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(/^Test passed$/));
    expect(run.read).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(cards[2]!.querySelector("img")).not.toBeNull());
    await userEvent.click(screen.getByRole("button", { name: "Run again" }));
    expect(run.start).toHaveBeenLastCalledWith("checkout", true);
  });

  it("says a step the guard stopped was skipped, with the reason in words, not that it was lost", async () => {
    urls();
    const start = vi.fn(async () => ({ state: "ready" as const, kind: "preview" as const, digest: "d1", result: "pass" as const,
      screens: { cart: { frame: true, result: "pass" as const }, pay: { frame: false, reason: "not_reached" }, done: { frame: false, reason: "not_reached" } },
      edges: { "cart.checkout": { result: "skipped" as const, reason: "guard_refused" }, "pay.submit": { result: null, reason: "not_reached" } } }));
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={source({ start })} />);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    await waitFor(() => expect(cards[1]).toHaveTextContent(/^2Skipped/));
    expect(cards[2]).toHaveTextContent(/^3Not reached/);
    await userEvent.click(within(cards[1]!).getByRole("button"));
    expect(screen.getByRole("dialog")).toHaveTextContent("Why: the test stopped before an action that would delete or cancel something.");
  });

  it("says in words why the test could not run", async () => {
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={source({ start: vi.fn(async () => ({ state: "failed" as const, reason: "watch.app_down" })) })} />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Couldn't run this journey's test: the app it opens isn't running."));
    expect(screen.getByRole("button", { name: "Run again" })).toBeEnabled();
    cleanup();
    // A launcher that fails or hangs is its own code, not "something went wrong in the Runtime".
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={source({ start: vi.fn(async () => ({ state: "failed" as const, reason: "watch.launch_failed" })) })} />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Couldn't run this journey's test: the app under test didn't start."));
  });

  it("draws the flowchart without a run line on a Runtime that has no run route", async () => {
    const start = vi.fn(async () => { throw Object.assign(new Error("The Runtime replied 404."), { httpStatus: 404 }); });
    render(<JourneyFlows view={one} onApprove={vi.fn()} run={source({ start })} />);
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(screen.queryByRole("button", { name: "Run again" })).toBeNull();
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem")).toHaveLength(3);
  });

  it("shows the live page on the card a Watch is playing", async () => {
    const made = urls();
    const liveFrame = vi.fn(async () => frame("live-page"));
    render(<JourneyFlows view={one} onApprove={vi.fn()} onWatch={vi.fn()} run={source({ liveFrame, start: vi.fn(async () => ({ state: "none" as const })) })}
      sessions={[watchRow({ edge: "cart.checkout", actIndex: 0, frame: true })]} />);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    await waitFor(() => expect(cards[1]!.querySelector("img")).not.toBeNull());
    expect(liveFrame).toHaveBeenCalledWith("w1", null);
    expect(cards[1]).toHaveAttribute("aria-current", "step");
    expect(cards[1]!.querySelector("img")!.getAttribute("src")).toBe(made[made.length - 1]);
    expect(cards[0]!.querySelector("img")).toBeNull();
  });
});

// #505: the owner reads both screens; the Studio shows what the act the watch is about to run does.
describe("JourneyFlows watch caption", () => {
  it("shows the act's caption under the readout while playing, and nothing between acts or after", () => {
    const { container, rerender } = render(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()}
      sessions={[watchRow({ edge: "cart.checkout", actIndex: 0, caption: "cart.checkout: Clicks “Checkout”" })]} />);
    const caption = () => container.querySelector(".journey-flow-watch-caption");
    // The owner reads what the step does, never the edge id (#505 review).
    expect(caption()?.textContent).toBe("Clicks “Checkout”");
    rerender(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()} sessions={[watchRow({ screen: "pay", stepIndex: 1, caption: null })]} />);
    expect(caption()).toBeNull();
    rerender(<JourneyFlows view={view} onApprove={vi.fn()} onWatch={vi.fn()}
      sessions={[watchRow({ state: "pass", caption: "pay.submit: Clicks “Pay”" })]} />);
    expect(caption()).toBeNull();
  });
});

// #518: the owner marks a draft's skipped step safe to watch. Regressions caught: a Mark safe
// that hides one of the edge's acts (one mark covers them all), a mark offered on an approved
// flow or to a Studio without the door, a stale mark read as valid, a refusal swallowed, and a
// skipped step of the journey's own run left without the button. Cost: jsdom render, no network.
describe("JourneyFlows mark safe", () => {
  const one = (extra: Partial<JourneyFlowView> = {}): JourneyFlowsView => ({ flows: [flow("checkout", extra)] });
  const stopped = watchRow({ state: "skipped", edge: null, screen: "pay", stepIndex: 1,
    skipped: { edge: "pay.submit", actIndex: 1, kind: "submit", role: "button", name: "Pay now", would: "pay" } });
  const marked = (base: JourneyFlowView) => base.edges.map((edge) => edge.id === "pay.submit" ? { ...edge, safe: { digest: `sha256:${"c".repeat(64)}` } } : edge);

  it("shows every act of the skipped step before the owner marks it safe, then says it is marked", async () => {
    const onMarkSafe = vi.fn(async () => undefined);
    const { rerender } = render(<JourneyFlows view={one()} onApprove={vi.fn()} onWatch={vi.fn()} onMarkSafe={onMarkSafe} sessions={[stopped]} />);
    const item = within(screen.getByRole("list", { name: "Skipped steps" })).getByRole("listitem");
    expect(item).toHaveTextContent("This step does: Fills in “Password”, then Submits with “Pay now”.");
    await userEvent.click(within(item).getByRole("button", { name: "Mark safe" }));
    expect(onMarkSafe).toHaveBeenCalledWith("checkout", "pay.submit");

    const base = flow("checkout");
    rerender(<JourneyFlows view={one({ edges: marked(base) })} onApprove={vi.fn()} onWatch={vi.fn()} onMarkSafe={onMarkSafe} sessions={[stopped]} />);
    const after = within(screen.getByRole("list", { name: "Skipped steps" })).getByRole("listitem");
    expect(after).toHaveTextContent("Marked safe. Watch it again to play this step.");
    expect(within(after).queryByRole("button")).toBeNull();

    // The edge changed after the mark: the Runtime says the mark is void, so it is offered again.
    rerender(<JourneyFlows view={one({ edges: marked(base), findings: [{ code: "flow.safe_stale", pointer: "/edges/1/safe", message: "void", severity: "warning" }] })}
      onApprove={vi.fn()} onWatch={vi.fn()} onMarkSafe={onMarkSafe} sessions={[stopped]} />);
    expect(within(screen.getByRole("list", { name: "Skipped steps" })).getByRole("button", { name: "Mark safe" })).toBeEnabled();
  });

  it("offers the mark for a step the journey's own run skipped, and shows the Runtime's refusal", async () => {
    const onMarkSafe = vi.fn(async () => { throw new Error("only a draft's act is marked safe"); });
    const run: JourneyRunSource = {
      start: vi.fn(async () => ({ state: "ready" as const, kind: "preview" as const, result: "pass" as const, screens: {}, edges: { "cart.checkout": { result: "skipped" as const, reason: "guard_refused" } } })),
      read: vi.fn(async () => ({ state: "none" as const })), screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null),
    };
    render(<JourneyFlows view={one()} onApprove={vi.fn()} onMarkSafe={onMarkSafe} run={run} />);
    const item = await screen.findByText(/This step does: Clicks “Checkout”\./);
    await userEvent.click(within(item.closest("li")!).getByRole("button", { name: "Mark safe" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't mark this step safe: only a draft's act is marked safe");
  });

  it("offers no mark on an approved flow, or without the owner's door", () => {
    const approved = one({ status: "approved", approvable: false, approved: { revision: "a".repeat(40), digest: `sha256:${"b".repeat(64)}` } });
    const { rerender } = render(<JourneyFlows view={approved} onApprove={vi.fn()} onWatch={vi.fn()} onMarkSafe={vi.fn()} sessions={[stopped]} />);
    expect(screen.queryByRole("list", { name: "Skipped steps" })).toBeNull();
    rerender(<JourneyFlows view={one()} onApprove={vi.fn()} onWatch={vi.fn()} sessions={[stopped]} />);
    expect(screen.queryByRole("list", { name: "Skipped steps" })).toBeNull();
  });
});

// #548: an approved flow plays by itself only up to its first act that changes data, then waits
// for the owner. Regressions caught: a held step shown as an ordinary skip (or offered Mark safe,
// which is a draft's door), the owner not told what would run or where, a click that re-runs
// with `force` instead of `confirm` (so the Runtime holds again), and a confirm sent without a
// click. Cost: jsdom render, no network.
describe("JourneyFlows held step", () => {
  const approved: JourneyFlowsView = { flows: [flow("checkout", { status: "approved", approvable: false, approved: { revision: "a".repeat(40), digest: `sha256:${"b".repeat(64)}` } })] };
  const held = { state: "ready" as const, kind: "replay" as const, result: "pass" as const, held: { edge: "pay.submit", act: "Pay now", base: "http://localhost:3000" },
    screens: { cart: { frame: false, result: "pass" as const }, pay: { frame: false, result: "pass" as const }, done: { frame: false, reason: "not_reached" } },
    edges: { "cart.checkout": { result: "pass" as const }, "pay.submit": { result: "skipped" as const, reason: "confirm_needed" } } };
  const done = { state: "ready" as const, kind: "replay" as const, result: "pass" as const, screens: {}, edges: {} };

  it("says what the held step would do and where, and runs it only on the owner's click, as a confirm", async () => {
    const start = vi.fn(async (_flow: string, _force: boolean, confirm?: boolean) => (confirm ? done : held));
    const run: JourneyRunSource = { start, read: vi.fn(async () => held), screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null) };
    render(<JourneyFlows view={approved} onApprove={vi.fn()} onMarkSafe={vi.fn()} run={run} />);
    const bar = await screen.findByRole("group", { name: "Step waiting for you" });
    expect(bar).toHaveTextContent("The next step changes data at http://localhost:3000: Fills in “Password”, then Submits with “Pay now”. Run it?");
    expect(screen.getByRole("status")).toHaveTextContent(/^Stopped before a step that changes data/);
    const cards = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(cards[2]).toHaveTextContent(/^3Waiting for you/);
    expect(screen.queryByRole("list", { name: "Skipped steps" })).toBeNull();
    expect(start.mock.calls).toEqual([["checkout", false]]);

    await userEvent.click(within(bar).getByRole("button", { name: "Run it" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "Step waiting for you" })).toBeNull());
    expect(start.mock.calls).toEqual([["checkout", false], ["checkout", false, true]]);
    expect(screen.getByRole("status")).toHaveTextContent(/^Test passed/);
  });

  // The reviewer's note on #565: nothing showed that the click was taken. The Runtime ignores a
  // second confirm while one runs, but the owner could not see that.
  it("shows the Run it click was taken until the Runtime answers", async () => {
    let answer: (value: typeof done) => void = () => undefined;
    const start = vi.fn((_flow: string, _force: boolean, confirm?: boolean) => (confirm ? new Promise<typeof done>((resolve) => { answer = resolve; }) : Promise.resolve(held)));
    const run: JourneyRunSource = { start, read: vi.fn(async () => held), screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null) };
    render(<JourneyFlows view={approved} onApprove={vi.fn()} run={run} />);
    await userEvent.click(await screen.findByRole("button", { name: "Run it" }));
    const pending = screen.getByRole("button", { name: "Running…" });
    expect(pending).toBeDisabled();
    await userEvent.click(pending);
    expect(start.mock.calls.filter((call) => call[2] === true)).toHaveLength(1);
    answer(done);
    await waitFor(() => expect(screen.queryByRole("group", { name: "Step waiting for you" })).toBeNull());
  });

  it("offers no Run it when nothing is held", async () => {
    const run: JourneyRunSource = { start: vi.fn(async () => done), read: vi.fn(async () => done), screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null) };
    render(<JourneyFlows view={approved} onApprove={vi.fn()} run={run} />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(/^Test passed/));
    expect(screen.queryByRole("button", { name: "Run it" })).toBeNull();
  });
});
