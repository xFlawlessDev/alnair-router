import { createApp, defineComponent, h, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import OverviewPage from "./OverviewPage.vue";
import { ApiError, api } from "@/lib/api";
import type { UpdateStatus } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 0;
    readonly type = undefined;
  },
  api: {
    health: vi.fn(),
    version: vi.fn(),
    initState: vi.fn(),
    modelCatalog: vi.fn(),
    usageSummary: vi.fn(),
    usageModels: vi.fn(),
    activity: vi.fn(),
    update: vi.fn(),
  },
}));

vi.mock("vue-sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

// The page links out with RouterLink; a stub keeps the suite free of a real
// router instance while preserving the anchor in the DOM.
vi.mock("vue-router", () => ({
  RouterLink: defineComponent({
    name: "RouterLink",
    props: { to: { type: [String, Object], required: true } },
    setup(props, { slots }) {
      return () =>
        h(
          "a",
          { href: typeof props.to === "string" ? props.to : String(props.to) },
          slots.default?.(),
        );
    },
  }),
}));

const status = (overrides: Partial<UpdateStatus> = {}): UpdateStatus => ({
  name: "alnair-router",
  version: "0.1.0",
  latest_version: "0.1.0",
  update_available: false,
  release_url: null,
  release_notes: null,
  published_at: null,
  checked_at: "2026-02-01T00:00:00.000Z",
  check_enabled: true,
  error: null,
  ...overrides,
});

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async () => {
  const app = createApp(OverviewPage as never);
  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);
  await settle();

  const banner = () => container.querySelector('[data-testid="update-banner"]');

  return {
    container,
    banner,
    meCopy: () =>
      container.querySelector<HTMLButtonElement>(
        '[data-testid="my-usage-copy"]',
      ),
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("OverviewPage version check", () => {
  beforeEach(() => {
    vi.mocked(api.health).mockResolvedValue({
      status: "ok",
      service: "alnair-router",
      version: "0.1.0",
    });
    vi.mocked(api.version).mockResolvedValue({
      name: "alnair-router",
      version: "0.1.0",
    });
    vi.mocked(api.initState).mockResolvedValue({
      initialized: true,
      connections: 1,
      enabled_connections: 1,
      require_api_key: true,
    });
    vi.mocked(api.modelCatalog).mockResolvedValue({ object: "list", data: [] });
    vi.mocked(api.usageSummary).mockResolvedValue({
      requests: 0,
      ok_requests: 0,
      error_requests: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      cached_tokens: 0,
      reasoning_tokens: 0,
      cost_usd: 0,
      cost_input_usd: 0,
      cost_output_usd: 0,
      cost_reasoning_usd: 0,
      avg_latency_ms: 0,
      savings: {
        requests: 0,
        saved_rtk_tokens: 0,
        saved_headroom_tokens: 0,
        saved_terse_tokens: 0,
        saved_caveman_tokens: 0,
        saved_ponytail_tokens: 0,
        saved_cost_usd: 0,
      },
    });
    vi.mocked(api.usageModels).mockResolvedValue([]);
    vi.mocked(api.activity).mockResolvedValue({
      uptime_ms: 1000,
      active: [],
      connections: [],
      events: [],
    });
    vi.mocked(api.update).mockResolvedValue(status());
  });

  it("shows no banner when the router is up to date", async () => {
    const { banner, container, unmount } = await mount();

    expect(banner()).toBeNull();
    expect(container.textContent).toContain("alnair-router v0.1.0");

    unmount();
  });

  it("shows the version and a link when a newer release exists", async () => {
    vi.mocked(api.update).mockResolvedValue(
      status({
        latest_version: "0.2.0",
        update_available: true,
        release_url: "https://example.test/release",
      }),
    );

    const { banner, container, unmount } = await mount();

    expect(banner()).toBeTruthy();
    expect(banner()!.textContent).toContain("v0.2.0 is available");
    expect(banner()!.textContent).toContain("runs v0.1.0");
    expect(
      banner()!.querySelector('a[href="https://example.test/release"]'),
    ).toBeTruthy();

    unmount();
  });

  it("stays quiet when the update check itself fails", async () => {
    // A failed lookup is decoration: the page must still render its status
    // cards rather than showing a warning.
    vi.mocked(api.update).mockResolvedValue(
      status({ latest_version: null, error: "cannot reach GitHub" }),
    );

    const { banner, container, unmount } = await mount();

    expect(banner()).toBeNull();
    expect(container.textContent).not.toContain("Could not load");

    unmount();
  });

  it("does not let a rejected update call fail the whole page", async () => {
    vi.mocked(api.update).mockRejectedValue(new ApiError("nope", 500));

    const { banner, container, unmount } = await mount();

    expect(banner()).toBeNull();
    // The core status card still rendered from the other settled calls.
    expect(container.textContent).toContain("Healthy");

    unmount();
  });

  it("copies the self-service usage URL", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    const { meCopy, unmount } = await mount();

    const button = meCopy();
    expect(button).toBeTruthy();
    expect(button!.textContent).toContain("/me");

    button!.click();
    await settle();

    expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/me`);

    unmount();
  });
});
