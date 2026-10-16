import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { TestCanvas } from "./test-canvas";
import type { TestFrame } from "../runtime/test-frames";

const userEvent = fastUserEvent();

const frames: TestFrame[] = [
  { stepId: "a", n: 1, verb: "SEES", text: "Journey tab", status: "passed", reason: null, expected: ["tab selected"], inputs: [], intent: null, checks: [], checkStatus: [], failure: null },
  { stepId: "b", n: 2, verb: "DOES", text: "Click Mark safe", status: "waits_for_you", reason: "data-changing", expected: [], inputs: [], intent: null, checks: [], checkStatus: [], failure: null },
];

function setup(selected = 0, frameUrl = (id: string) => (id === "a" ? "blob:a" : null)) {
  const p = { frames, selected, onSelect: vi.fn(), frameUrl, onMarkSafe: vi.fn(), onSendBack: vi.fn() };
  render(<TestCanvas {...p} />);
  return p;
}

describe("TestCanvas", () => {
  it("says what the journey proves, what the step typed and every check (#1287)", () => {
    const strong: TestFrame[] = [{ stepId: "salvo", n: 1, verb: "DOES", text: "Submits “Salvar”", status: "passed", reason: null,
      expected: ["success"], inputs: ["Types “parceiro” into “Tag Amazon”", "Submits “Salvar”"], intent: "Tag Amazon as a partner", checks: ["shows cell “parceiro”"], checkStatus: ["passed"], failure: null }];
    render(<TestCanvas frames={strong} purpose="The owner tags a store and the tag stays after a reload." selected={0}
      onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.getByText("The owner tags a store and the tag stays after a reload.")).toBeInTheDocument();
    expect(screen.getByText("Tag Amazon as a partner")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Inputs" })).toHaveTextContent("Types “parceiro” into “Tag Amazon”");
    const checks = screen.getByRole("list", { name: "Assertions" });
    expect(checks).toHaveTextContent("shows cell “parceiro”");
    expect(checks).toHaveTextContent("state is success");
  });

  it("marks each assertion on its own and says what the screen showed (#1286)", () => {
    const red: TestFrame[] = [{ stepId: "s", n: 1, verb: "SEES", text: "Produto", status: "failed", reason: "expect_missing", expected: [], inputs: [], intent: null,
      checks: ["shows heading “Produto encontrado”", "shows region “Também encontramos em”", "shows button “Salvar item”"],
      checkStatus: ["failed", "passed", "passed"], failure: "Expected heading 'Produto encontrado' — the screen shows heading 'Produto conferido'" }];
    render(<TestCanvas frames={red} selected={0} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    const rows = screen.getByRole("list", { name: "Assertions" }).querySelectorAll("li");
    expect([...rows].map((row) => row.getAttribute("data-check"))).toEqual(["failed", "passed", "passed"]);
    expect([...rows].map((row) => row.querySelector(".tc-mark")!.textContent)).toEqual(["✕", "✓", "✓"]);
    expect(rows[1]).toHaveTextContent("(seen)");
    expect(screen.getByRole("status")).toHaveTextContent("Expected heading 'Produto encontrado' — the screen shows heading 'Produto conferido'");
  });

  it("flags a journey that does not say what it proves (#1287)", () => {
    setup(0);
    expect(screen.getByRole("note")).toHaveTextContent("This journey does not say what it proves");
  });

  it("shows the recorded frame in the emulated browser", () => {
    setup(0);
    expect(screen.getByRole("img", { name: "Frame 1: Journey tab" })).toHaveAttribute("src", "blob:a");
    expect(screen.getByText("emulated · frame 1/2")).toBeInTheDocument();
  });

  it("card click and next button move the frame", async () => {
    const p = setup(0);
    await userEvent.click(screen.getByRole("button", { name: /^2 DOES/ }));
    await userEvent.click(screen.getByRole("button", { name: "Next frame" }));
    expect(p.onSelect.mock.calls).toEqual([[1], [1]]);
  });

  it("previous is clamped at the first frame", async () => {
    const p = setup(0);
    await userEvent.click(screen.getByRole("button", { name: "Previous frame" }));
    expect(p.onSelect).toHaveBeenCalledWith(0);
  });

  it("a frame that waits for you offers mark safe and send back", async () => {
    const p = setup(1);
    expect(screen.getByText("No frame recorded for this step")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    await userEvent.click(screen.getByRole("button", { name: "Send back" }));
    expect(p.onMarkSafe).toHaveBeenCalledWith("b");
    expect(p.onSendBack).toHaveBeenCalledWith("b");
  });

  it("without a send-back handler, Send back is disabled and says why", () => {
    render(<TestCanvas frames={frames} selected={1} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Send back — not available yet" })).toBeDisabled();
  });

  it("a mark safe that fails shows the error beside the buttons", async () => {
    const onMarkSafe = vi.fn().mockRejectedValue(new Error("owner token refused"));
    render(<TestCanvas frames={frames} selected={1} onSelect={vi.fn()} frameUrl={() => null} onMarkSafe={onMarkSafe} />);
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Mark safe failed: owner token refused");
  });

  it("restyled inspector: code box, console placeholder, and a scrubber that jumps", async () => {
    const p = setup(0);
    expect(screen.getByText("No console captured")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Jump to frame 2" }));
    expect(p.onSelect).toHaveBeenCalledWith(1);
  });

  it("a passed frame offers no decision buttons", () => {
    setup(0);
    expect(screen.queryByRole("button", { name: "I watched it — mark safe" })).toBeNull();
  });
});
