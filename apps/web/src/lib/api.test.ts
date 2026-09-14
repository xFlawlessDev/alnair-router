import { afterEach, describe, expect, it, vi } from 'vitest';

import { setAdminToken } from './adminToken';
import { ApiError, api, buildUrl } from './api';

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
});
