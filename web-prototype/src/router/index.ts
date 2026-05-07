import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router';
import { useAuthStore } from '@/stores/auth';

const MainLayout = () => import('@/layouts/MainLayout.vue');
const BlankLayout = () => import('@/layouts/BlankLayout.vue');

const routes: RouteRecordRaw[] = [
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
    path: '/',
    component: MainLayout,
    redirect: '/dashboard',
    children: [
      {
        path: 'dashboard',
        name: 'Dashboard',
        component: () => import('@/views/dashboard/Dashboard.vue'),
        meta: { title: 'nav.dashboard', module: 'dashboard', breadcrumb: ['nav.dashboard'] },
      },
      // Task management — unified "同步任务" merges snapshot + cdc
      {
        path: 'tasks/sync',
        name: 'SyncTasks',
        component: () => import('@/views/tasks/SyncTaskList.vue'),
        meta: { title: 'nav.tasks.sync', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.sync'] },
      },
      {
        path: 'tasks/check',
        name: 'CheckTasks',
        component: () => import('@/views/tasks/CheckTaskList.vue'),
        meta: { title: 'nav.tasks.check', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.check'] },
      },
      {
        path: 'tasks/struct',
        name: 'StructTasks',
        component: () => import('@/views/tasks/StructTaskList.vue'),
        meta: { title: 'nav.tasks.struct', module: 'tasks', breadcrumb: ['nav.tasks._label', 'nav.tasks.struct'] },
      },
      // Legacy taxonomy redirects (ADR-0006). Preserve any query / hash payload.
      // snapshot/cdc are now sub-modes of /tasks/sync — preserve the mode filter via query.
      { path: 'tasks/snapshot', redirect: (to) => ({ path: '/tasks/sync', query: { ...to.query, mode: 'snapshot' } }) },
      { path: 'tasks/cdc', redirect: (to) => ({ path: '/tasks/sync', query: { ...to.query, mode: 'cdc' } }) },
      { path: 'tasks/replay', redirect: { path: '/tasks/sync' } },
      { path: 'tasks/verify', redirect: { path: '/tasks/check' } },
      {
        path: 'tasks/create/:type(snapshot|cdc|check|struct)',
        name: 'CreateTask',
        component: () => import('@/views/tasks/CreateTaskWizard.vue'),
        meta: { title: 'task.action.create', module: 'tasks', hideInMenu: true },
      },
      {
        path: 'tasks/create/:legacy(sync|replay|verify)',
        redirect: (to) => {
          const legacy = String(to.params.legacy);
          const next = legacy === 'verify' ? 'check' : 'snapshot';
          return { path: `/tasks/create/${next}`, query: to.query };
        },
      },
      {
        path: 'tasks/:category(snapshot|cdc|check|struct)/:id',
        name: 'TaskDetail',
        component: () => import('@/views/tasks/TaskDetail.vue'),
        meta: { title: 'task.action.view', module: 'tasks', hideInMenu: true },
      },
      {
        path: 'tasks/:legacy(sync|replay|verify)/:id',
        redirect: (to) => {
          const legacy = String(to.params.legacy);
          const next = legacy === 'verify' ? 'check' : 'snapshot';
          return { path: `/tasks/${next}/${to.params.id}`, query: to.query, hash: to.hash };
        },
      },
      // Alerts
      {
        path: 'alerts/current',
        name: 'CurrentAlerts',
        component: () => import('@/views/alerts/CurrentAlerts.vue'),
        meta: { title: 'nav.alerts.current', module: 'alerts', breadcrumb: ['nav.alerts._label', 'nav.alerts.current'] },
      },
      {
        path: 'alerts/history',
        name: 'HistoryAlerts',
        component: () => import('@/views/alerts/HistoryAlerts.vue'),
        meta: { title: 'nav.alerts.history', module: 'alerts', breadcrumb: ['nav.alerts._label', 'nav.alerts.history'] },
      },
      // License
      {
        path: 'license',
        name: 'License',
        component: () => import('@/views/license/License.vue'),
        meta: { title: 'nav.license', module: 'license', breadcrumb: ['nav.license'] },
      },
      // System
      {
        path: 'system/monitor',
        name: 'SystemMonitor',
        component: () => import('@/views/system/SystemMonitor.vue'),
        meta: { title: 'nav.system.monitor', module: 'system', breadcrumb: ['nav.system._label', 'nav.system.monitor'] },
      },
      {
        path: 'system/operate-log',
        name: 'OperateLog',
        component: () => import('@/views/system/OperateLog.vue'),
        meta: { title: 'nav.system.operateLog', module: 'system', breadcrumb: ['nav.system._label', 'nav.system.operateLog'] },
      },
      // Ops
      {
        path: 'ops/management',
        name: 'OpsManagement',
        component: () => import('@/views/ops/OpsManagement.vue'),
        meta: { title: 'nav.ops.management', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.management'] },
      },
      {
        path: 'ops/control-log',
        name: 'ControlLog',
        component: () => import('@/views/ops/ControlLog.vue'),
        meta: { title: 'nav.ops.controlLog', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.controlLog'] },
      },
      {
        path: 'ops/global-params',
        name: 'GlobalParams',
        component: () => import('@/views/ops/GlobalParams.vue'),
        meta: { title: 'nav.ops.globalParams', module: 'ops', breadcrumb: ['nav.ops._label', 'nav.ops.globalParams'] },
      },
      // Alert Monitor
      {
        path: 'alert-monitor/metrics',
        name: 'MetricRules',
        component: () => import('@/views/alertMonitor/MetricRules.vue'),
        meta: { title: 'nav.alertMonitor.metrics', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.metrics'] },
      },
      {
        path: 'alert-monitor/events',
        name: 'EventRules',
        component: () => import('@/views/alertMonitor/EventRules.vue'),
        meta: { title: 'nav.alertMonitor.events', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.events'] },
      },
      {
        path: 'alert-monitor/monitor-setting',
        name: 'MonitorSetting',
        component: () => import('@/views/alertMonitor/MonitorSetting.vue'),
        meta: { title: 'nav.alertMonitor.monitorSetting', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.monitorSetting'] },
      },
      {
        path: 'alert-monitor/alarm-setting',
        name: 'AlarmSetting',
        component: () => import('@/views/alertMonitor/AlarmSetting.vue'),
        meta: { title: 'nav.alertMonitor.alarmSetting', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.alarmSetting'] },
      },
      {
        path: 'alert-monitor/alarm-template',
        name: 'AlarmTemplate',
        component: () => import('@/views/alertMonitor/AlarmTemplate.vue'),
        meta: { title: 'nav.alertMonitor.alarmTemplate', module: 'alertMonitor', breadcrumb: ['nav.alertMonitor._label', 'nav.alertMonitor.alarmTemplate'] },
      },
      {
        path: 'profile',
        name: 'Profile',
        component: () => import('@/views/misc/Profile.vue'),
        meta: { title: 'topbar.profile', hideInMenu: true },
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
  if (!to.meta?.public && !auth.isAuthenticated) {
    return { path: '/login', query: { redirect: to.fullPath } };
  }
  if (to.path === '/login' && auth.isAuthenticated) {
    return { path: '/dashboard' };
  }
});

export default router;
