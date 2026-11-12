import { formatNumber } from "@/lib/format";
import type { PriceTier } from "@/types/api";

/**
 * Editable form state for one conditional price rule.
 *
 * Numeric fields may be strings or numbers, as Vue normalizes number inputs;
 * a cleared input remains distinct from a typed zero. `timeWindow` toggles the
 * daily UTC condition, whose two bounds are `HH:MM` strings.
 */
export interface TierDraft {
  /** Stable identity for list rendering, so reordering keeps each row's DOM. */
  key: number;
  /** Blank means any context size. */
  contextAbove: string | number;
  timeWindow: boolean;
  start: string;
  end: string;
  input: string | number;
  output: string | number;
  cacheRead: string | number;
  cacheWrite: string | number;
  reasoning: string | number;
}

let nextKey = 0;

/** A blank rule: no conditions, zero rates. */
export function emptyTierDraft(): TierDraft {
  return {
    key: nextKey++,
    contextAbove: "",
    timeWindow: false,
    start: "00:00",
    end: "08:00",
    input: "0",
    output: "0",
    cacheRead: "",
    cacheWrite: "",
    reasoning: "",
  };
}

/** Minutes after UTC midnight as `HH:MM`. */
export function minutesToTime(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return `${String(hours).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
}

/** `HH:MM` as minutes after UTC midnight; null when the value is not a time. */
export function timeToMinutes(value: string): number | null {
  const match = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(value.trim());
  if (!match) return null;
  return Number(match[1]) * 60 + Number(match[2]);
}

/** Stored rule → editable draft. */
export function tierToDraft(tier: PriceTier): TierDraft {
  const start = tier.start_minute_utc;
  const end = tier.end_minute_utc;
  const timed = start !== null && end !== null;
  return {
    key: nextKey++,
    contextAbove:
      tier.context_above === null ? "" : String(tier.context_above),
    timeWindow: timed,
    start: timed ? minutesToTime(start) : "00:00",
    end: timed ? minutesToTime(end) : "08:00",
    input: String(tier.input_per_million_usd),
    output: String(tier.output_per_million_usd),
    cacheRead:
      tier.cache_read_per_million_usd === null
        ? ""
        : String(tier.cache_read_per_million_usd),
    cacheWrite:
      tier.cache_write_per_million_usd === null
        ? ""
        : String(tier.cache_write_per_million_usd),
    reasoning:
      tier.reasoning_per_million_usd === null
        ? ""
        : String(tier.reasoning_per_million_usd),
  };
}

/** Blank clears the field; anything else must be a non-negative number. */
function parseOptional(value: string | number): number | null {
  const trimmed = String(value).trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : Number.NaN;
}

export type TierParseResult = { tier: PriceTier } | { error: string };

/**
 * Editable draft → wire rule, or the first problem in human terms.
 *
 * The router requires both time bounds or neither, and a start different from
 * the end; a start after the end wraps past midnight.
 */
export function draftToTier(draft: TierDraft): TierParseResult {
  const context = parseOptional(draft.contextAbove);
  if (Number.isNaN(context)) {
    return { error: "context threshold must be zero or a positive number" };
  }
  if (context !== null && !Number.isInteger(context)) {
    return { error: "context threshold must be a whole number of tokens" };
  }

  let start: number | null = null;
  let end: number | null = null;
  if (draft.timeWindow) {
    start = timeToMinutes(draft.start);
    end = timeToMinutes(draft.end);
    if (start === null || end === null) {
      return { error: "set both UTC start and end times as HH:MM" };
    }
    if (start === end) {
      return { error: "UTC start and end times must differ" };
    }
  }

  const input = parseOptional(draft.input);
  const output = parseOptional(draft.output);
  if (input === null || Number.isNaN(input)) {
    return { error: "input rate is required and must be zero or positive" };
  }
  if (output === null || Number.isNaN(output)) {
    return { error: "output rate is required and must be zero or positive" };
  }

  const cacheRead = parseOptional(draft.cacheRead);
  const cacheWrite = parseOptional(draft.cacheWrite);
  const reasoning = parseOptional(draft.reasoning);
  if ([cacheRead, cacheWrite, reasoning].some((rate) => Number.isNaN(rate))) {
    return { error: "cache and reasoning rates must be zero or positive" };
  }

  return {
    tier: {
      context_above: context,
      start_minute_utc: start,
      end_minute_utc: end,
      input_per_million_usd: input,
      output_per_million_usd: output,
      cache_read_per_million_usd: cacheRead,
      cache_write_per_million_usd: cacheWrite,
      reasoning_per_million_usd: reasoning,
    },
  };
}

/** One-line condition summary, e.g. `context > 200,000 tokens · 00:00–08:00 UTC`. */
export function describeTier(tier: PriceTier): string {
  const parts: string[] = [];
  if (tier.context_above !== null) {
    parts.push(`context > ${formatNumber(tier.context_above)} tokens`);
  }
  if (tier.start_minute_utc !== null && tier.end_minute_utc !== null) {
    parts.push(
      `${minutesToTime(tier.start_minute_utc)}–${minutesToTime(tier.end_minute_utc)} UTC`,
    );
  }
  return parts.length ? parts.join(" · ") : "Any request";
}
