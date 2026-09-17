import type { SettingsPatch, SettingsResponse } from "@/types/api";

/**
 * Editable mirror of `GET /api/settings`. Numeric fields accept a string too:
 * the number inputs emit strings while they are being edited and `toInt`
 * normalizes them when the patch is built.
 */
export interface SettingsForm {
  require_api_key: boolean;
  admin_token_enabled: boolean;
  admin_token: string;
  readiness_upstream_checks: boolean;
  public_usage: boolean;
  store_key_secrets: boolean;
  lan_access: boolean;
  cors_origins: string;
  default_connection: string;
  max_attempts: string | number;
  max_retries_per_tier: string | number;
  max_retry_delay_ms: string | number;
  catalog_ttl_ms: string | number;
  connect_timeout_ms: string | number;
  idle_timeout_ms: string | number;
  max_concurrent: string | number;
  max_concurrent_per_connection: string | number;
  acquire_timeout_ms: string | number;
  requests_per_minute: string | number;
  burst: string | number;
  pricing_sync_enabled: boolean;
  pricing_sync_interval_secs: string | number;
  pricing_source_url: string;
  slimmer_enabled: boolean;
  slimmer_level: string;
  headroom_enabled: boolean;
  headroom_url: string;
  headroom_timeout_ms: string | number;
  terse_enabled: boolean;
  caveman_enabled: boolean;
  caveman_level: string;
  ponytail_enabled: boolean;
  ponytail_level: string;
}

/** Starting values, used before the response lands and after a failed load. */
export function blankForm(): SettingsForm {
  return {
    require_api_key: false,
    admin_token_enabled: false,
    admin_token: "",
    readiness_upstream_checks: false,
    public_usage: true,
    store_key_secrets: true,
    lan_access: false,
    cors_origins: "",
    default_connection: "",
    max_attempts: 5,
    max_retries_per_tier: 2,
    max_retry_delay_ms: 30000,
    catalog_ttl_ms: 1000,
    connect_timeout_ms: 10000,
    idle_timeout_ms: 60000,
    max_concurrent: 0,
    max_concurrent_per_connection: 0,
    acquire_timeout_ms: 30000,
    requests_per_minute: 0,
    burst: 0,
    pricing_sync_enabled: false,
    pricing_sync_interval_secs: 86400,
    pricing_source_url: "",
    slimmer_enabled: true,
    slimmer_level: "minimal",
    headroom_enabled: false,
    headroom_url: "http://localhost:8787",
    headroom_timeout_ms: 2000,
    terse_enabled: false,
    caveman_enabled: false,
    caveman_level: "full",
    ponytail_enabled: false,
    ponytail_level: "full",
  };
}

/** Copies an effective settings response into the editable form. */
export function hydrateForm(
  form: SettingsForm,
  response: SettingsResponse,
): void {
  form.require_api_key = response.server.require_api_key;
  form.admin_token_enabled = response.server.admin_token_set;
  form.admin_token = "";
  form.readiness_upstream_checks = response.server.readiness_upstream_checks;
  form.public_usage = response.server.public_usage;
  form.store_key_secrets = response.server.store_key_secrets;
  form.lan_access = response.server.lan_access;
  form.cors_origins = response.server.cors_origins.join("\n");
  form.default_connection = response.router.default_connection ?? "";
  form.max_attempts = response.router.max_attempts;
  form.max_retries_per_tier = response.router.max_retries_per_tier;
  form.max_retry_delay_ms = response.router.max_retry_delay_ms;
  form.catalog_ttl_ms = response.router.catalog_ttl_ms;
  form.connect_timeout_ms = response.router.connect_timeout_ms;
  form.idle_timeout_ms = response.router.idle_timeout_ms;
  form.max_concurrent = response.limits.max_concurrent;
  form.max_concurrent_per_connection =
    response.limits.max_concurrent_per_connection;
  form.acquire_timeout_ms = response.limits.acquire_timeout_ms;
  form.requests_per_minute = response.rate_limit.requests_per_minute;
  form.burst = response.rate_limit.burst;
  form.pricing_sync_enabled = response.pricing.sync_enabled;
  form.pricing_sync_interval_secs = response.pricing.sync_interval_secs;
  form.pricing_source_url = response.pricing.source_url;
  form.slimmer_enabled = response.token_saver.slimmer_enabled;
  form.slimmer_level = response.token_saver.slimmer_level;
  form.headroom_enabled = response.token_saver.headroom_enabled;
  form.headroom_url = response.token_saver.headroom_url;
  form.headroom_timeout_ms = response.token_saver.headroom_timeout_ms;
  form.terse_enabled = response.token_saver.terse_enabled;
  form.caveman_enabled = response.token_saver.caveman_enabled;
  form.caveman_level = response.token_saver.caveman_level;
  form.ponytail_enabled = response.token_saver.ponytail_enabled;
  form.ponytail_level = response.token_saver.ponytail_level;
}

/** Truncates a numeric field, clamping it to `minimum`. */
export function toInt(value: unknown, minimum = 0): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return minimum;
  return Math.max(minimum, Math.trunc(parsed));
}

/** Splits the CORS textarea into a trimmed, deduplicated origin list. */
export function parseOrigins(value: string): string[] {
  const origins: string[] = [];
  for (const entry of value.split(/[\n,]/)) {
    const origin = entry.trim();
    if (origin !== "" && !origins.includes(origin)) origins.push(origin);
  }
  return origins;
}

/**
 * Order carries no meaning for a CORS allowlist, so a reorder must not read as
 * an unsaved change. Both sides are sorted before they are compared.
 */
function sameOrigins(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const sortedLeft = [...left].sort();
  const sortedRight = [...right].sort();
  return sortedLeft.every((origin, index) => origin === sortedRight[index]);
}

/** Sends only the fields that actually changed. */
export function buildPatch(
  form: SettingsForm,
  current: SettingsResponse,
): SettingsPatch {
  const patch: SettingsPatch = {};

  if (form.require_api_key !== current.server.require_api_key) {
    patch.require_api_key = form.require_api_key;
  }
  if (!form.admin_token_enabled) {
    if (current.server.admin_token_set) patch.admin_token = null;
  } else if (form.admin_token.trim()) {
    patch.admin_token = form.admin_token.trim();
  }
  if (
    form.readiness_upstream_checks !== current.server.readiness_upstream_checks
  ) {
    patch.readiness_upstream_checks = form.readiness_upstream_checks;
  }
  if (form.public_usage !== current.server.public_usage) {
    patch.public_usage = form.public_usage;
  }
  if (form.store_key_secrets !== current.server.store_key_secrets) {
    patch.store_key_secrets = form.store_key_secrets;
  }
  if (form.lan_access !== current.server.lan_access) {
    patch.lan_access = form.lan_access;
  }

  const origins = parseOrigins(form.cors_origins);
  if (!sameOrigins(origins, current.server.cors_origins)) {
    patch.cors_origins = origins;
  }

  const connection = form.default_connection.trim();
  if (connection !== (current.router.default_connection ?? "")) {
    patch.default_connection = connection === "" ? null : connection;
  }

  const numbers: Array<[keyof SettingsPatch, number, number]> = [
    ["max_attempts", toInt(form.max_attempts, 1), current.router.max_attempts],
    [
      "max_retries_per_tier",
      toInt(form.max_retries_per_tier),
      current.router.max_retries_per_tier,
    ],
    [
      "max_retry_delay_ms",
      toInt(form.max_retry_delay_ms),
      current.router.max_retry_delay_ms,
    ],
    [
      "catalog_ttl_ms",
      toInt(form.catalog_ttl_ms),
      current.router.catalog_ttl_ms,
    ],
    [
      "connect_timeout_ms",
      toInt(form.connect_timeout_ms),
      current.router.connect_timeout_ms,
    ],
    [
      "idle_timeout_ms",
      toInt(form.idle_timeout_ms),
      current.router.idle_timeout_ms,
    ],
    [
      "max_concurrent",
      toInt(form.max_concurrent),
      current.limits.max_concurrent,
    ],
    [
      "max_concurrent_per_connection",
      toInt(form.max_concurrent_per_connection),
      current.limits.max_concurrent_per_connection,
    ],
    [
      "acquire_timeout_ms",
      toInt(form.acquire_timeout_ms),
      current.limits.acquire_timeout_ms,
    ],
    [
      "requests_per_minute",
      toInt(form.requests_per_minute),
      current.rate_limit.requests_per_minute,
    ],
    ["burst", toInt(form.burst), current.rate_limit.burst],
    [
      "pricing_sync_interval_secs",
      toInt(form.pricing_sync_interval_secs),
      current.pricing.sync_interval_secs,
    ],
  ];
  for (const [key, value, original] of numbers) {
    if (value !== original) {
      (patch as Record<string, number>)[key] = value;
    }
  }

  if (form.pricing_sync_enabled !== current.pricing.sync_enabled) {
    patch.pricing_sync_enabled = form.pricing_sync_enabled;
  }
  if (form.pricing_source_url.trim() !== current.pricing.source_url) {
    patch.pricing_source_url = form.pricing_source_url.trim();
  }

  const saverValues: Array<[keyof SettingsPatch, unknown, unknown]> = [
    [
      "slimmer_enabled",
      form.slimmer_enabled,
      current.token_saver.slimmer_enabled,
    ],
    ["slimmer_level", form.slimmer_level, current.token_saver.slimmer_level],
    [
      "headroom_enabled",
      form.headroom_enabled,
      current.token_saver.headroom_enabled,
    ],
    [
      "headroom_url",
      form.headroom_url.trim().replace(/\/$/, ""),
      current.token_saver.headroom_url,
    ],
    [
      "headroom_timeout_ms",
      toInt(form.headroom_timeout_ms, 1),
      current.token_saver.headroom_timeout_ms,
    ],
    ["terse_enabled", form.terse_enabled, current.token_saver.terse_enabled],
    [
      "caveman_enabled",
      form.caveman_enabled,
      current.token_saver.caveman_enabled,
    ],
    ["caveman_level", form.caveman_level, current.token_saver.caveman_level],
    [
      "ponytail_enabled",
      form.ponytail_enabled,
      current.token_saver.ponytail_enabled,
    ],
    ["ponytail_level", form.ponytail_level, current.token_saver.ponytail_level],
  ];
  for (const [key, value, original] of saverValues) {
    if (value !== original) (patch as Record<string, unknown>)[key] = value;
  }

  return patch;
}

export interface LevelOption {
  value: string;
  label: string;
  hint?: string;
}

export const SLIMMER_LEVELS: LevelOption[] = [
  {
    value: "minimal",
    label: "Minimal",
    hint: "Safe grouping, nothing dropped",
  },
  {
    value: "aggressive",
    label: "Aggressive",
    hint: "Head/tail truncation of long output",
  },
];

/** Terse and Caveman both write a system directive, so they are one choice. */
export const CAVEMAN_LEVELS: LevelOption[] = [
  { value: "lite", label: "Lite" },
  { value: "full", label: "Full" },
  { value: "ultra", label: "Ultra" },
  { value: "wenyan-lite", label: "文言文 Lite" },
  { value: "wenyan-full", label: "文言文 Full" },
  { value: "wenyan-ultra", label: "文言文 Ultra" },
];

export const PONYTAIL_LEVELS: LevelOption[] = [
  { value: "lite", label: "Lite" },
  { value: "full", label: "Full" },
  { value: "ultra", label: "Ultra" },
];

export type OutputDirective = "off" | "terse" | "caveman";

export const OUTPUT_DIRECTIVES: { value: OutputDirective; label: string }[] = [
  { value: "off", label: "Off — no directive" },
  { value: "terse", label: "Terse — shortest useful answer" },
  { value: "caveman", label: "Caveman — strongest terseness" },
];

/** The single active output directive. Caveman wins if both are somehow set. */
export function readOutputDirective(
  form: Pick<SettingsForm, "terse_enabled" | "caveman_enabled">,
): OutputDirective {
  if (form.caveman_enabled) return "caveman";
  if (form.terse_enabled) return "terse";
  return "off";
}

/** Writes the directive back as the mutually exclusive pair the API stores. */
export function applyOutputDirective(
  form: Pick<SettingsForm, "terse_enabled" | "caveman_enabled">,
  directive: OutputDirective,
): void {
  form.terse_enabled = directive === "terse";
  form.caveman_enabled = directive === "caveman";
}
