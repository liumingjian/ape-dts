import { describe, expect, it } from 'vitest';
import { setupServer } from 'msw/node';
import { taskHandlers } from '@/mock/handlers/tasks';
import { miscHandlers } from '@/mock/handlers/misc';

describe('mock task runs contract', () => {
  it('returns the per-run data consumed by task detail polling', async () => {
    const server = setupServer(...taskHandlers, ...miscHandlers);
    server.listen({ onUnhandledRequest: 'error' });

    try {
      const listResponse = await fetch('/api/tasks?category=snapshot&page=1&page_size=1');
      const listBody = await listResponse.json() as { items: Array<{ id: string }>; pageSize: number };
      const taskId = listBody.items[0]?.id;

      expect(listResponse.status).toBe(200);
      expect(taskId).toEqual(expect.any(String));
      expect(listBody.pageSize).toBe(1);

      const runId = `run_${taskId}`;
      const runsResponse = await fetch(`/api/tasks/${taskId}/runs?page=1&size=1`);
      const runsBody = await runsResponse.json() as {
        items: Array<{ id: string; taskId: string; status: string }>;
      };

      expect(runsResponse.status).toBe(200);
      expect(runsBody.items).toHaveLength(1);
      expect(runsBody.items[0]).toEqual(expect.objectContaining({
        id: runId,
        taskId,
        status: expect.any(String),
      }));

      const metricsResponse = await fetch(
        `/api/runs/${runId}/metrics?metric=extractor_rps_avg&from=0&to=${Date.now()}&step=60`,
      );
      const metricsBody = await metricsResponse.json() as {
        metric: string;
        data: Array<{ ts: number; value: number }>;
      };

      expect(metricsResponse.status).toBe(200);
      expect(metricsBody.metric).toBe('extractor_rps_avg');
      expect(metricsBody.data.length).toBeGreaterThan(0);
      expect(metricsBody.data[0]).toEqual(expect.objectContaining({
        ts: expect.any(Number),
        value: expect.any(Number),
      }));

      const latestResponse = await fetch(`/api/runs/${runId}/metrics/latest`);
      const latestBody = await latestResponse.json() as Record<string, number>;

      expect(latestResponse.status).toBe(200);
      expect(latestBody).toEqual(expect.objectContaining({
        extractor_rps_avg: expect.any(Number),
        sinker_rps_avg: expect.any(Number),
        pipeline_queue_size: expect.any(Number),
      }));

      const objectsResponse = await fetch(`/api/runs/${runId}/objects`);
      const objectsBody = await objectsResponse.json() as Array<{
        schema: string;
        table: string;
        state: string;
      }>;

      expect(objectsResponse.status).toBe(200);
      expect(objectsBody.length).toBeGreaterThan(0);
      expect(objectsBody[0]).toEqual(expect.objectContaining({
        schema: expect.any(String),
        table: expect.any(String),
        state: expect.stringMatching(/^(pending|loading|completed)$/),
      }));

      const resourceGroupsResponse = await fetch('/api/resource_groups');
      const resourceGroupsBody = await resourceGroupsResponse.json() as Array<{ id: string; name: string }>;

      expect(resourceGroupsResponse.status).toBe(200);
      expect(resourceGroupsBody.length).toBeGreaterThan(0);
    } finally {
      server.close();
    }
  });
});
