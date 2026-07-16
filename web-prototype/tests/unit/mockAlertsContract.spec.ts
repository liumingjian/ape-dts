import { describe, expect, it } from 'vitest';
import { setupServer } from 'msw/node';
import { alertHandlers } from '@/mock/handlers/alerts';

describe('mock alerts contract', () => {
  it('returns backend alert fields consumed by dashboard polling', async () => {
    const server = setupServer(...alertHandlers);
    server.listen({ onUnhandledRequest: 'error' });

    const response = await fetch('/api/alerts?status=firing&size=1');
    const body = await response.json();
    server.close();

    expect(response.status).toBe(200);
    expect(body.items).toHaveLength(1);
    expect(body.items[0]).toEqual(expect.objectContaining({
      severity: expect.any(String),
      firedAt: expect.any(String),
      status: 'firing',
    }));
    expect(body.items[0]).not.toHaveProperty('level');
    expect(body.items[0]).not.toHaveProperty('firstAt');
  });
});
