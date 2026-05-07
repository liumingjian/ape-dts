import { describe, expect, it } from 'vitest';
import { downsample, isMonotonic, queryRange } from '@/utils/metricsSeries';
import { makeSeries } from '../helpers/fixtures';

describe('MetricsSeries (TimeSeriesStore frontend projection)', () => {
  describe('downsample', () => {
    it('returns empty array for empty input', () => {
      expect(downsample([], 1000)).toEqual([]);
    });

    it('returns empty array when stepMs is non-positive', () => {
      const points = makeSeries(0, 5, 1000, (i) => i);
      expect(downsample(points, 0)).toEqual([]);
      expect(downsample(points, -1)).toEqual([]);
    });

    it('averages points within the same bucket', () => {
      const points = [
        { ts: 0, value: 10 },
        { ts: 500, value: 20 },
        { ts: 1000, value: 30 },
        { ts: 1500, value: 40 },
      ];
      const out = downsample(points, 1000);
      expect(out).toEqual([
        { ts: 0, value: 15 },
        { ts: 1000, value: 35 },
      ]);
    });

    it('preserves monotonic timestamps in the output', () => {
      const points = makeSeries(0, 100, 1234, (i) => i);
      const out = downsample(points, 5000);
      expect(isMonotonic(out)).toBe(true);
    });
  });

  describe('queryRange', () => {
    it('filters points to inclusive bounds', () => {
      const points = makeSeries(0, 10, 1000, (i) => i);
      const out = queryRange(points, 3000, 6000);
      expect(out.map((p) => p.ts)).toEqual([3000, 4000, 5000, 6000]);
    });

    it('returns empty when range is inverted', () => {
      const points = makeSeries(0, 5, 1000, (i) => i);
      expect(queryRange(points, 5000, 1000)).toEqual([]);
    });
  });

  describe('isMonotonic', () => {
    it('passes ordered series', () => {
      expect(isMonotonic(makeSeries(0, 5, 100, (i) => i))).toBe(true);
    });

    it('detects out-of-order series', () => {
      expect(
        isMonotonic([
          { ts: 0, value: 1 },
          { ts: 100, value: 2 },
          { ts: 50, value: 3 },
        ]),
      ).toBe(false);
    });
  });

  it.todo('retention boundary: drops points older than retention window');
  it.todo('concurrent ingest preserves monotonic per-series timestamps');

  it('downsample handles sparse buckets without emitting empty slots', () => {
    const points = [
      { ts: 0, value: 10 },
      { ts: 200, value: 30 },
      { ts: 5000, value: 50 },
      { ts: 12_000, value: 70 },
    ];
    const stepMs = 1000;
    const out = downsample(points, stepMs);
    expect(out.map((p) => p.ts)).toEqual([0, 5000, 12_000]);
    expect(out.every((p) => Number.isFinite(p.value))).toBe(true);
  });

  it.todo('queryRange honors metric_name + (task_id, run_id) compound key');
});
