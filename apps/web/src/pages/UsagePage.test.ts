import { createApp, nextTick } from "vue";
import { createMemoryHistory, createRouter } from "vue-router";
import { beforeEach, describe, expect, it, vi } from "vitest";

import UsagePage from "./UsagePage.vue";
import { ApiError, api } from "@/lib/api";
import type { UsageBucket, UsageRecord, UsageSummary } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: {
    listKeys: vi.fn(),
    usageFacets: vi.fn(),
    activity: vi.fn(),
    listUsage: vi.fn(),
    usageSummary: vi.fn(),
    usageTimeseries: vi.fn(),
  },
}));

// The filter bar's combobox needs a real popover to interact with, so it is
// replaced by a stub that emits the same events the page listens for.
vi.mock("@/components/usage/UsageFilterBar.vue", () => ({
  default: {
    name: "UsageFilterBar",
    props: ["apiKeyId", "model", "range"],
    emits: ["update:model", "clear"],
    template: `
      <div>
        <button
          type="button"
          aria-label="Set model filter"
          @click="$emit('update:model', 'nonexistent')"
        >model</button>
        <button type="button" aria-label="Clear filter stub" @click="$emit('clear')">
          clear
        </button>
      </div>
    `,
  },
}));

const record = (index: number): UsageRecord => ({
  id: `u${index}`,
  created_at: "2026-01-01T00:00:00Z",
  api_key_id: null,
  requested_model: "oa/gpt-4o",
  resolved_provider: "openai-compatible",
  resolved_model: "gpt-4o",
  connection_name: "openai-main",
  attempt: 1,
  status: "ok",
  prompt_tokens: 10,
  completion_tokens: 20,
  cached_tokens: 0,
  reasoning_tokens: 0,
  cost_usd: 0.01,
  cost_input_usd: 0.004,
  cost_output_usd: 0.006,
  cost_reasoning_usd: 0,
  latency_ms: 100,
});

const summary = (overrides: Partial<UsageSummary> = {}): UsageSummary => ({
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
  ...overrides,
});

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

/** Polling is paused on mount so a test drives every request explicitly. */
const mount = async () => {
  const app = createApp(UsagePage as never);
  // The page links to /guide and /logs, so it needs a router to mount.
  app.use(
    createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: "/", component: { template: "<div />" } },
        { path: "/guide", component: { template: "<div />" } },
        { path: "/logs", component: { template: "<div />" } },
      ],
    }),
  );
  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);
  await settle();

  const control = (label: string) =>
    [...container.querySelectorAll("button")].find(
      (candidate) =>
        candidate.getAttribute("aria-label") === label ||
        candidate.textContent?.trim() === label,
    );

  // Stop the 2s/5s loops: timers left running make the suite flaky.
  // Clicking Pause sets live=false, which calls stopPolling().
  control("Pause")?.click();
  await settle();

  return {
    container,
    control,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("UsagePage", () => {
  beforeEach(() => {
    vi.mocked(api.listKeys).mockResolvedValue([]);
    vi.mocked(api.usageFacets).mockResolvedValue({
      models: [],
      providers: [],
      connections: [],
    });
    vi.mocked(api.activity).mockResolvedValue({
      uptime_ms: 0,
      active: [],
      connections: [],
      events: [],
    });
    vi.mocked(api.usageTimeseries).mockResolvedValue([] as UsageBucket[]);
    vi.mocked(api.listUsage).mockResolvedValue([]);
    vi.mocked(api.usageSummary).mockResolvedValue(summary());
  });

  it("reports the total row count and pages through it", async () => {
    vi.mocked(api.listUsage).mockResolvedValue(
      Array.from({ length: 100 }, (_, index) => record(index)),
    );
    vi.mocked(api.usageSummary).mockResolvedValue(summary({ requests: 200 }));

    const { container, control, unmount } = await mount();

    expect(container.textContent).toContain("Showing rows 1–100 of 200");

    control("Next")!.click();
    await settle();

    // The last page is exactly full, so Next must not offer an empty one.
    expect(container.textContent).toContain("Showing rows 101–200 of 200");
    expect(control("Next")!.hasAttribute("disabled")).toBe(true);

    control("Previous")!.click();
    await settle();
    expect(container.textContent).toContain("Showing rows 1–100 of 200");

    unmount();
  });

  it("keeps Next disabled when there is nothing more to page to", async () => {
    vi.mocked(api.listUsage).mockResolvedValue(
      Array.from({ length: 50 }, (_, index) => record(index)),
    );
    vi.mocked(api.usageSummary).mockResolvedValue(summary({ requests: 50 }));

    const { container, control, unmount } = await mount();

    expect(container.textContent).toContain("Showing rows 1–50 of 50");
    expect(control("Next")!.hasAttribute("disabled")).toBe(true);

    unmount();
  });

  it("distinguishes an empty result from an empty history", async () => {
    const { container, control, unmount } = await mount();

    expect(container.textContent).toContain("No usage recorded");
    expect(container.textContent).not.toContain("No rows match these filters");

    // A filter with no matches must not claim the log is empty.
    control("Set model filter")!.click();
    // The model filter is debounced, so let the timer fire before asserting.
    await new Promise((resolve) => setTimeout(resolve, 350));
    await settle();

    expect(container.textContent).toContain("No rows match these filters");
    expect(api.listUsage).toHaveBeenLastCalledWith(
      100,
      0,
      expect.objectContaining({ model: "nonexistent" }),
      { field: "time", descending: true },
    );

    // Clearing from the empty state restores the unfiltered view.
    control("Clear filters")!.click();
    await settle();
    expect(api.listUsage).toHaveBeenLastCalledWith(
      100,
      0,
      expect.objectContaining({ model: null }),
      { field: "time", descending: true },
    );

    unmount();
  });

  it("sorts by a column header and resets to the first page", async () => {
    vi.mocked(api.listUsage).mockResolvedValue([record(0)]);
    vi.mocked(api.usageSummary).mockResolvedValue(summary({ requests: 1 }));

    const { control, unmount } = await mount();

    control("Sort by Cost")!.click();
    await settle();

    expect(api.listUsage).toHaveBeenLastCalledWith(100, 0, expect.anything(), {
      field: "cost",
      descending: true,
    });

    // Re-clicking the active column flips the direction.
    control("Sort by Cost")!.click();
    await settle();

    expect(api.listUsage).toHaveBeenLastCalledWith(100, 0, expect.anything(), {
      field: "cost",
      descending: false,
    });

    unmount();
  });

  it("flags a failed refresh instead of presenting stale rows as live", async () => {
    vi.mocked(api.listUsage).mockResolvedValue([record(0)]);
    vi.mocked(api.usageSummary).mockResolvedValue(summary({ requests: 1 }));

    const { container, control, unmount } = await mount();

    // Wait for initial silent poll to populate table
    await new Promise((resolve) => setTimeout(resolve, 500));
    await settle();

    // Verify we have the data before simulating a failure
    expect(container.textContent).toContain("Showing rows 1–1 of 1");

    vi.mocked(api.usageSummary).mockRejectedValue(
      new ApiError("router unreachable", 0),
    );
    control("Refresh")!.click();
    
    // Wait longer for the async load + render to complete with the error
    await new Promise((resolve) => setTimeout(resolve, 300));
    await settle();

    expect(container.textContent).toContain(
      "Showing the last successful refresh",
    );
    expect(container.textContent).toContain("router unreachable");
    // The last good rows stay on screen rather than blanking out.
    expect(container.textContent).toContain("Showing rows 1–1 of 1");

    unmount();
  });

  it("shows table skeletons rather than bare text on the first load", async () => {
    let release: (() => void) | undefined;
    vi.mocked(api.usageSummary).mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => resolve(summary());
        }),
    );

    const app = createApp(UsagePage as never);
    app.use(
      createRouter({
        history: createMemoryHistory(),
        routes: [{ path: "/", component: { template: "<div />" } }],
      }),
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    app.mount(container);
    await nextTick();

    expect(container.querySelector(".animate-pulse")).toBeTruthy();

    release?.();
    await settle();
    app.unmount();
    container.remove();
  });
});
