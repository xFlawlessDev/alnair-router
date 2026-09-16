import { createApp, h, nextTick } from "vue";
import { describe, expect, it, vi } from "vitest";

import ConnectionsToolbar from "./ConnectionsToolbar.vue";
import {
  DEFAULT_FILTERS,
  type ConnectionFilters,
  type ConnectionsView,
} from "@/lib/connectionsView";

interface MountOptions {
  filters?: Partial<ConnectionFilters>;
  view?: ConnectionsView;
}

const mount = (options: MountOptions = {}) => {
  const pickedView = vi.fn();
  const cleared = vi.fn();
  const searched = vi.fn();

  const app = createApp({
    setup() {
      return () =>
        h(ConnectionsToolbar, {
          filters: { ...DEFAULT_FILTERS, ...options.filters },
          view: options.view ?? "list",
          group: "none",
          shown: 2,
          total: 5,
          "onUpdate:view": pickedView,
          "onUpdate:search": searched,
          onClear: cleared,
        });
    },
  });

  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);

  const button = (label: string) =>
    [...container.querySelectorAll("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === label,
    );

  return {
    container,
    pickedView,
    cleared,
    searched,
    button,
    unmount: () => {
      app.unmount();
      container.remove();
    },
  };
};

describe("ConnectionsToolbar", () => {
  it("reports the visible count and emits the picked view", async () => {
    const { container, pickedView, button, unmount } = mount();
    await nextTick();

    expect(container.textContent).toContain("2 of 5");

    const grid = button("Grid view");
    expect(grid, "grid toggle should render").toBeTruthy();
    grid!.click();
    await nextTick();

    expect(pickedView).toHaveBeenCalledWith("grid");

    unmount();
  });

  it("emits search updates as you type", async () => {
    const { container, searched, unmount } = mount();
    await nextTick();

    const input =
      container.querySelector<HTMLInputElement>("#connection-search");
    expect(input, "search input should render").toBeTruthy();
    input!.value = "groq";
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await nextTick();

    expect(searched).toHaveBeenCalledWith("groq");

    unmount();
  });

  it("offers clearing only while a filter is active", async () => {
    const idle = mount();
    await nextTick();
    expect(idle.container.textContent).not.toContain("Clear filters");
    idle.unmount();

    const filtered = mount({ filters: { search: "openai" } });
    await nextTick();

    const clear = [...filtered.container.querySelectorAll("button")].find(
      (candidate) => candidate.textContent?.includes("Clear filters"),
    );
    expect(clear, "clear button should appear").toBeTruthy();

    clear!.click();
    await nextTick();
    expect(filtered.cleared).toHaveBeenCalled();

    filtered.unmount();
  });
});
