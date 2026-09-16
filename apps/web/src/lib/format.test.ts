import { describe, expect, it } from "vitest";

import {
  formatCost,
  formatDateTime,
  formatDuration,
  formatLatency,
  formatRate,
  formatRelativeTime,
  isEnabled,
  maskSecret,
  parseHeaders,
  successRate,
} from "./format";

describe("isEnabled", () => {
  it("treats SQLite integers as booleans", () => {
    expect(isEnabled(1)).toBe(true);
    expect(isEnabled(0)).toBe(false);
  });
});

describe("parseHeaders", () => {
  it("parses a JSON object and stringifies values", () => {
    expect(parseHeaders('{"X-Org":"acme","Retries":3}')).toEqual({
      "X-Org": "acme",
      Retries: "3",
    });
  });

  it("falls back to an empty object for malformed input", () => {
    expect(parseHeaders("")).toEqual({});
    expect(parseHeaders("not json")).toEqual({});
    expect(parseHeaders("[1,2]")).toEqual({});
  });
});

describe("maskSecret", () => {
  it("keeps only the last characters visible", () => {
    const masked = maskSecret("sk-router-1234567890");
    expect(masked.endsWith("7890")).toBe(true);
    expect(masked).not.toContain("123456");
  });
});

describe("formatLatency", () => {
  it("uses milliseconds below one second", () => {
    expect(formatLatency(850)).toBe("850 ms");
  });

  it("uses seconds above one second", () => {
    expect(formatLatency(1500)).toBe("1.50 s");
  });

  it("renders zero and missing values as a dash", () => {
    expect(formatLatency(0)).toBe("—");
  });
});

describe("formatDuration", () => {
  it("counts seconds below a minute", () => {
    expect(formatDuration(45_000)).toBe("45s");
  });

  it("counts minutes below an hour", () => {
    expect(formatDuration(12 * 60_000)).toBe("12m");
  });

  it("keeps the leftover minutes on an hour-scale value", () => {
    expect(formatDuration(3 * 3_600_000 + 25 * 60_000)).toBe("3h 25m");
  });

  it("counts days with the leftover hours", () => {
    expect(formatDuration(2 * 86_400_000 + 5 * 3_600_000)).toBe("2d 5h");
  });

  it("renders zero and missing values as a dash", () => {
    expect(formatDuration(0)).toBe("—");
    expect(formatDuration(Number.NaN)).toBe("—");
  });
});

describe("formatCost", () => {
  it("renders zero as $0.00", () => {
    expect(formatCost(0)).toBe("$0.00");
  });

  it("keeps more precision for sub-dollar amounts", () => {
    // Locale-dependent separators: accept `0.0012` or `0,0012`.
    expect(formatCost(0.001234)).toMatch(/0[.,]0012/);
  });
});

describe("formatRate", () => {
  it("trims trailing zeros from per-million rates", () => {
    expect(formatRate(2.5)).toBe("$2.5");
    expect(formatRate(10)).toBe("$10");
    expect(formatRate(1.25)).toBe("$1.25");
  });

  it("renders missing values as a dash", () => {
    expect(formatRate(null)).toBe("—");
    expect(formatRate(undefined)).toBe("—");
  });
});

describe("successRate", () => {
  it("formats a percentage", () => {
    expect(successRate(3, 4)).toBe("75.0%");
  });

  it("renders no data as a dash", () => {
    expect(successRate(0, 0)).toBe("—");
  });
});

describe("formatDateTime", () => {
  it("renders missing values as a dash", () => {
    expect(formatDateTime(null)).toBe("—");
    expect(formatDateTime(undefined)).toBe("—");
  });
});

describe("formatRelativeTime", () => {
  it('renders recent timestamps as "just now"', () => {
    expect(formatRelativeTime(new Date().toISOString())).toBe("just now");
  });

  it("renders minutes and hours ago", () => {
    const minutes = new Date(Date.now() - 5 * 60_000).toISOString();
    expect(formatRelativeTime(minutes)).toBe("5m ago");

    const hours = new Date(Date.now() - 3 * 3_600_000).toISOString();
    expect(formatRelativeTime(hours)).toBe("3h ago");
  });

  it("renders missing values as a dash", () => {
    expect(formatRelativeTime(null)).toBe("—");
  });
});
