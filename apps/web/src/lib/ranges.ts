export interface UsageRange {
  value: string;
  label: string;
  /** Window length in milliseconds; 0 means all time. */
  ms: number;
}

export const USAGE_RANGES: UsageRange[] = [
  { value: "all", label: "All time", ms: 0 },
  { value: "1h", label: "Last hour", ms: 3_600_000 },
  { value: "24h", label: "Last 24 hours", ms: 86_400_000 },
  { value: "7d", label: "Last 7 days", ms: 604_800_000 },
  { value: "1M", label: "Last month", ms: 2_592_000_000 },
];

export function rangeToSince(value: string, now = Date.now()): string | null {
  const range = USAGE_RANGES.find((item) => item.value === value);
  return range && range.ms > 0 ? new Date(now - range.ms).toISOString() : null;
}
