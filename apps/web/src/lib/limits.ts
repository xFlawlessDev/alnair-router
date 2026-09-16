import type { BudgetMode } from "@/types/api";

/** The budget windows, in display order. */
export interface BudgetCap {
  field:
    | "daily_budget_usd"
    | "weekly_budget_usd"
    | "monthly_budget_usd"
    | "lifetime_budget_usd";
  label: string;
  suffix: string;
}

export const BUDGET_CAPS: BudgetCap[] = [
  { field: "daily_budget_usd", label: "Daily", suffix: "day" },
  { field: "weekly_budget_usd", label: "Weekly", suffix: "wk" },
  { field: "monthly_budget_usd", label: "Monthly", suffix: "mo" },
  { field: "lifetime_budget_usd", label: "Lifetime", suffix: "total" },
];

export interface BudgetCaps {
  daily_budget_usd: number | null;
  weekly_budget_usd: number | null;
  monthly_budget_usd: number | null;
  lifetime_budget_usd: number | null;
}

/** Token caps per window, mirroring the USD budgets. */
export interface TokenCap {
  field:
    | "daily_token_limit"
    | "weekly_token_limit"
    | "monthly_token_limit"
    | "lifetime_token_limit";
  label: string;
  suffix: string;
}

export const TOKEN_CAPS: TokenCap[] = [
  { field: "daily_token_limit", label: "Daily", suffix: "day" },
  { field: "weekly_token_limit", label: "Weekly", suffix: "wk" },
  { field: "monthly_token_limit", label: "Monthly", suffix: "mo" },
  { field: "lifetime_token_limit", label: "Lifetime", suffix: "total" },
];

export interface TokenCaps {
  daily_token_limit: number | null;
  weekly_token_limit: number | null;
  monthly_token_limit: number | null;
  lifetime_token_limit: number | null;
}

/** Token caps that are set, with their amount, in display order. */
export function configuredTokenCaps(
  caps: TokenCaps,
): { cap: TokenCap; amount: number }[] {
  return TOKEN_CAPS.flatMap((cap) => {
    const amount = caps[cap.field];
    return amount && amount > 0 ? [{ cap, amount }] : [];
  });
}

/** Caps that are set, with their amount, in display order. */
export function configuredCaps(
  caps: BudgetCaps,
): { cap: BudgetCap; amount: number }[] {
  return BUDGET_CAPS.flatMap((cap) => {
    const amount = caps[cap.field];
    return amount && amount > 0 ? [{ cap, amount }] : [];
  });
}

/** Key caps win per window; the plan fills what the key leaves empty. */
export function effectiveCaps(
  key: BudgetCaps,
  plan: BudgetCaps | null | undefined,
): BudgetCaps {
  return {
    daily_budget_usd: key.daily_budget_usd ?? plan?.daily_budget_usd ?? null,
    weekly_budget_usd: key.weekly_budget_usd ?? plan?.weekly_budget_usd ?? null,
    monthly_budget_usd:
      key.monthly_budget_usd ?? plan?.monthly_budget_usd ?? null,
    lifetime_budget_usd:
      key.lifetime_budget_usd ?? plan?.lifetime_budget_usd ?? null,
  };
}

/** Token caps follow the same key-first merge as the USD budgets. */
export function effectiveTokenCaps(
  key: TokenCaps,
  plan: TokenCaps | null | undefined,
): TokenCaps {
  return {
    daily_token_limit: key.daily_token_limit ?? plan?.daily_token_limit ?? null,
    weekly_token_limit:
      key.weekly_token_limit ?? plan?.weekly_token_limit ?? null,
    monthly_token_limit:
      key.monthly_token_limit ?? plan?.monthly_token_limit ?? null,
    lifetime_token_limit:
      key.lifetime_token_limit ?? plan?.lifetime_token_limit ?? null,
  };
}

/** Parses an amount field; blank is `null`, anything non-positive is invalid. */
export function parsePositive(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

/** Parses a token limit; blank is `null`, fractions and non-positives are invalid. */
export function parsePositiveInt(value: string): number | null {
  const parsed = parsePositive(value);
  return parsed !== null && Number.isInteger(parsed) ? parsed : null;
}

/** Converts an RFC 3339 timestamp into a `datetime-local` input value. */
export function toLocalDateTime(value: string | null | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (part: number): string => String(part).padStart(2, "0");
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}`,
  ].join("T");
}

/** Converts a `datetime-local` value into an RFC 3339 UTC string. */
export function fromLocalDateTime(value: string): string | null {
  if (!value.trim()) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

/** True when `expires_at` is in the past. */
export function isExpired(value: string | null | undefined): boolean {
  if (!value) return false;
  const date = new Date(value);
  return !Number.isNaN(date.getTime()) && date.getTime() <= Date.now();
}

export function budgetModeLabel(mode: BudgetMode): string {
  if (mode === "block") return "Blocked when exhausted";
  if (mode === "warn") return "Warn only";
  return "Off";
}
