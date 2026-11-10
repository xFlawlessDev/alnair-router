import { createApp, h, nextTick } from "vue";
import { afterEach, expect, it, vi } from "vitest";
import { api } from "@/lib/api";
import ProviderQuota from "./ProviderQuota.vue";
import type { Connection } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: { connectionQuota: vi.fn() },
}));
afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

it("shows Tavily API-key and plan credits, refreshes and keeps stale readings", async () => {
  vi.useFakeTimers();
  vi.mocked(api.connectionQuota)
    .mockResolvedValueOnce({
      key: { usage: 12, limit: null },
      account: {
        current_plan: "Free",
        plan_usage: 80,
        plan_limit: 1000,
        paygo_usage: 0,
        paygo_limit: 0,
      },
    })
    .mockRejectedValueOnce(new Error("offline"));
  const container = document.createElement("div");
  document.body.appendChild(container);
  const app = createApp({
    render: () =>
      h(ProviderQuota, {
        connection: {
          id: "tavily-id",
          provider_id: "tavily",
          provider_type: "openai-compatible",
        } as Connection,
      }),
  });
  app.mount(container);
  await vi.waitFor(() => expect(container.textContent).toContain("Unlimited"));
  expect(api.connectionQuota).toHaveBeenCalledWith("tavily-id");
  expect(container.querySelectorAll("progress")).toHaveLength(1);
  expect(container.querySelector("progress")?.max).toBe(1000);
  expect(container.querySelector("progress")?.value).toBe(80);
  await vi.advanceTimersByTimeAsync(60_000);
  await nextTick();
  expect(container.textContent).toContain("Showing last successful reading");
  expect(container.textContent).toContain("Unlimited");
  vi.mocked(api.connectionQuota).mockResolvedValueOnce({
    key: { usage: 0, limit: 0 },
    account: {
      current_plan: "Free",
      plan_usage: 1000,
      plan_limit: 1000,
      paygo_usage: 1,
      paygo_limit: 10,
    },
  });
  container.querySelector("button")?.click();
  await vi.waitFor(() =>
    expect(container.textContent).toContain("Pay as you go"),
  );
  expect(container.textContent).not.toContain("Could not load");
  app.unmount();
  container.remove();
  expect(vi.getTimerCount()).toBe(0);
});
