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
  /** Per-key requests-per-minute override; null inherits the default. */
  rate_limit_per_minute: number | null;
  /** Monthly spend cap in USD; null is uncapped. */
  monthly_budget_usd: number | null;
  budget_mode: BudgetMode;
  created_at: string;
  last_used_at: string | null;
}

export type BudgetMode = 'off' | 'warn' | 'block';

export interface ApiKeyInput {
  name: string;
  enabled?: boolean;
  rate_limit_per_minute?: number | null;
  monthly_budget_usd?: number | null;
  budget_mode?: BudgetMode;
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
  attempt: number;
  /** `ok` or `error`. */
  status: string;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  cost_usd: number;
  latency_ms: number;
}

export interface UsageSummary {
  requests: number;
  ok_requests: number;
  error_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  cost_usd: number;
  avg_latency_ms: number;
}
