import { describe, expect, it } from 'vitest';
import { setupServer } from 'msw/node';
import { authHandlers } from '@/mock/handlers/auth';

describe('mock auth contract', () => {
  it('returns the same flat user shape as the real login endpoint', async () => {
    const server = setupServer(...authHandlers);
    server.listen({ onUnhandledRequest: 'error' });

    const response = await fetch('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username: 'admin', password: 'admin123' }),
      headers: { 'Content-Type': 'application/json' },
    });

    const body = await response.json();
    server.close();

    expect(body).toEqual({
      username: 'admin',
      display_name: '超级管理员',
      role: 'admin',
    });
  });
});
