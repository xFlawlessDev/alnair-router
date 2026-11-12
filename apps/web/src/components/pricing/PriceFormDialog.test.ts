import { createApp, h, nextTick, ref } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PriceFormDialog from "./PriceFormDialog.vue";
import { api } from "@/lib/api";
import type { ModelPrice } from "@/types/api";

vi.mock("@/lib/api", () => ({
  ApiError: class ApiError extends Error {},
  api: { upsertPricing: vi.fn() },
}));

const settle = async () => {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
};

const input = (id: string) =>
  document.querySelector<HTMLInputElement>(`#${id}`)!;

/** Types into a `type="number"` input the way a browser binding would. */
const type = (id: string, value: string) => {
  const element = input(id);
  element.value = value;
  element.dispatchEvent(new Event("input", { bubbles: true }));
};

const clickButton = async (label: string) => {
  [...document.querySelectorAll("button")]
    .find((candidate) => candidate.textContent?.includes(label))!
    .click();
  await settle();
};

const mount = async (price: ModelPrice | null = null) => {
  const open = ref(true);
  const app = createApp({
    setup() {
      return () =>
        h(PriceFormDialog, {
          open: open.value,
          price,
          "onUpdate:open": (value: boolean) => {
            open.value = value;
          },
        });
    },
  });

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

const storedPrice = (overrides: Partial<ModelPrice> = {}): ModelPrice => ({
  model: "gpt-4o",
  input_per_million_usd: 2.5,
  output_per_million_usd: 10,
  cache_read_per_million_usd: null,
  cache_write_per_million_usd: null,
  reasoning_per_million_usd: null,
  tiers: [],
  unit_kind: null,
  unit_quantity: null,
  unit_price_usd: null,
  source: "override",
  updated_at: "2026-01-01T00:00:00Z",
  ...overrides,
});

describe("PriceFormDialog", () => {
  beforeEach(() => {
    vi.mocked(api.upsertPricing).mockResolvedValue({ updated: 1 });
  });

  it("saves numeric rate inputs bound to number fields", async () => {
    const { unmount } = await mount();

    type("price-model", "gpt-4o");
    type("price-input", "2.5");
    type("price-output", "10");
    type("price-cache-read", "1.25");
    await settle();

    await clickButton("Save price");

    // The number field stays a `number`; parsing must not assume a string.
    expect(api.upsertPricing).toHaveBeenCalledWith([
      {
        model: "gpt-4o",
        input_per_million_usd: 2.5,
        output_per_million_usd: 10,
        cache_read_per_million_usd: 1.25,
        cache_write_per_million_usd: null,
        reasoning_per_million_usd: null,
        tiers: [],
        unit_price: null,
      },
    ]);

    unmount();
  });

  it("saves a rate per 1,000 characters", async () => {
    const { unmount } = await mount();
    type("price-model", "voice-model");
    const select = document.querySelector<HTMLSelectElement>("#price-unit")!;
    select.value = "character";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    type("price-quantity", "1000");
    type("price-unit-rate", "0.03");
    await settle();
    await clickButton("Save price");
    expect(api.upsertPricing).toHaveBeenCalledWith([
      expect.objectContaining({
        model: "voice-model",
        unit_price: { unit: "character", quantity: 1000, price_usd: 0.03 },
      }),
    ]);
    unmount();
  });

  it("treats a cleared rate as null instead of failing", async () => {
    const { unmount } = await mount();

    type("price-model", "gpt-4o");
    type("price-input", "1");
    type("price-output", "1");
    type("price-reasoning", "0");
    await settle();

    await clickButton("Save price");

    expect(vi.mocked(api.upsertPricing).mock.calls[0]![0]).toMatchObject([
      { reasoning_per_million_usd: 0 },
    ]);

    unmount();
  });

  it("preserves existing conditional rules when editing", async () => {
    const { unmount } = await mount(
      storedPrice({
        tiers: [
          {
            context_above: 200000,
            start_minute_utc: null,
            end_minute_utc: null,
            input_per_million_usd: 5,
            output_per_million_usd: 20,
            cache_read_per_million_usd: null,
            cache_write_per_million_usd: null,
            reasoning_per_million_usd: null,
          },
          {
            context_above: null,
            start_minute_utc: 0,
            end_minute_utc: 480,
            input_per_million_usd: 1,
            output_per_million_usd: 2,
            cache_read_per_million_usd: 0.5,
            cache_write_per_million_usd: null,
            reasoning_per_million_usd: null,
          },
        ],
      }),
    );

    await clickButton("Save price");

    expect(vi.mocked(api.upsertPricing).mock.calls[0]![0]).toMatchObject([
      {
        model: "gpt-4o",
        tiers: [
          {
            context_above: 200000,
            start_minute_utc: null,
            end_minute_utc: null,
            input_per_million_usd: 5,
            output_per_million_usd: 20,
          },
          {
            context_above: null,
            start_minute_utc: 0,
            end_minute_utc: 480,
            input_per_million_usd: 1,
            output_per_million_usd: 2,
            cache_read_per_million_usd: 0.5,
          },
        ],
      },
    ]);

    unmount();
  });

  it("adds a rule with a context threshold and a UTC window", async () => {
    const { unmount } = await mount();
    type("price-model", "gpt-4o");
    type("price-input", "1");
    type("price-output", "2");
    await settle();

    await clickButton("Add rule");

    type("tier-context-0", "128000");
    type("tier-input-0", "3");
    type("tier-output-0", "6");
    document.querySelector<HTMLElement>('[role="checkbox"]')!.click();
    await settle();
    type("tier-start-0", "22:30");
    type("tier-end-0", "06:00");
    await settle();

    await clickButton("Save price");

    expect(vi.mocked(api.upsertPricing).mock.calls[0]![0]).toMatchObject([
      {
        tiers: [
          {
            context_above: 128000,
            start_minute_utc: 1350,
            end_minute_utc: 360,
            input_per_million_usd: 3,
            output_per_million_usd: 6,
            cache_read_per_million_usd: null,
            cache_write_per_million_usd: null,
            reasoning_per_million_usd: null,
          },
        ],
      },
    ]);

    unmount();
  });

  it("rejects a rule whose UTC window has equal bounds", async () => {
    const { unmount } = await mount();
    type("price-model", "gpt-4o");
    type("price-input", "1");
    type("price-output", "2");
    await settle();

    await clickButton("Add rule");
    document.querySelector<HTMLElement>('[role="checkbox"]')!.click();
    await settle();
    type("tier-start-0", "08:00");
    type("tier-end-0", "08:00");
    await settle();

    await clickButton("Save price");

    expect(api.upsertPricing).not.toHaveBeenCalled();

    unmount();
  });

  it("removes and reorders rules", async () => {
    const { unmount } = await mount(
      storedPrice({
        tiers: [
          {
            context_above: 1000,
            start_minute_utc: null,
            end_minute_utc: null,
            input_per_million_usd: 1,
            output_per_million_usd: 1,
            cache_read_per_million_usd: null,
            cache_write_per_million_usd: null,
            reasoning_per_million_usd: null,
          },
          {
            context_above: 2000,
            start_minute_utc: null,
            end_minute_utc: null,
            input_per_million_usd: 2,
            output_per_million_usd: 2,
            cache_read_per_million_usd: null,
            cache_write_per_million_usd: null,
            reasoning_per_million_usd: null,
          },
        ],
      }),
    );

    document
      .querySelector<HTMLElement>('[aria-label="Move rule 2 up"]')!
      .click();
    await settle();
    document
      .querySelector<HTMLElement>('[aria-label="Remove rule 2"]')!
      .click();
    await settle();

    await clickButton("Save price");

    expect(vi.mocked(api.upsertPricing).mock.calls[0]![0]).toMatchObject([
      {
        tiers: [
          {
            context_above: 2000,
            input_per_million_usd: 2,
            output_per_million_usd: 2,
          },
        ],
      },
    ]);

    unmount();
  });
});
