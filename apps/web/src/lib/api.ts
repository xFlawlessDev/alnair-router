import { getAdminToken } from '@/lib/adminToken';
import type {
  ActivitySnapshot,
  Alias,
  AliasChatTestInput,
  AliasChatTestResult,
  AliasInput,
  AliasTestResult,
  ApiKey,
  ApiKeyInput,
  ComboWithEntries,
  Connection,
  ConnectionInput,
  ConnectionTestResult,
  CreatedApiKey,
  HealthResponse,
  ID,
  InitState,
  KeyPlan,
  KeyPlanInput,
  ModelPrice,
  ModelPriceInput,
  PriceMatch,
  PricingSyncStatus,
  UpstreamModelsResponse,
  UsageRecord,
  UsageFacets,
  UsageFilter,
  UsageSummary,
  VersionResponse,
} from '@/types/api';

/** An error response from the router, or a transport failure (status 0). */
export class ApiError extends Error {
  readonly status: number;
  readonly type: string | undefined;

  constructor(message: string, status: number, type?: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.type = type;
  }
}

type QueryValue = string | number | boolean | null | undefined;

interface RequestOptions {
  body?: unknown;
  query?: Record<string, QueryValue>;
  signal?: AbortSignal;
}

export function buildUrl(path: string, query?: Record<string, QueryValue>): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value === undefined || value === null || value === '') continue;
    params.set(key, String(value));
  }
  const search = params.toString();
  return search ? `${path}?${search}` : path;
}

function errorMessage(payload: unknown): string | undefined {
  if (!payload || typeof payload !== 'object') return undefined;
  const error = (payload as Record<string, unknown>).error;
  if (!error || typeof error !== 'object') return undefined;
  const message = (error as Record<string, unknown>).message;
  return typeof message === 'string' ? message : undefined;
}

function errorType(payload: unknown): string | undefined {
  if (!payload || typeof payload !== 'object') return undefined;
  const error = (payload as Record<string, unknown>).error;
  if (!error || typeof error !== 'object') return undefined;
  const type = (error as Record<string, unknown>).type;
  return typeof type === 'string' ? type : undefined;
}

async function request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json' };
  const token = getAdminToken();
  if (token) headers.authorization = `Bearer ${token}`;

  const init: RequestInit = { method, headers, signal: options.signal };
  if (options.body !== undefined) {
    headers['content-type'] = 'application/json';
    init.body = JSON.stringify(options.body);
  }

  let response: Response;
  try {
    response = await fetch(buildUrl(path, options.query), init);
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error;
    throw new ApiError('Cannot reach the router. Is it running?', 0);
  }

  if (response.status === 204) return undefined as T;

  const text = await response.text();
  let payload: unknown;
  try {
    payload = text ? JSON.parse(text) : undefined;
  } catch {
    payload = undefined;
  }

  if (!response.ok) {
    throw new ApiError(
      errorMessage(payload) ?? `Request failed with status ${response.status}`,
      response.status,
      errorType(payload),
    );
  }

  return payload as T;
}

export const api = {
  health: () => request<HealthResponse>('GET', '/api/health'),
  version: () => request<VersionResponse>('GET', '/api/version'),
  initState: () => request<InitState>('GET', '/api/init'),

  listConnections: () => request<Connection[]>('GET', '/api/connections'),
  createConnection: (body: ConnectionInput) =>
    request<Connection>('POST', '/api/connections', { body }),
  updateConnection: (id: ID, body: Partial<ConnectionInput>) =>
    request<Connection>('PATCH', `/api/connections/${id}`, { body }),
  deleteConnection: (id: ID) => request<void>('DELETE', `/api/connections/${id}`),
  listUpstreamModels: (id: ID) =>
    request<UpstreamModelsResponse>('GET', `/api/connections/${id}/models`),
  testConnection: (id: ID) =>
    request<ConnectionTestResult>('POST', `/api/connections/${id}/test`),

  listAliases: () => request<Alias[]>('GET', '/api/aliases'),
  createAlias: (body: AliasInput) => request<Alias>('POST', '/api/aliases', { body }),
  updateAlias: (id: ID, body: Partial<AliasInput>) =>
    request<Alias>('PATCH', `/api/aliases/${id}`, { body }),
  deleteAlias: (id: ID) => request<void>('DELETE', `/api/aliases/${id}`),
  testAlias: (id: ID) => request<AliasTestResult>('POST', `/api/aliases/${id}/test`),
  testAliasChat: (id: ID, body: AliasChatTestInput) =>
    request<AliasChatTestResult>('POST', `/api/aliases/${id}/test-chat`, { body }),

  listCombos: () => request<ComboWithEntries[]>('GET', '/api/combos'),
  createCombo: (body: { name: string; description?: string | null; enabled?: boolean; entries?: string[] }) =>
    request<ComboWithEntries>('POST', '/api/combos', { body }),
  updateCombo: (
    id: ID,
    body: { name?: string; description?: string | null; enabled?: boolean; entries?: string[] },
  ) => request<ComboWithEntries>('PATCH', `/api/combos/${id}`, { body }),
  deleteCombo: (id: ID) => request<void>('DELETE', `/api/combos/${id}`),

  listKeys: () => request<ApiKey[]>('GET', '/api/keys'),
  createKey: (body: ApiKeyInput) => request<CreatedApiKey>('POST', '/api/keys', { body }),
  updateKey: (id: ID, body: Partial<ApiKeyInput>) =>
    request<ApiKey>('PATCH', `/api/keys/${id}`, { body }),
  deleteKey: (id: ID) => request<void>('DELETE', `/api/keys/${id}`),

  listPlans: () => request<KeyPlan[]>('GET', '/api/plans'),
  createPlan: (body: KeyPlanInput) => request<KeyPlan>('POST', '/api/plans', { body }),
  updatePlan: (id: ID, body: Partial<KeyPlanInput>) =>
    request<KeyPlan>('PATCH', `/api/plans/${id}`, { body }),
  deletePlan: (id: ID) => request<void>('DELETE', `/api/plans/${id}`),

  listUsage: (limit: number, offset: number, filter: UsageFilter = {}) =>
    request<UsageRecord[]>('GET', '/api/usage', {
      query: {
        limit,
        offset,
        since: filter.since,
        api_key_id: filter.api_key_id,
        model: filter.model,
        provider: filter.provider,
        connection: filter.connection,
      },
    }),
  usageSummary: (filter: UsageFilter = {}) =>
    request<UsageSummary>('GET', '/api/usage/summary', {
      query: {
        since: filter.since,
        api_key_id: filter.api_key_id,
        model: filter.model,
        provider: filter.provider,
        connection: filter.connection,
      },
    }),
  usageFacets: () => request<UsageFacets>('GET', '/api/usage/facets'),

  listPricing: () => request<ModelPrice[]>('GET', '/api/pricing'),
  upsertPricing: (prices: ModelPriceInput[]) =>
    request<{ updated: number }>('PUT', '/api/pricing', { body: { prices } }),
  deletePricing: (model?: string) =>
    request<{ deleted: number }>('DELETE', '/api/pricing', { query: { model } }),
  pricingSyncStatus: () => request<PricingSyncStatus | null>('GET', '/api/pricing/sync'),
  syncPricing: () => request<PricingSyncStatus>('POST', '/api/pricing/sync'),
  matchPricing: (model: string) =>
    request<PriceMatch>('GET', '/api/pricing/match', { query: { model } }),
  activity: (events = 100) =>
    request<ActivitySnapshot>('GET', '/api/activity', { query: { limit: events } }),
};
