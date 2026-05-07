export type Role = 'admin' | 'operator' | 'viewer';

export type Action =
  | 'task.create'
  | 'task.delete'
  | 'task.start'
  | 'task.stop'
  | 'task.pause'
  | 'task.resume'
  | 'task.read'
  | 'user.manage'
  | 'license.activate'
  | 'license.read'
  | 'alert.clear'
  | 'alert.read'
  | 'alert.rule.manage'
  | 'alarm.channel.manage'
  | 'alarm.template.manage'
  | 'monitor.setting.manage'
  | 'global.param.manage'
  | 'system.monitor.read'
  | 'operate.log.read'
  | 'control.log.read';

const MATRIX: Record<Role, ReadonlySet<Action>> = {
  admin: new Set<Action>([
    'task.create',
    'task.delete',
    'task.start',
    'task.stop',
    'task.pause',
    'task.resume',
    'task.read',
    'user.manage',
    'license.activate',
    'license.read',
    'alert.clear',
    'alert.read',
    'alert.rule.manage',
    'alarm.channel.manage',
    'alarm.template.manage',
    'monitor.setting.manage',
    'global.param.manage',
    'system.monitor.read',
    'operate.log.read',
    'control.log.read',
  ]),
  operator: new Set<Action>([
    'task.create',
    'task.start',
    'task.stop',
    'task.pause',
    'task.resume',
    'task.read',
    'alert.clear',
    'alert.read',
    'control.log.read',
  ]),
  viewer: new Set<Action>(['task.read', 'alert.read']),
};

export function canPerform(role: Role | null | undefined, action: Action): boolean {
  if (!role) return false;
  return MATRIX[role]?.has(action) ?? false;
}

export type NavModule =
  | 'dashboard'
  | 'tasks'
  | 'alerts'
  | 'alertMonitor'
  | 'system'
  | 'license'
  | 'ops';

export function visibleNavItems(role: Role | null | undefined): NavModule[] {
  if (!role) return [];
  const base: NavModule[] = ['dashboard', 'tasks', 'alerts'];
  if (role === 'admin') return [...base, 'alertMonitor', 'system', 'license', 'ops'];
  if (role === 'operator') return [...base, 'ops'];
  return base;
}
