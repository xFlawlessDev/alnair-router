import { afterEach, describe, expect, it, vi } from 'vitest';

import { setAdminToken } from './adminToken';
import { ApiError, api, buildUrl } from './api';
import { setClientKey } from './clientKey';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('buildUrl', () => {
  it('returns the path when no query is given', () => {
    expect(buildUrl('/api/health')).toBe('/api/health');
  });

  it('skips empty values and encodes the rest', () => {
    expect(buildUrl('/api/usage', { limit: 100, offset: 0, since: null, name: '' })).toBe(
      '/api/usage?limit=100&offset=0',
    );
  });
});

describe('api', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    setAdminToken('');
    setClientKey('');
  });

  it('returns the parsed payload on success', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ status: 'ok', service: 'x', version: '1' }));
    vi.stubGlobal('fetch', fetchMock);

    await expect(api.health()).resolves.toMatchObject({ status: 'ok' });
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/health',
      expect.objectContaining({ method: 'GET' }),
    );
  });

  it('throws ApiError with the server message and type', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse({ error: { message: 'no such combo', type: 'not_found_error' } }, 404),
      ),
    );

    const failure = await api.listConnections().catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({ message: 'no such combo', status: 404, type: 'not_found_error' });
  });

  it('reports transport failures as status 0', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('fetch failed')));

    const failure = await api.health().catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({ status: 0 });
  });

  it('resolves 204 responses to undefined', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    await expect(api.deleteAlias('alias-1')).resolves.toBeUndefined();
  });

  it('serializes bodies and attaches the admin token when set', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ id: 'kc-1' }, 201));
    vi.stubGlobal('fetch', fetchMock);
    setAdminToken('secret-token');

    await api.createKey({ name: 'laptop' });

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/keys');
    expect(init.method).toBe('POST');
    expect(init.headers).toMatchObject({
      authorization: 'Bearer secret-token',
      'content-type': 'application/json',
    });
    expect(init.body).toBe(JSON.stringify({ name: 'laptop' }));
  });

  it('creates plans and clears a key plan with an explicit null', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation(() => Promise.resolve(jsonResponse({ id: 'plan-1' }, 201)));
    vi.stubGlobal('fetch', fetchMock);

    await api.createPlan({
      name: 'team-free',
      allowed_models: ['openai/*'],
      rate_limit_per_minute: 60,
    });
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/plans');
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: 'POST' });

    await api.updateKey('key-1', { plan_id: null, allowed_models: null });
    const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit];
    expect(url).toBe('/api/keys/key-1');
    expect(init.method).toBe('PATCH');
    expect(init.body).toBe(JSON.stringify({ plan_id: null, allowed_models: null }));
  });

  it('serializes usage filters and skips blanks', async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(jsonResponse([])));
    vi.stubGlobal('fetch', fetchMock);

    await api.listUsage(100, 0, {
      since: '2026-01-01T00:00:00Z',
      api_key_id: 'key-1',
      model: 'gpt-4o',
      provider: null,
      connection: 'openai-main',
    });
    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/usage?limit=100&offset=0&since=2026-01-01T00%3A00%3A00Z&api_key_id=key-1&model=gpt-4o&connection=openai-main',
    );

    await api.usageSummary({});
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/usage/summary');
  });

  it('upserts pricing overrides and clears a single model', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation(() => Promise.resolve(jsonResponse({ updated: 1 })));
    vi.stubGlobal('fetch', fetchMock);

    await api.upsertPricing([
      {
        model: 'gpt-4o',
        input_per_million_usd: 2.5,
        output_per_million_usd: 10,
        cache_read_per_million_usd: 1.25,
      },
    ]);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/pricing');
    expect(init.method).toBe('PUT');
    expect(init.body).toBe(
      JSON.stringify({
        prices: [
          {
            model: 'gpt-4o',
            input_per_million_usd: 2.5,
            output_per_million_usd: 10,
            cache_read_per_million_usd: 1.25,
          },
        ],
      }),
    );

    await api.deletePricing('gpt-4o');
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/pricing?model=gpt-4o');
  });

  it('downloads a backup blob with the admin token attached', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(new Uint8Array([0x53, 0x51, 0x4c]), {
        status: 200,
        headers: { 'content-type': 'application/octet-stream' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);
    setAdminToken('secret-token');

    const blob = await api.downloadBackup();
    expect(blob.size).toBe(3);

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/backup');
    expect(init.headers).toMatchObject({
      authorization: 'Bearer secret-token',
      accept: 'application/octet-stream',
    });
  });

  it('uploads a backup and surfaces restore errors', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ error: { message: 'bad backup', type: 'invalid_request_error' } }, 400),
    );
    vi.stubGlobal('fetch', fetchMock);

    const file = new File([new Uint8Array([1, 2])], 'backup.sqlite');
    const failure = await api.restoreBackup(file).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({ message: 'bad backup', status: 400 });

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/restore');
    expect(init.method).toBe('POST');
    expect(init.body).toBe(file);
    expect(init.headers).toMatchObject({ 'content-type': 'application/octet-stream' });
  });

  it('sends the client key on the public usage call', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        jsonResponse({ key: { name: 'laptop', prefix: 'sk-router-ab' }, since: null, models: [] }),
      );
    vi.stubGlobal('fetch', fetchMock);
    setClientKey('sk-router-test');

    await api.myUsage('2026-01-01T00:00:00Z', 'hour');

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/public/usage?since=2026-01-01T00%3A00%3A00Z&bucket=hour');
    expect(init.headers).toMatchObject({ authorization: 'Bearer sk-router-test' });
  });
});
