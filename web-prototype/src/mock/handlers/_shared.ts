/**
 * Shared helpers for MSW handlers: latency, pagination, query parsing.
 */
import { HttpResponse, delay } from 'msw';
import { intBetween } from '../fake';

/** Random artificial latency so UI shows real spinners. */
export async function pause(min = 180, max = 520): Promise<void> {
  await delay(intBetween(min, max));
}

export function parsePage(url: URL): { page: number; size: number } {
  const page = Math.max(1, Number(url.searchParams.get('page') ?? 1));
  const size = Math.max(1, Math.min(200, Number(url.searchParams.get('size') ?? 20)));
  return { page, size };
}

export function paginate<T>(items: T[], page: number, size: number) {
  const total = items.length;
  const start = (page - 1) * size;
  return { items: items.slice(start, start + size), total, page, size };
}

export function q(url: URL, key: string): string | null {
  const v = url.searchParams.get(key);
  return v && v.length ? v : null;
}

export function ok(data: unknown) {
  return HttpResponse.json(data as never);
}

export function notFound(message = 'not_found') {
  return HttpResponse.json({ code: 'NOT_FOUND', message }, { status: 404 });
}

export function badRequest(message = 'bad_request', code = 'BAD_REQUEST') {
  return HttpResponse.json({ code, message }, { status: 400 });
}
