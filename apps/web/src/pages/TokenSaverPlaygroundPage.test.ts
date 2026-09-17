import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import TokenSaverPlaygroundPage from "./TokenSaverPlaygroundPage.vue";
import { api } from "@/lib/api";
import type { PlaygroundResult, SettingsResponse } from "@/types/api";

vi.mock("vue-router", () => ({
  RouterLink: { template: "<a><slot /></a>" },
}));

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 400;
    readonly type = "invalid_request_error";
  },
  api: {
    settings: vi.fn(),
    runPlayground: vi.fn(),
  },
}));

const tokenSaver = {
  slimmer_enabled: true,
  slimmer_level: "minimal",
  headroom_enabled: false,
  headroom_url: "http://localhost:8787",
  headroom_timeout_ms: 2000,
  terse_enabled: false,
  caveman_enabled: false,
  caveman_level: "full",
  ponytail_enabled: false,
  ponytail_level: "full",
};

const settings = {
  token_saver: tokenSaver,
} as unknown as SettingsResponse;

/** A run where the slimmer shrank a bulky tool result. */
const shrunk: PlaygroundResult = {
  model: "gpt-4o",
  active: true,
  tokens_before: 800,
  tokens_after: 120,
  prompt_tokens_saved: 680,
  steps: [
    {
      saver: "rtk",
      label: "RTK / Slimmer",
      side: "input",
      applied: true,
      tokens_before: 800,
      tokens_after: 120,
      delta: -680,
      detail: "1 tool result(s) · diff",
    },
  ],
  before: [{ role: "user", content: "review this" }],
  after: [{ role: "user", content: "review this" }],
  notes: ["rtk: 680 tokens from 1 tool result(s) (diff)"],
  totals: {
    saved_rtk_tokens: 680,
    saved_headroom_tokens: 0,
    saved_terse_tokens: 0,
    saved_caveman_tokens: 0,
    saved_ponytail_tokens: 0,
    saved_cost_usd: 0.0021,
  },
};

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async () => {
  const app = createApp(TokenSaverPlaygroundPage as never);
  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);
  await settle();

  return {
    container,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

const button = (container: HTMLElement, label: string) =>
  [...container.querySelectorAll("button")].find((candidate) =>
    candidate.textContent?.includes(label),
  );

describe("TokenSaverPlaygroundPage", () => {
  beforeEach(() => {
    vi.mocked(api.settings).mockResolvedValue(settings);
    vi.mocked(api.runPlayground).mockResolvedValue(shrunk);
  });

  it("runs the pipeline and shows the measured prompt reduction", async () => {
    const { container, unmount } = await mount();

    button(container, "Run pipeline")!.click();
    await settle();

    expect(api.runPlayground).toHaveBeenCalledTimes(1);
    // The editor is seeded with the first sample, so a run needs no typing.
    const request = vi.mocked(api.runPlayground).mock.calls[0]![0];
    expect(request.messages.length).toBeGreaterThan(0);

    expect(container.textContent).toContain("RTK / Slimmer");
    expect(container.textContent).toContain("800");
    expect(container.textContent).toContain("120");
    // 680 of 800 removed is 85%.
    expect(container.textContent).toContain("85.0%");
    expect(container.textContent).toContain("680");

    unmount();
  });

  it("reports a rejected run instead of a result", async () => {
    vi.mocked(api.runPlayground).mockRejectedValue(
      new Error("terse and caveman are mutually exclusive"),
    );

    const { container, unmount } = await mount();
    button(container, "Run pipeline")!.click();
    await settle();

    expect(container.textContent).toContain("The run was rejected");
    expect(container.textContent).not.toContain("Prompt reduction");

    unmount();
  });

  it("refuses to send malformed message JSON", async () => {
    const { container, unmount } = await mount();

    const editor = container.querySelector<HTMLTextAreaElement>(
      "#playground-messages",
    );
    editor!.value = "{ not json";
    editor!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    button(container, "Run pipeline")!.click();
    await settle();

    // A local problem must not become a round trip.
    expect(api.runPlayground).not.toHaveBeenCalled();
    expect(container.textContent).toContain("Not valid JSON");

    unmount();
  });

  it("toggles a saver for one run without saving it", async () => {
    const { container, unmount } = await mount();

    // Caveman is off in the saved settings; turning it on here must be an
    // override, never a settings write.
    button(container, "Caveman")!.click();
    await settle();

    button(container, "Run pipeline")!.click();
    await settle();

    const request = vi.mocked(api.runPlayground).mock.calls[0]![0];
    expect(request.overrides).toMatchObject({ caveman_enabled: true });

    unmount();
  });

  it("clears the other directive when the mutual pair is toggled", async () => {
    vi.mocked(api.settings).mockResolvedValue({
      token_saver: { ...tokenSaver, terse_enabled: true },
    } as unknown as SettingsResponse);

    const { container, unmount } = await mount();

    button(container, "Caveman")!.click();
    await settle();

    button(container, "Run pipeline")!.click();
    await settle();

    // Terse and caveman cannot both be on, so enabling one must clear the other
    // rather than let the server reject the run.
    const request = vi.mocked(api.runPlayground).mock.calls[0]![0];
    expect(request.overrides).toMatchObject({
      caveman_enabled: true,
      terse_enabled: false,
    });

    unmount();
  });
});
