import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ConsolePage from "./ConsolePage.vue";
import { ApiError, api } from "@/lib/api";
import type { ActivityEvent, ActivitySnapshot } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 0;
    readonly type = undefined;
  },
  api: { activity: vi.fn() },
}));

const event = (overrides: Partial<ActivityEvent> = {}): ActivityEvent => ({
  seq: 1,
  at: "2026-01-01T00:00:00.000Z",
  level: "info",
  kind: "attempt.start",
  connection: "openai-main",
  model: "gpt-4o",
  message: "tier 1 → openai-main / gpt-4o (default:openai-main)",
  ...overrides,
});

const snapshot = (
  overrides: Partial<ActivitySnapshot> = {},
): ActivitySnapshot => ({
  uptime_ms: 1000,
  active: [],
  connections: [],
  events: [event()],
  ...overrides,
});

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async () => {
  const app = createApp(ConsolePage as never);
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

  // Stop the 1.5s poll; a timer left running makes the suite flaky.
  control("Pause")?.click();
  await settle();

  const feed = () =>
    container.querySelector<HTMLElement>('[role="log"]') as HTMLElement;

  return {
    container,
    control,
    feed,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("ConsolePage", () => {
  beforeEach(() => {
    vi.mocked(api.activity).mockResolvedValue(snapshot());
  });

  it("renders each event with its level, kind and message", async () => {
    vi.mocked(api.activity).mockResolvedValue(
      snapshot({
        events: [
          event({ seq: 1, level: "info", kind: "attempt.start" }),
          event({
            seq: 2,
            level: "error",
            kind: "auth.denied",
            connection: undefined,
            model: undefined,
            message: "POST /v1/chat/completions — missing Authorization header",
            status: 401,
          }),
        ],
      }),
    );

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("2 events");
    expect(container.textContent).toContain("attempt.start");
    expect(container.textContent).toContain("auth.denied");
    expect(container.textContent).toContain("401");
    // The unauthenticated event has no connection, so the column shows a dash.
    expect(container.textContent).toContain("—");

    unmount();
  });

  it("filters by level and search, and reports the reduced count", async () => {
    vi.mocked(api.activity).mockResolvedValue(
      snapshot({
        events: [
          event({ seq: 1, level: "info", message: "all good" }),
          event({ seq: 2, level: "error", message: "upstream exploded" }),
        ],
      }),
    );

    const { container, control, unmount } = await mount();

    control("Show error events")!.click();
    await settle();

    expect(container.textContent).toContain("1 of 2 events");
    expect(container.textContent).toContain("upstream exploded");
    expect(container.textContent).not.toContain("all good");

    // Level and search are combined, so a term that misses the surviving event
    // must empty the feed rather than fall back to the buffered list.
    const search = container.querySelector<HTMLInputElement>(
      '[aria-label="Search events"]',
    );
    search!.value = "all good";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    expect(container.textContent).toContain("No events match this filter");

    unmount();
  });

  it("distinguishes an empty buffer from an empty filter result", async () => {
    vi.mocked(api.activity).mockResolvedValue(snapshot({ events: [] }));

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("No activity yet");
    expect(container.textContent).not.toContain("No events match this filter");

    unmount();
  });

  it("expands an event to its details and copies it as one line", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    vi.mocked(api.activity).mockResolvedValue(
      snapshot({
        events: [
          event({ seq: 7, latency_ms: 420, status: 200, message: "answered" }),
        ],
      }),
    );

    const { container, unmount } = await mount();

    const row = container.querySelector<HTMLButtonElement>(
      '[aria-expanded="false"]',
    );
    expect(row).toBeTruthy();
    row!.click();
    await settle();

    expect(container.textContent).toContain("seq 7");
    expect(container.textContent).toContain("Copy line");

    const copy = [...container.querySelectorAll("button")].find((candidate) =>
      candidate.textContent?.includes("Copy line"),
    );
    copy!.click();
    await settle();

    expect(writeText).toHaveBeenCalledTimes(1);
    const line = writeText.mock.calls[0]![0] as string;
    expect(line).toContain("INFO");
    expect(line).toContain("attempt.start");
    expect(line).toContain("openai-main");
    expect(line).toContain("420ms");
    expect(line).toContain("200");

    unmount();
  });

  it("surfaces in-flight attempts with their elapsed time", async () => {
    vi.mocked(api.activity).mockResolvedValue(
      snapshot({
        active: [
          {
            connection_id: "c1",
            connection: "openai-main",
            model: "gpt-4o",
            source: "alias:oa",
            tier: 2,
            elapsed_ms: 250,
          },
        ],
      }),
    );

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("1 in flight");
    // Sub-second waits must not render as the "—" that formatLatency uses.
    expect(container.textContent).toContain("250 ms");

    unmount();
  });

  it("flags a failed poll instead of presenting a stale feed as live", async () => {
    const { container, control, unmount } = await mount();

    expect(container.textContent).toContain("attempt.start");

    vi.mocked(api.activity).mockRejectedValue(
      new ApiError("router unreachable", 0),
    );
    control("Refresh")!.click();
    await settle();

    expect(container.textContent).toContain("Refresh failed");
    expect(container.textContent).toContain("router unreachable");
    // The last good events stay on screen rather than blanking out.
    expect(container.textContent).toContain("attempt.start");

    unmount();
  });

  it("offers a retry when the first load never succeeded", async () => {
    vi.mocked(api.activity).mockRejectedValue(
      new ApiError("router unreachable", 0),
    );

    const { container, control, unmount } = await mount();

    expect(container.textContent).toContain("router unreachable");
    expect(container.textContent).not.toContain("attempt.start");

    vi.mocked(api.activity).mockResolvedValue(snapshot());
    control("Retry")!.click();
    await settle();

    expect(container.textContent).toContain("attempt.start");

    unmount();
  });
});
