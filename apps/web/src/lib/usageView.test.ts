import { describe, expect, it } from "vitest";

import { formatNumber } from "@/lib/format";
import {
  ALL_FILTER,
  bucketForRange,
  DEFAULT_USAGE_SORT,
  hasActiveFilters,
  hasNextPage,
  isSlowLatency,
  nextSort,
  paginationLabel,
} from "./usageView";

const noFilters = {
  apiKeyId: ALL_FILTER,
  model: "",
  provider: ALL_FILTER,
  connection: ALL_FILTER,
  range: "all",
};

describe("usageView", () => {
  it("only reports filters when something actually narrows the query", () => {
    expect(hasActiveFilters(noFilters)).toBe(false);
    // Whitespace in the model box is not a filter.
    expect(hasActiveFilters({ ...noFilters, model: "   " })).toBe(false);

    expect(hasActiveFilters({ ...noFilters, range: "24h" })).toBe(true);
    expect(hasActiveFilters({ ...noFilters, apiKeyId: "key-1" })).toBe(true);
    expect(hasActiveFilters({ ...noFilters, provider: "openai" })).toBe(true);
    expect(hasActiveFilters({ ...noFilters, connection: "openai-main" })).toBe(
      true,
    );
    expect(hasActiveFilters({ ...noFilters, model: "gpt-4o" })).toBe(true);
  });

  it("does not offer a Next page that would land on nothing", () => {
    // A last page that is exactly full must not have a Next.
    expect(hasNextPage(100, 100, 200)).toBe(false);
    expect(hasNextPage(0, 100, 200)).toBe(true);
    expect(hasNextPage(100, 100, 201)).toBe(true);
    expect(hasNextPage(0, 0, 0)).toBe(false);
    // A partial page is always the last one.
    expect(hasNextPage(0, 37, 37)).toBe(false);
  });

  it("labels the pager, grouping large totals", () => {
    // The separator is locale-dependent, so compare against the formatter.
    expect(paginationLabel(0, 100, 12345)).toBe(
      `Showing rows 1–100 of ${formatNumber(12345)}`,
    );
    expect(paginationLabel(100, 100, 12345)).toBe(
      `Showing rows 101–200 of ${formatNumber(12345)}`,
    );
    // Without a total it degrades to the plain range.
    expect(paginationLabel(50, 50, 0)).toBe("Showing rows 51–100");
    expect(paginationLabel(0, 0, 0)).toBe("Showing rows 0–0");
  });

  it("flips direction only when the active column is re-clicked", () => {
    expect(nextSort(DEFAULT_USAGE_SORT, "cost")).toEqual({
      field: "cost",
      descending: true,
    });
    expect(nextSort({ field: "cost", descending: true }, "cost")).toEqual({
      field: "cost",
      descending: false,
    });
    expect(nextSort({ field: "cost", descending: false }, "cost")).toEqual({
      field: "cost",
      descending: true,
    });
    // A different column always resets to descending.
    expect(nextSort({ field: "cost", descending: false }, "model")).toEqual({
      field: "model",
      descending: true,
    });
  });

  it("flags latency only at twice the window average", () => {
    expect(isSlowLatency(200, 100)).toBe(true);
    expect(isSlowLatency(199, 100)).toBe(false);
    // No average means nothing to compare against, so nothing is flagged.
    expect(isSlowLatency(5000, 0)).toBe(false);
  });

  it("picks hour buckets for the short ranges", () => {
    expect(bucketForRange("1h")).toBe("hour");
    expect(bucketForRange("24h")).toBe("hour");
    expect(bucketForRange("7d")).toBe("day");
    expect(bucketForRange("1M")).toBe("day");
    expect(bucketForRange("all")).toBe("day");
  });
});
