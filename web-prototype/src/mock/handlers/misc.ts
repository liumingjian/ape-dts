/**
 * Miscellaneous ops / system / license handlers — simpler list+patch CRUD.
 */
import { http } from 'msw';
import { pause, ok, notFound, parsePage, paginate, q } from './_shared';
import { db } from '../db';
import type { GlobalParam, User, ResourceGroup } from '@/types/domain';
import { id } from '../fake';

export const miscHandlers = [
  /* ---- Licenses ---- */
  http.get('/api/licenses', async () => {
    await pause();
    return ok({ items: db.licenses, total: db.licenses.length, page: 1, size: 50 });
  }),

  http.post('/api/licenses/activate', async ({ request }) => {
    await pause(700, 1500);
    const body = (await request.json().catch(() => ({}))) as { key?: string };
    if (!body.key || body.key.length < 8) {
      return ok({ ok: false, message: '激活码无效或已过期' });
    }
    return ok({ ok: true, message: 'License 激活成功，有效期已延长 365 天' });
  }),

  /* ---- Resource groups ---- */
  http.get('/api/resource-groups', async () => {
    await pause();
    return ok({ items: db.resourceGroups, total: db.resourceGroups.length, page: 1, size: 50 });
  }),

  http.post('/api/resource-groups', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<ResourceGroup>;
    const rg: ResourceGroup = {
      id: id('rg'),
      name: body.name ?? `rg_${Date.now()}`,
      description: body.description ?? '',
      taskCount: 0,
    };
    db.resourceGroups.push(rg);
    return ok(rg);
  }),

  /* ---- Users ---- */
  http.get('/api/users', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    let items = [...db.users];
    if (key) items = items.filter((u) => u.username.includes(key) || u.displayName.toLowerCase().includes(key));
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  http.post('/api/users', async ({ request }) => {
    await pause();
    const body = (await request.json().catch(() => ({}))) as Partial<User>;
    const u: User = {
      id: id('u'),
      username: body.username ?? `user_${Date.now()}`,
      displayName: body.displayName ?? body.username ?? '',
      role: body.role ?? 'viewer',
      email: body.email ?? '',
      lastLoginAt: new Date().toISOString(),
    };
    db.users.push(u);
    return ok(u);
  }),

  http.patch('/api/users/:id', async ({ params, request }) => {
    await pause();
    const u = db.users.find((x) => x.id === String(params.id));
    if (!u) return notFound();
    Object.assign(u, await request.json().catch(() => ({})));
    return ok(u);
  }),

  http.delete('/api/users/:id', async ({ params }) => {
    await pause();
    const idx = db.users.findIndex((u) => u.id === String(params.id));
    if (idx < 0) return notFound();
    db.users.splice(idx, 1);
    return ok({ ok: true });
  }),

  /* ---- System hosts ---- */
  http.get('/api/hosts', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    let items = [...db.hosts];
    if (key) items = items.filter((h) => h.hostname.toLowerCase().includes(key) || h.ip.includes(key));
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  /* ---- Operate logs ---- */
  http.get('/api/logs/operate', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    let items = [...db.operateLogs].sort((a, b) => b.at.localeCompare(a.at));
    if (key) items = items.filter((l) => l.user.includes(key) || l.action.toLowerCase().includes(key));
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  /* ---- Control logs ---- */
  http.get('/api/logs/control', async ({ request }) => {
    await pause();
    const url = new URL(request.url);
    const key = q(url, 'q')?.toLowerCase();
    let items = [...db.controlLogs].sort((a, b) => b.at.localeCompare(a.at));
    if (key) items = items.filter((l) => l.taskName.toLowerCase().includes(key) || l.operator.includes(key));
    const { page, size } = parsePage(url);
    return ok(paginate(items, page, size));
  }),

  /* ---- Global params ---- */
  http.get('/api/global-params', async () => {
    await pause();
    return ok({ items: db.globalParams, total: db.globalParams.length, page: 1, size: 50 });
  }),

  http.patch('/api/global-params/:key', async ({ params, request }) => {
    await pause();
    const target = String(params.key);
    const p = db.globalParams.find((x) => x.key === target);
    if (!p) return notFound();
    const patch = (await request.json().catch(() => ({}))) as Partial<GlobalParam>;
    Object.assign(p, patch, { updatedAt: new Date().toISOString() });
    return ok(p);
  }),
];
