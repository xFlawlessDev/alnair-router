import { createApp, h, nextTick } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import OAuthQuota from "./OAuthQuota.vue";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: { listOAuthAccounts: vi.fn(), oauthAccountQuota: vi.fn() },
}));

afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });

describe("OAuthQuota", () => {
  it("shows each usage window, refreshes and marks stale data on failure", async () => {
    vi.useFakeTimers();
    vi.mocked(api.listOAuthAccounts).mockResolvedValue([{ id: "a", label: "work", provider_key: "openai-codex", enabled: 1 } as never]);
    vi.mocked(api.oauthAccountQuota).mockResolvedValueOnce({
      plan_type: "plus", rate_limit: { allowed: true, limit_reached: false,
        primary_window: { used_percent: 42, limit_window_seconds: 18000, reset_at: 1800000000 }, secondary_window: null },
      additional_rate_limits: [],
    }).mockRejectedValueOnce(new Error("offline"));
    const container = document.createElement("div");
    document.body.appendChild(container);
    const app = createApp({ render: () => h(OAuthQuota, { connectionId: "c" }) });
    app.mount(container);
    await vi.waitFor(() => expect(container.textContent).toContain("42% used"));
    expect(container.textContent).toContain("5-hour window");
    expect(api.oauthAccountQuota).toHaveBeenCalledWith("c", "a");
    await vi.advanceTimersByTimeAsync(60_000);
    await nextTick();
    expect(container.textContent).toContain("Showing last successful reading");
    expect(container.textContent).toContain("42% used");
    app.unmount();
    container.remove();
    expect(vi.getTimerCount()).toBe(0);
  });
});
