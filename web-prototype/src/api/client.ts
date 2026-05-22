import router from '@/router';
import { useAuthStore } from '@/stores/auth';

export interface ApiError {
  status: number;
  code?: string;
  message: string;
  details?: unknown;
  /** Server-side correlation ID surfaced via `details.requestId` when available. */
  requestId?: string;
}

/** Read `details.requestId` if it's a string (server correlation token). */
function readRequestId(details: unknown): string | undefined {
  if (details && typeof details === 'object' && 'requestId' in details) {
    const id = (details as { requestId?: unknown }).requestId;
    if (typeof id === 'string' && id.length > 0) return id;
  }
  return undefined;
}

export interface RequestOptions extends RequestInit {
  timeoutMs?: number;
  parseAs?: 'json' | 'text';
}

/** Read the API base URL at call-time so tests can override import.meta.env. */
function getBase(): string {
  return (import.meta as { env?: Record<string, string> }).env?.VITE_API_BASE ?? '/api';
}
const DEFAULT_TIMEOUT_MS = 30_000;
const UNSAFE_METHODS = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

function readCookie(name: string): string | null {
  if (typeof document === 'undefined' || !document.cookie) return null;
  const target = `${name}=`;
  for (const part of document.cookie.split(';')) {
    const trimmed = part.trim();
    if (trimmed.startsWith(target)) return decodeURIComponent(trimmed.slice(target.length));
  }
  return null;
}

function attachCsrf(method: string, headers: Record<string, string>): Record<string, string> {
  if (!UNSAFE_METHODS.has(method.toUpperCase())) return headers;
  const token = readCookie('XSRF-TOKEN');
  if (token) headers['X-XSRF-TOKEN'] = token;
  return headers;
}

async function handleUnauthorized(): Promise<void> {
  try {
    useAuthStore().logout();
  } catch {
    /* pinia not active yet — ignore */
  }
  if (typeof window === 'undefined') return;
  if (router.currentRoute.value.path === '/login') return;
  const redirect = window.location.pathname + window.location.search;
  await router.push({ path: '/login', query: { redirect } });
}

export async function apiFetch<T>(path: string, init: RequestOptions = {}): Promise<T> {
  const method = (init.method ?? 'GET').toUpperCase();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...((init.headers as Record<string, string> | undefined) ?? {}),
  };
  attachCsrf(method, headers);

  const controller = new AbortController();
  const timeoutMs = init.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const signal = init.signal ?? controller.signal;

  let res: Response;
  try {
    res = await fetch(`${getBase()}${path}`, { ...init, method, headers, signal });
  } finally {
    clearTimeout(timer);
  }

  // Parse body for all non-2xx responses (including 401) so we get the error code
  const text = await res.text();
  const parseAs = init.parseAs ?? 'json';
  const data = parseAs === 'text' ? text : text ? JSON.parse(text) : null;

  if (res.status === 401) {
    // If already on /login, don't redirect — let the caller surface the error
    const onLoginPage = typeof window !== 'undefined' && router.currentRoute.value.path === '/login';
    if (!onLoginPage) {
      await handleUnauthorized();
    }
    const err: ApiError = {
      status: 401,
      code: data?.code,
      message: data?.message ?? 'unauthorized',
      details: data?.details,
      requestId: readRequestId(data?.details),
    };
    console.warn('[api]', method, path, '→', res.status, err.code, err.message, err.requestId ?? '');
    throw err;
  }

  if (!res.ok) {
    const err: ApiError = {
      status: res.status,
      code: data?.code,
      message: data?.message ?? res.statusText,
      details: data?.details,
      requestId: readRequestId(data?.details),
    };
    console.error('[api]', method, path, '→', res.status, err.code, err.message, err.requestId ?? '', err.details);
    throw err;
  }
  return data as T;
}

export const api = {
  get: <T>(path: string, init?: RequestOptions) => apiFetch<T>(path, init),
  post: <T>(path: string, body?: unknown, init?: RequestOptions) =>
    apiFetch<T>(path, {
      ...init,
      method: 'POST',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    }),
  put: <T>(path: string, body?: unknown, init?: RequestOptions) =>
    apiFetch<T>(path, {
      ...init,
      method: 'PUT',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    }),
  patch: <T>(path: string, body?: unknown, init?: RequestOptions) =>
    apiFetch<T>(path, {
      ...init,
      method: 'PATCH',
      body: body !== undefined ? JSON.stringify(body) : undefined,
    }),
  del: <T>(path: string, init?: RequestOptions) => apiFetch<T>(path, { ...init, method: 'DELETE' }),
};
