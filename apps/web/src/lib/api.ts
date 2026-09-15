import { getAdminToken } from '@/lib/adminToken';
import type {
  Alias,
  AliasInput,
  ApiKey,
  ApiKeyInput,
  ComboWithEntries,
  Connection,
  ConnectionInput,
  CreatedApiKey,
  HealthResponse,
  ID,
  InitState,
  UsageRecord,
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

  listAliases: () => request<Alias[]>('GET', '/api/aliases'),
  createAlias: (body: AliasInput) => request<Alias>('POST', '/api/aliases', { body }),
  updateAlias: (id: ID, body: Partial<AliasInput>) =>
    request<Alias>('PATCH', `/api/aliases/${id}`, { body }),
  deleteAlias: (id: ID) => request<void>('DELETE', `/api/aliases/${id}`),

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

  listUsage: (limit: number, offset: number) =>
    request<UsageRecord[]>('GET', '/api/usage', { query: { limit, offset } }),
  usageSummary: (since?: string | null) =>
    request<UsageSummary>('GET', '/api/usage/summary', { query: { since } }),
};
