import { describe, expect, it } from "vitest";

import { formatNumber } from "@/lib/format";
import {
  describeTier,
  draftToTier,
  emptyTierDraft,
  minutesToTime,
  tierToDraft,
  timeToMinutes,
} from "./pricingTiers";
import type { PriceTier } from "@/types/api";

const tier: PriceTier = {
  context_above: 200000,
  start_minute_utc: 1350,
  end_minute_utc: 360,
  input_per_million_usd: 5,
  output_per_million_usd: 20,
  cache_read_per_million_usd: null,
  cache_write_per_million_usd: 1,
  reasoning_per_million_usd: null,
};

describe("minutesToTime / timeToMinutes", () => {
  it("round-trips minutes after UTC midnight", () => {
    expect(minutesToTime(0)).toBe("00:00");
    expect(minutesToTime(1350)).toBe("22:30");
    expect(minutesToTime(1439)).toBe("23:59");
    expect(timeToMinutes("22:30")).toBe(1350);
    expect(timeToMinutes("00:00")).toBe(0);
  });

  it("rejects values that are not a clock time", () => {
    expect(timeToMinutes("24:00")).toBeNull();
    expect(timeToMinutes("7:05")).toBeNull();
    expect(timeToMinutes("")).toBeNull();
  });
});

describe("tierToDraft / draftToTier", () => {
  it("round-trips a stored rule", () => {
    const draft = tierToDraft(tier);
    expect(draft).toMatchObject({
      contextAbove: "200000",
      timeWindow: true,
      start: "22:30",
      end: "06:00",
      input: "5",
      output: "20",
      cacheRead: "",
      cacheWrite: "1",
      reasoning: "",
    });
    expect(draftToTier(draft)).toEqual({ tier });
  });

  it("treats blank optional fields as unconstrained", () => {
    const draft = { ...emptyTierDraft(), input: "1", output: "2" };
    expect(draftToTier(draft)).toEqual({
      tier: {
        context_above: null,
        start_minute_utc: null,
        end_minute_utc: null,
        input_per_million_usd: 1,
        output_per_million_usd: 2,
        cache_read_per_million_usd: null,
        cache_write_per_million_usd: null,
        reasoning_per_million_usd: null,
      },
    });
  });

  it("requires both window bounds to differ", () => {
    const draft = {
      ...emptyTierDraft(),
      input: "1",
      output: "2",
      timeWindow: true,
      start: "08:00",
      end: "08:00",
    };
    expect(draftToTier(draft)).toEqual({
      error: "UTC start and end times must differ",
    });
  });

  it("rejects a fractional context threshold", () => {
    const draft = {
      ...emptyTierDraft(),
      input: "1",
      output: "2",
      contextAbove: "1.5",
    };
    expect(draftToTier(draft)).toEqual({
      error: "context threshold must be a whole number of tokens",
    });
  });

  it("requires input and output rates", () => {
    const draft = { ...emptyTierDraft(), input: "", output: "2" };
    expect(draftToTier(draft)).toEqual({
      error: "input rate is required and must be zero or positive",
    });
  });
});

describe("describeTier", () => {
  it("summarizes both conditions", () => {
    expect(describeTier(tier)).toBe(
      `context > ${formatNumber(200000)} tokens · 22:30–06:00 UTC`,
    );
  });

  it("names an unconditional rule", () => {
    expect(
      describeTier({
        ...tier,
        context_above: null,
        start_minute_utc: null,
        end_minute_utc: null,
      }),
    ).toBe("Any request");
  });
});
