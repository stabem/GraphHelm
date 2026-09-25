import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DocumentEditor } from "./document-editor";

const document = { evidenceId: "ev", index: 0, path: "docs/rule.md", title: "Refund rule", projectId: "a".repeat(64) };
const readDocument = async () => ({ content: "Original rule", contentSha256: "old" });
describe("project document editor", () => {
  it("keeps a conflicted draft and its reason and guards close", async () => {
    const onClose = vi.fn();
    const saveDocument = vi.fn().mockRejectedValue({ httpStatus: 409, diagnostics: [{ path: "/expectedSha256" }] });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={onClose} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "Owner revision" } });
    expect(screen.getByRole("button", { name: "Save project file" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "Changed return period" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("file changed");
    expect(screen.getByLabelText("File content")).toHaveValue("Owner revision");
    fireEvent.click(screen.getByRole("button", { name: "Close document editor" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
  it("retries an uncertain save with the same key and distinguishes notification from acknowledgement", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockResolvedValue({ content: "New rule", contentSha256: "new" });
    const saveDocument = vi.fn().mockRejectedValueOnce(new Error("network"))
      .mockResolvedValue({ contentSha256: "new", notification: { status: "pending", notifiedRuns: [], pendingRuns: ["run-2"] } });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(2));
    expect(saveDocument.mock.calls[0][1]).toEqual(saveDocument.mock.calls[1][1]);
    expect(readDocument).toHaveBeenCalledTimes(2);
    expect(await screen.findByText(/Some run notifications are still pending/)).toHaveTextContent("has not been confirmed");
    expect(screen.getByLabelText("File content")).toHaveValue("New rule");
    expect(screen.getByRole("button", { name: "Save project file" })).toBeDisabled();
  });
  it("keeps the original uncertain request after the draft is edited", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockResolvedValue({ content: "First revision", contentSha256: "new" });
    const saveDocument = vi.fn().mockRejectedValueOnce(new Error("network")).mockResolvedValueOnce({
      contentSha256: "new",
      notification: { status: "recorded", notifiedRuns: ["run-2"], pendingRuns: [] },
    });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "First revision" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "First reason" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Second draft" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "Second reason" } });
    expect(screen.getByRole("button", { name: "Save project file" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(2));
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
    expect(screen.getByLabelText("File content")).toHaveValue("Second draft");
    expect(screen.getByLabelText("Why are you changing this?")).toHaveValue("Second reason");
  });
  it("keeps an uncertain retry visible after the draft returns to the original content", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockResolvedValue({ content: "New rule", contentSha256: "new" });
    const saveDocument = vi.fn().mockRejectedValueOnce(new Error("network")).mockResolvedValueOnce({
      contentSha256: "new",
      notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] },
    });
    const onClose = vi.fn();
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={onClose} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Original rule" } });
    expect(screen.getByRole("button", { name: "Retry save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Close document editor" }));
    expect(screen.getByRole("alertdialog")).toHaveTextContent("may have committed");
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(2));
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
  });
  it("prioritizes uncertain save abandonment when closing a dirty draft", async () => {
    const saveDocument = vi.fn().mockRejectedValueOnce(new Error("network"));
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Second draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Close document editor" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("may have committed");
    expect(dialog).toHaveTextContent("retry key");
    expect(dialog).toHaveTextContent("pending run notifications");
  });
  it("cannot discard an open draft dialog while its save is in flight", async () => {
    let resolveSave!: (value: { contentSha256: string; notification: { status: "recorded"; notifiedRuns: string[]; pendingRuns: string[] } }) => void;
    const saveDocument = vi.fn(() => new Promise<{
      contentSha256: string;
      notification: { status: "recorded"; notifiedRuns: string[]; pendingRuns: string[] };
    }>((resolve) => { resolveSave = resolve; }));
    const onClose = vi.fn();
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={onClose} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Close document editor" }));
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    const discard = screen.getByRole("button", { name: "Discard draft" });
    expect(discard).toBeDisabled();
    fireEvent.click(discard);
    expect(onClose).not.toHaveBeenCalled();
    resolveSave({ contentSha256: "new", notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] } });
    await waitFor(() => expect(discard).toBeEnabled());
  });
  it("abandons an uncertain request after a definitive 400 so the corrected draft gets a new key", async () => {
    const saveDocument = vi.fn()
      .mockRejectedValueOnce(new Error("network"))
      .mockRejectedValueOnce({ httpStatus: 400 })
      .mockResolvedValueOnce({ contentSha256: "new", notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] } });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "First draft" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "First reason" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Corrected draft" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "Corrected reason" } });
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("alert")).toHaveTextContent("Correct it and save again");
    expect(screen.queryByRole("button", { name: "Retry save" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save project file" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(3));
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
    expect(saveDocument.mock.calls[2][1].content).toBe("Corrected draft");
    expect(saveDocument.mock.calls[2][1].reason).toBe("Corrected reason");
    expect(saveDocument.mock.calls[2][1].idempotencyKey).not.toBe(saveDocument.mock.calls[0][1].idempotencyKey);
  });
  it("blocks saving after an expected revision conflict diagnostic", async () => {
    const saveDocument = vi.fn().mockRejectedValue({ httpStatus: 409, diagnostics: [{ path: "/expectedSha256" }] });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "Owner revision" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "Changed return period" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("file changed");
    expect(screen.getByRole("button", { name: "Save project file" })).toBeDisabled();
  });
  it("keeps a retryable save after a 409 document rejection", async () => {
    const saveDocument = vi.fn().mockRejectedValue({ httpStatus: 409, diagnostics: [{ path: "/document" }] });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "Owner revision" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "Changed return period" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: "Retry save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await waitFor(() => expect(saveDocument).toHaveBeenCalledTimes(2));
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
  });
  it("keeps retry after a post-save reread failure", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockRejectedValueOnce({ httpStatus: 400 });
    const saveDocument = vi.fn().mockRejectedValueOnce(new Error("network")).mockResolvedValueOnce({
      contentSha256: "new",
      notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] },
    });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: "Retry save" })).toBeEnabled();
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
  });
  it("retries pending notices with the original save while keeping a newer draft", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockResolvedValue({ content: "First revision", contentSha256: "new" });
    const saveDocument = vi.fn()
      .mockResolvedValueOnce({ contentSha256: "new", notification: { status: "pending", notifiedRuns: [], pendingRuns: ["run-2"] } })
      .mockResolvedValueOnce({ contentSha256: "new", notification: { status: "recorded", notifiedRuns: ["run-2"], pendingRuns: [] } });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "First revision" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("button", { name: "Retry run notices" });
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Second draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Retry run notices" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Retry run notices" })).not.toBeInTheDocument());
    expect(saveDocument.mock.calls[1][1]).toEqual(saveDocument.mock.calls[0][1]);
    expect(screen.getByLabelText("File content")).toHaveValue("Second draft");
    expect(screen.getByText("Unsaved draft")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save project file" })).toBeEnabled();
  });
  it("re-reads the live revision after a notice retry and preserves the draft on change", async () => {
    const readDocument = vi.fn()
      .mockResolvedValueOnce({ content: "Original rule", contentSha256: "old" })
      .mockResolvedValueOnce({ content: "Someone else's revision", contentSha256: "other" });
    const saveDocument = vi.fn().mockResolvedValueOnce({
      contentSha256: "new",
      notification: { status: "pending", notifiedRuns: [], pendingRuns: ["run-2"] },
    }).mockResolvedValueOnce({
      contentSha256: "new",
      notification: { status: "recorded", notifiedRuns: ["run-2"], pendingRuns: [] },
    });
    render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
    fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "First revision" } });
    fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("button", { name: "Retry run notices" });
    fireEvent.change(screen.getByLabelText("File content"), { target: { value: "Second draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Retry run notices" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("file changed");
    expect(readDocument).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("File content")).toHaveValue("Second draft");
    expect(screen.getByText("Unsaved draft")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save project file" })).toBeDisabled();
  });
  it("uses the portable idempotency key fallback when randomUUID is unavailable", async () => {
    const originalRandomUUID = globalThis.crypto.randomUUID;
    Object.defineProperty(globalThis.crypto, "randomUUID", { value: undefined, configurable: true });
    try {
      const saveDocument = vi.fn().mockResolvedValue({
        contentSha256: "new",
        notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] },
      });
      render(<DocumentEditor document={document} readDocument={readDocument} saveDocument={saveDocument} onClose={vi.fn()} />);
      fireEvent.change(await screen.findByLabelText("File content"), { target: { value: "New rule" } });
      fireEvent.change(screen.getByLabelText("Why are you changing this?"), { target: { value: "New terms" } });
      fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
      await waitFor(() => expect(saveDocument).toHaveBeenCalledOnce());
      expect(saveDocument.mock.calls[0][1].idempotencyKey).toMatch(/^[0-9a-f]{32}$/);
      expect(screen.queryByText("Saving…")).not.toBeInTheDocument();
    } finally {
      Object.defineProperty(globalThis.crypto, "randomUUID", { value: originalRandomUUID, configurable: true });
    }
  });
});
