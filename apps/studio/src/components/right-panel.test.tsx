import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { RightPanel } from "./right-panel";

const userEvent = fastUserEvent();
afterEach(cleanup);

describe("RightPanel", () => {
  it("shows honest empty states for journeys and before/after until phase 4 data exists", () => {
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Journeys" })).toHaveTextContent("No journeys mapped yet");
    expect(screen.getByRole("region", { name: "Before and after" })).toHaveTextContent("No before/after screenshots yet");
    expect(screen.getByRole("region", { name: "What just happened" })).toHaveTextContent("Nothing recorded yet");
  });

  it("lists what just happened as bot verb object, each opening its record", async () => {
    const onOpen = vi.fn();
    render(<RightPanel activity={[{ sequence: 7, text: "loja kit 1 asked you “Merge now?”", at: null }]} onOpenActivity={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: /loja kit 1 asked you/ }));
    expect(onOpen).toHaveBeenCalledWith(7);
  });
});
