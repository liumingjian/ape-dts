import { describe, expect, it } from 'vitest';
import type { RouteRecordRaw } from 'vue-router';
import type { Role } from '@/auth/permissions';
import { menu } from '@/config/menu';
import { routes } from '@/router/index';

interface InventoryEntry {
  path: string;
  name?: string;
  component: string;
  public: boolean;
  roles: Role[];
  module?: string;
  hidden: boolean;
}

const ALL_ROLES: Role[] = ['admin', 'operator', 'viewer'];
const INVENTORY: InventoryEntry[] = [
  { path: '/login', name: 'Login', component: 'Login', public: true, roles: [], hidden: true },
  { path: '/forbidden', name: 'Forbidden', component: 'Forbidden', public: true, roles: [], hidden: true },
  { path: '/:pathMatch(.*)*', component: 'NotFound', public: true, roles: [], hidden: true },
  { path: '/profile', name: 'Profile', component: 'Profile', public: false, roles: ALL_ROLES, hidden: true },
  { path: '/dashboard', name: 'Dashboard', component: 'Dashboard', public: false, roles: ALL_ROLES, module: 'dashboard', hidden: false },
  { path: '/tasks/migration', name: 'MigrationTasks', component: 'SyncTaskList', public: false, roles: ALL_ROLES, module: 'tasks', hidden: false },
  { path: '/tasks/check', name: 'CheckTasks', component: 'CheckTaskList', public: false, roles: ALL_ROLES, module: 'tasks', hidden: false },
  { path: '/tasks/struct', name: 'StructTasks', component: 'StructTaskList', public: false, roles: ALL_ROLES, module: 'tasks', hidden: false },
  { path: '/tasks/create/:type(migration|check|struct)', name: 'CreateTask', component: 'CreateTaskWizard', public: false, roles: ['admin', 'operator'], module: 'tasks', hidden: true },
  { path: '/tasks/:category(migration|check|struct)/:id', name: 'TaskDetail', component: 'TaskDetail', public: false, roles: ALL_ROLES, module: 'tasks', hidden: true },
  { path: '/alerts/current', name: 'CurrentAlerts', component: 'CurrentAlerts', public: false, roles: ALL_ROLES, module: 'alerts', hidden: false },
  { path: '/alerts/history', name: 'HistoryAlerts', component: 'HistoryAlerts', public: false, roles: ALL_ROLES, module: 'alerts', hidden: false },
  { path: '/ops/management', name: 'OpsManagement', component: 'OpsManagement', public: false, roles: ['admin', 'operator'], module: 'ops', hidden: false },
  { path: '/ops/control-log', name: 'ControlLog', component: 'ControlLog', public: false, roles: ['admin', 'operator'], module: 'ops', hidden: false },
  { path: '/ops/global-params', name: 'GlobalParams', component: 'GlobalParams', public: false, roles: ['admin'], module: 'ops', hidden: false },
  { path: '/license', name: 'License', component: 'License', public: false, roles: ALL_ROLES, module: 'license', hidden: false },
  { path: '/system/users', name: 'UserManagement', component: 'UserManagement', public: false, roles: ['admin'], module: 'system', hidden: false },
  { path: '/system/monitor', name: 'SystemMonitor', component: 'SystemMonitor', public: false, roles: ['admin'], module: 'system', hidden: false },
  { path: '/system/operate-log', name: 'OperateLog', component: 'OperateLog', public: false, roles: ['admin'], module: 'system', hidden: false },
  { path: '/alert-monitor/metrics', name: 'MetricRules', component: 'MetricRules', public: false, roles: ['admin'], module: 'alertMonitor', hidden: false },
  { path: '/alert-monitor/events', name: 'EventRules', component: 'EventRules', public: false, roles: ['admin'], module: 'alertMonitor', hidden: false },
  { path: '/alert-monitor/monitor-setting', name: 'MonitorSetting', component: 'MonitorSetting', public: false, roles: ['admin'], module: 'alertMonitor', hidden: false },
  { path: '/alert-monitor/alarm-setting', name: 'AlarmSetting', component: 'AlarmSetting', public: false, roles: ['admin'], module: 'alertMonitor', hidden: false },
  { path: '/alert-monitor/alarm-template', name: 'AlarmTemplate', component: 'AlarmTemplate', public: false, roles: ['admin'], module: 'alertMonitor', hidden: false },
];

const COMPATIBILITY_PATHS = [
  '/tasks/:legacy(snapshot|cdc|sync)',
  '/tasks/replay',
  '/tasks/verify',
  '/tasks/create/:legacy(snapshot|cdc|sync|replay|verify)',
  '/tasks/:legacy(snapshot|cdc|sync|replay|verify)/:id',
];

function canonicalPath(parentPath: string, childPath: string) {
  if (!childPath) return parentPath;
  if (childPath.startsWith('/')) return childPath;
  if (parentPath === '/') return `/${childPath}`;
  return `${parentPath.replace(/\/$/, '')}/${childPath}`;
}

function leafRoutes(tree: RouteRecordRaw[], parentPath = ''): Array<{ path: string; route: RouteRecordRaw }> {
  return tree.flatMap((route) => {
    const path = canonicalPath(parentPath, route.path);
    if (route.children?.length) return leafRoutes(route.children, path);
    return [{ path, route }];
  });
}

function menuPaths() {
  return menu.flatMap((item) => item.children?.map((child) => child.to) ?? [item.to]).filter(Boolean);
}

async function componentName(route: RouteRecordRaw) {
  const loader = route.component as (() => Promise<{ default: { __name?: string; name?: string } }>) | undefined;
  const module = await loader?.();
  return module?.default.__name ?? module?.default.name;
}

describe('canonical Console page inventory', () => {
  const leaves = leafRoutes(routes);
  const rendered = leaves.filter(({ route }) => route.component && !route.redirect);
  const pathsInMenu = menuPaths();

  it.each(INVENTORY)('$path declares its component, auth, roles, and menu status', async (expected) => {
    const actual = rendered.find((entry) => entry.path === expected.path);
    expect(actual, `${expected.path} should be a rendered canonical route`).toBeDefined();
    expect(actual?.route.name).toBe(expected.name);
    expect(await componentName(actual!.route)).toBe(expected.component);
    expect(Boolean(actual?.route.meta?.public)).toBe(expected.public);
    expect(actual?.route.meta?.roles ?? []).toEqual(expected.roles);
    expect(actual?.route.meta?.module).toBe(expected.module);
    expect(Boolean(actual?.route.meta?.hideInMenu)).toBe(expected.hidden);
    expect(pathsInMenu.includes(expected.path)).toBe(!expected.hidden);
  });

  it('has no rendered canonical page outside the executable inventory', () => {
    expect(rendered.map((entry) => entry.path).sort()).toEqual(INVENTORY.map((entry) => entry.path).sort());
  });

  it('keeps every compatibility route redirect-only', () => {
    for (const path of COMPATIBILITY_PATHS) {
      const actual = leaves.find((entry) => entry.path === path);
      expect(actual, `${path} should exist`).toBeDefined();
      expect(actual?.route.redirect, `${path} should redirect`).toBeDefined();
      expect(actual?.route.component, `${path} must not render`).toBeUndefined();
    }
  });
});
