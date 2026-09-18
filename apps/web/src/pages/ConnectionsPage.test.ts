import { createApp, nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ConnectionsPage from "./ConnectionsPage.vue";
import { api } from "@/lib/api";
import type { Connection, ProviderPreset } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: {
    listConnections: vi.fn(),
    listProviders: vi.fn(),
    createConnection: vi.fn(),
    updateConnection: vi.fn(),
    testConnection: vi.fn(),
    deleteConnection: vi.fn(),
  },
}));

/** Identifies a fixture; everything else falls back to a default. */
type PresetIdentity = Pick<ProviderPreset, "id" | "label">;

const preset = (
  overrides: PresetIdentity & Partial<ProviderPreset>,
): ProviderPreset => ({
  provider_type: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  category: "api_key",
  auth: "api_key",
  default_headers: {},
  api_key_url: null,
  docs_url: null,
  note: null,
  configured: 0,
  ...overrides,
});

const connection = (overrides: Partial<Connection>): Connection => ({
  id: "c1",
  name: "openai-main",
  provider_type: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  api_key: "sk-test",
  custom_headers: "{}",
  enabled: 1,
  connect_timeout_ms: null,
  idle_timeout_ms: null,
  pricing_model: null,
  provider_id: "openai",
  account_count: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...overrides,
});

const rows = [
  connection({ id: "a", name: "openai-main", provider_id: "openai" }),
  connection({
    id: "b",
    name: "claude-main",
    provider_id: "anthropic",
    provider_type: "anthropic-native",
    base_url: "https://api.anthropic.com/v1",
  }),
];

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mount = async () => {
  const app = createApp(ConnectionsPage as never);
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
  [...container.querySelectorAll("button")].find(
    (candidate) => candidate.getAttribute("aria-label") === label,
  );

describe("ConnectionsPage", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.mocked(api.listConnections).mockResolvedValue(rows);
    vi.mocked(api.listProviders).mockResolvedValue({
      object: "list",
      data: [
        preset({ id: "openai", label: "OpenAI" }),
        preset({ id: "anthropic", label: "Anthropic" }),
      ],
    });
  });

  it("lists every connection and filters them from the toolbar", async () => {
    const { container, unmount } = await mount();

    expect(container.textContent).toContain("openai-main");
    expect(container.textContent).toContain("claude-main");
    expect(container.textContent).toContain("2 of 2");

    const input =
      container.querySelector<HTMLInputElement>("#connection-search");
    input!.value = "claude";
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    expect(container.textContent).toContain("claude-main");
    expect(container.textContent).not.toContain("openai-main");
    expect(container.textContent).toContain("1 of 2");

    unmount();
  });

  it("swaps the table for cards when the grid view is picked", async () => {
    const { container, unmount } = await mount();
    expect(container.querySelector("table")).toBeTruthy();

    button(container, "Grid view")!.click();
    await settle();

    expect(container.querySelector("table")).toBeNull();
    expect(container.textContent).toContain("openai-main");
    expect(container.textContent).toContain("claude-main");
    // The switch to the table comes back with the list toggle.
    button(container, "List view")!.click();
    await settle();
    expect(container.querySelector("table")).toBeTruthy();

    unmount();
  });

  it("renders group headings for a remembered grouping", async () => {
    window.localStorage.setItem("connections.group", "provider");

    const { container, unmount } = await mount();

    const headings = [...container.querySelectorAll("h3")].map((heading) =>
      heading.textContent?.trim(),
    );
    expect(headings).toEqual(["Anthropic · 1", "OpenAI · 1"]);

    unmount();
  });

  it("exposes a single Add provider entry point", async () => {
    const { container, unmount } = await mount();

    const labels = [...container.querySelectorAll("button")].map((candidate) =>
      candidate.textContent?.trim(),
    );
    expect(labels).toContain("Add provider");
    expect(labels).not.toContain("Add connection");

    unmount();
  });

  it("quick-adds a keyless preset without opening the form", async () => {
    vi.mocked(api.listProviders).mockResolvedValue({
      object: "list",
      data: [
        preset({ id: "openai", label: "OpenAI" }),
        preset({
          id: "ollama",
          label: "Ollama",
          category: "local",
          auth: "none",
          base_url: "http://localhost:11434/v1",
        }),
      ],
    });
    vi.mocked(api.createConnection).mockResolvedValue(connection({}));

    const { container, unmount } = await mount();

    [...container.querySelectorAll("button")]
      .find((candidate) => candidate.textContent?.includes("Add provider"))!
      .click();
    await settle();

    document
      .querySelector<HTMLButtonElement>(
        'button[aria-label="Quick add Ollama"]',
      )!
      .click();
    await settle();

    expect(api.createConnection).toHaveBeenCalledWith(
      expect.objectContaining({
        provider_id: "ollama",
        base_url: "http://localhost:11434/v1",
      }),
    );

    unmount();
  });
});
