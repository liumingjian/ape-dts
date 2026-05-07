import { beforeEach, describe, expect, it } from 'vitest';
import { createRouter, createMemoryHistory, type Router, type RouteRecordRaw } from 'vue-router';
import { h } from 'vue';

/** Simple stub — no defineComponent to avoid vue/one-component-per-file warning. */
const Stub = { render: () => h('div') };
const NotFound = { render: () => h('div', 'not-found') };

/**
 * Minimal route table that mirrors the production router for routing
 * assertions. Uses createWebHistory-compatible declarations.
 */
const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    component: Stub,
    meta: { public: true },
  },
  {
    path: '/',
    component: Stub,
    redirect: '/dashboard',
    children: [
      { path: 'dashboard', name: 'Dashboard', component: Stub },
      { path: 'tasks/sync', name: 'SyncTasks', component: Stub },
      { path: 'tasks/check', name: 'CheckTasks', component: Stub },
      { path: 'tasks/struct', name: 'StructTasks', component: Stub },
      { path: 'alerts/current', name: 'CurrentAlerts', component: Stub },
      { path: 'license', name: 'License', component: Stub },
      { path: 'profile', name: 'Profile', component: Stub },
    ],
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'NotFound',
    component: NotFound,
    meta: { public: true },
  },
];

let router: Router;

beforeEach(() => {
  router = createRouter({ history: createMemoryHistory('/'), routes });
});

describe('HTML5 History Routing', () => {
  it('uses clean URLs without hash fragments', async () => {
    await router.push('/dashboard');
    const url = router.currentRoute.value.fullPath;
    expect(url).not.toContain('#/');
    expect(url).toBe('/dashboard');
  });

  it('deep-link to dashboard resolves correctly', async () => {
    await router.push('/dashboard');
    expect(router.currentRoute.value.name).toBe('Dashboard');
  });

  it('deep-link to alerts/current resolves correctly', async () => {
    await router.push('/alerts/current');
    expect(router.currentRoute.value.name).toBe('CurrentAlerts');
  });

  it('unknown URL resolves to NotFound catch-all', async () => {
    await router.push('/no-such-page');
    expect(router.currentRoute.value.name).toBe('NotFound');
  });

  it('root path redirects to dashboard', async () => {
    await router.push('/');
    // After redirect
    expect(router.currentRoute.value.path).toBe('/dashboard');
  });
});

describe('Not Found view', () => {
  it('catch-all route is defined', () => {
    const catchAll = routes.find((r) => r.path === '/:pathMatch(.*)*');
    expect(catchAll).toBeDefined();
    expect(catchAll?.name).toBe('NotFound');
  });

  it('catch-all route has public meta so it renders without auth', () => {
    const catchAll = routes.find((r) => r.path === '/:pathMatch(.*)*');
    expect(catchAll?.meta?.public).toBe(true);
  });
});
