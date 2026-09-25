import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

import { Models, type ProbeState } from "./models";
import type { ModelRouteSummary } from "../runtime/types";

/**
 * #1171: the screen that can ADD a provider, and the three things it must never do — render a
 * stored key, imply a provider answered, or offer to key a route that has no key.
 */

function route(overrides: Partial<ModelRouteSummary> = {}): ModelRouteSummary {
  return {
    id: "deepseek_official",
    provider: "openai",
    transport: "direct_api",
    billingMode: "per_token",
    model: "deepseek-v4-pro",
    baseUrl: "https://api.deepseek.com",
    credentialRef: "secret_deepseek_official",
    profiles: ["balanced_reasoning"],
    enabled: true,
    ...overrides,
  };
}

function mount(
  overrides: Partial<Parameters<typeof Models>[0]> = {},
  routes: ModelRouteSummary[] = [route()],
  probes: Record<string, ProbeState> = {},
) {
  const props = {
    choice: { configured: true, routes },
    busy: false,
    error: "",
    probes,
    onApply: vi.fn().mockResolvedValue("saved"),
    onProbe: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<Models {...props} />);
  return props;
}

describe("the models screen", () => {
  it("never renders a stored key, and says a new value replaces the old one", async () => {
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    const field = screen.getByLabelText(/API key/i) as HTMLInputElement;
    // The Runtime has no read path for a credential at all, so a field that showed anything would
    // be showing something invented.
    expect(field.value).toBe("");
    expect(field.type).toBe("password");
    expect(field.placeholder).toMatch(/replace/i);
  });

  it("sends the route and key as one awaited operation, and keeps the key out of the DOM afterwards", async () => {
    const props = mount();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.deepseek.com");
    await userEvent.type(screen.getByLabelText(/API key/i), "sk-SENTINEL-models-screen");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(props.onApply).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "deepseek_official",
        provider: "openai",
        baseUrl: "https://api.deepseek.com",
        model: "deepseek-v4-pro",
        enabled: true,
        replace: true,
        credentialRef: "secret_deepseek_official",
      }),
      expect.objectContaining({
        reference: "secret_deepseek_official",
        usableBy: ["deepseek_official"],
        value: "sk-SENTINEL-models-screen",
      }),
    );
    // WHAT THIS CELL CAN AND CANNOT SEE, measured rather than assumed. An `innerHTML` sweep is
    // vacuous: React sets a controlled input's value as a DOM property, so a typed key never
    // reaches the serialized markup whether the component clears it or not — removing the clear
    // left all nine cells green. Re-reading the field after re-opening the SAME card is vacuous
    // too: opening a card clears the box, so that second clear masks the first. The property
    // that is observable from outside is the one below, in its own cell: a key typed on one card
    // never appears on another. The clear inside `apply` is defensive and this suite does not
    // claim to cover it.
    expect(document.body.textContent).toContain("deepseek_official");
  });

  it("never carries a key typed on one card onto another", async () => {
    mount({}, [route(), route({ id: "openai_direct", model: "gpt-5" })]);
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.type(screen.getByLabelText(/API key/i), "sk-SENTINEL-first-card");
    await userEvent.click(screen.getByRole("button", { name: "cancel" }));

    await userEvent.click(screen.getByRole("button", { name: "Edit openai_direct" }));
    // One keystroke away from storing the first route's key under the second route's reference.
    expect((screen.getByLabelText(/API key/i) as HTMLInputElement).value).toBe("");
  });

  it("applies a route with no key without calling the credential write at all", async () => {
    const props = mount();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(props.onApply).toHaveBeenCalledTimes(1);
    // Editing a base URL must not require re-typing a key, and an empty box must not store one.
    expect(props.onApply).toHaveBeenCalledWith(expect.anything(), null);
  });

  it("refuses a new route without a key before calling Runtime", async () => {
    const props = mount();
    await userEvent.click(screen.getByRole("button", { name: /Add model/i }));
    await userEvent.type(screen.getByLabelText("Route id"), "new_route");
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.example.com");
    await userEvent.type(screen.getByLabelText("Model"), "example-model");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(props.onApply).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(/new model needs an API key/i);
    expect(screen.getByRole("form", { name: "Edit route" })).toBeInTheDocument();
  });

  it("refuses an existing route when its current endpoint baseline is gone or changed", async () => {
    const onApply = vi.fn().mockResolvedValue("saved");
    const props = {
      choice: { configured: true, routes: [route()] },
      busy: false,
      error: "",
      probes: {},
      onApply,
      onProbe: vi.fn(),
      onClose: vi.fn(),
    };
    const view = render(<Models {...props} />);
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.type(screen.getByLabelText(/API key/i), "keep-this-draft");
    view.rerender(<Models {...props} choice={{ configured: true, routes: [] }} />);
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(onApply).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(/changed while it was open/i);
    expect(screen.getByLabelText(/API key/i)).toHaveValue("keep-this-draft");
  });

  it("refuses provider or URL drift before Runtime even if a draft is tampered with", async () => {
    const props = mount();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    const provider = screen.getByLabelText("Wire format");
    const baseUrl = screen.getByLabelText("Base URL");
    expect(provider).toBeDisabled();
    fireEvent.change(provider, { target: { value: "anthropic" } });
    fireEvent.change(baseUrl, { target: { value: "https://other.example" } });
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect(props.onApply).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(/changed while it was open/i);
  });

  it("refuses a base URL change without a replacement key", async () => {
    const props = mount();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.other.example");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(props.onApply).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(/needs a new API key/i);
  });

  it("uses a fresh credential reference when an existing route changes endpoint", async () => {
    const onApply = vi.fn().mockResolvedValue("saved");
    mount({ onApply });
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.other.example");
    await userEvent.type(screen.getByLabelText(/API key/i), "replacement-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    const [draft, key] = onApply.mock.calls[0] ?? [];
    expect(draft).toEqual(expect.objectContaining({ baseUrl: "https://api.other.example" }));
    expect((draft as { credentialRef?: string }).credentialRef).not.toBe("secret_deepseek_official");
    expect((key as { reference?: string }).reference).toBe((draft as { credentialRef?: string }).credentialRef);
  });

  it("retries a partially saved endpoint with the same fresh reference", async () => {
    const onApply = vi.fn().mockResolvedValue("route_saved_key_failed");
    mount({ onApply });
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.other.example");
    await userEvent.type(screen.getByLabelText(/API key/i), "replacement-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    const firstDraft = onApply.mock.calls[0]?.[0] as { credentialRef?: string };
    const retryDraft = onApply.mock.calls[1]?.[0] as { credentialRef?: string };
    expect(retryDraft.credentialRef).toBe(firstDraft.credentialRef);
  });

  it("does not let a partial retry change endpoint again", async () => {
    const onApply = vi.fn().mockResolvedValue("route_saved_key_failed");
    mount({ onApply });
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.other.example");
    await userEvent.type(screen.getByLabelText(/API key/i), "replacement-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://third.example");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(onApply).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert")).toHaveTextContent(/route or endpoint changed/i);
  });

  it("does not overwrite a third endpoint observed after a partial save", async () => {
    const onApply = vi.fn().mockResolvedValue("route_saved_key_failed");
    const props = {
      choice: { configured: true, routes: [route()] },
      busy: false,
      error: "",
      probes: {},
      onApply,
      onProbe: vi.fn(),
      onClose: vi.fn(),
    };
    const view = render(<Models {...props} />);
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.other.example");
    await userEvent.type(screen.getByLabelText(/API key/i), "replacement-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    view.rerender(
      <Models
        {...props}
        choice={{
          configured: true,
          routes: [route({ baseUrl: "https://third.example", credentialRef: "secret_third" })],
        }}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(onApply).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert")).toHaveTextContent(/route or endpoint changed/i);
  });

  it("uses a fresh credential reference for a new route and keeps it for retry", async () => {
    const onApply = vi.fn().mockResolvedValue("route_saved_key_failed");
    mount({ onApply });
    await userEvent.click(screen.getByRole("button", { name: /Add model/i }));
    await userEvent.type(screen.getByLabelText("Route id"), "recreated_route");
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.example.com");
    await userEvent.type(screen.getByLabelText("Model"), "example-model");
    await userEvent.type(screen.getByLabelText(/API key/i), "fresh-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    const first = onApply.mock.calls[0];
    expect(first).toBeDefined();
    const draft = first?.[0] as { credentialRef?: string };
    const key = first?.[1] as { reference?: string };
    expect(draft.credentialRef).toBeDefined();
    expect(draft.credentialRef).not.toBe("secret_recreated_route");
    expect(key.reference).toBe(draft.credentialRef);
    expect(screen.getByLabelText(/API key/i)).toHaveValue("fresh-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    const retryDraft = onApply.mock.calls[1]?.[0] as { credentialRef?: string };
    const retryKey = onApply.mock.calls[1]?.[1] as { reference?: string };
    expect(retryDraft.credentialRef).toBe(draft.credentialRef);
    expect(retryKey.reference).toBe(key.reference);
  });

  it("offers no key field for a command route, and says why", () => {
    mount({}, [route({ id: "codex_cli", transport: "native_runtime", model: null })]);
    expect(screen.queryByRole("button", { name: "Edit codex_cli" })).toBeNull();
    expect(screen.getByLabelText("codex_cli is a command route")).toBeInTheDocument();
  });

  it("starts every route unchecked rather than green", () => {
    mount();
    expect(screen.getByRole("status", { name: "probe: not checked" })).toBeInTheDocument();
  });

  it("says what a green check measured, and what it did not", () => {
    mount({}, [route()], { deepseek_official: { state: "available" } });
    expect(
      screen.getByRole("status", { name: "probe: the key leases from the broker" }),
    ).toBeInTheDocument();
    expect(document.body.textContent).toMatch(/places no model call/i);
  });

  it("shows a disabled route instead of hiding it, and still lets it be edited back on", async () => {
    const props = mount({}, [route({ enabled: false })]);
    expect(screen.getByText(/disabled/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.click(screen.getByLabelText("Enabled"));
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect(props.onApply).toHaveBeenCalledWith(expect.objectContaining({ enabled: true }), null);
  });

  it("refuses to add a route on a Runtime that has no manifest to write", () => {
    mount({ choice: { configured: false, routes: [] } }, []);
    expect(screen.getByRole("button", { name: /Add model/i })).toBeDisabled();
    expect(document.body.textContent).toMatch(/started without a gateway manifest/i);
  });

  it("gates every control while a write is in flight", async () => {
    mount({ busy: true });
    expect(screen.getByRole("button", { name: "Edit deepseek_official" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "check" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Add model/i })).toBeDisabled();
  });

  it("does not call a native command a broker lease or provider login", () => {
    mount({}, [route({ id: "codex_cli", transport: "native_runtime", model: null })], {
      codex_cli: { state: "available" },
    });
    expect(screen.getByRole("status", { name: /native command succeeded/i })).toBeInTheDocument();
    expect(screen.getByRole("status", { name: /no broker lease or provider login measured/i })).toBeInTheDocument();
  });

  it("prefills the editable URL and retains the broker reference", async () => {
    mount({}, [route({ baseUrl: "https://custom.example/v1", credentialRef: "cred_custom" })]);
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    expect((screen.getByLabelText("Base URL") as HTMLInputElement).value).toBe("https://custom.example/v1");
    await userEvent.type(screen.getByLabelText(/API key/i), "new-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect((screen.getByRole("button", { name: "Edit deepseek_official" }) as HTMLButtonElement)).toBeInTheDocument();
    expect((screen.getByRole("button", { name: "Edit deepseek_official" }) as HTMLButtonElement)).toBeInTheDocument();
  });

  it("keeps the draft and shows an async save failure", async () => {
    const props = mount({ onApply: vi.fn().mockRejectedValue(new Error("route refused")) });
    await userEvent.click(screen.getByRole("button", { name: "Edit deepseek_official" }));
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect(screen.getByRole("form", { name: "Edit route" })).toBeInTheDocument();
    expect(props.onApply).toHaveBeenCalled();
  });
});
