import { formatNumber } from "@/lib/format";
import type { UsageSortField } from "@/types/api";

/** Sentinel because Select values cannot be empty strings. */
export const ALL_FILTER = "__all__";

export interface UsageSortState {
  field: UsageSortField;
  descending: boolean;
}

/** The table's opening order: newest attempt first. */
export const DEFAULT_USAGE_SORT: UsageSortState = {
  field: "time",
  descending: true,
};

export interface UsageFilterSelection {
  apiKeyId: string;
  model: string;
  provider: string;
  connection: string;
  range: string;
}

/** True when anything narrows the result set, so an empty table can say why. */
export function hasActiveFilters(selection: UsageFilterSelection): boolean {
  return (
    selection.apiKeyId !== ALL_FILTER ||
    selection.model.trim() !== "" ||
    selection.provider !== ALL_FILTER ||
    selection.connection !== ALL_FILTER ||
    selection.range !== "all"
  );
}

/**
 * A Next page exists only while the rows already fetched fall short of the
 * filtered count. A last page that is exactly full must not offer a Next that
 * lands on an empty page.
 */
export function hasNextPage(
  offset: number,
  shown: number,
  total: number,
): boolean {
  return offset + shown < total;
}

/** Pager label, e.g. `Showing rows 1–100 of 12,345`. */
export function paginationLabel(
  offset: number,
  shown: number,
  total: number,
): string {
  const from = shown > 0 ? offset + 1 : 0;
  const to = offset + shown;
  return total > 0
    ? `Showing rows ${from}–${to} of ${formatNumber(total)}`
    : `Showing rows ${from}–${to}`;
}

/** Re-clicking the active column flips direction; a new column starts descending. */
export function nextSort(
  current: UsageSortState,
  field: UsageSortField,
): UsageSortState {
  return current.field === field
    ? { field, descending: !current.descending }
    : { field, descending: true };
}

/**
 * Latency rows are flagged once they reach twice the window's average, so
 * outliers stand out without flagging ordinary variation.
 */
export function isSlowLatency(latencyMs: number, averageMs: number): boolean {
  return averageMs > 0 && latencyMs >= averageMs * 2;
}

/** Short windows read better as hours; longer ones as days. */
export function bucketForRange(range: string): "hour" | "day" {
  return range === "1h" || range === "24h" ? "hour" : "day";
}
