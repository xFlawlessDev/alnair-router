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

    const save = [...document.querySelectorAll("button")].find((candidate) =>
      candidate.textContent?.includes("Save price"),
    );
    save!.click();
    await settle();

    // The number field stays a `number`; parsing must not assume a string.
    expect(api.upsertPricing).toHaveBeenCalledWith([
      {
        model: "gpt-4o",
        input_per_million_usd: 2.5,
        output_per_million_usd: 10,
        cache_read_per_million_usd: 1.25,
        cache_write_per_million_usd: null,
        reasoning_per_million_usd: null,
      },
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

    const save = [...document.querySelectorAll("button")].find((candidate) =>
      candidate.textContent?.includes("Save price"),
    );
    save!.click();
    await settle();

    expect(vi.mocked(api.upsertPricing).mock.calls[0]![0]).toMatchObject([
      { reasoning_per_million_usd: 0 },
    ]);

    unmount();
  });
});
