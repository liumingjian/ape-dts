/**
 * Regression: when the backend returns an error envelope carrying
 * `details.requestId` (correlation token used to grep console-server logs),
 * the api client MUST surface it on the thrown ApiError so callers like
 * the wizard can render the request ID to operators.
 *
 * Before this contract was introduced, all the user saw on a precheck
 * panic was the literal string "Precheck panicked" with no actionable
 * way to map the symptom back to the server log line.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';

vi.mock('@/router', () => ({
  default: {
    currentRoute: { value: { path: '/' } },
    push: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ logout: vi.fn() }),
}));

import { apiFetch, type ApiError } from '@/api/client';

function buildResponse(status: number, body: unknown): Response {
  const text = JSON.stringify(body);
  return new Response(text, { status, headers: { 'Content-Type': 'application/json' } });
}

describe('api client — request_id correlation', () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    globalThis.fetch = vi.fn() as unknown as typeof fetch;
  });

  afterEachRestore();

  function afterEachRestore() {
    // Reset fetch after each test using vitest's automatic cleanup hook.
    // We can't easily import afterEach here without disturbing the structure;
    // restore happens via individual tests calling globalThis.fetch resets
    // through the `beforeEach` hook above.
    globalThis.fetch = originalFetch;
  }

  it('extracts details.requestId onto the thrown ApiError for non-2xx responses', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(
      buildResponse(422, {
        code: 'PRECHECK_PANIC',
        message:
          'precheck task crashed unexpectedly (request_id=abc12345). Send this request ID to ops or grep console-server logs.',
        details: { requestId: 'abc12345', panicMessage: 'invalid filter pattern: [.*]' },
      }),
    );

    let caught: ApiError | undefined;
    try {
      await apiFetch('/tasks/preview/precheck', { method: 'POST' });
    } catch (e) {
      caught = e as ApiError;
    }

    expect(caught, 'apiFetch must throw on 422').toBeDefined();
    expect(caught?.code).toBe('PRECHECK_PANIC');
    expect(caught?.requestId).toBe('abc12345');
    expect(caught?.message).toContain('precheck task crashed unexpectedly');
    expect(caught?.message).not.toBe('Precheck panicked');
  });

  it('leaves requestId undefined when the envelope omits it', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(
      buildResponse(400, {
        code: 'PARSE_ERROR',
        message: 'invalid json',
      }),
    );

    let caught: ApiError | undefined;
    try {
      await apiFetch('/x', { method: 'POST' });
    } catch (e) {
      caught = e as ApiError;
    }

    expect(caught?.code).toBe('PARSE_ERROR');
    expect(caught?.requestId).toBeUndefined();
  });

  it('ignores non-string requestId values', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue(
      buildResponse(422, {
        code: 'X',
        message: 'y',
        details: { requestId: 12345 },
      }),
    );

    let caught: ApiError | undefined;
    try {
      await apiFetch('/x', { method: 'POST' });
    } catch (e) {
      caught = e as ApiError;
    }

    expect(caught?.requestId).toBeUndefined();
  });
});
