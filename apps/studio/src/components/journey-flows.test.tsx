// #353 / #465: the owner approves journeys in the Journey tab. Regressions caught: a long card list
// that buries the one to approve, raw finding codes, pointers and URLs shown to the owner, steps
// without what they do and show, an Approve offered on a flow the Runtime would refuse, a refusal
// swallowed, a settled approval offered again, and a Watch that does not light the step being
// played. Cost: jsdom render, no network.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { JourneyFlowView, JourneyFlowsView, LiveSession } from "../runtime/types";
import { JourneyFlows } from "./journey-flows";

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
  it("says what a titled screen shows by its title and folds the checked controls under details", () => {
    const base = flow("checkout");
    const titled: JourneyFlowsView = { flows: [{ ...base, screens: base.screens.map((item) => item.id === "cart" ? { ...item, title: "Your cart, with the total" } : item) }] };
    render(<JourneyFlows view={titled} onApprove={vi.fn()} />);
    const [first, second] = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(within(first!).getByText("Sees").parentElement).toHaveTextContent(/^Sees Your cart, with the total$/);
    const details = within(first!).getByRole("group");
    expect(details).not.toHaveAttribute("open");
    expect(details).toHaveTextContent(/^detailsthe “Cart” heading$/);
    // A screen without a title still reads by what is checked, with nothing to fold.
    expect(second).toHaveTextContent("Sees the “Password” field");
    expect(within(second!).queryByRole("group")).toBeNull();
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

  it("shows every path with its own steps, and watches the one asked for", async () => {
    const onWatch = vi.fn(async () => undefined);
    render(<JourneyFlows view={twoWays} onApprove={vi.fn()} onWatch={onWatch} />);
    expect(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button")).toHaveTextContent("3 steps · 2 ways");
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem")).toHaveLength(3);
    expect(screen.getByRole("heading", { name: "Also approved: the “cancel” way" })).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Steps: cancel" })).getAllByRole("listitem").map((step) => step.textContent)).toEqual([
      "1Sees the “Cart” heading",
      "2Does Clicks “Cancel order”Sees the “Order cancelled” heading",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Watch “cancel”" }));
    expect(onWatch).toHaveBeenCalledWith("checkout", "cancel");
  });

  it("lights the watched step only on the path being played", () => {
    render(<JourneyFlows view={twoWays} onApprove={vi.fn()} onWatch={vi.fn()}
      sessions={[watchRow({ path: "cancel", edge: "cart.cancel", actIndex: 0, stepCount: 2 })]} />);
    expect(within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem").every((step) => !step.hasAttribute("aria-current"))).toBe(true);
    expect(within(screen.getByRole("list", { name: "Steps: cancel" })).getAllByRole("listitem").map((step) => step.getAttribute("aria-current")))
      .toEqual([null, "step"]);
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
