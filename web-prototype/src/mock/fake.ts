/**
 * Small helpers for seeded, realistic mock data generation.
 * Faker is used but we keep a seed so reloads produce the same shape.
 */
import { faker } from '@faker-js/faker';

faker.seed(20260420);

export const rng = faker;

export function pick<T>(arr: readonly T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

export function pickN<T>(arr: readonly T[], n: number): T[] {
  const copy = [...arr];
  const out: T[] = [];
  for (let i = 0; i < n && copy.length; i++) {
    const idx = Math.floor(Math.random() * copy.length);
    out.push(copy.splice(idx, 1)[0]);
  }
  return out;
}

export function intBetween(a: number, b: number): number {
  return Math.floor(a + Math.random() * (b - a + 1));
}

export function floatBetween(a: number, b: number, digits = 2): number {
  const v = a + Math.random() * (b - a);
  const m = 10 ** digits;
  return Math.round(v * m) / m;
}

export function id(prefix = 'id'): string {
  const r = Math.random().toString(36).slice(2, 10);
  return `${prefix}_${r}${Date.now().toString(36).slice(-4)}`;
}

export function jitter(base: number, pct = 0.15): number {
  return base * (1 + (Math.random() - 0.5) * 2 * pct);
}

/* ----- artificial network simulation ----- */
const BASE_LATENCY_MIN = 300;
const BASE_LATENCY_MAX = 900;
const FAILURE_RATE = 0.05;

export async function simulateNetwork(
  { allowFail = true, latency }: { allowFail?: boolean; latency?: [number, number] } = {},
): Promise<void> {
  const [lo, hi] = latency ?? [BASE_LATENCY_MIN, BASE_LATENCY_MAX];
  await new Promise((r) => setTimeout(r, intBetween(lo, hi)));
  if (allowFail && Math.random() < FAILURE_RATE) {
    throw new Error('mock_network_failure');
  }
}

export function isoMinus(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

export function isoPlus(minutes: number): string {
  return new Date(Date.now() + minutes * 60_000).toISOString();
}
