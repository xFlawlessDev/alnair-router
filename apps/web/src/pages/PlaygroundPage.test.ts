import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PlaygroundPage from "./PlaygroundPage.vue";
import { api } from "@/lib/api";

vi.mock("vue-router", () => ({
  RouterLink: { template: "<a><slot /></a>" },
}));

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 0;
    readonly type = undefined;
  },
  api: {
    settings: vi.fn(),
    modelCatalog: vi.fn(),
  },
  streamPlaygroundChat: vi.fn(),
}));

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async () => {
  const app = createApp(PlaygroundPage as never);
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

const tab = (container: HTMLElement, label: string) =>
  [...container.querySelectorAll("button[role=tab]")].find((candidate) =>
    candidate.textContent?.includes(label),
  );

/** Reka UI tabs activate on mousedown, not click. */
const switchTab = async (container: HTMLElement, label: string) => {
  tab(container, label)!.dispatchEvent(
    new MouseEvent("mousedown", { bubbles: true }),
  );
  await settle();
};

describe("PlaygroundPage", () => {
  beforeEach(() => {
    vi.mocked(api.settings).mockResolvedValue({
      token_saver: { slimmer_enabled: true, caveman_enabled: false },
    } as never);
    vi.mocked(api.modelCatalog).mockResolvedValue({
      object: "list",
      data: [{ id: "chatty" }],
    } as never);
  });

  it("offers both playground tabs and opens on chat", async () => {
    const { container, unmount } = await mount();

    expect(tab(container, "Chat")).toBeTruthy();
    expect(tab(container, "Token Saver")).toBeTruthy();
    // Chat is the default, so the hub reads as a live tool rather than a
    // token-saver sub-page.
    expect(container.textContent).toContain("Transcript");
    expect(container.textContent).not.toContain("Run pipeline");

    unmount();
  });

  it("swaps to the pipeline view when the token saver tab is picked", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Token Saver");

    expect(container.textContent).toContain("Run pipeline");
    expect(container.textContent).toContain("Sample request");

    unmount();
  });

  it("keeps a per-run saver override across both tabs", async () => {
    const { container, unmount } = await mount();

    // Caveman is off in the saved settings; the override set here must follow
    // the user to the token saver tab rather than resetting on the switch.
    const caveman = () =>
      [...container.querySelectorAll<HTMLElement>("button[aria-pressed]")].find(
        (candidate) => candidate.textContent?.includes("Caveman"),
      );
    caveman()!.click();
    await settle();

    await switchTab(container, "Token Saver");

    expect(caveman()?.getAttribute("aria-pressed")).toBe("true");

    unmount();
  });
});
