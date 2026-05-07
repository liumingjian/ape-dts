export type Role = 'admin' | 'operator' | 'viewer';

export type Action =
  | 'task.create'
  | 'task.delete'
  | 'task.start'
  | 'task.stop'
  | 'task.read'
  | 'user.manage'
  | 'license.activate'
  | 'alert.clear';

const MATRIX: Record<Role, ReadonlySet<Action>> = {
  admin: new Set<Action>([
    'task.create',
    'task.delete',
    'task.start',
    'task.stop',
    'task.read',
    'user.manage',
    'license.activate',
    'alert.clear',
  ]),
  operator: new Set<Action>([
    'task.create',
    'task.start',
    'task.stop',
    'task.read',
    'alert.clear',
  ]),
  viewer: new Set<Action>(['task.read']),
};

export function canPerform(role: Role | null | undefined, action: Action): boolean {
  if (!role) return false;
  return MATRIX[role]?.has(action) ?? false;
}

export type NavModule =
  | 'dashboard'
  | 'tasks'
  | 'alerts'
  | 'alert-monitor'
  | 'system'
  | 'license'
  | 'ops';

export function visibleNavItems(role: Role | null | undefined): NavModule[] {
  if (!role) return [];
  const base: NavModule[] = ['dashboard', 'tasks', 'alerts'];
  if (role === 'admin') return [...base, 'alert-monitor', 'system', 'license', 'ops'];
  if (role === 'operator') return [...base, 'ops'];
  return base;
}
