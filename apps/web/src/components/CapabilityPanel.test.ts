import { createApp, nextTick } from "vue";
import { describe, expect, it, vi } from "vitest";
import CapabilityPanel from "./CapabilityPanel.vue";
import { api } from "@/lib/api";
import type { CapabilityCatalog, ModelCapabilities } from "@/types/api";

vi.mock("@/lib/api", () => ({
  api: { saveCapabilities: vi.fn().mockResolvedValue(undefined), deleteCapabilities: vi.fn() },
}));

const settle = async () => {
  await nextTick();
  await Promise.resolve();
  await nextTick();
};
const capabilities: ModelCapabilities = {
  context: null, reasoning: null, tool_call: null,
  reasoning_levels: ["legacy-level"], input_formats: [], output_formats: null,
};

async function mount(initialContext: number | null = null) {
  const catalog: CapabilityCatalog = {
    entries: [{ provider: "p", model: "m", capabilities: { ...capabilities, input_formats: ["new-format"] } }],
    overrides: [{ scope: "alias", target_id: "a", capabilities: { ...capabilities, context: initialContext } }], updated_at: null,
  };
  const el = document.createElement("div");
  document.body.appendChild(el);
  const app = createApp(CapabilityPanel, { scope: "alias", id: "a", targets: [], catalog });
  app.mount(el);
  await settle();
  [...document.querySelectorAll("button")].find((button) => button.textContent?.includes("Edit capabilities"))!.click();
  await settle();
  return () => { app.unmount(); el.remove(); };
}

function source(label: string, value: string) {
  const select = document.querySelector<HTMLSelectElement>(`select[aria-label="${label} source"]`)!;
  select.value = value;
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

async function save() {
  [...document.querySelectorAll("button")].find((button) => button.textContent?.includes("Save capabilities"))!.click();
  await settle();
}

describe("capability multiselect", () => {
  it("preserves existing unknown values, empty overrides and inheritance", async () => {
    vi.mocked(api.saveCapabilities).mockClear();
    const unmount = await mount();
    expect(document.body.textContent).toContain("legacy-level");
    await save();
    expect(api.saveCapabilities).toHaveBeenCalledWith("alias", "a", capabilities);
    unmount();
  });

  it("selects catalog values and switches an override back to inheritance", async () => {
    vi.mocked(api.saveCapabilities).mockClear();
    const unmount = await mount();
    source("Reasoning levels", "catalog");
    const option = [...document.querySelectorAll("label")].find((label) => label.textContent?.trim() === "new-format")!;
    option.querySelector<HTMLElement>('[role="checkbox"]')!.click();
    await settle();
    await save();
    expect(api.saveCapabilities).toHaveBeenCalledWith("alias", "a", expect.objectContaining({
      reasoning_levels: null, input_formats: ["new-format"], output_formats: null,
    }));
    unmount();
  });

  it("saves a numeric context window", async () => {
    vi.mocked(api.saveCapabilities).mockClear();
    const unmount = await mount();
    const context = document.querySelector<HTMLInputElement>("#cap-context-a")!;
    context.value = "128000";
    context.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    await save();
    expect(api.saveCapabilities).toHaveBeenCalledWith(
      "alias", "a", expect.objectContaining({ context: 128000 }),
    );
    unmount();
  });

  it.each([
    ["128K", 128000],
    ["1M", 1000000],
    ["1.5m", 1500000],
    ["131072", 131072],
  ])("saves %s as %i tokens", async (text, expected) => {
    vi.mocked(api.saveCapabilities).mockClear();
    const unmount = await mount();
    const context = document.querySelector<HTMLInputElement>("#cap-context-a")!;
    context.value = text;
    context.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    await save();
    expect(api.saveCapabilities).toHaveBeenCalledWith(
      "alias", "a", expect.objectContaining({ context: expected }),
    );
    unmount();
  });

  it("rejects invalid and fractional token counts", async () => {
    const unmount = await mount();
    const context = document.querySelector<HTMLInputElement>("#cap-context-a")!;
    for (const text of ["0", "0.0001K", "128KB"]) {
      vi.mocked(api.saveCapabilities).mockClear();
      context.value = text;
      context.dispatchEvent(new Event("input", { bubbles: true }));
      await settle();
      await save();
      expect(api.saveCapabilities).not.toHaveBeenCalled();
    }
    unmount();
  });

  it.each([[128000, "128K"], [1000000, "1M"], [131072, "131072"]])(
    "edits %i tokens without losing precision",
    async (tokens, expected) => {
      const unmount = await mount(tokens);
      expect(document.querySelector<HTMLInputElement>("#cap-context-a")!.value).toBe(expected);
      unmount();
    },
  );
});
