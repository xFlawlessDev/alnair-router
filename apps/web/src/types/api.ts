export type ID = string;

export type AsyncState = 'idle' | 'loading' | 'success' | 'error';

/** Provider families the router can dispatch to. */
export type ProviderType = 'openai-compatible' | 'anthropic-native';

export const PROVIDER_TYPES: ProviderType[] = ['openai-compatible', 'anthropic-native'];

export interface HealthResponse {
  status: string;
  service: string;
  version: string;
}

export interface VersionResponse {
  name: string;
  version: string;
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
  source?: 'override' | 'sync';
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
}

export interface ConnectionTestResult {
  ok: boolean;
  models_count: number;
  latency_ms: number;
  message: string;
}

export interface AliasTestResult {
  ok: boolean;
  message: string;
  model: string | null;
  model_available: boolean | null;
  models_count: number;
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

export type BudgetMode = 'off' | 'warn' | 'block';

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
  /** Plaintext secret, returned exactly once at creation time. */
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
  source: 'override' | 'sync';
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
  api_key_id?: string | null;
  model?: string | null;
  provider?: string | null;
  connection?: string | null;
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
  level: 'info' | 'warn' | 'error';
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

/** Values that only change by editing `config.toml` and restarting. */
export interface DeploymentInfo {
  host: string;
  port: number;
  binds_loopback: boolean;
  serve_dashboard: boolean;
  tray: boolean;
  allow_unauthenticated_admin: boolean;
  cors_origins: string[];
  database_url: string;
  secrets_key_set: boolean;
}

export interface SettingsResponse {
  server: ServerSettings;
  router: RouterSettings;
  limits: LimitsSettings;
  rate_limit: RateLimitSettings;
  pricing: PricingSettings;
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
  kind: 'alias' | 'combo';
  /** Connection that serves this row. */
  provider: string;
  provider_type: ProviderType;
  /** Concrete upstream model; null when an alias accepts any model. */
  upstream_model: string | null;
  /** 1-based combo tier; null for aliases. */
  tier: number | null;
  price: PriceQuote | null;
  /** Catalog key that answered, when it differs from the upstream model. */
  price_matched: string | null;
  price_source: 'override' | 'sync' | null;
}

export interface ModelCatalogResponse {
  object: 'list';
  data: ModelCatalogEntry[];
}

/** Per-model rollup on the self-service usage page. */
export interface PublicModelUsage {
  model: string;
  requests: number;
  error_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cost_usd: number;
}

export type UsageBucketSize = 'hour' | 'day';

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

/** `GET /api/public/usage` response for the connected client key. */
export interface MyUsageResponse {
  key: { name: string; prefix: string };
  since: string | null;
  until: string | null;
  bucket: UsageBucketSize;
  summary: UsageSummary;
  models: PublicModelUsage[];
  timeseries: UsageBucket[];
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
}
