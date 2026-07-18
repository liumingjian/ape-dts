/* eslint-disable vue/one-component-per-file -- test file with stub components */
/**
 * Batched metrics fetch: validates that TaskDetail polls one authoritative
 * GET /tasks/:id/detail aggregate instead of issuing per-metric requests.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia, setActivePinia } from 'pinia';
import { createRouter, createMemoryHistory } from 'vue-router';
import { defineComponent, h, ref } from 'vue';
import type { ApiTask } from '@/types/domain';

/* ---------- API mock ---------- */
const mockGet = vi.fn();
vi.mock('@/api/client', () => ({
  api: {
    get: (...args: unknown[]) => mockGet(...args),
    post: vi.fn().mockResolvedValue({}),
    put: vi.fn().mockResolvedValue({}),
    delete: vi.fn().mockResolvedValue({}),
    patch: vi.fn().mockResolvedValue({}),
  },
}));

/* ---------- SSE composable mock ---------- */
vi.mock('@/composables/useLogStream', () => ({
  useLogStream: vi.fn(() => ({
    lines: ref([]),
    state: ref('disconnected'),
    close: vi.fn(),
  })),
}));

/* ---------- Fixture ---------- */
const SNAPSHOT_FIXTURE: ApiTask = {
  id: 'snap-1',
  taskId: 'snapshot_mysql_mysql_snap1',
  name: 'test-snapshot',
  kind: 'snapshot',
  dbTypeSource: 'mysql',
  dbTypeTarget: 'postgres',
  sourceEndpoint: { url: '' },
  targetEndpoint: { url: '' },
  extractor: null,
  sinker: null,
  filter: null,
  router: null,
  parallelizer: null,
  pipeline: null,
  resumer: null,
  processor: null,
  runtime: null,
  metrics: { extractor_pushed_rps_avg: 100, progress: 87 },
  resourceGroupId: 'rg-1',
  ownerUserId: 'u-1',
  status: 'running',
  createdAt: '2026-05-07T06:00:00.000Z',
  updatedAt: '2026-05-07T06:01:00.000Z',
};

/* ---------- Stubs ---------- */
const Stub = defineComponent({ name: 'Placeholder', render: () => h('div') });

const KpiCardStub = defineComponent({
  name: 'KpiCard',
  props: {
    label: { type: String, default: '' },
    value: { type: Number, default: 0 },
    unit: { type: String, default: '' },
    sentinelText: { type: String, default: undefined },
    badge: { type: String, default: '' },
    iconComp: { type: [Object, Function], default: null },
  },
  template: `<div class="kpi-card-stub"><span class="kpi-label">{{ label }}</span><span class="kpi-value">{{ sentinelText ?? value }}</span><span class="kpi-unit" v-if="unit && !sentinelText">{{ unit }}</span></div>`,
});

const GLOBAL_STUBS = {
  KpiCard: KpiCardStub,
  ChartCard: true,
  StatusBadge: true,
  LevelBadge: true,
  VChart: true,
  ElProgress: true,
  ElTable: true,
  ElTableColumn: true,
  ElTabPane: true,
  ElTabs: true,
  ElDrawer: true,
  ElDialog: true,
  ElPagination: true,
  ElSkeleton: true,
  ElAlert: true,
  ElEmpty: true,
  ElButtonGroup: true,
  ElSelect: true,
  ElOption: true,
  ElInput: true,
  ElInputNumber: true,
  ElSwitch: true,
};

/* ---------- Harness ---------- */
async function buildHarness(taskFixture: ApiTask = SNAPSHOT_FIXTURE) {
  setActivePinia(createPinia());
  const i18n = createI18n({
    legacy: false,
    locale: 'zh-CN',
    fallbackLocale: 'zh-CN',
    messages: {
      'zh-CN': {
        taskDetail: {
          back: '返回',
          kpi: { status: '状态', rps: 'RPS', latency: '延迟', progress: '进度' },
          action: { start: '启动', stop: '停止', pause: '暂停', resume: '恢复', delete: '删除', edit: '编辑' },
          tab: { config: '配置', objects: '对象', logs: '日志', monitor: '监控', alerts: '告警', history: '历史' },
          log: { connected: '已连接', reconnecting: '重连中', disconnected: '已断开', reconnect: '重连', resume: '继续', pause: '暂停', follow: '跟随' },
          editor: { title: '编辑', tip: '提示' },
          alerts: { none: '无告警' },
          history: { viewLogs: '查看日志', archivedLogs: '归档日志' },
        },
        task: { status: { running: '运行中', stopped: '已停止', completed: '已完成', failed: '失败', draft: '草稿', ready: '就绪', paused: '已暂停', stopping: '停止中', creating: '创建中', pending: '待定' } },
        common: { empty: '暂无数据', close: '关闭' },
      },
      'en-US': {},
    },
    missingWarn: false,
    fallbackWarn: false,
  });

  const mockRunId = taskFixture.id + '-run-1';
  const aggregate = {
    task: { ...taskFixture, configuredExtractType: 'snapshot', selectedObjects: [] },
    currentRun: { id: mockRunId, status: 'running', currentPhase: 'snapshot', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
    phases: {
      snapshot: { status: 'running', startedAt: null, completedAt: null },
      transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
      cdc: { status: 'skipped', startedAt: null, completedAt: null },
    },
    metricsSnapshot: { runId: mockRunId, phase: 'snapshot', sampledAt: '2026-05-07T06:01:00.000Z', values: { extractor_rps_avg: 100, progress: 87, pipeline_queue_size: 1024 } },
    progress: { runId: mockRunId, phase: 'snapshot', kind: 'snapshot', percent: 87, copiedRecords: null, estimatedTotalRecords: null, totalIsEstimate: false },
  };
  mockGet.mockImplementation((url: string) => {
    if (url.endsWith(`/tasks/${taskFixture.id}/detail`)) return Promise.resolve(aggregate);
    if (url.includes('/runs') && url.includes('/logs')) return Promise.resolve('');
    if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
    if (url.includes('/alerts')) return Promise.resolve({ items: [] });
    return Promise.resolve({});
  });

  const TaskDetail = (await import('@/views/tasks/TaskDetail.vue')).default;
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/tasks/:category/:id', name: 'TaskDetail', component: TaskDetail },
      { path: '/', component: Stub },
    ],
  });

  const cat = taskFixture.kind === 'cdc' ? 'cdc' : 'snapshot';
  await router.push({ path: `/tasks/${cat}/${taskFixture.id}` });
  await router.isReady();

  return { TaskDetail, router, i18n };
}

/* ---------- Helpers ---------- */
function countPerMetricCalls(): number {
  return mockGet.mock.calls.filter(
    (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).includes('/metrics?metric='),
  ).length;
}

function countDetailCalls(): number {
  return mockGet.mock.calls.filter(
    (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).endsWith('/detail'),
  ).length;
}

describe('Batched metrics fetch', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockGet.mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('loadDetailMetrics does not make per-metric serial fetches', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    expect(countPerMetricCalls()).toBe(0);
  });

  it('loadMonitorMetrics does not make per-metric serial fetches', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Advance one poll cycle — monitor tab metrics should not trigger serial fetches
    vi.advanceTimersByTime(5_000);
    await flushPromises();

    expect(countPerMetricCalls()).toBe(0);
  });

  it('polls exactly one task detail aggregate per cycle', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    mockGet.mockClear();

    vi.advanceTimersByTime(5_000);
    await flushPromises();

    expect(countDetailCalls()).toBe(1);
  });

  it('aggregate with no metrics snapshot does not crash', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/snap-1/detail')) return Promise.resolve({
        task: { ...SNAPSHOT_FIXTURE, configuredExtractType: 'snapshot', selectedObjects: [] },
        currentRun: { id: 'snap-1-run-1', status: 'running', currentPhase: 'snapshot', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
        phases: {
          snapshot: { status: 'running', startedAt: null, completedAt: null },
          transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
          cdc: { status: 'skipped', startedAt: null, completedAt: null },
        },
        metricsSnapshot: null,
        progress: null,
      });
      if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
      if (url.includes('/alerts')) return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });

    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Should render without error
    expect(wrapper.html()).toBeTruthy();
    // Advance a poll cycle with empty response — still no crash
    vi.advanceTimersByTime(5_000);
    await flushPromises();
    expect(wrapper.html()).toBeTruthy();
  });

  it('polling cadence is 5000 ms', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    mockGet.mockClear();

    // First poll cycle
    vi.advanceTimersByTime(5_000);
    await flushPromises();
    expect(countDetailCalls()).toBe(1);

    // Second poll cycle
    vi.advanceTimersByTime(5_000);
    await flushPromises();
    expect(countDetailCalls()).toBe(2);

    // Third poll cycle
    vi.advanceTimersByTime(5_000);
    await flushPromises();
    expect(countDetailCalls()).toBe(3);
  });

  it('polling stops when component unmounts', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    mockGet.mockClear();

    // Unmount (simulates navigation away)
    wrapper.unmount();
    await flushPromises();

    // Advance 20 s — no further calls
    vi.advanceTimersByTime(20_000);
    await flushPromises();

    expect(countDetailCalls()).toBe(0);
  });
});
