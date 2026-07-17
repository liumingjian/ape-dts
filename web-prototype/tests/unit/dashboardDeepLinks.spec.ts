/* eslint-disable vue/one-component-per-file -- test file with stub components */
import { mount, type VueWrapper } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { defineComponent, h } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMemoryHistory, createRouter, type Router } from 'vue-router';
import { nextTick } from 'vue';
import Dashboard from '@/views/dashboard/Dashboard.vue';
import type { ActivityEvent, DashboardSummary, DashboardTopTask } from '@/types/domain';

const summary: DashboardSummary = {
  kpi: {
    running: { total: 1, delta: 0 },
    todayAlerts: { total: 1, delta: 0 },
    totalRps: { value: 0, delta: 0 },
    avgLatencyMs: { value: 0, delta: 0 },
  },
  kpiSparks: { running: [], todayAlerts: [], totalRps: [], avgLatencyMs: [] },
  statusDist: [{ status: 'running', count: 1 }],
  engineDist: [{ engine: 'mysql', count: 1 }],
  alertTrend: [],
  rpsSeries: [],
  latencySeries: [],
  topRunningTasks: [],
  recentTasks: [],
  topAlerts: [],
  recentEvents: [],
  licenseWarnCount: 0,
};

vi.mock('@/composables/useDashboardData', () => ({
  useDashboardData: () => ({
    summary,
    loading: false,
    load: vi.fn(),
  }),
}));

vi.mock('@/composables/useEcharts', () => ({
  AXIS_BASE: {},
  BRAND_PALETTE: ['#0F766E'],
}));

const i18n = createI18n({
  legacy: false,
  locale: 'en-US',
  fallbackLocale: 'en-US',
  messages: { 'en-US': {}, 'zh-CN': {} },
  missingWarn: false,
  fallbackWarn: false,
});

const Stub = defineComponent({ name: 'Stub', render: () => h('div') });

const KpiCardStub = defineComponent({
  name: 'KpiCard',
  props: { label: String },
  emits: ['click'],
  template: '<button class="kpi-card" @click="$emit(\'click\')">{{ label }}</button>',
});

const RunningTaskGridStub = defineComponent({
  name: 'RunningTaskGrid',
  emits: ['select', 'more'],
  setup(_, { emit }) {
    function selectCdc() {
      const task: DashboardTopTask = {
        id: 'cdc-task',
        name: 'CDC task',
        category: 'cdc',
        status: 'running',
        sourceEngine: 'mysql',
        targetEngine: 'postgres',
        rps: 1,
        latencyMs: 1,
        spark: [],
      };
      emit('select', task);
    }
    return { selectCdc };
  },
  template: '<button class="running-task" @click="selectCdc">running</button><button class="running-more" @click="$emit(\'more\')">more</button>',
});

const ActivityTimelineStub = defineComponent({
  name: 'ActivityTimeline',
  emits: ['select'],
  setup(_, { emit }) {
    function selectCdcTask() {
      const event: ActivityEvent = {
        id: 'evt-cdc',
        type: 'task.started',
        category: 'task',
        tone: 'success',
        title: 'CDC task',
        taskId: 'cdc-task',
        taskCategory: 'cdc',
        taskSyncMode: 'cdc',
        occurredAt: '2026-01-01T00:00:00.000Z',
      };
      emit('select', event);
    }
    function selectCheckAlert() {
      const event: ActivityEvent = {
        id: 'evt-check-alert',
        type: 'alert.triggered',
        category: 'alert',
        tone: 'danger',
        title: 'Check alert',
        taskId: 'check-task',
        taskCategory: 'check',
        alertLevel: 'critical',
        occurredAt: '2026-01-01T00:00:00.000Z',
      };
      emit('select', event);
    }
    return { selectCdcTask, selectCheckAlert };
  },
  template: '<button class="activity-cdc" @click="selectCdcTask">cdc activity</button><button class="activity-alert" @click="selectCheckAlert">alert activity</button>',
});

async function flushRouter() {
  await nextTick();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function mountDashboard(): Promise<{ router: Router; wrapper: VueWrapper }> {
  const router = createRouter({
    history: createMemoryHistory('/'),
    routes: [
      { path: '/', component: Stub },
      { path: '/dashboard', component: Dashboard },
      { path: '/tasks/migration', component: Stub },
      { path: '/tasks/migration/:id', component: Stub },
      { path: '/tasks/check/:id', component: Stub },
      { path: '/alerts/current', component: Stub },
    ],
  });
  await router.push('/dashboard');
  await router.isReady();

  const wrapper = mount(Dashboard, {
    global: {
      plugins: [router, i18n],
      stubs: {
        PageHeader: { template: '<section><slot name="actions" /></section>' },
        KpiCard: KpiCardStub,
        ChartCard: Stub,
        EmptyState: Stub,
        ActivityTimeline: ActivityTimelineStub,
        RunningTaskGrid: RunningTaskGridStub,
        VChart: Stub,
        ElSegmented: Stub,
        ElButton: { template: '<button @click="$emit(\'click\')"><slot /></button>' },
        IconActivity: Stub,
        IconAlertTriangle: Stub,
        IconBolt: Stub,
        IconClock: Stub,
        IconRefresh: Stub,
      },
    },
  });

  return { router, wrapper };
}

describe('Dashboard production deep-links', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('links running KPI and more action to the canonical migration list', async () => {
    const { router, wrapper } = await mountDashboard();

    await wrapper.find('.kpi-card').trigger('click');
    await flushRouter();
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.status).toBe('running');

    await router.push('/dashboard');
    await wrapper.find('.running-more').trigger('click');
    await flushRouter();
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.status).toBe('running');
  });

  it('preserves CDC mode for running task and recent activity links', async () => {
    const { router, wrapper } = await mountDashboard();

    await wrapper.find('.running-task').trigger('click');
    await flushRouter();
    expect(router.currentRoute.value.path).toBe('/tasks/migration/cdc-task');
    expect(router.currentRoute.value.query.mode).toBe('cdc');

    await router.push('/dashboard');
    await wrapper.find('.activity-cdc').trigger('click');
    await flushRouter();
    expect(router.currentRoute.value.path).toBe('/tasks/migration/cdc-task');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
  });

  it('keeps alert activity links on their real task module with alerts tab', async () => {
    const { router, wrapper } = await mountDashboard();

    await wrapper.find('.activity-alert').trigger('click');
    await flushRouter();
    expect(router.currentRoute.value.path).toBe('/tasks/check/check-task');
    expect(router.currentRoute.value.query.tab).toBe('alerts');
  });
});
