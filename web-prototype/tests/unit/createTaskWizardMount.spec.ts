/**
 * Regression: clicking "create snapshot task" in the sidebar previously crashed
 * the wizard with a Temporal Dead Zone ReferenceError because `watch(() => form.source.engine)`
 * was registered before `const form = reactive(...)` had been initialised. The watcher
 * source is invoked synchronously during setup() to track reactive deps, so the
 * page rendered as a white screen.
 *
 * This test mounts the component in the same conditions and asserts it does not throw.
 */
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia, setActivePinia } from 'pinia';
import { createRouter, createMemoryHistory } from 'vue-router';
import { defineComponent, h } from 'vue';

vi.mock('@/api/client', () => ({
  api: {
    get: vi.fn().mockResolvedValue([]),
    post: vi.fn().mockResolvedValue({}),
    put: vi.fn().mockResolvedValue({}),
    delete: vi.fn().mockResolvedValue({}),
  },
}));

const Stub = defineComponent({ name: 'Placeholder', render: () => h('div') });

async function buildHarness(query: Record<string, string> = {}) {
  setActivePinia(createPinia());
  const i18n = createI18n({
    legacy: false,
    locale: 'zh-CN',
    fallbackLocale: 'zh-CN',
    messages: { 'zh-CN': {}, 'en-US': {} },
    missingWarn: false,
    fallbackWarn: false,
  });

  const Wizard = (await import('@/views/tasks/CreateTaskWizard.vue')).default;

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/tasks/create/:type', name: 'CreateTask', component: Wizard },
      { path: '/', component: Stub },
    ],
  });

  await router.push({ path: '/tasks/create/snapshot', query });
  await router.isReady();

  return { Wizard, router, i18n };
}

describe('CreateTaskWizard mount', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
  });

  it('mounts without throwing for /tasks/create/snapshot (no query)', async () => {
    const { Wizard, router, i18n } = await buildHarness();
    expect(() => {
      mount(Wizard, {
        global: {
          plugins: [router, i18n],
          stubs: {
            ConnectionTestCard: Stub,
          },
        },
      });
    }).not.toThrow();
  });

  it('mounts without throwing for /tasks/create/snapshot?mode=snapshot', async () => {
    const { Wizard, router, i18n } = await buildHarness({ mode: 'snapshot' });
    expect(() => {
      mount(Wizard, {
        global: {
          plugins: [router, i18n],
          stubs: {
            ConnectionTestCard: Stub,
          },
        },
      });
    }).not.toThrow();
  });

  it('mounts without throwing for /tasks/create/cdc', async () => {
    setActivePinia(createPinia());
    const i18n = createI18n({
      legacy: false,
      locale: 'zh-CN',
      fallbackLocale: 'zh-CN',
      messages: { 'zh-CN': {}, 'en-US': {} },
      missingWarn: false,
      fallbackWarn: false,
    });
    const Wizard = (await import('@/views/tasks/CreateTaskWizard.vue')).default;
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/tasks/create/:type', name: 'CreateTask', component: Wizard },
        { path: '/', component: Stub },
      ],
    });
    await router.push({ path: '/tasks/create/cdc' });
    await router.isReady();
    expect(() => {
      mount(Wizard, {
        global: {
          plugins: [router, i18n],
          stubs: {
            ConnectionTestCard: Stub,
          },
        },
      });
    }).not.toThrow();
  });
});
