import { describe, expect, it, vi } from 'vitest';
import { admissionUnknown, ApiError, createApi, responseData } from './api';

describe('authenticated generated client', () => {
  it('sends the token only as a bearer header with no cookies, cache, redirects or referrer', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response('{}', { headers: { 'content-type': 'application/json' } }));
    const api = createApi('synthetic-token', fetcher);
    await api.GET('/api/v1/session');
    const request = fetcher.mock.calls[0]?.[0];
    expect(request).toBeInstanceOf(Request);
    if (!(request instanceof Request)) throw new Error('Expected a Request');
    expect(request.headers.get('authorization')).toBe('Bearer synthetic-token');
    expect(request.url).not.toContain('synthetic-token');
    expect(request.credentials).toBe('omit');
    expect(request.redirect).toBe('error');
    expect(request.cache).toBe('no-store');
    expect(request.referrerPolicy).toBe('no-referrer');
  });
  it('does not retry a mutation after an ambiguous transport failure', async () => {
    const fetcher = vi.fn<typeof fetch>().mockRejectedValue(new TypeError('Connection closed'));
    const api = createApi('synthetic-token', fetcher);
    await expect(api.DELETE('/api/v1/runs', { body: {} })).rejects.toThrow('Connection closed');
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
  it('distinguishes uncertain admission from a definite refusal', () => {
    expect(admissionUnknown(new TypeError('Connection closed'))).toBe(true);
    expect(admissionUnknown(new ApiError(503))).toBe(true);
    expect(admissionUnknown(new ApiError(422))).toBe(false);
    expect(admissionUnknown(new ApiError(409))).toBe(false);
    expect(() => responseData({ response: new Response('{}', { status: 403 }) })).toThrow(ApiError);
  });
});
