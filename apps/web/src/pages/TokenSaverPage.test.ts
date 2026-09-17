import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import TokenSaverPage from "./TokenSaverPage.vue";
import { api } from "@/lib/api";
import type { SettingsResponse, UsageSummary } from "@/types/api";

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
    updateSettings: vi.fn(),
    resetSettings: vi.fn(),
    usageSummary: vi.fn(),
    testHeadroom: vi.fn(),
  },
}));

/**
 * A whole response: `hydrateForm` reads every field, so a partial fixture would
 * throw before the page ever renders.
 */
const settingsResponse = (): SettingsResponse =>
  ({
    server: {
      require_api_key: false,
      readiness_upstream_checks: false,
      public_usage: true,
      store_key_secrets: true,
      lan_access: false,
      cors_origins: [],
      admin_token_set: false,
    },
    router: {
      default_connection: null,
      max_attempts: 5,
      max_retries_per_tier: 2,
      max_retry_delay_ms: 30000,
      catalog_ttl_ms: 1000,
      connect_timeout_ms: 10000,
      idle_timeout_ms: 60000,
    },
    limits: {
      max_concurrent: 0,
      max_concurrent_per_connection: 0,
      acquire_timeout_ms: 30000,
    },
    rate_limit: { requests_per_minute: 0, burst: 0 },
    pricing: { sync_enabled: false, sync_interval_secs: 86400, source_url: "" },
    token_saver: {
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
    },
    overrides: [],
    deployment: {
      host: "127.0.0.1",
      port: 7878,
      binds_loopback: true,
      serve_dashboard: true,
      tray: true,
      allow_unauthenticated_admin: false,
      database_url: "sqlite::memory:",
      secrets_key_set: true,
    },
  }) as unknown as SettingsResponse;

const usage = {
  prompt_tokens: 0,
  savings: {
    requests: 0,
    saved_rtk_tokens: 0,
    saved_headroom_tokens: 0,
    saved_terse_tokens: 0,
    saved_caveman_tokens: 0,
    saved_ponytail_tokens: 0,
    saved_cost_usd: 0,
  },
} as unknown as UsageSummary;

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

/** The save bar animates out, so its removal needs a real frame to land. */
const settleTransition = async () => {
  await settle();
  await new Promise((resolve) => setTimeout(resolve, 50));
  await nextTick();
};

const mount = async () => {
  const app = createApp(TokenSaverPage as never);
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
    candidate.textContent?.trim().includes(label),
  );

/** reka-ui switches a tab on mousedown, so a plain click never activates it. */
const switchTab = async (container: HTMLElement, label: string) => {
  button(container, label)!.dispatchEvent(
    new MouseEvent("mousedown", { bubbles: true }),
  );
  await settle();
};

const toggle = async (container: HTMLElement, id: string) => {
  container.querySelector<HTMLElement>(`#${id}`)!.click();
  await settle();
};

describe("TokenSaverPage", () => {
  beforeEach(() => {
    vi.mocked(api.settings).mockResolvedValue(settingsResponse());
    vi.mocked(api.usageSummary).mockResolvedValue(usage);
    vi.mocked(api.updateSettings).mockResolvedValue(settingsResponse());
  });

  it("loads savings and the pipeline configuration together", async () => {
    const { container, unmount } = await mount();

    expect(api.usageSummary).toHaveBeenCalledTimes(1);
    expect(api.settings).toHaveBeenCalledTimes(1);

    // The Configuration tab owns the savers that used to live in Settings.
    expect(button(container, "Configuration")).toBeTruthy();
    unmount();
  });

  it("shows the saver controls on the configuration tab", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");

    expect(container.querySelector("#setting-slimmer")).toBeTruthy();
    expect(container.querySelector("#setting-ponytail")).toBeTruthy();

    unmount();
  });

  it("offers no save bar until a saver setting changes", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");
    expect(container.textContent).not.toContain("Unsaved changes");

    await toggle(container, "setting-slimmer");

    expect(container.textContent).toContain("Unsaved changes");
    unmount();
  });

  it("saves only the token saver keys", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");
    await toggle(container, "setting-ponytail");

    button(container, "Save changes")!.click();
    await settle();

    expect(api.updateSettings).toHaveBeenCalledTimes(1);
    // The page must not smuggle unrelated settings into the patch.
    expect(vi.mocked(api.updateSettings).mock.calls[0]![0]).toEqual({
      ponytail_enabled: true,
    });

    unmount();
  });

  it("discards unsaved edits rather than writing them", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");
    await toggle(container, "setting-slimmer");
    expect(container.textContent).toContain("Unsaved changes");

    button(container, "Discard")!.click();
    await settleTransition();

    expect(api.updateSettings).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain("Unsaved changes");
    // The switch must visibly revert, not just drop the pending diff.
    expect(
      container.querySelector("#setting-slimmer")?.getAttribute("aria-checked"),
    ).toBe("true");

    unmount();
  });

  it("ships a Headroom run guide on the configuration tab", async () => {
    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");

    expect(container.textContent).toContain("Running Headroom");
    expect(container.textContent).toContain('pipx install "headroom-ai[all]"');
    expect(container.textContent).toContain("headroom proxy --port 8787");
    // The loopback-only rule is the failure operators hit first.
    expect(container.textContent).toContain("HEADROOM_COMPRESS_ALLOW_REMOTE=1");

    unmount();
  });

  it("reports a failed configuration load without hiding the tab", async () => {
    vi.mocked(api.settings).mockRejectedValue(new Error("router unreachable"));

    const { container, unmount } = await mount();

    await switchTab(container, "Configuration");

    expect(container.textContent).toContain("Cannot load configuration");
    // A failed load must not offer a save bar for a form that never hydrated.
    expect(container.textContent).not.toContain("Unsaved changes");

    unmount();
  });
});
