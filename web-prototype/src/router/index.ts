import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import type { Role } from '@/auth/permissions';
import { isMigrationMode, modeForLegacyTaskPath } from '@/utils/migrationMode';

declare module 'vue-router' {
  interface RouteMeta {
    title?: string;
    public?: boolean;
    hideInMenu?: boolean;
    module?: string;
    breadcrumb?: string[];
    roles?: Role[];
  }
}

const MainLayout = () => import('@/layouts/MainLayout.vue');
const BlankLayout = () => import('@/layouts/BlankLayout.vue');

const ALL_ROLES: Role[] = ['admin', 'operator', 'viewer'];

function modeForLegacyRoute(legacy: string, explicitMode: unknown) {
  if (isMigrationMode(explicitMode)) return explicitMode;
  if (legacy === 'cdc') return 'cdc';
  if (legacy === 'sync') return 'snapshot_cdc';
  return 'snapshot';
}

export const routes: RouteRecordRaw[] = [
  {
    path: '/login',
    component: BlankLayout,
    children: [
      {
        path: '',
        name: 'Login',
        component: () => import('@/views/auth/Login.vue'),
        meta: { title: '登录', public: true },
      },
    ],
  },
  {
    path: '/forbidden',
    name: 'Forbidden',
    component: () => import('@/views/misc/Forbidden.vue'),
    meta: { title: 'forbidden.title', public: true, hideInMenu: true },
  },
  {
    path: '/',
    component: MainLayout,
    redirect: '/dashboard',
    children: [
      {
        path: 'dashboard',
        name: 'Dashboard',
        component: () => import('@/views/dashboard/Dashboard.vue'),
        meta: { title: 'nav.dashboard', module: 'dashboard', breadcrumb: ['nav.dashboard'], roles: ALL_ROLES },
      },
      {
        path: 'tasks/migration',
        name: 'MigrationTasks',
        component: () => import('@/views/tasks/SyncTaskList.vue'),
        meta: { title: 'nav.tasks.migration', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.migration'], roles: ALL_ROLES },
      },
      {
        path: 'tasks/check',
        name: 'CheckTasks',
        component: () => import('@/views/tasks/CheckTaskList.vue'),
        meta: { title: 'nav.tasks.check', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.check'], roles: ALL_ROLES },
      },
      {
        path: 'tasks/struct',
        name: 'StructTasks',
        component: () => import('@/views/tasks/StructTaskList.vue'),
        meta: { title: 'nav.tasks.struct', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.struct'], roles: ALL_ROLES },
      },
      { path: 'tasks/:legacy(snapshot|cdc|sync)', redirect: (to) => {
        const existingMode = isMigrationMode(to.query.mode) ? to.query.mode : undefined;
        const mode = existingMode ?? modeForLegacyTaskPath(to.path);
        return { path: '/tasks/migration', query: { ...to.query, ...(mode ? { mode } : {}) }, hash: to.hash };
      }},
      { path: 'tasks/replay', redirect: (to) => ({ path: '/tasks/migration', query: to.query, hash: to.hash }) },
      { path: 'tasks/verify', redirect: { path: '/tasks/check' } },
      {
        path: 'tasks/create/:type(migration|check|struct)',
        name: 'CreateTask',
        component: () => import('@/views/tasks/CreateTaskWizard.vue'),
        meta: { title: 'task.action.create', module: 'tasks', hideInMenu: true, roles: ['admin', 'operator'] },
      },
      {
        path: 'tasks/create/:legacy(snapshot|cdc|sync|replay|verify)',
	        redirect: (to) => {
	          const legacy = String(to.params.legacy);
	          if (legacy === 'verify') return { path: '/tasks/create/check', query: to.query, hash: to.hash };
	          const mode = modeForLegacyRoute(legacy, to.query.mode);
	          return { path: '/tasks/create/migration', query: { ...to.query, mode }, hash: to.hash };
	        },
	      },
      {
        path: 'tasks/:category(migration|check|struct)/:id',
        name: 'TaskDetail',
        component: () => import('@/views/tasks/TaskDetail.vue'),
        meta: { title: 'task.action.view', module: 'tasks', hideInMenu: true, roles: ALL_ROLES },
      },
      {
        path: 'tasks/:legacy(snapshot|cdc|sync|replay|verify)/:id',
	        redirect: (to) => {
	          const legacy = String(to.params.legacy);
	          if (legacy === 'verify') return { path: `/tasks/check/${to.params.id}`, query: to.query, hash: to.hash };
	          const mode = modeForLegacyRoute(legacy, to.query.mode);
	          return { path: `/tasks/migration/${to.params.id}`, query: { ...to.query, mode }, hash: to.hash };
	        },
	      },
      // Alerts
      {
        path: 'alerts/current',
        name: 'CurrentAlerts',
        component: () => import('@/views/alerts/CurrentAlerts.vue'),
        meta: { title: 'nav.alerts.current', module: 'alerts', breadcrumb: ['nav.alerts._label', 'nav.alerts.current'], roles: ALL_ROLES },
      },
      {
        path: 'alerts/history',
        name: 'HistoryAlerts',
        component: () => import('@/views/alerts/HistoryAlerts.vue'),
        meta: { title: 'nav.alerts.history', module: 'alerts', breadcrumb: ['nav.alerts._label', 'nav.alerts.history'], roles: ALL_ROLES },
      },
      // License — readable by all roles; activate restricted to admin via component
      {
        path: 'license',
        name: 'License',
        component: () => import('@/views/license/License.vue'),
        meta: { title: 'nav.license', module: 'license', breadcrumb: ['nav.license'], roles: ALL_ROLES },
      },
      // System — admin only
      {
        path: 'system/users',
        name: 'UserManagement',
        component: () => import('@/views/system/UserManagement.vue'),
        meta: { title: 'nav.system.users', module: 'system', breadcrumb: ['nav.system._label', 'nav.system.users'], roles: ['admin'] },
      },
      {
        path: 'system/monitor',
        name: 'SystemMonitor',
        component: () => import('@/views/system/SystemMonitor.vue'),
        meta: { title: 'nav.system.monitor', module: 'system', breadcrumb: ['nav.system._label', 'nav.system.monitor'], roles: ['admin'] },
      },
      {
        path: 'system/operate-log',
        name: 'OperateLog',
        component: () => import('@/views/system/OperateLog.vue'),
        meta: { title: 'nav.system.operateLog', module: 'system', breadcrumb: ['nav.system._label', 'nav.system.operateLog'], roles: ['admin'] },
      },
      // Ops — admin + operator for management/control-log; admin-only for global-params
      {
        path: 'ops/management',
        name: 'OpsManagement',
        component: () => import('@/views/ops/OpsManagement.vue'),
        meta: { title: 'nav.ops.management', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.management'], roles: ['admin', 'operator'] },
      },
      {
        path: 'ops/control-log',
        name: 'ControlLog',
        component: () => import('@/views/ops/ControlLog.vue'),
        meta: { title: 'nav.ops.controlLog', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.controlLog'], roles: ['admin', 'operator'] },
      },
      {
        path: 'ops/global-params',
        name: 'GlobalParams',
        component: () => import('@/views/ops/GlobalParams.vue'),
        meta: { title: 'nav.ops.globalParams', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.globalParams'], roles: ['admin'] },
      },
      // Alert Monitor — admin only
      {
        path: 'alert-monitor/metrics',
        name: 'MetricRules',
        component: () => import('@/views/alertMonitor/MetricRules.vue'),
        meta: { title: 'nav.alertMonitor.metrics', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.metrics'], roles: ['admin'] },
      },
      {
        path: 'alert-monitor/events',
        name: 'EventRules',
        component: () => import('@/views/alertMonitor/EventRules.vue'),
        meta: { title: 'nav.alertMonitor.events', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.events'], roles: ['admin'] },
      },
      {
        path: 'alert-monitor/monitor-setting',
        name: 'MonitorSetting',
        component: () => import('@/views/alertMonitor/MonitorSetting.vue'),
        meta: { title: 'nav.alertMonitor.monitorSetting', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.monitorSetting'], roles: ['admin'] },
      },
      {
        path: 'alert-monitor/alarm-setting',
        name: 'AlarmSetting',
        component: () => import('@/views/alertMonitor/AlarmSetting.vue'),
        meta: { title: 'nav.alertMonitor.alarmSetting', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.alarmSetting'], roles: ['admin'] },
      },
      {
        path: 'alert-monitor/alarm-template',
        name: 'AlarmTemplate',
        component: () => import('@/views/alertMonitor/AlarmTemplate.vue'),
        meta: { title: 'nav.alertMonitor.alarmTemplate', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.alarmTemplate'], roles: ['admin'] },
      },
      // /users shorthand redirect → /system/users
      { path: 'users', redirect: { path: '/system/users' } },
      {
        path: 'profile',
        name: 'Profile',
        component: () => import('@/views/misc/Profile.vue'),
        meta: { title: 'topbar.profile', hideInMenu: true, roles: ALL_ROLES },
      },
    ],
  },
  {
    path: '/:pathMatch(.*)*',
    component: () => import('@/views/misc/NotFound.vue'),
    meta: { public: true, hideInMenu: true },
  },
];

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
  scrollBehavior: () => ({ top: 0 }),
});

router.beforeEach((to) => {
  const auth = useAuthStore();

  // Public routes bypass all checks
  if (to.meta?.public) {
    // Authenticated /login → dashboard (even though /login is public)
    if (to.path === '/login' && auth.isAuthenticated) {
      return { path: '/dashboard' };
    }
    return true;
  }

  // Unauthenticated → login with redirect (preserving full path + query)
  if (!auth.isAuthenticated) {
    const redirect = to.fullPath;
    return { path: '/login', query: redirect ? { redirect } : undefined };
  }

  // Role-based access control
  const required = to.meta?.roles;
  if (required?.length && auth.user?.role) {
    if (!required.includes(auth.user.role)) {
      return { path: '/forbidden', replace: true };
    }
  }

  return true;
});

export default router;
