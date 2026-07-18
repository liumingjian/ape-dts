import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { createI18n } from 'vue-i18n';
import { defineComponent, h, nextTick } from 'vue';
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

const documentVisibility = vi.hoisted(() => ({ isVisible: { value: true } }));

vi.mock('@/composables/useDocumentVisibility', () => ({
  useDocumentVisibility: () => documentVisibility,
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
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
    documentVisibility.isVisible.value = true;
    vi.useRealTimers();
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

  it('restores shareable list state and sends the exact canonical backend query', async () => {
    await mountTaskList('/tasks/migration?mode=cdc&status=running&engine=mysql&resource_group=rg-prod&q=orders&page=2&page_size=50&sort=updated_at&order=desc');
    const taskCall = vi.mocked(api.get).mock.calls.find(([url]) => String(url).startsWith('/tasks?'));
    expect(taskCall?.[0]).toBe('/tasks?category=migration&page=2&page_size=50&resource_group=rg-prod&engine=mysql&status=running&mode=cdc&q=orders&sort=updated_at&order=desc');
  });

  it('reloads the server list when browser navigation restores prior state', async () => {
    const { router } = await mountTaskList('/tasks/migration?status=running&page=2');
    vi.mocked(api.get).mockClear();

    await router.push('/tasks/migration?status=failed&page=3&page_size=20');
    await nextTick();

    const taskCall = vi.mocked(api.get).mock.calls.find(([url]) => String(url).startsWith('/tasks?'));
    expect(taskCall?.[0]).toBe('/tasks?category=migration&page=3&page_size=20&status=failed');
  });

  it('keeps the newest URL state when list requests resolve out of order', async () => {
    const first = deferred<{ items: ApiTask[]; total: number }>();
    const second = deferred<{ items: ApiTask[]; total: number }>();
    vi.mocked(api.get).mockImplementation(async (url: string) => {
      if (url === '/license') return { maxTasks: 0, currentTasks: 0 };
      if (url === '/resource_groups') return [];
      if (url.includes('page=2')) return first.promise;
      if (url.includes('page=3')) return second.promise;
      return { items: [], total: 0 };
    });

    const { router, wrapper } = await mountTaskList('/tasks/migration?page=2');
    await router.push('/tasks/migration?page=3');
    await nextTick();

    second.resolve({ items: [taskItem('cdc', 'cdc')], total: 1 });
    await Promise.resolve();
    await nextTick();
    first.resolve({ items: [taskItem('snapshot', 'snapshot')], total: 1 });
    await Promise.resolve();
    await nextTick();

    expect(wrapper.findAll('.row').map((row) => row.text())).toEqual(['cdc-cdc']);
  });

  it('pauses five-second polling while the document is hidden', async () => {
    vi.useFakeTimers();
    await mountTaskList('/tasks/migration?status=running&page=2');
    vi.mocked(api.get).mockClear();

    documentVisibility.isVisible.value = false;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(vi.mocked(api.get).mock.calls.some(([url]) => String(url).startsWith('/tasks?'))).toBe(false);

    documentVisibility.isVisible.value = true;
    await vi.advanceTimersByTimeAsync(5_000);
    const taskCall = vi.mocked(api.get).mock.calls.find(([url]) => String(url).startsWith('/tasks?'));
    expect(taskCall?.[0]).toBe('/tasks?category=migration&page=2&page_size=10&status=running');
  });

  it('renders production diagnostics and retries the exact failed request', async () => {
    const failedRequest = '/tasks?category=migration&page=2&page_size=10&status=running';
    vi.mocked(api.get).mockImplementation(async (url: string) => {
      if (url === '/license') return { maxTasks: 0, currentTasks: 0 };
      if (url === '/resource_groups') return [];
      throw { status: 503, code: 'TASK_LIST_UNAVAILABLE', message: 'database is locked', requestId: 'req-list-12' };
    });

    const { wrapper } = await mountTaskList('/tasks/migration?status=running&page=2');
    await nextTick();

    const diagnostics = wrapper.get('[data-testid="task-list-diagnostics"]');
    expect(diagnostics.text()).toContain('TASK_LIST_UNAVAILABLE');
    expect(diagnostics.text()).toContain('database is locked');
    expect(diagnostics.text()).toContain('503');
    expect(diagnostics.text()).toContain('req-list-12');
    expect(diagnostics.text()).toContain('taskList.diagnostics.lastRefresh');

    vi.mocked(api.get).mockResolvedValue({ items: [], total: 0 });
    await wrapper.get('[data-testid="task-list-retry"]').trigger('click');
    expect(vi.mocked(api.get).mock.calls.some(([url]) => url === failedRequest)).toBe(true);
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
