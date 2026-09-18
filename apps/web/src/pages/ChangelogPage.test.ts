import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ChangelogPage from "./ChangelogPage.vue";
import { ApiError, api } from "@/lib/api";
import type { UpdateStatus } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {
    readonly status = 0;
    readonly type = undefined;
  },
  api: { update: vi.fn() },
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
  const app = createApp(ChangelogPage as never);
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

  const search = () =>
    container.querySelector<HTMLInputElement>(
      '[aria-label="Filter the changelog"]',
    ) as HTMLInputElement;

  return {
    container,
    control,
    search,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("ChangelogPage", () => {
  beforeEach(() => {
    vi.mocked(api.update).mockResolvedValue(status());
  });

  it("renders the bundled releases with their changes", async () => {
    const { container, unmount } = await mount();

    // The shipped CHANGELOG.md is parsed at import time, so the latest release
    // and one of its bullets must be on screen.
    expect(container.textContent).toContain("v0.1.0");
    expect(container.textContent).toContain("Features");
    expect(container.textContent).toContain("Bug Fixes");
    expect(container.textContent).toContain("Running");

    // Markdown is rendered, not echoed: no link syntax or backticks survive,
    // and a commit bullet becomes a real anchor.
    expect(container.textContent).not.toContain("](");
    expect(container.textContent).not.toContain("`");
    expect(container.querySelector('a[href*="/commit/"]')).toBeTruthy();

    unmount();
  });

  it("reports when a newer release is available", async () => {
    vi.mocked(api.update).mockResolvedValue(
      status({
        latest_version: "9.9.9",
        update_available: true,
        release_url: "https://example.test/release",
        release_notes: "Big changes",
      }),
    );

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("v9.9.9 available");
    expect(container.textContent).toContain("Big changes");
    const link = container.querySelector<HTMLAnchorElement>(
      'a[href="https://example.test/release"]',
    );
    expect(link).toBeTruthy();

    unmount();
  });

  it("shows an up-to-date badge when no newer release exists", async () => {
    const { container, unmount } = await mount();

    expect(container.textContent).toContain("Up to date");
    expect(container.textContent).not.toContain("available");

    unmount();
  });

  it("falls back to the running version when the lookup failed", async () => {
    vi.mocked(api.update).mockResolvedValue(
      status({
        latest_version: null,
        checked_at: null,
        error: "cannot reach GitHub",
      }),
    );

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("Check failed");
    expect(container.textContent).toContain("cannot reach GitHub");
    // The page still names the running version, which is the point of the
    // fallback.
    expect(container.textContent).toContain("Running v0.1.0");

    unmount();
  });

  it("explains when update checks are turned off", async () => {
    vi.mocked(api.update).mockResolvedValue(
      status({ check_enabled: false, latest_version: null }),
    );

    const { container, unmount } = await mount();

    expect(container.textContent).toContain("Update checks off");
    expect(container.textContent).not.toContain("Up to date");

    unmount();
  });

  it("surfaces a transport failure on the manual refresh", async () => {
    const { container, control, unmount } = await mount();

    vi.mocked(api.update).mockRejectedValue(
      new ApiError("router unreachable", 0),
    );
    control("Check for updates")!.click();
    await settle();

    expect(container.textContent).toContain("router unreachable");

    unmount();
  });

  it("filters releases by version and by change text", async () => {
    const { container, search, unmount } = await mount();

    // A term matching a bullet keeps only the matching changes.
    search().value = "providers";
    search().dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    expect(container.textContent).toContain("OpenAI-compatible wire format");
    // A bullet that does not match the term is dropped from the release.
    expect(container.textContent).not.toContain("Bug Fixes");

    search().value = "zzz-no-such-change";
    search().dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    expect(container.textContent).toContain("Nothing matches");

    unmount();
  });
});
