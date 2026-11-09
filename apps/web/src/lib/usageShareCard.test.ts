import { describe, expect, it } from "vitest";
import { formatCompact, formatCost } from "./format";
import {
  buildUsageShareSvg,
  rankUsageModels,
  usageCardPeriod,
  type UsageShareSnapshot,
} from "./usageShareCard";

const snapshot = (): UsageShareSnapshot => ({
  name: "Development",
  since: null,
  until: null,
  capturedAt: new Date("2026-11-10T12:00:00Z"),
  summary: {
    requests: 100,
    ok_requests: 98,
    prompt_tokens: 28000,
    completion_tokens: 6000,
    cost_usd: 0.24,
  },
});

describe("usage share image", () => {
  it("uses actual totals and a fixed square export size", () => {
    const svg = buildUsageShareSvg(snapshot());
    const document = new DOMParser().parseFromString(svg, "image/svg+xml");
    expect(document.querySelector("parsererror")).toBeNull();
    expect(document.documentElement.getAttribute("viewBox")).toBe(
      "0 0 1080 1080",
    );
    expect(document.documentElement.textContent).toContain(
      formatCompact(34000),
    );
    expect(document.documentElement.textContent).toContain("98.0% successful");
    expect(document.documentElement.textContent).toContain(formatCost(0.24));
  });

  it("escapes names as text and never serializes unrelated credentials", () => {
    const data = {
      ...snapshot(),
      name: '<script onload="x">& key',
      api_key: "sk-live-not-for-sharing",
      prefix: "private-prefix",
      base_url: "https://private-router.example",
    };
    const svg = buildUsageShareSvg(data);
    const document = new DOMParser().parseFromString(svg, "image/svg+xml");
    expect(document.querySelector("parsererror")).toBeNull();
    expect(document.querySelector("script")).toBeNull();
    expect(document.documentElement.textContent).toContain(data.name);
    expect(svg).not.toContain(data.api_key);
    expect(svg).not.toContain(data.prefix);
    expect(svg).not.toContain(data.base_url);
  });

  it("keeps the response time window rather than the current filter selection", () => {
    expect(usageCardPeriod(snapshot())).toBe("All time");
    expect(
      usageCardPeriod({
        ...snapshot(),
        since: "2026-11-01T00:00:00Z",
        until: "2026-11-02T06:30:00Z",
      }),
    ).toBe("Nov 1, 2026, 00:00 to Nov 2, 2026, 06:30 UTC");
  });

  it("does not invent a success rate for a key with no requests", () => {
    const data = snapshot();
    data.summary = {
      requests: 0,
      ok_requests: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      cost_usd: 0,
    };
    expect(buildUsageShareSvg(data)).toContain("No requests in this period");
  });

  it("ranks top models by prompt plus completion tokens without changing the input", () => {
    const models = [
      {
        model: "many-requests",
        requests: 90,
        error_requests: 0,
        prompt_tokens: 100,
        completion_tokens: 100,
        cost_usd: 0.1,
      },
      {
        model: "most-tokens",
        requests: 10,
        error_requests: 0,
        prompt_tokens: 1000,
        completion_tokens: 500,
        cost_usd: 0.1,
      },
    ];
    expect(rankUsageModels(models, 1700)[0].model).toBe("most-tokens");
    expect(models[0].model).toBe("many-requests");
    const svg = buildUsageShareSvg({ ...snapshot(), models });
    expect(svg).toContain("TOP MODELS / TOKEN SHARE");
    expect(svg).toContain("most-tokens");
    expect(rankUsageModels([], 0)).toEqual([]);
  });

  it("renders the landscape size and computes shares against all key tokens", () => {
    const models = [
      {
        model: "primary",
        requests: 10,
        error_requests: 0,
        prompt_tokens: 16000,
        completion_tokens: 1000,
        cost_usd: 0.1,
      },
    ];
    const svg = buildUsageShareSvg({ ...snapshot(), models }, "landscape");
    const document = new DOMParser().parseFromString(svg, "image/svg+xml");
    expect(document.querySelector("parsererror")).toBeNull();
    expect(document.documentElement.getAttribute("viewBox")).toBe(
      "0 0 1200 630",
    );
    expect(document.documentElement.textContent).toContain("50%");
    expect(document.documentElement.textContent).toContain("TOKENS / REQUEST");
  });
});
