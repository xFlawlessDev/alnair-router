import type { SettingsPatch } from "@/types/api";

/**
 * A group of related settings rendered as one panel. Sections own disjoint sets
 * of patch keys, which is what lets a page show a per-section "unsaved" marker
 * and a "customized" badge without duplicating the diff logic.
 */
export interface SettingsSection {
  id: string;
  label: string;
  description: string;
  /** Override key prefixes that mark this section as customized. */
  prefixes: string[];
  /** Patch keys this section writes. */
  keys: (keyof SettingsPatch)[];
}

/** Tabs of the Settings page. */
export type SettingsSectionId =
  "security" | "routing" | "limits" | "pricing" | "data";

export interface SettingsPageSection extends SettingsSection {
  id: SettingsSectionId;
}

export const SETTINGS_SECTIONS: SettingsPageSection[] = [
  {
    id: "security",
    label: "Security",
    description:
      "Client keys, admin access, CORS and how far the listener reaches.",
    prefixes: ["server."],
    keys: [
      "require_api_key",
      "admin_token",
      "readiness_upstream_checks",
      "public_usage",
      "store_key_secrets",
      "lan_access",
      "cors_origins",
    ],
  },
  {
    id: "routing",
    label: "Routing",
    description: "Fallback behaviour and upstream timeouts.",
    prefixes: ["router."],
    keys: [
      "default_connection",
      "max_attempts",
      "max_retries_per_tier",
      "max_retry_delay_ms",
      "catalog_ttl_ms",
      "connect_timeout_ms",
      "idle_timeout_ms",
    ],
  },
  {
    id: "limits",
    label: "Limits",
    description:
      "Concurrency slots and the default rate limit applied to client keys.",
    prefixes: ["limits.", "rate_limit."],
    keys: [
      "max_concurrent",
      "max_concurrent_per_connection",
      "acquire_timeout_ms",
      "requests_per_minute",
      "burst",
    ],
  },
  {
    id: "pricing",
    label: "Pricing",
    description:
      "Background crawl of a LiteLLM or models.dev catalog; dashboard overrides always win.",
    prefixes: ["pricing."],
    keys: [
      "pricing_sync_enabled",
      "pricing_sync_interval_secs",
      "pricing_source_url",
    ],
  },
  {
    id: "data",
    label: "Data",
    description:
      "Back up every stored record, restore one, and read the deployment values that need a restart.",
    prefixes: [],
    keys: [],
  },
];

/**
 * The token-saving controls live on the Token Saving page beside the savings
 * they produce, but are stored and validated as ordinary settings overrides.
 */
export const TOKEN_SAVER_SECTION: SettingsSection = {
  id: "token-saving",
  label: "Configuration",
  description:
    "Compress tool context before it reaches a provider, then nudge completions toward the smallest useful answer.",
  prefixes: ["token_saver."],
  keys: [
    "slimmer_enabled",
    "slimmer_level",
    "headroom_enabled",
    "headroom_url",
    "headroom_timeout_ms",
    "terse_enabled",
    "caveman_enabled",
    "caveman_level",
    "ponytail_enabled",
    "ponytail_level",
  ],
};

/** Every section descriptor, for tests that assert key coverage. */
export const ALL_SETTINGS_SECTIONS: SettingsSection[] = [
  ...SETTINGS_SECTIONS,
  TOKEN_SAVER_SECTION,
];

/** Patch keys of one section that differ from the saved response. */
export function changedKeys(
  patch: SettingsPatch,
  keys: (keyof SettingsPatch)[],
): (keyof SettingsPatch)[] {
  const present = new Set(Object.keys(patch));
  return keys.filter((key) => present.has(key));
}

/** True when the section has unsaved edits, or any edits at all for `data`. */
export function sectionIsDirty(
  patch: SettingsPatch,
  section: SettingsSection,
): boolean {
  return changedKeys(patch, section.keys).length > 0;
}

/** True when a stored override belongs to the section. */
export function sectionIsCustomized(
  overrides: string[],
  section: SettingsSection,
): boolean {
  return overrides.some((key) =>
    section.prefixes.some((prefix) => key.startsWith(prefix)),
  );
}
