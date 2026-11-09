import { createApp, nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import UsageShareCard from "./UsageShareCard.vue";
import { buildUsageShareSvg, usageShareFile } from "@/lib/usageShareCard";
import type { MyUsageResponse } from "@/types/api";

vi.mock("@/lib/usageShareCard", () => ({
  buildUsageShareSvg: vi.fn(() => '<svg xmlns="http://www.w3.org/2000/svg"/>'),
  usageShareFile: vi.fn(),
  USAGE_CARD_DIMENSIONS: {
    square: { width: 1080, height: 1080 },
    landscape: { width: 1200, height: 630 },
  },
  rankUsageModels: vi.fn(() => []),
}));
vi.mock("vue-sonner", () => ({ toast: { info: vi.fn() } }));

const usage = {
  key: { name: "Development", prefix: "sk-fixture" },
  since: null,
  until: null,
  summary: { requests: 10, prompt_tokens: 100, completion_tokens: 20 },
} as MyUsageResponse;
const settle = async () => {
  await nextTick();
  await Promise.resolve();
  await nextTick();
};
const mounted: (() => void)[] = [];

async function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  const app = createApp(UsageShareCard, { usage });
  app.mount(host);
  mounted.push(() => {
    app.unmount();
    host.remove();
  });
  await settle();
  return {
    host,
    button: (name: string) =>
      [...host.querySelectorAll("button")].find((el) =>
        el.textContent?.includes(name),
      )!,
  };
}

describe("UsageShareCard actions", () => {
  const png = new File(["fixture-image"], "alnair-api-usage.png", {
    type: "image/png",
  });
  let click: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    vi.mocked(usageShareFile).mockResolvedValue(png);
    click = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});
    vi.stubGlobal(
      "URL",
      Object.assign(URL, {
        createObjectURL: vi.fn(() => "blob:fixture"),
        revokeObjectURL: vi.fn(),
      }),
    );
    Object.defineProperty(navigator, "share", {
      configurable: true,
      value: vi.fn().mockResolvedValue(undefined),
    });
    Object.defineProperty(navigator, "canShare", {
      configurable: true,
      value: vi.fn(() => true),
    });
  });
  afterEach(() => {
    mounted.splice(0).forEach((cleanup) => cleanup());
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps export disabled until PNG preparation completes", async () => {
    let resolve!: (file: File) => void;
    vi.mocked(usageShareFile).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const { button } = await mount();
    expect(button("Download PNG").disabled).toBe(true);
    resolve(png);
    await settle();
    expect(button("Download PNG").disabled).toBe(false);
  });

  it("regenerates the preview and PNG for the selected format", async () => {
    const { button, host } = await mount();
    expect(button("Landscape").getAttribute("aria-pressed")).toBe("true");
    expect(usageShareFile).toHaveBeenLastCalledWith(
      expect.any(String),
      "landscape",
    );
    button("Square").click();
    await settle();
    expect(button("Square").getAttribute("aria-pressed")).toBe("true");
    expect(buildUsageShareSvg).toHaveBeenLastCalledWith(
      expect.any(Object),
      "square",
    );
    expect(usageShareFile).toHaveBeenLastCalledWith(
      expect.any(String),
      "square",
    );
    expect(host.querySelector("img")?.getAttribute("width")).toBe("1080");
    expect(host.querySelector("img")?.getAttribute("height")).toBe("1080");
  });

  it("shares the prepared image as a file", async () => {
    const { button } = await mount();
    button("Share image").click();
    await settle();
    expect(navigator.share).toHaveBeenCalledWith({
      files: [png],
      title: "My Alnair API usage",
    });
    expect(click).not.toHaveBeenCalled();
  });

  it("downloads the PNG when device file sharing is unavailable", async () => {
    vi.mocked(navigator.canShare).mockReturnValue(false);
    const { button } = await mount();
    button("Share image").click();
    await settle();
    expect(navigator.share).not.toHaveBeenCalled();
    expect(click).toHaveBeenCalledOnce();
    expect(URL.createObjectURL).toHaveBeenCalledWith(png);
  });

  it("does not download when the user cancels the share sheet", async () => {
    vi.mocked(navigator.share).mockRejectedValue(
      new DOMException("Cancelled", "AbortError"),
    );
    const { button } = await mount();
    button("Share image").click();
    await settle();
    expect(click).not.toHaveBeenCalled();
  });
});
