import type { TimeSeriesPoint } from '@/types/domain';

export function downsample(points: TimeSeriesPoint[], stepMs: number): TimeSeriesPoint[] {
  if (points.length === 0 || stepMs <= 0) return [];
  const buckets = new Map<number, { sum: number; count: number }>();
  for (const p of points) {
    const bucket = Math.floor(p.ts / stepMs) * stepMs;
    const slot = buckets.get(bucket) ?? { sum: 0, count: 0 };
    slot.sum += p.value;
    slot.count += 1;
    buckets.set(bucket, slot);
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => a - b)
    .map(([ts, slot]) => ({ ts, value: slot.sum / slot.count }));
}

export function queryRange(
  points: TimeSeriesPoint[],
  fromTs: number,
  toTs: number,
): TimeSeriesPoint[] {
  if (toTs < fromTs) return [];
  return points.filter((p) => p.ts >= fromTs && p.ts <= toTs);
}

export function isMonotonic(points: TimeSeriesPoint[]): boolean {
  for (let i = 1; i < points.length; i++) {
    if (points[i].ts < points[i - 1].ts) return false;
  }
  return true;
}
