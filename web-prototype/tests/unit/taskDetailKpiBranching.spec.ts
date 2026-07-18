/* eslint-disable vue/one-component-per-file -- test file with stub components */
/**
 * TaskDetail KPI strip branching: snapshot vs CDC mode.
 * Validates that the KPI strip renders different tiles based on the aggregate's
 * current Run phase and that missing metric observations use sentinel states.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia, setActivePinia } from 'pinia';
import { createRouter, createMemoryHistory } from 'vue-router';
import { defineComponent, h, ref } from 'vue';
import { ElProgress } from 'element-plus';
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

/* ---------- Shared fixtures ---------- */
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
  metrics: {
    extractor_pushed_rps_avg: 100,
    sinker_rps_avg: 90,
    sinker_sinked_records: 3,
    extractor_plan_records: 8,
    progress: 87,
    finished_progress_count: 3,
    total_progress_count: 8,
    pipeline_queue_size: 1024,
  },
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
  metrics: {
    extractor_pushed_rps_avg: 50,
    sinker_rps_avg: 0,
    sinker_sinked_records: 24,
    lag: 3,
    pipeline_queue_size: 12,
  },
};

/* ---------- Stub components ---------- */
const Stub = defineComponent({ name: 'Placeholder', render: () => h('div') });

/* ---------- Common stubs for ElementPlus / child components ---------- */
const KpiCardStub = defineComponent({
  name: 'KpiCard',
  props: {
    label: { type: String, default: '' },
    value: { type: Number, default: 0 },
    unit: { type: String, default: '' },
    sentinelText: { type: String, default: undefined },
    badge: { type: String, default: '' },
    iconComp: { type: [Object, Function], default: null },
    tone: { type: String, default: 'default' },
    inverse: { type: Boolean, default: false },
    delta: { type: Number, default: undefined },
    spark: { type: Array, default: () => [] },
    accent: { type: Boolean, default: false },
    compareLabel: { type: String, default: '' },
  },
  template: `<div class="kpi-card-stub"><span class="kpi-label">{{ label }}</span><span class="kpi-value">{{ sentinelText ?? value }}</span><span class="kpi-unit" v-if="unit && !sentinelText">{{ unit }}</span><span class="kpi-badge" v-if="badge">{{ badge }}</span></div>`,
});

const GLOBAL_STUBS = {
  KpiCard: KpiCardStub,
  ChartCard: true as const,
  StatusBadge: true as const,
  LevelBadge: true as const,
  VChart: true as const,
  ElTable: true as const,
  ElTableColumn: true as const,
  ElTabPane: true as const,
  ElTabs: true as const,
  ElDrawer: true as const,
  ElDialog: true as const,
  ElPagination: true as const,
  ElSkeleton: true as const,
  ElAlert: true as const,
  ElEmpty: true as const,
  ElButtonGroup: true as const,
  ElSelect: true as const,
  ElOption: true as const,
  ElInput: true as const,
  ElInputNumber: true as const,
  ElSwitch: true as const,
};

/* ---------- Build harness ---------- */
async function buildHarness(taskFixture: ApiTask) {
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
    ? { sinker_rps_avg: 0, sinker_sinked_records: 24, lag: 3, pipeline_queue_size: 12, timestamp: 1_747_000_000 }
    : { extractor_rps_avg: 100, sinker_rps_avg: 90, sinker_sinked_records: 3, extractor_plan_records: 8, progress: 87, finished_progress_count: 3, total_progress_count: 8, pipeline_queue_size: 1024 };
  const selectedObjects = phase === 'snapshot'
    ? ['db.t1', 'db.t2', 'db.t3', 'db.t4', 'db.t5', 'db.t6', 'db.t7', 'db.t8']
    : [];
  const aggregate = {
    task: { ...taskFixture, configuredExtractType: phase, selectedObjects },
    currentRun: { id: mockRunId, status: 'running', currentPhase: phase, startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
    phases: {
      snapshot: { status: phase === 'snapshot' ? 'running' : 'skipped', startedAt: null, completedAt: null },
      transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
      cdc: { status: phase === 'cdc' ? 'running' : 'skipped', startedAt: null, completedAt: null },
    },
    metricsSnapshot: { runId: mockRunId, phase, sampledAt: '2026-05-07T06:01:00.000Z', values },
    progress: phase === 'snapshot'
      ? { runId: mockRunId, phase, kind: 'snapshot', percent: 87, copiedRecords: 3, estimatedTotalRecords: 8, totalIsEstimate: true }
      : { runId: mockRunId, phase, kind: 'cdc', percent: null, copiedRecords: null, estimatedTotalRecords: null, totalIsEstimate: false },
  };
  mockGet.mockImplementation((url: string) => {
    if (url.endsWith(`/tasks/${taskFixture.id}/detail`)) return Promise.resolve(aggregate);
    if (url.includes('/runs') && url.endsWith('/objects')) return Promise.resolve([
      { schema: 'db', table: 't1', state: 'completed' },
      { schema: 'db', table: 't2', state: 'completed' },
      { schema: 'db', table: 't3', state: 'completed' },
      { schema: 'db', table: 't4', state: 'loading' },
      { schema: 'db', table: 't5', state: 'pending' },
      { schema: 'db', table: 't6', state: 'pending' },
      { schema: 'db', table: 't7', state: 'pending' },
      { schema: 'db', table: 't8', state: 'pending' },
    ]);
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

describe('TaskDetail KPI branching', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockGet.mockReset();
  });

  it('snapshot task renders el-progress for progress bar', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const progress = wrapper.findComponent(ElProgress);
    expect(progress.exists()).toBe(true);
  });

  it('snapshot task shows 已完成/总表 indicator', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const text = wrapper.text();
    expect(text).toMatch(/3\s*\/\s*8/);
  });

  it('snapshot task labels the row denominator as estimated', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    expect(wrapper.text()).toMatch(/3\s*\/\s*estimated\s+8\s+records/i);
  });

  it('snapshot task shows copied records and Estimating total without a percentage', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/snap-1/detail')) return Promise.resolve({
        task: { ...SNAPSHOT_FIXTURE, configuredExtractType: 'snapshot', selectedObjects: [] },
        currentRun: { id: 'snap-1-run-1', status: 'running', currentPhase: 'snapshot', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
        phases: {
          snapshot: { status: 'running', startedAt: null, completedAt: null },
          transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
          cdc: { status: 'skipped', startedAt: null, completedAt: null },
        },
        metricsSnapshot: { runId: 'snap-1-run-1', phase: 'snapshot', sampledAt: '2026-05-07T06:01:00.000Z', values: { sinker_sinked_records: 3 } },
        progress: { runId: 'snap-1-run-1', phase: 'snapshot', kind: 'snapshot', percent: null, copiedRecords: 3, estimatedTotalRecords: null, totalIsEstimate: false },
      });
      if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
      if (url.includes('/alerts')) return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    expect(wrapper.findComponent(ElProgress).exists()).toBe(false);
    expect(wrapper.text()).toContain('3 records');
    expect(wrapper.text()).toContain('Estimating total');
  });

  it('CDC real zero throughput renders as idle rather than missing', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    expect(wrapper.text()).toMatch(/0\s*rows\/s/);
    expect(wrapper.text()).toContain('No new changes');
    expect(wrapper.text()).not.toContain('100%');
  });

  it('CDC missing throughput renders an em dash with a reason', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/cdc-1/detail')) return Promise.resolve({
        task: { ...CDC_FIXTURE, configuredExtractType: 'cdc', selectedObjects: [] },
        currentRun: { id: 'cdc-1-run-1', status: 'running', currentPhase: 'cdc', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
        phases: {
          snapshot: { status: 'skipped', startedAt: null, completedAt: null },
          transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
          cdc: { status: 'running', startedAt: null, completedAt: null },
        },
        metricsSnapshot: { runId: 'cdc-1-run-1', phase: 'cdc', sampledAt: '2026-05-07T06:01:00.000Z', values: { lag: 3, pipeline_queue_size: 0 } },
        progress: { runId: 'cdc-1-run-1', phase: 'cdc', kind: 'cdc', percent: null, copiedRecords: null, estimatedTotalRecords: null, totalIsEstimate: false },
      });
      if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
      if (url.includes('/alerts')) return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain('Apply throughput');
    expect(wrapper.text()).toContain('—');
    expect(wrapper.text()).toContain('No sample received');
  });

  it('renders Run-scoped metric query diagnostics with retry context', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/cdc-1/detail')) return Promise.resolve({
        task: { ...CDC_FIXTURE, configuredExtractType: 'cdc', selectedObjects: [] },
        currentRun: { id: 'cdc-1-run-1', status: 'running', currentPhase: 'cdc', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
        phases: {
          snapshot: { status: 'skipped', startedAt: null, completedAt: null },
          transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
          cdc: { status: 'running', startedAt: null, completedAt: null },
        },
        metricsSnapshot: { runId: 'cdc-1-run-1', phase: 'cdc', sampledAt: new Date().toISOString(), values: { sinker_rps_avg: 0, lag: 0, pipeline_queue_size: 0 } },
        progress: null,
      });
      if (url.includes('/runs/cdc-1-run-1/metrics?metric=sinker_rps_avg')) {
        return Promise.reject({ status: 400, code: 'UNKNOWN_METRIC', message: 'metric has no samples', requestId: 'req-metric-15' });
      }
      if (url.includes('/runs/cdc-1-run-1/metrics?metric=')) return Promise.resolve({ metric: 'other', data: [] });
      if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
      if (url.includes('/alerts')) return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const diagnostic = wrapper.get('[data-testid="metric-diagnostics"]');
    expect(diagnostic.text()).toContain('sinker_rps_avg');
    expect(diagnostic.text()).toContain('UNKNOWN_METRIC');
    expect(diagnostic.text()).toContain('metric has no samples');
    expect(diagnostic.text()).toContain('req-metric-15');
    expect(diagnostic.text()).toContain('Last refresh');
    expect(diagnostic.text()).toContain('Retry');
    expect(diagnostic.text()).toContain('Copy diagnostics');
    expect(mockGet.mock.calls.some(([url]) => typeof url === 'string' && url.includes('/runs/cdc-1-run-1/metrics?metric=sinker_rps_avg'))).toBe(true);
  });

  it('renders stale samples distinctly from missing samples', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    expect(wrapper.text()).toContain('Metrics stale');
    expect(wrapper.text()).toContain('Last sample');
  });

  it('snapshot task hides Lag tile', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const text = wrapper.text();
    expect(text).not.toContain('Lag');
  });

  it('CDC task hides progress bar', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const progress = wrapper.findComponent(ElProgress);
    expect(progress.exists()).toBe(false);
  });

  it('CDC task shows replication lag in seconds', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const text = wrapper.text();
    expect(text).toContain('Replication lag');
    expect(text).toMatch(/3\s*s/);
  });

  it('CDC task shows queue backlog value', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const text = wrapper.text();
    expect(text).toContain('Queue backlog');
    expect(text).toContain('12');
  });

  it('CDC Lag shows sentinel when the aggregate has no metrics snapshot', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/cdc-1/detail')) return Promise.resolve({
        task: { ...CDC_FIXTURE, configuredExtractType: 'cdc', selectedObjects: [] },
        currentRun: { id: 'cdc-1-run-1', status: 'running', currentPhase: 'cdc', startedAt: null, stoppedAt: null, exitCode: null, checkpoint: null },
        phases: {
          snapshot: { status: 'skipped', startedAt: null, completedAt: null },
          transitioning_to_cdc: { status: 'skipped', startedAt: null, completedAt: null },
          cdc: { status: 'running', startedAt: null, completedAt: null },
        },
        metricsSnapshot: null,
        progress: null,
      });
      if (url.includes('/runs')) return Promise.resolve({ items: [], total: 0 });
      if (url.includes('/alerts')) return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const text = wrapper.text();
    // Should NOT show "0 秒" — sentinel state is "—"
    expect(text).not.toContain('0 秒');
  });

  it('snapshot progress is unknown when the aggregate has no progress observation', async () => {
    const fixtureNoProgress: ApiTask = {
      ...SNAPSHOT_FIXTURE,
      metrics: { extractor_pushed_rps_avg: 100, pipeline_queue_size: 1024 },
    };
    const { TaskDetail, router, i18n } = await buildHarness(fixtureNoProgress);
    mockGet.mockImplementation((url: string) => {
      if (url.endsWith('/tasks/snap-1/detail')) return Promise.resolve({
        task: { ...fixtureNoProgress, configuredExtractType: 'snapshot', selectedObjects: [] },
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
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    const progress = wrapper.findComponent(ElProgress);
    expect(progress.exists()).toBe(false);
  });

  it('branching uses the aggregate current Run phase', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE);
    const wrapper = mount(TaskDetail, {
      global: {
        plugins: [router, i18n],
        components: { ElProgress },
        stubs: GLOBAL_STUBS,
      },
    });
    await flushPromises();

    // Snapshot fixture should have progress bar
    expect(wrapper.findComponent(ElProgress).exists()).toBe(true);
  });
});
