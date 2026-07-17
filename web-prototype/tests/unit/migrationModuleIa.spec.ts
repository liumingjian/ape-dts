import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { createI18n } from 'vue-i18n';
import { defineComponent, h } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMemoryHistory, createRouter, type RouteRecordRaw } from 'vue-router';
import TaskListView from '@/components/TaskListView.vue';
import { menu } from '@/config/menu';
import { routes } from '@/router/index';
import { api } from '@/api/client';
import type { ApiTask } from '@/types/domain';
import { mapApiTask } from '@/types/domain';
import { createPathForView, detailPathForView } from '@/utils/migrationMode';

vi.mock('@/api/client', () => ({
  api: {
    get: vi.fn().mockResolvedValue({ items: [], total: 0 }),
    post: vi.fn().mockResolvedValue({ id: 'created-task', category: 'migration' }),
    put: vi.fn().mockResolvedValue({}),
    del: vi.fn().mockResolvedValue({}),
  },
}));

vi.mock('@/composables/useRbac', () => ({
  useRbac: () => ({ can: () => true }),
}));

vi.mock('@/composables/useDocumentVisibility', () => ({
  useDocumentVisibility: () => ({ isVisible: { value: true } }),
}));

const Stub = defineComponent({ name: 'Stub', render: () => h('div') });

const i18n = createI18n({
  legacy: false,
  locale: 'en-US',
  fallbackLocale: 'en-US',
  messages: { 'en-US': {}, 'zh-CN': {} },
  missingWarn: false,
  fallbackWarn: false,
});

function getLeafRoutes(source: RouteRecordRaw[]): RouteRecordRaw[] {
  return source.flatMap((route) => {
    if (route.children?.length) return getLeafRoutes(route.children);
    return route.path ? [route] : [];
  });
}

function isRenderedLeaf(route: RouteRecordRaw): boolean {
  return !('redirect' in route) && Boolean(route.component);
}

function buildRouter(routeTable: RouteRecordRaw[] = routes) {
  return createRouter({
    history: createMemoryHistory('/'),
    routes: routeTable,
  });
}

function taskItem(kind: 'snapshot' | 'cdc', extractType: 'snapshot' | 'snapshot_and_cdc' | 'cdc'): ApiTask {
  return {
    id: `${kind}-${extractType}`,
    taskId: `${kind}-${extractType}`,
    name: `${kind}-${extractType}`,
    kind,
    dbTypeSource: 'mysql',
    dbTypeTarget: 'postgres',
    sourceEndpoint: { url: 'mysql://root:pw@127.0.0.1:3306/app' },
    targetEndpoint: { url: 'postgres://root:pw@127.0.0.2:5432/app' },
    extractor: { extract_type: extractType },
    sinker: null,
    filter: null,
    router: null,
    parallelizer: { parallel_type: 'snapshot', parallel_size: 1 },
    pipeline: { buffer_size: 16000, checkpoint_interval_secs: 10 },
    resumer: { resume_type: 'from_log' },
    processor: null,
    runtime: null,
    metrics: null,
    resourceGroupId: 'default',
    ownerUserId: 'admin',
    status: 'running',
    createdAt: '2026-01-01T00:00:00.000Z',
    updatedAt: '2026-01-01T00:00:00.000Z',
  };
}

async function mountTaskList(path: string) {
  setActivePinia(createPinia());
  const router = buildRouter([
    {
      path: '/',
      component: Stub,
      children: [
        { path: 'tasks/migration', component: TaskListView, props: { viewKind: 'migration' } },
        { path: 'tasks/migration/:id', component: Stub },
        { path: 'tasks/create/migration', component: Stub },
      ],
    },
  ]);
  await router.push(path);
  await router.isReady();

  const wrapper = mount(TaskListView, {
    props: { viewKind: 'migration' },
    global: {
      plugins: [router, i18n],
      stubs: {
        PageHeader: { template: '<section><slot name="actions" /></section>' },
        EngineTag: Stub,
        StatusBadge: Stub,
        ElTable: { props: ['data'], template: '<div><slot /><button v-for="row in data" :key="row.id" class="row" @click="$emit(\'row-click\', row)">{{ row.id }}</button></div>' },
        ElTableColumn: { template: '<div><slot :row="{ id: \'snapshot-snapshot\', category: \'snapshot\', name: \'row\', source: { engine: \'mysql\' }, target: { engine: \'postgres\' }, status: \'running\', metrics: {}, progressPercent: 0, resourceGroup: \'default\', instanceIp: \'127.0.0.1\', syncMode: \'snapshot\' }" /></div>' },
        ElButton: { template: '<button @click="$emit(\'click\')"><slot /></button>' },
        ElTooltip: { template: '<span><slot /></span>' },
        ElSelect: { template: '<select><slot /></select>' },
        ElOption: Stub,
        ElInput: Stub,
        ElTag: Stub,
        ElPopover: { template: '<div><slot name="reference" /><slot /></div>' },
        ElCheckbox: Stub,
        ElLink: { template: '<button @click="$emit(\'click\')"><slot /></button>' },
        ElProgress: Stub,
        ElDropdown: { template: '<div><slot /><slot name="dropdown" /></div>' },
        ElDropdownMenu: { template: '<div><slot /></div>' },
        ElDropdownItem: { template: '<button><slot /></button>' },
        ElButtonGroup: { template: '<div><slot /></div>' },
        IconRefresh: Stub,
        IconPlus: Stub,
        IconChevronDown: Stub,
        IconPlayerPlay: Stub,
        IconPlayerPause: Stub,
        IconPlayerStop: Stub,
        IconTrash: Stub,
        IconFileExport: Stub,
        IconFileImport: Stub,
        IconTemplate: Stub,
        IconDownload: Stub,
        IconLayoutRows: Stub,
        IconLayoutList: Stub,
        IconColumns: Stub,
        IconSearch: Stub,
      },
    },
  });
  await Promise.resolve();
  return { router, wrapper };
}

async function mountWizardAt(path: string) {
  setActivePinia(createPinia());
  const Wizard = (await import('@/views/tasks/CreateTaskWizard.vue')).default;
  const router = buildRouter([
    {
      path: '/',
      component: Stub,
      children: [
        { path: 'tasks/create/:type', component: Wizard },
        { path: 'tasks/migration/:id', component: Stub },
      ],
    },
  ]);
  await router.push(path);
  await router.isReady();
  const wrapper = mount(Wizard, {
    global: {
      plugins: [router, i18n],
      stubs: {
        ConnectionTestCard: Stub,
        EngineTag: Stub,
      },
    },
  });
  await Promise.resolve();
  return { wrapper };
}

describe('Migration module routes and menu', () => {
  it('exposes Migration, Check and Struct only in the task sidebar', () => {
    const tasks = menu.find((item) => item.key === 'tasks');
    expect(tasks?.children?.map((item) => item.key)).toEqual([
      'tasks.migration',
      'tasks.check',
      'tasks.struct',
    ]);
    expect(tasks?.children?.[0]?.to).toBe('/tasks/migration');
  });

  it('defines canonical migration list/create/detail leaves', () => {
    const leaves = getLeafRoutes(routes);
    const list = leaves.find((route) => route.path === 'tasks/migration');
    const create = leaves.find((route) => route.path === 'tasks/create/:type(migration|check|struct)');
    const detail = leaves.find((route) => route.path === 'tasks/:category(migration|check|struct)/:id');

    expect(list?.name).toBe('MigrationTasks');
    expect(create?.name).toBe('CreateTask');
    expect(detail?.name).toBe('TaskDetail');
    expect(list && isRenderedLeaf(list)).toBe(true);
    expect(create && isRenderedLeaf(create)).toBe(true);
    expect(detail && isRenderedLeaf(detail)).toBe(true);
  });

  it('redirects legacy task paths to migration while preserving query and hash', async () => {
    const router = buildRouter();
    await router.push('/tasks/cdc/abc-123?tab=logs#tail');
    expect(router.currentRoute.value.path).toBe('/tasks/migration/abc-123');
    expect(router.currentRoute.value.query.mode).toBe('cdc');
    expect(router.currentRoute.value.query.tab).toBe('logs');
    expect(router.currentRoute.value.hash).toBe('#tail');

    await router.push('/tasks/snapshot?status=running#top');
    expect(router.currentRoute.value.path).toBe('/tasks/migration');
    expect(router.currentRoute.value.query.mode).toBe('snapshot');
    expect(router.currentRoute.value.query.status).toBe('running');
    expect(router.currentRoute.value.hash).toBe('#top');
  });
});

describe('Migration task list behavior', () => {
  beforeEach(() => {
    vi.mocked(api.get).mockReset();
    vi.mocked(api.get).mockImplementation(async (url: string) => {
      if (url === '/license') return { maxTasks: 0, currentTasks: 0 };
      return {
        items: [
          taskItem('snapshot', 'snapshot'),
          taskItem('snapshot', 'snapshot_and_cdc'),
          taskItem('cdc', 'cdc'),
        ],
        total: 3,
      };
    });
  });

  it('loads migration from the server category and syncs mode from the URL', async () => {
    await mountTaskList('/tasks/migration?mode=cdc&status=running');
    const taskCall = vi.mocked(api.get).mock.calls.find(([url]) => String(url).startsWith('/tasks?'));
    expect(taskCall?.[0]).toContain('category=migration');
    expect(taskCall?.[0]).toContain('mode=cdc');
    expect(taskCall?.[0]).toContain('status=running');
  });

  it('maps persisted snapshot_and_cdc tasks to the snapshot_cdc display mode', () => {
    const task = mapApiTask(taskItem('snapshot', 'snapshot_and_cdc'));
    expect(task.syncMode).toBe('snapshot_cdc');
    expect(task.extractType).toBe('snapshot_and_cdc');
  });

  it('uses migration create and detail paths', async () => {
    await mountTaskList('/tasks/migration');
    expect(detailPathForView('migration', 'snapshot-snapshot')).toBe('/tasks/migration/snapshot-snapshot');
    expect(createPathForView('migration')).toBe('/tasks/create/migration');
  });
});

describe('Migration wizard payload semantics', () => {
  beforeEach(() => {
    vi.mocked(api.get).mockReset();
    vi.mocked(api.post).mockReset();
    vi.mocked(api.get).mockResolvedValue([]);
    vi.mocked(api.post).mockResolvedValue({ id: 'created-task', category: 'migration' });
  });

  it.each([
    ['/tasks/create/migration?mode=snapshot', 'snapshot', 'snapshot'],
    ['/tasks/create/migration?mode=snapshot_cdc', 'snapshot', 'snapshot_and_cdc'],
    ['/tasks/create/migration?mode=cdc', 'cdc', 'cdc'],
    ['/tasks/create/cdc', 'cdc', 'cdc'],
  ])('posts %s as kind=%s extract_type=%s', async (path, expectedKind, expectedExtractType) => {
    const { wrapper } = await mountWizardAt(path);
    await (wrapper.vm as InstanceType<typeof import('@/views/tasks/CreateTaskWizard.vue')['default']>).onSubmit();
    const taskPost = vi.mocked(api.post).mock.calls.find(([url]) => url === '/tasks');
    expect(taskPost?.[1]).toMatchObject({
      kind: expectedKind,
      extractor: { extract_type: expectedExtractType },
    });
  });
});
