/**
 * Sidebar navigation — order preserved per reference screenshots:
 * 任务管理 → 告警管理 → License 管理 → 系统管理 → 运维管理 → 告警监控
 * (Dashboard sits at top as the new home.)
 */

export interface MenuItem {
  key: string;
  labelKey: string; // i18n key
  icon?: string;
  to?: string;
  children?: MenuItem[];
}

export const menu: MenuItem[] = [
  { key: 'dashboard', labelKey: 'nav.dashboard', icon: 'tabler:layout-dashboard', to: '/dashboard' },
  {
    key: 'tasks',
    labelKey: 'nav.tasks._label',
    icon: 'tabler:arrows-exchange',
    children: [
      { key: 'tasks.sync', labelKey: 'nav.tasks.sync', to: '/tasks/sync' },
      { key: 'tasks.check', labelKey: 'nav.tasks.check', to: '/tasks/check' },
      { key: 'tasks.struct', labelKey: 'nav.tasks.struct', to: '/tasks/struct' },
    ],
  },
  {
    key: 'alerts',
    labelKey: 'nav.alerts._label',
    icon: 'tabler:bell',
    children: [
      { key: 'alerts.current', labelKey: 'nav.alerts.current', to: '/alerts/current' },
      { key: 'alerts.history', labelKey: 'nav.alerts.history', to: '/alerts/history' },
    ],
  },
  { key: 'license', labelKey: 'nav.license', icon: 'tabler:license', to: '/license' },
  {
    key: 'system',
    labelKey: 'nav.system._label',
    icon: 'tabler:server-2',
    children: [
      { key: 'system.monitor', labelKey: 'nav.system.monitor', to: '/system/monitor' },
      { key: 'system.operateLog', labelKey: 'nav.system.operateLog', to: '/system/operate-log' },
    ],
  },
  {
    key: 'ops',
    labelKey: 'nav.ops._label',
    icon: 'tabler:tools',
    children: [
      { key: 'ops.management', labelKey: 'nav.ops.management', to: '/ops/management' },
      { key: 'ops.controlLog', labelKey: 'nav.ops.controlLog', to: '/ops/control-log' },
      { key: 'ops.globalParams', labelKey: 'nav.ops.globalParams', to: '/ops/global-params' },
    ],
  },
  {
    key: 'alertMonitor',
    labelKey: 'nav.alertMonitor._label',
    icon: 'tabler:activity-heartbeat',
    children: [
      { key: 'alertMonitor.metrics', labelKey: 'nav.alertMonitor.metrics', to: '/alert-monitor/metrics' },
      { key: 'alertMonitor.events', labelKey: 'nav.alertMonitor.events', to: '/alert-monitor/events' },
      { key: 'alertMonitor.monitorSetting', labelKey: 'nav.alertMonitor.monitorSetting', to: '/alert-monitor/monitor-setting' },
      { key: 'alertMonitor.alarmSetting', labelKey: 'nav.alertMonitor.alarmSetting', to: '/alert-monitor/alarm-setting' },
      { key: 'alertMonitor.alarmTemplate', labelKey: 'nav.alertMonitor.alarmTemplate', to: '/alert-monitor/alarm-template' },
    ],
  },
];
