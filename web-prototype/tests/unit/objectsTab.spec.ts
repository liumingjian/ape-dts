/* eslint-disable vue/one-component-per-file -- test file with stub components */
/**
 * Objects tab real data: validates that the Objects tab fetches from
 * GET /runs/:id/objects, populates the objects ref with TableLoadState rows,
 * and uses stateTagType to map state → el-tag type.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { createPinia, setActivePinia } from 'pinia';
import { createRouter, createMemoryHistory } from 'vue-router';
import { defineComponent, h, ref } from 'vue';
import type { ApiTask, TableLoadState } from '@/types/domain';

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

const MOCK_OBJECTS: TableLoadState[] = [
  { schema: 'public', table: 'orders', state: 'loading' },
  { schema: 'public', table: 'users', state: 'completed' },
];

/* ---------- Stub components ---------- */
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
  ChartCard: true as const,
  StatusBadge: true as const,
  LevelBadge: true as const,
  VChart: true as const,
  ElProgress: true as const,
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
  ElTag: true as const,
};

/* ---------- Harness ---------- */
async function buildHarness(taskFixture: ApiTask = SNAPSHOT_FIXTURE, objectsResponse: TableLoadState[] | null = MOCK_OBJECTS) {
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
          objects: { col: { name: '名称', type: '类型', rows: '行数', status: '状态', schema: 'Schema', table: 'Table', state: 'State' } },
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
  mockGet.mockImplementation((url: string) => {
    if (url.includes('/tasks/') && !url.includes('/runs')) return Promise.resolve(taskFixture);
    if (url.includes('/metrics/latest')) return Promise.resolve({});
    if (url.includes('/runs') && url.includes('/objects')) {
      return Promise.resolve(objectsResponse ?? []);
    }
    if (url.includes('/runs') && url.includes('/logs')) return Promise.resolve('');
    if (url.includes('/runs') && !url.includes('/metrics') && !url.includes('/objects')) {
      return Promise.resolve({
        items: [{
          id: mockRunId, taskId: taskFixture.id, status: 'running',
          startedAt: null, stoppedAt: null, exitCode: null,
          logDir: null, iniPath: null, pid: null, position: null,
          createdAt: '2026-05-07T06:00:00.000Z',
        }],
        total: 1,
      });
    }
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
function countObjectsCalls(): number {
  return mockGet.mock.calls.filter(
    (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).includes('/objects'),
  ).length;
}

function objectsCallUrl(): string | undefined {
  const call = mockGet.mock.calls.find(
    (call: unknown[]) => typeof call[0] === 'string' && (call[0] as string).includes('/objects'),
  );
  return call ? (call[0] as string) : undefined;
}

describe('Objects tab real data', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockGet.mockReset();
  });

  it('switching to Objects tab triggers GET /runs/:id/objects', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();
    mockGet.mockClear();

    // Simulate tab change to 'objects'
    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();

    expect(countObjectsCalls()).toBeGreaterThanOrEqual(1);
  });

  it('Objects fetch URL contains /runs/:id/objects', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();
    mockGet.mockClear();

    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();

    const url = objectsCallUrl();
    expect(url).toContain('/runs/');
    expect(url).toContain('/objects');
  });

  it('objects ref is populated from API response', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Trigger objects load
    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();

    // Verify the objects ref contains the mock data
    const objects: TableLoadState[] = (wrapper.vm as any).objects;
    expect(objects).toHaveLength(2);
    expect(objects[0].schema).toBe('public');
    expect(objects[0].table).toBe('orders');
    expect(objects[0].state).toBe('loading');
    expect(objects[1].table).toBe('users');
    expect(objects[1].state).toBe('completed');
  });

  it('objects ref is empty when API returns []', async () => {
    const { TaskDetail, router, i18n } = await buildHarness(SNAPSHOT_FIXTURE, []);
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();

    const objects: TableLoadState[] = (wrapper.vm as any).objects;
    expect(objects).toHaveLength(0);
  });

  it('stateTagType returns correct el-tag type for each state', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    const fn = (wrapper.vm as any).stateTagType as (s: TableLoadState['state']) => string;
    expect(fn('pending')).toBe('info');
    expect(fn('loading')).toBe('warning');
    expect(fn('completed')).toBe('success');
  });

  it('objects load on mount when initial tab is objects', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    // Override route to start on objects tab
    await router.push({ path: `/tasks/snapshot/${SNAPSHOT_FIXTURE.id}`, query: { tab: 'objects' } });
    await router.isReady();

    mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // Should have called the objects endpoint during mount
    expect(countObjectsCalls()).toBeGreaterThanOrEqual(1);
  });

  it('objects are fetched again when re-visiting the objects tab', async () => {
    const { TaskDetail, router, i18n } = await buildHarness();
    const wrapper = mount(TaskDetail, {
      global: { plugins: [router, i18n], stubs: GLOBAL_STUBS },
    });
    await flushPromises();

    // First visit
    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();
    const firstCount = countObjectsCalls();

    // Navigate away
    (wrapper.vm as any).activeTab = 'config';
    await flushPromises();

    // Revisit
    (wrapper.vm as any).activeTab = 'objects';
    await flushPromises();
    const secondCount = countObjectsCalls();

    expect(secondCount).toBeGreaterThan(firstCount);
  });
});
