import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";

import { useImageUrl } from "./use-image-url";

let counter = 0;
const create = vi.fn(() => `blob:fake-${++counter}`);
const revoke = vi.fn();

beforeEach(() => {
  counter = 0; create.mockClear(); revoke.mockClear();
  Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke });
});
afterEach(cleanup);

describe("useImageUrl", () => {
  it("makes a blob URL and revokes it on unmount", async () => {
    const load = vi.fn(() => Promise.resolve(new Blob(["x"], { type: "image/png" })));
    const { result, unmount } = renderHook(() => useImageUrl(load, "a"));
    await waitFor(() => expect(result.current.url).toBe("blob:fake-1"));
    unmount();
    expect(revoke).toHaveBeenCalledWith("blob:fake-1");
  });

  it("revokes the old URL when the key changes", async () => {
    const load = vi.fn(() => Promise.resolve(new Blob(["x"])));
    const { result, rerender } = renderHook(({ k }) => useImageUrl(load, k), { initialProps: { k: "a" } });
    await waitFor(() => expect(result.current.url).toBe("blob:fake-1"));
    rerender({ k: "b" });
    expect(revoke).toHaveBeenCalledWith("blob:fake-1");
    await waitFor(() => expect(result.current.url).toBe("blob:fake-2"));
  });

  it("revokes a load that resolves after unmount, and reports errors", async () => {
    let resolve: (blob: Blob) => void = () => undefined;
    const late = () => new Promise<Blob>((r) => { resolve = r; });
    const { unmount } = renderHook(() => useImageUrl(late, "a"));
    unmount();
    await act(async () => { resolve(new Blob(["x"])); await Promise.resolve(); });
    expect(revoke).toHaveBeenCalledWith("blob:fake-1");
    const { result } = renderHook(() => useImageUrl(() => Promise.reject(new Error("no")), "b"));
    await waitFor(() => expect(result.current.error).toBe(true));
  });
});
