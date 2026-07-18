/* eslint-disable vue/one-component-per-file -- test file with stub components */
/**
 * Monitor lag gating: validates that the `lag` metric is only included
 * in the Monitor chart series while the aggregate's current Run phase is CDC.
 *
 * Bug: the Monitor chart (and Dashboard latency chart) queried `lag` for
 * ALL task types, causing GET /runs/:id/metrics?metric=lag → 400
 * VALIDATION_FAILED for snapshot tasks.
 *
 * Fix: gate `lag` on the aggregate current Run phase (TaskDetail.vue) and on
 * task kind (useDashboardData.ts).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia, setActivePinia } from 'pinia';
import { createRouter, createMemoryHistory } from 'vue-router';
import { defineComponent, h, ref } from 'vue';
import { readFileSync } from 'fs';
import { resolve } from 'path';
import type { ApiTask } from '@/types/domain';

const ROOT = resolve(__dirname, '../../src');

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

/* ---------- Fixtures ---------- */
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

const CDC_FIXTURE: ApiTask = {
  ...SNAPSHOT_FIXTURE,
  id: 'cdc-1',
  taskId: 'cdc_mysql_mysql_cdc1',
  name: 'test-cdc',
  kind: 'cdc',
  metrics: { extractor_pushed_rps_avg: 100, lag: 5, pipeline_queue_size: 1024 },
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
  template: `<div class="kpi-card-stub"><span class="kpi-label">{{ label }}</span><span class="kpi-value">{{ sentinelText ?? value }}</span></div>`,
});

const ChartCardStub = defineComponent({
  name: 'ChartCard',
  props: {
    title: { type: String, default: '' },
    height: { type: Number, default: 200 },
  },
  template: `<div class="chart-card-stub" :data-title="title"><slot /></div>`,
});

const GLOBAL_STUBS = {
  KpiCard: KpiCardStub,
  ChartCard: ChartCardStub,
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
  const phase = taskFixture.kind === 'cdc' ? 'cdc' : 'snapshot';
  const values = phase === 'cdc'
    ? { extractor_rps_avg: 100, lag: 5, pipeline_queue_size: 1024 }
    : { extractor_rps_avg: 100, progress: 87, pipeline_queue_size: 1024 };
  const aggregate = {
    task: { ...taskFixture, configuredExtractType: phase, selectedObjects: [] },
    currentRun: { id: mockRunId, status: 'running', currentPhase: phase, startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
    phases: {
      snapshot: { status: phase === 'snapshot' ? 'running' : 'skipped', startedAt: null, completedAt: null },
      transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
      cdc: { status: phase === 'cdc' ? 'running' : 'skipped', startedAt: null, completedAt: null },
    },
    metricsSnapshot: { runId: mockRunId, phase, sampledAt: '2026-05-07T06:01:00.000Z', values },
    progress: phase === 'snapshot'
      ? { runId: mockRunId, phase, kind: 'snapshot', percent: 87, copiedRecords: null, estimatedTotalRecords: null, totalIsEstimate: false }
      : null,
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

/* ---------- Source-code assertions ---------- */
describe('Monitor lag gating — source code', () => {
  it('lag in MONITOR_METRIC_NAMES is gated on the current Run phase', () => {
    const source = readFileSync(resolve(ROOT, 'views/tasks/TaskDetail.vue'), 'utf-8');

    // MONITOR_METRIC_NAMES must be a computed (not a plain const) that
    // conditionally includes 'lag' only when syncMode === 'cdc'
    const monIdx = source.indexOf('MONITOR_METRIC_NAMES');
    // Use "const monitorSeries" to find the definition in the script section
    // (avoid matching the template reference which appears earlier)
    const seriesIdx = source.indexOf('const monitorSeries', monIdx);
    const monitorSection = source.substring(monIdx, seriesIdx);

    // Must be a computed, not a plain const array
    expect(monitorSection).toContain('MONITOR_METRIC_NAMES = computed');

    // Must reference currentPhase and lag together
    expect(monitorSection).toContain('currentPhase');
    expect(monitorSection).toContain('lag');
    expect(monitorSection).toContain('cdc');
  });

  it('useDashboardData does not query lag for non-CDC tasks', () => {
    const source = readFileSync(resolve(ROOT, 'composables/useDashboardData.ts'), 'utf-8');

    // The lag fetch must be gated on the task's kind (only CDC tasks)
    // Find the lag fetch block and verify it's inside a kind check
    const lagIndex = source.indexOf('metric=lag');
    if (lagIndex === -1) {
      // If there's no lag fetch at all, that's also acceptable
      return;
    }

    // Look backwards from the lag fetch to find the enclosing if-statement
    const beforeLag = source.substring(Math.max(0, lagIndex - 200), lagIndex);
    const hasKindGate = beforeLag.includes('kind') && beforeLag.includes('cdc');
    expect(hasKindGate).toBe(true);
  });
});

/* ---------- Runtime assertions ---------- */
describe('Monitor lag gating — runtime', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockGet.mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('snapshot task: no per-metric lag call and no lag in Monitor charts', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Advance a poll cycle so metrics are loaded
    vi.advanceTimersByTime(5_000);
    await flushPromises();

    // No call to the per-metric lag endpoint
    const lagPerMetricCalls = mockGet.mock.calls.filter(
      (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).includes('/metrics?metric=lag'),
    );
    expect(lagPerMetricCalls.length).toBe(0);

    // No lag ChartCard rendered for snapshot task
    const chartCards = wrapper.findAll('.chart-card-stub');
    const lagChart = chartCards.find(c => c.attributes('data-title') === 'lag');
    expect(lagChart).toBeUndefined();
  });

  it('CDC task: batched endpoint returns lag and Monitor includes it', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Advance two poll cycles so lag data accumulates
    vi.advanceTimersByTime(5_000);
    await flushPromises();
    vi.advanceTimersByTime(5_000);
    await flushPromises();

    // Verify the aggregate endpoint was polled and supplied lag data
    const detailCalls = mockGet.mock.calls.filter(
      (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).endsWith('/detail'),
    );
    expect(detailCalls.length).toBeGreaterThan(0);

    // The CDC KPI strip should show Lag tile (already tested in taskDetailKpiBranching)
    // Here we verify that the component renders without errors when lag data is present
    expect(wrapper.html()).toBeTruthy();
    expect(wrapper.html()).toContain('Replication lag');
  });
});
