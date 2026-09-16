import { describe, expect, it } from "vitest";

import {
  configuredCaps,
  configuredTokenCaps,
  effectiveCaps,
  effectiveTokenCaps,
  fromLocalDateTime,
  isExpired,
  parsePositive,
  parsePositiveInt,
  toLocalDateTime,
} from "./limits";

describe("parsePositive", () => {
  it("treats blanks as unset and rejects non-positive values", () => {
    expect(parsePositive("")).toBeNull();
    expect(parsePositive("  ")).toBeNull();
    expect(parsePositive("2.5")).toBe(2.5);
    expect(parsePositive("0")).toBeNull();
    expect(parsePositive("-1")).toBeNull();
    expect(parsePositive("nope")).toBeNull();
  });
});

describe("parsePositiveInt", () => {
  it("requires a whole number and treats blanks as unset", () => {
    expect(parsePositiveInt("")).toBeNull();
    expect(parsePositiveInt("250000")).toBe(250_000);
    expect(parsePositiveInt("0")).toBeNull();
    expect(parsePositiveInt("-5")).toBeNull();
    expect(parsePositiveInt("1.5")).toBeNull();
    expect(parsePositiveInt("abc")).toBeNull();
  });
});

describe("configuredCaps", () => {
  it("lists only the windows that are set, in order", () => {
    const caps = configuredCaps({
      daily_budget_usd: 1,
      weekly_budget_usd: null,
      monthly_budget_usd: 25,
      lifetime_budget_usd: 100,
    });

    expect(caps.map(({ cap, amount }) => [cap.suffix, amount])).toEqual([
      ["day", 1],
      ["mo", 25],
      ["total", 100],
    ]);
  });

  it("is empty when nothing is capped", () => {
    expect(
      configuredCaps({
        daily_budget_usd: null,
        weekly_budget_usd: null,
        monthly_budget_usd: null,
        lifetime_budget_usd: null,
      }),
    ).toEqual([]);
  });
});

describe("configuredTokenCaps", () => {
  it("lists only the token windows that are set", () => {
    const caps = configuredTokenCaps({
      daily_token_limit: 1000,
      weekly_token_limit: null,
      monthly_token_limit: 50000,
      lifetime_token_limit: null,
    });

    expect(caps.map(({ cap, amount }) => [cap.suffix, amount])).toEqual([
      ["day", 1000],
      ["mo", 50000],
    ]);
  });
});

describe("effectiveCaps", () => {
  it("lets key caps win and fills the rest from the plan", () => {
    const caps = effectiveCaps(
      {
        daily_budget_usd: 1,
        weekly_budget_usd: null,
        monthly_budget_usd: 25,
        lifetime_budget_usd: null,
      },
      {
        daily_budget_usd: 9,
        weekly_budget_usd: 50,
        monthly_budget_usd: 100,
        lifetime_budget_usd: 500,
      },
    );

    expect(caps).toEqual({
      daily_budget_usd: 1,
      weekly_budget_usd: 50,
      monthly_budget_usd: 25,
      lifetime_budget_usd: 500,
    });
  });

  it("keeps the key caps without a plan", () => {
    const caps = effectiveCaps(
      {
        daily_budget_usd: null,
        weekly_budget_usd: 5,
        monthly_budget_usd: null,
        lifetime_budget_usd: null,
      },
      undefined,
    );

    expect(caps.weekly_budget_usd).toBe(5);
    expect(caps.daily_budget_usd).toBeNull();
  });
});

describe("effectiveTokenCaps", () => {
  it("lets key token limits win and fills the rest from the plan", () => {
    const caps = effectiveTokenCaps(
      {
        daily_token_limit: 1000,
        weekly_token_limit: null,
        monthly_token_limit: null,
        lifetime_token_limit: 90000,
      },
      {
        daily_token_limit: 9999,
        weekly_token_limit: 50000,
        monthly_token_limit: 500000,
        lifetime_token_limit: null,
      },
    );

    expect(caps).toEqual({
      daily_token_limit: 1000,
      weekly_token_limit: 50000,
      monthly_token_limit: 500000,
      lifetime_token_limit: 90000,
    });
  });
});

describe("datetime-local round trip", () => {
  it("converts a timestamp into an input value and back", () => {
    const iso = "2030-01-02T03:04:00.000Z";
    const local = toLocalDateTime(iso);

    expect(local).toMatch(/^2030-01-0[12]T\d{2}:04$/);
    expect(fromLocalDateTime(local)).toBe(iso);
  });

  it("treats blanks as null", () => {
    expect(toLocalDateTime(null)).toBe("");
    expect(fromLocalDateTime("")).toBeNull();
  });
});

describe("isExpired", () => {
  it("compares against now and ignores empty values", () => {
    expect(isExpired(null)).toBe(false);
    expect(isExpired(new Date(Date.now() - 60_000).toISOString())).toBe(true);
    expect(isExpired(new Date(Date.now() + 60_000).toISOString())).toBe(false);
  });
});
