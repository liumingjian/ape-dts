/**
 * Alert center handlers — active + history alerts, clear action, trend stats.
 */
import { http } from 'msw';
import { pause, ok, notFound, parsePage, paginate, q } from './_shared';
import { db } from '../db';
import type { Alert, AlertLevel, ApiAlert } from '@/types/domain';

function toApiAlert(alert: Alert): ApiAlert {
  return {
    id: alert.id,
    taskId: alert.taskId,
    runId: null,
    ruleId: null,
    metricName: alert.source,
    operator: null,
    severity: alert.level,
    status: alert.status === 'active' ? 'firing' : alert.status,
    silenced: false,
    firedAt: alert.firstAt,
    recoveredAt: alert.status === 'cleared' ? alert.lastAt : null,
    clearedAt: alert.clearedAt ?? null,
    deliveredAt: null,
    clearedBy: null,
    lastError: alert.message,
    createdAt: alert.firstAt,
  };
}

function filterAlerts(list: typeof db.activeAlerts, url: URL) {
  let items = [...list];
  const level = q(url, 'level');
  const source = q(url, 'source');
  const engine = q(url, 'engine');
  const taskId = q(url, 'taskId');
  const key = q(url, 'q')?.toLowerCase();
  if (level) items = items.filter((a) => a.level === level);
  if (source) items = items.filter((a) => a.source === source);
  if (engine) items = items.filter((a) => a.engine === engine);
  if (taskId) items = items.filter((a) => a.taskId === taskId);
  if (key) items = items.filter((a) => a.message.toLowerCase().includes(key) || a.taskName.toLowerCase().includes(key));
  return items;
}

export const alertHandlers = [
  http.get('/api/alerts', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const status = q(url, 'status') ?? 'firing';
    const pool = status === 'cleared' ? db.historyAlerts : db.activeAlerts;
    const items = filterAlerts(pool, url);
    const { page, size } = parsePage(url);
    const pageData = paginate(items.sort((a, b) => b.lastAt.localeCompare(a.lastAt)), page, size);
    return ok({ ...pageData, items: pageData.items.map(toApiAlert) });
  }),

  // Legacy aliases for backward compatibility
  http.get('/api/alerts/active', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const items = filterAlerts(db.activeAlerts, url);
    const { page, size } = parsePage(url);
    const pageData = paginate(items.sort((a, b) => b.lastAt.localeCompare(a.lastAt)), page, size);
    return ok({ ...pageData, items: pageData.items.map(toApiAlert) });
  }),

  http.get('/api/alerts/history', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const items = filterAlerts(db.historyAlerts, url);
    const { page, size } = parsePage(url);
    const pageData = paginate(items.sort((a, b) => b.lastAt.localeCompare(a.lastAt)), page, size);
    return ok({ ...pageData, items: pageData.items.map(toApiAlert) });
  }),

  http.post('/api/alerts/:id/clear', async ({ params }) => {
    await pause();
    const id = String(params.id);
    const idx = db.activeAlerts.findIndex((a) => a.id === id);
    if (idx < 0) return notFound();
    const a = db.activeAlerts[idx];
    a.status = 'cleared';
    a.clearedAt = new Date().toISOString();
    db.activeAlerts.splice(idx, 1);
    db.historyAlerts.unshift(a);
    return ok(a);
  }),

  http.post('/api/alerts/clear_batch', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as { ids?: string[] };
    const ids = body.ids ?? [];
    const now = new Date().toISOString();
    let cleared = 0;
    for (const id of ids) {
      const idx = db.activeAlerts.findIndex((a) => a.id === id);
      if (idx >= 0) {
        const a = db.activeAlerts[idx];
        a.status = 'cleared';
        a.clearedAt = now;
        db.activeAlerts.splice(idx, 1);
        db.historyAlerts.unshift(a);
        cleared++;
      }
    }
    return ok({ cleared });
  }),

  /** Trend stats for dashboard & alert page header bars. */
  http.get('/api/alerts/trend', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const days = Math.max(1, Math.min(90, Number(url.searchParams.get('days') ?? 14)));
    const now = new Date();
    const levels: AlertLevel[] = ['critical', 'major', 'minor', 'info'];
    const trend: { date: string; critical: number; major: number; minor: number; info: number }[] = [];
    for (let d = days - 1; d >= 0; d--) {
      const date = new Date(now.getTime() - d * 86400_000).toISOString().slice(0, 10);
      const row: Record<string, number> = {};
      for (const l of levels) {
        const base = l === 'critical' ? 2 : l === 'major' ? 4 : l === 'minor' ? 7 : 3;
        row[l] = Math.max(0, Math.round(base + (Math.random() - 0.5) * 6));
      }
      trend.push({
        date,
        critical: row.critical,
        major: row.major,
        minor: row.minor,
        info: row.info,
      });
    }
    return ok({ trend });
  }),
];
