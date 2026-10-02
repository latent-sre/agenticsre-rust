import createClient from 'openapi-fetch';
import type { paths } from './generated/api';
import type { Problem } from './model';

export class ApiError extends Error {
  constructor(readonly status: number, readonly problem?: Problem) {
    super(problem?.detail ?? 'The workbench did not return a usable response.');
    this.name = 'ApiError';
  }
}

export function createApi(token: string, fetcher: typeof fetch = fetch) {
  return createClient<paths>({
    baseUrl: window.location.origin,
    fetch: (request: Request) => fetcher(request),
    headers: { Authorization: `Bearer ${token}` },
    credentials: 'omit', redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer',
  });
}
export type Api = ReturnType<typeof createApi>;

export function responseData<T>(result: { data?: T; error?: Problem; response: Response }): T {
  if (!result.response.ok || result.data === undefined) throw new ApiError(result.response.status, result.error);
  return result.data;
}
export function requestSignal(signal?: AbortSignal) {
  const timeout = AbortSignal.timeout(10_000);
  return signal ? AbortSignal.any([signal, timeout]) : timeout;
}
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  return 'Cannot reach the workbench. Check that the local server is still running, then try again.';
}
export const admissionUnknown = (error: unknown) => !(error instanceof ApiError) || error.status >= 500;
