import { describe, expect, it } from 'vitest';
import { readFileSync } from 'fs';
import { resolve } from 'path';

const ROOT = resolve(__dirname, '../../src');

/**
 * Bug 6: SSE status pill diverges after reconnect — TaskDetail manages
 * separate sseState ref instead of reading logStreamHandle.state.
 * Fix: TaskDetail should derive sseState from logStreamHandle.state,
 * not maintain a separate ref.
 *
 * Bug 7: Detail KPI charts go stale — detailMetricSeries loaded only
 * in onMounted; add to 5s polling interval.
 * Fix: loadDetailMetrics should be called on each polling tick.
 */

describe('TaskDetail SSE state source', () => {
  it('sseState should be derived from logStreamHandle.state, not a separate ref', () => {
    const source = readFileSync(resolve(ROOT, 'views/tasks/TaskDetail.vue'), 'utf-8');
    // After fix: sseState should NOT be an independent ref
    // It should be a computed that reads from logStreamHandle.state
    expect(source).not.toMatch(/sseState\s*=\s*ref</);
  });
});

describe('TaskDetail KPI charts polling', () => {
  it('chart data is refreshed in the polling interval via /metrics/latest (not just onMounted)', () => {
    const source = readFileSync(resolve(ROOT, 'views/tasks/TaskDetail.vue'), 'utf-8');
    // After batched-metrics refactor: loadLatestMetrics is the single fetcher
    // that updates rawLatestMetrics + metricsHistory (which feeds detailMetricSeries).
    // It must be called inside the setInterval callback, not only at onMounted.
    expect(source).toMatch(/loadLatestMetrics/);
    const lines = source.split('\n');
    let inPollInterval = false;
    let foundInPoll = false;
    for (const line of lines) {
      if (line.includes('setInterval')) inPollInterval = true;
      if (inPollInterval && line.includes('loadLatestMetrics')) foundInPoll = true;
      if (inPollInterval && line.includes('POLL_INTERVAL')) inPollInterval = false;
    }
    expect(foundInPoll).toBe(true);
  });
});

describe('useDashboardData — Bug 2: RPS+latency chart always empty', () => {
  it('useDashboardData should fetch metric series from /api/runs/:id/metrics', () => {
    const source = readFileSync(resolve(ROOT, 'composables/useDashboardData.ts'), 'utf-8');
    // After fix: useDashboardData should call /runs/:runId/metrics API
    expect(source).toMatch(/\/runs\/.*\/metrics/);
  });

  it('rpsSeries and latencySeries should not be hardcoded empty arrays', () => {
    const source = readFileSync(resolve(ROOT, 'composables/useDashboardData.ts'), 'utf-8');
    // After fix: the computed summary should not hardcode empty arrays
    // for rpsSeries and latencySeries
    expect(source).not.toMatch(/rpsSeries:\s*\[\]/);
    expect(source).not.toMatch(/latencySeries:\s*\[\]\s*,/);
  });
});
