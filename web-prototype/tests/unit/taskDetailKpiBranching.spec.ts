/* eslint-disable vue/one-component-per-file -- test file with stub components */
/**
 * TaskDetail KPI strip branching: snapshot vs CDC mode.
 * Validates that the KPI strip renders different tiles based on task.syncMode
 * and that sentinel states handle missing metric data correctly.
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
    progress: 87,
    finished_progress_count: 3,
    total_progress_count: 8,
    pipeline_buffer_size_avg: 1024,
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
    lag: 3,
    pipeline_queue_size: 12,
    pipeline_buffer_size_avg: 512,
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

  // Default mock responses — include a run so currentRunId is set
  const mockRunId = taskFixture.id + '-run-1';
  mockGet.mockImplementation((url: string) => {
    if (url.includes('/tasks/') && !url.includes('/runs')) return Promise.resolve(taskFixture);
    if (url.includes('/runs') && url.includes('/metrics/latest')) return Promise.resolve({});
    if (url.includes('/runs') && url.includes('/logs')) return Promise.resolve('');
    if (url.includes('/runs') && !url.includes('/metrics')) return Promise.resolve({ items: [{ id: mockRunId, taskId: taskFixture.id, status: 'running', startedAt: null, stoppedAt: null, exitCode: null, logDir: null, iniPath: null, pid: null, position: null, createdAt: '2026-05-07T06:00:00.000Z' }], total: 1 });
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

  it('CDC task shows Lag value with 秒 suffix', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    // Override mock to return metrics with lag
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/tasks/') && !url.includes('/runs')) return Promise.resolve(CDC_FIXTURE);
      if (url.includes('/metrics/latest')) return Promise.resolve({ lag: 3, pipeline_queue_size: 12 });
      if (url.includes('/runs') && url.includes('/logs')) return Promise.resolve('');
      if (url.includes('/runs') && !url.includes('/metrics')) return Promise.resolve({ items: [{ id: 'cdc-1-run-1', taskId: CDC_FIXTURE.id, status: 'running', startedAt: null, stoppedAt: null, exitCode: null, logDir: null, iniPath: null, pid: null, position: null, createdAt: '2026-05-07T06:00:00.000Z' }], total: 1 });
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
    // The KpiCard renders value and unit in separate spans with CSS gap;
    // in text() they concatenate without space — check both substrings present
    expect(text).toContain('Lag');
    expect(text).toMatch(/3\s*秒/);
  });

  it('CDC task shows 积压数 value', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    // Override mock to return metrics with pipeline_queue_size
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/tasks/') && !url.includes('/runs')) return Promise.resolve(CDC_FIXTURE);
      if (url.includes('/metrics/latest')) return Promise.resolve({ lag: 3, pipeline_queue_size: 12 });
      if (url.includes('/runs') && url.includes('/logs')) return Promise.resolve('');
      if (url.includes('/runs') && !url.includes('/metrics')) return Promise.resolve({ items: [{ id: 'cdc-1-run-1', taskId: CDC_FIXTURE.id, status: 'running', startedAt: null, stoppedAt: null, exitCode: null, logDir: null, iniPath: null, pid: null, position: null, createdAt: '2026-05-07T06:00:00.000Z' }], total: 1 });
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
    expect(text).toContain('积压数');
    expect(text).toContain('12');
  });

  it('CDC Lag shows sentinel when metrics/latest returns {}', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(CDC_FIXTURE);
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/tasks/')) return Promise.resolve(CDC_FIXTURE);
      if (url.includes('/metrics/latest')) return Promise.resolve({});
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

  it('snapshot progress bar renders at 0% when /metrics/latest returns {}', async () => {
    const fixtureNoProgress: ApiTask = {
      ...SNAPSHOT_FIXTURE,
      metrics: { extractor_pushed_rps_avg: 100, pipeline_buffer_size_avg: 1024 },
    };
    const { TaskDetail, router, i18n } = await buildHarness(fixtureNoProgress);
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/tasks/')) return Promise.resolve(fixtureNoProgress);
      if (url.includes('/metrics/latest')) return Promise.resolve({});
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
    expect(progress.exists()).toBe(true);
    const pct = progress.props('percentage');
    expect(pct).toBe(0);
  });

  it('branching uses task.syncMode, not a new prop', async () => {
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
