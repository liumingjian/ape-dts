import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { apiFetch, type ApiError } from '@/api/client';
import { useAuthStore } from '@/stores/auth';

vi.mock('@/router', () => {
  const push = vi.fn().mockResolvedValue(undefined);
  return {
    default: {
      push,
      currentRoute: { value: { path: '/dashboard' } },
    },
  };
});

import router from '@/router';

function jsonResponse(body: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status: init.status ?? 200,
    headers: { 'Content-Type': 'application/json', ...(init.headers ?? {}) },
  });
}

describe('api/client · apiFetch', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    Object.defineProperty(document, 'cookie', { writable: true, value: '' });
    Object.defineProperty(window, 'location', {
      writable: true,
      value: { pathname: '/tasks/snapshot', search: '?q=1' },
    });
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    (router.push as ReturnType<typeof vi.fn>).mockClear();
  });

  it('returns parsed JSON on 200', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({ ok: 1 }));
    const got = await apiFetch<{ ok: number }>('/ping');
    expect(got).toEqual({ ok: 1 });
  });

  it('attaches X-XSRF-TOKEN header on POST when XSRF-TOKEN cookie is present', async () => {
    Object.defineProperty(document, 'cookie', {
      writable: true,
      value: 'foo=bar; XSRF-TOKEN=abc%20def; baz=qux',
    });
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({}));
    await apiFetch('/tasks', { method: 'POST', body: '{}' });
    const headers = (fetchSpy.mock.calls[0][1] as RequestInit).headers as Record<string, string>;
    expect(headers['X-XSRF-TOKEN']).toBe('abc def');
  });

  it('does not attach CSRF header on GET', async () => {
    Object.defineProperty(document, 'cookie', {
      writable: true,
      value: 'XSRF-TOKEN=should-not-be-sent',
    });
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({}));
    await apiFetch('/ping');
    const headers = (fetchSpy.mock.calls[0][1] as RequestInit).headers as Record<string, string>;
    expect(headers['X-XSRF-TOKEN']).toBeUndefined();
  });

  it('on 401 logs out and redirects to /login with redirect query', async () => {
    const auth = useAuthStore();
    auth.login('admin', 'admin123');
    expect(auth.isAuthenticated).toBe(true);

    vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({}, { status: 401 }));
    await expect(apiFetch('/me')).rejects.toMatchObject({ status: 401 });

    expect(auth.isAuthenticated).toBe(false);
    expect(router.push).toHaveBeenCalledWith({
      path: '/login',
      query: { redirect: '/tasks/snapshot?q=1' },
    });
  });

  it('on non-401 error throws ApiError with status and message from body', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({ code: 'E_TASK_NOT_FOUND', message: 'task missing' }, { status: 404 }),
    );
    const expected: ApiError = {
      status: 404,
      code: 'E_TASK_NOT_FOUND',
      message: 'task missing',
    };
    await expect(apiFetch('/tasks/x')).rejects.toEqual(expected);
  });

  it('surfaces details field from error envelope on 4xx', async () => {
    const detailPayload = { field: 'source.url', error: 'required' };
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({
        code: 'VALIDATION_FAILED',
        message: 'validation failed',
        details: detailPayload,
      }, { status: 422 }),
    );
    try {
      await apiFetch('/tasks');
      expect.unreachable('should have thrown');
    } catch (err) {
      const apiErr = err as ApiError;
      expect(apiErr.status).toBe(422);
      expect(apiErr.code).toBe('VALIDATION_FAILED');
      expect(apiErr.details).toEqual(detailPayload);
    }
  });

  it('prefixes fetch URL with /api base path', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({ ok: 1 }));
    await apiFetch('/test');
    const calledUrl = fetchSpy.mock.calls[0][0] as string;
    expect(calledUrl).toContain('/api/test');
  });
});
