import { describe, expect, it } from 'vitest';
import { setupServer } from 'msw/node';
import { alertMonitorHandlers } from '@/mock/handlers/alertMonitor';

describe('mock alert rules contract', () => {
  it('returns backend metric rule fields consumed by MetricRules', async () => {
    const server = setupServer(...alertMonitorHandlers);
    server.listen({ onUnhandledRequest: 'error' });

    const response = await fetch('/api/alert_rules?page=1&size=1');
    const body = await response.json();
    server.close();

    expect(response.status).toBe(200);
    expect(body.total).toEqual(expect.any(Number));
    expect(body.items).toHaveLength(1);
    expect(body.items[0]).toEqual(expect.objectContaining({
      metricName: expect.any(String),
      dwellSecs: expect.any(Number),
      enabled: expect.any(Boolean),
      severity: expect.any(String),
    }));
    expect(body.items[0]).not.toHaveProperty('metric');
    expect(body.items[0]).not.toHaveProperty('periodMin');
    expect(body.items[0]).not.toHaveProperty('status');
  });
});
