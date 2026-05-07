import { beforeEach, describe, expect, it } from 'vitest';
import { createRouter, createMemoryHistory, type Router, type RouteRecordRaw } from 'vue-router';
import { defineComponent, h } from 'vue';

const Stub = defineComponent({
  name: 'Stub',
  render: () => h('div'),
});

// Mirror the production router's task-section route table. Kept in lockstep
// with src/router/index.ts. The unified `/tasks/sync` view absorbs both
// snapshot and cdc; legacy /tasks/snapshot|cdc redirect into it preserving
// the mode filter via query.
const routes: RouteRecordRaw[] = [
  {
    path: '/',
    component: Stub,
    children: [
      { path: 'tasks/sync', name: 'SyncTasks', component: Stub },
      { path: 'tasks/check', name: 'CheckTasks', component: Stub },
      { path: 'tasks/struct', name: 'StructTasks', component: Stub },
      { path: 'tasks/snapshot', redirect: (to) => ({ path: '/tasks/sync', query: { ...to.query, mode: 'snapshot' } }) },
      { path: 'tasks/cdc', redirect: (to) => ({ path: '/tasks/sync', query: { ...to.query, mode: 'cdc' } }) },
      { path: 'tasks/replay', redirect: { path: '/tasks/sync' } },
      { path: 'tasks/verify', redirect: { path: '/tasks/check' } },
      {
        path: 'tasks/create/:type(snapshot|cdc|check|struct)',
        name: 'CreateTask',
        component: Stub,
      },
      {
        path: 'tasks/create/:legacy(sync|replay|verify)',
        redirect: (to) => {
          const legacy = String(to.params.legacy);
          const next = legacy === 'verify' ? 'check' : 'snapshot';
          return { path: `/tasks/create/${next}`, query: to.query };
        },
      },
      {
        path: 'tasks/:category(snapshot|cdc|check|struct)/:id',
        name: 'TaskDetail',
        component: Stub,
      },
      {
        path: 'tasks/:legacy(sync|replay|verify)/:id',
        redirect: (to) => {
          const legacy = String(to.params.legacy);
          const next = legacy === 'verify' ? 'check' : 'snapshot';
          return { path: `/tasks/${next}/${to.params.id}`, query: to.query, hash: to.hash };
        },
      },
    ],
  },
];

let router: Router;

beforeEach(() => {
  router = createRouter({ history: createMemoryHistory('/'), routes });
});

describe('Router taxonomy redirects', () => {
  it('redirects legacy /tasks/snapshot → /tasks/sync?mode=snapshot', async () => {
    await router.push('/tasks/snapshot');
    expect(router.currentRoute.value.path).toBe('/tasks/sync');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');
  });

  it('redirects legacy /tasks/cdc → /tasks/sync?mode=cdc', async () => {
    await router.push('/tasks/cdc');
    expect(router.currentRoute.value.path).toBe('/tasks/sync');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
  });

  it('preserves additional query params when redirecting snapshot/cdc → sync', async () => {
    await router.push('/tasks/snapshot?status=running&engine=mysql');
    expect(router.currentRoute.value.path).toBe('/tasks/sync');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');
    expect(router.currentRoute.value.query.status).toBe('running');
    expect(router.currentRoute.value.query.engine).toBe('mysql');
  });

  it('redirects /tasks/replay → /tasks/sync', async () => {
    await router.push('/tasks/replay');
    expect(router.currentRoute.value.path).toBe('/tasks/sync');
  });

  it('redirects /tasks/verify → /tasks/check', async () => {
    await router.push('/tasks/verify');
    expect(router.currentRoute.value.path).toBe('/tasks/check');
  });

  it('redirects legacy detail path /tasks/sync/:id → /tasks/snapshot/:id', async () => {
    await router.push('/tasks/sync/abc-123?tab=alerts');
    expect(router.currentRoute.value.path).toBe('/tasks/snapshot/abc-123');
    expect(router.currentRoute.value.query.tab).toBe('alerts');
  });

  it('redirects legacy detail path /tasks/verify/:id → /tasks/check/:id', async () => {
    await router.push('/tasks/verify/abc-123');
    expect(router.currentRoute.value.path).toBe('/tasks/check/abc-123');
  });

  it('redirects legacy create path /tasks/create/sync → /tasks/create/snapshot', async () => {
    await router.push('/tasks/create/sync');
    expect(router.currentRoute.value.path).toBe('/tasks/create/snapshot');
  });

  it('redirects legacy create path /tasks/create/verify → /tasks/create/check', async () => {
    await router.push('/tasks/create/verify');
    expect(router.currentRoute.value.path).toBe('/tasks/create/check');
  });

  it('resolves canonical path /tasks/sync directly', async () => {
    await router.push('/tasks/sync');
    expect(router.currentRoute.value.path).toBe('/tasks/sync');
    expect(router.currentRoute.value.name).toBe('SyncTasks');
  });

  it('resolves canonical path /tasks/struct directly', async () => {
    await router.push('/tasks/struct');
    expect(router.currentRoute.value.path).toBe('/tasks/struct');
    expect(router.currentRoute.value.name).toBe('StructTasks');
  });
});
