import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api";
import ProviderHealthPage from "./ProviderHealthPage.vue";
import type { Connection } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: {
    listConnections: vi.fn(),
    activity: vi.fn(),
    testConnection: vi.fn(),
    providerHealth: vi.fn(),
    listCombos: vi.fn(),
  },
}));

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

describe("ProviderHealthPage", () => {
  beforeEach(() => {
    vi.mocked(api.listConnections).mockResolvedValue([
      {
        id: "one",
        name: "Primary",
        enabled: 1,
        provider_type: "openai-compatible",
        updated_at: "2026-01-01",
      } as Connection,
      {
        id: "two",
        name: "Offline",
        enabled: 0,
        provider_type: "openai-compatible",
        updated_at: "2026-01-01",
      } as Connection,
    ]);
    vi.mocked(api.activity).mockResolvedValue({
      uptime_ms: 0,
      active: [],
      connections: [],
      events: [],
    });
    vi.mocked(api.providerHealth).mockResolvedValue({
      since: "2026-01-01T00:00:00Z",
      providers: [],
      models: [],
      references: [],
      overall: [],
    });
    vi.mocked(api.listCombos).mockResolvedValue([]);
    vi.mocked(api.testConnection).mockResolvedValue({
      ok: true,
      message: "Connected",
      models_count: 1,
      enumerable: true,
      latency_ms: 23,
      notice: null,
    });
  });

  it("shows unchecked and disabled connections and displays a requested probe", async () => {
    const app = createApp(ProviderHealthPage);
    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await settle();
    expect(container.textContent).toContain("unknown");
    expect(container.textContent).toContain("disabled");
    expect(api.testConnection).not.toHaveBeenCalled();
    const check = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Check Primary"]',
    );
    check!.click();
    await settle();
    expect(api.testConnection).toHaveBeenCalledWith("one");
    expect(container.textContent).toContain("healthy");
    [...container.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "Probes")!
      .click();
    await settle();
    expect(container.textContent).toContain("Connected");
    expect(container.textContent).toContain("23 ms");
    app.unmount();
    container.remove();
  });
});
