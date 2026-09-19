export type ID = string;

export type AsyncState = "idle" | "loading" | "success" | "error";

/** Provider families the router can dispatch to. */
export type ProviderType =
  "openai-compatible" | "anthropic-native" | "command-code" | "codebuddy-intl";

export const PROVIDER_TYPES: ProviderType[] = [
  "openai-compatible",
  "anthropic-native",
  "command-code",
  "codebuddy-intl",
];

export interface HealthResponse {
  status: string;
  service: string;
  version: string;
}

export interface VersionResponse {
  name: string;
  version: string;
}

/** `GET /api/update` — newest release compared against the running version. */
export interface UpdateStatus {
  name: string;
  /** Version this router was built from. */
  version: string;
  /** Newest release GitHub reports; null when skipped or the lookup failed. */
  latest_version: string | null;
  /** True only when `latest_version` is strictly greater than `version`. */
  update_available: boolean;
  release_url: string | null;
  release_notes: string | null;
  published_at: string | null;
  /** When the lookup was made; null when the answer was never fetched. */
  checked_at: string | null;
  check_enabled: boolean;
  /** Why no `latest_version` came back, when the lookup failed. */
  error: string | null;
}

/** `GET /api/auth/status` — what the sign-in screen needs to know. */
export interface AuthStatus {
  password_set: boolean;
  setup_required: boolean;
  authenticated: boolean;
  admin_token_set: boolean;
  /** True when the admin API needs no credential at all (loopback posture). */
  admin_open: boolean;
}

/** Rotating access/refresh pair returned by login, setup and refresh. */
export interface AuthSession {
  access_token: string;
  refresh_token: string;
  access_expires_at: string;
  refresh_expires_at: string;
}

export interface InitState {
  initialized: boolean;
  connections: number;
  enabled_connections: number;
  require_api_key: boolean;
}

export interface Connection {
  id: ID;
  name: string;
  provider_type: ProviderType;
  base_url: string;
  api_key: string | null;
  /** JSON object serialized as a string. */
  custom_headers: string;
  enabled: number;
  /** Connect/first-byte timeout override in ms; null inherits the default. */
  connect_timeout_ms: number | null;
  /** Stream idle timeout override in ms; null inherits the default. */
  idle_timeout_ms: number | null;
  /** Catalog model id used for price lookups; null uses the upstream id. */
  pricing_model: string | null;
  /** Prompt-cache retention: "none", "short" (5m) or "long" (1h). */
  cache_retention: string;
  /** Credential presentation: "api_key" (provider default) or "bearer". */
  auth_style: string;
  /** Built-in provider preset this connection was created from. */
  provider_id: string | null;
  /** Enabled extra API keys rotating behind this connection. */
  account_count: number;
  created_at: string;
  updated_at: string;
}

export interface ConnectionInput {
  name: string;
  provider_type: ProviderType;
  base_url: string;
  api_key?: string | null;
  custom_headers?: Record<string, string>;
  enabled?: boolean;
  connect_timeout_ms?: number | null;
  idle_timeout_ms?: number | null;
  pricing_model?: string | null;
  /** Opt into Anthropic-style prompt caching for this connection. */
  cache_retention?: string | null;
  /** Send the credential as Authorization: Bearer instead of x-api-key. */
  auth_style?: string | null;
  /** Preset to derive the endpoint and wire family from. */
  provider_id?: string | null;
}

/** Tier a provider preset belongs to; the picker groups by this. */
export type ProviderCategory = "api_key" | "free_tier" | "local";

/** Picker section order; mirrors the order the API returns. */
export const PROVIDER_CATEGORIES: { id: ProviderCategory; label: string }[] = [
  { id: "api_key", label: "API key providers" },
  { id: "free_tier", label: "Free tier providers" },
  { id: "local", label: "Local servers" },
];

/** A built-in upstream template shown by the provider picker. */
export interface ProviderPreset {
  id: string;
  label: string;
  provider_type: ProviderType;
  base_url: string;
  category: ProviderCategory;
  auth: "api_key" | "none";
  default_headers: Record<string, string>;
  api_key_url: string | null;
  docs_url: string | null;
  note: string | null;
  /** Connections currently using this preset. */
  configured: number;
}

export interface ProviderPresetResponse {
  object: "list";
  data: ProviderPreset[];
}

/** An extra API key attached to a connection; the secret is write-only. */
export interface ConnectionAccount {
  id: ID;
  connection_id: ID;
  label: string;
  enabled: number;
  created_at: string;
  updated_at: string;
}

export interface ConnectionAccountInput {
  label: string;
  api_key?: string | null;
  enabled?: boolean;
}

/** Rate shape returned by the pricing match tool. */
export interface PriceQuote {
  input_per_million_usd: number;
  output_per_million_usd: number;
  cache_read_per_million_usd?: number | null;
  cache_write_per_million_usd?: number | null;
  reasoning_per_million_usd?: number | null;
}

export interface PriceMatch {
  model: string;
  matched: string | null;
  source?: "override" | "sync";
  price?: PriceQuote;
}

export interface Alias {
  id: ID;
  prefix: string;
  connection_id: ID;
  model_override: string | null;
  enabled: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface AliasInput {
  prefix: string;
  connection_id: ID;
  model_override?: string | null;
  enabled?: boolean;
  sort_order?: number;
}

export interface UpstreamModel {
  id: string;
  name: string;
}

export interface UpstreamModelsResponse {
  connection_id: ID;
  provider_type: ProviderType;
  base_url: string;
  latency_ms: number;
  models: UpstreamModel[];
  enumerable: boolean;
}

export interface ConnectionTestResult {
  ok: boolean;
  models_count: number;
  enumerable: boolean;
  latency_ms: number;
  message: string;
}

export interface AliasTestResult {
  ok: boolean;
  message: string;
  model: string | null;
  model_available: boolean | null;
  models_count: number;
  enumerable: boolean;
  latency_ms: number;
}

export interface AliasChatTestInput {
  prompt?: string;
  model?: string;
}

export interface AliasChatTestResult {
  ok: boolean;
  message: string;
  content?: string;
  finish_reason?: string | null;
  model?: string;
  source?: string;
  provider_type?: string;
  attempts?: number;
  prompt_tokens?: number;
  completion_tokens?: number;
  cost_usd?: number;
  latency_ms?: number;
}

export interface Combo {
  id: ID;
  name: string;
  description: string | null;
  enabled: number;
  created_at: string;
  updated_at: string;
}

export interface ComboEntry {
  id: ID;
  combo_id: ID;
  model_ref: string;
  position: number;
  enabled: number;
}

export interface ComboWithEntries {
  combo: Combo;
  entries: ComboEntry[];
}

export interface ComboInput {
  name: string;
  description?: string | null;
  enabled?: boolean;
  entries?: string[];
}

export interface ApiKey {
  id: ID;
  name: string;
  prefix: string;
  enabled: number;
  /** Per-key requests-per-minute override; null inherits the plan or default. */
  rate_limit_per_minute: number | null;
  /** Daily spend cap in USD; null inherits the plan or is uncapped. */
  daily_budget_usd: number | null;
  /** Weekly spend cap in USD; null inherits the plan or is uncapped. */
  weekly_budget_usd: number | null;
  /** Monthly spend cap in USD; null inherits the plan or is uncapped. */
  monthly_budget_usd: number | null;
  /** Lifetime spend cap in USD with no reset; null inherits the plan or is uncapped. */
  lifetime_budget_usd: number | null;
  /** Daily token cap (prompt + completion); null inherits the plan or is uncapped. */
  daily_token_limit: number | null;
  /** Weekly token cap (prompt + completion); null inherits the plan or is uncapped. */
  weekly_token_limit: number | null;
  /** Monthly token cap (prompt + completion); null inherits the plan or is uncapped. */
  monthly_token_limit: number | null;
  /** Lifetime token cap (prompt + completion); null inherits the plan or is uncapped. */
  lifetime_token_limit: number | null;
  budget_mode: BudgetMode;
  /** Plan whose rules fill the fields this key leaves empty. */
  plan_id: ID | null;
  /** Model allowlist patterns on the key itself; null inherits the plan. */
  allowed_models: string[] | null;
  created_at: string;
  last_used_at: string | null;
  /** When the key stops authenticating; null never expires. */
  expires_at: string | null;
}

export type BudgetMode = "off" | "warn" | "block";

export interface ApiKeyInput {
  name: string;
  enabled?: boolean;
  rate_limit_per_minute?: number | null;
  daily_budget_usd?: number | null;
  weekly_budget_usd?: number | null;
  monthly_budget_usd?: number | null;
  lifetime_budget_usd?: number | null;
  daily_token_limit?: number | null;
  weekly_token_limit?: number | null;
  monthly_token_limit?: number | null;
  lifetime_token_limit?: number | null;
  budget_mode?: BudgetMode;
  plan_id?: string | null;
  allowed_models?: string[] | null;
  expires_at?: string | null;
}

/** Reusable rule set: model allowlist plus limits, applied to any key. */
export interface KeyPlan {
  id: ID;
  name: string;
  description: string;
  /** Model patterns; empty allows any model. `*` and `prefix/*` wildcards work. */
  allowed_models: string[];
  rate_limit_per_minute: number | null;
  /** Daily spend cap in USD; null is uncapped. */
  daily_budget_usd: number | null;
  /** Weekly spend cap in USD; null is uncapped. */
  weekly_budget_usd: number | null;
  /** Monthly spend cap in USD; null is uncapped. */
  monthly_budget_usd: number | null;
  /** Lifetime spend cap in USD with no reset; null is uncapped. */
  lifetime_budget_usd: number | null;
  /** Daily token cap (prompt + completion); null is uncapped. */
  daily_token_limit: number | null;
  /** Weekly token cap (prompt + completion); null is uncapped. */
  weekly_token_limit: number | null;
  /** Monthly token cap (prompt + completion); null is uncapped. */
  monthly_token_limit: number | null;
  /** Lifetime token cap (prompt + completion); null is uncapped. */
  lifetime_token_limit: number | null;
  budget_mode: BudgetMode;
  created_at: string;
  updated_at: string;
  /** When the plan stops applying; attached keys are rejected after this. */
  expires_at: string | null;
}

export interface KeyPlanInput {
  name: string;
  description?: string;
  allowed_models?: string[];
  rate_limit_per_minute?: number | null;
  daily_budget_usd?: number | null;
  weekly_budget_usd?: number | null;
  monthly_budget_usd?: number | null;
  lifetime_budget_usd?: number | null;
  daily_token_limit?: number | null;
  weekly_token_limit?: number | null;
  monthly_token_limit?: number | null;
  lifetime_token_limit?: number | null;
  budget_mode?: BudgetMode;
  expires_at?: string | null;
}

export interface CreatedApiKey {
  key: ApiKey;
  /** Plaintext secret, shown at creation and on rotate. */
  secret: string;
}

/** `GET /api/keys/{id}/secret` — decrypted secret, admin only. */
export interface RevealedApiKey {
  secret: string;
}

export interface UsageRecord {
  id: ID;
  created_at: string;
  api_key_id: ID | null;
  requested_model: string;
  resolved_provider: string | null;
  resolved_model: string | null;
  /** Connection that served the attempt, snapshotted at write time. */
  connection_name: string | null;
  attempt: number;
  /** `ok` or `error`. */
  status: string;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  /** Reasoning tokens, included in `completion_tokens`. */
  reasoning_tokens: number;
  cost_usd: number;
  /** Input share of `cost_usd`. */
  cost_input_usd: number;
  /** Output share, excluding the reasoning premium. */
  cost_output_usd: number;
  /** Reasoning premium over the output rate. */
  cost_reasoning_usd: number;
  latency_ms: number;
}

export interface UsageSummary {
  requests: number;
  ok_requests: number;
  error_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  reasoning_tokens: number;
  cost_usd: number;
  cost_input_usd: number;
  cost_output_usd: number;
  cost_reasoning_usd: number;
  avg_latency_ms: number;
  /** Token-saver rollup for the same window. */
  savings: SavingsSummary;
}

/**
 * What the token-saving pipeline saved over a window.
 *
 * RTK and Headroom figures are measured; the directive figures are estimates
 * derived from the completion, so the UI must label them as such rather than
 * presenting all five as measurements.
 */
export interface SavingsSummary {
  /** Requests where at least one saver contributed. */
  requests: number;
  saved_rtk_tokens: number;
  saved_headroom_tokens: number;
  saved_terse_tokens: number;
  saved_caveman_tokens: number;
  saved_ponytail_tokens: number;
  saved_cost_usd: number;
}

/** `POST /api/token-saver/headroom/test` result. */
export interface HeadroomTestResult {
  ok: boolean;
  /** Operator-readable outcome, including why a probe failed. */
  message: string;
  latency_ms: number;
  version?: string;
}

/** A message in the shape the router forwards upstream. */
export interface PlaygroundMessage {
  role: string;
  content: string;
  tool_call_id?: string;
}

/** Final saving figures for a single run, without the window aggregate. */
export interface SavingsTotals {
  saved_rtk_tokens: number;
  saved_headroom_tokens: number;
  saved_terse_tokens: number;
  saved_caveman_tokens: number;
  saved_ponytail_tokens: number;
  saved_cost_usd: number;
}

/** One pipeline step, as reported by the playground. */
export interface PlaygroundStep {
  /** Stable id: `rtk`, `headroom`, `terse`, `caveman` or `ponytail`. */
  saver: string;
  label: string;
  /** `input` shrinks the prompt; `output` asks for a shorter answer. */
  side: "input" | "output";
  /** False when the step declined; the token counts are then unchanged. */
  applied: boolean;
  tokens_before: number;
  tokens_after: number;
  /**
   * Signed, measured from the rewritten prompt: negative for a saving, positive
   * for a directive, which adds the instruction text it injects.
   */
  delta: number;
  detail: string;
}

/** `POST /api/token-saver/playground` body. */
export interface PlaygroundRequest {
  messages: PlaygroundMessage[];
  model?: string;
  /** Enables the output-side estimate; omitted means no guess is made. */
  assumed_completion_tokens?: number;
  /** Per-run settings; omitted runs the live configuration. */
  overrides?: Partial<TokenSaverSettings>;
}

/** One message in a playground chat, in the OpenAI wire shape. */
export interface PlaygroundChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

/** `POST /api/playground/chat` body. */
export interface PlaygroundChatRequest {
  /** Alias prefix, combo name, or `prefix/model` reference to resolve. */
  model: string;
  messages: PlaygroundChatMessage[];
  temperature?: number;
  max_tokens?: number;
  /** Per-run settings; omitted runs the live configuration. */
  overrides?: Partial<TokenSaverSettings>;
}

/** First frame of the playground stream: the tier that answered. */
export interface PlaygroundChatRouter {
  model: string;
  /** `alias:<prefix>`, `combo:<name>` or `direct`, as the resolver reports it. */
  source: string;
  provider_type: ProviderType;
  /** Tiers attempted before one answered. */
  attempts: number;
}

/** Terminal frame of the playground stream: tokens, cost and pipeline savings. */
export interface PlaygroundChatUsage {
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: number;
  savings: SavingsTotals;
}

/** Frames `POST /api/playground/chat` emits, discriminated by `type`. */
export type PlaygroundChatFrame =
  | ({ type: "router" } & PlaygroundChatRouter)
  | { type: "delta"; text: string }
  | { type: "thinking"; text: string }
  | {
      type: "tool_call";
      id: string;
      name: string;
      arguments: string;
    }
  | ({ type: "usage" } & PlaygroundChatUsage)
  | { type: "error"; message: string };

/** `POST /api/token-saver/playground` result. */
export interface PlaygroundResult {
  model: string;
  /** False when every saver is off, so nothing would change. */
  active: boolean;
  tokens_before: number;
  tokens_after: number;
  prompt_tokens_saved: number;
  steps: PlaygroundStep[];
  /** Messages exactly as submitted. */
  before: PlaygroundMessage[];
  /** Messages exactly as they would be sent upstream. */
  after: PlaygroundMessage[];
  notes: string[];
  /** Null when no completion length was supplied, so nothing could be priced. */
  totals: SavingsTotals | null;
}

/** One stored model price (USD per million tokens). */
export interface ModelPrice {
  model: string;
  input_per_million_usd: number;
  output_per_million_usd: number;
  cache_read_per_million_usd: number | null;
  cache_write_per_million_usd: number | null;
  reasoning_per_million_usd: number | null;
  /** `override` (set here) or `sync` (crawled catalog). */
  source: "override" | "sync";
  updated_at: string;
}

export interface ModelPriceInput {
  model: string;
  input_per_million_usd: number;
  output_per_million_usd: number;
  cache_read_per_million_usd?: number | null;
  cache_write_per_million_usd?: number | null;
  reasoning_per_million_usd?: number | null;
}

export interface PricingSyncStatus {
  source: string;
  synced_at: string;
  model_count: number;
}

/** Distinct values seen in usage rows, for the filter pickers. */
export interface UsageFacets {
  /** Requested model references, most used first. */
  models: string[];
  providers: string[];
  /** Connection names that served requests, most used first. */
  connections: string[];
}

/** Spend for one API key, split by budget window. */
export interface KeySpend {
  api_key_id: ID;
  /** Spend since the start of the current UTC day. */
  daily_usd: number;
  /** Spend since the start of the current UTC week. */
  weekly_usd: number;
  /** Spend since the start of the current UTC month. */
  monthly_usd: number;
  /** All recorded spend; the lifetime window. */
  lifetime_usd: number;
  /** Prompt + completion tokens since the start of the current UTC day. */
  daily_tokens: number;
  /** Prompt + completion tokens since the start of the current UTC week. */
  weekly_tokens: number;
  /** Prompt + completion tokens since the start of the current UTC month. */
  monthly_tokens: number;
  /** All recorded prompt + completion tokens; the lifetime window. */
  lifetime_tokens: number;
}

/** Usage filters; `model` is a case-insensitive substring match. */
export interface UsageFilter {
  since?: string | null;
  /** Inclusive upper bound, used by the month selector. */
  until?: string | null;
  api_key_id?: string | null;
  model?: string | null;
  provider?: string | null;
  connection?: string | null;
}

/** Column the usage table is sorted by; mirrors the router's whitelist. */
export type UsageSortField =
  "time" | "model" | "connection" | "status" | "tokens" | "cost" | "latency";

/** Sort state for the usage table. */
export interface UsageSort {
  field: UsageSortField;
  descending: boolean;
}

export interface ActiveAttempt {
  connection_id: ID;
  connection: string;
  model: string;
  source: string;
  tier: number;
  elapsed_ms: number;
}

export interface ConnectionActivity {
  id: ID;
  name: string;
  in_flight: number;
  requests: number;
  failures: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_latency_ms: number;
  last_activity?: string;
}

export interface ActivityEvent {
  seq: number;
  at: string;
  level: "info" | "warn" | "error";
  kind: string;
  connection?: string;
  model?: string;
  message: string;
  latency_ms?: number;
  status?: number;
}

export interface ActivitySnapshot {
  uptime_ms: number;
  active: ActiveAttempt[];
  connections: ConnectionActivity[];
  events: ActivityEvent[];
}

/** Dashboard-managed settings: effective values from `GET /api/settings`. */
export interface ServerSettings {
  require_api_key: boolean;
  readiness_upstream_checks: boolean;
  public_usage: boolean;
  /** Keep a reversible copy of minted keys so the dashboard can reveal them. */
  store_key_secrets: boolean;
  /** Bind every interface so the LAN can reach the router. */
  lan_access: boolean;
  /** Browser CORS allowlist; empty means no CORS headers; `*` allows any. */
  cors_origins: string[];
  /** Whether an admin token is configured; the value itself is write-only. */
  admin_token_set: boolean;
}

export interface RouterSettings {
  default_connection: string | null;
  max_attempts: number;
  max_retries_per_tier: number;
  max_retry_delay_ms: number;
  catalog_ttl_ms: number;
  connect_timeout_ms: number;
  idle_timeout_ms: number;
}

export interface LimitsSettings {
  max_concurrent: number;
  max_concurrent_per_connection: number;
  acquire_timeout_ms: number;
}

export interface RateLimitSettings {
  requests_per_minute: number;
  burst: number;
}

export interface PricingSettings {
  sync_enabled: boolean;
  sync_interval_secs: number;
  source_url: string;
}

export interface TokenSaverSettings {
  slimmer_enabled: boolean;
  slimmer_level: "minimal" | "aggressive" | string;
  headroom_enabled: boolean;
  headroom_url: string;
  headroom_timeout_ms: number;
  terse_enabled: boolean;
  caveman_enabled: boolean;
  caveman_level: string;
  ponytail_enabled: boolean;
  ponytail_level: "lite" | "full" | "ultra" | string;
}

/** Values that only change by editing `config.toml` and restarting. */
export interface DeploymentInfo {
  host: string;
  port: number;
  binds_loopback: boolean;
  serve_dashboard: boolean;
  tray: boolean;
  allow_unauthenticated_admin: boolean;
  database_url: string;
  secrets_key_set: boolean;
}

export interface SettingsResponse {
  server: ServerSettings;
  router: RouterSettings;
  limits: LimitsSettings;
  rate_limit: RateLimitSettings;
  pricing: PricingSettings;
  token_saver: TokenSaverSettings;
  /** Dotted keys the dashboard has customized, e.g. `server.require_api_key`. */
  overrides: string[];
  deployment: DeploymentInfo;
}

/** Row count for one table replaced by a restore. */
export interface RestoreTableCount {
  table: string;
  rows: number;
}

/** Result of importing a database backup. */
export interface RestoreSummary {
  tables: RestoreTableCount[];
  total_rows: number;
}

/** One routable model reference in the admin model catalog. */
export interface ModelCatalogEntry {
  /** Model id to pass as `model` on /v1 calls. */
  id: string;
  kind: "alias" | "combo";
  /** Connection that serves this row. */
  provider: string;
  provider_type: ProviderType;
  /** Built-in preset behind the connection; null for hand-made connections. */
  provider_id: string | null;
  /** Concrete upstream model; null when an alias accepts any model. */
  upstream_model: string | null;
  /** 1-based combo tier; null for aliases. */
  tier: number | null;
  price: PriceQuote | null;
  /** Catalog key that answered, when it differs from the upstream model. */
  price_matched: string | null;
  price_source: "override" | "sync" | null;
}

export interface ModelCatalogResponse {
  object: "list";
  data: ModelCatalogEntry[];
}

/** Per-model rollup; shared by the admin and self-service usage views. */
export interface ModelUsage {
  model: string;
  requests: number;
  error_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: number;
}

export type UsageBucketSize = "hour" | "day";

/** One (bucket, model) cell of the usage trend. */
export interface UsageBucket {
  /** RFC 3339 bucket start for `hour`, `YYYY-MM-DD` for `day`. */
  bucket: string;
  /** Requested model reference, so charts can stack by model. */
  model: string;
  requests: number;
  error_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: number;
}

/** Budget caps for a key, resolved from key + plan. Null means uncapped. */
export interface PublicBudgetCaps {
  daily_budget_usd: number | null;
  weekly_budget_usd: number | null;
  monthly_budget_usd: number | null;
  lifetime_budget_usd: number | null;
  daily_token_limit: number | null;
  weekly_token_limit: number | null;
  monthly_token_limit: number | null;
  lifetime_token_limit: number | null;
}

/** `GET /api/public/usage` response for the connected client key. */
export interface MyUsageResponse {
  key: { name: string; prefix: string };
  since: string | null;
  until: string | null;
  bucket: UsageBucketSize;
  summary: UsageSummary;
  models: ModelUsage[];
  timeseries: UsageBucket[];
  spend: KeySpend;
  budget: PublicBudgetCaps;
}

/** One catalog row the connected key may call. */
export interface PublicCatalogEntry {
  id: string;
  kind: "alias" | "combo";
  /** 1-based combo tier; null for aliases. */
  tier: number | null;
  /** Concrete upstream model; null for aliases that accept any model. */
  upstream_model: string | null;
  price: PriceQuote | null;
}

/** `GET /api/public/models` response for the connected client key. */
export interface PublicCatalogResponse {
  /** The key's allowlist patterns; empty means every model is allowed. */
  allowed_models: string[];
  data: PublicCatalogEntry[];
}

/**
 * `PATCH /api/settings` body: absent fields stay untouched. A `null` or blank
 * `admin_token` forces "no admin token", and a `null` `default_connection`
 * forces "no default connection".
 */
export interface SettingsPatch {
  require_api_key?: boolean;
  admin_token?: string | null;
  readiness_upstream_checks?: boolean;
  public_usage?: boolean;
  store_key_secrets?: boolean;
  lan_access?: boolean;
  cors_origins?: string[];
  default_connection?: string | null;
  max_attempts?: number;
  max_retries_per_tier?: number;
  max_retry_delay_ms?: number;
  catalog_ttl_ms?: number;
  connect_timeout_ms?: number;
  idle_timeout_ms?: number;
  max_concurrent?: number;
  max_concurrent_per_connection?: number;
  acquire_timeout_ms?: number;
  requests_per_minute?: number;
  burst?: number;
  pricing_sync_enabled?: boolean;
  pricing_sync_interval_secs?: number;
  pricing_source_url?: string;
  slimmer_enabled?: boolean;
  slimmer_level?: string;
  headroom_enabled?: boolean;
  headroom_url?: string;
  headroom_timeout_ms?: number;
  terse_enabled?: boolean;
  caveman_enabled?: boolean;
  caveman_level?: string;
  ponytail_enabled?: boolean;
  ponytail_level?: string;
}
