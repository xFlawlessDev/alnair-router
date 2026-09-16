import { getAdminToken } from "@/lib/adminToken";
import { getClientKey } from "@/lib/clientKey";
import { getAccessToken, getRefreshToken, setSession } from "@/lib/session";
import type {
  ActivitySnapshot,
  Alias,
  AliasChatTestInput,
  AliasChatTestResult,
  AliasInput,
  AliasTestResult,
  ApiKey,
  ApiKeyInput,
  AuthSession,
  AuthStatus,
  ComboWithEntries,
  Connection,
  ConnectionAccount,
  ConnectionAccountInput,
  ConnectionInput,
  ConnectionTestResult,
  CreatedApiKey,
  HealthResponse,
  ID,
  InitState,
  KeyPlan,
  KeyPlanInput,
  KeySpend,
  ModelCatalogResponse,
  ModelPrice,
  ModelPriceInput,
  ModelUsage,
  MyUsageResponse,
  PriceMatch,
  ProviderPresetResponse,
  PublicCatalogResponse,
  PricingSyncStatus,
  RestoreSummary,
  RevealedApiKey,
  SettingsPatch,
  SettingsResponse,
  UpstreamModelsResponse,
  UsageRecord,
  UsageFacets,
  UsageFilter,
  UsageSummary,
  VersionResponse,
} from "@/types/api";

/** An error response from the router, or a transport failure (status 0). */
export class ApiError extends Error {
  readonly status: number;
  readonly type: string | undefined;

  constructor(message: string, status: number, type?: string) {
    super(message);
    this.name = "ApiError";
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

export function buildUrl(
  path: string,
  query?: Record<string, QueryValue>,
): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value === undefined || value === null || value === "") continue;
    params.set(key, String(value));
  }
  const search = params.toString();
  return search ? `${path}?${search}` : path;
}

function errorMessage(payload: unknown): string | undefined {
  if (!payload || typeof payload !== "object") return undefined;
  const error = (payload as Record<string, unknown>).error;
  if (!error || typeof error !== "object") return undefined;
  const message = (error as Record<string, unknown>).message;
  return typeof message === "string" ? message : undefined;
}

function errorType(payload: unknown): string | undefined {
  if (!payload || typeof payload !== "object") return undefined;
  const error = (payload as Record<string, unknown>).error;
  if (!error || typeof error !== "object") return undefined;
  const type = (error as Record<string, unknown>).type;
  return typeof type === "string" ? type : undefined;
}

/** Bearer for admin calls: the password session first, then a legacy token. */
function authHeader(): string {
  const access = getAccessToken();
  if (access) return `Bearer ${access}`;
  const token = getAdminToken();
  return token ? `Bearer ${token}` : "";
}

async function send(
  method: string,
  path: string,
  options: RequestOptions,
): Promise<Response> {
  const headers: Record<string, string> = { accept: "application/json" };
  const authorization = authHeader();
  if (authorization) headers.authorization = authorization;

  const init: RequestInit = { method, headers, signal: options.signal };
  if (options.body !== undefined) {
    headers["content-type"] = "application/json";
    init.body = JSON.stringify(options.body);
  }

  try {
    return await fetch(buildUrl(path, options.query), init);
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError")
      throw error;
    throw new ApiError("Cannot reach the router. Is it running?", 0);
  }
}

let refreshing: Promise<boolean> | null = null;

/** Rotates the session pair once, deduping parallel 401s. */
async function refreshSession(): Promise<boolean> {
  const refreshToken = getRefreshToken();
  if (!refreshToken) return false;

  refreshing ??= (async () => {
    try {
      const response = await fetch("/api/auth/refresh", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          accept: "application/json",
        },
        body: JSON.stringify({ refresh_token: refreshToken }),
      });
      if (!response.ok) {
        setSession(null);
        return false;
      }
      setSession((await response.json()) as AuthSession);
      return true;
    } catch {
      return false;
    } finally {
      refreshing = null;
    }
  })();

  return refreshing;
}

async function request<T>(
  method: string,
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  let response = await send(method, path, options);

  // An expired access token is refreshed once, then the call is replayed.
  if (
    response.status === 401 &&
    !path.startsWith("/api/auth/") &&
    getRefreshToken()
  ) {
    if (await refreshSession()) {
      response = await send(method, path, options);
    }
  }

  if (response.status === 204) return undefined as T;

  const text = await response.text();
  let payload: unknown;
  let parsed = true;
  try {
    payload = text ? JSON.parse(text) : undefined;
  } catch {
    parsed = false;
    payload = undefined;
  }

  if (!response.ok) {
    throw new ApiError(
      errorMessage(payload) ?? `Request failed with status ${response.status}`,
      response.status,
      errorType(payload),
    );
  }

  // A 200 that is not JSON usually means an older router answered through the
  // SPA fallback (or a proxy returned an error page); fail loudly instead of
  // handing callers an `undefined` payload.
  if (!parsed && text.trim() !== "") {
    throw new ApiError(
      `Unexpected non-JSON response from ${path}. Is the router up to date?`,
      response.status,
    );
  }

  return payload as T;
}

/** Sends a non-JSON request (binary backup download/upload). */
async function rawRequest(
  path: string,
  init: RequestInit = {},
  token = getAdminToken(),
): Promise<Response> {
  const headers: Record<string, string> = {
    ...((init.headers as Record<string, string> | undefined) ?? {}),
  };
  if (token) headers.authorization = `Bearer ${token}`;

  let response: Response;
  try {
    response = await fetch(path, { ...init, headers });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError")
      throw error;
    throw new ApiError("Cannot reach the router. Is it running?", 0);
  }

  if (!response.ok) {
    const text = await response.text();
    let payload: unknown;
    try {
      payload = text ? JSON.parse(text) : undefined;
    } catch {
      payload = undefined;
    }
    throw new ApiError(
      errorMessage(payload) ?? `Request failed with status ${response.status}`,
      response.status,
      errorType(payload),
    );
  }

  return response;
}

export const api = {
  health: () => request<HealthResponse>("GET", "/api/health"),
  version: () => request<VersionResponse>("GET", "/api/version"),
  initState: () => request<InitState>("GET", "/api/init"),

  authStatus: () => request<AuthStatus>("GET", "/api/auth/status"),
  login: (password: string) =>
    request<AuthSession>("POST", "/api/auth/login", { body: { password } }),
  setup: (setupCode: string, password: string) =>
    request<AuthSession>("POST", "/api/auth/setup", {
      body: { setup_code: setupCode, password },
    }),
  logout: () => request<{ signed_out: boolean }>("POST", "/api/auth/logout"),
  changePassword: (currentPassword: string, newPassword: string) =>
    request<AuthSession>("PATCH", "/api/auth/password", {
      body: { current_password: currentPassword, new_password: newPassword },
    }),

  listConnections: () => request<Connection[]>("GET", "/api/connections"),
  listProviders: () => request<ProviderPresetResponse>("GET", "/api/providers"),
  createConnection: (body: ConnectionInput) =>
    request<Connection>("POST", "/api/connections", { body }),
  updateConnection: (id: ID, body: Partial<ConnectionInput>) =>
    request<Connection>("PATCH", `/api/connections/${id}`, { body }),
  deleteConnection: (id: ID) =>
    request<void>("DELETE", `/api/connections/${id}`),
  listUpstreamModels: (id: ID) =>
    request<UpstreamModelsResponse>("GET", `/api/connections/${id}/models`),
  testConnection: (id: ID) =>
    request<ConnectionTestResult>("POST", `/api/connections/${id}/test`),
  listConnectionAccounts: (id: ID) =>
    request<ConnectionAccount[]>("GET", `/api/connections/${id}/accounts`),
  createConnectionAccount: (id: ID, body: ConnectionAccountInput) =>
    request<ConnectionAccount>("POST", `/api/connections/${id}/accounts`, {
      body,
    }),
  updateConnectionAccount: (
    id: ID,
    accountId: ID,
    body: Partial<ConnectionAccountInput>,
  ) =>
    request<ConnectionAccount>(
      "PATCH",
      `/api/connections/${id}/accounts/${accountId}`,
      { body },
    ),
  deleteConnectionAccount: (id: ID, accountId: ID) =>
    request<void>("DELETE", `/api/connections/${id}/accounts/${accountId}`),

  listAliases: () => request<Alias[]>("GET", "/api/aliases"),
  createAlias: (body: AliasInput) =>
    request<Alias>("POST", "/api/aliases", { body }),
  updateAlias: (id: ID, body: Partial<AliasInput>) =>
    request<Alias>("PATCH", `/api/aliases/${id}`, { body }),
  deleteAlias: (id: ID) => request<void>("DELETE", `/api/aliases/${id}`),
  testAlias: (id: ID) =>
    request<AliasTestResult>("POST", `/api/aliases/${id}/test`),
  testAliasChat: (id: ID, body: AliasChatTestInput) =>
    request<AliasChatTestResult>("POST", `/api/aliases/${id}/test-chat`, {
      body,
    }),

  listCombos: () => request<ComboWithEntries[]>("GET", "/api/combos"),
  createCombo: (body: {
    name: string;
    description?: string | null;
    enabled?: boolean;
    entries?: string[];
  }) => request<ComboWithEntries>("POST", "/api/combos", { body }),
  updateCombo: (
    id: ID,
    body: {
      name?: string;
      description?: string | null;
      enabled?: boolean;
      entries?: string[];
    },
  ) => request<ComboWithEntries>("PATCH", `/api/combos/${id}`, { body }),
  deleteCombo: (id: ID) => request<void>("DELETE", `/api/combos/${id}`),

  listKeys: () => request<ApiKey[]>("GET", "/api/keys"),
  createKey: (body: ApiKeyInput) =>
    request<CreatedApiKey>("POST", "/api/keys", { body }),
  updateKey: (id: ID, body: Partial<ApiKeyInput>) =>
    request<ApiKey>("PATCH", `/api/keys/${id}`, { body }),
  deleteKey: (id: ID) => request<void>("DELETE", `/api/keys/${id}`),
  revealKey: (id: ID) =>
    request<RevealedApiKey>("GET", `/api/keys/${id}/secret`),
  rotateKey: (id: ID) =>
    request<CreatedApiKey>("POST", `/api/keys/${id}/rotate`),
  usageByKey: () => request<KeySpend[]>("GET", "/api/usage/keys"),

  listPlans: () => request<KeyPlan[]>("GET", "/api/plans"),
  createPlan: (body: KeyPlanInput) =>
    request<KeyPlan>("POST", "/api/plans", { body }),
  updatePlan: (id: ID, body: Partial<KeyPlanInput>) =>
    request<KeyPlan>("PATCH", `/api/plans/${id}`, { body }),
  deletePlan: (id: ID) => request<void>("DELETE", `/api/plans/${id}`),

  listUsage: (limit: number, offset: number, filter: UsageFilter = {}) =>
    request<UsageRecord[]>("GET", "/api/usage", {
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
    request<UsageSummary>("GET", "/api/usage/summary", {
      query: {
        since: filter.since,
        api_key_id: filter.api_key_id,
        model: filter.model,
        provider: filter.provider,
        connection: filter.connection,
      },
    }),
  usageFacets: () => request<UsageFacets>("GET", "/api/usage/facets"),

  usageModels: (filter: UsageFilter = {}) =>
    request<ModelUsage[]>("GET", "/api/usage/models", {
      query: {
        since: filter.since,
        api_key_id: filter.api_key_id,
        model: filter.model,
        provider: filter.provider,
        connection: filter.connection,
      },
    }),

  listPricing: () => request<ModelPrice[]>("GET", "/api/pricing"),
  modelCatalog: () => request<ModelCatalogResponse>("GET", "/api/models"),
  upsertPricing: (prices: ModelPriceInput[]) =>
    request<{ updated: number }>("PUT", "/api/pricing", { body: { prices } }),
  deletePricing: (model?: string) =>
    request<{ deleted: number }>("DELETE", "/api/pricing", {
      query: { model },
    }),
  pricingSyncStatus: () =>
    request<PricingSyncStatus | null>("GET", "/api/pricing/sync"),
  syncPricing: () => request<PricingSyncStatus>("POST", "/api/pricing/sync"),
  matchPricing: (model: string) =>
    request<PriceMatch>("GET", "/api/pricing/match", { query: { model } }),
  activity: (events = 100) =>
    request<ActivitySnapshot>("GET", "/api/activity", {
      query: { limit: events },
    }),

  settings: () => request<SettingsResponse>("GET", "/api/settings"),
  updateSettings: (body: SettingsPatch) =>
    request<SettingsResponse>("PATCH", "/api/settings", { body }),
  resetSettings: () => request<SettingsResponse>("DELETE", "/api/settings"),

  downloadBackup: async (): Promise<Blob> => {
    const response = await rawRequest("/api/backup", {
      headers: { accept: "application/octet-stream" },
    });
    return response.blob();
  },
  restoreBackup: async (file: File): Promise<RestoreSummary> => {
    const response = await rawRequest("/api/restore", {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: file,
    });
    return (await response.json()) as RestoreSummary;
  },

  myUsage: async (
    since?: string | null,
    bucket?: "hour" | "day",
    until?: string | null,
  ): Promise<MyUsageResponse> => {
    const response = await rawRequest(
      buildUrl("/api/public/usage", { since, until, bucket }),
      {},
      getClientKey(),
    );
    return (await response.json()) as MyUsageResponse;
  },
  myModels: async (): Promise<PublicCatalogResponse> => {
    const response = await rawRequest("/api/public/models", {}, getClientKey());
    return (await response.json()) as PublicCatalogResponse;
  },
};
