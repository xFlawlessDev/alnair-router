import { createApp, h, nextTick, ref } from "vue";
import { describe, expect, it, vi } from "vitest";

import ProviderPickerDialog from "./ProviderPickerDialog.vue";
import { api } from "@/lib/api";
import type { ProviderPreset } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: { listProviders: vi.fn() },
}));

/** Identifies a fixture; everything else falls back to a default. */
type PresetIdentity = Pick<
  ProviderPreset,
  "id" | "label" | "base_url" | "category"
>;

const preset = (
  overrides: PresetIdentity & Partial<ProviderPreset>,
): ProviderPreset => ({
  provider_type: "openai-compatible",
  auth: "api_key",
  default_headers: {},
  api_key_url: null,
  docs_url: null,
  note: null,
  configured: 0,
  ...overrides,
});

const presets: ProviderPreset[] = [
  preset({
    id: "openai",
    label: "OpenAI",
    base_url: "https://api.openai.com/v1",
    category: "api_key",
  }),
  preset({
    id: "openrouter",
    label: "OpenRouter",
    base_url: "https://openrouter.ai/api/v1",
    category: "free_tier",
  }),
  preset({
    id: "ollama",
    label: "Ollama",
    base_url: "http://localhost:11434/v1",
    category: "local",
    auth: "none",
  }),
];

/** Lets the picker's async provider load settle. */
const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const mountPicker = async (providers: ProviderPreset[] = presets) => {
  vi.mocked(api.listProviders).mockResolvedValue({
    object: "list",
    data: providers,
  });

  const open = ref(false);
  const selected = vi.fn();
  const quickAdd = vi.fn();
  const custom = vi.fn();

  const app = createApp({
    setup() {
      return () =>
        h(ProviderPickerDialog, {
          open: open.value,
          "onUpdate:open": (value: boolean) => {
            open.value = value;
          },
          onSelect: selected,
          onQuickAdd: quickAdd,
          onCustom: custom,
        });
    },
  });

  const container = document.createElement("div");
  document.body.appendChild(container);
  app.mount(container);

  open.value = true;
  await settle();

  return {
    selected,
    quickAdd,
    custom,
    cleanup: () => {
      app.unmount();
      container.remove();
    },
  };
};

const searchFor = async (term: string) => {
  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="Search providers"]',
  );
  expect(input, "search input should render").toBeTruthy();
  input!.value = term;
  input!.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
};

const card = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("[data-preset-card]")].find(
    (button) => button.getAttribute("aria-label") === `Configure ${label}`,
  );

describe("ProviderPickerDialog", () => {
  it("groups presets into tiers and marks keyed ones", async () => {
    const { cleanup } = await mountPicker();

    const text = document.body.textContent ?? "";
    expect(text).toContain("API key providers");
    expect(text).toContain("Free tier providers");
    expect(text).toContain("Local servers");
    expect(text).toContain("OpenAI");
    expect(text).toContain("OpenRouter");
    expect(text).toContain("Ollama");
    expect(text).toContain("no key");
    expect(text).toContain("key required");

    for (const label of ["OpenAI", "OpenRouter", "Ollama"]) {
      expect(card(label), `${label} card should render`).toBeTruthy();
    }

    cleanup();
  });

  it("drops tiers that have no match for the search term", async () => {
    const { cleanup } = await mountPicker();

    await searchFor("openrouter");

    const text = document.body.textContent ?? "";
    expect(text).toContain("OpenRouter");
    expect(text).toContain("Free tier providers");
    expect(text).not.toContain("API key providers");
    expect(text).not.toContain("Local servers");

    cleanup();
  });

  it("filters by tier without touching the search box", async () => {
    const { cleanup } = await mountPicker();

    const local = document.querySelector<HTMLButtonElement>(
      'button[aria-label="Local servers"]',
    );
    expect(local, "tier filter should render").toBeTruthy();
    local!.click();
    await settle();

    const text = document.body.textContent ?? "";
    expect(text).toContain("Ollama");
    expect(text).not.toContain("OpenAI");
    expect(text).not.toContain("OpenRouter");

    cleanup();
  });

  it("emits the picked preset", async () => {
    const { selected, cleanup } = await mountPicker();

    card("OpenRouter")!.click();
    await nextTick();

    expect(selected).toHaveBeenCalledWith(
      expect.objectContaining({ id: "openrouter" }),
    );

    cleanup();
  });

  it("only quick-adds keyless or already-configured presets", async () => {
    const { quickAdd, cleanup } = await mountPicker([
      ...presets,
      preset({
        id: "openai-work",
        label: "OpenAI Work",
        base_url: "https://api.openai.com/v1",
        category: "api_key",
        configured: 2,
      }),
    ]);

    const quickAddButton = (label: string) =>
      document.querySelector<HTMLButtonElement>(
        `button[aria-label="Quick add ${label}"], button[aria-label="Duplicate ${label}"]`,
      );

    expect(quickAddButton("OpenAI"), "keyed preset has no shortcut").toBeNull();
    expect(quickAddButton("Ollama")).toBeTruthy();

    quickAddButton("Ollama")!.click();
    await nextTick();
    expect(quickAdd).toHaveBeenCalledWith(
      expect.objectContaining({ id: "ollama" }),
    );

    expect(quickAddButton("OpenAI Work")).toBeTruthy();

    cleanup();
  });

  it("offers a custom endpoint shortcut only before searching", async () => {
    const { custom, cleanup } = await mountPicker();

    const customButton = document.querySelector<HTMLButtonElement>(
      'button[data-preset-card][aria-label="Custom endpoint"]',
    );
    expect(customButton, "custom card should render").toBeTruthy();
    customButton!.click();
    await nextTick();
    expect(custom).toHaveBeenCalled();

    await searchFor("openai");
    expect(
      document.querySelector(
        'button[data-preset-card][aria-label="Custom endpoint"]',
      ),
      "custom card hides while searching",
    ).toBeNull();

    cleanup();
  });
});
